//! Record-level parsing of the `Contents`, Quill and Escher streams.
//!
//! Every read is bounds-checked and returns `None` past the end of the data.
//! Counts and offsets taken from the file are clamped to the size of the stream
//! they point into before anything is allocated, and loops are capped, so a
//! hostile file costs time proportional to its own size.
//!
//! Layout (the format has no public specification; the field positions were
//! learned from the corpus and from reading libmspub and Apache POI HPBF as
//! format references, see ADR 0017):
//!
//! * `Contents` begins `E8 AC <version> 00`. Version `0x2C` is Publisher 2002
//!   and later: a trailer holds a directory of typed, id-tagged blocks that
//!   point at page, shape and document chunks. Version `0x22` is Publisher 98
//!   and 2000: a flat table of chunk offsets with a type marker at the start of
//!   each chunk. A file of that version with no `Quill` storage is Publisher 98
//!   proper, which keeps its text inside `Contents`.
//! * `Quill/QuillSub/CONTENTS` holds the stories for 2000 and later: UTF-16
//!   text plus character and paragraph property runs in 512-byte blocks.
//! * `Escher/EscherStm` (2002 and later) holds shape geometry as Office Art
//!   records with a few Publisher-specific length conventions.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::IoError;

use super::report::ImportReport;

/// Most blocks read from one container, and most records from one stream.
const MAX_ITEMS: usize = 200_000;
/// Deepest nesting followed (groups, Escher containers).
const MAX_DEPTH: usize = 64;
/// Largest file-supplied offset or length kept, so sums of a few such values
/// cannot overflow even a 32-bit `usize`. Streams are capped well below it.
const MAX_OFF: usize = 0x3fff_ffff;
/// Geometry beyond this many EMU (about 2.3 km) is clamped.
const EMU_LIMIT: i128 = 1 << 31;
/// Style records parsed per file, and the longest style record read; with these
/// the style work is linear in the stream size.
const MAX_STYLE_CALLS: usize = 20_000;
const MAX_STYLE_BYTES: usize = 2048;
/// Work charged per block walked, on top of its bytes.
const BLOCK_COST: usize = 16;
/// Longest font name kept, in characters; longer names are cut at parse time.
const MAX_FONT_NAME_CHARS: usize = 128;
/// Pages kept from one file; the builder uses the same limit.
pub const MAX_PAGES_PARSED: usize = 2_000;

fn co(v: u32) -> usize {
    (v as usize).min(MAX_OFF)
}

/// Which Publisher generation wrote the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    /// Publisher 98: text inside `Contents`, no Quill storage.
    Pub98,
    /// Publisher 2000: flat chunk table, Quill text.
    Pub2000,
    /// Publisher 2002 and later (through 2010 in the corpus): block directory, Quill text, Escher shapes.
    Pub2002,
}

impl Version {
    pub fn label(self) -> &'static str {
        match self {
            Version::Pub98 => "Publisher 98",
            Version::Pub2000 => "Publisher 2000",
            Version::Pub2002 => "Publisher 2002 and later",
        }
    }
}

/// Bounds-checked little-endian reads.
#[derive(Clone, Copy)]
pub struct Bytes<'a>(pub &'a [u8]);

impl<'a> Bytes<'a> {
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn u8(&self, o: usize) -> Option<u8> {
        self.0.get(o).copied()
    }
    pub fn u16(&self, o: usize) -> Option<u16> {
        let b = self.0.get(o..o.checked_add(2)?)?;
        Some(u16::from_le_bytes([b[0], b[1]]))
    }
    pub fn u32(&self, o: usize) -> Option<u32> {
        let b = self.0.get(o..o.checked_add(4)?)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub fn i32(&self, o: usize) -> Option<i32> {
        self.u32(o).map(|v| v as i32)
    }
    pub fn slice(&self, o: usize, len: usize) -> Option<&'a [u8]> {
        self.0.get(o..o.checked_add(len)?)
    }
}

// ---------------------------------------------------------------------------
// Typed blocks
// ---------------------------------------------------------------------------

pub const B_STRING: u8 = 0xc0;
pub const B_CONTAINER: u8 = 0x88;
pub const B_DIRECTORY: u8 = 0x90;
pub const B_SEQNUM: u8 = 0x70;

/// Variable-length blocks that hold other blocks (everything variable but strings).
pub fn is_container(ty: u8) -> bool {
    matches!(ty, 0x80 | 0x82 | 0x88 | 0x8a | 0x90 | 0x98 | 0xa0)
}

/// A block: an id byte, a type byte that fixes the length class, then data.
#[derive(Debug, Clone, Copy)]
pub struct Block {
    pub id: u8,
    pub ty: u8,
    /// Offset of the data (for variable blocks, of the length field).
    pub data_off: usize,
    /// Data length (for variable blocks it includes the 4-byte length field).
    pub data_len: usize,
    /// Value of a fixed 2- or 4-byte block, else 0.
    pub value: u32,
    /// Offset just past the block.
    pub end: usize,
    /// Whether the type byte is one the importer knows.
    pub known: bool,
}

/// `Some(len)` for fixed-length types, `None` for variable ones (length field
/// first), `Some(0)` for unknown types (`known` is then false).
fn block_kind(ty: u8) -> (Option<usize>, bool) {
    match ty {
        0x78 | 0x05 | 0x08 | 0x0a => (Some(0), true),
        0x10 | 0x12 | 0x18 | 0x1a | 0x07 => (Some(2), true),
        0x20 | 0x22 | 0x58 | 0x68 | 0x70 | 0xb8 => (Some(4), true),
        0x28 => (Some(8), true),
        0x38 => (Some(16), true),
        0x48 => (Some(24), true),
        0xc0 | 0x80 | 0x82 | 0x88 | 0x8a | 0x90 | 0x98 | 0xa0 => (None, true),
        _ => (Some(0), false),
    }
}

/// Read one block at `pos`. `None` when it does not fit in the data.
pub fn block_at(b: &Bytes, pos: usize) -> Option<Block> {
    let id = b.u8(pos)?;
    let ty = b.u8(pos + 1)?;
    let data_off = pos + 2;
    let (fixed, known) = block_kind(ty);
    let (data_len, value) = match fixed {
        Some(n) => {
            b.slice(data_off, n)?;
            let v = match n {
                2 => u32::from(b.u16(data_off)?),
                4 => b.u32(data_off)?,
                _ => 0,
            };
            (n, v)
        }
        None => {
            let n = co(b.u32(data_off)?);
            if n < 4 {
                return None;
            }
            b.slice(data_off, n)?;
            (n, 0)
        }
    };
    Some(Block {
        id,
        ty,
        data_off,
        data_len,
        value,
        end: data_off + data_len,
        known,
    })
}

/// Sub-blocks of a variable block, in order. Stops at the first block that
/// does not fit. Unknown block types are reported.
pub fn children(b: &Bytes, blk: &Block, rep: &mut ImportReport) -> Vec<Block> {
    let mut out = Vec::new();
    if blk.data_len < 4 {
        return out;
    }
    sequence(b, blk.data_off + 4, blk.end, rep, &mut out);
    out
}

/// Blocks laid end to end in `[start, end)`.
pub fn sequence(b: &Bytes, start: usize, end: usize, rep: &mut ImportReport, out: &mut Vec<Block>) {
    let mut pos = start;
    let mut unknown = 0usize;
    let mut first_unknown = None;
    while pos + 2 <= end && out.len() < MAX_ITEMS {
        let Some(k) = block_at(b, pos) else { break };
        // Every block walked costs its bytes plus a fixed amount, so a stream of tiny
        // blocks is limited by count as well as by size.
        if k.end > end || k.end <= pos || !rep.charge_work(BLOCK_COST + (k.end - pos)) {
            break;
        }
        if !k.known {
            unknown += 1;
            first_unknown.get_or_insert((k.ty, k.id));
        }
        out.push(k);
        pos = k.end;
    }
    if let Some((ty, id)) = first_unknown {
        rep.drop_many(
            "unknown-block",
            unknown,
            format!("type 0x{ty:02x} id 0x{id:02x}"),
        );
    }
}

// ---------------------------------------------------------------------------
// Parsed model
// ---------------------------------------------------------------------------

/// A rectangle in EMU relative to the page centre (Publisher's convention).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EmuRect {
    pub xs: i64,
    pub ys: i64,
    pub xe: i64,
    pub ye: i64,
}

impl EmuRect {
    pub fn new(a: i64, b: i64, c: i64, d: i64) -> Self {
        Self {
            xs: a.min(c),
            ys: b.min(d),
            xe: a.max(c),
            ye: b.max(d),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PageRec {
    pub seq: u32,
    /// Seq numbers of the shapes listed on the page, back to front.
    pub shapes: Vec<u32>,
    /// Seq of the master page this page uses (2002+, page block 0x0d).
    pub master: Option<u32>,
}

/// A table shape (2002+, chunk type 0x10 with a 0x63 cell chunk below it).
#[derive(Debug, Clone, Default)]
pub struct TableRec {
    pub rows: u32,
    pub cols: u32,
    /// End position of each column / row from the table's origin, in EMU.
    pub col_ends: Vec<i64>,
    pub row_ends: Vec<i64>,
    /// Per cell, in file order: first row, last row, first column, last column.
    pub cells: Vec<[u32; 4]>,
}

/// Fill and line of a shape (Escher properties), as colour references.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LookRec {
    pub fill: Option<u32>,
    pub fill_type: Option<u32>,
    /// Fill boolean flags: fFilled is bit 4 (0x10) with its "use" bit at 0x10_0000.
    pub fill_flags: Option<u32>,
    pub line: Option<u32>,
    pub line_width: Option<i64>,
    pub line_flags: Option<u32>,
}

impl LookRec {
    /// The fill colour when the shape is explicitly filled.
    pub fn fill_color(&self) -> Option<u32> {
        let on = match self.fill_flags {
            Some(f) if f & 0x10_0000 != 0 => f & 0x10 != 0,
            _ => false,
        };
        if on {
            self.fill.filter(|c| *c != 0xffff_ffff)
        } else {
            None
        }
    }
    /// The outline colour when the outline is explicitly on.
    pub fn line_color(&self) -> Option<u32> {
        let on = match self.line_flags {
            Some(f) if f & 0x8_0000 != 0 => f & 0x8 != 0,
            _ => false,
        };
        if on {
            self.line.filter(|c| *c != 0xffff_ffff)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ShapeRec {
    pub seq: u32,
    /// Seq of the containing group or page chunk.
    pub parent: Option<u32>,
    pub page: Option<u32>,
    /// Previous and next frame of a linked text chain.
    pub prev: Option<u32>,
    pub next: Option<u32>,
    pub text_id: Option<u32>,
    pub rect: Option<EmuRect>,
    /// 0 top, 1 middle, 2 bottom.
    pub valign: u8,
    /// Text insets in EMU: left, top, right, bottom.
    pub insets: Option<[i64; 4]>,
    pub columns: Option<u32>,
    /// Column gap in EMU.
    pub column_gap: Option<i64>,
    /// Escher shape type or 2000 marker, kept for T24.
    pub shape_type: u32,
    /// Rotation in degrees.
    pub rotation: f64,
    /// Escher shape type (FSP instance): 1 rectangle, 2 rounded rectangle,
    /// 3 ellipse, 20 line, 202 text box; 0 when unknown.
    pub esc_type: u32,
    /// 1-based index into the BLIP store (Escher property pib).
    pub pib: Option<u32>,
    pub flip_h: bool,
    pub flip_v: bool,
    pub look: LookRec,
    pub table: Option<TableRec>,
}

/// A picture from the BLIP store.
#[derive(Debug, Clone, PartialEq)]
pub enum Blip {
    /// PNG or JPEG bytes (a GIF is converted to PNG on the way in).
    Raster(Vec<u8>),
    /// A Windows or Enhanced metafile, a Mac PICT, a DIB or a TIFF: not imported.
    Unsupported(&'static str),
    /// An empty or unreadable slot.
    Missing,
    /// The same picture as the earlier entry with this (0-based) index.
    Same(usize),
    /// Not kept: the file's pictures exceed the byte budget.
    OverBudget,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CharStyle {
    pub bold: bool,
    pub italic: bool,
    /// Underline kind (0 none).
    pub underline: u32,
    pub size_pt: Option<f64>,
    /// Colour reference: type byte 0x08 = palette index, otherwise `0x00BBGGRR`.
    pub color: Option<u32>,
    pub font: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParaStyle {
    /// Raw alignment value: 0 left, 1 right, 2 centre, 6 justify.
    pub align: Option<u32>,
    pub default_char: Option<u32>,
    /// `(is_points, amount)`: points, or a multiple of single spacing.
    pub line_spacing: Option<(bool, f64)>,
    pub space_before: Option<u32>,
    pub space_after: Option<u32>,
    pub first_indent: Option<i32>,
    pub left_indent: Option<u32>,
    pub right_indent: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct RawPara {
    pub style: ParaStyle,
    pub spans: Vec<(String, CharStyle)>,
}

#[derive(Debug, Clone, Default)]
pub struct RawStory {
    pub id: u32,
    pub paras: Vec<RawPara>,
}

/// Everything read from a file.
#[derive(Debug, Default)]
pub struct PubData {
    pub width_emu: i64,
    pub height_emu: i64,
    /// Pages in reading order (masters and placeholders removed).
    pub pages: Vec<PageRec>,
    pub shapes: Vec<ShapeRec>,
    pub stories: Vec<RawStory>,
    pub fonts: Vec<String>,
    pub palette: Vec<[u8; 3]>,
    pub default_char: Vec<CharStyle>,
    pub default_para: Vec<ParaStyle>,
    pub text_colors: Vec<u32>,
    /// Master pages that pages refer to (2002+).
    pub masters: Vec<PageRec>,
    /// The BLIP store, in file order (pib is 1-based into this).
    pub blips: Vec<Blip>,
}

/// Detect the version from the `Contents` header.
pub fn detect(contents: &[u8], has_quill: bool) -> Result<Version, IoError> {
    let b = Bytes(contents);
    if contents.len() < 4 || b.u8(0) != Some(0xe8) || b.u8(1) != Some(0xac) || b.u8(3) != Some(0) {
        return Err(IoError::Format(
            "pub: Contents stream has no Publisher header (E8 AC xx 00)".into(),
        ));
    }
    match b.u8(2) {
        Some(0x2c) => Ok(Version::Pub2002),
        Some(0x22) if has_quill => Ok(Version::Pub2000),
        Some(0x22) => Ok(Version::Pub98),
        Some(v) => Err(IoError::Format(format!(
            "pub: unsupported Publisher version byte 0x{v:02x} (supported: 0x22 = Publisher 98/2000, 0x2c = Publisher 2002 and later)"
        ))),
        None => Err(IoError::Format("pub: truncated Contents header".into())),
    }
}

// ---------------------------------------------------------------------------
// Colours of the 2000-era scheme
// ---------------------------------------------------------------------------

// The 56 built-in palette entries of the 2000-era file format: constants of the
// format (what each colour index means). The values were checked against the table
// libmspub documents for the same index range; only these data values are shared,
// no code structure.
const INDEXED: [[u8; 3]; 56] = [
    [0, 0, 0],
    [255, 255, 255],
    [255, 0, 0],
    [0, 255, 0],
    [0, 0, 255],
    [255, 255, 0],
    [0, 255, 255],
    [255, 0, 255],
    [128, 128, 128],
    [192, 192, 192],
    [128, 0, 0],
    [0, 128, 0],
    [0, 0, 128],
    [128, 128, 0],
    [0, 128, 128],
    [128, 0, 128],
    [255, 153, 51],
    [51, 0, 51],
    [0, 0, 153],
    [0, 153, 0],
    [153, 153, 0],
    [204, 102, 0],
    [153, 0, 0],
    [204, 153, 204],
    [102, 102, 255],
    [102, 255, 102],
    [255, 255, 153],
    [255, 204, 153],
    [255, 102, 102],
    [255, 153, 0],
    [0, 102, 255],
    [255, 204, 0],
    [153, 0, 51],
    [102, 51, 0],
    [66, 66, 66],
    [255, 153, 102],
    [153, 51, 0],
    [255, 102, 0],
    [51, 51, 0],
    [153, 204, 0],
    [255, 255, 153],
    [0, 51, 0],
    [51, 153, 102],
    [204, 255, 204],
    [0, 51, 102],
    [51, 204, 204],
    [204, 255, 255],
    [51, 102, 255],
    [0, 204, 255],
    [153, 204, 255],
    [51, 51, 153],
    [102, 102, 153],
    [153, 51, 102],
    [204, 153, 255],
    [51, 51, 51],
    [150, 150, 150],
];

/// A 2000-era colour word as `(r, g, b)`.
fn color_2k(hex: u32) -> [u8; 3] {
    match (hex >> 24) as u8 {
        0x80 | 0x00 => INDEXED
            .get((hex & 0xff) as usize)
            .copied()
            .unwrap_or([0, 0, 0]),
        0x90 | 0x20 => [hex as u8, (hex >> 8) as u8, (hex >> 16) as u8],
        _ => [0, 0, 0],
    }
}

/// Translate a 2000-era colour word to the 2002 convention used in `CharStyle`.
fn translate_2k_color(r: u32) -> u32 {
    match (r >> 24) as u8 {
        0xc0 | 0xe0 => (r & 0xff) | (0x08 << 24),
        _ => {
            let c = color_2k(r);
            u32::from(c[0]) | (u32::from(c[1]) << 8) | (u32::from(c[2]) << 16)
        }
    }
}

// ---------------------------------------------------------------------------
// Chunk directory
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct ChunkRef {
    pub ty: u32,
    pub offset: usize,
    pub parent: u32,
    pub seq: u32,
}

const C_SHAPE: u32 = 0x01;
const C_ALTSHAPE: u32 = 0x20;
const C_DOCUMENT: u32 = 0x44;
const C_PAGE: u32 = 0x43;
const C_PALETTE: u32 = 0x5c;
const C_GROUP: u32 = 0x30;
const C_LOGO: u32 = 0x31;
const C_TABLE: u32 = 0x10;
const C_TABLE_CELLS: u32 = 0x63;
/// Most rows plus columns of one table read (the model allows 200 and 40).
const TABLE_MAX_LINES: usize = 240;

/// The 2002+ directory: trailer at `0x1a`, a directory block of chunk references.
pub fn chunks_2002(b: &Bytes, rep: &mut ImportReport) -> Vec<ChunkRef> {
    let mut out = Vec::new();
    let Some(trailer) = b.u32(0x1a).map(co) else {
        return out;
    };
    let Some(tlen) = b.u32(trailer).map(co) else {
        return out;
    };
    let tend = trailer.saturating_add(tlen).min(b.len());
    // Top-level blocks in the trailer; the directory is the 0x90 one.
    let mut tops = Vec::new();
    sequence(b, trailer + 4, tend, rep, &mut tops);
    for top in tops.iter().filter(|t| t.ty == B_DIRECTORY) {
        let entries = children(b, top, rep);
        for (seq, e) in entries.iter().enumerate() {
            if e.ty != B_CONTAINER {
                continue;
            }
            if !rep.charge_items(1) {
                break;
            }
            let (mut ty, mut off, mut parent) = (None, None, 0u32);
            for sub in children(b, e, rep) {
                match sub.id {
                    2 => ty = Some(sub.value),
                    4 => off = Some(sub.value),
                    5 => parent = sub.value,
                    _ => {}
                }
            }
            if let (Some(ty), Some(off)) = (ty, off) {
                out.push(ChunkRef {
                    ty,
                    offset: co(off),
                    parent,
                    seq: seq as u32,
                });
            }
        }
    }
    out
}

/// The 98/2000 table: `u16` count at the trailer offset (`0x16`), 10-byte entries.
fn chunks_2000(b: &Bytes, rep: &mut ImportReport) -> Vec<(u32, u32, usize, u16)> {
    // (seq id, parent, offset, type marker)
    let mut out = Vec::new();
    let Some(trailer) = b.u32(0x16).map(co) else {
        return out;
    };
    let Some(n) = b.u16(trailer).map(usize::from) else {
        return out;
    };
    for i in 0..n {
        if !rep.charge_items(1) {
            break;
        }
        let p = trailer + 2 + i * 10;
        let (Some(id), Some(parent), Some(off)) = (b.u16(p + 2), b.u16(p + 4), b.u32(p + 6)) else {
            break;
        };
        let marker = b.u16(co(off)).unwrap_or(0xffff);
        out.push((u32::from(id), u32::from(parent), co(off), marker));
    }
    out
}

// ---------------------------------------------------------------------------
// Publisher 2002 and later
// ---------------------------------------------------------------------------

/// Placeholder ("dummy") page seq numbers in 2002+ files.
fn is_dummy_page_2002(seq: u32) -> bool {
    matches!(seq, 0x10d | 0x110 | 0x113 | 0x117)
}

fn parse_contents_2002(b: &Bytes, rep: &mut ImportReport, data: &mut PubData) {
    let chunks = chunks_2002(b, rep);
    let mut page_list: Vec<u32> = Vec::new();
    let mut page_recs: HashMap<u32, PageRec> = HashMap::new();
    let mut masters: HashSet<u32> = HashSet::new();
    let mut shapes: Vec<ShapeRec> = Vec::new();
    let mut table_cells: HashMap<u32, Vec<[u32; 4]>> = HashMap::new();
    for c in &chunks {
        if rep.exhausted() {
            break;
        }
        if !rep.first_visit(0, c.offset, c.ty) {
            continue;
        }
        match c.ty {
            C_DOCUMENT => parse_document_2002(b, c.offset, rep, data, &mut page_list),
            C_PAGE => {
                let mut rec = PageRec {
                    seq: c.seq,
                    shapes: Vec::new(),
                    master: None,
                };
                if parse_page_2002(b, c.offset, rep, &mut rec) {
                    masters.insert(c.seq);
                }
                if is_dummy_page_2002(c.seq) {
                    masters.insert(c.seq);
                }
                if rep.charge_items(1) {
                    page_recs.insert(c.seq, rec);
                }
            }
            C_SHAPE | C_ALTSHAPE | C_GROUP | C_LOGO | C_TABLE => {
                let mut s = ShapeRec {
                    seq: c.seq,
                    parent: Some(c.parent),
                    shape_type: c.ty,
                    ..Default::default()
                };
                parse_shape_2002(b, c.offset, rep, &mut s);
                if rep.charge_items(1) {
                    shapes.push(s);
                }
            }
            C_PALETTE => parse_palette_2002(b, c.offset, rep, data),
            C_TABLE_CELLS => {
                let cells = parse_table_cells_2002(b, c.offset, rep);
                table_cells.insert(c.parent, cells);
            }
            _ => rep.drop_item("chunk", format!("type 0x{:02x}", c.ty)),
        }
    }
    // Page order: the document's page list, else chunk order. Masters are not pages.
    let order: Vec<u32> = if page_list.is_empty() {
        chunks
            .iter()
            .filter(|c| c.ty == C_PAGE)
            .map(|c| c.seq)
            .collect()
    } else {
        page_list
    };
    let mut seen = HashSet::new();
    for seq in order {
        if masters.contains(&seq) || !seen.insert(seq) {
            continue;
        }
        // Placeholder and template pages are listed too but carry no shapes.
        if let Some(p) = page_recs.get(&seq).filter(|p| !p.shapes.is_empty()) {
            if data.pages.len() >= MAX_PAGES_PARSED {
                rep.drop_item("page", "page limit reached");
                break;
            }
            if !rep.charge_items(p.shapes.len() + 1) {
                break;
            }
            data.pages.push(p.clone());
        }
    }
    // Cell chunks belong to the table shape that is their parent.
    for s in &mut shapes {
        if let (Some(t), Some(cells)) = (s.table.as_mut(), table_cells.remove(&s.seq)) {
            t.cells = cells;
        }
    }
    // Master pages that pages refer to; a dangling reference is cleared.
    // A master must be a page chunk that is not itself one of the document's pages.
    let mut master_seqs: Vec<u32> = Vec::new();
    let real_pages: HashSet<u32> = data.pages.iter().map(|p| p.seq).collect();
    let mut page_as_master = 0usize;
    for p in &mut data.pages {
        if let Some(m) = p.master {
            if real_pages.contains(&m) {
                page_as_master += 1;
                p.master = None;
            } else if page_recs.contains_key(&m) {
                if !master_seqs.contains(&m) && master_seqs.len() < MAX_ITEMS {
                    master_seqs.push(m);
                }
            } else {
                p.master = None;
            }
        }
    }
    rep.drop_many(
        "master",
        page_as_master,
        "a page names another page of the document as its master",
    );
    for m in &master_seqs {
        if let Some(rec) = page_recs.get(m)
            && rep.charge_items(rec.shapes.len() + 1)
        {
            data.masters.push(rec.clone());
        }
    }
    let mut page_of: HashMap<u32, u32> = HashMap::new();
    for p in data.pages.iter().chain(data.masters.iter()) {
        for s in &p.shapes {
            page_of.entry(*s).or_insert(p.seq);
        }
    }
    let parent_of: HashMap<u32, u32> = shapes
        .iter()
        .filter_map(|s| Some((s.seq, s.parent?)))
        .collect();
    // Memo: seq -> (nearest listed ancestor's page, hops to it), or `None` when no
    // page is reachable. Each seq is resolved once, so the walks are linear.
    let mut memo: HashMap<u32, Option<(u32, usize)>> = HashMap::new();
    for s in &mut shapes {
        // A shape is on the page that lists it, or on the page of its nearest listed ancestor group.
        let mut path: Vec<u32> = Vec::new();
        let mut cur = s.seq;
        let base: Option<(u32, usize)> = loop {
            if let Some(known) = memo.get(&cur).copied() {
                break known;
            }
            if let Some(p) = page_of.get(&cur) {
                break Some((*p, 0));
            }
            // Marked unresolved while on the walk, so a cycle ends it with no page.
            memo.insert(cur, None);
            path.push(cur);
            match parent_of.get(&cur) {
                Some(next) if *next != cur => cur = *next,
                _ => break None,
            }
        };
        let k = path.len();
        for (i, n) in path.iter().enumerate() {
            memo.insert(*n, base.map(|(p, d)| (p, k - i + d)));
        }
        // Only a page within MAX_DEPTH hops counts, as before.
        if let Some((p, d)) = base.map(|(p, d)| (p, k + d))
            && d < MAX_DEPTH
        {
            s.page = Some(p);
        }
    }
    data.shapes = shapes;
}

fn parse_document_2002(
    b: &Bytes,
    off: usize,
    rep: &mut ImportReport,
    data: &mut PubData,
    page_list: &mut Vec<u32>,
) {
    let Some(len) = b.u32(off).map(co) else {
        return;
    };
    let end = off.saturating_add(len).min(b.len());
    let mut blocks = Vec::new();
    sequence(b, off + 4, end, rep, &mut blocks);
    for blk in blocks {
        match blk.id {
            0x12 if blk.data_len >= 4 && blk.ty != B_STRING => {
                for sub in children(b, &blk, rep) {
                    match sub.id {
                        1 => data.width_emu = i64::from(sub.value),
                        2 => data.height_emu = i64::from(sub.value),
                        _ => {}
                    }
                }
            }
            0x02 if blk.data_len >= 4 && blk.ty != B_STRING => {
                for sub in children(b, &blk, rep) {
                    if sub.id == 0 && rep.charge_items(1) {
                        page_list.push(sub.value);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Returns whether the page is a master (it has a non-empty name of its own).
/// Also reads the page's master reference (block 0x0d) into `rec.master`.
fn parse_page_2002(b: &Bytes, off: usize, rep: &mut ImportReport, rec: &mut PageRec) -> bool {
    let Some(len) = b.u32(off).map(co) else {
        return false;
    };
    let end = off.saturating_add(len).min(b.len());
    let mut blocks = Vec::new();
    sequence(b, off + 4, end, rep, &mut blocks);
    let mut master = false;
    for blk in blocks {
        match blk.id {
            0x02 if is_container(blk.ty) => {
                for sub in children(b, &blk, rep) {
                    if sub.ty == B_SEQNUM && rep.charge_items(1) {
                        rec.shapes.push(sub.value);
                    }
                }
            }
            0x0d if blk.ty == 0x68 => rec.master = Some(blk.value),
            0x0e if blk.ty == B_STRING => {
                let s = b
                    .slice(blk.data_off + 4, blk.data_len.saturating_sub(4))
                    .unwrap_or(&[]);
                master = s.iter().any(|c| *c != 0);
            }
            _ => {}
        }
    }
    master
}

fn parse_shape_2002(b: &Bytes, off: usize, rep: &mut ImportReport, s: &mut ShapeRec) {
    let Some(len) = b.u32(off).map(co) else {
        return;
    };
    let end = off.saturating_add(len).min(b.len());
    let mut blocks = Vec::new();
    sequence(b, off + 4, end, rep, &mut blocks);
    for blk in blocks {
        match blk.id {
            0x27 if blk.ty != B_STRING && blk.data_len <= 4 => s.text_id = Some(blk.value),
            0x35 if blk.data_len <= 4 => s.valign = (blk.value as u8).min(2),
            0x36 if blk.data_len == 4 && blk.ty != B_STRING => s.prev = Some(blk.value),
            0x37 if blk.data_len == 4 && blk.ty != B_STRING => s.next = Some(blk.value),
            // Table geometry: rows, columns, then a directory of column and row ends.
            0x66 if s.shape_type == C_TABLE && blk.ty == 0x20 => {
                s.table.get_or_insert_with(TableRec::default).rows = blk.value;
            }
            0x67 if s.shape_type == C_TABLE && blk.ty == 0x20 => {
                s.table.get_or_insert_with(TableRec::default).cols = blk.value;
            }
            0x6d if s.shape_type == C_TABLE && blk.ty == B_DIRECTORY => {
                let mut ends = Vec::new();
                for e in children(b, &blk, rep).iter().take(TABLE_MAX_LINES) {
                    for sub in children(b, e, rep) {
                        if sub.id == 1 && sub.data_len == 4 && rep.charge_items(1) {
                            ends.push(i64::from(sub.value as i32));
                        }
                    }
                }
                s.table.get_or_insert_with(TableRec::default).row_ends = ends;
            }
            _ => {}
        }
    }
    // The directory lists the column ends first, then the row ends.
    if let Some(t) = s.table.as_mut() {
        let all = std::mem::take(&mut t.row_ends);
        let nc = (t.cols as usize).min(all.len());
        t.col_ends = all[..nc].to_vec();
        t.row_ends = all[nc..].to_vec();
    }
}

/// Cells of a table (chunk 0x63): per cell the first and last row and column
/// (block ids 1 to 4, each 0 when absent).
fn parse_table_cells_2002(b: &Bytes, off: usize, rep: &mut ImportReport) -> Vec<[u32; 4]> {
    let mut out = Vec::new();
    let Some(len) = b.u32(off).map(co) else {
        return out;
    };
    let end = off.saturating_add(len).min(b.len());
    let mut blocks = Vec::new();
    sequence(b, off + 4, end, rep, &mut blocks);
    for blk in blocks.iter().filter(|k| k.id == 0x02 && k.ty == 0xa0) {
        for cell in children(b, blk, rep)
            .iter()
            .take(TABLE_MAX_LINES * TABLE_MAX_LINES)
        {
            let mut v = [0u32; 4];
            for sub in children(b, cell, rep) {
                if (1..=4).contains(&sub.id) && sub.data_len == 4 {
                    v[usize::from(sub.id) - 1] = sub.value;
                }
            }
            if !rep.charge_items(1) {
                return out;
            }
            out.push(v);
        }
    }
    out
}

fn parse_palette_2002(b: &Bytes, off: usize, rep: &mut ImportReport, data: &mut PubData) {
    let Some(len) = b.u32(off).map(co) else {
        return;
    };
    let end = off.saturating_add(len).min(b.len());
    let mut blocks = Vec::new();
    sequence(b, off + 4, end, rep, &mut blocks);
    for blk in blocks.iter().filter(|k| k.ty == 0xa0) {
        for sub in children(b, blk, rep) {
            if sub.ty == B_CONTAINER {
                for e in children(b, &sub, rep) {
                    if e.id == 1 && rep.charge_items(1) {
                        data.palette.push([
                            e.value as u8,
                            (e.value >> 8) as u8,
                            (e.value >> 16) as u8,
                        ]);
                    }
                }
            } else if sub.ty == 0x78 && rep.charge_items(1) {
                data.palette.push([0, 0, 0]);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Publisher 98 and 2000
// ---------------------------------------------------------------------------

const TEXT_MARKER_2000: u16 = 0x0008;
const TEXT_MARKER_98: u16 = 0x0000;

fn is_dummy_page_2000(seq: u32) -> bool {
    matches!(seq, 0x116 | 0x108 | 0x10b | 0x10d | 0x119)
}

fn parse_contents_2000(b: &Bytes, version: Version, rep: &mut ImportReport, data: &mut PubData) {
    let is98 = version == Version::Pub98;
    let table = chunks_2000(b, rep);
    let mut doc_off = None;
    let mut page_seqs: Vec<u32> = Vec::new();
    let mut shapes: Vec<(u32, u32, usize, u16)> = Vec::new();
    for &(seq, parent, off, marker) in &table {
        if rep.exhausted() {
            break;
        }
        if !rep.first_visit(0, off, u32::from(marker)) {
            continue;
        }
        match marker {
            0x0014 => {
                if rep.charge_items(1) {
                    page_seqs.push(seq);
                }
            }
            0x0015 => doc_off = Some(off),
            0x0047 => {
                // Palette: 8 colour words after 0xA0 bytes.
                for j in 0..8 {
                    if let Some(v) = b.u32(off + 0xa0 + 4 * j)
                        && rep.charge_items(1)
                    {
                        data.palette.push(color_2k(v));
                    }
                }
            }
            0x0000 | 0x0004 | 0x0005 | 0x0006 | 0x0007 | 0x0008 | 0x0002 | 0x000f => {
                if rep.charge_items(1) {
                    shapes.push((seq, parent, off, marker));
                }
            }
            0x0021 => {}
            m => rep.drop_item("chunk", format!("marker 0x{m:04x}")),
        }
    }
    let mut banner = false;
    if let Some(off) = doc_off {
        data.width_emu = i64::from(b.u32(off + 0x14).unwrap_or(0));
        data.height_emu = i64::from(b.u32(off + 0x18).unwrap_or(0));
        // A coordinate-system mark of 7 means a banner, which has a larger origin offset.
        banner = b.u16(off + 0x12) == Some(7);
    }
    let text_marker = if is98 {
        TEXT_MARKER_98
    } else {
        TEXT_MARKER_2000
    };
    let text_id_off = if is98 { 0x46 } else { 0x58 };
    let parents: HashMap<u32, u32> = table.iter().map(|t| (t.0, t.1)).collect();
    let page_set: HashSet<u32> = page_seqs.iter().copied().collect();
    let mut with_shapes: HashSet<u32> = HashSet::new();
    for &(seq, parent, off, marker) in &shapes {
        let mut s = ShapeRec {
            seq,
            shape_type: u32::from(marker),
            ..Default::default()
        };
        // Page: follow parents up to a page chunk.
        let mut p = parent;
        for _ in 0..MAX_DEPTH {
            if page_set.contains(&p) {
                s.page = Some(p);
                break;
            }
            match parents.get(&p) {
                Some(next) if *next != p => p = *next,
                _ => break,
            }
        }
        let Some(page) = s.page else { continue };
        if is_dummy_page_2000(page) || page == 0x109 {
            continue;
        }
        if marker == 0x000f {
            continue; // groups hold children; the children are listed on their own
        }
        if let (Some(xs), Some(ys), Some(xe), Some(ye)) = (
            b.i32(off + 6),
            b.i32(off + 10),
            b.i32(off + 14),
            b.i32(off + 18),
        ) {
            let (dx, dy) = if is98 {
                // Publisher 98 stores coordinates from a page-corner origin.
                let inch = 914_400i64;
                let o = if banner { 120 * inch } else { 25 * inch };
                (o, o)
            } else {
                (0, 0)
            };
            s.rect = Some(EmuRect::new(
                i64::from(xs) - dx,
                i64::from(ys) - dy,
                i64::from(xe) - dx,
                i64::from(ye) - dy,
            ));
        }
        if marker == text_marker {
            s.text_id = b.u16(off + text_id_off).map(u32::from);
        }
        with_shapes.insert(page);
        if rep.charge_items(1) {
            data.shapes.push(s);
        }
    }
    // Pages: every normal page chunk in seq order.
    page_seqs.sort_unstable();
    page_seqs.dedup();
    // Shapes bucketed by page once, so this is linear in pages + shapes.
    let mut by_page: HashMap<u32, Vec<u32>> = HashMap::new();
    for s in &data.shapes {
        if let Some(p) = s.page {
            by_page.entry(p).or_default().push(s.seq);
        }
    }
    for seq in page_seqs {
        if is_dummy_page_2000(seq) || seq == 0x109 {
            continue;
        }
        let shapes_here = by_page.remove(&seq).unwrap_or_default();
        if shapes_here.is_empty() {
            continue; // placeholder page
        }
        if data.pages.len() >= MAX_PAGES_PARSED {
            rep.drop_item("page", "page limit reached");
            break;
        }
        data.pages.push(PageRec {
            seq,
            shapes: shapes_here,
            master: None,
        });
    }
}

// ---------------------------------------------------------------------------
// Publisher 98 text (inside Contents)
// ---------------------------------------------------------------------------

/// Publisher 98 keeps its text in `Contents` itself, as single-byte text in the
/// local code page, with Word-style 512-byte property pages.
fn parse_text_98(b: &Bytes, rep: &mut ImportReport, data: &mut PubData) {
    let Some(base) = b.u32(0x12).map(co) else {
        return;
    };
    let (Some(start), Some(end), Some(p1), Some(p2)) = (
        b.u32(base + 14).map(co),
        b.u32(base + 18).map(co),
        b.u16(base + 22).map(usize::from),
        b.u16(base + 24).map(usize::from),
    ) else {
        return;
    };
    if end < start || end > b.len() {
        rep.drop_item("text", "Publisher 98 text range outside the stream");
        return;
    }
    let text = &b.0[start..end];
    if !rep.charge_work(text.len()) {
        return;
    }
    // Span ends and styles from the property pages p1..p2.
    let mut spans: Vec<(usize, CharStyle)> = Vec::new();
    let mut base_pos = 0u32;
    for (n, page) in (p1..p2.max(p1)).take(1024).enumerate() {
        let off = page * 0x200;
        if !rep.charge_work(0x200) {
            break;
        }
        let Some(count) = b.u8(off + 0x1ff).map(usize::from) else {
            break;
        };
        let Some(first) = b.u32(off) else { break };
        if n == 0 {
            base_pos = first;
        }
        let ends: Vec<u32> = (0..count).filter_map(|j| b.u32(off + 4 + 4 * j)).collect();
        let idx: Vec<u8> = (0..ends.len())
            .filter_map(|j| b.u8(off + 4 + 4 * count + j))
            .collect();
        // Style records follow the index bytes, aligned to the next non-zero byte.
        let mut pos = off + 4 + 4 * count + idx.len();
        while pos < off + 0x200 && b.u8(pos) == Some(0) {
            pos += 1;
        }
        let mut styles: HashMap<u8, CharStyle> = HashMap::new();
        let mut guard = 0;
        while pos < off + 0x1ff && guard < 512 {
            guard += 1;
            let Some(l) = b.u8(pos).map(usize::from) else {
                break;
            };
            let key = ((pos - off) / 2) as u8;
            if let Some(rec) = b.slice(pos + 1, l)
                && rep.charge_items(1)
            {
                styles.insert(key, char_style_98(rec));
            }
            pos += 1 + l;
        }
        for (j, e) in ends.iter().enumerate() {
            let st = idx
                .get(j)
                .and_then(|i| styles.get(i))
                .cloned()
                .unwrap_or_default();
            if !rep.charge_items(1) {
                break;
            }
            spans.push((co(e.saturating_sub(base_pos)), st));
        }
    }
    // Split into stories at form feeds, paragraphs at CR LF.
    let mut story = RawStory::default();
    let mut para = RawPara::default();
    let mut cur = String::new();
    let mut cur_style = CharStyle::default();
    let mut span_i = 0usize;
    let mut span_end = spans.first().map(|s| s.0).unwrap_or(usize::MAX);
    let mut i = 0usize;
    let mut next_id = 0u32;
    let flush = |cur: &mut String, para: &mut RawPara, st: &CharStyle, rep: &mut ImportReport| {
        if !cur.is_empty() && rep.charge_items(1) {
            para.spans.push((std::mem::take(cur), st.clone()));
        } else {
            cur.clear();
        }
    };
    if let Some(s) = spans.first() {
        cur_style = s.1.clone();
    }
    while i < text.len() {
        while i >= span_end && span_i + 1 < spans.len() {
            flush(&mut cur, &mut para, &cur_style, rep);
            span_i += 1;
            span_end = spans[span_i].0;
            cur_style = spans[span_i].1.clone();
        }
        let ch = text[i];
        match ch {
            0x0d => {
                if text.get(i + 1) == Some(&0x0a) {
                    i += 1;
                }
                flush(&mut cur, &mut para, &cur_style, rep);
                if rep.charge_items(1) {
                    story.paras.push(std::mem::take(&mut para));
                } else {
                    break;
                }
            }
            0x0c => {
                flush(&mut cur, &mut para, &cur_style, rep);
                if !para.spans.is_empty() {
                    story.paras.push(std::mem::take(&mut para));
                }
                story.id = next_id;
                next_id += 1;
                if rep.charge_items(1) {
                    data.stories.push(std::mem::take(&mut story));
                } else {
                    break;
                }
            }
            0x0b => cur.push(' '),
            0x09 => cur.push('\t'),
            c if c >= 0x20 => cur.push(cp1252(c)),
            _ => {}
        }
        i += 1;
    }
    flush(&mut cur, &mut para, &cur_style, rep);
    if !para.spans.is_empty() {
        story.paras.push(para);
    }
    if !story.paras.is_empty() {
        story.id = next_id;
        data.stories.push(story);
    }
}

fn char_style_98(rec: &[u8]) -> CharStyle {
    let mut st = CharStyle::default();
    let len = rec.len();
    if len >= 1 {
        st.bold = rec[0] & 1 != 0;
        st.italic = rec[0] & 2 != 0;
    }
    if len >= 3 {
        st.font = Some(u32::from(rec[2]));
    }
    if len >= 9 && rec[8] & 1 != 0 {
        st.underline = 1;
    }
    let mut var = 0i32;
    if len >= 6 {
        var = i32::from(i16::from_le_bytes([rec[4], rec[5]]));
    } else if len >= 5 {
        var = i32::from(rec[4] as i8);
    }
    st.size_pt = Some(10.0 + f64::from(var) / 2.0);
    if len >= 16 {
        let raw = u32::from_le_bytes([rec[12], rec[13], rec[14], rec[15]]);
        st.color = Some(translate_2k_color(raw));
    }
    st
}

/// Windows-1252 single byte to `char`.
pub fn cp1252(b: u8) -> char {
    const HI: [u16; 32] = [
        0x20ac, 0x81, 0x201a, 0x0192, 0x201e, 0x2026, 0x2020, 0x2021, 0x02c6, 0x2030, 0x0160,
        0x2039, 0x0152, 0x8d, 0x017d, 0x8f, 0x90, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013,
        0x2014, 0x02dc, 0x2122, 0x0161, 0x203a, 0x0153, 0x9d, 0x017e, 0x0178,
    ];
    if (0x80..0xa0).contains(&b) {
        char::from_u32(u32::from(HI[usize::from(b - 0x80)])).unwrap_or('\u{fffd}')
    } else {
        char::from(b)
    }
}

// ---------------------------------------------------------------------------
// Quill
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct QChunk {
    name: [u8; 4],
    offset: usize,
}

fn quill_chunks(b: &Bytes, rep: &mut ImportReport) -> Vec<QChunk> {
    let mut out = Vec::new();
    let mut list = 0x18usize;
    let mut seen = HashSet::new();
    while list != 0xffff_ffff && out.len() < MAX_ITEMS && seen.insert(list) && seen.len() < 4096 {
        let (Some(n), Some(next)) = (b.u16(list + 2), b.u32(list + 4)) else {
            break;
        };
        for i in 0..usize::from(n) {
            if out.len() >= MAX_ITEMS || !rep.charge_items(1) {
                break;
            }
            let p = list + 8 + i * 24;
            let (Some(name), Some(off)) = (b.slice(p + 2, 4), b.u32(p + 16)) else {
                break;
            };
            out.push(QChunk {
                name: [name[0], name[1], name[2], name[3]],
                offset: co(off),
            });
        }
        list = co(next);
    }
    out
}

fn parse_para_style(b: &Bytes, off: usize, rep: &mut ImportReport) -> ParaStyle {
    let mut st = ParaStyle::default();
    let Some(len) = b.u32(off).map(co) else {
        return st;
    };
    if len > MAX_STYLE_BYTES {
        rep.drop_item(
            "style-truncated",
            format!("style record of {len} bytes read only to {MAX_STYLE_BYTES}"),
        );
    }
    let end = off.saturating_add(len.min(MAX_STYLE_BYTES)).min(b.len());
    let mut blocks = Vec::new();
    sequence(b, off + 4, end, rep, &mut blocks);
    for blk in blocks {
        let v = blk.value;
        match blk.id {
            0x04 => st.align = Some(v & 0xff),
            0x19 => st.default_char = Some(v),
            0x34 => {
                if v & 1 != 0 {
                    st.line_spacing = Some((true, f64::from(v - 1) / 8.0 * 72.0 / 914_400.0));
                } else if v & 2 != 0 {
                    st.line_spacing = Some((false, f64::from(v - 2) / 914_400.0 * 72.0 / 96.0));
                }
            }
            0x12 => st.space_before = Some(v),
            0x13 => st.space_after = Some(v),
            0x0c => st.first_indent = Some(v as i32),
            0x0d => st.left_indent = Some(v),
            0x0e => st.right_indent = Some(v),
            _ => {}
        }
    }
    st
}

fn parse_char_style(b: &Bytes, off: usize, rep: &mut ImportReport, ctx: &StyleCtx) -> CharStyle {
    let mut st = CharStyle::default();
    let Some(len) = b.u32(off).map(co) else {
        return st;
    };
    if len > MAX_STYLE_BYTES {
        rep.drop_item(
            "style-truncated",
            format!("style record of {len} bytes read only to {MAX_STYLE_BYTES}"),
        );
    }
    let end = off.saturating_add(len.min(MAX_STYLE_BYTES)).min(b.len());
    let mut blocks = Vec::new();
    sequence(b, off + 4, end, rep, &mut blocks);
    for blk in &blocks {
        let v = blk.value;
        match blk.id {
            0x02 => st.bold = true,
            0x03 => st.italic = true,
            0x1e => st.underline = v & 0xff,
            0x0c if blk.data_len <= 4 => st.size_pt = Some(f64::from(v) * 72.0 / 914_400.0),
            0x2e if blk.data_len <= 4 => st.color = ctx.color(v),
            0x44 if is_container(blk.ty) => {
                for sub in children(b, blk, rep) {
                    if sub.id == 0 {
                        st.color = ctx.color(sub.value);
                        break;
                    }
                }
            }
            0x24 if is_container(blk.ty) => {
                // 2002+: containers per script, each starting with the font index.
                // 2000: the font index values directly.
                if let Some(sub) = children(b, blk, rep).first() {
                    if is_container(sub.ty) {
                        if let Some(first) = children(b, sub, rep).first() {
                            st.font = Some(first.value);
                        }
                    } else if sub.data_len == 2 {
                        st.font = Some(sub.value);
                    }
                }
            }
            _ => {}
        }
    }
    st
}

/// How a colour word in a character style maps to a colour reference.
struct StyleCtx {
    legacy: bool,
    text_colors: Vec<u32>,
}

impl StyleCtx {
    fn color(&self, raw: u32) -> Option<u32> {
        if self.legacy {
            Some(translate_2k_color(raw))
        } else {
            self.text_colors.get(raw as usize).copied()
        }
    }
}

/// A run boundary: exclusive end offset in the Quill stream, and the style record.
type Boundary<T> = Vec<(usize, T)>;

/// `(end offset, style record offset)` of each entry of a property chunk.
fn property_runs_raw(b: &Bytes, chunk: &QChunk, rep: &mut ImportReport) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let off = chunk.offset;
    let Some(n) = b.u16(off).map(usize::from) else {
        return out;
    };
    for i in 0..n {
        let (Some(end), Some(rel)) = (b.u32(off + 8 + 4 * i), b.u16(off + 8 + 4 * n + 2 * i))
        else {
            break;
        };
        if !rep.charge_items(1) {
            break;
        }
        out.push((co(end), off + usize::from(rel)));
    }
    out
}

/// Take one style parse from the per-file allowance; report when it is spent.
fn style_allowed(left: &mut usize, rep: &mut ImportReport) -> bool {
    if *left == 0 {
        rep.drop_item(
            "style-budget",
            format!("more than {MAX_STYLE_CALLS} style records; the rest use defaults"),
        );
        return false;
    }
    *left -= 1;
    true
}

fn parse_quill(b: &Bytes, legacy: bool, rep: &mut ImportReport, data: &mut PubData) {
    let chunks = quill_chunks(b, rep);
    if chunks.is_empty() {
        rep.drop_item("quill", "no chunk list");
        return;
    }
    // Each (offset, name) chunk is parsed once, however many list entries name it.
    let chunks: Vec<QChunk> = chunks
        .into_iter()
        .filter(|c| rep.first_visit(1, c.offset, u32::from_le_bytes(c.name)))
        .collect();
    let mut lengths: Vec<u32> = Vec::new();
    let mut ids: Vec<u32> = Vec::new();
    let mut text_off = None;
    let mut stsh_seen = 0;
    // Colours and fonts first: styles refer to them.
    for c in &chunks {
        match &c.name {
            b"PL  " => {
                let Some(n) = b.u32(c.offset).map(co) else {
                    continue;
                };
                let mut pos = c.offset + 12;
                for _ in 0..n.min(b.len() / 8) {
                    let Some(l) = b.u32(pos).map(co) else {
                        break;
                    };
                    // A stride below 4 never advances; stop rather than spin.
                    if l < 4 || rep.exhausted() {
                        break;
                    }
                    // Charged on every entry: a stride of 4 walks no blocks, so
                    // `sequence` alone would not bound the loop.
                    let span = l.min(b.len().saturating_sub(pos));
                    if !rep.charge_work(BLOCK_COST + span) {
                        break;
                    }
                    let mut blocks = Vec::new();
                    sequence(
                        b,
                        pos + 4,
                        pos.saturating_add(l).min(b.len()),
                        rep,
                        &mut blocks,
                    );
                    for blk in blocks.iter().filter(|k| k.id == 1) {
                        if rep.charge_items(1) {
                            data.text_colors.push(blk.value);
                        }
                    }
                    pos = pos.saturating_add(l);
                }
            }
            b"FONT" => {
                let Some(n) = b.u32(c.offset + 4).map(co) else {
                    continue;
                };
                let n = n.min(b.len() / 6);
                let mut pos = c.offset + 8 + 12 + 4 * n;
                for _ in 0..n {
                    let Some(nl) = b.u16(pos).map(usize::from) else {
                        break;
                    };
                    if !rep.charge_items(1) {
                        break;
                    }
                    pos += 2;
                    // Charge only the bytes that exist to be read.
                    let avail = (nl * 2).min(b.len().saturating_sub(pos));
                    if !rep.charge_work(avail) {
                        break;
                    }
                    let mut name = if nl > 0 {
                        let raw = b.slice(pos, avail).unwrap_or(&[]);
                        utf16(raw)
                    } else {
                        String::new()
                    };
                    pos += nl * 2 + 4;
                    // Every text run keeps a copy of its font name, so names are short.
                    if name.chars().count() > MAX_FONT_NAME_CHARS {
                        let kept: String = name.chars().take(MAX_FONT_NAME_CHARS).collect();
                        rep.drop_item(
                            "font-name-truncated",
                            format!("font name of {nl} units cut to {MAX_FONT_NAME_CHARS} chars"),
                        );
                        name = kept;
                    }
                    data.fonts.push(name);
                }
            }
            _ => {}
        }
    }
    let ctx = StyleCtx {
        legacy,
        text_colors: data.text_colors.clone(),
    };
    let mut style_left = MAX_STYLE_CALLS;
    let mut char_runs: Boundary<CharStyle> = Vec::new();
    let mut para_runs: Boundary<ParaStyle> = Vec::new();
    for c in &chunks {
        match &c.name {
            b"TEXT" => text_off = Some(c.offset),
            b"STRS" => {
                let Some(n) = b.u32(c.offset).map(co) else {
                    continue;
                };
                let Some(skip) = b.u32(c.offset + 4).map(co) else {
                    continue;
                };
                let start = c.offset.saturating_add(4).saturating_add(skip);
                for j in 0..n.min(b.len() / 4) {
                    let Some(l) = b.u32(start + 4 * j) else {
                        break;
                    };
                    if !rep.charge_items(1) {
                        break;
                    }
                    lengths.push(l);
                }
            }
            b"SYID" => {
                let Some(n) = b.u32(c.offset + 4).map(co) else {
                    continue;
                };
                for j in 0..n.min(b.len() / 4) {
                    let Some(v) = b.u32(c.offset + 8 + 4 * j) else {
                        break;
                    };
                    if !rep.charge_items(1) {
                        break;
                    }
                    ids.push(v);
                }
            }
            b"FDPC" => {
                for (end, at) in property_runs_raw(b, c, rep) {
                    let st = if style_allowed(&mut style_left, rep) {
                        parse_char_style(b, at, rep, &ctx)
                    } else {
                        CharStyle::default()
                    };
                    char_runs.push((end, st));
                }
            }
            b"FDPP" => {
                for (end, at) in property_runs_raw(b, c, rep) {
                    let st = if style_allowed(&mut style_left, rep) {
                        parse_para_style(b, at, rep)
                    } else {
                        ParaStyle::default()
                    };
                    para_runs.push((end, st));
                }
            }
            b"STSH" => {
                stsh_seen += 1;
                if stsh_seen == 2 {
                    let Some(n) = b.u32(c.offset + 4).map(co) else {
                        continue;
                    };
                    let n = n.min(b.len() / 4);
                    for i in 0..n {
                        let Some(rel) = b.u32(c.offset + 20 + 4 * i) else {
                            break;
                        };
                        if !rep.charge_items(1) || !style_allowed(&mut style_left, rep) {
                            break;
                        }
                        let at = c.offset + 20 + co(rel) + 2;
                        if i % 2 == 0 {
                            data.default_char.push(parse_char_style(b, at, rep, &ctx));
                        } else {
                            data.default_para.push(parse_para_style(b, at, rep));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let Some(text_off) = text_off else {
        rep.drop_item("quill", "no TEXT chunk");
        return;
    };
    // Stories in order; each is `lengths[j]` UTF-16 units.
    let mut cursor = text_off;
    for (j, len) in lengths.iter().enumerate() {
        let id = ids.get(j).copied().unwrap_or(j as u32);
        let bytes = co(*len).saturating_mul(2);
        let start = cursor;
        let end = start.saturating_add(bytes).min(b.len());
        cursor = start.saturating_add(bytes);
        if !rep.charge_items(1) {
            break;
        }
        if start >= b.len() {
            data.stories.push(RawStory { id, paras: vec![] });
            continue;
        }
        let paras = build_paras(b, start, end, &para_runs, &char_runs, rep);
        data.stories.push(RawStory { id, paras });
    }
}

/// Cut the text in `[start, end)` into paragraphs and style spans.
fn build_paras(
    b: &Bytes,
    start: usize,
    end: usize,
    para_runs: &Boundary<ParaStyle>,
    char_runs: &Boundary<CharStyle>,
    rep: &mut ImportReport,
) -> Vec<RawPara> {
    if !rep.charge_work(end.saturating_sub(start)) {
        return Vec::new();
    }
    // Runs are sorted by end offset; find the first that ends after `start`.
    let first_para = para_runs.partition_point(|(e, _)| *e <= start);
    let first_char = char_runs.partition_point(|(e, _)| *e <= start);
    let mut paras = Vec::new();
    let mut pi = first_para;
    let mut ci = first_char;
    let mut pos = start;
    while pos < end {
        let para_end = para_runs
            .get(pi)
            .map(|p| p.0)
            .unwrap_or(end)
            .clamp(pos + 2, end.max(pos + 2))
            .min(end);
        let style = para_runs.get(pi).map(|p| p.1.clone()).unwrap_or_default();
        let mut spans: Vec<(String, CharStyle)> = Vec::new();
        let mut p = pos;
        while p < para_end {
            let span_end = char_runs
                .get(ci)
                .map(|c| c.0)
                .unwrap_or(para_end)
                .clamp(p + 2, para_end.max(p + 2))
                .min(para_end);
            let cs = char_runs.get(ci).map(|c| c.1.clone()).unwrap_or_default();
            let raw = b.slice(p, (span_end - p) & !1).unwrap_or(&[]);
            let text = clean_text(&utf16(raw));
            if !text.is_empty() {
                if !rep.charge_items(1) {
                    break;
                }
                spans.push((text, cs));
            }
            p = span_end;
            if char_runs.get(ci).is_some_and(|c| c.0 <= span_end) {
                ci += 1;
            }
        }
        if !rep.charge_items(1) {
            break;
        }
        paras.push(RawPara { style, spans });
        pos = para_end;
        if para_runs.get(pi).is_some_and(|c| c.0 <= para_end) {
            pi += 1;
        }
        if paras.len() >= MAX_ITEMS {
            break;
        }
    }
    paras
}

/// Drop control characters (paragraph marks are handled by the caller); a vertical
/// tab is a line break and becomes a space.
fn clean_text(s: &str) -> String {
    s.chars()
        .filter_map(|c| match c {
            '\r' | '\u{c}' => None,
            '\u{b}' | '\u{2028}' => Some(' '),
            '\t' => Some('\t'),
            c if (c as u32) < 0x20 => None,
            c => Some(c),
        })
        .collect()
}

pub fn utf16(raw: &[u8]) -> String {
    let units = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c));
    char::decode_utf16(units)
        .map(|r| r.unwrap_or('\u{fffd}'))
        .collect()
}

// ---------------------------------------------------------------------------
// Escher
// ---------------------------------------------------------------------------

const ESC_DGG: u16 = 0xf000;
const ESC_BSTORE: u16 = 0xf001;
const ESC_DG: u16 = 0xf002;
const ESC_SPGR: u16 = 0xf003;
const ESC_SP: u16 = 0xf004;
const ESC_FSP: u16 = 0xf00a;
const ESC_FSPGR: u16 = 0xf009;
const ESC_FOPT: u16 = 0xf00b;
const ESC_CHILD_ANCHOR: u16 = 0xf00f;
const ESC_CLIENT_ANCHOR: u16 = 0xf010;
const ESC_CLIENT_DATA: u16 = 0xf011;
const ESC_TERTIARY_FOPT: u16 = 0xf122;

#[derive(Clone, Copy)]
struct EscRec {
    ty: u16,
    inst: u16,
    off: usize,
    len: usize,
}

fn esc_rec(b: &Bytes, pos: usize, limit: usize) -> Option<EscRec> {
    let v = b.u16(pos)?;
    let ty = b.u16(pos + 2)?;
    let len = co(b.u32(pos + 4)?);
    let off = pos + 8;
    if off.checked_add(len)? > limit {
        return None;
    }
    Some(EscRec {
        ty,
        inst: v >> 4,
        off,
        len,
    })
}

/// Geometry and properties found in the Escher stream, keyed by shape seq.
#[derive(Debug, Default, Clone)]
pub struct EscherShape {
    pub rect: Option<EmuRect>,
    pub shape_type: u32,
    pub rotation: f64,
    pub insets: Option<[i64; 4]>,
    pub columns: Option<u32>,
    pub column_gap: Option<i64>,
    pub is_group: bool,
    pub pib: Option<u32>,
    pub flip_h: bool,
    pub flip_v: bool,
    pub look: LookRec,
}

/// Combine two boolean-property words: bits whose "use" bit is set in `new`
/// replace those in `old`.
fn merge_flags(old: Option<u32>, new: u32) -> u32 {
    let old = old.unwrap_or(0);
    let used = new >> 16;
    let value = (old & 0xffff & !used) | (new & 0xffff & used);
    (((old >> 16) | used) << 16) | value
}

/// Shapes by Publisher seq and the BLIP store of an `EscherStm` stream.
#[derive(Debug, Default)]
pub struct EscherData {
    pub shapes: BTreeMap<u32, EscherShape>,
    pub blips: Vec<Blip>,
}

pub fn parse_escher(buf: &[u8], delay: Option<&[u8]>, rep: &mut ImportReport) -> EscherData {
    let b = Bytes(buf);
    let mut out = BTreeMap::new();
    let mut blips: Vec<Blip> = Vec::new();
    let mut pos = 0usize;
    let mut guard = 0;
    while pos + 8 <= b.len() && guard < MAX_ITEMS {
        guard += 1;
        if !rep.charge_items(1) {
            break;
        }
        let Some(r) = esc_rec(&b, pos, b.len()) else {
            break;
        };
        match r.ty {
            ESC_DG => {
                walk_group(&b, &r, (0, 0, 1, 1), (0, 0, 1, 1), 0, rep, &mut out);
            }
            ESC_DGG => {
                // The drawing group holds the BLIP store container.
                let mut p = r.off;
                let end = r.off + r.len;
                let mut guard = 0;
                while p + 8 <= end && guard < 256 {
                    guard += 1;
                    let Some(c) = esc_rec(&b, p, end) else { break };
                    if c.ty == ESC_BSTORE {
                        super::images::read_store(buf, (c.off, c.len), delay, rep, &mut blips);
                    }
                    p = c.off + c.len;
                }
            }
            t => rep.drop_item("escher", format!("top-level record 0x{t:04x}")),
        }
        pos = r.off + r.len;
        if matches!(r.ty, ESC_DGG | ESC_DG) {
            pos += 4; // Publisher puts a 4-byte tail after these containers
        }
    }
    EscherData { shapes: out, blips }
}

type Coord = (i64, i64, i64, i64);

fn walk_group(
    b: &Bytes,
    group: &EscRec,
    coord_sys: Coord,
    group_abs: Coord,
    depth: usize,
    rep: &mut ImportReport,
    out: &mut BTreeMap<u32, EscherShape>,
) {
    if depth > MAX_DEPTH {
        rep.drop_item("escher", "group nesting too deep");
        return;
    }
    let mut pos = group.off;
    let end = group.off + group.len;
    let mut sys = coord_sys;
    let mut abs = group_abs;
    let mut guard = 0;
    while pos + 8 <= end && guard < MAX_ITEMS {
        guard += 1;
        if !rep.charge_items(1) {
            break;
        }
        let Some(r) = esc_rec(b, pos, end) else { break };
        match r.ty {
            ESC_SPGR => walk_group(b, &r, sys, abs, depth + 1, rep, out),
            ESC_SP => parse_sp(b, &r, &mut sys, &mut abs, rep, out),
            ESC_DG => walk_group(b, &r, sys, abs, depth + 1, rep, out),
            _ => {}
        }
        pos = r.off + r.len;
    }
}

fn parse_sp(
    b: &Bytes,
    sp: &EscRec,
    sys: &mut Coord,
    abs: &mut Coord,
    rep: &mut ImportReport,
    out: &mut BTreeMap<u32, EscherShape>,
) {
    let this_sys = *sys;
    let mut shape = EscherShape::default();
    let mut seq: Option<u32> = None;
    let mut defines_coords = false;
    let mut anchor: Option<(EscRec, bool)> = None;
    let mut pos = sp.off;
    let end = sp.off + sp.len;
    let mut guard = 0;
    while pos + 8 <= end && guard < 256 {
        guard += 1;
        if !rep.charge_items(1) {
            break;
        }
        let Some(r) = esc_rec(b, pos, end) else { break };
        match r.ty {
            ESC_FSPGR => {
                if let (Some(a), Some(c), Some(d), Some(e)) = (
                    b.i32(r.off),
                    b.i32(r.off + 4),
                    b.i32(r.off + 8),
                    b.i32(r.off + 12),
                ) {
                    *sys = (
                        i64::from(a.min(d)),
                        i64::from(c.min(e)),
                        i64::from(a.max(d)),
                        i64::from(c.max(e)),
                    );
                    defines_coords = true;
                }
            }
            ESC_FSP => {
                shape.shape_type = u32::from(r.inst);
                let flags = b.u32(r.off + 4).unwrap_or(0);
                shape.is_group = flags & 1 != 0;
                shape.flip_h = flags & 0x40 != 0;
                shape.flip_v = flags & 0x80 != 0;
            }
            ESC_CLIENT_DATA => {
                for (id, v) in pairs(b, r.off + 4, r.off + r.len) {
                    if id == 0x6801 {
                        seq = Some(v);
                    }
                }
            }
            ESC_CLIENT_ANCHOR => anchor = Some((r, true)),
            ESC_CHILD_ANCHOR => anchor = Some((r, false)),
            ESC_FOPT => {
                for (id, v) in fopt_pairs(b, &r) {
                    match id {
                        0x0004 => shape.rotation = f64::from(v as i32) / 65536.0,
                        0x0081 => set_inset(&mut shape.insets, 0, v),
                        0x0082 => set_inset(&mut shape.insets, 1, v),
                        0x0083 => set_inset(&mut shape.insets, 2, v),
                        0x0084 => set_inset(&mut shape.insets, 3, v),
                        _ => fill_line_prop(&mut shape, id, v),
                    }
                }
            }
            ESC_TERTIARY_FOPT => {
                for (id, v) in fopt_pairs(b, &r) {
                    match id {
                        0x008c => shape.columns = Some(v),
                        0x008d => shape.column_gap = Some(i64::from(v)),
                        _ => fill_line_prop(&mut shape, id, v),
                    }
                }
            }
            _ => {}
        }
        pos = r.off + r.len;
    }
    let Some(seq) = seq else {
        rep.drop_item("escher", "shape without a Publisher shape id");
        return;
    };
    if let Some((r, is_client)) = anchor {
        let rect = if is_client {
            let mut v = [0i64; 4];
            for (id, val) in pairs(b, r.off + 4, r.off + r.len) {
                match id {
                    0x2001 => v[0] = i64::from(val as i32),
                    0x2002 => v[1] = i64::from(val as i32),
                    0x2003 => v[2] = i64::from(val as i32),
                    0x2004 => v[3] = i64::from(val as i32),
                    _ => {}
                }
            }
            Some(EmuRect::new(v[0], v[1], v[2], v[3]))
        } else {
            child_rect(b, r.off, this_sys, *abs)
        };
        if let Some(rc) = rect {
            if defines_coords {
                *abs = (rc.xs, rc.ys, rc.xe, rc.ye);
            }
            shape.rect = Some(rc);
        }
    }
    out.insert(seq, shape);
}

/// Picture, fill and line properties (MS-ODRAW property numbers; the picture
/// index carries the "blip" flag 0x4000 in its id).
fn fill_line_prop(shape: &mut EscherShape, id: u16, v: u32) {
    match id {
        0x4104 | 0x0104 => shape.pib = Some(v),
        0x0180 => shape.look.fill_type = Some(v),
        0x0181 => shape.look.fill = Some(v),
        0x01bf => shape.look.fill_flags = Some(merge_flags(shape.look.fill_flags, v)),
        0x01c0 => shape.look.line = Some(v),
        0x01cb => shape.look.line_width = Some(i64::from(v as i32)),
        0x01ff => shape.look.line_flags = Some(merge_flags(shape.look.line_flags, v)),
        _ => {}
    }
}

fn set_inset(slot: &mut Option<[i64; 4]>, i: usize, v: u32) {
    let a = slot.get_or_insert([36_576; 4]);
    a[i] = i64::from(v as i32);
}

fn child_rect(b: &Bytes, off: usize, sys: Coord, abs: Coord) -> Option<EmuRect> {
    let (a, c, d, e) = (
        i64::from(b.i32(off)?),
        i64::from(b.i32(off + 4)?),
        i64::from(b.i32(off + 8)?),
        i64::from(b.i32(off + 12)?),
    );
    // File-supplied values: arithmetic in i128, clamped back to a sane EMU range.
    let sw = (i128::from(sys.2) - i128::from(sys.0)).max(1);
    let sh = (i128::from(sys.3) - i128::from(sys.1)).max(1);
    let gw = i128::from(abs.2) - i128::from(abs.0);
    let gh = i128::from(abs.3) - i128::from(abs.1);
    let f = |v: i64, s0: i64, sd: i128, a0: i64, gd: i128| -> i64 {
        let r = (i128::from(v) - i128::from(s0)) * gd / sd + i128::from(a0);
        r.clamp(-EMU_LIMIT, EMU_LIMIT) as i64
    };
    Some(EmuRect::new(
        f(a, sys.0, sw, abs.0, gw),
        f(c, sys.1, sh, abs.1, gh),
        f(d, sys.0, sw, abs.0, gw),
        f(e, sys.1, sh, abs.1, gh),
    ))
}

/// `(u16 id, u32 value)` pairs in `[start, end)`.
fn pairs(b: &Bytes, start: usize, end: usize) -> Vec<(u16, u32)> {
    let mut out = Vec::new();
    let mut p = start;
    while p + 6 <= end && out.len() < 4096 {
        let (Some(id), Some(v)) = (b.u16(p), b.u32(p + 2)) else {
            break;
        };
        out.push((id, v));
        p += 6;
    }
    out
}

/// The scalar properties of an FOPT record (count in the record instance).
fn fopt_pairs(b: &Bytes, r: &EscRec) -> Vec<(u16, u32)> {
    let n = usize::from(r.inst).min(r.len / 6);
    pairs(b, r.off, r.off + n * 6)
}

// ---------------------------------------------------------------------------
// Entry point for the parsed file
// ---------------------------------------------------------------------------

/// Parse the streams of a container into [`PubData`].
pub fn parse(
    version: Version,
    contents: &[u8],
    quill: Option<&[u8]>,
    escher: Option<&[u8]>,
    delay: Option<&[u8]>,
    rep: &mut ImportReport,
) -> PubData {
    let mut data = PubData::default();
    let b = Bytes(contents);
    rep.set_input(
        contents.len()
            + quill.map_or(0, <[u8]>::len)
            + escher.map_or(0, <[u8]>::len)
            + delay.map_or(0, <[u8]>::len),
    );
    match version {
        Version::Pub2002 => {
            parse_contents_2002(&b, rep, &mut data);
            if let Some(q) = quill {
                parse_quill(&Bytes(q), false, rep, &mut data);
            }
            if let Some(e) = escher {
                let esc = parse_escher(e, delay, rep);
                data.blips = esc.blips;
                let geo = esc.shapes;
                for s in &mut data.shapes {
                    if let Some(g) = geo.get(&s.seq) {
                        s.esc_type = g.shape_type;
                        s.pib = g.pib;
                        s.flip_h = g.flip_h;
                        s.flip_v = g.flip_v;
                        s.look = g.look;
                        s.rect = g.rect;
                        s.rotation = g.rotation;
                        s.insets = g.insets;
                        s.columns = g.columns;
                        s.column_gap = g.column_gap;
                        if s.shape_type == 0 {
                            s.shape_type = g.shape_type;
                        }
                    }
                }
            }
        }
        Version::Pub2000 => {
            parse_contents_2000(&b, version, rep, &mut data);
            if let Some(q) = quill {
                parse_quill(&Bytes(q), true, rep, &mut data);
            }
        }
        Version::Pub98 => {
            parse_contents_2000(&b, version, rep, &mut data);
            parse_text_98(&b, rep, &mut data);
        }
    }
    data
}

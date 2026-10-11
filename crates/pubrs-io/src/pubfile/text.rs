//! Map parsed Publisher data onto a pubrs [`Document`], through `DocBuilder`
//! commands: pages, text frames with their geometry, linked chains, story text
//! with character and paragraph formatting.

use std::collections::HashMap;

use pubrs_core::builder::DocBuilder;
use pubrs_core::command::Command;
use pubrs_core::geom::{Margins, Rect, Size};
use pubrs_core::id::{AssetId, FrameId, MasterId, PageId, SeqIds, StoryId, SwatchId};
use pubrs_core::model::{
    Document, Frame, FrameContent, ImageFit, ImageFrame, LineDirection, MasterPage, PageSetup,
    Paragraph, Parent, Run, Shape, ShapeFrame, Story, Stroke, TextFrame, TextWrap, VerticalAlign,
};
use pubrs_core::style::{Align, CharProps, ParaProps};
use pubrs_core::table::{CellStyle, TABLE_MAX_COLS, TABLE_MAX_ROWS, TableFrame};
use pubrs_core::{Color, Paint, SchemeSlot, Swatch, SwatchValue};

use crate::IoError;
use crate::assets::import_image;

use super::records::{
    Blip, CharStyle, EmuRect, LookRec, ParaStyle, PubData, RawPara, RawStory, ShapeRec, TableRec,
};
use super::report::ImportReport;

/// EMU per point.
const EMU_PT: f64 = 12_700.0;
/// Pages and frames beyond these limits are dropped and reported, so a hostile
/// file cannot make the import build an enormous document.
pub const MAX_PAGES: usize = super::records::MAX_PAGES_PARSED;
pub const MAX_FRAMES: usize = 20_000;
/// Master pages built from one file.
const MAX_MASTERS: usize = 100;
/// Chunk type of a group shape (records.rs `C_GROUP`).
const GROUP_CHUNK: u32 = 0x30;
/// Smallest and largest page dimension accepted, in points.
const MIN_PAGE: f64 = 4.0;
const MAX_PAGE: f64 = 14_400.0;
/// Default margin guides (Publisher's layout guides are not read yet).
const DEFAULT_MARGIN: f64 = 36.0;

fn pt(emu: i64) -> f64 {
    emu as f64 / EMU_PT
}

fn char_props(span: &CharStyle, dflt: &CharStyle, data: &PubData, colors: &Colors) -> CharProps {
    let font = span
        .font
        .or(dflt.font)
        .and_then(|i| data.fonts.get(i as usize))
        .filter(|n| !n.is_empty())
        .or_else(|| data.fonts.first().filter(|n| !n.is_empty()));
    let mut p = CharProps {
        font_family: font.cloned(),
        font_size: span
            .size_pt
            .or(dflt.size_pt)
            .filter(|s| *s > 0.0 && *s < 2000.0),
        ..Default::default()
    };
    // Publisher stores bold and italic as a toggle against the style default.
    if span.bold ^ dflt.bold {
        p.bold = Some(true);
    }
    if span.italic ^ dflt.italic {
        p.italic = Some(true);
    }
    if span.underline != 0 || dflt.underline != 0 {
        p.underline = Some(true);
    }
    p.color = span.color.or(dflt.color).and_then(|c| colors.paint(c));
    p
}

fn para_props(st: &ParaStyle, dflt: &ParaStyle, size_hint: f64) -> ParaProps {
    let pick = |a: Option<u32>, b: Option<u32>| a.or(b);
    let mut p = ParaProps {
        align: pick(st.align, dflt.align).map(|a| match a {
            2 => Align::Center,
            1 => Align::Right,
            6 => Align::Justify,
            _ => Align::Left,
        }),
        ..Default::default()
    };
    match st.line_spacing.or(dflt.line_spacing) {
        // "sp" spacing: 1 sp is single spacing, about 1.2 times the font size.
        Some((false, x)) if x > 0.0 && x < 20.0 => p.line_spacing = Some(x * 1.2),
        Some((true, points)) if points > 0.0 => {
            p.line_spacing = Some((points / size_hint.max(1.0)).clamp(0.5, 10.0));
        }
        _ => {}
    }
    let emu = |v: Option<u32>| v.map(|e| f64::from(e) / EMU_PT).filter(|x| *x < 2000.0);
    p.space_before = emu(pick(st.space_before, dflt.space_before)).filter(|x| *x > 0.0);
    p.space_after = emu(pick(st.space_after, dflt.space_after)).filter(|x| *x > 0.0);
    p.left_indent = emu(pick(st.left_indent, dflt.left_indent)).filter(|x| *x > 0.0);
    p.right_indent = emu(pick(st.right_indent, dflt.right_indent)).filter(|x| *x > 0.0);
    p.first_line_indent = st
        .first_indent
        .or(dflt.first_indent)
        .map(|e| f64::from(e) / EMU_PT)
        .filter(|x| x.abs() < 2000.0 && *x != 0.0);
    p
}

fn convert_para(raw: &RawPara, data: &PubData, colors: &Colors) -> Paragraph {
    let dflt_para = data.default_para.first().cloned().unwrap_or_default();
    let dchar_idx = raw
        .style
        .default_char
        .or(dflt_para.default_char)
        .unwrap_or(0);
    let dchar = data
        .default_char
        .get(dchar_idx as usize)
        .or_else(|| data.default_char.first())
        .cloned()
        .unwrap_or_default();
    let mut runs: Vec<Run> = Vec::new();
    let mut size = dchar.size_pt.unwrap_or(10.0);
    for (i, (text, st)) in raw.spans.iter().enumerate() {
        let props = char_props(st, &dchar, data, colors);
        if i == 0 {
            size = props.font_size.unwrap_or(size);
        }
        match runs.last_mut() {
            Some(last) if last.props == props => last.text.push_str(text),
            _ => runs.push(Run {
                text: text.clone(),
                style: None,
                props,
                field: None,
            }),
        }
    }
    if runs.is_empty() {
        runs.push(Run {
            text: String::new(),
            style: None,
            props: char_props(&CharStyle::default(), &dchar, data, colors),
            field: None,
        });
    }
    Paragraph {
        style: None,
        props: para_props(&raw.style, &dflt_para, size),
        runs,
    }
}

fn convert_story(raw: &RawStory, data: &PubData, colors: &Colors) -> Vec<Paragraph> {
    let mut paras: Vec<Paragraph> = raw
        .paras
        .iter()
        .map(|p| convert_para(p, data, colors))
        .collect();
    if paras.is_empty() {
        paras.push(Paragraph::default());
    }
    paras
}

fn frame_rect(s: &ShapeRec, size: Size) -> Rect {
    let clamp = |v: f64| v.clamp(-MAX_PAGE, MAX_PAGE);
    match s.rect {
        Some(EmuRect { xs, ys, xe, ye }) => {
            let x = size.width / 2.0 + pt(xs);
            let y = size.height / 2.0 + pt(ys);
            Rect::new(
                clamp(x),
                clamp(y),
                (pt(xe) - pt(xs)).clamp(1.0, MAX_PAGE),
                (pt(ye) - pt(ys)).clamp(1.0, MAX_PAGE),
            )
        }
        None => Rect::new(
            DEFAULT_MARGIN,
            DEFAULT_MARGIN,
            (size.width - 2.0 * DEFAULT_MARGIN).max(1.0),
            (size.height - 2.0 * DEFAULT_MARGIN).max(1.0),
        ),
    }
}

/// The swatches made from the file's colour scheme, by palette index.
struct Colors {
    swatches: Vec<SwatchId>,
}

impl Colors {
    /// A paint for a colour reference: type 0x08 is a palette (scheme) index,
    /// which becomes a swatch; anything else is a direct `0x00BBGGRR` colour.
    fn paint(&self, r: u32) -> Option<Paint> {
        if (r >> 24) as u8 == 0x08 {
            let id = self.swatches.get((r & 0xff_ffff) as usize)?;
            Some(Paint::swatch(*id))
        } else {
            Some(Paint::Color(Color::rgb(
                r as u8,
                (r >> 8) as u8,
                (r >> 16) as u8,
            )))
        }
    }
}

/// Create the scheme swatches. Palette entries 0 to 6 are, in order, Publisher's
/// Main, Accent 1 to 4, Hyperlink and Followed Hyperlink (the corpus' three
/// distinct schemes agree: black or navy text, then accents, then blue and
/// purple links); entry 7 is the white page background.
fn make_swatches(
    b: &mut DocBuilder<SeqIds>,
    palette: &[[u8; 3]],
    rep: &mut ImportReport,
) -> Result<Colors, pubrs_core::command::CommandError> {
    let mut swatches = Vec::new();
    for (i, c) in palette.iter().take(8).enumerate() {
        let (id, name) = match SchemeSlot::ALL.get(i) {
            Some(slot) => (slot.swatch_id(), slot.swatch_name().to_string()),
            None => (SwatchId::new(&mut b.ids), "Scheme: Background".to_string()),
        };
        b.exec(Command::AddSwatch {
            swatch: Swatch {
                id,
                name,
                value: SwatchValue::Rgb(Color::rgb(c[0], c[1], c[2])),
            },
        })?;
        swatches.push(id);
        rep.swatches += 1;
    }
    Ok(Colors { swatches })
}

/// Fill and outline of a shape as frame style.
fn apply_look(frame: &mut Frame, look: &LookRec, colors: &Colors) {
    frame.style.fill = look.fill_color().and_then(|c| colors.paint(c));
    frame.style.stroke = look.line_color().and_then(|c| {
        let width = look
            .line_width
            .map_or(0.75, |w| pt(w.clamp(0, 12_700 * 1000)).max(0.25));
        Some(Stroke {
            color: colors.paint(c)?,
            width,
            dash: Default::default(),
        })
    });
}

/// The blip behind a picture shape, as an asset (made once per blip).
enum Pic {
    Asset(AssetId),
    Dropped(&'static str, String),
}

/// The index of the entry that holds the picture (entries repeating an
/// earlier picture share its asset).
fn canonical_blip(data: &PubData, i: usize) -> usize {
    match data.blips.get(i) {
        Some(Blip::Same(first)) if *first < data.blips.len() => *first,
        _ => i,
    }
}

fn picture_asset(
    b: &mut DocBuilder<SeqIds>,
    data: &PubData,
    cache: &mut HashMap<usize, Pic>,
    pib: u32,
) -> Result<Option<AssetId>, pubrs_core::command::CommandError> {
    let idx = pib as usize;
    if idx == 0 || idx > data.blips.len() {
        return Ok(None);
    }
    let i = canonical_blip(data, idx - 1);
    if let std::collections::hash_map::Entry::Vacant(slot) = cache.entry(i) {
        let entry = match &data.blips[i] {
            Blip::Raster(bytes) => {
                match import_image(
                    &mut b.ids,
                    &format!("Publisher picture {idx}"),
                    bytes.clone(),
                ) {
                    Ok((asset, data)) => {
                        let id = asset.id;
                        b.exec(Command::AddAsset { asset, data })?;
                        Pic::Asset(id)
                    }
                    Err(e) => Pic::Dropped("picture", format!("picture {idx} cannot be read: {e}")),
                }
            }
            Blip::Unsupported(kind) => {
                let k = match *kind {
                    "WMF" => "picture-wmf",
                    "EMF" => "picture-emf",
                    _ => "picture",
                };
                Pic::Dropped(
                    k,
                    format!(
                        "{kind} pictures are not imported; an empty picture frame marks the place"
                    ),
                )
            }
            Blip::OverBudget => Pic::Dropped("picture", "blip byte budget exceeded".into()),
            Blip::Same(_) => Pic::Dropped("picture", format!("picture {idx} is unreadable")),
            Blip::Missing => {
                Pic::Dropped("picture", format!("picture {idx} is empty or unreadable"))
            }
        };
        slot.insert(entry);
    }
    Ok(match cache.get(&i) {
        Some(Pic::Asset(id)) => Some(*id),
        _ => None,
    })
}

/// A table shape's grid, when the cells and the story fit it exactly.
struct TableGrid {
    cols: Vec<f64>,
    rows: Vec<f64>,
}

fn table_grid(t: &TableRec, story: &RawStory) -> Result<TableGrid, String> {
    let (rows, cols) = (t.rows as usize, t.cols as usize);
    if rows == 0 || cols == 0 || rows > TABLE_MAX_ROWS || cols > TABLE_MAX_COLS {
        return Err(format!(
            "{rows} rows by {cols} columns is outside the table limits"
        ));
    }
    if t.col_ends.len() != cols || t.row_ends.len() != rows {
        return Err("the column and row positions do not match the grid".into());
    }
    if t.cells.len() != rows * cols {
        return Err(format!(
            "{} cells for a {rows} by {cols} grid",
            t.cells.len()
        ));
    }
    for (i, c) in t.cells.iter().enumerate() {
        let (r, k) = ((i / cols) as u32, (i % cols) as u32);
        if *c != [r, r, k, k] {
            return Err("merged or reordered cells".into());
        }
    }
    if story.paras.len() != rows * cols {
        return Err(format!(
            "{} paragraphs of text for {} cells",
            story.paras.len(),
            rows * cols
        ));
    }
    let diffs = |ends: &[i64]| -> Option<Vec<f64>> {
        let mut prev = 0i64;
        let mut out = Vec::new();
        for e in ends {
            if *e <= prev {
                return None;
            }
            out.push(pt(*e - prev).clamp(1.0, MAX_PAGE));
            prev = *e;
        }
        Some(out)
    };
    match (diffs(&t.col_ends), diffs(&t.row_ends)) {
        (Some(cols), Some(rows)) => {
            let (w, h): (f64, f64) = (cols.iter().sum(), rows.iter().sum());
            if w > pubrs_core::command::MAX_DIMENSION || h > pubrs_core::command::MAX_DIMENSION {
                return Err(format!("{w:.0} by {h:.0} pt is larger than a table may be"));
            }
            Ok(TableGrid { cols, rows })
        }
        _ => Err("column or row positions are not increasing".into()),
    }
}

/// Build the document.
pub fn build(data: &PubData, rep: &mut ImportReport) -> Result<Document, IoError> {
    let (w, h) = (pt(data.width_emu), pt(data.height_emu));
    if !(MIN_PAGE..=MAX_PAGE).contains(&w) || !(MIN_PAGE..=MAX_PAGE).contains(&h) {
        return Err(IoError::Format(format!(
            "pub: page size {w:.1} x {h:.1} pt is missing or out of range"
        )));
    }
    if data.pages.is_empty() {
        return Err(IoError::Format("pub: the file has no pages".into()));
    }
    let size = Size {
        width: w,
        height: h,
    };
    let setup = PageSetup {
        size,
        margins: Margins::uniform(DEFAULT_MARGIN.min(w / 4.0).min(h / 4.0)),
        bleed: 0.0,
        ..PageSetup::default()
    };
    let mut b = DocBuilder::new(SeqIds::new(0x5055_4252), setup);
    let corrupt = |e: pubrs_core::command::CommandError| {
        IoError::Format(format!("pub: could not build the document: {e}"))
    };

    // Colour scheme swatches.
    let colors = make_swatches(&mut b, &data.palette, rep).map_err(corrupt)?;

    // Pages.
    let mut page_ids: Vec<(u32, PageId)> = Vec::new();
    for (i, p) in data.pages.iter().enumerate() {
        if i >= MAX_PAGES {
            rep.drop_many("page", data.pages.len() - MAX_PAGES, "page limit reached");
            break;
        }
        let id = if i == 0 {
            b.page(0)
        } else {
            b.add_page().map_err(corrupt)?
        };
        page_ids.push((p.seq, id));
    }
    rep.pages = page_ids.len();

    // Master pages, and which page uses which.
    let mut parent_of: HashMap<u32, Parent> = page_ids
        .iter()
        .map(|(seq, id)| (*seq, Parent::Page(*id)))
        .collect();
    let mut master_ids: HashMap<u32, MasterId> = HashMap::new();
    for (n, m) in data.masters.iter().enumerate().take(MAX_MASTERS) {
        let id = MasterId::new(&mut b.ids);
        let name = if data.masters.len() == 1 {
            "Master".to_string()
        } else {
            format!("Master {}", n + 1)
        };
        b.exec(Command::AddMaster {
            master: MasterPage {
                id,
                name,
                frames: Vec::new(),
                guides: Vec::new(),
            },
        })
        .map_err(corrupt)?;
        master_ids.insert(m.seq, id);
        parent_of.insert(m.seq, Parent::Master(id));
        rep.masters += 1;
    }
    rep.drop_many(
        "master",
        data.masters.len().saturating_sub(MAX_MASTERS),
        "master page limit reached",
    );
    for (p, (_, page)) in data.pages.iter().zip(&page_ids) {
        if let Some(m) = p.master.and_then(|m| master_ids.get(&m)) {
            b.exec(Command::SetPageMaster {
                page: *page,
                master: Some(*m),
            })
            .map_err(corrupt)?;
        }
    }

    // Stories by text id.
    let stories: HashMap<u32, &RawStory> = data.stories.iter().map(|s| (s.id, s)).collect();
    let shape_by_seq: HashMap<u32, &ShapeRec> = data.shapes.iter().map(|s| (s.seq, s)).collect();

    // Shapes in page order, back to front (pages first, then masters).
    let mut ordered: Vec<&ShapeRec> = Vec::new();
    // Shapes bucketed by page once, so this stays linear in pages + shapes.
    let mut shapes_on: HashMap<u32, Vec<&ShapeRec>> = HashMap::new();
    for s in &data.shapes {
        if let Some(p) = s.page {
            shapes_on.entry(p).or_default().push(s);
        }
    }
    for p in data.pages.iter().chain(data.masters.iter()) {
        let mut seen = std::collections::HashSet::new();
        for seq in &p.shapes {
            if let Some(s) = shape_by_seq.get(seq)
                && s.page == Some(p.seq)
                && seen.insert(*seq)
            {
                ordered.push(s);
            }
        }
        for s in shapes_on.get(&p.seq).into_iter().flatten() {
            if seen.insert(s.seq) {
                ordered.push(s);
            }
        }
    }

    // Chains: shapes sharing a text id are one story. Flow order follows the
    // prev/next links when the file has them, else page order. Tables take
    // their text as cells, so they are not in a chain.
    let mut by_text: HashMap<u32, Vec<&ShapeRec>> = HashMap::new();
    for s in &ordered {
        if let (Some(t), Some(_)) = (s.text_id, s.page)
            && stories.contains_key(&t)
        {
            by_text.entry(t).or_default().push(s);
        }
    }
    let mut chains: HashMap<u32, Vec<u32>> = HashMap::new();
    for (t, list) in &by_text {
        chains.insert(*t, chain_order(list));
    }
    let mut frame_of: HashMap<u32, FrameId> = HashMap::new();
    let mut used_stories = std::collections::HashSet::new();
    let mut pics: HashMap<usize, Pic> = HashMap::new();
    let mut pic_drops: Vec<(&'static str, String)> = Vec::new();
    for s in ordered {
        let Some(parent) = s.page.and_then(|p| parent_of.get(&p)).copied() else {
            continue;
        };
        // Groups only hold other shapes, which are listed on their own.
        if s.shape_type == GROUP_CHUNK && s.text_id.is_none() && s.pib.is_none() {
            continue;
        }
        if rep.frames >= MAX_FRAMES {
            rep.drop_item("frame", "frame limit reached");
            continue;
        }
        let fid = FrameId::new(&mut b.ids);
        let rect = frame_rect(s, size);

        // Table.
        if let Some(t) = &s.table {
            let grid = match s.text_id.and_then(|t| stories.get(&t)) {
                Some(raw) => table_grid(t, raw).map(|g| (g, *raw)),
                None => Err("the table has no text".into()),
            };
            match grid {
                Ok((g, raw)) => {
                    add_table(&mut b, parent, fid, rect, &g, raw, data, &colors)
                        .map_err(corrupt)?;
                    if let Some(id) = s.text_id {
                        used_stories.insert(id);
                    }
                    rep.frames += 1;
                    rep.tables += 1;
                    continue;
                }
                // Fall through: the text still imports as a text frame.
                Err(why) => rep.drop_item("table", format!("kept as a text frame: {why}")),
            }
        }

        // Text frame.
        if let Some(text_id) = s.text_id {
            let Some(raw) = stories.get(&text_id) else {
                rep.drop_item(
                    "text-frame",
                    format!("no story {text_id} for shape {}", s.seq),
                );
                continue;
            };
            let sid = StoryId::new(&mut b.ids);
            let is_head = chains.get(&text_id).and_then(|c| c.first()) == Some(&s.seq);
            let mut tf = TextFrame::new(sid);
            if let Some(i) = s.insets {
                tf.insets = Margins {
                    left: pt(i[0]).clamp(0.0, 200.0),
                    top: pt(i[1]).clamp(0.0, 200.0),
                    right: pt(i[2]).clamp(0.0, 200.0),
                    bottom: pt(i[3]).clamp(0.0, 200.0),
                };
            } else {
                tf.insets = Margins::uniform(2.88);
            }
            if let Some(c) = s.columns.filter(|c| (1..=64).contains(c)) {
                tf.columns = c;
            }
            if let Some(g) = s.column_gap {
                tf.column_gap = pt(g).clamp(0.0, 200.0);
            }
            tf.vertical_align = match s.valign {
                1 => VerticalAlign::Middle,
                2 => VerticalAlign::Bottom,
                _ => VerticalAlign::Top,
            };
            let mut story = Story::new(sid, fid);
            if is_head {
                story.paragraphs = convert_story(raw, data, &colors);
                used_stories.insert(text_id);
            }
            let mut frame = Frame::new(fid, rect, FrameContent::Text(tf));
            frame.rotation = rotation(s);
            apply_look(&mut frame, &s.look, &colors);
            b.frame(parent, frame, Some(story)).map_err(corrupt)?;
            rep.frames += 1;
            frame_of.insert(s.seq, fid);
            continue;
        }

        // Picture.
        if let Some(pib) = s.pib {
            let asset = picture_asset(&mut b, data, &mut pics, pib).map_err(corrupt)?;
            let mut img = ImageFrame::new(asset);
            img.fit = ImageFit::Stretch;
            let mut frame = Frame::new(fid, rect, FrameContent::Image(img));
            frame.wrap = TextWrap::default();
            frame.rotation = rotation(s);
            b.frame(parent, frame, None).map_err(corrupt)?;
            rep.frames += 1;
            if asset.is_some() {
                rep.pictures += 1;
            } else if let Some(Pic::Dropped(kind, why)) =
                pics.get(&canonical_blip(data, (pib as usize).wrapping_sub(1)))
            {
                pic_drops.push((kind, why.clone()));
            } else {
                pic_drops.push((
                    "picture",
                    format!(
                        "picture {pib} is outside the picture store of {} pictures",
                        data.blips.len()
                    ),
                ));
            }
            continue;
        }

        // Plain shapes.
        let shape = match s.esc_type {
            1 => Some(Shape::Rectangle { corner_radius: 0.0 }),
            2 => Some(Shape::Rectangle {
                corner_radius: rect.width.min(rect.height) / 6.0,
            }),
            3 => Some(Shape::Ellipse),
            20 => Some(Shape::Line {
                direction: if s.flip_h ^ s.flip_v {
                    LineDirection::UpRight
                } else {
                    LineDirection::DownRight
                },
            }),
            _ => None,
        };
        match shape {
            Some(shape) => {
                if matches!(shape, Shape::Line { .. }) && s.look.line_color().is_none() {
                    rep.drop_item("shape", "a line with no visible outline");
                    continue;
                }
                let mut frame = Frame::new(fid, rect, FrameContent::Shape(ShapeFrame { shape }));
                frame.rotation = rotation(s);
                apply_look(&mut frame, &s.look, &colors);
                b.frame(parent, frame, None).map_err(corrupt)?;
                rep.frames += 1;
                rep.shapes += 1;
            }
            None if s.esc_type != 0 => {
                rep.drop_item("shape", format!("Escher shape type {}", s.esc_type));
            }
            None => rep.drop_item("shape", format!("shape record type 0x{:02x}", s.shape_type)),
        }
    }
    for (kind, why) in pic_drops {
        rep.drop_item(kind, why);
    }
    // Link each chain in flow order.
    let mut text_ids: Vec<u32> = chains.keys().copied().collect();
    text_ids.sort_unstable();
    for t in text_ids {
        let frames: Vec<FrameId> = chains[&t]
            .iter()
            .filter_map(|seq| frame_of.get(seq).copied())
            .collect();
        for pair in frames.windows(2) {
            b.link(pair[0], pair[1]).map_err(corrupt)?;
        }
    }
    let unused = data
        .stories
        .iter()
        .filter(|s| !used_stories.contains(&s.id) && s.paras.iter().any(|p| !p.spans.is_empty()))
        .count();
    rep.drop_many("story", unused, "text with no frame on a page");
    let doc = b.finish();
    // The families the document uses, as the file names them.
    let mut fonts = std::collections::BTreeSet::new();
    for st in doc.stories.values() {
        for p in &st.paragraphs {
            for r in &p.runs {
                if let Some(f) = &r.props.font_family {
                    fonts.insert(f.clone());
                }
            }
        }
    }
    rep.fonts = fonts.into_iter().collect();
    Ok(doc)
}

fn rotation(s: &ShapeRec) -> f64 {
    if s.rotation.is_finite() {
        s.rotation.rem_euclid(360.0)
    } else {
        0.0
    }
}

/// Add a table frame whose cells take the story's paragraphs in row-major order.
#[allow(clippy::too_many_arguments)]
fn add_table(
    b: &mut DocBuilder<SeqIds>,
    parent: Parent,
    fid: FrameId,
    rect: Rect,
    g: &TableGrid,
    raw: &RawStory,
    data: &PubData,
    colors: &Colors,
) -> Result<(), pubrs_core::command::CommandError> {
    let ids: Vec<StoryId> = (0..g.cols.len() * g.rows.len())
        .map(|_| StoryId::new(&mut b.ids))
        .collect();
    let style = CellStyle {
        insets: Margins::uniform(2.88),
        ..CellStyle::default()
    };
    let table = TableFrame::grid(g.cols.clone(), g.rows.clone(), &ids, &style)
        .map_err(pubrs_core::command::CommandError::invalid)?;
    let mut rect = rect;
    rect.width = table.width();
    rect.height = g.rows.iter().sum();
    let frame = Frame::new(fid, rect, FrameContent::Table(Box::new(table)));
    let stories = ids.iter().map(|s| Story::new(*s, fid)).collect();
    b.exec(Command::AddTable {
        parent,
        index: None,
        frame,
        stories,
    })?;
    for (sid, para) in ids.iter().zip(&raw.paras) {
        b.exec(Command::SetStory {
            story: *sid,
            paragraphs: vec![convert_para(para, data, colors)],
        })?;
    }
    Ok(())
}

/// The shapes of one story in flow order: start at a shape with no (listed)
/// predecessor and follow `next`; anything left over keeps page order.
fn chain_order(list: &[&ShapeRec]) -> Vec<u32> {
    let seqs: std::collections::HashSet<u32> = list.iter().map(|s| s.seq).collect();
    let next: HashMap<u32, u32> = list
        .iter()
        .filter_map(|s| s.next.filter(|n| seqs.contains(n)).map(|n| (s.seq, n)))
        .collect();
    let has_prev: std::collections::HashSet<u32> = next.values().copied().collect();
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for s in list {
        if has_prev.contains(&s.seq) {
            continue;
        }
        let mut cur = Some(s.seq);
        while let Some(c) = cur {
            if !seen.insert(c) {
                break;
            }
            out.push(c);
            cur = next.get(&c).copied();
        }
    }
    for s in list {
        if seen.insert(s.seq) {
            out.push(s.seq);
        }
    }
    out
}

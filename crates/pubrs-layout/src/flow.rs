//! Story flow: paragraphs are shaped with parley one paragraph at a time
//! and broken line by line into the chain's regions (frames × columns),
//! honouring text wrap around other frames. Text that does not fit is
//! reported as overflow.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use parley::{
    Affinity, Alignment, AlignmentOptions, BreakReason, Cluster, Cursor, FontFamily, FontFeatures,
    FontStyle, FontWeight, Layout, LayoutContext, LineHeight, PositionedLayoutItem, StyleProperty,
    VerticalAlign as ScriptAlign, YieldData,
};
use pubrs_core::style::{
    Baseline, Caps, DEFAULT_TAB_INTERVAL, DropCap, ListKind, MAX_LIST_LEVEL, TabAlign, TabLeader,
    TabStop,
};
use pubrs_core::text::{BreakKind, is_break};
use pubrs_core::{
    Align, CharAttrs, Document, Frame, FrameContent, FrameId, Ink, LineDirection, Owner, PageId,
    ParaAttrs, Paragraph, Parent, Point, Rect, Run, Shape, StoryId, VerticalAlign, WrapMode,
};

use crate::fonts::{FaceTraits, FontLibrary};
use crate::hyphen::{self, BreakParams, LineCx, LineInfo, Shadow};
use crate::types::{
    Decoration, DocumentLayout, FontHandle, FrameText, GlyphRun, LineBox, Overflow,
    PositionedGlyph, StoryLayout,
};
use crate::wrap::{self, AlphaContours, Outline, band_intervals};

/// Shapes and flows text. Holds the font library and parley's caches.
pub struct Layouter {
    pub fonts: FontLibrary,
    lcx: LayoutContext<Ink>,
    /// Per (family, bold, italic): OS/2 script metrics and `smcp` support.
    traits: HashMap<(String, bool, bool), FaceTraits>,
    /// Where wrap finds the visible outline of placed images.
    pub(crate) contours: Option<Arc<dyn AlphaContours>>,
}

impl std::fmt::Debug for Layouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Layouter")
            .field("fonts", &self.fonts)
            .finish()
    }
}

/// A frame text wraps around, in the text frame's local space.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Obstacle {
    /// Bounding box, already outset by the margin. All `square` wrap uses.
    rect: Rect,
    mode: WrapMode,
    /// Text fills every slot of a line beside the frame (> 3 em).
    both_sides: bool,
    /// The outline `tight` and `through` follow, with its margin. `None`
    /// follows the bounding box (frames without an outline of their own).
    outline: Option<(Outline, f64)>,
}

/// A rectangle text can flow into: one column of one frame (frame-local).
/// With the story and the styles, the regions are all a story's flow reads
/// from the document, so they are part of the cache key
/// ([`crate::cache::StoryKey`]).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Region {
    pub(crate) frame: FrameId,
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) w: f64,
    pub(crate) h: f64,
    pub(crate) valign: VerticalAlign,
    pub(crate) obstacles: Vec<Obstacle>,
    /// Rank of the frame's page among the pages of the chain, in document
    /// order (`None` on a master). Relative, so adding pages elsewhere
    /// leaves it alone; page breaks compare it.
    pub(crate) page: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
enum Avail {
    /// Usable x and width for the line.
    Span(f64, f64),
    /// Several slots, left to right, each wider than the minimum: a line
    /// is broken into each in turn (`both_sides` or `through` wrap).
    Slots(Vec<(f64, f64)>),
    /// Nothing fits in this band; resume at this y.
    Skip(f64),
}

impl Region {
    fn bottom(&self) -> f64 {
        self.y + self.h
    }

    /// Free horizontal space for a line occupying `y0..y1`.
    fn available(&self, y0: f64, y1: f64, min_w: f64) -> Avail {
        let (left, right) = (self.x, self.x + self.w);
        let mut blocked: Vec<(f64, f64)> = Vec::new();
        let mut skip_to: Option<f64> = None;
        let mut multi = false;
        for o in &self.obstacles {
            let r = o.rect;
            if r.top() >= y1 || r.bottom() <= y0 || r.right() <= left || r.left() >= right {
                continue;
            }
            match o.mode {
                WrapMode::TopBottom => {
                    skip_to = Some(skip_to.map_or(r.bottom(), |s: f64| s.max(r.bottom())));
                }
                WrapMode::Square => {
                    blocked.push((r.left(), r.right()));
                    multi |= o.both_sides;
                }
                WrapMode::Tight | WrapMode::Through => {
                    multi |= o.both_sides || o.mode == WrapMode::Through;
                    match &o.outline {
                        None => blocked.push((r.left(), r.right())),
                        Some((outline, margin)) => {
                            let iv = band_intervals(&outline.0, y0, y1, *margin);
                            if o.mode == WrapMode::Tight {
                                if let (Some(a), Some(b)) = (iv.first(), iv.last()) {
                                    blocked.push((a.0, b.1));
                                }
                            } else {
                                blocked.extend(iv);
                            }
                        }
                    }
                }
                WrapMode::None => {}
            }
        }
        if let Some(y) = skip_to {
            return Avail::Skip(y);
        }
        if blocked.is_empty() {
            return Avail::Span(left, self.w);
        }
        blocked.sort_by(|a, b| a.0.total_cmp(&b.0));
        if multi {
            let mut slots: Vec<(f64, f64)> = Vec::new();
            let mut x = left;
            for (b0, b1) in &blocked {
                if *b0 > x && b0 - x >= min_w {
                    slots.push((x, b0 - x));
                }
                x = x.max(*b1);
            }
            if right > x && right - x >= min_w {
                slots.push((x, right - x));
            }
            return match slots.len() {
                0 => Avail::Skip(self.skip_target(y0, y1)),
                1 => Avail::Span(slots[0].0, slots[0].1),
                _ => Avail::Slots(slots),
            };
        }
        let mut best = (left, 0.0);
        let mut x = left;
        for (b0, b1) in &blocked {
            if *b0 > x && b0 - x > best.1 {
                best = (x, b0 - x);
            }
            x = x.max(*b1);
        }
        if right > x && right - x > best.1 {
            best = (x, right - x);
        }
        if best.1 >= min_w {
            Avail::Span(best.0, best.1)
        } else {
            Avail::Skip(self.skip_target(y0, y1))
        }
    }

    /// Where to resume when nothing fits in `y0..y1`: below the lowest
    /// obstacle in the band, or a little lower for outlines, whose gaps can
    /// open again before their bounding box ends.
    fn skip_target(&self, y0: f64, y1: f64) -> f64 {
        self.obstacles
            .iter()
            .filter(|o| o.rect.top() < y1 && o.rect.bottom() > y0)
            .map(|o| {
                if o.outline.is_some() {
                    o.rect.bottom().min(y0 + 1.0)
                } else {
                    o.rect.bottom()
                }
            })
            .fold(f64::INFINITY, f64::min)
    }
}

/// Everything a row of slots changes while it is broken, so a row can be
/// taken back as a whole.
struct RowCheckpoint {
    state: parley::layout::BreakerState,
    pos: Option<usize>,
    placed: usize,
    infos: usize,
    first_line: bool,
    cap_at: Option<(usize, f64)>,
    first_x: f64,
    explicit: usize,
}

impl RowCheckpoint {
    #[allow(clippy::too_many_arguments)]
    fn restore(
        self,
        bl: &mut parley::layout::BreakLines<'_, Ink>,
        lcx: Option<&mut LineCx>,
        placements: &mut Vec<(usize, f64, f64)>,
        line_infos: &mut Vec<LineInfo>,
        first_line: &mut bool,
        cap_at: &mut Option<(usize, f64)>,
        first_x: &mut f64,
        explicit: &mut usize,
    ) {
        bl.revert_to(self.state);
        if let (Some(c), Some(p)) = (lcx, self.pos) {
            c.pos = p;
            c.pending = None;
        }
        placements.truncate(self.placed);
        line_infos.truncate(self.infos);
        *first_line = self.first_line;
        *cap_at = self.cap_at;
        *first_x = self.first_x;
        *explicit = self.explicit;
    }
}

/// How a row of slots ended.
enum RowEnd {
    /// Every slot got a line; go on to the next row.
    Next,
    /// The text ended in this row.
    Done,
    /// A line is taller than the band measured for it and the free space
    /// differs for the taller band: lay the row out again with this height.
    Taller(f64),
    /// A line would not fit under the region's bottom.
    NextRegion,
    /// The breaker yielded something that is not a line.
    Again,
    /// A line ended at a frame or column break.
    Jump(BreakKind),
}

/// Position of the flow: region index and frame-local y.
#[derive(Debug, Clone, Copy)]
struct FlowCursor {
    region: usize,
    y: f64,
}

/// A laid-out line waiting to be assigned to its frame.
/// Consecutive clusters of one style: (source byte range, glyph count) each.
type ClusterGroup = Vec<(Range<usize>, usize)>;

struct PlacedLine {
    region: usize,
    line: LineBox,
}

/// The flow state before a paragraph, to rewind to it (keeps).
#[derive(Debug, Clone, Copy)]
struct ParaMark {
    cur: FlowCursor,
    /// `placed.len()` before the paragraph.
    placed: usize,
    /// Story offset of the paragraph's first character.
    start: usize,
    keep_with_next: bool,
}

/// One paragraph to flow, with what stays fixed across its passes.
#[derive(Clone, Copy)]
struct ParaJob<'a> {
    doc: &'a Document,
    para: &'a Paragraph,
    pa: &'a ParaAttrs,
    /// Story offset of the paragraph's first character.
    start: usize,
    regions: &'a [Region],
    /// Apply keeps (off for the fallback layout without them).
    keeps: bool,
    /// The list label of the paragraph (generated text).
    label: Option<&'a str>,
}

/// A drop cap shaped at its size: glyph runs with x from the cap's left
/// edge and y from its baseline.
struct Cap {
    runs: Vec<GlyphRun>,
    width: f64,
    /// x of each character boundary of the cap (`chars + 1` entries).
    carets: Vec<f64>,
    /// Lines the cap spans.
    lines: usize,
    /// Body line height the cap was sized for.
    pitch: f64,
    /// Space between the cap and the text beside it.
    gap: f64,
}

/// Character counts at byte positions of one paragraph's text, counted
/// forward from the previous query. Lines are visited in order, so a
/// paragraph costs one pass instead of one scan from its start per line.
struct CharCounter<'t> {
    text: &'t str,
    byte: usize,
    chars: usize,
}

impl<'t> CharCounter<'t> {
    fn new(text: &'t str) -> Self {
        Self {
            text,
            byte: 0,
            chars: 0,
        }
    }

    /// Characters before `byte` (clamped to the text length).
    fn at(&mut self, byte: usize) -> usize {
        let b = byte.min(self.text.len());
        if b < self.byte {
            self.byte = 0;
            self.chars = 0;
        }
        self.chars += self.text[self.byte..b].chars().count();
        self.byte = b;
        self.chars
    }
}

/// Lookups the flow needs that are linear on [`Document`] (the owner of a
/// frame, the paint order of a page), built once per layout pass in
/// O(frames) and answering exactly as [`Document::owner_of`] and
/// [`Document::paint_order`] do.
pub(crate) struct DocIndex {
    /// First parent of each frame, in `Document::parent_of` precedence:
    /// pages in order, then masters, then groups in id order.
    parent: HashMap<FrameId, Parent>,
    /// Paint order per page or master, filled on first use.
    orders: HashMap<Owner, Vec<FrameId>>,
    /// Index of each page in the document.
    page_index: HashMap<PageId, usize>,
    pub(crate) contours: Option<Arc<dyn AlphaContours>>,
    /// Cell index of each cell story, per table frame, built on first use
    /// (a pass over a table costs its cells once, not once per story).
    cells: HashMap<FrameId, HashMap<StoryId, usize>>,
}

impl DocIndex {
    /// The cell of `table` (frame `frame`) showing `story`.
    fn cell_index(
        &mut self,
        frame: FrameId,
        table: &pubrs_core::TableFrame,
        story: StoryId,
    ) -> Option<usize> {
        self.cells
            .entry(frame)
            .or_insert_with(|| {
                table
                    .cells
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (c.story, i))
                    .collect()
            })
            .get(&story)
            .copied()
    }

    pub(crate) fn new(doc: &Document, contours: Option<Arc<dyn AlphaContours>>) -> Self {
        let mut parent = HashMap::with_capacity(doc.frames.len());
        let mut page_index = HashMap::with_capacity(doc.pages.len());
        for (i, p) in doc.pages.iter().enumerate() {
            page_index.insert(p.id, i);
            for f in &p.frames {
                parent.entry(*f).or_insert(Parent::Page(p.id));
            }
        }
        for m in doc.masters.iter() {
            for f in &m.frames {
                parent.entry(*f).or_insert(Parent::Master(m.id));
            }
        }
        for g in doc.frames.values() {
            if let FrameContent::Group(group) = &g.content {
                for c in &group.children {
                    parent.entry(*c).or_insert(Parent::Group(g.id));
                }
            }
        }
        Self {
            parent,
            orders: HashMap::new(),
            page_index,
            contours,
            cells: HashMap::new(),
        }
    }

    /// Same answer as [`Document::owner_of`].
    fn owner_of(&self, frame: FrameId) -> Option<Owner> {
        let mut current = frame;
        for _ in 0..64 {
            match *self.parent.get(&current)? {
                Parent::Page(p) => return Some(Owner::Page(p)),
                Parent::Master(m) => return Some(Owner::Master(m)),
                Parent::Group(g) => current = g,
            }
        }
        None
    }

    /// Same answer as [`Document::paint_order`].
    fn paint_order(&mut self, doc: &Document, owner: Owner) -> &[FrameId] {
        self.orders
            .entry(owner)
            .or_insert_with(|| doc.paint_order(owner))
    }
}

fn to_parley_align(a: Align) -> Alignment {
    match a {
        Align::Left => Alignment::Left,
        Align::Center => Alignment::Center,
        Align::Right => Alignment::Right,
        Align::Justify => Alignment::Justify,
    }
}

impl Layouter {
    pub fn new(fonts: FontLibrary) -> Self {
        Self {
            fonts,
            lcx: LayoutContext::new(),
            traits: HashMap::new(),
            contours: None,
        }
    }

    /// Set where `tight` and `through` wrap find the visible outline of
    /// placed images. Without one, images wrap by their frame rectangle.
    pub fn set_contours(&mut self, contours: Option<Arc<dyn AlphaContours>>) {
        self.contours = contours;
    }

    /// Lay out every story from scratch. Interactive callers use
    /// [`crate::cache::StoryLayoutCache`], which gives the same result and
    /// recomposes only the stories whose inputs changed.
    pub fn layout_document(&mut self, doc: &Document) -> DocumentLayout {
        let mut index = DocIndex::new(doc, self.contours.clone());
        let mut out = DocumentLayout::default();
        for (id, story) in &doc.stories {
            let regions = regions_for(doc, &mut index, *id, &story.frames);
            out.stories
                .insert(*id, Arc::new(self.flow_story(doc, *id, &regions)));
        }
        for (id, f) in &doc.frames {
            if let Some(w) = f.as_wordart() {
                out.wordart.insert(*id, Arc::new(self.layout_wordart(w)));
            }
            if f.as_table().is_some() {
                let (layout, adjusted) = crate::table::table_pass(doc, f, &out.stories);
                for (s, l) in adjusted {
                    out.stories.insert(s, l);
                }
                out.tables.insert(*id, Arc::new(layout));
            }
        }
        out
    }

    /// Shape a WordArt frame's text on one line at its size, with the
    /// baseline at y = 0 and the first pen position at x = 0 (ADR 0020).
    /// The warp and the fit to the frame are applied at render time.
    pub fn layout_wordart(
        &mut self,
        art: &pubrs_core::WordArtFrame,
    ) -> crate::types::WordArtLayout {
        let text: String = art
            .text
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        let mut out = crate::types::WordArtLayout::default();
        if text.trim().is_empty() {
            return out;
        }
        if !self.fonts.ensure_family(&art.font) {
            out.missing_font = Some(art.font.clone());
        }
        let attrs = CharAttrs {
            font_family: art.font.clone(),
            font_size: art.size,
            ..CharAttrs::default()
        };
        let text_arc: Arc<str> = Arc::from(text.as_str());
        for (font, run) in self.shape_one_line(&text, &attrs, art.size) {
            let mut glyphs = Vec::with_capacity(run.glyphs.len());
            for (id, x, y, advance, range) in run.glyphs {
                out.width = out.width.max(x + advance);
                glyphs.push(PositionedGlyph {
                    id,
                    x,
                    y,
                    advance,
                    text: range,
                });
            }
            out.runs.push(GlyphRun {
                font,
                font_size: run.size,
                color: run.color,
                synthetic_bold: run.bold,
                synthetic_skew: run.skew,
                text: text_arc.clone(),
                glyphs,
            });
        }
        out
    }

    /// Lay out one story across its frame chain.
    pub fn layout_story(&mut self, doc: &Document, story_id: StoryId) -> StoryLayout {
        let regions = match doc.story(story_id) {
            Some(story) => regions_for(
                doc,
                &mut DocIndex::new(doc, self.contours.clone()),
                story_id,
                &story.frames,
            ),
            None => Vec::new(),
        };
        self.flow_story(doc, story_id, &regions)
    }

    /// Flow one story into `regions` (from [`regions_for`] over its chain).
    ///
    /// Keeps never cost visible text (ADR 0015): if paragraphs were moved
    /// for keeps and the story then overflows, it is flowed again without
    /// keeps, and that layout is used when it shows more of the story.
    pub(crate) fn flow_story(
        &mut self,
        doc: &Document,
        story_id: StoryId,
        regions: &[Region],
    ) -> StoryLayout {
        let (out, moved) = self.flow_story_keeps(doc, story_id, regions, true);
        match out.overflow {
            Some(o) if moved => {
                let (plain, _) = self.flow_story_keeps(doc, story_id, regions, false);
                if plain.overflow.is_none_or(|p| p.offset > o.offset) {
                    plain
                } else {
                    out
                }
            }
            _ => out,
        }
    }

    /// Flow one story, with or without keeps; also says whether keeps moved
    /// any paragraph.
    fn flow_story_keeps(
        &mut self,
        doc: &Document,
        story_id: StoryId,
        regions: &[Region],
        keeps: bool,
    ) -> (StoryLayout, bool) {
        let mut out = StoryLayout {
            story: Some(story_id),
            ..Default::default()
        };
        let Some(story) = doc.story(story_id) else {
            return (out, false);
        };
        for f in &story.frames {
            if doc.frame(*f).is_some_and(shows_story_text) {
                out.frames.insert(*f, FrameText::default());
            }
        }
        let mut placed: Vec<PlacedLine> = Vec::new();
        let mut cur = FlowCursor {
            region: 0,
            y: regions.first().map_or(0.0, |r| r.y),
        };
        let mut para_start = 0usize;
        // Keeps rewind to the state before a paragraph (ADR 0015): `marks`
        // holds it for every paragraph flowed so far, `moved` the
        // paragraphs already moved for keep with next (each moves once), and
        // `push` the paragraph to flow again from the top of a region.
        let paras = &story.paragraphs;
        let mut marks: Vec<ParaMark> = Vec::with_capacity(paras.len());
        let mut moved: Vec<usize> = Vec::new();
        let mut push: Option<(usize, usize)> = None;
        // After a keep-with-next move: the paragraphs flowed again (first,
        // last). If one of them overflows, the move is undone.
        let mut reflow: Option<(usize, usize)> = None;
        let mut any_moved = false;
        // List labels depend only on the paragraphs before, never on where
        // they fall, so they are numbered once here and a rewind needs no
        // counters (ADR 0019).
        let pas: Vec<ParaAttrs> = paras.iter().map(|p| doc.styles.resolve_para(p)).collect();
        let labels = list_labels(&pas);
        let mut pi = 0usize;
        while let Some(para) = paras.get(pi) {
            let pa = pas[pi].clone();
            marks.truncate(pi);
            marks.push(ParaMark {
                cur,
                placed: placed.len(),
                start: para_start,
                keep_with_next: pa.keep_with_next,
            });
            let start_at = push.take().filter(|(p, _)| *p == pi).map(|(_, r)| r);
            let overflow = if cur.region >= regions.len() {
                Some(para_start)
            } else {
                let job = ParaJob {
                    doc,
                    para,
                    pa: &pa,
                    start: para_start,
                    regions,
                    keeps,
                    label: labels.get(pi).and_then(|l| l.as_deref()),
                };
                let (of, m) = self.flow_kept(job, &mut cur, &mut placed, &mut out, start_at);
                any_moved |= m;
                of
            };
            if let Some(offset) = overflow {
                if let Some((j, _)) = reflow.take().filter(|(j, i)| (*j..=*i).contains(&pi)) {
                    // The move pushed a kept paragraph out of the chain:
                    // flow them where they were (`moved` holds `j`, so it is
                    // not moved again).
                    let m = marks[j];
                    cur = m.cur;
                    placed.truncate(m.placed);
                    para_start = m.start;
                    pi = j;
                    continue;
                }
                out.overflow = Some(Overflow {
                    offset,
                    hidden_chars: story.char_len().saturating_sub(offset),
                });
                break;
            }
            if keeps
                && let Some((j, region)) = keep_with_next_move(&marks, &placed, regions, &moved)
            {
                moved.push(j);
                any_moved = true;
                let m = marks[j];
                cur = m.cur;
                placed.truncate(m.placed);
                para_start = m.start;
                push = Some((j, region));
                reflow = Some((j, pi));
                pi = j;
                continue;
            }
            if reflow.is_some_and(|(_, i)| pi >= i) {
                reflow = None;
            }
            para_start += para.char_len() + 1;
            pi += 1;
        }
        apply_vertical_align(regions, &mut placed);
        for p in placed {
            let frame = regions[p.region].frame;
            for r in &p.line.runs {
                for g in r.glyphs.iter().filter(|g| g.id == 0) {
                    out.missing_glyphs += 1;
                    if let Some(s) = r.text.get(g.text.clone()) {
                        out.missing_chars
                            .extend(s.chars().filter(|c| !c.is_control()));
                    }
                }
            }
            if let Some(ft) = out.frames.get_mut(&frame) {
                if ft.lines.is_empty() {
                    ft.chars = p.line.chars.clone();
                } else {
                    ft.chars.end = p.line.chars.end;
                }
                ft.lines.push(p.line);
            }
        }
        (out, any_moved)
    }

    /// Flow one paragraph and apply its own keeps (keep together, widows
    /// and orphans). When one breaks at a region boundary, the paragraph is
    /// rewound and flowed again: from the top of the next region, or with a
    /// region break moved up so the last region gets enough lines. At most
    /// two such passes. A pass that overflows (the first did not), or a
    /// keep-together move that still splits the paragraph, is undone: the
    /// last good pass stands, so keeps never hide text the paragraph showed.
    /// `start_at` starts the paragraph at the top of that region (keep with
    /// next moved it there). Also returns whether the paragraph was moved.
    fn flow_kept(
        &mut self,
        job: ParaJob<'_>,
        cur: &mut FlowCursor,
        placed: &mut Vec<PlacedLine>,
        out: &mut StoryLayout,
        start_at: Option<usize>,
    ) -> (Option<usize>, bool) {
        let from = (*cur, placed.len());
        let (pa, regions) = (job.pa, job.regions);
        let n = usize::from(pa.widow_orphan);
        // Break characters place lines on purpose: no keeps across them.
        let rules = job.keeps
            && (pa.keep_together || n >= 2)
            && !job.para.runs.iter().any(|r| r.text.contains(is_break));
        let of = self.flow_at(job, from, start_at, None, cur, placed, out);
        if !rules || of.is_some() {
            return (of, false);
        }
        // The last good pass: (start region, forced region break).
        let mut good: (Option<usize>, Option<usize>) = (start_at, None);
        for _ in 0..2 {
            let (push, forced) = good;
            let lines = &placed[from.1..];
            let Some(first) = lines.first() else {
                break;
            };
            let r0 = first.region;
            let n_first = lines.iter().take_while(|l| l.region == r0).count();
            if n_first == lines.len() {
                break;
            }
            // Moving the paragraph helps only if it does not already start
            // at the top of its region; it moves one region, once. Keeping
            // it together needs it to fit in the next region.
            let can_push = push.is_none()
                && r0 + 1 < regions.len()
                && first.line.rect.y > regions[r0].y + 1e-6;
            let height: f64 = lines.iter().map(|l| l.line.rect.height).sum();
            let together = can_push
                && pa.keep_together
                && regions.get(r0 + 1).is_some_and(|r| height <= r.h + 0.01);
            let next = if together || (can_push && n_first < n) {
                (Some(r0 + 1), forced)
            } else {
                let total = lines.len();
                let rl = lines[total - 1].region;
                let n_last = lines.iter().rev().take_while(|l| l.region == rl).count();
                if n_last >= n || forced.is_some() {
                    break;
                }
                // Start the last region `n` lines from the end, if the
                // region before keeps enough lines (all of them may go when
                // it is a middle region of a long paragraph).
                let s_last = total - n_last;
                let r_prev = lines[s_last - 1].region;
                let s_prev = lines[..s_last]
                    .iter()
                    .rposition(|l| l.region != r_prev)
                    .map_or(0, |i| i + 1);
                let k = total.saturating_sub(n);
                if k > s_prev && (s_prev > 0 || k - s_prev >= n) {
                    (push, Some(k))
                } else if s_prev == 0 && can_push {
                    (Some(r0 + 1), None)
                } else {
                    break;
                }
            };
            let of = self.flow_at(job, from, next.0, next.1, cur, placed, out);
            let lines = &placed[from.1..];
            let split = lines
                .first()
                .is_some_and(|f| lines.iter().any(|l| l.region != f.region));
            if of.is_some() || (together && split) {
                let of = self.flow_at(job, from, good.0, good.1, cur, placed, out);
                return (of, good != (start_at, None));
            }
            good = next;
        }
        (None, good != (start_at, None))
    }

    /// Rewind to `from` (cursor, placed lines) and flow the paragraph,
    /// starting at the top of region `push` if given and later, with a
    /// region break before line `forced` if given.
    #[allow(clippy::too_many_arguments)]
    fn flow_at(
        &mut self,
        job: ParaJob<'_>,
        from: (FlowCursor, usize),
        push: Option<usize>,
        forced: Option<usize>,
        cur: &mut FlowCursor,
        placed: &mut Vec<PlacedLine>,
        out: &mut StoryLayout,
    ) -> Option<usize> {
        *cur = from.0;
        placed.truncate(from.1);
        if let Some(t) = push.filter(|t| *t < job.regions.len() && cur.region < *t) {
            *cur = FlowCursor {
                region: t,
                y: job.regions[t].y,
            };
        }
        self.flow_paragraph(
            job.doc,
            job.para,
            job.pa,
            job.start,
            job.regions,
            job.label,
            cur,
            placed,
            out,
            forced,
        )
    }

    /// Shape a drop cap: the paragraph's first characters `orig[..cap_b]`
    /// in the style of `attrs`, sized so its cap height reaches from the
    /// first line's cap height to the baseline of line `lines` of body text
    /// in `body`.
    fn shape_cap(
        &mut self,
        orig: &Arc<str>,
        cap_b: usize,
        attrs: &CharAttrs,
        body: &CharAttrs,
        line_spacing: f64,
        dc: DropCap,
    ) -> Option<Cap> {
        let text = orig.get(..cap_b)?;
        let lines = usize::from(dc.lines.max(1));
        let pitch = (body.font_size * line_spacing).max(1.0);
        // Cap height as a fraction of the font size, from the cap's font.
        let probe = self.shape_one_line(text, attrs, attrs.font_size);
        let ratio = probe
            .first()
            .and_then(|(f, _)| cap_height_ratio(f))
            .unwrap_or(0.7);
        let size = body.font_size + (lines - 1) as f64 * pitch / ratio;
        let shaped = self.shape_one_line(text, attrs, size);
        let mut runs = Vec::new();
        let mut boundaries: Vec<(usize, f64)> = Vec::new();
        let mut width = 0.0f64;
        for (font, run) in shaped {
            let mut glyphs = Vec::new();
            for (id, x, y, advance, range) in run.glyphs {
                if !range.is_empty() {
                    boundaries.push((range.start, x));
                }
                glyphs.push(PositionedGlyph {
                    id,
                    x,
                    y,
                    advance,
                    text: range,
                });
                width = width.max(x + advance);
            }
            runs.push(GlyphRun {
                font,
                font_size: run.size,
                color: run.color,
                synthetic_bold: run.bold,
                synthetic_skew: run.skew,
                text: orig.clone(),
                glyphs,
            });
        }
        boundaries.sort_by_key(|b| b.0);
        let mut carets = Vec::new();
        for (b, _) in text.char_indices() {
            let x = boundaries
                .iter()
                .take_while(|c| c.0 <= b)
                .last()
                .map_or(0.0, |c| c.1);
            carets.push(x);
        }
        carets.push(width);
        Some(Cap {
            runs,
            width,
            carets,
            lines,
            pitch,
            gap: body.font_size * 0.25,
        })
    }

    /// OS/2 script metrics and `smcp` support of the face parley picks for
    /// `a`'s family, weight and style (found by shaping one letter).
    fn face_traits(&mut self, a: &CharAttrs) -> FaceTraits {
        let key = (a.font_family.to_lowercase(), a.bold, a.italic);
        if let Some(t) = self.traits.get(&key) {
            return *t;
        }
        let probe = CharAttrs {
            font_size: 16.0,
            ..a.clone()
        };
        let mut buf = [0u8; 4];
        let shaped = self.shape_one_line('a'.encode_utf8(&mut buf), &probe, 16.0);
        let t = shaped.first().map_or_else(FaceTraits::default, |(f, _)| {
            FaceTraits::from_font(f.bytes(), f.index)
        });
        self.traits.insert(key, t);
        t
    }

    /// What `a` needs from the shaper beyond family, weight and style: the
    /// size after super/subscript, the baseline shift, font features and the
    /// case mapping. Plain attributes cost nothing.
    fn derive(&mut self, a: &CharAttrs) -> Derived {
        let mut d = Derived {
            size: a.font_size,
            shift: 0.0,
            features: String::new(),
            caps: CapsMode::Off,
            strike: a.strikethrough,
        };
        let mut feats: Vec<&str> = Vec::new();
        if !a.ligatures {
            feats.push("\"liga\" 0, \"clig\" 0");
        }
        if !a.kerning {
            feats.push("\"kern\" 0");
        }
        let scripted = matches!(a.baseline, Baseline::Superscript | Baseline::Subscript);
        let traits = (scripted || a.caps == Caps::Small).then(|| self.face_traits(a));
        match (a.baseline, traits) {
            (Baseline::Superscript, Some(t)) => {
                d.size = a.font_size * t.sup.scale;
                d.shift = (a.font_size * t.sup.shift) as f32;
            }
            (Baseline::Subscript, Some(t)) => {
                d.size = a.font_size * t.sub.scale;
                d.shift = (a.font_size * t.sub.shift) as f32;
            }
            (Baseline::Shift(v), _) => d.shift = v as f32,
            _ => {}
        }
        match a.caps {
            Caps::None => {}
            Caps::All => d.caps = CapsMode::Upper,
            Caps::Small if traits.is_some_and(|t| t.smcp) => feats.push("\"smcp\" 1"),
            Caps::Small => d.caps = CapsMode::SmallSynth,
        }
        d.features = feats.join(", ");
        d
    }

    /// Shape `text` on one unbroken line in one style at `size`: per glyph
    /// run its font, and glyphs as (id, x, y from the baseline, advance,
    /// source bytes).
    fn shape_one_line(
        &mut self,
        text: &str,
        attrs: &CharAttrs,
        size: f64,
    ) -> Vec<(FontHandle, Shaped)> {
        let mut a = attrs.clone();
        a.font_size = size;
        let d = Derived::plain(&a, size);
        // A capitalised drop cap or probe: same byte lengths, so ranges hold.
        let mapped: String;
        let text = if matches!(a.caps, Caps::All | Caps::Small) {
            mapped = text.chars().map(|c| upper1(c).unwrap_or(c)).collect();
            mapped.as_str()
        } else {
            text
        };
        let mut builder = self
            .lcx
            .ranged_builder(&mut self.fonts.fcx, text, 1.0, false);
        push_attrs(&mut builder, None, &a, &d, None);
        let mut layout: Layout<Ink> = builder.build(text);
        layout.break_all_lines(None);
        let mut out = Vec::new();
        for line in layout.lines() {
            let base = line.metrics().baseline;
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(gr) = item else {
                    continue;
                };
                let run = gr.run();
                let mut ranges: Vec<Range<usize>> = Vec::new();
                for c in run.visual_clusters() {
                    let r = c.text_range();
                    for j in 0..c.glyphs().count() {
                        ranges.push(if j == 0 { r.clone() } else { r.end..r.end });
                    }
                }
                let positioned: Vec<parley::Glyph> = gr.positioned_glyphs().collect();
                if ranges.len() != positioned.len() {
                    ranges = vec![0..0; positioned.len()];
                }
                let synth = run.synthesis();
                let font = run.font();
                out.push((
                    FontHandle {
                        data: font.data.clone(),
                        index: font.index,
                    },
                    Shaped {
                        size: f64::from(run.font_size()),
                        color: gr.style().brush,
                        bold: synth.embolden(),
                        skew: synth.skew().map(f64::from),
                        glyphs: positioned
                            .iter()
                            .zip(ranges)
                            .map(|(g, r)| {
                                (
                                    g.id,
                                    f64::from(g.x),
                                    f64::from(g.y - base),
                                    f64::from(g.advance),
                                    r,
                                )
                            })
                            .collect(),
                    },
                ));
            }
        }
        out
    }

    /// The glyph repeated for a tab leader, in the style of `attrs`.
    fn leader_glyph(&mut self, ch: char, attrs: &CharAttrs) -> Option<Leader> {
        let mut buf = [0u8; 4];
        let shaped = self.shape_one_line(ch.encode_utf8(&mut buf), attrs, attrs.font_size);
        let (font, s) = shaped.into_iter().next()?;
        let g = s.glyphs.first()?;
        Some(Leader {
            font,
            size: s.size,
            color: s.color,
            bold: s.bold,
            skew: s.skew,
            id: g.0,
            advance: g.3,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn flow_paragraph(
        &mut self,
        doc: &Document,
        para: &Paragraph,
        pa: &ParaAttrs,
        para_start: usize,
        regions: &[Region],
        label: Option<&str>,
        cur: &mut FlowCursor,
        placed: &mut Vec<PlacedLine>,
        out: &mut StoryLayout,
        break_before: Option<usize>,
    ) -> Option<usize> {
        let orig: String = para.text();
        let text_arc: Arc<str> = Arc::from(orig.as_str());
        // A drop cap takes the paragraph's first characters out of the
        // lines: `body` is the rest, shaped as usual; the cap is shaped on
        // its own and drawn on the first line (ADR 0015).
        let cap_chars = pa.drop_cap.map_or(0, |d| {
            orig.chars()
                .take(usize::from(d.chars))
                .take_while(|c| !ends_line(*c))
                .count()
        });
        let cap_b = if cap_chars == 0 {
            0
        } else {
            orig.char_indices()
                .nth(cap_chars)
                .map_or(orig.len(), |(i, _)| i)
        };
        let body: &str = &orig[cap_b..];
        // Resolved formats with their byte ranges in `body`. Runs wholly in
        // the cap are left out, except the last (an all-cap paragraph still
        // has a body format).
        let mut run_attrs: Vec<(Range<usize>, CharAttrs)> = Vec::with_capacity(para.runs.len());
        let mut b = 0;
        for r in &para.runs {
            let e = b + r.text.len();
            if cap_b == 0 || e > cap_b || e == orig.len() {
                run_attrs.push((
                    b.max(cap_b) - cap_b..e.max(cap_b) - cap_b,
                    doc.styles.resolve_char(para, r),
                ));
            }
            b = e;
        }
        let default_attrs = || doc.styles.resolve_char(para, &Run::default());
        let cap = match pa.drop_cap {
            Some(dc) if cap_chars > 0 => {
                let cap_run = para.runs.iter().find(|r| !r.text.is_empty());
                let attrs =
                    cap_run.map_or_else(default_attrs, |r| doc.styles.resolve_char(para, r));
                if !self.fonts.ensure_family(&attrs.font_family) {
                    out.missing_fonts.insert(attrs.font_family.clone());
                }
                let body_attrs = run_attrs
                    .first()
                    .map_or_else(default_attrs, |a| a.1.clone());
                self.shape_cap(&text_arc, cap_b, &attrs, &body_attrs, pa.line_spacing, dc)
            }
            _ => None,
        };
        let cap_lines = cap.as_ref().map_or(0, |c| c.lines);
        let cap_indent = cap.as_ref().map_or(0.0, |c| c.width + c.gap);
        let has_breaks = body.contains(is_break);
        // Tabs: parley shapes each as a space and the segments after it are
        // moved to the tab stops afterwards (ADR 0019). `tab_chars` holds
        // (character index, byte) of every tab in the paragraph.
        let tab_chars: Vec<(usize, usize)> = if body.contains('\t') {
            orig.char_indices()
                .enumerate()
                .filter(|(i, (_, c))| *c == '\t' && *i >= cap_chars)
                .map(|(i, (b, _))| (i, b))
                .collect()
        } else {
            Vec::new()
        };
        let has_tabs = !tab_chars.is_empty();
        // A drop cap takes the first lines: no label beside it.
        let label = label.filter(|_| cap.is_none());
        let list = pa.list.filter(|_| label.is_some());
        let left_indent = list.map_or(pa.left_indent, |l| pa.left_indent.max(l.indent));
        let mut leader_cache: HashMap<(char, usize), Option<Leader>> = HashMap::new();
        // `text` is what parley shapes: the paragraph with soft hyphens
        // inserted at hyphenation points (ADR 0014), and break characters
        // shaped as "\n" (same byte length, ADR 0015). Story offsets are
        // always counted on `orig`.
        let hy_lang = hyphen::lang_of(&pa.language).filter(|_| pa.hyphenate);
        // If the line analysis cannot follow the paragraph (right-to-left
        // text, a line end it cannot place), it is laid out again once
        // without the inserted soft hyphens (ADR 0014).
        for (_, a) in &run_attrs {
            if !self.fonts.ensure_family(&a.font_family) {
                out.missing_fonts.insert(a.font_family.clone());
            }
        }
        let derived: Vec<Derived> = run_attrs.iter().map(|(_, a)| self.derive(a)).collect();
        let first_attrs = run_attrs
            .first()
            .map_or_else(default_attrs, |s| s.1.clone());
        let first_d = run_attrs.first().map_or_else(
            || Derived::plain(&first_attrs, first_attrs.font_size),
            |_| derived[0].clone(),
        );
        let cur0 = *cur;
        let mut plain = hy_lang.is_none();
        let (shadow, mut layout, mut lcx, line_infos, placements, overflowed, cap_at, first_x) = 'attempt: loop {
            *cur = cur0;
            let mut shadow = match hy_lang.filter(|_| !plain) {
                Some(lang) => Shadow::hyphenated(body, lang),
                None => Shadow::plain(body),
            };
            if has_breaks {
                shadow.text = std::borrow::Cow::Owned(shadow.text.replace(is_break, "\n"));
            }
            if has_tabs {
                shadow.text = std::borrow::Cow::Owned(shadow.text.replace('\t', " "));
            }
            let breaks = if has_breaks {
                newline_kinds(&shadow.text, body, &shadow)
            } else {
                Vec::new()
            };
            let sranges: Vec<Range<usize>> = run_attrs
                .iter()
                .map(|(r, _)| shadow.from_orig(r.start)..shadow.from_orig(r.end))
                .collect();
            // All caps and synthesized small caps change letters in place
            // (same bytes, same characters), never the story text.
            let mut small: Vec<(Range<usize>, f64)> = Vec::new();
            if derived.iter().any(|d| d.caps != CapsMode::Off) {
                let (mapped, s) = apply_caps(&shadow.text, &sranges, &derived);
                shadow.text = std::borrow::Cow::Owned(mapped);
                small = s;
            }
            let text: &str = &shadow.text;
            let spans: Vec<(Range<usize>, &CharAttrs)> = sranges
                .iter()
                .cloned()
                .zip(run_attrs.iter().map(|(_, a)| a))
                .collect();
            let first = first_attrs.clone();
            let mut builder = self
                .lcx
                .ranged_builder(&mut self.fonts.fcx, text, 1.0, false);
            builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
                pa.line_spacing as f32,
            )));
            push_attrs(&mut builder, None, &first, &first_d, None);
            for ((range, a), d) in spans.iter().zip(&derived) {
                if !range.is_empty() && **a != first {
                    push_attrs(&mut builder, Some(range.clone()), a, d, Some(&first_d));
                }
            }
            for (range, size) in &small {
                builder.push(StyleProperty::FontSize(*size as f32), range.clone());
            }
            let mut layout: Layout<Ink> = builder.build(text);
            // Tab segments are never stretched: a paragraph with tabs is
            // set flush left and the stops place its text.
            let justified = pa.align == Align::Justify && !has_tabs;
            let limited = justified && pa.justify_limits.is_some();
            let mut lcx =
                (limited || text.contains(hyphen::SHY)).then(|| LineCx::new(&mut layout, text));
            if !plain && !shadow.inserted.is_empty() && lcx.as_ref().is_some_and(|c| !c.ok()) {
                plain = true;
                continue 'attempt;
            }
            let bparams = BreakParams {
                shadow: &shadow,
                justify: justified,
                limits: pa.justify_limits,
            };
            let mut line_infos: Vec<LineInfo> = Vec::new();

            let max_w = regions.iter().map(|r| r.w).fold(1.0, f64::max) as f32;
            let min_w = (first.font_size * 3.0).max(18.0);
            let mut placements: Vec<(usize, f64, f64)> = Vec::new();
            let mut overflowed = false;
            // Region and x of the drop cap (set with the first line).
            let mut cap_at: Option<(usize, f64)> = None;
            // x where the first line's text starts.
            let mut first_x = 0.0f64;
            {
                let mut bl = layout.break_lines();
                bl.state_mut().set_layout_max_advance(max_w);
                let mut first_line = true;
                let mut est_h = (first.font_size * pa.line_spacing).max(1.0);
                let mut retried = false;
                let mut guard = 0u32;
                let mut explicit = 0usize;
                loop {
                    guard += 1;
                    if guard > 100_000 {
                        overflowed = true;
                        break;
                    }
                    // A widow fix moves the region break up to this line.
                    if break_before == Some(placements.len())
                        && placements.last().is_some_and(|p| p.0 == cur.region)
                    {
                        advance_region(cur, regions);
                    }
                    let Some(r) = regions.get(cur.region) else {
                        overflowed = true;
                        break;
                    };
                    let mut y = cur.y;
                    if first_line && y > r.y + 1e-6 {
                        y += pa.space_before;
                    }
                    // A drop cap that would hang below the region starts
                    // the paragraph in the next one.
                    if first_line
                        && cap_lines > 1
                        && y > r.y + 1e-6
                        && y + cap_lines as f64 * est_h > r.bottom() + 0.01
                        && cur.region + 1 < regions.len()
                    {
                        advance_region(cur, regions);
                        continue;
                    }
                    let avail = r.available(y, y + est_h, min_w);
                    let single;
                    let slots: &[(f64, f64)] = match &avail {
                        Avail::Span(x, w) => {
                            single = [(*x, *w)];
                            &single
                        }
                        Avail::Slots(v) => v,
                        Avail::Skip(ny) => {
                            cur.y = ny.max(cur.y + 0.5);
                            if cur.y >= r.bottom() {
                                advance_region(cur, regions);
                            }
                            continue;
                        }
                    };
                    // A row of several slots breaks one line into each, left
                    // to right, at the same y. `bl.revert` undoes only the
                    // last line, so keep a checkpoint of the whole row.
                    let multi = slots.len() > 1;
                    let checkpoint = multi.then(|| RowCheckpoint {
                        state: bl.state().clone(),
                        pos: lcx.as_ref().map(|c| c.pos),
                        placed: placements.len(),
                        infos: line_infos.len(),
                        first_line,
                        cap_at,
                        first_x,
                        explicit,
                    });
                    let mut row_h = 0.0f64;
                    let mut end = RowEnd::Next;
                    for &(ax, aw) in slots {
                        let indent = if cap.is_some() {
                            // The first `cap_lines` lines beside the cap make
                            // room for it; no first-line indent with a cap.
                            let beside = placements.len() < cap_lines
                                && cap_at.is_none_or(|(cr, _)| cr == cur.region);
                            pa.left_indent + if beside { cap_indent } else { 0.0 }
                        } else {
                            left_indent
                                + if first_line && list.is_none() {
                                    pa.first_line_indent
                                } else {
                                    0.0
                                }
                        };
                        let x = ax + indent.max(0.0);
                        if first_line {
                            first_x = x;
                        }
                        let w = (aw - indent.max(0.0) - pa.right_indent).max(1.0);
                        {
                            let st = bl.state_mut();
                            st.set_line_x(x as f32);
                            st.set_line_max_advance((w as f32).min(max_w));
                            st.set_line_y(y);
                        }
                        let next = match lcx.as_mut() {
                            Some(c) => c.break_line(&mut bl, &bparams, text, (w as f32).min(max_w)),
                            None => bl.break_next(),
                        };
                        let Some(data) = next else {
                            end = RowEnd::Done;
                            break;
                        };
                        let YieldData::LineBreak(d) = data else {
                            end = RowEnd::Again;
                            break;
                        };
                        let h = d.line_y_end - d.line_y_start;
                        if h > est_h + 0.01
                            && !retried
                            && !r.obstacles.is_empty()
                            && r.available(y, y + h.max(row_h), min_w) != avail
                        {
                            end = RowEnd::Taller(h);
                            break;
                        }
                        if y + h > r.bottom() + 0.01 {
                            end = RowEnd::NextRegion;
                            break;
                        }
                        if let Some(c) = lcx.as_mut() {
                            let info = c.pending.take();
                            if let Some(i) = &info {
                                c.pos = i.end;
                            }
                            line_infos.push(info.unwrap_or_default());
                        }
                        if first_line && cap.is_some() {
                            cap_at = Some((cur.region, ax + pa.left_indent.max(0.0)));
                        }
                        placements.push((cur.region, y, h));
                        row_h = row_h.max(h);
                        first_line = false;
                        // A line ending at a break character: the next line
                        // goes where the break sends it (a soft line break
                        // only moves on to the next slot).
                        if d.reason == BreakReason::Explicit {
                            let kind = breaks.get(explicit).copied().flatten();
                            explicit += 1;
                            if let Some(k) = kind {
                                end = RowEnd::Jump(k);
                                break;
                            }
                        }
                    }
                    match end {
                        RowEnd::Taller(h) => {
                            match checkpoint {
                                Some(cp) => {
                                    cp.restore(
                                        &mut bl,
                                        lcx.as_mut(),
                                        &mut placements,
                                        &mut line_infos,
                                        &mut first_line,
                                        &mut cap_at,
                                        &mut first_x,
                                        &mut explicit,
                                    );
                                }
                                None => {
                                    bl.revert();
                                }
                            }
                            est_h = h;
                            retried = true;
                            continue;
                        }
                        RowEnd::NextRegion => {
                            retried = false;
                            match checkpoint {
                                Some(cp) => {
                                    cp.restore(
                                        &mut bl,
                                        lcx.as_mut(),
                                        &mut placements,
                                        &mut line_infos,
                                        &mut first_line,
                                        &mut cap_at,
                                        &mut first_x,
                                        &mut explicit,
                                    );
                                }
                                None => {
                                    bl.revert();
                                }
                            }
                            advance_region(cur, regions);
                            continue;
                        }
                        RowEnd::Again => {
                            if let Some(cp) = checkpoint {
                                cp.restore(
                                    &mut bl,
                                    lcx.as_mut(),
                                    &mut placements,
                                    &mut line_infos,
                                    &mut first_line,
                                    &mut cap_at,
                                    &mut first_x,
                                    &mut explicit,
                                );
                            }
                            continue;
                        }
                        RowEnd::Done => {
                            if row_h > 0.0 {
                                cur.y = y + row_h;
                            }
                            break;
                        }
                        RowEnd::Next => {
                            retried = false;
                            cur.y = y + row_h;
                            est_h = row_h;
                        }
                        RowEnd::Jump(k) => {
                            retried = false;
                            est_h = row_h;
                            cur.region = break_target(k, cur.region, regions);
                            cur.y = regions.get(cur.region).map_or(0.0, |r| r.y);
                        }
                    }
                }
                bl.finish();
            }
            if !plain && !shadow.inserted.is_empty() && lcx.as_ref().is_some_and(|c| !c.ok()) {
                plain = true;
                continue 'attempt;
            }
            break (
                shadow, layout, lcx, line_infos, placements, overflowed, cap_at, first_x,
            );
        };
        let text: &str = &shadow.text;
        layout.align(
            to_parley_align(if has_tabs { Align::Left } else { pa.align }),
            AlignmentOptions {
                align_when_overflowing: false,
                last_line_alignment: None,
            },
        );

        let n_lines = placements.len();
        // The cap sits on the baseline of line `cap_lines` when that line is
        // beside it, else where that line would be.
        let cap_base = cap.as_ref().zip(cap_at).and_then(|(c, (cr, _))| {
            let b0 = f64::from(layout.get(0)?.metrics().baseline);
            let k = c.lines - 1;
            match (placements.get(k), layout.get(k)) {
                (Some(p), Some(l)) if p.0 == cr => Some(f64::from(l.metrics().baseline)),
                _ => Some(b0 + k as f64 * c.pitch),
            }
        });
        let mut groups_cache: HashMap<usize, (Vec<ClusterGroup>, usize)> = HashMap::new();
        let mut last_end_byte = 0usize;
        let mut counter = CharCounter::new(text);
        let offsets = ClusterOffsets::new(&layout, text.len());
        for (li, line) in layout.lines().enumerate() {
            let Some(&(region, y, h)) = placements.get(li) else {
                break;
            };
            let m = line.metrics();
            let tr = line.text_range();
            let (tb0, tb1) = (tr.start.min(text.len()), tr.end.min(text.len()));
            last_end_byte = tb1;
            // A break character ending the line belongs to no line, like
            // the "\n" between paragraphs: the caret sits before it at the
            // end of this line and after it at the start of the next.
            let te = if has_breaks && tb1 > tb0 && text.as_bytes().get(tb1 - 1) == Some(&b'\n') {
                tb1 - 1
            } else {
                tb1
            };
            let (c0, c1) = (
                counter.at(tb0) - shadow.inserted_before(tb0),
                counter.at(te) - shadow.inserted_before(te),
            );
            let info = line_infos.get(li).copied().unwrap_or_default();
            let letter = info.letter;
            let mut end_x = 0.0f64;
            let body0 = para_start + cap_chars;
            let chars = body0 + c0..body0 + c1;
            let mut runs = Vec::new();
            let mut decorations = Vec::new();
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(gr) = item else {
                    continue;
                };
                let run = gr.run();
                // Unique per shaped run on a line (cluster ranges count from
                // the start of each shaped run, so runs would collide).
                let key = run.text_range().start;
                let entry = groups_cache.entry(key).or_insert_with(|| {
                    let mut groups: Vec<ClusterGroup> = Vec::new();
                    let mut last_style = None;
                    for c in run.visual_clusters() {
                        let s = c.style_index();
                        if last_style != Some(s) {
                            groups.push(Vec::new());
                            last_style = Some(s);
                        }
                        groups
                            .last_mut()
                            .expect("pushed")
                            .push((c.text_range(), c.glyphs().count()));
                    }
                    (groups, 0)
                });
                let group = entry.0.get(entry.1).cloned().unwrap_or_default();
                entry.1 += 1;
                let positioned: Vec<parley::Glyph> = gr.positioned_glyphs().collect();
                let mut ranges: Vec<Range<usize>> = Vec::with_capacity(positioned.len());
                for (range, count) in &group {
                    for j in 0..*count {
                        ranges.push(if j == 0 {
                            range.clone()
                        } else {
                            range.end..range.end
                        });
                    }
                }
                if ranges.len() != positioned.len() {
                    let all = group
                        .iter()
                        .map(|g| g.0.clone())
                        .reduce(|a, b| a.start.min(b.start)..a.end.max(b.end))
                        .unwrap_or(0..0);
                    ranges = (0..positioned.len())
                        .map(|i| {
                            if i == 0 {
                                all.clone()
                            } else {
                                all.end..all.end
                            }
                        })
                        .collect();
                }
                let style = gr.style();
                let synth = run.synthesis();
                let font = run.font();
                // Letter spacing added to a justified line past its word-space
                // cap: every character gap grows by `letter`.
                let before = |b: usize| -> f64 {
                    if letter == 0.0 {
                        return 0.0;
                    }
                    let b = b.clamp(tb0, tb1);
                    let n = text[tb0..b].chars().count()
                        - (shadow.inserted_before(b) - shadow.inserted_before(tb0));
                    n as f64 * letter
                };
                let run_start = ranges.iter().map(|r| r.start).min().unwrap_or(tb0);
                let run_end = ranges.iter().map(|r| r.end).max().unwrap_or(tb0);
                let run_shift = before(run_start);
                let run_extra = before(run_end) - run_shift;
                end_x = f64::from(gr.offset() + gr.advance()) + before(run_end);
                runs.push(GlyphRun {
                    font: FontHandle {
                        data: font.data.clone(),
                        index: font.index,
                    },
                    font_size: f64::from(run.font_size()),
                    color: style.brush,
                    synthetic_bold: synth.embolden(),
                    synthetic_skew: synth.skew().map(f64::from),
                    text: text_arc.clone(),
                    glyphs: positioned
                        .iter()
                        .zip(ranges)
                        // Soft hyphens are invisible inside a line.
                        .filter(|(_, r)| text.get(r.clone()) != Some("\u{ad}"))
                        .map(|(g, r)| PositionedGlyph {
                            id: g.id,
                            x: f64::from(g.x) + before(r.start),
                            y: f64::from(g.y),
                            advance: f64::from(g.advance),
                            text: cap_b + shadow.to_orig(r.start)..cap_b + shadow.to_orig(r.end),
                        })
                        .collect(),
                });
                if let Some(ul) = &style.underline {
                    let fm = run.font_metrics();
                    let off = ul.offset.unwrap_or(fm.underline_offset);
                    let size = ul.size.unwrap_or(fm.underline_size).max(0.5);
                    decorations.push(Decoration {
                        rect: Rect::new(
                            f64::from(gr.offset()) + run_shift,
                            f64::from(gr.baseline() - off),
                            f64::from(gr.advance()) + run_extra,
                            f64::from(size),
                        ),
                        color: ul.brush,
                    });
                }
                if let Some(st) = &style.strikethrough {
                    let fm = run.font_metrics();
                    let off = st.offset.unwrap_or(fm.strikethrough_offset);
                    let size = st.size.unwrap_or(fm.strikethrough_size).max(0.5);
                    decorations.push(Decoration {
                        rect: Rect::new(
                            f64::from(gr.offset()) + run_shift,
                            f64::from(gr.baseline() - off),
                            f64::from(gr.advance()) + run_extra,
                            f64::from(size),
                        ),
                        color: st.brush,
                    });
                }
            }
            // A line that breaks at a soft hyphen ends with the font's
            // hyphen glyph (its width was reserved when the line was broken).
            if info.hyphen
                && tb1 >= tb0 + hyphen::SHY_LEN
                && tb1 < text.len()
                && text.get(tb1 - hyphen::SHY_LEN..tb1) == Some("\u{ad}")
                && let Some(c) = lcx.as_mut()
                && let Some((font, size, gid, adv)) = c.hyphen_glyph(tb1 - hyphen::SHY_LEN)
                && let Some(last) = runs.last()
            {
                let mut hr: GlyphRun = last.clone();
                let (gy, o) = (
                    last.glyphs.last().map_or(0.0, |g| g.y),
                    cap_b + shadow.to_orig(tb1),
                );
                hr.font = font;
                hr.font_size = f64::from(size);
                hr.glyphs = vec![PositionedGlyph {
                    id: gid,
                    x: end_x,
                    y: gy,
                    advance: f64::from(adv),
                    text: o..o,
                }];
                runs.push(hr);
            }
            let (mut carets, mut spans) = caret_positions(
                &layout,
                &offsets,
                text,
                &shadow,
                tb0..te,
                body0 + c0,
                letter,
                f64::from(m.inline_min_coord),
            );
            let mut rect = Rect::new(
                f64::from(m.inline_min_coord),
                y,
                info.full_w.map_or(
                    f64::from(m.inline_max_coord - m.inline_min_coord),
                    f64::from,
                ),
                h,
            );
            let mut chars = chars;
            // Tabs: the text after each tab moves to its stop; leaders fill
            // the gap (ADR 0019).
            let line_tabs: Vec<(usize, usize)> = tab_chars
                .iter()
                .map(|(i, b)| (para_start + i, *b))
                .filter(|(o, _)| (chars.start..chars.end).contains(o))
                .collect();
            if !line_tabs.is_empty() {
                let region_x = regions[region].x;
                let offs: Vec<usize> = line_tabs.iter().map(|t| t.0).collect();
                let bytes: Vec<usize> = line_tabs.iter().map(|t| t.1).collect();
                let sep = decimal_sep(&pa.language);
                let segs = plan_tabs(
                    &offs,
                    &|o| caret_at(&carets, o, rect.x),
                    chars.end,
                    &|a, b| {
                        orig.chars()
                            .skip(a - para_start)
                            .take(b - a)
                            .position(|c| c == sep)
                            .map(|p| a + p)
                    },
                    &pa.tab_stops,
                    region_x,
                );
                let shift_of = |k: usize| if k == 0 { 0.0 } else { segs[k - 1].shift };
                for c in &mut carets {
                    c.1 += shift_of(offs.partition_point(|&t| t < c.0));
                }
                let mut moved: Vec<GlyphRun> = Vec::with_capacity(runs.len() + segs.len());
                for r in std::mem::take(&mut runs) {
                    let GlyphRun {
                        font,
                        font_size,
                        color,
                        synthetic_bold,
                        synthetic_skew,
                        text,
                        glyphs,
                    } = r;
                    let mut groups: Vec<(usize, Vec<PositionedGlyph>)> = Vec::new();
                    for mut g in glyphs {
                        if g.text.len() == 1 && bytes.binary_search(&g.text.start).is_ok() {
                            continue;
                        }
                        let k = bytes.partition_point(|&b| b < g.text.start);
                        g.x += shift_of(k);
                        match groups.last_mut() {
                            Some((gk, v)) if *gk == k => v.push(g),
                            _ => groups.push((k, vec![g])),
                        }
                    }
                    for (_, glyphs) in groups {
                        moved.push(GlyphRun {
                            font: font.clone(),
                            font_size,
                            color,
                            synthetic_bold,
                            synthetic_skew,
                            text: text.clone(),
                            glyphs,
                        });
                    }
                }
                runs = moved;
                let mut split: Vec<Decoration> = Vec::with_capacity(decorations.len());
                for d in std::mem::take(&mut decorations) {
                    for k in 0..=segs.len() {
                        let lo = if k == 0 {
                            f64::NEG_INFINITY
                        } else {
                            segs[k - 1].nat_start
                        };
                        let hi = segs.get(k).map_or(f64::INFINITY, |s| s.nat_before);
                        let (x0, x1) = (d.rect.x.max(lo), d.rect.right().min(hi));
                        if x1 - x0 > 0.01 {
                            split.push(Decoration {
                                rect: Rect::new(x0 + shift_of(k), d.rect.y, x1 - x0, d.rect.height),
                                color: d.color,
                            });
                        }
                    }
                }
                decorations = split;
                let base = f64::from(m.baseline);
                for (k, seg) in segs.iter().enumerate() {
                    let Some(ch) = seg.leader.glyph() else {
                        continue;
                    };
                    let ai = run_attrs
                        .iter()
                        .position(|(r, _)| r.contains(&(bytes[k] - cap_b)))
                        .unwrap_or(0);
                    let leader = match leader_cache.get(&(ch, ai)) {
                        Some(l) => l.clone(),
                        None => {
                            let attrs = run_attrs
                                .get(ai)
                                .map_or_else(default_attrs, |a| a.1.clone());
                            let l = self.leader_glyph(ch, &attrs);
                            leader_cache.insert((ch, ai), l.clone());
                            l
                        }
                    };
                    if let Some(run) =
                        leader.and_then(|l| leader_run(&l, ch, region_x, seg.pen, seg.start, base))
                    {
                        runs.push(run);
                    }
                }
                let end = caret_at(&carets, chars.end, rect.right());
                let right = rect.right().max(end);
                rect.width = right - rect.x;
            }
            // A list item's first line starts with its bullet or number,
            // hanging left of the text.
            let mut list_label = None;
            if li == 0
                && let (Some(text_label), Some(l)) = (label, list)
            {
                let attrs = run_attrs
                    .first()
                    .map_or_else(default_attrs, |a| a.1.clone());
                let shaped = self.shape_one_line(text_label, &attrs, attrs.font_size);
                let width = shaped
                    .iter()
                    .flat_map(|(_, s)| s.glyphs.iter())
                    .map(|g| g.1 + g.3)
                    .fold(0.0f64, f64::max);
                let gap = attrs.font_size * 0.25;
                let mut x0 = first_x - l.indent;
                if x0 + width + gap > first_x {
                    x0 = first_x - gap - width;
                }
                x0 = x0.max(regions[region].x);
                let base = f64::from(m.baseline);
                let label_text: Arc<str> = Arc::from(text_label);
                let mut label_runs: Vec<GlyphRun> = shaped
                    .into_iter()
                    .map(|(font, s)| GlyphRun {
                        font,
                        font_size: s.size,
                        color: s.color,
                        synthetic_bold: s.bold,
                        synthetic_skew: s.skew,
                        text: label_text.clone(),
                        glyphs: s
                            .glyphs
                            .into_iter()
                            .map(|(id, gx, gy, advance, text)| PositionedGlyph {
                                id,
                                x: x0 + gx,
                                y: base + gy,
                                advance,
                                text,
                            })
                            .collect(),
                    })
                    .collect();
                label_runs.append(&mut runs);
                runs = label_runs;
                let right = rect.right();
                rect.x = rect.x.min(x0);
                rect.width = right - rect.x;
                list_label = Some(text_label.to_string());
            }
            // The drop cap is drawn by the first line: its glyphs come first,
            // its characters start the line and have carets across it.
            if li == 0
                && let (Some(c), Some((_, cap_x)), Some(base)) = (&cap, cap_at, cap_base)
            {
                let mut all: Vec<GlyphRun> = c
                    .runs
                    .iter()
                    .map(|r| {
                        let mut r = r.clone();
                        for g in &mut r.glyphs {
                            g.x += cap_x;
                            g.y += base;
                        }
                        r
                    })
                    .collect();
                all.append(&mut runs);
                runs = all;
                let mut cc: Vec<(usize, f64)> = (0..cap_chars)
                    .map(|k| {
                        (
                            para_start + k,
                            cap_x + c.carets.get(k).copied().unwrap_or(0.0),
                        )
                    })
                    .collect();
                cc.append(&mut carets);
                carets = cc;
                if !spans.is_empty() {
                    let mut sp = vec![(0.0, 0.0); cap_chars];
                    sp.append(&mut spans);
                    spans = sp;
                }
                chars.start = para_start;
                let right = rect.right();
                rect.x = rect.x.min(cap_x);
                rect.width = right - rect.x;
            }
            let ends_paragraph = li + 1 == n_lines && !overflowed;
            placed.push(PlacedLine {
                region,
                line: LineBox {
                    rect,
                    baseline: f64::from(m.baseline),
                    chars,
                    runs,
                    decorations,
                    carets,
                    spans,
                    rtl: layout.is_rtl(),
                    ends_paragraph,
                    list_label,
                },
            });
        }
        if overflowed {
            if n_lines == 0 {
                return Some(para_start);
            }
            let byte = last_end_byte;
            return Some(para_start + cap_chars + counter.at(byte) - shadow.inserted_before(byte));
        }
        // A paragraph shorter than its drop cap keeps the cap's lines.
        if let (Some(c), Some((cr, _)), Some(&(_, y0, _))) = (&cap, cap_at, placements.first())
            && cur.region == cr
        {
            cur.y = cur.y.max(y0 + c.lines as f64 * c.pitch);
        }
        cur.y += pa.space_after;
        None
    }
}

/// A glyph repeated to make a tab leader.
#[derive(Debug, Clone)]
struct Leader {
    font: FontHandle,
    size: f64,
    color: Ink,
    bold: bool,
    skew: Option<f64>,
    id: u32,
    advance: f64,
}

/// A tab on a laid-out line, resolved against the stops.
#[derive(Debug, Clone, Copy)]
struct TabSeg {
    /// Natural x at the end of the text before the tab.
    nat_before: f64,
    /// Natural x where the text after the tab starts.
    nat_start: f64,
    /// How far everything after the tab moves.
    shift: f64,
    /// Where the pen is when the tab begins (placed x).
    pen: f64,
    /// Where the text after the tab starts (placed x).
    start: f64,
    leader: TabLeader,
}

/// The decimal separator a language writes: a comma for most of continental
/// Europe and Latin America, a full stop otherwise.
fn decimal_sep(language: &str) -> char {
    let base = language
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    const COMMA: [&str; 20] = [
        "de", "fr", "es", "it", "pt", "nl", "ru", "pl", "sv", "da", "nb", "nn", "no", "fi", "cs",
        "sk", "hu", "tr", "uk", "bg",
    ];
    if COMMA.contains(&base.as_str()) {
        ','
    } else {
        '.'
    }
}

/// Caret x of story offset `o` among a line's carets.
fn caret_at(carets: &[(usize, f64)], o: usize, default: f64) -> f64 {
    match carets.binary_search_by_key(&o, |c| c.0) {
        Ok(i) => carets[i].1,
        Err(i) => carets
            .get(i.min(carets.len().saturating_sub(1)))
            .map_or(default, |c| c.1),
    }
}

/// Place the text after each of a line's tabs. `tabs` are the story offsets
/// of the tabs; `caret` gives the natural x of an offset, `line_end` the
/// offset where the line ends, `sep_at(a, b)` the offset of the first decimal
/// separator in `a..b`, and `origin` the x the stops are measured from.
/// Stops are used in order; past the last one (or with none) the default
/// grid applies.
fn plan_tabs(
    tabs: &[usize],
    caret: &dyn Fn(usize) -> f64,
    line_end: usize,
    sep_at: &dyn Fn(usize, usize) -> Option<usize>,
    stops: &[TabStop],
    origin: f64,
) -> Vec<TabSeg> {
    let mut out: Vec<TabSeg> = Vec::with_capacity(tabs.len());
    let mut prev_shift = 0.0f64;
    for (i, &off) in tabs.iter().enumerate() {
        let nat_before = caret(off);
        let nat_start = caret(off + 1);
        let pen = nat_before + prev_shift;
        let end_off = tabs.get(i + 1).copied().unwrap_or(line_end);
        let width = (caret(end_off) - nat_start).max(0.0);
        let rel = pen - origin;
        let stop = stops
            .iter()
            .find(|s| s.pos > rel + 0.01)
            .copied()
            .unwrap_or_else(|| TabStop {
                pos: ((rel + 0.01) / DEFAULT_TAB_INTERVAL).floor() * DEFAULT_TAB_INTERVAL
                    + DEFAULT_TAB_INTERVAL,
                align: TabAlign::Left,
                leader: TabLeader::None,
            });
        let lead = match stop.align {
            TabAlign::Left => 0.0,
            TabAlign::Center => width / 2.0,
            TabAlign::Right => width,
            TabAlign::Decimal => {
                sep_at(off + 1, end_off).map_or(width, |o| (caret(o) - nat_start).clamp(0.0, width))
            }
        };
        let start = (origin + stop.pos - lead).max(pen);
        let shift = start - nat_start;
        out.push(TabSeg {
            nat_before,
            nat_start,
            shift,
            pen,
            start,
            leader: stop.leader,
        });
        prev_shift = shift;
    }
    out
}

/// Leader glyphs filling `from..to`, on a grid anchored at `origin` so
/// that the leaders of consecutive lines line up. Dots are spaced two
/// advances apart, dashes and lines one.
fn leader_run(
    l: &Leader,
    ch: char,
    origin: f64,
    from: f64,
    to: f64,
    base: f64,
) -> Option<GlyphRun> {
    let pitch = l.advance * if ch == '.' { 2.0 } else { 1.0 };
    if pitch < 0.5 || to - from < pitch {
        return None;
    }
    let mut n = ((from + 1.0 - origin) / pitch).ceil();
    let mut glyphs = Vec::new();
    while glyphs.len() < 4000 {
        let x = origin + n * pitch;
        if x + l.advance > to - 1.0 {
            break;
        }
        let i = glyphs.len();
        glyphs.push(PositionedGlyph {
            id: l.id,
            x,
            y: base,
            advance: l.advance,
            text: i..i + 1,
        });
        n += 1.0;
    }
    if glyphs.is_empty() {
        return None;
    }
    Some(GlyphRun {
        font: l.font.clone(),
        font_size: l.size,
        color: l.color,
        synthetic_bold: l.bold,
        synthetic_skew: l.skew,
        text: Arc::from(ch.to_string().repeat(glyphs.len())),
        glyphs,
    })
}

/// The label of every paragraph that is a list item: a bullet, or its
/// number. Counters run per level through the story: a numbered item
/// continues its level's count (or restarts at `start`) and resets the
/// deeper levels; paragraphs that are not list items change nothing, so a
/// count continues across them (ADR 0019).
fn list_labels(pas: &[ParaAttrs]) -> Vec<Option<String>> {
    // No list in the story: nothing to number (callers index with `get`).
    if !pas.iter().any(|p| p.list.is_some()) {
        return Vec::new();
    }
    let mut counters = [0u32; MAX_LIST_LEVEL as usize + 1];
    pas.iter()
        .map(|pa| {
            let l = pa.list?;
            let lv = usize::from(l.level.min(MAX_LIST_LEVEL));
            for c in &mut counters[lv + 1..] {
                *c = 0;
            }
            match l.kind {
                ListKind::Number => {
                    let n = l.start.unwrap_or(counters[lv].saturating_add(1));
                    counters[lv] = n;
                    Some(l.format.label(n))
                }
                ListKind::Bullet => Some(l.bullet.to_string()),
                ListKind::None => None,
            }
        })
        .collect()
}

fn advance_region(cur: &mut FlowCursor, regions: &[Region]) {
    cur.region += 1;
    cur.y = regions.get(cur.region).map_or(0.0, |r| r.y);
}

/// Glyphs of one shaped run (see [`Layouter::shape_one_line`]).
struct Shaped {
    /// The size parley shaped at.
    size: f64,
    color: Ink,
    bold: bool,
    skew: Option<f64>,
    /// (glyph id, x, y from the baseline, advance, source bytes).
    glyphs: Vec<(u32, f64, f64, f64, Range<usize>)>,
}

/// Cap height as a fraction of the em, from the font's OS/2 table.
fn cap_height_ratio(font: &FontHandle) -> Option<f64> {
    use skrifa::MetadataProvider;
    let f = skrifa::FontRef::from_index(font.bytes(), font.index).ok()?;
    let m = f.metrics(
        skrifa::instance::Size::unscaled(),
        skrifa::instance::LocationRef::default(),
    );
    let cap = f64::from(m.cap_height?);
    let em = f64::from(m.units_per_em);
    (cap > 0.0 && em > 0.0).then(|| cap / em)
}

/// Whether `c` ends a line: a break character or one parley treats as a
/// newline (CR, LF, U+2028, U+2029). A drop cap stops before one.
fn ends_line(c: char) -> bool {
    is_break(c) || matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// For each character parley ends a line at (a newline: "\n", CR, CR LF,
/// U+2028, U+2029), in order: the kind of break character shaped as that
/// "\n", or `None` for a line or paragraph separator in the text.
fn newline_kinds(text: &str, body: &str, shadow: &Shadow<'_>) -> Vec<Option<BreakKind>> {
    let mut out = Vec::new();
    let mut it = text.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        match c {
            // CR LF is one newline; the LF entry stands for it.
            '\r' if it.peek().is_some_and(|(_, n)| *n == '\n') => {}
            '\n' => out.push(
                body.get(shadow.to_orig(i)..)
                    .and_then(|s| s.chars().next())
                    .and_then(BreakKind::of),
            ),
            '\r' | '\u{2028}' | '\u{2029}' => out.push(None),
            _ => {}
        }
    }
    out
}

/// The region the text after a break in region `from` continues in
/// (`regions.len()` when there is none: the rest overflows). A column break
/// goes to the next region; a frame break to the next frame of the chain;
/// a page break to the next frame of the chain on a later page (pages are
/// never added, ADR 0015).
fn break_target(kind: BreakKind, from: usize, regions: &[Region]) -> usize {
    let n = regions.len();
    let Some(r) = regions.get(from) else {
        return n;
    };
    let later = from + 1..n;
    match kind {
        BreakKind::Column => from + 1,
        BreakKind::Frame => later
            .into_iter()
            .find(|j| regions[*j].frame != r.frame)
            .unwrap_or(n),
        BreakKind::Page => match r.page {
            Some(p) => later
                .into_iter()
                .find(|j| regions[*j].page.is_some_and(|q| q > p))
                .unwrap_or(n),
            None => n,
        },
    }
}

/// Keep with next, checked after the last paragraph in `marks` is flowed:
/// if it starts in a later region than the paragraph before ends, and that
/// one keeps with next, the paragraph to flow again and the region to start
/// it at. It is the first of the run of keep-with-next paragraphs before
/// the last that start in the region where that run ends, so a paragraph is
/// never moved more than one region on and the rewind never reaches more
/// than one region back. Nothing moves when that paragraph is at the top of
/// its region (the group cannot fit any better), when it has been moved for
/// this rule already, or when there is no next region.
fn keep_with_next_move(
    marks: &[ParaMark],
    placed: &[PlacedLine],
    regions: &[Region],
    moved: &[usize],
) -> Option<(usize, usize)> {
    let i = marks.len().checked_sub(1)?;
    let prev = i.checked_sub(1)?;
    if !marks[prev].keep_with_next {
        return None;
    }
    let first = placed.get(marks[i].placed)?;
    let last = placed.get(marks[i].placed.checked_sub(1)?)?;
    let r = last.region;
    if first.region == r {
        return None;
    }
    let starts_in_r =
        |j: usize| marks[j].placed < marks[j + 1].placed && placed[marks[j].placed].region == r;
    if !starts_in_r(prev) {
        return None;
    }
    let mut j = prev;
    while j > 0 && marks[j - 1].keep_with_next && starts_in_r(j - 1) {
        j -= 1;
    }
    let at_top = placed[marks[j].placed].line.rect.y <= regions[r].y + 1e-6;
    (!at_top && r + 1 < regions.len() && !moved.contains(&j)).then_some((j, r + 1))
}

/// How a run's letters are mapped for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CapsMode {
    Off,
    /// All caps: every letter with a one-character capital.
    Upper,
    /// Small caps without an `smcp` feature: lowercase letters become
    /// capitals at [`SMALL_CAPS_SCALE`] of the run's size.
    SmallSynth,
}

/// Size of synthesized small capitals relative to the run.
const SMALL_CAPS_SCALE: f64 = 0.7;

/// What a run's attributes ask of the shaper beyond family, weight and
/// style (see [`Layouter::derive`]).
#[derive(Debug, Clone, PartialEq)]
struct Derived {
    /// Font size after super/subscript scaling.
    size: f64,
    /// Baseline shift in points, positive up.
    shift: f32,
    /// CSS font-feature settings ("" for none).
    features: String,
    caps: CapsMode,
    strike: bool,
}

impl Derived {
    /// For text shaped on its own (drop caps, leaders, probes): the size and
    /// the ligature and kerning switches only.
    fn plain(a: &CharAttrs, size: f64) -> Self {
        let mut feats: Vec<&str> = Vec::new();
        if !a.ligatures {
            feats.push("\"liga\" 0, \"clig\" 0");
        }
        if !a.kerning {
            feats.push("\"kern\" 0");
        }
        Self {
            size,
            shift: 0.0,
            features: feats.join(", "),
            caps: CapsMode::Off,
            strike: false,
        }
    }
}

/// The one-character capital of `c`, if it has one of the same UTF-8
/// length. Mapping only such letters keeps the shadow text a one-to-one,
/// same-byte-offset copy of the story text ('ß' stays 'ß').
fn upper1(c: char) -> Option<char> {
    let mut it = c.to_uppercase();
    let u = it.next()?;
    (it.next().is_none() && u != c && u.len_utf8() == c.len_utf8()).then_some(u)
}

/// Map `text` for display: letters in all-caps and small-caps runs become
/// capitals (see [`upper1`]). `ranges` are the byte ranges of the runs with
/// their `derived` data. Returns the new text and the byte ranges that
/// became synthesized small capitals, with their font size.
fn apply_caps(
    text: &str,
    ranges: &[Range<usize>],
    derived: &[Derived],
) -> (String, Vec<(Range<usize>, f64)>) {
    let mut out = String::with_capacity(text.len());
    let mut small: Vec<(Range<usize>, f64)> = Vec::new();
    let mut si = 0;
    for (i, c) in text.char_indices() {
        while si < ranges.len() && ranges[si].end <= i {
            si += 1;
        }
        let mode = ranges
            .get(si)
            .filter(|r| r.start <= i)
            .map_or(CapsMode::Off, |_| derived[si].caps);
        match (mode, upper1(c)) {
            (CapsMode::Off, _) | (_, None) => out.push(c),
            (CapsMode::Upper, Some(u)) => out.push(u),
            (CapsMode::SmallSynth, Some(u)) => {
                out.push(u);
                let size = derived[si].size * SMALL_CAPS_SCALE;
                match small.last_mut() {
                    Some((r, s)) if r.end == i && *s == size => r.end = i + c.len_utf8(),
                    _ => small.push((i..i + c.len_utf8(), size)),
                }
            }
        }
    }
    (out, small)
}

/// Push a run's attributes. With no `range` they are the defaults; with a
/// range they override the defaults, and the properties beyond the original
/// seven are pushed only where they differ from `base` (the defaults), so
/// text without the newer attributes is shaped exactly as before.
fn push_attrs(
    b: &mut parley::RangedBuilder<'_, Ink>,
    range: Option<Range<usize>>,
    a: &CharAttrs,
    d: &Derived,
    base: Option<&Derived>,
) {
    let props: [StyleProperty<'_, Ink>; 7] = [
        StyleProperty::FontFamily(FontFamily::named(&a.font_family)),
        StyleProperty::FontSize(d.size as f32),
        StyleProperty::FontWeight(if a.bold {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        }),
        StyleProperty::FontStyle(if a.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        }),
        StyleProperty::Underline(a.underline),
        StyleProperty::Brush(a.color),
        StyleProperty::LetterSpacing(a.letter_spacing as f32),
    ];
    let (b_shift, b_feat, b_strike) = base.map_or((0.0, "", false), |x| {
        (x.shift, x.features.as_str(), x.strike)
    });
    let mut more: Vec<StyleProperty<'_, Ink>> = Vec::new();
    if d.shift != b_shift {
        more.push(StyleProperty::VerticalAlign(ScriptAlign::length(d.shift)));
    }
    if d.features != b_feat {
        more.push(StyleProperty::FontFeatures(FontFeatures::from(
            d.features.as_str(),
        )));
    }
    if d.strike != b_strike {
        more.push(StyleProperty::Strikethrough(d.strike));
    }
    for p in props.into_iter().chain(more) {
        match &range {
            Some(r) => b.push(p, r.clone()),
            None => b.push_default(p),
        }
    }
}

/// A character's box as offsets from its caret, `(left, right)`.
type CharSpan = (f32, f32);

/// Caret x for each character boundary of the line whose paragraph bytes
/// are `bytes`; `first` is the story offset of its first character.
/// `text` is the shadow text: boundaries at an inserted soft hyphen are
/// skipped (it is not a story character), and `letter` is added per
/// character for justified lines stretched with letter spacing. Positions
/// are frame-local, like glyphs: parley measures them from the line's
/// start, `line_x`, which is added.
#[allow(clippy::too_many_arguments)]
fn caret_positions(
    layout: &Layout<Ink>,
    offsets: &ClusterOffsets,
    text: &str,
    shadow: &Shadow,
    bytes: Range<usize>,
    first: usize,
    letter: f64,
    line_x: f64,
) -> (Vec<(usize, f64)>, Vec<CharSpan>) {
    let (b0, b1) = (bytes.start, bytes.end);
    // Boxes of the characters are only kept for lines that can reorder.
    let bidi = layout.is_rtl() || text.get(b0..b1).is_some_and(|l| l.chars().any(is_rtl_char));
    let mut spans: Vec<CharSpan> = Vec::new();
    let mut boundaries: Vec<usize> = match text.get(b0..b1) {
        Some(line) => line.char_indices().map(|(i, _)| b0 + i).collect(),
        None => text
            .char_indices()
            .map(|(i, _)| i)
            .filter(|i| *i >= b0 && *i < b1)
            .collect(),
    };
    boundaries.push(b1);
    let mut out = Vec::with_capacity(boundaries.len());
    let mut k = 0usize;
    for b in &boundaries {
        if *b != b1 && shadow.is_inserted(*b) {
            continue;
        }
        let at_end = *b == b1 && *b != b0;
        let affinity = if at_end {
            Affinity::Upstream
        } else {
            Affinity::Downstream
        };
        let cursor = Cursor::from_byte_index(layout, *b, affinity);
        out.push((
            first + k,
            line_x + caret_x(layout, offsets, cursor) + k as f64 * letter,
        ));
        if bidi {
            spans.push(char_span(layout, text, *b, *b == b1));
        }
        k += 1;
    }
    (out, spans)
}

/// Whether `c` is written right to left (Hebrew, Arabic, Syriac, Thaana,
/// N'Ko and their presentation forms, and the historic right-to-left scripts).
pub(crate) fn is_rtl_char(c: char) -> bool {
    matches!(c as u32,
        0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF
        | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF | 0x200F | 0x202B | 0x202E | 0x2067)
}

/// The box of the character at paragraph byte `b`, as offsets from its
/// caret: a left-to-right character extends right of its caret, a
/// right-to-left one left of it. (A caret sits at the leading edge of the
/// character after it, so at a change of direction it is not next to the
/// character before it; this is what tells them apart.) The end of the
/// line has an empty box.
fn char_span(layout: &Layout<Ink>, text: &str, b: usize, end: bool) -> CharSpan {
    if end {
        return (0.0, 0.0);
    }
    let Some(c) = Cluster::from_byte_index(layout, b) else {
        return (0.0, 0.0);
    };
    let r = c.text_range();
    let n = text.get(r.clone()).map_or(1, |t| t.chars().count()).max(1);
    let k = text
        .get(r.start..b.min(r.end))
        .map_or(0, |t| t.chars().count()) as f32;
    let w = c.advance() / n as f32;
    if c.is_rtl() {
        (-(k + 1.0) * w, -k * w)
    } else {
        (k * w, (k + 1.0) * w)
    }
}

/// Leading-edge x of every cluster of a laid-out paragraph, indexed by the
/// cluster's first text byte. Equal, bit for bit, to what
/// `Cluster::visual_offset` returns: the line offset, plus the advance of
/// each earlier run, plus the advance of each earlier cluster of the run,
/// added in that order. One pass per line instead of one per caret.
/// (Layouts here hold no inline boxes, so every line item is a run.)
struct ClusterOffsets(Vec<f32>);

impl ClusterOffsets {
    fn new(layout: &Layout<Ink>, text_len: usize) -> Self {
        let mut v = vec![0.0f32; text_len + 1];
        for line in layout.lines() {
            let mut base = line.metrics().offset;
            for run in line.runs() {
                let mut x = base;
                for c in run.visual_clusters() {
                    if let Some(slot) = v.get_mut(c.text_range().start) {
                        *slot = x;
                    }
                    x += c.advance();
                }
                base += run.advance();
            }
        }
        Self(v)
    }

    fn get(&self, c: &Cluster<'_, Ink>) -> f32 {
        self.0
            .get(c.text_range().start)
            .copied()
            .unwrap_or_default()
    }
}

/// `cursor.geometry(layout, w).x0` without parley's walk along the line for
/// every call: the same choice of cluster edge, with the cluster's offset
/// read from `offsets`.
fn caret_x(layout: &Layout<Ink>, offsets: &ClusterOffsets, cursor: Cursor) -> f64 {
    let edge = |c: &Cluster<'_, Ink>, at_end: bool| {
        let mut x = offsets.get(c);
        if at_end {
            x += c.advance();
        }
        f64::from(x)
    };
    let last_line = || {
        layout
            .get(layout.len().saturating_sub(1))
            .map_or(0.0, |l| f64::from(l.metrics().offset))
    };
    match cursor.visual_clusters(layout) {
        [Some(left), Some(right)] => {
            if !left.is_end_of_line() {
                edge(&left, true)
            } else if left.is_soft_line_break() {
                let upstream = cursor.affinity() == Affinity::Upstream;
                if left.is_rtl() != upstream {
                    edge(&left, true)
                } else {
                    edge(&right, false)
                }
            } else {
                edge(&right, false)
            }
        }
        [Some(left), None] if left.is_hard_line_break() => last_line(),
        [Some(left), _] => edge(&left, true),
        [_, Some(right)] => edge(&right, false),
        _ => last_line(),
    }
}

/// Frame regions of a chain: each text frame's content box split into
/// columns, with the wrap obstacles over it. The layout flows into exactly
/// these values, and the cache key ([`crate::cache::StoryKey`]) compares
/// them, so the two cannot disagree.
pub(crate) fn regions_for(
    doc: &Document,
    index: &mut DocIndex,
    story: StoryId,
    frames: &[FrameId],
) -> Vec<Region> {
    let mut out = Vec::new();
    for fid in frames {
        let Some(f) = doc.frame(*fid) else { continue };
        if let Some(table) = f.as_table() {
            // A table cell: one region in cell-local space (ADR 0024).
            out.extend(
                index
                    .cell_index(*fid, table, story)
                    .map(|i| crate::table::cell_region(*fid, table, &table.cells[i])),
            );
            continue;
        }
        let Some(t) = f.as_text() else { continue };
        let Some(owner) = index.owner_of(*fid) else {
            continue;
        };
        let mut content = Rect::new(0.0, 0.0, f.rect.width, f.rect.height).inset(&t.insets);
        // BorderArt takes its width off the text area (ADR 0020).
        if let Some(b) = &f.style.border_art {
            content = content.inset(&pubrs_core::Margins::uniform(b.width.max(0.0)));
        }
        let cols = t.columns.max(1);
        let gap = t.column_gap.max(0.0);
        let cw = ((content.width - gap * f64::from(cols - 1)) / f64::from(cols)).max(1.0);
        let obstacles = obstacles_for(doc, index, owner, f);
        let page = match owner {
            Owner::Page(p) => index.page_index.get(&p).map(|i| *i as u32),
            Owner::Master(_) => None,
        };
        for c in 0..cols {
            out.push(Region {
                frame: *fid,
                x: content.x + f64::from(c) * (cw + gap),
                y: content.y,
                w: cw,
                h: content.height,
                valign: t.vertical_align,
                obstacles: obstacles.clone(),
                page,
            });
        }
    }
    // Page indices become ranks among the chain's pages.
    let mut pages: Vec<u32> = out.iter().filter_map(|r| r.page).collect();
    pages.sort_unstable();
    pages.dedup();
    for r in &mut out {
        r.page = r
            .page
            .and_then(|p| pages.binary_search(&p).ok().map(|i| i as u32));
    }
    out
}

/// Whether a frame's stories get a [`FrameText`]: text frames and tables.
pub(crate) fn shows_story_text(f: &Frame) -> bool {
    matches!(f.content, FrameContent::Text(_) | FrameContent::Table(_))
}

/// Frames above text frame `tf` on its page (or master) `owner` whose wrap
/// setting pushes text away.
fn obstacles_for(doc: &Document, index: &mut DocIndex, owner: Owner, tf: &Frame) -> Vec<Obstacle> {
    let story = tf.as_text().map(|t| t.story);
    let contours = index.contours.clone();
    let order = index.paint_order(doc, owner);
    let Some(pos) = order.iter().position(|f| *f == tf.id) else {
        return Vec::new();
    };
    order[pos + 1..]
        .iter()
        .filter_map(|id| doc.frame(*id))
        .filter(|f| f.wrap.mode != WrapMode::None)
        .filter(|f| f.as_text().map(|t| t.story) != story || story.is_none())
        .map(|f| {
            let margin = f.wrap.margin;
            let outline = matches!(f.wrap.mode, WrapMode::Tight | WrapMode::Through)
                .then(|| outline_in(tf, f, contours.as_deref()))
                .flatten();
            let rect = match outline.as_ref().and_then(|p| wrap::poly_bounds(p)) {
                Some(b) => b.outset(margin),
                None => obstacle_bounds_in(tf, f)
                    .outset(margin)
                    .translate(-tf.rect.x, -tf.rect.y),
            };
            Obstacle {
                rect,
                mode: f.wrap.mode,
                both_sides: f.wrap.both_sides,
                outline: outline.map(|p| (Outline(Arc::new(p)), margin)),
            }
        })
        .collect()
}

/// The outline of `obstacle` as polygons in the text frame's local space:
/// its shape, or the visible pixels of its image, rotated into place and
/// through the inverse of the text frame's rotation (as
/// [`obstacle_bounds_in`] does for the bounding box). `None` for frames
/// without an outline of their own and for images with no contour: they
/// wrap by their bounding box.
///
/// The polygons are not offset by the wrap margin; [`band_intervals`] grows
/// each covered span by it (a square offset, so the clearance is never less
/// than the margin).
fn outline_in(
    tf: &Frame,
    obstacle: &Frame,
    contours: Option<&dyn AlphaContours>,
) -> Option<Vec<Vec<Point>>> {
    let (w, h) = (obstacle.rect.width, obstacle.rect.height);
    let local: Vec<Vec<Point>> = match &obstacle.content {
        FrameContent::Shape(s) => vec![match s.shape {
            Shape::Rectangle { corner_radius } => wrap::rounded_rect(w, h, corner_radius),
            Shape::Ellipse => wrap::ellipse(w, h),
            Shape::Line { direction } => {
                let (a, b) = match direction {
                    LineDirection::DownRight => (Point::new(0.0, 0.0), Point::new(w, h)),
                    LineDirection::UpRight => (Point::new(0.0, h), Point::new(w, 0.0)),
                };
                let width = obstacle.style.stroke.as_ref().map_or(0.0, |s| s.width);
                wrap::stroked_line(a, b, width)
            }
        }],
        FrameContent::Image(img) => {
            let c = contours?.contour(img.asset?)?;
            let (t, clip) = pubrs_core::image_placement(
                f64::from(c.width),
                f64::from(c.height),
                img.crop,
                img.fit,
                w,
                h,
            );
            let polys: Vec<Vec<Point>> = c
                .polys
                .iter()
                .map(|poly| {
                    let mapped: Vec<Point> = poly
                        .iter()
                        .map(|p| {
                            Point::new(
                                t[0] * p.x + t[2] * p.y + t[4],
                                t[1] * p.x + t[3] * p.y + t[5],
                            )
                        })
                        .collect();
                    wrap::clip_to_rect(&mapped, Rect::new(clip.x, clip.y, clip.width, clip.height))
                })
                .filter(|p| p.len() >= 3)
                .collect();
            if polys.is_empty() {
                return None;
            }
            polys
        }
        _ => return None,
    };
    let (oc, tc) = (obstacle.rect.center(), tf.rect.center());
    let upright = tf.rotation % 360.0 == 0.0;
    Some(
        local
            .into_iter()
            .map(|poly| {
                poly.into_iter()
                    .map(|p| {
                        let mut q = Point::new(obstacle.rect.x + p.x, obstacle.rect.y + p.y)
                            .rotated_about(oc, obstacle.rotation);
                        if !upright {
                            q = q.rotated_about(tc, -tf.rotation);
                        }
                        Point::new(q.x - tf.rect.x, q.y - tf.rect.y)
                    })
                    .collect()
            })
            .collect(),
    )
}

/// The obstacle's page-space outline mapped into the text frame's unrotated
/// space (inverse of the text frame's rotation about its centre), as an AABB.
fn obstacle_bounds_in(tf: &Frame, obstacle: &Frame) -> Rect {
    if tf.rotation % 360.0 == 0.0 {
        return obstacle.bounds();
    }
    let centre = tf.rect.center();
    let mut x0 = f64::INFINITY;
    let mut y0 = f64::INFINITY;
    let mut x1 = f64::NEG_INFINITY;
    let mut y1 = f64::NEG_INFINITY;
    for p in obstacle.rect.rotated_corners(obstacle.rotation) {
        let q = p.rotated_about(centre, -tf.rotation);
        x0 = x0.min(q.x);
        y0 = y0.min(q.y);
        x1 = x1.max(q.x);
        y1 = y1.max(q.y);
    }
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

/// Shift lines down in regions whose frame is middle- or bottom-aligned.
fn apply_vertical_align(regions: &[Region], placed: &mut [PlacedLine]) {
    for (ri, r) in regions.iter().enumerate() {
        let factor = match r.valign {
            VerticalAlign::Top => continue,
            VerticalAlign::Middle => 0.5,
            VerticalAlign::Bottom => 1.0,
        };
        // A drop cap on a paragraph shorter than the cap sits on a baseline
        // below its lines; its first glyph's baseline counts too. (Other
        // lines' baselines are inside their boxes, so nothing changes.)
        let bottom = placed
            .iter()
            .filter(|p| p.region == ri)
            .map(|p| {
                let cap = p.line.runs.first().and_then(|r| r.glyphs.first());
                cap.map_or(p.line.rect.bottom(), |g| p.line.rect.bottom().max(g.y))
            })
            .fold(f64::NEG_INFINITY, f64::max);
        if !bottom.is_finite() {
            continue;
        }
        let dy = (r.bottom() - bottom).max(0.0) * factor;
        if dy == 0.0 {
            continue;
        }
        for p in placed.iter_mut().filter(|p| p.region == ri) {
            let l = &mut p.line;
            l.rect.y += dy;
            l.baseline += dy;
            for run in &mut l.runs {
                for g in &mut run.glyphs {
                    g.y += dy;
                }
            }
            for d in &mut l.decorations {
                d.rect.y += dy;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// `caret_x` must give exactly what parley's `Cursor::geometry` gives, for
    /// every boundary and both affinities, in wrapped, aligned, mixed-style,
    /// combining-mark and right-to-left text.
    #[test]
    fn caret_x_matches_parley_cursor_geometry() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fonts");
        let mut lay = Layouter::new(FontLibrary::from_dir(&dir).expect("test fonts"));
        let texts = [
            "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor.",
            "Cafe\u{301} na\u{ef}ve  double  spaces and a verylongwordthatmustbreakacrosslines.",
            "Mixed \u{5e9}\u{5dc}\u{5d5}\u{5dd} \u{5e2}\u{5d5}\u{5dc}\u{5dd} and English, then \u{627}\u{644}\u{639}\u{631}\u{628}\u{64a}\u{629} text.",
            "",
            "x",
        ];
        let aligns = [
            Alignment::Left,
            Alignment::Center,
            Alignment::Right,
            Alignment::Justify,
        ];
        let mut checked = 0;
        for text in texts {
            for align in aligns {
                let mut builder = lay.lcx.ranged_builder(&mut lay.fonts.fcx, text, 1.0, false);
                builder.push_default(StyleProperty::FontSize(12.0));
                let at = |n: usize| text.char_indices().nth(n).map_or(text.len(), |(i, _)| i);
                if text.len() > 12 {
                    builder.push(StyleProperty::FontWeight(FontWeight::BOLD), at(6)..at(12));
                    builder.push(StyleProperty::FontSize(17.0), at(9)..at(14));
                }
                let mut layout: Layout<Ink> = builder.build(text);
                layout.break_all_lines(Some(110.0));
                layout.align(align, AlignmentOptions::default());
                let offsets = ClusterOffsets::new(&layout, text.len());
                let bounds = text.char_indices().map(|(i, _)| i).chain([text.len()]);
                for b in bounds {
                    for affinity in [Affinity::Downstream, Affinity::Upstream] {
                        let cursor = Cursor::from_byte_index(&layout, b, affinity);
                        let want = cursor.geometry(&layout, 1.0).x0;
                        let got = caret_x(&layout, &offsets, cursor);
                        assert_eq!(
                            got.to_bits(),
                            want.to_bits(),
                            "{text:?} {align:?} byte {b} {affinity:?}: {got} != {want}"
                        );
                        checked += 1;
                    }
                }
            }
        }
        assert!(checked > 500);
    }

    #[test]
    fn char_counter_counts_forward_and_back() {
        let text = "a\u{e9}\u{5d0}b";
        let mut c = CharCounter::new(text);
        assert_eq!(c.at(0), 0);
        assert_eq!(c.at(1), 1);
        assert_eq!(c.at(5), 3);
        assert_eq!(c.at(3), 2);
        assert_eq!(c.at(99), 4);
    }
}

//! Story flow: paragraphs are shaped with parley one paragraph at a time
//! and broken line by line into the chain's regions (frames × columns),
//! honouring text wrap around other frames. Text that does not fit is
//! reported as overflow.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use parley::{
    Affinity, Alignment, AlignmentOptions, Cluster, Cursor, FontFamily, FontStyle, FontWeight,
    Layout, LayoutContext, LineHeight, PositionedLayoutItem, StyleProperty, YieldData,
};
use pubrs_core::{
    Align, CharAttrs, Document, Frame, FrameContent, FrameId, Ink, Owner, Paragraph, Parent, Rect,
    Run, StoryId, VerticalAlign, WrapMode,
};

use crate::fonts::FontLibrary;
use crate::hyphen::{self, BreakParams, LineCx, LineInfo, Shadow};
use crate::types::{
    Decoration, DocumentLayout, FontHandle, FrameText, GlyphRun, LineBox, Overflow,
    PositionedGlyph, StoryLayout,
};

/// Shapes and flows text. Holds the font library and parley's caches.
pub struct Layouter {
    pub fonts: FontLibrary,
    lcx: LayoutContext<Ink>,
}

impl std::fmt::Debug for Layouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Layouter")
            .field("fonts", &self.fonts)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Obstacle {
    rect: Rect,
    mode: WrapMode,
}

/// A rectangle text can flow into: one column of one frame (frame-local).
/// With the story and the styles, the regions are all a story's flow reads
/// from the document, so they are part of the cache key
/// ([`crate::cache::StoryKey`]).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Region {
    frame: FrameId,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    valign: VerticalAlign,
    obstacles: Vec<Obstacle>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Avail {
    /// Usable x and width for the line.
    Span(f64, f64),
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
        for o in &self.obstacles {
            let r = o.rect;
            if r.top() >= y1 || r.bottom() <= y0 || r.right() <= left || r.left() >= right {
                continue;
            }
            match o.mode {
                WrapMode::TopBottom => {
                    skip_to = Some(skip_to.map_or(r.bottom(), |s: f64| s.max(r.bottom())));
                }
                WrapMode::Square => blocked.push((r.left(), r.right())),
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
            let next = self
                .obstacles
                .iter()
                .filter(|o| o.rect.top() < y1 && o.rect.bottom() > y0)
                .map(|o| o.rect.bottom())
                .fold(f64::INFINITY, f64::min);
            Avail::Skip(next)
        }
    }
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
}

impl DocIndex {
    pub(crate) fn new(doc: &Document) -> Self {
        let mut parent = HashMap::with_capacity(doc.frames.len());
        for p in doc.pages.iter() {
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
        }
    }

    /// Lay out every story from scratch. Interactive callers use
    /// [`crate::cache::StoryLayoutCache`], which gives the same result and
    /// recomposes only the stories whose inputs changed.
    pub fn layout_document(&mut self, doc: &Document) -> DocumentLayout {
        let mut index = DocIndex::new(doc);
        let mut out = DocumentLayout::default();
        for (id, story) in &doc.stories {
            let regions = regions_for(doc, &mut index, &story.frames);
            out.stories
                .insert(*id, Arc::new(self.flow_story(doc, *id, &regions)));
        }
        out
    }

    /// Lay out one story across its frame chain.
    pub fn layout_story(&mut self, doc: &Document, story_id: StoryId) -> StoryLayout {
        let regions = match doc.story(story_id) {
            Some(story) => regions_for(doc, &mut DocIndex::new(doc), &story.frames),
            None => Vec::new(),
        };
        self.flow_story(doc, story_id, &regions)
    }

    /// Flow one story into `regions` (from [`regions_for`] over its chain).
    pub(crate) fn flow_story(
        &mut self,
        doc: &Document,
        story_id: StoryId,
        regions: &[Region],
    ) -> StoryLayout {
        let mut out = StoryLayout {
            story: Some(story_id),
            ..Default::default()
        };
        let Some(story) = doc.story(story_id) else {
            return out;
        };
        for f in &story.frames {
            if doc.frame(*f).and_then(|f| f.as_text()).is_some() {
                out.frames.insert(*f, FrameText::default());
            }
        }
        let mut placed: Vec<PlacedLine> = Vec::new();
        let mut cur = FlowCursor {
            region: 0,
            y: regions.first().map_or(0.0, |r| r.y),
        };
        let mut para_start = 0usize;
        let last = story.paragraphs.len().saturating_sub(1);
        for (pi, para) in story.paragraphs.iter().enumerate() {
            let overflow = if cur.region >= regions.len() {
                Some(para_start)
            } else {
                self.flow_paragraph(
                    doc,
                    para,
                    para_start,
                    regions,
                    &mut cur,
                    &mut placed,
                    &mut out,
                    pi == last,
                )
            };
            if let Some(offset) = overflow {
                out.overflow = Some(Overflow {
                    offset,
                    hidden_chars: story.char_len().saturating_sub(offset),
                });
                break;
            }
            para_start += para.char_len() + 1;
        }
        apply_vertical_align(regions, &mut placed);
        for p in placed {
            let frame = regions[p.region].frame;
            out.missing_glyphs += p
                .line
                .runs
                .iter()
                .flat_map(|r| r.glyphs.iter())
                .filter(|g| g.id == 0)
                .count();
            if let Some(ft) = out.frames.get_mut(&frame) {
                if ft.lines.is_empty() {
                    ft.chars = p.line.chars.clone();
                } else {
                    ft.chars.end = p.line.chars.end;
                }
                ft.lines.push(p.line);
            }
        }
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn flow_paragraph(
        &mut self,
        doc: &Document,
        para: &Paragraph,
        para_start: usize,
        regions: &[Region],
        cur: &mut FlowCursor,
        placed: &mut Vec<PlacedLine>,
        out: &mut StoryLayout,
        is_last_paragraph: bool,
    ) -> Option<usize> {
        let pa = doc.styles.resolve_para(para);
        let orig: String = para.text();
        // `text` is what parley shapes: the paragraph with soft hyphens
        // inserted at hyphenation points (ADR 0014). Story offsets are
        // always counted on `orig`.
        let hy_lang = hyphen::lang_of(&pa.language).filter(|_| pa.hyphenate);
        // If the line analysis cannot follow the paragraph (right-to-left
        // text, a line end it cannot place), it is laid out again once
        // without the inserted soft hyphens (ADR 0014).
        let cur0 = *cur;
        let mut plain = hy_lang.is_none();
        let (shadow, mut layout, mut lcx, line_infos, placements, overflowed) = 'attempt: loop {
            *cur = cur0;
            let shadow = match hy_lang.filter(|_| !plain) {
                Some(lang) => Shadow::hyphenated(&orig, lang),
                None => Shadow::plain(&orig),
            };
            let text: &str = &shadow.text;
            let mut spans: Vec<(Range<usize>, CharAttrs)> = Vec::new();
            let mut b = 0;
            for r in &para.runs {
                let attrs = doc.styles.resolve_char(para, r);
                spans.push((
                    shadow.from_orig(b)..shadow.from_orig(b + r.text.len()),
                    attrs,
                ));
                b += r.text.len();
            }
            let first = spans
                .first()
                .map(|s| s.1.clone())
                .unwrap_or_else(|| doc.styles.resolve_char(para, &Run::default()));
            for (_, a) in &spans {
                if !self.fonts.ensure_family(&a.font_family) {
                    out.missing_fonts.insert(a.font_family.clone());
                }
            }
            let mut builder = self
                .lcx
                .ranged_builder(&mut self.fonts.fcx, text, 1.0, false);
            builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
                pa.line_spacing as f32,
            )));
            push_attrs(&mut builder, None, &first);
            for (range, a) in &spans {
                if !range.is_empty() && *a != first {
                    push_attrs(&mut builder, Some(range.clone()), a);
                }
            }
            let mut layout: Layout<Ink> = builder.build(text);
            let justified = pa.align == Align::Justify;
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
            {
                let mut bl = layout.break_lines();
                bl.state_mut().set_layout_max_advance(max_w);
                let mut first_line = true;
                let mut est_h = (first.font_size * pa.line_spacing).max(1.0);
                let mut retried = false;
                let mut guard = 0u32;
                loop {
                    guard += 1;
                    if guard > 100_000 {
                        overflowed = true;
                        break;
                    }
                    let Some(r) = regions.get(cur.region) else {
                        overflowed = true;
                        break;
                    };
                    let mut y = cur.y;
                    if first_line && y > r.y + 1e-6 {
                        y += pa.space_before;
                    }
                    let (ax, aw) = match r.available(y, y + est_h, min_w) {
                        Avail::Span(x, w) => (x, w),
                        Avail::Skip(ny) => {
                            cur.y = ny.max(cur.y + 0.5);
                            if cur.y >= r.bottom() {
                                advance_region(cur, regions);
                            }
                            continue;
                        }
                    };
                    let indent = pa.left_indent
                        + if first_line {
                            pa.first_line_indent
                        } else {
                            0.0
                        };
                    let x = ax + indent.max(0.0);
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
                    let Some(data) = next else { break };
                    let YieldData::LineBreak(d) = data else {
                        continue;
                    };
                    let h = d.line_y_end - d.line_y_start;
                    if h > est_h + 0.01
                        && !retried
                        && !r.obstacles.is_empty()
                        && r.available(y, y + h, min_w) != Avail::Span(ax, aw)
                    {
                        bl.revert();
                        est_h = h;
                        retried = true;
                        continue;
                    }
                    retried = false;
                    if y + h > r.bottom() + 0.01 {
                        bl.revert();
                        advance_region(cur, regions);
                        continue;
                    }
                    if let Some(c) = lcx.as_mut() {
                        let info = c.pending.take();
                        if let Some(i) = &info {
                            c.pos = i.end;
                        }
                        line_infos.push(info.unwrap_or_default());
                    }
                    placements.push((cur.region, y, h));
                    cur.y = y + h;
                    first_line = false;
                    est_h = h;
                }
                bl.finish();
            }
            if !plain && !shadow.inserted.is_empty() && lcx.as_ref().is_some_and(|c| !c.ok()) {
                plain = true;
                continue 'attempt;
            }
            break (shadow, layout, lcx, line_infos, placements, overflowed);
        };
        let text: &str = &shadow.text;
        layout.align(
            to_parley_align(pa.align),
            AlignmentOptions {
                align_when_overflowing: false,
                last_line_alignment: None,
            },
        );

        let text_arc: Arc<str> = Arc::from(orig.as_str());
        let n_lines = placements.len();
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
            let (c0, c1) = (
                counter.at(tb0) - shadow.inserted_before(tb0),
                counter.at(tb1) - shadow.inserted_before(tb1),
            );
            let info = line_infos.get(li).copied().unwrap_or_default();
            let letter = info.letter;
            let mut end_x = 0.0f64;
            let chars = para_start + c0..para_start + c1;
            let mut runs = Vec::new();
            let mut decorations = Vec::new();
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(gr) = item else {
                    continue;
                };
                let run = gr.run();
                let key = run.cluster_range().start;
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
                            text: shadow.to_orig(r.start)..shadow.to_orig(r.end),
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
                let (gy, o) = (last.glyphs.last().map_or(0.0, |g| g.y), shadow.to_orig(tb1));
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
            let carets = caret_positions(
                &layout,
                &offsets,
                text,
                &shadow,
                tb0..tb1,
                para_start + c0,
                letter,
            );
            let ends_paragraph = li + 1 == n_lines && !overflowed;
            placed.push(PlacedLine {
                region,
                line: LineBox {
                    rect: Rect::new(
                        f64::from(m.inline_min_coord),
                        y,
                        info.full_w.map_or(
                            f64::from(m.inline_max_coord - m.inline_min_coord),
                            f64::from,
                        ),
                        h,
                    ),
                    baseline: f64::from(m.baseline),
                    chars,
                    runs,
                    decorations,
                    carets,
                    ends_paragraph,
                },
            });
        }
        if overflowed {
            let byte = if n_lines == 0 { 0 } else { last_end_byte };
            return Some(para_start + counter.at(byte) - shadow.inserted_before(byte));
        }
        cur.y += pa.space_after;
        let _ = is_last_paragraph;
        None
    }
}

fn advance_region(cur: &mut FlowCursor, regions: &[Region]) {
    cur.region += 1;
    cur.y = regions.get(cur.region).map_or(0.0, |r| r.y);
}

fn push_attrs(b: &mut parley::RangedBuilder<'_, Ink>, range: Option<Range<usize>>, a: &CharAttrs) {
    let props: [StyleProperty<'_, Ink>; 7] = [
        StyleProperty::FontFamily(FontFamily::named(&a.font_family)),
        StyleProperty::FontSize(a.font_size as f32),
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
    for p in props {
        match &range {
            Some(r) => b.push(p, r.clone()),
            None => b.push_default(p),
        }
    }
}

/// Caret x for each character boundary of the line whose paragraph bytes
/// are `bytes`; `first` is the story offset of its first character.
/// `text` is the shadow text: boundaries at an inserted soft hyphen are
/// skipped (it is not a story character), and `letter` is added per
/// character for justified lines stretched with letter spacing.
fn caret_positions(
    layout: &Layout<Ink>,
    offsets: &ClusterOffsets,
    text: &str,
    shadow: &Shadow,
    bytes: Range<usize>,
    first: usize,
    letter: f64,
) -> Vec<(usize, f64)> {
    let (b0, b1) = (bytes.start, bytes.end);
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
            caret_x(layout, offsets, cursor) + k as f64 * letter,
        ));
        k += 1;
    }
    out
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
pub(crate) fn regions_for(doc: &Document, index: &mut DocIndex, frames: &[FrameId]) -> Vec<Region> {
    let mut out = Vec::new();
    for fid in frames {
        let Some(f) = doc.frame(*fid) else { continue };
        let Some(t) = f.as_text() else { continue };
        let Some(owner) = index.owner_of(*fid) else {
            continue;
        };
        let content = Rect::new(0.0, 0.0, f.rect.width, f.rect.height).inset(&t.insets);
        let cols = t.columns.max(1);
        let gap = t.column_gap.max(0.0);
        let cw = ((content.width - gap * f64::from(cols - 1)) / f64::from(cols)).max(1.0);
        let obstacles = obstacles_for(doc, index, owner, f);
        for c in 0..cols {
            out.push(Region {
                frame: *fid,
                x: content.x + f64::from(c) * (cw + gap),
                y: content.y,
                w: cw,
                h: content.height,
                valign: t.vertical_align,
                obstacles: obstacles.clone(),
            });
        }
    }
    out
}

/// Frames above text frame `tf` on its page (or master) `owner` whose wrap
/// setting pushes text away.
fn obstacles_for(doc: &Document, index: &mut DocIndex, owner: Owner, tf: &Frame) -> Vec<Obstacle> {
    let story = tf.as_text().map(|t| t.story);
    let order = index.paint_order(doc, owner);
    let Some(pos) = order.iter().position(|f| *f == tf.id) else {
        return Vec::new();
    };
    order[pos + 1..]
        .iter()
        .filter_map(|id| doc.frame(*id))
        .filter(|f| f.wrap.mode != WrapMode::None)
        .filter(|f| f.as_text().map(|t| t.story) != story || story.is_none())
        .map(|f| Obstacle {
            rect: obstacle_bounds_in(tf, f)
                .outset(f.wrap.margin)
                .translate(-tf.rect.x, -tf.rect.y),
            mode: f.wrap.mode,
        })
        .collect()
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
        let bottom = placed
            .iter()
            .filter(|p| p.region == ri)
            .map(|p| p.line.rect.bottom())
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

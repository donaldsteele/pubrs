//! Story flow: paragraphs are shaped with parley one paragraph at a time
//! and broken line by line into the chain's regions (frames × columns),
//! honouring text wrap around other frames. Text that does not fit is
//! reported as overflow.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use parley::{
    Affinity, Alignment, AlignmentOptions, Cursor, FontFamily, FontStyle, FontWeight, Layout,
    LayoutContext, LineHeight, PositionedLayoutItem, StyleProperty, YieldData,
};
use pubrs_core::{
    Align, CharAttrs, Color, Document, FrameId, Paragraph, Rect, Run, StoryId, VerticalAlign,
    WrapMode,
};

use crate::fonts::FontLibrary;
use crate::types::{
    Decoration, DocumentLayout, FontHandle, FrameText, GlyphRun, LineBox, Overflow,
    PositionedGlyph, StoryLayout,
};

/// Shapes and flows text. Holds the font library and parley's caches.
pub struct Layouter {
    pub fonts: FontLibrary,
    lcx: LayoutContext<Color>,
}

impl std::fmt::Debug for Layouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Layouter")
            .field("fonts", &self.fonts)
            .finish()
    }
}

#[derive(Debug, Clone, Copy)]
struct Obstacle {
    rect: Rect,
    mode: WrapMode,
}

/// A rectangle text can flow into: one column of one frame (frame-local).
#[derive(Debug, Clone)]
struct Region {
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

fn byte_to_char(text: &str, byte: usize) -> usize {
    let b = byte.min(text.len());
    text[..b].chars().count()
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

    /// Lay out every story.
    pub fn layout_document(&mut self, doc: &Document) -> DocumentLayout {
        let ids: Vec<StoryId> = doc.stories.keys().copied().collect();
        let mut out = DocumentLayout::default();
        for id in ids {
            out.stories.insert(id, self.layout_story(doc, id));
        }
        out
    }

    /// Lay out one story across its frame chain.
    pub fn layout_story(&mut self, doc: &Document, story_id: StoryId) -> StoryLayout {
        let mut out = StoryLayout {
            story: Some(story_id),
            ..Default::default()
        };
        let Some(story) = doc.story(story_id) else {
            return out;
        };
        let regions = regions_for(doc, &story.frames);
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
                    &regions,
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
        apply_vertical_align(&regions, &mut placed);
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
        let text: String = para.text();
        let mut spans: Vec<(Range<usize>, CharAttrs)> = Vec::new();
        let mut b = 0;
        for r in &para.runs {
            let attrs = doc.styles.resolve_char(para, r);
            spans.push((b..b + r.text.len(), attrs));
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
            .ranged_builder(&mut self.fonts.fcx, &text, 1.0, false);
        builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
            pa.line_spacing as f32,
        )));
        push_attrs(&mut builder, None, &first);
        for (range, a) in &spans {
            if !range.is_empty() && *a != first {
                push_attrs(&mut builder, Some(range.clone()), a);
            }
        }
        let mut layout: Layout<Color> = builder.build(&text);

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
                let Some(data) = bl.break_next() else { break };
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
                placements.push((cur.region, y, h));
                cur.y = y + h;
                first_line = false;
                est_h = h;
            }
            bl.finish();
        }
        layout.align(
            to_parley_align(pa.align),
            AlignmentOptions {
                align_when_overflowing: false,
                last_line_alignment: None,
            },
        );

        let text_arc: Arc<str> = Arc::from(text.as_str());
        let n_lines = placements.len();
        let mut groups_cache: HashMap<usize, (Vec<ClusterGroup>, usize)> = HashMap::new();
        let mut last_end_byte = 0usize;
        for (li, line) in layout.lines().enumerate() {
            let Some(&(region, y, h)) = placements.get(li) else {
                break;
            };
            let m = line.metrics();
            let tr = line.text_range();
            let (tb0, tb1) = (tr.start.min(text.len()), tr.end.min(text.len()));
            last_end_byte = tb1;
            let chars =
                para_start + byte_to_char(&text, tb0)..para_start + byte_to_char(&text, tb1);
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
                        .map(|(g, text)| PositionedGlyph {
                            id: g.id,
                            x: f64::from(g.x),
                            y: f64::from(g.y),
                            advance: f64::from(g.advance),
                            text,
                        })
                        .collect(),
                });
                if let Some(ul) = &style.underline {
                    let fm = run.font_metrics();
                    let off = ul.offset.unwrap_or(fm.underline_offset);
                    let size = ul.size.unwrap_or(fm.underline_size).max(0.5);
                    decorations.push(Decoration {
                        rect: Rect::new(
                            f64::from(gr.offset()),
                            f64::from(gr.baseline() - off),
                            f64::from(gr.advance()),
                            f64::from(size),
                        ),
                        color: ul.brush,
                    });
                }
            }
            let carets = caret_positions(&layout, &text, tb0, tb1, para_start);
            let ends_paragraph = li + 1 == n_lines && !overflowed;
            placed.push(PlacedLine {
                region,
                line: LineBox {
                    rect: Rect::new(
                        f64::from(m.inline_min_coord),
                        y,
                        f64::from(m.inline_max_coord - m.inline_min_coord),
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
            return Some(para_start + byte_to_char(&text, byte));
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

fn push_attrs(
    b: &mut parley::RangedBuilder<'_, Color>,
    range: Option<Range<usize>>,
    a: &CharAttrs,
) {
    let props: [StyleProperty<'_, Color>; 7] = [
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

/// Caret x for each character boundary on a line.
fn caret_positions(
    layout: &Layout<Color>,
    text: &str,
    b0: usize,
    b1: usize,
    para_start: usize,
) -> Vec<(usize, f64)> {
    let mut out = Vec::new();
    let mut boundaries: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .filter(|i| *i >= b0 && *i < b1)
        .collect();
    boundaries.push(b1);
    let base = byte_to_char(text, b0);
    for (k, b) in boundaries.iter().enumerate() {
        let at_end = *b == b1 && *b != b0;
        let affinity = if at_end {
            Affinity::Upstream
        } else {
            Affinity::Downstream
        };
        let bb = Cursor::from_byte_index(layout, *b, affinity).geometry(layout, 1.0);
        out.push((para_start + base + k, bb.x0));
    }
    out
}

/// Frame regions of a chain: each text frame's content box split into columns.
fn regions_for(doc: &Document, frames: &[FrameId]) -> Vec<Region> {
    let mut out = Vec::new();
    for fid in frames {
        let Some(f) = doc.frame(*fid) else { continue };
        let Some(t) = f.as_text() else { continue };
        if doc.owner_of(*fid).is_none() {
            continue;
        }
        let content = Rect::new(0.0, 0.0, f.rect.width, f.rect.height).inset(&t.insets);
        let cols = t.columns.max(1);
        let gap = t.column_gap.max(0.0);
        let cw = ((content.width - gap * f64::from(cols - 1)) / f64::from(cols)).max(1.0);
        let obstacles = obstacles_for(doc, *fid);
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

/// Frames above `frame` on the same page whose wrap setting pushes text away.
fn obstacles_for(doc: &Document, frame: FrameId) -> Vec<Obstacle> {
    let Some(owner) = doc.owner_of(frame) else {
        return Vec::new();
    };
    let Some(tf) = doc.frame(frame) else {
        return Vec::new();
    };
    let story = tf.as_text().map(|t| t.story);
    let order = doc.paint_order(owner);
    let Some(pos) = order.iter().position(|f| *f == frame) else {
        return Vec::new();
    };
    order[pos + 1..]
        .iter()
        .filter_map(|id| doc.frame(*id))
        .filter(|f| f.wrap.mode != WrapMode::None)
        .filter(|f| f.as_text().map(|t| t.story) != story || story.is_none())
        .map(|f| Obstacle {
            rect: f
                .bounds()
                .outset(f.wrap.margin)
                .translate(-tf.rect.x, -tf.rect.y),
            mode: f.wrap.mode,
        })
        .collect()
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

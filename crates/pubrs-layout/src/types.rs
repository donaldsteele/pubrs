//! Layout results. All coordinates are in points, **frame-local**: relative
//! to the top-left of the frame's unrotated rect, y down. Renderers apply the
//! frame's position and rotation.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::sync::Arc;

use parley::fontique::Blob;
use pubrs_core::{FrameId, Ink, Rect, StoryId};

/// Font bytes and face index, shared with renderers and the PDF writer.
#[derive(Clone)]
pub struct FontHandle {
    pub data: Blob<u8>,
    pub index: u32,
}

impl FontHandle {
    pub fn bytes(&self) -> &[u8] {
        self.data.data()
    }

    /// Process-unique key for caching parsed fonts (not stable across runs).
    pub fn key(&self) -> (u64, u32) {
        (self.data.id(), self.index)
    }
}

impl std::fmt::Debug for FontHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "FontHandle({} bytes, face {})",
            self.data.len(),
            self.index
        )
    }
}

impl PartialEq for FontHandle {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}

/// One positioned glyph.
#[derive(Debug, Clone, PartialEq)]
pub struct PositionedGlyph {
    pub id: u32,
    /// Pen position on the baseline, frame-local.
    pub x: f64,
    pub y: f64,
    pub advance: f64,
    /// Source text of this glyph as a byte range into [`GlyphRun::text`]
    /// (empty for ligature continuations).
    pub text: Range<usize>,
}

/// Glyphs sharing one font, size and colour.
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphRun {
    pub font: FontHandle,
    pub font_size: f64,
    pub color: Ink,
    /// Embolden outlines (no bold face available).
    pub synthetic_bold: bool,
    /// Shear angle in degrees for synthetic italic.
    pub synthetic_skew: Option<f64>,
    /// The source text the glyphs map into (the paragraph's text).
    pub text: std::sync::Arc<str>,
    pub glyphs: Vec<PositionedGlyph>,
}

/// An underline or other filled decoration rectangle.
#[derive(Debug, Clone, PartialEq)]
pub struct Decoration {
    pub rect: Rect,
    pub color: Ink,
}

/// One line of text in a frame.
#[derive(Debug, Clone, PartialEq)]
pub struct LineBox {
    /// Line box (x and width of the available space; y and line height).
    pub rect: Rect,
    /// Baseline y, frame-local.
    pub baseline: f64,
    /// Story character offsets covered by this line (including trailing spaces).
    pub chars: Range<usize>,
    pub runs: Vec<GlyphRun>,
    pub decorations: Vec<Decoration>,
    /// Caret x position for every character offset in `chars` (and its end), ascending.
    pub carets: Vec<(usize, f64)>,
    /// For lines that can reorder (right-to-left paragraphs or characters
    /// only; empty otherwise): the box of the character at each caret, as
    /// offsets from the caret's x, `(left, right)`. A caret is the leading
    /// edge of the character after it, so a right-to-left character lies
    /// left of its caret. Parallel to `carets`; the last entry is empty.
    pub spans: Vec<(f32, f32)>,
    /// The paragraph runs right to left (parley's base direction).
    pub rtl: bool,
    /// True for the last line of a paragraph.
    pub ends_paragraph: bool,
    /// The bullet or number drawn in the hanging indent of a list item's
    /// first line (generated text, not part of the story).
    pub list_label: Option<String>,
}

impl LineBox {
    /// Caret x for a story offset on this line (nearest known position).
    pub fn caret_x(&self, offset: usize) -> f64 {
        match self.carets.binary_search_by_key(&offset, |c| c.0) {
            Ok(i) => self.carets[i].1,
            Err(i) => self
                .carets
                .get(i.min(self.carets.len().saturating_sub(1)))
                .map_or(self.rect.x, |c| c.1),
        }
    }

    /// Story offset nearest to frame-local `x` on this line. Carets need not
    /// rise with the offset (right-to-left text); the visually nearest wins,
    /// and of equal ones the earliest offset.
    pub fn offset_at_x(&self, x: f64) -> usize {
        self.carets
            .iter()
            .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
            .map_or(self.chars.start, |c| c.0)
    }

    /// True if characters of this line can be out of reading order (it has
    /// right-to-left characters or sits in a right-to-left paragraph).
    pub fn is_bidi(&self) -> bool {
        !self.spans.is_empty()
    }

    /// True if the line's paragraph runs right to left (stored by layout).
    pub fn is_rtl(&self) -> bool {
        self.rtl
    }

    /// Every place a caret can be drawn on a bidi line, as (offset, x). Each
    /// offset has its `carets` entry (the leading edge of the character
    /// after it) and, where the character before it lies the other way,
    /// a second place: that character's trailing edge. At the edge of a
    /// right-to-left word inside left-to-right text the two are far apart.
    /// Empty for lines in reading order, where `carets` is all there is.
    pub fn slots(&self) -> Vec<(usize, f64)> {
        if !self.is_bidi() {
            return Vec::new();
        }
        let mut v = Vec::with_capacity(self.carets.len() * 2);
        for (i, &(o, x)) in self.carets.iter().enumerate() {
            v.push((o, x));
            if i > 0 {
                let (px, (lo, hi)) = (self.carets[i - 1].1, self.spans[i - 1]);
                let t = px + f64::from(if lo < 0.0 { lo } else { hi });
                if (t - x).abs() > 0.01 {
                    v.push((o, t));
                }
            }
        }
        v
    }

    /// The caret place nearest to `x`: (offset, x). Equal ones: the
    /// earliest offset.
    pub fn slot_at_x(&self, x: f64) -> (usize, f64) {
        let slots = self.slots();
        let all: &[(usize, f64)] = if slots.is_empty() {
            &self.carets
        } else {
            &slots
        };
        all.iter()
            .copied()
            .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
            .unwrap_or((self.chars.start, self.rect.x))
    }

    /// Where `offset` is drawn: the place nearest `hint` (the x it was
    /// last put at, by an arrow key or a click), else its leading edge.
    pub fn slot_x(&self, offset: usize, hint: Option<f64>) -> f64 {
        let primary = self.caret_x(offset);
        let Some(h) = hint else { return primary };
        self.slots()
            .into_iter()
            .filter(|s| s.0 == offset)
            .map(|s| s.1)
            .min_by(|a, b| (a - h).abs().total_cmp(&(b - h).abs()))
            .filter(|x| (x - h).abs() < 0.5)
            .unwrap_or(primary)
    }

    /// The caret place one step left or right of (`offset`, `x`) by
    /// position on the line, not by order in the text. `None` at the
    /// line's visual end (the caller then moves to the next line).
    pub fn visual_step(&self, offset: usize, x: f64, left: bool) -> Option<(usize, f64)> {
        const EPS: f64 = 0.01;
        let mut best: Option<(usize, f64)> = None;
        for (o, cx) in self.slots() {
            let ahead = if left { cx < x - EPS } else { cx > x + EPS };
            if !ahead {
                continue;
            }
            let better = match best {
                None => true,
                Some((bo, bx)) => {
                    if (cx - bx).abs() <= EPS {
                        o.abs_diff(offset) < bo.abs_diff(offset)
                    } else if left {
                        cx > bx
                    } else {
                        cx < bx
                    }
                }
            };
            if better {
                best = Some((o, cx));
            }
        }
        best
    }

    /// The leftmost (or rightmost) caret place of a bidi line, not
    /// counting the offset after a soft-wrap space when `skip_end` (it is
    /// drawn on the next line). Of places at the same x, the earliest
    /// offset at the left edge and the latest at the right.
    pub fn visual_edge(&self, right: bool, skip_end: bool) -> (usize, f64) {
        const EPS: f64 = 0.01;
        let mut best: Option<(usize, f64)> = None;
        for (o, cx) in self.slots() {
            if skip_end && o == self.chars.end && self.chars.end > self.chars.start {
                continue;
            }
            best = match best {
                None => Some((o, cx)),
                Some((bo, bx)) => {
                    let beats = if right { cx > bx + EPS } else { cx < bx - EPS };
                    let ties = (cx - bx).abs() <= EPS;
                    if beats || (ties && (if right { o > bo } else { o < bo })) {
                        Some((o, cx))
                    } else {
                        Some((bo, bx))
                    }
                }
            };
        }
        best.unwrap_or((self.chars.start, self.caret_x(self.chars.start)))
    }

    /// Horizontal extents (left, right) of the characters `from..to` on
    /// this line, merged where they touch. On a line in reading order that
    /// is one extent from caret to caret; on a bidi line a selection can
    /// be several pieces.
    pub fn selection_spans(&self, from: usize, to: usize) -> Vec<(f64, f64)> {
        if !self.is_bidi() {
            return vec![(self.caret_x(from), self.caret_x(to))];
        }
        let mut v: Vec<(f64, f64)> = self
            .carets
            .iter()
            .zip(&self.spans)
            .filter(|((o, _), _)| *o >= from && *o < to)
            .map(|((_, x), (lo, hi))| (x + f64::from(*lo), x + f64::from(*hi)))
            .filter(|(a, b)| b > a)
            .collect();
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut out: Vec<(f64, f64)> = Vec::new();
        for (a, b) in v {
            match out.last_mut() {
                Some(last) if a <= last.1 + 0.5 => last.1 = last.1.max(b),
                _ => out.push((a, b)),
            }
        }
        out
    }
}

/// The text shown by one frame of a chain.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FrameText {
    pub lines: Vec<LineBox>,
    /// Story character range shown in this frame (empty if none).
    pub chars: Range<usize>,
}

/// Text that did not fit in the chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overflow {
    /// First story character offset that is not shown.
    pub offset: usize,
    /// Number of characters hidden.
    pub hidden_chars: usize,
}

/// Layout of one story across its frame chain.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StoryLayout {
    pub story: Option<StoryId>,
    /// Per frame of the chain.
    pub frames: BTreeMap<FrameId, FrameText>,
    pub overflow: Option<Overflow>,
    /// Families requested but not installed (drawn with the fallback font).
    pub missing_fonts: BTreeSet<String>,
    /// Characters no available font could draw.
    pub missing_glyphs: usize,
    /// Which characters those were (each once, in code point order).
    pub missing_chars: BTreeSet<char>,
}

/// Layout of every story in a document. Stories are shared (`Arc`) so a
/// layout assembled from the [`crate::cache::StoryLayoutCache`] costs a
/// pointer per story, not a copy of every glyph.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DocumentLayout {
    pub stories: BTreeMap<StoryId, Arc<StoryLayout>>,
    /// Shaped text of every WordArt frame.
    pub wordart: BTreeMap<FrameId, Arc<WordArtLayout>>,
    /// The table pass of every table frame (row geometry, cell rectangles).
    /// A cell's lines are in `stories[cell story].frames[table frame]`, in
    /// cell-local space: draw them at [`crate::TableLayout::origin`].
    pub tables: BTreeMap<FrameId, Arc<crate::table::TableLayout>>,
}

/// The shaped text of one WordArt frame: glyph pen positions on a single
/// baseline at y = 0, starting at x = 0. The warp is applied at render time.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WordArtLayout {
    pub runs: Vec<GlyphRun>,
    /// Advance width of the line in points.
    pub width: f64,
    /// The family was requested but is not installed (a fallback is used).
    pub missing_font: Option<String>,
}

impl DocumentLayout {
    /// The text laid out in a frame, if it is a text frame.
    pub fn frame(&self, frame: FrameId) -> Option<&FrameText> {
        self.stories.values().find_map(|s| s.frames.get(&frame))
    }

    /// The text of one cell of a table (lines in cell-local space).
    pub fn cell_text(&self, frame: FrameId, story: StoryId) -> Option<&FrameText> {
        self.stories.get(&story)?.frames.get(&frame)
    }

    /// The table pass of a table frame.
    pub fn table(&self, frame: FrameId) -> Option<&Arc<crate::table::TableLayout>> {
        self.tables.get(&frame)
    }

    /// Table-local position of a cell's cell-local origin (its top-left
    /// corner plus the vertical alignment shift).
    pub fn cell_origin(&self, frame: FrameId, story: StoryId) -> Option<pubrs_core::Point> {
        self.table(frame)?
            .origin(story)
            .map(|(x, y)| pubrs_core::Point::new(x, y))
    }

    /// The shaped text of a WordArt frame.
    pub fn wordart(&self, frame: FrameId) -> Option<&Arc<WordArtLayout>> {
        self.wordart.get(&frame)
    }

    pub fn missing_fonts(&self) -> BTreeSet<String> {
        self.stories
            .values()
            .flat_map(|s| s.missing_fonts.iter().cloned())
            .chain(self.wordart.values().filter_map(|w| w.missing_font.clone()))
            .collect()
    }
}

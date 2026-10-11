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

    /// Story offset nearest to frame-local `x` on this line.
    pub fn offset_at_x(&self, x: f64) -> usize {
        self.carets
            .iter()
            .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
            .map_or(self.chars.start, |c| c.0)
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

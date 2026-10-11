//! `pubrs-layout`: text shaping and flow.
//!
//! [`Layouter`] turns the stories of a [`pubrs_core::Document`] into
//! positioned glyph runs, one [`FrameText`] per frame of each linked chain,
//! reporting overflow and missing fonts. Shaping uses parley; fonts come from
//! a [`FontLibrary`] (bundled OFL fonts plus optional system fonts).
//! [`StoryLayoutCache`] keeps per-story results between edits and recomposes
//! only the stories whose inputs changed.

pub mod cache;
pub mod flow;
pub mod fonts;
pub mod types;

pub use cache::{CacheStats, StoryLayoutCache};
pub use flow::Layouter;
pub use fonts::{FALLBACK_SANS, FALLBACK_SERIF, FamilyEntry, FontLibrary, FontSource};
pub use types::{
    Decoration, DocumentLayout, FontHandle, FrameText, GlyphRun, LineBox, Overflow,
    PositionedGlyph, StoryLayout,
};

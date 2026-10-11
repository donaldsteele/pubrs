//! `pubrs-core`: the document model, commands, undo/redo and validation.
//!
//! This crate has no I/O, no UI and no rendering dependencies, and compiles
//! with `--no-default-features`. Every mutation of a [`Document`] is a
//! [`Command`]; the [`Editor`] applies commands atomically and records each
//! as a forward/backward [`Patch`] pair for undo and redo.

pub mod align;
pub mod builder;
pub mod bytes;
pub mod color;
pub mod command;
pub mod geom;
pub mod history;
pub mod id;
pub mod merge;
pub mod model;
pub mod patch;
pub mod snap;
pub mod style;
pub mod swatch;
pub mod table;
pub mod text;
pub mod validate;

pub use builder::DocBuilder;
pub use bytes::Bytes;
pub use color::Color;
pub use command::{
    CmdResult, Command, CommandError, TextFrameProps, apply_list_commands, clear_list_commands,
    list_level_command,
};
pub use geom::{Margins, Point, Rect, Size};
pub use history::{Change, Editor, HistoryItem};
#[cfg(feature = "random-ids")]
pub use id::RandomIds;
pub use id::{
    AssetId, DocumentId, FrameId, GuideId, IdSource, MasterId, PageId, SeqIds, StoryId, StyleId,
    SwatchId,
};
pub use model::*;
pub use patch::{Patch, Touched};
pub use style::{
    Align, Baseline, Caps, CharAttrs, CharProps, JustifyLimits, ParaAttrs, ParaProps, Style,
    StyleKind, StyleSheet,
};
pub use swatch::{
    Cmyk, GradientKind, GradientStop, Ink, InkSpec, Paint, ResolvedGradient, ResolvedInk,
    ResolvedPaint, Scheme, SchemeId, SchemeSlot, SolidPaint, Swatch, SwatchValue,
};
pub use text::RunFormat;
pub use validate::{Warning, WarningCode};

//! `pubrs-io`: the `.pubrs` file format, schema migration, image import and
//! decoding, templates, spell checking and export orchestration.

pub mod assets;
pub mod error;
pub mod export;
pub mod format;
pub mod images;
pub mod merge_source;
pub mod migrate;
pub mod presets;
pub mod pubfile;
pub mod spell;
pub mod templates;

pub use assets::{
    ImageInfo, PASTE_MAX_SIDE, PastedPng, crop_png, image_info, import_image, rgba_to_png,
    sniff_format,
};
pub use error::IoError;
pub use export::{ExportContext, ExportError};
pub use format::{MIMETYPE, load, read_bytes, save, write_bytes};
pub use images::{ContourStore, ImageCache};
pub use presets::{SheetPreset, sheet_preset, sheet_presets};

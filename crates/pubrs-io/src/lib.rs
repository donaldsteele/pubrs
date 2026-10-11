//! `pubrs-io`: the `.pubrs` file format, schema migration, image import and
//! decoding, templates and export orchestration.

pub mod assets;
pub mod error;
pub mod export;
pub mod format;
pub mod images;
pub mod migrate;
pub mod presets;
pub mod pubfile;
pub mod templates;

pub use assets::{ImageInfo, image_info, import_image, sniff_format};
pub use error::IoError;
pub use export::{ExportContext, ExportError};
pub use format::{MIMETYPE, load, read_bytes, save, write_bytes};
pub use images::ImageCache;
pub use presets::{SheetPreset, sheet_preset, sheet_presets};

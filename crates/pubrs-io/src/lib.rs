//! `pubrs-io`: the `.pubrs` file format, schema migration, image import,
//! templates and export orchestration.

pub mod assets;
pub mod error;
pub mod format;
pub mod migrate;

pub use assets::{ImageInfo, image_info, import_image, sniff_format};
pub use error::IoError;
pub use format::{MIMETYPE, load, read_bytes, save, write_bytes};

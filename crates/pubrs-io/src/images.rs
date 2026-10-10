//! Decoding embedded images for renderers.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, OnceLock};

use image::ImageDecoder;
use pubrs_core::{AssetId, Document, ImageFormat};
use pubrs_render::{ImageContent, ImageResolver, RasterImage, pixmap_from_rgba};

use crate::error::IoError;

/// usvg options whose font database holds the bundled fonts, so text in
/// SVGs renders identically everywhere.
pub fn svg_options() -> &'static usvg::Options<'static> {
    static OPTS: OnceLock<usvg::Options<'static>> = OnceLock::new();
    OPTS.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        for (_, data) in pubrs_layout::fonts::BUNDLED_FONTS {
            db.load_font_data(data.to_vec());
        }
        db.set_sans_serif_family(pubrs_layout::FALLBACK_SANS);
        db.set_serif_family(pubrs_layout::FALLBACK_SERIF);
        let mut opts = usvg::Options {
            font_family: pubrs_layout::FALLBACK_SANS.to_string(),
            ..Default::default()
        };
        opts.fontdb = Arc::new(db);
        opts
    })
}

/// Decode a PNG or JPEG, applying EXIF orientation.
/// Returns straight RGBA8 pixels and whether the orientation changed them.
pub fn decode_raster(bytes: &[u8]) -> Result<(image::RgbaImage, bool), IoError> {
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| IoError::Image(e.to_string()))?;
    let mut decoder = reader
        .into_decoder()
        .map_err(|e| IoError::Image(e.to_string()))?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img =
        image::DynamicImage::from_decoder(decoder).map_err(|e| IoError::Image(e.to_string()))?;
    let rotated = orientation != image::metadata::Orientation::NoTransforms;
    if rotated {
        img.apply_orientation(orientation);
    }
    Ok((img.to_rgba8(), rotated))
}

/// Decode an asset's bytes into drawable content.
pub fn decode_asset(format: ImageFormat, bytes: &Arc<[u8]>) -> Result<ImageContent, IoError> {
    match format {
        ImageFormat::Svg => {
            let tree = usvg::Tree::from_data(bytes, svg_options())
                .map_err(|e| IoError::Image(e.to_string()))?;
            Ok(ImageContent::Svg(Arc::new(tree)))
        }
        ImageFormat::Png | ImageFormat::Jpeg => {
            let (rgba, rotated) = decode_raster(bytes)?;
            let (w, h) = rgba.dimensions();
            let pixmap = pixmap_from_rgba(w, h, rgba.as_raw())
                .ok_or_else(|| IoError::Image("image too large".into()))?;
            Ok(ImageContent::Raster(Arc::new(RasterImage {
                width: w,
                height: h,
                pixmap,
                encoded: bytes.clone(),
                format,
                passthrough: !rotated,
            })))
        }
    }
}

/// Decoded images of a document, keyed by asset id. Asset bytes never change
/// for a given id, so entries stay valid until the asset is removed.
#[derive(Default)]
pub struct ImageCache {
    entries: HashMap<AssetId, ImageContent>,
    failed: HashMap<AssetId, String>,
}

impl std::fmt::Debug for ImageCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageCache")
            .field("decoded", &self.entries.len())
            .field("failed", &self.failed.len())
            .finish()
    }
}

impl ImageCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Decode any new assets of `doc` and forget removed ones.
    pub fn sync(&mut self, doc: &Document) {
        self.entries.retain(|id, _| doc.assets.contains_key(id));
        self.failed.retain(|id, _| doc.assets.contains_key(id));
        for (id, asset) in &doc.assets {
            if self.entries.contains_key(id) || self.failed.contains_key(id) {
                continue;
            }
            let Some(bytes) = doc.blobs.get(id) else {
                continue;
            };
            match decode_asset(asset.format, bytes) {
                Ok(c) => {
                    self.entries.insert(*id, c);
                }
                Err(e) => {
                    tracing::warn!(asset = %id, "cannot decode image: {e}");
                    self.failed.insert(*id, e.to_string());
                }
            }
        }
    }

    /// Assets that failed to decode, with the reason.
    pub fn failures(&self) -> &HashMap<AssetId, String> {
        &self.failed
    }
}

impl ImageResolver for ImageCache {
    fn image(&self, asset: AssetId) -> Option<ImageContent> {
        self.entries.get(&asset).cloned()
    }
}

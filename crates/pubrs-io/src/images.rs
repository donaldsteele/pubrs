//! Decoding embedded images for renderers.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use image::ImageDecoder;
use pubrs_core::{AssetId, Document, FrameContent, ImageFormat};
use pubrs_layout::{AlphaContours, Contour, trace_contour};
use pubrs_render::{ImageContent, ImageResolver, ImageSnapshot, RasterImage, pixmap_from_rgba};

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
                mips: OnceLock::new(),
            })))
        }
    }
}

/// Alpha contours of decoded raster images, shared with the layouter
/// ([`pubrs_layout::AlphaContours`]) so text wrap can follow the visible
/// pixels. Traced once per asset, when the image is decoded.
#[derive(Clone, Default)]
pub struct ContourStore(Arc<Mutex<HashMap<AssetId, Option<Arc<Contour>>>>>);

impl ContourStore {
    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<AssetId, Option<Arc<Contour>>>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn set(&self, asset: AssetId, contour: Option<Contour>) {
        self.map().insert(asset, contour.map(Arc::new));
    }
}

impl AlphaContours for ContourStore {
    fn contour(&self, asset: AssetId) -> Option<Arc<Contour>> {
        self.map().get(&asset).cloned().flatten()
    }
}

/// Trace the 50% alpha outline of a decoded raster image.
fn trace_raster(img: &RasterImage) -> Option<Contour> {
    let alpha: Vec<u8> = img
        .pixmap
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| p[3])
        .collect();
    trace_contour(img.width, img.height, &alpha)
}

/// Decoded images of a document, keyed by asset id. Asset bytes never change
/// for a given id, so entries stay valid until the asset is removed. Raster
/// entries keep half-size mip levels for zoomed-out drawing in the GUI
/// ([`RasterImage::mips`], built on first use by the render worker).
#[derive(Default)]
pub struct ImageCache {
    entries: HashMap<AssetId, ImageContent>,
    failed: HashMap<AssetId, String>,
    /// Shared copy of `entries` for render threads, until they change.
    snapshot: Option<ImageSnapshot>,
    /// Alpha outlines for text wrap, shared with the layouter.
    contours: ContourStore,
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

    /// The contours of this cache's raster images, for the layouter.
    pub fn contours(&self) -> ContourStore {
        self.contours.clone()
    }

    /// Decode any new assets of `doc` and forget removed ones.
    pub fn sync(&mut self, doc: &Document) {
        self.forget_removed(doc);
        for (id, asset) in &doc.assets {
            self.decode(doc, *id, asset.format);
        }
    }

    /// Decode only the assets that tight or through wrap needs an outline
    /// of (layout runs before any drawing). Cheap when there are none.
    pub fn sync_for_wrap(&mut self, doc: &Document) {
        self.forget_removed(doc);
        for f in doc.frames.values() {
            if !matches!(
                f.wrap.mode,
                pubrs_core::WrapMode::Tight | pubrs_core::WrapMode::Through
            ) {
                continue;
            }
            if let FrameContent::Image(img) = &f.content
                && let Some(id) = img.asset
                && let Some(asset) = doc.assets.get(&id)
            {
                self.decode(doc, id, asset.format);
            }
        }
    }

    fn forget_removed(&mut self, doc: &Document) {
        let before = self.entries.len();
        self.entries.retain(|id, _| doc.assets.contains_key(id));
        if self.entries.len() != before {
            self.snapshot = None;
        }
        self.failed.retain(|id, _| doc.assets.contains_key(id));
        self.contours
            .map()
            .retain(|id, _| doc.assets.contains_key(id));
    }

    fn decode(&mut self, doc: &Document, id: AssetId, format: ImageFormat) {
        if self.entries.contains_key(&id) || self.failed.contains_key(&id) {
            return;
        }
        let Some(bytes) = doc.blobs.get(&id) else {
            return;
        };
        match decode_asset(format, bytes) {
            Ok(c) => {
                let contour = match &c {
                    ImageContent::Raster(r) => trace_raster(r),
                    _ => None,
                };
                self.contours.set(id, contour);
                self.entries.insert(id, c);
                self.snapshot = None;
            }
            Err(e) => {
                tracing::warn!(asset = %id, "cannot decode image: {e}");
                self.failed.insert(id, e.to_string());
            }
        }
    }

    /// Assets that failed to decode, with the reason.
    pub fn failures(&self) -> &HashMap<AssetId, String> {
        &self.failed
    }

    /// The decoded images as an immutable snapshot for render threads. Cheap
    /// to call again: the snapshot is rebuilt only after `sync` changed the
    /// cache.
    pub fn snapshot(&mut self) -> ImageSnapshot {
        self.snapshot
            .get_or_insert_with(|| ImageSnapshot::new(self.entries.clone()))
            .clone()
    }
}

impl ImageResolver for ImageCache {
    fn image(&self, asset: AssetId) -> Option<ImageContent> {
        self.entries.get(&asset).cloned()
    }
}

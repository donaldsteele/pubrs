//! Decoded images supplied to renderers. Decoding lives in `pubrs-io`;
//! renderers only consume [`ImageContent`] through an [`ImageResolver`].

use std::sync::Arc;

use pubrs_core::{AssetId, ImageFormat};
use resvg::tiny_skia;

/// A decoded raster image plus its original encoded bytes (for PDF pass-through).
pub struct RasterImage {
    pub width: u32,
    pub height: u32,
    /// Premultiplied RGBA8 pixels.
    pub pixmap: tiny_skia::Pixmap,
    pub encoded: Arc<[u8]>,
    pub format: ImageFormat,
    /// True if `encoded` shows exactly `pixmap` (false when EXIF rotation was applied),
    /// so a PDF writer may embed the original bytes.
    pub passthrough: bool,
}

impl std::fmt::Debug for RasterImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "RasterImage({}x{} {:?})",
            self.width, self.height, self.format
        )
    }
}

/// Image content ready to draw.
#[derive(Debug, Clone)]
pub enum ImageContent {
    Raster(Arc<RasterImage>),
    Svg(Arc<usvg::Tree>),
}

/// Looks up decoded images by asset id.
pub trait ImageResolver {
    fn image(&self, asset: AssetId) -> Option<ImageContent>;
}

/// A resolver with no images (frames with images draw nothing).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoImages;

impl ImageResolver for NoImages {
    fn image(&self, _: AssetId) -> Option<ImageContent> {
        None
    }
}

impl<T: ImageResolver + ?Sized> ImageResolver for &T {
    fn image(&self, asset: AssetId) -> Option<ImageContent> {
        (**self).image(asset)
    }
}

/// Premultiply straight RGBA8 into a tiny-skia pixmap.
pub fn pixmap_from_rgba(width: u32, height: u32, rgba: &[u8]) -> Option<tiny_skia::Pixmap> {
    if rgba.len() != (width as usize) * (height as usize) * 4 {
        return None;
    }
    let mut data = Vec::with_capacity(rgba.len());
    for [r, g, b, a] in rgba.as_chunks::<4>().0 {
        let c = tiny_skia::ColorU8::from_rgba(*r, *g, *b, *a).premultiply();
        data.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    tiny_skia::Pixmap::from_vec(data, tiny_skia::IntSize::from_wh(width, height)?)
}

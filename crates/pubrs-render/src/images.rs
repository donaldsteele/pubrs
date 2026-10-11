//! Decoded images supplied to renderers. Decoding lives in `pubrs-io`;
//! renderers only consume [`ImageContent`] through an [`ImageResolver`].

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use pubrs_core::{AssetId, ImageFormat};
use resvg::tiny_skia;

use crate::scene::Affine;

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
    /// Half-size reductions of `pixmap` (`[0]` is half, `[1]` a quarter, ...),
    /// built on first use and kept with the image. Only the GUI draws from
    /// them (the canvas viewport and the page panel's thumbnails); exports,
    /// the thumbnail stored in saved files and golden tests never do.
    pub mips: OnceLock<Vec<tiny_skia::Pixmap>>,
}

/// Smallest side a mip level is reduced to.
const MIN_MIP_SIDE: u32 = 8;

impl RasterImage {
    /// The reductions of this image, built on the first call.
    pub fn mips(&self) -> &[tiny_skia::Pixmap] {
        self.mips.get_or_init(|| build_mips(&self.pixmap))
    }

    /// The pixmap to sample when the image is drawn with `image_to_device`
    /// (image pixels → device pixels), and the transform from that pixmap's
    /// pixels to image pixels. A level is chosen only while the image is
    /// shown at less than half its size, so a mip is never magnified.
    pub fn mip_for(&self, image_to_device: Affine) -> (&tiny_skia::Pixmap, Affine) {
        let [a, b, c, d, ..] = image_to_device.0;
        let scale = a.hypot(b).max(c.hypot(d));
        if !(scale > 0.0 && scale < 0.5) {
            return (&self.pixmap, Affine::IDENTITY);
        }
        let level = ((1.0 / scale).log2().floor() as usize).max(1);
        let mips = self.mips();
        let Some(pm) = mips.get(level.min(mips.len()).wrapping_sub(1)) else {
            return (&self.pixmap, Affine::IDENTITY);
        };
        let sx = f64::from(self.pixmap.width()) / f64::from(pm.width());
        let sy = f64::from(self.pixmap.height()) / f64::from(pm.height());
        (pm, Affine::scale(sx, sy))
    }
}

/// Successive 2×2 box-filtered halvings of a premultiplied pixmap (odd edges
/// average the pixels that exist), down to [`MIN_MIP_SIDE`].
pub fn build_mips(base: &tiny_skia::Pixmap) -> Vec<tiny_skia::Pixmap> {
    let mut out: Vec<tiny_skia::Pixmap> = Vec::new();
    loop {
        let src = out.last().unwrap_or(base);
        let (w, h) = (src.width() as usize, src.height() as usize);
        if w.max(h) <= MIN_MIP_SIDE as usize {
            break;
        }
        let (dw, dh) = (w.div_ceil(2), h.div_ceil(2));
        let Some(mut dst) = tiny_skia::Pixmap::new(dw as u32, dh as u32) else {
            break;
        };
        let s = src.data();
        let d = dst.data_mut();
        for y in 0..dh {
            let rows = [2 * y, (2 * y + 1).min(h - 1)];
            let ny = if rows[0] == rows[1] { 1 } else { 2 };
            for x in 0..dw {
                let cols = [2 * x, (2 * x + 1).min(w - 1)];
                let nx = if cols[0] == cols[1] { 1 } else { 2 };
                let n = (nx * ny) as u32;
                for ch in 0..4 {
                    let mut sum = 0u32;
                    for &r in &rows[..ny] {
                        for &c in &cols[..nx] {
                            sum += u32::from(s[(r * w + c) * 4 + ch]);
                        }
                    }
                    d[(y * dw + x) * 4 + ch] = ((sum + n / 2) / n) as u8;
                }
            }
        }
        out.push(dst);
    }
    out
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

/// An immutable, cheaply cloned set of decoded images, handed to render
/// threads (the viewport worker) while the session keeps its cache.
#[derive(Clone, Default)]
pub struct ImageSnapshot(Arc<HashMap<AssetId, ImageContent>>);

impl ImageSnapshot {
    pub fn new(images: HashMap<AssetId, ImageContent>) -> Self {
        Self(Arc::new(images))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for ImageSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ImageSnapshot({} images)", self.0.len())
    }
}

impl ImageResolver for ImageSnapshot {
    fn image(&self, asset: AssetId) -> Option<ImageContent> {
        self.0.get(&asset).cloned()
    }
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

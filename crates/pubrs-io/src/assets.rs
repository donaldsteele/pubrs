//! Image import: format detection and intrinsic size.

use std::io::Cursor;

use pubrs_core::{Asset, AssetId, Bytes, IdSource, ImageFormat};

use crate::error::IoError;

/// Largest accepted image dimension in pixels.
pub const MAX_IMAGE_PIXELS: u32 = 20_000;

/// Format and pixel size of an encoded image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageInfo {
    pub format: ImageFormat,
    pub width: u32,
    pub height: u32,
}

/// Detect PNG, JPEG or SVG from the first bytes.
pub fn sniff_format(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(ImageFormat::Png);
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(ImageFormat::Jpeg);
    }
    let head = &bytes[..bytes.len().min(4096)];
    let text = String::from_utf8_lossy(head);
    let t = text.trim_start_matches('\u{feff}').trim_start();
    if (t.starts_with('<') && t.contains("<svg")) || bytes.starts_with(&[0x1f, 0x8b]) {
        return Some(ImageFormat::Svg);
    }
    None
}

/// Decode just enough of an image to learn its format and size.
pub fn image_info(bytes: &[u8]) -> Result<ImageInfo, IoError> {
    let format =
        sniff_format(bytes).ok_or_else(|| IoError::Image("not a PNG, JPEG or SVG image".into()))?;
    let (width, height) = match format {
        ImageFormat::Png | ImageFormat::Jpeg => {
            let fmt = if format == ImageFormat::Png {
                image::ImageFormat::Png
            } else {
                image::ImageFormat::Jpeg
            };
            // Report the size after EXIF rotation, matching what is drawn.
            use image::ImageDecoder;
            use image::metadata::Orientation as O;
            let mut dec = image::ImageReader::with_format(Cursor::new(bytes), fmt)
                .into_decoder()
                .map_err(|e| IoError::Image(e.to_string()))?;
            let (w, h) = dec.dimensions();
            match dec.orientation().unwrap_or(O::NoTransforms) {
                O::Rotate90 | O::Rotate270 | O::Rotate90FlipH | O::Rotate270FlipH => (h, w),
                _ => (w, h),
            }
        }
        ImageFormat::Svg => {
            let tree = usvg::Tree::from_data(bytes, crate::images::svg_options())
                .map_err(|e| IoError::Image(e.to_string()))?;
            let s = tree.size();
            (
                s.width().ceil().max(1.0) as u32,
                s.height().ceil().max(1.0) as u32,
            )
        }
    };
    if width == 0 || height == 0 || width > MAX_IMAGE_PIXELS || height > MAX_IMAGE_PIXELS {
        return Err(IoError::Image(format!(
            "image size {width}x{height} is outside 1..={MAX_IMAGE_PIXELS} pixels"
        )));
    }
    Ok(ImageInfo {
        format,
        width,
        height,
    })
}

/// Build an asset (metadata + bytes) from an encoded image.
pub fn import_image(
    ids: &mut dyn IdSource,
    name: &str,
    bytes: Vec<u8>,
) -> Result<(Asset, Bytes), IoError> {
    let info = image_info(&bytes)?;
    let asset = Asset {
        id: AssetId::new(ids),
        name: name.to_string(),
        format: info.format,
        pixel_width: info.width,
        pixel_height: info.height,
    };
    Ok((asset, Bytes::new(bytes)))
}

/// Longest side of an image pasted from the clipboard, in pixels.
pub const PASTE_MAX_SIDE: u32 = 8_000;

/// A PNG made from clipboard pixels.
#[derive(Debug, Clone)]
pub struct PastedPng {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// The image was larger than the limit and was scaled down.
    pub downscaled: bool,
    /// Size of the original, in pixels.
    pub original: (u32, u32),
}

/// Encode straight RGBA pixels as PNG, scaling them down first when a side
/// is longer than `max_side` (aspect ratio kept).
pub fn rgba_to_png(
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    max_side: u32,
) -> Result<PastedPng, IoError> {
    if width == 0 || height == 0 || rgba.len() as u64 != u64::from(width) * u64::from(height) * 4 {
        return Err(IoError::Image(
            "clipboard image has no usable pixels".into(),
        ));
    }
    let img = image::RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| IoError::Image("clipboard image has no usable pixels".into()))?;
    let longest = width.max(height);
    let (img, downscaled) = if longest > max_side {
        let scale = f64::from(max_side) / f64::from(longest);
        let w = ((f64::from(width) * scale).round() as u32).clamp(1, max_side);
        let h = ((f64::from(height) * scale).round() as u32).clamp(1, max_side);
        (
            image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle),
            true,
        )
    } else {
        (img, false)
    };
    let mut png = Vec::new();
    img.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| IoError::Image(e.to_string()))?;
    Ok(PastedPng {
        png,
        width: img.width(),
        height: img.height(),
        downscaled,
        original: (width, height),
    })
}

/// Cut a rectangle (pixels, clamped to the image) out of a PNG and encode
/// it again.
pub fn crop_png(png: &[u8], x: u32, y: u32, w: u32, h: u32) -> Result<Vec<u8>, IoError> {
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .map_err(|e| IoError::Image(e.to_string()))?;
    let x = x.min(img.width().saturating_sub(1));
    let y = y.min(img.height().saturating_sub(1));
    let w = w.clamp(1, img.width() - x);
    let h = h.clamp(1, img.height() - y);
    let mut out = Vec::new();
    img.crop_imm(x, y, w, h)
        .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(|e| IoError::Image(e.to_string()))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_svg_and_reads_size() {
        let svg = br#"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" width="120" height="80"><rect width="10" height="10"/></svg>"#;
        let info = image_info(svg).unwrap();
        assert_eq!(info.format, ImageFormat::Svg);
        assert_eq!((info.width, info.height), (120, 80));
    }

    #[test]
    fn pasted_pixels_become_png_and_shrink_past_the_limit() {
        let small = rgba_to_png(vec![255; 4 * 6 * 3], 6, 3, 8).unwrap();
        assert!(!small.downscaled);
        let info = image_info(&small.png).unwrap();
        assert_eq!((info.width, info.height), (6, 3));
        let big = rgba_to_png(vec![200; 4 * 40 * 10], 40, 10, 8).unwrap();
        assert!(big.downscaled);
        assert_eq!((big.width, big.height), (8, 2));
        assert_eq!(big.original, (40, 10));
        assert!(rgba_to_png(vec![0; 3], 1, 1, 8).is_err());
        let cut = crop_png(&small.png, 2, 1, 100, 100).unwrap();
        let info = image_info(&cut).unwrap();
        assert_eq!((info.width, info.height), (4, 2));
    }

    #[test]
    fn rejects_garbage() {
        assert!(image_info(b"hello").is_err());
        assert!(image_info(b"\x89PNG\r\n\x1a\nbroken").is_err());
    }
}

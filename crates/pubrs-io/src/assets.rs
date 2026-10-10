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
    fn rejects_garbage() {
        assert!(image_info(b"hello").is_err());
        assert!(image_info(b"\x89PNG\r\n\x1a\nbroken").is_err());
    }
}

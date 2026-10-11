//! The Escher BLIP store of a `.pub` file: pictures.
//!
//! Publisher 2002 and later keep the pictures in the `Escher/EscherDelayStm`
//! stream. The `EscherStm` stream starts with a drawing-group container (DGG)
//! that holds a BLIP store container (`0xF001`) of file BLIP store entries
//! (FBSE, `0xF007`). An FBSE says which kind of picture it is, how large and
//! where it lives: either inside the FBSE record itself or at `foDelay` in the
//! delay stream, as a BLIP record (`0xF018..=0xF117`).
//!
//! Corpus findings (the only specification, ADR 0026):
//!
//! * `51318`, `SampleBrochure`, `SampleNewsletter` carry BLIPs; the others have
//!   an empty delay stream or none.
//! * Their FBSEs have no embedded BLIP; all use `foDelay`.
//! * The record types are WMF (`0xF01B`), JPEG (`0xF01D`) and PNG (`0xF01E`).
//!   The two "PNG" BLIPs of `SampleBrochure` hold GIF data, so the picture
//!   format is sniffed from the bytes, not trusted from the record type.
//! * Every WMF in the corpus draws vector records only (no embedded bitmap), so
//!   no metafile extractor is written (ADR 0026).
//!
//! Everything read here is bounded: the BLIP must lie inside its stream, a GIF
//! is decoded only when its header dimensions are small, and each BLIP charges
//! the import budget.

use std::io::Cursor;

use image::ImageDecoder;

use super::records::{Blip, Bytes};
use super::report::ImportReport;

/// Most BLIPs read from one file.
pub const MAX_BLIPS: usize = 10_000;
/// A GIF is converted only when it has at most this many pixels (16 Mpx; the
/// RGBA copy is then at most 64 MiB).
const MAX_GIF_PIXELS: u64 = 16 * 1024 * 1024;
const FBSE_HEADER: usize = 36;
const MAX_GIF_SIDE: u32 = 8192;
/// Most picture bytes kept from one file, over all BLIPs.
pub const MAX_RETAINED_BLIP_BYTES: usize = 256 * 1024 * 1024;

/// BLIP record instances that carry a second 16-byte UID.
fn two_uids(inst: u16) -> bool {
    matches!(
        inst,
        0x3d5 | 0x217 | 0x543 | 0x46b | 0x6e3 | 0x6e1 | 0x7a9 | 0x6e5
    )
}

/// Read the BLIP record at `pos` in `buf` (the delay stream, or the bytes
/// following an FBSE header).
pub fn decode_blip(buf: &[u8], pos: usize, rep: &mut ImportReport, retained: &mut usize) -> Blip {
    let b = Bytes(buf);
    let (Some(vi), Some(ty), Some(len)) = (b.u16(pos), b.u16(pos + 2), b.u32(pos + 4)) else {
        return Blip::Missing;
    };
    let inst = vi >> 4;
    if !(0xf018..=0xf117).contains(&ty) {
        return Blip::Missing;
    }
    let Some(body) = pos.checked_add(8).and_then(|o| b.slice(o, len as usize)) else {
        return Blip::Missing;
    };
    if !rep.charge_work(body.len().saturating_add(64)) {
        return Blip::Missing;
    }
    let uid = if two_uids(inst) { 32 } else { 16 };
    match ty {
        0xf01a => Blip::Unsupported("EMF"),
        0xf01b => Blip::Unsupported("WMF"),
        0xf01c => Blip::Unsupported("PICT"),
        0xf029 => Blip::Unsupported("TIFF"),
        0xf01f => Blip::Unsupported("DIB"),
        _ => {
            // Bitmap BLIPs have one tag byte after the UIDs.
            let Some(data) = body.get(uid + 1..) else {
                return Blip::Missing;
            };
            raster(data, rep, retained)
        }
    }
}

/// Classify bitmap bytes by content: PNG and JPEG as they are, GIF converted
/// to PNG, anything else unsupported.
fn raster(data: &[u8], rep: &mut ImportReport, retained: &mut usize) -> Blip {
    let mut keep = |n: usize| -> bool {
        if retained.saturating_add(n) > MAX_RETAINED_BLIP_BYTES {
            return false;
        }
        *retained += n;
        true
    };
    if data.starts_with(b"\x89PNG\r\n\x1a\n") || data.starts_with(&[0xff, 0xd8, 0xff]) {
        if keep(data.len()) {
            Blip::Raster(data.to_vec())
        } else {
            Blip::OverBudget
        }
    } else if data.starts_with(b"GIF8") {
        match gif_to_png(data, rep) {
            Some(png) if keep(png.len()) => Blip::Raster(png),
            Some(_) => Blip::OverBudget,
            None => Blip::Unsupported("GIF"),
        }
    } else if data.starts_with(b"BM") {
        Blip::Unsupported("DIB")
    } else {
        Blip::Unsupported("unknown picture format")
    }
}

/// Decode the first frame of a GIF and re-encode it as PNG, within bounds.
fn gif_to_png(data: &[u8], rep: &mut ImportReport) -> Option<Vec<u8>> {
    let mut reader = image::ImageReader::with_format(Cursor::new(data), image::ImageFormat::Gif);
    // Explicit limits, so the bound does not depend on the crate's defaults.
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_GIF_SIDE);
    limits.max_image_height = Some(MAX_GIF_SIDE);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let dec = reader.into_decoder().ok()?;
    let (w, h) = dec.dimensions();
    let px = u64::from(w) * u64::from(h);
    if w == 0 || h == 0 || px > MAX_GIF_PIXELS {
        return None;
    }
    let work = usize::try_from(px).unwrap_or(usize::MAX);
    if !rep.charge_work(work) {
        return None;
    }
    let img = image::DynamicImage::from_decoder(dec).ok()?.into_rgba8();
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .ok()?;
    Some(out)
}

/// Read every FBSE under a BLIP store container (`store`: offset and length of
/// the container's body in `esc`).
pub fn read_store(
    esc: &[u8],
    store: (usize, usize),
    delay: Option<&[u8]>,
    rep: &mut ImportReport,
    out: &mut Vec<Blip>,
) {
    let b = Bytes(esc);
    let end = store.0.saturating_add(store.1).min(esc.len());
    let mut pos = store.0;
    let mut retained = 0usize;
    // Each distinct BLIP (stream, offset) is decoded once; later entries that
    // point at it share it (`Blip::Same`), so repeated references cost nothing.
    let mut seen: std::collections::HashMap<(u8, usize), usize> = std::collections::HashMap::new();
    while pos + 8 <= end && out.len() < MAX_BLIPS {
        let (Some(ty), Some(len)) = (b.u16(pos + 2), b.u32(pos + 4)) else {
            break;
        };
        let len = len as usize;
        let body = pos + 8;
        let Some(next) = body.checked_add(len).filter(|n| *n <= end) else {
            break;
        };
        if !rep.charge_items(1) {
            break;
        }
        let idx = out.len();
        let mut decode = |stream: u8, buf: &[u8], at: usize, rep: &mut ImportReport| -> Blip {
            match seen.get(&(stream, at)) {
                Some(first) => Blip::Same(*first),
                None => {
                    seen.insert((stream, at), idx);
                    decode_blip(buf, at, rep, &mut retained)
                }
            }
        };
        let blip = match ty {
            0xf007 if len >= FBSE_HEADER => {
                let name = usize::from(b.u8(body + 33).unwrap_or(0));
                let embedded = body + FBSE_HEADER + name;
                if embedded + 8 <= next {
                    // The BLIP is inside the FBSE.
                    decode(0, &esc[..next], embedded, rep)
                } else {
                    let fo = b.u32(body + 28).unwrap_or(u32::MAX) as usize;
                    match delay {
                        Some(d) if fo < d.len() => decode(1, d, fo, rep),
                        _ => Blip::Missing,
                    }
                }
            }
            // A BLIP directly in the store (no FBSE).
            0xf018..=0xf117 => decode(0, &esc[..next], pos, rep),
            _ => Blip::Missing,
        };
        out.push(blip);
        pos = next;
    }
}

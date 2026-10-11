//! PDF export tests: the PDF is parsed back with lopdf (we don't trust the writer).
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::{BTreeMap, HashSet};
use std::io::Cursor;

use lopdf::{Dictionary, Object, ObjectId};
use pubrs_core::*;
use pubrs_io::{ExportContext, import_image};
use pubrs_render::{FontEmbedding, PdfOptions};

use common::test_fonts;

// ---------------------------------------------------------------- lopdf helpers

fn nums(doc: &lopdf::Document, o: &Object) -> Option<Vec<f64>> {
    let (_, o) = doc.dereference(o).ok()?;
    o.as_array()
        .ok()?
        .iter()
        .map(|v| v.as_float().ok().map(f64::from))
        .collect()
}

fn inherited(doc: &lopdf::Document, page: &Dictionary, key: &[u8]) -> Option<Vec<f64>> {
    let mut node = page;
    for _ in 0..64 {
        if let Ok(o) = node.get(key) {
            return nums(doc, o);
        }
        let parent = node.get(b"Parent").ok()?.as_reference().ok()?;
        node = doc.get_dictionary(parent).ok()?;
    }
    None
}

#[derive(Debug, PartialEq)]
struct Boxes {
    media: Vec<f64>,
    crop: Vec<f64>,
    bleed: Vec<f64>,
    trim: Vec<f64>,
}

fn page_boxes(doc: &lopdf::Document, page: &Dictionary) -> Boxes {
    let media = inherited(doc, page, b"MediaBox").expect("MediaBox");
    let crop = inherited(doc, page, b"CropBox").unwrap_or(media.clone());
    let own = |k: &[u8]| page.get(k).ok().and_then(|o| nums(doc, o));
    Boxes {
        bleed: own(b"BleedBox").unwrap_or(crop.clone()),
        trim: own(b"TrimBox").unwrap_or(crop.clone()),
        media,
        crop,
    }
}

fn deref_dict<'a>(
    doc: &'a lopdf::Document,
    o: &'a Object,
) -> Option<(Option<ObjectId>, &'a Dictionary)> {
    let (id, o) = doc.dereference(o).ok()?;
    match o {
        Object::Dictionary(d) => Some((id, d)),
        Object::Stream(s) => Some((id, &s.dict)),
        _ => None,
    }
}

fn collect_fonts<'a>(
    doc: &'a lopdf::Document,
    res: &'a Dictionary,
    out: &mut BTreeMap<ObjectId, &'a Dictionary>,
    seen: &mut HashSet<ObjectId>,
) {
    if let Some((_, fonts)) = res.get(b"Font").ok().and_then(|o| deref_dict(doc, o)) {
        for (_, v) in fonts.iter() {
            if let Some((Some(id), d)) = deref_dict(doc, v) {
                out.insert(id, d);
            }
        }
    }
    for key in [b"XObject".as_slice(), b"Pattern"] {
        if let Some((_, xs)) = res.get(key).ok().and_then(|o| deref_dict(doc, o)) {
            for (_, v) in xs.iter() {
                if let Some((Some(id), d)) = deref_dict(doc, v)
                    && seen.insert(id)
                    && let Some((_, r)) = d.get(b"Resources").ok().and_then(|o| deref_dict(doc, o))
                {
                    collect_fonts(doc, r, out, seen);
                }
            }
        }
    }
}

fn name(d: &Dictionary, k: &[u8]) -> String {
    d.get(k)
        .ok()
        .and_then(|o| o.as_name().ok())
        .map(|n| String::from_utf8_lossy(n).into_owned())
        .unwrap_or_default()
}

/// (BaseFont, embedded, subset) for every font used anywhere in the document.
fn fonts(doc: &lopdf::Document) -> Vec<(String, bool, bool)> {
    let mut all = BTreeMap::new();
    let mut seen = HashSet::new();
    for (_, pid) in doc.get_pages() {
        let page = doc.get_dictionary(pid).unwrap();
        let mut node = page;
        let res = loop {
            if let Ok(o) = node.get(b"Resources") {
                break deref_dict(doc, o).map(|(_, d)| d);
            }
            match node.get(b"Parent").ok().and_then(|p| p.as_reference().ok()) {
                Some(p) => node = doc.get_dictionary(p).unwrap(),
                None => break None,
            }
        };
        if let Some(res) = res {
            collect_fonts(doc, res, &mut all, &mut seen);
        }
    }
    all.values()
        .map(|font| {
            let base = name(font, b"BaseFont");
            let desc = if name(font, b"Subtype") == "Type0" {
                font.get(b"DescendantFonts")
                    .ok()
                    .and_then(|o| doc.dereference(o).ok())
                    .and_then(|(_, a)| a.as_array().ok())
                    .and_then(|a| a.first())
                    .and_then(|o| deref_dict(doc, o))
                    .map(|(_, d)| d)
            } else {
                Some(*font)
            };
            let fd = desc
                .and_then(|d| d.get(b"FontDescriptor").ok())
                .and_then(|o| deref_dict(doc, o))
                .map(|(_, d)| d);
            let embedded = fd.is_some_and(|fd| {
                [b"FontFile".as_slice(), b"FontFile2", b"FontFile3"]
                    .iter()
                    .any(|k| fd.has(k))
            });
            let b = base.as_bytes();
            let subset = b.len() > 7 && b[6] == b'+' && b[..6].iter().all(u8::is_ascii_uppercase);
            (base, embedded, subset)
        })
        .collect()
}

fn image_filters(doc: &lopdf::Document) -> Vec<String> {
    doc.objects
        .values()
        .filter_map(|o| o.as_stream().ok())
        .filter(|s| name(&s.dict, b"Subtype") == "Image")
        .map(|s| match s.dict.get(b"Filter") {
            Ok(Object::Name(n)) => String::from_utf8_lossy(n).into_owned(),
            Ok(Object::Array(a)) => a
                .iter()
                .filter_map(|o| o.as_name().ok())
                .map(|n| String::from_utf8_lossy(n).into_owned())
                .collect::<Vec<_>>()
                .join(","),
            _ => String::new(),
        })
        .collect()
}

// ---------------------------------------------------------------- fixtures

fn jpeg_bytes() -> Vec<u8> {
    let img = image::RgbImage::from_fn(300, 200, |x, y| image::Rgb([x as u8, y as u8, 90]));
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
    out.into_inner()
}

/// Two letter-size pages with bleed, text on both (linked), a JPEG and a shape.
fn brochure() -> Document {
    let mut b = DocBuilder::new(SeqIds::new(11), PageSetup::default());
    b.exec(Command::SetBleed { bleed: 9.0 }).unwrap();
    b.exec(Command::SetMeta {
        title: Some("Bakery Brochure".into()),
        author: Some("pubrs tests".into()),
    })
    .unwrap();
    let p1 = b.page(0);
    let p2 = b.add_page().unwrap();
    let bg = b
        .shape(
            p1,
            Rect::new(-9.0, -9.0, 630.0, 200.0),
            Shape::Rectangle { corner_radius: 0.0 },
        )
        .unwrap();
    b.style(
        bg,
        FrameStyle {
            fill: Some(Color::rgb(254, 243, 199).into()),
            stroke: None,
            opacity: 1.0,
        },
    )
    .unwrap();
    let (asset, data) = import_image(&mut b.ids, "bread.jpg", jpeg_bytes()).unwrap();
    let aid = asset.id;
    b.exec(Command::AddAsset { asset, data }).unwrap();
    b.image(p1, Rect::new(306.0, 220.0, 270.0, 180.0), Some(aid))
        .unwrap();
    let long = "Our bakery opened in 1987. ".repeat(40);
    let a = b
        .text(
            p1,
            Rect::new(36.0, 220.0, 250.0, 300.0),
            &format!("Golden Crust Bakery\n{long}"),
        )
        .unwrap();
    let c = b.text(p2, Rect::new(36.0, 36.0, 540.0, 700.0), "").unwrap();
    b.link(a, c).unwrap();
    let s = b.story_of(a);
    b.exec(Command::ApplyCharProps {
        story: s,
        start: 0,
        end: 19,
        props: CharProps {
            bold: Some(true),
            font_size: Some(24.0),
            font_family: Some("Liberation Serif".into()),
            italic: Some(true),
            ..Default::default()
        },
    })
    .unwrap();
    b.finish()
}

fn export(doc: &Document, opts: &PdfOptions) -> (Vec<u8>, lopdf::Document) {
    let mut ctx = ExportContext::new(test_fonts());
    let bytes = ctx.export_pdf(doc, opts).unwrap();
    let parsed = lopdf::Document::load_mem(&bytes).expect("valid PDF");
    (bytes, parsed)
}

// ---------------------------------------------------------------- tests

#[test]
fn page_count_and_boxes_without_bleed() {
    let doc = brochure();
    let (_, pdf) = export(&doc, &PdfOptions::default());
    let pages = pdf.get_pages();
    assert_eq!(pages.len(), 2);
    for (_, id) in pages {
        let b = page_boxes(&pdf, pdf.get_dictionary(id).unwrap());
        assert_eq!(b.media, vec![0.0, 0.0, 612.0, 792.0]);
        assert_eq!(b.trim, b.media);
        assert_eq!(b.bleed, b.media);
        assert_eq!(b.crop, b.media);
    }
}

#[test]
fn bleed_sets_media_and_bleed_box_around_trim() {
    let doc = brochure();
    let opts = PdfOptions {
        bleed: true,
        ..Default::default()
    };
    let (_, pdf) = export(&doc, &opts);
    let (_, id) = pdf.get_pages().into_iter().next().unwrap();
    let b = page_boxes(&pdf, pdf.get_dictionary(id).unwrap());
    assert_eq!(b.media, vec![0.0, 0.0, 630.0, 810.0]);
    assert_eq!(b.bleed, vec![0.0, 0.0, 630.0, 810.0]);
    assert_eq!(b.trim, vec![9.0, 9.0, 621.0, 801.0]);
    assert_eq!(b.crop, b.media);
}

#[test]
fn crop_marks_need_a_slug_outside_the_bleed() {
    let doc = brochure();
    let opts = PdfOptions {
        bleed: true,
        crop_marks: true,
        ..Default::default()
    };
    let (_, pdf) = export(&doc, &opts);
    let (_, id) = pdf.get_pages().into_iter().next().unwrap();
    let b = page_boxes(&pdf, pdf.get_dictionary(id).unwrap());
    // slug = bleed (9) + mark length (18) + margin (3) = 30
    assert_eq!(b.media, vec![0.0, 0.0, 672.0, 852.0]);
    assert_eq!(b.trim, vec![30.0, 30.0, 642.0, 822.0]);
    assert_eq!(b.bleed, vec![21.0, 21.0, 651.0, 831.0]);
    // Registration colour (/Separation /All) is used for the marks.
    let content = String::from_utf8_lossy(&pdf.get_page_content(id)).into_owned();
    let has_sep = pdf
        .objects
        .values()
        .any(|o| matches!(o, Object::Array(a) if a.first().and_then(|n| n.as_name().ok()) == Some(b"Separation")));
    assert!(
        has_sep || content.contains("/All"),
        "registration colour space present"
    );
}

#[test]
fn fonts_are_embedded_subsets_and_text_extracts() {
    let doc = brochure();
    let (_, pdf) = export(&doc, &PdfOptions::default());
    let fonts = fonts(&pdf);
    assert!(fonts.len() >= 2, "{fonts:?}");
    for (base, embedded, subset) in &fonts {
        assert!(embedded, "{base} not embedded");
        assert!(subset, "{base} not subset");
    }
    assert!(fonts.iter().any(|f| f.0.contains("LiberationSans")));
    assert!(fonts.iter().any(|f| f.0.contains("LiberationSerif")));
    // lopdf turns kerning (TJ adjustments) into spaces, so compare without whitespace.
    let compact = |s: String| s.split_whitespace().collect::<String>();
    let text = compact(pdf.extract_text(&[1]).unwrap());
    assert!(text.contains("GoldenCrustBakery"), "{text}");
    let text2 = compact(pdf.extract_text(&[2]).unwrap());
    assert!(
        text2.contains("bakeryopenedin1987"),
        "linked frame on page 2: {text2}"
    );
    let info = pdf
        .trailer
        .get(b"Info")
        .and_then(|o| o.as_reference())
        .unwrap();
    let info = pdf.get_dictionary(info).unwrap();
    let title = lopdf::decode_text_string(info.get(b"Title").unwrap()).unwrap();
    assert_eq!(title, "Bakery Brochure");
}

#[test]
fn outlines_mode_embeds_no_fonts() {
    let doc = brochure();
    let opts = PdfOptions {
        font_embedding: FontEmbedding::Outlines,
        ..Default::default()
    };
    let (_, pdf) = export(&doc, &opts);
    assert!(fonts(&pdf).is_empty());
}

#[test]
fn jpeg_is_passed_through() {
    let doc = brochure();
    let (_, pdf) = export(&doc, &PdfOptions::default());
    let filters = image_filters(&pdf);
    assert!(
        filters.iter().any(|f| f.contains("DCTDecode")),
        "{filters:?}"
    );
}

#[test]
fn export_is_deterministic() {
    let doc = brochure();
    let (a, _) = export(&doc, &PdfOptions::default());
    let (b, _) = export(&doc, &PdfOptions::default());
    assert_eq!(a, b);
}

/// Rasterise the PDF with hayro and compare with our own PNG renderer: text,
/// shapes and images must land in the same places (catches glyph-advance and
/// transform bugs that a structural parse cannot).
#[test]
fn pdf_renders_like_png() {
    use hayro::hayro_interpret::InterpreterSettings;
    use hayro::hayro_syntax::Pdf;
    use hayro::vello_cpu::color::palette::css::WHITE;
    use hayro::{PixmapSettings, RenderCache, RenderSettings, render};
    use pubrs_render::RasterOptions;
    use pubrs_render::tiny_skia::Pixmap;

    let doc = brochure();
    let mut ctx = ExportContext::new(test_fonts());
    let pdf_bytes = ctx.export_pdf(&doc, &PdfOptions::default()).unwrap();
    let png = ctx
        .page_png(
            &doc,
            0,
            &RasterOptions {
                dpi: 72.0,
                ..Default::default()
            },
        )
        .unwrap();
    let ours = Pixmap::decode_png(&png).unwrap();
    let pdf = Pdf::new(pdf_bytes).unwrap();
    let cache = RenderCache::new();
    let page = &pdf.pages()[0];
    let theirs = render(
        page,
        &cache,
        &InterpreterSettings::default(),
        &RenderSettings::default(),
        &PixmapSettings {
            x_scale: 1.0,
            y_scale: 1.0,
            bg_color: WHITE,
        },
    );
    let theirs = Pixmap::decode_png(&theirs.into_png().unwrap()).unwrap();
    assert_eq!(
        (theirs.width(), theirs.height()),
        (ours.width(), ours.height())
    );
    // Compare 8x8-pixel block averages of darkness: robust to anti-aliasing
    // and hinting differences, sensitive to anything misplaced.
    let block = |pm: &Pixmap, bx: u32, by: u32| -> f64 {
        let mut sum = 0.0;
        for y in by * 8..(by * 8 + 8).min(pm.height()) {
            for x in bx * 8..(bx * 8 + 8).min(pm.width()) {
                let p = pm.pixel(x, y).unwrap();
                sum += 765.0 - f64::from(p.red()) - f64::from(p.green()) - f64::from(p.blue());
            }
        }
        sum / 64.0
    };
    let (bw, bh) = (ours.width() / 8, ours.height() / 8);
    let mut worst = 0.0f64;
    let mut bad = 0;
    for by in 0..bh {
        for bx in 0..bw {
            let d = (block(&ours, bx, by) - block(&theirs, bx, by)).abs();
            worst = worst.max(d);
            if d > 120.0 {
                bad += 1;
            }
        }
    }
    let total = bw * bh;
    eprintln!("PDF vs PNG: {bad}/{total} blocks differ, worst {worst:.1}");
    assert!(
        f64::from(bad) / f64::from(total) < 0.01,
        "PDF rendering diverges from PNG: {bad}/{total} blocks"
    );
}

#[test]
fn sheets_and_booklets_are_imposed_unless_flat() {
    let sizes = |doc: &Document, opts: &PdfOptions| -> Vec<Vec<f64>> {
        let (_, pdf) = export(doc, opts);
        pdf.get_pages()
            .into_values()
            .map(|id| page_boxes(&pdf, pdf.get_dictionary(id).unwrap()).media)
            .collect()
    };
    // Two 189 x 72 label pages, repeat fill: one Letter sheet each.
    let sheet = pubrs_io::sheet_preset("avery-5160")
        .unwrap()
        .layout(SheetFill::Repeat);
    let mut b = DocBuilder::new(
        SeqIds::new(15),
        PageSetup {
            size: sheet.label_size,
            sheet: Some(sheet),
            ..PageSetup::default()
        },
    );
    b.add_page().unwrap();
    let doc = b.finish();
    let letter = vec![0.0, 0.0, 612.0, 792.0];
    assert_eq!(sizes(&doc, &PdfOptions::default()), vec![letter.clone(); 2]);
    let flat = PdfOptions {
        flat: true,
        ..Default::default()
    };
    assert_eq!(sizes(&doc, &flat), vec![vec![0.0, 0.0, 189.0, 72.0]; 2]);

    // The brochure (2 Letter pages) as a booklet: padded to 4, two sides, double width.
    let mut booklet = brochure();
    booklet.setup.booklet = true;
    assert_eq!(
        sizes(&booklet, &PdfOptions::default()),
        vec![vec![0.0, 0.0, 1224.0, 792.0]; 2]
    );
}

/// Gradient fills become PDF shadings; swatch colours are written as their
/// RGB equivalent (DeviceCMYK and Separation come with the print output
/// milestone).
#[test]
fn gradients_export_as_shadings_and_swatches_as_rgb() {
    let doc = common::colour_document();
    let (bytes, parsed) = export(&doc, &PdfOptions::default());
    assert!(bytes.starts_with(b"%PDF-"));
    let mut shadings = Vec::new();
    for o in parsed.objects.values() {
        let Ok(d) = o.as_dict() else { continue };
        if let Ok(t) = d.get(b"ShadingType").and_then(Object::as_i64) {
            shadings.push(t);
        }
    }
    // Axial (2) for the linear gradient, radial (3) for the radial one.
    assert!(shadings.contains(&2), "{shadings:?}");
    assert!(shadings.contains(&3), "{shadings:?}");
    let again = export(&doc, &PdfOptions::default()).0;
    assert_eq!(bytes.len(), again.len(), "deterministic size");
}

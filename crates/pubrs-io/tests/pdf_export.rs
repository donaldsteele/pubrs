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

// ---------------------------------------------------------------- colour

/// Operands of every `op` operator on every page.
fn operands(pdf: &lopdf::Document, op: &str) -> Vec<Vec<f64>> {
    let mut out = Vec::new();
    for (_, id) in pdf.get_pages() {
        let data = pdf.get_page_content(id);
        let content = lopdf::content::Content::decode(&data).unwrap();
        for o in content.operations.iter().filter(|o| o.operator == op) {
            out.push(
                o.operands
                    .iter()
                    .filter_map(|v| v.as_float().ok().map(f64::from))
                    .collect(),
            );
        }
    }
    out
}

/// The PDF value of a percent after krilla's u8 quantisation.
fn lv(percent: f64) -> f64 {
    (percent / 100.0 * 255.0).round() / 255.0
}

fn near(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
}

/// The `/Separation` spaces in the file: (colorant name, alternate space, C1).
fn separations(pdf: &lopdf::Document) -> Vec<(String, String, Vec<f64>)> {
    let mut out = Vec::new();
    for o in pdf.objects.values() {
        let Ok(a) = o.as_array() else { continue };
        if a.first().and_then(|n| n.as_name().ok()) != Some(b"Separation") {
            continue;
        }
        let colorant = String::from_utf8_lossy(a[1].as_name().unwrap()).replace("#20", " ");
        let alt = a[2].as_name().map_or_else(
            |_| "(array)".into(),
            |n| String::from_utf8_lossy(n).into_owned(),
        );
        let c1 = pdf
            .dereference(&a[3])
            .ok()
            .and_then(|(_, f)| f.as_dict().ok().and_then(|d| d.get(b"C1").ok()))
            .and_then(|c| nums(pdf, c))
            .unwrap_or_default();
        out.push((colorant, alt, c1));
    }
    out
}

/// A page with a CMYK swatch at 50%, a spot swatch at 40%, an RGB fill, and
/// text in the spot colour.
fn ink_document() -> Document {
    let mut b = DocBuilder::new(SeqIds::new(31), PageSetup::default());
    let p = b.page(0);
    let cyan = SwatchId::new(&mut b.ids);
    let spot = SwatchId::new(&mut b.ids);
    for (id, name, value) in [
        (
            cyan,
            "Deep Cyan",
            SwatchValue::Cmyk(Cmyk::new(100.0, 50.0, 0.0, 20.0)),
        ),
        (
            spot,
            "Brand Red",
            SwatchValue::Spot {
                name: "PANTONE 186 C".into(),
                cmyk: Cmyk::new(0.0, 100.0, 81.0, 4.0),
            },
        ),
    ] {
        b.exec(Command::AddSwatch {
            swatch: Swatch {
                id,
                name: name.into(),
                value,
            },
        })
        .unwrap();
    }
    let rectangle = Shape::Rectangle { corner_radius: 0.0 };
    for (i, paint) in [
        Paint::Swatch {
            swatch: cyan,
            tint: 50.0,
        },
        Paint::Swatch {
            swatch: spot,
            tint: 40.0,
        },
        Paint::Color(Color::rgb(10, 20, 30)),
    ]
    .into_iter()
    .enumerate()
    {
        let f = b
            .shape(
                p,
                Rect::new(20.0 + 90.0 * i as f64, 20.0, 80.0, 80.0),
                rectangle,
            )
            .unwrap();
        b.style(
            f,
            FrameStyle {
                fill: Some(paint),
                stroke: None,
                opacity: 1.0,
            },
        )
        .unwrap();
    }
    let t = b
        .text(p, Rect::new(20.0, 120.0, 300.0, 40.0), "Spot text")
        .unwrap();
    let story = b.story_of(t);
    b.exec(Command::ApplyCharProps {
        story,
        start: 0,
        end: 9,
        props: CharProps {
            color: Some(Paint::swatch(spot)),
            underline: Some(true),
            ..Default::default()
        },
    })
    .unwrap();
    b.finish()
}

#[test]
fn cmyk_and_spot_colours_are_written_exactly_with_tints_applied() {
    let (_, pdf) = export(&ink_document(), &PdfOptions::default());
    // CMYK 100/50/0/20 at 50% -> 50/25/0/10 percent, as u8 levels / 255.
    let k = operands(&pdf, "k");
    assert!(
        k.iter()
            .any(|v| near(v, &[lv(50.0), lv(25.0), 0.0, lv(10.0)])),
        "DeviceCMYK fill with the tint applied to the components: {k:?}"
    );
    // The spot is a Separation named after the spot, with its CMYK alternative.
    let seps = separations(&pdf);
    let spot = seps
        .iter()
        .find(|(n, ..)| n == "PANTONE 186 C")
        .unwrap_or_else(|| panic!("a /Separation /PANTONE 186 C space: {seps:?}"));
    assert_eq!(spot.1, "DeviceCMYK");
    assert!(
        near(&spot.2, &[0.0, lv(100.0), lv(81.0), lv(4.0)]),
        "{:?}",
        spot.2
    );
    // The tint of a spot is its percent: 40% fills, 100% text and underline.
    let scn = operands(&pdf, "scn");
    assert!(scn.iter().any(|v| near(v, &[lv(40.0)])), "{scn:?}");
    assert!(scn.iter().any(|v| near(v, &[1.0])), "{scn:?}");
    // RGB stays RGB, and nothing else sneaks in as RGB for the swatches.
    let rg = operands(&pdf, "rg");
    assert!(
        rg.iter()
            .any(|v| near(v, &[10.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0])),
        "{rg:?}"
    );
}

#[test]
fn colour_levels_are_quantised_to_u8() {
    // C 33.3 -> round(0.333 * 255) = 85 -> 85/255 = 0.33333: krilla stores
    // components as u8 (ADR 0018).
    let mut b = DocBuilder::new(SeqIds::new(32), PageSetup::default());
    let p = b.page(0);
    let id = SwatchId::new(&mut b.ids);
    b.exec(Command::AddSwatch {
        swatch: Swatch {
            id,
            name: "Third".into(),
            value: SwatchValue::Cmyk(Cmyk::new(33.3, 0.0, 0.0, 0.0)),
        },
    })
    .unwrap();
    let f = b
        .shape(
            p,
            Rect::new(10.0, 10.0, 50.0, 50.0),
            Shape::Rectangle { corner_radius: 0.0 },
        )
        .unwrap();
    b.style(
        f,
        FrameStyle {
            fill: Some(Paint::swatch(id)),
            stroke: None,
            opacity: 1.0,
        },
    )
    .unwrap();
    let (_, pdf) = export(&b.finish(), &PdfOptions::default());
    let k = operands(&pdf, "k");
    assert!(
        k.iter().any(|v| (v[0] - 85.0 / 255.0).abs() < 1e-4),
        "{k:?}"
    );
}

/// The colour spaces named by the shadings in the file.
fn shading_spaces(pdf: &lopdf::Document) -> Vec<String> {
    let mut out = Vec::new();
    for o in pdf.objects.values() {
        let d = match o {
            Object::Dictionary(d) => d,
            Object::Stream(s) => &s.dict,
            _ => continue,
        };
        if d.get(b"ShadingType").is_err() {
            continue;
        }
        let Ok(cs) = d.get(b"ColorSpace") else {
            continue;
        };
        let (_, cs) = pdf.dereference(cs).unwrap();
        out.push(match cs {
            Object::Name(n) => String::from_utf8_lossy(n).into_owned(),
            Object::Array(a) => String::from_utf8_lossy(a[0].as_name().unwrap()).into_owned(),
            _ => "?".into(),
        });
    }
    out
}

fn gradient_document(stops: [(Paint, Paint); 3]) -> Document {
    let mut b = DocBuilder::new(SeqIds::new(33), PageSetup::default());
    let p = b.page(0);
    for (i, (a, z)) in stops.into_iter().enumerate() {
        let to_solid = |p: Paint| match p {
            Paint::Color(c) => SolidPaint::Color(c),
            Paint::Swatch { swatch, tint } => SolidPaint::Swatch { swatch, tint },
            Paint::Gradient { .. } => unreachable!(),
        };
        let f = b
            .shape(
                p,
                Rect::new(20.0, 20.0 + 100.0 * i as f64, 200.0, 80.0),
                Shape::Rectangle { corner_radius: 0.0 },
            )
            .unwrap();
        b.style(
            f,
            FrameStyle {
                fill: Some(Paint::Gradient {
                    gradient: GradientKind::Linear,
                    stops: vec![
                        GradientStop {
                            offset: 0.0,
                            color: to_solid(a),
                        },
                        GradientStop {
                            offset: 1.0,
                            color: to_solid(z),
                        },
                    ],
                    angle: 0.0,
                }),
                stroke: None,
                opacity: 1.0,
            },
        )
        .unwrap();
    }
    b.finish()
}

#[test]
fn gradients_export_as_shadings_in_cmyk_separation_or_rgb() {
    let mut doc = gradient_document([
        // CMYK + CMYK, one spot twice, and a mix with RGB.
        (Paint::Color(Color::BLACK), Paint::Color(Color::BLACK)),
        (Paint::Color(Color::BLACK), Paint::Color(Color::BLACK)),
        (Paint::Color(Color::BLACK), Paint::Color(Color::BLACK)),
    ]);
    // Swatches go in through commands so the ids are real.
    let mut ids = SeqIds::new(900);
    let cyan = SwatchId::new(&mut ids);
    let magenta = SwatchId::new(&mut ids);
    let spot_a = SwatchId::new(&mut ids);
    let spot_b = SwatchId::new(&mut ids);
    let mut ed = Editor::new(doc.clone());
    for (id, name, value) in [
        (
            cyan,
            "C",
            SwatchValue::Cmyk(Cmyk::new(100.0, 0.0, 0.0, 0.0)),
        ),
        (
            magenta,
            "M",
            SwatchValue::Cmyk(Cmyk::new(0.0, 100.0, 0.0, 0.0)),
        ),
        (
            spot_a,
            "Spot A",
            SwatchValue::Spot {
                name: "Brand Gold".into(),
                cmyk: Cmyk::new(0.0, 20.0, 100.0, 10.0),
            },
        ),
        (
            spot_b,
            "Spot A light",
            SwatchValue::Spot {
                name: "Brand Gold".into(),
                cmyk: Cmyk::new(0.0, 20.0, 100.0, 10.0),
            },
        ),
    ] {
        ed.execute(Command::AddSwatch {
            swatch: Swatch {
                id,
                name: name.into(),
                value,
            },
        })
        .unwrap();
    }
    doc = ed.doc().clone();
    let frames: Vec<FrameId> = doc.pages[0].frames.clone();
    let mut set = |frame: FrameId, a: SolidPaint, z: SolidPaint| {
        ed.execute(Command::SetFrameStyle {
            frame,
            style: FrameStyle {
                fill: Some(Paint::Gradient {
                    gradient: GradientKind::Linear,
                    stops: vec![
                        GradientStop {
                            offset: 0.0,
                            color: a,
                        },
                        GradientStop {
                            offset: 1.0,
                            color: z,
                        },
                    ],
                    angle: 0.0,
                }),
                stroke: None,
                opacity: 1.0,
            },
        })
        .unwrap();
    };
    set(
        frames[0],
        SolidPaint::swatch(cyan),
        SolidPaint::swatch(magenta),
    );
    set(
        frames[1],
        SolidPaint::swatch(spot_a),
        SolidPaint::Swatch {
            swatch: spot_b,
            tint: 30.0,
        },
    );
    set(
        frames[2],
        SolidPaint::swatch(cyan),
        Color::rgb(250, 250, 250).into(),
    );
    let (_, pdf) = export(ed.doc(), &PdfOptions::default());
    let mut spaces = shading_spaces(&pdf);
    spaces.sort();
    assert_eq!(
        spaces,
        ["DeviceCMYK", "DeviceRGB", "Separation"],
        "CMYK stops stay CMYK, stops of one spot stay Separation, a mix with RGB falls back to RGB"
    );
    assert!(separations(&pdf).iter().any(|(n, ..)| n == "Brand Gold"));
}

/// Gradient fills become PDF shadings, and the document's CMYK and spot
/// swatches become DeviceCMYK and Separation colours, not RGB.
#[test]
fn gradients_export_as_shadings_and_swatches_as_cmyk_and_separation() {
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
    // "Process Cyan" 100/10/0/5 is a DeviceCMYK fill (and stroke for the spot).
    let k = operands(&parsed, "k");
    assert!(
        k.iter().any(|v| near(v, &[1.0, lv(10.0), 0.0, lv(5.0)])),
        "{k:?}"
    );
    assert!(
        separations(&parsed)
            .iter()
            .any(|(n, alt, _)| n == "PANTONE 186 C" && alt == "DeviceCMYK")
    );
    let again = export(&doc, &PdfOptions::default()).0;
    assert_eq!(bytes.len(), again.len(), "deterministic size");
}

// ---------------------------------------------------------------- print standards

use pubrs_render::PdfStandard;

fn x4() -> PdfOptions {
    PdfOptions {
        standard: PdfStandard::PdfX4,
        bleed: true,
        ..Default::default()
    }
}

fn name_of(d: &Dictionary, k: &[u8]) -> String {
    String::from_utf8_lossy(d.get(k).unwrap().as_name().unwrap()).into_owned()
}

fn text_of(d: &Dictionary, k: &[u8]) -> String {
    String::from_utf8_lossy(d.get(k).unwrap().as_str().unwrap()).into_owned()
}

#[test]
fn bundled_profile_is_a_cmyk_output_profile() {
    let icc = pubrs_render::pdf::GENERIC_CMYK_ICC;
    assert_eq!(icc.len(), 237_034);
    assert_eq!(&icc[12..16], b"prtr");
    assert_eq!(&icc[16..20], b"CMYK");
    assert_eq!(&icc[20..24], b"Lab ");
    assert_eq!(&icc[36..40], b"acsp");
    assert!(pubrs_render::pdf::check_output_profile(icc).is_ok());
    // Not a CMYK output profile: refused with a reason.
    let mut rgb_display = icc.to_vec();
    rgb_display[12..16].copy_from_slice(b"mntr");
    assert!(
        pubrs_render::pdf::check_output_profile(&rgb_display)
            .unwrap_err()
            .contains("prtr")
    );
    assert!(pubrs_render::pdf::check_output_profile(b"nope").is_err());
}

#[test]
fn pdfx4_has_the_output_intent_versions_boxes_and_the_profile_bytes() {
    let doc = common::colour_document();
    let (bytes, pdf) = export(&doc, &x4());
    assert!(bytes.starts_with(b"%PDF-1.6"), "PDF 1.6");
    let root = pdf.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let catalog = pdf.get_dictionary(root).unwrap();
    let intents = catalog.get(b"OutputIntents").unwrap().as_array().unwrap();
    assert_eq!(intents.len(), 1);
    let oi = dd(&pdf, &intents[0]);
    assert_eq!(name_of(oi, b"S"), "GTS_PDFX");
    assert_eq!(text_of(oi, b"OutputConditionIdentifier"), "Custom");
    assert_eq!(text_of(oi, b"OutputCondition"), "Generic CMYK (SWOP-like)");
    assert_eq!(text_of(oi, b"RegistryName"), "http://www.color.org");
    let profile = pdf
        .get_object(
            oi.get(b"DestOutputProfile")
                .unwrap()
                .as_reference()
                .unwrap(),
        )
        .unwrap()
        .as_stream()
        .unwrap();
    assert_eq!(profile.dict.get(b"N").unwrap().as_i64().unwrap(), 4);
    assert_eq!(
        profile.content.as_slice(),
        pubrs_render::pdf::GENERIC_CMYK_ICC,
        "the exported PDF carries the profile byte for byte"
    );
    // Document information and XMP.
    let info = dd(&pdf, pdf.trailer.get(b"Info").unwrap());
    assert_eq!(text_of(info, b"GTS_PDFXVersion"), "PDF/X-4");
    assert_eq!(name_of(info, b"Trapped"), "False");
    let meta = pdf
        .get_object(catalog.get(b"Metadata").unwrap().as_reference().unwrap())
        .unwrap()
        .as_stream()
        .unwrap();
    let xmp = String::from_utf8_lossy(&meta.content);
    assert!(xmp.contains("pdfxid:GTS_PDFXVersion>PDF/X-4<"), "{xmp}");
    // TrimBox and BleedBox on every page, and the colours are CMYK.
    for (_, id) in pdf.get_pages() {
        let page = pdf.get_dictionary(id).unwrap();
        let b = page_boxes(&pdf, page);
        assert_eq!(b.trim, vec![9.0, 9.0, 309.0, 209.0]);
        assert_eq!(b.bleed, vec![0.0, 0.0, 318.0, 218.0]);
        assert!(page.has(b"TrimBox") && page.has(b"BleedBox"));
    }
    // CMYK is written against the output profile (ICCBased, N 4), the same
    // numbers as DeviceCMYK would carry.
    let cmyk_scn: Vec<_> = operands(&pdf, "scn")
        .into_iter()
        .filter(|v| v.len() == 4)
        .collect();
    assert!(
        cmyk_scn
            .iter()
            .any(|v| near(v, &[1.0, lv(10.0), 0.0, lv(5.0)])),
        "{cmyk_scn:?}"
    );
    assert!(separations(&pdf).iter().any(|(n, ..)| n == "PANTONE 186 C"));
    assert!(pubrs_render::pdfx::check(&bytes).is_empty());
}

#[test]
fn pdfx4_checker_reports_what_is_missing() {
    // Plain PDF (no standard): the checker names the missing pieces.
    let (bytes, _) = export(&common::colour_document(), &PdfOptions::default());
    let issues = pubrs_render::pdfx::check(&bytes);
    assert!(
        issues.iter().any(|i| i.contains("output intent")),
        "{issues:?}"
    );
    assert!(issues.iter().any(|i| i.contains("PDF/X-4")), "{issues:?}");
}

#[test]
fn pdfx4_rgb_is_icc_based_not_device_rgb() {
    // Picture and RGB colours under the CMYK intent are colour-managed: the
    // checker would flag DeviceRGB.
    let doc = brochure();
    let (bytes, pdf) = export(&doc, &x4());
    assert!(pubrs_render::pdfx::check(&bytes).is_empty());
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains("/DeviceRGB"), "no DeviceRGB anywhere");
    assert!(
        operands(&pdf, "rg").is_empty(),
        "RGB fills use an ICC space"
    );
}

#[test]
fn pdfx4_refuses_a_profile_that_is_not_cmyk() {
    let mut doc = common::colour_document();
    doc.meta.title = "x".into();
    let mut ctx = ExportContext::new(test_fonts());
    let mut bad = pubrs_render::pdf::GENERIC_CMYK_ICC.to_vec();
    bad[16..20].copy_from_slice(b"RGB ");
    let err = ctx
        .export_pdf(
            &doc,
            &PdfOptions {
                output_intent_icc: Some(bad),
                ..x4()
            },
        )
        .unwrap_err();
    assert!(err.to_string().contains("CMYK"), "{err}");
}

#[test]
fn pdfa2b_exports_with_krillas_validator() {
    let (bytes, pdf) = export(
        &common::colour_document(),
        &PdfOptions {
            standard: PdfStandard::PdfA2b,
            ..Default::default()
        },
    );
    assert!(bytes.starts_with(b"%PDF-1.7"));
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("pdfaid:part"), "PDF/A identification in XMP");
    assert!(text.contains("/GTS_PDFA1"), "PDF/A output intent");
    assert!(operands(&pdf, "rg").is_empty(), "device-independent colour");
}

#[test]
fn pdfa2b_with_an_x4_only_profile_option_is_ignored() {
    // `output_intent_icc` only applies to PDF/X-4.
    let (bytes, _) = export(
        &common::colour_document(),
        &PdfOptions {
            standard: PdfStandard::PdfA2b,
            output_intent_icc: Some(vec![0; 4]),
            ..Default::default()
        },
    );
    assert!(bytes.starts_with(b"%PDF-"));
}

// ---------------------------------------------------------------- accessibility

fn picture_document(alt: &str) -> Document {
    let mut b = DocBuilder::new(SeqIds::new(41), PageSetup::default());
    let p = b.page(0);
    let (asset, data) = import_image(&mut b.ids, "bread.jpg", jpeg_bytes()).unwrap();
    let aid = asset.id;
    b.exec(Command::AddAsset { asset, data }).unwrap();
    let img = b
        .image(p, Rect::new(36.0, 36.0, 270.0, 180.0), Some(aid))
        .unwrap();
    b.exec(Command::SetImageAlt {
        frame: img,
        alt_text: alt.into(),
    })
    .unwrap();
    b.text(p, Rect::new(36.0, 240.0, 270.0, 60.0), "A caption")
        .unwrap();
    b.finish()
}

fn alt_values(pdf: &lopdf::Document) -> Vec<String> {
    let mut out = Vec::new();
    for o in pdf.objects.values() {
        let Ok(d) = o.as_dict() else { continue };
        if d.get(b"S").ok().and_then(|s| s.as_name().ok()) == Some(b"Figure")
            && let Ok(alt) = d.get(b"Alt")
        {
            out.push(match alt {
                Object::String(s, _) => lopdf::decode_text_string(&Object::String(
                    s.clone(),
                    lopdf::StringFormat::Literal,
                ))
                .unwrap_or_default(),
                _ => String::new(),
            });
        }
    }
    out
}

#[test]
fn tagged_pdf_writes_figures_with_alt_text() {
    let doc = picture_document("A loaf of sourdough on a board");
    let (_, plain) = export(&doc, &PdfOptions::default());
    assert!(alt_values(&plain).is_empty(), "untagged by default");
    let (_, tagged) = export(
        &doc,
        &PdfOptions {
            tagged: true,
            ..Default::default()
        },
    );
    assert_eq!(alt_values(&tagged), ["A loaf of sourdough on a board"]);
    let root = tagged.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let catalog = tagged.get_dictionary(root).unwrap();
    assert!(catalog.has(b"StructTreeRoot"));
    assert!(
        dd(&tagged, catalog.get(b"MarkInfo").unwrap())
            .get(b"Marked")
            .unwrap()
            .as_bool()
            .unwrap()
    );
    // Text becomes paragraphs; the picture a figure.
    let roles: HashSet<String> = tagged
        .objects
        .values()
        .filter_map(|o| o.as_dict().ok())
        .filter_map(|d| d.get(b"S").ok().and_then(|s| s.as_name().ok()))
        .map(|n| String::from_utf8_lossy(n).into_owned())
        .collect();
    assert!(roles.contains("Figure") && roles.contains("P"), "{roles:?}");
}

#[test]
fn tagged_pdfx4_keeps_passing_the_checker() {
    let doc = picture_document("Alt");
    let (bytes, _) = export(
        &doc,
        &PdfOptions {
            tagged: true,
            ..x4()
        },
    );
    assert!(pubrs_render::pdfx::check(&bytes).is_empty());
}

fn dd<'a>(doc: &'a lopdf::Document, o: &'a Object) -> &'a Dictionary {
    deref_dict(doc, o).expect("dictionary").1
}

#[test]
fn checker_flags_device_rgb_under_the_cmyk_intent() {
    // Plain krilla output has DeviceRGB; giving it a CMYK output intent
    // without colour-managing the RGB is what the checker must catch.
    let (bytes, _) = export(&ink_document(), &PdfOptions::default());
    let patched = pubrs_render::pdfx::make_pdfx4(
        &bytes,
        "t",
        pubrs_render::pdf::GENERIC_CMYK_ICC,
        "Generic CMYK (SWOP-like)",
    )
    .expect("krilla output is the simple kind");
    let issues = pubrs_render::pdfx::check(&patched);
    assert!(
        issues.iter().any(|i| i.contains("RGB")),
        "device RGB under a CMYK intent is reported: {issues:?}"
    );
}

#[test]
fn print_package_has_the_pdf_the_document_and_a_readme() {
    let mut ctx = ExportContext::new(test_fonts());
    let doc = common::colour_document();
    let layout = ctx.layout(&doc);
    let zip_bytes = ctx
        .export_print_package(&doc, &layout, &PdfOptions::default(), "flyer")
        .unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(zip_bytes)).unwrap();
    let names: Vec<String> = zip.file_names().map(|n| n.unwrap().to_string()).collect();
    assert_eq!(names, ["flyer-pdfx4.pdf", "flyer.pubrs", "README.txt"]);
    let mut pdf = Vec::new();
    std::io::Read::read_to_end(&mut zip.by_name("flyer-pdfx4.pdf").unwrap(), &mut pdf).unwrap();
    assert!(String::from_utf8_lossy(&pdf).contains("/GTS_PDFXVersion"));
    assert!(pubrs_render::pdfx::check(&pdf).is_empty());
    let mut source = Vec::new();
    std::io::Read::read_to_end(&mut zip.by_name("flyer.pubrs").unwrap(), &mut source).unwrap();
    assert_eq!(pubrs_io::read_bytes(&source).unwrap(), doc);
    let mut readme = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("README.txt").unwrap(), &mut readme).unwrap();
    assert!(readme.contains("Liberation"), "fonts used: {readme}");
    assert!(
        readme.contains("Process Cyan: CMYK C100 M10 Y0 K5"),
        "{readme}"
    );
    assert!(
        readme
            .contains("Brand Red: Spot colour \"PANTONE 186 C\", alternative CMYK C0 M100 Y81 K4"),
        "{readme}"
    );
    assert!(readme.contains("Brand Blue: RGB #1E3A8A"), "{readme}");
    assert!(readme.contains("PDF/X-4"), "{readme}");
}

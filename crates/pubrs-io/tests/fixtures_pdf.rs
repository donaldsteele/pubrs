//! PDF/A-2b and PDF/X-4 fixtures. Writes `target/pdf-standards/{a2b,x4}.pdf`
//! (the newsletter template) for the veraPDF step in CI, and checks that
//! `pdfx::check` finds nothing wrong in PDF/X-4 exports of the newsletter, a
//! label sheet and a booklet (and of a document with CMYK, spot colour and
//! gradients).
#![allow(clippy::unwrap_used)]

mod common;

use std::path::PathBuf;

use pubrs_core::*;
use pubrs_io::ExportContext;
use pubrs_io::templates::{builtin, instantiate};
use pubrs_render::{PdfOptions, PdfStandard};

use common::test_fonts;

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/pdf-standards");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn newsletter() -> Document {
    let t = builtin("newsletter").expect("built-in newsletter");
    instantiate(&t, "newsletter", &mut SeqIds::new(901))
}

/// Avery 5160 labels, one record per label, with a CMYK fill.
fn label_sheet() -> Document {
    let mut b = DocBuilder::new(SeqIds::new(902), PageSetup::default());
    let preset = pubrs_io::sheet_preset("avery-5160").unwrap();
    b.exec(Command::SetPageSize { size: preset.label }).unwrap();
    b.exec(Command::SetSheet {
        sheet: Some(preset.layout(SheetFill::Repeat)),
    })
    .unwrap();
    let p = b.page(0);
    let ink = SwatchId::new(&mut b.ids);
    b.exec(Command::AddSwatch {
        swatch: Swatch {
            id: ink,
            name: "Label Blue".into(),
            value: SwatchValue::Cmyk(Cmyk::new(90.0, 40.0, 0.0, 10.0)),
        },
    })
    .unwrap();
    let bar = b
        .shape(
            p,
            Rect::new(0.0, 0.0, 189.0, 10.0),
            Shape::Rectangle { corner_radius: 0.0 },
        )
        .unwrap();
    b.style(
        bar,
        FrameStyle {
            fill: Some(Paint::swatch(ink)),
            stroke: None,
            opacity: 1.0,
            ..Default::default()
        },
    )
    .unwrap();
    b.text(
        p,
        Rect::new(8.0, 14.0, 170.0, 40.0),
        "Golden Crust\n12 Mill Lane",
    )
    .unwrap();
    b.finish()
}

/// Four pages printed as a saddle-stitched booklet.
fn booklet() -> Document {
    let mut b = DocBuilder::new(SeqIds::new(903), PageSetup::default());
    b.exec(Command::SetBleed { bleed: 9.0 }).unwrap();
    b.exec(Command::SetBooklet { booklet: true }).unwrap();
    let mut pages = vec![b.page(0)];
    for _ in 0..3 {
        pages.push(b.add_page().unwrap());
    }
    for (i, p) in pages.into_iter().enumerate() {
        let bg = b
            .shape(
                p,
                Rect::new(-9.0, -9.0, 630.0, 100.0),
                Shape::Rectangle { corner_radius: 0.0 },
            )
            .unwrap();
        b.style(
            bg,
            FrameStyle {
                fill: Some(Color::rgb(254, 243, 199).into()),
                stroke: None,
                opacity: 1.0,
                ..Default::default()
            },
        )
        .unwrap();
        b.text(
            p,
            Rect::new(36.0, 120.0, 540.0, 300.0),
            &format!(
                "Page {}\n{}",
                i + 1,
                "Fresh bread every morning. ".repeat(30)
            ),
        )
        .unwrap();
    }
    b.finish()
}

/// WordArt (gradient fill, outline, shadow), drop shadows on a shape and a
/// translucent frame, and BorderArt: the M25 content that must stay
/// PDF/X-4 clean (shadows are RGBA images with a soft mask).
fn effects() -> Document {
    let mut b = DocBuilder::new(SeqIds::new(904), PageSetup::default());
    let p = b.page(0);
    let spot = SwatchId::new(&mut b.ids);
    b.exec(Command::AddSwatch {
        swatch: Swatch {
            id: spot,
            name: "Shadow Brown".into(),
            value: SwatchValue::Cmyk(Cmyk::new(30.0, 60.0, 80.0, 40.0)),
        },
    })
    .unwrap();
    let shadow = |color: Paint| Shadow {
        offset_x: 5.0,
        offset_y: 6.0,
        blur: 4.0,
        color,
        opacity: 0.6,
    };
    let art = b
        .wordart(
            p,
            Rect::new(40.0, 40.0, 500.0, 160.0),
            WordArtFrame {
                text: "Grand Opening".into(),
                font: "Liberation Sans".into(),
                size: 90.0,
                warp: Warp::ArchUp,
                fill: Paint::Gradient {
                    gradient: GradientKind::Linear,
                    stops: vec![
                        GradientStop {
                            offset: 0.0,
                            color: Color::rgb(250, 204, 21).into(),
                        },
                        GradientStop {
                            offset: 1.0,
                            color: Color::rgb(220, 38, 38).into(),
                        },
                    ],
                    angle: 90.0,
                },
                outline: Some(Stroke {
                    color: Color::rgb(30, 20, 60).into(),
                    width: 2.0,
                    dash: Dash::Solid,
                }),
                shadow: Some(shadow(Color::BLACK.into())),
            },
        )
        .unwrap();
    let _ = art;
    let r = b
        .shape(
            p,
            Rect::new(60.0, 260.0, 220.0, 120.0),
            Shape::Rectangle {
                corner_radius: 10.0,
            },
        )
        .unwrap();
    b.style(
        r,
        FrameStyle {
            fill: Some(Color::rgb(30, 120, 220).into()),
            shadow: Some(shadow(Paint::swatch(spot))),
            opacity: 0.9,
            ..Default::default()
        },
    )
    .unwrap();
    let t = b
        .text(p, Rect::new(320.0, 260.0, 230.0, 120.0), "Bordered text")
        .unwrap();
    b.style(
        t,
        FrameStyle {
            fill: Some(Color::rgb(254, 249, 195).into()),
            border_art: Some(BorderArt {
                pattern: "stars".into(),
                width: 20.0,
            }),
            ..Default::default()
        },
    )
    .unwrap();
    b.finish()
}

fn export(doc: &Document, standard: PdfStandard) -> Vec<u8> {
    let mut ctx = ExportContext::new(test_fonts());
    ctx.export_pdf(
        doc,
        &PdfOptions {
            standard,
            bleed: true,
            ..Default::default()
        },
    )
    .unwrap_or_else(|e| panic!("{standard:?}: {e}"))
}

#[test]
fn pdfx4_checker_passes_on_newsletter_label_sheet_and_booklet() {
    for (name, doc) in [
        ("newsletter", newsletter()),
        ("label sheet", label_sheet()),
        ("booklet", booklet()),
        ("colour document", common::colour_document()),
        ("effects", effects()),
    ] {
        let pdf = export(&doc, PdfStandard::PdfX4);
        let issues = pubrs_render::pdfx::check(&pdf);
        assert!(issues.is_empty(), "{name}: {issues:?}");
    }
}

#[test]
fn writes_the_standards_fixtures() {
    let dir = out_dir();
    let news = newsletter();
    let a2b = export(&news, PdfStandard::PdfA2b);
    assert!(a2b.starts_with(b"%PDF-"));
    std::fs::write(dir.join("a2b.pdf"), &a2b).unwrap();
    let x4 = export(&news, PdfStandard::PdfX4);
    assert!(pubrs_render::pdfx::check(&x4).is_empty());
    std::fs::write(dir.join("x4.pdf"), &x4).unwrap();
    // More documents in each standard, for a local veraPDF run.
    for (name, doc) in [
        ("label", label_sheet()),
        ("booklet", booklet()),
        ("colour", common::colour_document()),
        ("effects", effects()),
    ] {
        std::fs::write(
            dir.join(format!("a2b-{name}.pdf")),
            export(&doc, PdfStandard::PdfA2b),
        )
        .unwrap();
        std::fs::write(
            dir.join(format!("x4-{name}.pdf")),
            export(&doc, PdfStandard::PdfX4),
        )
        .unwrap();
    }
}

/// The PDF of the effects page, rasterised by an independent renderer
/// (hayro), puts WordArt outlines, shadow bitmaps and BorderArt where our
/// own PNG does (8x8 block darkness, as in `pdf_export`).
#[test]
fn effects_pdf_renders_like_png() {
    use hayro::hayro_interpret::InterpreterSettings;
    use hayro::hayro_syntax::Pdf;
    use hayro::vello_cpu::color::palette::css::WHITE;
    use hayro::{PixmapSettings, RenderCache, RenderSettings, render};
    use pubrs_render::RasterOptions;
    use pubrs_render::tiny_skia::Pixmap;

    let doc = effects();
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
    let theirs = render(
        &pdf.pages()[0],
        &RenderCache::new(),
        &InterpreterSettings::default(),
        &RenderSettings::default(),
        &PixmapSettings {
            x_scale: 1.0,
            y_scale: 1.0,
            bg_color: WHITE,
        },
    )
    .into_png()
    .unwrap();
    std::fs::write(out_dir().join("effects-hayro.png"), &theirs).unwrap();
    std::fs::write(out_dir().join("effects-ours.png"), &png).unwrap();
    let theirs = Pixmap::decode_png(&theirs).unwrap();
    assert_eq!(
        (theirs.width(), theirs.height()),
        (ours.width(), ours.height())
    );
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
    let mut bad = 0;
    for by in 0..bh {
        for bx in 0..bw {
            if (block(&ours, bx, by) - block(&theirs, bx, by)).abs() > 120.0 {
                bad += 1;
            }
        }
    }
    let total = bw * bh;
    eprintln!("effects PDF vs PNG: {bad}/{total} blocks differ");
    assert!(
        f64::from(bad) / f64::from(total) < 0.02,
        "PDF effects diverge from PNG: {bad}/{total} blocks"
    );
}

//! Shared helpers for golden-file tests.
#![allow(dead_code)]

use std::path::PathBuf;

use pubrs_layout::FontLibrary;
use pubrs_render::tiny_skia::Pixmap;

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn test_fonts() -> FontLibrary {
    FontLibrary::from_dir(&root().join("tests/fonts")).expect("tests/fonts")
}

/// Per-channel difference treated as equal (anti-aliasing noise across CPUs).
pub const CHANNEL_TOLERANCE: u8 = 8;
/// Fraction of pixels allowed to exceed the channel tolerance.
pub const MAX_BAD_FRACTION: f64 = 0.002;

/// Compare PNG bytes against `tests/golden/render/<name>.png`.
/// `PUBRS_BLESS=1` rewrites the golden. On mismatch the actual image and a
/// diff mask are written to `target/golden-diffs/`.
pub fn assert_png_golden(name: &str, actual_png: &[u8]) {
    let path = root().join(format!("tests/golden/render/{name}.png"));
    let bless = std::env::var_os("PUBRS_BLESS").is_some();
    if bless || !path.exists() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual_png).unwrap();
        assert!(
            bless,
            "golden {name} was missing and has been written; re-run"
        );
        return;
    }
    let expected_png = std::fs::read(&path).unwrap();
    if expected_png == actual_png {
        eprintln!("golden {name}: byte-identical");
        return;
    }
    let expected = Pixmap::decode_png(&expected_png).expect("golden decodes");
    let actual = Pixmap::decode_png(actual_png).expect("actual decodes");
    let out_dir = root().join("target/golden-diffs");
    let fail = |msg: String| -> ! {
        std::fs::create_dir_all(&out_dir).unwrap();
        std::fs::write(out_dir.join(format!("{name}.actual.png")), actual_png).unwrap();
        panic!(
            "golden {name}: {msg}; actual written to {}",
            out_dir.display()
        );
    };
    if (expected.width(), expected.height()) != (actual.width(), actual.height()) {
        fail(format!(
            "size {}x{} != golden {}x{}",
            actual.width(),
            actual.height(),
            expected.width(),
            expected.height()
        ));
    }
    let mut bad = 0usize;
    let mut max = 0u8;
    let mut mask = Pixmap::new(actual.width(), actual.height()).unwrap();
    for (i, (a, e)) in actual
        .data()
        .chunks(4)
        .zip(expected.data().chunks(4))
        .enumerate()
    {
        let d = a.iter().zip(e).map(|(x, y)| x.abs_diff(*y)).max().unwrap();
        max = max.max(d);
        if d > CHANNEL_TOLERANCE {
            bad += 1;
            mask.data_mut()[i * 4..i * 4 + 4].copy_from_slice(&[255, 0, 0, 255]);
        }
    }
    let total = (actual.width() * actual.height()) as usize;
    let frac = bad as f64 / total as f64;
    eprintln!(
        "golden {name}: not byte-identical; {bad} px over tolerance ({frac:.5}), max channel diff {max}"
    );
    if frac > MAX_BAD_FRACTION {
        std::fs::create_dir_all(&out_dir).unwrap();
        std::fs::write(
            out_dir.join(format!("{name}.diff.png")),
            mask.encode_png().unwrap(),
        )
        .unwrap();
        fail(format!("{bad} pixels differ ({:.3}%)", frac * 100.0));
    }
}

use pubrs_core::*;

/// A page that uses every kind of paint: linear and radial gradients, tints
/// (25, 50 and 100 percent) of one swatch, a CMYK fill, a spot-colour
/// outline, scheme-slot fills and text coloured by swatches. It is the
/// colour golden and the contents of the current schema fixture.
pub fn colour_document() -> Document {
    let setup = PageSetup {
        size: Size::new(300.0, 200.0),
        margins: Margins::uniform(18.0),
        bleed: 9.0,
        ..PageSetup::default()
    };
    let mut b = DocBuilder::new(SeqIds::new(2101), setup);
    let p = b.page(0);
    let cyan = SwatchId::new(&mut b.ids);
    let spot = SwatchId::new(&mut b.ids);
    let brand = SwatchId::new(&mut b.ids);
    b.exec(Command::ApplyScheme {
        scheme: SchemeId("copper_dawn".into()),
    })
    .unwrap();
    for (id, name, value) in [
        (
            cyan,
            "Process Cyan",
            SwatchValue::Cmyk(Cmyk::new(100.0, 10.0, 0.0, 5.0)),
        ),
        (
            spot,
            "Brand Red",
            SwatchValue::Spot {
                name: "PANTONE 186 C".into(),
                cmyk: Cmyk::new(0.0, 100.0, 81.0, 4.0),
            },
        ),
        (
            brand,
            "Brand Blue",
            SwatchValue::Rgb(Color::rgb(30, 58, 138)),
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
    let rect = |b: &mut DocBuilder<SeqIds>, x: f64, y: f64, w: f64, h: f64, shape: Shape| {
        b.shape(p, Rect::new(x, y, w, h), shape).unwrap()
    };
    let rectangle = Shape::Rectangle { corner_radius: 0.0 };
    let fill = |b: &mut DocBuilder<SeqIds>, f: FrameId, paint: Paint, stroke: Option<Stroke>| {
        b.style(
            f,
            FrameStyle {
                fill: Some(paint),
                stroke,
                opacity: 1.0,
                ..Default::default()
            },
        )
        .unwrap();
    };
    let stops = |a: SolidPaint, z: SolidPaint| {
        vec![
            GradientStop {
                offset: 0.0,
                color: a,
            },
            GradientStop {
                offset: 1.0,
                color: z,
            },
        ]
    };
    // Gradients.
    let g1 = rect(&mut b, 18.0, 18.0, 120.0, 50.0, rectangle);
    fill(
        &mut b,
        g1,
        Paint::Gradient {
            gradient: GradientKind::Linear,
            stops: stops(
                Color::rgb(30, 58, 138).into(),
                Color::rgb(251, 191, 36).into(),
            ),
            angle: 20.0,
        },
        None,
    );
    let g2 = rect(&mut b, 162.0, 18.0, 120.0, 50.0, Shape::Ellipse);
    fill(
        &mut b,
        g2,
        Paint::Gradient {
            gradient: GradientKind::Radial,
            stops: stops(SolidPaint::swatch(spot), Color::WHITE.into()),
            angle: 0.0,
        },
        None,
    );
    // Tints of one swatch.
    for (i, tint) in [25.0, 50.0, 100.0].into_iter().enumerate() {
        let f = rect(&mut b, 18.0 + 40.0 * i as f64, 80.0, 34.0, 34.0, rectangle);
        fill(
            &mut b,
            f,
            Paint::Swatch {
                swatch: brand,
                tint,
            },
            None,
        );
    }
    // A CMYK fill with a spot outline, and a scheme-slot fill.
    let c = rect(&mut b, 150.0, 80.0, 60.0, 34.0, rectangle);
    fill(
        &mut b,
        c,
        Paint::swatch(cyan),
        Some(Stroke {
            color: Paint::swatch(spot),
            width: 3.0,
            dash: Dash::Solid,
        }),
    );
    let s = rect(&mut b, 222.0, 80.0, 60.0, 34.0, rectangle);
    fill(&mut b, s, SchemeSlot::Main.paint(), None);
    // Text coloured by swatches.
    let t = b
        .text(
            p,
            Rect::new(18.0, 126.0, 264.0, 56.0),
            "Swatches recolour text too.\nScheme accent and a 50% tint.",
        )
        .unwrap();
    let story = b.story_of(t);
    for (start, end, color) in [
        (0, 8, Paint::swatch(spot)),
        (28, 41, SchemeSlot::Accent1.paint()),
        (
            48,
            56,
            Paint::Swatch {
                swatch: brand,
                tint: 50.0,
            },
        ),
    ] {
        b.exec(Command::ApplyCharProps {
            story,
            start,
            end,
            props: CharProps {
                color: Some(color),
                ..Default::default()
            },
        })
        .unwrap();
    }
    b.finish()
}

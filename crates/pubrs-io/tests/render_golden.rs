//! Golden-image tests for the software renderer. Fonts come only from
//! `tests/fonts/`; regenerate with `PUBRS_BLESS=1 cargo test -p pubrs-io --test render_golden`.

mod common;

use std::io::Cursor;

use pubrs_core::*;
use pubrs_io::{ExportContext, import_image};
use pubrs_render::RasterOptions;

use common::{assert_png_golden, test_fonts};

fn small_page() -> PageSetup {
    PageSetup {
        size: Size::new(300.0, 200.0),
        margins: Margins::uniform(18.0),
        bleed: 9.0,
    }
}

fn opts() -> RasterOptions {
    RasterOptions {
        dpi: 144.0,
        ..Default::default()
    }
}

fn render(doc: &Document, page: usize, opts: &RasterOptions) -> Vec<u8> {
    let mut ctx = ExportContext::new(test_fonts());
    ctx.page_png(doc, page, opts).unwrap()
}

fn style(fill: Option<Color>, stroke: Option<(Color, f64, Dash)>, opacity: f64) -> FrameStyle {
    FrameStyle {
        fill,
        stroke: stroke.map(|(color, width, dash)| Stroke { color, width, dash }),
        opacity,
    }
}

fn gradient_png() -> Vec<u8> {
    let img = image::RgbaImage::from_fn(120, 80, |x, y| {
        let check = ((x / 10) + (y / 10)) % 2 == 0;
        image::Rgba([
            (x * 2) as u8,
            (y * 3) as u8,
            if check { 200 } else { 90 },
            255,
        ])
    });
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="60" viewBox="0 0 100 60">
  <rect x="2" y="2" width="96" height="56" rx="8" fill="#FDE68A" stroke="#92400E" stroke-width="3"/>
  <circle cx="25" cy="30" r="16" fill="#DC2626"/>
  <text x="46" y="38" font-family="Liberation Sans" font-size="20" fill="#1E3A8A">SVG</text>
</svg>"##;

#[test]
fn shapes() {
    let mut b = DocBuilder::new(SeqIds::new(1), small_page());
    let p = b.page(0);
    let r = b
        .shape(
            p,
            Rect::new(20.0, 20.0, 100.0, 60.0),
            Shape::Rectangle {
                corner_radius: 12.0,
            },
        )
        .unwrap();
    b.style(
        r,
        style(
            Some(Color::rgb(30, 120, 220)),
            Some((Color::rgb(10, 30, 60), 3.0, Dash::Dashed)),
            1.0,
        ),
    )
    .unwrap();
    let e = b
        .shape(p, Rect::new(90.0, 50.0, 120.0, 80.0), Shape::Ellipse)
        .unwrap();
    b.style(e, style(Some(Color::rgb(220, 40, 40)), None, 0.5))
        .unwrap();
    let l = b
        .shape(
            p,
            Rect::new(20.0, 150.0, 260.0, 30.0),
            Shape::Line {
                direction: LineDirection::UpRight,
            },
        )
        .unwrap();
    b.style(l, style(None, Some((Color::BLACK, 2.0, Dash::Dotted)), 1.0))
        .unwrap();
    let rot = b
        .shape(
            p,
            Rect::new(200.0, 20.0, 60.0, 60.0),
            Shape::Rectangle { corner_radius: 0.0 },
        )
        .unwrap();
    b.style(
        rot,
        style(
            Some(Color::rgb(40, 160, 80)),
            Some((Color::BLACK, 1.0, Dash::Solid)),
            1.0,
        ),
    )
    .unwrap();
    b.exec(Command::SetRotation {
        frame: rot,
        degrees: 30.0,
    })
    .unwrap();
    let doc = b.finish();
    assert_png_golden("shapes", &render(&doc, 0, &opts()));
}

#[test]
fn text() {
    let mut b = DocBuilder::new(SeqIds::new(2), small_page());
    let p = b.page(0);
    let title = b.doc().styles.by_name("Heading 1").unwrap().id;
    let a = b
        .text(
            p,
            Rect::new(18.0, 18.0, 130.0, 164.0),
            "Golden Crust\nFresh sourdough, rye and seeded loaves baked every morning. \
             Celebration cakes to order, with gluten-free options on Fridays.",
        )
        .unwrap();
    let c = b.text(p, Rect::new(158.0, 18.0, 124.0, 80.0), "").unwrap();
    b.link(a, c).unwrap();
    let s = b.story_of(a);
    b.exec(Command::SetParaStyle {
        story: s,
        start: 0,
        end: 0,
        style: Some(title),
    })
    .unwrap();
    b.exec(Command::ApplyCharProps {
        story: s,
        start: 0,
        end: 6,
        props: CharProps {
            color: Some(Color::rgb(180, 83, 9)),
            ..Default::default()
        },
    })
    .unwrap();
    b.exec(Command::ApplyCharProps {
        story: s,
        start: 13,
        end: 28,
        props: CharProps {
            italic: Some(true),
            underline: Some(true),
            font_family: Some("Liberation Serif".into()),
            ..Default::default()
        },
    })
    .unwrap();
    b.exec(Command::ApplyParaProps {
        story: s,
        start: 14,
        end: 14,
        props: ParaProps {
            align: Some(Align::Justify),
            ..Default::default()
        },
    })
    .unwrap();
    let note = b
        .text(
            p,
            Rect::new(158.0, 110.0, 124.0, 72.0),
            "Open daily\n7am – 3pm",
        )
        .unwrap();
    b.style(
        note,
        style(
            Some(Color::rgb(254, 243, 199)),
            Some((Color::rgb(180, 83, 9), 1.0, Dash::Solid)),
            1.0,
        ),
    )
    .unwrap();
    let ns = b.story_of(note);
    b.exec(Command::ApplyParaProps {
        story: ns,
        start: 0,
        end: 20,
        props: ParaProps {
            align: Some(Align::Center),
            ..Default::default()
        },
    })
    .unwrap();
    b.exec(Command::ApplyCharProps {
        story: ns,
        start: 0,
        end: 10,
        props: CharProps {
            bold: Some(true),
            font_size: Some(16.0),
            ..Default::default()
        },
    })
    .unwrap();
    b.exec(Command::SetTextFrame {
        frame: note,
        props: TextFrameProps {
            vertical_align: Some(VerticalAlign::Middle),
            ..Default::default()
        },
    })
    .unwrap();
    b.exec(Command::SetRotation {
        frame: note,
        degrees: 350.0,
    })
    .unwrap();
    let doc = b.finish();
    assert_png_golden("text", &render(&doc, 0, &opts()));
}

#[test]
fn images() {
    let mut b = DocBuilder::new(SeqIds::new(3), small_page());
    let p = b.page(0);
    let (png, png_data) = import_image(&mut b.ids, "gradient.png", gradient_png()).unwrap();
    let (svg, svg_data) = import_image(&mut b.ids, "badge.svg", SVG.as_bytes().to_vec()).unwrap();
    let (png_id, svg_id) = (png.id, svg.id);
    b.exec(Command::AddAsset {
        asset: png,
        data: png_data,
    })
    .unwrap();
    b.exec(Command::AddAsset {
        asset: svg,
        data: svg_data,
    })
    .unwrap();
    let fill = b
        .image(p, Rect::new(10.0, 10.0, 80.0, 80.0), Some(png_id))
        .unwrap();
    let fit = b
        .image(p, Rect::new(100.0, 10.0, 80.0, 80.0), Some(png_id))
        .unwrap();
    b.exec(Command::SetImageFit {
        frame: fit,
        fit: ImageFit::Fit,
    })
    .unwrap();
    b.style(
        fit,
        style(
            None,
            Some((Color::rgb(100, 100, 100), 1.0, Dash::Solid)),
            1.0,
        ),
    )
    .unwrap();
    let cropped = b
        .image(p, Rect::new(190.0, 10.0, 100.0, 80.0), Some(png_id))
        .unwrap();
    b.exec(Command::SetImageCrop {
        frame: cropped,
        crop: Crop {
            left: 0.25,
            top: 0.25,
            right: 0.0,
            bottom: 0.0,
        },
    })
    .unwrap();
    let rotated = b
        .image(p, Rect::new(20.0, 110.0, 90.0, 60.0), Some(png_id))
        .unwrap();
    b.exec(Command::SetImageFit {
        frame: rotated,
        fit: ImageFit::Stretch,
    })
    .unwrap();
    b.exec(Command::SetRotation {
        frame: rotated,
        degrees: 15.0,
    })
    .unwrap();
    b.image(p, Rect::new(130.0, 105.0, 150.0, 90.0), Some(svg_id))
        .unwrap();
    let doc = b.finish();
    assert!(doc.frame(fill).is_some());
    assert_png_golden("images", &render(&doc, 0, &opts()));
}

#[test]
fn master_page_and_bleed() {
    let mut b = DocBuilder::new(SeqIds::new(4), small_page());
    let p = b.page(0);
    let master = MasterId::new(&mut b.ids);
    b.exec(Command::AddMaster {
        master: MasterPage {
            id: master,
            name: "Band".into(),
            frames: vec![],
            guides: vec![],
        },
    })
    .unwrap();
    let band = FrameId::new(&mut b.ids);
    let mut f = Frame::new(
        band,
        Rect::new(-9.0, 150.0, 318.0, 59.0),
        FrameContent::Shape(ShapeFrame {
            shape: Shape::Rectangle { corner_radius: 0.0 },
        }),
    );
    f.style.fill = Some(Color::rgb(30, 58, 138));
    b.frame(Parent::Master(master), f, None).unwrap();
    b.exec(Command::SetPageMaster {
        page: p,
        master: Some(master),
    })
    .unwrap();
    let t = b
        .text(
            p,
            Rect::new(18.0, 160.0, 264.0, 30.0),
            "Page content above the master band",
        )
        .unwrap();
    let s = b.story_of(t);
    b.exec(Command::ApplyCharProps {
        story: s,
        start: 0,
        end: 34,
        props: CharProps {
            color: Some(Color::WHITE),
            font_size: Some(14.0),
            ..Default::default()
        },
    })
    .unwrap();
    let doc = b.finish();
    let with_bleed = RasterOptions {
        dpi: 72.0,
        include_bleed: true,
        ..Default::default()
    };
    let png = render(&doc, 0, &with_bleed);
    let pm = pubrs_render::tiny_skia::Pixmap::decode_png(&png).unwrap();
    assert_eq!(
        (pm.width(), pm.height()),
        (318, 218),
        "trim + 2 × 9pt bleed at 72 dpi"
    );
    assert_png_golden("master_bleed", &png);
}

#[test]
fn missing_page_is_an_error() {
    let doc = DocBuilder::new(SeqIds::new(5), small_page()).finish();
    let mut ctx = ExportContext::new(test_fonts());
    let err = ctx.page_png(&doc, 3, &opts()).unwrap_err();
    assert_eq!(err.code(), "not_found");
}

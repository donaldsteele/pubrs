//! Golden-image tests for the software renderer. Fonts come only from
//! `tests/fonts/`; regenerate with `PUBRS_BLESS=1 cargo test -p pubrs-io --test render_golden`.
#![allow(clippy::unwrap_used)]

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
        ..PageSetup::default()
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
        fill: fill.map(Into::into),
        stroke: stroke.map(|(color, width, dash)| Stroke {
            color: color.into(),
            width,
            dash,
        }),
        opacity,
        ..Default::default()
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
            color: Some(Color::rgb(180, 83, 9).into()),
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
    f.style.fill = Some(Color::rgb(30, 58, 138).into());
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
            color: Some(Color::WHITE.into()),
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

// ---- imposition (M23) ----

fn preset_layout(id: &str, fill: SheetFill) -> SheetLayout {
    pubrs_io::sheet_preset(id).unwrap().layout(fill)
}

/// A 2.625 x 1 in label with a tinted border, one line of text and a bar
/// that overflows the label (the clip must keep it inside its cell).
#[test]
fn imposed_sheet_avery_5160() {
    let sheet = preset_layout("avery-5160", SheetFill::Repeat);
    let setup = PageSetup {
        size: sheet.label_size,
        margins: Margins::uniform(6.0),
        sheet: Some(sheet),
        ..PageSetup::default()
    };
    let mut b = DocBuilder::new(SeqIds::new(23), setup);
    let p = b.page(0);
    let border = b
        .shape(
            p,
            Rect::new(2.0, 2.0, 185.0, 68.0),
            Shape::Rectangle { corner_radius: 4.0 },
        )
        .unwrap();
    b.style(
        border,
        style(
            Some(Color::rgb(253, 230, 138)),
            Some((Color::rgb(146, 64, 14), 1.5, Dash::Solid)),
            1.0,
        ),
    )
    .unwrap();
    b.text(
        p,
        Rect::new(8.0, 8.0, 170.0, 40.0),
        "Golden Crust Bakery\n12 Mill Lane",
    )
    .unwrap();
    let bar = b
        .shape(
            p,
            Rect::new(150.0, 50.0, 80.0, 40.0),
            Shape::Rectangle { corner_radius: 0.0 },
        )
        .unwrap();
    b.style(bar, style(Some(Color::rgb(30, 120, 220)), None, 1.0))
        .unwrap();
    let doc = b.finish();
    let mut ctx = ExportContext::new(test_fonts());
    let png = ctx.sheet_png(&doc, 0, &opts()).unwrap();
    assert_png_golden("imposed_sheet_avery_5160", &png);
}

/// Eight 300 x 200 pages numbered 1..8; sheet 1 front is [8, 1].
#[test]
fn imposed_booklet_8_pages() {
    let setup = PageSetup {
        booklet: true,
        ..small_page()
    };
    let mut b = DocBuilder::new(SeqIds::new(24), setup);
    for _ in 1..8 {
        b.add_page().unwrap();
    }
    for i in 0..8 {
        let p = b.page(i);
        let bg = b
            .shape(
                p,
                Rect::new(10.0, 10.0, 280.0, 180.0),
                Shape::Rectangle { corner_radius: 0.0 },
            )
            .unwrap();
        let shade = 120 + 16 * i as u8;
        b.style(
            bg,
            style(Some(Color::rgb(shade, 200, 255 - shade)), None, 1.0),
        )
        .unwrap();
        b.text(
            p,
            Rect::new(30.0, 60.0, 240.0, 80.0),
            &format!("Page {}", i + 1),
        )
        .unwrap();
    }
    let doc = b.finish();
    let mut ctx = ExportContext::new(test_fonts());
    let layout = ctx.layout(&doc);
    assert_eq!(ctx.output_scenes(&doc, &layout, true).unwrap().len(), 4);
    assert_png_golden(
        "imposed_booklet_front_1",
        &ctx.sheet_png(&doc, 0, &opts()).unwrap(),
    );
    assert_png_golden(
        "imposed_booklet_back_1",
        &ctx.sheet_png(&doc, 1, &opts()).unwrap(),
    );
}

const NEWSLETTER: &str = "Harvest Festival Newsletter\nThe community gardening association \
celebrates extraordinary participation this autumn. Volunteers organised comprehensive \
workshops, demonstrating sustainable cultivation techniques throughout neighbouring \
communities. Unforgettable contributions came from international exhibitors, particularly \
those representing agricultural cooperatives. Registration for next season's educational \
programmes opens immediately; interested participants should contact the administration \
office.";

/// A justified two-column newsletter column pair, with or without
/// hyphenation (and the justify limits that go with it).
fn newsletter(hyphenate: bool) -> Document {
    let mut b = DocBuilder::new(SeqIds::new(14), small_page());
    let p = b.page(0);
    let f = b
        .text(p, Rect::new(18.0, 18.0, 264.0, 164.0), NEWSLETTER)
        .unwrap();
    b.exec(Command::SetTextFrame {
        frame: f,
        props: TextFrameProps {
            columns: Some(2),
            column_gap: Some(10.0),
            ..Default::default()
        },
    })
    .unwrap();
    let s = b.story_of(f);
    let size = |start, end, v| Command::ApplyCharProps {
        story: s,
        start,
        end,
        props: CharProps {
            font_size: Some(v),
            ..Default::default()
        },
    };
    b.exec(size(0, NEWSLETTER.chars().count(), 9.0)).unwrap();
    b.exec(size(0, 27, 12.0)).unwrap();
    b.exec(Command::ApplyParaProps {
        story: s,
        start: 28,
        end: 28,
        props: ParaProps {
            align: Some(Align::Justify),
            hyphenate: hyphenate.then_some(true),
            justify_limits: hyphenate.then(JustifyLimits::default),
            ..Default::default()
        },
    })
    .unwrap();
    b.finish()
}

#[test]
fn newsletter_justified() {
    assert_png_golden(
        "newsletter_justified",
        &render(&newsletter(false), 0, &opts()),
    );
}

#[test]
fn newsletter_justified_hyphenated() {
    assert_png_golden(
        "newsletter_justified_hyphenated",
        &render(&newsletter(true), 0, &opts()),
    );
}

#[test]
fn colour() {
    let doc = common::colour_document();
    assert_png_golden("colour", &render(&doc, 0, &opts()));
}

/// M15: a two-column page with a heading kept with its paragraph, a
/// three-line drop cap, and a column break starting the second column.
#[test]
fn paragraph_controls_drop_cap_and_column_break() {
    let text = format!(
        "Harvest notes\nThe orchard volunteers gathered early on Saturday, sorting apples \
and pears for the market stall while the bakers prepared fresh loaves for the morning \
crowd.{}Next week: jam-making in the village hall, from ten until noon.",
        pubrs_core::text::COLUMN_BREAK
    );
    let mut b = DocBuilder::new(SeqIds::new(15), small_page());
    let p = b.page(0);
    let f = b
        .text(p, Rect::new(18.0, 18.0, 264.0, 164.0), &text)
        .unwrap();
    b.exec(Command::SetTextFrame {
        frame: f,
        props: TextFrameProps {
            columns: Some(2),
            column_gap: Some(10.0),
            ..Default::default()
        },
    })
    .unwrap();
    let s = b.story_of(f);
    b.exec(Command::ApplyCharProps {
        story: s,
        start: 0,
        end: 13,
        props: CharProps {
            bold: Some(true),
            font_size: Some(13.0),
            ..Default::default()
        },
    })
    .unwrap();
    b.exec(Command::ApplyParaProps {
        story: s,
        start: 0,
        end: 0,
        props: ParaProps {
            keep_with_next: Some(true),
            space_after: Some(4.0),
            ..Default::default()
        },
    })
    .unwrap();
    b.exec(Command::ApplyParaProps {
        story: s,
        start: 14,
        end: 14,
        props: ParaProps {
            drop_cap: Some(pubrs_core::style::DropCap { lines: 3, chars: 1 }),
            widow_orphan: Some(2),
            ..Default::default()
        },
    })
    .unwrap();
    assert_png_golden("paragraph_controls", &render(&b.finish(), 0, &opts()));
}

/// M16: a menu with prices on a right tab and dot leaders (the leaders of
/// consecutive lines line up), under a centred heading.
#[test]
fn tabs_menu_with_dot_leaders() {
    use pubrs_core::style::{TabAlign, TabLeader, TabStop};
    let mut b = DocBuilder::new(SeqIds::new(16), small_page());
    let p = b.page(0);
    let f = b
        .text(
            p,
            Rect::new(18.0, 18.0, 264.0, 164.0),
            "Lunch menu\nTomato soup\t4.50\nGrilled salmon with lemon\t18.00\nGarden salad\t7.25\n\
Sourdough and butter\t3.00\nHouse lemonade\t2.75",
        )
        .unwrap();
    let s = b.story_of(f);
    b.exec(Command::ApplyCharProps {
        story: s,
        start: 0,
        end: 10,
        props: CharProps {
            bold: Some(true),
            font_size: Some(14.0),
            ..Default::default()
        },
    })
    .unwrap();
    b.exec(Command::ApplyParaProps {
        story: s,
        start: 0,
        end: 0,
        props: ParaProps {
            align: Some(Align::Center),
            space_after: Some(6.0),
            ..Default::default()
        },
    })
    .unwrap();
    let len = b.doc().story(s).unwrap().char_len();
    b.exec(Command::ApplyParaProps {
        story: s,
        start: 11,
        end: len,
        props: ParaProps {
            tab_stops: Some(vec![TabStop {
                pos: 240.0,
                align: TabAlign::Right,
                leader: TabLeader::Dot,
            }]),
            space_after: Some(3.0),
            ..Default::default()
        },
    })
    .unwrap();
    assert_png_golden("tabs_menu", &render(&b.finish(), 0, &opts()));
}

/// M16: a nested numbered list (1. a. b. 2.) and a bulleted list.
#[test]
fn nested_numbered_list() {
    use pubrs_core::style::{List, ListFormat, ListKind};
    let mut b = DocBuilder::new(SeqIds::new(17), small_page());
    let p = b.page(0);
    let f = b
        .text(
            p,
            Rect::new(18.0, 18.0, 264.0, 164.0),
            "Before opening\nWash the counters\nSweep the floor\nSet out the tables\n\
Count the till\nOpen the doors\nWhat to bake\nSourdough\nRye",
        )
        .unwrap();
    let s = b.story_of(f);
    let items = [
        (1, ListKind::Number, 0, ListFormat::Decimal),
        (2, ListKind::Number, 1, ListFormat::LowerAlpha),
        (3, ListKind::Number, 1, ListFormat::LowerAlpha),
        (4, ListKind::Number, 0, ListFormat::Decimal),
        (5, ListKind::Number, 0, ListFormat::Decimal),
        (7, ListKind::Bullet, 0, ListFormat::Decimal),
        (8, ListKind::Bullet, 0, ListFormat::Decimal),
    ];
    for (para, kind, level, format) in items {
        let at = b.doc().story(s).unwrap().paragraph_start(para);
        b.exec(Command::ApplyParaProps {
            story: s,
            start: at,
            end: at,
            props: ParaProps {
                left_indent: Some(18.0 * (f64::from(level) + 1.0)),
                list: Some(List {
                    kind,
                    format,
                    level,
                    ..Default::default()
                }),
                ..Default::default()
            },
        })
        .unwrap();
    }
    for para in [0, 6] {
        let at = b.doc().story(s).unwrap().paragraph_start(para);
        let end = at + b.doc().story(s).unwrap().paragraphs[para].char_len();
        b.exec(Command::ApplyCharProps {
            story: s,
            start: at,
            end,
            props: CharProps {
                bold: Some(true),
                ..Default::default()
            },
        })
        .unwrap();
    }
    assert_png_golden("nested_list", &render(&b.finish(), 0, &opts()));
}

/// M17: every character format on one page: subscript and superscript, a
/// custom baseline shift, synthesized small caps, all caps, strikethrough,
/// and kerning off.
#[test]
fn character_formats() {
    let mut b = DocBuilder::new(SeqIds::new(17), small_page());
    let p = b.page(0);
    let text = "H2O and E=mc2 up 3 pt\nSmall Caps Bakery\nall caps stra\u{df}e\nStruck through, kept\nKerning AVAVAV tight\nKerning AVAVAV off";
    let f = b
        .text(p, Rect::new(18.0, 18.0, 264.0, 164.0), text)
        .unwrap();
    let s = b.story_of(f);
    let mut apply = |start: usize, end: usize, props: CharProps| {
        b.exec(Command::ApplyCharProps {
            story: s,
            start,
            end,
            props,
        })
        .unwrap();
    };
    let len = text.chars().count();
    apply(
        0,
        len,
        CharProps {
            font_size: Some(14.0),
            ..Default::default()
        },
    );
    let sub = Baseline::Subscript;
    let sup = Baseline::Superscript;
    let at = |s: &str, needle: &str| -> usize {
        let byte = s.find(needle).unwrap();
        s[..byte].chars().count()
    };
    let base = |baseline| CharProps {
        baseline: Some(baseline),
        ..Default::default()
    };
    apply(1, 2, base(sub));
    apply(12, 13, base(sup));
    let up = at(text, "up 3 pt");
    apply(up, up + 7, base(Baseline::Shift(3.0)));
    let sc = at(text, "Small Caps Bakery");
    apply(
        sc,
        sc + 17,
        CharProps {
            caps: Some(Caps::Small),
            ..Default::default()
        },
    );
    let ac = at(text, "all caps stra\u{df}e");
    apply(
        ac,
        ac + 15,
        CharProps {
            caps: Some(Caps::All),
            ..Default::default()
        },
    );
    let st = at(text, "Struck through");
    apply(
        st,
        st + 14,
        CharProps {
            strikethrough: Some(true),
            ..Default::default()
        },
    );
    let ko = at(text, "Kerning AVAVAV off");
    apply(
        ko,
        ko + 18,
        CharProps {
            kerning: Some(false),
            ..Default::default()
        },
    );
    assert_png_golden("character_formats", &render(&b.finish(), 0, &opts()));
}

// ---- M25: WordArt, shadows and BorderArt (ADR 0020) ----

fn wordart(text: &str, warp: Warp, fill: Paint) -> WordArtFrame {
    WordArtFrame {
        text: text.into(),
        font: "Liberation Sans".into(),
        size: 44.0,
        warp,
        fill,
        outline: Some(Stroke {
            color: Color::rgb(20, 24, 60).into(),
            width: 1.2,
            dash: Dash::Solid,
        }),
        shadow: None,
    }
}

fn gradient(a: Color, b: Color, angle: f64) -> Paint {
    Paint::Gradient {
        gradient: GradientKind::Linear,
        stops: vec![
            GradientStop {
                offset: 0.0,
                color: a.into(),
            },
            GradientStop {
                offset: 1.0,
                color: b.into(),
            },
        ],
        angle,
    }
}

/// All eight warps on one page, with solid and gradient fills.
#[test]
fn wordart_warps() {
    let setup = PageSetup {
        size: Size::new(420.0, 520.0),
        margins: Margins::uniform(18.0),
        bleed: 0.0,
        ..PageSetup::default()
    };
    let mut b = DocBuilder::new(SeqIds::new(21), setup);
    let p = b.page(0);
    let fills = [
        Color::rgb(30, 100, 200).into(),
        gradient(Color::rgb(250, 204, 21), Color::rgb(220, 38, 38), 90.0),
        gradient(Color::rgb(56, 189, 248), Color::rgb(30, 58, 138), 90.0),
        Color::rgb(0, 130, 90).into(),
        gradient(Color::rgb(244, 114, 182), Color::rgb(124, 58, 237), 0.0),
        Color::rgb(200, 60, 20).into(),
        gradient(Color::rgb(163, 230, 53), Color::rgb(21, 128, 61), 90.0),
        Color::rgb(120, 40, 160).into(),
    ];
    for (i, warp) in Warp::ALL.into_iter().enumerate() {
        let (col, row) = ((i % 2) as f64, (i / 2) as f64);
        let rect = Rect::new(14.0 + col * 200.0, 14.0 + row * 126.0, 192.0, 118.0);
        let text = if warp == Warp::Circle {
            "Round and round"
        } else {
            "WordArt"
        };
        let f = b
            .wordart(p, rect, wordart(text, warp, fills[i].clone()))
            .unwrap();
        b.exec(Command::SetFrameStyle {
            frame: f,
            style: FrameStyle {
                stroke: Some(Stroke {
                    color: Color::rgb(200, 205, 215).into(),
                    width: 0.5,
                    dash: Dash::Dotted,
                }),
                ..Default::default()
            },
        })
        .unwrap();
    }
    assert_png_golden(
        "wordart_warps",
        &render(
            &b.finish(),
            0,
            &RasterOptions {
                dpi: 120.0,
                ..opts()
            },
        ),
    );
}

fn drop_shadow(dx: f64, dy: f64, blur: f64, opacity: f64) -> Shadow {
    Shadow {
        offset_x: dx,
        offset_y: dy,
        blur,
        color: Color::rgb(0, 0, 0).into(),
        opacity,
    }
}

/// Soft shadows under a shape, a translucent rotated frame, text and WordArt.
#[test]
fn shadows() {
    let mut b = DocBuilder::new(SeqIds::new(22), small_page());
    let p = b.page(0);
    let e = b
        .shape(p, Rect::new(24.0, 24.0, 110.0, 70.0), Shape::Ellipse)
        .unwrap();
    let mut st = style(Some(Color::rgb(30, 120, 220)), None, 1.0);
    st.shadow = Some(drop_shadow(6.0, 7.0, 5.0, 0.55));
    b.style(e, st).unwrap();
    let r = b
        .shape(
            p,
            Rect::new(190.0, 30.0, 80.0, 60.0),
            Shape::Rectangle { corner_radius: 8.0 },
        )
        .unwrap();
    let mut st = style(Some(Color::rgb(250, 204, 21)), None, 0.8);
    st.shadow = Some(drop_shadow(7.0, 7.0, 9.0, 0.8));
    b.style(r, st).unwrap();
    b.exec(Command::SetRotation {
        frame: r,
        degrees: 15.0,
    })
    .unwrap();
    let t = b
        .text(
            p,
            Rect::new(150.0, 115.0, 130.0, 60.0),
            "Hard shadow on text",
        )
        .unwrap();
    let mut st = style(None, None, 1.0);
    st.shadow = Some(drop_shadow(2.0, 2.0, 0.0, 1.0));
    b.style(t, st).unwrap();
    let mut art = wordart("Soft", Warp::SlantUp, Color::rgb(220, 38, 38).into());
    art.shadow = Some(drop_shadow(-4.0, 6.0, 6.0, 0.5));
    b.wordart(p, Rect::new(18.0, 105.0, 120.0, 80.0), art)
        .unwrap();
    assert_png_golden("shadows", &render(&b.finish(), 0, &opts()));
}

fn bordered(pattern: &str, width: f64, fill: Color, text: &str, seed: u64) -> Document {
    let mut b = DocBuilder::new(SeqIds::new(seed), small_page());
    let p = b.page(0);
    let f = b
        .text(p, Rect::new(40.0, 30.0, 220.0, 140.0), text)
        .unwrap();
    let mut st = style(Some(fill), None, 1.0);
    st.border_art = Some(BorderArt {
        pattern: pattern.into(),
        width,
    });
    b.style(f, st).unwrap();
    b.exec(Command::SetTextFrame {
        frame: f,
        props: TextFrameProps {
            insets: Some(Margins::uniform(6.0)),
            ..Default::default()
        },
    })
    .unwrap();
    b.finish()
}

#[test]
fn borderart_stars() {
    let doc = bordered(
        "stars",
        22.0,
        Color::rgb(254, 249, 195),
        "Grand opening! Join us on Saturday.",
        23,
    );
    assert_png_golden("borderart_stars", &render(&doc, 0, &opts()));
}

#[test]
fn borderart_ropes() {
    let doc = bordered(
        "ropes",
        16.0,
        Color::rgb(240, 249, 255),
        "Harbour festival: boats, music and food.",
        24,
    );
    assert_png_golden("borderart_ropes", &render(&doc, 0, &opts()));
}

// ---- tight, through and both-sides wrap (M18) ----

const WRAP_LOREM: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do \
eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud \
exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in \
reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint \
occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. \
Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium doloremque laudantium, \
totam rem aperiam, eaque ipsa quae ab illo inventore veritatis et quasi architecto beatae vitae \
dicta sunt explicabo. Nemo enim ipsam voluptatem quia voluptas sit aspernatur aut odit aut fugit.";

/// A 100 x 100 RGBA PNG: a red disc on transparency, or a blue arch.
fn alpha_png(arch: bool) -> Vec<u8> {
    let img = image::RgbaImage::from_fn(100, 100, |x, y| {
        let (dx, dy) = (f64::from(x) - 49.5, f64::from(y) - 49.5);
        let on = if arch {
            y < 22 || !(12..88).contains(&x)
        } else {
            dx * dx + dy * dy <= 48.0 * 48.0
        };
        match (on, arch) {
            (false, _) => image::Rgba([0, 0, 0, 0]),
            (true, false) => image::Rgba([200, 40, 40, 255]),
            (true, true) => image::Rgba([40, 70, 190, 255]),
        }
    });
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn wrap_doc() -> Document {
    let mut b = DocBuilder::new(
        SeqIds::new(18),
        PageSetup {
            size: Size::new(400.0, 300.0),
            margins: Margins::uniform(18.0),
            ..PageSetup::default()
        },
    );
    let p = b.page(0);
    let (disc, disc_data) = import_image(&mut b.ids, "disc.png", alpha_png(false)).unwrap();
    let (arch, arch_data) = import_image(&mut b.ids, "arch.png", alpha_png(true)).unwrap();
    let (disc_id, arch_id) = (disc.id, arch.id);
    for (asset, data) in [(disc, disc_data), (arch, arch_data)] {
        b.exec(Command::AddAsset { asset, data }).unwrap();
    }
    b.text(p, Rect::new(18.0, 18.0, 364.0, 264.0), WRAP_LOREM)
        .unwrap();
    let ellipse = b
        .shape(p, Rect::new(110.0, 30.0, 110.0, 90.0), Shape::Ellipse)
        .unwrap();
    b.style(ellipse, style(Some(Color::rgb(250, 190, 80)), None, 1.0))
        .unwrap();
    b.wrap(
        ellipse,
        TextWrap {
            mode: WrapMode::Tight,
            margin: 6.0,
            both_sides: true,
        },
    )
    .unwrap();
    let disc_frame = b
        .image(p, Rect::new(255.0, 150.0, 110.0, 110.0), Some(disc_id))
        .unwrap();
    b.wrap(
        disc_frame,
        TextWrap {
            mode: WrapMode::Tight,
            margin: 6.0,
            both_sides: true,
        },
    )
    .unwrap();
    let arch_frame = b
        .image(p, Rect::new(30.0, 150.0, 100.0, 100.0), Some(arch_id))
        .unwrap();
    b.wrap(
        arch_frame,
        TextWrap {
            mode: WrapMode::Through,
            margin: 4.0,
            both_sides: false,
        },
    )
    .unwrap();
    b.finish()
}

#[test]
fn tight_wrap() {
    let doc = wrap_doc();
    let png = render(&doc, 0, &opts());
    assert_png_golden("tight_wrap", &png);
}

#[test]
fn image_cache_traces_the_alpha_of_decoded_pngs() {
    let doc = wrap_doc();
    let mut ctx = ExportContext::new(test_fonts());
    // Layout decodes the images tight or through wrap needs.
    ctx.layout(&doc);
    let contours = ctx.images.contours();
    let mut found = 0;
    for id in doc.assets.keys() {
        let c = pubrs_layout::AlphaContours::contour(&contours, *id).expect("traced");
        assert_eq!((c.width, c.height), (100, 100));
        assert!(!c.polys.is_empty() && c.polys.iter().all(|p| p.len() >= 3));
        found += 1;
    }
    assert_eq!(found, 2);
}

// ---- tables (ADR 0024) ----

/// A 6-row, 4-column price table with a merged header row, header and band
/// fills, thick and dashed borders, insets, vertical alignment, a merged
/// 2x1 cell and one row grown by wrapped text.
fn table_doc() -> Document {
    use pubrs_core::table::CellStyle;
    let mut b = DocBuilder::new(
        SeqIds::new(31),
        PageSetup {
            size: Size::new(360.0, 400.0),
            margins: Margins::uniform(24.0),
            ..PageSetup::default()
        },
    );
    let p = b.page(0);
    let line = |w: f64, color: Color, dash: Dash| Stroke {
        color: color.into(),
        width: w,
        dash,
    };
    let grey = Color::rgb(0x6B, 0x72, 0x80);
    let style = CellStyle {
        borders: std::array::from_fn(|_| Some(line(0.75, grey, Dash::Solid))),
        ..CellStyle::default()
    };
    let t = b
        .table(
            Parent::Page(p),
            Rect::new(24.0, 24.0, 312.0, 360.0),
            6,
            4,
            &style,
        )
        .unwrap();
    for (r, c, text) in [
        (0, 0, "Market stall prices"),
        (1, 0, "Stall"),
        (1, 1, "Item"),
        (1, 2, "Qty"),
        (1, 3, "Price"),
        (2, 0, "Cakes"),
        (
            2,
            1,
            "Victoria sponge, lemon drizzle and the very large fruit cake",
        ),
        (2, 2, "12"),
        (2, 3, "2.50"),
        (3, 0, "Plants"),
        (3, 1, "Herbs"),
        (3, 2, "30"),
        (3, 3, "1.20"),
        (5, 3, "Total"),
    ] {
        b.cell_text(t, r, c, text).unwrap();
    }
    let exec = |b: &mut DocBuilder<SeqIds>, c: Command| {
        b.exec(c).unwrap();
    };
    exec(
        &mut b,
        Command::TableMergeCells {
            frame: t,
            row: 0,
            col: 0,
            row_span: 1,
            col_span: 4,
        },
    );
    exec(
        &mut b,
        Command::TableMergeCells {
            frame: t,
            row: 3,
            col: 0,
            row_span: 2,
            col_span: 1,
        },
    );
    let navy = Color::rgb(0x1E, 0x3A, 0x5F);
    let cell = |fill: Option<Color>, valign, insets| CellStyle {
        fill: fill.map(Paint::from),
        valign,
        insets,
        borders: std::array::from_fn(|_| Some(line(1.5, navy, Dash::Solid))),
    };
    let style_at = |b: &mut DocBuilder<SeqIds>, r, c, s: CellStyle| {
        b.exec(Command::TableSetCellStyle {
            frame: t,
            row: r,
            col: c,
            style: s,
        })
        .unwrap();
    };
    style_at(
        &mut b,
        0,
        0,
        cell(
            Some(Color::rgb(0xDB, 0xE4, 0xF5)),
            VerticalAlign::Middle,
            Margins::uniform(8.0),
        ),
    );
    for c in 0..4 {
        style_at(
            &mut b,
            1,
            c,
            cell(
                Some(Color::rgb(0xF3, 0xF4, 0xF6)),
                VerticalAlign::Bottom,
                Margins::uniform(4.0),
            ),
        );
    }
    let dashed = CellStyle {
        borders: std::array::from_fn(|_| Some(line(1.0, grey, Dash::Dashed))),
        valign: VerticalAlign::Middle,
        ..CellStyle::default()
    };
    style_at(&mut b, 3, 0, dashed.clone());
    style_at(
        &mut b,
        5,
        3,
        CellStyle {
            valign: VerticalAlign::Bottom,
            ..dashed
        },
    );
    b.exec(Command::TableSetRowHeight {
        frame: t,
        row: 1,
        min_height: 32.0,
    })
    .unwrap();
    b.finish()
}

#[test]
fn table_with_merged_header() {
    let doc = table_doc();
    assert!(pubrs_core::validate::check_integrity(&doc).is_empty());
    assert_png_golden("table_merged_header", &render(&doc, 0, &opts()));
}

// ---- built-in templates (M30) ----

/// Page 1 of every built-in template, placeholders unfilled (what the user
/// sees). The calendar shows January of its default year.
#[test]
fn template_goldens() {
    for name in pubrs_io::templates::BUILTIN {
        let doc = pubrs_io::templates::builtin(name).unwrap();
        assert!(
            pubrs_core::validate::check_integrity(&doc).is_empty(),
            "{name}"
        );
        assert_png_golden(&format!("template_{name}_p1"), &render(&doc, 0, &opts()));
    }
}

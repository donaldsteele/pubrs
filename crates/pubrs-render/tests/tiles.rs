//! Tiled rendering is byte-identical to untiled rendering (plan Locked
//! Decision 4, Risk 2). The pages are built like the golden pages of
//! `crates/pubrs-io/tests/render_golden.rs` (pubrs-render cannot depend on
//! pubrs-io, so images are decoded here directly), plus a stress page with
//! rotation, opacity, dashes, bleed and frames off the page. Each is drawn
//! tiled at 1, 2, 7 and 16 threads with automatic and odd tile sizes, and
//! compared with the untiled render. Goldens stay rendered untiled.
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use pubrs_core::*;
use pubrs_layout::{FontLibrary, Layouter};
use pubrs_render::tiny_skia::Pixmap;
use pubrs_render::{
    ImageContent, ImageSnapshot, PageScene, PixelRect, RasterImage, RasterOptions, RasterRenderer,
    Region, TileOptions, TileRenderer, build_page_scene_with, pixmap_from_rgba,
};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fonts() -> FontLibrary {
    FontLibrary::from_dir(&root().join("tests/fonts")).unwrap()
}

fn small_page() -> PageSetup {
    PageSetup {
        size: Size::new(300.0, 200.0),
        margins: Margins::uniform(18.0),
        bleed: 9.0,
        ..PageSetup::default()
    }
}

fn style(fill: Option<Color>, stroke: Option<(Color, f64, Dash)>, opacity: f64) -> FrameStyle {
    FrameStyle {
        fill: fill.map(Into::into),
        stroke: stroke.map(|(color, width, dash)| pubrs_core::Stroke {
            color: color.into(),
            width,
            dash,
        }),
        opacity,
    }
}

const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="60" viewBox="0 0 100 60">
  <rect x="2" y="2" width="96" height="56" rx="8" fill="#FDE68A" stroke="#92400E" stroke-width="3"/>
  <circle cx="25" cy="30" r="16" fill="#DC2626"/>
  <text x="46" y="38" font-family="Liberation Sans" font-size="20" fill="#1E3A8A">SVG</text>
</svg>"##;

/// The golden test's gradient image, as decoded pixels.
fn gradient() -> RasterImage {
    let (w, h) = (120u32, 80u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let check = ((x / 10) + (y / 10)) % 2 == 0;
            rgba.extend_from_slice(&[
                (x * 2) as u8,
                (y * 3) as u8,
                if check { 200 } else { 90 },
                255,
            ]);
        }
    }
    RasterImage {
        width: w,
        height: h,
        pixmap: pixmap_from_rgba(w, h, &rgba).unwrap(),
        encoded: Arc::from(Vec::new()),
        format: ImageFormat::Png,
        passthrough: true,
        mips: OnceLock::new(),
    }
}

fn svg_tree() -> usvg::Tree {
    let mut db = usvg::fontdb::Database::new();
    db.load_fonts_dir(root().join("tests/fonts"));
    let mut opts = usvg::Options {
        font_family: "Liberation Sans".into(),
        ..Default::default()
    };
    opts.fontdb = Arc::new(db);
    usvg::Tree::from_data(SVG.as_bytes(), &opts).unwrap()
}

fn add_asset(b: &mut DocBuilder<SeqIds>, format: ImageFormat, w: u32, h: u32) -> AssetId {
    let id = AssetId::new(&mut b.ids);
    b.exec(Command::AddAsset {
        asset: Asset {
            id,
            name: format!("{id}"),
            format,
            pixel_width: w,
            pixel_height: h,
        },
        data: Bytes::new(vec![0u8; 4]),
    })
    .unwrap();
    id
}

struct Page {
    name: &'static str,
    doc: Document,
    images: ImageSnapshot,
    opts: RasterOptions,
}

fn opts144() -> RasterOptions {
    RasterOptions {
        dpi: 144.0,
        ..Default::default()
    }
}

fn shapes() -> Page {
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
    Page {
        name: "shapes",
        doc: b.finish(),
        images: ImageSnapshot::default(),
        opts: opts144(),
    }
}

fn text() -> Page {
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
    Page {
        name: "text",
        doc: b.finish(),
        images: ImageSnapshot::default(),
        opts: opts144(),
    }
}

fn images() -> Page {
    let mut b = DocBuilder::new(SeqIds::new(3), small_page());
    let p = b.page(0);
    let png_id = add_asset(&mut b, ImageFormat::Png, 120, 80);
    let svg_id = add_asset(&mut b, ImageFormat::Svg, 100, 60);
    b.image(p, Rect::new(10.0, 10.0, 80.0, 80.0), Some(png_id))
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
    let mut map = HashMap::new();
    map.insert(png_id, ImageContent::Raster(Arc::new(gradient())));
    map.insert(svg_id, ImageContent::Svg(Arc::new(svg_tree())));
    Page {
        name: "images",
        doc: b.finish(),
        images: ImageSnapshot::new(map),
        opts: opts144(),
    }
}

fn master_bleed() -> Page {
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
    Page {
        name: "master_bleed",
        doc: b.finish(),
        images: ImageSnapshot::default(),
        opts: RasterOptions {
            dpi: 72.0,
            include_bleed: true,
            ..Default::default()
        },
    }
}

/// Everything at once: overlapping translucent frames, rotated and flipped
/// geometry, frames hanging off the page, a big image, bold-italic text in
/// a rotated frame, a transparent background and an odd resolution.
fn stress() -> Page {
    let mut b = DocBuilder::new(SeqIds::new(6), small_page());
    let p = b.page(0);
    let png_id = add_asset(&mut b, ImageFormat::Png, 120, 80);
    let big = b
        .image(p, Rect::new(-20.0, -15.0, 200.0, 140.0), Some(png_id))
        .unwrap();
    b.style(
        big,
        style(None, Some((Color::BLACK, 4.0, Dash::Solid)), 0.8),
    )
    .unwrap();
    for (i, deg) in [0.0, 45.0, 90.0, 180.0, 271.0].iter().enumerate() {
        let x = 30.0 + 50.0 * i as f64;
        let f = b
            .shape(
                p,
                Rect::new(x, 60.0 + 10.0 * i as f64, 70.0, 45.0),
                if i % 2 == 0 {
                    Shape::Rectangle { corner_radius: 0.0 }
                } else {
                    Shape::Ellipse
                },
            )
            .unwrap();
        b.style(
            f,
            style(
                Some(Color::rgba(40 * i as u8, 90, 200 - 30 * i as u8, 200)),
                Some((Color::rgb(20, 20, 20), 0.4 + i as f64, Dash::Dashed)),
                if i % 2 == 1 { 0.6 } else { 1.0 },
            ),
        )
        .unwrap();
        b.exec(Command::SetRotation {
            frame: f,
            degrees: *deg,
        })
        .unwrap();
    }
    let t = b
        .text(
            p,
            Rect::new(120.0, 100.0, 170.0, 110.0),
            "Tiles must agree with the whole page, pixel for pixel, at every seam. \
             Glyphs straddle tile edges here on purpose.",
        )
        .unwrap();
    let s = b.story_of(t);
    b.exec(Command::ApplyCharProps {
        story: s,
        start: 0,
        end: 40,
        props: CharProps {
            bold: Some(true),
            italic: Some(true),
            underline: Some(true),
            font_size: Some(15.0),
            ..Default::default()
        },
    })
    .unwrap();
    b.exec(Command::SetRotation {
        frame: t,
        degrees: 12.0,
    })
    .unwrap();
    let edge = b
        .shape(
            p,
            Rect::new(250.0, 150.0, 80.0, 80.0),
            Shape::Rectangle { corner_radius: 0.0 },
        )
        .unwrap();
    b.style(edge, style(Some(Color::rgb(250, 200, 0)), None, 1.0))
        .unwrap();
    // An empty picture frame (the editor's placeholder), a rotated SVG (drawn
    // through a clip mask) and a translucent SVG (inside an opacity layer).
    let empty = b
        .image(p, Rect::new(200.0, 18.0, 70.0, 40.0), None)
        .unwrap();
    b.exec(Command::SetRotation {
        frame: empty,
        degrees: 8.0,
    })
    .unwrap();
    let svg_id = add_asset(&mut b, ImageFormat::Svg, 100, 60);
    let tilted = b
        .image(p, Rect::new(10.0, 130.0, 100.0, 60.0), Some(svg_id))
        .unwrap();
    b.exec(Command::SetRotation {
        frame: tilted,
        degrees: 21.0,
    })
    .unwrap();
    let faint = b
        .image(p, Rect::new(95.0, 140.0, 90.0, 54.0), Some(svg_id))
        .unwrap();
    b.style(
        faint,
        style(None, Some((Color::BLACK, 1.5, Dash::Dotted)), 0.55),
    )
    .unwrap();
    let mut map = HashMap::new();
    map.insert(png_id, ImageContent::Raster(Arc::new(gradient())));
    map.insert(svg_id, ImageContent::Svg(Arc::new(svg_tree())));
    Page {
        name: "stress",
        doc: b.finish(),
        images: ImageSnapshot::new(map),
        opts: RasterOptions {
            dpi: 101.3,
            include_bleed: true,
            background: None,
        },
    }
}

/// Gradient fills (linear at several angles, radial, rotated, translucent,
/// off the page) and swatch tints: shader pixels must not depend on where
/// the tile starts. Odd resolution so edges fall mid-pixel.
fn colour() -> Page {
    let mut b = DocBuilder::new(SeqIds::new(7), small_page());
    let p = b.page(0);
    let ink = SwatchId::new(&mut b.ids);
    b.exec(Command::AddSwatch {
        swatch: Swatch {
            id: ink,
            name: "Ink".into(),
            value: SwatchValue::Cmyk(Cmyk::new(80.0, 20.0, 0.0, 10.0)),
        },
    })
    .unwrap();
    let stops = |a: SolidPaint, z: SolidPaint| {
        vec![
            GradientStop {
                offset: 0.0,
                color: a,
            },
            GradientStop {
                offset: 0.4,
                color: Color::rgb(250, 204, 21).into(),
            },
            GradientStop {
                offset: 1.0,
                color: z,
            },
        ]
    };
    let mut add = |rect: Rect, shape: Shape, fill: Paint, rotation: f64, opacity: f64| {
        let f = b.shape(p, rect, shape).unwrap();
        b.style(
            f,
            FrameStyle {
                fill: Some(fill),
                stroke: None,
                opacity,
            },
        )
        .unwrap();
        if rotation != 0.0 {
            b.exec(Command::SetRotation {
                frame: f,
                degrees: rotation,
            })
            .unwrap();
        }
    };
    let rectangle = Shape::Rectangle { corner_radius: 6.0 };
    for (i, angle) in [0.0, 33.0, 90.0, 215.0].into_iter().enumerate() {
        add(
            Rect::new(10.0 + 70.0 * i as f64, 10.0, 62.0, 50.0),
            rectangle,
            Paint::Gradient {
                gradient: GradientKind::Linear,
                stops: stops(SolidPaint::swatch(ink), Color::rgb(190, 24, 93).into()),
                angle,
            },
            0.0,
            1.0,
        );
    }
    add(
        Rect::new(30.0, 80.0, 120.0, 70.0),
        Shape::Ellipse,
        Paint::Gradient {
            gradient: GradientKind::Radial,
            stops: stops(Color::WHITE.into(), SolidPaint::swatch(ink)),
            angle: 0.0,
        },
        0.0,
        0.8,
    );
    add(
        Rect::new(120.0, 90.0, 110.0, 60.0),
        rectangle,
        Paint::Gradient {
            gradient: GradientKind::Linear,
            stops: stops(Color::BLACK.into(), Color::WHITE.into()),
            angle: 12.0,
        },
        27.0,
        0.7,
    );
    add(
        Rect::new(-25.0, 150.0, 120.0, 70.0),
        Shape::Ellipse,
        Paint::Gradient {
            gradient: GradientKind::Radial,
            stops: stops(Color::rgb(30, 58, 138).into(), Color::TRANSPARENT.into()),
            angle: 0.0,
        },
        0.0,
        1.0,
    );
    for (i, tint) in [25.0, 50.0, 100.0].into_iter().enumerate() {
        add(
            Rect::new(190.0 + 30.0 * i as f64, 160.0, 26.0, 30.0),
            Shape::Rectangle { corner_radius: 0.0 },
            Paint::Swatch { swatch: ink, tint },
            0.0,
            1.0,
        );
    }
    Page {
        name: "colour",
        doc: b.finish(),
        images: ImageSnapshot::new(HashMap::new()),
        opts: RasterOptions {
            dpi: 101.3,
            include_bleed: true,
            background: None,
        },
    }
}

fn scene(page: &Page) -> PageScene {
    let layout = Layouter::new(fonts()).layout_document(&page.doc);
    build_page_scene_with(
        &page.doc,
        &layout,
        0,
        pubrs_render::SceneOptions {
            show_empty_images: true,
        },
    )
    .unwrap()
}

fn tile_configs(threads: usize) -> Vec<TileOptions> {
    vec![
        TileOptions {
            threads,
            tile_width: None,
            tile_height: None,
        },
        TileOptions::fixed(threads, 37, 23),
        TileOptions::fixed(threads, 101, 13),
        TileOptions::fixed(threads, 7, 61),
        TileOptions::fixed(threads, 10_000, 1),
    ]
}

fn first_difference(a: &Pixmap, b: &Pixmap) -> Option<(u32, u32)> {
    let w = a.width() as usize;
    a.data()
        .chunks(4)
        .zip(b.data().chunks(4))
        .position(|(x, y)| x != y)
        .map(|i| ((i % w) as u32, (i / w) as u32))
}

#[test]
fn every_golden_page_tiles_byte_identically() {
    for page in [
        shapes(),
        text(),
        images(),
        master_bleed(),
        stress(),
        colour(),
    ] {
        let scene = scene(&page);
        let reference = RasterRenderer::new()
            .render(&scene, &page.images, &page.opts)
            .unwrap();
        for threads in [1, 2, 7, 16] {
            for options in tile_configs(threads) {
                let tiled = TileRenderer::new(options)
                    .render_page(&scene, &page.images, &page.opts)
                    .unwrap();
                assert_eq!(
                    (tiled.width(), tiled.height()),
                    (reference.width(), reference.height())
                );
                assert!(
                    tiled.data() == reference.data(),
                    "{}: tiled with {options:?} differs, first at {:?}",
                    page.name,
                    first_difference(&tiled, &reference)
                );
            }
        }
    }
}

/// Regions wider than 8191 px, where tiny-skia splits every draw into its
/// own sub-tiles (`DrawTiler`), tile exactly too.
#[test]
fn regions_wider_than_8191_px_tile_exactly() {
    let page = stress();
    let scene = scene(&page);
    let region = Region {
        opts: RasterOptions {
            dpi: 2160.0,
            ..page.opts
        },
        rect: PixelRect::new(0, 2900, 8300, 140),
        mips: false,
    };
    let reference = RasterRenderer::new()
        .render_region(&scene, &page.images, &region)
        .unwrap();
    let full = PixelRect::new(0, 0, 8300, 140);
    for options in [
        TileOptions::fixed(7, 4099, 23),
        TileOptions::fixed(16, 8300, 13),
        TileOptions {
            threads: 2,
            tile_width: None,
            tile_height: None,
        },
    ] {
        let out = TileRenderer::new(options)
            .render_rects(&scene, &page.images, &region, &[full])
            .unwrap();
        let tiled = &out[0].1;
        assert!(
            tiled.data() == reference.data(),
            "tiled with {options:?} differs, first at {:?}",
            first_difference(tiled, &reference)
        );
    }
}

#[test]
fn page_region_is_the_page_render() {
    let page = text();
    let scene = scene(&page);
    let whole = RasterRenderer::new()
        .render(&scene, &page.images, &page.opts)
        .unwrap();
    let region = Region::page(&scene, page.opts);
    let same = RasterRenderer::new()
        .render_region(&scene, &page.images, &region)
        .unwrap();
    assert!(whole.data() == same.data());
}

/// Viewport-style regions (offset, partly off the page, zoomed in or out
/// with image mips) and damage-style rectangles inside them match the
/// untiled region render.
#[test]
fn regions_and_damage_rects_match_the_untiled_region() {
    for page in [images(), stress(), text()] {
        let scene = scene(&page);
        for (dpi, rect, mips) in [
            (288.0, PixelRect::new(137, -40, 420, 333), false),
            (577.0, PixelRect::new(900, 610, 517, 389), false),
            (24.0, PixelRect::new(-7, 5, 100, 61), true),
            (11.0, PixelRect::new(0, 0, 48, 33), true),
        ] {
            let region = Region {
                opts: RasterOptions { dpi, ..page.opts },
                rect,
                mips,
            };
            let reference = RasterRenderer::new()
                .render_region(&scene, &page.images, &region)
                .unwrap();
            let rw = rect.width as i32;
            let rh = rect.height as i32;
            let damage = [
                PixelRect::new(0, 0, rect.width, rect.height),
                PixelRect::new(rw / 3, rh / 4, (rw / 2) as u32, (rh / 3) as u32),
                PixelRect::new(-5, rh - 9, 40, 30),
                PixelRect::new(rw - 17, 3, 11, (rh - 6) as u32),
            ];
            for threads in [1, 7, 16] {
                let mut tiles = TileRenderer::new(TileOptions::fixed(threads, 29, 17));
                let out = tiles
                    .render_rects(&scene, &page.images, &region, &damage)
                    .unwrap();
                assert_eq!(out.len(), damage.len());
                for (r, pm) in out {
                    let crop = reference
                        .clone_rect(
                            pubrs_render::tiny_skia::IntRect::from_xywh(
                                r.x, r.y, r.width, r.height,
                            )
                            .unwrap(),
                        )
                        .unwrap();
                    assert!(
                        pm.data() == crop.data(),
                        "{} at {dpi} dpi, region {rect:?}, rect {r:?}, {threads} threads: \
                         first difference at {:?}",
                        page.name,
                        first_difference(&pm, &crop)
                    );
                }
            }
        }
    }
}

#[test]
fn mips_are_used_only_when_asked() {
    let page = images();
    let scene = scene(&page);
    let opts = RasterOptions {
        dpi: 12.0,
        ..Default::default()
    };
    let plain = RasterRenderer::new()
        .render(&scene, &page.images, &opts)
        .unwrap();
    let region = Region {
        mips: true,
        ..Region::page(&scene, opts)
    };
    let mipped = RasterRenderer::new()
        .render_region(&scene, &page.images, &region)
        .unwrap();
    assert!(
        plain.data() != mipped.data(),
        "at 12 dpi the 120 px image is drawn below half size, so a mip level is sampled"
    );
    let again = RasterRenderer::new()
        .render(&scene, &page.images, &opts)
        .unwrap();
    assert!(plain.data() == again.data(), "exports never use mips");
}

/// Imposed sheets clip nodes to their cell; tiled output must still equal
/// the untiled render.
#[test]
fn clipped_nodes_tile_byte_identically() {
    let page = shapes();
    let mut scene = scene(&page);
    let n = scene.nodes.len();
    assert!(n >= 2);
    for (i, node) in scene.nodes.iter_mut().enumerate() {
        node.clip = Some(pubrs_core::Rect::new(
            20.0 + 10.0 * i as f64,
            15.0,
            120.0,
            90.0 + 5.0 * i as f64,
        ));
    }
    let reference = RasterRenderer::new()
        .render(&scene, &page.images, &page.opts)
        .unwrap();
    for threads in [1, 4] {
        for options in tile_configs(threads) {
            let tiled = TileRenderer::new(options)
                .render_page(&scene, &page.images, &page.opts)
                .unwrap();
            assert!(
                tiled.data() == reference.data(),
                "clipped: tiled with {options:?} differs, first at {:?}",
                first_difference(&tiled, &reference)
            );
        }
    }
}

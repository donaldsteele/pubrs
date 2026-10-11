//! WordArt warps, drop shadows and BorderArt (ADR 0020): geometry, exact
//! shadow pixels, damage coverage and the PDF's vector outlines.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use pubrs_core::*;
use pubrs_layout::{FontLibrary, Layouter};
use pubrs_render::scene::PathSeg;
use pubrs_render::{
    NoImages, PageDamage, PageState, PdfOptions, Prim, RasterOptions, RasterRenderer, SceneOptions,
    build_page_scene, build_page_scene_with, page_damage, render_pdf,
};

fn fonts() -> FontLibrary {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    FontLibrary::from_dir(&root.join("tests/fonts")).unwrap()
}

fn setup() -> PageSetup {
    PageSetup {
        size: Size::new(300.0, 200.0),
        margins: Margins::uniform(10.0),
        bleed: 0.0,
        ..PageSetup::default()
    }
}

fn art(text: &str, warp: Warp) -> WordArtFrame {
    WordArtFrame {
        text: text.into(),
        font: "Liberation Sans".into(),
        size: 60.0,
        warp,
        fill: Color::rgb(200, 30, 30).into(),
        outline: None,
        shadow: None,
    }
}

fn doc_with(frames: impl FnOnce(&mut DocBuilder<SeqIds>, PageId)) -> Document {
    let mut b = DocBuilder::new(SeqIds::new(3), setup());
    let p = b.page(0);
    frames(&mut b, p);
    b.finish()
}

fn scene_of(doc: &Document) -> pubrs_render::PageScene {
    let layout = Layouter::new(fonts()).layout_document(doc);
    build_page_scene(doc, &layout, 0).unwrap()
}

fn rect_shape(b: &mut DocBuilder<SeqIds>, p: PageId, r: Rect, style: FrameStyle) -> FrameId {
    let f = b
        .shape(p, r, Shape::Rectangle { corner_radius: 0.0 })
        .unwrap();
    b.style(f, style).unwrap();
    f
}

#[test]
fn every_warp_fits_its_frame_and_is_vector() {
    for warp in Warp::ALL {
        let doc = doc_with(|b, p| {
            b.wordart(
                p,
                Rect::new(20.0, 30.0, 220.0, 90.0),
                art("Hello World", warp),
            )
            .unwrap();
        });
        let scene = scene_of(&doc);
        assert_eq!(scene.nodes.len(), 1, "{warp:?}");
        let Prim::Path {
            path,
            origin,
            size,
            fill,
            ..
        } = &scene.nodes[0].prims[0]
        else {
            panic!("{warp:?}: the first primitive is a path");
        };
        assert!(fill.is_some());
        let (l, t, r, b) = path.bounds().unwrap();
        // The path is relative to its box, which lies inside the frame.
        assert!(l >= -0.01 && t >= -0.01, "{warp:?}: {l} {t}");
        assert!(r <= size.0 + 0.01 && b <= size.1 + 0.01, "{warp:?}");
        assert!(origin.0 >= -0.01 && origin.1 >= -0.01, "{warp:?}");
        assert!(
            origin.0 + size.0 <= 220.0 + 0.01,
            "{warp:?}: {origin:?} {size:?}"
        );
        assert!(
            origin.1 + size.1 <= 90.0 + 0.01,
            "{warp:?}: {origin:?} {size:?}"
        );
        // The box is centred.
        let (cx, cy) = (origin.0 + size.0 / 2.0, origin.1 + size.1 / 2.0);
        assert!(
            (cx - 110.0).abs() < 0.5 && (cy - 45.0).abs() < 0.5,
            "{warp:?}"
        );
        // Affine warps keep curves; the others are polylines.
        let curvy = path
            .segs
            .iter()
            .any(|s| matches!(s, PathSeg::Quad(..) | PathSeg::Cubic(..)));
        let affine = matches!(warp, Warp::None | Warp::SlantUp | Warp::SlantDown);
        assert_eq!(curvy, affine, "{warp:?}");
        // The same input gives the same path.
        let again = scene_of(&doc);
        let Prim::Path { path: p2, .. } = &again.nodes[0].prims[0] else {
            unreachable!()
        };
        assert_eq!(path.segs, p2.segs, "{warp:?}");
    }
}

#[test]
fn text_is_scaled_down_to_fit_never_up() {
    let size_of = |font_size: f64, w: f64, h: f64| {
        let doc = doc_with(|b, p| {
            let mut a = art("Fit", Warp::None);
            a.size = font_size;
            b.wordart(p, Rect::new(10.0, 10.0, w, h), a).unwrap();
        });
        let scene = scene_of(&doc);
        let Prim::Path { size, .. } = &scene.nodes[0].prims[0] else {
            panic!()
        };
        *size
    };
    let small = size_of(20.0, 280.0, 180.0);
    let big = size_of(40.0, 280.0, 180.0);
    assert!(
        (big.0 / small.0 - 2.0).abs() < 0.02,
        "nominal size scales the text"
    );
    // A frame smaller than the text shrinks it into the frame.
    let squeezed = size_of(200.0, 100.0, 50.0);
    assert!(squeezed.0 <= 100.01 && squeezed.1 <= 50.01, "{squeezed:?}");
    // Aspect ratio is kept.
    let ratio = |s: (f64, f64)| s.0 / s.1;
    assert!((ratio(squeezed) - ratio(size_of(200.0, 1000.0, 1000.0))).abs() < 0.01);
}

#[test]
fn empty_wordart_draws_nothing_without_a_style() {
    let doc = doc_with(|b, p| {
        b.wordart(
            p,
            Rect::new(10.0, 10.0, 100.0, 50.0),
            art("   ", Warp::Wave),
        )
        .unwrap();
    });
    assert!(scene_of(&doc).nodes.is_empty());
}

fn render(doc: &Document, dpi: f64, background: Option<Color>) -> pubrs_render::tiny_skia::Pixmap {
    let scene = scene_of(doc);
    RasterRenderer::new()
        .render(
            &scene,
            &NoImages,
            &RasterOptions {
                dpi,
                include_bleed: false,
                background,
            },
        )
        .unwrap()
}

fn shadowed(dx: f64, dy: f64, blur: f64, opacity: f64) -> Document {
    doc_with(|b, p| {
        rect_shape(
            b,
            p,
            Rect::new(40.0, 40.0, 60.0, 40.0),
            FrameStyle {
                fill: Some(Color::rgb(0, 0, 255).into()),
                shadow: Some(Shadow {
                    offset_x: dx,
                    offset_y: dy,
                    blur,
                    color: Color::rgb(255, 0, 0).into(),
                    opacity,
                }),
                ..Default::default()
            },
        );
    })
}

#[test]
fn a_hard_shadow_is_the_silhouette_moved_whole_pixels() {
    let pm = render(&shadowed(5.0, 7.0, 0.0, 1.0), 72.0, None);
    let px = |x: u32, y: u32| pm.pixel(x, y).unwrap();
    // Under the frame: its own blue. Right of it, inside the shadow: red.
    assert_eq!((px(70, 60).red(), px(70, 60).blue()), (0, 255));
    let s = px(103, 60);
    assert_eq!((s.red(), s.green(), s.blue(), s.alpha()), (255, 0, 0, 255));
    let below = px(60, 85);
    assert_eq!((below.red(), below.alpha()), (255, 255));
    // Outside it: untouched (transparent).
    assert_eq!(px(106, 60).alpha(), 0);
    assert_eq!(px(60, 88).alpha(), 0);
    assert_eq!(px(42, 45).blue(), 255);
}

#[test]
fn a_soft_shadow_fades_out_within_three_radii() {
    let pm = render(&shadowed(0.0, 0.0, 6.0, 1.0), 72.0, None);
    // Blur 6 pt at 72 dpi: radius 3 px per pass, reach 9 px.
    let a = |x: u32, y: u32| pm.pixel(x, y).unwrap().alpha();
    assert_eq!(a(100 + 10, 60), 0, "beyond the reach");
    let ramp: Vec<u8> = (100..110).map(|x| a(x, 60)).collect();
    assert!(ramp.windows(2).all(|w| w[0] >= w[1]), "monotone: {ramp:?}");
    assert!(
        ramp[0] > 60 && ramp[0] < 200,
        "about half strength at the edge: {ramp:?}"
    );
    // Opacity scales it.
    let half = render(&shadowed(0.0, 0.0, 6.0, 0.5), 72.0, None);
    let h = half.pixel(101, 60).unwrap().alpha();
    assert!((i32::from(h) * 2 - i32::from(a(101, 60))).abs() <= 2);
}

#[test]
fn a_shadow_of_a_frame_hanging_off_the_page_still_falls_on_it() {
    let doc = doc_with(|b, p| {
        rect_shape(
            b,
            p,
            Rect::new(-80.0, 20.0, 90.0, 40.0),
            FrameStyle {
                fill: Some(Color::rgb(0, 0, 255).into()),
                shadow: Some(Shadow {
                    offset_x: 8.0,
                    offset_y: 0.0,
                    blur: 0.0,
                    color: Color::BLACK.into(),
                    opacity: 1.0,
                }),
                ..Default::default()
            },
        );
    });
    let pm = render(&doc, 72.0, None);
    assert_eq!(pm.pixel(14, 30).unwrap().alpha(), 255, "shadow on the page");
    assert_eq!(pm.pixel(14, 30).unwrap().blue(), 0);
}

#[test]
fn damage_covers_every_pixel_a_wordart_or_shadow_edit_changes() {
    let before = doc_with(|b, p| {
        b.wordart(
            p,
            Rect::new(20.0, 20.0, 200.0, 80.0),
            art("Damage", Warp::ArchUp),
        )
        .unwrap();
        rect_shape(
            b,
            p,
            Rect::new(40.0, 120.0, 60.0, 40.0),
            FrameStyle {
                fill: Some(Color::rgb(0, 0, 255).into()),
                ..Default::default()
            },
        );
    });
    let wa = before
        .frames
        .values()
        .find(|f| f.as_wordart().is_some())
        .unwrap()
        .id;
    let rect = before
        .frames
        .values()
        .find(|f| f.kind() == FrameKind::Shape)
        .unwrap()
        .id;
    let edits = [
        Command::SetWordArt {
            frame: wa,
            art: WordArtFrame {
                shadow: Some(Shadow {
                    offset_x: 6.0,
                    offset_y: 5.0,
                    blur: 5.0,
                    color: Color::BLACK.into(),
                    opacity: 0.7,
                }),
                ..art("Damage", Warp::Circle)
            },
        },
        Command::SetFrameStyle {
            frame: rect,
            style: FrameStyle {
                fill: Some(Color::rgb(0, 0, 255).into()),
                shadow: Some(Shadow {
                    offset_x: -9.0,
                    offset_y: 9.0,
                    blur: 7.0,
                    color: Color::BLACK.into(),
                    opacity: 0.9,
                }),
                border_art: Some(BorderArt {
                    pattern: "dots".into(),
                    width: 12.0,
                }),
                ..Default::default()
            },
        },
    ];
    for cmd in edits {
        let mut ed = Editor::new(before.clone());
        let change = ed.execute(cmd).unwrap();
        let after = ed.doc().clone();
        let mut layouter = Layouter::new(fonts());
        let (ol, nl) = (
            layouter.layout_document(&before),
            layouter.layout_document(&after),
        );
        let damage = page_damage(
            PageState {
                doc: &before,
                layout: &ol,
                page: 0,
            },
            PageState {
                doc: &after,
                layout: &nl,
                page: 0,
            },
            Some(&change.touched),
        );
        let PageDamage::Rects(rects) = damage else {
            panic!("expected rectangles");
        };
        let (a, b) = (
            render(&before, 72.0, Some(Color::WHITE)),
            render(&after, 72.0, Some(Color::WHITE)),
        );
        let mut changed = 0;
        for y in 0..a.height() {
            for x in 0..a.width() {
                if a.pixel(x, y) != b.pixel(x, y) {
                    changed += 1;
                    let (px, py) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
                    assert!(
                        rects
                            .iter()
                            .any(|r| px >= r.x && px <= r.right() && py >= r.y && py <= r.bottom()),
                        "pixel ({x}, {y}) changed outside the damage {rects:?}"
                    );
                }
            }
        }
        assert!(changed > 100, "the edit should change pixels ({changed})");
    }
}

fn bordered(w: f64, h: f64, width: f64) -> Document {
    doc_with(|b, p| {
        rect_shape(
            b,
            p,
            Rect::new(20.0, 20.0, w, h),
            FrameStyle {
                border_art: Some(BorderArt {
                    pattern: "stars".into(),
                    width,
                }),
                ..Default::default()
            },
        );
    })
}

#[test]
fn border_art_tiles_a_whole_number_per_edge() {
    let scene = scene_of(&bordered(220.0, 140.0, 22.0));
    let svgs: Vec<Rect> = scene.nodes[0]
        .prims
        .iter()
        .filter_map(|p| match p {
            Prim::Svg { clip, .. } => Some(*clip),
            _ => None,
        })
        .collect();
    // 4 corners; 176 / 22 = 8 tiles on the long edges, 96 / 22 -> 4 on the short.
    assert_eq!(svgs.len(), 4 + 2 * 8 + 2 * 4);
    // Every piece lies inside the frame, against one of its edges.
    for c in &svgs {
        assert!(c.x >= -1e-9 && c.y >= -1e-9, "{c:?}");
        assert!(
            c.right() <= 220.0 + 1e-9 && c.bottom() <= 140.0 + 1e-9,
            "{c:?}"
        );
        let near_edge =
            c.x < 1e-6 || c.y < 1e-6 || c.right() > 220.0 - 1e-6 || c.bottom() > 140.0 - 1e-6;
        assert!(near_edge, "{c:?}");
    }
    // Top-edge tiles run end to end with no gaps.
    let mut top: Vec<&Rect> = svgs
        .iter()
        .filter(|c| c.y < 1e-6 && c.x > 1.0 && c.right() < 219.0)
        .collect();
    top.sort_by(|a, b| a.x.total_cmp(&b.x));
    assert_eq!(top.len(), 8);
    for w in top.windows(2) {
        assert!((w[0].right() - w[1].x).abs() < 1e-6);
    }
    // A frame too small for its border draws none.
    assert!(scene_of(&bordered(30.0, 30.0, 22.0)).nodes.is_empty());
}

#[test]
fn every_border_pattern_is_bundled() {
    for name in BORDER_ART_PATTERNS {
        let pat = pubrs_render::scene::border_pattern(name).unwrap_or_else(|| panic!("{name}"));
        assert!(pat.tile.size().width() > 0.0 && pat.corner.size().height() > 0.0);
    }
    assert!(pubrs_render::scene::border_pattern("nope").is_none());
}

#[test]
fn text_inside_border_art_is_inset_by_its_width() {
    let make = |border: Option<BorderArt>| {
        let doc = doc_with(|b, p| {
            let f = b
                .text(p, Rect::new(20.0, 20.0, 240.0, 150.0), "Inset")
                .unwrap();
            b.style(
                f,
                FrameStyle {
                    border_art: border,
                    ..Default::default()
                },
            )
            .unwrap();
        });
        let layout = Layouter::new(fonts()).layout_document(&doc);
        let fid = doc.frames.values().next().unwrap().id;
        layout.frame(fid).unwrap().lines[0].runs[0].glyphs[0].x
    };
    let plain = make(None);
    let with = make(Some(BorderArt {
        pattern: "hearts".into(),
        width: 20.0,
    }));
    assert!((with - plain - 20.0).abs() < 1e-6, "{plain} {with}");
}

#[test]
fn pdf_has_wordart_as_outlines_and_shadows_as_images() {
    let doc = doc_with(|b, p| {
        let mut a = art("Outlined", Warp::Wave);
        a.shadow = Some(Shadow {
            offset_x: 4.0,
            offset_y: 4.0,
            blur: 3.0,
            color: Color::BLACK.into(),
            opacity: 0.5,
        });
        b.wordart(p, Rect::new(20.0, 20.0, 240.0, 100.0), a)
            .unwrap();
    });
    let layout = Layouter::new(fonts()).layout_document(&doc);
    let scene = build_page_scene_with(&doc, &layout, 0, SceneOptions::default()).unwrap();
    let pdf = render_pdf(&[scene], &NoImages, &PdfOptions::default()).unwrap();
    let text = String::from_utf8_lossy(&pdf);
    // No font was embedded for the letters, but a soft-masked image was.
    assert!(!text.contains("/FontFile"), "WordArt is outlines, not text");
    assert!(text.contains("/SMask"), "the shadow carries a soft mask");
}

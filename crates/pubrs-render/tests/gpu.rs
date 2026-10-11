//! vello vs tiny-skia: the GPU path draws the same page (within AA noise).
//! Skips when no GPU adapter exists. Run with `--features gpu`.
#![cfg(feature = "gpu")]

use std::path::PathBuf;

use pubrs_core::*;
use pubrs_layout::{FontLibrary, Layouter};
use pubrs_render::gpu::GpuRenderer;
use pubrs_render::{NoImages, RasterOptions, RasterRenderer, build_page_scene};

#[test]
fn gpu_matches_software_within_tolerance() {
    // Software adapters are fine for vello on Linux (llvmpipe); WARP crashes.
    let mut gpu = match GpuRenderer::with_software(!cfg!(windows)) {
        Ok(g) => {
            eprintln!("adapter: {}", g.adapter());
            g
        }
        Err(e) => {
            eprintln!("skipping: {e}");
            return;
        }
    };
    let mut b = DocBuilder::new(
        SeqIds::new(9),
        PageSetup {
            size: Size::new(300.0, 200.0),
            margins: Margins::uniform(18.0),
            bleed: 0.0,
            ..PageSetup::default()
        },
    );
    let p = b.page(0);
    let r = b
        .shape(
            p,
            Rect::new(20.0, 20.0, 120.0, 80.0),
            Shape::Rectangle {
                corner_radius: 10.0,
            },
        )
        .unwrap();
    b.style(
        r,
        FrameStyle {
            fill: Some(Color::rgb(30, 120, 220)),
            stroke: None,
            opacity: 1.0,
        },
    )
    .unwrap();
    let e = b
        .shape(p, Rect::new(100.0, 60.0, 150.0, 100.0), Shape::Ellipse)
        .unwrap();
    b.style(
        e,
        FrameStyle {
            fill: Some(Color::rgb(220, 40, 40)),
            stroke: None,
            opacity: 0.5,
        },
    )
    .unwrap();
    b.text(p, Rect::new(20.0, 120.0, 260.0, 60.0), "GPU and CPU agree")
        .unwrap();
    let doc = b.finish();
    let fonts =
        FontLibrary::from_dir(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fonts"))
            .unwrap();
    let layout = Layouter::new(fonts).layout_document(&doc);
    let scene = build_page_scene(&doc, &layout, 0).unwrap();
    let opts = RasterOptions {
        dpi: 144.0,
        ..Default::default()
    };
    let cpu = RasterRenderer::new()
        .render(&scene, &NoImages, &opts)
        .unwrap()
        .take_demultiplied();
    let (w, h, gpu_px) = gpu.render_rgba(&scene, &NoImages, &opts).unwrap();
    assert_eq!((w, h), (600, 400));
    let total: u64 = cpu
        .iter()
        .zip(&gpu_px)
        .map(|(a, b)| u64::from(a.abs_diff(*b)))
        .sum();
    let mean = total as f64 / cpu.len() as f64;
    eprintln!(
        "adapter {}: mean channel difference {mean:.3}",
        gpu.adapter()
    );
    assert!(
        mean < 2.0,
        "GPU output differs too much from tiny-skia: {mean}"
    );
}

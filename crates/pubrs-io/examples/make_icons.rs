//! Regenerate the app and document icons from packaging/icons/*.svg:
//! `cargo run -p pubrs-io --example make_icons`
//!
//! Writes PNGs (16–1024 px), a Windows .ico (PNG entries) and a macOS .icns.

use std::path::Path;

fn render(svg: &[u8], px: u32) -> Vec<u8> {
    let tree = usvg::Tree::from_data(svg, pubrs_io::images::svg_options()).expect("valid svg");
    let size = tree.size();
    let s = px as f32 / size.width().max(size.height());
    let mut pm = resvg::tiny_skia::Pixmap::new(px, px).expect("pixmap");
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(s, s),
        &mut pm.as_mut(),
    );
    pm.encode_png().expect("png")
}

fn ico(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (px, png) in images {
        let dim = if *px >= 256 { 0 } else { *px as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in images {
        out.extend_from_slice(png);
    }
    out
}

fn icns(entries: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let body: usize = entries.iter().map(|(_, d)| 8 + d.len()).sum();
    let mut out = Vec::new();
    out.extend_from_slice(b"icns");
    out.extend_from_slice(&((8 + body) as u32).to_be_bytes());
    for (kind, data) in entries {
        out.extend_from_slice(*kind);
        out.extend_from_slice(&((8 + data.len()) as u32).to_be_bytes());
        out.extend_from_slice(data);
    }
    out
}

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/icons");
    for (name, svg_file) in [
        ("pubrs", "pubrs.svg"),
        ("pubrs-document", "pubrs-document.svg"),
    ] {
        let svg = std::fs::read(dir.join(svg_file)).expect("svg");
        let sizes = [16u32, 24, 32, 48, 64, 128, 256, 512, 1024];
        let pngs: Vec<(u32, Vec<u8>)> = sizes.iter().map(|s| (*s, render(&svg, *s))).collect();
        for (s, png) in &pngs {
            std::fs::write(dir.join(format!("{name}-{s}.png")), png).expect("write");
        }
        let pick = |s: u32| pngs.iter().find(|p| p.0 == s).expect("size").1.clone();
        let ico_sizes = [16u32, 24, 32, 48, 64, 128, 256];
        let ico_imgs: Vec<(u32, Vec<u8>)> = ico_sizes.iter().map(|s| (*s, pick(*s))).collect();
        std::fs::write(dir.join(format!("{name}.ico")), ico(&ico_imgs)).expect("ico");
        let entries: Vec<(&[u8; 4], Vec<u8>)> = vec![
            (b"icp4", pick(16)),
            (b"icp5", pick(32)),
            (b"icp6", pick(64)),
            (b"ic07", pick(128)),
            (b"ic08", pick(256)),
            (b"ic09", pick(512)),
            (b"ic10", pick(1024)),
        ];
        std::fs::write(dir.join(format!("{name}.icns")), icns(&entries)).expect("icns");
        println!("wrote {name} icons");
    }
}

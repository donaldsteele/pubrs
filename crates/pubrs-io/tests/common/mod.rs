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

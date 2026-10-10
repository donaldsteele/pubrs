//! Runs the real `pubrs` binary: script replay, validate, render, export.

use std::path::PathBuf;
use std::process::Command;

fn bin() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_pubrs"));
    c.arg("--no-system-fonts");
    c
}

#[test]
fn script_then_validate_render_export() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cli");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    let st = bin()
        .arg("script")
        .arg(root.join("examples/scripts/flyer.json"))
        .arg("--out")
        .arg(&out)
        .status()
        .unwrap();
    assert!(st.success());
    let doc = out.join("flyer.pubrs");
    assert!(doc.exists());

    let v = bin()
        .args(["validate", "--json"])
        .arg(&doc)
        .output()
        .unwrap();
    assert!(v.status.success(), "{}", String::from_utf8_lossy(&v.stdout));
    let report: serde_json::Value = serde_json::from_slice(&v.stdout).unwrap();
    assert_eq!(report["ok"], true);

    let png = out.join("page.png");
    assert!(
        bin()
            .args(["render", "--dpi", "36", "--page", "1", "-o"])
            .arg(&png)
            .arg(&doc)
            .status()
            .unwrap()
            .success()
    );
    assert!(std::fs::read(&png).unwrap().starts_with(b"\x89PNG"));

    let pdf = out.join("out.pdf");
    assert!(
        bin()
            .args(["export", "--crop-marks", "-o"])
            .arg(&pdf)
            .arg(&doc)
            .status()
            .unwrap()
            .success()
    );
    assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF-"));
}

#[test]
fn failing_script_and_missing_file_exit_nonzero() {
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cli-fail");
    std::fs::create_dir_all(&out).unwrap();
    let script = out.join("bad.json");
    std::fs::write(&script, r#"[{"tool": "doc_info"}]"#).unwrap();
    let st = bin().arg("script").arg(&script).status().unwrap();
    assert_eq!(st.code(), Some(2));
    let st = bin()
        .args(["validate", "/definitely/not/here.pubrs"])
        .status()
        .unwrap();
    assert_eq!(st.code(), Some(2));
}

/// The sample every installer ships opens, has no print problems and exports.
#[test]
fn bundled_sample_validates_and_exports() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let sample = root.join("examples/samples/bakery-trifold.pubrs");
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cli-sample");
    std::fs::create_dir_all(&out).unwrap();

    let v = bin()
        .args(["validate", "--json"])
        .arg(&sample)
        .output()
        .unwrap();
    assert!(v.status.success(), "{}", String::from_utf8_lossy(&v.stdout));
    let report: serde_json::Value = serde_json::from_slice(&v.stdout).unwrap();
    assert_eq!(report["ok"], true, "{report}");

    let pdf = out.join("sample.pdf");
    assert!(
        bin()
            .args(["export", "-o"])
            .arg(&pdf)
            .arg(&sample)
            .status()
            .unwrap()
            .success()
    );
    let bytes = std::fs::read(&pdf).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    assert!(bytes.windows(9).any(|w| w == b"/BleedBox"));
}

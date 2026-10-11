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

/// The sample every installer ships opens, has no print problems (but alt text) and exports.
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
    let report: serde_json::Value = serde_json::from_slice(&v.stdout).unwrap();
    // The sample is byte-pinned as the schema-1 fixture, from before alt text
    // existed: its two pictures have none, and that is all the checker finds.
    let codes: Vec<&str> = report["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["code"].as_str().unwrap())
        .collect();
    assert_eq!(codes, ["missing_alt_text", "missing_alt_text"], "{report}");
    assert_eq!(v.status.code(), Some(1));
    // --commercial also flags the RGB colours of a CMYK job.
    let c = bin()
        .args(["validate", "--json", "--commercial"])
        .arg(&sample)
        .output()
        .unwrap();
    let report: serde_json::Value = serde_json::from_slice(&c.stdout).unwrap();
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["code"] == "rgb_in_cmyk_job"),
        "{report}"
    );

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

#[test]
fn perf_table_prints_keystroke_rows() {
    let out = bin()
        .args(["perf", "--pages", "2", "--runs", "1", "--check"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("measurement"), "{text}");
    assert!(text.contains("verdict"), "{text}");
    assert!(text.contains("keystroke: command"), "{text}");
    assert!(text.contains("keystroke: relayout"), "{text}");
    assert!(text.contains("keystroke: total"), "{text}");
}

#[test]
fn import_pub_then_export_and_validate() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cli-import");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    let src = root.join("tests/corpus/pub/poi/SampleNewsletter.pub");
    let dst = out.join("newsletter.pubrs");
    let r = bin()
        .arg("import")
        .arg(&src)
        .arg("-o")
        .arg(&dst)
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let text = String::from_utf8_lossy(&r.stdout);
    assert!(text.starts_with("Imported 4 pages, "), "{text}");
    assert!(text.contains("1 picture"), "{text}");
    assert!(dst.exists());

    // --json prints the report.
    let r = bin()
        .arg("import")
        .arg(&src)
        .arg("-o")
        .arg(out.join("again.pubrs"))
        .arg("--json")
        .output()
        .unwrap();
    assert!(r.status.success());
    let full: serde_json::Value = serde_json::from_slice(&r.stdout).unwrap();
    assert!(full["output"].as_str().unwrap().ends_with("again.pubrs"));
    let rep = &full["report"];
    assert_eq!(rep["pages"], 4);
    assert_eq!(rep["pictures"], 1);
    assert!(
        rep["dropped"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["kind"] == "picture-wmf")
    );
    assert!(
        rep["dropped"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["kind"] == "font-fallback")
    );

    // The imported document exports and validates like any other.
    let pdf = out.join("newsletter.pdf");
    let st = bin()
        .arg("export")
        .arg(&dst)
        .arg("-o")
        .arg(&pdf)
        .status()
        .unwrap();
    assert!(st.success());
    assert!(std::fs::metadata(&pdf).unwrap().len() > 1000);
    let v = bin()
        .arg("validate")
        .arg(&dst)
        .arg("--json")
        .output()
        .unwrap();
    assert!(!v.stdout.is_empty());
    let _: serde_json::Value = serde_json::from_slice(&v.stdout).unwrap();

    // The default output (next to the input) is never overwritten; an explicit -o is.
    let copy = out.join("copy.pub");
    std::fs::copy(&src, &copy).unwrap();
    let r = bin().arg("import").arg(&copy).output().unwrap();
    assert!(r.status.success());
    let r = bin().arg("import").arg(&copy).output().unwrap();
    assert!(!r.status.success());
    assert!(String::from_utf8_lossy(&r.stderr).contains("already exists"));
    let r = bin()
        .arg("import")
        .arg(&copy)
        .arg("-o")
        .arg(out.join("copy.pubrs"))
        .output()
        .unwrap();
    assert!(r.status.success());

    // A missing file is an error, not a panic.
    let r = bin()
        .arg("import")
        .arg(out.join("nope.pub"))
        .output()
        .unwrap();
    assert!(!r.status.success());
    assert!(String::from_utf8_lossy(&r.stderr).contains("file_not_found"));
}

/// `pubrs merge`: a design with two fields and a CSV give a PDF with a page
/// per record, and a new publication with the same pages.
#[test]
fn merge_writes_a_page_per_record_as_pdf_or_publication() {
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cli-merge");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    let script = out.join("design.json");
    std::fs::write(
        &script,
        r#"[
  {"tool": "doc_new", "args": {"page_size": {"width": 300, "height": 120}, "title": "Card"}, "as": "doc"},
  {"tool": "frame_add_text", "args": {"x": 10, "y": 10, "width": 280, "height": 100, "text": "Dear , from ."}, "as": "t"},
  {"tool": "merge_set_source", "args": {"inline_csv": "Name,City\nx,y\n"}},
  {"tool": "merge_insert_field", "args": {"frame_id": "${t.frame_id}", "offset": 5, "field": "Name"}},
  {"tool": "merge_insert_field", "args": {"frame_id": "${t.frame_id}", "offset": 13, "field": "City"}},
  {"tool": "merge_clear", "args": {"confirm": true}},
  {"tool": "doc_save_as", "args": {"path": "${out_dir}/design.pubrs"}}
]"#,
    )
    .unwrap();
    assert!(
        bin()
            .arg("script")
            .arg(&script)
            .arg("--out")
            .arg(&out)
            .status()
            .unwrap()
            .success()
    );
    let design = out.join("design.pubrs");
    // Semicolon separated, quoted fields with a comma and a newline.
    let csv = out.join("people.csv");
    std::fs::write(
        &csv,
        "\u{feff}Name;City\nAnn Lee;Oslo\n\"Kim; Bo\";\"Rome\nItaly\"\nCy Fox;Paris\n",
    )
    .unwrap();

    let pdf = out.join("merged.pdf");
    let r = bin()
        .args(["merge"])
        .arg(&design)
        .arg("--data")
        .arg(&csv)
        .arg("-o")
        .arg(&pdf)
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let say = String::from_utf8_lossy(&r.stdout);
    assert!(
        say.contains("3 records") && say.contains("3 pages"),
        "{say}"
    );
    let bytes = std::fs::read(&pdf).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));

    let publication = out.join("merged.pubrs");
    let r = bin()
        .args(["merge"])
        .arg(&design)
        .arg("--data")
        .arg(&csv)
        .args(["--as", "publication", "-o"])
        .arg(&publication)
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let v = bin()
        .args(["validate", "--json"])
        .arg(&publication)
        .output()
        .unwrap();
    let report: serde_json::Value = serde_json::from_slice(&v.stdout).unwrap();
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|w| w["code"] != "merge_field_missing" && w["code"] != "overflow"),
        "{report}"
    );
    let render = bin()
        .args(["render", "--dpi", "36", "--page", "3", "-o"])
        .arg(out.join("p3.png"))
        .arg(&publication)
        .status()
        .unwrap();
    assert!(render.success(), "the publication has a third page");

    // No data anywhere: an error with a hint, and a non-zero exit.
    let r = bin().args(["merge"]).arg(&design).output().unwrap();
    assert!(!r.status.success());
    assert!(String::from_utf8_lossy(&r.stderr).contains("merge_set_source"));
}

/// The address_labels template merged with 30 records: one 5160 sheet.
#[test]
fn merge_address_labels_template_gives_thirty_labels_on_one_sheet() {
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cli-labels");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    let script = out.join("labels.json");
    std::fs::write(
        &script,
        r#"[
  {"tool": "template_apply", "args": {"name": "address_labels"}},
  {"tool": "doc_save_as", "args": {"path": "${out_dir}/labels.pubrs"}}
]"#,
    )
    .unwrap();
    assert!(
        bin()
            .arg("script")
            .arg(&script)
            .arg("--out")
            .arg(&out)
            .status()
            .unwrap()
            .success()
    );
    let mut csv = String::from("Name,Address,City,State,Zip\n");
    for i in 1..=30 {
        csv.push_str(&format!(
            "Person {i},{i} Main St,Town {i},TX,{}\n",
            75000 + i
        ));
    }
    let list = out.join("list.csv");
    std::fs::write(&list, csv).unwrap();
    let pdf = out.join("labels.pdf");
    let r = bin()
        .arg("merge")
        .arg(out.join("labels.pubrs"))
        .arg("--data")
        .arg(&list)
        .arg("-o")
        .arg(&pdf)
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let say = String::from_utf8_lossy(&r.stdout);
    assert!(
        say.contains("30 records") && say.contains("1 pages"),
        "{say}"
    );
}

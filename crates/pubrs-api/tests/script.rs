//! Script replay: runs examples/scripts/flyer.json through the same tool
//! registry the MCP server uses, then checks the outputs.

use std::collections::BTreeMap;
use std::path::PathBuf;

use pubrs_api::{ScriptFile, Session, SessionConfig, run_script};
use serde_json::{Value, json};

#[test]
fn flyer_script_replays_and_exports() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script_path = root.join("examples/scripts/flyer.json");
    let script: ScriptFile =
        serde_json::from_str(&std::fs::read_to_string(&script_path).unwrap()).unwrap();
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("script-flyer");
    std::fs::create_dir_all(&out).unwrap();
    let mut vars = BTreeMap::new();
    vars.insert(
        "script_dir".into(),
        json!(script_path.parent().unwrap().display().to_string()),
    );
    vars.insert("out_dir".into(), json!(out.display().to_string()));
    let mut s = Session::new(SessionConfig {
        id_seed: Some(3),
        font_dir: Some(root.join("tests/fonts")),
        ..Default::default()
    });
    let steps = script.steps();
    let report = run_script(&mut s, &steps, vars);
    for r in &report.results {
        assert!(r.ok, "step {} ({}) failed: {:?}", r.index, r.tool, r.error);
    }
    assert!(report.ok);
    let by_tool = |name: &str| -> &Value {
        report
            .results
            .iter()
            .find(|r| r.tool == name)
            .unwrap()
            .output
            .as_ref()
            .unwrap()
    };
    assert_eq!(
        by_tool("validate")["ok"],
        json!(true),
        "{}",
        by_tool("validate")
    );
    assert_eq!(
        by_tool("text_overflow_status")["any_overflow"],
        json!(false)
    );
    let pdf = lopdf::Document::load(out.join("flyer.pdf")).unwrap();
    assert_eq!(pdf.get_pages().len(), 1);
    assert!(out.join("flyer.png").exists());
    let reopened = pubrs_api::io::load(&out.join("flyer.pubrs")).unwrap();
    assert_eq!(reopened.frames.len(), 7);
    assert_eq!(reopened.assets.len(), 2);

    // Replaying the same script in a fresh session with the same seed gives the same document.
    let mut s2 = Session::new(SessionConfig {
        id_seed: Some(3),
        font_dir: Some(root.join("tests/fonts")),
        ..Default::default()
    });
    let mut vars = BTreeMap::new();
    vars.insert(
        "script_dir".into(),
        json!(script_path.parent().unwrap().display().to_string()),
    );
    let out2 = out.join("second");
    std::fs::create_dir_all(&out2).unwrap();
    vars.insert("out_dir".into(), json!(out2.display().to_string()));
    assert!(run_script(&mut s2, &steps, vars).ok);
    let again = pubrs_api::io::load(&out2.join("flyer.pubrs")).unwrap();
    assert_eq!(again, reopened, "deterministic replay");
}

#[test]
fn expected_errors_and_unknown_tools() {
    let mut s = Session::default();
    let steps: Vec<pubrs_api::Step> = serde_json::from_value(json!([
        {"tool": "doc_info", "expect_error": "no_document"},
        {"tool": "doc_new", "args": {}},
        {"tool": "frame_delete", "args": {"frame_id": "00000000-0000-0000-0000-000000000001", "confirm": true}, "expect_error": "not_found"},
        {"tool": "frame_add_text", "args": {"x": 1, "y": 2, "width": 3, "height": 4, "bogus": 1}, "expect_error": "invalid_argument"},
        {"tool": "no_such_tool", "expect_error": "unknown_tool"}
    ]))
    .unwrap();
    let report = run_script(&mut s, &steps, BTreeMap::new());
    assert!(report.ok, "{:#?}", report.results);
}

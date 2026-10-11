//! Templates through the API.

use std::path::PathBuf;

use pubrs_api::core::WarningCode;
use pubrs_api::tools::doc::DocRef;
use pubrs_api::tools::templates::*;
use pubrs_api::{Session, SessionConfig};

fn session(tag: &str) -> Session {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("user-templates-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    Session::new(SessionConfig {
        id_seed: Some(9),
        font_dir: Some(root.join("tests/fonts")),
        user_templates_dir: Some(dir),
        ..Default::default()
    })
}

#[test]
fn every_builtin_applies_with_only_placeholder_warnings() {
    let mut s = session("builtins");
    let list = s.template_list(TemplateList {}).unwrap().templates;
    assert_eq!(list.len(), 4);
    for t in list {
        let out = s
            .template_apply(TemplateApply {
                name: t.name.clone(),
                title: Some(format!("My {}", t.title)),
            })
            .unwrap();
        assert_eq!(out.placeholders.len(), t.placeholders.len());
        let v = s.validate(DocRef::default()).unwrap();
        // Full-width bands run to the page edge on purpose; the checker says
        // so (object_in_unprintable_area), and finds nothing else wrong.
        for w in &v.warnings {
            assert!(
                matches!(
                    w.code,
                    WarningCode::EmptyPlaceholder | WarningCode::ObjectInUnprintableArea
                ),
                "{}: {w:?}",
                t.name
            );
        }
        let empty = v
            .warnings
            .iter()
            .filter(|w| w.code == WarningCode::EmptyPlaceholder)
            .count();
        assert!(empty <= t.placeholders.len(), "{}", t.name);
        assert!(empty > 0, "{}: unfilled template should warn", t.name);
    }
}

#[test]
fn user_templates_round_trip() {
    let mut s = session("user");
    s.template_apply(TemplateApply {
        name: "business_card".into(),
        title: None,
    })
    .unwrap();
    let saved = s
        .template_save(TemplateSave {
            doc_id: None,
            name: "my_card".into(),
        })
        .unwrap();
    assert!(saved.path.ends_with("my_card.pubrs"));
    let names: Vec<String> = s
        .template_list(TemplateList {})
        .unwrap()
        .templates
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert!(names.contains(&"my_card".to_string()));
    let applied = s
        .template_apply(TemplateApply {
            name: "my_card".into(),
            title: None,
        })
        .unwrap();
    assert!(applied.placeholders.iter().any(|p| p.name == "name"));
    let e = s
        .template_save(TemplateSave {
            doc_id: None,
            name: "../evil".into(),
        })
        .unwrap_err();
    assert_eq!(e.code, "invalid_argument");
    let e = s
        .template_apply(TemplateApply {
            name: "nope".into(),
            title: None,
        })
        .unwrap_err();
    assert_eq!(e.code, "not_found");
}

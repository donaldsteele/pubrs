//! Templates through the API.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use pubrs_api::core::{PlaceholderKind, WarningCode};
use pubrs_api::tools::doc::DocRef;
use pubrs_api::tools::query::ValidateIn;
use pubrs_api::tools::templates::*;
use pubrs_api::{Session, SessionConfig};
use serde_json::json;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn session(tag: &str) -> Session {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("user-templates-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    Session::new(SessionConfig {
        id_seed: Some(9),
        font_dir: Some(root().join("tests/fonts")),
        user_templates_dir: Some(dir),
        ..Default::default()
    })
}

/// Fill every placeholder: text with an edited copy of its sample text, pictures
/// with a small SVG that gets alt text.
fn fill(s: &mut Session, out: &TemplateApplyOut) {
    let svg = root().join("tests/fixtures/template_fill.svg");
    for p in &out.placeholders {
        match p.kind {
            PlaceholderKind::Text => {
                // Edit the sample text the template shows (a hint can be a description).
                let sample = {
                    let open = s.get(Some(out.doc_id)).unwrap();
                    open.doc().story(p.story_id.unwrap()).unwrap().plain_text()
                };
                let text = format!("{}.", sample.trim_end());
                s.text_set(
                    serde_json::from_value(json!({
                        "doc_id": out.doc_id, "frame_id": p.frame_id, "text": text
                    }))
                    .unwrap(),
                )
                .unwrap();
            }
            PlaceholderKind::Image => {
                s.image_place(
                    serde_json::from_value(json!({
                        "doc_id": out.doc_id, "frame_id": p.frame_id,
                        "path": svg.display().to_string()
                    }))
                    .unwrap(),
                )
                .unwrap();
                s.image_set_alt_text(
                    serde_json::from_value(json!({
                        "doc_id": out.doc_id, "frame_id": p.frame_id,
                        "alt_text": format!("Sample picture for {}", p.name)
                    }))
                    .unwrap(),
                )
                .unwrap();
            }
        }
    }
}

#[test]
fn every_builtin_applies_with_only_placeholder_warnings_and_validates_clean_once_filled() {
    let mut s = session("builtins");
    let list = s.template_list(TemplateList {}).unwrap().templates;
    assert_eq!(list.len(), 13);
    for t in list {
        let out = s
            .template_apply(TemplateApply {
                name: t.name.clone(),
                title: Some(format!("My {}", t.title)),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(out.placeholders.len(), t.placeholders.len());
        let v = s.validate(DocRef::default()).unwrap();
        for w in &v.warnings {
            assert!(
                matches!(
                    w.code,
                    WarningCode::EmptyPlaceholder | WarningCode::EmptyImageFrame
                ),
                "{}: {w:?}",
                t.name
            );
        }
        assert!(
            !v.warnings.is_empty(),
            "{}: unfilled template should warn",
            t.name
        );
        fill(&mut s, &out);
        let v = s.validate(DocRef::default()).unwrap();
        let names: Vec<_> = v
            .warnings
            .iter()
            .filter_map(|w| {
                out.placeholders
                    .iter()
                    .find(|p| Some(p.frame_id) == w.frame)
                    .map(|p| (&p.name, &p.hint))
            })
            .collect();
        assert!(
            v.warnings.is_empty(),
            "{}: {:?} {names:?}",
            t.name,
            v.warnings
        );
        // Commercial job: RGB scheme swatches may be flagged; report the count.
        let c = s
            .validate(ValidateIn {
                commercial: true,
                ..Default::default()
            })
            .unwrap();
        println!("commercial {}: {} warnings", t.name, c.warnings.len());
    }
}

#[test]
fn calendar_year_option() {
    let mut s = session("calendar");
    let apply = |s: &mut Session, name: &str, year| {
        s.template_apply(TemplateApply {
            name: name.into(),
            year,
            ..Default::default()
        })
    };
    let out = apply(&mut s, "calendar", Some(2026)).unwrap();
    assert_eq!(out.page_ids.len(), 12);
    assert_eq!(out.placeholders.len(), 12);
    let title = s.get(Some(out.doc_id)).unwrap().doc().meta.title.clone();
    assert_eq!(title, "Calendar 2026");
    // Default year.
    let out = apply(&mut s, "calendar", None).unwrap();
    let title = s.get(Some(out.doc_id)).unwrap().doc().meta.title.clone();
    assert_eq!(
        title,
        format!(
            "Calendar {}",
            pubrs_api::io::templates::DEFAULT_CALENDAR_YEAR
        )
    );
    for bad in [1599, 10_000, -5] {
        let e = apply(&mut s, "calendar", Some(bad)).unwrap_err();
        assert_eq!(e.code, "invalid_argument", "{bad}");
    }
    assert!(apply(&mut s, "calendar", Some(1600)).is_ok());
    assert!(apply(&mut s, "calendar", Some(9999)).is_ok());
    let e = apply(&mut s, "flyer", Some(2026)).unwrap_err();
    assert_eq!(e.code, "invalid_argument");
    assert!(e.message.contains("calendar template only"));
}

#[test]
fn sheet_templates_carry_their_sheet() {
    let mut s = session("sheets");
    for (name, preset, w, h) in [
        ("address_labels", "avery-5160", 189.0, 72.0),
        ("business_cards", "avery-5371", 252.0, 144.0),
    ] {
        let out = s
            .template_apply(TemplateApply {
                name: name.into(),
                ..Default::default()
            })
            .unwrap();
        let open = s.get(Some(out.doc_id)).unwrap();
        let setup = &open.doc().setup;
        let sheet = setup.sheet.as_ref().unwrap();
        assert_eq!(sheet.preset, preset);
        assert_eq!((setup.size.width, setup.size.height), (w, h));
        assert_eq!(setup.bleed, 0.0);
    }
}

#[test]
fn user_templates_round_trip() {
    let mut s = session("user");
    s.template_apply(TemplateApply {
        name: "business_card".into(),
        title: None,
        ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
        })
        .unwrap_err();
    assert_eq!(e.code, "not_found");
}

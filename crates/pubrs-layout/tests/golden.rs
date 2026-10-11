//! Layout golden test: line breaks and positions of a reference document,
//! using only the fonts in `tests/fonts/`. Regenerate with `PUBRS_BLESS=1`.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use pubrs_core::*;
use pubrs_layout::*;
use serde_json::{Value, json};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn reference_document() -> Document {
    let mut b = DocBuilder::new(SeqIds::new(1), PageSetup::default());
    let p = b.page(0);
    let styles = b.doc().styles.clone();
    let title = styles.by_name("Title").unwrap().id;
    let body = styles.by_name("Body").unwrap().id;
    let para = |style, text: &str| Paragraph {
        style: Some(style),
        props: ParaProps::default(),
        runs: vec![Run::plain(text)],
    };
    let a = b
        .text_paragraphs(
            Parent::Page(p),
            Rect::new(36.0, 36.0, 260.0, 130.0),
            vec![
                para(title, "Golden Crust Bakery"),
                para(
                    body,
                    "Fresh sourdough, rye and seeded loaves baked every morning from 6am. \
                     Our starter is older than the shop, and we still shape every loaf by hand.",
                ),
                para(
                    body,
                    "Order celebration cakes three days ahead. Gluten-free and vegan options \
                     are available on Fridays and Saturdays while stocks last.",
                ),
            ],
        )
        .unwrap();
    let c = b.text(p, Rect::new(316.0, 36.0, 260.0, 300.0), "").unwrap();
    b.link(a, c).unwrap();
    let s = b.story_of(a);
    b.exec(Command::ApplyParaProps {
        story: s,
        start: 0,
        end: 0,
        props: ParaProps {
            align: Some(Align::Center),
            ..Default::default()
        },
    })
    .unwrap();
    b.exec(Command::ApplyParaProps {
        story: s,
        start: 25,
        end: 25,
        props: ParaProps {
            align: Some(Align::Justify),
            ..Default::default()
        },
    })
    .unwrap();
    let pic = b
        .shape(p, Rect::new(336.0, 46.0, 120.0, 60.0), Shape::Ellipse)
        .unwrap();
    b.wrap(
        pic,
        TextWrap {
            mode: WrapMode::Square,
            margin: 6.0,
        },
    )
    .unwrap();
    b.finish()
}

fn r2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn snapshot(doc: &Document, lay: &DocumentLayout) -> Value {
    let mut stories = Vec::new();
    for (sid, sl) in &lay.stories {
        let story = doc.story(*sid).unwrap();
        let chars: Vec<char> = story.plain_text().chars().collect();
        let mut frames = Vec::new();
        for (i, fid) in story.frames.iter().enumerate() {
            let ft = &sl.frames[fid];
            let lines: Vec<Value> = ft
                .lines
                .iter()
                .map(|l| {
                    let first = l.runs.first().and_then(|r| r.glyphs.first());
                    json!({
                        "text": chars[l.chars.clone()].iter().collect::<String>(),
                        "x": r2(l.rect.x), "y": r2(l.rect.y),
                        "w": r2(l.rect.width), "h": r2(l.rect.height),
                        "baseline": r2(l.baseline),
                        "first_glyph_x": first.map(|g| r2(g.x)),
                        "glyphs": l.runs.iter().map(|r| r.glyphs.len()).sum::<usize>(),
                    })
                })
                .collect();
            frames.push(json!({ "chain_index": i, "lines": lines }));
        }
        stories.push(json!({
            "frames": frames,
            "overflow": sl.overflow.map(|o| json!({"offset": o.offset, "hidden": o.hidden_chars})),
        }));
    }
    json!({ "stories": stories })
}

#[test]
fn reference_layout_matches_golden() {
    let doc = reference_document();
    let fonts = FontLibrary::from_dir(&root().join("tests/fonts")).unwrap();
    let lay = Layouter::new(fonts).layout_document(&doc);
    let actual = serde_json::to_string_pretty(&snapshot(&doc, &lay)).unwrap() + "\n";
    let path = root().join("tests/golden/layout/reference.json");
    if std::env::var_os("PUBRS_BLESS").is_some() || !path.exists() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &actual).unwrap();
        if std::env::var_os("PUBRS_BLESS").is_none() {
            panic!("golden file was missing and has been written; re-run to compare");
        }
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap()
        .replace("\r\n", "\n");
    if expected != actual {
        let out = root().join("target/golden-diffs/layout-reference.actual.json");
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        std::fs::write(&out, &actual).unwrap();
        panic!(
            "layout differs from {}; actual written to {}",
            path.display(),
            out.display()
        );
    }
}

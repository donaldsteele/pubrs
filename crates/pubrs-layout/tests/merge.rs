//! Merge fields in the flow (M24, ADR 0023): a field shown as its value
//! lays out exactly like the same text typed in place, drop caps, tabs, caps
//! and hyphenation keep working around it, and the cache follows the preview.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use pubrs_core::style::{DropCap, TabAlign, TabLeader, TabStop};
use pubrs_core::*;
use pubrs_layout::*;

fn layouter() -> Layouter {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fonts");
    Layouter::new(FontLibrary::from_dir(&dir).expect("test fonts"))
}

fn source(values: &[(&str, &str)], preview: bool) -> MergeSource {
    let mut m = MergeSource::new(
        "t.csv",
        MergeKind::Csv,
        None,
        values.iter().map(|v| v.0.to_string()).collect(),
        vec![values.iter().map(|v| v.1.to_string()).collect()],
    );
    m.preview = preview;
    m
}

/// A one-frame document whose story is `paras`.
fn doc_of(paras: Vec<Paragraph>, merge: Option<MergeSource>) -> (Document, StoryId) {
    let mut b = DocBuilder::new(SeqIds::new(3), PageSetup::default());
    let p = b.page(0);
    let f = b.text(p, Rect::new(36.0, 36.0, 220.0, 400.0), "").unwrap();
    let s = b.story_of(f);
    b.exec(Command::SetStory {
        story: s,
        paragraphs: paras,
    })
    .unwrap();
    let mut doc = b.finish();
    doc.merge = merge;
    (doc, s)
}

fn field(name: &str, props: CharProps) -> Run {
    Run::field(name, None, props)
}

fn text(s: &str, props: CharProps) -> Run {
    Run {
        text: s.into(),
        props,
        ..Run::default()
    }
}

type Ink = Vec<(f64, f64, f64, f64, Vec<(u32, f64, f64, f64)>)>;

/// What a layout draws, without the text it indexes: per line its box and
/// glyphs.
fn ink(doc: &Document, s: StoryId) -> Ink {
    let l = layouter().layout_document(doc);
    l.stories[&s]
        .frames
        .values()
        .flat_map(|f| f.lines.iter())
        .map(|line| {
            (
                line.rect.x,
                line.rect.y,
                line.rect.width,
                line.baseline,
                line.runs
                    .iter()
                    .flat_map(|r| r.glyphs.iter().map(|g| (g.id, g.x, g.y, g.advance)))
                    .collect(),
            )
        })
        .collect()
}

/// A paragraph with a field in the middle, and the same with the value typed.
fn pair(
    props: ParaProps,
    before: &str,
    value: &str,
    after: &str,
    char_props: CharProps,
) -> ((Document, StoryId), (Document, StoryId)) {
    let with_field = Paragraph {
        style: None,
        props: props.clone(),
        runs: vec![
            text(before, char_props.clone()),
            field("F", char_props.clone()),
            text(after, char_props.clone()),
        ],
    };
    let typed = Paragraph {
        style: None,
        props,
        runs: vec![text(&format!("{before}{value}{after}"), char_props)],
    };
    (
        doc_of(vec![with_field], Some(source(&[("F", value)], true))),
        doc_of(vec![typed], None),
    )
}

const TAIL: &str = " extensive international information about the committee's responsibilities \
    and the organisational arrangements that were considered appropriate.";

fn same_ink(a: &(Document, StoryId), b: &(Document, StoryId)) {
    assert_eq!(ink(&a.0, a.1), ink(&b.0, b.1));
}

#[test]
fn a_shown_value_lays_out_like_the_same_typed_text() {
    for value in ["Z", "Zed", "Wolfeschlegelsteinhausen", ""] {
        let (a, b) = pair(
            ParaProps::default(),
            "Dear ",
            value,
            TAIL,
            CharProps::default(),
        );
        if value.is_empty() {
            // The space after an empty field goes (the collapse rule).
            let (_, b2) = pair(ParaProps::default(), "Dear", "", TAIL, CharProps::default());
            same_ink(&a, &b2);
        } else {
            same_ink(&a, &b);
        }
    }
}

#[test]
fn hyphenation_after_a_field_lands_where_it_would_without_one() {
    let props = ParaProps {
        hyphenate: Some(true),
        align: Some(style::Align::Justify),
        ..Default::default()
    };
    for value in ["Z", "Zed", "Zed Zed Zed"] {
        let (a, b) = pair(props.clone(), "", value, TAIL, CharProps::default());
        same_ink(&a, &b);
    }
}

#[test]
fn a_drop_cap_and_a_field_in_the_same_paragraph() {
    let props = ParaProps {
        drop_cap: Some(DropCap { lines: 3, chars: 1 }),
        ..Default::default()
    };
    let (a, b) = pair(props.clone(), "Once ", "Zed", TAIL, CharProps::default());
    same_ink(&a, &b);
    // A field first: no cap is made of it (it has no letters yet).
    let para = Paragraph {
        style: None,
        props,
        runs: vec![
            field("F", CharProps::default()),
            text(TAIL, CharProps::default()),
        ],
    };
    let (doc, s) = doc_of(vec![para], Some(source(&[("F", "Zed")], true)));
    assert!(!ink(&doc, s).is_empty());
}

#[test]
fn tab_stops_and_all_caps_work_with_a_field() {
    let props = ParaProps {
        tab_stops: Some(vec![TabStop {
            pos: 120.0,
            align: TabAlign::Left,
            leader: TabLeader::Dot,
        }]),
        ..Default::default()
    };
    let caps = CharProps {
        caps: Some(style::Caps::All),
        ..Default::default()
    };
    for cp in [CharProps::default(), caps] {
        let mk = |inner: Vec<Run>| Paragraph {
            style: None,
            props: props.clone(),
            runs: inner,
        };
        let a = doc_of(
            vec![mk(vec![
                text("Item\t", cp.clone()),
                field("F", cp.clone()),
                text("\tend", cp.clone()),
            ])],
            Some(source(&[("F", "price")], true)),
        );
        let b = doc_of(vec![mk(vec![text("Item\tprice\tend", cp)])], None);
        same_ink(&a, &b);
    }
}

#[test]
fn carets_follow_story_characters_around_fields() {
    let mk = |value: &str| {
        let para = Paragraph {
            style: None,
            props: ParaProps::default(),
            runs: vec![
                text("ab ", CharProps::default()),
                field("F", CharProps::default()),
                text(" cd", CharProps::default()),
            ],
        };
        let (doc, s) = doc_of(vec![para], Some(source(&[("F", value)], true)));
        let l = layouter().layout_document(&doc);
        l.stories[&s].frames.values().next().unwrap().lines[0].clone()
    };
    for value in ["", "x", "a long value"] {
        let line = mk(value);
        let offs: Vec<usize> = line.carets.iter().map(|c| c.0).collect();
        assert_eq!(offs, (0..=7).collect::<Vec<_>>(), "value {value:?}");
        assert!(line.carets.windows(2).all(|w| w[1].1 >= w[0].1 - 1e-9));
    }
}

/// The cache keys fields on the preview state (Risk 1): stepping records and
/// switching the preview recompose exactly the stories that have fields, and
/// the incremental layout always equals a full one.
#[test]
fn the_cache_follows_the_preview_record_and_switch() {
    let mut b = DocBuilder::new(SeqIds::new(5), PageSetup::default());
    let p = b.page(0);
    let fa = b.text(p, Rect::new(36.0, 36.0, 220.0, 100.0), "").unwrap();
    let fb = b
        .text(p, Rect::new(300.0, 36.0, 220.0, 100.0), "Plain text")
        .unwrap();
    let (sa, sb) = (b.story_of(fa), b.story_of(fb));
    b.exec(Command::SetStory {
        story: sa,
        paragraphs: vec![Paragraph {
            style: None,
            props: ParaProps::default(),
            runs: vec![
                text("Hello ", CharProps::default()),
                field("F", CharProps::default()),
            ],
        }],
    })
    .unwrap();
    let mut ed = Editor::new(b.finish());
    let m = MergeSource::new(
        "t.csv",
        MergeKind::Csv,
        None,
        vec!["F".into()],
        vec![
            vec!["Ann".into()],
            vec!["Bartholomew".into()],
            vec!["".into()],
        ],
    );
    ed.execute(Command::SetMergeSource { merge: Some(m) })
        .unwrap();
    let (mut lay, mut cache) = (layouter(), StoryLayoutCache::new());
    let check = |cache: &mut StoryLayoutCache, lay: &mut Layouter, doc: &Document| {
        let inc = cache.layout(lay, doc);
        let full = lay.layout_document(doc);
        for id in full.stories.keys() {
            assert_eq!(inc.stories.get(id), full.stories.get(id), "story {id}");
        }
        inc
    };
    check(&mut cache, &mut lay, ed.doc());
    assert_eq!(cache.stats().recomposed, 2);
    // Preview on: only the story with the field is laid out again.
    ed.execute(Command::SetMergePreview {
        index: None,
        preview: Some(true),
    })
    .unwrap();
    let l = check(&mut cache, &mut lay, ed.doc());
    assert_eq!((cache.stats().recomposed, cache.stats().reused), (1, 1));
    let width = |l: &DocumentLayout| {
        l.stories[&sa].frames.values().next().unwrap().lines[0]
            .carets
            .last()
            .unwrap()
            .1
    };
    let w_ann = width(&l);
    // Step ▶: a wider value, again one story.
    ed.execute(Command::SetMergePreview {
        index: Some(1),
        preview: None,
    })
    .unwrap();
    let l = check(&mut cache, &mut lay, ed.doc());
    assert_eq!(cache.stats().recomposed, 1);
    assert!(width(&l) > w_ann);
    // Step ▶ to the empty value, ◀ back, off again; undo too.
    for step in [Some(2usize), Some(0)] {
        ed.execute(Command::SetMergePreview {
            index: step,
            preview: None,
        })
        .unwrap();
        check(&mut cache, &mut lay, ed.doc());
        assert_eq!(cache.stats().recomposed, 1);
    }
    ed.execute(Command::SetMergePreview {
        index: None,
        preview: Some(false),
    })
    .unwrap();
    check(&mut cache, &mut lay, ed.doc());
    ed.undo().unwrap();
    ed.undo().unwrap();
    check(&mut cache, &mut lay, ed.doc());
    // Clearing the data returns the story to «Field» names.
    ed.execute(Command::SetMergeSource { merge: None }).unwrap();
    check(&mut cache, &mut lay, ed.doc());
    assert_eq!(cache.stats().recomposed, 1);
    let _ = sb;
}

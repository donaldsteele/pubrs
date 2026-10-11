//! The story layout cache: incremental layout equals a full layout after
//! any edit, and untouched stories are not laid out again (ADR 0012).
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::Arc;

use proptest::prelude::*;
use pubrs_core::*;
use pubrs_layout::*;

const LOREM: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod \
tempor incididunt ut labore et dolore magna aliqua.\nUt enim ad minim veniam, quis nostrud \
exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.\nDuis aute irure dolor in \
reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur.";

fn layouter() -> Layouter {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fonts");
    Layouter::new(FontLibrary::from_dir(&dir).expect("test fonts"))
}

/// Incremental layout of `doc`, asserted equal to a full layout made with
/// the same layouter (font handles compare by identity).
fn check(cache: &mut StoryLayoutCache, lay: &mut Layouter, doc: &Document) -> DocumentLayout {
    let inc = cache.layout(lay, doc);
    let full = lay.layout_document(doc);
    for id in full.stories.keys().chain(inc.stories.keys()) {
        assert_eq!(
            inc.stories.get(id),
            full.stories.get(id),
            "story {id}: incremental layout differs from full layout"
        );
    }
    inc
}

struct Two {
    ed: Editor,
    a: StoryId,
    b: StoryId,
    a_frame: FrameId,
    b_frame: FrameId,
}

/// Two one-frame stories side by side on one page.
fn two_stories() -> Two {
    let mut b = DocBuilder::new(SeqIds::new(7), PageSetup::default());
    let p = b.page(0);
    let a_frame = b
        .text(p, Rect::new(36.0, 36.0, 250.0, 300.0), LOREM)
        .unwrap();
    let b_frame = b
        .text(p, Rect::new(320.0, 36.0, 250.0, 300.0), LOREM)
        .unwrap();
    let (a, bs) = (b.story_of(a_frame), b.story_of(b_frame));
    Two {
        ed: Editor::new(b.finish()),
        a,
        b: bs,
        a_frame,
        b_frame,
    }
}

#[test]
fn untouched_story_is_not_recomposed() {
    let mut t = two_stories();
    let (mut lay, mut cache) = (layouter(), StoryLayoutCache::new());
    let first = check(&mut cache, &mut lay, t.ed.doc());
    assert_eq!(cache.stats().recomposed, 2);
    t.ed.execute(Command::InsertText {
        story: t.a,
        at: 5,
        text: "xyz".into(),
        format: None,
    })
    .unwrap();
    let second = check(&mut cache, &mut lay, t.ed.doc());
    let s = cache.stats();
    assert_eq!((s.recomposed, s.reused, s.dropped), (1, 1, 0));
    assert_eq!(s.total_recomposed, 3);
    // The untouched story is the very same allocation, not a copy.
    assert!(Arc::ptr_eq(&first.stories[&t.b], &second.stories[&t.b]));
    assert!(first.stories[&t.a] != second.stories[&t.a], "a changed");
    // Nothing changed: nothing recomposed.
    check(&mut cache, &mut lay, t.ed.doc());
    assert_eq!(cache.stats().recomposed, 0);
    // Moving a frame without wrap changes nothing frame-local.
    t.ed.execute(Command::SetFrameRect {
        frame: t.b_frame,
        rect: Rect::new(330.0, 50.0, 250.0, 300.0),
    })
    .unwrap();
    check(&mut cache, &mut lay, t.ed.doc());
    assert_eq!(cache.stats().recomposed, 0);
}

#[test]
fn moving_a_wrapping_frame_invalidates_the_story_it_wraps() {
    let mut t = two_stories();
    // Story b's frame wraps text; it is painted above story a's frame.
    t.ed.execute(Command::SetWrap {
        frame: t.b_frame,
        wrap: TextWrap {
            mode: WrapMode::Square,
            margin: 6.0,
        },
    })
    .unwrap();
    let (mut lay, mut cache) = (layouter(), StoryLayoutCache::new());
    let before = check(&mut cache, &mut lay, t.ed.doc());
    // Move b (same size) over the lower right of a: a flows around it.
    t.ed.execute(Command::SetFrameRect {
        frame: t.b_frame,
        rect: Rect::new(150.0, 60.0, 250.0, 300.0),
    })
    .unwrap();
    let after = check(&mut cache, &mut lay, t.ed.doc());
    let s = cache.stats();
    assert_eq!((s.recomposed, s.reused), (1, 1), "only story a recomposes");
    assert!(before.stories[&t.a] != after.stories[&t.a], "a reflowed");
    assert!(Arc::ptr_eq(&before.stories[&t.b], &after.stories[&t.b]));
    // Turning the wrap off also reaches story a.
    t.ed.execute(Command::SetWrap {
        frame: t.b_frame,
        wrap: TextWrap {
            mode: WrapMode::None,
            margin: 6.0,
        },
    })
    .unwrap();
    check(&mut cache, &mut lay, t.ed.doc());
    assert_eq!(cache.stats().recomposed, 1);
}

#[test]
fn undo_of_a_replaced_layout_recomputes() {
    let mut t = two_stories();
    let (mut lay, mut cache) = (layouter(), StoryLayoutCache::new());
    let start = check(&mut cache, &mut lay, t.ed.doc());
    for (k, ch) in ["a", "b", "c"].iter().enumerate() {
        t.ed.execute_coalesced(
            Command::InsertText {
                story: t.a,
                at: 10 + k,
                text: (*ch).into(),
                format: None,
            },
            "typing",
        )
        .unwrap();
        check(&mut cache, &mut lay, t.ed.doc());
    }
    t.ed.undo().unwrap();
    let undone = check(&mut cache, &mut lay, t.ed.doc());
    assert_eq!(cache.stats().recomposed, 1);
    assert_eq!(undone, start);
    t.ed.redo().unwrap();
    check(&mut cache, &mut lay, t.ed.doc());
}

#[test]
fn stylesheet_edits_and_deleted_stories() {
    let mut t = two_stories();
    let (mut lay, mut cache) = (layouter(), StoryLayoutCache::new());
    check(&mut cache, &mut lay, t.ed.doc());
    let mut style = t.ed.doc().styles.styles[0].clone();
    style.character.font_size = Some(17.0);
    t.ed.execute(Command::UpdateStyle { style }).unwrap();
    check(&mut cache, &mut lay, t.ed.doc());
    assert_eq!(
        cache.stats().recomposed,
        2,
        "a stylesheet edit reaches every story"
    );
    t.ed.execute(Command::DeleteFrame { frame: t.a_frame })
        .unwrap();
    let l = check(&mut cache, &mut lay, t.ed.doc());
    if t.ed.doc().story(t.a).is_none() {
        assert_eq!(cache.stats().dropped, 1);
        assert!(!l.stories.contains_key(&t.a));
    }
}

// ---------------------------------------------------------------- proptest

#[derive(Debug, Clone)]
struct Op {
    kind: u8,
    a: usize,
    b: usize,
    c: usize,
    f: [f64; 4],
    text: String,
    flag: bool,
}

fn op() -> impl Strategy<Value = Op> {
    (
        0u8..25,
        0usize..8,
        0usize..8,
        0usize..200,
        prop::array::uniform4(-20.0f64..400.0),
        "[a-z \n]{0,12}",
        any::<bool>(),
    )
        .prop_map(|(kind, a, b, c, f, text, flag)| Op {
            kind,
            a,
            b,
            c,
            f,
            text,
            flag,
        })
}

fn pick<T: Clone>(v: &[T], i: usize) -> Option<T> {
    (!v.is_empty()).then(|| v[i % v.len()].clone())
}

/// Two pages: two stories with two-frame chains, a lone story, a picture
/// frame that wraps, and a shape.
fn world() -> (SeqIds, Editor) {
    let mut b = DocBuilder::new(SeqIds::new(11), PageSetup::default());
    let p0 = b.page(0);
    let p1 = b.add_page().unwrap();
    let long = format!("{LOREM}\n{LOREM}");
    let a1 = b
        .text(p0, Rect::new(36.0, 36.0, 260.0, 220.0), &long)
        .unwrap();
    let a2 = b.text(p1, Rect::new(36.0, 36.0, 260.0, 220.0), "").unwrap();
    b.link(a1, a2).unwrap();
    let c1 = b
        .text(p0, Rect::new(320.0, 36.0, 250.0, 300.0), LOREM)
        .unwrap();
    let c2 = b
        .text(p0, Rect::new(320.0, 360.0, 250.0, 200.0), "")
        .unwrap();
    b.link(c1, c2).unwrap();
    b.text(p1, Rect::new(320.0, 36.0, 250.0, 300.0), LOREM)
        .unwrap();
    let pic = b
        .image(p0, Rect::new(200.0, 120.0, 160.0, 120.0), None)
        .unwrap();
    b.wrap(
        pic,
        TextWrap {
            mode: WrapMode::Square,
            margin: 4.0,
        },
    )
    .unwrap();
    b.shape(p1, Rect::new(100.0, 400.0, 80.0, 80.0), Shape::Ellipse)
        .unwrap();
    let ids = SeqIds::new(99_000);
    (ids, Editor::new(b.finish()))
}

/// A command for `op` against the current document (may fail; failures
/// leave the document unchanged). `None` means undo or redo.
fn command(ids: &mut SeqIds, ed: &Editor, op: &Op) -> Option<Command> {
    let doc = ed.doc();
    let frames: Vec<FrameId> = doc.frames.keys().copied().collect();
    let tframes: Vec<FrameId> = doc
        .frames
        .values()
        .filter(|f| f.as_text().is_some())
        .map(|f| f.id)
        .collect();
    let stories: Vec<StoryId> = doc.stories.keys().copied().collect();
    let styles: Vec<Style> = doc.styles.styles.clone();
    let pages: Vec<PageId> = doc.pages.iter().map(|p| p.id).collect();
    let frame = pick(&frames, op.a).unwrap_or(FrameId::new(ids));
    let story = pick(&stories, op.a).unwrap_or(StoryId::new(ids));
    let len = doc.story(story).map_or(0, |s| s.char_len());
    let at = if len == 0 { 0 } else { op.c % (len + 1) };
    let end = (at + op.b * 7).min(len);
    let [x, y, w, h] = op.f;
    Some(match op.kind {
        0..=2 => Command::InsertText {
            story,
            at,
            text: op.text.clone(),
            format: None,
        },
        3 => Command::DeleteText {
            story,
            start: at,
            end,
        },
        4 => Command::SetFrameRect {
            frame,
            rect: Rect::new(x, y, w.abs() + 20.0, h.abs() + 20.0),
        },
        5 => Command::SetRotation { frame, degrees: x },
        6 => Command::ApplyCharProps {
            story,
            start: at,
            end,
            props: CharProps {
                bold: Some(op.flag),
                font_size: Some(w.abs() / 20.0 + 6.0),
                baseline: match op.c % 4 {
                    0 => None,
                    1 => Some(pubrs_core::Baseline::Superscript),
                    2 => Some(pubrs_core::Baseline::Subscript),
                    _ => Some(pubrs_core::Baseline::Shift(y / 40.0)),
                },
                caps: match op.b % 3 {
                    0 => None,
                    1 => Some(pubrs_core::Caps::Small),
                    _ => Some(pubrs_core::Caps::All),
                },
                strikethrough: op.a.is_multiple_of(3).then_some(op.flag),
                ligatures: op.a.is_multiple_of(2).then_some(op.flag),
                kerning: op.b.is_multiple_of(2).then_some(op.flag),
                ..Default::default()
            },
        },
        7 => Command::ApplyParaProps {
            story,
            start: at,
            end,
            props: ParaProps {
                align: Some(match op.b % 4 {
                    0 => Align::Left,
                    1 => Align::Center,
                    2 => Align::Right,
                    _ => Align::Justify,
                }),
                space_before: Some(y.abs() / 20.0),
                line_spacing: op.flag.then_some(1.5),
                first_line_indent: Some(h.abs() / 40.0),
                ..Default::default()
            },
        },
        8 => {
            let style = pick(&styles, op.b)?;
            match style.kind {
                StyleKind::Paragraph => Command::SetParaStyle {
                    story,
                    start: at,
                    end,
                    style: op.flag.then_some(style.id),
                },
                StyleKind::Character => Command::SetCharStyle {
                    story,
                    start: at,
                    end,
                    style: op.flag.then_some(style.id),
                },
            }
        }
        9 => {
            let mut style = pick(&styles, op.b)?;
            style.character.font_size = Some(w.abs() / 25.0 + 6.0);
            style.paragraph.space_after = Some(h.abs() / 30.0);
            Command::UpdateStyle { style }
        }
        10 => Command::LinkFrames {
            from: pick(&tframes, op.a)?,
            to: pick(&tframes, op.b)?,
        },
        11 => Command::UnlinkFrame {
            frame: pick(&tframes, op.a)?,
            new_story: StoryId::new(ids),
        },
        12 => Command::SetWrap {
            frame,
            wrap: TextWrap {
                mode: match op.c % 3 {
                    0 => WrapMode::None,
                    1 => WrapMode::Square,
                    _ => WrapMode::TopBottom,
                },
                margin: w / 20.0,
            },
        },
        13 => Command::SetTextFrame {
            frame: pick(&tframes, op.a)?,
            props: TextFrameProps {
                insets: Some(Margins::uniform(x.abs() / 40.0)),
                columns: Some(1 + (op.b % 3) as u32),
                column_gap: Some(h.abs() / 20.0),
                vertical_align: Some(match op.c % 3 {
                    0 => VerticalAlign::Top,
                    1 => VerticalAlign::Middle,
                    _ => VerticalAlign::Bottom,
                }),
            },
        },
        14 => Command::SetZIndex { frame, index: op.b },
        15 => Command::DeleteFrame { frame },
        16 => {
            let fid = FrameId::new(ids);
            let sid = StoryId::new(ids);
            let mut s = Story::new(sid, fid);
            s.insert_text(0, &op.text, None);
            let mut f = Frame::new(
                fid,
                Rect::new(x, y, w.abs() + 30.0, h.abs() + 30.0),
                FrameContent::Text(TextFrame::new(sid)),
            );
            f.wrap.mode = if op.flag {
                WrapMode::Square
            } else {
                WrapMode::None
            };
            Command::AddFrame {
                parent: Parent::Page(pick(&pages, op.b)?),
                index: None,
                frame: f,
                story: Some(s),
            }
        }
        17 => Command::ReplaceText {
            story,
            start: at,
            end,
            text: op.text.clone(),
        },
        18 => Command::SetMargins {
            margins: Margins::uniform(x.abs() / 10.0),
        },
        // M15: keeps, widows and orphans, drop caps; break characters; page order.
        19 => Command::ApplyParaProps {
            story,
            start: at,
            end,
            props: ParaProps {
                keep_with_next: Some(op.flag),
                keep_together: Some(op.b.is_multiple_of(2)),
                widow_orphan: Some((op.b % 4) as u8),
                drop_cap: (!op.b.is_multiple_of(3)).then_some(pubrs_core::style::DropCap {
                    lines: (op.b % 4) as u8,
                    chars: 1 + (op.c % 3) as u8,
                }),
                ..Default::default()
            },
        },
        20 => Command::InsertText {
            story,
            at,
            text: pubrs_core::text::BreakKind::ALL[op.b % 3]
                .char()
                .to_string(),
            format: None,
        },
        21 => Command::MovePage {
            page: pick(&pages, op.b)?,
            index: op.c % pages.len().max(1),
        },
        _ => return None,
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn incremental_layout_equals_full_layout(ops in prop::collection::vec(op(), 1..32)) {
        let (mut ids, mut ed) = world();
        let (mut lay, mut cache) = (layouter(), StoryLayoutCache::new());
        check(&mut cache, &mut lay, ed.doc());
        for o in &ops {
            match command(&mut ids, &ed, o) {
                Some(cmd) => {
                    let r = if o.kind == 0 && o.flag {
                        ed.execute_coalesced(cmd, "typing")
                    } else {
                        ed.execute(cmd)
                    };
                    drop(r);
                }
                None if o.flag => drop(ed.undo()),
                None => drop(ed.redo()),
            }
            check(&mut cache, &mut lay, ed.doc());
        }
        // Undo everything: still equal at every step.
        while ed.undo().is_some() {
            check(&mut cache, &mut lay, ed.doc());
        }
    }
}

/// M15: keeps, widow and orphan control, drop caps and break characters
/// are story inputs: toggling each recomposes only its story, the result
/// equals a full layout, and toggling back gives the first layout again.
/// Page breaks read the order of the chain's pages, which moving another
/// page leaves alone.
#[test]
fn keeps_drop_caps_and_breaks_recompose_only_their_story() {
    let mut t = two_stories();
    let (mut lay, mut cache) = (layouter(), StoryLayoutCache::new());
    let start = check(&mut cache, &mut lay, t.ed.doc());
    let variants = [
        ParaProps {
            keep_with_next: Some(true),
            ..Default::default()
        },
        ParaProps {
            keep_together: Some(true),
            ..Default::default()
        },
        ParaProps {
            widow_orphan: Some(2),
            ..Default::default()
        },
        ParaProps {
            drop_cap: Some(pubrs_core::style::DropCap { lines: 3, chars: 2 }),
            ..Default::default()
        },
    ];
    for props in variants {
        t.ed.execute(Command::ApplyParaProps {
            story: t.a,
            start: 0,
            end: 0,
            props,
        })
        .unwrap();
        check(&mut cache, &mut lay, t.ed.doc());
        assert_eq!(cache.stats().recomposed, 1);
        t.ed.undo().unwrap();
        assert_eq!(check(&mut cache, &mut lay, t.ed.doc()), start);
    }
    for kind in pubrs_core::text::BreakKind::ALL {
        t.ed.execute(Command::InsertText {
            story: t.b,
            at: 20,
            text: kind.char().to_string(),
            format: None,
        })
        .unwrap();
        check(&mut cache, &mut lay, t.ed.doc());
        assert_eq!(cache.stats().recomposed, 1);
        t.ed.undo().unwrap();
        assert_eq!(check(&mut cache, &mut lay, t.ed.doc()), start);
    }
    let page = t.ed.doc().pages[0].id;
    t.ed.execute(Command::AddPage {
        page: Page::new(PageId::new(&mut SeqIds::new(77_000))),
        index: Some(0),
    })
    .unwrap();
    check(&mut cache, &mut lay, t.ed.doc());
    assert_eq!(cache.stats().recomposed, 0, "page ranks are relative");
    assert_ne!(t.ed.doc().pages[0].id, page);
}

#[test]
fn tab_stops_and_lists_recompose_only_their_story() {
    use pubrs_core::style::{List, ListKind, TabAlign, TabLeader, TabStop};
    let mut t = two_stories();
    let (mut lay, mut cache) = (layouter(), StoryLayoutCache::new());
    let start = check(&mut cache, &mut lay, t.ed.doc());
    let len = t.ed.doc().story(t.b).unwrap().char_len();
    let edits = [
        // A tab in the text, then stops for it.
        Command::InsertText {
            story: t.b,
            at: 5,
            text: "\t".into(),
            format: None,
        },
        Command::ApplyParaProps {
            story: t.b,
            start: 0,
            end: len,
            props: ParaProps {
                tab_stops: Some(vec![TabStop {
                    pos: 120.0,
                    align: TabAlign::Right,
                    leader: TabLeader::Dot,
                }]),
                ..Default::default()
            },
        },
        // The first paragraph becomes a list item; the numbering of the
        // later ones depends on it.
        Command::ApplyParaProps {
            story: t.b,
            start: 0,
            end: len,
            props: ParaProps {
                left_indent: Some(18.0),
                list: Some(List {
                    kind: ListKind::Number,
                    ..Default::default()
                }),
                ..Default::default()
            },
        },
        Command::ApplyParaProps {
            story: t.b,
            start: 0,
            end: 0,
            props: ParaProps {
                list: Some(List {
                    kind: ListKind::None,
                    ..Default::default()
                }),
                ..Default::default()
            },
        },
    ];
    for e in edits {
        t.ed.execute(e).unwrap();
        let lay2 = check(&mut cache, &mut lay, t.ed.doc());
        assert_eq!(cache.stats().recomposed, 1, "only story b is recomposed");
        assert!(Arc::ptr_eq(&start.stories[&t.a], &lay2.stories[&t.a]));
    }
    while t.ed.undo().is_some() {
        check(&mut cache, &mut lay, t.ed.doc());
    }
    assert_eq!(check(&mut cache, &mut lay, t.ed.doc()), start);
}

#[test]
fn character_attributes_recompose_only_their_story() {
    use pubrs_core::{Baseline, Caps};
    let mut t = two_stories();
    let (mut lay, mut cache) = (layouter(), StoryLayoutCache::new());
    let start = check(&mut cache, &mut lay, t.ed.doc());
    let props = [
        CharProps {
            baseline: Some(Baseline::Superscript),
            ..Default::default()
        },
        CharProps {
            baseline: Some(Baseline::Shift(-3.0)),
            caps: Some(Caps::Small),
            ..Default::default()
        },
        CharProps {
            caps: Some(Caps::All),
            strikethrough: Some(true),
            ..Default::default()
        },
        CharProps {
            ligatures: Some(false),
            kerning: Some(false),
            ..Default::default()
        },
    ];
    for p in props {
        t.ed.execute(Command::ApplyCharProps {
            story: t.b,
            start: 10,
            end: 90,
            props: p,
        })
        .unwrap();
        let lay2 = check(&mut cache, &mut lay, t.ed.doc());
        assert_eq!(cache.stats().recomposed, 1, "only story b is recomposed");
        assert!(Arc::ptr_eq(&start.stories[&t.a], &lay2.stories[&t.a]));
    }
    while t.ed.undo().is_some() {
        check(&mut cache, &mut lay, t.ed.doc());
    }
    assert_eq!(check(&mut cache, &mut lay, t.ed.doc()), start);
}

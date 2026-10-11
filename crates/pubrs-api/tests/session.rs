//! End-to-end tests of the Session through its typed methods.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use pubrs_api::core::{Align, Color, FrameKind, ImageFit, PageSizePreset, WarningCode};
use pubrs_api::tools::doc::*;
use pubrs_api::tools::export::*;
use pubrs_api::tools::find::*;
use pubrs_api::tools::frames::*;
use pubrs_api::tools::images::*;
use pubrs_api::tools::styles::*;
use pubrs_api::tools::text::*;
use pubrs_api::{Session, SessionConfig};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn tmp(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("api-session");
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

fn session() -> Session {
    Session::new(SessionConfig {
        id_seed: Some(7),
        font_dir: Some(root().join("tests/fonts")),
        ..Default::default()
    })
}

fn write_png(path: &PathBuf, w: u32, h: u32) {
    let mut pm = pubrs_api::render::tiny_skia::Pixmap::new(w, h).unwrap();
    pm.fill(pubrs_api::render::tiny_skia::Color::from_rgba8(
        200, 120, 40, 255,
    ));
    std::fs::write(path, pm.encode_png().unwrap()).unwrap();
}

fn text_frame(
    s: &mut Session,
    page: pubrs_api::core::PageId,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    text: &str,
) -> FrameAddOut {
    s.frame_add_text(FrameAddText {
        doc_id: None,
        page_id: Some(page),
        master_id: None,
        x,
        y,
        width: w,
        height: h,
        text: Some(text.into()),
        markup: false,
        style: None,
        font_family: None,
        font_size: None,
        color: None,
        align: None,
        columns: None,
        name: None,
    })
    .unwrap()
}

#[test]
fn flyer_flow_save_reopen_equal() {
    let mut s = session();
    let doc = s
        .doc_new(DocNew {
            page_size: Some(PageSizeIn {
                preset: Some(PageSizePreset::Letter),
                ..Default::default()
            }),
            bleed: Some(9.0),
            pages: Some(2),
            title: Some("Bakery".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(doc.page_ids.len(), 2);
    assert_eq!(doc.revision, s.get(None).unwrap().revision());
    let p1 = doc.page_ids[0];
    let p2 = doc.page_ids[1];

    // Headline with markup and a style.
    let headline = s
        .frame_add_text(FrameAddText {
            doc_id: None,
            page_id: Some(p1),
            master_id: None,
            x: 36.0,
            y: 36.0,
            width: 540.0,
            height: 80.0,
            text: Some("# Golden **Crust**".into()),
            markup: true,
            style: None,
            font_family: None,
            font_size: None,
            color: Some(Color::rgb(180, 83, 9).into()),
            align: Some(Align::Center),
            columns: None,
            name: Some("headline".into()),
        })
        .unwrap();
    let story = headline.story_id.unwrap();
    let got = s
        .text_get(TextRef {
            doc_id: None,
            frame_id: Some(headline.frame_id),
            story_id: None,
        })
        .unwrap();
    assert_eq!(got.text, "Golden Crust");
    assert_eq!(got.paragraphs[0].style.as_deref(), Some("Heading 1"));
    assert_eq!(got.paragraphs[0].align, Align::Center);
    assert!(
        got.paragraphs[0]
            .runs
            .iter()
            .any(|r| r.text == "Crust" && r.bold)
    );
    assert_eq!(got.paragraphs[0].runs[0].color, Color::rgb(180, 83, 9));

    // Body text that overflows, then a linked frame on page 2.
    let body = text_frame(
        &mut s,
        p1,
        36.0,
        140.0,
        250.0,
        100.0,
        &"Fresh bread daily. ".repeat(60),
    );
    let status = s.text_overflow_status(DocRef::default()).unwrap();
    assert!(status.any_overflow);
    let cont = text_frame(&mut s, p2, 36.0, 36.0, 540.0, 700.0, "");
    let linked = s
        .text_link_frames(TextLinkFrames {
            doc_id: None,
            from_frame_id: body.frame_id,
            to_frame_id: cont.frame_id,
        })
        .unwrap();
    assert!(!linked.overflowing);
    assert!(
        !s.text_overflow_status(DocRef::default())
            .unwrap()
            .any_overflow
    );

    // An image from disk.
    let png = tmp("photo.png");
    write_png(&png, 900, 600);
    let img = s
        .frame_add_image(FrameAddImage {
            doc_id: None,
            page_id: Some(p1),
            master_id: None,
            x: 306.0,
            y: 140.0,
            width: 270.0,
            height: 180.0,
            path: Some(png.display().to_string()),
            base64: None,
            svg: None,
            filename: None,
            fit: Some(ImageFit::Fill),
            name: None,
        })
        .unwrap();
    assert!(img.asset_id.is_some());
    s.image_crop(ImageCrop {
        doc_id: None,
        frame_id: img.frame_id,
        left: 0.1,
        top: 0.0,
        right: 0.1,
        bottom: 0.0,
    })
    .unwrap();

    // Validation: clean document has no warnings.
    let v = s.validate(DocRef::default()).unwrap();
    assert!(v.ok, "{:?}", v.warnings);

    // Export, save, reopen, compare JSON.
    let pdf = tmp("flyer.pdf");
    let out = s
        .export_pdf(ExportPdf {
            path: Some(pdf.display().to_string()),
            crop_marks: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(out.page_count, 2);
    assert!(out.base64.is_none());
    let parsed = lopdf::Document::load(&pdf).unwrap();
    assert_eq!(parsed.get_pages().len(), 2);

    let before = s.doc_to_json(DocRef::default()).unwrap().document;
    let saved = s
        .doc_save_as(DocSaveAs {
            doc_id: None,
            path: Some(tmp("flyer").display().to_string()),
        })
        .unwrap();
    assert!(saved.path.ends_with("flyer.pubrs"));
    let reopened = s.doc_open(DocOpen { path: saved.path }).unwrap();
    assert_ne!(reopened.doc_id, doc.doc_id, "a new session handle");
    let after = s
        .doc_to_json(DocRef {
            doc_id: Some(reopened.doc_id),
        })
        .unwrap()
        .document;
    assert_eq!(
        serde_json::to_value(&before).unwrap(),
        serde_json::to_value(&after).unwrap()
    );
    assert_eq!(s.doc_list(DocRef::default()).unwrap().documents.len(), 2);
    let _ = story;
}

#[test]
fn undo_redo_and_history() {
    let mut s = session();
    let d = s.doc_new(DocNew::default()).unwrap();
    let f = text_frame(&mut s, d.page_ids[0], 10.0, 10.0, 100.0, 50.0, "abc");
    s.frame_move(FrameMove {
        doc_id: None,
        frame_id: f.frame_id,
        x: Some(50.0),
        y: None,
        dx: None,
        dy: Some(5.0),
        snap: false,
    })
    .unwrap();
    let get = |s: &mut Session| {
        s.frame_get(FrameRef {
            doc_id: None,
            frame_id: f.frame_id,
        })
        .unwrap()
        .summary
        .rect
    };
    assert_eq!((get(&mut s).x, get(&mut s).y), (50.0, 15.0));
    let u = s.undo(DocRef::default()).unwrap();
    assert!(u.done && u.can_redo);
    assert_eq!(u.label.as_deref(), Some("set frame rect"));
    assert_eq!(get(&mut s).x, 10.0);
    s.redo(DocRef::default()).unwrap();
    assert_eq!(get(&mut s).x, 50.0);
    let h = s.history_list(DocRef::default()).unwrap();
    assert_eq!(h.entries.len(), 2);
    assert!(s.undo(DocRef::default()).unwrap().done);
    assert!(s.undo(DocRef::default()).unwrap().done);
    assert!(!s.undo(DocRef::default()).unwrap().done);
}

#[test]
fn structured_errors() {
    let mut s = session();
    let e = s.doc_info(DocRef::default()).unwrap_err();
    assert_eq!(e.code, "no_document");
    assert!(!e.hint.is_empty());
    let d = s.doc_new(DocNew::default()).unwrap();
    let f = text_frame(&mut s, d.page_ids[0], 10.0, 10.0, 100.0, 50.0, "x");
    let e = s
        .frame_delete(FrameDelete {
            doc_id: None,
            frame_id: f.frame_id,
            confirm: false,
        })
        .unwrap_err();
    assert_eq!(e.code, "confirmation_required");
    s.frame_lock(FrameLock {
        doc_id: None,
        frame_id: f.frame_id,
        locked: true,
    })
    .unwrap();
    let e = s
        .frame_rotate(FrameRotate {
            doc_id: None,
            frame_id: f.frame_id,
            degrees: 10.0,
        })
        .unwrap_err();
    assert_eq!(e.code, "locked");
    assert!(e.hint.contains("frame_lock"));
    let e = s
        .text_replace_range(TextReplaceRange {
            doc_id: None,
            frame_id: Some(f.frame_id),
            story_id: None,
            start: 0,
            end: 99,
            text: String::new(),
        })
        .unwrap_err();
    assert_eq!(e.code, "invalid_argument");
    let e = s
        .doc_open(DocOpen {
            path: tmp("missing.pubrs").display().to_string(),
        })
        .unwrap_err();
    assert_eq!(e.code, "file_not_found");
    // Closing with unsaved changes needs confirmation.
    let e = s.doc_close(DocClose::default()).unwrap_err();
    assert_eq!(e.code, "confirmation_required");
    let closed = s
        .doc_close(DocClose {
            doc_id: None,
            confirm: true,
        })
        .unwrap();
    assert!(closed.discarded_changes);
}

#[test]
fn styles_align_group_and_zorder() {
    let mut s = session();
    let d = s.doc_new(DocNew::default()).unwrap();
    let p = d.page_ids[0];
    let st = s
        .style_create(StyleCreate {
            doc_id: None,
            name: "Price".into(),
            kind: None,
            based_on: Some("Body".into()),
            paragraph: Default::default(),
            character: pubrs_api::core::CharProps {
                font_size: Some(18.0),
                bold: Some(true),
                ..Default::default()
            },
        })
        .unwrap();
    let f = text_frame(&mut s, p, 10.0, 10.0, 200.0, 50.0, "$4.50");
    s.text_apply_para_style(TextApplyParaStyle {
        doc_id: None,
        frame_id: Some(f.frame_id),
        story_id: None,
        start: None,
        end: None,
        style: Some("price".into()),
        align: Some(Align::Right),
        line_spacing: None,
        space_before: None,
        space_after: None,
        first_line_indent: None,
        left_indent: None,
        hyphenate: None,
        language: None,
        justify_limits: None,
        right_indent: None,
        keep_with_next: None,
        keep_together: None,
        widow_orphan: None,
        drop_cap: None,
        tab_stops: None,
        list: None,
    })
    .unwrap();
    let got = s
        .text_get(TextRef {
            doc_id: None,
            frame_id: Some(f.frame_id),
            story_id: None,
        })
        .unwrap();
    assert_eq!(got.paragraphs[0].style.as_deref(), Some("Price"));
    assert_eq!(got.paragraphs[0].runs[0].font_size, 18.0);
    assert_eq!(
        got.paragraphs[0].runs[0].font_family, "Liberation Serif",
        "inherited from Body"
    );
    let list = s.style_list(Default::default()).unwrap();
    assert!(list.styles.iter().any(|x| x.style_id == st.style_id));

    let mk = |s: &mut Session, x: f64| {
        s.frame_add_shape(FrameAddShape {
            doc_id: None,
            page_id: Some(p),
            master_id: None,
            shape: ShapeKind::Rectangle,
            x,
            y: x / 2.0,
            width: 40.0,
            height: 40.0,
            corner_radius: None,
            line_direction: None,
            fill: None,
            stroke: None,
            stroke_width: None,
            dash: None,
            name: None,
        })
        .unwrap()
        .frame_id
    };
    let a = mk(&mut s, 100.0);
    let b = mk(&mut s, 200.0);
    let c = mk(&mut s, 400.0);
    s.frame_align(FrameAlign {
        doc_id: None,
        frame_ids: vec![a, b, c],
        edge: pubrs_api::core::align::AlignEdge::Top,
        relative_to: RelativeTo::Selection,
    })
    .unwrap();
    s.frame_distribute(FrameDistribute {
        doc_id: None,
        frame_ids: vec![a, b, c],
        axis: pubrs_api::core::align::DistributeAxis::Horizontal,
        within: RelativeTo::Selection,
    })
    .unwrap();
    let frames = s.frame_list(FrameList::default()).unwrap().frames;
    let pos = |id| frames.iter().find(|f| f.frame_id == id).unwrap().rect;
    assert_eq!(pos(a).y, 50.0);
    assert_eq!(pos(c).y, 50.0);
    assert_eq!(pos(b).x, 250.0, "evenly spaced between 100 and 400");
    let g = s
        .frame_group(FrameGroup {
            doc_id: None,
            frame_ids: vec![a, b],
        })
        .unwrap();
    s.frame_set_zorder(FrameSetZorder {
        doc_id: None,
        frame_id: g.group_id,
        action: Some(ZAction::SendToBack),
        index: None,
    })
    .unwrap();
    let frames = s
        .frame_list(FrameList {
            kind: Some(FrameKind::Group),
            ..Default::default()
        })
        .unwrap()
        .frames;
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].z_index, 0);
    let ug = s
        .frame_ungroup(FrameUngroup {
            doc_id: None,
            group_id: g.group_id,
        })
        .unwrap();
    assert_eq!(ug.frame_ids, vec![a, b]);
}

#[test]
fn preview_and_png_export() {
    let mut s = session();
    s.doc_new(DocNew::default()).unwrap();
    let pv = s.render_preview(RenderPreview::default()).unwrap();
    assert_eq!((pv.width, pv.height), (510, 660), "letter at 60 dpi");
    assert!(!pv.png_base64.is_empty());
    let png = s
        .export_png(ExportPng {
            dpi: Some(72.0),
            ..Default::default()
        })
        .unwrap();
    assert_eq!((png.width, png.height), (612, 792));
}

#[test]
fn validate_reports_overflow_and_empty_placeholder() {
    let mut s = session();
    let d = s.doc_new(DocNew::default()).unwrap();
    let f = text_frame(
        &mut s,
        d.page_ids[0],
        10.0,
        10.0,
        60.0,
        30.0,
        &"word ".repeat(50),
    );
    s.frame_set_placeholder(FrameSetPlaceholder {
        doc_id: None,
        frame_id: f.frame_id,
        name: Some("headline".into()),
        hint: None,
    })
    .unwrap();
    let v = s.validate(DocRef::default()).unwrap();
    let codes: Vec<WarningCode> = v.warnings.iter().map(|w| w.code).collect();
    assert!(codes.contains(&WarningCode::Overflow), "{codes:?}");
    assert!(codes.contains(&WarningCode::EmptyPlaceholder), "{codes:?}");
    s.text_set(TextSet {
        doc_id: None,
        frame_id: Some(f.frame_id),
        story_id: None,
        text: "Hi".into(),
        markup: false,
    })
    .unwrap();
    assert!(s.validate(DocRef::default()).unwrap().ok);
}

#[test]
fn find_and_replace_all_across_stories() {
    let mut s = session();
    let d = s.doc_new(DocNew::default()).unwrap();
    let a = text_frame(
        &mut s,
        d.page_ids[0],
        10.0,
        10.0,
        300.0,
        100.0,
        "Café au lait, cafe noir\nA cat concat",
    );
    let b = text_frame(&mut s, d.page_ids[0], 10.0, 200.0, 300.0, 100.0, "CAT cats");
    let find = |s: &mut Session, q: &str, cs: bool, ww: bool| {
        s.text_find(TextFind {
            doc_id: None,
            frame_id: None,
            story_id: None,
            query: q.into(),
            case_sensitive: cs,
            whole_word: ww,
            start: None,
            end: None,
        })
        .unwrap()
    };
    // Across both stories, in reading order.
    let r = find(&mut s, "cat", false, false);
    assert_eq!(r.count, 4);
    assert_eq!(r.matches[0].story_id, a.story_id.unwrap());
    assert_eq!(r.matches[0].frame_id, Some(a.frame_id));
    assert_eq!(r.matches[3].story_id, b.story_id.unwrap());
    assert_eq!(r.matches[3].frame_id, Some(b.frame_id));
    assert!(r.matches[0].context.contains("cat"));
    assert_eq!(find(&mut s, "cat", true, false).count, 3);
    assert_eq!(find(&mut s, "cat", false, true).count, 2);
    assert_eq!(find(&mut s, "café", false, false).count, 1);
    assert_eq!(find(&mut s, "zzz", false, false).count, 0);

    // Selection-only range on one story.
    let sel = s
        .text_find(TextFind {
            doc_id: None,
            frame_id: Some(a.frame_id),
            story_id: None,
            query: "c".into(),
            case_sensitive: false,
            whole_word: false,
            start: Some(0),
            end: Some(4),
        })
        .unwrap();
    assert_eq!(sel.count, 1);
    assert_eq!((sel.matches[0].start, sel.matches[0].end), (0, 1));

    // Replace all: one undo step restores everything.
    let before = s.history_list(DocRef::default()).unwrap().entries.len();
    let rep = |s: &mut Session, q: &str, with: &str, range: Option<(usize, usize)>| {
        s.text_replace_all(TextReplaceAll {
            doc_id: None,
            frame_id: range.map(|_| a.frame_id),
            story_id: None,
            query: q.into(),
            replacement: with.into(),
            case_sensitive: false,
            whole_word: false,
            start: range.map(|r| r.0),
            end: range.map(|r| r.1),
        })
        .unwrap()
    };
    let out = rep(&mut s, "cat", "dog", None);
    assert_eq!(out.count, 4);
    assert_eq!(find(&mut s, "cat", false, false).count, 0);
    assert_eq!(find(&mut s, "dog", false, false).count, 4);
    assert_eq!(
        s.history_list(DocRef::default()).unwrap().entries.len(),
        before + 1
    );
    assert!(s.undo(DocRef::default()).unwrap().done);
    assert_eq!(find(&mut s, "cat", false, false).count, 4);
    assert_eq!(find(&mut s, "dog", false, false).count, 0);

    // A zero-match call is a no-op.
    let rev = s.history_list(DocRef::default()).unwrap().entries.len();
    let none = rep(&mut s, "zzz", "y", None);
    assert_eq!(none.count, 0);
    assert_eq!(
        s.history_list(DocRef::default()).unwrap().entries.len(),
        rev
    );

    // Selection-only replacement touches only the range.
    let out = rep(&mut s, "ca", "X", Some((0, 6)));
    assert_eq!(out.count, 1);
    assert_eq!(find(&mut s, "ca", false, false).count, 5);
}

#[test]
fn spelling_tools_check_suggest_and_add_words() {
    use pubrs_api::tools::spell::*;
    use serde_json::json;
    let dir = tmp("spelling-dicts");
    let _ = std::fs::remove_dir_all(&dir);
    let config = SessionConfig {
        id_seed: Some(7),
        font_dir: Some(root().join("tests/fonts")),
        dictionaries_dir: Some(dir.clone()),
        ..Default::default()
    };
    let mut s = Session::new(config.clone());
    let doc = s.doc_new(DocNew::default()).unwrap();
    let text = "Teh baker bakes bred.\nUne baguette trés bonne.\nVisit https://pubrs.example, ask XQZT about Kerncraft.";
    let f = text_frame(&mut s, doc.page_ids[0], 72.0, 72.0, 400.0, 300.0, text);
    pubrs_api::call_tool(
        &mut s,
        "text_apply_para_style",
        json!({"frame_id": f.frame_id, "start": 22, "end": 23, "language": "fr-FR"}),
    )
    .unwrap();
    let check = |s: &mut Session, caps: bool| {
        s.spell_check(SpellCheck {
            doc_id: None,
            frame_id: None,
            story_id: None,
            ignore_all_caps: caps,
        })
        .unwrap()
    };
    let out = check(&mut s, true);
    let words: Vec<&str> = out.issues.iter().map(|i| i.word.as_str()).collect();
    assert_eq!(words, ["Teh", "Kerncraft"]);
    assert_eq!((out.issues[0].start, out.issues[0].end), (0, 3));
    assert_eq!(out.issues[0].frame_id, Some(f.frame_id));
    assert_eq!(out.issues[0].language, "en-US");
    assert_eq!(out.unchecked_languages, ["fr-FR"]);
    assert_eq!(out.dictionaries, ["en_US"]);
    // Capitals only are checked on request.
    let caps: Vec<String> = check(&mut s, false)
        .issues
        .into_iter()
        .map(|i| i.word)
        .collect();
    assert_eq!(caps, ["Teh", "XQZT", "Kerncraft"]);
    // One story by frame gives the same as the document here.
    let one = s
        .spell_check(SpellCheck {
            doc_id: None,
            frame_id: Some(f.frame_id),
            story_id: None,
            ignore_all_caps: true,
        })
        .unwrap();
    assert_eq!(one.issues, out.issues);

    let sug = s
        .spell_suggest(SpellSuggest {
            word: "Teh".into(),
            language: Some(out.issues[0].language.clone()),
        })
        .unwrap();
    assert!(!sug.correct);
    assert_eq!(sug.dictionary, "en_US");
    assert!(
        sug.suggestions
            .iter()
            .any(|w| w.eq_ignore_ascii_case("the")),
        "{:?}",
        sug.suggestions
    );
    // Fix it the way a client would: replace the range.
    s.text_replace_range(TextReplaceRange {
        doc_id: None,
        frame_id: Some(f.frame_id),
        story_id: None,
        start: out.issues[0].start,
        end: out.issues[0].end,
        text: "The".into(),
    })
    .unwrap();
    let added = s
        .spell_add_word(SpellAddWord {
            word: "Kerncraft".into(),
        })
        .unwrap();
    assert!(added.added);
    assert_eq!(
        added.path.as_deref().map(PathBuf::from),
        Some(dir.join("user-words.txt"))
    );
    assert_eq!(check(&mut s, true).count, 0);
    // A new session reads the user dictionary.
    let mut s2 = Session::new(config);
    s2.doc_new(DocNew::default()).unwrap();
    let f2 = text_frame(
        &mut s2,
        doc.page_ids[0],
        0.0,
        0.0,
        300.0,
        100.0,
        "Kerncraft rocks",
    );
    let again = s2
        .spell_check(SpellCheck {
            doc_id: None,
            frame_id: Some(f2.frame_id),
            story_id: None,
            ignore_all_caps: true,
        })
        .unwrap();
    assert_eq!(again.count, 0);

    // Errors.
    let e = s
        .spell_add_word(SpellAddWord {
            word: "two words".into(),
        })
        .unwrap_err();
    assert_eq!(e.code, "invalid_argument");
    let e = s
        .spell_suggest(SpellSuggest {
            word: "chaton".into(),
            language: Some("fr".into()),
        })
        .unwrap_err();
    assert_eq!(e.code, "not_found");
    let e = pubrs_api::call_tool(&mut s, "spell_check", json!({"language": "en"})).unwrap_err();
    assert_eq!(e.code, "invalid_argument", "unknown fields are refused");
    let _ = std::fs::remove_dir_all(dir);
}

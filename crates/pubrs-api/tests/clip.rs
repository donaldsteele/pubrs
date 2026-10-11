//! Copying frames between open documents (`frame_copy_to_doc`, the
//! clipboard's paste): ids, text, chains, groups, pictures, styles and
//! swatches arrive intact, as one undo step.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use pubrs_api::core::{Color, DocumentId, FrameId, FrameKind, Paint, Parent};
use pubrs_api::tools::frames::{ClipPayload, FrameCopyToDoc, Offset, PasteAt};
use pubrs_api::{Session, SessionConfig, call_tool};
use serde_json::{Value, json};

fn session() -> Session {
    Session::new(SessionConfig {
        id_seed: Some(11),
        font_dir: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fonts")),
        ..Default::default()
    })
}

fn call(s: &mut Session, tool: &str, args: Value) -> Value {
    call_tool(s, tool, args).unwrap_or_else(|e| panic!("{tool}: {} ({})", e.message, e.hint))
}

fn id<T: serde::de::DeserializeOwned>(v: &Value, key: &str) -> T {
    serde_json::from_value(v[key].clone()).unwrap()
}

fn new_doc(s: &mut Session) -> (DocumentId, pubrs_api::core::PageId) {
    let out = call(s, "doc_new", json!({}));
    (
        id(&out, "doc_id"),
        id::<Vec<pubrs_api::core::PageId>>(&out, "page_ids")[0],
    )
}

struct Source {
    doc: DocumentId,
    all: Vec<FrameId>,
    asset_bytes: Vec<u8>,
}

fn source(s: &mut Session) -> Source {
    let (doc, page) = new_doc(s);
    let sw = call(
        s,
        "swatch_create",
        json!({"doc_id": doc, "name": "Crust", "value": {"rgb": "#B45309"}}),
    );
    let head = call(
        s,
        "frame_add_text",
        json!({"doc_id": doc, "page_id": page, "x": 36, "y": 36, "width": 300, "height": 40,
               "text": "# Fresh bread", "markup": true}),
    );
    call(
        s,
        "text_apply_char_style",
        json!({"doc_id": doc, "frame_id": head["frame_id"], "color": {"swatch": sw["swatch_id"]}}),
    );
    // Two linked frames carrying one story.
    let a = call(
        s,
        "frame_add_text",
        json!({"doc_id": doc, "page_id": page, "x": 36, "y": 100, "width": 100, "height": 30, "text": "Alpha beta gamma delta epsilon zeta eta theta iota kappa lambda"}),
    );
    let b = call(
        s,
        "frame_add_text",
        json!({"doc_id": doc, "page_id": page, "x": 160, "y": 100, "width": 100, "height": 30}),
    );
    call(
        s,
        "text_link_frames",
        json!({"doc_id": doc, "from_frame_id": a["frame_id"], "to_frame_id": b["frame_id"]}),
    );
    // A picture.
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("api-clip");
    std::fs::create_dir_all(&dir).unwrap();
    let png = dir.join(format!(
        "{}.png",
        std::thread::current().name().unwrap_or("t")
    ));
    let mut pm = pubrs_api::render::tiny_skia::Pixmap::new(20, 10).unwrap();
    pm.fill(pubrs_api::render::tiny_skia::Color::from_rgba8(
        10, 120, 200, 255,
    ));
    let bytes = pm.encode_png().unwrap();
    std::fs::write(&png, &bytes).unwrap();
    let img = call(
        s,
        "frame_add_image",
        json!({"doc_id": doc, "page_id": page, "x": 36, "y": 200, "width": 80, "height": 40, "path": png.display().to_string()}),
    );
    // A group of two shapes.
    let r1 = call(
        s,
        "frame_add_shape",
        json!({"doc_id": doc, "page_id": page, "shape": "rectangle", "x": 200, "y": 200, "width": 30, "height": 30}),
    );
    let r2 = call(
        s,
        "frame_add_shape",
        json!({"doc_id": doc, "page_id": page, "shape": "ellipse", "x": 240, "y": 200, "width": 30, "height": 30}),
    );
    let g = call(
        s,
        "frame_group",
        json!({"doc_id": doc, "frame_ids": [r1["frame_id"], r2["frame_id"]]}),
    );
    Source {
        doc,
        all: vec![
            id(&head, "frame_id"),
            id(&a, "frame_id"),
            id(&b, "frame_id"),
            id(&img, "frame_id"),
            id(&g, "group_id"),
        ],
        asset_bytes: bytes,
    }
}

#[test]
fn frames_arrive_in_another_document_as_one_undo_step() {
    let mut s = session();
    let src = source(&mut s);
    let (dst, _) = new_doc(&mut s);
    let before = s.get(Some(dst)).unwrap().revision();
    let out = s
        .frame_copy_to_doc(FrameCopyToDoc {
            from_doc_id: src.doc,
            frame_ids: src.all.clone(),
            to_doc_id: dst,
            page_index: 0,
            offset: Some(Offset { x: 10.0, y: 20.0 }),
        })
        .unwrap();
    assert_eq!(out.frame_ids.len(), 5);
    assert_eq!(
        s.get(Some(dst)).unwrap().revision(),
        before + 1,
        "one command"
    );
    let (d, from) = (
        s.get(Some(dst)).unwrap().doc().clone(),
        s.get(Some(src.doc)).unwrap().doc().clone(),
    );
    // New ids everywhere; the source is untouched.
    assert!(out.frame_ids.iter().all(|f| !from.frames.contains_key(f)));
    assert_eq!(
        from.frames.len(),
        7,
        "source: head, 2 chain, picture, 2 shapes, group"
    );
    // Order and kinds survive.
    let kinds: Vec<FrameKind> = out
        .frame_ids
        .iter()
        .map(|f| d.frame(*f).unwrap().kind())
        .collect();
    assert_eq!(
        kinds,
        vec![
            FrameKind::Text,
            FrameKind::Text,
            FrameKind::Text,
            FrameKind::Image,
            FrameKind::Group
        ]
    );
    // Offset applied.
    let (o, n) = (
        from.frame(src.all[0]).unwrap().rect,
        d.frame(out.frame_ids[0]).unwrap().rect,
    );
    assert_eq!((n.x - o.x, n.y - o.y), (10.0, 20.0));
    // The chain is a chain with the whole text in front.
    let t1 = d.frame(out.frame_ids[1]).unwrap().as_text().unwrap().story;
    let t2 = d.frame(out.frame_ids[2]).unwrap().as_text().unwrap().story;
    assert_eq!(t1, t2);
    assert_eq!(
        d.story(t1).unwrap().frames,
        vec![out.frame_ids[1], out.frame_ids[2]]
    );
    assert!(d.story(t1).unwrap().plain_text().starts_with("Alpha beta"));
    // The picture's bytes came along.
    let asset = d
        .frame(out.frame_ids[3])
        .unwrap()
        .as_image()
        .unwrap()
        .asset
        .unwrap();
    assert_eq!(&d.blobs[&asset][..], &src.asset_bytes[..]);
    // The group has its two members, which are new frames too.
    let kids = match &d.frame(out.frame_ids[4]).unwrap().content {
        pubrs_api::core::FrameContent::Group(g) => g.children.clone(),
        c => panic!("{c:?}"),
    };
    assert_eq!(kids.len(), 2);
    assert!(
        kids.iter()
            .all(|k| d.parent_of(*k) == Some(Parent::Group(out.frame_ids[4])))
    );
    // Heading style matched by name; the swatch the target lacks became its colour.
    let head_story = d
        .story(d.frame(out.frame_ids[0]).unwrap().as_text().unwrap().story)
        .unwrap();
    assert_eq!(
        head_story.paragraphs[0].style,
        Some(d.styles.by_name("Heading 1").unwrap().id)
    );
    let colour = head_story.paragraphs[0].runs[0].props.color.clone();
    assert_eq!(colour, Some(Paint::Color(Color::rgb(0xB4, 0x53, 0x09))));
    // Undo takes it all back, assets included.
    s.undo(Default::default()).ok();
    let d = s.get(Some(dst)).unwrap().doc();
    assert!(d.frames.is_empty() && d.assets.is_empty() && d.blobs.is_empty());
}

#[test]
fn pasting_twice_reuses_the_picture_and_the_same_document_keeps_swatches() {
    let mut s = session();
    let src = source(&mut s);
    let payload: ClipPayload = s
        .clip_copy(Some(src.doc), &[src.all[0], src.all[3]])
        .unwrap();
    // Through the clipboard's text form.
    let payload = ClipPayload::from_json(&payload.to_json()).unwrap();
    let page = s.get(Some(src.doc)).unwrap().doc().pages[0].id;
    let at = PasteAt {
        parent: Parent::Page(page),
        dx: 18.0,
        dy: 18.0,
    };
    let a = s.clip_paste(Some(src.doc), &payload, at).unwrap();
    let b = s.clip_paste(Some(src.doc), &payload, at).unwrap();
    let d = s.get(Some(src.doc)).unwrap().doc();
    assert_eq!(d.assets.len(), 1, "same bytes, one asset");
    for f in a.frame_ids.iter().chain(&b.frame_ids) {
        if let Some(i) = d.frame(*f).unwrap().as_image() {
            assert_eq!(i.asset, d.assets.keys().next().copied());
        }
    }
    // Same document: the swatch is still a swatch.
    let story = d
        .story(d.frame(a.frame_ids[0]).unwrap().as_text().unwrap().story)
        .unwrap();
    assert!(matches!(
        story.paragraphs[0].runs[0].props.color,
        Some(Paint::Swatch { .. })
    ));
}

#[test]
fn copy_to_doc_reports_bad_requests() {
    let mut s = session();
    let src = source(&mut s);
    let (dst, _) = new_doc(&mut s);
    let bad = |s: &mut Session, req: FrameCopyToDoc| s.frame_copy_to_doc(req).unwrap_err().code;
    let req = |frames: Vec<FrameId>, page_index: usize| FrameCopyToDoc {
        from_doc_id: src.doc,
        frame_ids: frames,
        to_doc_id: dst,
        page_index,
        offset: None,
    };
    assert_eq!(bad(&mut s, req(vec![], 0)), "invalid_argument");
    assert_eq!(bad(&mut s, req(src.all.clone(), 5)), "not_found");
    let ghost = FrameId::new(&mut pubrs_api::core::SeqIds::new(999));
    assert_eq!(bad(&mut s, req(vec![ghost], 0)), "not_found");
    let r = call_tool(
        &mut s,
        "frame_copy_to_doc",
        json!({"from_doc_id": src.doc, "frame_ids": [], "to_doc_id": dst, "page_index": 0, "bogus": 1}),
    );
    assert!(r.is_err(), "unknown fields are refused");
}

#[test]
fn a_table_copies_with_fresh_cell_stories() {
    let mut s = session();
    let (src, page) = new_doc(&mut s);
    let t = call(
        &mut s,
        "table_add",
        json!({"doc_id": src, "page_id": page, "x": 36, "y": 36, "width": 240, "height": 80, "rows": 2, "cols": 2}),
    );
    let tf: FrameId = id(&t, "frame_id");
    for (r, c, text) in [(0, 0, "A1"), (0, 1, "B1"), (1, 0, "A2"), (1, 1, "B2")] {
        call(
            &mut s,
            "table_set_cell_text",
            json!({"doc_id": src, "frame_id": tf, "row": r, "col": c, "text": text}),
        );
    }
    let (dst, _) = new_doc(&mut s);
    let out = s
        .frame_copy_to_doc(FrameCopyToDoc {
            from_doc_id: src,
            frame_ids: vec![tf],
            to_doc_id: dst,
            page_index: 0,
            offset: None,
        })
        .unwrap();
    let nf = out.frame_ids[0];
    let (a, b) = (
        s.get(Some(src)).unwrap().doc().clone(),
        s.get(Some(dst)).unwrap().doc().clone(),
    );
    assert!(pubrs_api::core::validate::check_integrity(&a).is_empty());
    assert!(pubrs_api::core::validate::check_integrity(&b).is_empty());
    let text = |d: &pubrs_api::core::Document, f: FrameId| -> Vec<String> {
        d.frame(f)
            .unwrap()
            .as_table()
            .unwrap()
            .cells
            .iter()
            .map(|c| d.story(c.story).unwrap().plain_text())
            .collect()
    };
    assert_eq!(text(&a, tf), text(&b, nf));
    assert_eq!(text(&b, nf), vec!["A1", "B1", "A2", "B2"]);
    let (sa, sb): (Vec<_>, Vec<_>) = (
        a.frame(tf)
            .unwrap()
            .as_table()
            .unwrap()
            .cells
            .iter()
            .map(|c| c.story)
            .collect(),
        b.frame(nf)
            .unwrap()
            .as_table()
            .unwrap()
            .cells
            .iter()
            .map(|c| c.story)
            .collect(),
    );
    assert!(sb.iter().all(|x| !sa.contains(x)));
    assert!(sb.iter().all(|x| b.story(*x).unwrap().frames == [nf]));
    // Same-document paste: fresh stories again, and the clipboard text has the cells.
    let payload = s.clip_copy(Some(src), &[tf]).unwrap();
    assert_eq!(payload.plain_text(), "A1\tB1\nA2\tB2");
    let again = s
        .clip_paste(
            Some(src),
            &payload,
            PasteAt {
                parent: Parent::Page(page),
                dx: 18.0,
                dy: 18.0,
            },
        )
        .unwrap();
    let a2 = s.get(Some(src)).unwrap().doc();
    assert!(pubrs_api::core::validate::check_integrity(a2).is_empty());
    let cells: Vec<_> = a2
        .frame(again.frame_ids[0])
        .unwrap()
        .as_table()
        .unwrap()
        .cells
        .iter()
        .map(|c| c.story)
        .collect();
    assert!(cells.iter().all(|x| !sa.contains(x)));
}

fn one_text(s: &mut Session) -> (DocumentId, pubrs_api::core::PageId, FrameId) {
    let (doc, page) = new_doc(s);
    let t = call(
        s,
        "frame_add_text",
        json!({"doc_id": doc, "page_id": page, "x": 36, "y": 36, "width": 200, "height": 40, "text": "Hello"}),
    );
    (doc, page, id(&t, "frame_id"))
}

fn paste(
    s: &mut Session,
    doc: DocumentId,
    page: pubrs_api::core::PageId,
    p: &ClipPayload,
) -> pubrs_api::ApiResult<pubrs_api::tools::frames::FrameCopyOut> {
    s.clip_paste(
        Some(doc),
        p,
        PasteAt {
            parent: Parent::Page(page),
            dx: 0.0,
            dy: 0.0,
        },
    )
}

#[test]
fn a_swatched_table_pastes_with_plain_colours_where_the_swatch_is_missing() {
    let mut s = session();
    let (src, page) = new_doc(&mut s);
    let sw = call(
        &mut s,
        "swatch_create",
        json!({"doc_id": src, "name": "Brand", "value": {"rgb": "#112233"}}),
    );
    let t = call(
        &mut s,
        "table_add",
        json!({"doc_id": src, "page_id": page, "x": 36, "y": 36, "width": 200, "height": 60,
               "rows": 2, "cols": 2, "header_fill": {"swatch": sw["swatch_id"]}}),
    );
    let tf: FrameId = id(&t, "frame_id");
    call(
        &mut s,
        "table_set_cell_style",
        json!({"doc_id": src, "frame_id": tf, "row": 1, "col": 0, "fill": {"swatch": sw["swatch_id"], "tint": 50},
               "border": {"color": {"swatch": sw["swatch_id"]}, "width": 2}}),
    );
    let (dst, _) = new_doc(&mut s);
    let out = s
        .frame_copy_to_doc(FrameCopyToDoc {
            from_doc_id: src,
            frame_ids: vec![tf],
            to_doc_id: dst,
            page_index: 0,
            offset: None,
        })
        .unwrap();
    let d = s.get(Some(dst)).unwrap().doc();
    assert!(pubrs_api::core::validate::check_integrity(d).is_empty());
    let table = d.frame(out.frame_ids[0]).unwrap().as_table().unwrap();
    let mut seen = 0;
    for c in &table.cells {
        for p in c
            .fill
            .iter()
            .chain(c.borders.iter().flatten().map(|b| &b.color))
        {
            assert!(matches!(p, Paint::Color(_)), "{p:?}");
            seen += 1;
        }
    }
    assert!(seen >= 5, "{seen} paints checked");
    // Header fill is the swatch colour; the 50% tint cell is lighter.
    assert_eq!(
        table.cells[0].fill,
        Some(Paint::Color(Color::rgb(0x11, 0x22, 0x33)))
    );
    assert_ne!(table.cells[2].fill, table.cells[0].fill);
}

#[test]
fn a_same_named_swatch_is_used_and_a_same_id_other_name_is_not_adopted() {
    let mut s = session();
    let (src, page) = new_doc(&mut s);
    let sw = call(
        &mut s,
        "swatch_create",
        json!({"doc_id": src, "name": "Brand", "value": {"rgb": "#112233"}}),
    );
    let t = call(
        &mut s,
        "frame_add_text",
        json!({"doc_id": src, "page_id": page, "x": 10, "y": 10, "width": 100, "height": 30, "text": "x"}),
    );
    call(
        &mut s,
        "text_apply_char_style",
        json!({"doc_id": src, "frame_id": t["frame_id"], "color": {"swatch": sw["swatch_id"]}}),
    );
    let (dst, dpage) = new_doc(&mut s);
    let other = call(
        &mut s,
        "swatch_create",
        json!({"doc_id": dst, "name": "brand", "value": {"rgb": "#FF0000"}}),
    );
    let payload = s.clip_copy(Some(src), &[id(&t, "frame_id")]).unwrap();
    let out = paste(&mut s, dst, dpage, &payload).unwrap();
    let d = s.get(Some(dst)).unwrap().doc();
    let st = d
        .story(d.frame(out.frame_ids[0]).unwrap().as_text().unwrap().story)
        .unwrap();
    let want: pubrs_api::core::SwatchId = id(&other, "swatch_id");
    assert!(
        matches!(&st.paragraphs[0].runs[0].props.color, Some(Paint::Swatch { swatch, .. }) if *swatch == want)
    );
    // A payload whose swatch id exists in the target under another name: the
    // colour is used, the target's swatch is not adopted.
    let mut forged = payload.clone();
    forged.swatches[0].id = want;
    forged.swatches[0].name = "Something else".into();
    for f in &mut forged.stories {
        for p in &mut f.paragraphs {
            for r in &mut p.runs {
                r.props.color = Some(Paint::Swatch {
                    swatch: want,
                    tint: 100.0,
                });
            }
        }
    }
    let out = paste(&mut s, dst, dpage, &forged).unwrap();
    let d = s.get(Some(dst)).unwrap().doc();
    let st = d
        .story(d.frame(out.frame_ids[0]).unwrap().as_text().unwrap().story)
        .unwrap();
    assert_eq!(
        st.paragraphs[0].runs[0].props.color,
        Some(Paint::Color(Color::rgb(0x11, 0x22, 0x33)))
    );
}

#[test]
fn hostile_payloads_are_refused_or_repaired() {
    let t0 = std::time::Instant::now();
    let mut s = session();
    let (doc, page, f) = one_text(&mut s);
    let good = s.clip_copy(Some(doc), &[f]).unwrap();
    // Oversized JSON.
    assert!(ClipPayload::parse(&" ".repeat(pubrs_api::tools::clip::MAX_CLIP_JSON + 1)).is_err());
    // Too many stories.
    let mut many = good.clone();
    let st = many.stories[0].clone();
    many.stories = vec![st; 30_000];
    assert!(many.check().is_err());
    assert!(paste(&mut s, doc, page, &many).is_err());
    assert!(ClipPayload::from_json(&many.to_json()).is_none());
    // Too many frames and swatches.
    let mut frames = good.clone();
    frames.frames = vec![frames.frames[0].clone(); 5_001];
    assert!(frames.check().is_err());
    // A story with no paragraphs is repaired.
    let mut empty = good.clone();
    empty.stories[0].paragraphs.clear();
    let out = paste(&mut s, doc, page, &empty).unwrap();
    let d = s.get(Some(doc)).unwrap().doc();
    let st = d
        .story(d.frame(out.frame_ids[0]).unwrap().as_text().unwrap().story)
        .unwrap();
    assert_eq!(st.paragraphs.len(), 1);
    assert!(pubrs_api::core::validate::check_integrity(d).is_empty());
    // Hostile text frame settings.
    let mut cols = good.clone();
    if let pubrs_api::core::FrameContent::Text(t) = &mut cols.frames[0].content {
        t.columns = 4_000_000_000;
    }
    assert!(paste(&mut s, doc, page, &cols).is_err());
    assert!(t0.elapsed().as_secs_f64() < 2.0, "{:?}", t0.elapsed());
}

fn picture_payload(s: &mut Session) -> (DocumentId, pubrs_api::core::PageId, ClipPayload) {
    let (doc, page) = new_doc(s);
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("api-clip");
    std::fs::create_dir_all(&dir).unwrap();
    let png = dir.join(format!(
        "h-{}.png",
        std::thread::current().name().unwrap_or("t")
    ));
    let mut pm = pubrs_api::render::tiny_skia::Pixmap::new(20, 10).unwrap();
    pm.fill(pubrs_api::render::tiny_skia::Color::from_rgba8(
        1, 2, 3, 255,
    ));
    std::fs::write(&png, pm.encode_png().unwrap()).unwrap();
    let img = call(
        s,
        "frame_add_image",
        json!({"doc_id": doc, "page_id": page, "x": 10, "y": 10, "width": 40, "height": 20, "path": png.display().to_string()}),
    );
    let p = s.clip_copy(Some(doc), &[id(&img, "frame_id")]).unwrap();
    (doc, page, p)
}

#[test]
fn pasted_images_are_read_from_their_bytes() {
    let mut s = session();
    let (doc, page, good) = picture_payload(&mut s);
    // Junk bytes that claim to be huge.
    let mut junk = good.clone();
    junk.assets[0].data = pubrs_api::core::Bytes::new(b"\x89PNG\r\n\x1a\njunk junk".to_vec());
    junk.assets[0].asset.pixel_width = 4_000;
    junk.assets[0].asset.pixel_height = 1_000;
    assert!(paste(&mut s, doc, page, &junk).is_err());
    // A real picture whose metadata lies about its size: the bytes win.
    let mut lie = good.clone();
    lie.assets[0].asset.pixel_width = 4_000_000;
    lie.assets[0].asset.pixel_height = 4_000_000;
    lie.assets[0].asset.format = pubrs_api::core::ImageFormat::Jpeg;
    let (dst, dpage) = new_doc(&mut s);
    let out = paste(&mut s, dst, dpage, &lie).unwrap();
    let d = s.get(Some(dst)).unwrap().doc();
    let a = d
        .frame(out.frame_ids[0])
        .unwrap()
        .as_image()
        .unwrap()
        .asset
        .unwrap();
    let m = &d.assets[&a];
    assert_eq!((m.pixel_width, m.pixel_height), (20, 10));
    assert_eq!(m.format, pubrs_api::core::ImageFormat::Png);
    // An image frame that names an image the payload does not carry.
    let mut missing = good.clone();
    missing.assets.clear();
    assert!(paste(&mut s, dst, dpage, &missing).is_err());
    // Too many images.
    let mut lots = good;
    lots.assets = vec![lots.assets[0].clone(); 300];
    assert!(lots.check().is_err());
}

#[test]
fn a_group_with_one_member_pastes_ungrouped() {
    let mut s = session();
    let (doc, page) = new_doc(&mut s);
    let r1 = call(
        &mut s,
        "frame_add_shape",
        json!({"doc_id": doc, "page_id": page, "shape": "rectangle", "x": 10, "y": 10, "width": 20, "height": 20}),
    );
    let r2 = call(
        &mut s,
        "frame_add_shape",
        json!({"doc_id": doc, "page_id": page, "shape": "ellipse", "x": 40, "y": 10, "width": 20, "height": 20}),
    );
    let g = call(
        &mut s,
        "frame_group",
        json!({"doc_id": doc, "frame_ids": [r1["frame_id"], r2["frame_id"]]}),
    );
    let mut p = s.clip_copy(Some(doc), &[id(&g, "group_id")]).unwrap();
    // Drop one member from the payload: the group has a single member left.
    let gone: FrameId = id(&r2, "frame_id");
    p.frames.retain(|f| f.id != gone);
    let (dst, dpage) = new_doc(&mut s);
    let out = paste(&mut s, dst, dpage, &p).unwrap();
    assert_eq!(out.frame_ids.len(), 1);
    let d = s.get(Some(dst)).unwrap().doc();
    assert!(d.frames.values().all(|f| f.kind() != FrameKind::Group));
    assert_eq!(d.frames.len(), 1);
    assert!(d.frame(out.frame_ids[0]).is_some());
    assert!(pubrs_api::core::validate::check_integrity(d).is_empty());
}

/// A payload whose run colour names a swatch that neither it nor the document
/// has is refused whole, and nothing is applied. Note: this is refused by the
/// specific check (the commands validate paints), not by the clone and
/// `check_integrity` catch-all at the end of `clip_paste`. No payload was
/// found that passes every command's own validation and still leaves an
/// integrity error: probes with a dangling swatch in a table cell fill or
/// border, a WordArt shadow, a style colour or a run colour, two frames
/// sharing a story, a group listing a member twice and a paragraph style used
/// as a character style were all refused by the command that builds them (or
/// repaired), so the catch-all stays a safety net with no test of its own.
#[test]
fn a_run_colour_with_a_missing_swatch_is_refused_and_nothing_is_applied() {
    let mut s = session();
    let (doc, page, f) = one_text(&mut s);
    let mut p = s.clip_copy(Some(doc), &[f]).unwrap();
    let ghost = pubrs_api::core::SwatchId::new(&mut pubrs_api::core::SeqIds::new(999));
    assert!(p.swatches.is_empty());
    p.stories[0].paragraphs[0].runs[0].props.color = Some(Paint::swatch(ghost));
    p.check().expect("the payload is within the caps");
    let before = s.get(Some(doc)).unwrap().revision();
    let err = paste(&mut s, doc, page, &p).unwrap_err();
    assert!(
        err.message.contains("swatch") && err.message.contains("does not exist"),
        "{}",
        err.message
    );
    assert_eq!(s.get(Some(doc)).unwrap().revision(), before);
    assert!(pubrs_api::core::validate::check_integrity(s.get(Some(doc)).unwrap().doc()).is_empty());
}

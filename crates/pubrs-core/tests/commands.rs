//! Command system and undo/redo tests.
#![allow(clippy::unwrap_used)]

use pubrs_core::*;

struct Fixture {
    ids: SeqIds,
    ed: Editor,
}

impl Fixture {
    fn new() -> Self {
        let mut ids = SeqIds::new(42);
        let doc = Document::new(&mut ids, PageSetup::default());
        Self {
            ids,
            ed: Editor::new(doc),
        }
    }

    fn page(&self) -> PageId {
        self.ed.doc().pages[0].id
    }

    fn text_frame(&mut self, rect: Rect, text: &str) -> (FrameId, StoryId) {
        let fid = FrameId::new(&mut self.ids);
        let sid = StoryId::new(&mut self.ids);
        let mut story = Story::new(sid, fid);
        story.insert_text(0, text, None);
        let frame = Frame::new(fid, rect, FrameContent::Text(TextFrame::new(sid)));
        self.ed
            .execute(Command::AddFrame {
                parent: Parent::Page(self.page()),
                index: None,
                frame,
                story: Some(story),
            })
            .unwrap();
        (fid, sid)
    }

    fn shape(&mut self, rect: Rect) -> FrameId {
        let fid = FrameId::new(&mut self.ids);
        let frame = Frame::new(
            fid,
            rect,
            FrameContent::Shape(ShapeFrame {
                shape: Shape::Ellipse,
            }),
        );
        self.ed
            .execute(Command::AddFrame {
                parent: Parent::Page(self.page()),
                index: None,
                frame,
                story: None,
            })
            .unwrap();
        fid
    }
}

#[test]
fn undo_redo_restores_every_state_exactly() {
    let mut fx = Fixture::new();
    let mut states = vec![fx.ed.doc().clone()];
    let (t1, s1) = fx.text_frame(Rect::new(36.0, 36.0, 200.0, 100.0), "Hello");
    states.push(fx.ed.doc().clone());
    let sh = fx.shape(Rect::new(300.0, 300.0, 50.0, 50.0));
    states.push(fx.ed.doc().clone());

    let page2 = PageId::new(&mut fx.ids);
    let group = FrameId::new(&mut fx.ids);
    let commands = vec![
        Command::InsertText {
            story: s1,
            at: 5,
            text: " world\nSecond".into(),
            format: None,
        },
        Command::ApplyCharProps {
            story: s1,
            start: 0,
            end: 5,
            props: CharProps {
                bold: Some(true),
                color: Some(Color::rgb(200, 0, 0)),
                ..Default::default()
            },
        },
        Command::SetFrameRect {
            frame: t1,
            rect: Rect::new(40.0, 40.0, 220.0, 120.0),
        },
        Command::SetRotation {
            frame: sh,
            degrees: 30.0,
        },
        Command::AddPage {
            page: Page::new(page2),
            index: None,
        },
        Command::SetPageSize {
            size: PageSizePreset::A4.size(),
        },
        Command::Group {
            group,
            frames: vec![t1, sh],
        },
        Command::SetFrameRect {
            frame: group,
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
        },
        Command::Ungroup { group },
        Command::DeleteFrame { frame: sh },
    ];
    for c in commands {
        fx.ed.execute(c).unwrap();
        states.push(fx.ed.doc().clone());
    }
    let final_state = fx.ed.doc().clone();
    let n = states.len();
    for i in (0..n - 1).rev() {
        fx.ed.undo().expect("can undo");
        assert_eq!(fx.ed.doc(), &states[i], "undo to state {i}");
    }
    assert!(fx.ed.undo().is_none());
    for state in states.iter().skip(1) {
        fx.ed.redo().expect("can redo");
        assert_eq!(fx.ed.doc(), state);
    }
    assert_eq!(fx.ed.doc(), &final_state);
    assert!(fx.ed.redo().is_none());
}

#[test]
fn failing_command_is_atomic() {
    let mut fx = Fixture::new();
    let (_, s1) = fx.text_frame(Rect::new(0.0, 0.0, 100.0, 100.0), "abc");
    let before = fx.ed.doc().clone();
    let rev = fx.ed.revision();
    let bad = Command::Batch {
        label: "mixed".into(),
        commands: vec![
            Command::InsertText {
                story: s1,
                at: 0,
                text: "zzz".into(),
                format: None,
            },
            Command::DeleteText {
                story: s1,
                start: 0,
                end: 999,
            },
        ],
    };
    let err = fx.ed.execute(bad).unwrap_err();
    assert_eq!(err.code(), "invalid_argument");
    assert_eq!(fx.ed.doc(), &before);
    assert_eq!(fx.ed.revision(), rev);
}

#[test]
fn revision_counts_every_change_and_noops_do_not() {
    let mut fx = Fixture::new();
    assert_eq!(fx.ed.revision(), 0);
    let (t, _) = fx.text_frame(Rect::new(0.0, 0.0, 10.0, 10.0), "");
    assert_eq!(fx.ed.revision(), 1);
    let rect = fx.ed.doc().frame(t).unwrap().rect;
    let change = fx
        .ed
        .execute(Command::SetFrameRect { frame: t, rect })
        .unwrap();
    assert_eq!(change.revision, 1, "no-op does not bump revision");
    fx.ed.undo();
    assert_eq!(fx.ed.revision(), 2);
    assert!(fx.ed.is_dirty());
}

#[test]
fn names_match_serialized_tags_and_round_trip() {
    let mut ids = SeqIds::new(1);
    let f = FrameId::new(&mut ids);
    let s = StoryId::new(&mut ids);
    let samples = vec![
        Command::SetMeta {
            title: Some("t".into()),
            author: None,
        },
        Command::SetBleed { bleed: 9.0 },
        Command::DeleteFrame { frame: f },
        Command::InsertText {
            story: s,
            at: 0,
            text: "x".into(),
            format: Some(RunFormat {
                style: None,
                props: CharProps {
                    italic: Some(true),
                    ..Default::default()
                },
            }),
        },
        Command::SetImageCrop {
            frame: f,
            crop: Crop {
                left: 0.1,
                ..Default::default()
            },
        },
        Command::AddAsset {
            asset: Asset {
                id: AssetId::new(&mut ids),
                name: "a.png".into(),
                format: ImageFormat::Png,
                pixel_width: 1,
                pixel_height: 1,
            },
            data: Bytes::new(vec![1, 2, 3]),
        },
        Command::Batch {
            label: "b".into(),
            commands: vec![Command::SetBleed { bleed: 1.0 }],
        },
        Command::Patch(Patch::default()),
    ];
    for c in samples {
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["op"], c.name(), "{v}");
        let back: Command = serde_json::from_value(v).unwrap();
        assert_eq!(back, c);
    }
}

#[test]
fn locked_frames_cannot_move_or_be_deleted() {
    let mut fx = Fixture::new();
    let sh = fx.shape(Rect::new(0.0, 0.0, 10.0, 10.0));
    fx.ed
        .execute(Command::SetLocked {
            frame: sh,
            locked: true,
        })
        .unwrap();
    let e = fx
        .ed
        .execute(Command::SetFrameRect {
            frame: sh,
            rect: Rect::new(1.0, 1.0, 10.0, 10.0),
        })
        .unwrap_err();
    assert_eq!(e.code(), "locked");
    assert_eq!(
        fx.ed
            .execute(Command::DeleteFrame { frame: sh })
            .unwrap_err()
            .code(),
        "locked"
    );
}

#[test]
fn link_and_unlink_frames() {
    let mut fx = Fixture::new();
    let (a, sa) = fx.text_frame(Rect::new(0.0, 0.0, 100.0, 50.0), "First");
    let (b, sb) = fx.text_frame(Rect::new(0.0, 60.0, 100.0, 50.0), "Second");
    fx.ed
        .execute(Command::LinkFrames { from: a, to: b })
        .unwrap();
    let doc = fx.ed.doc();
    assert!(doc.story(sb).is_none(), "target story merged");
    let story = doc.story(sa).unwrap();
    assert_eq!(story.frames, vec![a, b]);
    assert_eq!(story.plain_text(), "First\nSecond");
    assert_eq!(doc.story_of(b), Some(sa));

    let ns = StoryId::new(&mut fx.ids);
    fx.ed
        .execute(Command::UnlinkFrame {
            frame: a,
            new_story: ns,
        })
        .unwrap();
    let doc = fx.ed.doc();
    assert_eq!(doc.story(sa).unwrap().frames, vec![a]);
    assert_eq!(doc.story(ns).unwrap().frames, vec![b]);
    assert!(doc.story(ns).unwrap().is_empty());
    assert_eq!(doc.story(sa).unwrap().plain_text(), "First\nSecond");
}

#[test]
fn deleting_frames_maintains_story_chains() {
    let mut fx = Fixture::new();
    let (a, sa) = fx.text_frame(Rect::new(0.0, 0.0, 100.0, 50.0), "Text");
    let (b, _) = fx.text_frame(Rect::new(0.0, 60.0, 100.0, 50.0), "");
    fx.ed
        .execute(Command::LinkFrames { from: a, to: b })
        .unwrap();
    fx.ed.execute(Command::DeleteFrame { frame: a }).unwrap();
    assert_eq!(fx.ed.doc().story(sa).unwrap().frames, vec![b]);
    fx.ed.execute(Command::DeleteFrame { frame: b }).unwrap();
    assert!(fx.ed.doc().story(sa).is_none());
    assert!(fx.ed.doc().pages[0].frames.is_empty());
}

#[test]
fn group_preserves_order_and_moves_children() {
    let mut fx = Fixture::new();
    let a = fx.shape(Rect::new(0.0, 0.0, 10.0, 10.0));
    let b = fx.shape(Rect::new(100.0, 0.0, 10.0, 10.0));
    let c = fx.shape(Rect::new(50.0, 50.0, 10.0, 10.0));
    let g = FrameId::new(&mut fx.ids);
    fx.ed
        .execute(Command::Group {
            group: g,
            frames: vec![c, a],
        })
        .unwrap();
    let doc = fx.ed.doc();
    assert_eq!(doc.pages[0].frames, vec![b, g], "group takes topmost slot");
    assert_eq!(doc.frame(g).unwrap().children(), &[a, c]);
    assert_eq!(doc.frame(g).unwrap().rect, Rect::new(0.0, 0.0, 60.0, 60.0));
    fx.ed
        .execute(Command::SetFrameRect {
            frame: g,
            rect: Rect::new(100.0, 100.0, 120.0, 60.0),
        })
        .unwrap();
    let doc = fx.ed.doc();
    assert_eq!(
        doc.frame(a).unwrap().rect,
        Rect::new(100.0, 100.0, 20.0, 10.0)
    );
    assert_eq!(
        doc.frame(c).unwrap().rect,
        Rect::new(200.0, 150.0, 20.0, 10.0)
    );
    fx.ed.execute(Command::Ungroup { group: g }).unwrap();
    assert_eq!(fx.ed.doc().pages[0].frames, vec![b, a, c]);
}

#[test]
fn typing_coalesces_into_one_undo_step() {
    let mut fx = Fixture::new();
    let (_, s) = fx.text_frame(Rect::new(0.0, 0.0, 100.0, 50.0), "");
    let before = fx.ed.doc().clone();
    for (i, ch) in "abc".chars().enumerate() {
        fx.ed
            .execute_coalesced(
                Command::InsertText {
                    story: s,
                    at: i,
                    text: ch.to_string(),
                    format: None,
                },
                "typing",
            )
            .unwrap();
    }
    assert_eq!(fx.ed.doc().story(s).unwrap().plain_text(), "abc");
    fx.ed.undo();
    assert_eq!(fx.ed.doc(), &before);
    fx.ed.redo();
    assert_eq!(fx.ed.doc().story(s).unwrap().plain_text(), "abc");
    let h = fx.ed.history();
    assert_eq!(h.last().unwrap().name, "batch");
}

#[test]
fn history_respects_limit() {
    let mut ids = SeqIds::new(5);
    let doc = Document::new(&mut ids, PageSetup::default());
    let mut ed = Editor::new(doc).with_limit(3);
    for i in 0..5 {
        ed.execute(Command::SetBleed {
            bleed: f64::from(i) + 1.0,
        })
        .unwrap();
    }
    assert_eq!(ed.history().len(), 3);
    while ed.undo().is_some() {}
    assert_eq!(ed.doc().setup.bleed, 2.0);
}

#[test]
fn patches_serialize_with_assets() {
    let mut fx = Fixture::new();
    let id = AssetId::new(&mut fx.ids);
    fx.ed
        .execute(Command::AddAsset {
            asset: Asset {
                id,
                name: "x.png".into(),
                format: ImageFormat::Png,
                pixel_width: 2,
                pixel_height: 2,
            },
            data: Bytes::new(vec![9, 8, 7]),
        })
        .unwrap();
    let mut empty = Fixture::new().ed.doc().clone();
    let patch = Patch::diff(&empty, fx.ed.doc());
    let json = serde_json::to_string(&Command::Patch(patch)).unwrap();
    let back: Command = serde_json::from_str(&json).unwrap();
    back.apply(&mut empty).unwrap();
    assert_eq!(&empty, fx.ed.doc());
    assert_eq!(&*empty.blobs[&id], &[9, 8, 7]);
}

#[test]
fn styles_delete_clears_references() {
    let mut fx = Fixture::new();
    let (_, s) = fx.text_frame(Rect::new(0.0, 0.0, 100.0, 50.0), "Title");
    let h1 = fx.ed.doc().styles.by_name("Heading 1").unwrap().id;
    fx.ed
        .execute(Command::SetParaStyle {
            story: s,
            start: 0,
            end: 0,
            style: Some(h1),
        })
        .unwrap();
    let e = fx
        .ed
        .execute(Command::SetCharStyle {
            story: s,
            start: 0,
            end: 2,
            style: Some(h1),
        })
        .unwrap_err();
    assert_eq!(
        e.code(),
        "invalid_argument",
        "paragraph style used as char style"
    );
    fx.ed.execute(Command::DeleteStyle { style: h1 }).unwrap();
    assert_eq!(fx.ed.doc().story(s).unwrap().paragraphs[0].style, None);
    let normal = fx.ed.doc().styles.default_paragraph;
    assert!(
        fx.ed
            .execute(Command::DeleteStyle { style: normal })
            .is_err()
    );
}

#[test]
fn delete_page_removes_its_frames_and_last_page_is_protected() {
    let mut fx = Fixture::new();
    let p1 = fx.page();
    let (_, s) = fx.text_frame(Rect::new(0.0, 0.0, 10.0, 10.0), "x");
    let p2 = PageId::new(&mut fx.ids);
    fx.ed
        .execute(Command::AddPage {
            page: Page::new(p2),
            index: Some(0),
        })
        .unwrap();
    assert_eq!(fx.ed.doc().pages[0].id, p2);
    fx.ed.execute(Command::DeletePage { page: p1 }).unwrap();
    assert!(fx.ed.doc().frames.is_empty());
    assert!(fx.ed.doc().story(s).is_none());
    assert!(fx.ed.execute(Command::DeletePage { page: p2 }).is_err());
}

// ---- golden patch JSON (Risk 3: the Arc refactor must not change patches) ----

struct Recorder {
    fx: Fixture,
    out: Vec<serde_json::Value>,
}

impl Recorder {
    fn record(&mut self, name: &str, f: impl FnOnce(&mut Editor)) {
        let before = self.fx.ed.doc().clone();
        f(&mut self.fx.ed);
        let after = self.fx.ed.doc();
        let fwd = Patch::diff(&before, after);
        let bwd = Patch::diff(after, &before);
        self.out.push(serde_json::json!({
            "step": name,
            "forward": serde_json::to_value(&fwd).unwrap(),
            "backward": serde_json::to_value(&bwd).unwrap(),
        }));
    }

    fn run(&mut self, c: Command) {
        let name = c.name().to_string();
        self.record(&name, |ed| {
            ed.execute(c).unwrap();
        });
    }
}

#[test]
fn patch_json_matches_the_golden_corpus() {
    let mut r = Recorder {
        fx: Fixture::new(),
        out: Vec::new(),
    };
    let ids = &mut r.fx.ids;
    let page = r.fx.ed.doc().pages[0].id;
    let page2 = PageId::new(ids);
    let page3 = PageId::new(ids);
    let master = MasterId::new(ids);
    let g1 = GuideId::new(ids);
    let g2 = GuideId::new(ids);
    let mg = GuideId::new(ids);
    let (f1, s1) = (FrameId::new(ids), StoryId::new(ids));
    let (f2, s2) = (FrameId::new(ids), StoryId::new(ids));
    let (f3, s3) = (FrameId::new(ids), StoryId::new(ids));
    let shape = FrameId::new(ids);
    let img = FrameId::new(ids);
    let group = FrameId::new(ids);
    let mframe = FrameId::new(ids);
    let new_story = StoryId::new(ids);
    let asset = AssetId::new(ids);
    let asset2 = AssetId::new(ids);
    let cstyle = StyleId::new(ids);
    let pstyle = StyleId::new(ids);
    let text_frame = |fid, sid, rect| {
        (
            Frame::new(fid, rect, FrameContent::Text(TextFrame::new(sid))),
            Story::new(sid, fid),
        )
    };

    r.run(Command::SetMeta {
        title: Some("Golden".into()),
        author: Some("Ann".into()),
    });
    r.run(Command::SetPageSize {
        size: PageSizePreset::A4.size(),
    });
    r.run(Command::SetMargins {
        margins: Margins::uniform(30.0),
    });
    r.run(Command::SetBleed { bleed: 9.0 });
    r.run(Command::SetGrid {
        grid: GridSettings {
            show_grid: true,
            ..Default::default()
        },
    });
    r.run(Command::AddPage {
        page: Page::new(page2),
        index: None,
    });
    r.run(Command::AddPage {
        page: Page::new(page3),
        index: Some(0),
    });
    r.run(Command::MovePage {
        page: page3,
        index: 2,
    });
    r.run(Command::AddMaster {
        master: MasterPage {
            id: master,
            name: "M".into(),
            frames: vec![],
            guides: vec![],
        },
    });
    r.run(Command::RenameMaster {
        master,
        name: "Master A".into(),
    });
    r.run(Command::SetPageMaster {
        page,
        master: Some(master),
    });
    r.run(Command::AddGuide {
        owner: Owner::Page(page),
        guide: Guide {
            id: g1,
            axis: Axis::Vertical,
            position: 100.0,
        },
    });
    r.run(Command::AddGuide {
        owner: Owner::Page(page),
        guide: Guide {
            id: g2,
            axis: Axis::Horizontal,
            position: 50.0,
        },
    });
    r.run(Command::MoveGuide {
        owner: Owner::Page(page),
        guide: g1,
        position: 120.0,
    });
    r.run(Command::RemoveGuide {
        owner: Owner::Page(page),
        guide: g2,
    });
    r.run(Command::AddGuide {
        owner: Owner::Master(master),
        guide: Guide {
            id: mg,
            axis: Axis::Vertical,
            position: 10.0,
        },
    });
    let (fr, st) = text_frame(f1, s1, Rect::new(36.0, 36.0, 200.0, 100.0));
    r.run(Command::AddFrame {
        parent: Parent::Page(page),
        index: None,
        frame: fr,
        story: Some(st),
    });
    let (fr, st) = text_frame(f2, s2, Rect::new(36.0, 200.0, 200.0, 100.0));
    r.run(Command::AddFrame {
        parent: Parent::Page(page),
        index: Some(0),
        frame: fr,
        story: Some(st),
    });
    let (fr, st) = text_frame(f3, s3, Rect::new(36.0, 400.0, 200.0, 100.0));
    r.run(Command::AddFrame {
        parent: Parent::Page(page2),
        index: None,
        frame: fr,
        story: Some(st),
    });
    r.run(Command::AddFrame {
        parent: Parent::Page(page),
        index: None,
        frame: Frame::new(
            shape,
            Rect::new(300.0, 300.0, 50.0, 50.0),
            FrameContent::Shape(ShapeFrame {
                shape: Shape::Ellipse,
            }),
        ),
        story: None,
    });
    r.run(Command::AddFrame {
        parent: Parent::Master(master),
        index: None,
        frame: Frame::new(
            mframe,
            Rect::new(0.0, 0.0, 20.0, 20.0),
            FrameContent::Shape(ShapeFrame {
                shape: Shape::Rectangle { corner_radius: 2.0 },
            }),
        ),
        story: None,
    });
    r.run(Command::AddAsset {
        asset: Asset {
            id: asset,
            name: "a.png".into(),
            format: ImageFormat::Png,
            pixel_width: 2,
            pixel_height: 2,
        },
        data: Bytes::new(vec![1, 2, 3, 4]),
    });
    r.run(Command::AddAsset {
        asset: Asset {
            id: asset2,
            name: "b.png".into(),
            format: ImageFormat::Png,
            pixel_width: 1,
            pixel_height: 1,
        },
        data: Bytes::new(vec![9]),
    });
    r.run(Command::AddFrame {
        parent: Parent::Page(page2),
        index: None,
        frame: Frame::new(
            img,
            Rect::new(10.0, 10.0, 80.0, 60.0),
            FrameContent::Image(ImageFrame::new(None)),
        ),
        story: None,
    });
    r.run(Command::SetImage {
        frame: img,
        asset: Some(asset),
    });
    r.run(Command::SetImageCrop {
        frame: img,
        crop: Crop {
            left: 0.1,
            ..Default::default()
        },
    });
    r.run(Command::SetImageFit {
        frame: img,
        fit: ImageFit::Fit,
    });
    r.run(Command::SetLockAspect {
        frame: img,
        lock: false,
    });
    r.run(Command::RemoveAsset { asset: asset2 });
    r.run(Command::SetFrameRect {
        frame: f1,
        rect: Rect::new(40.0, 40.0, 220.0, 120.0),
    });
    r.run(Command::SetRotation {
        frame: shape,
        degrees: 30.0,
    });
    r.run(Command::SetZIndex {
        frame: f1,
        index: 0,
    });
    r.run(Command::SetLocked {
        frame: shape,
        locked: true,
    });
    r.run(Command::SetLocked {
        frame: shape,
        locked: false,
    });
    r.run(Command::SetFrameStyle {
        frame: shape,
        style: FrameStyle {
            fill: Some(Color::rgb(10, 20, 30)),
            stroke: None,
            opacity: 0.5,
        },
    });
    r.run(Command::SetWrap {
        frame: shape,
        wrap: TextWrap {
            mode: WrapMode::Square,
            margin: 4.0,
        },
    });
    r.run(Command::SetFrameName {
        frame: shape,
        name: Some("Blob".into()),
    });
    r.run(Command::SetPlaceholder {
        frame: f2,
        placeholder: Some(Placeholder {
            name: "headline".into(),
            kind: PlaceholderKind::Text,
            hint: "Type".into(),
        }),
    });
    r.run(Command::SetTextFrame {
        frame: f1,
        props: TextFrameProps {
            columns: Some(2),
            column_gap: Some(10.0),
            ..Default::default()
        },
    });
    r.run(Command::SetShape {
        frame: shape,
        shape: Shape::Rectangle { corner_radius: 5.0 },
    });
    r.run(Command::SetStory {
        story: s1,
        paragraphs: vec![Paragraph::default()],
    });
    r.run(Command::InsertText {
        story: s1,
        at: 0,
        text: "Hello world\nSecond paragraph".into(),
        format: None,
    });
    r.run(Command::ReplaceText {
        story: s1,
        start: 0,
        end: 5,
        text: "Howdy".into(),
    });
    r.run(Command::DeleteText {
        story: s1,
        start: 5,
        end: 6,
    });
    r.run(Command::ApplyCharProps {
        story: s1,
        start: 0,
        end: 5,
        props: CharProps {
            bold: Some(true),
            ..Default::default()
        },
    });
    r.run(Command::AddStyle {
        style: Style {
            id: cstyle,
            name: "Loud".into(),
            kind: StyleKind::Character,
            based_on: None,
            paragraph: ParaProps::default(),
            character: CharProps {
                italic: Some(true),
                ..Default::default()
            },
        },
    });
    r.run(Command::AddStyle {
        style: Style {
            id: pstyle,
            name: "Pull".into(),
            kind: StyleKind::Paragraph,
            based_on: None,
            paragraph: ParaProps::default(),
            character: CharProps::default(),
        },
    });
    r.run(Command::SetCharStyle {
        story: s1,
        start: 1,
        end: 3,
        style: Some(cstyle),
    });
    r.run(Command::ClearCharFormat {
        story: s1,
        start: 0,
        end: 2,
    });
    r.run(Command::ApplyParaProps {
        story: s1,
        start: 0,
        end: 3,
        props: ParaProps {
            align: Some(Align::Center),
            ..Default::default()
        },
    });
    r.run(Command::SetParaStyle {
        story: s1,
        start: 0,
        end: 3,
        style: Some(pstyle),
    });
    let mut updated = r.fx.ed.doc().styles.styles.last().unwrap().clone();
    updated.name = "Pull Quote".into();
    r.run(Command::UpdateStyle { style: updated });
    r.run(Command::DeleteStyle { style: cstyle });
    r.run(Command::LinkFrames { from: f1, to: f2 });
    r.run(Command::UnlinkFrame {
        frame: f1,
        new_story,
    });
    r.run(Command::Group {
        group,
        frames: vec![f1, shape],
    });
    r.run(Command::Ungroup { group });
    r.run(Command::SetPageMaster { page, master: None });
    r.run(Command::DeleteMaster { master });
    r.run(Command::Batch {
        label: "batch".into(),
        commands: vec![
            Command::SetBleed { bleed: 3.0 },
            Command::InsertText {
                story: s3,
                at: 0,
                text: "in batch".into(),
                format: None,
            },
        ],
    });
    r.run(Command::DeleteFrame { frame: shape });
    r.run(Command::DeletePage { page: page3 });
    let raw = Patch {
        setup: Some(PageSetup::default()),
        ..Default::default()
    };
    r.run(Command::Patch(raw));

    // Coalesced typing is one history entry; the patch spans all keystrokes.
    r.record("coalesced typing", |ed| {
        for (i, ch) in ["a", "b", "c"].into_iter().enumerate() {
            ed.execute_coalesced(
                Command::InsertText {
                    story: s3,
                    at: i,
                    text: ch.into(),
                    format: None,
                },
                "typing",
            )
            .unwrap();
        }
    });
    r.record("undo", |ed| {
        ed.undo().unwrap();
    });
    r.record("redo", |ed| {
        ed.redo().unwrap();
    });
    r.record("undo twice", |ed| {
        ed.undo().unwrap();
        ed.undo().unwrap();
    });

    let actual = serde_json::to_string_pretty(&r.out).unwrap() + "\n";
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/patch_corpus.json");
    if std::env::var_os("PUBRS_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &actual).unwrap();
    }
    let expected = std::fs::read_to_string(&path)
        .expect("fixture missing; run with PUBRS_BLESS=1 on known-good code");
    assert!(
        expected == actual,
        "patch JSON drifted from {}; if intended, re-run with PUBRS_BLESS=1",
        path.display()
    );
}

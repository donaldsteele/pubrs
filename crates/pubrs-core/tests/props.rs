//! Property tests: random command sequences (including failing ones) keep
//! undo/redo exact.

use std::sync::Arc;

use proptest::prelude::*;
use pubrs_core::*;

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
        0u8..18,
        0usize..6,
        0usize..6,
        0usize..30,
        prop::array::uniform4(-50.0f64..600.0),
        "[a-z \n]{0,8}",
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

struct World {
    ids: SeqIds,
    ed: Editor,
}

impl World {
    fn frames(&self) -> Vec<FrameId> {
        self.ed.doc().frames.keys().copied().collect()
    }

    fn text_frames(&self) -> Vec<FrameId> {
        self.ed
            .doc()
            .frames
            .iter()
            .filter(|(_, f)| matches!(f.content, FrameContent::Text(_)))
            .map(|(id, _)| *id)
            .collect()
    }

    fn stories(&self) -> Vec<StoryId> {
        self.ed.doc().stories.keys().copied().collect()
    }

    fn pick<T: Copy>(v: &[T], i: usize) -> Option<T> {
        if v.is_empty() {
            None
        } else {
            Some(v[i % v.len()])
        }
    }

    /// Build a command from an op against the current state. Some pick ids
    /// that do not exist, or ranges out of bounds, so failures are common.
    fn command(&mut self, op: &Op) -> Command {
        let frames = self.frames();
        let tframes = self.text_frames();
        let stories = self.stories();
        let pages: Vec<PageId> = self.ed.doc().pages.iter().map(|p| p.id).collect();
        let bogus_frame = FrameId::new(&mut self.ids);
        let bogus_story = StoryId::new(&mut self.ids);
        let frame = World::pick(&frames, op.a).unwrap_or(bogus_frame);
        let story = World::pick(&stories, op.a).unwrap_or(bogus_story);
        let [x, y, w, h] = op.f;
        match op.kind {
            0 | 1 => Command::InsertText {
                story,
                at: op.c,
                text: op.text.clone(),
                format: None,
            },
            2 => Command::DeleteText {
                story,
                start: op.b,
                end: op.c,
            },
            3 => Command::SetFrameRect {
                frame,
                rect: Rect::new(x, y, w, h),
            },
            4 => Command::SetRotation { frame, degrees: x },
            5 => Command::ApplyCharProps {
                story,
                start: op.b,
                end: op.c,
                props: CharProps {
                    bold: Some(op.flag),
                    font_size: Some(w.abs() / 10.0 + 4.0),
                    ..Default::default()
                },
            },
            6 => Command::LinkFrames {
                from: World::pick(&tframes, op.a).unwrap_or(bogus_frame),
                to: World::pick(&tframes, op.b).unwrap_or(bogus_frame),
            },
            7 => Command::UnlinkFrame {
                frame: World::pick(&tframes, op.a).unwrap_or(bogus_frame),
                new_story: StoryId::new(&mut self.ids),
            },
            8 => Command::SetWrap {
                frame,
                wrap: TextWrap {
                    mode: match op.c % 3 {
                        0 => WrapMode::None,
                        1 => WrapMode::Square,
                        _ => WrapMode::TopBottom,
                    },
                    margin: w,
                },
            },
            9 => {
                let page = PageId::new(&mut self.ids);
                Command::AddPage {
                    page: Page::new(page),
                    index: if op.flag { Some(op.c) } else { None },
                }
            }
            10 => Command::DeletePage {
                page: World::pick(&pages, op.a).unwrap_or(PageId::new(&mut self.ids)),
            },
            11 | 12 => {
                let fid = FrameId::new(&mut self.ids);
                let rect = Rect::new(x, y, w, h);
                let (content, story) = if op.flag {
                    let sid = StoryId::new(&mut self.ids);
                    let mut s = Story::new(sid, fid);
                    s.insert_text(0, &op.text, None);
                    (FrameContent::Text(TextFrame::new(sid)), Some(s))
                } else {
                    (
                        FrameContent::Shape(ShapeFrame {
                            shape: Shape::Ellipse,
                        }),
                        None,
                    )
                };
                Command::AddFrame {
                    parent: Parent::Page(
                        World::pick(&pages, op.b).unwrap_or(PageId::new(&mut self.ids)),
                    ),
                    index: None,
                    frame: Frame::new(fid, rect, content),
                    story,
                }
            }
            13 => Command::DeleteFrame { frame },
            14 => Command::SetLocked {
                frame,
                locked: op.flag,
            },
            15 => Command::ReplaceText {
                story,
                start: op.b,
                end: op.c,
                text: op.text.clone(),
            },
            16 => Command::SetFrameName {
                frame,
                name: Some(op.text.clone()),
            },
            _ => Command::SetBleed { bleed: x },
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn undo_all_is_start_redo_all_is_end(ops in prop::collection::vec(op(), 1..40)) {
        let mut ids = SeqIds::new(7);
        let doc = Document::new(&mut ids, PageSetup::default());
        let mut w = World { ids, ed: Editor::new(doc) };
        let start = w.ed.doc().clone();
        for o in &ops {
            let cmd = w.command(o);
            let before = w.ed.doc().clone();
            let coalesce = o.kind == 0 && o.flag;
            let r = if coalesce {
                w.ed.execute_coalesced(cmd, "typing")
            } else {
                w.ed.execute(cmd)
            };
            match r {
                Ok(change) => {
                    // Sharing: whatever the change did not touch is the very
                    // same allocation as before (no copy, no churn).
                    let after = w.ed.doc();
                    if change.touched != Touched::default() {
                        for (id, f) in &before.frames {
                            if let Some(g) = after.frames.get(id)
                                && !change.touched.frames.contains(id)
                            {
                                prop_assert!(Arc::ptr_eq(f, g), "frame {id:?} copied but untouched");
                            }
                        }
                        for (id, s) in &before.stories {
                            if let Some(t) = after.stories.get(id)
                                && !change.touched.stories.contains(id)
                            {
                                prop_assert!(Arc::ptr_eq(s, t), "story {id:?} copied but untouched");
                            }
                        }
                    }
                }
                Err(_) => {
                    prop_assert_eq!(w.ed.doc(), &before, "failed command left doc changed");
                }
            }
        }
        let end = w.ed.doc().clone();
        let mut n = 0;
        while w.ed.undo().is_some() {
            n += 1;
            prop_assert!(n < 1000);
        }
        prop_assert_eq!(w.ed.doc(), &start);
        while w.ed.redo().is_some() {}
        prop_assert_eq!(w.ed.doc(), &end);
    }
}

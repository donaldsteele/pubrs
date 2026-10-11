//! Table frames: commands, undo, merge/split, integrity and the property
//! test that random structural edits undo and redo exactly.
#![allow(clippy::unwrap_used)]

use proptest::prelude::*;
use pubrs_core::table::{CellStyle, new_table};
use pubrs_core::validate::check_integrity;
use pubrs_core::*;

struct W {
    ids: SeqIds,
    ed: Editor,
}

impl W {
    fn new() -> Self {
        let mut ids = SeqIds::new(5);
        let doc = Document::new(&mut ids, PageSetup::default());
        Self {
            ids,
            ed: Editor::new(doc),
        }
    }

    fn add(&mut self, rows: usize, cols: usize) -> FrameId {
        let (frame, stories) = new_table(
            &mut self.ids,
            Rect::new(40.0, 40.0, 300.0, 60.0 * rows as f64),
            rows,
            cols,
            &CellStyle::default(),
        )
        .unwrap();
        let id = frame.id;
        let page = self.ed.doc().pages[0].id;
        self.ed
            .execute(Command::AddTable {
                parent: Parent::Page(page),
                index: None,
                frame,
                stories,
            })
            .unwrap();
        id
    }

    fn table(&self, f: FrameId) -> &TableFrame {
        self.ed.doc().frame(f).unwrap().as_table().unwrap()
    }

    fn ids_n(&mut self, n: usize) -> Vec<StoryId> {
        (0..n).map(|_| StoryId::new(&mut self.ids)).collect()
    }

    fn ok(&self) {
        let errs = check_integrity(self.ed.doc());
        assert!(errs.is_empty(), "{errs:?}");
    }

    fn set_text(&mut self, f: FrameId, r: u32, c: u32, t: &str) {
        let s = self.table(f).cell_at(r, c).unwrap().story;
        self.ed
            .execute(Command::SetStory {
                story: s,
                paragraphs: vec![Paragraph::plain(t)],
            })
            .unwrap();
    }

    fn text(&self, f: FrameId, r: u32, c: u32) -> String {
        let s = self.table(f).cell_at(r, c).unwrap().story;
        self.ed.doc().story(s).unwrap().plain_text()
    }
}

#[test]
fn add_table_creates_one_story_per_cell_and_undoes() {
    let mut w = W::new();
    let before = w.ed.doc().clone();
    let f = w.add(3, 4);
    w.ok();
    assert_eq!(w.table(f).cells.len(), 12);
    assert_eq!(w.ed.doc().stories.len(), 12);
    let (tf, cell) = w.ed.doc().cell_of_story(w.table(f).cells[5].story).unwrap();
    assert_eq!((tf, cell.row, cell.col), (f, 1, 1));
    assert!(w.ed.doc().story_of(f).is_none(), "story_of stays text-only");
    w.ed.undo().unwrap();
    assert_eq!(w.ed.doc(), &before);
    w.ed.redo().unwrap();
    assert_eq!(w.table(f).cells.len(), 12);
}

#[test]
fn add_frame_refuses_a_table_and_add_table_checks_stories() {
    let mut w = W::new();
    let (frame, stories) = new_table(
        &mut w.ids,
        Rect::new(0.0, 0.0, 100.0, 40.0),
        2,
        2,
        &CellStyle::default(),
    )
    .unwrap();
    let page = w.ed.doc().pages[0].id;
    let parent = Parent::Page(page);
    assert!(
        w.ed.execute(Command::AddFrame {
            parent,
            index: None,
            frame: frame.clone(),
            story: None
        })
        .is_err()
    );
    assert!(
        w.ed.execute(Command::AddTable {
            parent,
            index: None,
            frame: frame.clone(),
            stories: stories[..3].to_vec()
        })
        .is_err()
    );
    let mut wrong = stories.clone();
    wrong[0].frames = vec![FrameId::new(&mut w.ids)];
    assert!(
        w.ed.execute(Command::AddTable {
            parent,
            index: None,
            frame,
            stories: wrong
        })
        .is_err()
    );
    assert!(w.ed.doc().frames.is_empty());
}

#[test]
fn insert_and_delete_rows_and_columns_with_undo() {
    let mut w = W::new();
    let f = w.add(2, 2);
    w.set_text(f, 1, 1, "keep");
    let start = w.ed.doc().clone();
    let st = w.ids_n(2);
    w.ed.execute(Command::TableInsertRows {
        frame: f,
        at: 1,
        count: 1,
        stories: st,
    })
    .unwrap();
    assert_eq!(w.table(f).rows.len(), 3);
    assert_eq!(w.text(f, 2, 1), "keep");
    w.ok();
    let st = w.ids_n(3);
    w.ed.execute(Command::TableInsertCols {
        frame: f,
        at: 2,
        count: 1,
        stories: st,
    })
    .unwrap();
    assert_eq!(w.table(f).cols.len(), 3);
    let width = w.table(f).width();
    assert!((w.ed.doc().frame(f).unwrap().rect.width - width).abs() < 1e-9);
    w.ok();
    // Wrong number of ids is refused and changes nothing.
    let rev = w.ed.revision();
    assert!(
        w.ed.execute(Command::TableInsertRows {
            frame: f,
            at: 0,
            count: 1,
            stories: vec![]
        })
        .is_err()
    );
    assert_eq!(w.ed.revision(), rev);
    let stories_before = w.ed.doc().stories.len();
    w.ed.execute(Command::TableDeleteRows {
        frame: f,
        at: 0,
        count: 2,
    })
    .unwrap();
    assert_eq!(w.ed.doc().stories.len(), stories_before - 6);
    w.ok();
    assert!(
        w.ed.execute(Command::TableDeleteRows {
            frame: f,
            at: 0,
            count: 1
        })
        .is_err(),
        "the last row stays"
    );
    // Undo the deletion and the inserts: back to the plain 2x2 with text.
    for _ in 0..3 {
        w.ed.undo().unwrap();
    }
    assert_eq!(w.ed.doc(), &start);
}

#[test]
fn merge_appends_text_and_split_restores_cells() {
    let mut w = W::new();
    let f = w.add(2, 3);
    w.set_text(f, 0, 0, "A");
    w.set_text(f, 0, 1, "B");
    w.set_text(f, 1, 0, "C");
    let before = w.ed.doc().clone();
    w.ed.execute(Command::TableMergeCells {
        frame: f,
        row: 0,
        col: 0,
        row_span: 2,
        col_span: 2,
    })
    .unwrap();
    w.ok();
    assert_eq!(w.table(f).cells.len(), 3);
    assert_eq!(w.text(f, 1, 1), "A\nB\nC");
    assert_eq!(w.ed.doc().stories.len(), 3);
    // Partial overlap and single positions are refused.
    for (r, c, rs, cs) in [(1, 1, 1, 2), (0, 0, 1, 1), (0, 0, 9, 1)] {
        assert!(
            w.ed.execute(Command::TableMergeCells {
                frame: f,
                row: r,
                col: c,
                row_span: rs,
                col_span: cs
            })
            .is_err()
        );
    }
    let st = w.ids_n(3);
    w.ed.execute(Command::TableSplitCell {
        frame: f,
        row: 1,
        col: 1,
        stories: st,
    })
    .unwrap();
    w.ok();
    assert_eq!(w.table(f).cells.len(), 6);
    assert_eq!(w.text(f, 0, 0), "A\nB\nC", "the first cell keeps the text");
    w.ed.undo().unwrap();
    w.ed.undo().unwrap();
    assert_eq!(w.ed.doc(), &before);
}

#[test]
fn style_and_size_commands_validate() {
    let mut w = W::new();
    let f = w.add(2, 2);
    let style = CellStyle {
        fill: Some(Color::rgb(200, 220, 255).into()),
        borders: [
            Some(Stroke {
                color: Color::BLACK.into(),
                width: 1.0,
                dash: Dash::Solid,
            }),
            None,
            None,
            None,
        ],
        insets: Margins::uniform(2.0),
        valign: VerticalAlign::Middle,
    };
    w.ed.execute(Command::TableSetCellStyle {
        frame: f,
        row: 1,
        col: 0,
        style: style.clone(),
    })
    .unwrap();
    assert_eq!(w.table(f).cell_at(1, 0).unwrap().style(), style);
    let mut bad = style;
    bad.insets.top = -1.0;
    assert!(
        w.ed.execute(Command::TableSetCellStyle {
            frame: f,
            row: 0,
            col: 0,
            style: bad
        })
        .is_err()
    );
    w.ed.execute(Command::TableSetColWidth {
        frame: f,
        col: 0,
        width: 80.0,
    })
    .unwrap();
    assert_eq!(w.table(f).cols[0], 80.0);
    for w0 in [0.0, -3.0, f64::NAN] {
        assert!(
            w.ed.execute(Command::TableSetColWidth {
                frame: f,
                col: 0,
                width: w0
            })
            .is_err()
        );
    }
    assert!(
        w.ed.execute(Command::TableSetColWidth {
            frame: f,
            col: 5,
            width: 10.0
        })
        .is_err()
    );
    w.ed.execute(Command::TableSetRowHeight {
        frame: f,
        row: 1,
        min_height: 50.0,
    })
    .unwrap();
    assert_eq!(w.table(f).rows[1].min_height, 50.0);
    assert!(
        w.ed.execute(Command::TableSetRowHeight {
            frame: f,
            row: 1,
            min_height: -1.0
        })
        .is_err()
    );
}

#[test]
fn resizing_the_frame_scales_the_columns() {
    let mut w = W::new();
    let f = w.add(2, 2);
    w.ed.execute(Command::SetFrameRect {
        frame: f,
        rect: Rect::new(40.0, 40.0, 600.0, 100.0),
    })
    .unwrap();
    assert!((w.table(f).width() - 600.0).abs() < 1e-9);
}

#[test]
fn deleting_the_table_deletes_its_cell_stories() {
    let mut w = W::new();
    let before = w.ed.doc().clone();
    let f = w.add(2, 2);
    w.ed.execute(Command::DeleteFrame { frame: f }).unwrap();
    assert!(w.ed.doc().stories.is_empty());
    w.ok();
    w.ed.undo().unwrap();
    assert_eq!(w.ed.doc().stories.len(), 4);
    w.ok();
    w.ed.undo().unwrap();
    assert_eq!(w.ed.doc(), &before);
}

#[test]
fn integrity_catches_a_cell_story_that_is_not_the_tables() {
    let mut w = W::new();
    let f = w.add(1, 2);
    w.ok();
    let mut doc = w.ed.doc().clone();
    let s = doc.frame(f).unwrap().as_table().unwrap().cells[0].story;
    doc.story_mut(s).unwrap().frames.clear();
    assert!(!check_integrity(&doc).is_empty());
    let mut doc = w.ed.doc().clone();
    doc.stories.remove(&s);
    assert!(!check_integrity(&doc).is_empty());
}

// ---- property: random structural edits keep undo/redo exact ----

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn random_table_edits_undo_and_redo_exactly(
        ops in prop::collection::vec((0u8..9, 0usize..6, 0usize..6, 1usize..4, 1usize..4, "[a-z]{0,4}"), 1..40)
    ) {
        let mut w = W::new();
        let start = w.ed.doc().clone();
        let f = w.add(3, 3);
        for (kind, a, b, c, d, text) in &ops {
            let t = w.table(f).clone();
            let cmd = match kind {
                0 => {
                    let n = t.insert_rows_needed(*a, *c).unwrap_or(0);
                    Command::TableInsertRows { frame: f, at: *a as u32, count: *c as u32, stories: w.ids_n(n) }
                }
                1 => {
                    let n = t.insert_cols_needed(*a, *c).unwrap_or(0);
                    Command::TableInsertCols { frame: f, at: *a as u32, count: *c as u32, stories: w.ids_n(n) }
                }
                2 => Command::TableDeleteRows { frame: f, at: *a as u32, count: *c as u32 },
                3 => Command::TableDeleteCols { frame: f, at: *a as u32, count: *c as u32 },
                4 => Command::TableMergeCells { frame: f, row: *a as u32, col: *b as u32, row_span: *c as u32, col_span: *d as u32 },
                5 => {
                    let n = t.split_needed(*a as u32, *b as u32).unwrap_or(0);
                    Command::TableSplitCell { frame: f, row: *a as u32, col: *b as u32, stories: w.ids_n(n) }
                }
                6 => match t.cell_at(*a as u32, *b as u32) {
                    Some(cell) => Command::SetStory { story: cell.story, paragraphs: vec![Paragraph::plain(text)] },
                    None => Command::TableSetRowHeight { frame: f, row: 99, min_height: 1.0 },
                },
                7 => Command::TableSetColWidth { frame: f, col: *a as u32, width: 20.0 + *c as f64 * 10.0 },
                _ => Command::TableSetRowHeight { frame: f, row: *a as u32, min_height: *c as f64 * 7.0 },
            };
            let before = w.ed.doc().clone();
            if w.ed.execute(cmd).is_err() {
                prop_assert_eq!(w.ed.doc(), &before);
            }
            let errs = check_integrity(w.ed.doc());
            prop_assert!(errs.is_empty(), "{errs:?}");
        }
        let end = w.ed.doc().clone();
        while w.ed.undo().is_some() {}
        prop_assert_eq!(w.ed.doc(), &start);
        while w.ed.redo().is_some() {}
        prop_assert_eq!(w.ed.doc(), &end);
    }
}

#[test]
fn validate_has_nothing_to_say_about_a_plain_table() {
    let mut w = W::new();
    let f = w.add(2, 2);
    w.set_text(f, 0, 0, "Cell");
    let warnings = pubrs_core::validate::validate(w.ed.doc());
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn absurd_inserts_are_refused_before_anything_is_allocated() {
    let mut w = W::new();
    let f = w.add(3, 3);
    let t = w.table(f).clone();
    let started = std::time::Instant::now();
    for (at, count) in [(0, u32::MAX as usize), (1, 0), (9, 1), (4, 1), (0, 201)] {
        assert!(
            t.insert_rows_needed(at, count).is_err(),
            "rows {at} {count}"
        );
        assert!(
            t.insert_cols_needed(at, count).is_err(),
            "cols {at} {count}"
        );
        for rows in [true, false] {
            let cmd = if rows {
                Command::TableInsertRows {
                    frame: f,
                    at: at as u32,
                    count: count as u32,
                    stories: vec![],
                }
            } else {
                Command::TableInsertCols {
                    frame: f,
                    at: at as u32,
                    count: count as u32,
                    stories: vec![],
                }
            };
            assert!(w.ed.execute(cmd).is_err());
        }
    }
    assert!(t.insert_rows_needed(3, 1).is_ok(), "at the end is fine");
    // Merge spans whose product overflows are an error, not a panic.
    for (rs, cs) in [(u32::MAX, u32::MAX), (65_536, 65_536), (0, 5)] {
        assert!(
            w.ed.execute(Command::TableMergeCells {
                frame: f,
                row: 0,
                col: 0,
                row_span: rs,
                col_span: cs
            })
            .is_err()
        );
    }
    assert!(started.elapsed().as_secs() < 1);
}

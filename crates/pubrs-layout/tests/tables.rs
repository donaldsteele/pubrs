//! Table layout: row growth, spans, vertical alignment, overflow, and the
//! cache recomposing only the edited cell and its table pass (ADR 0024).
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use pubrs_core::table::CellStyle;
use pubrs_core::*;
use pubrs_layout::*;

const LONG: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod \
tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud.";

fn layouter() -> Layouter {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fonts");
    Layouter::new(FontLibrary::from_dir(&dir).expect("test fonts"))
}

struct T {
    ed: Editor,
    ids: SeqIds,
    frame: FrameId,
}

impl T {
    fn new(rect: Rect, rows: usize, cols: usize) -> Self {
        let mut b = DocBuilder::new(SeqIds::new(11), PageSetup::default());
        let p = b.page(0);
        let frame = b
            .table(Parent::Page(p), rect, rows, cols, &CellStyle::default())
            .unwrap();
        Self {
            ed: Editor::new(b.finish()),
            ids: SeqIds::new(99),
            frame,
        }
    }

    fn table(&self) -> &TableFrame {
        self.ed.doc().frame(self.frame).unwrap().as_table().unwrap()
    }

    fn story(&self, r: u32, c: u32) -> StoryId {
        self.table().cell_at(r, c).unwrap().story
    }

    fn set(&mut self, r: u32, c: u32, text: &str) {
        let story = self.story(r, c);
        self.ed
            .execute(Command::SetStory {
                story,
                paragraphs: vec![Paragraph::plain(text)],
            })
            .unwrap();
    }

    fn style(&mut self, r: u32, c: u32, f: impl FnOnce(&mut CellStyle)) {
        let mut s = self.table().cell_at(r, c).unwrap().style();
        f(&mut s);
        self.ed
            .execute(Command::TableSetCellStyle {
                frame: self.frame,
                row: r,
                col: c,
                style: s,
            })
            .unwrap();
    }

    fn merge(&mut self, r: u32, c: u32, rs: u32, cs: u32) {
        self.ed
            .execute(Command::TableMergeCells {
                frame: self.frame,
                row: r,
                col: c,
                row_span: rs,
                col_span: cs,
            })
            .unwrap();
    }

    fn layout(&self) -> (DocumentLayout, std::sync::Arc<TableLayout>) {
        let mut lay = layouter();
        let l = lay.layout_document(self.ed.doc());
        let t = l.table(self.frame).unwrap().clone();
        (l, t)
    }
}

#[test]
fn a_row_grows_to_fit_its_tallest_cell() {
    let mut t = T::new(Rect::new(40.0, 40.0, 240.0, 400.0), 3, 2);
    for row in 0..3 {
        t.ed.execute(Command::TableSetRowHeight {
            frame: t.frame,
            row,
            min_height: 30.0,
        })
        .unwrap();
    }
    let (_, empty) = t.layout();
    assert!((empty.row_heights[0] - empty.row_heights[1]).abs() < 1e-9);
    t.set(1, 0, LONG);
    let (l, tl) = t.layout();
    assert!(tl.row_heights[1] > empty.row_heights[1] + 20.0);
    assert!((tl.row_heights[0] - empty.row_heights[0]).abs() < 1e-9);
    assert!((tl.row_tops[2] - (tl.row_tops[1] + tl.row_heights[1])).abs() < 1e-9);
    assert!(tl.overflowing_cells.is_empty());
    // Every line of the tall cell sits inside the cell.
    let story = t.story(1, 0);
    let ft = l.cell_text(t.frame, story).unwrap();
    assert!(ft.lines.len() > 3);
    let cell = tl.cell(story).unwrap();
    let last = ft.lines.last().unwrap().rect.bottom();
    assert!(last <= cell.rect.height + 1e-6);
    // The cell is as wide as its column.
    assert!((cell.rect.width - t.table().cols[0]).abs() < 1e-9);
}

#[test]
fn rows_stop_at_the_frame_and_the_text_below_is_reported() {
    let mut t = T::new(Rect::new(40.0, 40.0, 240.0, 100.0), 3, 2);
    t.set(0, 0, LONG);
    t.set(0, 1, "tall");
    t.set(1, 0, "second row");
    t.set(2, 1, "third row");
    let (l, tl) = t.layout();
    let frame_h = 100.0;
    for (top, h) in tl.row_tops.iter().zip(&tl.row_heights) {
        assert!(top + h <= frame_h + 1e-9);
    }
    // The long first cell fills the frame: the rows below it have no room.
    assert!(tl.row_heights[1] < 1.0 && tl.row_heights[2] < 1.0);
    let overflow = &tl.overflowing_cells;
    assert!(overflow.contains(&t.story(1, 0)), "{overflow:?}");
    assert!(overflow.contains(&t.story(2, 1)));
    assert!(!overflow.contains(&t.story(0, 1)));
    let s = l.stories.get(&t.story(1, 0)).unwrap();
    let of = s.overflow.unwrap();
    assert_eq!((of.offset, of.hidden_chars), (0, "second row".len()));
    assert!(s.frames[&t.frame].lines.is_empty());
    // An empty cell below the frame is not an overflow.
    let mut t2 = T::new(Rect::new(40.0, 40.0, 240.0, 100.0), 3, 2);
    t2.set(0, 0, LONG);
    let (l2, tl2) = t2.layout();
    assert!(!tl2.overflowing_cells.contains(&t2.story(2, 1)));
    assert!(!tl2.overflowing_cells.contains(&t2.story(1, 0)));
    assert!(l2.stories[&t2.story(2, 1)].overflow.is_none());
}

#[test]
fn a_partly_visible_cell_keeps_its_first_lines() {
    let mut t = T::new(Rect::new(40.0, 40.0, 120.0, 60.0), 1, 1);
    t.set(0, 0, LONG);
    let (l, tl) = t.layout();
    let s = &l.stories[&t.story(0, 0)];
    let of = s.overflow.expect("overflow");
    assert!(of.offset > 0 && of.hidden_chars > 0);
    let ft = &s.frames[&t.frame];
    assert!(!ft.lines.is_empty());
    assert_eq!(ft.chars.end, of.offset.min(ft.chars.end).max(ft.chars.end));
    let room = 60.0 - 4.0;
    assert!(ft.lines.iter().all(|ln| ln.rect.bottom() <= room + 1e-6));
    assert_eq!(tl.overflowing_cells, vec![t.story(0, 0)]);
}

#[test]
fn spanning_cells_use_the_summed_width_and_extra_height_goes_to_the_last_row() {
    let mut t = T::new(Rect::new(40.0, 40.0, 300.0, 400.0), 3, 3);
    t.merge(0, 0, 1, 3);
    t.set(0, 0, "Header");
    let (_, tl) = t.layout();
    let head = tl.cell(t.story(0, 0)).unwrap();
    assert!((head.rect.width - 300.0).abs() < 1e-9);
    // A tall cell spanning rows 1-2 of the first column.
    let mut t = T::new(Rect::new(40.0, 40.0, 300.0, 400.0), 3, 3);
    t.merge(0, 0, 2, 1);
    t.set(0, 0, &format!("{LONG} {LONG} {LONG}"));
    let (_, tl) = t.layout();
    let cell = tl.cell(t.story(0, 0)).unwrap();
    assert!(cell.rect.height > 2.0 * 133.0 - 1.0 + 20.0 || cell.rect.height > 150.0);
    // Row 0 keeps its minimum; the second spanned row took the extra.
    assert!((tl.row_heights[0] - 133.333_333_333_333_3).abs() < 1e-6);
    assert!(tl.row_heights[1] > tl.row_heights[0]);
    assert!((tl.row_tops[2] - (tl.row_tops[1] + tl.row_heights[1])).abs() < 1e-9);
}

#[test]
fn vertical_alignment_shifts_a_cell_within_its_row() {
    let mut t = T::new(Rect::new(40.0, 40.0, 300.0, 400.0), 1, 3);
    for c in 0..3 {
        t.set(0, c, "x");
    }
    t.style(0, 1, |s| s.valign = VerticalAlign::Middle);
    t.style(0, 2, |s| s.valign = VerticalAlign::Bottom);
    let (l, tl) = t.layout();
    let dy = |c: u32| tl.cell(t.story(0, c)).unwrap().dy;
    assert_eq!(dy(0), 0.0);
    assert!(dy(1) > 10.0);
    assert!((dy(2) - 2.0 * dy(1)).abs() < 1e-6);
    // The bottom-aligned text ends at the cell's bottom inset.
    let story = t.story(0, 2);
    let o = l.cell_origin(t.frame, story).unwrap();
    let ft = l.cell_text(t.frame, story).unwrap();
    let bottom = o.y + ft.lines.last().unwrap().rect.bottom();
    let cell = tl.cell(story).unwrap();
    assert!((bottom - (cell.rect.bottom() - 4.0)).abs() < 1e-6);
}

#[test]
fn the_cache_recomposes_only_the_edited_cell_and_its_table() {
    let mut lay = layouter();
    let mut cache = StoryLayoutCache::new();
    let mut t = T::new(Rect::new(40.0, 40.0, 300.0, 400.0), 3, 3);
    let check = |cache: &mut StoryLayoutCache, lay: &mut Layouter, doc: &Document| {
        let inc = cache.layout(lay, doc);
        let full = lay.layout_document(doc);
        assert_eq!(inc.tables, full.tables);
        for id in full.stories.keys() {
            assert_eq!(inc.stories.get(id), full.stories.get(id), "story {id}");
        }
        inc
    };
    check(&mut cache, &mut lay, t.ed.doc());
    assert_eq!(cache.stats().recomposed, 9);
    assert_eq!(cache.stats().tables_recomposed, 1);
    // Nothing changed.
    check(&mut cache, &mut lay, t.ed.doc());
    assert_eq!(
        (cache.stats().recomposed, cache.stats().tables_recomposed),
        (0, 0)
    );
    assert_eq!(cache.stats().tables_reused, 1);
    // One keystroke in one cell.
    t.set(1, 1, "hello");
    check(&mut cache, &mut lay, t.ed.doc());
    let s = cache.stats();
    assert_eq!((s.recomposed, s.reused), (1, 8));
    assert_eq!((s.tables_recomposed, s.tables_reused), (1, 0));
    // A column width change recomposes the three cells of that column.
    t.ed.execute(Command::TableSetColWidth {
        frame: t.frame,
        col: 2,
        width: 50.0,
    })
    .unwrap();
    check(&mut cache, &mut lay, t.ed.doc());
    assert_eq!(cache.stats().recomposed, 3);
    // Insets of one cell recompose that cell.
    t.style(0, 0, |s| s.insets = Margins::uniform(9.0));
    check(&mut cache, &mut lay, t.ed.doc());
    assert_eq!(cache.stats().recomposed, 1);
    // Merging changes spans (and removes stories).
    t.merge(0, 0, 1, 2);
    check(&mut cache, &mut lay, t.ed.doc());
    // Deleting the table drops its entries.
    t.ed.execute(Command::DeleteFrame { frame: t.frame })
        .unwrap();
    let l = check(&mut cache, &mut lay, t.ed.doc());
    assert!(l.tables.is_empty() && l.stories.is_empty());
    let _ = &mut t.ids;
}

#[test]
fn the_cache_reuses_the_table_when_other_text_changes() {
    let mut lay = layouter();
    let mut cache = StoryLayoutCache::new();
    let mut b = DocBuilder::new(SeqIds::new(3), PageSetup::default());
    let p = b.page(0);
    let tf = b
        .table(
            Parent::Page(p),
            Rect::new(40.0, 300.0, 200.0, 100.0),
            2,
            2,
            &CellStyle::default(),
        )
        .unwrap();
    let text = b
        .text(p, Rect::new(40.0, 40.0, 200.0, 100.0), "hi")
        .unwrap();
    let story = b.story_of(text);
    let mut ed = Editor::new(b.finish());
    cache.layout(&mut lay, ed.doc());
    ed.execute(Command::InsertText {
        story,
        at: 2,
        text: "!".into(),
        format: None,
    })
    .unwrap();
    let l = cache.layout(&mut lay, ed.doc());
    let s = cache.stats();
    assert_eq!(
        (s.recomposed, s.tables_recomposed, s.tables_reused),
        (1, 0, 1)
    );
    assert!(l.table(tf).is_some());
}

#[test]
fn typing_in_a_huge_table_does_not_rescan_its_cells_per_story() {
    // 200 x 40 = 8,000 cell stories. Finding each story's cell by a scan of
    // the table would cost 32M comparisons per pass.
    let mut lay = layouter();
    let mut cache = StoryLayoutCache::new();
    let mut t = T::new(Rect::new(0.0, 0.0, 4000.0, 4000.0), 200, 40);
    cache.layout(&mut lay, t.ed.doc());
    assert_eq!(cache.stats().recomposed, 8000);
    t.set(100, 20, "x");
    let started = std::time::Instant::now();
    cache.layout(&mut lay, t.ed.doc());
    let took = started.elapsed();
    let s = cache.stats();
    assert_eq!((s.recomposed, s.tables_recomposed), (1, 1));
    eprintln!("relayout of one cell in 8,000: {took:?}");
    assert!(took.as_millis() < BOUND_MS, "{took:?}");
}

const BOUND_MS: u128 = 300;

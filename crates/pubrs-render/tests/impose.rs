//! Imposition: cell placement, fill order, booklet order. Deterministic.

use pubrs_core::id::SeqIds;
use pubrs_core::{Color, FrameId, PageSetup, Point, Rect, SheetFill, SheetLayout, Size};
use pubrs_render::impose::{
    ImposeError, booklet_plan, impose, impose_booklet, impose_sheet, is_imposed, sheet_plan,
};
use pubrs_render::scene::{Affine, Node, PageScene, Prim};

const IN: f64 = 72.0;

/// A label page whose single node carries `tag` as its rect width.
fn label(tag: f64, w: f64, h: f64) -> PageScene {
    let mut ids = SeqIds::new(tag as u64);
    PageScene {
        width: w,
        height: h,
        bleed: 0.0,
        nodes: vec![Node {
            frame: FrameId::new(&mut ids),
            transform: Affine::translate(3.0, 4.0),
            opacity: 1.0,
            shadow: None,
            prims: vec![Prim::Rect {
                rect: Rect::new(0.0, 0.0, tag, 1.0),
                color: Color::BLACK.into(),
            }],
            clip: None,
        }],
    }
}

fn tag(n: &Node) -> u32 {
    match &n.prims[0] {
        Prim::Rect { rect, .. } => rect.width as u32,
        _ => panic!("unexpected prim"),
    }
}

fn avery_5160(fill: SheetFill) -> SheetLayout {
    SheetLayout {
        preset: "avery-5160".into(),
        sheet_size: Size::new(8.5 * IN, 11.0 * IN),
        rows: 10,
        cols: 3,
        label_size: Size::new(2.625 * IN, IN),
        origin: Point::new(0.1875 * IN, 0.5 * IN),
        pitch: Size::new(2.75 * IN, IN),
        fill,
    }
}

#[test]
fn repeat_fills_every_cell_of_a_sheet_per_page() {
    let pages = [label(1.0, 189.0, 72.0), label(2.0, 189.0, 72.0)];
    let sheets = impose_sheet(&pages, &avery_5160(SheetFill::Repeat)).unwrap();
    assert_eq!(sheets.len(), 2);
    let s = &sheets[0];
    assert_eq!((s.width, s.height, s.bleed), (612.0, 792.0, 0.0));
    assert_eq!(s.nodes.len(), 30);
    assert!(s.nodes.iter().all(|n| tag(n) == 1));
    assert!(sheets[1].nodes.iter().all(|n| tag(n) == 2));
}

#[test]
fn cell_placement_for_5160() {
    let pages = [label(1.0, 189.0, 72.0)];
    let s = &impose_sheet(&pages, &avery_5160(SheetFill::Repeat)).unwrap()[0];
    // Cell (row 0, col 0): origin (13.5, 36). The node's own (3, 4) offset rides along.
    let (x, y) = s.nodes[0].transform.apply(0.0, 0.0);
    assert_eq!((x, y), (13.5 + 3.0, 36.0 + 4.0));
    assert_eq!(s.nodes[0].clip, Some(Rect::new(13.5, 36.0, 189.0, 72.0)));
    // Cell index 1 is column 1 of row 0 (pitch 2.75 in = 198 pt).
    let (x, y) = s.nodes[1].transform.apply(0.0, 0.0);
    assert_eq!((x, y), (13.5 + 198.0 + 3.0, 36.0 + 4.0));
    // Cell index 3 is row 1, column 0.
    assert_eq!(
        s.nodes[3].clip,
        Some(Rect::new(13.5, 36.0 + 72.0, 189.0, 72.0))
    );
    // The last cell: row 9, column 2.
    assert_eq!(
        s.nodes[29].clip,
        Some(Rect::new(13.5 + 396.0, 36.0 + 648.0, 189.0, 72.0))
    );
}

#[test]
fn sequence_fills_cells_in_page_order_and_spills_to_a_new_sheet() {
    let plan = sheet_plan(&avery_5160(SheetFill::Sequence), 32);
    assert_eq!(plan.len(), 2);
    assert_eq!(plan[0].len(), 30);
    assert_eq!(plan[0][0], Some(0));
    assert_eq!(plan[0][29], Some(29));
    assert_eq!(plan[1][0], Some(30));
    assert_eq!(plan[1][1], Some(31));
    assert_eq!(plan[1][2], None);

    let pages: Vec<_> = (1..=4).map(|i| label(f64::from(i), 189.0, 72.0)).collect();
    let sheets = impose_sheet(&pages, &avery_5160(SheetFill::Sequence)).unwrap();
    assert_eq!(sheets.len(), 1);
    let tags: Vec<_> = sheets[0].nodes.iter().map(tag).collect();
    assert_eq!(tags, [1, 2, 3, 4]);
}

#[test]
fn a_page_that_disagrees_with_the_label_is_refused_with_a_hint() {
    let pages = [label(1.0, 300.0, 200.0)];
    let err = impose_sheet(&pages, &avery_5160(SheetFill::Repeat)).unwrap_err();
    assert!(matches!(err, ImposeError::SizeMismatch { .. }));
    assert!(
        err.to_string()
            .contains("set the page size to 2.625 x 1.000 in first")
    );
}

fn order(plan: &[[[Option<usize>; 2]; 2]]) -> Vec<[Option<u32>; 2]> {
    plan.iter()
        .flatten()
        .map(|s| s.map(|p| p.map(|i| i as u32 + 1)))
        .collect()
}

#[test]
fn booklet_order_for_8_pages() {
    assert_eq!(
        order(&booklet_plan(8)),
        [
            [Some(8), Some(1)],
            [Some(2), Some(7)],
            [Some(6), Some(3)],
            [Some(4), Some(5)],
        ]
    );
}

#[test]
fn booklet_order_for_5_pages_pads_to_8_with_blanks() {
    assert_eq!(
        order(&booklet_plan(5)),
        [
            [None, Some(1)],
            [Some(2), None],
            [None, Some(3)],
            [Some(4), Some(5)],
        ]
    );
}

#[test]
fn booklet_sheets_are_twice_as_wide_and_place_pages_on_their_halves() {
    let pages: Vec<_> = (1..=5).map(|i| label(f64::from(i), 100.0, 150.0)).collect();
    let sheets = impose_booklet(&pages).unwrap();
    assert_eq!(sheets.len(), 4);
    assert!(sheets.iter().all(|s| s.width == 200.0 && s.height == 150.0));
    // Sheet 4 (back of the inner sheet): page 4 left, page 5 right.
    let last = &sheets[3];
    assert_eq!(last.nodes.iter().map(tag).collect::<Vec<_>>(), [4, 5]);
    assert_eq!(last.nodes[0].clip, Some(Rect::new(0.0, 0.0, 100.0, 150.0)));
    assert_eq!(
        last.nodes[1].clip,
        Some(Rect::new(100.0, 0.0, 100.0, 150.0))
    );
    assert_eq!(last.nodes[1].transform.apply(0.0, 0.0), (103.0, 4.0));
    // Sheet 1 front has a blank left half (page 8 does not exist).
    assert_eq!(sheets[0].nodes.iter().map(tag).collect::<Vec<_>>(), [1]);
}

#[test]
fn impose_dispatches_on_the_setup() {
    let pages = [label(1.0, 189.0, 72.0)];
    let mut setup = PageSetup::default();
    assert!(!is_imposed(&setup));
    assert_eq!(impose(&pages, &setup).unwrap().len(), 1);
    setup.sheet = Some(avery_5160(SheetFill::Repeat));
    assert!(is_imposed(&setup));
    assert_eq!(impose(&pages, &setup).unwrap()[0].width, 612.0);
    setup.sheet = None;
    setup.booklet = true;
    assert_eq!(impose(&pages, &setup).unwrap()[0].width, 378.0);
    assert!(impose(&[], &setup).is_err());
}

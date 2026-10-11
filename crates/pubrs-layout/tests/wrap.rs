//! Tight, through and both-sides wrap (ADR 0022).
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use pubrs_core::*;
use pubrs_layout::*;

const LOREM: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod \
tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud \
exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in \
reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint \
occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. \
Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium doloremque laudantium, \
totam rem aperiam, eaque ipsa quae ab illo inventore veritatis et quasi architecto beatae vitae.";

fn fonts() -> FontLibrary {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fonts");
    FontLibrary::from_dir(&dir).expect("test fonts")
}

fn builder() -> DocBuilder<SeqIds> {
    DocBuilder::new(SeqIds::new(7), PageSetup::default())
}

fn wrap(mode: WrapMode, margin: f64, both_sides: bool) -> TextWrap {
    TextWrap {
        mode,
        margin,
        both_sides,
    }
}

/// Contours by asset, standing in for the image cache.
#[derive(Default)]
struct Contours(HashMap<AssetId, Arc<Contour>>);

impl AlphaContours for Contours {
    fn contour(&self, asset: AssetId) -> Option<Arc<Contour>> {
        self.0.get(&asset).cloned()
    }
}

fn layout_with(doc: &Document, contours: Option<Contours>) -> DocumentLayout {
    let mut l = Layouter::new(fonts());
    if let Some(c) = contours {
        l.set_contours(Some(Arc::new(c)));
    }
    l.layout_document(doc)
}

fn layout(doc: &Document) -> DocumentLayout {
    layout_with(doc, None)
}

/// A text frame with an ellipse above it, wrapped as given.
fn ellipse_doc(w: TextWrap) -> (Document, FrameId) {
    let mut b = builder();
    let p = b.page(0);
    let f = b.text(p, Rect::new(0.0, 0.0, 300.0, 300.0), LOREM).unwrap();
    let e = b
        .shape(p, Rect::new(0.0, 0.0, 100.0, 100.0), Shape::Ellipse)
        .unwrap();
    b.wrap(e, w).unwrap();
    (b.finish(), f)
}

/// Right edge of the ellipse (0,0,100,100) over `y0..y1`.
fn ellipse_right(y0: f64, y1: f64) -> f64 {
    let dy = if y0 <= 50.0 && y1 >= 50.0 {
        0.0
    } else {
        (y0 - 50.0).abs().min((y1 - 50.0).abs())
    };
    if dy >= 50.0 {
        return 0.0;
    }
    50.0 + 50.0 * (1.0 - (dy / 50.0).powi(2)).sqrt()
}

#[test]
fn tight_ellipse_hugs_the_outline_where_square_keeps_the_box() {
    let (sq, f) = ellipse_doc(wrap(WrapMode::Square, 4.0, false));
    let (ti, _) = ellipse_doc(wrap(WrapMode::Tight, 4.0, false));
    let sq = layout(&sq);
    let ti = layout(&ti);
    let (sl, tl) = (&sq.frame(f).unwrap().lines, &ti.frame(f).unwrap().lines);
    // Square: every line beside the box starts right of it.
    for l in sl.iter().filter(|l| l.rect.y < 104.0) {
        assert!(l.rect.x >= 104.0 - 1e-6, "{}", l.rect.x);
    }
    // Tight: the first line sits where the ellipse is narrow, left of the box.
    let top = tl.iter().find(|l| l.rect.y < 10.0).unwrap();
    assert!(top.rect.x < 100.0, "{}", top.rect.x);
    // And no line ever overlaps the outline, margin included.
    for l in tl.iter().filter(|l| l.rect.y < 104.0) {
        let edge = ellipse_right(l.rect.y, l.rect.bottom());
        assert!(
            l.rect.x + 1e-6 >= edge + 4.0 || l.rect.x + 1e-6 >= 104.0,
            "line at y={} starts at {} but the ellipse reaches {}",
            l.rect.y,
            l.rect.x,
            edge
        );
    }
    // Tight is never farther in than square.
    assert!(tl.len() >= sl.len() - 2);
}

#[test]
fn margin_moves_the_text_away() {
    let x_at = |m: f64| {
        let (d, f) = ellipse_doc(wrap(WrapMode::Tight, m, false));
        let lay = layout(&d);
        lay.frame(f)
            .unwrap()
            .lines
            .iter()
            .find(|l| l.rect.y > 40.0 && l.rect.y < 60.0)
            .unwrap()
            .rect
            .x
    };
    let (a, b) = (x_at(0.0), x_at(12.0));
    assert!(b - a > 11.0 && b - a < 13.0, "{a} {b}");
    assert!(a >= 100.0 - 1e-6);
}

#[test]
fn tight_line_follows_the_diagonal() {
    let mut b = builder();
    let p = b.page(0);
    let f = b.text(p, Rect::new(0.0, 0.0, 300.0, 200.0), LOREM).unwrap();
    let l = b
        .shape(
            p,
            Rect::new(60.0, 0.0, 60.0, 100.0),
            Shape::Line {
                direction: LineDirection::DownRight,
            },
        )
        .unwrap();
    b.wrap(l, wrap(WrapMode::Tight, 2.0, false)).unwrap();
    let doc = b.finish();
    let lay = layout(&doc);
    let lines = &lay.frame(f).unwrap().lines;
    let beside: Vec<_> = lines.iter().filter(|l| l.rect.bottom() < 100.0).collect();
    assert!(beside.len() > 4);
    // The text is on the wider (right) side and clears the segment.
    for l in &beside {
        assert!(
            l.rect.x >= 60.0 + 0.6 * l.rect.bottom() - 1e-6,
            "y={} x={}",
            l.rect.y,
            l.rect.x
        );
    }
    // The first line is much closer than the box's right edge would allow.
    assert!(beside[0].rect.x < 100.0, "{}", beside[0].rect.x);
}

fn inside_rotated_rect(p: (f64, f64), centre: (f64, f64), half: (f64, f64), deg: f64) -> bool {
    let (s, c) = (-deg).to_radians().sin_cos();
    let (dx, dy) = (p.0 - centre.0, p.1 - centre.1);
    let (x, y) = (dx * c - dy * s, dx * s + dy * c);
    x.abs() < half.0 && y.abs() < half.1
}

#[test]
fn tight_rotated_rectangle_follows_the_diamond() {
    let build = |mode| {
        let mut b = builder();
        let p = b.page(0);
        let f = b.text(p, Rect::new(0.0, 0.0, 300.0, 200.0), LOREM).unwrap();
        let r = b
            .shape(
                p,
                Rect::new(0.0, 0.0, 100.0, 40.0),
                Shape::Rectangle { corner_radius: 0.0 },
            )
            .unwrap();
        b.wrap(r, wrap(mode, 0.0, false)).unwrap();
        let mut doc = b.finish();
        Arc::make_mut(doc.frames.get_mut(&r).unwrap()).rotation = 45.0;
        (doc, f)
    };
    let (sq, f) = build(WrapMode::Square);
    let (ti, _) = build(WrapMode::Tight);
    let (sq, ti) = (layout(&sq), layout(&ti));
    let sq_lines = &sq.frame(f).unwrap().lines;
    let ti_lines = &ti.frame(f).unwrap().lines;
    // The rotated box reaches x = 99.5 and down to y = 69.5.
    for l in sq_lines.iter().filter(|l| l.rect.y < 69.0) {
        assert!(l.rect.x >= 99.0, "square line at {}", l.rect.x);
    }
    // Toward the diamond's bottom corner the tight text comes back left.
    assert!(
        ti_lines
            .iter()
            .any(|l| l.rect.y > 50.0 && l.rect.y < 69.0 && l.rect.x < 92.0),
        "{:?}",
        ti_lines
            .iter()
            .map(|l| (l.rect.y, l.rect.x))
            .collect::<Vec<_>>()
    );
    // No line's left edge is inside the rotated rectangle.
    for l in ti_lines {
        for k in 0..=4 {
            let y = l.rect.y + l.rect.height * f64::from(k) / 4.0;
            assert!(
                !inside_rotated_rect((l.rect.x + 0.01, y), (50.0, 20.0), (50.0, 20.0), 45.0),
                "line at y={} x={} is inside",
                l.rect.y,
                l.rect.x
            );
        }
    }
}

/// An alpha buffer from a predicate.
fn alpha(w: usize, h: usize, f: impl Fn(usize, usize) -> bool) -> Vec<u8> {
    (0..w * h)
        .map(|i| if f(i % w, i / w) { 255 } else { 0 })
        .collect()
}

fn image_doc(mode: WrapMode, both_sides: bool, a: &[u8], px: u32) -> (Document, FrameId, Contours) {
    let mut b = builder();
    let p = b.page(0);
    let asset = add_asset(&mut b);
    let f = b.text(p, Rect::new(0.0, 0.0, 300.0, 200.0), LOREM).unwrap();
    let img = b
        .image(p, Rect::new(0.0, 0.0, 100.0, 100.0), Some(asset))
        .unwrap();
    b.wrap(img, wrap(mode, 0.0, both_sides)).unwrap();
    let mut c = Contours::default();
    c.0.insert(asset, Arc::new(trace_contour(px, px, a).unwrap()));
    (b.finish(), f, c)
}

#[test]
fn tight_wrap_follows_the_alpha_of_a_png() {
    let n = 100usize;
    let disc = alpha(n, n, |x, y| {
        let (dx, dy) = (x as f64 - 49.5, y as f64 - 49.5);
        dx * dx + dy * dy <= 49.0 * 49.0
    });
    let (doc, f, c) = image_doc(WrapMode::Tight, false, &disc, n as u32);
    let tight = layout_with(&doc, Some(c));
    // Without a contour the image wraps by its rectangle.
    let boxed = layout(&doc);
    let top_t = tight.frame(f).unwrap().lines[0].rect.x;
    let top_b = boxed.frame(f).unwrap().lines[0].rect.x;
    assert!(top_t < 90.0 && top_b >= 100.0, "{top_t} {top_b}");
    // The outline may cut at most about a pixel into the visible disc.
    for l in &tight.frame(f).unwrap().lines {
        for k in 0..=4 {
            let y = l.rect.y + l.rect.height * f64::from(k) / 4.0;
            let (dx, dy) = (l.rect.x + 0.01 - 50.0, y - 50.0);
            assert!(dx * dx + dy * dy >= 47.5 * 47.5, "inside at y={y}");
        }
    }
}

#[test]
fn image_without_a_contour_wraps_by_its_rectangle_in_every_mode() {
    let mut b = builder();
    let p = b.page(0);
    let asset = add_asset(&mut b);
    let f = b.text(p, Rect::new(0.0, 0.0, 300.0, 200.0), LOREM).unwrap();
    let img = b
        .image(p, Rect::new(0.0, 0.0, 100.0, 100.0), Some(asset))
        .unwrap();
    let mut doc = b.finish();
    let mut xs = Vec::new();
    for m in [WrapMode::Square, WrapMode::Tight] {
        Arc::make_mut(doc.frames.get_mut(&img).unwrap()).wrap = wrap(m, 6.0, false);
        xs.push(layout(&doc).frame(f).unwrap().lines[0].rect.x);
    }
    assert_eq!(xs[0], xs[1]);
}

/// An upside-down U: a bar on top and two legs.
fn arch(n: usize) -> Vec<u8> {
    alpha(n, n, |x, y| y < 20 || !(20..80).contains(&x))
}

#[test]
fn through_enters_the_notch_that_tight_blocks() {
    let n = 100usize;
    let a = arch(n);
    let (doc, f, c) = image_doc(WrapMode::Tight, false, &a, n as u32);
    let tight = layout_with(&doc, Some(c));
    let (doc, f2, c) = image_doc(WrapMode::Through, false, &a, n as u32);
    let through = layout_with(&doc, Some(c));
    assert_eq!(f, f2);
    let in_notch = |lay: &DocumentLayout| {
        lay.frame(f)
            .unwrap()
            .lines
            .iter()
            .filter(|l| l.rect.y > 25.0 && l.rect.bottom() < 95.0 && l.rect.right() <= 80.0)
            .count()
    };
    assert_eq!(in_notch(&tight), 0);
    assert!(in_notch(&through) >= 3, "{}", in_notch(&through));
    // The notch lines sit between the legs, and the right side is used too.
    for l in through
        .frame(f)
        .unwrap()
        .lines
        .iter()
        .filter(|l| l.rect.y > 25.0 && l.rect.bottom() < 95.0)
    {
        assert!(l.rect.x >= 20.0 - 1e-6 && l.rect.x < 80.0 || l.rect.x >= 100.0 - 1e-6);
    }
    assert!(
        through
            .frame(f)
            .unwrap()
            .lines
            .iter()
            .any(|l| l.rect.x >= 100.0)
    );
}

#[test]
fn concave_l_shape_tight_and_through_agree_on_a_plain_edge() {
    // An L has one covered span per line, so through = tight.
    let n = 100usize;
    let l = alpha(n, n, |x, y| x < 30 || y >= 70);
    let (doc, f, c) = image_doc(WrapMode::Tight, false, &l, n as u32);
    let a = layout_with(&doc, Some(c));
    let (doc, _, c) = image_doc(WrapMode::Through, false, &l, n as u32);
    let b = layout_with(&doc, Some(c));
    let xs = |lay: &DocumentLayout| {
        lay.frame(f)
            .unwrap()
            .lines
            .iter()
            .map(|l| (l.rect.y.to_bits(), l.rect.x.to_bits()))
            .collect::<Vec<_>>()
    };
    assert_eq!(xs(&a), xs(&b));
    // Text is beside the stem at the top, below the foot further down.
    assert!(a.frame(f).unwrap().lines[0].rect.x >= 30.0 - 1e-6);
}

/// A text frame with a square obstacle at x..x+50 (y 0..100).
fn slots_doc(x: f64, both: bool) -> (Document, FrameId) {
    let mut b = builder();
    let p = b.page(0);
    let f = b.text(p, Rect::new(0.0, 0.0, 300.0, 200.0), LOREM).unwrap();
    let r = b
        .shape(
            p,
            Rect::new(x, 0.0, 50.0, 100.0),
            Shape::Rectangle { corner_radius: 0.0 },
        )
        .unwrap();
    b.wrap(r, wrap(WrapMode::Square, 0.0, both)).unwrap();
    (b.finish(), f)
}

#[test]
fn both_sides_fills_every_slot_left_to_right() {
    let (doc, f) = slots_doc(100.0, true);
    let lay = layout(&doc);
    let ft = lay.frame(f).unwrap();
    let first = ft.lines[0].rect.y;
    let row: Vec<_> = ft.lines.iter().filter(|l| l.rect.y == first).collect();
    assert_eq!(
        row.len(),
        2,
        "{:?}",
        ft.lines.iter().map(|l| l.rect).collect::<Vec<_>>()
    );
    assert_eq!(row[0].rect.x, 4.0);
    assert!((row[0].rect.width - 96.0).abs() < 1e-6);
    assert!(row[1].rect.x >= 150.0 - 1e-6);
    // Story order, slot by slot: the right slot continues the left.
    assert_eq!(row[0].chars.end, row[1].chars.start);
    // Same top, so baselines agree for the same style.
    assert_eq!(row[0].baseline, row[1].baseline);
    // Carets work in both slots and the lines are contiguous overall.
    for pair in ft.lines.windows(2) {
        assert_eq!(pair[0].chars.end, pair[1].chars.start);
    }
    for l in &row {
        assert!(!l.carets.is_empty());
        assert!(l.caret_x(l.chars.start) >= l.rect.x - 1e-6);
    }
    // Without the flag only the wider side is used (today's behaviour).
    let (doc, f) = slots_doc(100.0, false);
    let lay = layout(&doc);
    let ft = lay.frame(f).unwrap();
    let first = ft.lines[0].rect.y;
    assert_eq!(ft.lines.iter().filter(|l| l.rect.y == first).count(), 1);
    assert!(ft.lines[0].rect.x >= 150.0 - 1e-6);
}

#[test]
fn slots_narrower_than_three_em_are_dropped() {
    // The default size is 12 pt, so the minimum is 36 pt. A left slot of
    // 26 pt (frame inset 4, obstacle at 30) is skipped; one of 40 is kept.
    let count = |x: f64| {
        let (doc, f) = slots_doc(x, true);
        let lay = layout(&doc);
        let ft = lay.frame(f).unwrap();
        let first = ft.lines[0].rect.y;
        ft.lines.iter().filter(|l| l.rect.y == first).count()
    };
    assert_eq!(count(30.0), 1);
    assert_eq!(count(44.0), 2);
}

#[test]
fn both_sides_keeps_selection_and_hit_geometry_consistent() {
    let (doc, f) = slots_doc(100.0, true);
    let lay = layout(&doc);
    let ft = lay.frame(f).unwrap();
    // Every character offset is covered by exactly one line.
    let mut next = 0;
    for l in &ft.lines {
        assert_eq!(l.chars.start, next);
        next = l.chars.end;
    }
    assert_eq!(next, ft.chars.end);
    // Lines in one row do not overlap horizontally.
    for a in &ft.lines {
        for b in &ft.lines {
            if a.rect.y == b.rect.y && a.chars.start < b.chars.start {
                assert!(a.rect.right() <= b.rect.x + 1e-6);
            }
        }
    }
}

#[test]
fn toggling_wrap_keeps_incremental_equal_to_full() {
    let (mut doc, _) = ellipse_doc(wrap(WrapMode::Square, 4.0, false));
    let ellipse = *doc
        .frames
        .values()
        .find(|f| f.as_text().is_none())
        .map(|f| &f.id)
        .unwrap();
    // Mid-line, so there is room on both sides.
    Arc::make_mut(doc.frames.get_mut(&ellipse).unwrap()).rect.x = 100.0;
    let mut lay = Layouter::new(fonts());
    let mut cache = StoryLayoutCache::new();
    let steps = [
        wrap(WrapMode::Tight, 4.0, false),
        wrap(WrapMode::Tight, 4.0, true),
        wrap(WrapMode::Through, 4.0, false),
        wrap(WrapMode::Through, 10.0, true),
        wrap(WrapMode::Square, 4.0, true),
        wrap(WrapMode::None, 4.0, false),
        wrap(WrapMode::Tight, 4.0, false),
    ];
    let mut seen = Vec::new();
    for w in steps {
        Arc::make_mut(doc.frames.get_mut(&ellipse).unwrap()).wrap = w;
        let inc = cache.layout(&mut lay, &doc);
        let full = lay.layout_document(&doc);
        for (id, s) in &full.stories {
            assert_eq!(inc.stories.get(id), Some(s), "{w:?}");
        }
        seen.push(inc);
    }
    // The settings really change the layout, and the same setting gives the
    // same layout again.
    assert_ne!(seen[0].stories, seen[1].stories);
    assert_ne!(seen[0].stories, seen[5].stories);
    assert_eq!(seen[0].stories, seen[6].stories);
    // A second pass with nothing changed reuses every story.
    cache.layout(&mut lay, &doc);
    assert_eq!(cache.stats().recomposed, 0);
}

#[test]
fn contour_changes_enter_the_cache_key() {
    let n = 100usize;
    let disc = alpha(n, n, |x, y| {
        let (dx, dy) = (x as f64 - 49.5, y as f64 - 49.5);
        dx * dx + dy * dy <= 49.0 * 49.0
    });
    let (doc, f, c) = image_doc(WrapMode::Tight, false, &disc, n as u32);
    let mut lay = Layouter::new(fonts());
    let mut cache = StoryLayoutCache::new();
    // Not yet decoded: the rectangle.
    let before = cache.layout(&mut lay, &doc);
    lay.set_contours(Some(Arc::new(c)));
    let after = cache.layout(&mut lay, &doc);
    assert_ne!(
        before.frame(f).unwrap().lines[0].rect.x,
        after.frame(f).unwrap().lines[0].rect.x
    );
    assert_eq!(cache.stats().recomposed, 1);
    assert_eq!(after.stories, lay.layout_document(&doc).stories);
}

#[test]
fn square_without_both_sides_is_unchanged_by_the_new_fields() {
    // Square keeps the widest gap only, whatever the outline machinery does.
    let (doc, f) = slots_doc(100.0, false);
    let lay = layout(&doc);
    assert!(
        lay.frame(f)
            .unwrap()
            .lines
            .iter()
            .all(|l| l.rect.x == 4.0 || l.rect.x >= 150.0)
    );
}

/// Register a stand-in PNG asset (layout only needs the id to exist).
fn add_asset(b: &mut DocBuilder<SeqIds>) -> AssetId {
    let id = AssetId::new(&mut b.ids);
    b.exec(Command::AddAsset {
        asset: Asset {
            id,
            name: "t.png".into(),
            format: ImageFormat::Png,
            pixel_width: 100,
            pixel_height: 100,
        },
        data: Bytes::new(vec![0u8; 4]),
    })
    .unwrap();
    id
}

#[test]
fn slots_keep_the_story_whole_wherever_the_obstacle_sits() {
    // Sweep an ellipse across the frame: every character is placed once, in
    // order, whatever slots the lines are cut into.
    for step in 0..30 {
        let (mut doc, f) = ellipse_doc(wrap(WrapMode::Through, 3.0, true));
        let e = *doc
            .frames
            .values()
            .find(|f| f.as_text().is_none())
            .map(|f| &f.id)
            .unwrap();
        Arc::make_mut(doc.frames.get_mut(&e).unwrap()).rect = Rect::new(
            f64::from(step) * 9.0,
            f64::from(step % 7) * 10.0,
            60.0 + f64::from(step % 5) * 15.0,
            80.0,
        );
        let lay = layout(&doc);
        let ft = lay.frame(f).unwrap();
        let mut next = 0;
        for l in &ft.lines {
            assert_eq!(l.chars.start, next, "step {step}");
            next = l.chars.end;
        }
    }
}

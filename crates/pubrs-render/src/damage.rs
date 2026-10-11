//! Damage: the parts of a page that must be redrawn after a change, in page
//! points, so the viewport re-renders and uploads only those.
//!
//! Rules ([`page_damage`]):
//!
//! 1. A different page, master, page size or bleed: the whole page.
//! 2. Frames named by [`Touched`] (or, without it, frames whose shared `Arc`
//!    differs between the two documents) that are on the page before or
//!    after: their old and new bounds. Bounds come from what the frame draws
//!    in the editor (fills, strokes with their reach, empty-picture
//!    placeholders, image clips, the ink of its text), through its rotation.
//! 3. Frames added, removed or restacked on the page (or its master): their
//!    old and new bounds.
//! 4. Text frames on the page whose story was laid out again (typing, style
//!    edits, a wrap obstacle moving): every line whose drawing changed, old
//!    and new ink, through the frame's old and new transforms. Lines that
//!    only moved in character offsets but draw the same glyphs are skipped.
//! 5. Image frames on the page showing a touched asset: their bounds.
//! 6. A changed swatch list or colour scheme: the whole page (swatch values
//!    are resolved when drawing, so no frame or layout changed).
//!
//! Whatever paints over or under a damaged rectangle is redrawn with it by
//! the renderer, clipped to the rectangle. Rectangles are clipped to the
//! page (with bleed) and merged until at most [`MAX_RECTS`] remain; a
//! non-finite rectangle makes the damage the whole page.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use pubrs_core::{Document, FrameId, Owner, Rect, Touched};
use pubrs_layout::{DocumentLayout, FrameText, GlyphRun, LineBox};
use skrifa::instance::{LocationRef, Size};
use skrifa::{FontRef, MetadataProvider};

use crate::scene::{Affine, Geom, Prim, SceneOptions, frame_node_with, frame_transform};

/// Most rectangles a page's damage is merged down to.
pub const MAX_RECTS: usize = 8;

/// Rectangles closer than this (points) are merged.
const MERGE_GAP: f64 = 2.0;

/// What changed on a page.
#[derive(Debug, Clone, PartialEq)]
pub enum PageDamage {
    /// Redraw everything.
    Full,
    /// Redraw these page-point rectangles (none: nothing changed).
    Rects(Vec<Rect>),
}

impl PageDamage {
    /// True when nothing needs redrawing.
    pub fn is_empty(&self) -> bool {
        matches!(self, PageDamage::Rects(r) if r.is_empty())
    }
}

/// One revision of a page: its document, layout and page index.
#[derive(Debug, Clone, Copy)]
pub struct PageState<'a> {
    pub doc: &'a Document,
    pub layout: &'a DocumentLayout,
    pub page: usize,
}

impl PageState<'_> {
    /// Frames drawn on the page (master first), in paint order, with the
    /// groups that hold them.
    fn frames(&self) -> Vec<FrameId> {
        let Some(page) = self.doc.pages.get(self.page) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if let Some(m) = page.master
            && self.doc.master(m).is_some()
        {
            out.extend(self.doc.paint_order(Owner::Master(m)));
        }
        out.extend(self.doc.paint_order(Owner::Page(page.id)));
        out
    }

    fn text(&self, frame: FrameId) -> Option<&FrameText> {
        let story = self.doc.story_of(frame)?;
        self.layout.stories.get(&story)?.frames.get(&frame)
    }
}

/// Page-point damage between two revisions of one page. `touched` is the
/// union of [`Touched`] of the changes between them; `None` (unknown, e.g.
/// after undo) compares every frame on the page by its shared `Arc`.
pub fn page_damage(old: PageState, new: PageState, touched: Option<&Touched>) -> PageDamage {
    let (Some(op), Some(np)) = (old.doc.pages.get(old.page), new.doc.pages.get(new.page)) else {
        return PageDamage::Full;
    };
    if op.id != np.id || op.master != np.master || old.doc.setup != new.doc.setup {
        return PageDamage::Full;
    }
    // Rule 6: a swatch or scheme edit recolours anything that refers to it,
    // on any page, without touching the frames: repaint the whole page.
    let same_swatches =
        Arc::ptr_eq(&old.doc.swatches, &new.doc.swatches) || old.doc.swatches == new.doc.swatches;
    if !same_swatches || old.doc.scheme != new.doc.scheme || touched.is_some_and(|t| t.swatches) {
        return PageDamage::Full;
    }
    let old_order = old.frames();
    let new_order = new.frames();
    let old_set: BTreeSet<FrameId> = old_order.iter().copied().collect();
    let new_set: BTreeSet<FrameId> = new_order.iter().copied().collect();
    let all: BTreeSet<FrameId> = old_set.union(&new_set).copied().collect();

    // Rules 2 and 3: frames whose bounds are damaged whole.
    let mut whole: BTreeSet<FrameId> = BTreeSet::new();
    match touched {
        Some(t) => {
            for f in &t.frames {
                for d in old
                    .doc
                    .descendants(*f)
                    .into_iter()
                    .chain(new.doc.descendants(*f))
                {
                    if all.contains(&d) {
                        whole.insert(d);
                    }
                }
            }
            for f in &all {
                let shows_touched_asset = |doc: &Document| {
                    doc.frame(*f)
                        .and_then(|fr| fr.as_image())
                        .and_then(|i| i.asset)
                        .is_some_and(|a| t.assets.contains(&a))
                };
                if shows_touched_asset(old.doc) || shows_touched_asset(new.doc) {
                    whole.insert(*f);
                }
            }
        }
        None => {
            for f in &all {
                let same = match (old.doc.frames.get(f), new.doc.frames.get(f)) {
                    (Some(a), Some(b)) => Arc::ptr_eq(a, b) || a == b,
                    _ => false,
                };
                if !same {
                    whole.insert(*f);
                }
            }
        }
    }
    whole.extend(old_set.symmetric_difference(&new_set).copied());
    whole.extend(restacked(&old_order, &new_order));

    let mut rects: Vec<Rect> = Vec::new();
    for f in &whole {
        if old_set.contains(f) {
            rects.extend(frame_bounds(old, *f));
        }
        if new_set.contains(f) {
            rects.extend(frame_bounds(new, *f));
        }
    }

    // Rule 4: lines that draw differently.
    let mut fonts = FontBoxes::default();
    for f in all.iter().filter(|f| !whole.contains(f)) {
        let (Some(of), Some(nf)) = (old.doc.frame(*f), new.doc.frame(*f)) else {
            continue;
        };
        let (Some(os), Some(ns)) = (old.doc.story_of(*f), new.doc.story_of(*f)) else {
            continue;
        };
        if let (Some(a), Some(b)) = (old.layout.stories.get(&os), new.layout.stories.get(&ns))
            && Arc::ptr_eq(a, b)
        {
            continue;
        }
        let empty = FrameText::default();
        let ot = old.text(*f).unwrap_or(&empty);
        let nt = new.text(*f).unwrap_or(&empty);
        let (otf, ntf) = (
            frame_transform(of.rect, of.rotation),
            frame_transform(nf.rect, nf.rotation),
        );
        for i in 0..ot.lines.len().max(nt.lines.len()) {
            let (a, b) = (ot.lines.get(i), nt.lines.get(i));
            if let (Some(a), Some(b)) = (a, b)
                && draws_same(a, b)
            {
                continue;
            }
            if let Some(ink) = a.and_then(|l| line_ink(l, &mut fonts)) {
                rects.push(map_rect(otf, ink));
            }
            if let Some(ink) = b.and_then(|l| line_ink(l, &mut fonts)) {
                rects.push(map_rect(ntf, ink));
            }
        }
    }

    // A non-finite rectangle (a degenerate transform or font) cannot be
    // clipped or merged: redraw everything rather than lose it.
    if rects.iter().any(|r| {
        !(r.x.is_finite() && r.y.is_finite() && r.width.is_finite() && r.height.is_finite())
    }) {
        return PageDamage::Full;
    }
    let b = new.doc.setup.bleed;
    let page = Rect::new(
        -b,
        -b,
        new.doc.setup.size.width + 2.0 * b,
        new.doc.setup.size.height + 2.0 * b,
    );
    PageDamage::Rects(merge(rects, page))
}

/// Frames present before and after whose relative paint order changed.
fn restacked(old: &[FrameId], new: &[FrameId]) -> Vec<FrameId> {
    let new_set: BTreeSet<FrameId> = new.iter().copied().collect();
    let old_set: BTreeSet<FrameId> = old.iter().copied().collect();
    let a: Vec<FrameId> = old
        .iter()
        .copied()
        .filter(|f| new_set.contains(f))
        .collect();
    let b: Vec<FrameId> = new
        .iter()
        .copied()
        .filter(|f| old_set.contains(f))
        .collect();
    if a == b {
        return Vec::new();
    }
    let pos: BTreeMap<FrameId, usize> = b.iter().enumerate().map(|(i, f)| (*f, i)).collect();
    a.iter()
        .enumerate()
        .filter(|(i, f)| pos.get(f) != Some(i))
        .map(|(_, f)| *f)
        .collect()
}

/// Two lines draw the same pixels (same glyphs at the same places in the
/// same font, size and colour, and the same decorations).
fn draws_same(a: &LineBox, b: &LineBox) -> bool {
    a.decorations == b.decorations
        && a.runs.len() == b.runs.len()
        && a.runs.iter().zip(&b.runs).all(|(x, y)| runs_same(x, y))
}

fn runs_same(a: &GlyphRun, b: &GlyphRun) -> bool {
    a.font == b.font
        && a.font_size == b.font_size
        && a.color == b.color
        && a.synthetic_bold == b.synthetic_bold
        && a.synthetic_skew == b.synthetic_skew
        && a.glyphs.len() == b.glyphs.len()
        && a.glyphs
            .iter()
            .zip(&b.glyphs)
            .all(|(g, h)| g.id == h.id && g.x == h.x && g.y == h.y)
}

/// Font bounding boxes (font units, y up) and units per em, by font key.
#[derive(Default)]
struct FontBoxes(HashMap<(u64, u32), (f64, [f64; 4])>);

impl FontBoxes {
    fn get(&mut self, run: &GlyphRun) -> (f64, [f64; 4]) {
        *self.0.entry(run.font.key()).or_insert_with(|| {
            let font = FontRef::from_index(run.font.bytes(), run.font.index).ok();
            let metrics = font.map(|f| f.metrics(Size::unscaled(), LocationRef::default()));
            let upem = metrics
                .as_ref()
                .map_or(1000.0, |m| f64::from(m.units_per_em.max(1)));
            let bbox = metrics.and_then(|m| m.bounds).map_or(
                [-0.5 * upem, -0.5 * upem, 1.5 * upem, 1.5 * upem],
                |b| {
                    [
                        f64::from(b.x_min),
                        f64::from(b.y_min),
                        f64::from(b.x_max),
                        f64::from(b.y_max),
                    ]
                },
            );
            (upem, bbox)
        })
    }
}

/// Frame-local box holding every mark a line can make: each glyph's origin
/// plus the font's bounding box (grown for synthetic bold and italic and a
/// tenth of an em of slack), and the decorations.
fn line_ink(line: &LineBox, fonts: &mut FontBoxes) -> Option<Rect> {
    let mut out: Option<Rect> = None;
    let mut add = |r: Rect| out = Some(out.map_or(r, |o| o.union(&r)));
    for run in &line.runs {
        if let Some(r) = run_ink(run, fonts) {
            add(r);
        }
    }
    for d in &line.decorations {
        add(d.rect);
    }
    out
}

/// Frame-local box of a glyph run's marks (see [`line_ink`]).
fn run_ink(run: &GlyphRun, fonts: &mut FontBoxes) -> Option<Rect> {
    {
        if run.glyphs.is_empty() {
            return None;
        }
        let (upem, [x0, y0, x1, y1]) = fonts.get(run);
        let k = run.font_size / upem;
        let slack = 0.1 * run.font_size
            + if run.synthetic_bold {
                0.03 * run.font_size
            } else {
                0.0
            };
        let lean = run.synthetic_skew.map_or(0.0, |s| {
            s.to_radians().tan().abs() * y0.abs().max(y1.abs()) * k
        });
        let (mut l, mut t, mut r, mut b) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for g in &run.glyphs {
            l = l.min(g.x + x0 * k);
            r = r.max(g.x + x1 * k);
            t = t.min(g.y - y1 * k);
            b = b.max(g.y - y0 * k);
        }
        let g = slack + lean;
        Some(Rect::new(l - g, t - g, r - l + 2.0 * g, b - t + 2.0 * g))
    }
}

/// Page-point bounds of everything a frame draws, from the primitives the
/// editor's scene gives it (empty picture placeholders included): fills,
/// image clips and rectangles by their boxes, strokes grown by twice their
/// width (miter joins; a diagonal's butt caps reach half of it), glyph runs
/// by their ink, all through the frame's transform. `None` when the frame
/// draws nothing (a group, an invisible or empty frame).
fn frame_bounds(state: PageState, id: FrameId) -> Option<Rect> {
    let node = frame_node_with(
        state.doc,
        state.layout,
        id,
        SceneOptions {
            show_empty_images: true,
        },
    )?;
    let mut fonts = FontBoxes::default();
    let mut out: Option<Rect> = None;
    for prim in &node.prims {
        let local = match prim {
            Prim::Fill { geom, .. } => Some(geom_box(*geom, 0.0)),
            Prim::Stroke { geom, width, .. } => Some(geom_box(*geom, 2.0 * width.max(0.5))),
            Prim::Image { clip, .. } => Some(*clip),
            Prim::Rect { rect, .. } => Some(*rect),
            Prim::Glyphs(run, _) => run_ink(run, &mut fonts),
        };
        if let Some(r) = local {
            let m = map_rect(node.transform, r);
            out = Some(out.map_or(m, |o| o.union(&m)));
        }
    }
    out
}

/// Frame-local box of a shape's geometry, grown by `grow` on every side.
fn geom_box(g: Geom, grow: f64) -> Rect {
    let (x0, y0, x1, y1) = match g {
        Geom::Rect { w, h, .. } | Geom::Ellipse { w, h } => (0.0, 0.0, w, h),
        Geom::Line { x0, y0, x1, y1 } => (x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)),
    };
    Rect::new(
        x0 - grow,
        y0 - grow,
        x1 - x0 + 2.0 * grow,
        y1 - y0 + 2.0 * grow,
    )
}

/// Axis-aligned bounds of a transformed rectangle.
fn map_rect(t: Affine, r: Rect) -> Rect {
    let pts = [
        t.apply(r.x, r.y),
        t.apply(r.right(), r.y),
        t.apply(r.right(), r.bottom()),
        t.apply(r.x, r.bottom()),
    ];
    let (mut l, mut tp, mut rr, mut b) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (x, y) in pts {
        l = l.min(x);
        tp = tp.min(y);
        rr = rr.max(x);
        b = b.max(y);
    }
    Rect::new(l, tp, rr - l, b - tp)
}

fn near(a: &Rect, b: &Rect) -> bool {
    a.x - MERGE_GAP <= b.right()
        && b.x - MERGE_GAP <= a.right()
        && a.y - MERGE_GAP <= b.bottom()
        && b.y - MERGE_GAP <= a.bottom()
}

/// Clip to `page`, merge overlapping or adjacent rectangles, then merge the
/// cheapest pairs until at most [`MAX_RECTS`] remain.
fn merge(rects: Vec<Rect>, page: Rect) -> Vec<Rect> {
    let mut out: Vec<Rect> = rects
        .into_iter()
        .filter_map(|r| r.intersection(&page))
        .filter(|r| r.width > 0.0 && r.height > 0.0)
        .collect();
    loop {
        let mut merged = false;
        let mut i = 0;
        while i < out.len() {
            let mut j = i + 1;
            while j < out.len() {
                if near(&out[i], &out[j]) {
                    let u = out[i].union(&out[j]);
                    out[i] = u;
                    out.swap_remove(j);
                    merged = true;
                } else {
                    j += 1;
                }
            }
            i += 1;
        }
        if !merged {
            break;
        }
    }
    while out.len() > MAX_RECTS {
        let area = |r: &Rect| r.width * r.height;
        let mut best = (0, 1, f64::MAX);
        for i in 0..out.len() {
            for j in i + 1..out.len() {
                let grow = area(&out[i].union(&out[j])) - area(&out[i]) - area(&out[j]);
                if grow < best.2 {
                    best = (i, j, grow);
                }
            }
        }
        let u = out[best.0].union(&out[best.1]);
        out[best.0] = u;
        out.swap_remove(best.1);
    }
    out.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
    out
}

/// A hash of everything that decides how a page draws: its setup, master,
/// frames in paint order (all their fields) and the glyphs and decorations
/// of their text. Equal fingerprints mean equal pixels (barring hash
/// collisions); used to skip re-rendering thumbnails of pages an edit did
/// not reach. Valid within one process only.
pub fn page_fingerprint(state: PageState) -> u64 {
    use std::fmt::Write as _;
    use std::hash::{Hash, Hasher};

    struct H(std::collections::hash_map::DefaultHasher);
    impl std::fmt::Write for H {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            self.0.write(s.as_bytes());
            Ok(())
        }
    }
    let mut h = H(std::collections::hash_map::DefaultHasher::new());
    let page = state.doc.pages.get(state.page);
    let _ = write!(
        h,
        "{:?}{:?}{:?}",
        state.doc.setup,
        page.map(|p| p.id),
        page.map(|p| p.master)
    );
    for f in state.frames() {
        if let Some(frame) = state.doc.frame(f) {
            let _ = write!(h, "{frame:?}");
        }
        if let Some(text) = state.text(f) {
            for line in &text.lines {
                for run in &line.runs {
                    run.font.key().hash(&mut h.0);
                    run.font_size.to_bits().hash(&mut h.0);
                    let _ = write!(
                        h,
                        "{:?}{}{:?}",
                        run.color, run.synthetic_bold, run.synthetic_skew
                    );
                    for g in &run.glyphs {
                        g.id.hash(&mut h.0);
                        g.x.to_bits().hash(&mut h.0);
                        g.y.to_bits().hash(&mut h.0);
                    }
                }
                let _ = write!(h, "{:?}", line.decorations);
            }
        }
    }
    h.0.finish()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use pubrs_core::{
        Command, DocBuilder, Editor, FrameStyle, PageSetup, SeqIds, Shape, Size, Touched,
    };
    use pubrs_layout::{FontLibrary, Layouter};

    use super::*;

    thread_local! {
        /// One layouter for every layout, as in the app: font handles (and
        /// so glyph runs) compare equal across layouts.
        static LAYOUTER: std::cell::RefCell<Layouter> =
            std::cell::RefCell::new(Layouter::new(FontLibrary::bundled()));
    }

    fn layout(doc: &Document) -> DocumentLayout {
        LAYOUTER.with(|l| l.borrow_mut().layout_document(doc))
    }

    fn setup() -> PageSetup {
        PageSetup {
            size: Size::new(400.0, 300.0),
            bleed: 9.0,
            ..Default::default()
        }
    }

    /// Apply `cmd` to `doc`; returns the new document and what it touched.
    fn apply(doc: &Document, cmd: Command) -> (Document, Touched) {
        let mut ed = Editor::new(doc.clone());
        let change = ed.execute(cmd).unwrap();
        (ed.doc().clone(), change.touched)
    }

    fn damage(old: &Document, new: &Document, touched: Option<&Touched>) -> PageDamage {
        let (ol, nl) = (layout(old), layout(new));
        page_damage(
            PageState {
                doc: old,
                layout: &ol,
                page: 0,
            },
            PageState {
                doc: new,
                layout: &nl,
                page: 0,
            },
            touched,
        )
    }

    fn rects(d: &PageDamage) -> &[Rect] {
        match d {
            PageDamage::Rects(r) => r,
            PageDamage::Full => panic!("expected rectangles, got the whole page"),
        }
    }

    fn covers(d: &PageDamage, r: Rect) -> bool {
        // Every corner of `r` lies in some damage rectangle.
        let pts = [
            (r.x, r.y),
            (r.right(), r.y),
            (r.x, r.bottom()),
            (r.right(), r.bottom()),
        ];
        pts.iter().all(|(x, y)| {
            rects(d)
                .iter()
                .any(|d| *x >= d.x && *x <= d.right() && *y >= d.y && *y <= d.bottom())
        })
    }

    fn touches(d: &PageDamage, x: f64, y: f64) -> bool {
        rects(d)
            .iter()
            .any(|d| x >= d.x && x <= d.right() && y >= d.y && y <= d.bottom())
    }

    fn shape_doc() -> (Document, FrameId) {
        let mut b = DocBuilder::new(SeqIds::new(1), setup());
        let p = b.page(0);
        let f = b
            .shape(
                p,
                Rect::new(20.0, 20.0, 60.0, 40.0),
                Shape::Rectangle { corner_radius: 0.0 },
            )
            .unwrap();
        b.style(
            f,
            FrameStyle {
                fill: Some(pubrs_core::Color::BLACK.into()),
                ..Default::default()
            },
        )
        .unwrap();
        (b.finish(), f)
    }

    #[test]
    fn no_change_no_damage() {
        let (doc, _) = shape_doc();
        assert!(damage(&doc, &doc, None).is_empty());
        assert!(damage(&doc, &doc, Some(&Touched::default())).is_empty());
    }

    #[test]
    fn a_swatch_or_scheme_edit_damages_the_whole_page() {
        use pubrs_core::{Cmyk, Swatch, SwatchId, SwatchValue};
        let (doc, _) = shape_doc();
        let id = SwatchId::new(&mut SeqIds::new(9));
        let add = Command::AddSwatch {
            swatch: Swatch {
                id,
                name: "Ink".into(),
                value: SwatchValue::Cmyk(Cmyk::new(0.0, 0.0, 0.0, 100.0)),
            },
        };
        let (with, touched) = apply(&doc, add);
        assert!(touched.swatches);
        for t in [Some(&touched), None] {
            assert_eq!(damage(&doc, &with, t), PageDamage::Full);
        }
        let (edited, touched) = apply(
            &with,
            Command::UpdateSwatch {
                swatch: Swatch {
                    id,
                    name: "Ink".into(),
                    value: SwatchValue::Cmyk(Cmyk::new(0.0, 0.0, 0.0, 50.0)),
                },
            },
        );
        for t in [Some(&touched), None] {
            assert_eq!(damage(&with, &edited, t), PageDamage::Full);
        }
        let (scheme, touched) = apply(
            &edited,
            Command::ApplyScheme {
                scheme: pubrs_core::SchemeId("coral_reef".into()),
            },
        );
        assert_eq!(damage(&edited, &scheme, Some(&touched)), PageDamage::Full);
    }

    #[test]
    fn a_moved_frame_damages_its_old_and_new_bounds() {
        let (doc, f) = shape_doc();
        let (moved, touched) = apply(
            &doc,
            Command::SetFrameRect {
                frame: f,
                rect: Rect::new(250.0, 200.0, 60.0, 40.0),
            },
        );
        for t in [Some(&touched), None] {
            let d = damage(&doc, &moved, t);
            assert!(covers(&d, Rect::new(20.0, 20.0, 60.0, 40.0)), "{d:?}");
            assert!(covers(&d, Rect::new(250.0, 200.0, 60.0, 40.0)), "{d:?}");
            assert!(!touches(&d, 160.0, 120.0), "the gap between stays clean");
        }
    }

    #[test]
    fn rotation_damages_the_rotated_bounds() {
        let (doc, f) = shape_doc();
        let (rotated, touched) = apply(
            &doc,
            Command::SetRotation {
                frame: f,
                degrees: 45.0,
            },
        );
        let d = damage(&doc, &rotated, Some(&touched));
        // The rotated 60x40 box about (50, 40) reaches about 35 pt from it.
        assert!(touches(&d, 50.0, 40.0 - 34.0), "{d:?}");
        assert!(touches(&d, 50.0 + 34.0, 40.0 + 10.0), "{d:?}");
    }

    fn text_doc() -> (Document, FrameId, FrameId) {
        let mut b = DocBuilder::new(SeqIds::new(2), setup());
        let p = b.page(0);
        let a = b
            .text(
                p,
                Rect::new(20.0, 20.0, 160.0, 200.0),
                "The first paragraph runs over several lines so that its opening \
                 line sits far above anything typed later on in the story.\n\
                 Second paragraph.\nThird paragraph, quite short.",
            )
            .unwrap();
        let c = b.text(p, Rect::new(220.0, 20.0, 160.0, 200.0), "").unwrap();
        b.link(a, c).unwrap();
        (b.finish(), a, c)
    }

    #[test]
    fn typing_damages_only_the_lines_that_change() {
        let (doc, a, _) = text_doc();
        let story = doc.story_of(a).unwrap();
        let text = doc.story(story).unwrap().plain_text();
        let at = text[..text.find("Second").unwrap()].chars().count() + 6;
        let (typed, touched) = apply(
            &doc,
            Command::InsertText {
                story,
                at,
                text: "!".into(),
                format: None,
            },
        );
        let d = damage(&doc, &typed, Some(&touched));
        let l = layout(&typed);
        let lines = &l.frame(a).unwrap().lines;
        let second = lines
            .iter()
            .find(|line| line.chars.contains(&at))
            .unwrap()
            .rect;
        assert!(
            touches(
                &d,
                20.0 + second.x + 5.0,
                20.0 + second.y + second.height / 2.0
            ),
            "the edited line is damaged: {d:?}"
        );
        let first = lines[0].rect;
        assert!(
            !touches(
                &d,
                20.0 + first.x + 5.0,
                20.0 + first.y + first.height / 2.0
            ),
            "the first line did not change: {d:?}"
        );
        assert!(
            !touches(&d, 300.0, 100.0),
            "the empty linked frame is clean"
        );
    }

    #[test]
    fn reflow_damages_the_next_frame_of_the_chain() {
        let (doc, a, c) = text_doc();
        let story = doc.story_of(a).unwrap();
        let long = "More words that push the story on. ".repeat(30);
        let (typed, touched) = apply(
            &doc,
            Command::InsertText {
                story,
                at: 0,
                text: long,
                format: None,
            },
        );
        let l = layout(&typed);
        let into_c = l.frame(c).unwrap().lines.first().unwrap().rect;
        let d = damage(&doc, &typed, Some(&touched));
        assert!(
            touches(
                &d,
                220.0 + into_c.x + 5.0,
                20.0 + into_c.y + into_c.height / 2.0
            ),
            "text flowing into the second frame is damaged: {d:?}"
        );
        assert!(rects(&d).len() <= MAX_RECTS);
    }

    #[test]
    fn restacking_damages_the_frames_that_moved_in_paint_order() {
        let mut b = DocBuilder::new(SeqIds::new(3), setup());
        let p = b.page(0);
        let x = b
            .shape(p, Rect::new(20.0, 20.0, 50.0, 50.0), Shape::Ellipse)
            .unwrap();
        let y = b
            .shape(p, Rect::new(40.0, 40.0, 50.0, 50.0), Shape::Ellipse)
            .unwrap();
        for f in [x, y] {
            b.style(
                f,
                FrameStyle {
                    fill: Some(pubrs_core::Color::BLACK.into()),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        let doc = b.finish();
        let (front, touched) = apply(&doc, Command::SetZIndex { frame: x, index: 1 });
        for t in [Some(&touched), None] {
            let d = damage(&doc, &front, t);
            assert!(covers(&d, Rect::new(20.0, 20.0, 50.0, 50.0)), "{d:?}");
        }
    }

    #[test]
    fn empty_picture_placeholders_damage_their_stroke_reach() {
        let mut b = DocBuilder::new(SeqIds::new(4), setup());
        let p = b.page(0);
        let pic = b.image(p, Rect::new(40.0, 40.0, 50.0, 30.0), None).unwrap();
        let doc = b.finish();
        let (moved, touched) = apply(
            &doc,
            Command::SetFrameRect {
                frame: pic,
                rect: Rect::new(150.0, 40.0, 50.0, 30.0),
            },
        );
        let d = damage(&doc, &moved, Some(&touched));
        // The 1 pt diagonals end in butt caps half a point past the corners.
        assert!(covers(&d, Rect::new(39.5, 39.5, 51.0, 31.0)), "{d:?}");
        assert!(covers(&d, Rect::new(149.5, 39.5, 51.0, 31.0)), "{d:?}");
    }

    #[test]
    fn invisible_frames_damage_nothing() {
        let mut b = DocBuilder::new(SeqIds::new(5), setup());
        let p = b.page(0);
        let ghost = b
            .shape(p, Rect::new(20.0, 20.0, 50.0, 50.0), Shape::Ellipse)
            .unwrap();
        let doc = b.finish();
        let (moved, touched) = apply(
            &doc,
            Command::SetFrameRect {
                frame: ghost,
                rect: Rect::new(120.0, 20.0, 50.0, 50.0),
            },
        );
        assert!(damage(&doc, &moved, Some(&touched)).is_empty());
    }

    #[test]
    fn page_setup_changes_damage_everything() {
        let (doc, _) = shape_doc();
        let (bled, touched) = apply(&doc, Command::SetBleed { bleed: 18.0 });
        assert_eq!(damage(&doc, &bled, Some(&touched)), PageDamage::Full);
    }

    #[test]
    fn merging_clips_to_the_page_and_caps_the_count() {
        let page = Rect::new(-9.0, -9.0, 418.0, 318.0);
        let many: Vec<Rect> = (0..40)
            .map(|i| Rect::new(f64::from(i) * 10.0, f64::from(i % 7) * 40.0, 4.0, 4.0))
            .chain([Rect::new(-50.0, -50.0, 60.0, 60.0)])
            .collect();
        let out = merge(many, page);
        assert!(out.len() <= MAX_RECTS);
        assert!(
            out.iter()
                .all(|r| r.x >= page.x && r.y >= page.y && r.right() <= page.right())
        );
        let touching = merge(
            vec![
                Rect::new(0.0, 0.0, 10.0, 10.0),
                Rect::new(11.0, 0.0, 10.0, 10.0),
            ],
            page,
        );
        assert_eq!(touching, vec![Rect::new(0.0, 0.0, 21.0, 10.0)]);
    }

    #[test]
    fn fingerprints_follow_drawing() {
        let (doc, f) = shape_doc();
        let l = layout(&doc);
        let state = |d| PageState {
            doc: d,
            layout: &l,
            page: 0,
        };
        let same = doc.clone();
        assert_eq!(
            page_fingerprint(state(&doc)),
            page_fingerprint(state(&same))
        );
        let (moved, _) = apply(
            &doc,
            Command::SetFrameRect {
                frame: f,
                rect: Rect::new(21.0, 20.0, 60.0, 40.0),
            },
        );
        assert_ne!(
            page_fingerprint(state(&doc)),
            page_fingerprint(state(&moved))
        );
    }
}

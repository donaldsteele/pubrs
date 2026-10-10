//! Snapping to page edges, margins, guides, other frames and the grid.

use crate::geom::{Point, Rect};
use crate::id::FrameId;
use crate::model::{Axis, Document, Owner};

/// Candidate x and y lines to snap to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SnapTargets {
    pub xs: Vec<f64>,
    pub ys: Vec<f64>,
}

/// Collect snap lines for a page (or master): page edges and centre,
/// margins, guides (page and its master), and edges/centres of other frames.
pub fn snap_targets(doc: &Document, owner: Owner, exclude: &[FrameId]) -> SnapTargets {
    let mut t = SnapTargets::default();
    let s = doc.setup.size;
    let m = doc.setup.margins;
    t.xs.extend([0.0, s.width / 2.0, s.width, m.left, s.width - m.right]);
    t.ys.extend([0.0, s.height / 2.0, s.height, m.top, s.height - m.bottom]);
    if doc.grid.snap_to_guides {
        let mut guides = doc.guides(owner).cloned().unwrap_or_default();
        if let Owner::Page(p) = owner
            && let Some(mid) = doc.page(p).and_then(|p| p.master)
            && let Some(mg) = doc.guides(Owner::Master(mid))
        {
            guides.extend(mg.iter().copied());
        }
        for g in guides {
            match g.axis {
                Axis::Vertical => t.xs.push(g.position),
                Axis::Horizontal => t.ys.push(g.position),
            }
        }
    }
    for id in doc.all_frames_of(owner) {
        if exclude.contains(&id) {
            continue;
        }
        if let Some(f) = doc.frame(id) {
            let b = f.bounds();
            t.xs.extend([b.left(), b.center().x, b.right()]);
            t.ys.extend([b.top(), b.center().y, b.bottom()]);
        }
    }
    t
}

/// The target nearest to `v` within `tolerance`, if any.
pub fn nearest(v: f64, targets: &[f64], tolerance: f64) -> Option<f64> {
    targets
        .iter()
        .copied()
        .filter(|t| (t - v).abs() <= tolerance)
        .min_by(|a, b| (a - v).abs().total_cmp(&(b - v).abs()))
}

fn snap_to_grid(v: f64, spacing: f64) -> f64 {
    (v / spacing).round() * spacing
}

/// Snap a point (e.g. a resize handle) using the document's settings.
pub fn snap_point(doc: &Document, owner: Owner, p: Point, exclude: &[FrameId]) -> Point {
    let tol = doc.grid.snap_tolerance;
    let t = snap_targets(doc, owner, exclude);
    let mut out = p;
    if let Some(x) = nearest(p.x, &t.xs, tol) {
        out.x = x;
    } else if doc.grid.snap_to_grid {
        out.x = snap_to_grid(p.x, doc.grid.spacing);
    }
    if let Some(y) = nearest(p.y, &t.ys, tol) {
        out.y = y;
    } else if doc.grid.snap_to_grid {
        out.y = snap_to_grid(p.y, doc.grid.spacing);
    }
    out
}

/// Snap a moved rectangle: its left, centre or right edge (whichever is
/// closest to a target) and likewise vertically. Size is preserved.
pub fn snap_move(doc: &Document, owner: Owner, r: Rect, exclude: &[FrameId]) -> Rect {
    let tol = doc.grid.snap_tolerance;
    let t = snap_targets(doc, owner, exclude);
    let best = |edges: [f64; 3], targets: &[f64]| -> Option<f64> {
        edges
            .iter()
            .filter_map(|e| nearest(*e, targets, tol).map(|s| s - e))
            .min_by(|a, b| a.abs().total_cmp(&b.abs()))
    };
    let dx = best([r.left(), r.center().x, r.right()], &t.xs).or_else(|| {
        doc.grid
            .snap_to_grid
            .then(|| snap_to_grid(r.x, doc.grid.spacing) - r.x)
    });
    let dy = best([r.top(), r.center().y, r.bottom()], &t.ys).or_else(|| {
        doc.grid
            .snap_to_grid
            .then(|| snap_to_grid(r.y, doc.grid.spacing) - r.y)
    });
    r.translate(dx.unwrap_or(0.0), dy.unwrap_or(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::{GuideId, SeqIds};
    use crate::model::{Guide, PageSetup};

    #[test]
    fn snaps_to_margin_and_guides() {
        let mut ids = SeqIds::new(1);
        let mut doc = Document::new(&mut ids, PageSetup::default());
        let page = doc.pages[0].id;
        doc.pages[0].guides.push(Guide {
            id: GuideId::new(&mut ids),
            axis: Axis::Horizontal,
            position: 200.0,
        });
        let r = snap_move(
            &doc,
            Owner::Page(page),
            Rect::new(38.0, 197.0, 100.0, 50.0),
            &[],
        );
        assert_eq!(r, Rect::new(36.0, 200.0, 100.0, 50.0));
        let far = Rect::new(100.0, 100.0, 10.0, 10.0);
        assert_eq!(snap_move(&doc, Owner::Page(page), far, &[]), far);
    }

    #[test]
    fn grid_snapping_when_enabled() {
        let mut ids = SeqIds::new(1);
        let mut doc = Document::new(&mut ids, PageSetup::default());
        doc.grid.snap_to_grid = true;
        doc.grid.snap_to_guides = false;
        doc.grid.snap_tolerance = 0.0;
        let page = doc.pages[0].id;
        let p = snap_point(&doc, Owner::Page(page), Point::new(100.0, 110.0), &[]);
        assert_eq!(p, Point::new(108.0, 108.0));
    }
}

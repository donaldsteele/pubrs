//! Outline-following text wrap: obstacle polygons, the alpha contour of
//! placed images, and the scanline maths that turns a polygon into the
//! x-intervals it covers over a band of a line (ADR 0022).

use std::sync::Arc;

use pubrs_core::{AssetId, Point, Rect};

/// Alpha at or above this counts as covered (50%).
pub const ALPHA_THRESHOLD: u8 = 128;
/// The coverage mask is at most this many cells along its longer side.
const MAX_MASK: u32 = 256;
/// Segments of the polygon standing for an ellipse.
pub const ELLIPSE_SEGMENTS: usize = 64;
/// Segments per quarter of a rounded rectangle corner.
const CORNER_SEGMENTS: usize = 8;

/// The outline of the visible pixels of an image: closed polygons in the
/// image's own pixel space (origin top-left, y down). Holes are polygons
/// of their own; coverage is even-odd.
#[derive(Debug, Clone, PartialEq)]
pub struct Contour {
    pub width: u32,
    pub height: u32,
    pub polys: Vec<Vec<Point>>,
}

/// Source of image contours for layout. `pubrs-layout` cannot decode
/// images, so the caller supplies this (`pubrs-io`'s image cache does).
/// `None` means "no usable contour": the frame rectangle is used.
pub trait AlphaContours: Send + Sync {
    fn contour(&self, asset: AssetId) -> Option<Arc<Contour>>;
}

/// Polygons in the text frame's local space, with equality that is cheap
/// for the common case (same allocation) and exact otherwise, so the
/// story cache key covers outline changes.
#[derive(Debug, Clone)]
pub(crate) struct Outline(pub Arc<Vec<Vec<Point>>>);

impl PartialEq for Outline {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || *self.0 == *other.0
    }
}

/// Trace the outline of the pixels with alpha at or above 50%.
///
/// `alpha` holds `width * height` alpha bytes, row by row. Returns `None`
/// for an image that is fully opaque or fully transparent (the frame
/// rectangle stands in), or when the buffer does not match.
///
/// The mask is reduced to at most 256 cells along its longer side, a cell
/// being covered when any of its pixels is (so the outline errs outward).
/// The boundary of the covered cells is followed into closed loops, which
/// are simplified with Douglas-Peucker at a tolerance of one cell (one
/// pixel for images up to 256 px).
pub fn trace_contour(width: u32, height: u32, alpha: &[u8]) -> Option<Contour> {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || alpha.len() != w * h {
        return None;
    }
    if alpha.iter().all(|a| *a == 255) {
        return None;
    }
    let s = (width.max(height)).div_ceil(MAX_MASK).max(1) as usize;
    let (mw, mh) = (w.div_ceil(s), h.div_ceil(s));
    let mut mask = vec![false; mw * mh];
    let mut any = false;
    for y in 0..h {
        let row = &alpha[y * w..(y + 1) * w];
        for (x, a) in row.iter().enumerate() {
            if *a >= ALPHA_THRESHOLD {
                mask[(y / s) * mw + x / s] = true;
                any = true;
            }
        }
    }
    if !any {
        return None;
    }
    let set = |x: isize, y: isize| {
        x >= 0
            && y >= 0
            && (x as usize) < mw
            && (y as usize) < mh
            && mask[y as usize * mw + x as usize]
    };
    // Directed boundary edges between grid vertices, clockwise around each
    // covered cell (y down).
    let vw = mw + 1;
    let vid = |x: usize, y: usize| (y * vw + x) as u32;
    let mut edges: Vec<(u32, u32)> = Vec::new();
    for y in 0..mh {
        for x in 0..mw {
            if !mask[y * mw + x] {
                continue;
            }
            let (xi, yi) = (x as isize, y as isize);
            if !set(xi, yi - 1) {
                edges.push((vid(x, y), vid(x + 1, y)));
            }
            if !set(xi + 1, yi) {
                edges.push((vid(x + 1, y), vid(x + 1, y + 1)));
            }
            if !set(xi, yi + 1) {
                edges.push((vid(x + 1, y + 1), vid(x, y + 1)));
            }
            if !set(xi - 1, yi) {
                edges.push((vid(x, y + 1), vid(x, y)));
            }
        }
    }
    edges.sort_unstable();
    let mut used = vec![false; edges.len()];
    let mut polys = Vec::new();
    let tol = s as f64;
    for start in 0..edges.len() {
        if used[start] {
            continue;
        }
        let mut loop_pts: Vec<(u32, u32)> = Vec::new();
        let mut e = start;
        loop {
            used[e] = true;
            let (from, to) = edges[e];
            loop_pts.push((from % vw as u32, from / vw as u32));
            let lo = edges.partition_point(|(f, _)| *f < to);
            let next = (lo..edges.len())
                .take_while(|i| edges[*i].0 == to)
                .find(|i| !used[*i]);
            match next {
                Some(n) => e = n,
                None => break,
            }
        }
        let pts: Vec<Point> = loop_pts
            .iter()
            .map(|(x, y)| {
                Point::new(
                    (*x as usize * s).min(w) as f64,
                    (*y as usize * s).min(h) as f64,
                )
            })
            .collect();
        let pts = drop_collinear(pts);
        let pts = simplify_closed(&pts, tol);
        if pts.len() >= 3 {
            polys.push(pts);
        }
    }
    (!polys.is_empty()).then_some(Contour {
        width,
        height,
        polys,
    })
}

fn drop_collinear(pts: Vec<Point>) -> Vec<Point> {
    let n = pts.len();
    if n < 4 {
        return pts;
    }
    (0..n)
        .filter(|i| {
            let (a, b, c) = (pts[(i + n - 1) % n], pts[*i], pts[(i + 1) % n]);
            (b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x) != 0.0
        })
        .map(|i| pts[i])
        .collect()
}

fn dist_to_chord(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = dx.hypot(dy);
    if len == 0.0 {
        return p.distance(a);
    }
    ((p.x - a.x) * dy - (p.y - a.y) * dx).abs() / len
}

fn dp_open(pts: &[Point], tol: f64, out: &mut Vec<Point>) {
    // `pts` runs from a kept point to a kept point; push the kept interior.
    if pts.len() < 3 {
        return;
    }
    let (a, b) = (pts[0], pts[pts.len() - 1]);
    let (mut worst, mut at) = (0.0, 0);
    for (i, p) in pts.iter().enumerate().take(pts.len() - 1).skip(1) {
        let d = dist_to_chord(*p, a, b);
        if d > worst {
            worst = d;
            at = i;
        }
    }
    if worst > tol {
        dp_open(&pts[..=at], tol, out);
        out.push(pts[at]);
        dp_open(&pts[at..], tol, out);
    }
}

/// Douglas-Peucker on a closed loop: split at the first vertex and the one
/// farthest from it.
fn simplify_closed(pts: &[Point], tol: f64) -> Vec<Point> {
    if pts.len() <= 3 {
        return pts.to_vec();
    }
    let far = (1..pts.len())
        .max_by(|a, b| {
            pts[0]
                .distance(pts[*a])
                .total_cmp(&pts[0].distance(pts[*b]))
        })
        .unwrap_or(1);
    let mut out = vec![pts[0]];
    dp_open(&pts[..=far], tol, &mut out);
    out.push(pts[far]);
    let mut back: Vec<Point> = pts[far..].to_vec();
    back.push(pts[0]);
    dp_open(&back, tol, &mut out);
    out
}

/// A rectangle with optionally rounded corners, clockwise.
pub(crate) fn rounded_rect(w: f64, h: f64, radius: f64) -> Vec<Point> {
    let r = radius.min(w / 2.0).min(h / 2.0).max(0.0);
    if r <= 0.0 {
        return vec![
            Point::new(0.0, 0.0),
            Point::new(w, 0.0),
            Point::new(w, h),
            Point::new(0.0, h),
        ];
    }
    let mut out = Vec::with_capacity(4 * (CORNER_SEGMENTS + 1));
    // (corner centre, start angle in degrees)
    let corners = [
        (Point::new(w - r, r), -90.0),
        (Point::new(w - r, h - r), 0.0),
        (Point::new(r, h - r), 90.0),
        (Point::new(r, r), 180.0),
    ];
    for (c, start) in corners {
        for i in 0..=CORNER_SEGMENTS {
            let a = (start + 90.0 * i as f64 / CORNER_SEGMENTS as f64).to_radians();
            // Scaled out so the chords stay outside the true arc.
            let k = 1.0 / (std::f64::consts::FRAC_PI_2 / CORNER_SEGMENTS as f64 / 2.0).cos();
            let (sn, cs) = a.sin_cos();
            out.push(Point::new(c.x + r * k * cs, c.y + r * k * sn));
        }
    }
    for p in &mut out {
        p.x = p.x.clamp(0.0, w);
        p.y = p.y.clamp(0.0, h);
    }
    out
}

/// A circumscribed polygon of the ellipse filling `w` × `h`.
pub(crate) fn ellipse(w: f64, h: f64) -> Vec<Point> {
    let k = 1.0 / (std::f64::consts::PI / ELLIPSE_SEGMENTS as f64).cos();
    (0..ELLIPSE_SEGMENTS)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / ELLIPSE_SEGMENTS as f64;
            let (s, c) = a.sin_cos();
            Point::new(w / 2.0 + w / 2.0 * k * c, h / 2.0 + h / 2.0 * k * s)
        })
        .collect()
}

/// The stroked outline of a segment (butt caps).
pub(crate) fn stroked_line(a: Point, b: Point, width: f64) -> Vec<Point> {
    let half = (width / 2.0).max(0.25);
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = dx.hypot(dy);
    let (nx, ny) = if len > 0.0 {
        (-dy / len * half, dx / len * half)
    } else {
        (0.0, half)
    };
    vec![
        Point::new(a.x + nx, a.y + ny),
        Point::new(b.x + nx, b.y + ny),
        Point::new(b.x - nx, b.y - ny),
        Point::new(a.x - nx, a.y - ny),
    ]
}

/// Sutherland-Hodgman clip of a polygon to a rectangle.
pub(crate) fn clip_to_rect(poly: &[Point], r: Rect) -> Vec<Point> {
    type Inside = fn(Point, Rect) -> bool;
    type Cross = fn(Point, Point, Rect) -> Point;
    let sides: [(Inside, Cross); 4] = [
        (
            |p, r| p.x >= r.left(),
            |a, b, r| {
                let t = (r.left() - a.x) / (b.x - a.x);
                Point::new(r.left(), a.y + t * (b.y - a.y))
            },
        ),
        (
            |p, r| p.x <= r.right(),
            |a, b, r| {
                let t = (r.right() - a.x) / (b.x - a.x);
                Point::new(r.right(), a.y + t * (b.y - a.y))
            },
        ),
        (
            |p, r| p.y >= r.top(),
            |a, b, r| {
                let t = (r.top() - a.y) / (b.y - a.y);
                Point::new(a.x + t * (b.x - a.x), r.top())
            },
        ),
        (
            |p, r| p.y <= r.bottom(),
            |a, b, r| {
                let t = (r.bottom() - a.y) / (b.y - a.y);
                Point::new(a.x + t * (b.x - a.x), r.bottom())
            },
        ),
    ];
    let mut cur = poly.to_vec();
    for (inside, cross) in sides {
        if cur.is_empty() {
            break;
        }
        let mut next = Vec::with_capacity(cur.len() + 2);
        for i in 0..cur.len() {
            let (a, b) = (cur[i], cur[(i + 1) % cur.len()]);
            match (inside(a, r), inside(b, r)) {
                (true, true) => next.push(b),
                (true, false) => next.push(cross(a, b, r)),
                (false, true) => {
                    next.push(cross(a, b, r));
                    next.push(b);
                }
                (false, false) => {}
            }
        }
        cur = next;
    }
    cur
}

/// Bounds of a set of polygons.
pub(crate) fn poly_bounds(polys: &[Vec<Point>]) -> Option<Rect> {
    let mut it = polys.iter().flatten();
    let first = it.next()?;
    let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.x, first.y);
    for p in it {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    Some(Rect::new(x0, y0, x1 - x0, y1 - y0))
}

/// The x-intervals covered by the polygons (even-odd) somewhere within the
/// horizontal band `y0..y1`, grown by `margin` on every side (a square
/// offset, so conservative against a round one).
///
/// Exact per sub-band: between two consecutive vertex heights the edges
/// that cross the whole sub-band keep their order, so pairing them up
/// gives the covered spans, and each span is taken at its widest.
pub(crate) fn band_intervals(
    polys: &[Vec<Point>],
    y0: f64,
    y1: f64,
    margin: f64,
) -> Vec<(f64, f64)> {
    let (ya, yb) = (y0 - margin, (y1 + margin).max(y0 - margin + 1e-6));
    // Edges as (top y, bottom y, x at top, x at bottom), horizontal ones left out.
    let mut edges: Vec<(f64, f64, f64, f64)> = Vec::new();
    let mut ys: Vec<f64> = vec![ya, yb];
    for poly in polys {
        for i in 0..poly.len() {
            let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
            if p.y == q.y {
                continue;
            }
            let e = if p.y < q.y {
                (p.y, q.y, p.x, q.x)
            } else {
                (q.y, p.y, q.x, p.x)
            };
            if e.1 <= ya || e.0 >= yb {
                continue;
            }
            edges.push(e);
            for y in [e.0, e.1] {
                if y > ya && y < yb {
                    ys.push(y);
                }
            }
        }
    }
    ys.sort_by(f64::total_cmp);
    ys.dedup();
    let at = |e: &(f64, f64, f64, f64), y: f64| e.2 + (e.3 - e.2) * (y - e.0) / (e.1 - e.0);
    let mut spans: Vec<(f64, f64)> = Vec::new();
    for pair in ys.windows(2) {
        let (c0, c1) = (pair[0], pair[1]);
        let mut crossing: Vec<(f64, f64)> = edges
            .iter()
            .filter(|e| e.0 <= c0 && e.1 >= c1)
            .map(|e| (at(e, c0), at(e, c1)))
            .collect();
        crossing.sort_by(|a, b| (a.0 + a.1).total_cmp(&(b.0 + b.1)));
        for i in 0..crossing.len() / 2 {
            let (a, b) = (crossing[2 * i], crossing[2 * i + 1]);
            let lo = a.0.min(a.1);
            let hi = b.0.max(b.1);
            spans.push((lo - margin, hi + margin));
        }
    }
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f64, f64)> = Vec::with_capacity(spans.len());
    for s in spans {
        match merged.last_mut() {
            Some(last) if s.0 <= last.1 => last.1 = last.1.max(s.1),
            _ => merged.push(s),
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_band() {
        let sq = vec![rounded_rect(10.0, 10.0, 0.0)];
        let iv = band_intervals(&sq, 2.0, 4.0, 0.0);
        assert_eq!(iv, vec![(0.0, 10.0)]);
        assert!(band_intervals(&sq, 11.0, 12.0, 0.0).is_empty());
        // The margin grows the band too.
        assert_eq!(band_intervals(&sq, 11.0, 12.0, 2.0), vec![(-2.0, 12.0)]);
    }

    #[test]
    fn ellipse_band_is_narrower_near_the_top() {
        let e = vec![ellipse(100.0, 100.0)];
        let top = band_intervals(&e, 0.0, 2.0, 0.0);
        let mid = band_intervals(&e, 49.0, 51.0, 0.0);
        assert!(top[0].1 - top[0].0 < 40.0);
        assert!(mid[0].1 - mid[0].0 >= 100.0);
    }

    #[test]
    fn holes_are_even_odd() {
        let outer = rounded_rect(30.0, 30.0, 0.0);
        let hole: Vec<Point> = vec![
            Point::new(10.0, 10.0),
            Point::new(20.0, 10.0),
            Point::new(20.0, 20.0),
            Point::new(10.0, 20.0),
        ];
        let iv = band_intervals(&[outer, hole], 14.0, 16.0, 0.0);
        assert_eq!(iv, vec![(0.0, 10.0), (20.0, 30.0)]);
    }

    #[test]
    fn traces_a_disc_and_an_l() {
        let n = 40usize;
        let mut a = vec![0u8; n * n];
        for y in 0..n {
            for x in 0..n {
                let (dx, dy) = (x as f64 - 19.5, y as f64 - 19.5);
                if dx * dx + dy * dy <= 18.0 * 18.0 {
                    a[y * n + x] = 255;
                }
            }
        }
        let c = trace_contour(n as u32, n as u32, &a).expect("disc");
        assert_eq!(c.polys.len(), 1);
        assert!(c.polys[0].len() < 80, "{}", c.polys[0].len());
        let b = poly_bounds(&c.polys).expect("bounds");
        assert!(b.width > 34.0 && b.width <= 40.0);
        // Fully opaque and fully clear images have no contour.
        assert!(trace_contour(2, 2, &[255; 4]).is_none());
        assert!(trace_contour(2, 2, &[0; 4]).is_none());
    }

    #[test]
    fn clip_keeps_inside() {
        let p = rounded_rect(10.0, 10.0, 0.0);
        let c = clip_to_rect(&p, Rect::new(5.0, 5.0, 10.0, 10.0));
        let b = poly_bounds(&[c]).expect("bounds");
        assert_eq!((b.x, b.y, b.width, b.height), (5.0, 5.0, 5.0, 5.0));
    }
}

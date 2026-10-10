//! Align and distribute: pure geometry that yields per-frame translations.
//! Clients turn the result into `set_frame_rect` commands in one batch.

use serde::{Deserialize, Serialize};

use crate::geom::Rect;
use crate::id::FrameId;

/// Which edge or centre line to align.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum AlignEdge {
    Left,
    /// Horizontal centres.
    Center,
    Right,
    Top,
    /// Vertical centres.
    Middle,
    Bottom,
}

/// Direction to distribute along.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DistributeAxis {
    Horizontal,
    Vertical,
}

/// Translation of one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Move {
    pub frame: FrameId,
    pub dx: f64,
    pub dy: f64,
}

/// Align the bounds of `items` to `edge` of `reference` (usually the union of
/// the selection, the page, or the margin box).
pub fn align(items: &[(FrameId, Rect)], edge: AlignEdge, reference: Rect) -> Vec<Move> {
    items
        .iter()
        .map(|(id, b)| {
            let (dx, dy) = match edge {
                AlignEdge::Left => (reference.left() - b.left(), 0.0),
                AlignEdge::Center => (reference.center().x - b.center().x, 0.0),
                AlignEdge::Right => (reference.right() - b.right(), 0.0),
                AlignEdge::Top => (0.0, reference.top() - b.top()),
                AlignEdge::Middle => (0.0, reference.center().y - b.center().y),
                AlignEdge::Bottom => (0.0, reference.bottom() - b.bottom()),
            };
            Move { frame: *id, dx, dy }
        })
        .collect()
}

/// Space items so the gaps between them are equal. With `within`, items are
/// spread across that span; otherwise the outermost items stay put.
pub fn distribute(
    items: &[(FrameId, Rect)],
    axis: DistributeAxis,
    within: Option<Rect>,
) -> Vec<Move> {
    if items.len() < 2 {
        return Vec::new();
    }
    let horizontal = axis == DistributeAxis::Horizontal;
    let start = |r: &Rect| if horizontal { r.left() } else { r.top() };
    let len = |r: &Rect| if horizontal { r.width } else { r.height };
    let mut sorted: Vec<(FrameId, Rect)> = items.to_vec();
    sorted.sort_by(|a, b| start(&a.1).total_cmp(&start(&b.1)));
    let (lo, hi) = match within {
        Some(w) if horizontal => (w.left(), w.right()),
        Some(w) => (w.top(), w.bottom()),
        None => {
            let last = &sorted[sorted.len() - 1].1;
            (start(&sorted[0].1), start(last) + len(last))
        }
    };
    let total: f64 = sorted.iter().map(|(_, r)| len(r)).sum();
    let gap = (hi - lo - total) / (sorted.len() - 1) as f64;
    let mut pos = lo;
    sorted
        .iter()
        .map(|(id, r)| {
            let d = pos - start(r);
            pos += len(r) + gap;
            if horizontal {
                Move {
                    frame: *id,
                    dx: d,
                    dy: 0.0,
                }
            } else {
                Move {
                    frame: *id,
                    dx: 0.0,
                    dy: d,
                }
            }
        })
        .collect()
}

/// Union of the bounds of `items`.
pub fn bounds_of(items: &[(FrameId, Rect)]) -> Option<Rect> {
    items.iter().map(|(_, r)| *r).reduce(|a, b| a.union(&b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::SeqIds;

    fn ids(n: usize) -> Vec<FrameId> {
        let mut s = SeqIds::new(1);
        (0..n).map(|_| FrameId::new(&mut s)).collect()
    }

    #[test]
    fn align_left_and_middle() {
        let f = ids(2);
        let items = vec![
            (f[0], Rect::new(10.0, 0.0, 20.0, 20.0)),
            (f[1], Rect::new(50.0, 40.0, 10.0, 40.0)),
        ];
        let reference = bounds_of(&items).unwrap();
        let m = align(&items, AlignEdge::Left, reference);
        assert_eq!((m[1].dx, m[1].dy), (-40.0, 0.0));
        let m = align(&items, AlignEdge::Middle, reference);
        assert_eq!(m[0].dy, 30.0);
        assert_eq!(m[1].dy, -20.0);
    }

    #[test]
    fn distribute_equal_gaps() {
        let f = ids(3);
        let items = vec![
            (f[0], Rect::new(0.0, 0.0, 10.0, 10.0)),
            (f[2], Rect::new(90.0, 0.0, 10.0, 10.0)),
            (f[1], Rect::new(20.0, 0.0, 20.0, 10.0)),
        ];
        let m = distribute(&items, DistributeAxis::Horizontal, None);
        let moved: Vec<(FrameId, f64)> = m.iter().map(|m| (m.frame, m.dx)).collect();
        // Span 0..100 holds 40pt of frames: two gaps of 30pt.
        assert_eq!(moved, vec![(f[0], 0.0), (f[1], 20.0), (f[2], 0.0)]);
    }
}

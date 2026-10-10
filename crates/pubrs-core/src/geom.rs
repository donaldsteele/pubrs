//! Geometry. All lengths are in points (1/72 inch). The coordinate origin is
//! the top-left corner of the page trim box, x grows right, y grows down.

use serde::{Deserialize, Serialize};

/// A position in points, origin top-left of the page, y down.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Point {
    /// Points from the left edge of the page.
    pub x: f64,
    /// Points from the top edge of the page.
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Rotate this point by `degrees` (clockwise on screen, since y is down) around `center`.
    pub fn rotated_about(self, center: Point, degrees: f64) -> Point {
        let (s, c) = degrees.to_radians().sin_cos();
        let dx = self.x - center.x;
        let dy = self.y - center.y;
        Point::new(center.x + dx * c - dy * s, center.y + dx * s + dy * c)
    }

    pub fn distance(self, other: Point) -> f64 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }
}

/// A width and height in points.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Size {
    /// Width in points.
    pub width: f64,
    /// Height in points.
    pub height: f64,
}

impl Size {
    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}

/// An axis-aligned rectangle in points: top-left corner plus size.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Rect {
    /// Left edge, points from the page's left edge.
    pub x: f64,
    /// Top edge, points from the page's top edge.
    pub y: f64,
    /// Width in points (>= 0).
    pub width: f64,
    /// Height in points (>= 0).
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn from_points(a: Point, b: Point) -> Self {
        let x = a.x.min(b.x);
        let y = a.y.min(b.y);
        Self::new(x, y, (a.x - b.x).abs(), (a.y - b.y).abs())
    }

    pub fn left(&self) -> f64 {
        self.x
    }
    pub fn top(&self) -> f64 {
        self.y
    }
    pub fn right(&self) -> f64 {
        self.x + self.width
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }
    pub fn center(&self) -> Point {
        Point::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
    pub fn origin(&self) -> Point {
        Point::new(self.x, self.y)
    }
    pub fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }

    pub fn translate(&self, dx: f64, dy: f64) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.width, self.height)
    }

    pub fn inset(&self, m: &Margins) -> Rect {
        Rect::new(
            self.x + m.left,
            self.y + m.top,
            (self.width - m.left - m.right).max(0.0),
            (self.height - m.top - m.bottom).max(0.0),
        )
    }

    pub fn outset(&self, d: f64) -> Rect {
        Rect::new(
            self.x - d,
            self.y - d,
            self.width + 2.0 * d,
            self.height + 2.0 * d,
        )
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x <= self.right() && p.y >= self.y && p.y <= self.bottom()
    }

    /// True if the rectangles overlap with positive area.
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }

    pub fn intersection(&self, o: &Rect) -> Option<Rect> {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = self.right().min(o.right());
        let y1 = self.bottom().min(o.bottom());
        (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
    }

    pub fn union(&self, o: &Rect) -> Rect {
        let x0 = self.x.min(o.x);
        let y0 = self.y.min(o.y);
        let x1 = self.right().max(o.right());
        let y1 = self.bottom().max(o.bottom());
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }

    /// True if `o` lies entirely inside `self`.
    pub fn contains_rect(&self, o: &Rect) -> bool {
        o.x >= self.x && o.y >= self.y && o.right() <= self.right() && o.bottom() <= self.bottom()
    }

    /// The four corners after rotating the rectangle by `degrees` about its centre.
    pub fn rotated_corners(&self, degrees: f64) -> [Point; 4] {
        let c = self.center();
        [
            Point::new(self.x, self.y),
            Point::new(self.right(), self.y),
            Point::new(self.right(), self.bottom()),
            Point::new(self.x, self.bottom()),
        ]
        .map(|p| p.rotated_about(c, degrees))
    }

    /// Axis-aligned bounds of the rectangle rotated by `degrees` about its centre.
    pub fn rotated_bounds(&self, degrees: f64) -> Rect {
        if degrees % 360.0 == 0.0 {
            return *self;
        }
        let pts = self.rotated_corners(degrees);
        let mut x0 = f64::INFINITY;
        let mut y0 = f64::INFINITY;
        let mut x1 = f64::NEG_INFINITY;
        let mut y1 = f64::NEG_INFINITY;
        for p in pts {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }

    pub fn is_finite(&self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
    }
}

/// Distances from each edge, in points. Used for page margins and frame insets.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Margins {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

impl Margins {
    pub const fn uniform(v: f64) -> Self {
        Self {
            top: v,
            right: v,
            bottom: v,
            left: v,
        }
    }

    pub fn is_valid(&self) -> bool {
        [self.top, self.right, self.bottom, self.left]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0)
    }
}

/// Points per inch.
pub const PT_PER_INCH: f64 = 72.0;
/// Points per millimetre.
pub const PT_PER_MM: f64 = 72.0 / 25.4;

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn rotation_bounds_of_square_at_45_degrees() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = r.rotated_bounds(45.0);
        let d = 10.0 * std::f64::consts::SQRT_2;
        assert!(close(b.width, d) && close(b.height, d));
        assert!(close(b.center().x, 5.0) && close(b.center().y, 5.0));
    }

    #[test]
    fn rotate_point_is_clockwise_with_y_down() {
        let p = Point::new(1.0, 0.0).rotated_about(Point::new(0.0, 0.0), 90.0);
        assert!(close(p.x, 0.0) && close(p.y, 1.0));
    }

    #[test]
    fn intersection_and_union() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, 5.0, 10.0, 10.0);
        assert_eq!(a.intersection(&b), Some(Rect::new(5.0, 5.0, 5.0, 5.0)));
        assert_eq!(a.union(&b), Rect::new(0.0, 0.0, 15.0, 15.0));
        assert!(!a.intersects(&Rect::new(10.0, 0.0, 5.0, 5.0)));
    }
}

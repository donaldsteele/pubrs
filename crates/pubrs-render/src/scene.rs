//! Backend-independent display list for one page.
//!
//! Every frame becomes a [`Node`] whose primitives are in frame-local
//! coordinates (origin at the top-left of the frame's unrotated rect) and
//! whose transform maps frame-local points to page points.

use std::sync::Arc;

use pubrs_core::{
    AssetId, Color, Crop, Dash, Document, FrameContent, FrameId, ImageFit, LineDirection, Owner,
    Rect, Shape,
};
use pubrs_layout::{DocumentLayout, GlyphRun};

/// A 2D affine transform `[a, b, c, d, e, f]`: x' = a·x + c·y + e, y' = b·x + d·y + f.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine(pub [f64; 6]);

impl Affine {
    pub const IDENTITY: Affine = Affine([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    pub fn translate(x: f64, y: f64) -> Self {
        Affine([1.0, 0.0, 0.0, 1.0, x, y])
    }

    pub fn scale(sx: f64, sy: f64) -> Self {
        Affine([sx, 0.0, 0.0, sy, 0.0, 0.0])
    }

    /// Clockwise rotation (y down) by `deg` degrees about the origin.
    pub fn rotate(deg: f64) -> Self {
        let (s, c) = deg.to_radians().sin_cos();
        Affine([c, s, -s, c, 0.0, 0.0])
    }

    /// Horizontal shear used for synthetic italics (`deg` leans glyph tops right).
    pub fn skew_x(deg: f64) -> Self {
        Affine([1.0, 0.0, -deg.to_radians().tan(), 1.0, 0.0, 0.0])
    }

    /// `self` after `other`: apply `other` first, then `self`.
    pub fn then(self, after: Affine) -> Affine {
        after.compose(self)
    }

    pub fn compose(self, o: Affine) -> Affine {
        let [a, b, c, d, e, f] = self.0;
        let [oa, ob, oc, od, oe, of] = o.0;
        Affine([
            a * oa + c * ob,
            b * oa + d * ob,
            a * oc + c * od,
            b * oc + d * od,
            a * oe + c * of + e,
            b * oe + d * of + f,
        ])
    }

    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }

    /// Uniform scale factor (sqrt of the determinant's magnitude).
    pub fn scale_factor(&self) -> f64 {
        let [a, b, c, d, ..] = self.0;
        (a * d - b * c).abs().sqrt()
    }
}

/// Geometry of a filled or stroked shape, frame-local.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Geom {
    Rect { w: f64, h: f64, radius: f64 },
    Ellipse { w: f64, h: f64 },
    Line { x0: f64, y0: f64, x1: f64, y1: f64 },
}

/// A primitive inside a frame node (frame-local coordinates).
#[derive(Debug, Clone)]
pub enum Prim {
    Fill {
        geom: Geom,
        color: Color,
    },
    Stroke {
        geom: Geom,
        color: Color,
        width: f64,
        dash: Dash,
    },
    /// An image: `transform` maps image pixels (or SVG user units) to
    /// frame-local points; drawing is clipped to `clip`.
    Image {
        asset: AssetId,
        transform: Affine,
        clip: Rect,
    },
    Glyphs(Arc<GlyphRun>),
    /// A filled rectangle (underlines).
    Rect {
        rect: Rect,
        color: Color,
    },
}

/// One frame's drawing.
#[derive(Debug, Clone)]
pub struct Node {
    pub frame: FrameId,
    /// Frame-local → page.
    pub transform: Affine,
    pub opacity: f64,
    pub prims: Vec<Prim>,
}

/// Everything drawn on one page, in paint order.
#[derive(Debug, Clone)]
pub struct PageScene {
    /// Trim size in points.
    pub width: f64,
    pub height: f64,
    pub bleed: f64,
    pub nodes: Vec<Node>,
}

/// Intrinsic size of an asset in its own units (pixels, or SVG user units).
pub trait AssetSizes {
    fn intrinsic_size(&self, asset: AssetId) -> Option<(f64, f64)>;
}

impl AssetSizes for Document {
    fn intrinsic_size(&self, asset: AssetId) -> Option<(f64, f64)> {
        self.assets
            .get(&asset)
            .map(|a| (f64::from(a.pixel_width), f64::from(a.pixel_height)))
    }
}

/// Where a (cropped) source image lands inside a `fw` × `fh` frame.
/// Returns the transform from source units to frame-local points and the
/// visible destination rect.
pub fn image_placement(
    src_w: f64,
    src_h: f64,
    crop: Crop,
    fit: ImageFit,
    fw: f64,
    fh: f64,
) -> (Affine, Rect) {
    let sx0 = crop.left * src_w;
    let sy0 = crop.top * src_h;
    let sw = (src_w * (1.0 - crop.left - crop.right)).max(1e-6);
    let sh = (src_h * (1.0 - crop.top - crop.bottom)).max(1e-6);
    let (kx, ky) = match fit {
        ImageFit::Stretch => (fw / sw, fh / sh),
        ImageFit::Fill => {
            let s = (fw / sw).max(fh / sh);
            (s, s)
        }
        ImageFit::Fit => {
            let s = (fw / sw).min(fh / sh);
            (s, s)
        }
    };
    let dw = sw * kx;
    let dh = sh * ky;
    let dx = (fw - dw) / 2.0;
    let dy = (fh - dh) / 2.0;
    let t = Affine([kx, 0.0, 0.0, ky, dx - sx0 * kx, dy - sy0 * ky]);
    let frame = Rect::new(0.0, 0.0, fw, fh);
    let dest = Rect::new(dx, dy, dw, dh);
    (
        t,
        dest.intersection(&frame)
            .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
    )
}

/// Frame-local → page transform for a frame rect and rotation.
pub fn frame_transform(rect: Rect, rotation: f64) -> Affine {
    let c = rect.center();
    let local_to_center = Affine::translate(-rect.width / 2.0, -rect.height / 2.0);
    local_to_center
        .then(Affine::rotate(rotation))
        .then(Affine::translate(c.x, c.y))
}

fn geom_of(shape: Shape, w: f64, h: f64) -> Geom {
    match shape {
        Shape::Rectangle { corner_radius } => Geom::Rect {
            w,
            h,
            radius: corner_radius.min(w / 2.0).min(h / 2.0).max(0.0),
        },
        Shape::Ellipse => Geom::Ellipse { w, h },
        Shape::Line { direction } => match direction {
            LineDirection::DownRight => Geom::Line {
                x0: 0.0,
                y0: 0.0,
                x1: w,
                y1: h,
            },
            LineDirection::UpRight => Geom::Line {
                x0: 0.0,
                y0: h,
                x1: w,
                y1: 0.0,
            },
        },
    }
}

/// Build the scene for page `index` (master frames first, then the page's).
pub fn build_page_scene(
    doc: &Document,
    layout: &DocumentLayout,
    index: usize,
) -> Option<PageScene> {
    let page = doc.pages.get(index)?;
    let mut nodes = Vec::new();
    if let Some(m) = page.master
        && doc.master(m).is_some()
    {
        for id in doc.paint_order(Owner::Master(m)) {
            if let Some(n) = frame_node(doc, layout, id) {
                nodes.push(n);
            }
        }
    }
    for id in doc.paint_order(Owner::Page(page.id)) {
        if let Some(n) = frame_node(doc, layout, id) {
            nodes.push(n);
        }
    }
    Some(PageScene {
        width: doc.setup.size.width,
        height: doc.setup.size.height,
        bleed: doc.setup.bleed,
        nodes,
    })
}

/// The display node for one (non-group) frame.
pub fn frame_node(doc: &Document, layout: &DocumentLayout, id: FrameId) -> Option<Node> {
    let f = doc.frame(id)?;
    let (w, h) = (f.rect.width, f.rect.height);
    let mut prims = Vec::new();
    let outline = match &f.content {
        FrameContent::Shape(s) => geom_of(s.shape, w, h),
        _ => Geom::Rect { w, h, radius: 0.0 },
    };
    if let Some(fill) = f.style.fill
        && !matches!(outline, Geom::Line { .. })
    {
        prims.push(Prim::Fill {
            geom: outline,
            color: fill,
        });
    }
    match &f.content {
        FrameContent::Image(img) => {
            if let Some(asset) = img.asset
                && let Some((sw, sh)) = doc.intrinsic_size(asset)
            {
                let (transform, clip) = image_placement(sw, sh, img.crop, img.fit, w, h);
                if clip.width > 0.0 && clip.height > 0.0 {
                    prims.push(Prim::Image {
                        asset,
                        transform,
                        clip,
                    });
                }
            }
        }
        FrameContent::Text(_) => {
            if let Some(ft) = layout.frame(id) {
                for line in &ft.lines {
                    for run in &line.runs {
                        prims.push(Prim::Glyphs(Arc::new(run.clone())));
                    }
                    for d in &line.decorations {
                        prims.push(Prim::Rect {
                            rect: d.rect,
                            color: d.color,
                        });
                    }
                }
            }
        }
        FrameContent::Group(_) => return None,
        FrameContent::Shape(_) => {}
    }
    if let Some(s) = f.style.stroke
        && s.width > 0.0
    {
        prims.push(Prim::Stroke {
            geom: outline,
            color: s.color,
            width: s.width,
            dash: s.dash,
        });
    }
    if prims.is_empty() || f.style.opacity <= 0.0 {
        return None;
    }
    Some(Node {
        frame: id,
        transform: frame_transform(f.rect, f.rotation),
        opacity: f.style.opacity.clamp(0.0, 1.0),
        prims,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
    }

    #[test]
    fn frame_transform_rotates_about_centre() {
        let t = frame_transform(Rect::new(10.0, 20.0, 100.0, 50.0), 90.0);
        // Centre stays put.
        assert!(close(t.apply(50.0, 25.0), (60.0, 45.0)));
        // Top-left corner rotates clockwise around the centre.
        assert!(close(t.apply(0.0, 0.0), (85.0, -5.0)));
        let id = frame_transform(Rect::new(10.0, 20.0, 100.0, 50.0), 0.0);
        assert!(close(id.apply(0.0, 0.0), (10.0, 20.0)));
    }

    #[test]
    fn fill_covers_frame_and_fit_letterboxes() {
        // 200x100 image into a 100x100 frame.
        let (t, clip) =
            image_placement(200.0, 100.0, Crop::default(), ImageFit::Fill, 100.0, 100.0);
        assert_eq!(clip, Rect::new(0.0, 0.0, 100.0, 100.0));
        assert!(
            close(t.apply(100.0, 50.0), (50.0, 50.0)),
            "image centre at frame centre"
        );
        let (_, clip) = image_placement(200.0, 100.0, Crop::default(), ImageFit::Fit, 100.0, 100.0);
        assert_eq!(clip, Rect::new(0.0, 25.0, 100.0, 50.0));
        let crop = Crop {
            left: 0.5,
            ..Default::default()
        };
        let (t, _) = image_placement(200.0, 100.0, crop, ImageFit::Stretch, 100.0, 100.0);
        assert!(
            close(t.apply(100.0, 0.0), (0.0, 0.0)),
            "crop starts at x=100px"
        );
    }
}

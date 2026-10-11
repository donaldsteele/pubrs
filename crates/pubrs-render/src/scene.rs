//! Backend-independent display list for one page.
//!
//! Every frame becomes a [`Node`] whose primitives are in frame-local
//! coordinates (origin at the top-left of the frame's unrotated rect) and
//! whose transform maps frame-local points to page points.

use std::sync::{Arc, OnceLock};

use resvg::tiny_skia as ts;

use pubrs_core::{
    AssetId, Color, Crop, Dash, Document, FrameContent, FrameId, GradientKind, ImageFit,
    LineDirection, Owner, Rect, ResolvedInk, ResolvedPaint, Shape,
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

/// Where a gradient runs, in frame-local points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GradientAxis {
    Linear { x0: f64, y0: f64, x1: f64, y1: f64 },
    Radial { cx: f64, cy: f64, r: f64 },
}

/// The gradient line for a `w` x `h` box. Linear gradients run through the
/// centre at `angle` degrees clockwise from left-to-right and exactly span
/// the box; radial gradients run from the centre to the farthest corner.
pub fn gradient_axis(kind: GradientKind, angle: f64, w: f64, h: f64) -> GradientAxis {
    let (cx, cy) = (w / 2.0, h / 2.0);
    match kind {
        GradientKind::Linear => {
            let (s, c) = angle.to_radians().sin_cos();
            let half = (w * c.abs() + h * s.abs()) / 2.0;
            GradientAxis::Linear {
                x0: cx - c * half,
                y0: cy - s * half,
                x1: cx + c * half,
                y1: cy + s * half,
            }
        }
        GradientKind::Radial => GradientAxis::Radial {
            cx,
            cy,
            r: (w.hypot(h) / 2.0).max(f64::EPSILON),
        },
    }
}

impl Geom {
    /// Width and height of the shape's box.
    pub fn size(&self) -> (f64, f64) {
        match *self {
            Geom::Rect { w, h, .. } | Geom::Ellipse { w, h } => (w, h),
            Geom::Line { x0, y0, x1, y1 } => ((x1 - x0).abs(), (y1 - y0).abs()),
        }
    }
}

/// One segment of a [`PathData`] (frame-local points).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathSeg {
    Move(f32, f32),
    Line(f32, f32),
    Quad(f32, f32, f32, f32),
    Cubic(f32, f32, f32, f32, f32, f32),
    Close,
}

/// A vector outline (warped WordArt letters). Backends convert it to their
/// own path type; the tiny-skia form is built once and kept.
#[derive(Debug, Default)]
pub struct PathData {
    pub segs: Vec<PathSeg>,
    ts: OnceLock<Option<ts::Path>>,
}

impl Clone for PathData {
    fn clone(&self) -> Self {
        Self::new(self.segs.clone())
    }
}

impl PartialEq for PathData {
    fn eq(&self, o: &Self) -> bool {
        self.segs == o.segs
    }
}

impl PathData {
    pub fn new(segs: Vec<PathSeg>) -> Self {
        Self {
            segs,
            ts: OnceLock::new(),
        }
    }

    /// The tiny-skia path (None when empty or degenerate).
    pub fn ts_path(&self) -> Option<&ts::Path> {
        self.ts
            .get_or_init(|| {
                let mut pb = ts::PathBuilder::new();
                for s in &self.segs {
                    match *s {
                        PathSeg::Move(x, y) => pb.move_to(x, y),
                        PathSeg::Line(x, y) => pb.line_to(x, y),
                        PathSeg::Quad(a, b, x, y) => pb.quad_to(a, b, x, y),
                        PathSeg::Cubic(a, b, c, d, x, y) => pb.cubic_to(a, b, c, d, x, y),
                        PathSeg::Close => pb.close(),
                    }
                }
                pb.finish()
            })
            .as_ref()
    }

    /// Box of all points, control points included: `(left, top, right, bottom)`.
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        let (mut l, mut t, mut r, mut b) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        let mut any = false;
        let mut add = |x: f32, y: f32| {
            any = true;
            l = l.min(x);
            r = r.max(x);
            t = t.min(y);
            b = b.max(y);
        };
        for s in &self.segs {
            match *s {
                PathSeg::Move(x, y) | PathSeg::Line(x, y) => add(x, y),
                PathSeg::Quad(a, b2, x, y) => {
                    add(a, b2);
                    add(x, y);
                }
                PathSeg::Cubic(a, b2, c, d, x, y) => {
                    add(a, b2);
                    add(c, d);
                    add(x, y);
                }
                PathSeg::Close => {}
            }
        }
        any.then(|| (f64::from(l), f64::from(t), f64::from(r), f64::from(b)))
    }
}

/// The outline of a [`Prim::Path`].
#[derive(Debug, Clone, PartialEq)]
pub struct PathStroke {
    pub color: ResolvedInk,
    pub width: f64,
    pub dash: Dash,
}

/// A drop shadow of a whole node: its silhouette offset, blurred and tinted
/// (ADR 0020). Offsets and blur are page points.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeShadow {
    pub dx: f64,
    pub dy: f64,
    pub blur: f64,
    /// Tint (alpha included).
    pub color: ResolvedInk,
    pub opacity: f64,
}

/// A primitive inside a frame node (frame-local coordinates).
#[derive(Debug, Clone)]
pub enum Prim {
    Fill {
        geom: Geom,
        paint: ResolvedPaint,
    },
    Stroke {
        geom: Geom,
        color: ResolvedInk,
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
    /// A glyph run with its ink resolved against the document's swatches
    /// (`run.color` is the unresolved reference).
    Glyphs(Arc<GlyphRun>, ResolvedInk),
    /// A filled rectangle (underlines).
    Rect {
        rect: Rect,
        color: ResolvedInk,
    },
    /// A vector outline: warped WordArt letters. The path is relative to
    /// `origin` (frame-local) and `size` is its box, which a gradient fill
    /// spans; `text` is the text the outline spells (the
    /// tagged PDF's ActualText).
    Path {
        path: Arc<PathData>,
        fill: Option<ResolvedPaint>,
        stroke: Option<PathStroke>,
        origin: (f64, f64),
        size: (f64, f64),
        text: Option<Arc<str>>,
    },
    /// A parsed SVG (BorderArt): `transform` maps SVG user units to
    /// frame-local points; drawing is clipped to `clip`.
    Svg {
        tree: Arc<usvg::Tree>,
        transform: Affine,
        clip: Rect,
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
    /// Drop shadow under the whole node.
    pub shadow: Option<NodeShadow>,
    /// Clip in page points (imposed sheets clip each page to its cell).
    /// `None` for ordinary pages. The GPU backend ignores it.
    pub clip: Option<Rect>,
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
    let (t, dest) = pubrs_core::image_placement(src_w, src_h, crop, fit, fw, fh);
    (Affine(t), dest)
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

/// Options for scene building.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SceneOptions {
    /// Draw empty picture frames as grey boxes with a cross (previews and
    /// the editor; never in exports).
    pub show_empty_images: bool,
}

/// Build the scene for page `index` (master frames first, then the page's).
pub fn build_page_scene(
    doc: &Document,
    layout: &DocumentLayout,
    index: usize,
) -> Option<PageScene> {
    build_page_scene_with(doc, layout, index, SceneOptions::default())
}

/// Like [`build_page_scene`] with options.
pub fn build_page_scene_with(
    doc: &Document,
    layout: &DocumentLayout,
    index: usize,
    opts: SceneOptions,
) -> Option<PageScene> {
    let page = doc.pages.get(index)?;
    let mut nodes = Vec::new();
    if let Some(m) = page.master
        && doc.master(m).is_some()
    {
        for id in doc.paint_order(Owner::Master(m)) {
            if let Some(n) = frame_node_with(doc, layout, id, opts) {
                nodes.push(n);
            }
        }
    }
    for id in doc.paint_order(Owner::Page(page.id)) {
        if let Some(n) = frame_node_with(doc, layout, id, opts) {
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
    frame_node_with(doc, layout, id, SceneOptions::default())
}

/// Like [`frame_node`] with options.
pub fn frame_node_with(
    doc: &Document,
    layout: &DocumentLayout,
    id: FrameId,
    opts: SceneOptions,
) -> Option<Node> {
    let f = doc.frame(id)?;
    let (w, h) = (f.rect.width, f.rect.height);
    let mut prims = Vec::new();
    let outline = match &f.content {
        FrameContent::Shape(s) => geom_of(s.shape, w, h),
        _ => Geom::Rect { w, h, radius: 0.0 },
    };
    if let Some(fill) = &f.style.fill
        && !matches!(outline, Geom::Line { .. })
    {
        prims.push(Prim::Fill {
            geom: outline,
            paint: doc.resolve_paint(fill),
        });
    }
    match &f.content {
        FrameContent::Image(img) => {
            if img.asset.is_none() && opts.show_empty_images {
                prims.push(Prim::Fill {
                    geom: Geom::Rect { w, h, radius: 0.0 },
                    paint: ResolvedPaint::Solid(Color::rgb(0xE5, 0xE7, 0xEB).into()),
                });
                for (x0, y0, x1, y1) in [(0.0, 0.0, w, h), (0.0, h, w, 0.0)] {
                    prims.push(Prim::Stroke {
                        geom: Geom::Line { x0, y0, x1, y1 },
                        color: Color::rgb(0x9C, 0xA3, 0xAF).into(),
                        width: 1.0,
                        dash: Dash::Solid,
                    });
                }
            }
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
                        prims.push(Prim::Glyphs(
                            Arc::new(run.clone()),
                            doc.resolve_ink(run.color),
                        ));
                    }
                    for d in &line.decorations {
                        prims.push(Prim::Rect {
                            rect: d.rect,
                            color: doc.resolve_ink(d.color),
                        });
                    }
                }
            }
        }
        FrameContent::Table(table) => table_prims(doc, layout, id, table, &mut prims),
        FrameContent::Group(_) => return None,
        FrameContent::Shape(_) => {}
        FrameContent::WordArt(art) => {
            if let Some(wl) = layout.wordart(id)
                && let Some(wt) = crate::warp::warp_text(wl, art, w, h)
            {
                let (origin, size) = (wt.origin, wt.size);
                prims.push(Prim::Path {
                    origin,
                    path: Arc::new(wt.path),
                    fill: Some(doc.resolve_paint(&art.fill)),
                    stroke: art
                        .outline
                        .as_ref()
                        .filter(|o| o.width > 0.0)
                        .map(|o| PathStroke {
                            color: doc.resolve_solid(&o.color),
                            width: o.width,
                            dash: o.dash,
                        }),
                    size,
                    text: Some(Arc::from(art.text.as_str())),
                });
            }
        }
    }
    if let Some(s) = &f.style.stroke
        && s.width > 0.0
    {
        prims.push(Prim::Stroke {
            geom: outline,
            color: doc.resolve_solid(&s.color),
            width: s.width,
            dash: s.dash,
        });
    }
    if let Some(b) = &f.style.border_art {
        border_art_prims(&b.pattern, b.width, w, h, &mut prims);
    }
    if prims.is_empty() || f.style.opacity <= 0.0 {
        return None;
    }
    let shadow = f
        .style
        .shadow
        .as_ref()
        .or(match &f.content {
            FrameContent::WordArt(a) => a.shadow.as_ref(),
            _ => None,
        })
        .filter(|s| s.opacity > 0.0)
        .map(|s| NodeShadow {
            dx: s.offset_x,
            dy: s.offset_y,
            blur: s.blur,
            color: doc.resolve_solid(&s.color),
            opacity: s.opacity,
        });
    Some(Node {
        frame: id,
        transform: frame_transform(f.rect, f.rotation),
        opacity: f.style.opacity.clamp(0.0, 1.0),
        prims,
        shadow,
        clip: None,
    })
}

// ---------------------------------------------------------------------------
// Tables

/// A glyph run moved by (`dx`, `dy`).
fn shifted_run(run: &GlyphRun, dx: f64, dy: f64) -> GlyphRun {
    let mut r = run.clone();
    for g in &mut r.glyphs {
        g.x += dx;
        g.y += dy;
    }
    r
}

/// One border segment on a grid line, before overlaps are resolved.
struct Edge {
    /// Order of the cell side that made it (pieces keep their parent's).
    seq: usize,
    horizontal: bool,
    /// Coordinate of the line (y for horizontal, x for vertical).
    at: f64,
    from: f64,
    to: f64,
    stroke: pubrs_core::Stroke,
}

/// The primitives of a table: cell fills, then each cell's lines at its
/// rectangle, then borders. A border two cells share is drawn once, the
/// later cell's winning where both set one (ADR 0024).
fn table_prims(
    doc: &Document,
    layout: &DocumentLayout,
    id: FrameId,
    table: &pubrs_core::TableFrame,
    prims: &mut Vec<Prim>,
) {
    let Some(tl) = layout.table(id) else { return };
    for (cell, cl) in table.cells.iter().zip(&tl.cells) {
        if let Some(fill) = &cell.fill
            && cl.rect.width > 0.0
            && cl.rect.height > 0.0
        {
            prims.push(Prim::Rect {
                rect: cl.rect,
                color: doc.resolve_solid(fill),
            });
        }
    }
    for (cell, cl) in table.cells.iter().zip(&tl.cells) {
        let Some(ft) = layout.cell_text(id, cell.story) else {
            continue;
        };
        let (dx, dy) = (cl.rect.x, cl.rect.y + cl.dy);
        for line in &ft.lines {
            for run in &line.runs {
                prims.push(Prim::Glyphs(
                    Arc::new(shifted_run(run, dx, dy)),
                    doc.resolve_ink(run.color),
                ));
            }
            for d in &line.decorations {
                prims.push(Prim::Rect {
                    rect: Rect::new(d.rect.x + dx, d.rect.y + dy, d.rect.width, d.rect.height),
                    color: doc.resolve_ink(d.color),
                });
            }
        }
    }
    // Segments bucketed by grid line, so a side only meets the segments on
    // its own line.
    let key = |v: f64| (v * 1000.0).round() as i64;
    let mut lines: std::collections::HashMap<(bool, i64), Vec<Edge>> =
        std::collections::HashMap::new();
    let mut seq = 0usize;
    for (cell, cl) in table.cells.iter().zip(&tl.cells) {
        let r = cl.rect;
        if r.height <= 0.0 && r.width <= 0.0 {
            continue;
        }
        let sides = [
            (true, r.y, r.x, r.right()),
            (false, r.right(), r.y, r.bottom()),
            (true, r.bottom(), r.x, r.right()),
            (false, r.x, r.y, r.bottom()),
        ];
        for (side, (horizontal, at, from, to)) in sides.into_iter().enumerate() {
            let Some(stroke) = cell.borders[side].as_ref().filter(|s| s.width > 0.0) else {
                continue;
            };
            // The later cell wins: cut its span out of what is there.
            let line = lines.entry((horizontal, key(at))).or_default();
            let mut kept: Vec<Edge> = Vec::with_capacity(line.len() + 1);
            for e in line.drain(..) {
                if e.to <= from || e.from >= to {
                    kept.push(e);
                    continue;
                }
                if e.from < from {
                    kept.push(Edge {
                        to: from,
                        stroke: e.stroke.clone(),
                        ..e
                    });
                }
                if e.to > to {
                    kept.push(Edge {
                        from: to,
                        stroke: e.stroke.clone(),
                        ..e
                    });
                }
            }
            kept.push(Edge {
                seq,
                horizontal,
                at,
                from,
                to,
                stroke: stroke.clone(),
            });
            seq += 1;
            *line = kept;
        }
    }
    let mut edges: Vec<Edge> = lines.into_values().flatten().collect();
    edges.sort_by(|a, b| a.seq.cmp(&b.seq).then(a.from.total_cmp(&b.from)));
    for e in edges {
        // Centred on the grid line; ends reach half a width so corners close.
        let half = e.stroke.width / 2.0;
        let geom = if e.horizontal {
            Geom::Line {
                x0: e.from - half,
                y0: e.at,
                x1: e.to + half,
                y1: e.at,
            }
        } else {
            Geom::Line {
                x0: e.at,
                y0: e.from - half,
                x1: e.at,
                y1: e.to + half,
            }
        };
        prims.push(Prim::Stroke {
            geom,
            color: doc.resolve_solid(&e.stroke.color),
            width: e.stroke.width,
            dash: e.stroke.dash,
        });
    }
}

// ---------------------------------------------------------------------------
// BorderArt

/// A bundled BorderArt pattern: the tile repeated along an edge and the
/// corner, both 32 x 32 user units.
pub struct BorderPattern {
    pub tile: Arc<usvg::Tree>,
    pub corner: Arc<usvg::Tree>,
}

macro_rules! border_svgs {
    ($($name:literal),*) => {
        [$((
            $name,
            include_bytes!(concat!("../../../assets/borderart/", $name, "-tile.svg")) as &[u8],
            include_bytes!(concat!("../../../assets/borderart/", $name, "-corner.svg")) as &[u8],
        )),*]
    };
}

/// The pattern called `name` (one of [`pubrs_core::BORDER_ART_PATTERNS`]).
pub fn border_pattern(name: &str) -> Option<&'static BorderPattern> {
    static ALL: OnceLock<Vec<(&'static str, BorderPattern)>> = OnceLock::new();
    let all = ALL.get_or_init(|| {
        let src = border_svgs!(
            "stars",
            "hearts",
            "dots",
            "waves",
            "zigzag",
            "diamonds",
            "leaves",
            "ropes",
            "checks",
            "scallops",
            "arrows",
            "snowflakes"
        );
        let opts = usvg::Options::default();
        src.iter()
            .filter_map(|(n, tile, corner)| {
                Some((
                    *n,
                    BorderPattern {
                        tile: Arc::new(usvg::Tree::from_data(tile, &opts).ok()?),
                        corner: Arc::new(usvg::Tree::from_data(corner, &opts).ok()?),
                    },
                ))
            })
            .collect()
    });
    all.iter().find(|(n, _)| *n == name).map(|(_, p)| p)
}

/// Side length of the pattern SVGs in user units.
const BORDER_UNITS: f64 = 32.0;

/// Quarter turns clockwise as an exact matrix (no trigonometry), applied
/// after `translate`: svg units → frame-local.
fn quarter(turns: u8, tx: f64, ty: f64, sx: f64, sy: f64) -> Affine {
    let (a, b, c, d) = match turns % 4 {
        0 => (1.0, 0.0, 0.0, 1.0),
        1 => (0.0, 1.0, -1.0, 0.0),
        2 => (-1.0, 0.0, 0.0, -1.0),
        _ => (0.0, -1.0, 1.0, 0.0),
    };
    // rotate(scale(p)) + t
    Affine([a * sx, b * sx, c * sy, d * sy, tx, ty])
}

/// Tiles of a border `width` points wide along the inside of a `w` x `h`
/// frame: four corners and, along each edge, a whole number of tiles scaled
/// to fit (nearest to square). Frames too small for two corners get none.
pub fn border_art_prims(pattern: &str, width: f64, w: f64, h: f64, out: &mut Vec<Prim>) {
    let Some(pat) = border_pattern(pattern) else {
        return;
    };
    let bw = width;
    if !(bw > 0.0 && w >= 2.0 * bw && h >= 2.0 * bw) {
        return;
    }
    let k = bw / BORDER_UNITS;
    let mut put = |tree: &Arc<usvg::Tree>, m: Affine| {
        // The mapped unit square is axis-aligned (quarter turns).
        let (x0, y0) = m.apply(0.0, 0.0);
        let (x1, y1) = m.apply(BORDER_UNITS, BORDER_UNITS);
        let clip = Rect::new(x0.min(x1), y0.min(y1), (x1 - x0).abs(), (y1 - y0).abs());
        out.push(Prim::Svg {
            tree: tree.clone(),
            transform: m,
            clip,
        });
    };
    let corners = [(0u8, 0.0, 0.0), (1, w, 0.0), (2, w, h), (3, 0.0, h)];
    for (turns, x, y) in corners {
        put(&pat.corner, quarter(turns, x, y, k, k));
    }
    // Edges: (turns, run length, start corner, direction along the edge).
    let edges = [
        (0u8, w - 2.0 * bw, (bw, 0.0), (1.0, 0.0)),
        (1, h - 2.0 * bw, (w, bw), (0.0, 1.0)),
        (2, w - 2.0 * bw, (w - bw, h), (-1.0, 0.0)),
        (3, h - 2.0 * bw, (0.0, h - bw), (0.0, -1.0)),
    ];
    for (turns, len, (x0, y0), (dx, dy)) in edges {
        let n = (len / bw).round().max(1.0);
        if len < 0.5 {
            continue;
        }
        let step = len / n;
        for i in 0..n as usize {
            let off = i as f64 * step;
            put(
                &pat.tile,
                quarter(turns, x0 + dx * off, y0 + dy * off, step / BORDER_UNITS, k),
            );
        }
    }
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

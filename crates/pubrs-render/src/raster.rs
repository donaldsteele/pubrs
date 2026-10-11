//! Software rasteriser (tiny-skia). Deterministic: used for PNG export,
//! previews, golden tests and as the GUI fallback when no GPU is available.

use std::collections::HashMap;

use pubrs_core::{Color, Dash, ResolvedPaint};
use resvg::tiny_skia as ts;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::OutlinePen;
use skrifa::{FontRef, GlyphId, MetadataProvider};

use crate::images::{ImageContent, ImageResolver};
use crate::scene::{Affine, Geom, GradientAxis, Node, PageScene, Prim, gradient_axis};

/// Largest raster dimension accepted, in pixels.
pub const MAX_PIXELS_PER_SIDE: u32 = 16_384;

/// Raster output settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RasterOptions {
    /// Resolution in dots per inch (72 = one pixel per point).
    pub dpi: f64,
    /// Include the bleed area around the trim box.
    pub include_bleed: bool,
    /// Paper colour; `None` leaves the background transparent.
    pub background: Option<Color>,
}

impl Default for RasterOptions {
    fn default() -> Self {
        Self {
            dpi: 150.0,
            include_bleed: false,
            background: Some(Color::WHITE),
        }
    }
}

/// Raster rendering errors.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum RenderError {
    #[error("output would be {0}x{1} pixels; lower the dpi (max {MAX_PIXELS_PER_SIDE} per side)")]
    TooLarge(u32, u32),
    #[error("invalid render size")]
    InvalidSize,
    #[error("PNG encoding failed: {0}")]
    Png(String),
}

/// Pixel dimensions of a page rendered with `opts`.
pub fn pixel_size(scene: &PageScene, opts: &RasterOptions) -> (u32, u32) {
    let b = if opts.include_bleed { scene.bleed } else { 0.0 };
    let s = opts.dpi / 72.0;
    (
        ((scene.width + 2.0 * b) * s).ceil().max(1.0) as u32,
        ((scene.height + 2.0 * b) * s).ceil().max(1.0) as u32,
    )
}

/// Page → device transform for `opts`.
pub fn device_transform(scene: &PageScene, opts: &RasterOptions) -> Affine {
    let b = if opts.include_bleed { scene.bleed } else { 0.0 };
    let s = opts.dpi / 72.0;
    Affine::translate(b, b).then(Affine::scale(s, s))
}

pub(crate) fn to_ts(a: Affine) -> ts::Transform {
    let [sx, ky, kx, sy, tx, ty] = a.0.map(|v| v as f32);
    ts::Transform::from_row(sx, ky, kx, sy, tx, ty)
}

/// A fill paint in frame-local space (the fill transform maps it to device
/// space). A gradient that cannot be built draws as its first stop.
fn fill_paint_of(p: &ResolvedPaint, geom: &Geom) -> ts::Paint<'static> {
    let ResolvedPaint::Gradient(g) = p else {
        return paint_of(p.solid_rgba());
    };
    let stops: Vec<ts::GradientStop> = g
        .stops
        .iter()
        .map(|(o, c)| {
            ts::GradientStop::new(
                *o as f32,
                ts::Color::from_rgba8(c.rgba.r, c.rgba.g, c.rgba.b, c.rgba.a),
            )
        })
        .collect();
    let (w, h) = geom.size();
    let shader = match gradient_axis(g.kind, g.angle, w, h) {
        GradientAxis::Linear { x0, y0, x1, y1 } => ts::LinearGradient::new(
            ts::Point::from_xy(x0 as f32, y0 as f32),
            ts::Point::from_xy(x1 as f32, y1 as f32),
            stops,
            ts::SpreadMode::Pad,
            ts::Transform::identity(),
        ),
        GradientAxis::Radial { cx, cy, r } => {
            let c = ts::Point::from_xy(cx as f32, cy as f32);
            ts::RadialGradient::new(
                c,
                0.0,
                c,
                r as f32,
                stops,
                ts::SpreadMode::Pad,
                ts::Transform::identity(),
            )
        }
    };
    match shader {
        Some(shader) => ts::Paint {
            shader,
            anti_alias: true,
            ..Default::default()
        },
        None => paint_of(p.solid_rgba()),
    }
}

fn paint_of(c: Color) -> ts::Paint<'static> {
    let mut p = ts::Paint::default();
    p.set_color_rgba8(c.r, c.g, c.b, c.a);
    p.anti_alias = true;
    p
}

/// Path for a shape geometry (frame-local).
pub(crate) fn geom_path(g: Geom) -> Option<ts::Path> {
    match g {
        Geom::Rect { w, h, radius } => {
            let (w, h, r) = (w as f32, h as f32, radius as f32);
            if w <= 0.0 || h <= 0.0 {
                return None;
            }
            if r <= 0.0 {
                return Some(ts::PathBuilder::from_rect(ts::Rect::from_xywh(
                    0.0, 0.0, w, h,
                )?));
            }
            let k = 0.552_284_8 * r;
            let mut pb = ts::PathBuilder::new();
            pb.move_to(r, 0.0);
            pb.line_to(w - r, 0.0);
            pb.cubic_to(w - r + k, 0.0, w, r - k, w, r);
            pb.line_to(w, h - r);
            pb.cubic_to(w, h - r + k, w - r + k, h, w - r, h);
            pb.line_to(r, h);
            pb.cubic_to(r - k, h, 0.0, h - r + k, 0.0, h - r);
            pb.line_to(0.0, r);
            pb.cubic_to(0.0, r - k, r - k, 0.0, r, 0.0);
            pb.close();
            pb.finish()
        }
        Geom::Ellipse { w, h } => {
            ts::PathBuilder::from_oval(ts::Rect::from_xywh(0.0, 0.0, w as f32, h as f32)?)
        }
        Geom::Line { x0, y0, x1, y1 } => {
            let mut pb = ts::PathBuilder::new();
            pb.move_to(x0 as f32, y0 as f32);
            pb.line_to(x1 as f32, y1 as f32);
            pb.finish()
        }
    }
}

/// Dash pattern in user units for a stroke width.
pub fn dash_pattern(dash: Dash, width: f64) -> Option<Vec<f64>> {
    let w = width.max(0.5);
    match dash {
        Dash::Solid => None,
        Dash::Dashed => Some(vec![3.0 * w, 2.0 * w]),
        Dash::Dotted => Some(vec![w, 1.5 * w]),
    }
}

struct PathPen(ts::PathBuilder);

impl OutlinePen for PathPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, -y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, -y);
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.0.quad_to(cx0, -cy0, x, -y);
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0.cubic_to(cx0, -cy0, cx1, -cy1, x, -y);
    }
    fn close(&mut self) {
        self.0.close();
    }
}

type FontKey = (u64, u32);

/// Glyph outlines in font units (y down), cached per font.
#[derive(Default)]
pub struct GlyphCache {
    outlines: HashMap<(FontKey, u32), Option<ts::Path>>,
    upem: HashMap<FontKey, f32>,
}

impl GlyphCache {
    /// Units per em of a font (1000 if unreadable).
    pub fn upem(&mut self, key: FontKey, bytes: &[u8]) -> f32 {
        *self.upem.entry(key).or_insert_with(|| {
            FontRef::from_index(bytes, key.1)
                .map(|f| {
                    f32::from(
                        f.metrics(Size::unscaled(), LocationRef::default())
                            .units_per_em,
                    )
                })
                .unwrap_or(1000.0)
        })
    }

    /// Outline of a glyph in font units, y down, origin on the baseline.
    pub fn outline(&mut self, key: FontKey, bytes: &[u8], gid: u32) -> Option<&ts::Path> {
        self.outlines
            .entry((key, gid))
            .or_insert_with(|| {
                let font = FontRef::from_index(bytes, key.1).ok()?;
                let glyph = font.outline_glyphs().get(GlyphId::new(gid))?;
                let mut pen = PathPen(ts::PathBuilder::new());
                glyph.draw(Size::unscaled(), &mut pen).ok()?;
                pen.0.finish()
            })
            .as_ref()
    }

    /// Load every outline and units-per-em a scene draws, so drawing can
    /// read the cache without mutating it (and share it across threads).
    pub fn prepare(&mut self, scene: &PageScene) {
        for node in &scene.nodes {
            for prim in &node.prims {
                if let Prim::Glyphs(run, _) = prim {
                    let (key, bytes) = (run.font.key(), run.font.bytes());
                    self.upem(key, bytes);
                    for g in &run.glyphs {
                        self.outline(key, bytes, g.id);
                    }
                }
            }
        }
    }

    pub(crate) fn cached_upem(&self, key: FontKey) -> f32 {
        self.upem.get(&key).copied().unwrap_or(1000.0)
    }

    pub(crate) fn cached_outline(&self, key: FontKey, gid: u32) -> Option<&ts::Path> {
        self.outlines.get(&(key, gid)).and_then(Option::as_ref)
    }
}

/// A rectangle of whole pixels: columns `x..x + width`, rows `y..y + height`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// From edges; empty when `right <= x` or `bottom <= y`.
    pub fn from_ltrb(x: i32, y: i32, right: i32, bottom: i32) -> Self {
        Self::new(x, y, (right - x).max(0) as u32, (bottom - y).max(0) as u32)
    }

    pub fn right(&self) -> i32 {
        self.x + self.width as i32
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.height as i32
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub fn area(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    /// The overlap, or `None` when there is none.
    pub fn intersect(&self, o: &PixelRect) -> Option<PixelRect> {
        let r = Self::from_ltrb(
            self.x.max(o.x),
            self.y.max(o.y),
            self.right().min(o.right()),
            self.bottom().min(o.bottom()),
        );
        (!r.is_empty()).then_some(r)
    }

    /// The smallest rect holding both (an empty rect adds nothing).
    pub fn union(&self, o: &PixelRect) -> PixelRect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        Self::from_ltrb(
            self.x.min(o.x),
            self.y.min(o.y),
            self.right().max(o.right()),
            self.bottom().max(o.bottom()),
        )
    }

    pub fn contains(&self, o: &PixelRect) -> bool {
        o.x >= self.x && o.y >= self.y && o.right() <= self.right() && o.bottom() <= self.bottom()
    }

    /// Grown by `px` on every side.
    pub fn outset(&self, px: i32) -> PixelRect {
        Self::from_ltrb(
            self.x - px,
            self.y - px,
            self.right() + px,
            self.bottom() + px,
        )
    }

    /// Moved by (`dx`, `dy`).
    pub fn offset(&self, dx: i32, dy: i32) -> PixelRect {
        Self::new(self.x + dx, self.y + dy, self.width, self.height)
    }

    /// Whole pixels touched by a float rectangle `[x0, x1) × [y0, y1)`.
    pub fn covering(x0: f64, y0: f64, x1: f64, y1: f64) -> PixelRect {
        let clamp = |v: f64| v.clamp(-1.0e9, 1.0e9) as i32;
        Self::from_ltrb(
            clamp(x0.floor()),
            clamp(y0.floor()),
            clamp(x1.ceil()),
            clamp(y1.ceil()),
        )
    }
}

/// A pixel region of a page raster: the pixels `rect` of the whole page
/// rendered with `opts` (same transform, same pixel grid). The region
/// itself is the unit of rendering: tiles of a region are pixel-aligned
/// sub-rectangles of it and are drawn with exactly its transform and clip,
/// so tiled output equals [`RasterRenderer::render_region`] byte for byte.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Region {
    pub opts: RasterOptions,
    /// Pixels of the page raster this region covers (may extend past it).
    pub rect: PixelRect,
    /// Sample images from half-size mip levels when they are drawn at less
    /// than half size. The GUI's canvas and page thumbnails only: exports,
    /// file thumbnails and goldens never set it.
    pub mips: bool,
}

impl Region {
    /// The whole page, exactly as [`RasterRenderer::render`] draws it.
    pub fn page(scene: &PageScene, opts: RasterOptions) -> Self {
        let (w, h) = pixel_size(scene, &opts);
        Self {
            opts,
            rect: PixelRect::new(0, 0, w, h),
            mips: false,
        }
    }

    /// Page points → region pixels.
    pub fn transform(&self, scene: &PageScene) -> Affine {
        let page = device_transform(scene, &self.opts);
        if self.rect.x == 0 && self.rect.y == 0 {
            page
        } else {
            page.then(Affine::translate(
                -f64::from(self.rect.x),
                -f64::from(self.rect.y),
            ))
        }
    }

    pub(crate) fn check_size(&self) -> Result<(), RenderError> {
        let (w, h) = (self.rect.width, self.rect.height);
        if w > MAX_PIXELS_PER_SIDE || h > MAX_PIXELS_PER_SIDE {
            return Err(RenderError::TooLarge(w, h));
        }
        if w == 0 || h == 0 {
            return Err(RenderError::InvalidSize);
        }
        Ok(())
    }

    /// The premultiplied background pixel (transparent without a paper colour),
    /// exactly as `Pixmap::fill` writes it.
    pub(crate) fn background_pixel(&self) -> ts::PremultipliedColorU8 {
        match self.opts.background {
            Some(bg) => ts::Color::from_rgba8(bg.r, bg.g, bg.b, bg.a)
                .premultiply()
                .to_color_u8(),
            None => ts::PremultipliedColorU8::TRANSPARENT,
        }
    }
}

/// tiny-skia renderer with glyph caching.
#[derive(Default)]
pub struct RasterRenderer {
    glyphs: GlyphCache,
}

impl RasterRenderer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Render a page scene to a premultiplied pixmap.
    pub fn render(
        &mut self,
        scene: &PageScene,
        images: &dyn ImageResolver,
        opts: &RasterOptions,
    ) -> Result<ts::Pixmap, RenderError> {
        self.render_region(scene, images, &Region::page(scene, *opts))
    }

    /// Render one region of a page on this thread, untiled: the reference
    /// that tiled rendering ([`crate::tiles::TileRenderer`]) reproduces.
    pub fn render_region(
        &mut self,
        scene: &PageScene,
        images: &dyn ImageResolver,
        region: &Region,
    ) -> Result<ts::Pixmap, RenderError> {
        region.check_size()?;
        let mut pm = ts::Pixmap::new(region.rect.width, region.rect.height)
            .ok_or(RenderError::InvalidSize)?;
        if let Some(bg) = region.opts.background {
            pm.fill(ts::Color::from_rgba8(bg.r, bg.g, bg.b, bg.a));
        }
        self.glyphs.prepare(scene);
        let painter = Painter {
            glyphs: &self.glyphs,
            images,
            mips: region.mips,
        };
        let base = region.transform(scene);
        let (w, h) = (pm.width(), pm.height());
        // Nodes of an imposed sheet carry a clip (their cell): draw each run
        // of equally clipped nodes on a layer and composite it through a mask.
        let mut i = 0;
        while i < scene.nodes.len() {
            let clip = scene.nodes[i].clip;
            let mut j = i;
            while j < scene.nodes.len() && scene.nodes[j].clip == clip {
                j += 1;
            }
            let run = &scene.nodes[i..j];
            i = j;
            let Some(c) = clip else {
                for node in run {
                    painter.draw_node(&mut pm, node, base);
                }
                continue;
            };
            let mask = ts::Rect::from_xywh(c.x as f32, c.y as f32, c.width as f32, c.height as f32)
                .map(ts::PathBuilder::from_rect)
                .and_then(|path| {
                    let mut m = ts::Mask::new(w, h)?;
                    m.fill_path(&path, ts::FillRule::Winding, true, to_ts(base));
                    Some(m)
                });
            let (Some(mask), Some(mut layer)) = (mask, ts::Pixmap::new(w, h)) else {
                continue;
            };
            for node in run {
                painter.draw_node(&mut layer, node, base);
            }
            pm.draw_pixmap(
                0,
                0,
                layer.as_ref(),
                &ts::PixmapPaint::default(),
                ts::Transform::identity(),
                Some(&mask),
            );
        }
        Ok(pm)
    }

    /// Render a page scene to PNG bytes (straight alpha RGBA).
    pub fn render_png(
        &mut self,
        scene: &PageScene,
        images: &dyn ImageResolver,
        opts: &RasterOptions,
    ) -> Result<Vec<u8>, RenderError> {
        let pm = self.render(scene, images, opts)?;
        pm.encode_png().map_err(|e| RenderError::Png(e.to_string()))
    }
}

/// A pixel-aligned window of the pixmap being drawn: the tile. Pixels outside
/// it may be drawn but are never read back.
pub(crate) type Window = PixelRect;

/// Margin (pixels) kept around a window when a shape is cut down to it.
const CUT_MARGIN: i32 = 2;

/// Draws scene nodes. Shared by the untiled renderer (no window) and the
/// tile renderer (a window per tile). With a window, axis-aligned rectangles
/// and image clips are cut to the window plus [`CUT_MARGIN`]: their vertical
/// edges keep the same x and every row inside the window keeps full coverage,
/// so the window's pixels are exactly those of the whole shape; everything
/// else is drawn whole with the region's transform and clip.
pub(crate) struct Painter<'a, R: ImageResolver + ?Sized> {
    pub glyphs: &'a GlyphCache,
    pub images: &'a R,
    pub mips: bool,
}

impl<R: ImageResolver + ?Sized> Painter<'_, R> {
    /// Draw a node over the whole pixmap (the untiled path).
    pub fn draw_node(&self, pm: &mut ts::Pixmap, node: &Node, base: Affine) {
        let t = node.transform.then(base);
        if node.opacity < 1.0 {
            let Some(mut layer) = ts::Pixmap::new(pm.width(), pm.height()) else {
                return;
            };
            for p in &node.prims {
                self.draw_prim(&mut layer, p, t, None);
            }
            let paint = ts::PixmapPaint {
                opacity: node.opacity as f32,
                ..Default::default()
            };
            pm.draw_pixmap(
                0,
                0,
                layer.as_ref(),
                &paint,
                ts::Transform::identity(),
                None,
            );
        } else {
            for p in &node.prims {
                self.draw_prim(pm, p, t, None);
            }
        }
    }

    /// Draw one primitive with node transform `t`. With `window`, only the
    /// window's pixels are guaranteed (and as little else as is cheap).
    pub fn draw_prim(&self, pm: &mut ts::Pixmap, prim: &Prim, t: Affine, window: Option<Window>) {
        let tt = to_ts(t);
        match prim {
            Prim::Fill { geom, paint } => {
                let solid = matches!(paint, ResolvedPaint::Solid(_));
                if let (
                    Some(w),
                    Geom::Rect {
                        w: gw,
                        h: gh,
                        radius,
                    },
                ) = (window, geom)
                    && solid
                    && *radius <= 0.0
                    && let Some(r) = ts::Rect::from_xywh(0.0, 0.0, *gw as f32, *gh as f32)
                    && let Some(cut) = cut_rect(r, tt, w)
                {
                    pm.fill_path(
                        &cut,
                        &paint_of(paint.solid_rgba()),
                        ts::FillRule::Winding,
                        ts::Transform::identity(),
                        None,
                    );
                    return;
                }
                if let Some(path) = geom_path(*geom) {
                    pm.fill_path(
                        &path,
                        &fill_paint_of(paint, geom),
                        ts::FillRule::Winding,
                        tt,
                        None,
                    );
                }
            }
            Prim::Stroke {
                geom,
                color,
                width,
                dash,
            } => {
                if let Some(path) = geom_path(*geom) {
                    let stroke = ts::Stroke {
                        width: *width as f32,
                        dash: dash_pattern(*dash, *width).and_then(|d| {
                            ts::StrokeDash::new(d.into_iter().map(|v| v as f32).collect(), 0.0)
                        }),
                        ..Default::default()
                    };
                    pm.stroke_path(&path, &paint_of(color.rgba), &stroke, tt, None);
                }
            }
            Prim::Rect { rect, color } => {
                if let Some(r) = ts::Rect::from_xywh(
                    rect.x as f32,
                    rect.y as f32,
                    rect.width as f32,
                    rect.height as f32,
                ) {
                    // `fill_rect` with a transform fills the rect's path.
                    if let Some(w) = window
                        && !tt.is_identity()
                        && let Some(cut) = cut_rect(r, tt, w)
                    {
                        pm.fill_path(
                            &cut,
                            &paint_of(color.rgba),
                            ts::FillRule::Winding,
                            ts::Transform::identity(),
                            None,
                        );
                        return;
                    }
                    pm.fill_rect(r, &paint_of(color.rgba), tt, None);
                }
            }
            Prim::Image {
                asset,
                transform,
                clip,
            } => {
                let Some(content) = self.images.image(*asset) else {
                    return;
                };
                let Some(clip_rect) = ts::Rect::from_xywh(
                    clip.x as f32,
                    clip.y as f32,
                    clip.width as f32,
                    clip.height as f32,
                ) else {
                    return;
                };
                let clip_path = ts::PathBuilder::from_rect(clip_rect);
                match content {
                    ImageContent::Raster(img) => {
                        let (pixmap, pattern) = if self.mips {
                            let (pm, to_image) = img.mip_for(transform.then(t));
                            (pm, to_image.then(*transform))
                        } else {
                            (&img.pixmap, *transform)
                        };
                        let mut paint = ts::Paint {
                            shader: ts::Pattern::new(
                                pixmap.as_ref(),
                                ts::SpreadMode::Pad,
                                ts::FilterQuality::Bicubic,
                                1.0,
                                to_ts(pattern),
                            ),
                            anti_alias: true,
                            ..Default::default()
                        };
                        if let Some(w) = window
                            && !tt.is_identity()
                            && let Some(cut) = cut_rect(clip_rect, tt, w)
                        {
                            // What `fill_path` does with a transform, then
                            // the cut path in device space.
                            paint.shader.transform(tt);
                            pm.fill_path(
                                &cut,
                                &paint,
                                ts::FillRule::Winding,
                                ts::Transform::identity(),
                                None,
                            );
                            return;
                        }
                        pm.fill_path(&clip_path, &paint, ts::FillRule::Winding, tt, None);
                    }
                    ImageContent::Svg(tree) => {
                        draw_svg(pm, &tree, transform.then(t), &clip_path, tt, window);
                    }
                }
            }
            Prim::Glyphs(run, ink) => {
                let key = run.font.key();
                let upem = self.glyphs.cached_upem(key);
                let k = run.font_size / f64::from(upem);
                let mut glyph_space = Affine::scale(k, k);
                if let Some(skew) = run.synthetic_skew {
                    glyph_space = glyph_space.then(Affine::skew_x(skew));
                }
                let paint = paint_of(ink.rgba);
                let bold = run.synthetic_bold.then(|| ts::Stroke {
                    width: upem * 0.03,
                    line_join: ts::LineJoin::Round,
                    ..Default::default()
                });
                let grow = if bold.is_some() {
                    f64::from(upem) * 0.03
                } else {
                    0.0
                };
                for g in &run.glyphs {
                    let Some(path) = self.glyphs.cached_outline(key, g.id) else {
                        continue;
                    };
                    let gt_affine = glyph_space.then(Affine::translate(g.x, g.y)).then(t);
                    if let Some(w) = window
                        && !glyph_box(path, gt_affine, grow)
                            .is_some_and(|b| b.intersect(&w).is_some())
                    {
                        continue;
                    }
                    let gt = to_ts(gt_affine);
                    pm.fill_path(path, &paint, ts::FillRule::Winding, gt, None);
                    if let Some(s) = &bold {
                        pm.stroke_path(path, &paint, s, gt, None);
                    }
                }
            }
        }
    }
}

/// Device-pixel box of a glyph outline drawn with `gt`, grown by `grow` font
/// units (synthetic bold) and the anti-aliasing margin.
pub(crate) fn glyph_box(path: &ts::Path, gt: Affine, grow: f64) -> Option<PixelRect> {
    let b = path.bounds();
    let (l, t, r, bo) = (
        f64::from(b.left()) - grow,
        f64::from(b.top()) - grow,
        f64::from(b.right()) + grow,
        f64::from(b.bottom()) + grow,
    );
    Some(affine_box(gt, l, t, r, bo).outset(CUT_MARGIN))
}

/// Pixels covered by the axis-aligned box of a transformed rectangle.
pub(crate) fn affine_box(t: Affine, x0: f64, y0: f64, x1: f64, y1: f64) -> PixelRect {
    let pts = [
        t.apply(x0, y0),
        t.apply(x1, y0),
        t.apply(x1, y1),
        t.apply(x0, y1),
    ];
    let (mut l, mut tp, mut r, mut b) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (x, y) in pts {
        l = l.min(x);
        tp = tp.min(y);
        r = r.max(x);
        b = b.max(y);
    }
    PixelRect::covering(l, tp, r, b)
}

/// A rectangle `r` drawn with the axis-aligned, upright transform `tt`, cut
/// to the window plus [`CUT_MARGIN`] in device space. `None` when `tt`
/// rotates, skews or flips (draw the whole shape) or when nothing was cut.
///
/// The device path is built exactly as `fill_path` builds it (the same f32
/// transform of the same rect path), then any side lying beyond the margin
/// moves to the margin: kept sides are bit-identical, so is every edge
/// crossing the window, and rows and columns inside the window stay fully
/// covered on the cut sides.
fn cut_rect(r: ts::Rect, tt: ts::Transform, w: Window) -> Option<ts::Path> {
    if tt.kx != 0.0 || tt.ky != 0.0 || tt.sx <= 0.0 || tt.sy <= 0.0 {
        return None;
    }
    let dev = ts::PathBuilder::from_rect(r).transform(tt)?;
    let b = dev.bounds();
    let (wl, wt, wr, wb) = (
        (w.x - CUT_MARGIN) as f32,
        (w.y - CUT_MARGIN) as f32,
        (w.right() + CUT_MARGIN) as f32,
        (w.bottom() + CUT_MARGIN) as f32,
    );
    let (l, t, rr, bb) = (
        b.left().max(wl),
        b.top().max(wt),
        b.right().min(wr),
        b.bottom().min(wb),
    );
    if (l, t, rr, bb) == (b.left(), b.top(), b.right(), b.bottom()) {
        return Some(dev);
    }
    if l >= rr || t >= bb {
        // Outside the window: the caller draws it whole (still exact).
        return None;
    }
    Some(ts::PathBuilder::from_rect(ts::Rect::from_ltrb(
        l, t, rr, bb,
    )?))
}

/// Render an SVG clipped to `clip` (frame-local path drawn with `frame_ts`).
/// With a window, the SVG is rendered as in the untiled path but only the
/// window's pixels are composited.
fn draw_svg(
    pm: &mut ts::Pixmap,
    tree: &usvg::Tree,
    svg_to_device: Affine,
    clip: &ts::Path,
    frame_ts: ts::Transform,
    window: Option<Window>,
) {
    let Some(dev_clip) = clip.clone().transform(frame_ts) else {
        return;
    };
    let b = dev_clip.bounds();
    let x0 = b.left().floor().max(0.0) as i32;
    let y0 = b.top().floor().max(0.0) as i32;
    let x1 = (b.right().ceil() as i32).min(pm.width() as i32);
    let y1 = (b.bottom().ceil() as i32).min(pm.height() as i32);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let target = PixelRect::from_ltrb(x0, y0, x1, y1);
    let target = match window {
        Some(w) => match target.intersect(&w) {
            Some(r) => r,
            None => return,
        },
        None => target,
    };
    let Some(mut sub) = ts::Pixmap::new((x1 - x0) as u32, (y1 - y0) as u32) else {
        return;
    };
    let local = svg_to_device.then(Affine::translate(-f64::from(x0), -f64::from(y0)));
    resvg::render(tree, to_ts(local), &mut sub.as_mut());
    let axis_aligned = frame_ts.kx == 0.0 && frame_ts.ky == 0.0;
    let mask = if axis_aligned {
        None
    } else {
        let mut m = ts::Mask::new(pm.width(), pm.height());
        if let Some(m) = m.as_mut() {
            m.fill_path(clip, ts::FillRule::Winding, true, frame_ts);
        }
        m
    };
    if window.is_none() {
        pm.draw_pixmap(
            x0,
            y0,
            sub.as_ref(),
            &ts::PixmapPaint::default(),
            ts::Transform::identity(),
            mask.as_ref(),
        );
    } else {
        composite(
            pm,
            x0,
            y0,
            sub.as_ref(),
            &ts::PixmapPaint::default(),
            target,
            mask.as_ref(),
        );
    }
}

/// `pm.draw_pixmap(x, y, src, paint, identity, mask)` restricted to the
/// pixels of `rect`: the same pattern paint and per-pixel pipeline as
/// tiny-skia's `draw_pixmap`, filling only `rect`.
pub(crate) fn composite(
    pm: &mut ts::Pixmap,
    x: i32,
    y: i32,
    src: ts::PixmapRef,
    paint: &ts::PixmapPaint,
    rect: PixelRect,
    mask: Option<&ts::Mask>,
) {
    let Some(r) = ts::Rect::from_ltrb(
        rect.x as f32,
        rect.y as f32,
        rect.right() as f32,
        rect.bottom() as f32,
    ) else {
        return;
    };
    let fill = ts::Paint {
        shader: ts::Pattern::new(
            src,
            ts::SpreadMode::Pad,
            paint.quality,
            paint.opacity,
            ts::Transform::from_translate(x as f32, y as f32),
        ),
        blend_mode: paint.blend_mode,
        anti_alias: false,
        force_hq_pipeline: false,
        ..Default::default()
    };
    pm.fill_rect(r, &fill, ts::Transform::identity(), mask);
}

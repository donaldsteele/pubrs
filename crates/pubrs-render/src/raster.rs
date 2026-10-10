//! Software rasteriser (tiny-skia). Deterministic: used for PNG export,
//! previews, golden tests and as the GUI fallback when no GPU is available.

use std::collections::HashMap;

use pubrs_core::{Color, Dash};
use resvg::tiny_skia as ts;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::OutlinePen;
use skrifa::{FontRef, GlyphId, MetadataProvider};

use crate::images::{ImageContent, ImageResolver};
use crate::scene::{Affine, Geom, Node, PageScene, Prim};

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
        let (w, h) = pixel_size(scene, opts);
        if w > MAX_PIXELS_PER_SIDE || h > MAX_PIXELS_PER_SIDE {
            return Err(RenderError::TooLarge(w, h));
        }
        let mut pm = ts::Pixmap::new(w, h).ok_or(RenderError::InvalidSize)?;
        if let Some(bg) = opts.background {
            pm.fill(ts::Color::from_rgba8(bg.r, bg.g, bg.b, bg.a));
        }
        let base = device_transform(scene, opts);
        for node in &scene.nodes {
            self.draw_node(&mut pm, node, base, images);
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

    fn draw_node(
        &mut self,
        pm: &mut ts::Pixmap,
        node: &Node,
        base: Affine,
        images: &dyn ImageResolver,
    ) {
        let t = node.transform.then(base);
        if node.opacity < 1.0 {
            let Some(mut layer) = ts::Pixmap::new(pm.width(), pm.height()) else {
                return;
            };
            for p in &node.prims {
                self.draw_prim(&mut layer, p, t, images);
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
                self.draw_prim(pm, p, t, images);
            }
        }
    }

    fn draw_prim(
        &mut self,
        pm: &mut ts::Pixmap,
        prim: &Prim,
        t: Affine,
        images: &dyn ImageResolver,
    ) {
        let tt = to_ts(t);
        match prim {
            Prim::Fill { geom, color } => {
                if let Some(path) = geom_path(*geom) {
                    pm.fill_path(&path, &paint_of(*color), ts::FillRule::Winding, tt, None);
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
                    pm.stroke_path(&path, &paint_of(*color), &stroke, tt, None);
                }
            }
            Prim::Rect { rect, color } => {
                if let Some(r) = ts::Rect::from_xywh(
                    rect.x as f32,
                    rect.y as f32,
                    rect.width as f32,
                    rect.height as f32,
                ) {
                    pm.fill_rect(r, &paint_of(*color), tt, None);
                }
            }
            Prim::Image {
                asset,
                transform,
                clip,
            } => {
                let Some(content) = images.image(*asset) else {
                    return;
                };
                let Some(clip_path) = ts::Rect::from_xywh(
                    clip.x as f32,
                    clip.y as f32,
                    clip.width as f32,
                    clip.height as f32,
                )
                .map(ts::PathBuilder::from_rect) else {
                    return;
                };
                match content {
                    ImageContent::Raster(img) => {
                        let paint = ts::Paint {
                            shader: ts::Pattern::new(
                                img.pixmap.as_ref(),
                                ts::SpreadMode::Pad,
                                ts::FilterQuality::Bicubic,
                                1.0,
                                to_ts(*transform),
                            ),
                            anti_alias: true,
                            ..Default::default()
                        };
                        pm.fill_path(&clip_path, &paint, ts::FillRule::Winding, tt, None);
                    }
                    ImageContent::Svg(tree) => {
                        draw_svg(pm, &tree, transform.then(t), &clip_path, tt);
                    }
                }
            }
            Prim::Glyphs(run) => {
                let key = run.font.key();
                let bytes = run.font.bytes();
                let upem = self.glyphs.upem(key, bytes);
                let k = run.font_size / f64::from(upem);
                let mut glyph_space = Affine::scale(k, k);
                if let Some(skew) = run.synthetic_skew {
                    glyph_space = glyph_space.then(Affine::skew_x(skew));
                }
                let paint = paint_of(run.color);
                let bold = run.synthetic_bold.then(|| ts::Stroke {
                    width: upem * 0.03,
                    line_join: ts::LineJoin::Round,
                    ..Default::default()
                });
                for g in &run.glyphs {
                    let Some(path) = self.glyphs.outline(key, bytes, g.id) else {
                        continue;
                    };
                    let gt = to_ts(glyph_space.then(Affine::translate(g.x, g.y)).then(t));
                    pm.fill_path(path, &paint, ts::FillRule::Winding, gt, None);
                    if let Some(s) = &bold {
                        pm.stroke_path(path, &paint, s, gt, None);
                    }
                }
            }
        }
    }
}

/// Render an SVG clipped to `clip` (frame-local path drawn with `frame_ts`).
fn draw_svg(
    pm: &mut ts::Pixmap,
    tree: &usvg::Tree,
    svg_to_device: Affine,
    clip: &ts::Path,
    frame_ts: ts::Transform,
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
    pm.draw_pixmap(
        x0,
        y0,
        sub.as_ref(),
        &ts::PixmapPaint::default(),
        ts::Transform::identity(),
        mask.as_ref(),
    );
}

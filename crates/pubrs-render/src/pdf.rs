//! PDF output (krilla). Print-ready: subset embedded fonts, bleed, crop
//! marks, MediaBox/CropBox/BleedBox/TrimBox set. The same PDF is used for
//! printing on every OS.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use krilla::color::{rgb, separation};
use krilla::geom::{Path, PathBuilder, Point, Rect as KRect, Size, Transform};
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{Fill, FillRule, LineCap, LineJoin, Stroke, StrokeDash};
use krilla::surface::Surface;
use krilla::text::{Font, GlyphId};
use krilla::{Data, Document};
use krilla_svg::{SurfaceExt, SvgSettings};
use pubrs_core::{AssetId, Color, ImageFormat};
use pubrs_layout::GlyphRun;

use crate::images::{ImageContent, ImageResolver};
use crate::raster::dash_pattern;
use crate::scene::{Affine, Geom, PageScene, Prim};

/// How fonts are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontEmbedding {
    /// Embed subsets of the fonts used (text stays selectable).
    #[default]
    Subset,
    /// Convert text to outlines (no fonts in the file).
    Outlines,
}

/// PDF export settings.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PdfOptions {
    /// Extend artwork into the document's bleed area and set the BleedBox.
    pub bleed: bool,
    /// Draw crop marks outside the bleed area.
    pub crop_marks: bool,
    pub font_embedding: FontEmbedding,
    pub title: Option<String>,
    pub author: Option<String>,
}

/// Distance of crop marks from the trim edge when there is no bleed.
const MARK_MIN_OFFSET: f64 = 6.0;
const MARK_LENGTH: f64 = 18.0;
const MARK_MARGIN: f64 = 3.0;
const MARK_WIDTH: f32 = 0.25;

/// PDF failures.
#[derive(Debug, Clone, thiserror::Error)]
pub enum PdfError {
    #[error("no pages to export")]
    NoPages,
    #[error("PDF writer: {0}")]
    Krilla(String),
}

/// Geometry of the page boxes for a trim size, in surface coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageBoxes {
    /// Offset of the trim box from the media box origin on every side.
    pub slug: f64,
    pub bleed: f64,
    pub media: (f64, f64),
}

/// Compute page boxes for a scene and options.
pub fn page_boxes(scene: &PageScene, opts: &PdfOptions) -> PageBoxes {
    let bleed = if opts.bleed {
        scene.bleed.max(0.0)
    } else {
        0.0
    };
    let slug = if opts.crop_marks {
        let offset = bleed.max(MARK_MIN_OFFSET);
        bleed.max(offset + MARK_LENGTH + MARK_MARGIN)
    } else {
        bleed
    };
    PageBoxes {
        slug,
        bleed,
        media: (scene.width + 2.0 * slug, scene.height + 2.0 * slug),
    }
}

fn kt(a: Affine) -> Transform {
    let [sx, ky, kx, sy, tx, ty] = a.0.map(|v| v as f32);
    Transform::from_row(sx, ky, kx, sy, tx, ty)
}

fn krect(x: f64, y: f64, w: f64, h: f64) -> Option<KRect> {
    KRect::from_xywh(x as f32, y as f32, w as f32, h as f32)
}

fn rgb_of(c: Color) -> rgb::Color {
    rgb::Color::new(c.r, c.g, c.b)
}

fn alpha_of(c: Color) -> NormalizedF32 {
    NormalizedF32::new(f32::from(c.a) / 255.0).unwrap_or(NormalizedF32::ONE)
}

fn fill_of(c: Color) -> Fill {
    Fill {
        paint: rgb_of(c).into(),
        opacity: alpha_of(c),
        rule: FillRule::NonZero,
    }
}

fn geom_path(g: Geom) -> Option<Path> {
    let mut pb = PathBuilder::new();
    match g {
        Geom::Rect { w, h, radius } => {
            let (w, h, r) = (w as f32, h as f32, radius as f32);
            if w <= 0.0 || h <= 0.0 {
                return None;
            }
            if r <= 0.0 {
                pb.push_rect(KRect::from_xywh(0.0, 0.0, w, h)?);
            } else {
                let k = 0.552_284_8 * r;
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
            }
        }
        Geom::Ellipse { w, h } => {
            let (rx, ry) = (w as f32 / 2.0, h as f32 / 2.0);
            if rx <= 0.0 || ry <= 0.0 {
                return None;
            }
            let (cx, cy) = (rx, ry);
            let (kx, ky) = (0.552_284_8 * rx, 0.552_284_8 * ry);
            pb.move_to(cx + rx, cy);
            pb.cubic_to(cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry);
            pb.cubic_to(cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy);
            pb.cubic_to(cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry);
            pb.cubic_to(cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy);
            pb.close();
        }
        Geom::Line { x0, y0, x1, y1 } => {
            pb.move_to(x0 as f32, y0 as f32);
            pb.line_to(x1 as f32, y1 as f32);
        }
    }
    pb.finish()
}

fn rect_path(x: f64, y: f64, w: f64, h: f64) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.push_rect(krect(x, y, w, h)?);
    pb.finish()
}

/// One glyph of a pre-shaped run, positioned relative to the run origin.
struct PlacedGlyph {
    id: u32,
    /// Advance in ems (krilla multiplies by the requested size).
    advance: f32,
    /// Upward baseline offset in ems.
    y_up: f32,
    range: Range<usize>,
}

impl krilla::text::Glyph for PlacedGlyph {
    fn glyph_id(&self) -> GlyphId {
        GlyphId::new(self.id)
    }
    fn text_range(&self) -> Range<usize> {
        self.range.clone()
    }
    fn x_advance(&self, size: f32) -> f32 {
        self.advance * size
    }
    fn x_offset(&self, _size: f32) -> f32 {
        0.0
    }
    fn y_offset(&self, size: f32) -> f32 {
        self.y_up * size
    }
    fn y_advance(&self, _size: f32) -> f32 {
        0.0
    }
    fn location(&self) -> Option<krilla::surface::Location> {
        None
    }
}

struct BlobBytes(pubrs_layout::types::FontHandle);

impl AsRef<[u8]> for BlobBytes {
    fn as_ref(&self) -> &[u8] {
        self.0.bytes()
    }
}

/// Write pages to a PDF.
pub fn render_pdf(
    pages: &[PageScene],
    images: &dyn ImageResolver,
    opts: &PdfOptions,
) -> Result<Vec<u8>, PdfError> {
    if pages.is_empty() {
        return Err(PdfError::NoPages);
    }
    let mut doc = Document::new();
    let mut meta = krilla::metadata::Metadata::new().creator("pubrs".to_string());
    if let Some(t) = &opts.title
        && !t.is_empty()
    {
        meta = meta.title(t.clone());
    }
    if let Some(a) = &opts.author
        && !a.is_empty()
    {
        meta = meta.authors(vec![a.clone()]);
    }
    doc.set_metadata(meta);
    let mut fonts: HashMap<(u64, u32), Option<Font>> = HashMap::new();
    let mut image_cache: HashMap<AssetId, Option<krilla::image::Image>> = HashMap::new();
    for scene in pages {
        let boxes = page_boxes(scene, opts);
        let (mw, mh) = boxes.media;
        let s = boxes.slug;
        let b = boxes.bleed;
        let settings = PageSettings::from_wh(mw as f32, mh as f32)
            .ok_or_else(|| PdfError::Krilla("invalid page size".into()))?
            .with_media_box(krect(0.0, 0.0, mw, mh))
            .with_crop_box(krect(0.0, 0.0, mw, mh))
            .with_bleed_box(krect(
                s - b,
                s - b,
                scene.width + 2.0 * b,
                scene.height + 2.0 * b,
            ))
            .with_trim_box(krect(s, s, scene.width, scene.height));
        let mut page = doc.start_page_with(settings);
        let mut surface = page.surface();
        if opts.crop_marks {
            draw_crop_marks(&mut surface, scene, s, b.max(MARK_MIN_OFFSET));
        }
        if let Some(clip) = rect_path(s - b, s - b, scene.width + 2.0 * b, scene.height + 2.0 * b) {
            surface.push_clip_path(&clip, &FillRule::NonZero);
        }
        let base = Affine::translate(s, s);
        for node in &scene.nodes {
            surface.push_transform(&kt(node.transform.then(base)));
            let layered = node.opacity < 1.0;
            if layered {
                surface.push_opacity(
                    NormalizedF32::new(node.opacity as f32).unwrap_or(NormalizedF32::ONE),
                );
            }
            for prim in &node.prims {
                draw_prim(
                    &mut surface,
                    prim,
                    images,
                    opts,
                    &mut fonts,
                    &mut image_cache,
                );
            }
            if layered {
                surface.pop();
            }
            surface.pop();
        }
        surface.pop();
        surface.finish();
        page.finish();
    }
    doc.finish().map_err(|e| PdfError::Krilla(format!("{e:?}")))
}

fn draw_prim(
    surface: &mut Surface<'_>,
    prim: &Prim,
    images: &dyn ImageResolver,
    opts: &PdfOptions,
    fonts: &mut HashMap<(u64, u32), Option<Font>>,
    image_cache: &mut HashMap<AssetId, Option<krilla::image::Image>>,
) {
    match prim {
        Prim::Fill { geom, color } => {
            if let Some(p) = geom_path(*geom) {
                surface.set_stroke(None);
                surface.set_fill(Some(fill_of(*color)));
                surface.draw_path(&p);
            }
        }
        Prim::Stroke {
            geom,
            color,
            width,
            dash,
        } => {
            if let Some(p) = geom_path(*geom) {
                surface.set_fill(None);
                surface.set_stroke(Some(Stroke {
                    paint: rgb_of(*color).into(),
                    width: *width as f32,
                    opacity: alpha_of(*color),
                    dash: dash_pattern(*dash, *width).map(|d| StrokeDash {
                        array: d.into_iter().map(|v| v as f32).collect(),
                        offset: 0.0,
                    }),
                    ..Default::default()
                }));
                surface.draw_path(&p);
                surface.set_stroke(None);
            }
        }
        Prim::Rect { rect, color } => {
            if let Some(p) = rect_path(rect.x, rect.y, rect.width, rect.height) {
                surface.set_stroke(None);
                surface.set_fill(Some(fill_of(*color)));
                surface.draw_path(&p);
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
            let Some(clip_path) = rect_path(clip.x, clip.y, clip.width, clip.height) else {
                return;
            };
            surface.push_clip_path(&clip_path, &FillRule::NonZero);
            surface.push_transform(&kt(*transform));
            match &content {
                ImageContent::Raster(img) => {
                    let image = image_cache
                        .entry(*asset)
                        .or_insert_with(|| krilla_image(img))
                        .clone();
                    if let (Some(image), Some(size)) =
                        (image, Size::from_wh(img.width as f32, img.height as f32))
                    {
                        surface.draw_image(image, size);
                    }
                }
                ImageContent::Svg(tree) => {
                    let s = tree.size();
                    if let Some(size) = Size::from_wh(s.width(), s.height()) {
                        surface.draw_svg(tree, size, SvgSettings::default());
                    }
                }
            }
            surface.pop();
            surface.pop();
        }
        Prim::Glyphs(run) => draw_run(surface, run, opts, fonts),
    }
}

fn krilla_image(img: &crate::images::RasterImage) -> Option<krilla::image::Image> {
    let data = || Data::from(Arc::new(img.encoded.to_vec()));
    let from_pixels = || {
        let pm = img.pixmap.clone();
        krilla::image::Image::from_rgba8(pm.take_demultiplied(), img.width, img.height)
    };
    if img.passthrough {
        match img.format {
            ImageFormat::Jpeg => krilla::image::Image::from_jpeg(data(), true).ok(),
            ImageFormat::Png => krilla::image::Image::from_png(data(), true).ok(),
            ImageFormat::Svg => None,
        }
        .or_else(|| Some(from_pixels()))
    } else {
        Some(from_pixels())
    }
}

fn draw_run(
    surface: &mut Surface<'_>,
    run: &GlyphRun,
    opts: &PdfOptions,
    fonts: &mut HashMap<(u64, u32), Option<Font>>,
) {
    let Some(first) = run.glyphs.first() else {
        return;
    };
    let font = fonts
        .entry(run.font.key())
        .or_insert_with(|| {
            let bytes: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(BlobBytes(run.font.clone()));
            Font::new(Data::from(bytes), run.font.index)
        })
        .clone();
    let Some(font) = font else {
        return;
    };
    let (x0, y0) = (first.x, first.y);
    let em = run.font_size.max(1e-6);
    let glyphs: Vec<PlacedGlyph> = run
        .glyphs
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let next_x = run.glyphs.get(i + 1).map_or(g.x + g.advance, |n| n.x);
            PlacedGlyph {
                id: g.id,
                advance: ((next_x - g.x) / em) as f32,
                y_up: ((y0 - g.y) / em) as f32,
                range: g.text.clone(),
            }
        })
        .collect();
    let mut origin = Affine::translate(x0, y0);
    if let Some(skew) = run.synthetic_skew {
        origin = Affine::skew_x(skew).then(origin);
    }
    surface.push_transform(&kt(origin));
    surface.set_fill(Some(fill_of(run.color)));
    if run.synthetic_bold {
        surface.set_stroke(Some(Stroke {
            paint: rgb_of(run.color).into(),
            width: (run.font_size * 0.03) as f32,
            line_join: LineJoin::Round,
            opacity: alpha_of(run.color),
            ..Default::default()
        }));
    } else {
        surface.set_stroke(None);
    }
    surface.draw_glyphs(
        Point::from_xy(0.0, 0.0),
        &glyphs,
        font,
        &run.text,
        run.font_size as f32,
        opts.font_embedding == FontEmbedding::Outlines,
    );
    surface.set_stroke(None);
    surface.pop();
}

/// Corner crop marks in registration colour, outside the bleed area.
fn draw_crop_marks(surface: &mut Surface<'_>, scene: &PageScene, slug: f64, offset: f64) {
    let reg = separation::Color::new(
        255,
        separation::SeparationSpace::new(
            separation::SeparationColorant::AllColorants,
            rgb::Color::black().into(),
        ),
    );
    let reg: krilla::color::Color = reg.into();
    surface.set_fill(None);
    surface.set_stroke(Some(Stroke {
        paint: reg.into(),
        width: MARK_WIDTH,
        line_cap: LineCap::Butt,
        ..Default::default()
    }));
    let (l, t) = (slug, slug);
    let (r, bt) = (slug + scene.width, slug + scene.height);
    let o = offset;
    let len = MARK_LENGTH;
    let lines = [
        // top-left
        (l - o - len, t, l - o, t),
        (l, t - o - len, l, t - o),
        // top-right
        (r + o, t, r + o + len, t),
        (r, t - o - len, r, t - o),
        // bottom-left
        (l - o - len, bt, l - o, bt),
        (l, bt + o, l, bt + o + len),
        // bottom-right
        (r + o, bt, r + o + len, bt),
        (r, bt + o, r, bt + o + len),
    ];
    for (x0, y0, x1, y1) in lines {
        let mut pb = PathBuilder::new();
        pb.move_to(x0 as f32, y0 as f32);
        pb.line_to(x1 as f32, y1 as f32);
        if let Some(p) = pb.finish() {
            surface.draw_path(&p);
        }
    }
    surface.set_stroke(None);
}

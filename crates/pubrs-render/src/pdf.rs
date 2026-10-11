//! PDF output (krilla). Print-ready: subset embedded fonts, bleed, crop
//! marks, MediaBox/CropBox/BleedBox/TrimBox set. The same PDF is used for
//! printing on every OS.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use krilla::color::{cmyk, rgb, separation};
use krilla::geom::{Path, PathBuilder, Point, Rect as KRect, Size, Transform};
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{
    Fill, FillRule, LineCap, LineJoin, LinearGradient, RadialGradient, SpreadMethod, Stop, Stroke,
    StrokeDash,
};
use krilla::surface::Surface;
use krilla::text::{Font, GlyphId};
use krilla::{Data, Document};
use krilla_svg::{SurfaceExt, SvgSettings};
use pubrs_core::{
    AssetId, Cmyk, Color, FrameId, ImageFormat, InkSpec, ResolvedGradient, ResolvedInk,
    ResolvedPaint,
};
use pubrs_layout::GlyphRun;

use crate::images::{ImageContent, ImageResolver};
use crate::raster::dash_pattern;
use crate::scene::{Affine, Geom, GradientAxis, PageScene, Prim, gradient_axis};

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
    /// Export the document's own pages even when it has a label sheet or
    /// booklet set up. By default (false) such documents are imposed.
    pub flat: bool,
    /// Which archival or print standard to write.
    pub standard: PdfStandard,
    /// A CMYK output profile (ICC bytes) to use as the PDF/X-4 output intent
    /// instead of the bundled one. Ignored for other standards.
    pub output_intent_icc: Option<Vec<u8>>,
    /// Write a tagged PDF: text as paragraphs, pictures as figures with
    /// their alt text, everything else as artifacts.
    pub tagged: bool,
    /// Alt text of picture frames, by frame, for tagged output.
    pub alt_text: HashMap<FrameId, String>,
}

/// The standard a PDF is written to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PdfStandard {
    /// Ordinary PDF 1.7: DeviceRGB, DeviceCMYK and Separation colours.
    #[default]
    None,
    /// PDF/X-4 for commercial printing (PDF 1.6, CMYK output intent). The
    /// result is checked and an export that fails the check is an error.
    PdfX4,
    /// PDF/A-2b for archiving (krilla's validator).
    PdfA2b,
}

/// The bundled CMYK output profile (see ASSETS.md): ICC v2, class `prtr`.
pub const GENERIC_CMYK_ICC: &[u8] =
    include_bytes!("../../../assets/icc/generic-cmyk-swop-like.icc");

/// Output condition name of the bundled profile.
pub const GENERIC_CMYK_CONDITION: &str = "Generic CMYK (SWOP-like)";

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
    #[error("not a usable CMYK output profile: {0}")]
    BadProfile(String),
    #[error("the PDF/X-4 result failed its checks: {}", .0.join("; "))]
    NotPdfX4(Vec<String>),
}

/// Validate a CMYK output profile: ICC header with class `prtr`, colour
/// space `CMYK` and the `acsp` signature.
pub fn check_output_profile(icc: &[u8]) -> Result<(), String> {
    if icc.len() < 128 {
        return Err("shorter than an ICC header (128 bytes)".into());
    }
    if &icc[36..40] != b"acsp" {
        return Err("no ICC signature 'acsp' at byte 36".into());
    }
    if &icc[12..16] != b"prtr" {
        return Err("profile class is not 'prtr' (output device)".into());
    }
    if &icc[16..20] != b"CMYK" {
        return Err("colour space is not 'CMYK'".into());
    }
    if krilla::icc::ICCProfile::<4>::new(icc).is_none() {
        return Err("the profile could not be parsed as a four-channel ICC profile".into());
    }
    Ok(())
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

/// Percent (0 to 100) to a PDF byte level. krilla stores every colour
/// component as a `u8`, so the value written is `round(p / 100 * 255) / 255`
/// (ADR 0018).
fn level(percent: f64) -> u8 {
    let p = if percent.is_finite() {
        percent.clamp(0.0, 100.0)
    } else {
        0.0
    };
    (p / 100.0 * 255.0).round() as u8
}

/// A DeviceCMYK colour with each component scaled by the tint (percent).
fn cmyk_scaled(c: &Cmyk, tint: f64) -> cmyk::Color {
    let t = if tint.is_finite() {
        tint.clamp(0.0, 100.0) / 100.0
    } else {
        1.0
    };
    cmyk::Color::new(
        level(c.c * t),
        level(c.m * t),
        level(c.y * t),
        level(c.k * t),
    )
}

/// The exact PDF colour of an ink: RGB as RGB, CMYK as DeviceCMYK with the
/// tint applied to the components, a spot colour as a Separation named after
/// the spot (tint = tint percent) with its CMYK alternative as the fallback.
fn color_of(ink: &ResolvedInk) -> krilla::color::Color {
    match &ink.spec {
        InkSpec::Rgb => rgb_of(ink.rgba).into(),
        InkSpec::Cmyk { cmyk, tint } => cmyk_scaled(cmyk, *tint).into(),
        InkSpec::Spot { name, cmyk, tint } => separation::Color::new(
            level(*tint),
            separation::SeparationSpace::new(
                separation::SeparationColorant::Custom(name.clone()),
                cmyk_scaled(cmyk, 100.0).into(),
            ),
        )
        .into(),
    }
}

fn fill_of_ink(ink: &ResolvedInk) -> Fill {
    Fill {
        paint: color_of(ink).into(),
        opacity: alpha_of(ink.rgba),
        rule: FillRule::NonZero,
    }
}

/// The colour space shared by every stop of a gradient. krilla writes one
/// colour space per shading, so stops of mixed kinds are brought to a common
/// one: all-spot stops of one spot stay Separation, CMYK and spot stops
/// become DeviceCMYK (the spot's alternative at its tint), and anything mixed
/// with RGB falls back to RGB screen colours.
fn gradient_stop_color(g: &ResolvedGradient, ink: &ResolvedInk) -> krilla::color::Color {
    let all_cmykish = g.stops.iter().all(|(_, i)| !matches!(i.spec, InkSpec::Rgb));
    let spot_names: Vec<&str> = g
        .stops
        .iter()
        .filter_map(|(_, i)| match &i.spec {
            InkSpec::Spot { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let one_spot = spot_names.len() == g.stops.len() && spot_names.windows(2).all(|w| w[0] == w[1]);
    if one_spot {
        return color_of(ink);
    }
    if all_cmykish {
        return match &ink.spec {
            InkSpec::Cmyk { cmyk, tint } | InkSpec::Spot { cmyk, tint, .. } => {
                cmyk_scaled(cmyk, *tint).into()
            }
            InkSpec::Rgb => rgb_of(ink.rgba).into(),
        };
    }
    rgb_of(ink.rgba).into()
}

/// A fill for a resolved paint: exact CMYK and spot colours (see
/// [`color_of`]); gradients use [`gradient_stop_color`].
fn fill_of_paint(p: &ResolvedPaint, geom: &Geom) -> Fill {
    let ResolvedPaint::Gradient(g) = p else {
        let ResolvedPaint::Solid(ink) = p else {
            return fill_of_ink(&ResolvedInk::from(p.solid_rgba()));
        };
        return fill_of_ink(ink);
    };
    let stops: Vec<Stop> = g
        .stops
        .iter()
        .map(|(o, c)| Stop {
            offset: NormalizedF32::new(*o as f32).unwrap_or(NormalizedF32::ZERO),
            color: gradient_stop_color(g, c),
            opacity: alpha_of(c.rgba),
        })
        .collect();
    let (w, h) = geom.size();
    let paint = match gradient_axis(g.kind, g.angle, w, h) {
        GradientAxis::Linear { x0, y0, x1, y1 } => LinearGradient {
            x1: x0 as f32,
            y1: y0 as f32,
            x2: x1 as f32,
            y2: y1 as f32,
            transform: Transform::identity(),
            spread_method: SpreadMethod::Pad,
            stops,
            anti_alias: true,
        }
        .into(),
        GradientAxis::Radial { cx, cy, r } => RadialGradient {
            fx: cx as f32,
            fy: cy as f32,
            fr: 0.0,
            cx: cx as f32,
            cy: cy as f32,
            cr: r as f32,
            transform: Transform::identity(),
            spread_method: SpreadMethod::Pad,
            stops,
            anti_alias: true,
        }
        .into(),
    };
    Fill {
        paint,
        opacity: NormalizedF32::ONE,
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

/// Today's date from the system clock, for the standards that require a
/// creation date.
fn today() -> krilla::metadata::DateTime {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let year = (yoe + era * 400 + i64::from(month <= 2)) as u16;
    krilla::metadata::DateTime::new(year)
        .month(month)
        .day(day)
        .hour((rem / 3600) as u8)
        .minute((rem % 3600 / 60) as u8)
        .second((rem % 60) as u8)
}

/// The CMYK profile a standard-conforming export uses.
fn output_profile(opts: &PdfOptions) -> Result<Vec<u8>, PdfError> {
    match (&opts.output_intent_icc, opts.standard) {
        (Some(icc), PdfStandard::PdfX4) => {
            check_output_profile(icc).map_err(PdfError::BadProfile)?;
            Ok(icc.clone())
        }
        _ => Ok(GENERIC_CMYK_ICC.to_vec()),
    }
}

fn serialize_settings(
    opts: &PdfOptions,
    icc: &[u8],
) -> Result<krilla::SerializeSettings, PdfError> {
    use krilla::configure::{Archival, ConfigurationBuilder, PdfVersion, Validator};
    let mut s = krilla::SerializeSettings {
        enable_tagging: opts.tagged,
        ..Default::default()
    };
    let cfg_err = |e| PdfError::Krilla(format!("{e:?}"));
    match opts.standard {
        PdfStandard::None => {}
        PdfStandard::PdfX4 => {
            // PDF 1.6 with device-independent colour: RGB becomes ICC-based
            // sRGB and CMYK is written against the output profile.
            s.configuration = ConfigurationBuilder::new()
                .with_version(PdfVersion::Pdf16)
                .finish()
                .map_err(cfg_err)?;
            s.no_device_cs = true;
            s.cmyk_profile = krilla::icc::ICCProfile::<4>::new(icc);
        }
        PdfStandard::PdfA2b => {
            s.configuration = ConfigurationBuilder::new()
                .set_validator(Validator::A(Archival::A2_B))
                .finish()
                .map_err(cfg_err)?;
            s.cmyk_profile = krilla::icc::ICCProfile::<4>::new(icc);
        }
    }
    Ok(s)
}

/// Builds the tag tree of a tagged export.
#[derive(Default)]
struct Tagger {
    root: krilla::tagging::TagTree,
    /// Text leaves of the current frame.
    para: Vec<krilla::tagging::Node>,
}

impl Tagger {
    fn end_frame(&mut self) {
        if !self.para.is_empty() {
            let children = std::mem::take(&mut self.para);
            self.root.push(krilla::tagging::TagGroup::with_children(
                krilla::tagging::Tag::P,
                children,
            ));
        }
    }
}

/// Draw one primitive, tagged when the export is tagged.
fn draw_tagged(
    surface: &mut Surface<'_>,
    prim: &Prim,
    frame: FrameId,
    tagger: &mut Option<Tagger>,
    opts: &PdfOptions,
    state: &mut DrawState<'_>,
) {
    use krilla::tagging::{Artifact, ArtifactType, ContentTag, Node as TagNode, SpanTag};
    let Some(tagger) = tagger else {
        draw_prim(
            surface,
            prim,
            state.images,
            opts,
            state.fonts,
            state.image_cache,
        );
        return;
    };
    let tag = match prim {
        Prim::Glyphs(..) => ContentTag::Span(SpanTag::empty()),
        Prim::Image { .. } => ContentTag::Other,
        _ => ContentTag::Artifact(Artifact::with_kind(ArtifactType::Layout)),
    };
    let artifact = matches!(tag, ContentTag::Artifact(_));
    let id = surface.start_tagged(tag);
    draw_prim(
        surface,
        prim,
        state.images,
        opts,
        state.fonts,
        state.image_cache,
    );
    surface.end_tagged();
    if artifact {
        return;
    }
    match prim {
        Prim::Image { .. } => {
            let alt = opts.alt_text.get(&frame).filter(|a| !a.is_empty()).cloned();
            tagger.root.push(krilla::tagging::TagGroup::with_children(
                krilla::tagging::Tag::Figure(alt),
                vec![TagNode::Leaf(id)],
            ));
        }
        _ => tagger.para.push(TagNode::Leaf(id)),
    }
}

struct DrawState<'a> {
    images: &'a dyn ImageResolver,
    fonts: &'a mut HashMap<(u64, u32), Option<Font>>,
    image_cache: &'a mut HashMap<AssetId, Option<krilla::image::Image>>,
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
    let icc = output_profile(opts)?;
    let mut doc = Document::new_with(serialize_settings(opts, &icc)?);
    let mut meta = krilla::metadata::Metadata::new().creator("pubrs".to_string());
    if opts.standard != PdfStandard::None {
        meta = meta.creation_date(today());
    }
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
    let mut tagger = opts.tagged.then(Tagger::default);
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
            if tagger.is_some() {
                surface.start_tagged(krilla::tagging::ContentTag::Artifact(
                    krilla::tagging::Artifact::with_kind(krilla::tagging::ArtifactType::Page),
                ));
            }
            draw_crop_marks(&mut surface, scene, s, b.max(MARK_MIN_OFFSET));
            if tagger.is_some() {
                surface.end_tagged();
            }
        }
        if let Some(clip) = rect_path(s - b, s - b, scene.width + 2.0 * b, scene.height + 2.0 * b) {
            surface.push_clip_path(&clip, &FillRule::NonZero);
        }
        let base = Affine::translate(s, s);
        let mut open_clip: Option<pubrs_core::Rect> = None;
        for node in &scene.nodes {
            if node.clip != open_clip {
                if open_clip.is_some() {
                    surface.pop();
                }
                if let Some(c) = node.clip
                    && let Some(p) = rect_path(s + c.x, s + c.y, c.width, c.height)
                {
                    surface.push_clip_path(&p, &FillRule::NonZero);
                } else if node.clip.is_some() {
                    return Err(PdfError::Krilla("invalid clip rectangle".into()));
                }
                open_clip = node.clip;
            }
            surface.push_transform(&kt(node.transform.then(base)));
            let layered = node.opacity < 1.0;
            if layered {
                surface.push_opacity(
                    NormalizedF32::new(node.opacity as f32).unwrap_or(NormalizedF32::ONE),
                );
            }
            let mut state = DrawState {
                images,
                fonts: &mut fonts,
                image_cache: &mut image_cache,
            };
            for prim in &node.prims {
                draw_tagged(
                    &mut surface,
                    prim,
                    node.frame,
                    &mut tagger,
                    opts,
                    &mut state,
                );
            }
            if let Some(t) = &mut tagger {
                t.end_frame();
            }
            if layered {
                surface.pop();
            }
            surface.pop();
        }
        if open_clip.is_some() {
            surface.pop();
        }
        surface.pop();
        surface.finish();
        page.finish();
    }
    if let Some(t) = tagger {
        doc.set_tag_tree(t.root);
    }
    let bytes = doc
        .finish()
        .map_err(|e| PdfError::Krilla(format!("{e:?}")))?;
    if opts.standard != PdfStandard::PdfX4 {
        return Ok(bytes);
    }
    let cond = if opts.output_intent_icc.is_some() {
        "Custom CMYK output profile"
    } else {
        GENERIC_CMYK_CONDITION
    };
    let title = opts.title.as_deref().unwrap_or("Untitled");
    let out = crate::pdfx::make_pdfx4(&bytes, title, &icc, cond).ok_or_else(|| {
        PdfError::Krilla("could not add the PDF/X-4 output intent to the PDF".into())
    })?;
    let issues = crate::pdfx::check(&out);
    if issues.is_empty() {
        Ok(out)
    } else {
        Err(PdfError::NotPdfX4(issues))
    }
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
        Prim::Fill { geom, paint } => {
            if let Some(p) = geom_path(*geom) {
                surface.set_stroke(None);
                surface.set_fill(Some(fill_of_paint(paint, geom)));
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
                    paint: color_of(color).into(),
                    width: *width as f32,
                    opacity: alpha_of(color.rgba),
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
                surface.set_fill(Some(fill_of_ink(color)));
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
                        .or_insert_with(|| krilla_image(img, opts.standard == PdfStandard::None))
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
        Prim::Glyphs(run, ink) => draw_run(surface, run, ink, opts, fonts),
    }
}

fn krilla_image(
    img: &crate::images::RasterImage,
    interpolate: bool,
) -> Option<krilla::image::Image> {
    let data = || Data::from(Arc::new(img.encoded.to_vec()));
    let from_pixels = || {
        let pm = img.pixmap.clone();
        krilla::image::Image::from_rgba8(pm.take_demultiplied(), img.width, img.height)
    };
    if img.passthrough {
        match img.format {
            ImageFormat::Jpeg => krilla::image::Image::from_jpeg(data(), interpolate).ok(),
            ImageFormat::Png => krilla::image::Image::from_png(data(), interpolate).ok(),
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
    ink: &ResolvedInk,
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
    surface.set_fill(Some(fill_of_ink(ink)));
    if run.synthetic_bold {
        surface.set_stroke(Some(Stroke {
            paint: color_of(ink).into(),
            width: (run.font_size * 0.03) as f32,
            line_join: LineJoin::Round,
            opacity: alpha_of(ink.rgba),
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

/// The full name of a font ("Inter Bold"), for reports such as the print
/// package's font list.
pub fn font_name(font: &pubrs_layout::types::FontHandle) -> Option<String> {
    use skrifa::MetadataProvider;
    let f = skrifa::FontRef::from_index(font.bytes(), font.index).ok()?;
    let name = f
        .localized_strings(skrifa::string::StringId::FULL_NAME)
        .english_or_first()?
        .chars()
        .collect::<String>();
    Some(name)
}

/// The distinct fonts the glyph runs of `pages` use, sorted by name.
pub fn fonts_used(pages: &[PageScene]) -> Vec<String> {
    let mut names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut seen: std::collections::HashSet<(u64, u32)> = std::collections::HashSet::new();
    for node in pages.iter().flat_map(|p| &p.nodes) {
        for prim in &node.prims {
            if let Prim::Glyphs(run, _) = prim
                && seen.insert(run.font.key())
            {
                names.insert(font_name(&run.font).unwrap_or_else(|| "(unnamed font)".into()));
            }
        }
    }
    names.into_iter().collect()
}

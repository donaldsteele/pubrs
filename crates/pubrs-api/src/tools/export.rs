//! Export tools: export_pdf, export_png, render_preview.

use base64::Engine;
use std::collections::BTreeMap;
use std::sync::Arc;

use pubrs_core::{Document, DocumentId, Frame, FrameId, PageId, Story, StoryId, Touched};
use pubrs_layout::DocumentLayout;
use pubrs_render::{FontEmbedding, PdfOptions, RasterOptions};
use pubrs_render::{
    ImageSnapshot, PageDamage, PageScene, PageState, PixelRect, Region, SceneOptions,
    build_page_scene_with, page_damage, page_fingerprint,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, ApiResult};
use crate::session::Session;

/// Font handling in PDFs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FontEmbeddingIn {
    /// Embed subsets of the fonts (selectable text). Default.
    #[default]
    Subset,
    /// Convert text to outlines (no fonts embedded).
    Outlines,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportPdf {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Output file path. Without a path the PDF is returned as base64.
    #[serde(default)]
    pub path: Option<String>,
    /// Include the document's bleed (default: true when the document has bleed).
    #[serde(default)]
    pub bleed: Option<bool>,
    /// Add crop marks outside the bleed (default false).
    #[serde(default)]
    pub crop_marks: bool,
    #[serde(default)]
    pub font_embedding: FontEmbeddingIn,
    /// Impose the pages onto label sheets or booklet sheets when the document
    /// has a sheet or booklet set (default true). false exports the plain pages.
    #[serde(default)]
    pub impose: Option<bool>,
    /// Also return the bytes as base64 when a path is given.
    #[serde(default)]
    pub return_base64: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ExportPdfOut {
    pub doc_id: DocumentId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub bytes: u64,
    /// Pages in the PDF (sheets when imposed).
    pub page_count: usize,
    /// "sheet" or "booklet" when the PDF is imposed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imposed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base64: Option<String>,
    pub revision: u64,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportPng {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Page to render (default: first page).
    #[serde(default)]
    pub page_id: Option<PageId>,
    /// Resolution in dots per inch (default 150).
    #[serde(default)]
    pub dpi: Option<f64>,
    /// Include the bleed area.
    #[serde(default)]
    pub include_bleed: bool,
    /// Output file path. Without a path the PNG is returned as base64.
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub return_base64: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ExportPngOut {
    pub doc_id: DocumentId,
    pub page_id: PageId,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base64: Option<String>,
    pub revision: u64,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderPreview {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Page to look at (default: first page).
    #[serde(default)]
    pub page_id: Option<PageId>,
    /// Resolution (default 60 dpi, max 150) — low so the image is cheap to view.
    #[serde(default)]
    pub dpi: Option<f64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RenderPreviewOut {
    pub doc_id: DocumentId,
    pub page_id: PageId,
    pub width: u32,
    pub height: u32,
    pub mime_type: String,
    /// PNG image, base64.
    pub png_base64: String,
    pub revision: u64,
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn write_file(path: &str, bytes: &[u8]) -> ApiResult<()> {
    let p = std::path::Path::new(path);
    if let Some(dir) = p.parent()
        && !dir.as_os_str().is_empty()
        && !dir.exists()
    {
        std::fs::create_dir_all(dir).map_err(|e| {
            ApiError::new(
                "io_error",
                format!("cannot create {}: {e}", dir.display()),
                "check the path",
            )
        })?;
    }
    pubrs_io::format::write_atomic(p, bytes).map_err(Into::into)
}

impl Session {
    fn page_index_or_first(
        &self,
        doc_id: Option<DocumentId>,
        page: Option<PageId>,
    ) -> ApiResult<usize> {
        match page {
            Some(p) => self.require_page(doc_id, p),
            None => Ok(0),
        }
    }

    /// Render a page to PNG bytes (used by tools, resources and the CLI).
    /// With `preview`, empty picture frames are drawn as grey boxes.
    pub fn page_png(
        &mut self,
        doc_id: Option<DocumentId>,
        index: usize,
        opts: &RasterOptions,
        preview: bool,
    ) -> ApiResult<(u32, u32, Vec<u8>)> {
        let layout = self.layout(doc_id)?;
        let id = self.resolve(doc_id)?;
        self.rasters += 1;
        let parts = self.doc_with_caches(id)?;
        let scene = pubrs_render::SceneOptions {
            show_empty_images: preview,
        };
        let pm = parts
            .ctx
            .render_page_with(parts.doc, &layout, index, opts, scene)?;
        let (w, h) = (pm.width(), pm.height());
        let png = pm
            .encode_png()
            .map_err(|e| ApiError::new("export_failed", e.to_string(), "try a lower dpi"))?;
        Ok((w, h, png))
    }

    /// Render a page to straight RGBA8 for interactive display, using the GPU
    /// when available (feature `gpu`).
    pub fn page_rgba(
        &mut self,
        doc_id: Option<DocumentId>,
        index: usize,
        opts: &RasterOptions,
    ) -> ApiResult<(u32, u32, Vec<u8>)> {
        let layout = self.layout(doc_id)?;
        let id = self.resolve(doc_id)?;
        self.rasters += 1;
        let parts = self.doc_with_caches(id)?;
        let scene = pubrs_render::build_page_scene_with(
            parts.doc,
            &layout,
            index,
            pubrs_render::SceneOptions {
                show_empty_images: true,
            },
        )
        .ok_or_else(|| {
            ApiError::not_found("page", &index.to_string(), "list pages with page_list")
        })?;
        parts.ctx.images.sync(parts.doc);
        let software = parts.software;
        let r = parts
            .rasterizer
            .get_or_insert_with(|| new_rasterizer(software));
        r.render_rgba(&scene, &parts.ctx.images, opts)
            .map_err(|e| ApiError::new("export_failed", e.to_string(), "try a smaller zoom"))
    }

    /// Which raster path interactive rendering uses.
    pub fn render_backend(&mut self) -> String {
        let software = self.config.software_rendering;
        self.rasterizer
            .get_or_insert_with(|| new_rasterizer(software))
            .describe()
    }

    /// Small first-page PNG stored in saved files.
    pub fn thumbnail_png(&mut self, doc_id: Option<DocumentId>) -> ApiResult<Vec<u8>> {
        let size = self.get(doc_id)?.doc().setup.size;
        let dpi = (256.0 * 72.0 / size.width.max(size.height)).clamp(4.0, 72.0);
        Ok(self
            .page_png(
                doc_id,
                0,
                &RasterOptions {
                    dpi,
                    ..Default::default()
                },
                false,
            )?
            .2)
    }

    pub fn export_pdf(&mut self, req: ExportPdf) -> ApiResult<ExportPdfOut> {
        let id = self.resolve(req.doc_id)?;
        let layout = self.layout(Some(id))?;
        let parts = self.doc_with_caches(id)?;
        let doc = parts.doc;
        let opts = PdfOptions {
            bleed: req.bleed.unwrap_or(doc.setup.bleed > 0.0),
            crop_marks: req.crop_marks,
            font_embedding: match req.font_embedding {
                FontEmbeddingIn::Subset => FontEmbedding::Subset,
                FontEmbeddingIn::Outlines => FontEmbedding::Outlines,
            },
            title: None,
            author: None,
            flat: !req.impose.unwrap_or(true),
        };
        let pdf = parts.ctx.export_pdf_with(doc, &layout, &opts)?;
        let revision = parts.revision;
        let (page_count, imposed) = if opts.flat {
            (doc.pages.len(), None)
        } else {
            (
                pubrs_render::impose::output_pages(&doc.setup, doc.pages.len()),
                pubrs_render::impose::kind(&doc.setup).map(String::from),
            )
        };
        let path = match &req.path {
            Some(p) => Some(self.resolve_path(p)),
            None => self.default_path(Some(id), "pdf"),
        };
        if let Some(p) = &path {
            write_file(&p.display().to_string(), &pdf)?;
        }
        Ok(ExportPdfOut {
            doc_id: id,
            path: path.as_ref().map(|p| p.display().to_string()),
            bytes: pdf.len() as u64,
            page_count,
            imposed,
            base64: (path.is_none() || req.return_base64).then(|| b64(&pdf)),
            revision,
        })
    }

    pub fn export_png(&mut self, req: ExportPng) -> ApiResult<ExportPngOut> {
        let id = self.resolve(req.doc_id)?;
        let index = self.page_index_or_first(Some(id), req.page_id)?;
        let dpi = req.dpi.unwrap_or(150.0);
        if !(dpi.is_finite() && (1.0..=1200.0).contains(&dpi)) {
            return Err(ApiError::invalid(
                "dpi must be between 1 and 1200",
                "150 is typical, 300 for print",
            ));
        }
        let (w, h, png) = self.page_png(
            Some(id),
            index,
            &RasterOptions {
                dpi,
                include_bleed: req.include_bleed,
                ..Default::default()
            },
            false,
        )?;
        let path = match &req.path {
            Some(p) => Some(self.resolve_path(p)),
            None => self
                .default_path(Some(id), "png")
                .map(|p| p.with_extension(format!("p{}.png", index + 1))),
        };
        if let Some(p) = &path {
            write_file(&p.display().to_string(), &png)?;
        }
        let open = self.get(Some(id))?;
        Ok(ExportPngOut {
            doc_id: id,
            page_id: open.doc().pages[index].id,
            width: w,
            height: h,
            bytes: png.len() as u64,
            path: path.as_ref().map(|p| p.display().to_string()),
            base64: (path.is_none() || req.return_base64).then(|| b64(&png)),
            revision: open.revision(),
        })
    }

    pub fn render_preview(&mut self, req: RenderPreview) -> ApiResult<RenderPreviewOut> {
        let id = self.resolve(req.doc_id)?;
        let index = self.page_index_or_first(Some(id), req.page_id)?;
        let dpi = req.dpi.unwrap_or(60.0).clamp(10.0, 150.0);
        let (w, h, png) = self.page_png(
            Some(id),
            index,
            &RasterOptions {
                dpi,
                ..Default::default()
            },
            true,
        )?;
        let open = self.get(Some(id))?;
        Ok(RenderPreviewOut {
            doc_id: id,
            page_id: open.doc().pages[index].id,
            width: w,
            height: h,
            mime_type: "image/png".into(),
            png_base64: b64(&png),
            revision: open.revision(),
        })
    }
}

fn new_rasterizer(software: bool) -> pubrs_render::Rasterizer {
    if software {
        pubrs_render::Rasterizer::cpu()
    } else {
        pubrs_render::Rasterizer::auto()
    }
}

/// An immutable view of one page for render threads (the GUI's viewport
/// worker): never touched by the session again, cheap to make and clone.
#[derive(Debug, Clone)]
pub struct PageSnapshot {
    pub doc_id: DocumentId,
    pub revision: u64,
    /// The page's index in the document.
    pub page_index: usize,
    /// A document holding just this page (as page 0), its master, the
    /// frames on them, the stories they show with their chains' frames,
    /// any extra frames asked for, and the image assets drawn. Frames and
    /// stories are the session document's own `Arc`s, so unchanged ones
    /// compare pointer-equal across snapshots.
    pub doc: Arc<Document>,
    /// Layout of the whole document at `revision` (shared, not copied).
    pub layout: Arc<DocumentLayout>,
    pub images: ImageSnapshot,
}

impl PageSnapshot {
    /// The page as [`pubrs_render::damage`] sees it.
    pub fn state(&self) -> PageState<'_> {
        PageState {
            doc: &self.doc,
            layout: &self.layout,
            page: 0,
        }
    }

    /// The page's display list as the editor shows it (empty picture
    /// frames drawn as placeholders).
    pub fn scene(&self) -> ApiResult<PageScene> {
        build_page_scene_with(
            &self.doc,
            &self.layout,
            0,
            SceneOptions {
                show_empty_images: true,
            },
        )
        .ok_or_else(|| {
            ApiError::not_found(
                "page",
                &self.page_index.to_string(),
                "list pages with page_list",
            )
        })
    }

    /// A hash of how the page draws (see [`pubrs_render::page_fingerprint`]).
    pub fn fingerprint(&self) -> u64 {
        page_fingerprint(self.state())
    }

    /// What changed on the page since `older` (a snapshot of the same
    /// page). `touched` narrows the search when every change in between is
    /// known (see [`crate::session::touched_between`]).
    pub fn damage_since(&self, older: &PageSnapshot, touched: Option<&Touched>) -> PageDamage {
        if older.doc_id != self.doc_id {
            return PageDamage::Full;
        }
        page_damage(older.state(), self.state(), touched)
    }
}

/// Region pixels to redraw for page-point damage: each rectangle covered by
/// whole pixels, grown by two for anti-aliasing and clipped to the region.
/// `None` means the whole region.
pub fn damage_pixels(
    damage: &PageDamage,
    scene: &PageScene,
    region: &Region,
) -> Option<Vec<PixelRect>> {
    let PageDamage::Rects(rects) = damage else {
        return None;
    };
    let to_px = region.transform(scene);
    let bounds = PixelRect::new(0, 0, region.rect.width, region.rect.height);
    Some(
        rects
            .iter()
            .filter_map(|r| {
                let (x0, y0) = to_px.apply(r.x, r.y);
                let (x1, y1) = to_px.apply(r.right(), r.bottom());
                PixelRect::covering(x0.min(x1), y0.min(y1), x0.max(x1), y1.max(y0))
                    .outset(2)
                    .intersect(&bounds)
            })
            .collect(),
    )
}

/// The page-raster pixels a viewport renders: the visible pixels grown by a
/// quarter of the viewport's size on every side (so small pans need no
/// render), clipped to the page raster `page_px` and to `max_side` pixels a
/// side (the GPU's largest texture). The margin shrinks first; when even the
/// visible pixels exceed `max_side`, the region is their middle (the caller
/// lowers the resolution to avoid that, see the canvas).
pub fn viewport_region(
    visible: PixelRect,
    page_px: (u32, u32),
    max_side: u32,
) -> Option<PixelRect> {
    let page = PixelRect::new(0, 0, page_px.0, page_px.1);
    let shown = visible.intersect(&page)?;
    let (mx, my) = (visible.width as i32 / 4, visible.height as i32 / 4);
    let grown = PixelRect::from_ltrb(
        visible.x - mx,
        visible.y - my,
        visible.right() + mx,
        visible.bottom() + my,
    )
    .intersect(&page)?;
    let max = max_side.max(1) as i32;
    let (x0, x1) = clamp_span((shown.x, shown.right()), (grown.x, grown.right()), max);
    let (y0, y1) = clamp_span((shown.y, shown.bottom()), (grown.y, grown.bottom()), max);
    Some(PixelRect::from_ltrb(x0, y0, x1, y1))
}

/// One axis of [`viewport_region`]: `grown` (holding `shown`) cut to at most
/// `max`, keeping `shown` and sharing the margin between its two sides.
fn clamp_span(shown: (i32, i32), grown: (i32, i32), max: i32) -> (i32, i32) {
    if grown.1 - grown.0 <= max {
        return grown;
    }
    let width = shown.1 - shown.0;
    if width >= max {
        let start = shown.0 + (width - max) / 2;
        return (start, start + max);
    }
    let extra = max - width;
    let (left_room, right_room) = (shown.0 - grown.0, grown.1 - shown.1);
    let left = left_room.min(extra / 2);
    let right = right_room.min(extra - left);
    let left = left_room.min(extra - right);
    (shown.0 - left, shown.1 + right)
}
impl Session {
    /// Snapshot page `index` for a render thread (see [`PageSnapshot`]).
    /// `extra_frames` (e.g. the selection) and `extra_story` (the story
    /// being edited) are included with their stories for overlays. Costs
    /// what the page holds, not what the document holds.
    pub fn page_snapshot(
        &mut self,
        doc_id: Option<DocumentId>,
        index: usize,
        extra_frames: &[FrameId],
        extra_story: Option<StoryId>,
    ) -> ApiResult<PageSnapshot> {
        let layout = self.layout(doc_id)?;
        let id = self.resolve(doc_id)?;
        let parts = self.doc_with_caches(id)?;
        let doc = parts.doc;
        if index >= doc.pages.len() {
            return Err(ApiError::not_found(
                "page",
                &index.to_string(),
                "list pages with page_list",
            ));
        }
        parts.ctx.images.sync(doc);
        let images = parts.ctx.images.snapshot();
        Ok(PageSnapshot {
            doc_id: id,
            revision: parts.revision,
            page_index: index,
            doc: Arc::new(page_document(doc, index, extra_frames, extra_story)),
            layout,
            images,
        })
    }
}

/// The part of a document one page draws (see [`PageSnapshot::doc`]).
pub fn page_document(
    doc: &Document,
    index: usize,
    extra_frames: &[FrameId],
    extra_story: Option<StoryId>,
) -> Document {
    fn add_story(
        doc: &Document,
        story: StoryId,
        frames: &mut BTreeMap<FrameId, Arc<Frame>>,
        stories: &mut BTreeMap<StoryId, Arc<Story>>,
    ) {
        if stories.contains_key(&story) {
            return;
        }
        let Some(s) = doc.stories.get(&story) else {
            return;
        };
        for f in &s.frames {
            if let Some(frame) = doc.frames.get(f) {
                frames.entry(*f).or_insert_with(|| frame.clone());
            }
        }
        stories.insert(story, s.clone());
    }
    let shell = |pages, masters, frames, stories, assets| Document {
        schema_version: doc.schema_version,
        id: doc.id,
        meta: doc.meta.clone(),
        setup: doc.setup.clone(),
        grid: doc.grid,
        pages: Arc::new(pages),
        masters: Arc::new(masters),
        frames,
        stories,
        styles: doc.styles.clone(),
        assets,
        blobs: BTreeMap::new(),
        swatches: doc.swatches.clone(),
        scheme: doc.scheme.clone(),
    };
    let Some(page) = doc.pages.get(index).cloned() else {
        return shell(
            Vec::new(),
            Vec::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
        );
    };
    let masters: Vec<_> = page
        .master
        .and_then(|m| doc.master(m))
        .cloned()
        .into_iter()
        .collect();
    let mut frames = BTreeMap::new();
    let mut stories = BTreeMap::new();
    let mut assets = BTreeMap::new();
    let seeds = page
        .frames
        .iter()
        .chain(masters.iter().flat_map(|m| m.frames.iter()))
        .chain(extra_frames);
    for id in seeds.flat_map(|f| doc.descendants(*f)) {
        let Some(f) = doc.frames.get(&id) else {
            continue;
        };
        frames.insert(id, f.clone());
        if let Some(t) = f.as_text() {
            add_story(doc, t.story, &mut frames, &mut stories);
        }
        if let Some(a) = f.as_image().and_then(|i| i.asset)
            && let Some(asset) = doc.assets.get(&a)
        {
            assets.insert(a, asset.clone());
        }
    }
    if let Some(s) = extra_story {
        add_story(doc, s, &mut frames, &mut stories);
    }
    shell(vec![page], masters, frames, stories, assets)
}

#[cfg(test)]
mod tests {
    use pubrs_core::{
        Color, Command, DocBuilder, DocumentId, FrameStyle, PageSetup, Rect, SeqIds, Shape,
    };
    use pubrs_render::{TileOptions, TileRenderer};

    use super::*;
    use crate::SessionConfig;
    use crate::session::touched_between;

    #[test]
    fn viewport_region_adds_a_quarter_margin_inside_the_page() {
        let r = viewport_region(PixelRect::new(100, 200, 400, 300), (1000, 1000), 8192);
        assert_eq!(r, Some(PixelRect::new(0, 125, 600, 450)));
        let off = viewport_region(PixelRect::new(-900, 0, 100, 100), (1000, 1000), 8192);
        assert_eq!(off, None);
    }

    #[test]
    fn viewport_region_fits_the_largest_texture() {
        // A 6K portrait screen at 2x: the margin shrinks so the region fits.
        let visible = PixelRect::new(3000, 4000, 3384, 6016);
        let r = viewport_region(visible, (20_000, 30_000), 8192).expect("region");
        assert!(r.width <= 8192 && r.height <= 8192, "{r:?}");
        assert!(
            r.contains(&visible),
            "the visible pixels stay inside: {r:?}"
        );
        assert_eq!(r.height, 8192, "the margin is kept as far as it fits");
        // Even the visible pixels are too many: the region is the middle of them.
        let huge = PixelRect::new(0, 0, 9000, 500);
        let r = viewport_region(huge, (20_000, 20_000), 8192).expect("region");
        assert_eq!((r.width, r.x), (8192, 404), "{r:?}");
        assert!(huge.contains(&PixelRect::new(r.x, r.y, r.width, 500)));
        // Near a page edge the margin goes where there is page.
        let edge = PixelRect::new(0, 0, 7000, 100);
        let r = viewport_region(edge, (8000, 100), 7500).expect("region");
        assert_eq!((r.x, r.width), (0, 7500));
    }

    /// Apply `edits` (`None` = undo) one by one, keeping a texture of `region`
    /// up to date by damage repaints only, and check after each that it
    /// equals a full render of the current revision byte for byte.
    fn check_damage_repaints(
        s: &mut Session,
        id: DocumentId,
        region: Region,
        edits: Vec<Option<Command>>,
    ) {
        let mut tiles = TileRenderer::new(TileOptions::fixed(4, 97, 41));
        let whole = PixelRect::new(0, 0, region.rect.width, region.rect.height);
        let render = |tiles: &mut TileRenderer, snap: &PageSnapshot, rects: &[PixelRect]| {
            let scene = snap.scene().expect("scene");
            tiles
                .render_rects(&scene, &snap.images, &region, rects)
                .expect("render")
        };
        let mut shown = s.page_snapshot(Some(id), 0, &[], None).expect("snapshot");
        let mut texture = render(&mut tiles, &shown, &[whole]).remove(0).1;
        for edit in edits {
            match edit {
                Some(c) => {
                    s.exec(Some(id), c).expect("edit");
                }
                None => {
                    s.get_mut(Some(id)).expect("doc").editor.undo();
                }
            }
            let snap = s.page_snapshot(Some(id), 0, &[], None).expect("snapshot");
            let open = s.get(Some(id)).expect("doc");
            let touched = touched_between(
                &open.changes_since(shown.revision),
                shown.revision,
                snap.revision,
            );
            let damage = snap.damage_since(&shown, touched.as_ref());
            let scene = snap.scene().expect("scene");
            let rects = damage_pixels(&damage, &scene, &region).unwrap_or_else(|| vec![whole]);
            assert!(!rects.is_empty(), "every edit here changes pixels");
            for (r, pm) in render(&mut tiles, &snap, &rects) {
                let w = texture.width() as usize;
                for row in 0..r.height as usize {
                    let dst = ((r.y as usize + row) * w + r.x as usize) * 4;
                    let src = row * r.width as usize * 4;
                    let n = r.width as usize * 4;
                    texture.data_mut()[dst..dst + n].copy_from_slice(&pm.data()[src..src + n]);
                }
            }
            let full = render(&mut tiles, &snap, &[whole]).remove(0).1;
            let first = texture
                .data()
                .chunks(4)
                .zip(full.data().chunks(4))
                .position(|(a, b)| a != b)
                .map(|i| (i % full.width() as usize, i / full.width() as usize));
            assert!(
                first.is_none(),
                "after {damage:?} the patched texture differs from a full render at {first:?}"
            );
            shown = snap;
        }
    }

    fn session() -> Session {
        Session::new(SessionConfig {
            software_rendering: true,
            ..Default::default()
        })
    }

    #[test]
    fn damage_repaints_keep_the_texture_exact() {
        let mut b = DocBuilder::new(SeqIds::new(7), PageSetup::default());
        let p = b.page(0);
        let text = b
            .text(
                p,
                Rect::new(72.0, 72.0, 200.0, 300.0),
                "Damage repaint keeps the texture equal to a full render.\nSecond paragraph.",
            )
            .expect("text");
        let next = b
            .text(p, Rect::new(300.0, 72.0, 200.0, 120.0), "")
            .expect("text");
        b.link(text, next).expect("link");
        let box_ = b
            .shape(
                p,
                Rect::new(100.0, 450.0, 120.0, 80.0),
                Shape::Rectangle { corner_radius: 0.0 },
            )
            .expect("shape");
        b.style(
            box_,
            FrameStyle {
                fill: Some(Color::rgb(200, 30, 30).into()),
                opacity: 0.7,
                ..Default::default()
            },
        )
        .expect("style");
        let story = b.story_of(text);
        let mut s = session();
        let id = s.insert(b.finish(), None);
        let region = Region {
            opts: RasterOptions {
                dpi: 100.0,
                include_bleed: true,
                ..Default::default()
            },
            rect: PixelRect::new(30, 40, 700, 900),
            mips: true,
        };
        check_damage_repaints(
            &mut s,
            id,
            region,
            vec![
                Some(Command::InsertText {
                    story,
                    at: 7,
                    text: "s".into(),
                    format: None,
                }),
                Some(Command::InsertText {
                    story,
                    at: 0,
                    text: "More words that flow on into the next frame. ".repeat(12),
                    format: None,
                }),
                Some(Command::SetFrameRect {
                    frame: box_,
                    rect: Rect::new(300.0, 400.0, 120.0, 80.0),
                }),
                Some(Command::SetRotation {
                    frame: box_,
                    degrees: 20.0,
                }),
                None, // undo: not logged, the whole page is compared
            ],
        );
    }

    /// The editor draws empty picture frames as placeholders whose 1 pt
    /// diagonals poke past the frame; at 800% on a 2x screen that is
    /// several pixels, so the damage must reach as far as the strokes.
    #[test]
    fn empty_picture_frames_repaint_exactly_at_high_zoom() {
        let mut b = DocBuilder::new(SeqIds::new(8), PageSetup::default());
        let p = b.page(0);
        let pic = b
            .image(p, Rect::new(40.0, 40.0, 50.0, 30.0), None)
            .expect("picture");
        let mut s = session();
        let id = s.insert(b.finish(), None);
        let dpi = 72.0 * 8.0 * 2.0;
        let region = Region {
            opts: RasterOptions {
                dpi,
                include_bleed: true,
                ..Default::default()
            },
            rect: PixelRect::new(500, 500, 1400, 1100),
            mips: true,
        };
        check_damage_repaints(
            &mut s,
            id,
            region,
            vec![
                Some(Command::SetFrameRect {
                    frame: pic,
                    rect: Rect::new(52.0, 47.0, 50.0, 30.0),
                }),
                Some(Command::SetFrameRect {
                    frame: pic,
                    rect: Rect::new(52.0, 47.0, 31.0, 44.0),
                }),
                Some(Command::SetRotation {
                    frame: pic,
                    degrees: 33.0,
                }),
                None,
            ],
        );
    }
}

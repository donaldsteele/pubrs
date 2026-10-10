//! Export orchestration: layout + image decoding + rendering.

use pubrs_core::Document;
use pubrs_layout::{DocumentLayout, FontLibrary, Layouter};
use pubrs_render::{
    PdfOptions, RasterOptions, RasterRenderer, RenderError, SceneOptions, build_page_scene,
    build_page_scene_with, render_pdf, tiny_skia,
};

use crate::images::ImageCache;

/// Errors from exporting.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("page {0} does not exist (document has {1} pages)")]
    NoSuchPage(usize, usize),
    #[error(transparent)]
    Render(#[from] RenderError),
    #[error("PDF export failed: {0}")]
    Pdf(String),
}

impl ExportError {
    pub fn code(&self) -> &'static str {
        match self {
            ExportError::NoSuchPage(..) => "not_found",
            ExportError::Render(RenderError::TooLarge(..)) => "invalid_argument",
            _ => "export_failed",
        }
    }
}

/// Holds the caches needed to lay out and draw documents. One per session
/// (or per thread); reusable across documents.
pub struct ExportContext {
    pub layouter: Layouter,
    pub images: ImageCache,
    pub raster: RasterRenderer,
}

impl std::fmt::Debug for ExportContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExportContext")
            .field("layouter", &self.layouter)
            .field("images", &self.images)
            .finish()
    }
}

impl ExportContext {
    pub fn new(fonts: FontLibrary) -> Self {
        Self {
            layouter: Layouter::new(fonts),
            images: ImageCache::new(),
            raster: RasterRenderer::new(),
        }
    }

    /// Lay out every story of `doc`.
    pub fn layout(&mut self, doc: &Document) -> DocumentLayout {
        self.layouter.layout_document(doc)
    }

    /// Render one page (0-based) to a premultiplied pixmap.
    pub fn render_page(
        &mut self,
        doc: &Document,
        layout: &DocumentLayout,
        page: usize,
        opts: &RasterOptions,
    ) -> Result<tiny_skia::Pixmap, ExportError> {
        self.render_page_with(doc, layout, page, opts, SceneOptions::default())
    }

    /// Like [`ExportContext::render_page`] with scene options (e.g. previews
    /// that show empty picture frames).
    pub fn render_page_with(
        &mut self,
        doc: &Document,
        layout: &DocumentLayout,
        page: usize,
        opts: &RasterOptions,
        scene_opts: SceneOptions,
    ) -> Result<tiny_skia::Pixmap, ExportError> {
        let scene = build_page_scene_with(doc, layout, page, scene_opts)
            .ok_or(ExportError::NoSuchPage(page, doc.pages.len()))?;
        self.images.sync(doc);
        Ok(self.raster.render(&scene, &self.images, opts)?)
    }

    /// Export every page to PDF.
    pub fn export_pdf(
        &mut self,
        doc: &Document,
        opts: &PdfOptions,
    ) -> Result<Vec<u8>, ExportError> {
        let layout = self.layout(doc);
        self.images.sync(doc);
        let scenes: Vec<_> = (0..doc.pages.len())
            .filter_map(|i| build_page_scene(doc, &layout, i))
            .collect();
        let mut opts = opts.clone();
        if opts.title.is_none() && !doc.meta.title.is_empty() {
            opts.title = Some(doc.meta.title.clone());
        }
        if opts.author.is_none() && !doc.meta.author.is_empty() {
            opts.author = Some(doc.meta.author.clone());
        }
        render_pdf(&scenes, &self.images, &opts).map_err(|e| ExportError::Pdf(e.to_string()))
    }

    /// Render one page (0-based) to PNG bytes, laying out the document first.
    pub fn page_png(
        &mut self,
        doc: &Document,
        page: usize,
        opts: &RasterOptions,
    ) -> Result<Vec<u8>, ExportError> {
        let layout = self.layout(doc);
        let pm = self.render_page(doc, &layout, page, opts)?;
        pm.encode_png()
            .map_err(|e| ExportError::Render(RenderError::Png(e.to_string())))
    }
}

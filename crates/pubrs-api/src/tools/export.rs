//! Export tools: export_pdf, export_png, render_preview.

use base64::Engine;
use pubrs_core::{DocumentId, PageId};
use pubrs_render::{FontEmbedding, PdfOptions, RasterOptions};
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
    pub page_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base64: Option<String>,
    pub revision: u64,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportPng {
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
    pub fn page_png(
        &mut self,
        doc_id: Option<DocumentId>,
        index: usize,
        opts: &RasterOptions,
    ) -> ApiResult<(u32, u32, Vec<u8>)> {
        let layout = self.layout(doc_id)?;
        let id = self.resolve(doc_id)?;
        let doc = self.get(Some(id))?.doc().clone();
        let pm = self.ctx.render_page(&doc, &layout, index, opts)?;
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
        let doc = self.get(Some(id))?.doc().clone();
        let scene = pubrs_render::build_page_scene(&doc, &layout, index).ok_or_else(|| {
            ApiError::not_found("page", &index.to_string(), "list pages with page_list")
        })?;
        self.ctx.images.sync(&doc);
        let r = self
            .rasterizer
            .get_or_insert_with(pubrs_render::Rasterizer::auto);
        r.render_rgba(&scene, &self.ctx.images, opts)
            .map_err(|e| ApiError::new("export_failed", e.to_string(), "try a smaller zoom"))
    }

    /// Which raster path interactive rendering uses.
    pub fn render_backend(&mut self) -> String {
        self.rasterizer
            .get_or_insert_with(pubrs_render::Rasterizer::auto)
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
            )?
            .2)
    }

    pub fn export_pdf(&mut self, req: ExportPdf) -> ApiResult<ExportPdfOut> {
        let id = self.resolve(req.doc_id)?;
        let doc = self.get(Some(id))?.doc().clone();
        let opts = PdfOptions {
            bleed: req.bleed.unwrap_or(doc.setup.bleed > 0.0),
            crop_marks: req.crop_marks,
            font_embedding: match req.font_embedding {
                FontEmbeddingIn::Subset => FontEmbedding::Subset,
                FontEmbeddingIn::Outlines => FontEmbedding::Outlines,
            },
            title: None,
            author: None,
        };
        let pdf = self.ctx.export_pdf(&doc, &opts)?;
        if let Some(p) = &req.path {
            write_file(p, &pdf)?;
        }
        Ok(ExportPdfOut {
            doc_id: id,
            path: req.path.clone(),
            bytes: pdf.len() as u64,
            page_count: doc.pages.len(),
            base64: (req.path.is_none() || req.return_base64).then(|| b64(&pdf)),
            revision: self.get(Some(id))?.revision(),
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
        )?;
        if let Some(p) = &req.path {
            write_file(p, &png)?;
        }
        let open = self.get(Some(id))?;
        Ok(ExportPngOut {
            doc_id: id,
            page_id: open.doc().pages[index].id,
            width: w,
            height: h,
            bytes: png.len() as u64,
            path: req.path.clone(),
            base64: (req.path.is_none() || req.return_base64).then(|| b64(&png)),
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

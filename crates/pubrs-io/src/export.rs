//! Export orchestration: layout + image decoding + rendering.

use pubrs_core::Document;
use pubrs_layout::{DocumentLayout, FontLibrary, Layouter};
use pubrs_render::impose::impose;
use pubrs_render::{
    PageScene, PdfOptions, PdfStandard, RasterOptions, RasterRenderer, RenderError, SceneOptions,
    build_page_scene, build_page_scene_with, render_pdf, tiny_skia,
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
        self.export_pdf_with(doc, &layout, opts)
    }

    /// Export every page to PDF with a layout the caller already has (for
    /// example a session's incremental one).
    pub fn export_pdf_with(
        &mut self,
        doc: &Document,
        layout: &DocumentLayout,
        opts: &PdfOptions,
    ) -> Result<Vec<u8>, ExportError> {
        // Sheets and booklets are imposed by default; `flat` opts out.
        let scenes = self.output_scenes(doc, layout, !opts.flat)?;
        let mut opts = opts.clone();
        if opts.title.is_none() && !doc.meta.title.is_empty() {
            opts.title = Some(doc.meta.title.clone());
        }
        if opts.author.is_none() && !doc.meta.author.is_empty() {
            opts.author = Some(doc.meta.author.clone());
        }
        if opts.tagged && opts.alt_text.is_empty() {
            opts.alt_text = alt_texts(doc);
        }
        render_pdf(&scenes, &self.images, &opts).map_err(|e| ExportError::Pdf(e.to_string()))
    }

    /// A zip for a commercial printer: the PDF/X-4 file, the `.pubrs`
    /// document with its assets, and a README listing the fonts and colours.
    /// `opts.standard` is forced to PDF/X-4.
    pub fn export_print_package(
        &mut self,
        doc: &Document,
        layout: &DocumentLayout,
        opts: &PdfOptions,
        stem: &str,
    ) -> Result<Vec<u8>, ExportError> {
        let mut opts = opts.clone();
        opts.standard = PdfStandard::PdfX4;
        let pdf = self.export_pdf_with(doc, layout, &opts)?;
        let scenes = self.output_scenes(doc, layout, !opts.flat)?;
        let fonts = pubrs_render::pdf::fonts_used(&scenes);
        let source =
            crate::format::write_bytes(doc, None).map_err(|e| ExportError::Pdf(e.to_string()))?;
        let readme = print_readme(doc, &fonts, stem, &opts);
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        // The PDF and the .pubrs are already compressed, so they are stored; the
        // zip is then the same size every time.
        let options = |m| zip::write::SimpleFileOptions::default().compression_method(m);
        for (name, bytes, method) in [
            (
                format!("{stem}-pdfx4.pdf"),
                pdf,
                zip::CompressionMethod::Stored,
            ),
            (
                format!("{stem}.pubrs"),
                source,
                zip::CompressionMethod::Stored,
            ),
            (
                "README.txt".to_string(),
                readme.into_bytes(),
                zip::CompressionMethod::Deflated,
            ),
        ] {
            zip.start_file(name, options(method))
                .map_err(|e| ExportError::Pdf(e.to_string()))?;
            std::io::Write::write_all(&mut zip, &bytes)
                .map_err(|e| ExportError::Pdf(e.to_string()))?;
        }
        let out = zip.finish().map_err(|e| ExportError::Pdf(e.to_string()))?;
        Ok(out.into_inner())
    }

    /// The scenes a PDF of `doc` is made of: one per page, or one per sheet
    /// (label sheet) or booklet side when `imposed` and the document has a
    /// sheet or booklet set up.
    pub fn output_scenes(
        &mut self,
        doc: &Document,
        layout: &DocumentLayout,
        imposed: bool,
    ) -> Result<Vec<PageScene>, ExportError> {
        self.images.sync(doc);
        let scenes: Vec<_> = (0..doc.pages.len())
            .filter_map(|i| build_page_scene(doc, layout, i))
            .collect();
        if imposed {
            impose(&scenes, &doc.setup).map_err(|e| ExportError::Pdf(e.to_string()))
        } else {
            Ok(scenes)
        }
    }

    /// Render imposed sheet `index` (0-based) to PNG bytes.
    pub fn sheet_png(
        &mut self,
        doc: &Document,
        index: usize,
        opts: &RasterOptions,
    ) -> Result<Vec<u8>, ExportError> {
        let layout = self.layout(doc);
        let scenes = self.output_scenes(doc, &layout, true)?;
        let scene = scenes
            .get(index)
            .ok_or(ExportError::NoSuchPage(index, scenes.len()))?;
        let pm = self.raster.render(scene, &self.images, opts)?;
        pm.encode_png()
            .map_err(|e| ExportError::Render(RenderError::Png(e.to_string())))
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

/// Alt text of every picture frame that has some.
pub fn alt_texts(doc: &Document) -> std::collections::HashMap<pubrs_core::FrameId, String> {
    doc.frames
        .iter()
        .filter_map(|(id, f)| {
            f.as_image()
                .filter(|i| !i.alt_text.is_empty())
                .map(|i| (*id, i.alt_text.clone()))
        })
        .collect()
}

fn cmyk_text(c: pubrs_core::Cmyk) -> String {
    format!("C{} M{} Y{} K{}", c.c, c.m, c.y, c.k)
}

/// The README of a print package.
fn print_readme(doc: &Document, fonts: &[String], stem: &str, opts: &PdfOptions) -> String {
    use pubrs_core::SwatchValue;
    let mut s = String::new();
    let title = if doc.meta.title.is_empty() {
        stem
    } else {
        &doc.meta.title
    };
    s.push_str(&format!(
        "{title}\n{}\n\n",
        "=".repeat(title.chars().count())
    ));
    s.push_str("Files\n-----\n");
    s.push_str(&format!("{stem}-pdfx4.pdf  the print-ready PDF/X-4 file\n"));
    s.push_str(&format!(
        "{stem}.pubrs      the editable pubrs document, with its images\n\n"
    ));
    s.push_str("Job\n---\n");
    s.push_str(&format!(
        "Pages: {}\nTrim size: {} x {} pt\nBleed: {} pt{}\nCrop marks: {}\n",
        doc.pages.len(),
        doc.setup.size.width,
        doc.setup.size.height,
        if opts.bleed { doc.setup.bleed } else { 0.0 },
        if opts.bleed { "" } else { " (not included)" },
        if opts.crop_marks { "yes" } else { "no" },
    ));
    s.push_str(&format!(
        "Standard: PDF/X-4, output intent {} (CMYK, embedded profile)\n\n",
        if opts.output_intent_icc.is_some() {
            "Custom CMYK output profile"
        } else {
            pubrs_render::pdf::GENERIC_CMYK_CONDITION
        }
    ));
    s.push_str("Fonts (embedded as subsets)\n---------------------------\n");
    if fonts.is_empty() {
        s.push_str("(no text)\n");
    }
    for f in fonts {
        s.push_str(&format!("{f}\n"));
    }
    s.push_str("\nColours (swatches)\n------------------\n");
    if doc.swatches.is_empty() {
        s.push_str("(no swatches; colours are RGB)\n");
    }
    for sw in doc.swatches.iter() {
        let v = match &sw.value {
            SwatchValue::Rgb(c) => format!("RGB {}", c.to_hex()),
            SwatchValue::Cmyk(c) => format!("CMYK {}", cmyk_text(*c)),
            SwatchValue::Spot { name, cmyk, .. } => {
                format!(
                    "Spot colour \"{name}\", alternative CMYK {}",
                    cmyk_text(*cmyk)
                )
            }
        };
        s.push_str(&format!("{}: {v}\n", sw.name));
    }
    s.push_str(
        "\nCMYK values are written as entered. Spot colours are separate plates named as above.\n",
    );
    s
}

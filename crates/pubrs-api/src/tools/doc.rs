//! Document tools: doc_new, doc_open, doc_save, doc_save_as, doc_close,
//! doc_list, doc_info, doc_set_page_size, doc_set_margins, doc_set_bleed,
//! doc_set_info, doc_set_grid.

use std::path::PathBuf;

use pubrs_core::{
    Command, DocumentId, GridSettings, Margins, MasterId, PageId, PageSetup, PageSizePreset, Size,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::require_confirm;
use crate::error::{ApiError, ApiResult};
use crate::session::{Changed, Session};

/// Page orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    #[default]
    Portrait,
    Landscape,
}

/// Page size: a preset or explicit width/height.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PageSizeIn {
    /// Named size: letter (8.5x11 in), legal, tabloid, a3, a4, a5, a6,
    /// business_card (3.5x2 in), postcard (4x6 in).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<PageSizePreset>,
    /// Custom width in points (used when no preset is given).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    /// Custom height in points (used when no preset is given).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
    /// Swap width and height for landscape presets. Default portrait.
    #[serde(default)]
    pub orientation: Orientation,
}

impl PageSizeIn {
    fn size(&self) -> ApiResult<Size> {
        let mut s = match (self.preset, self.width, self.height) {
            (Some(p), _, _) => p.size(),
            (None, Some(w), Some(h)) => Size::new(w, h),
            _ => {
                return Err(ApiError::invalid(
                    "give a preset or both width and height",
                    "e.g. {\"preset\": \"letter\"} or {\"width\": 612, \"height\": 792}",
                ));
            }
        };
        let landscape = self.orientation == Orientation::Landscape;
        if self.preset.is_some() && landscape == (s.width < s.height) {
            s = Size::new(s.height, s.width);
        }
        Ok(s)
    }
}

/// Margins in points. `all` sets every side; individual sides override it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MarginsIn {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<f64>,
}

impl MarginsIn {
    fn apply(&self, base: Margins) -> Margins {
        let a = self.all;
        Margins {
            top: self.top.or(a).unwrap_or(base.top),
            right: self.right.or(a).unwrap_or(base.right),
            bottom: self.bottom.or(a).unwrap_or(base.bottom),
            left: self.left.or(a).unwrap_or(base.left),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocNew {
    /// Page size (default letter portrait).
    #[serde(default)]
    pub page_size: Option<PageSizeIn>,
    /// Margins (default 36pt = 0.5in on every side).
    #[serde(default)]
    pub margins: Option<MarginsIn>,
    /// Bleed in points (default 0; 9pt = 0.125in is typical for print).
    #[serde(default)]
    pub bleed: Option<f64>,
    /// Number of pages (default 1).
    #[serde(default)]
    pub pages: Option<u32>,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DocNewOut {
    pub doc_id: DocumentId,
    pub page_ids: Vec<PageId>,
    pub revision: u64,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocOpen {
    /// Path of a .pubrs file.
    pub path: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DocOpenOut {
    pub doc_id: DocumentId,
    pub path: String,
    pub title: String,
    pub page_ids: Vec<PageId>,
    pub revision: u64,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocRef {
    /// Document (default: the most recently used).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocSaveAs {
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Destination path; ".pubrs" is appended if missing.
    pub path: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DocSaveOut {
    pub doc_id: DocumentId,
    pub path: String,
    pub bytes: u64,
    pub revision: u64,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocClose {
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Required when the document has unsaved changes (they are discarded).
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DocCloseOut {
    pub doc_id: DocumentId,
    pub closed: bool,
    /// True if unsaved changes were discarded.
    pub discarded_changes: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DocEntry {
    pub doc_id: DocumentId,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub revision: u64,
    /// True if there are unsaved changes.
    pub dirty: bool,
    pub page_count: usize,
    /// True for the document used when doc_id is omitted.
    pub current: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DocListOut {
    pub documents: Vec<DocEntry>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MasterEntry {
    pub master_id: MasterId,
    pub name: String,
    pub frame_count: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DocInfoOut {
    pub doc_id: DocumentId,
    pub title: String,
    pub author: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    pub revision: u64,
    pub dirty: bool,
    /// Trim size in points.
    pub page_size: Size,
    pub margins: Margins,
    pub bleed: f64,
    pub grid: GridSettings,
    pub page_ids: Vec<PageId>,
    pub masters: Vec<MasterEntry>,
    pub frame_count: usize,
    pub story_count: usize,
    pub style_count: usize,
    pub asset_count: usize,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct DocSetPageSize {
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    #[serde(flatten)]
    pub size: PageSizeIn,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct DocSetMargins {
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    #[serde(flatten)]
    pub margins: MarginsIn,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocSetBleed {
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Bleed in points on every side (0..=144). 9pt = 0.125in is standard.
    pub bleed: f64,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocSetInfo {
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocSetGrid {
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Grid spacing in points.
    #[serde(default)]
    pub spacing: Option<f64>,
    #[serde(default)]
    pub show_grid: Option<bool>,
    #[serde(default)]
    pub snap_to_grid: Option<bool>,
    #[serde(default)]
    pub snap_to_guides: Option<bool>,
    /// Snap distance in points.
    #[serde(default)]
    pub snap_tolerance: Option<f64>,
}

fn normalize_path(path: &str) -> PathBuf {
    let mut p = PathBuf::from(path);
    if p.extension()
        .is_none_or(|e| !e.eq_ignore_ascii_case("pubrs"))
    {
        let mut s = p.into_os_string();
        s.push(".pubrs");
        p = PathBuf::from(s);
    }
    p
}

impl Session {
    pub fn doc_new(&mut self, req: DocNew) -> ApiResult<DocNewOut> {
        let mut setup = PageSetup::default();
        if let Some(ps) = &req.page_size {
            setup.size = ps.size()?;
        }
        let doc = pubrs_core::Document::new(&mut *self.ids, setup);
        let doc_id = self.insert(doc, None);
        let mut batch = Vec::new();
        if let Some(ps) = &req.page_size {
            batch.push(Command::SetPageSize { size: ps.size()? });
        }
        if let Some(m) = &req.margins {
            batch.push(Command::SetMargins {
                margins: m.apply(setup.margins),
            });
        }
        if let Some(b) = req.bleed {
            batch.push(Command::SetBleed { bleed: b });
        }
        if let Some(t) = &req.title {
            batch.push(Command::SetMeta {
                title: Some(t.clone()),
                author: None,
            });
        }
        let extra = req.pages.unwrap_or(1).clamp(1, 500) - 1;
        for _ in 0..extra {
            batch.push(Command::AddPage {
                page: pubrs_core::Page::new(PageId::new(&mut *self.ids)),
                index: None,
            });
        }
        // Applied directly (not as undo history) so a new document starts clean.
        let open = self.get_mut(Some(doc_id))?;
        let mut doc = open.doc().clone();
        for c in batch {
            if let Err(e) = c.apply(&mut doc) {
                self.remove(doc_id);
                return Err(e.into());
            }
        }
        open.editor.reset(doc);
        open.editor.mark_saved();
        let open = self.get(Some(doc_id))?;
        Ok(DocNewOut {
            doc_id,
            page_ids: open.doc().pages.iter().map(|p| p.id).collect(),
            revision: open.revision(),
        })
    }

    pub fn doc_open(&mut self, req: DocOpen) -> ApiResult<DocOpenOut> {
        let path = PathBuf::from(&req.path);
        let doc = pubrs_io::load(&path)?;
        let title = doc.meta.title.clone();
        let page_ids = doc.pages.iter().map(|p| p.id).collect();
        let doc_id = self.insert(doc, Some(path.clone()));
        Ok(DocOpenOut {
            doc_id,
            path: path.display().to_string(),
            title,
            page_ids,
            revision: self.get(Some(doc_id))?.revision(),
        })
    }

    fn save_to(&mut self, doc_id: Option<DocumentId>, path: PathBuf) -> ApiResult<DocSaveOut> {
        let id = self.resolve(doc_id)?;
        let thumb = self.thumbnail_png(Some(id)).ok();
        let open = self.get_mut(Some(id))?;
        let bytes = pubrs_io::write_bytes(open.doc(), thumb.as_deref())?;
        pubrs_io::format::write_atomic(&path, &bytes)?;
        open.path = Some(path.clone());
        open.editor.mark_saved();
        Ok(DocSaveOut {
            doc_id: id,
            path: path.display().to_string(),
            bytes: bytes.len() as u64,
            revision: open.revision(),
        })
    }

    pub fn doc_save(&mut self, req: DocRef) -> ApiResult<DocSaveOut> {
        let path = self.get(req.doc_id)?.path.clone().ok_or_else(|| {
            ApiError::invalid(
                "the document has never been saved",
                "use doc_save_as with a path",
            )
        })?;
        self.save_to(req.doc_id, path)
    }

    pub fn doc_save_as(&mut self, req: DocSaveAs) -> ApiResult<DocSaveOut> {
        self.save_to(req.doc_id, normalize_path(&req.path))
    }

    pub fn doc_close(&mut self, req: DocClose) -> ApiResult<DocCloseOut> {
        let id = self.resolve(req.doc_id)?;
        let dirty = self.get(Some(id))?.editor.is_dirty();
        if dirty {
            require_confirm(req.confirm, "closing a document with unsaved changes")?;
        }
        self.remove(id);
        Ok(DocCloseOut {
            doc_id: id,
            closed: true,
            discarded_changes: dirty,
        })
    }

    pub fn doc_list(&mut self, _req: DocRef) -> ApiResult<DocListOut> {
        let current = self.current();
        Ok(DocListOut {
            documents: self
                .documents()
                .map(|d| DocEntry {
                    doc_id: d.doc_id,
                    title: d.doc().meta.title.clone(),
                    path: d.path.as_ref().map(|p| p.display().to_string()),
                    revision: d.revision(),
                    dirty: d.editor.is_dirty(),
                    page_count: d.doc().pages.len(),
                    current: Some(d.doc_id) == current,
                })
                .collect(),
        })
    }

    pub fn doc_info(&mut self, req: DocRef) -> ApiResult<DocInfoOut> {
        let open = self.get(req.doc_id)?;
        let doc = open.doc();
        Ok(DocInfoOut {
            doc_id: open.doc_id,
            title: doc.meta.title.clone(),
            author: doc.meta.author.clone(),
            path: open.path.as_ref().map(|p| p.display().to_string()),
            template: doc.meta.template.clone(),
            revision: open.revision(),
            dirty: open.editor.is_dirty(),
            page_size: doc.setup.size,
            margins: doc.setup.margins,
            bleed: doc.setup.bleed,
            grid: doc.grid,
            page_ids: doc.pages.iter().map(|p| p.id).collect(),
            masters: doc
                .masters
                .iter()
                .map(|m| MasterEntry {
                    master_id: m.id,
                    name: m.name.clone(),
                    frame_count: doc.all_frames_of(pubrs_core::Owner::Master(m.id)).len(),
                })
                .collect(),
            frame_count: doc.frames.len(),
            story_count: doc.stories.len(),
            style_count: doc.styles.styles.len(),
            asset_count: doc.assets.len(),
        })
    }

    pub fn doc_set_page_size(&mut self, req: DocSetPageSize) -> ApiResult<Changed> {
        let size = req.size.size()?;
        self.exec(req.doc_id, Command::SetPageSize { size })
    }

    pub fn doc_set_margins(&mut self, req: DocSetMargins) -> ApiResult<Changed> {
        let base = self.get(req.doc_id)?.doc().setup.margins;
        self.exec(
            req.doc_id,
            Command::SetMargins {
                margins: req.margins.apply(base),
            },
        )
    }

    pub fn doc_set_bleed(&mut self, req: DocSetBleed) -> ApiResult<Changed> {
        self.exec(req.doc_id, Command::SetBleed { bleed: req.bleed })
    }

    pub fn doc_set_info(&mut self, req: DocSetInfo) -> ApiResult<Changed> {
        self.exec(
            req.doc_id,
            Command::SetMeta {
                title: req.title,
                author: req.author,
            },
        )
    }

    pub fn doc_set_grid(&mut self, req: DocSetGrid) -> ApiResult<Changed> {
        let mut g = self.get(req.doc_id)?.doc().grid;
        if let Some(v) = req.spacing {
            g.spacing = v;
        }
        if let Some(v) = req.show_grid {
            g.show_grid = v;
        }
        if let Some(v) = req.snap_to_grid {
            g.snap_to_grid = v;
        }
        if let Some(v) = req.snap_to_guides {
            g.snap_to_guides = v;
        }
        if let Some(v) = req.snap_tolerance {
            g.snap_tolerance = v;
        }
        self.exec(req.doc_id, Command::SetGrid { grid: g })
    }
}

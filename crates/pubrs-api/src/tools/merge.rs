//! Mail-merge tools: merge_set_source, merge_fields_list,
//! merge_insert_field, merge_preview, merge_export, merge_clear (ADR 0023).

use std::path::PathBuf;

use pubrs_core::merge::{MergeError, fields_used};
use pubrs_core::{Command, DocumentId, FrameId, MergeKind, MergeSource, StoryId};
use pubrs_io::merge_source::{self, MergeReadError};
use pubrs_render::{FontEmbedding, PdfOptions};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::export::{FontEmbeddingIn, b64, write_file};
use super::require_confirm;
use super::text::TextChangeOut;
use crate::error::{ApiError, ApiResult};
use crate::session::{Changed, Session};

impl From<MergeReadError> for ApiError {
    fn from(e: MergeReadError) -> Self {
        ApiError::new(e.code(), e.to_string(), e.hint())
    }
}

impl From<MergeError> for ApiError {
    fn from(e: MergeError) -> Self {
        ApiError::new("invalid_argument", e.to_string(), e.hint())
    }
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MergeSetSource {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// A .csv or .xlsx file. CSV is UTF-8 (BOM allowed), comma or semicolon
    /// separated, the first row names the fields.
    #[serde(default)]
    pub path: Option<String>,
    /// CSV text instead of a file.
    #[serde(default)]
    pub inline_csv: Option<String>,
    /// The sheet of an .xlsx file (default: the first).
    #[serde(default)]
    pub sheet: Option<String>,
    /// Read the file given earlier again (Refresh data): the preview record
    /// and switch are kept. Needs no path.
    #[serde(default)]
    pub refresh: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MergeSourceOut {
    pub source_name: String,
    /// csv or xlsx.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    /// Field names (the header row).
    pub fields: Vec<String>,
    pub record_count: usize,
    /// Sheet names, for an .xlsx file.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sheets: Vec<String>,
    #[serde(flatten)]
    pub change: Changed,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MergeFieldsList {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MergeFieldInfo {
    pub name: String,
    /// The previewed record's value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// How many times the document uses this field.
    pub uses: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MergeFieldsOut {
    pub has_data: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_name: Option<String>,
    pub fields: Vec<MergeFieldInfo>,
    pub record_count: usize,
    pub preview_index: usize,
    pub preview: bool,
    /// Field names the document uses that the data does not have.
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MergeInsertField {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    #[serde(default)]
    pub frame_id: Option<FrameId>,
    #[serde(default)]
    pub story_id: Option<StoryId>,
    /// Character offset (0 = start). Omit to append at the end.
    #[serde(default)]
    pub offset: Option<usize>,
    /// A field name from merge_fields_list.
    pub field: String,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MergePreview {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// 0-based record to show.
    #[serde(default)]
    pub index: Option<usize>,
    /// Show the record's values (true) or the «Field» names (false).
    #[serde(default)]
    pub on: Option<bool>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MergePreviewOut {
    pub preview: bool,
    pub index: usize,
    pub record_count: usize,
    #[serde(flatten)]
    pub change: Changed,
}

/// What a merge produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MergeFormat {
    /// One PDF with the pages once per record (default).
    #[default]
    Pdf,
    /// A new .pubrs publication with the pages once per record, fields
    /// replaced by text.
    Publication,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MergeExport {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    #[serde(default)]
    pub format: MergeFormat,
    /// Output file path. Without a path the file is returned as base64.
    #[serde(default)]
    pub path: Option<String>,
    /// pdf only: include the document's bleed (default: when it has bleed).
    #[serde(default)]
    pub bleed: Option<bool>,
    /// pdf only: add crop marks (default false).
    #[serde(default)]
    pub crop_marks: bool,
    /// pdf only: subset (default) or outlines.
    #[serde(default)]
    pub font_embedding: FontEmbeddingIn,
    /// pdf only: impose onto the label sheet or booklet (default true). On a
    /// sequence label sheet every record is one label; on a repeat sheet
    /// every record is a sheet.
    #[serde(default)]
    pub impose: Option<bool>,
    /// Also return the bytes as base64 when a path is given.
    #[serde(default)]
    pub return_base64: bool,
    /// publication only: also open the result as a new document.
    #[serde(default)]
    pub open: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MergeExportOut {
    pub doc_id: DocumentId,
    pub format: MergeFormat,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub bytes: u64,
    pub record_count: usize,
    /// Pages in the output (sheets when a PDF is imposed).
    pub page_count: usize,
    /// The new document, when `open` was set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opened_doc_id: Option<DocumentId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base64: Option<String>,
    pub revision: u64,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MergeClear {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Must be true: removes the merge data (fields in the text stay).
    #[serde(default)]
    pub confirm: bool,
}

fn kind_name(k: MergeKind) -> &'static str {
    match k {
        MergeKind::Csv => "csv",
        MergeKind::Xlsx => "xlsx",
    }
}

impl Session {
    fn merge_of(&self, doc_id: Option<DocumentId>) -> ApiResult<Option<MergeSource>> {
        Ok(self.get(doc_id)?.doc().merge.clone())
    }

    pub fn merge_set_source(&mut self, req: MergeSetSource) -> ApiResult<MergeSourceOut> {
        let id = self.resolve(req.doc_id)?;
        let given = [req.path.is_some(), req.inline_csv.is_some(), req.refresh]
            .into_iter()
            .filter(|b| *b)
            .count();
        if given != 1 {
            return Err(ApiError::invalid(
                "give exactly one of path, inline_csv or refresh",
                "path reads a .csv/.xlsx file, inline_csv takes CSV text, refresh reads the last file again",
            ));
        }
        if req.sheet.is_some() && req.inline_csv.is_some() {
            return Err(ApiError::invalid(
                "sheet only applies to an .xlsx file",
                "leave sheet out for CSV",
            ));
        }
        let old = self.merge_of(Some(id))?;
        let (source, remembered, sheets) = if req.refresh {
            let (path, sheet) = self.get(Some(id))?.merge_path.clone().ok_or_else(|| {
                ApiError::invalid(
                    "there is no file to refresh from in this session",
                    "call merge_set_source with a path first",
                )
            })?;
            let src = merge_source::load_file(&path, sheet.as_deref())?;
            (src, Some((path, sheet)), Vec::new())
        } else if let Some(text) = &req.inline_csv {
            (merge_source::from_inline_csv(text)?, None, Vec::new())
        } else {
            let path: PathBuf = self.resolve_path(req.path.as_deref().unwrap_or_default());
            let sheets = merge_source::sheet_names_of(&path)?;
            let src = merge_source::load_file(&path, req.sheet.as_deref())?;
            let sheet = src.sheet.clone();
            (src, Some((path, sheet)), sheets)
        };
        let source = merge_source::keep_preview(source, old.as_ref());
        let out_src = (
            source.source_name.clone(),
            kind_name(source.kind).to_string(),
            source.sheet.clone(),
            source.fields.clone(),
            source.record_count(),
        );
        let change = self.exec(
            Some(id),
            Command::SetMergeSource {
                merge: Some(source),
            },
        )?;
        self.get_mut(Some(id))?.merge_path = remembered;
        Ok(MergeSourceOut {
            source_name: out_src.0,
            kind: out_src.1,
            sheet: out_src.2,
            fields: out_src.3,
            record_count: out_src.4,
            sheets,
            change,
        })
    }

    pub fn merge_fields_list(&mut self, req: MergeFieldsList) -> ApiResult<MergeFieldsOut> {
        let doc = self.get(req.doc_id)?.doc();
        let mut uses: std::collections::BTreeMap<String, usize> = Default::default();
        for s in doc.stories.values() {
            for r in s.paragraphs.iter().flat_map(|p| p.runs.iter()) {
                if let Some(n) = &r.field {
                    *uses.entry(n.clone()).or_default() += 1;
                }
            }
        }
        let Some(m) = &doc.merge else {
            return Ok(MergeFieldsOut {
                has_data: false,
                source_name: None,
                fields: Vec::new(),
                record_count: 0,
                preview_index: 0,
                preview: false,
                missing: fields_used(doc).into_iter().collect(),
            });
        };
        let idx = m.current_index();
        Ok(MergeFieldsOut {
            has_data: true,
            source_name: Some(m.source_name.clone()),
            fields: m
                .fields
                .iter()
                .map(|f| MergeFieldInfo {
                    name: f.clone(),
                    value: m.value_in(idx, f).map(str::to_string),
                    uses: uses.get(f).copied().unwrap_or(0),
                })
                .collect(),
            record_count: m.record_count(),
            preview_index: idx,
            preview: m.preview,
            missing: fields_used(doc)
                .into_iter()
                .filter(|n| m.field_index(n).is_none())
                .collect(),
        })
    }

    pub fn merge_insert_field(&mut self, req: MergeInsertField) -> ApiResult<TextChangeOut> {
        let story = self.resolve_story(req.doc_id, req.frame_id, req.story_id)?;
        let len = self
            .get(req.doc_id)?
            .doc()
            .story(story)
            .map_or(0, |s| s.char_len());
        let change = self.exec(
            req.doc_id,
            Command::InsertMergeField {
                story,
                at: req.offset.unwrap_or(len),
                field: req.field,
            },
        )?;
        self.text_out(story, change)
    }

    pub fn merge_preview(&mut self, req: MergePreview) -> ApiResult<MergePreviewOut> {
        let id = self.resolve(req.doc_id)?;
        let change = self.exec(
            Some(id),
            Command::SetMergePreview {
                index: req.index,
                preview: req.on,
            },
        )?;
        let m = self
            .merge_of(Some(id))?
            .ok_or_else(|| ApiError::internal("merge data vanished"))?;
        Ok(MergePreviewOut {
            preview: m.preview,
            index: m.current_index(),
            record_count: m.record_count(),
            change,
        })
    }

    pub fn merge_clear(&mut self, req: MergeClear) -> ApiResult<Changed> {
        require_confirm(req.confirm, "removing the merge data")?;
        let id = self.resolve(req.doc_id)?;
        let change = self.exec(Some(id), Command::SetMergeSource { merge: None })?;
        self.get_mut(Some(id))?.merge_path = None;
        Ok(change)
    }

    pub fn merge_export(&mut self, req: MergeExport) -> ApiResult<MergeExportOut> {
        let id = self.resolve(req.doc_id)?;
        let ext = match req.format {
            MergeFormat::Pdf => "pdf",
            MergeFormat::Publication => "pubrs",
        };
        let path = match &req.path {
            Some(p) => Some(self.resolve_path(p)),
            None => self.default_path(Some(id), ext).map(|p| {
                let stem = p.file_stem().map_or_else(
                    || "merged".to_string(),
                    |s| format!("{}-merged", s.to_string_lossy()),
                );
                p.with_file_name(format!("{stem}.{ext}"))
            }),
        };
        let doc = self.get(Some(id))?.doc().clone();
        let data = pubrs_core::merge::merge_data(&doc)?.clone();
        let revision = self.get(Some(id))?.revision();
        let (bytes, page_count, opened) = match req.format {
            MergeFormat::Pdf => {
                let opts = PdfOptions {
                    bleed: req.bleed.unwrap_or(doc.setup.bleed > 0.0),
                    crop_marks: req.crop_marks,
                    font_embedding: match req.font_embedding {
                        FontEmbeddingIn::Subset => FontEmbedding::Subset,
                        FontEmbeddingIn::Outlines => FontEmbedding::Outlines,
                    },
                    flat: !req.impose.unwrap_or(true),
                    ..Default::default()
                };
                let n = doc.pages.len() * data.record_count();
                let pages = if opts.flat {
                    n
                } else {
                    pubrs_render::impose::output_pages(&doc.setup, n)
                };
                let parts = self.doc_with_caches(id)?;
                let pdf = parts.ctx.export_merged_pdf(&doc, &opts)?;
                (pdf, pages, None)
            }
            MergeFormat::Publication => {
                let merged = pubrs_core::merge::merge_to_publication(&doc)?;
                let pages = merged.pages.len();
                let bytes = pubrs_io::write_bytes(&merged, None)?;
                let opened = req.open.then(|| self.insert(merged, None));
                (bytes, pages, opened)
            }
        };
        if let Some(p) = &path {
            write_file(&p.display().to_string(), &bytes)?;
        }
        Ok(MergeExportOut {
            doc_id: id,
            format: req.format,
            path: path.as_ref().map(|p| p.display().to_string()),
            bytes: bytes.len() as u64,
            record_count: data.record_count(),
            page_count,
            opened_doc_id: opened,
            base64: (path.is_none() || req.return_base64).then(|| b64(&bytes)),
            revision,
        })
    }
}

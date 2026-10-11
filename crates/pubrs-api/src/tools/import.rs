//! Import tools: doc_import (Microsoft Publisher `.pub` files).

use pubrs_core::{DocumentId, PageId};
use pubrs_io::pubfile::ImportReport;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, ApiResult};
use crate::session::Session;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocImport {
    /// Path of a Microsoft Publisher (.pub) file.
    pub path: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DocImportOut {
    pub doc_id: DocumentId,
    /// The file name without its extension (the document has no path of its
    /// own until it is saved with doc_save_as).
    pub title: String,
    pub page_ids: Vec<PageId>,
    pub revision: u64,
    /// What was imported and what was dropped or substituted, by kind.
    pub report: ImportReport,
    /// One line describing the import.
    pub summary: String,
}

impl Session {
    pub fn doc_import(&mut self, req: DocImport) -> ApiResult<DocImportOut> {
        let path = self.resolve_path(&req.path);
        let len = std::fs::metadata(&path)
            .map_err(pubrs_io::IoError::from)?
            .len();
        let max = pubrs_io::pubfile::container::MAX_INPUT;
        if len > max as u64 {
            return Err(ApiError::new(
                "invalid_file",
                format!("the file is {len} bytes; the importer reads at most {max} bytes"),
                "the file is too large to be a Publisher document this importer can read",
            ));
        }
        let bytes = std::fs::read(&path).map_err(pubrs_io::IoError::from)?;
        let (mut doc, mut report) = pubrs_io::pubfile::import(&bytes).map_err(|e| match e {
            pubrs_io::IoError::Io(_) => ApiError::from(e),
            e => ApiError::new(
                "invalid_file",
                e.to_string(),
                "the file is not a Microsoft Publisher (.pub) document this importer can read",
            ),
        })?;
        pubrs_io::pubfile::note_missing_fonts(&mut report, &mut self.ctx.layouter.fonts);
        let title = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        doc.meta.title.clone_from(&title);
        let page_ids = doc.pages.iter().map(|p| p.id).collect();
        let doc_id = self.insert(doc, None);
        let summary = report.summary();
        Ok(DocImportOut {
            doc_id,
            title,
            page_ids,
            revision: self.get(Some(doc_id))?.revision(),
            report,
            summary,
        })
    }
}

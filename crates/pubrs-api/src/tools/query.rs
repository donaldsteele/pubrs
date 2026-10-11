//! Query tools: doc_to_json, validate, fonts_list.

use pubrs_core::validate::{
    ValidateOptions, Warning, WarningCode, validate_document, validate_page,
};
use pubrs_core::{Document, DocumentId, Owner, PageId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::doc::DocRef;
use crate::error::ApiResult;
use crate::session::Session;

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DocToJsonOut {
    pub doc_id: DocumentId,
    pub revision: u64,
    /// The complete document (the `document.json` of the saved file).
    pub document: Document,
}

/// Arguments of `validate`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ValidateIn {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Also run the checks for a commercial CMYK print job: RGB colours are
    /// flagged (rgb_in_cmyk_job) and the desktop-printer margin check is
    /// skipped. Default false.
    #[serde(default)]
    pub commercial: bool,
}

impl From<DocRef> for ValidateIn {
    fn from(r: DocRef) -> Self {
        Self {
            doc_id: r.doc_id,
            commercial: false,
        }
    }
}

/// Warnings split the way the Design Checker caches them.
#[derive(Debug, Clone)]
pub struct ValidateParts {
    pub doc_id: DocumentId,
    pub revision: u64,
    /// Frame-level warnings, per requested page, in page order.
    pub pages: Vec<(PageId, Vec<Warning>)>,
    /// Story-level warnings (hyphenation, overflow, fonts, glyphs) and
    /// undecodable images, for the whole document.
    pub document: Vec<Warning>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ValidateOut {
    pub doc_id: DocumentId,
    /// True if there are no warnings.
    pub ok: bool,
    pub warnings: Vec<Warning>,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FontEntry {
    pub family: String,
    /// bundled (always available, embedded in exports), system or added.
    pub source: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FontsListOut {
    pub families: Vec<FontEntry>,
    /// Family used when a requested font is missing.
    pub fallback: String,
}

impl Session {
    pub fn doc_to_json(&mut self, req: DocRef) -> ApiResult<DocToJsonOut> {
        let open = self.get(req.doc_id)?;
        Ok(DocToJsonOut {
            doc_id: open.doc_id,
            revision: open.revision(),
            document: open.doc().clone(),
        })
    }

    /// Model warnings plus overflow, missing fonts/glyphs and undecodable images.
    pub fn validate(&mut self, req: impl Into<ValidateIn>) -> ApiResult<ValidateOut> {
        let req = req.into();
        let parts = self.validate_parts(req.doc_id, req.commercial, None)?;
        let warnings: Vec<Warning> = parts
            .pages
            .into_iter()
            .flat_map(|(_, w)| w)
            .chain(parts.document)
            .collect();
        Ok(ValidateOut {
            doc_id: parts.doc_id,
            ok: warnings.is_empty(),
            warnings,
            revision: parts.revision,
        })
    }

    /// [`Self::validate`] in the pieces the Design Checker caches: the
    /// frame-level warnings of the pages asked for (all pages for `None`),
    /// and the story-level warnings of the whole document, which come from
    /// the cached layout.
    pub fn validate_parts(
        &mut self,
        doc_id: Option<DocumentId>,
        commercial: bool,
        pages: Option<&[PageId]>,
    ) -> ApiResult<ValidateParts> {
        let layout = self.layout(doc_id)?;
        let id = self.resolve(doc_id)?;
        let doc = self.get(Some(id))?.doc().clone();
        self.ctx.images.sync(&doc);
        let opts = ValidateOptions { commercial };
        let page_ids: Vec<PageId> = match pages {
            Some(want) => doc
                .pages
                .iter()
                .map(|pg| pg.id)
                .filter(|id| want.contains(id))
                .collect(),
            None => doc.pages.iter().map(|p| p.id).collect(),
        };
        let page_warnings: Vec<(PageId, Vec<Warning>)> = page_ids
            .into_iter()
            .map(|p| (p, validate_page(&doc, p, &opts)))
            .collect();
        let mut warnings = validate_document(&doc, &opts);
        let page_of = |f| match doc.owner_of(f) {
            Some(Owner::Page(p)) => Some(p),
            _ => None,
        };
        for (sid, sl) in &layout.stories {
            let Some(story) = doc.story(*sid) else {
                continue;
            };
            if let Some(o) = sl.overflow {
                let last = story.frames.last().copied();
                let in_cell = doc.table_of_story(*sid).is_some();
                warnings.push(Warning {
                    code: WarningCode::Overflow,
                    message: if in_cell {
                        format!(
                            "{} characters do not fit in the table cell",
                            o.hidden_chars
                        )
                    } else {
                        format!(
                            "{} characters do not fit in the text frame chain",
                            o.hidden_chars
                        )
                    },
                    hint: if in_cell {
                        "enlarge the table frame, shorten the cell text or reduce its font size".into()
                    } else {
                        "enlarge the frame, reduce the text or font size, or link another frame with text_link_frames".into()
                    },
                    page: last.and_then(page_of),
                    frame: last,
                    frames: chain(story),
                });
            }
            for family in &sl.missing_fonts {
                warnings.push(Warning {
                    code: WarningCode::MissingFont,
                    message: format!("font {family:?} is not installed; a fallback font is used"),
                    hint: "install the font or choose one from fonts_list".into(),
                    page: story.frames.first().copied().and_then(page_of),
                    frame: story.frames.first().copied(),
                    frames: chain(story),
                });
            }
            if sl.missing_glyphs > 0 {
                warnings.push(Warning {
                    code: WarningCode::MissingGlyphs,
                    message: missing_glyphs_message(sl.missing_glyphs, &sl.missing_chars),
                    hint: "choose a font that covers these characters (fonts_list)".into(),
                    page: story.frames.first().copied().and_then(page_of),
                    frame: story.frames.first().copied(),
                    frames: chain(story),
                });
            }
        }
        for (asset, reason) in self.ctx.images.failures() {
            if let Some(f) = doc
                .frames
                .values()
                .find(|f| f.as_image().is_some_and(|i| i.asset == Some(*asset)))
            {
                warnings.push(Warning {
                    code: WarningCode::EmptyImageFrame,
                    message: format!("image cannot be decoded: {reason}"),
                    hint: "replace it with image_replace".into(),
                    page: page_of(f.id),
                    frame: Some(f.id),
                    frames: Vec::new(),
                });
            }
        }
        let open = self.get(Some(id))?;
        Ok(ValidateParts {
            doc_id: id,
            revision: open.revision(),
            pages: page_warnings,
            document: warnings,
        })
    }

    pub fn fonts_list(&mut self, _req: DocRef) -> ApiResult<FontsListOut> {
        let families = self
            .ctx
            .layouter
            .fonts
            .families()
            .into_iter()
            .map(|f| FontEntry {
                family: f.name,
                source: match f.source {
                    pubrs_layout::FontSource::Bundled => "bundled",
                    pubrs_layout::FontSource::System => "system",
                    pubrs_layout::FontSource::Added => "added",
                }
                .into(),
            })
            .collect();
        Ok(FontsListOut {
            families,
            fallback: pubrs_layout::FALLBACK_SANS.into(),
        })
    }
}

/// The frames of a linked chain, empty for a single frame.
fn chain(story: &pubrs_core::Story) -> Vec<pubrs_core::FrameId> {
    if story.frames.len() > 1 {
        story.frames.clone()
    } else {
        Vec::new()
    }
}

/// How many characters were named in a missing-glyph warning at most.
const MISSING_NAMES: usize = 20;

/// "no available font covers: ✓ (U+2713), …" for the characters drawn as
/// missing-glyph boxes (at most [`MISSING_NAMES`] named).
fn missing_glyphs_message(count: usize, chars: &std::collections::BTreeSet<char>) -> String {
    let mut names: Vec<String> = chars
        .iter()
        .take(MISSING_NAMES)
        .map(|c| format!("{c} (U+{:04X})", u32::from(*c)))
        .collect();
    if chars.len() > MISSING_NAMES {
        names.push(format!("and {} more", chars.len() - MISSING_NAMES));
    }
    format!(
        "{count} character{} drawn as missing-glyph boxes; no available font covers: {}",
        if count == 1 { "" } else { "s" },
        names.join(", ")
    )
}

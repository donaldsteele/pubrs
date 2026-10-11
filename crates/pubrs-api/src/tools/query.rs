//! Query tools: doc_to_json, validate, fonts_list.

use pubrs_core::validate::{Warning, WarningCode, validate};
use pubrs_core::{Document, DocumentId, Owner};
use schemars::JsonSchema;
use serde::Serialize;

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
    pub fn validate(&mut self, req: DocRef) -> ApiResult<ValidateOut> {
        let layout = self.layout(req.doc_id)?;
        let id = self.resolve(req.doc_id)?;
        let doc = self.get(Some(id))?.doc().clone();
        self.ctx.images.sync(&doc);
        let mut warnings = validate(&doc);
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
                warnings.push(Warning {
                    code: WarningCode::Overflow,
                    message: format!(
                        "{} characters do not fit in the text frame chain",
                        o.hidden_chars
                    ),
                    hint: "enlarge the frame, reduce the text or font size, or link another frame with text_link_frames".into(),
                    page: last.and_then(page_of),
                    frame: last,
                });
            }
            for family in &sl.missing_fonts {
                warnings.push(Warning {
                    code: WarningCode::MissingFont,
                    message: format!("font {family:?} is not installed; a fallback font is used"),
                    hint: "install the font or choose one from fonts_list".into(),
                    page: story.frames.first().copied().and_then(page_of),
                    frame: story.frames.first().copied(),
                });
            }
            if sl.missing_glyphs > 0 {
                warnings.push(Warning {
                    code: WarningCode::MissingGlyphs,
                    message: missing_glyphs_message(sl.missing_glyphs, &sl.missing_chars),
                    hint: "choose a font that covers these characters (fonts_list)".into(),
                    page: story.frames.first().copied().and_then(page_of),
                    frame: story.frames.first().copied(),
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
                });
            }
        }
        let open = self.get(Some(id))?;
        Ok(ValidateOut {
            doc_id: id,
            ok: warnings.is_empty(),
            warnings,
            revision: open.revision(),
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

//! Template tools: template_list, template_apply, template_save.

use std::collections::BTreeMap;

use pubrs_core::{DocumentId, FrameId, Owner, PageId, PlaceholderKind, StoryId};
use pubrs_io::templates::{PlaceholderInfo, TemplateInfo, TemplateOptions, info, instantiate};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, ApiResult};
use crate::session::Session;

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateList {}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TemplateListOut {
    pub templates: Vec<TemplateInfo>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateApply {
    /// Template name from template_list: "flyer", "trifold_brochure",
    /// "business_card", "newsletter", "greeting_card", "postcard",
    /// "certificate", "menu", "letterhead", "envelope", "calendar",
    /// "address_labels" or "business_cards" (or a saved user template).
    pub name: String,
    /// Title for the new document.
    #[serde(default)]
    pub title: Option<String>,
    /// Calendar year, 1600 to 9999 (calendar template only; default 2027).
    #[serde(default)]
    pub year: Option<i32>,
}

/// A placeholder in the new document.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PlaceholderOut {
    /// Semantic name, e.g. "headline" or "cover_image".
    pub name: String,
    pub kind: PlaceholderKind,
    pub frame_id: FrameId,
    /// Text placeholders: the story to fill with text_set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub story_id: Option<StoryId>,
    pub page_id: PageId,
    /// What belongs here (sample text for text placeholders).
    pub hint: String,
    /// Frame size in points.
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TemplateApplyOut {
    /// The new document (now current).
    pub doc_id: DocumentId,
    pub page_ids: Vec<PageId>,
    /// Fill text placeholders with text_set and image placeholders with image_place.
    pub placeholders: Vec<PlaceholderOut>,
    /// Other named frames (e.g. "inside_body_continued" to link overflowing text into).
    pub named_frames: BTreeMap<String, FrameId>,
    pub revision: u64,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateSave {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Template name: letters, digits, "_" and "-".
    pub name: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TemplateSaveOut {
    pub name: String,
    pub path: String,
    pub placeholders: Vec<PlaceholderInfo>,
}

impl Session {
    pub fn template_list(&mut self, _req: TemplateList) -> ApiResult<TemplateListOut> {
        Ok(TemplateListOut {
            templates: self.templates.list(),
        })
    }

    pub fn template_apply(&mut self, req: TemplateApply) -> ApiResult<TemplateApplyOut> {
        if let Some(y) = req.year {
            if req.name != "calendar" {
                return Err(ApiError::invalid(
                    "year applies to the calendar template only",
                    "omit year, or apply the calendar template",
                ));
            }
            if !pubrs_io::templates::CALENDAR_YEARS.contains(&y) {
                return Err(ApiError::invalid(
                    format!("year {y} is outside 1600..=9999"),
                    "use a year from 1600 to 9999",
                ));
            }
        }
        let opts = TemplateOptions { year: req.year };
        let template = self.templates.load_with(&req.name, &opts).map_err(|e| {
            ApiError::not_found(
                "template",
                &req.name,
                &format!("{e}; list templates with template_list"),
            )
        })?;
        let mut doc = instantiate(&template, &req.name, &mut *self.ids);
        if let Some(t) = req.title {
            doc.meta.title = t;
        }
        let doc_id = self.insert(doc, None);
        let open = self.get(Some(doc_id))?;
        let doc = open.doc();
        let mut placeholders = Vec::new();
        let mut named_frames = BTreeMap::new();
        for page in doc.pages.iter() {
            for id in doc.all_frames_of(Owner::Page(page.id)) {
                let Some(f) = doc.frame(id) else { continue };
                match &f.placeholder {
                    Some(p) => placeholders.push(PlaceholderOut {
                        name: p.name.clone(),
                        kind: p.kind,
                        frame_id: f.id,
                        story_id: f.as_text().map(|t| t.story),
                        page_id: page.id,
                        hint: p.hint.clone(),
                        width: f.rect.width,
                        height: f.rect.height,
                    }),
                    None => {
                        if let Some(n) = &f.name {
                            named_frames.insert(n.clone(), f.id);
                        }
                    }
                }
            }
        }
        Ok(TemplateApplyOut {
            doc_id,
            page_ids: doc.pages.iter().map(|p| p.id).collect(),
            placeholders,
            named_frames,
            revision: open.revision(),
        })
    }

    pub fn template_save(&mut self, req: TemplateSave) -> ApiResult<TemplateSaveOut> {
        let doc = self.get(req.doc_id)?.doc().clone();
        let path = self.templates.save(&req.name, &doc).map_err(|e| {
            ApiError::invalid(
                e.to_string(),
                "use letters, digits, _ and -; the server needs a user template folder",
            )
        })?;
        Ok(TemplateSaveOut {
            placeholders: info(&doc, &req.name, false).placeholders,
            name: req.name,
            path: path.display().to_string(),
        })
    }
}

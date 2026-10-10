//! Style tools: style_list, style_create, style_update, style_delete.

use pubrs_core::{CharProps, Command, DocumentId, ParaProps, Style, StyleId, StyleKind};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::require_confirm;
use crate::error::{ApiError, ApiResult};
use crate::session::{Changed, Session};

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StyleList {
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Only paragraph or character styles.
    #[serde(default)]
    pub kind: Option<StyleKind>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct StyleEntry {
    pub style_id: StyleId,
    pub name: String,
    pub kind: StyleKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub based_on: Option<String>,
    pub paragraph: ParaProps,
    pub character: CharProps,
    /// True for the style used by paragraphs without a style.
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct StyleListOut {
    pub styles: Vec<StyleEntry>,
    pub revision: u64,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StyleCreate {
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Unique name.
    pub name: String,
    /// paragraph (default) or character.
    #[serde(default)]
    pub kind: Option<StyleKind>,
    /// Parent style name or id of the same kind; unset properties inherit from it.
    #[serde(default)]
    pub based_on: Option<String>,
    #[serde(default)]
    pub paragraph: ParaProps,
    #[serde(default)]
    pub character: CharProps,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct StyleCreateOut {
    pub style_id: StyleId,
    #[serde(flatten)]
    pub change: Changed,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StyleUpdate {
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Style name or id.
    pub style: String,
    /// Rename.
    #[serde(default)]
    pub name: Option<String>,
    /// New parent name or id ("" removes the parent).
    #[serde(default)]
    pub based_on: Option<String>,
    /// Paragraph properties to merge in (set fields win).
    #[serde(default)]
    pub paragraph: Option<ParaProps>,
    /// Character properties to merge in (set fields win).
    #[serde(default)]
    pub character: Option<CharProps>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StyleDelete {
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Style name or id. Text using it falls back to its parent / default.
    pub style: String,
    /// Must be true.
    #[serde(default)]
    pub confirm: bool,
}

impl Session {
    pub fn style_list(&mut self, req: StyleList) -> ApiResult<StyleListOut> {
        let open = self.get(req.doc_id)?;
        let doc = open.doc();
        Ok(StyleListOut {
            styles: doc
                .styles
                .styles
                .iter()
                .filter(|s| req.kind.is_none_or(|k| k == s.kind))
                .map(|s| StyleEntry {
                    style_id: s.id,
                    name: s.name.clone(),
                    kind: s.kind,
                    based_on: s
                        .based_on
                        .and_then(|b| doc.styles.get(b))
                        .map(|b| b.name.clone()),
                    paragraph: s.paragraph.clone(),
                    character: s.character.clone(),
                    is_default: s.id == doc.styles.default_paragraph,
                })
                .collect(),
            revision: open.revision(),
        })
    }

    pub fn style_create(&mut self, req: StyleCreate) -> ApiResult<StyleCreateOut> {
        let kind = req.kind.unwrap_or(StyleKind::Paragraph);
        let based_on = match &req.based_on {
            Some(b) => Some(self.resolve_style(req.doc_id, b, Some(kind))?),
            None => None,
        };
        let id = StyleId::new(&mut *self.ids);
        let mut change = self.exec(
            req.doc_id,
            Command::AddStyle {
                style: Style {
                    id,
                    name: req.name,
                    kind,
                    based_on,
                    paragraph: req.paragraph,
                    character: req.character,
                },
            },
        )?;
        change.affected_ids.push(id.to_string());
        Ok(StyleCreateOut {
            style_id: id,
            change,
        })
    }

    pub fn style_update(&mut self, req: StyleUpdate) -> ApiResult<Changed> {
        let id = self.resolve_style(req.doc_id, &req.style, None)?;
        let mut style = self
            .get(req.doc_id)?
            .doc()
            .styles
            .get(id)
            .cloned()
            .ok_or_else(|| ApiError::internal("style vanished"))?;
        if let Some(n) = req.name {
            style.name = n;
        }
        if let Some(b) = req.based_on {
            style.based_on = if b.is_empty() {
                None
            } else {
                Some(self.resolve_style(req.doc_id, &b, Some(style.kind))?)
            };
        }
        if let Some(p) = req.paragraph {
            style.paragraph = style.paragraph.merged(&p);
        }
        if let Some(c) = req.character {
            style.character = style.character.merged(&c);
        }
        let mut change = self.exec(req.doc_id, Command::UpdateStyle { style })?;
        change.affected_ids.push(id.to_string());
        Ok(change)
    }

    pub fn style_delete(&mut self, req: StyleDelete) -> ApiResult<Changed> {
        require_confirm(req.confirm, "deleting a style")?;
        let id = self.resolve_style(req.doc_id, &req.style, None)?;
        let mut change = self.exec(req.doc_id, Command::DeleteStyle { style: id })?;
        change.affected_ids.push(id.to_string());
        Ok(change)
    }
}

//! Colour tools: swatch_list, swatch_create, swatch_update, swatch_delete,
//! scheme_list, scheme_apply.

use pubrs_core::swatch::{SCHEMES, scheme as builtin_scheme, slot_of};
use pubrs_core::{
    Cmyk, Color, Command, DocumentId, Paint, SchemeId, SchemeSlot, Swatch, SwatchId, SwatchValue,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::require_confirm;
use crate::error::{ApiError, ApiResult};
use crate::session::{Changed, Session};

/// A swatch value as tools take it.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SwatchValueIn {
    /// An sRGB colour, `"#RRGGBB"`.
    Rgb(Color),
    /// Process colour, each channel 0 to 100.
    Cmyk(Cmyk),
    /// A named spot ink with its process (CMYK) alternative, which is
    /// required: it is what the screen and non-spot printers use.
    Spot {
        /// Ink name, e.g. "PANTONE 186 C".
        name: String,
        cmyk: Option<Cmyk>,
    },
}

impl SwatchValueIn {
    fn into_value(self) -> ApiResult<SwatchValue> {
        Ok(match self {
            SwatchValueIn::Rgb(c) => SwatchValue::Rgb(c),
            SwatchValueIn::Cmyk(c) => SwatchValue::Cmyk(c),
            SwatchValueIn::Spot { name, cmyk } => SwatchValue::Spot {
                name,
                cmyk: cmyk.ok_or_else(|| {
                    ApiError::invalid(
                        "a spot colour needs a CMYK alternative",
                        "add cmyk: {c, m, y, k} (0-100) to the spot value; it is what the screen and non-spot printers use",
                    )
                })?,
            },
        })
    }
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SwatchList {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SwatchEntry {
    pub swatch_id: SwatchId,
    pub name: String,
    pub value: SwatchValue,
    /// Screen colour as hex (CMYK by formula; spot shows its CMYK alternative).
    pub screen: Color,
    /// Set for the seven colour-scheme slot swatches, e.g. "main".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheme_slot: Option<String>,
    /// Where it is used (empty = unused, safe to delete).
    pub used_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SwatchListOut {
    pub swatches: Vec<SwatchEntry>,
    /// Id of the last colour scheme applied, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheme: Option<SchemeId>,
    pub revision: u64,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SwatchCreate {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Unique name.
    pub name: String,
    /// `{"rgb": "#1E3A8A"}`, `{"cmyk": {"c":100,"m":60,"y":0,"k":20}}` or
    /// `{"spot": {"name": "PANTONE 186 C", "cmyk": {...}}}`.
    pub value: SwatchValueIn,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SwatchCreateOut {
    pub swatch_id: SwatchId,
    #[serde(flatten)]
    pub change: Changed,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SwatchUpdate {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Swatch name or id.
    pub swatch: String,
    /// Rename.
    #[serde(default)]
    pub name: Option<String>,
    /// New value. Everything using the swatch recolours.
    #[serde(default)]
    pub value: Option<SwatchValueIn>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SwatchDelete {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Swatch name or id.
    pub swatch: String,
    /// What to use where the swatch is used (hex or `{"swatch": id, "tint"}`;
    /// a gradient only if the swatch is used by fills alone). Required when
    /// the swatch is in use.
    #[serde(default)]
    pub replace_with: Option<Paint>,
    /// Must be true.
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SchemeList {
    /// Document id; marks which scheme is applied (default: the current document, if any).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SchemeEntry {
    pub scheme_id: String,
    pub name: String,
    /// Slot colours: main, accent_1 to accent_4, hyperlink, followed_hyperlink.
    pub colors: SchemeColors,
    pub applied: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SchemeColors {
    pub main: Color,
    pub accent_1: Color,
    pub accent_2: Color,
    pub accent_3: Color,
    pub accent_4: Color,
    pub hyperlink: Color,
    pub followed_hyperlink: Color,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SchemeListOut {
    pub schemes: Vec<SchemeEntry>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SchemeApply {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Scheme id or name (see scheme_list).
    pub scheme: String,
}

fn slot_name(s: SchemeSlot) -> &'static str {
    match s {
        SchemeSlot::Main => "main",
        SchemeSlot::Accent1 => "accent_1",
        SchemeSlot::Accent2 => "accent_2",
        SchemeSlot::Accent3 => "accent_3",
        SchemeSlot::Accent4 => "accent_4",
        SchemeSlot::Hyperlink => "hyperlink",
        SchemeSlot::FollowedHyperlink => "followed_hyperlink",
    }
}

impl Session {
    /// Find a swatch by id or (case-insensitive) name.
    pub fn resolve_swatch(
        &self,
        doc_id: Option<DocumentId>,
        name_or_id: &str,
    ) -> ApiResult<SwatchId> {
        let doc = self.get(doc_id)?.doc();
        let by_id = name_or_id
            .parse::<SwatchId>()
            .ok()
            .and_then(|id| doc.swatch(id));
        let found = by_id.or_else(|| {
            doc.swatches
                .iter()
                .find(|s| s.name.eq_ignore_ascii_case(name_or_id.trim()))
        });
        found.map(|s| s.id).ok_or_else(|| {
            ApiError::not_found(
                "swatch",
                name_or_id,
                "list swatches with swatch_list or create one with swatch_create",
            )
        })
    }

    pub fn swatch_list(&mut self, req: SwatchList) -> ApiResult<SwatchListOut> {
        let open = self.get(req.doc_id)?;
        let doc = open.doc();
        Ok(SwatchListOut {
            swatches: doc
                .swatches
                .iter()
                .map(|s| SwatchEntry {
                    swatch_id: s.id,
                    name: s.name.clone(),
                    value: s.value.clone(),
                    screen: doc.resolve_swatch(s.id, 100.0).rgba,
                    scheme_slot: slot_of(s.id).map(|x| slot_name(x).to_string()),
                    used_by: doc.swatch_uses(s.id),
                })
                .collect(),
            scheme: doc.scheme.clone(),
            revision: open.revision(),
        })
    }

    pub fn swatch_create(&mut self, req: SwatchCreate) -> ApiResult<SwatchCreateOut> {
        let value = req.value.into_value()?;
        let id = SwatchId::new(&mut *self.ids);
        let mut change = self.exec(
            req.doc_id,
            Command::AddSwatch {
                swatch: Swatch {
                    id,
                    name: req.name,
                    value,
                },
            },
        )?;
        change.affected_ids.push(id.to_string());
        Ok(SwatchCreateOut {
            swatch_id: id,
            change,
        })
    }

    pub fn swatch_update(&mut self, req: SwatchUpdate) -> ApiResult<Changed> {
        let id = self.resolve_swatch(req.doc_id, &req.swatch)?;
        let mut swatch = self
            .get(req.doc_id)?
            .doc()
            .swatch(id)
            .cloned()
            .ok_or_else(|| ApiError::internal("swatch vanished"))?;
        if let Some(n) = req.name {
            swatch.name = n;
        }
        if let Some(v) = req.value {
            swatch.value = v.into_value()?;
        }
        let mut change = self.exec(req.doc_id, Command::UpdateSwatch { swatch })?;
        change.affected_ids.push(id.to_string());
        Ok(change)
    }

    pub fn swatch_delete(&mut self, req: SwatchDelete) -> ApiResult<Changed> {
        require_confirm(req.confirm, "deleting a swatch")?;
        let id = self.resolve_swatch(req.doc_id, &req.swatch)?;
        let mut change = self.exec(
            req.doc_id,
            Command::DeleteSwatch {
                swatch: id,
                replace_with: req.replace_with,
            },
        )?;
        change.affected_ids.push(id.to_string());
        Ok(change)
    }

    pub fn scheme_list(&mut self, req: SchemeList) -> ApiResult<SchemeListOut> {
        let current = self
            .get(req.doc_id)
            .ok()
            .and_then(|o| o.doc().scheme.clone());
        Ok(SchemeListOut {
            schemes: SCHEMES
                .iter()
                .map(|s| {
                    let c = |slot| s.color(slot);
                    SchemeEntry {
                        scheme_id: s.id.to_string(),
                        name: s.name.to_string(),
                        colors: SchemeColors {
                            main: c(SchemeSlot::Main),
                            accent_1: c(SchemeSlot::Accent1),
                            accent_2: c(SchemeSlot::Accent2),
                            accent_3: c(SchemeSlot::Accent3),
                            accent_4: c(SchemeSlot::Accent4),
                            hyperlink: c(SchemeSlot::Hyperlink),
                            followed_hyperlink: c(SchemeSlot::FollowedHyperlink),
                        },
                        applied: current.as_ref().is_some_and(|x| x.0 == s.id),
                    }
                })
                .collect(),
        })
    }

    pub fn scheme_apply(&mut self, req: SchemeApply) -> ApiResult<Changed> {
        let q = req.scheme.trim();
        let found = builtin_scheme(q)
            .or_else(|| SCHEMES.iter().find(|s| s.name.eq_ignore_ascii_case(q)))
            .ok_or_else(|| ApiError::not_found("scheme", q, "list schemes with scheme_list"))?;
        let mut change = self.exec(
            req.doc_id,
            Command::ApplyScheme {
                scheme: SchemeId(found.id.to_string()),
            },
        )?;
        change
            .affected_ids
            .extend(SchemeSlot::ALL.iter().map(|s| s.swatch_id().to_string()));
        Ok(change)
    }
}

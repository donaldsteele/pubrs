//! Tool implementations: typed request/response pairs on [`crate::Session`].
//!
//! Conventions for every tool:
//! - Lengths are points (1/72 inch). Coordinates have their origin at the
//!   top-left corner of the page (trim box), x to the right, y down.
//! - Colours are `#RRGGBB` or `#RRGGBBAA` strings, or `{"swatch": id, "tint": 0-100}`
//!   (and, for frame fills only, a gradient).
//! - `doc_id` is optional and defaults to the most recently used document.
//! - Mutating tools return `doc_id`, `revision` and `affected_ids`.

pub mod clip;
pub mod color;
pub mod doc;
pub mod export;
pub mod find;
pub mod frames;
pub mod history;
pub mod images;
pub mod pages;
pub mod query;
pub mod sheets;
pub mod spell;
pub mod styles;
pub mod tables;
pub mod templates;
pub mod text;
pub mod wordart;

use std::borrow::Cow;

use pubrs_core::{Document, Frame, FrameContent, FrameId, FrameKind, Owner, Paint, Rect};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize};

/// A paint or `"none"`: a hex colour, `{"swatch": id, "tint": 0-100}`, or
/// (frame fills only) a gradient `{"gradient": "linear"|"radial", "stops":
/// [{"offset", "color"}], "angle"}`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct OptColor(pub Option<Paint>);

impl<'de> Deserialize<'de> for OptColor {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        if let Some(s) = v.as_str()
            && (s.trim().eq_ignore_ascii_case("none") || s.trim().is_empty())
        {
            return Ok(OptColor(None));
        }
        Paint::deserialize(v)
            .map(|p| OptColor(Some(p)))
            .map_err(|e| {
                serde::de::Error::custom(format!(
                    "expected \"none\", a hex colour like \"#1E3A8A\", {{\"swatch\": id, \"tint\": 0-100}} or a gradient ({e})"
                ))
            })
    }
}

impl JsonSchema for OptColor {
    fn schema_name() -> Cow<'static, str> {
        "PaintOrNone".into()
    }
    fn json_schema(g: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "anyOf": [
                {"type": "string", "pattern": "^(none|#[0-9A-Fa-f]{6}([0-9A-Fa-f]{2})?)$"},
                g.subschema_for::<Paint>()
            ],
            "description": "\"none\", \"#RRGGBB\" / \"#RRGGBBAA\", {\"swatch\": id, \"tint\": 0-100}, or (fills only) a gradient."
        })
    }
}

/// Axis-aligned rectangle input in points (origin top-left of the page, y down).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize, JsonSchema)]
pub struct RectIn {
    /// Left edge in points from the page's left edge.
    pub x: f64,
    /// Top edge in points from the page's top edge.
    pub y: f64,
    /// Width in points.
    pub width: f64,
    /// Height in points.
    pub height: f64,
}

impl From<RectIn> for Rect {
    fn from(r: RectIn) -> Rect {
        Rect::new(r.x, r.y, r.width, r.height)
    }
}

impl From<Rect> for RectIn {
    fn from(r: Rect) -> RectIn {
        RectIn {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
        }
    }
}

/// Short description of a frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FrameSummary {
    pub frame_id: FrameId,
    pub kind: FrameKind,
    /// Page or master the frame is on.
    pub owner: Owner,
    /// Unrotated bounds (points).
    pub rect: RectIn,
    /// Clockwise rotation in degrees.
    pub rotation: f64,
    pub locked: bool,
    /// Index in its page's (or group's) z-order, 0 = backmost.
    pub z_index: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Template placeholder name, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    /// Text frames: the story id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub story_id: Option<pubrs_core::StoryId>,
    /// Text frames: first characters of the text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_preview: Option<String>,
    /// Image frames: the placed asset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<pubrs_core::AssetId>,
    /// Groups: member frame ids.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<FrameId>,
}

pub(crate) fn summarize(doc: &Document, f: &Frame) -> FrameSummary {
    let owner = doc.owner_of(f.id).unwrap_or(Owner::Page(doc.pages[0].id));
    let z_index = doc
        .parent_of(f.id)
        .and_then(|p| {
            doc.children(p)
                .and_then(|l| l.iter().position(|x| *x == f.id))
        })
        .unwrap_or(0);
    let (story_id, text_preview) = match &f.content {
        FrameContent::Text(t) => {
            let preview = doc.story(t.story).map(|s| {
                let text = s.plain_text();
                let mut p: String = text.chars().take(60).collect();
                if text.chars().count() > 60 {
                    p.push('…');
                }
                p
            });
            (Some(t.story), preview)
        }
        _ => (None, None),
    };
    FrameSummary {
        frame_id: f.id,
        kind: f.kind(),
        owner,
        rect: f.rect.into(),
        rotation: f.rotation,
        locked: f.locked,
        z_index,
        name: f.name.clone(),
        placeholder: f.placeholder.as_ref().map(|p| p.name.clone()),
        story_id,
        text_preview,
        asset_id: f.as_image().and_then(|i| i.asset),
        children: f.children().to_vec(),
    }
}

/// `true` must be passed to confirm destructive operations.
pub(crate) fn require_confirm(confirm: bool, action: &str) -> crate::ApiResult<()> {
    if confirm {
        Ok(())
    } else {
        Err(crate::ApiError::confirm(action))
    }
}

/// Default for optional boolean fields that default to true.
pub(crate) fn yes() -> bool {
    true
}

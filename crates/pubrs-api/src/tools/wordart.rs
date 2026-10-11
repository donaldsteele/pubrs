//! WordArt, drop shadows and BorderArt: frame_add_wordart, wordart_set,
//! border_art_list (and the shadow / border_art fields of frame_set_style).

use std::borrow::Cow;

use pubrs_core::{
    BORDER_ART_PATTERNS, BORDER_ART_WIDTH, BorderArt, Color, Command, Dash, DocumentId, Frame,
    FrameContent, FrameId, MasterId, PageId, Paint, Shadow, Stroke, Warp, WordArtFrame,
    style::DEFAULT_FONT_FAMILY,
};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize};

use super::OptColor;
use super::frames::FrameAddOut;
use crate::error::{ApiError, ApiResult};
use crate::session::{Changed, Session};

/// `"none"` (clears) or a `T`. Used by optional effects whose absence means
/// "leave unchanged".
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct NoneOr<T>(pub Option<T>);

impl<'de, T: serde::de::DeserializeOwned> Deserialize<'de> for NoneOr<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        if let Some(s) = v.as_str()
            && (s.trim().eq_ignore_ascii_case("none") || s.trim().is_empty())
        {
            return Ok(NoneOr(None));
        }
        T::deserialize(v)
            .map(|t| NoneOr(Some(t)))
            .map_err(|e| serde::de::Error::custom(format!("expected \"none\" or an object ({e})")))
    }
}

impl<T: JsonSchema> JsonSchema for NoneOr<T> {
    fn schema_name() -> Cow<'static, str> {
        format!("NoneOr{}", T::schema_name()).into()
    }
    fn json_schema(g: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "anyOf": [
                {"const": "none"},
                g.subschema_for::<T>()
            ],
            "description": "\"none\" removes it; otherwise an object."
        })
    }
}

/// A drop shadow.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShadowIn {
    /// Points to the right (default 4; negative = left).
    #[serde(default)]
    pub offset_x: Option<f64>,
    /// Points down (default 4; negative = up).
    #[serde(default)]
    pub offset_y: Option<f64>,
    /// Blur radius in points, 0 to 50 (default 4; 0 = hard edge).
    #[serde(default)]
    pub blur: Option<f64>,
    /// Colour or swatch (default black; gradients are not allowed).
    #[serde(default)]
    pub color: Option<Paint>,
    /// 0 to 1 (default 0.5).
    #[serde(default)]
    pub opacity: Option<f64>,
}

impl ShadowIn {
    pub fn into_shadow(self) -> Shadow {
        Shadow {
            offset_x: self.offset_x.unwrap_or(4.0),
            offset_y: self.offset_y.unwrap_or(4.0),
            blur: self.blur.unwrap_or(4.0),
            color: self.color.unwrap_or(Paint::Color(Color::BLACK)),
            opacity: self.opacity.unwrap_or(0.5),
        }
    }
}

/// A BorderArt border.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BorderArtIn {
    /// A pattern name from border_art_list.
    pub pattern: String,
    /// Width of the border band in points, 4 to 72 (default 18).
    #[serde(default)]
    pub width: Option<f64>,
}

impl BorderArtIn {
    pub fn into_border(self) -> BorderArt {
        BorderArt {
            pattern: self.pattern,
            width: self.width.unwrap_or(18.0),
        }
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameAddWordArt {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    #[serde(default)]
    pub page_id: Option<PageId>,
    #[serde(default)]
    pub master_id: Option<MasterId>,
    /// The words (one line, up to 200 characters).
    pub text: String,
    /// Left edge in points from the page's left edge.
    pub x: f64,
    /// Top edge in points from the page's top edge (y grows down).
    pub y: f64,
    /// Width in points.
    pub width: f64,
    /// Height in points.
    pub height: f64,
    /// Font family (default: the document's default font).
    #[serde(default)]
    pub font: Option<String>,
    /// Nominal font size in points (default 120). The warped text is scaled
    /// down to fit the frame, never up, so a large size fills the frame.
    #[serde(default)]
    pub size: Option<f64>,
    /// none (default), arch_up, arch_down, circle, wave, slant_up, slant_down or inflate.
    #[serde(default)]
    pub warp: Option<Warp>,
    /// Letter fill: a colour, swatch or gradient (default #1E3A8A).
    #[serde(default)]
    pub fill: Option<Paint>,
    /// Outline colour or swatch, or "none" (default none).
    #[serde(default)]
    pub outline: Option<OptColor>,
    /// Outline width in points (default 1.5 when there is an outline).
    #[serde(default)]
    pub outline_width: Option<f64>,
    /// Shadow under the letters.
    #[serde(default)]
    pub shadow: Option<ShadowIn>,
    /// Display name shown in frame lists.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WordArtSet {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// A WordArt frame.
    pub frame_id: FrameId,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub font: Option<String>,
    /// Nominal font size in points.
    #[serde(default)]
    pub size: Option<f64>,
    /// none, arch_up, arch_down, circle, wave, slant_up, slant_down or inflate.
    #[serde(default)]
    pub warp: Option<Warp>,
    /// Letter fill: a colour, swatch or gradient.
    #[serde(default)]
    pub fill: Option<Paint>,
    /// Outline colour or swatch, or "none".
    #[serde(default)]
    pub outline: Option<OptColor>,
    /// Outline width in points.
    #[serde(default)]
    pub outline_width: Option<f64>,
    #[serde(default)]
    pub dash: Option<Dash>,
    /// A shadow under the letters, or "none".
    #[serde(default)]
    pub shadow: Option<NoneOr<ShadowIn>>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BorderArtList {}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct BorderArtInfo {
    /// Pass this as `pattern` in frame_set_style's border_art.
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct BorderArtListOut {
    pub patterns: Vec<BorderArtInfo>,
    /// Allowed border widths in points.
    pub min_width: f64,
    pub max_width: f64,
}

fn describe(name: &str) -> &'static str {
    match name {
        "stars" => "Gold five-point stars, red starburst corners.",
        "hearts" => "Red hearts, dark-red corner hearts.",
        "dots" => "Blue dots with small light dots between.",
        "waves" => "Two teal wavy lines, ring corners.",
        "zigzag" => "Orange zigzag with bracket corners.",
        "diamonds" => "Purple diamonds with light centres.",
        "leaves" => "Green leaves with red berries in the corners.",
        "ropes" => "Twisted brown rope, ring corners.",
        "checks" => "Black and white chequered band.",
        "scallops" => "Pink scalloped rings.",
        "arrows" => "Navy arrows running clockwise, square corners.",
        "snowflakes" => "Blue six-armed snowflakes.",
        _ => "",
    }
}

fn stroke_of(
    color: Option<Paint>,
    width: Option<f64>,
    dash: Option<Dash>,
    old: Option<&Stroke>,
) -> Option<Stroke> {
    let color = color.or_else(|| old.map(|o| o.color.clone()))?;
    Some(Stroke {
        color,
        width: width.or(old.map(|o| o.width)).unwrap_or(1.5),
        dash: dash.or(old.map(|o| o.dash)).unwrap_or_default(),
    })
}

impl Session {
    pub fn frame_add_wordart(&mut self, req: FrameAddWordArt) -> ApiResult<FrameAddOut> {
        let owner = self.owner_for(req.doc_id, req.page_id, req.master_id)?;
        let rect = self.rect_checked(req.x, req.y, req.width, req.height)?;
        let outline = match req.outline {
            Some(c) => stroke_of(c.0, req.outline_width, None, None),
            None if req.outline_width.is_some() => stroke_of(
                Some(Paint::Color(Color::BLACK)),
                req.outline_width,
                None,
                None,
            ),
            None => None,
        };
        let art = WordArtFrame {
            text: req.text,
            font: req.font.unwrap_or_else(|| DEFAULT_FONT_FAMILY.to_string()),
            size: req.size.unwrap_or(120.0),
            warp: req.warp.unwrap_or_default(),
            fill: req
                .fill
                .unwrap_or(Paint::Color(Color::rgb(0x1E, 0x3A, 0x8A))),
            outline,
            shadow: req.shadow.map(ShadowIn::into_shadow),
        };
        let fid = self.new_frame_id();
        let mut frame = Frame::new(fid, rect, FrameContent::WordArt(Box::new(art)));
        frame.name = req.name;
        let change = self.exec(
            req.doc_id,
            Command::AddFrame {
                parent: owner.into(),
                index: None,
                frame,
                story: None,
            },
        )?;
        Ok(FrameAddOut {
            frame_id: fid,
            story_id: None,
            asset_id: None,
            change,
        })
    }

    pub fn wordart_set(&mut self, req: WordArtSet) -> ApiResult<Changed> {
        let doc = self.get(req.doc_id)?.doc();
        let frame = doc.frame(req.frame_id).ok_or_else(|| {
            ApiError::not_found(
                "frame",
                &req.frame_id.to_string(),
                "list frames with frame_list",
            )
        })?;
        let Some(old) = frame.as_wordart() else {
            return Err(ApiError::invalid(
                format!("frame {} is not WordArt", req.frame_id),
                "create one with frame_add_wordart; use text_set for text frames",
            ));
        };
        let mut art = old.clone();
        if let Some(t) = req.text {
            art.text = t;
        }
        if let Some(f) = req.font {
            art.font = f;
        }
        if let Some(s) = req.size {
            art.size = s;
        }
        if let Some(w) = req.warp {
            art.warp = w;
        }
        if let Some(f) = req.fill {
            art.fill = f;
        }
        match req.outline {
            Some(c) => {
                art.outline = stroke_of(c.0, req.outline_width, req.dash, old.outline.as_ref())
            }
            None if req.outline_width.is_some() || req.dash.is_some() => {
                art.outline = stroke_of(
                    Some(Paint::Color(Color::BLACK)),
                    req.outline_width,
                    req.dash,
                    old.outline.as_ref(),
                );
            }
            None => {}
        }
        if let Some(s) = req.shadow {
            art.shadow = s.0.map(ShadowIn::into_shadow);
        }
        self.exec(
            req.doc_id,
            Command::SetWordArt {
                frame: req.frame_id,
                art,
            },
        )
    }

    pub fn border_art_list(&mut self, _req: BorderArtList) -> ApiResult<BorderArtListOut> {
        Ok(BorderArtListOut {
            patterns: BORDER_ART_PATTERNS
                .iter()
                .map(|n| BorderArtInfo {
                    name: (*n).to_string(),
                    description: describe(n).to_string(),
                })
                .collect(),
            min_width: *BORDER_ART_WIDTH.start(),
            max_width: *BORDER_ART_WIDTH.end(),
        })
    }
}

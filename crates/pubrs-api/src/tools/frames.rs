//! Frame tools.

use pubrs_core::align::{AlignEdge, DistributeAxis, align, bounds_of, distribute};
use pubrs_core::{
    Align, CharProps, Color, Command, Dash, DocumentId, Frame, FrameContent, FrameId, FrameKind,
    ImageFit, LineDirection, MasterId, Owner, PageId, Paint, ParaProps, Paragraph, Parent,
    Placeholder, PlaceholderKind, Rect, Shape, ShapeFrame, Story, StoryId, Stroke, TextFrame,
    TextWrap, WrapMode,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{FrameSummary, OptColor, require_confirm, summarize};
use crate::error::{ApiError, ApiResult};
use crate::session::{Changed, Session};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameAddText {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Page to place the frame on (default: first page). Or give master_id.
    #[serde(default)]
    pub page_id: Option<PageId>,
    #[serde(default)]
    pub master_id: Option<MasterId>,
    /// Left edge (points from the page's left edge).
    pub x: f64,
    /// Top edge (points from the page's top edge, y grows down).
    pub y: f64,
    /// Width in points.
    pub width: f64,
    /// Height in points.
    pub height: f64,
    /// Initial text; "\n" separates paragraphs. With markup=true, `# `/`## `
    /// headings and **bold**, *italic*, __underline__ are recognised.
    #[serde(default)]
    pub text: Option<String>,
    /// Interpret markup in `text` (see text).
    #[serde(default)]
    pub markup: bool,
    /// Paragraph style name for all paragraphs (e.g. "Body", "Heading 1").
    #[serde(default)]
    pub style: Option<String>,
    /// Font family for the text, e.g. "Liberation Serif" (see fonts_list).
    #[serde(default)]
    pub font_family: Option<String>,
    /// Font size in points.
    #[serde(default)]
    pub font_size: Option<f64>,
    /// Text colour: hex or `{"swatch": id, "tint": 0-100}`.
    #[serde(default)]
    pub color: Option<Paint>,
    #[serde(default)]
    pub align: Option<Align>,
    /// Number of columns (1-12).
    #[serde(default)]
    pub columns: Option<u32>,
    /// Display name shown in frame lists.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FrameAddOut {
    pub frame_id: FrameId,
    /// Text frames: the story holding the text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub story_id: Option<StoryId>,
    /// Image frames: the placed image asset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<pubrs_core::AssetId>,
    #[serde(flatten)]
    pub change: Changed,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameAddImage {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    #[serde(default)]
    pub page_id: Option<PageId>,
    #[serde(default)]
    pub master_id: Option<MasterId>,
    /// Left edge in points from the page's left edge.
    pub x: f64,
    /// Top edge in points from the page's top edge (y grows down).
    pub y: f64,
    /// Width in points.
    pub width: f64,
    /// Height in points.
    pub height: f64,
    /// Image file (PNG, JPEG or SVG). Omit both path and base64 for an empty picture frame.
    #[serde(default)]
    pub path: Option<String>,
    /// Image bytes, base64.
    #[serde(default)]
    pub base64: Option<String>,
    /// SVG markup as text (drawn artwork when no photo is available).
    #[serde(default)]
    pub svg: Option<String>,
    /// Display name for base64 images.
    #[serde(default)]
    pub filename: Option<String>,
    /// fill (default, covers the frame), fit (letterbox) or stretch.
    #[serde(default)]
    pub fit: Option<ImageFit>,
    /// Display name shown in frame lists.
    #[serde(default)]
    pub name: Option<String>,
}

/// Shape type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShapeKind {
    Rectangle,
    Ellipse,
    Line,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameAddShape {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    #[serde(default)]
    pub page_id: Option<PageId>,
    #[serde(default)]
    pub master_id: Option<MasterId>,
    pub shape: ShapeKind,
    /// Bounding box. A line runs corner to corner; height 0 = horizontal line.
    /// Left edge in points from the page's left edge.
    pub x: f64,
    /// Top edge in points from the page's top edge (y grows down).
    pub y: f64,
    /// Width in points.
    pub width: f64,
    /// Height in points.
    pub height: f64,
    /// Rectangles: corner radius in points.
    #[serde(default)]
    pub corner_radius: Option<f64>,
    /// Lines: down_right (top-left to bottom-right, default) or up_right.
    #[serde(default)]
    pub line_direction: Option<LineDirection>,
    /// Fill colour (default none).
    #[serde(default)]
    pub fill: Option<OptColor>,
    /// Outline colour (default black for lines, none otherwise).
    #[serde(default)]
    pub stroke: Option<OptColor>,
    /// Outline width in points (default 1).
    #[serde(default)]
    pub stroke_width: Option<f64>,
    #[serde(default)]
    pub dash: Option<Dash>,
    /// Display name shown in frame lists.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameDelete {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub frame_id: FrameId,
    /// Must be true.
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameMove {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub frame_id: FrameId,
    /// New left edge in points (absolute). Or use dx.
    #[serde(default)]
    pub x: Option<f64>,
    /// New top edge in points (absolute). Or use dy.
    #[serde(default)]
    pub y: Option<f64>,
    /// Move right by this many points (relative).
    #[serde(default)]
    pub dx: Option<f64>,
    /// Move down by this many points (relative).
    #[serde(default)]
    pub dy: Option<f64>,
    /// Snap to guides, margins, page edges and other frames (and the grid if enabled).
    #[serde(default)]
    pub snap: bool,
}

/// Fixed point when resizing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    #[default]
    TopLeft,
    Center,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameResize {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub frame_id: FrameId,
    /// New width in points.
    pub width: f64,
    /// New height in points. With keep_aspect it is recomputed from width.
    #[serde(default)]
    pub height: Option<f64>,
    #[serde(default)]
    pub anchor: Anchor,
    /// Keep the current aspect ratio (default: true for image frames with
    /// lock_aspect, false otherwise).
    #[serde(default)]
    pub keep_aspect: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameRotate {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub frame_id: FrameId,
    /// Absolute clockwise rotation in degrees.
    pub degrees: f64,
}

/// Z-order change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ZAction {
    BringToFront,
    SendToBack,
    BringForward,
    SendBackward,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameSetZorder {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub frame_id: FrameId,
    #[serde(default)]
    pub action: Option<ZAction>,
    /// Explicit position among siblings, 0 = back (instead of action).
    #[serde(default)]
    pub index: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameGroup {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// At least two frames on the same page.
    pub frame_ids: Vec<FrameId>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FrameGroupOut {
    pub group_id: FrameId,
    #[serde(flatten)]
    pub change: Changed,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameUngroup {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub group_id: FrameId,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FrameUngroupOut {
    /// The former members.
    pub frame_ids: Vec<FrameId>,
    #[serde(flatten)]
    pub change: Changed,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameLock {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub frame_id: FrameId,
    /// true locks (cannot be moved, resized, rotated or deleted), false unlocks.
    #[serde(default = "super::yes")]
    pub locked: bool,
}

/// What to align/distribute relative to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RelativeTo {
    /// The bounding box of the given frames.
    #[default]
    Selection,
    /// The page trim box.
    Page,
    /// The page margin box.
    Margins,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameAlign {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub frame_ids: Vec<FrameId>,
    /// left, center, right, top, middle or bottom.
    pub edge: AlignEdge,
    #[serde(default)]
    pub relative_to: RelativeTo,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameDistribute {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// At least two frames.
    pub frame_ids: Vec<FrameId>,
    /// horizontal or vertical.
    pub axis: DistributeAxis,
    /// selection (default: outer frames stay put), page or margins.
    #[serde(default)]
    pub within: RelativeTo,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameSetStyle {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub frame_id: FrameId,
    /// Background fill, or "none".
    #[serde(default)]
    pub fill: Option<OptColor>,
    /// Outline colour, or "none".
    #[serde(default)]
    pub stroke: Option<OptColor>,
    /// Outline width in points.
    #[serde(default)]
    pub stroke_width: Option<f64>,
    #[serde(default)]
    pub dash: Option<Dash>,
    /// 0 (invisible) to 1 (opaque).
    #[serde(default)]
    pub opacity: Option<f64>,
    /// Display name (empty string clears).
    /// Display name shown in frame lists.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameSetWrap {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub frame_id: FrameId,
    /// none, square (text flows around the frame) or top_bottom.
    pub mode: WrapMode,
    /// Clearance in points (default 6).
    #[serde(default)]
    pub margin: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameSetPlaceholder {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub frame_id: FrameId,
    /// Semantic name like "headline" or "hero_image"; null removes the placeholder.
    pub name: Option<String>,
    /// Sample text / instruction shown until filled.
    #[serde(default)]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameList {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Only frames on this page.
    #[serde(default)]
    pub page_id: Option<PageId>,
    /// Only frames on this master page.
    #[serde(default)]
    pub master_id: Option<MasterId>,
    /// Only frames of this kind: text, image, shape or group.
    #[serde(default)]
    pub kind: Option<FrameKind>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FrameListOut {
    pub frames: Vec<FrameSummary>,
    pub revision: u64,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameRef {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub frame_id: FrameId,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FrameGetOut {
    pub summary: FrameSummary,
    /// The full frame as stored in the document.
    pub frame: Frame,
    /// Text frames: true if the frame's chain overflows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overflowing: Option<bool>,
    /// Image frames: print resolution of the placed image.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_dpi: Option<f64>,
    pub revision: u64,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameDuplicate {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub frame_id: FrameId,
    /// Offset of the copy in points (default 12, 12).
    #[serde(default)]
    pub dx: Option<f64>,
    #[serde(default)]
    pub dy: Option<f64>,
    /// Put the copy on another page.
    #[serde(default)]
    pub page_id: Option<PageId>,
}

fn base_text_props(req: &FrameAddText) -> (CharProps, ParaProps) {
    (
        CharProps {
            font_family: req.font_family.clone(),
            font_size: req.font_size,
            color: req.color.clone(),
            ..Default::default()
        },
        ParaProps {
            align: req.align,
            ..Default::default()
        },
    )
}

impl Session {
    pub(crate) fn rect_checked(&self, x: f64, y: f64, w: f64, h: f64) -> ApiResult<Rect> {
        let r = Rect::new(x, y, w, h);
        if !r.is_finite() || w < 0.0 || h < 0.0 {
            return Err(ApiError::invalid(
                "x, y, width and height must be finite; width/height >= 0",
                "sizes are in points (72 per inch)",
            ));
        }
        Ok(r)
    }

    /// Paragraphs for new text with optional markup, style and overrides.
    pub(crate) fn build_paragraphs(
        &self,
        doc_id: Option<DocumentId>,
        text: &str,
        markup: bool,
        style: Option<&str>,
        chars: &CharProps,
        para: &ParaProps,
    ) -> ApiResult<Vec<Paragraph>> {
        let style_id = match style {
            Some(s) => {
                Some(self.resolve_style(doc_id, s, Some(pubrs_core::StyleKind::Paragraph))?)
            }
            None => None,
        };
        let doc = self.get(doc_id)?.doc();
        let mut paras = if markup {
            crate::markup::parse(text, &doc.styles)
        } else {
            pubrs_core::text::normalize_newlines(text)
                .split('\n')
                .map(Paragraph::plain)
                .collect()
        };
        for p in &mut paras {
            if p.style.is_none() {
                p.style = style_id;
            }
            p.props = p.props.merged(para);
            for r in &mut p.runs {
                r.props = chars.merged(&r.props);
            }
        }
        Ok(paras)
    }

    pub fn frame_add_text(&mut self, req: FrameAddText) -> ApiResult<FrameAddOut> {
        let owner = self.owner_for(req.doc_id, req.page_id, req.master_id)?;
        let rect = self.rect_checked(req.x, req.y, req.width, req.height)?;
        let (chars, para) = base_text_props(&req);
        let paragraphs = self.build_paragraphs(
            req.doc_id,
            req.text.as_deref().unwrap_or(""),
            req.markup,
            req.style.as_deref(),
            &chars,
            &para,
        )?;
        let fid = self.new_frame_id();
        let sid = self.new_story_id();
        let mut tf = TextFrame::new(sid);
        if let Some(c) = req.columns {
            tf.columns = c;
        }
        let mut frame = Frame::new(fid, rect, FrameContent::Text(tf));
        frame.name = req.name.clone();
        let story = Story {
            id: sid,
            frames: vec![fid],
            paragraphs,
        };
        let change = self.exec(
            req.doc_id,
            Command::AddFrame {
                parent: owner.into(),
                index: None,
                frame,
                story: Some(story),
            },
        )?;
        Ok(FrameAddOut {
            frame_id: fid,
            story_id: Some(sid),
            asset_id: None,
            change,
        })
    }

    pub fn frame_add_image(&mut self, req: FrameAddImage) -> ApiResult<FrameAddOut> {
        let owner = self.owner_for(req.doc_id, req.page_id, req.master_id)?;
        let rect = self.rect_checked(req.x, req.y, req.width, req.height)?;
        let mut commands = Vec::new();
        let mut asset_id = None;
        if req.path.is_some() || req.base64.is_some() || req.svg.is_some() {
            let (cmd, id) = self.import_asset(
                req.path.as_deref(),
                req.base64.as_deref(),
                req.svg.as_deref(),
                req.filename.as_deref(),
            )?;
            commands.push(cmd);
            asset_id = Some(id);
        }
        let fid = self.new_frame_id();
        let mut img = pubrs_core::ImageFrame::new(asset_id);
        if let Some(fit) = req.fit {
            img.fit = fit;
        }
        let mut frame = Frame::new(fid, rect, FrameContent::Image(img));
        frame.name = req.name;
        commands.push(Command::AddFrame {
            parent: owner.into(),
            index: None,
            frame,
            story: None,
        });
        let change = self.exec(
            req.doc_id,
            Command::Batch {
                label: "add picture".into(),
                commands,
            },
        )?;
        Ok(FrameAddOut {
            frame_id: fid,
            story_id: None,
            asset_id,
            change,
        })
    }

    pub fn frame_add_shape(&mut self, req: FrameAddShape) -> ApiResult<FrameAddOut> {
        let owner = self.owner_for(req.doc_id, req.page_id, req.master_id)?;
        let rect = self.rect_checked(req.x, req.y, req.width, req.height)?;
        let shape = match req.shape {
            ShapeKind::Rectangle => Shape::Rectangle {
                corner_radius: req.corner_radius.unwrap_or(0.0),
            },
            ShapeKind::Ellipse => Shape::Ellipse,
            ShapeKind::Line => Shape::Line {
                direction: req.line_direction.unwrap_or_default(),
            },
        };
        let fid = self.new_frame_id();
        let mut frame = Frame::new(fid, rect, FrameContent::Shape(ShapeFrame { shape }));
        frame.name = req.name;
        frame.style.fill = req.fill.and_then(|c| c.0);
        let default_stroke = (req.shape == ShapeKind::Line).then_some(Paint::Color(Color::BLACK));
        let stroke_color = match req.stroke {
            Some(c) => c.0,
            None if req.stroke_width.is_some() => Some(Paint::Color(Color::BLACK)),
            None => default_stroke,
        };
        frame.style.stroke = stroke_color.map(|color| Stroke {
            color,
            width: req.stroke_width.unwrap_or(1.0),
            dash: req.dash.unwrap_or_default(),
        });
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

    pub fn frame_delete(&mut self, req: FrameDelete) -> ApiResult<Changed> {
        require_confirm(req.confirm, "deleting a frame")?;
        self.require_frame(req.doc_id, req.frame_id)?;
        self.exec(
            req.doc_id,
            Command::DeleteFrame {
                frame: req.frame_id,
            },
        )
    }

    fn frame(&self, doc_id: Option<DocumentId>, id: FrameId) -> ApiResult<Frame> {
        self.get(doc_id)?.doc().frame(id).cloned().ok_or_else(|| {
            ApiError::not_found("frame", &id.to_string(), "list frames with frame_list")
        })
    }

    pub fn frame_move(&mut self, req: FrameMove) -> ApiResult<Changed> {
        let f = self.frame(req.doc_id, req.frame_id)?;
        let mut r = f.rect;
        r.x = req.x.unwrap_or(r.x) + req.dx.unwrap_or(0.0);
        r.y = req.y.unwrap_or(r.y) + req.dy.unwrap_or(0.0);
        if req.snap {
            let doc = self.get(req.doc_id)?.doc();
            if let Some(owner) = doc.owner_of(req.frame_id) {
                let exclude = doc.descendants(req.frame_id);
                r = pubrs_core::snap::snap_move(doc, owner, r, &exclude);
            }
        }
        self.exec(
            req.doc_id,
            Command::SetFrameRect {
                frame: req.frame_id,
                rect: r,
            },
        )
    }

    pub fn frame_resize(&mut self, req: FrameResize) -> ApiResult<Changed> {
        let f = self.frame(req.doc_id, req.frame_id)?;
        let lock = f.as_image().is_some_and(|i| i.lock_aspect);
        let keep = req.keep_aspect.unwrap_or(lock && req.height.is_none());
        let w = req.width;
        let h = if keep && f.rect.width > 0.0 {
            w * f.rect.height / f.rect.width
        } else {
            req.height.unwrap_or(f.rect.height)
        };
        let r = match req.anchor {
            Anchor::TopLeft => Rect::new(f.rect.x, f.rect.y, w, h),
            Anchor::Center => {
                let c = f.rect.center();
                Rect::new(c.x - w / 2.0, c.y - h / 2.0, w, h)
            }
        };
        self.rect_checked(r.x, r.y, r.width, r.height)?;
        self.exec(
            req.doc_id,
            Command::SetFrameRect {
                frame: req.frame_id,
                rect: r,
            },
        )
    }

    pub fn frame_rotate(&mut self, req: FrameRotate) -> ApiResult<Changed> {
        self.exec(
            req.doc_id,
            Command::SetRotation {
                frame: req.frame_id,
                degrees: req.degrees,
            },
        )
    }

    pub fn frame_set_zorder(&mut self, req: FrameSetZorder) -> ApiResult<Changed> {
        let doc = self.get(req.doc_id)?.doc();
        self.require_frame(req.doc_id, req.frame_id)?;
        let parent = doc
            .parent_of(req.frame_id)
            .ok_or_else(|| ApiError::internal("frame has no parent"))?;
        let list = doc.children(parent).cloned().unwrap_or_default();
        let pos = list.iter().position(|f| *f == req.frame_id).unwrap_or(0);
        let last = list.len().saturating_sub(1);
        let index = match (req.index, req.action) {
            (Some(i), _) => i.min(last),
            (None, Some(ZAction::BringToFront)) => last,
            (None, Some(ZAction::SendToBack)) => 0,
            (None, Some(ZAction::BringForward)) => (pos + 1).min(last),
            (None, Some(ZAction::SendBackward)) => pos.saturating_sub(1),
            (None, None) => {
                return Err(ApiError::invalid(
                    "give action or index",
                    "e.g. {\"action\": \"bring_to_front\"}",
                ));
            }
        };
        self.exec(
            req.doc_id,
            Command::SetZIndex {
                frame: req.frame_id,
                index,
            },
        )
    }

    pub fn frame_group(&mut self, req: FrameGroup) -> ApiResult<FrameGroupOut> {
        let gid = self.new_frame_id();
        let change = self.exec(
            req.doc_id,
            Command::Group {
                group: gid,
                frames: req.frame_ids,
            },
        )?;
        Ok(FrameGroupOut {
            group_id: gid,
            change,
        })
    }

    pub fn frame_ungroup(&mut self, req: FrameUngroup) -> ApiResult<FrameUngroupOut> {
        let g = self.frame(req.doc_id, req.group_id)?;
        let members = g.children().to_vec();
        let change = self.exec(
            req.doc_id,
            Command::Ungroup {
                group: req.group_id,
            },
        )?;
        Ok(FrameUngroupOut {
            frame_ids: members,
            change,
        })
    }

    pub fn frame_lock(&mut self, req: FrameLock) -> ApiResult<Changed> {
        self.exec(
            req.doc_id,
            Command::SetLocked {
                frame: req.frame_id,
                locked: req.locked,
            },
        )
    }

    fn reference_rect(
        &self,
        doc_id: Option<DocumentId>,
        rel: RelativeTo,
        items: &[(FrameId, Rect)],
    ) -> ApiResult<Rect> {
        let doc = self.get(doc_id)?.doc();
        let page = Rect::new(0.0, 0.0, doc.setup.size.width, doc.setup.size.height);
        Ok(match rel {
            RelativeTo::Selection => bounds_of(items).unwrap_or(page),
            RelativeTo::Page => page,
            RelativeTo::Margins => page.inset(&doc.setup.margins),
        })
    }

    fn bounds_of(
        &self,
        doc_id: Option<DocumentId>,
        ids: &[FrameId],
    ) -> ApiResult<Vec<(FrameId, Rect)>> {
        ids.iter()
            .map(|id| self.frame(doc_id, *id).map(|f| (f.id, f.bounds())))
            .collect()
    }

    fn apply_moves(
        &mut self,
        doc_id: Option<DocumentId>,
        label: &str,
        moves: Vec<pubrs_core::align::Move>,
    ) -> ApiResult<Changed> {
        let mut commands = Vec::new();
        for m in moves {
            if m.dx == 0.0 && m.dy == 0.0 {
                continue;
            }
            let f = self.frame(doc_id, m.frame)?;
            commands.push(Command::SetFrameRect {
                frame: m.frame,
                rect: f.rect.translate(m.dx, m.dy),
            });
        }
        self.exec(
            doc_id,
            Command::Batch {
                label: label.into(),
                commands,
            },
        )
    }

    pub fn frame_align(&mut self, req: FrameAlign) -> ApiResult<Changed> {
        if req.frame_ids.is_empty() {
            return Err(ApiError::invalid(
                "frame_ids is empty",
                "pass the frames to align",
            ));
        }
        let items = self.bounds_of(req.doc_id, &req.frame_ids)?;
        let reference = self.reference_rect(req.doc_id, req.relative_to, &items)?;
        let moves = align(&items, req.edge, reference);
        self.apply_moves(req.doc_id, "align", moves)
    }

    pub fn frame_distribute(&mut self, req: FrameDistribute) -> ApiResult<Changed> {
        if req.frame_ids.len() < 2 {
            return Err(ApiError::invalid(
                "need at least two frames",
                "pass two or more frame ids",
            ));
        }
        let items = self.bounds_of(req.doc_id, &req.frame_ids)?;
        let within = match req.within {
            RelativeTo::Selection => None,
            other => Some(self.reference_rect(req.doc_id, other, &items)?),
        };
        let moves = distribute(&items, req.axis, within);
        self.apply_moves(req.doc_id, "distribute", moves)
    }

    pub fn frame_set_style(&mut self, req: FrameSetStyle) -> ApiResult<Changed> {
        let f = self.frame(req.doc_id, req.frame_id)?;
        let mut style = f.style.clone();
        if let Some(fill) = req.fill {
            style.fill = fill.0;
        }
        let existing = style.stroke.clone();
        let color = match req.stroke {
            Some(c) => c.0,
            None => existing
                .as_ref()
                .map(|s| s.color.clone())
                .or_else(|| req.stroke_width.map(|_| Paint::Color(Color::BLACK))),
        };
        style.stroke = color.map(|color| Stroke {
            color,
            width: req
                .stroke_width
                .or(existing.as_ref().map(|s| s.width))
                .unwrap_or(1.0),
            dash: req
                .dash
                .or(existing.as_ref().map(|s| s.dash))
                .unwrap_or_default(),
        });
        if let Some(o) = req.opacity {
            style.opacity = o;
        }
        let mut commands = vec![Command::SetFrameStyle {
            frame: req.frame_id,
            style,
        }];
        if let Some(name) = req.name {
            commands.push(Command::SetFrameName {
                frame: req.frame_id,
                name: (!name.is_empty()).then_some(name),
            });
        }
        self.exec(
            req.doc_id,
            Command::Batch {
                label: "frame style".into(),
                commands,
            },
        )
    }

    pub fn frame_set_wrap(&mut self, req: FrameSetWrap) -> ApiResult<Changed> {
        self.exec(
            req.doc_id,
            Command::SetWrap {
                frame: req.frame_id,
                wrap: TextWrap {
                    mode: req.mode,
                    margin: req.margin.unwrap_or(6.0),
                },
            },
        )
    }

    pub fn frame_set_placeholder(&mut self, req: FrameSetPlaceholder) -> ApiResult<Changed> {
        let f = self.frame(req.doc_id, req.frame_id)?;
        let placeholder = match req.name {
            None => None,
            Some(name) => {
                let kind = match f.kind() {
                    FrameKind::Text => PlaceholderKind::Text,
                    FrameKind::Image => PlaceholderKind::Image,
                    _ => {
                        return Err(ApiError::invalid(
                            "only text and image frames can be placeholders",
                            "pick a text or picture frame",
                        ));
                    }
                };
                let hint = match (req.hint, kind) {
                    (Some(h), _) => h,
                    (None, PlaceholderKind::Text) => self
                        .get(req.doc_id)?
                        .doc()
                        .story_of(f.id)
                        .and_then(|s| {
                            self.get(req.doc_id)
                                .ok()?
                                .doc()
                                .story(s)
                                .map(|s| s.plain_text())
                        })
                        .unwrap_or_default(),
                    (None, PlaceholderKind::Image) => String::new(),
                };
                Some(Placeholder { name, kind, hint })
            }
        };
        self.exec(
            req.doc_id,
            Command::SetPlaceholder {
                frame: req.frame_id,
                placeholder,
            },
        )
    }

    pub fn frame_list(&mut self, req: FrameList) -> ApiResult<FrameListOut> {
        let open = self.get(req.doc_id)?;
        let doc = open.doc();
        let owners: Vec<Owner> = match (req.page_id, req.master_id) {
            (Some(p), _) => {
                self.require_page(req.doc_id, p)?;
                vec![Owner::Page(p)]
            }
            (None, Some(m)) => vec![Owner::Master(m)],
            (None, None) => doc
                .pages
                .iter()
                .map(|p| Owner::Page(p.id))
                .chain(doc.masters.iter().map(|m| Owner::Master(m.id)))
                .collect(),
        };
        let frames = owners
            .iter()
            .flat_map(|o| doc.all_frames_of(*o))
            .filter_map(|id| doc.frame(id))
            .filter(|f| req.kind.is_none_or(|k| f.kind() == k))
            .map(|f| summarize(doc, f))
            .collect();
        Ok(FrameListOut {
            frames,
            revision: open.revision(),
        })
    }

    pub fn frame_get(&mut self, req: FrameRef) -> ApiResult<FrameGetOut> {
        let f = self.frame(req.doc_id, req.frame_id)?;
        let overflowing = match f.as_text() {
            Some(t) => {
                let layout = self.layout(req.doc_id)?;
                Some(
                    layout
                        .stories
                        .get(&t.story)
                        .is_some_and(|s| s.overflow.is_some()),
                )
            }
            None => None,
        };
        let open = self.get(req.doc_id)?;
        let doc = open.doc();
        Ok(FrameGetOut {
            summary: summarize(doc, &f),
            effective_dpi: pubrs_core::validate::effective_dpi(doc, f.id),
            frame: f,
            overflowing,
            revision: open.revision(),
        })
    }

    pub fn frame_duplicate(&mut self, req: FrameDuplicate) -> ApiResult<FrameAddOut> {
        let f = self.frame(req.doc_id, req.frame_id)?;
        if f.kind() == FrameKind::Group {
            return Err(ApiError::invalid(
                "groups cannot be duplicated yet",
                "ungroup, duplicate the members, then group again",
            ));
        }
        let doc = self.get(req.doc_id)?.doc();
        let parent: Parent = match req.page_id {
            Some(p) => {
                self.require_page(req.doc_id, p)?;
                Parent::Page(p)
            }
            None => doc
                .owner_of(f.id)
                .map(Parent::from)
                .ok_or_else(|| ApiError::internal("frame has no page"))?,
        };
        let story_src = f.as_text().and_then(|t| doc.story(t.story)).cloned();
        let fid = self.new_frame_id();
        let mut copy = f.clone();
        copy.id = fid;
        copy.locked = false;
        copy.placeholder = None;
        copy.rect = f
            .rect
            .translate(req.dx.unwrap_or(12.0), req.dy.unwrap_or(12.0));
        let mut story = None;
        let mut story_id = None;
        if let (Some(src), Some(t)) = (story_src, copy.as_text_mut()) {
            let sid = StoryId::new(&mut *self.ids);
            t.story = sid;
            story_id = Some(sid);
            story = Some(Story {
                id: sid,
                frames: vec![fid],
                paragraphs: src.paragraphs,
            });
        }
        let asset_id = copy.as_image().and_then(|i| i.asset);
        let change = self.exec(
            req.doc_id,
            Command::AddFrame {
                parent,
                index: None,
                frame: copy,
                story,
            },
        )?;
        Ok(FrameAddOut {
            frame_id: fid,
            story_id,
            asset_id,
            change,
        })
    }
}

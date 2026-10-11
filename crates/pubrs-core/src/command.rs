//! Commands: every document mutation.
//!
//! A command is plain serializable data with a stable snake_case name (the
//! `op` tag). Commands carry the ids of anything they create, so applying one
//! is deterministic. Inversion is generic: the [`crate::Editor`] diffs the
//! document before and after and stores the result as a [`Patch`], which is
//! itself a command (`op: "patch"`).

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::bytes::Bytes;
use crate::geom::{Margins, Point, Rect, Size};
use crate::id::{AssetId, FrameId, GuideId, MasterId, PageId, StoryId, StyleId};
use crate::model::{
    Asset, Crop, Document, Frame, FrameContent, FrameStyle, GridSettings, Guide, ImageFit,
    MasterPage, Page, Paragraph, Parent, Placeholder, PlaceholderKind, Shape, Story, TextWrap,
    VerticalAlign,
};
use crate::patch::Patch;
use crate::style::{CharProps, ParaProps, Style, StyleKind};
use crate::text::{RunFormat, normalize_paragraph};

/// Largest page or frame dimension accepted (200 inches, the PDF limit).
pub const MAX_DIMENSION: f64 = 14_400.0;

/// Why a command could not be applied. The document is left unchanged.
#[derive(Debug, Clone, PartialEq, thiserror::Error, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum CommandError {
    #[error("{kind} {id} not found")]
    NotFound { kind: String, id: String },
    #[error("frame {frame} is locked")]
    Locked { frame: FrameId },
    #[error("invalid argument: {message}")]
    Invalid { message: String },
    #[error("conflict: {message}")]
    Conflict { message: String },
    /// A command panicked; the document was rolled back. This is a bug.
    #[error("internal error: {message}")]
    Internal { message: String },
}

impl CommandError {
    pub fn not_found(kind: &str, id: impl ToString) -> Self {
        Self::NotFound {
            kind: kind.to_string(),
            id: id.to_string(),
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid {
            message: message.into(),
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict {
            message: message.into(),
        }
    }

    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound { .. } => "not_found",
            Self::Locked { .. } => "locked",
            Self::Invalid { .. } => "invalid_argument",
            Self::Conflict { .. } => "conflict",
            Self::Internal { .. } => "internal",
        }
    }
}

pub type CmdResult<T = ()> = Result<T, CommandError>;

/// Optional updates to a text frame's layout properties.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TextFrameProps {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insets: Option<Margins>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column_gap: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertical_align: Option<VerticalAlign>,
}

/// Every mutation of a document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Command {
    // ---- document ----
    SetMeta {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        author: Option<String>,
    },
    SetPageSize {
        size: Size,
    },
    SetMargins {
        margins: Margins,
    },
    SetBleed {
        bleed: f64,
    },
    SetGrid {
        grid: GridSettings,
    },
    // ---- pages ----
    AddPage {
        page: Page,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        index: Option<usize>,
    },
    DeletePage {
        page: PageId,
    },
    MovePage {
        page: PageId,
        index: usize,
    },
    AddMaster {
        master: MasterPage,
    },
    DeleteMaster {
        master: MasterId,
    },
    RenameMaster {
        master: MasterId,
        name: String,
    },
    SetPageMaster {
        page: PageId,
        master: Option<MasterId>,
    },
    AddGuide {
        owner: crate::model::Owner,
        guide: Guide,
    },
    RemoveGuide {
        owner: crate::model::Owner,
        guide: GuideId,
    },
    MoveGuide {
        owner: crate::model::Owner,
        guide: GuideId,
        position: f64,
    },
    // ---- frames ----
    AddFrame {
        parent: Parent,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        index: Option<usize>,
        frame: Frame,
        /// Required for text frames: a new story whose chain is `[frame.id]`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        story: Option<Story>,
    },
    DeleteFrame {
        frame: FrameId,
    },
    SetFrameRect {
        frame: FrameId,
        rect: Rect,
    },
    SetRotation {
        frame: FrameId,
        degrees: f64,
    },
    SetZIndex {
        frame: FrameId,
        index: usize,
    },
    Group {
        group: FrameId,
        frames: Vec<FrameId>,
    },
    Ungroup {
        group: FrameId,
    },
    SetLocked {
        frame: FrameId,
        locked: bool,
    },
    SetFrameStyle {
        frame: FrameId,
        style: FrameStyle,
    },
    SetWrap {
        frame: FrameId,
        wrap: TextWrap,
    },
    SetFrameName {
        frame: FrameId,
        name: Option<String>,
    },
    SetPlaceholder {
        frame: FrameId,
        placeholder: Option<Placeholder>,
    },
    SetTextFrame {
        frame: FrameId,
        props: TextFrameProps,
    },
    SetShape {
        frame: FrameId,
        shape: Shape,
    },
    // ---- text ----
    SetStory {
        story: StoryId,
        paragraphs: Vec<Paragraph>,
    },
    InsertText {
        story: StoryId,
        at: usize,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        format: Option<RunFormat>,
    },
    DeleteText {
        story: StoryId,
        start: usize,
        end: usize,
    },
    ReplaceText {
        story: StoryId,
        start: usize,
        end: usize,
        text: String,
    },
    ApplyCharProps {
        story: StoryId,
        start: usize,
        end: usize,
        props: CharProps,
    },
    SetCharStyle {
        story: StoryId,
        start: usize,
        end: usize,
        style: Option<StyleId>,
    },
    ClearCharFormat {
        story: StoryId,
        start: usize,
        end: usize,
    },
    ApplyParaProps {
        story: StoryId,
        start: usize,
        end: usize,
        props: ParaProps,
    },
    SetParaStyle {
        story: StoryId,
        start: usize,
        end: usize,
        style: Option<StyleId>,
    },
    LinkFrames {
        from: FrameId,
        to: FrameId,
    },
    UnlinkFrame {
        frame: FrameId,
        new_story: StoryId,
    },
    // ---- styles ----
    AddStyle {
        style: Style,
    },
    UpdateStyle {
        style: Style,
    },
    DeleteStyle {
        style: StyleId,
    },
    // ---- images ----
    AddAsset {
        asset: Asset,
        data: Bytes,
    },
    RemoveAsset {
        asset: AssetId,
    },
    SetImage {
        frame: FrameId,
        asset: Option<AssetId>,
    },
    SetImageCrop {
        frame: FrameId,
        crop: Crop,
    },
    SetImageFit {
        frame: FrameId,
        fit: ImageFit,
    },
    SetLockAspect {
        frame: FrameId,
        lock: bool,
    },
    // ---- composition ----
    /// Several commands applied atomically as one undo step.
    Batch {
        label: String,
        commands: Vec<Command>,
    },
    /// A raw state patch (the inverse of any command).
    Patch(Patch),
}

impl Command {
    /// The stable name of this command (equal to its serialized `op` tag).
    pub fn name(&self) -> &'static str {
        match self {
            Self::SetMeta { .. } => "set_meta",
            Self::SetPageSize { .. } => "set_page_size",
            Self::SetMargins { .. } => "set_margins",
            Self::SetBleed { .. } => "set_bleed",
            Self::SetGrid { .. } => "set_grid",
            Self::AddPage { .. } => "add_page",
            Self::DeletePage { .. } => "delete_page",
            Self::MovePage { .. } => "move_page",
            Self::AddMaster { .. } => "add_master",
            Self::DeleteMaster { .. } => "delete_master",
            Self::RenameMaster { .. } => "rename_master",
            Self::SetPageMaster { .. } => "set_page_master",
            Self::AddGuide { .. } => "add_guide",
            Self::RemoveGuide { .. } => "remove_guide",
            Self::MoveGuide { .. } => "move_guide",
            Self::AddFrame { .. } => "add_frame",
            Self::DeleteFrame { .. } => "delete_frame",
            Self::SetFrameRect { .. } => "set_frame_rect",
            Self::SetRotation { .. } => "set_rotation",
            Self::SetZIndex { .. } => "set_z_index",
            Self::Group { .. } => "group",
            Self::Ungroup { .. } => "ungroup",
            Self::SetLocked { .. } => "set_locked",
            Self::SetFrameStyle { .. } => "set_frame_style",
            Self::SetWrap { .. } => "set_wrap",
            Self::SetFrameName { .. } => "set_frame_name",
            Self::SetPlaceholder { .. } => "set_placeholder",
            Self::SetTextFrame { .. } => "set_text_frame",
            Self::SetShape { .. } => "set_shape",
            Self::SetStory { .. } => "set_story",
            Self::InsertText { .. } => "insert_text",
            Self::DeleteText { .. } => "delete_text",
            Self::ReplaceText { .. } => "replace_text",
            Self::ApplyCharProps { .. } => "apply_char_props",
            Self::SetCharStyle { .. } => "set_char_style",
            Self::ClearCharFormat { .. } => "clear_char_format",
            Self::ApplyParaProps { .. } => "apply_para_props",
            Self::SetParaStyle { .. } => "set_para_style",
            Self::LinkFrames { .. } => "link_frames",
            Self::UnlinkFrame { .. } => "unlink_frame",
            Self::AddStyle { .. } => "add_style",
            Self::UpdateStyle { .. } => "update_style",
            Self::DeleteStyle { .. } => "delete_style",
            Self::AddAsset { .. } => "add_asset",
            Self::RemoveAsset { .. } => "remove_asset",
            Self::SetImage { .. } => "set_image",
            Self::SetImageCrop { .. } => "set_image_crop",
            Self::SetImageFit { .. } => "set_image_fit",
            Self::SetLockAspect { .. } => "set_lock_aspect",
            Self::Batch { .. } => "batch",
            Self::Patch(_) => "patch",
        }
    }

    /// A short human-readable label for history lists.
    pub fn label(&self) -> String {
        match self {
            Self::Batch { label, .. } => label.clone(),
            other => other.name().replace('_', " "),
        }
    }

    /// Apply to `doc`. On error the document may be partially modified; use
    /// [`crate::Editor`] (which snapshots) for atomic application.
    pub fn apply(&self, doc: &mut Document) -> CmdResult {
        match self {
            Self::SetMeta { title, author } => {
                if let Some(t) = title {
                    doc.meta.title.clone_from(t);
                }
                if let Some(a) = author {
                    doc.meta.author.clone_from(a);
                }
                Ok(())
            }
            Self::SetPageSize { size } => {
                check_dim(size.width, "width")?;
                check_dim(size.height, "height")?;
                if size.width < 1.0 || size.height < 1.0 {
                    return Err(CommandError::invalid("page size must be at least 1pt"));
                }
                doc.setup.size = *size;
                Ok(())
            }
            Self::SetMargins { margins } => {
                if !margins.is_valid() {
                    return Err(CommandError::invalid("margins must be finite and >= 0"));
                }
                let s = doc.setup.size;
                if margins.left + margins.right >= s.width
                    || margins.top + margins.bottom >= s.height
                {
                    return Err(CommandError::invalid("margins leave no room on the page"));
                }
                doc.setup.margins = *margins;
                Ok(())
            }
            Self::SetBleed { bleed } => {
                if !(bleed.is_finite() && (0.0..=144.0).contains(bleed)) {
                    return Err(CommandError::invalid("bleed must be between 0 and 144pt"));
                }
                doc.setup.bleed = *bleed;
                Ok(())
            }
            Self::SetGrid { grid } => {
                if !(grid.spacing.is_finite() && grid.spacing >= 1.0)
                    || !(grid.snap_tolerance.is_finite() && grid.snap_tolerance >= 0.0)
                {
                    return Err(CommandError::invalid(
                        "grid spacing must be >= 1pt, tolerance >= 0",
                    ));
                }
                doc.grid = *grid;
                Ok(())
            }
            Self::AddPage { page, index } => {
                if doc.page(page.id).is_some() {
                    return Err(CommandError::conflict(format!(
                        "page {} already exists",
                        page.id
                    )));
                }
                if !page.frames.is_empty() {
                    return Err(CommandError::invalid(
                        "new pages must be empty; add frames with add_frame",
                    ));
                }
                if let Some(m) = page.master {
                    require_master(doc, m)?;
                }
                let at = index.unwrap_or(doc.pages.len()).min(doc.pages.len());
                doc.pages_mut().insert(at, page.clone());
                Ok(())
            }
            Self::DeletePage { page } => {
                let idx = doc
                    .page_index(*page)
                    .ok_or_else(|| CommandError::not_found("page", page))?;
                if doc.pages.len() == 1 {
                    return Err(CommandError::invalid("cannot delete the only page"));
                }
                let frames = doc.all_frames_of(crate::model::Owner::Page(*page));
                remove_frames(doc, &frames);
                doc.pages_mut().remove(idx);
                Ok(())
            }
            Self::MovePage { page, index } => {
                let idx = doc
                    .page_index(*page)
                    .ok_or_else(|| CommandError::not_found("page", page))?;
                let p = doc.pages_mut().remove(idx);
                let at = (*index).min(doc.pages.len());
                doc.pages_mut().insert(at, p);
                Ok(())
            }
            Self::AddMaster { master } => {
                if doc.master(master.id).is_some() {
                    return Err(CommandError::conflict("master already exists"));
                }
                if !master.frames.is_empty() {
                    return Err(CommandError::invalid("new masters must be empty"));
                }
                doc.masters_mut().push(master.clone());
                Ok(())
            }
            Self::DeleteMaster { master } => {
                require_master(doc, *master)?;
                let frames = doc.all_frames_of(crate::model::Owner::Master(*master));
                remove_frames(doc, &frames);
                doc.masters_mut().retain(|m| m.id != *master);
                for p in doc.pages_mut() {
                    if p.master == Some(*master) {
                        p.master = None;
                    }
                }
                Ok(())
            }
            Self::RenameMaster { master, name } => {
                let m = doc
                    .master_mut(*master)
                    .ok_or_else(|| CommandError::not_found("master", master))?;
                m.name.clone_from(name);
                Ok(())
            }
            Self::SetPageMaster { page, master } => {
                if let Some(m) = master {
                    require_master(doc, *m)?;
                }
                let p = doc
                    .page_mut(*page)
                    .ok_or_else(|| CommandError::not_found("page", page))?;
                p.master = *master;
                Ok(())
            }
            Self::AddGuide { owner, guide } => {
                check_coord(guide.position, "position")?;
                let guides = doc
                    .guides_mut(*owner)
                    .ok_or_else(|| CommandError::not_found("owner", format!("{owner:?}")))?;
                if guides.iter().any(|g| g.id == guide.id) {
                    return Err(CommandError::conflict("guide already exists"));
                }
                guides.push(*guide);
                Ok(())
            }
            Self::RemoveGuide { owner, guide } => {
                let guides = doc
                    .guides_mut(*owner)
                    .ok_or_else(|| CommandError::not_found("owner", format!("{owner:?}")))?;
                let before = guides.len();
                guides.retain(|g| g.id != *guide);
                if guides.len() == before {
                    return Err(CommandError::not_found("guide", guide));
                }
                Ok(())
            }
            Self::MoveGuide {
                owner,
                guide,
                position,
            } => {
                check_coord(*position, "position")?;
                let guides = doc
                    .guides_mut(*owner)
                    .ok_or_else(|| CommandError::not_found("owner", format!("{owner:?}")))?;
                let g = guides
                    .iter_mut()
                    .find(|g| g.id == *guide)
                    .ok_or_else(|| CommandError::not_found("guide", guide))?;
                g.position = *position;
                Ok(())
            }
            Self::AddFrame {
                parent,
                index,
                frame,
                story,
            } => add_frame(doc, *parent, *index, frame, story.as_ref()),
            Self::DeleteFrame { frame } => {
                let f = require_frame(doc, *frame)?;
                if f.locked {
                    return Err(CommandError::Locked { frame: *frame });
                }
                let parent = doc
                    .parent_of(*frame)
                    .ok_or_else(|| CommandError::invalid("frame has no parent"))?;
                let all = doc.descendants(*frame);
                remove_frames(doc, &all);
                if let Parent::Group(g) = parent {
                    cleanup_group(doc, g);
                }
                Ok(())
            }
            Self::SetFrameRect { frame, rect } => {
                check_rect(rect)?;
                let f = require_unlocked(doc, *frame)?;
                let (old, rotation, is_group) = (
                    f.rect,
                    f.rotation,
                    f.kind() == crate::model::FrameKind::Group,
                );
                if is_group {
                    transform_group(doc, *frame, old, *rect, rotation);
                }
                doc.frame_mut(*frame).expect("checked").rect = *rect;
                refresh_ancestors(doc, *frame);
                Ok(())
            }
            Self::SetRotation { frame, degrees } => {
                if !degrees.is_finite() {
                    return Err(CommandError::invalid("rotation must be finite"));
                }
                let deg = degrees.rem_euclid(360.0);
                let f = require_unlocked(doc, *frame)?;
                let (old, center, is_group) = (
                    f.rotation,
                    f.rect.center(),
                    f.kind() == crate::model::FrameKind::Group,
                );
                if is_group {
                    let delta = deg - old;
                    for d in doc.descendants(*frame).into_iter().skip(1) {
                        if let Some(c) = doc.frame_mut(d) {
                            let nc = c.rect.center().rotated_about(center, delta);
                            let oc = c.rect.center();
                            c.rect = c.rect.translate(nc.x - oc.x, nc.y - oc.y);
                            c.rotation = (c.rotation + delta).rem_euclid(360.0);
                        }
                    }
                }
                doc.frame_mut(*frame).expect("checked").rotation = deg;
                refresh_ancestors(doc, *frame);
                Ok(())
            }
            Self::SetZIndex { frame, index } => {
                require_frame(doc, *frame)?;
                let parent = doc
                    .parent_of(*frame)
                    .ok_or_else(|| CommandError::invalid("frame has no parent"))?;
                let list = doc.children_mut(parent).expect("parent exists");
                list.retain(|f| f != frame);
                let at = (*index).min(list.len());
                list.insert(at, *frame);
                Ok(())
            }
            Self::Group { group, frames } => group_frames(doc, *group, frames),
            Self::Ungroup { group } => {
                let g = require_frame(doc, *group)?;
                let FrameContent::Group(gf) = &g.content else {
                    return Err(CommandError::invalid("frame is not a group"));
                };
                let children = gf.children.clone();
                let parent = doc
                    .parent_of(*group)
                    .ok_or_else(|| CommandError::invalid("group has no parent"))?;
                let list = doc.children_mut(parent).expect("parent exists");
                let pos = list.iter().position(|f| f == group).expect("listed");
                list.splice(pos..=pos, children);
                doc.frames.remove(group);
                refresh_group_chain(doc, parent);
                Ok(())
            }
            Self::SetLocked { frame, locked } => {
                frame_mut(doc, *frame)?.locked = *locked;
                Ok(())
            }
            Self::SetFrameStyle { frame, style } => {
                if !(style.opacity.is_finite() && (0.0..=1.0).contains(&style.opacity)) {
                    return Err(CommandError::invalid("opacity must be between 0 and 1"));
                }
                if let Some(s) = &style.stroke
                    && !(s.width.is_finite() && (0.0..=288.0).contains(&s.width))
                {
                    return Err(CommandError::invalid("stroke width must be 0..=288pt"));
                }
                frame_mut(doc, *frame)?.style = *style;
                Ok(())
            }
            Self::SetWrap { frame, wrap } => {
                if !(wrap.margin.is_finite() && wrap.margin >= 0.0) {
                    return Err(CommandError::invalid("wrap margin must be >= 0"));
                }
                frame_mut(doc, *frame)?.wrap = *wrap;
                Ok(())
            }
            Self::SetFrameName { frame, name } => {
                frame_mut(doc, *frame)?.name.clone_from(name);
                Ok(())
            }
            Self::SetPlaceholder { frame, placeholder } => {
                let f = require_frame(doc, *frame)?;
                if let Some(p) = placeholder {
                    check_placeholder(doc, f, p)?;
                }
                frame_mut(doc, *frame)?.placeholder.clone_from(placeholder);
                Ok(())
            }
            Self::SetTextFrame { frame, props } => {
                if let Some(m) = &props.insets
                    && !m.is_valid()
                {
                    return Err(CommandError::invalid("insets must be finite and >= 0"));
                }
                if let Some(c) = props.columns
                    && !(1..=12).contains(&c)
                {
                    return Err(CommandError::invalid("columns must be 1..=12"));
                }
                if let Some(g) = props.column_gap
                    && !(g.is_finite() && g >= 0.0)
                {
                    return Err(CommandError::invalid("column gap must be >= 0"));
                }
                let t = frame_mut(doc, *frame)?
                    .as_text_mut()
                    .ok_or_else(|| CommandError::invalid("frame is not a text frame"))?;
                if let Some(v) = props.insets {
                    t.insets = v;
                }
                if let Some(v) = props.columns {
                    t.columns = v;
                }
                if let Some(v) = props.column_gap {
                    t.column_gap = v;
                }
                if let Some(v) = props.vertical_align {
                    t.vertical_align = v;
                }
                Ok(())
            }
            Self::SetShape { frame, shape } => {
                if let Shape::Rectangle { corner_radius } = shape
                    && !(corner_radius.is_finite() && *corner_radius >= 0.0)
                {
                    return Err(CommandError::invalid("corner radius must be >= 0"));
                }
                match &mut frame_mut(doc, *frame)?.content {
                    FrameContent::Shape(s) => {
                        s.shape = *shape;
                        Ok(())
                    }
                    _ => Err(CommandError::invalid("frame is not a shape")),
                }
            }
            Self::SetStory { story, paragraphs } => {
                for p in paragraphs {
                    check_paragraph(doc, p)?;
                }
                let s = story_mut(doc, *story)?;
                s.paragraphs.clone_from(paragraphs);
                if s.paragraphs.is_empty() {
                    s.paragraphs.push(Paragraph::default());
                }
                for p in &mut s.paragraphs {
                    normalize_paragraph(p);
                }
                Ok(())
            }
            Self::InsertText {
                story,
                at,
                text,
                format,
            } => {
                if let Some(f) = format {
                    check_char_style(doc, f.style)?;
                    check_char_props(&f.props)?;
                }
                let s = story_mut(doc, *story)?;
                check_offset(s, *at)?;
                s.insert_text(*at, text, format.clone());
                Ok(())
            }
            Self::DeleteText { story, start, end } => {
                let s = story_mut(doc, *story)?;
                check_range(s, *start, *end)?;
                s.delete_range(*start, *end);
                Ok(())
            }
            Self::ReplaceText {
                story,
                start,
                end,
                text,
            } => {
                let s = story_mut(doc, *story)?;
                check_range(s, *start, *end)?;
                s.replace_range(*start, *end, text);
                Ok(())
            }
            Self::ApplyCharProps {
                story,
                start,
                end,
                props,
            } => {
                check_char_props(props)?;
                let s = story_mut(doc, *story)?;
                check_range(s, *start, *end)?;
                s.apply_char_props(*start, *end, props);
                Ok(())
            }
            Self::SetCharStyle {
                story,
                start,
                end,
                style,
            } => {
                check_char_style(doc, *style)?;
                let s = story_mut(doc, *story)?;
                check_range(s, *start, *end)?;
                s.update_runs(*start, *end, |r| r.style = *style);
                Ok(())
            }
            Self::ClearCharFormat { story, start, end } => {
                let s = story_mut(doc, *story)?;
                check_range(s, *start, *end)?;
                s.update_runs(*start, *end, |r| {
                    r.style = None;
                    r.props = CharProps::default();
                });
                Ok(())
            }
            Self::ApplyParaProps {
                story,
                start,
                end,
                props,
            } => {
                if !props.is_valid() {
                    return Err(CommandError::invalid("invalid paragraph properties"));
                }
                let s = story_mut(doc, *story)?;
                check_range(s, *start, *end)?;
                s.apply_para_props(*start, *end, props);
                Ok(())
            }
            Self::SetParaStyle {
                story,
                start,
                end,
                style,
            } => {
                check_para_style(doc, *style)?;
                let s = story_mut(doc, *story)?;
                check_range(s, *start, *end)?;
                s.update_paragraphs(*start, *end, |p| p.style = *style);
                Ok(())
            }
            Self::LinkFrames { from, to } => link_frames(doc, *from, *to),
            Self::UnlinkFrame { frame, new_story } => unlink_frame(doc, *frame, *new_story),
            Self::AddStyle { style } => {
                if doc.styles.get(style.id).is_some() {
                    return Err(CommandError::conflict("style id already exists"));
                }
                check_style(doc, style)?;
                doc.styles.styles.push(style.clone());
                Ok(())
            }
            Self::UpdateStyle { style } => {
                let old = doc
                    .styles
                    .get(style.id)
                    .ok_or_else(|| CommandError::not_found("style", style.id))?;
                if old.kind != style.kind {
                    return Err(CommandError::invalid("a style's kind cannot change"));
                }
                check_style(doc, style)?;
                *doc.styles.get_mut(style.id).expect("checked") = style.clone();
                Ok(())
            }
            Self::DeleteStyle { style } => delete_style(doc, *style),
            Self::AddAsset { asset, data } => {
                if doc.assets.contains_key(&asset.id) {
                    return Err(CommandError::conflict("asset id already exists"));
                }
                if data.0.is_empty() || asset.pixel_width == 0 || asset.pixel_height == 0 {
                    return Err(CommandError::invalid(
                        "asset needs data and non-zero pixel size",
                    ));
                }
                doc.assets.insert(asset.id, asset.clone());
                doc.blobs.insert(asset.id, data.0.clone());
                Ok(())
            }
            Self::RemoveAsset { asset } => {
                if !doc.assets.contains_key(asset) {
                    return Err(CommandError::not_found("asset", asset));
                }
                let used = doc
                    .frames
                    .values()
                    .any(|f| f.as_image().is_some_and(|i| i.asset == Some(*asset)));
                if used {
                    return Err(CommandError::conflict("asset is still placed in a frame"));
                }
                doc.assets.remove(asset);
                doc.blobs.remove(asset);
                Ok(())
            }
            Self::SetImage { frame, asset } => {
                if let Some(a) = asset
                    && !doc.assets.contains_key(a)
                {
                    return Err(CommandError::not_found("asset", a));
                }
                let img = image_mut(doc, *frame)?;
                img.asset = *asset;
                img.crop = Crop::default();
                Ok(())
            }
            Self::SetImageCrop { frame, crop } => {
                if !crop.is_valid() {
                    return Err(CommandError::invalid(
                        "crop fractions must be in 0..1 and leave part of the image",
                    ));
                }
                image_mut(doc, *frame)?.crop = *crop;
                Ok(())
            }
            Self::SetImageFit { frame, fit } => {
                image_mut(doc, *frame)?.fit = *fit;
                Ok(())
            }
            Self::SetLockAspect { frame, lock } => {
                image_mut(doc, *frame)?.lock_aspect = *lock;
                Ok(())
            }
            Self::Batch { commands, .. } => {
                for c in commands {
                    c.apply(doc)?;
                }
                Ok(())
            }
            Self::Patch(p) => {
                p.apply(doc);
                Ok(())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// helpers

fn check_dim(v: f64, what: &str) -> CmdResult {
    if v.is_finite() && (0.0..=MAX_DIMENSION).contains(&v) {
        Ok(())
    } else {
        Err(CommandError::invalid(format!(
            "{what} must be between 0 and {MAX_DIMENSION}pt"
        )))
    }
}

fn check_coord(v: f64, what: &str) -> CmdResult {
    if v.is_finite() && v.abs() <= MAX_DIMENSION * 2.0 {
        Ok(())
    } else {
        Err(CommandError::invalid(format!("{what} out of range")))
    }
}

fn check_rect(r: &Rect) -> CmdResult {
    check_coord(r.x, "x")?;
    check_coord(r.y, "y")?;
    check_dim(r.width, "width")?;
    check_dim(r.height, "height")
}

fn require_master(doc: &Document, id: MasterId) -> CmdResult<&MasterPage> {
    doc.master(id)
        .ok_or_else(|| CommandError::not_found("master", id))
}

fn require_frame(doc: &Document, id: FrameId) -> CmdResult<&Frame> {
    doc.frame(id)
        .ok_or_else(|| CommandError::not_found("frame", id))
}

fn require_unlocked(doc: &Document, id: FrameId) -> CmdResult<&Frame> {
    let f = require_frame(doc, id)?;
    if f.locked {
        Err(CommandError::Locked { frame: id })
    } else {
        Ok(f)
    }
}

fn frame_mut(doc: &mut Document, id: FrameId) -> CmdResult<&mut Frame> {
    doc.frame_mut(id)
        .ok_or_else(|| CommandError::not_found("frame", id))
}

fn story_mut(doc: &mut Document, id: StoryId) -> CmdResult<&mut Story> {
    doc.story_mut(id)
        .ok_or_else(|| CommandError::not_found("story", id))
}

fn image_mut(doc: &mut Document, id: FrameId) -> CmdResult<&mut crate::model::ImageFrame> {
    frame_mut(doc, id)?
        .as_image_mut()
        .ok_or_else(|| CommandError::invalid("frame is not an image frame"))
}

fn check_offset(s: &Story, at: usize) -> CmdResult {
    let len = s.char_len();
    if at > len {
        return Err(CommandError::invalid(format!(
            "offset {at} is past the end of the story (length {len})"
        )));
    }
    Ok(())
}

fn check_range(s: &Story, start: usize, end: usize) -> CmdResult {
    if start > end {
        return Err(CommandError::invalid("start must be <= end"));
    }
    check_offset(s, end)
}

fn check_char_props(p: &CharProps) -> CmdResult {
    if p.is_valid() {
        Ok(())
    } else {
        Err(CommandError::invalid(
            "invalid character properties (font size must be 0..1296pt, family non-empty)",
        ))
    }
}

fn check_style_kind(doc: &Document, id: Option<StyleId>, kind: StyleKind) -> CmdResult {
    let Some(id) = id else { return Ok(()) };
    match doc.styles.get(id) {
        Some(s) if s.kind == kind => Ok(()),
        Some(_) => Err(CommandError::invalid(format!(
            "style {id} is not a {kind:?} style"
        ))),
        None => Err(CommandError::not_found("style", id)),
    }
}

fn check_char_style(doc: &Document, id: Option<StyleId>) -> CmdResult {
    check_style_kind(doc, id, StyleKind::Character)
}

fn check_para_style(doc: &Document, id: Option<StyleId>) -> CmdResult {
    check_style_kind(doc, id, StyleKind::Paragraph)
}

fn check_paragraph(doc: &Document, p: &Paragraph) -> CmdResult {
    check_para_style(doc, p.style)?;
    if !p.props.is_valid() {
        return Err(CommandError::invalid("invalid paragraph properties"));
    }
    for r in &p.runs {
        check_char_style(doc, r.style)?;
        check_char_props(&r.props)?;
    }
    Ok(())
}

fn check_style(doc: &Document, style: &Style) -> CmdResult {
    if style.name.trim().is_empty() {
        return Err(CommandError::invalid("style name must not be empty"));
    }
    if doc
        .styles
        .styles
        .iter()
        .any(|s| s.id != style.id && s.name.eq_ignore_ascii_case(&style.name))
    {
        return Err(CommandError::conflict(format!(
            "a style named {:?} already exists",
            style.name
        )));
    }
    if let Some(b) = style.based_on {
        check_style_kind(doc, Some(b), style.kind)?;
        if b == style.id || doc.styles.would_cycle(style.id, b) {
            return Err(CommandError::invalid("based_on would create a cycle"));
        }
    }
    check_char_props(&style.character)?;
    if !style.paragraph.is_valid() {
        return Err(CommandError::invalid("invalid paragraph properties"));
    }
    Ok(())
}

fn check_placeholder(doc: &Document, f: &Frame, p: &Placeholder) -> CmdResult {
    if p.name.trim().is_empty() {
        return Err(CommandError::invalid("placeholder name must not be empty"));
    }
    if let Some(other) = doc.placeholder(&p.name)
        && other != f.id
    {
        return Err(CommandError::conflict(format!(
            "placeholder {:?} already exists",
            p.name
        )));
    }
    let ok = matches!(
        (p.kind, &f.content),
        (PlaceholderKind::Text, FrameContent::Text(_))
            | (PlaceholderKind::Image, FrameContent::Image(_))
    );
    if !ok {
        return Err(CommandError::invalid(
            "placeholder kind must match the frame type",
        ));
    }
    Ok(())
}

fn add_frame(
    doc: &mut Document,
    parent: Parent,
    index: Option<usize>,
    frame: &Frame,
    story: Option<&Story>,
) -> CmdResult {
    if doc.frames.contains_key(&frame.id) {
        return Err(CommandError::conflict(format!(
            "frame {} already exists",
            frame.id
        )));
    }
    if doc.children(parent).is_none() {
        return Err(CommandError::not_found("parent", format!("{parent:?}")));
    }
    check_rect(&frame.rect)?;
    if !frame.rotation.is_finite() {
        return Err(CommandError::invalid("rotation must be finite"));
    }
    match &frame.content {
        FrameContent::Group(_) => {
            return Err(CommandError::invalid(
                "create groups with the group command",
            ));
        }
        FrameContent::Text(t) => {
            let Some(s) = story else {
                return Err(CommandError::invalid("a text frame needs a new story"));
            };
            if s.id != t.story || s.frames != [frame.id] {
                return Err(CommandError::invalid(
                    "the story must match the frame and list only it",
                ));
            }
            if doc.stories.contains_key(&s.id) {
                return Err(CommandError::conflict("story already exists"));
            }
            for p in &s.paragraphs {
                check_paragraph(doc, p)?;
            }
        }
        FrameContent::Image(i) => {
            if let Some(a) = i.asset
                && !doc.assets.contains_key(&a)
            {
                return Err(CommandError::not_found("asset", a));
            }
            if !i.crop.is_valid() {
                return Err(CommandError::invalid("invalid crop"));
            }
        }
        FrameContent::Shape(_) => {}
    }
    if story.is_some() && !matches!(frame.content, FrameContent::Text(_)) {
        return Err(CommandError::invalid("only text frames take a story"));
    }
    if let Some(p) = &frame.placeholder {
        check_placeholder(doc, frame, p)?;
    }
    doc.frames.insert(frame.id, Arc::new(frame.clone()));
    if let Some(s) = story {
        let mut s = s.clone();
        if s.paragraphs.is_empty() {
            s.paragraphs.push(Paragraph::default());
        }
        for p in &mut s.paragraphs {
            normalize_paragraph(p);
        }
        doc.stories.insert(s.id, Arc::new(s));
    }
    let list = doc.children_mut(parent).expect("checked");
    let at = index.unwrap_or(list.len()).min(list.len());
    list.insert(at, frame.id);
    if let Parent::Group(g) = parent {
        refresh_group_chain(doc, Parent::Group(g));
    }
    Ok(())
}

/// Remove frames (and nothing else) from the map, their parents' lists, and
/// story chains. Stories left with no frames are removed.
fn remove_frames(doc: &mut Document, frames: &[FrameId]) {
    for id in frames {
        if let Some(parent) = doc.parent_of(*id)
            && let Some(list) = doc.children_mut(parent)
        {
            list.retain(|f| f != id);
        }
        if let Some(f) = doc.frames.remove(id)
            && let FrameContent::Text(t) = &f.content
        {
            let empty = match doc.story_mut(t.story) {
                Some(s) => {
                    s.frames.retain(|f| f != id);
                    s.frames.is_empty()
                }
                None => false,
            };
            if empty {
                doc.stories.remove(&t.story);
            }
        }
    }
}

/// After a child was removed from group `g`: delete the group if empty,
/// otherwise refresh its bounds.
fn cleanup_group(doc: &mut Document, g: FrameId) {
    let empty = doc.frame(g).is_some_and(|f| f.children().is_empty());
    if empty {
        let parent = doc.parent_of(g);
        remove_frames(doc, &[g]);
        if let Some(Parent::Group(pg)) = parent {
            cleanup_group(doc, pg);
        }
    } else {
        refresh_group_chain(doc, Parent::Group(g));
    }
}

/// Recompute the bounds of every group containing `frame`.
fn refresh_ancestors(doc: &mut Document, frame: FrameId) {
    if let Some(p) = doc.parent_of(frame) {
        refresh_group_chain(doc, p);
    }
}

/// Recompute bounds of `parent` (if a group) and its group ancestors.
fn refresh_group_chain(doc: &mut Document, parent: Parent) {
    let mut current = parent;
    for _ in 0..64 {
        let Parent::Group(g) = current else { return };
        let bounds = doc
            .frame(g)
            .map(|f| f.children().to_vec())
            .unwrap_or_default()
            .iter()
            .filter_map(|c| doc.frame(*c).map(Frame::bounds))
            .reduce(|a, b| a.union(&b));
        if let (Some(b), Some(f)) = (bounds, doc.frame_mut(g)) {
            f.rect = b;
            f.rotation = 0.0;
        }
        match doc.parent_of(g) {
            Some(p) => current = p,
            None => return,
        }
    }
}

/// Map every descendant of a group from its old box to its new box.
fn transform_group(doc: &mut Document, group: FrameId, old: Rect, new: Rect, rotation: f64) {
    let sx = if old.width > 0.0 {
        new.width / old.width
    } else {
        1.0
    };
    let sy = if old.height > 0.0 {
        new.height / old.height
    } else {
        1.0
    };
    for d in doc.descendants(group).into_iter().skip(1) {
        let Some(f) = doc.frame_mut(d) else {
            continue;
        };
        let local = f.rect.center().rotated_about(old.center(), -rotation);
        let mapped = Point::new(
            new.x + (local.x - old.x) * sx,
            new.y + (local.y - old.y) * sy,
        )
        .rotated_about(new.center(), rotation);
        let relative = (f.rotation - rotation).rem_euclid(180.0);
        let swapped = (relative - 90.0).abs() < 45.0;
        let (fx, fy) = if swapped { (sy, sx) } else { (sx, sy) };
        let (w, h) = (f.rect.width * fx, f.rect.height * fy);
        f.rect = Rect::new(mapped.x - w / 2.0, mapped.y - h / 2.0, w, h);
    }
}

fn group_frames(doc: &mut Document, group: FrameId, frames: &[FrameId]) -> CmdResult {
    if doc.frames.contains_key(&group) {
        return Err(CommandError::conflict("group id already exists"));
    }
    let mut unique: Vec<FrameId> = Vec::new();
    for f in frames {
        require_frame(doc, *f)?;
        if !unique.contains(f) {
            unique.push(*f);
        }
    }
    if unique.len() < 2 {
        return Err(CommandError::invalid("group needs at least two frames"));
    }
    let parent = doc
        .parent_of(unique[0])
        .ok_or_else(|| CommandError::invalid("frame has no parent"))?;
    if unique.iter().any(|f| doc.parent_of(*f) != Some(parent)) {
        return Err(CommandError::invalid(
            "all frames must be on the same page and level",
        ));
    }
    let list = doc.children(parent).expect("exists").clone();
    let members: Vec<FrameId> = list
        .iter()
        .copied()
        .filter(|f| unique.contains(f))
        .collect();
    let top = list
        .iter()
        .rposition(|f| unique.contains(f))
        .expect("members listed");
    let insert_at = top + 1 - members.len();
    let bounds = members
        .iter()
        .filter_map(|m| doc.frame(*m).map(Frame::bounds))
        .reduce(|a, b| a.union(&b))
        .expect("non-empty");
    let mut g = Frame::new(
        group,
        bounds,
        FrameContent::Group(crate::model::GroupFrame {
            children: members.clone(),
        }),
    );
    g.wrap = TextWrap::default();
    let list = doc.children_mut(parent).expect("exists");
    list.retain(|f| !members.contains(f));
    list.insert(insert_at, group);
    doc.frames.insert(group, Arc::new(g));
    refresh_group_chain(doc, parent);
    Ok(())
}

fn link_frames(doc: &mut Document, from: FrameId, to: FrameId) -> CmdResult {
    if from == to {
        return Err(CommandError::invalid("cannot link a frame to itself"));
    }
    let from_story = require_frame(doc, from)?
        .as_text()
        .ok_or_else(|| CommandError::invalid("source is not a text frame"))?
        .story;
    let to_story = require_frame(doc, to)?
        .as_text()
        .ok_or_else(|| CommandError::invalid("target is not a text frame"))?
        .story;
    if from_story == to_story {
        return Err(CommandError::invalid(
            "frames are already in the same chain",
        ));
    }
    let target = doc
        .stories
        .get(&to_story)
        .ok_or_else(|| CommandError::not_found("story", to_story))?
        .clone();
    if target.frames.len() > 1 {
        return Err(CommandError::invalid(
            "target frame is already linked; unlink it first",
        ));
    }
    let src = story_mut(doc, from_story)?;
    if !target.is_empty() {
        if src.is_empty() {
            src.paragraphs = target.paragraphs.clone();
        } else {
            src.paragraphs.extend(target.paragraphs.iter().cloned());
        }
    }
    let pos = src
        .frames
        .iter()
        .position(|f| *f == from)
        .ok_or_else(|| CommandError::invalid("source frame missing from its story chain"))?;
    src.frames.insert(pos + 1, to);
    doc.stories.remove(&to_story);
    doc.frame_mut(to)
        .and_then(Frame::as_text_mut)
        .expect("checked")
        .story = from_story;
    Ok(())
}

fn unlink_frame(doc: &mut Document, frame: FrameId, new_story: StoryId) -> CmdResult {
    let story = require_frame(doc, frame)?
        .as_text()
        .ok_or_else(|| CommandError::invalid("frame is not a text frame"))?
        .story;
    if doc.stories.contains_key(&new_story) {
        return Err(CommandError::conflict("new story id already exists"));
    }
    let s = story_mut(doc, story)?;
    let pos = s
        .frames
        .iter()
        .position(|f| *f == frame)
        .ok_or_else(|| CommandError::invalid("frame missing from its story chain"))?;
    if pos + 1 >= s.frames.len() {
        return Err(CommandError::invalid(
            "frame is the last in its chain; nothing to unlink",
        ));
    }
    let rest: Vec<FrameId> = s.frames.split_off(pos + 1);
    let template = s.paragraphs.first().cloned().unwrap_or_default();
    let mut para = Paragraph {
        style: template.style,
        props: template.props,
        runs: template.runs.into_iter().take(1).collect(),
    };
    for r in &mut para.runs {
        r.text.clear();
    }
    normalize_paragraph(&mut para);
    for f in &rest {
        if let Some(t) = doc.frame_mut(*f).and_then(Frame::as_text_mut) {
            t.story = new_story;
        }
    }
    doc.stories.insert(
        new_story,
        Arc::new(Story {
            id: new_story,
            frames: rest,
            paragraphs: vec![para],
        }),
    );
    Ok(())
}

fn delete_style(doc: &mut Document, id: StyleId) -> CmdResult {
    if id == doc.styles.default_paragraph {
        return Err(CommandError::invalid(
            "cannot delete the default paragraph style",
        ));
    }
    let parent = doc
        .styles
        .get(id)
        .ok_or_else(|| CommandError::not_found("style", id))?
        .based_on;
    doc.styles.styles.retain(|s| s.id != id);
    for s in &mut doc.styles.styles {
        if s.based_on == Some(id) {
            s.based_on = parent;
        }
    }
    let uses = |s: &Story| {
        s.paragraphs
            .iter()
            .any(|p| p.style == Some(id) || p.runs.iter().any(|r| r.style == Some(id)))
    };
    for story in doc.stories.values_mut().filter(|s| uses(s)) {
        for p in &mut Arc::make_mut(story).paragraphs {
            if p.style == Some(id) {
                p.style = None;
            }
            for r in &mut p.runs {
                if r.style == Some(id) {
                    r.style = None;
                }
            }
            normalize_paragraph(p);
        }
    }
    Ok(())
}

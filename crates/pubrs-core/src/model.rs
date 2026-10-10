//! The document model.
//!
//! A [`Document`] is plain data: pages and master pages hold ordered lists of
//! frame ids (back to front), frames live in one id-keyed map, and the text of
//! linked text frames lives in [`Story`] objects shared by the chain.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::geom::{Margins, PT_PER_INCH, PT_PER_MM, Rect, Size};
use crate::id::{
    AssetId, DocumentId, FrameId, GuideId, IdSource, MasterId, PageId, StoryId, StyleId,
};
use crate::style::{CharProps, ParaProps, StyleSheet};

/// Current document schema version. See `migrate.rs` for the upgrade path.
pub const SCHEMA_VERSION: u32 = 1;

/// Shared, immutable asset bytes.
pub type Blob = Arc<[u8]>;

/// A complete publication.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Document {
    /// Version of this JSON schema. Older files are migrated on load.
    pub schema_version: u32,
    pub id: DocumentId,
    pub meta: DocMeta,
    /// Page size, margins and bleed shared by every page.
    pub setup: PageSetup,
    pub grid: GridSettings,
    /// Pages in reading order.
    pub pages: Vec<Page>,
    #[serde(default)]
    pub masters: Vec<MasterPage>,
    /// Every frame in the document, keyed by id. Pages, masters and groups
    /// reference frames by id.
    #[serde(default)]
    pub frames: BTreeMap<FrameId, Frame>,
    /// Text content. Each text frame points at a story; linked frames share one.
    #[serde(default)]
    pub stories: BTreeMap<StoryId, Story>,
    pub styles: StyleSheet,
    /// Metadata for embedded images. The bytes live in `blobs` (in memory) or
    /// `assets/` (in the file).
    #[serde(default)]
    pub assets: BTreeMap<AssetId, Asset>,
    /// Asset bytes. Not part of `document.json`.
    #[serde(skip)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub blobs: BTreeMap<AssetId, Blob>,
}

/// Descriptive metadata.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DocMeta {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub author: String,
    /// Name of the template this document was created from, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
}

/// Page geometry shared by all pages.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PageSetup {
    /// Trim size of each page in points.
    pub size: Size,
    /// Margin guides in points (layout aids; nothing is clipped to them).
    pub margins: Margins,
    /// Bleed in points added outside the trim on every side when exporting.
    pub bleed: f64,
}

impl Default for PageSetup {
    fn default() -> Self {
        Self {
            size: PageSizePreset::Letter.size(),
            margins: Margins::uniform(0.5 * PT_PER_INCH),
            bleed: 0.0,
        }
    }
}

/// Named page sizes (portrait).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PageSizePreset {
    /// 8.5 x 11 in.
    Letter,
    /// 8.5 x 14 in.
    Legal,
    /// 11 x 17 in.
    Tabloid,
    /// 297 x 420 mm.
    A3,
    /// 210 x 297 mm.
    A4,
    /// 148 x 210 mm.
    A5,
    /// 105 x 148 mm.
    A6,
    /// 3.5 x 2 in (landscape business card).
    BusinessCard,
    /// 4 x 6 in.
    Postcard,
}

impl PageSizePreset {
    pub const ALL: [PageSizePreset; 9] = [
        Self::Letter,
        Self::Legal,
        Self::Tabloid,
        Self::A3,
        Self::A4,
        Self::A5,
        Self::A6,
        Self::BusinessCard,
        Self::Postcard,
    ];

    pub fn size(self) -> Size {
        let inch = |w: f64, h: f64| Size::new(w * PT_PER_INCH, h * PT_PER_INCH);
        let mm = |w: f64, h: f64| Size::new(w * PT_PER_MM, h * PT_PER_MM);
        match self {
            Self::Letter => inch(8.5, 11.0),
            Self::Legal => inch(8.5, 14.0),
            Self::Tabloid => inch(11.0, 17.0),
            Self::A3 => mm(297.0, 420.0),
            Self::A4 => mm(210.0, 297.0),
            Self::A5 => mm(148.0, 210.0),
            Self::A6 => mm(105.0, 148.0),
            Self::BusinessCard => inch(3.5, 2.0),
            Self::Postcard => inch(4.0, 6.0),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Letter => "letter",
            Self::Legal => "legal",
            Self::Tabloid => "tabloid",
            Self::A3 => "a3",
            Self::A4 => "a4",
            Self::A5 => "a5",
            Self::A6 => "a6",
            Self::BusinessCard => "business_card",
            Self::Postcard => "postcard",
        }
    }
}

/// Grid and snapping settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GridSettings {
    /// Grid spacing in points.
    pub spacing: f64,
    pub show_grid: bool,
    pub snap_to_grid: bool,
    pub snap_to_guides: bool,
    /// Snap distance in points.
    pub snap_tolerance: f64,
}

impl Default for GridSettings {
    fn default() -> Self {
        Self {
            spacing: 18.0,
            show_grid: false,
            snap_to_grid: false,
            snap_to_guides: true,
            snap_tolerance: 6.0,
        }
    }
}

/// A page. Frames are listed back to front.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Page {
    pub id: PageId,
    /// Master page drawn beneath this page's frames.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub master: Option<MasterId>,
    /// Top-level frames, back to front.
    #[serde(default)]
    pub frames: Vec<FrameId>,
    #[serde(default)]
    pub guides: Vec<Guide>,
}

impl Page {
    pub fn new(id: PageId) -> Self {
        Self {
            id,
            master: None,
            frames: Vec::new(),
            guides: Vec::new(),
        }
    }
}

/// A master page: frames and guides repeated beneath every page that uses it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MasterPage {
    pub id: MasterId,
    pub name: String,
    #[serde(default)]
    pub frames: Vec<FrameId>,
    #[serde(default)]
    pub guides: Vec<Guide>,
}

/// Orientation of a ruler guide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    /// A horizontal line; `position` is its y coordinate.
    Horizontal,
    /// A vertical line; `position` is its x coordinate.
    Vertical,
}

/// A ruler guide on a page or master.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Guide {
    pub id: GuideId,
    pub axis: Axis,
    /// Points from the page's top edge (horizontal) or left edge (vertical).
    pub position: f64,
}

/// A page or master page: something that owns top-level frames and guides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Owner {
    Page(PageId),
    Master(MasterId),
}

/// Where a frame lives: directly on a page/master, or inside a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Parent {
    Page(PageId),
    Master(MasterId),
    Group(FrameId),
}

impl From<Owner> for Parent {
    fn from(o: Owner) -> Self {
        match o {
            Owner::Page(p) => Parent::Page(p),
            Owner::Master(m) => Parent::Master(m),
        }
    }
}

/// A frame: a positioned box holding text, an image, a shape, or a group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Frame {
    pub id: FrameId,
    /// Optional human-readable name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Unrotated bounds in page coordinates (points, origin top-left, y down).
    pub rect: Rect,
    /// Clockwise rotation in degrees about the centre of `rect`.
    #[serde(default)]
    pub rotation: f64,
    /// Locked frames cannot be moved, resized, rotated or deleted.
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub style: FrameStyle,
    /// How text in other frames wraps around this one.
    #[serde(default)]
    pub wrap: TextWrap,
    /// Template placeholder metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<Placeholder>,
    pub content: FrameContent,
}

impl Frame {
    pub fn new(id: FrameId, rect: Rect, content: FrameContent) -> Self {
        let wrap = match content {
            FrameContent::Image(_) => TextWrap {
                mode: WrapMode::Square,
                margin: 6.0,
            },
            _ => TextWrap::default(),
        };
        Self {
            id,
            name: None,
            rect,
            rotation: 0.0,
            locked: false,
            style: FrameStyle::default(),
            wrap,
            placeholder: None,
            content,
        }
    }

    pub fn kind(&self) -> FrameKind {
        match self.content {
            FrameContent::Text(_) => FrameKind::Text,
            FrameContent::Image(_) => FrameKind::Image,
            FrameContent::Shape(_) => FrameKind::Shape,
            FrameContent::Group(_) => FrameKind::Group,
        }
    }

    pub fn as_text(&self) -> Option<&TextFrame> {
        match &self.content {
            FrameContent::Text(t) => Some(t),
            _ => None,
        }
    }

    pub fn as_text_mut(&mut self) -> Option<&mut TextFrame> {
        match &mut self.content {
            FrameContent::Text(t) => Some(t),
            _ => None,
        }
    }

    pub fn as_image(&self) -> Option<&ImageFrame> {
        match &self.content {
            FrameContent::Image(i) => Some(i),
            _ => None,
        }
    }

    pub fn as_image_mut(&mut self) -> Option<&mut ImageFrame> {
        match &mut self.content {
            FrameContent::Image(i) => Some(i),
            _ => None,
        }
    }

    pub fn children(&self) -> &[FrameId] {
        match &self.content {
            FrameContent::Group(g) => &g.children,
            _ => &[],
        }
    }

    /// Axis-aligned bounds including rotation.
    pub fn bounds(&self) -> Rect {
        self.rect.rotated_bounds(self.rotation)
    }
}

/// The type of a frame, for filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FrameKind {
    Text,
    Image,
    Shape,
    Group,
}

/// What a frame contains.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FrameContent {
    Text(TextFrame),
    Image(ImageFrame),
    Shape(ShapeFrame),
    Group(GroupFrame),
}

/// A text box showing (part of) a story.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TextFrame {
    pub story: StoryId,
    /// Space between the frame edge and the text, in points.
    #[serde(default = "default_insets")]
    pub insets: Margins,
    /// Number of columns inside the frame (>= 1).
    #[serde(default = "one")]
    pub columns: u32,
    /// Gap between columns in points.
    #[serde(default = "default_gap")]
    pub column_gap: f64,
    #[serde(default)]
    pub vertical_align: VerticalAlign,
}

fn default_insets() -> Margins {
    Margins::uniform(4.0)
}
fn one() -> u32 {
    1
}
fn default_gap() -> f64 {
    12.0
}

impl TextFrame {
    pub fn new(story: StoryId) -> Self {
        Self {
            story,
            insets: default_insets(),
            columns: 1,
            column_gap: default_gap(),
            vertical_align: VerticalAlign::Top,
        }
    }
}

/// Vertical placement of text inside a text frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum VerticalAlign {
    #[default]
    Top,
    Middle,
    Bottom,
}

/// A picture frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ImageFrame {
    /// The placed image, or none for an empty picture placeholder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset: Option<AssetId>,
    #[serde(default)]
    pub fit: ImageFit,
    #[serde(default)]
    pub crop: Crop,
    /// Keep the frame's aspect ratio when resizing.
    #[serde(default = "yes")]
    pub lock_aspect: bool,
}

fn yes() -> bool {
    true
}

impl ImageFrame {
    pub fn new(asset: Option<AssetId>) -> Self {
        Self {
            asset,
            fit: ImageFit::Fill,
            crop: Crop::default(),
            lock_aspect: true,
        }
    }
}

/// How the (cropped) image is scaled into its frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ImageFit {
    /// Scale to cover the whole frame, keeping aspect; overflow is clipped.
    #[default]
    Fill,
    /// Scale to fit inside the frame, keeping aspect; may leave empty bands.
    Fit,
    /// Stretch to the frame exactly, ignoring aspect.
    Stretch,
}

/// Fractions (0..1) of the source image trimmed from each side before fitting.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Crop {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl Crop {
    pub fn is_valid(&self) -> bool {
        let ok = |v: f64| v.is_finite() && (0.0..1.0).contains(&v);
        ok(self.left)
            && ok(self.top)
            && ok(self.right)
            && ok(self.bottom)
            && self.left + self.right < 0.999
            && self.top + self.bottom < 0.999
    }
}

/// A vector shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ShapeFrame {
    pub shape: Shape,
}

/// Shape geometry, drawn inside the frame rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Shape {
    Rectangle {
        /// Corner radius in points.
        #[serde(default)]
        corner_radius: f64,
    },
    Ellipse,
    /// A straight line across the frame rectangle.
    Line {
        #[serde(default)]
        direction: LineDirection,
    },
}

/// Which diagonal of its frame a line follows. A zero-height frame gives a
/// horizontal line, a zero-width frame a vertical one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum LineDirection {
    /// From top-left to bottom-right.
    #[default]
    DownRight,
    /// From bottom-left to top-right.
    UpRight,
}

/// A group of frames that move together.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GroupFrame {
    /// Members, back to front.
    pub children: Vec<FrameId>,
}

/// Fill, stroke and opacity of a frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FrameStyle {
    /// Background fill; none = transparent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Color>,
    /// Outline; none = no outline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<Stroke>,
    /// Opacity of the whole frame, 0 (invisible) to 1 (opaque).
    #[serde(default = "full")]
    pub opacity: f64,
}

fn full() -> f64 {
    1.0
}

impl Default for FrameStyle {
    fn default() -> Self {
        Self {
            fill: None,
            stroke: None,
            opacity: 1.0,
        }
    }
}

/// An outline.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Stroke {
    pub color: Color,
    /// Line width in points.
    pub width: f64,
    #[serde(default)]
    pub dash: Dash,
}

/// Dash pattern of a stroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Dash {
    #[default]
    Solid,
    Dashed,
    Dotted,
}

/// How text in other frames flows around this frame.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TextWrap {
    pub mode: WrapMode,
    /// Extra clearance around the frame in points.
    #[serde(default)]
    pub margin: f64,
}

/// Text wrap mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum WrapMode {
    /// Text ignores this frame.
    #[default]
    None,
    /// Text flows around the frame's bounding box, on the wider side.
    Square,
    /// Text stops above the frame and resumes below it.
    TopBottom,
}

/// Template placeholder metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Placeholder {
    /// Semantic name, e.g. "headline" or "hero_image". Unique within a document.
    pub name: String,
    pub kind: PlaceholderKind,
    /// Sample text or instruction shown until the placeholder is filled.
    #[serde(default)]
    pub hint: String,
}

/// Placeholder content type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PlaceholderKind {
    Text,
    Image,
}

/// Text that flows through a chain of linked text frames.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Story {
    pub id: StoryId,
    /// The frame chain, in flow order. Text overflowing the last frame is hidden.
    pub frames: Vec<FrameId>,
    /// At least one paragraph.
    pub paragraphs: Vec<Paragraph>,
}

impl Story {
    pub fn new(id: StoryId, frame: FrameId) -> Self {
        Self {
            id,
            frames: vec![frame],
            paragraphs: vec![Paragraph::default()],
        }
    }
}

/// A paragraph: a paragraph style, local overrides, and styled runs.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Paragraph {
    /// Paragraph style; none = the document's default paragraph style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<StyleId>,
    /// Local paragraph overrides.
    #[serde(flatten)]
    pub props: ParaProps,
    /// Text runs. Never empty: an empty paragraph has one run with "" text.
    pub runs: Vec<Run>,
}

impl Paragraph {
    pub fn plain(text: &str) -> Self {
        Self {
            style: None,
            props: ParaProps::default(),
            runs: vec![Run::plain(text)],
        }
    }

    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }

    pub fn char_len(&self) -> usize {
        self.runs.iter().map(|r| r.text.chars().count()).sum()
    }
}

/// A run of text sharing one character format.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Run {
    pub text: String,
    /// Character style applied to this run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<StyleId>,
    /// Local character overrides.
    #[serde(flatten)]
    pub props: CharProps,
}

impl Run {
    pub fn plain(text: &str) -> Self {
        Self {
            text: text.to_string(),
            style: None,
            props: CharProps::default(),
        }
    }

    /// Same formatting as `other` (ignoring text).
    pub fn same_format(&self, other: &Run) -> bool {
        self.style == other.style && self.props == other.props
    }
}

/// Encoded image formats accepted as assets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ImageFormat {
    Png,
    Jpeg,
    Svg,
}

impl ImageFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Svg => "svg",
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Svg => "image/svg+xml",
        }
    }
}

/// Metadata about an embedded image.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Asset {
    pub id: AssetId,
    /// Original file name, for display.
    pub name: String,
    pub format: ImageFormat,
    /// Intrinsic width in pixels (SVG: CSS pixels at 96 dpi).
    pub pixel_width: u32,
    /// Intrinsic height in pixels (SVG: CSS pixels at 96 dpi).
    pub pixel_height: u32,
}

impl Document {
    /// A new, empty document with one page and the default style sheet.
    pub fn new(ids: &mut dyn IdSource, setup: PageSetup) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            id: DocumentId::new(ids),
            meta: DocMeta::default(),
            setup,
            grid: GridSettings::default(),
            pages: vec![Page::new(PageId::new(ids))],
            masters: Vec::new(),
            frames: BTreeMap::new(),
            stories: BTreeMap::new(),
            styles: StyleSheet::with_defaults(ids),
            assets: BTreeMap::new(),
            blobs: BTreeMap::new(),
        }
    }

    pub fn page(&self, id: PageId) -> Option<&Page> {
        self.pages.iter().find(|p| p.id == id)
    }

    pub fn page_mut(&mut self, id: PageId) -> Option<&mut Page> {
        self.pages.iter_mut().find(|p| p.id == id)
    }

    pub fn page_index(&self, id: PageId) -> Option<usize> {
        self.pages.iter().position(|p| p.id == id)
    }

    pub fn master(&self, id: MasterId) -> Option<&MasterPage> {
        self.masters.iter().find(|m| m.id == id)
    }

    pub fn master_mut(&mut self, id: MasterId) -> Option<&mut MasterPage> {
        self.masters.iter_mut().find(|m| m.id == id)
    }

    pub fn frame(&self, id: FrameId) -> Option<&Frame> {
        self.frames.get(&id)
    }

    pub fn frame_mut(&mut self, id: FrameId) -> Option<&mut Frame> {
        self.frames.get_mut(&id)
    }

    pub fn story(&self, id: StoryId) -> Option<&Story> {
        self.stories.get(&id)
    }

    pub fn story_mut(&mut self, id: StoryId) -> Option<&mut Story> {
        self.stories.get_mut(&id)
    }

    /// Guides of a page or master.
    pub fn guides(&self, owner: Owner) -> Option<&Vec<Guide>> {
        match owner {
            Owner::Page(p) => self.page(p).map(|p| &p.guides),
            Owner::Master(m) => self.master(m).map(|m| &m.guides),
        }
    }

    pub fn guides_mut(&mut self, owner: Owner) -> Option<&mut Vec<Guide>> {
        match owner {
            Owner::Page(p) => self.page_mut(p).map(|p| &mut p.guides),
            Owner::Master(m) => self.master_mut(m).map(|m| &mut m.guides),
        }
    }

    /// The ordered child list of a page, master or group.
    pub fn children(&self, parent: Parent) -> Option<&Vec<FrameId>> {
        match parent {
            Parent::Page(p) => self.page(p).map(|p| &p.frames),
            Parent::Master(m) => self.master(m).map(|m| &m.frames),
            Parent::Group(g) => match self.frame(g).map(|f| &f.content) {
                Some(FrameContent::Group(g)) => Some(&g.children),
                _ => None,
            },
        }
    }

    pub fn children_mut(&mut self, parent: Parent) -> Option<&mut Vec<FrameId>> {
        match parent {
            Parent::Page(p) => self.page_mut(p).map(|p| &mut p.frames),
            Parent::Master(m) => self.master_mut(m).map(|m| &mut m.frames),
            Parent::Group(g) => match self.frame_mut(g).map(|f| &mut f.content) {
                Some(FrameContent::Group(g)) => Some(&mut g.children),
                _ => None,
            },
        }
    }

    /// The container that lists `frame` as a child.
    pub fn parent_of(&self, frame: FrameId) -> Option<Parent> {
        for p in &self.pages {
            if p.frames.contains(&frame) {
                return Some(Parent::Page(p.id));
            }
        }
        for m in &self.masters {
            if m.frames.contains(&frame) {
                return Some(Parent::Master(m.id));
            }
        }
        self.frames.values().find_map(|f| match &f.content {
            FrameContent::Group(g) if g.children.contains(&frame) => Some(Parent::Group(f.id)),
            _ => None,
        })
    }

    /// The page or master a frame is on, following group nesting upward.
    pub fn owner_of(&self, frame: FrameId) -> Option<Owner> {
        let mut current = frame;
        for _ in 0..64 {
            match self.parent_of(current)? {
                Parent::Page(p) => return Some(Owner::Page(p)),
                Parent::Master(m) => return Some(Owner::Master(m)),
                Parent::Group(g) => current = g,
            }
        }
        None
    }

    /// `frame` and all frames nested inside it (depth-first, back to front).
    pub fn descendants(&self, frame: FrameId) -> Vec<FrameId> {
        let mut out = Vec::new();
        self.collect_descendants(frame, &mut out, 0);
        out
    }

    fn collect_descendants(&self, frame: FrameId, out: &mut Vec<FrameId>, depth: usize) {
        if depth > 64 {
            return;
        }
        out.push(frame);
        if let Some(f) = self.frame(frame) {
            for c in f.children() {
                self.collect_descendants(*c, out, depth + 1);
            }
        }
    }

    /// Every leaf (non-group) frame on a page or master, in paint order (back to front).
    pub fn paint_order(&self, owner: Owner) -> Vec<FrameId> {
        let Some(top) = self.children(owner.into()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for id in top {
            for d in self.descendants(*id) {
                if self.frame(d).is_some_and(|f| f.kind() != FrameKind::Group) {
                    out.push(d);
                }
            }
        }
        out
    }

    /// Every frame (including groups) under an owner, depth-first, back to front.
    pub fn all_frames_of(&self, owner: Owner) -> Vec<FrameId> {
        let Some(top) = self.children(owner.into()) else {
            return Vec::new();
        };
        top.iter().flat_map(|id| self.descendants(*id)).collect()
    }

    /// The story a text frame shows.
    pub fn story_of(&self, frame: FrameId) -> Option<StoryId> {
        self.frame(frame).and_then(|f| f.as_text()).map(|t| t.story)
    }

    /// Find a frame by its placeholder name.
    pub fn placeholder(&self, name: &str) -> Option<FrameId> {
        self.frames
            .values()
            .find(|f| f.placeholder.as_ref().is_some_and(|p| p.name == name))
            .map(|f| f.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::SeqIds;

    #[test]
    fn presets_have_expected_point_sizes() {
        let a4 = PageSizePreset::A4.size();
        assert!((a4.width - 595.2756).abs() < 1e-3 && (a4.height - 841.8898).abs() < 1e-3);
        assert_eq!(PageSizePreset::Letter.size(), Size::new(612.0, 792.0));
    }

    #[test]
    fn new_document_has_one_page_and_default_style() {
        let doc = Document::new(&mut SeqIds::new(1), PageSetup::default());
        assert_eq!(doc.pages.len(), 1);
        assert_eq!(doc.schema_version, SCHEMA_VERSION);
        assert!(doc.styles.get(doc.styles.default_paragraph).is_some());
    }
}

//! Text tools.

use pubrs_core::style::{
    DEFAULT_WIDOW_ORPHAN, DropCap, LinesOrFlag, List, ListFormat, ListKind, MAX_LIST_LEVEL, TabStop,
};
use pubrs_core::text::BreakKind;
use pubrs_core::{
    Align, Baseline, Caps, CharProps, Color, Command, DocumentId, FrameId, Margins, Paint,
    ParaProps, StoryId, StyleKind, TextFrameProps, VerticalAlign,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, ApiResult};
use crate::session::{Changed, Session};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextSet {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// A text frame (its whole chain's story is replaced). Or give story_id.
    #[serde(default)]
    pub frame_id: Option<FrameId>,
    #[serde(default)]
    pub story_id: Option<StoryId>,
    /// New text; "\n" separates paragraphs. Formatting of the first
    /// paragraph/run is kept (so template styling survives).
    pub text: String,
    /// Interpret `# `/`## ` headings and **bold**, *italic*, __underline__.
    #[serde(default)]
    pub markup: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TextChangeOut {
    pub story_id: StoryId,
    /// Story length in characters after the change.
    pub length: usize,
    /// True if the text no longer fits its frame chain.
    pub overflowing: bool,
    #[serde(flatten)]
    pub change: Changed,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextInsert {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    #[serde(default)]
    pub frame_id: Option<FrameId>,
    #[serde(default)]
    pub story_id: Option<StoryId>,
    /// Character offset (0 = start). Paragraphs count as one "\n" character.
    /// Omit to append at the end.
    #[serde(default)]
    pub offset: Option<usize>,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextReplaceRange {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    #[serde(default)]
    pub frame_id: Option<FrameId>,
    #[serde(default)]
    pub story_id: Option<StoryId>,
    /// First character to replace (inclusive).
    pub start: usize,
    /// End of the range (exclusive). start == end inserts.
    pub end: usize,
    /// Replacement text (may be empty to delete).
    pub text: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextApplyCharStyle {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    #[serde(default)]
    pub frame_id: Option<FrameId>,
    #[serde(default)]
    pub story_id: Option<StoryId>,
    /// Range start (character offset). Default 0.
    #[serde(default)]
    pub start: Option<usize>,
    /// Range end (exclusive). Default: end of story.
    #[serde(default)]
    pub end: Option<usize>,
    /// Character style name or id ("" removes the character style).
    #[serde(default)]
    pub style: Option<String>,
    /// Remove local character formatting first.
    #[serde(default)]
    pub clear: bool,
    #[serde(default)]
    pub font_family: Option<String>,
    /// Points.
    #[serde(default)]
    pub font_size: Option<f64>,
    #[serde(default)]
    pub bold: Option<bool>,
    #[serde(default)]
    pub italic: Option<bool>,
    #[serde(default)]
    pub underline: Option<bool>,
    /// Text colour: hex or `{"swatch": id, "tint": 0-100}`.
    #[serde(default)]
    pub color: Option<Paint>,
    /// Extra letter spacing in points.
    #[serde(default)]
    pub letter_spacing: Option<f64>,
    /// `"normal"`, `"superscript"`, `"subscript"` or `{"shift": points}` (positive raises).
    #[serde(default)]
    pub baseline: Option<Baseline>,
    /// `"none"`, `"small"` (small caps) or `"all"` (all caps). The text itself is unchanged.
    #[serde(default)]
    pub caps: Option<Caps>,
    #[serde(default)]
    pub strikethrough: Option<bool>,
    /// Standard ligatures (default on).
    #[serde(default)]
    pub ligatures: Option<bool>,
    /// Pair kerning (default on).
    #[serde(default)]
    pub kerning: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextApplyParaStyle {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    #[serde(default)]
    pub frame_id: Option<FrameId>,
    #[serde(default)]
    pub story_id: Option<StoryId>,
    /// Paragraphs touching start..end are changed. Default: all paragraphs.
    #[serde(default)]
    pub start: Option<usize>,
    #[serde(default)]
    pub end: Option<usize>,
    /// Paragraph style name or id (e.g. "Heading 1", "Body").
    #[serde(default)]
    pub style: Option<String>,
    #[serde(default)]
    pub align: Option<Align>,
    /// Line height as a multiple of the font size (1.2 = normal).
    #[serde(default)]
    pub line_spacing: Option<f64>,
    /// Points.
    #[serde(default)]
    pub space_before: Option<f64>,
    /// Points.
    #[serde(default)]
    pub space_after: Option<f64>,
    #[serde(default)]
    pub first_line_indent: Option<f64>,
    #[serde(default)]
    pub left_indent: Option<f64>,
    #[serde(default)]
    pub right_indent: Option<f64>,
    /// Break words at hyphenation points on loose lines (default off).
    #[serde(default)]
    pub hyphenate: Option<bool>,
    /// BCP 47 language tag for hyphenation (default "en-US").
    #[serde(default)]
    pub language: Option<String>,
    /// Caps on justified stretch: `{max_word_space, letter_spacing}` (defaults 1.33 and 0.05). Unset leaves it as is (inherited, or unbounded as in v0.1); `{"unbounded": true}` removes limits a paragraph style sets.
    #[serde(default)]
    pub justify_limits: Option<pubrs_core::JustifyLimits>,
    /// Keep the last line in the same column or frame as the next
    /// paragraph's first line (default off).
    #[serde(default)]
    pub keep_with_next: Option<bool>,
    /// Keep all lines in one column or frame (default off).
    #[serde(default)]
    pub keep_together: Option<bool>,
    /// Widow and orphan control: `true` (2 lines), a line count, or `false`/0 to turn it off (default off).
    #[serde(default)]
    pub widow_orphan: Option<LinesOrFlag>,
    /// Drop cap `{lines, chars}` (defaults 3 and 1); `{"lines": 0}` removes it.
    #[serde(default)]
    pub drop_cap: Option<DropCap>,
    /// Tab stops `[{pos, align: left|center|right|decimal, leader: none|dot|dash|line}]` (pos in points from the text edge); `[]` clears them. See text_set_tabs.
    #[serde(default)]
    pub tab_stops: Option<Vec<TabStop>>,
    /// Bullet or numbered list `{kind: bullet|number|none, bullet, format, level, indent, start}`; `{"kind": "none"}` removes it. See text_apply_list.
    #[serde(default)]
    pub list: Option<List>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextSetTabs {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    #[serde(default)]
    pub frame_id: Option<FrameId>,
    #[serde(default)]
    pub story_id: Option<StoryId>,
    /// Paragraphs touching start..end are changed. Default: all paragraphs.
    #[serde(default)]
    pub start: Option<usize>,
    #[serde(default)]
    pub end: Option<usize>,
    /// The tab stops, replacing the paragraphs' own: `pos` in points from the text edge, `align` left (default), center, right or decimal, `leader` none (default), dot, dash or line. An empty list clears them. Without stops a tab goes to the next 36 pt; past the last stop too.
    pub stops: Vec<TabStop>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextApplyList {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    #[serde(default)]
    pub frame_id: Option<FrameId>,
    #[serde(default)]
    pub story_id: Option<StoryId>,
    /// Paragraphs touching start..end become list items. Default: all paragraphs.
    #[serde(default)]
    pub start: Option<usize>,
    #[serde(default)]
    pub end: Option<usize>,
    /// bullet or number.
    pub kind: ListKind,
    /// Number format: "1." (default), "a.", "A.", "i." or "I.".
    #[serde(default)]
    pub format: Option<ListFormat>,
    /// Bullet character (default U+2022).
    #[serde(default)]
    pub bullet: Option<char>,
    /// Nesting level 0-8 (default 0). A deeper level restarts when a shallower item appears.
    #[serde(default)]
    pub level: Option<u8>,
    /// Hanging indent in points: the label sits this far left of the text (default 18).
    #[serde(default)]
    pub indent: Option<f64>,
    /// Number the first paragraph of the range from this value instead of continuing the count.
    #[serde(default)]
    pub restart_at: Option<u32>,
    /// Left indent of the text in points (default: (level + 1) x indent).
    #[serde(default)]
    pub left_indent: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextClearList {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    #[serde(default)]
    pub frame_id: Option<FrameId>,
    #[serde(default)]
    pub story_id: Option<StoryId>,
    /// Paragraphs touching start..end stop being list items. Default: all paragraphs.
    #[serde(default)]
    pub start: Option<usize>,
    #[serde(default)]
    pub end: Option<usize>,
    /// Leave the left indent as it is (default: it is reset to 0).
    #[serde(default)]
    pub keep_indent: bool,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextInsertBreak {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    #[serde(default)]
    pub frame_id: Option<FrameId>,
    #[serde(default)]
    pub story_id: Option<StoryId>,
    /// column (continue in the next column or frame), frame (the next frame of the chain) or page (the next frame of the chain on a later page; with none, the rest overflows).
    pub kind: BreakKind,
    /// Character offset (0 = start). Omit to append at the end.
    #[serde(default)]
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextLinkFrames {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Text frame whose overflow continues into `to_frame_id`.
    pub from_frame_id: FrameId,
    /// An unlinked text frame (its text, if any, is appended to the chain).
    pub to_frame_id: FrameId,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextUnlinkFrames {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// The link after this frame is broken; later frames get a new empty story.
    pub frame_id: FrameId,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TextUnlinkOut {
    /// Story now shown by the frames after the break.
    pub new_story_id: StoryId,
    #[serde(flatten)]
    pub change: Changed,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextRef {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    #[serde(default)]
    pub frame_id: Option<FrameId>,
    #[serde(default)]
    pub story_id: Option<StoryId>,
}

/// A run in `text_get` output.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RunOut {
    pub start: usize,
    pub end: usize,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    /// Local overrides.
    pub props: CharProps,
    /// Effective font family, size, weight and colour.
    pub font_family: String,
    pub font_size: f64,
    pub bold: bool,
    pub italic: bool,
    /// Effective colour as hex (swatches resolved).
    pub color: Color,
    /// Effective colour as stored: hex or a swatch reference.
    pub color_paint: Paint,
}

/// A paragraph in `text_get` output.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ParagraphOut {
    pub index: usize,
    pub start: usize,
    pub end: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    pub props: ParaProps,
    pub align: Align,
    /// The bullet or number drawn for a list item (generated text, not in the story).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_label: Option<String>,
    pub runs: Vec<RunOut>,
}

/// Which characters a frame of the chain shows.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FrameRange {
    pub frame_id: FrameId,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TextGetOut {
    pub story_id: StoryId,
    /// Plain text, paragraphs separated by "\n".
    pub text: String,
    /// Length in characters.
    pub length: usize,
    pub paragraphs: Vec<ParagraphOut>,
    /// Frames of the chain in flow order with the range each shows.
    pub frames: Vec<FrameRange>,
    /// Characters that do not fit (0 = everything shown).
    pub hidden_chars: usize,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct StoryOverflow {
    pub story_id: StoryId,
    pub frame_ids: Vec<FrameId>,
    pub overflowing: bool,
    /// Characters that do not fit.
    pub hidden_chars: usize,
    /// First character offset that is not shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_hidden_offset: Option<usize>,
    /// The last frame of the chain (enlarge it or link another frame after it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_frame_id: Option<FrameId>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct OverflowOut {
    pub any_overflow: bool,
    pub stories: Vec<StoryOverflow>,
    pub revision: u64,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextSetFrameOptions {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub frame_id: FrameId,
    /// Columns inside the frame (1-12).
    #[serde(default)]
    pub columns: Option<u32>,
    /// Gap between columns in points.
    #[serde(default)]
    pub column_gap: Option<f64>,
    /// Inner padding in points on every side.
    #[serde(default)]
    pub inset: Option<f64>,
    /// top, middle or bottom.
    #[serde(default)]
    pub vertical_align: Option<VerticalAlign>,
}

impl Session {
    fn text_out(&mut self, story: StoryId, change: Changed) -> ApiResult<TextChangeOut> {
        let layout = self.layout(Some(change.doc_id))?;
        let doc = self.get(Some(change.doc_id))?.doc();
        Ok(TextChangeOut {
            story_id: story,
            length: doc.story(story).map_or(0, |s| s.char_len()),
            overflowing: layout
                .stories
                .get(&story)
                .is_some_and(|s| s.overflow.is_some()),
            change,
        })
    }

    pub fn text_set(&mut self, req: TextSet) -> ApiResult<TextChangeOut> {
        let story = self.resolve_story(req.doc_id, req.frame_id, req.story_id)?;
        let doc = self.get(req.doc_id)?.doc();
        let s = doc.story(story).expect("resolved");
        let command = if req.markup {
            let first = s.paragraphs.first().cloned().unwrap_or_default();
            let base_run = first.runs.first().cloned().unwrap_or_default();
            let mut paras = crate::markup::parse(&req.text, &doc.styles);
            for p in &mut paras {
                if p.style.is_none() {
                    p.style = first.style;
                    p.props = first.props.clone();
                }
                for r in &mut p.runs {
                    r.props = base_run.props.merged(&r.props);
                    if r.style.is_none() {
                        r.style = base_run.style;
                    }
                }
            }
            Command::SetStory {
                story,
                paragraphs: paras,
            }
        } else {
            Command::ReplaceText {
                story,
                start: 0,
                end: s.char_len(),
                text: req.text,
            }
        };
        let change = self.exec(req.doc_id, command)?;
        self.text_out(story, change)
    }

    pub fn text_insert(&mut self, req: TextInsert) -> ApiResult<TextChangeOut> {
        let story = self.resolve_story(req.doc_id, req.frame_id, req.story_id)?;
        let len = self
            .get(req.doc_id)?
            .doc()
            .story(story)
            .map_or(0, |s| s.char_len());
        let change = self.exec(
            req.doc_id,
            Command::InsertText {
                story,
                at: req.offset.unwrap_or(len),
                text: req.text,
                format: None,
            },
        )?;
        self.text_out(story, change)
    }

    pub fn text_insert_break(&mut self, req: TextInsertBreak) -> ApiResult<TextChangeOut> {
        let story = self.resolve_story(req.doc_id, req.frame_id, req.story_id)?;
        let len = self
            .get(req.doc_id)?
            .doc()
            .story(story)
            .map_or(0, |s| s.char_len());
        let at = req.offset.unwrap_or(len);
        if at > len {
            return Err(ApiError::invalid(
                format!("offset {at} is outside the story (length {len})"),
                "use text_get to see character offsets",
            ));
        }
        let change = self.exec(
            req.doc_id,
            Command::InsertText {
                story,
                at,
                text: req.kind.char().to_string(),
                format: None,
            },
        )?;
        self.text_out(story, change)
    }

    pub fn text_replace_range(&mut self, req: TextReplaceRange) -> ApiResult<TextChangeOut> {
        let story = self.resolve_story(req.doc_id, req.frame_id, req.story_id)?;
        let change = self.exec(
            req.doc_id,
            Command::ReplaceText {
                story,
                start: req.start,
                end: req.end,
                text: req.text,
            },
        )?;
        self.text_out(story, change)
    }

    fn range(
        &self,
        doc_id: Option<DocumentId>,
        story: StoryId,
        s: Option<usize>,
        e: Option<usize>,
    ) -> ApiResult<(usize, usize)> {
        let len = self
            .get(doc_id)?
            .doc()
            .story(story)
            .map_or(0, |s| s.char_len());
        let start = s.unwrap_or(0);
        let end = e.unwrap_or(len);
        if start > end || end > len {
            return Err(ApiError::invalid(
                format!("range {start}..{end} is outside the story (length {len})"),
                "use text_get to see character offsets",
            ));
        }
        Ok((start, end))
    }

    pub fn text_apply_char_style(&mut self, req: TextApplyCharStyle) -> ApiResult<TextChangeOut> {
        let story = self.resolve_story(req.doc_id, req.frame_id, req.story_id)?;
        let (start, end) = self.range(req.doc_id, story, req.start, req.end)?;
        let mut commands = Vec::new();
        if req.clear {
            commands.push(Command::ClearCharFormat { story, start, end });
        }
        if let Some(name) = &req.style {
            let style = if name.is_empty() {
                None
            } else {
                Some(self.resolve_style(req.doc_id, name, Some(StyleKind::Character))?)
            };
            commands.push(Command::SetCharStyle {
                story,
                start,
                end,
                style,
            });
        }
        let props = CharProps {
            font_family: req.font_family,
            font_size: req.font_size,
            bold: req.bold,
            italic: req.italic,
            underline: req.underline,
            color: req.color,
            letter_spacing: req.letter_spacing,
            baseline: req.baseline,
            caps: req.caps,
            strikethrough: req.strikethrough,
            ligatures: req.ligatures,
            kerning: req.kerning,
        };
        if !props.is_empty() {
            commands.push(Command::ApplyCharProps {
                story,
                start,
                end,
                props,
            });
        }
        let change = self.exec(
            req.doc_id,
            Command::Batch {
                label: "character format".into(),
                commands,
            },
        )?;
        self.text_out(story, change)
    }

    pub fn text_apply_para_style(&mut self, req: TextApplyParaStyle) -> ApiResult<TextChangeOut> {
        let story = self.resolve_story(req.doc_id, req.frame_id, req.story_id)?;
        let (start, end) = self.range(req.doc_id, story, req.start, req.end)?;
        let mut commands = Vec::new();
        if let Some(name) = &req.style {
            let style = Some(self.resolve_style(req.doc_id, name, Some(StyleKind::Paragraph))?);
            commands.push(Command::SetParaStyle {
                story,
                start,
                end,
                style,
            });
        }
        let props = ParaProps {
            align: req.align,
            line_spacing: req.line_spacing,
            space_before: req.space_before,
            space_after: req.space_after,
            first_line_indent: req.first_line_indent,
            left_indent: req.left_indent,
            right_indent: req.right_indent,
            hyphenate: req.hyphenate,
            language: req.language,
            justify_limits: req.justify_limits,
            keep_with_next: req.keep_with_next,
            keep_together: req.keep_together,
            widow_orphan: req.widow_orphan.map(|w| match w {
                LinesOrFlag::Flag(true) => DEFAULT_WIDOW_ORPHAN,
                LinesOrFlag::Flag(false) => 0,
                LinesOrFlag::Lines(n) => n,
            }),
            drop_cap: req.drop_cap,
            tab_stops: req.tab_stops,
            list: req.list,
        };
        if props.widow_orphan.is_some_and(|n| n > 10)
            || props.drop_cap.is_some_and(|d| !d.is_valid())
            || props.list.is_some_and(|l| !l.is_valid())
            || props
                .tab_stops
                .iter()
                .flatten()
                .any(|s| !(0.0..=10_000.0).contains(&s.pos))
        {
            return Err(ApiError::invalid(
                "invalid keep, drop cap, tab or list settings",
                "widow_orphan is true, false or 0-10 lines; drop_cap lines are 0-10 and chars 1-15; tab stops 0-10000 points; list level 0-8",
            ));
        }
        if props != ParaProps::default() {
            commands.push(Command::ApplyParaProps {
                story,
                start,
                end,
                props,
            });
        }
        let change = self.exec(
            req.doc_id,
            Command::Batch {
                label: "paragraph format".into(),
                commands,
            },
        )?;
        self.text_out(story, change)
    }

    pub fn text_set_tabs(&mut self, req: TextSetTabs) -> ApiResult<TextChangeOut> {
        let story = self.resolve_story(req.doc_id, req.frame_id, req.story_id)?;
        let (start, end) = self.range(req.doc_id, story, req.start, req.end)?;
        let props = ParaProps {
            tab_stops: Some(req.stops),
            ..Default::default()
        };
        if !props.is_valid() {
            return Err(ApiError::invalid(
                "tab stops must have a position of 0-10000 points, and at most 64 of them",
                "pos is measured in points from the left edge of the text area",
            ));
        }
        let change = self.exec(
            req.doc_id,
            Command::ApplyParaProps {
                story,
                start,
                end,
                props,
            },
        )?;
        self.text_out(story, change)
    }

    pub fn text_apply_list(&mut self, req: TextApplyList) -> ApiResult<TextChangeOut> {
        let story = self.resolve_story(req.doc_id, req.frame_id, req.story_id)?;
        let (start, end) = self.range(req.doc_id, story, req.start, req.end)?;
        if req.kind == ListKind::None {
            return Err(ApiError::invalid(
                "kind is bullet or number",
                "use text_clear_list to remove a list",
            ));
        }
        if req.restart_at.is_some_and(|n| n > 1_000_000) {
            return Err(ApiError::invalid(
                "restart_at is at most 1000000",
                "number the list from a smaller value",
            ));
        }
        let defaults = List::default();
        let level = req.level.unwrap_or(0);
        let indent = req.indent.unwrap_or(defaults.indent);
        let list = List {
            kind: req.kind,
            bullet: req.bullet.unwrap_or(defaults.bullet),
            format: req.format.unwrap_or_default(),
            level,
            indent,
            start: None,
        };
        if !list.is_valid() {
            return Err(ApiError::invalid(
                format!(
                    "invalid list: level is 0-{MAX_LIST_LEVEL}, indent 0-720 points, bullet a printable character"
                ),
                "for example {\"kind\": \"number\", \"format\": \"a.\", \"level\": 1}",
            ));
        }
        let left_indent = req.left_indent.unwrap_or((f64::from(level) + 1.0) * indent);
        let commands = {
            let open = self.get(req.doc_id)?;
            let s = open.doc().story(story).expect("resolved");
            pubrs_core::apply_list_commands(story, s, start, end, list, left_indent, req.restart_at)
        };
        let change = self.exec(
            req.doc_id,
            Command::Batch {
                label: "apply list".into(),
                commands,
            },
        )?;
        self.text_out(story, change)
    }

    pub fn text_clear_list(&mut self, req: TextClearList) -> ApiResult<TextChangeOut> {
        let story = self.resolve_story(req.doc_id, req.frame_id, req.story_id)?;
        let (start, end) = self.range(req.doc_id, story, req.start, req.end)?;
        let commands = {
            let open = self.get(req.doc_id)?;
            let doc = open.doc();
            let s = doc.story(story).expect("resolved");
            pubrs_core::clear_list_commands(story, s, &doc.styles, start, end, req.keep_indent)
        };
        let change = self.exec(
            req.doc_id,
            Command::Batch {
                label: "clear list".into(),
                commands,
            },
        )?;
        self.text_out(story, change)
    }

    pub fn text_link_frames(&mut self, req: TextLinkFrames) -> ApiResult<TextChangeOut> {
        let story = self.resolve_story(req.doc_id, Some(req.from_frame_id), None)?;
        let change = self.exec(
            req.doc_id,
            Command::LinkFrames {
                from: req.from_frame_id,
                to: req.to_frame_id,
            },
        )?;
        self.text_out(story, change)
    }

    pub fn text_unlink_frames(&mut self, req: TextUnlinkFrames) -> ApiResult<TextUnlinkOut> {
        let new_story = self.new_story_id();
        let change = self.exec(
            req.doc_id,
            Command::UnlinkFrame {
                frame: req.frame_id,
                new_story,
            },
        )?;
        Ok(TextUnlinkOut {
            new_story_id: new_story,
            change,
        })
    }

    pub fn text_get(&mut self, req: TextRef) -> ApiResult<TextGetOut> {
        let story_id = self.resolve_story(req.doc_id, req.frame_id, req.story_id)?;
        let layout = self.layout(req.doc_id)?;
        let open = self.get(req.doc_id)?;
        let doc = open.doc();
        let story = doc.story(story_id).expect("resolved");
        let style_name = |id: Option<pubrs_core::StyleId>| {
            id.and_then(|i| doc.styles.get(i)).map(|s| s.name.clone())
        };
        // Labels sit on the first line of their item, which starts at the
        // paragraph's first character.
        let list_labels: std::collections::HashMap<usize, String> = layout
            .stories
            .get(&story_id)
            .into_iter()
            .flat_map(|s| s.frames.values())
            .flat_map(|f| f.lines.iter())
            .filter_map(|l| Some((l.chars.start, l.list_label.clone()?)))
            .collect();
        let mut paragraphs = Vec::new();
        let mut pos = 0;
        for (index, p) in story.paragraphs.iter().enumerate() {
            let start = pos;
            let mut runs = Vec::new();
            for r in &p.runs {
                let n = r.text.chars().count();
                let a = doc.styles.resolve_char(p, r);
                runs.push(RunOut {
                    start: pos,
                    end: pos + n,
                    text: r.text.clone(),
                    style: style_name(r.style),
                    props: r.props.clone(),
                    font_family: a.font_family,
                    font_size: a.font_size,
                    bold: a.bold,
                    italic: a.italic,
                    color: doc.resolve_ink(a.color).rgba,
                    color_paint: a.color.into(),
                });
                pos += n;
            }
            paragraphs.push(ParagraphOut {
                index,
                start,
                end: pos,
                style: style_name(p.style)
                    .or_else(|| style_name(Some(doc.styles.default_paragraph))),
                props: p.props.clone(),
                align: doc.styles.resolve_para(p).align,
                list_label: list_labels.get(&start).cloned(),
                runs,
            });
            pos += 1;
        }
        let sl = layout.stories.get(&story_id);
        let frames = story
            .frames
            .iter()
            .map(|f| {
                let r = sl
                    .and_then(|s| s.frames.get(f))
                    .map(|ft| ft.chars.clone())
                    .unwrap_or(0..0);
                FrameRange {
                    frame_id: *f,
                    start: r.start,
                    end: r.end,
                }
            })
            .collect();
        Ok(TextGetOut {
            story_id,
            text: story.plain_text(),
            length: story.char_len(),
            paragraphs,
            frames,
            hidden_chars: sl.and_then(|s| s.overflow).map_or(0, |o| o.hidden_chars),
            revision: open.revision(),
        })
    }

    pub fn text_overflow_status(&mut self, req: super::doc::DocRef) -> ApiResult<OverflowOut> {
        let layout = self.layout(req.doc_id)?;
        let open = self.get(req.doc_id)?;
        let doc = open.doc();
        let stories: Vec<StoryOverflow> = doc
            .stories
            .values()
            .map(|s| {
                let o = layout.stories.get(&s.id).and_then(|l| l.overflow);
                StoryOverflow {
                    story_id: s.id,
                    frame_ids: s.frames.clone(),
                    overflowing: o.is_some(),
                    hidden_chars: o.map_or(0, |o| o.hidden_chars),
                    first_hidden_offset: o.map(|o| o.offset),
                    last_frame_id: s.frames.last().copied(),
                }
            })
            .collect();
        Ok(OverflowOut {
            any_overflow: stories.iter().any(|s| s.overflowing),
            stories,
            revision: open.revision(),
        })
    }

    pub fn text_set_frame_options(&mut self, req: TextSetFrameOptions) -> ApiResult<Changed> {
        self.exec(
            req.doc_id,
            Command::SetTextFrame {
                frame: req.frame_id,
                props: TextFrameProps {
                    insets: req.inset.map(Margins::uniform),
                    columns: req.columns,
                    column_gap: req.column_gap,
                    vertical_align: req.vertical_align,
                },
            },
        )
    }
}

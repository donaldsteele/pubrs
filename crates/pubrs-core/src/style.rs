//! Paragraph and character styles, and resolution of effective formatting.
//!
//! Effective character formatting is resolved in this order (later wins):
//! built-in defaults, the default paragraph style chain, the paragraph's
//! style chain, the run's character style chain, the run's local props.

use serde::{Deserialize, Deserializer, Serialize};

use crate::color::Color;
use crate::id::{IdSource, StyleId};
use crate::model::{Paragraph, Run};
use crate::swatch::{Ink, Paint};

/// Font family used when nothing else is specified. Bundled with pubrs.
pub const DEFAULT_FONT_FAMILY: &str = "Liberation Sans";
/// Default font size in points.
pub const DEFAULT_FONT_SIZE: f64 = 11.0;

/// Character formatting overrides. Unset fields inherit.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CharProps {
    /// Font family name, e.g. "Liberation Serif".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    /// Font size in points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
    /// Text colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Paint>,
    /// Extra space between letters in points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub letter_spacing: Option<f64>,
    /// Baseline position: normal, superscript, subscript or a shift in
    /// points (positive raises). Unset inherits (normal).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<Baseline>,
    /// Capitalisation: none, small caps or all caps (display only; the
    /// story text is unchanged).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caps: Option<Caps>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strikethrough: Option<bool>,
    /// Standard ligatures (default on).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ligatures: Option<bool>,
    /// Pair kerning (default on).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kerning: Option<bool>,
}

/// Baseline position of a run.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Baseline {
    #[default]
    Normal,
    Superscript,
    Subscript,
    /// Shift in points, positive up.
    Shift(f64),
}

/// Capitalisation of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Caps {
    #[default]
    None,
    Small,
    All,
}

impl CharProps {
    /// Overlay `other` on top of `self` (set fields of `other` win).
    pub fn merged(&self, other: &CharProps) -> CharProps {
        CharProps {
            font_family: other
                .font_family
                .clone()
                .or_else(|| self.font_family.clone()),
            font_size: other.font_size.or(self.font_size),
            bold: other.bold.or(self.bold),
            italic: other.italic.or(self.italic),
            underline: other.underline.or(self.underline),
            color: other.color.clone().or_else(|| self.color.clone()),
            letter_spacing: other.letter_spacing.or(self.letter_spacing),
            baseline: other.baseline.or(self.baseline),
            caps: other.caps.or(self.caps),
            strikethrough: other.strikethrough.or(self.strikethrough),
            ligatures: other.ligatures.or(self.ligatures),
            kerning: other.kerning.or(self.kerning),
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == CharProps::default()
    }

    pub fn is_valid(&self) -> bool {
        self.font_size
            .is_none_or(|s| s.is_finite() && s > 0.0 && s <= 1296.0)
            && self.letter_spacing.is_none_or(|s| s.is_finite())
            && self.baseline.is_none_or(|b| match b {
                Baseline::Shift(v) => v.is_finite() && v.abs() <= 1296.0,
                _ => true,
            })
            && self
                .font_family
                .as_deref()
                .is_none_or(|f| !f.trim().is_empty())
    }
}

/// Paragraph formatting overrides. Unset fields inherit.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ParaProps {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<Align>,
    /// Line height as a multiple of the font size (1.0 = solid, 1.2 = normal).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_spacing: Option<f64>,
    /// Space above the paragraph in points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space_before: Option<f64>,
    /// Space below the paragraph in points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space_after: Option<f64>,
    /// Indent of the first line in points (relative to left_indent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_line_indent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_indent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right_indent: Option<f64>,
    /// Break words at hyphenation points when a line would be loose
    /// (default off).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hyphenate: Option<bool>,
    /// BCP 47 language tag used for hyphenation, e.g. "en-US" (default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Caps on how far justification may stretch a line. Unset inherits;
    /// `{"unbounded": true}` overrides inherited limits with none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub justify_limits: Option<JustifyLimits>,
    /// Keep the paragraph's last line in the same column or frame as the
    /// next paragraph's first line (default off).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_with_next: Option<bool>,
    /// Keep every line of the paragraph in one column or frame (default
    /// off).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_together: Option<bool>,
    /// Widow and orphan control: the fewest lines of the paragraph left on
    /// their own at the bottom or top of a column or frame (0-10). `true`
    /// means 2; `0`, `1` or `false` turns control off (default off).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "de_lines_or_flag"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "Option<LinesOrFlag>"))]
    pub widow_orphan: Option<u8>,
    /// Drop cap: the first characters drawn large, sunk into the first
    /// lines. Unset inherits; `{"lines": 0}` removes one a style sets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drop_cap: Option<DropCap>,
    /// Tab stops, replacing any a style sets. An empty list clears them.
    /// A paragraph that has tabs but no stops uses a stop every 36 pt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_stops: Option<Vec<TabStop>>,
    /// Bullet or numbered list membership. `{"kind": "none"}` removes a
    /// list a style sets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list: Option<List>,
}

/// Distance between the default tab stops, in points.
pub const DEFAULT_TAB_INTERVAL: f64 = 36.0;

/// How text lines up at a tab stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TabAlign {
    /// Text starts at the stop.
    #[default]
    Left,
    /// Text is centred on the stop.
    Center,
    /// Text ends at the stop.
    Right,
    /// The first `.` or `,` after the tab sits on the stop.
    Decimal,
}

/// What fills the gap before a tab stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TabLeader {
    #[default]
    None,
    Dot,
    Dash,
    Line,
}

impl TabLeader {
    /// The glyph repeated to fill the gap.
    pub fn glyph(self) -> Option<char> {
        match self {
            Self::None => None,
            Self::Dot => Some('.'),
            Self::Dash => Some('-'),
            Self::Line => Some('_'),
        }
    }
}

/// A tab stop.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TabStop {
    /// Distance from the frame's left text edge in points.
    pub pos: f64,
    #[serde(default)]
    pub align: TabAlign,
    #[serde(default)]
    pub leader: TabLeader,
}

/// Bullet or number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ListKind {
    #[default]
    Bullet,
    Number,
    /// Not a list; overrides a list a style sets.
    None,
}

/// How list numbers are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum ListFormat {
    #[default]
    #[serde(rename = "1.")]
    Decimal,
    #[serde(rename = "a.")]
    LowerAlpha,
    #[serde(rename = "A.")]
    UpperAlpha,
    #[serde(rename = "i.")]
    LowerRoman,
    #[serde(rename = "I.")]
    UpperRoman,
}

impl ListFormat {
    /// The label for item `n` (1-based), with its trailing full stop.
    pub fn label(self, n: u32) -> String {
        let n = n.max(1);
        match self {
            Self::Decimal => format!("{n}."),
            Self::LowerAlpha => format!("{}.", alpha(n, b'a')),
            Self::UpperAlpha => format!("{}.", alpha(n, b'A')),
            Self::LowerRoman => format!("{}.", roman(n).to_lowercase()),
            Self::UpperRoman => format!("{}.", roman(n)),
        }
    }
}

fn alpha(mut n: u32, base: u8) -> String {
    let mut out = Vec::new();
    while n > 0 {
        n -= 1;
        out.push(base + (n % 26) as u8);
        n /= 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

fn roman(mut n: u32) -> String {
    if n >= 4000 {
        return n.to_string();
    }
    let table = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (v, r) in table {
        while n >= v {
            out.push_str(r);
            n -= v;
        }
    }
    out
}

/// List membership of a paragraph. The label is generated text drawn in
/// the hanging indent; it is not part of the story.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct List {
    #[serde(default)]
    pub kind: ListKind,
    /// Bullet character (default U+2022).
    #[serde(default = "default_bullet")]
    pub bullet: char,
    /// Number format (default "1.").
    #[serde(default)]
    pub format: ListFormat,
    /// Nesting level, 0-8 (default 0).
    #[serde(default)]
    pub level: u8,
    /// Hanging indent in points: the label sits this far left of the text
    /// (default 18).
    #[serde(default = "default_list_indent")]
    pub indent: f64,
    /// Restart numbering at this value on this paragraph.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<u32>,
}

pub const MAX_LIST_LEVEL: u8 = 8;

fn default_bullet() -> char {
    '\u{2022}'
}

fn default_list_indent() -> f64 {
    18.0
}

impl Default for List {
    fn default() -> Self {
        Self {
            kind: ListKind::Bullet,
            bullet: default_bullet(),
            format: ListFormat::Decimal,
            level: 0,
            indent: default_list_indent(),
            start: None,
        }
    }
}

impl List {
    pub fn is_valid(&self) -> bool {
        self.level <= MAX_LIST_LEVEL
            && self.indent.is_finite()
            && (0.0..=720.0).contains(&self.indent)
            && !self.bullet.is_control()
            && self.start.is_none_or(|s| s <= 1_000_000)
    }
}

/// Lines kept together by widow and orphan control when it is turned on
/// without a number.
pub const DEFAULT_WIDOW_ORPHAN: u8 = 2;

/// A line count, or `true`/`false` for on (with the default count) and off.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum LinesOrFlag {
    /// `true` = on with 2 lines, `false` = off.
    Flag(bool),
    /// Fewest lines (0 or 1 = off).
    Lines(u8),
}

fn de_lines_or_flag<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u8>, D::Error> {
    Ok(Option::<LinesOrFlag>::deserialize(d)?.map(|v| match v {
        LinesOrFlag::Flag(true) => DEFAULT_WIDOW_ORPHAN,
        LinesOrFlag::Flag(false) => 0,
        LinesOrFlag::Lines(n) => n,
    }))
}

/// A drop cap: the paragraph's first `chars` characters drawn as one large
/// glyph run spanning `lines` lines, with those lines indented past it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DropCap {
    /// Lines the cap spans, from its cap height to the baseline of the
    /// last (1-10, default 3). 0 removes a drop cap the style sets.
    #[serde(default = "default_drop_cap_lines")]
    pub lines: u8,
    /// Characters drawn as the cap (1-15, default 1).
    #[serde(default = "default_drop_cap_chars")]
    pub chars: u8,
}

fn default_drop_cap_lines() -> u8 {
    3
}

fn default_drop_cap_chars() -> u8 {
    1
}

impl Default for DropCap {
    fn default() -> Self {
        Self {
            lines: default_drop_cap_lines(),
            chars: default_drop_cap_chars(),
        }
    }
}

impl DropCap {
    pub const MAX_LINES: u8 = 10;
    pub const MAX_CHARS: u8 = 15;

    pub fn is_valid(&self) -> bool {
        self.lines <= Self::MAX_LINES && (1..=Self::MAX_CHARS).contains(&self.chars)
    }
}

/// How far justification may stretch a line before it is left short.
/// Word spaces stretch up to `max_word_space` first, then letter spacing
/// up to `letter_spacing`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct JustifyLimits {
    /// Widest a word space may get, as a multiple of a normal space
    /// (default 1.33).
    #[serde(default = "default_max_word_space")]
    pub max_word_space: f64,
    /// Widest letter spacing, as a fraction of the font size (default 0.05).
    #[serde(default = "default_justify_letter_spacing")]
    pub letter_spacing: f64,
    /// `true` turns limits off: justification is unbounded, as when no
    /// limits are set. Use it to override limits a paragraph style sets.
    #[serde(default, skip_serializing_if = "is_false")]
    pub unbounded: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn default_max_word_space() -> f64 {
    1.33
}

fn default_justify_letter_spacing() -> f64 {
    0.05
}

impl Default for JustifyLimits {
    fn default() -> Self {
        Self {
            max_word_space: default_max_word_space(),
            letter_spacing: default_justify_letter_spacing(),
            unbounded: false,
        }
    }
}

impl JustifyLimits {
    pub fn is_valid(&self) -> bool {
        self.max_word_space.is_finite()
            && self.max_word_space >= 1.0
            && self.letter_spacing.is_finite()
            && (0.0..=1.0).contains(&self.letter_spacing)
    }
}

impl ParaProps {
    pub fn merged(&self, other: &ParaProps) -> ParaProps {
        ParaProps {
            align: other.align.or(self.align),
            line_spacing: other.line_spacing.or(self.line_spacing),
            space_before: other.space_before.or(self.space_before),
            space_after: other.space_after.or(self.space_after),
            first_line_indent: other.first_line_indent.or(self.first_line_indent),
            left_indent: other.left_indent.or(self.left_indent),
            right_indent: other.right_indent.or(self.right_indent),
            hyphenate: other.hyphenate.or(self.hyphenate),
            language: other.language.clone().or_else(|| self.language.clone()),
            justify_limits: other.justify_limits.or(self.justify_limits),
            keep_with_next: other.keep_with_next.or(self.keep_with_next),
            keep_together: other.keep_together.or(self.keep_together),
            widow_orphan: other.widow_orphan.or(self.widow_orphan),
            drop_cap: other.drop_cap.or(self.drop_cap),
            tab_stops: other.tab_stops.clone().or_else(|| self.tab_stops.clone()),
            list: other.list.or(self.list),
        }
    }

    pub fn is_valid(&self) -> bool {
        let fin = |v: Option<f64>| v.is_none_or(|v| v.is_finite());
        let nonneg = |v: Option<f64>| v.is_none_or(|v| v.is_finite() && v >= 0.0);
        self.line_spacing
            .is_none_or(|v| v.is_finite() && (0.1..=10.0).contains(&v))
            && nonneg(self.space_before)
            && nonneg(self.space_after)
            && fin(self.first_line_indent)
            && nonneg(self.left_indent)
            && nonneg(self.right_indent)
            && self
                .language
                .as_deref()
                .is_none_or(|l| !l.trim().is_empty() && l.len() <= 35)
            && self.justify_limits.is_none_or(|j| j.is_valid())
            && self.widow_orphan.is_none_or(|n| n <= 10)
            && self.drop_cap.is_none_or(|d| d.is_valid())
            && self.tab_stops.as_ref().is_none_or(|t| {
                t.len() <= 64
                    && t.iter()
                        .all(|s| s.pos.is_finite() && (0.0..=10_000.0).contains(&s.pos))
            })
            && self.list.is_none_or(|l| l.is_valid())
    }
}

/// Horizontal paragraph alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

/// Fully resolved character formatting.
#[derive(Debug, Clone, PartialEq)]
pub struct CharAttrs {
    pub font_family: String,
    pub font_size: f64,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub color: Ink,
    pub letter_spacing: f64,
    pub baseline: Baseline,
    pub caps: Caps,
    pub strikethrough: bool,
    pub ligatures: bool,
    pub kerning: bool,
}

impl Default for CharAttrs {
    fn default() -> Self {
        Self {
            font_family: DEFAULT_FONT_FAMILY.to_string(),
            font_size: DEFAULT_FONT_SIZE,
            bold: false,
            italic: false,
            underline: false,
            color: Ink::Rgba(Color::BLACK),
            letter_spacing: 0.0,
            baseline: Baseline::Normal,
            caps: Caps::None,
            strikethrough: false,
            ligatures: true,
            kerning: true,
        }
    }
}

impl CharAttrs {
    fn apply(&mut self, p: &CharProps) {
        if let Some(v) = &p.font_family {
            self.font_family.clone_from(v);
        }
        if let Some(v) = p.font_size {
            self.font_size = v;
        }
        if let Some(v) = p.bold {
            self.bold = v;
        }
        if let Some(v) = p.italic {
            self.italic = v;
        }
        if let Some(v) = p.underline {
            self.underline = v;
        }
        if let Some(v) = p.color.as_ref().and_then(Paint::ink) {
            self.color = v;
        }
        if let Some(v) = p.letter_spacing {
            self.letter_spacing = v;
        }
        if let Some(v) = p.baseline {
            self.baseline = v;
        }
        if let Some(v) = p.caps {
            self.caps = v;
        }
        if let Some(v) = p.strikethrough {
            self.strikethrough = v;
        }
        if let Some(v) = p.ligatures {
            self.ligatures = v;
        }
        if let Some(v) = p.kerning {
            self.kerning = v;
        }
    }
}

/// Fully resolved paragraph formatting.
#[derive(Debug, Clone, PartialEq)]
pub struct ParaAttrs {
    pub align: Align,
    pub line_spacing: f64,
    pub space_before: f64,
    pub space_after: f64,
    pub first_line_indent: f64,
    pub left_indent: f64,
    pub right_indent: f64,
    pub hyphenate: bool,
    pub language: String,
    /// `None` = unbounded justification (v0.1 behaviour, Locked Decision 2).
    pub justify_limits: Option<JustifyLimits>,
    pub keep_with_next: bool,
    pub keep_together: bool,
    /// Fewest lines left alone at a column or frame boundary; 0 or 1 = off.
    pub widow_orphan: u8,
    /// `None` = no drop cap (also for `lines: 0`).
    pub drop_cap: Option<DropCap>,
    /// Explicit stops sorted by position; empty = default grid only.
    pub tab_stops: Vec<TabStop>,
    /// `None` = not a list (also for kind `none`).
    pub list: Option<List>,
}

impl Default for ParaAttrs {
    fn default() -> Self {
        Self {
            align: Align::Left,
            line_spacing: 1.2,
            space_before: 0.0,
            space_after: 0.0,
            first_line_indent: 0.0,
            left_indent: 0.0,
            right_indent: 0.0,
            hyphenate: false,
            language: "en-US".to_string(),
            justify_limits: None,
            keep_with_next: false,
            keep_together: false,
            widow_orphan: 0,
            drop_cap: None,
            tab_stops: Vec::new(),
            list: None,
        }
    }
}

impl ParaAttrs {
    fn apply(&mut self, p: &ParaProps) {
        if let Some(v) = p.align {
            self.align = v;
        }
        if let Some(v) = p.line_spacing {
            self.line_spacing = v;
        }
        if let Some(v) = p.space_before {
            self.space_before = v;
        }
        if let Some(v) = p.space_after {
            self.space_after = v;
        }
        if let Some(v) = p.first_line_indent {
            self.first_line_indent = v;
        }
        if let Some(v) = p.left_indent {
            self.left_indent = v;
        }
        if let Some(v) = p.right_indent {
            self.right_indent = v;
        }
        if let Some(v) = p.hyphenate {
            self.hyphenate = v;
        }
        if let Some(v) = &p.language {
            self.language = v.clone();
        }
        if let Some(v) = p.justify_limits {
            self.justify_limits = (!v.unbounded).then_some(v);
        }
        if let Some(v) = p.keep_with_next {
            self.keep_with_next = v;
        }
        if let Some(v) = p.keep_together {
            self.keep_together = v;
        }
        if let Some(v) = p.widow_orphan {
            self.widow_orphan = v;
        }
        if let Some(v) = p.drop_cap {
            self.drop_cap = (v.lines > 0).then_some(v);
        }
        if let Some(v) = &p.tab_stops {
            let mut stops = v.clone();
            stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
            self.tab_stops = stops;
        }
        if let Some(v) = p.list {
            self.list = (v.kind != ListKind::None).then_some(v);
        }
    }
}

/// Whether a style applies to paragraphs or to runs of characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum StyleKind {
    Paragraph,
    Character,
}

/// A named style.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Style {
    pub id: StyleId,
    /// Unique display name, e.g. "Heading 1".
    pub name: String,
    pub kind: StyleKind,
    /// Parent style of the same kind; unset fields inherit from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub based_on: Option<StyleId>,
    /// Paragraph formatting (paragraph styles only).
    #[serde(default)]
    pub paragraph: ParaProps,
    /// Character formatting.
    #[serde(default)]
    pub character: CharProps,
}

/// All styles in a document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct StyleSheet {
    /// The paragraph style used by paragraphs with no style.
    pub default_paragraph: StyleId,
    pub styles: Vec<Style>,
}

impl StyleSheet {
    /// "Normal", "Title", "Heading 1", "Heading 2", "Body", "Caption" and "Emphasis".
    pub fn with_defaults(ids: &mut dyn IdSource) -> Self {
        let normal = StyleId::new(ids);
        let para = |id, name: &str, based_on, paragraph, character| Style {
            id,
            name: name.to_string(),
            kind: StyleKind::Paragraph,
            based_on,
            paragraph,
            character,
        };
        let styles = vec![
            para(
                normal,
                "Normal",
                None,
                ParaProps {
                    align: Some(Align::Left),
                    line_spacing: Some(1.2),
                    space_before: Some(0.0),
                    space_after: Some(0.0),
                    first_line_indent: Some(0.0),
                    left_indent: Some(0.0),
                    right_indent: Some(0.0),
                    ..ParaProps::default()
                },
                CharProps {
                    font_family: Some(DEFAULT_FONT_FAMILY.into()),
                    font_size: Some(DEFAULT_FONT_SIZE),
                    bold: Some(false),
                    italic: Some(false),
                    underline: Some(false),
                    color: Some(Paint::Color(Color::BLACK)),
                    letter_spacing: Some(0.0),
                    ..CharProps::default()
                },
            ),
            para(
                StyleId::new(ids),
                "Title",
                Some(normal),
                ParaProps {
                    line_spacing: Some(1.05),
                    space_after: Some(12.0),
                    ..Default::default()
                },
                CharProps {
                    font_size: Some(36.0),
                    bold: Some(true),
                    ..Default::default()
                },
            ),
            para(
                StyleId::new(ids),
                "Heading 1",
                Some(normal),
                ParaProps {
                    space_before: Some(12.0),
                    space_after: Some(6.0),
                    line_spacing: Some(1.1),
                    ..Default::default()
                },
                CharProps {
                    font_size: Some(20.0),
                    bold: Some(true),
                    ..Default::default()
                },
            ),
            para(
                StyleId::new(ids),
                "Heading 2",
                Some(normal),
                ParaProps {
                    space_before: Some(9.0),
                    space_after: Some(4.0),
                    ..Default::default()
                },
                CharProps {
                    font_size: Some(14.0),
                    bold: Some(true),
                    ..Default::default()
                },
            ),
            para(
                StyleId::new(ids),
                "Body",
                Some(normal),
                ParaProps {
                    space_after: Some(6.0),
                    line_spacing: Some(1.25),
                    ..Default::default()
                },
                CharProps {
                    font_family: Some("Liberation Serif".into()),
                    font_size: Some(11.0),
                    ..Default::default()
                },
            ),
            para(
                StyleId::new(ids),
                "Caption",
                Some(normal),
                ParaProps::default(),
                CharProps {
                    font_size: Some(9.0),
                    italic: Some(true),
                    ..Default::default()
                },
            ),
            Style {
                id: StyleId::new(ids),
                name: "Emphasis".into(),
                kind: StyleKind::Character,
                based_on: None,
                paragraph: ParaProps::default(),
                character: CharProps {
                    italic: Some(true),
                    ..Default::default()
                },
            },
        ];
        Self {
            default_paragraph: normal,
            styles,
        }
    }

    pub fn get(&self, id: StyleId) -> Option<&Style> {
        self.styles.iter().find(|s| s.id == id)
    }

    pub fn get_mut(&mut self, id: StyleId) -> Option<&mut Style> {
        self.styles.iter_mut().find(|s| s.id == id)
    }

    pub fn by_name(&self, name: &str) -> Option<&Style> {
        self.styles
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
    }

    /// The style and its `based_on` ancestors, root first. Cycles are cut.
    pub fn chain(&self, id: StyleId) -> Vec<&Style> {
        let mut out = Vec::new();
        let mut cur = self.get(id);
        while let Some(s) = cur {
            if out.iter().any(|o: &&Style| o.id == s.id) || out.len() > 32 {
                break;
            }
            out.push(s);
            cur = s.based_on.and_then(|b| self.get(b));
        }
        out.reverse();
        out
    }

    /// Effective paragraph formatting.
    pub fn resolve_para(&self, para: &Paragraph) -> ParaAttrs {
        let mut a = ParaAttrs::default();
        for s in self.chain(self.default_paragraph) {
            a.apply(&s.paragraph);
        }
        if let Some(id) = para.style {
            for s in self.chain(id) {
                a.apply(&s.paragraph);
            }
        }
        a.apply(&para.props);
        a
    }

    /// Effective character formatting of a run in a paragraph.
    pub fn resolve_char(&self, para: &Paragraph, run: &Run) -> CharAttrs {
        let mut a = CharAttrs::default();
        for s in self.chain(self.default_paragraph) {
            a.apply(&s.character);
        }
        if let Some(id) = para.style {
            for s in self.chain(id) {
                a.apply(&s.character);
            }
        }
        if let Some(id) = run.style {
            for s in self.chain(id) {
                a.apply(&s.character);
            }
        }
        a.apply(&run.props);
        a
    }

    /// Style ids that would form a cycle if `id` were based on `parent`.
    pub fn would_cycle(&self, id: StyleId, parent: StyleId) -> bool {
        self.chain(parent).iter().any(|s| s.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::SeqIds;

    #[test]
    fn resolution_order_run_wins() {
        let sheet = StyleSheet::with_defaults(&mut SeqIds::new(3));
        let h1 = sheet.by_name("heading 1").unwrap().id;
        let emph = sheet.by_name("Emphasis").unwrap().id;
        let para = Paragraph {
            style: Some(h1),
            props: ParaProps::default(),
            runs: vec![],
        };
        let run = Run {
            text: "x".into(),
            style: Some(emph),
            props: CharProps {
                font_size: Some(30.0),
                ..Default::default()
            },
        };
        let a = sheet.resolve_char(&para, &run);
        assert!(a.bold, "from Heading 1");
        assert!(a.italic, "from Emphasis");
        assert_eq!(a.font_size, 30.0, "local override wins");
        assert_eq!(a.font_family, DEFAULT_FONT_FAMILY, "from Normal");
        let p = sheet.resolve_para(&para);
        assert_eq!(p.space_before, 12.0);
    }

    #[test]
    fn cycles_are_cut() {
        let mut sheet = StyleSheet::with_defaults(&mut SeqIds::new(3));
        let a = sheet.styles[1].id;
        let b = sheet.styles[2].id;
        sheet.styles[1].based_on = Some(b);
        sheet.styles[2].based_on = Some(a);
        assert!(sheet.chain(a).len() <= 2);
        assert!(sheet.would_cycle(a, b));
    }
}

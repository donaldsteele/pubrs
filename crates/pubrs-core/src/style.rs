//! Paragraph and character styles, and resolution of effective formatting.
//!
//! Effective character formatting is resolved in this order (later wins):
//! built-in defaults, the default paragraph style chain, the paragraph's
//! style chain, the run's character style chain, the run's local props.

use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::id::{IdSource, StyleId};
use crate::model::{Paragraph, Run};

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
    pub color: Option<Color>,
    /// Extra space between letters in points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub letter_spacing: Option<f64>,
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
            color: other.color.or(self.color),
            letter_spacing: other.letter_spacing.or(self.letter_spacing),
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == CharProps::default()
    }

    pub fn is_valid(&self) -> bool {
        self.font_size
            .is_none_or(|s| s.is_finite() && s > 0.0 && s <= 1296.0)
            && self.letter_spacing.is_none_or(|s| s.is_finite())
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
    pub color: Color,
    pub letter_spacing: f64,
}

impl Default for CharAttrs {
    fn default() -> Self {
        Self {
            font_family: DEFAULT_FONT_FAMILY.to_string(),
            font_size: DEFAULT_FONT_SIZE,
            bold: false,
            italic: false,
            underline: false,
            color: Color::BLACK,
            letter_spacing: 0.0,
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
        if let Some(v) = p.color {
            self.color = v;
        }
        if let Some(v) = p.letter_spacing {
            self.letter_spacing = v;
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
                },
                CharProps {
                    font_family: Some(DEFAULT_FONT_FAMILY.into()),
                    font_size: Some(DEFAULT_FONT_SIZE),
                    bold: Some(false),
                    italic: Some(false),
                    underline: Some(false),
                    color: Some(Color::BLACK),
                    letter_spacing: Some(0.0),
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

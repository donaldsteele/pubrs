//! Templates: ordinary documents whose frames carry placeholder metadata.
//!
//! Built-in templates are constructed here through commands with
//! deterministic ids; user templates are `.pubrs` files in a folder.

use std::path::{Path, PathBuf};

use pubrs_core::style::{List, ListKind, TabAlign, TabLeader, TabStop};
use pubrs_core::table::CellStyle;
use pubrs_core::{
    Align, Axis, CharProps, Color, Command, Dash, DocBuilder, Document, FrameId, Guide, GuideId,
    IdSource, JustifyLimits, Margins, MasterId, MasterPage, Owner, PageId, PageSetup,
    PageSizePreset, Paint, ParaProps, Paragraph, Parent, Placeholder, PlaceholderKind, Rect, Run,
    SchemeSlot, SeqIds, Shape, Size, Stroke, StyleId, TextWrap, VerticalAlign, WrapMode,
};
use serde::{Deserialize, Serialize};

use crate::error::IoError;

/// One placeholder of a template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlaceholderInfo {
    pub name: String,
    pub kind: PlaceholderKind,
    /// Sample text / what to put there.
    pub hint: String,
    /// 0-based page index.
    pub page_index: usize,
    /// Frame size in points (for choosing text length and image aspect).
    pub width: f64,
    pub height: f64,
}

/// Template metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TemplateInfo {
    /// Identifier used with template_apply.
    pub name: String,
    pub title: String,
    pub description: String,
    pub builtin: bool,
    /// Trim size in points.
    pub page_size: Size,
    pub page_count: usize,
    pub placeholders: Vec<PlaceholderInfo>,
    /// Named frames that are not placeholders (e.g. continuation frames).
    pub named_frames: Vec<String>,
}

/// Built-in template names.
pub const BUILTIN: [&str; 13] = [
    "flyer",
    "trifold_brochure",
    "business_card",
    "newsletter",
    "greeting_card",
    "postcard",
    "certificate",
    "menu",
    "letterhead",
    "envelope",
    "calendar",
    "address_labels",
    "business_cards",
];

/// The year the `calendar` template shows unless asked otherwise. A constant,
/// so thumbnails, goldens and ids are stable.
pub const DEFAULT_CALENDAR_YEAR: i32 = 2027;

/// Years the `calendar` template accepts.
pub const CALENDAR_YEARS: std::ops::RangeInclusive<i32> = 1600..=9999;

/// Options for building a built-in template.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TemplateOptions {
    /// Calendar year (`calendar` only); default [`DEFAULT_CALENDAR_YEAR`].
    pub year: Option<i32>,
}

/// The colour scheme each built-in template starts with.
fn scheme_of(name: &str) -> &'static str {
    match name {
        "newsletter" => "juniper_slate",
        "greeting_card" => "plum_orchard",
        "postcard" => "coral_reef",
        "certificate" => "copper_dawn",
        "menu" => "saffron_market",
        "letterhead" => "slate_and_ember",
        "envelope" => "glacier_mint",
        "calendar" => "meadowlark",
        "address_labels" => "orchard_frost",
        "business_cards" => "midnight_orchid",
        _ => pubrs_core::swatch::DEFAULT_SCHEME,
    }
}

// Brand colours are colour-scheme slots, so applying a scheme recolours a
// template. The default scheme's slot values are the colours these were
// designed with (NAVY #1E3A5F, ORANGE #C2410C, CREAM #FFF7ED).
const NAVY: Paint = SchemeSlot::Main.paint();
const ORANGE: Paint = SchemeSlot::Accent1.paint();
const CREAM: Paint = SchemeSlot::Accent2.paint();
const INK: Color = Color::rgb(0x1F, 0x29, 0x37);
const WHITE: Color = Color::WHITE;
const SANS: &str = "Liberation Sans";
const SERIF: &str = "Liberation Serif";

fn title_of(name: &str) -> &'static str {
    match name {
        "flyer" => "Flyer",
        "trifold_brochure" => "Trifold brochure",
        "business_card" => "Business card",
        "newsletter" => "Newsletter",
        "greeting_card" => "Greeting card",
        "postcard" => "Postcard",
        "certificate" => "Certificate",
        "menu" => "Restaurant menu",
        "letterhead" => "Letterhead",
        "envelope" => "#10 envelope",
        "calendar" => "Annual calendar",
        "address_labels" => "Address labels",
        "business_cards" => "Business cards (sheet)",
        _ => "Template",
    }
}

fn description_of(name: &str) -> &'static str {
    match name {
        "flyer" => {
            "Letter portrait flyer with a colour header, headline, hero photo, body text, call-to-action box and contact line. 9pt bleed."
        }
        "trifold_brochure" => {
            "Letter landscape, two sides, three panels each (folds at 264pt and 528pt). Outside: inside flap, back panel with contact details, front cover with photo. Inside: story across panels (link inside_body to inside_body_continued), photo, features list. 9pt bleed."
        }
        "business_card" => {
            "3.5 x 2 in business card with logo strip, name, job title, company and contact lines. 9pt bleed."
        }
        "newsletter" => {
            "Four-page Letter newsletter: boxed masthead, lead story flowing through two columns onto page 2, photos with captions, sidebar, pull quote, an events table, bulleted and numbered lists, an opening-hours table and a back-page mailing panel."
        }
        "greeting_card" => {
            "Quarter-fold greeting card on one Letter portrait page with fold guides at the centre lines. Bottom half: back (left) and front (right). Top half, rotated 180 degrees: the inside spread. Picture and text placeholders."
        }
        "postcard" => {
            "6 x 4 in landscape postcard, two pages: a full-bleed picture front with a caption band, and a back with a message area, address block and stamp box. 9pt bleed."
        }
        "certificate" => {
            "Letter landscape certificate with a BorderArt border, title, recipient, reason, date and signature lines and a seal or logo picture."
        }
        "menu" => {
            "Two-page Letter restaurant menu: sections with prices on a right tab with dot leaders, a bulleted list of specials and a drinks list."
        }
        "letterhead" => {
            "Letter letterhead: logo, organisation name and address, a thin rule, an empty body area for the letter and a footer."
        }
        "envelope" => {
            "#10 envelope (9.5 x 4.125 in) with a return address and logo at top left, a stamp box and the recipient block just right of centre."
        }
        "calendar" => {
            "Twelve Letter landscape pages, one per month: a picture, the month and year, and a Sunday-first table of days with the neighbouring months' days in grey. Set the year with the year option."
        }
        "address_labels" => {
            "Avery 5160 address labels (30 per sheet, 2.625 x 1 in): one label whose address block is merge fields (Name, Address, City, State, Zip): load a list with merge_set_source and merge to put one record on each label."
        }
        "business_cards" => {
            "Avery 5371 business cards (10 per sheet, 3.5 x 2 in): name, job title, company, contact lines and a logo, repeated down the sheet."
        }
        _ => "",
    }
}

/// Builder helpers shared by the templates.
struct T {
    b: DocBuilder<SeqIds>,
}

/// Character formatting that overrides nothing.
fn ov() -> CharProps {
    CharProps::default()
}

fn props(family: &str, size: f64, bold: bool, italic: bool, color: impl Into<Paint>) -> CharProps {
    CharProps {
        font_family: Some(family.into()),
        font_size: Some(size),
        bold: Some(bold),
        italic: Some(italic),
        color: Some(color.into()),
        ..Default::default()
    }
}

fn para(align: Align, line_spacing: f64, space_after: f64) -> ParaProps {
    ParaProps {
        align: Some(align),
        line_spacing: Some(line_spacing),
        space_after: Some(space_after),
        ..Default::default()
    }
}

/// How `T::table` looks.
struct TableLook {
    header: bool,
    header_fill: Paint,
    band: bool,
    band_fill: Paint,
    border: Paint,
    size: f64,
    valign: VerticalAlign,
}

impl TableLook {
    fn standard() -> Self {
        Self {
            header: true,
            header_fill: NAVY,
            band: true,
            band_fill: CREAM,
            border: NAVY,
            size: 10.0,
            valign: VerticalAlign::Middle,
        }
    }
}

impl T {
    fn new(seed: u64, setup: PageSetup, title: &str, template: &str) -> Self {
        let mut b = DocBuilder::new(SeqIds::new(seed), setup);
        b.exec(Command::SetMeta {
            title: Some(title.into()),
            author: None,
        })
        .expect("meta");
        let mut t = Self { b };
        t.b.editor.reset({
            let mut d = t.b.doc().clone();
            d.meta.template = Some(template.into());
            // The template's scheme slot swatches, with their fixed ids.
            pubrs_core::swatch::apply_scheme(
                &mut d,
                pubrs_core::swatch::scheme(scheme_of(template)).expect("scheme"),
            );
            d
        });
        t
    }

    fn rect(&mut self, page: PageId, r: Rect, fill: impl Into<Paint>) -> FrameId {
        let f = self
            .b
            .shape(page, r, Shape::Rectangle { corner_radius: 0.0 })
            .expect("shape");
        self.b
            .style(
                f,
                pubrs_core::FrameStyle {
                    fill: Some(fill.into()),
                    stroke: None,
                    opacity: 1.0,
                    ..Default::default()
                },
            )
            .expect("style");
        f
    }

    fn line(&mut self, owner: Owner, r: Rect, color: impl Into<Paint>, width: f64) -> FrameId {
        let id = FrameId::new(&mut self.b.ids);
        let mut f = pubrs_core::Frame::new(
            id,
            r,
            pubrs_core::FrameContent::Shape(pubrs_core::ShapeFrame {
                shape: Shape::Line {
                    direction: Default::default(),
                },
            }),
        );
        f.style.stroke = Some(Stroke {
            color: color.into(),
            width,
            dash: Default::default(),
        });
        self.b.frame(owner.into(), f, None).expect("line")
    }

    /// A text frame; with `ph` it becomes a placeholder whose hint is the text.
    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        page: PageId,
        r: Rect,
        text: &str,
        chars: CharProps,
        para: ParaProps,
        ph: Option<&str>,
        valign: VerticalAlign,
    ) -> FrameId {
        self.text_s(page, r, text, None, chars, para, ph, valign)
    }

    /// The id of a named paragraph style (see `styles`).
    fn style_id(&self, name: &str) -> StyleId {
        self.b
            .doc()
            .styles
            .styles
            .iter()
            .find(|s| s.name == name)
            .map(|s| s.id)
            .expect("style")
    }

    /// Like `text`, with a named paragraph style; `chars` and `para` override it.
    #[allow(clippy::too_many_arguments)]
    fn text_s(
        &mut self,
        page: PageId,
        r: Rect,
        text: &str,
        style: Option<&str>,
        chars: CharProps,
        para: ParaProps,
        ph: Option<&str>,
        valign: VerticalAlign,
    ) -> FrameId {
        let style = style.map(|n| self.style_id(n));
        let paragraphs: Vec<Paragraph> = text
            .split('\n')
            .map(|line| Paragraph {
                style,
                props: para.clone(),
                runs: vec![Run {
                    text: line.into(),
                    style: None,
                    props: chars.clone(),
                    field: None,
                }],
            })
            .collect();
        let f = self
            .b
            .text_paragraphs(Parent::Page(page), r, paragraphs)
            .expect("text");
        if let Some(name) = ph {
            self.b
                .exec(Command::SetPlaceholder {
                    frame: f,
                    placeholder: Some(Placeholder {
                        name: name.into(),
                        kind: PlaceholderKind::Text,
                        hint: text.into(),
                    }),
                })
                .expect("placeholder");
        }
        self.b
            .exec(Command::SetTextFrame {
                frame: f,
                props: pubrs_core::TextFrameProps {
                    insets: Some(Margins::uniform(2.0)),
                    vertical_align: Some(valign),
                    ..Default::default()
                },
            })
            .expect("text frame options");
        f
    }

    fn image(&mut self, page: PageId, r: Rect, ph: &str, hint: &str) -> FrameId {
        let f = self.b.image(page, r, None).expect("image");
        self.b
            .exec(Command::SetPlaceholder {
                frame: f,
                placeholder: Some(Placeholder {
                    name: ph.into(),
                    kind: PlaceholderKind::Image,
                    hint: hint.into(),
                }),
            })
            .expect("placeholder");
        self.b
            .wrap(
                f,
                TextWrap {
                    mode: WrapMode::None,
                    margin: 0.0,
                    both_sides: false,
                },
            )
            .expect("wrap");
        f
    }

    /// Give a placeholder a descriptive hint, so its sample text counts as
    /// usable content (e.g. a section heading such as "About Us").
    fn describe(&mut self, frame: FrameId, hint: &str) {
        let mut p = self
            .b
            .doc()
            .frame(frame)
            .and_then(|f| f.placeholder.clone())
            .expect("placeholder");
        p.hint = hint.into();
        self.b
            .exec(Command::SetPlaceholder {
                frame,
                placeholder: Some(p),
            })
            .expect("hint");
    }

    /// Set up the named styles every text-heavy template shares, by updating
    /// the document's default "Body", "Heading 1", "Heading 2" and "Caption".
    /// Body enables justification limits (TODO: M14) and widow/orphan control (M15).
    fn styles(&mut self) {
        let edit = |t: &mut Self, name: &str, f: &dyn Fn(&mut pubrs_core::Style)| {
            let mut st =
                t.b.doc()
                    .styles
                    .styles
                    .iter()
                    .find(|s| s.name == name)
                    .cloned()
                    .expect("default style");
            f(&mut st);
            t.b.exec(Command::UpdateStyle { style: st })
                .expect("update style");
        };
        edit(self, "Body", &|s| {
            s.paragraph.line_spacing = Some(1.3);
            s.paragraph.space_after = Some(6.0);
            s.paragraph.justify_limits = Some(JustifyLimits::default());
            s.paragraph.widow_orphan = Some(2);
            s.character.font_family = Some(SERIF.into());
            s.character.font_size = Some(11.0);
            s.character.color = Some(INK.into());
        });
        edit(self, "Heading 1", &|s| {
            s.paragraph.keep_with_next = Some(true);
            s.paragraph.widow_orphan = Some(2);
            s.character.font_family = Some(SANS.into());
            s.character.color = Some(NAVY);
        });
        edit(self, "Heading 2", &|s| {
            s.paragraph.keep_with_next = Some(true);
            s.paragraph.widow_orphan = Some(2);
            s.character.font_family = Some(SANS.into());
            s.character.color = Some(NAVY);
        });
        edit(self, "Caption", &|s| {
            s.paragraph.widow_orphan = Some(2);
            s.character.font_family = Some(SANS.into());
            s.character.color = Some(INK.into());
        });
    }

    /// A rectangle with an outline and no fill.
    fn outline(&mut self, page: PageId, r: Rect, color: impl Into<Paint>, width: f64) -> FrameId {
        let f = self
            .b
            .shape(page, r, Shape::Rectangle { corner_radius: 0.0 })
            .expect("shape");
        self.b
            .style(
                f,
                pubrs_core::FrameStyle {
                    stroke: Some(Stroke {
                        color: color.into(),
                        width,
                        dash: Dash::Solid,
                    }),
                    ..Default::default()
                },
            )
            .expect("style");
        f
    }

    /// A table whose cells are all filled: `cells[row][col]`, column widths and
    /// row heights in points. The first row is a header when `look.header` is set.
    fn table(
        &mut self,
        page: PageId,
        origin: (f64, f64),
        cols: &[f64],
        rows: &[f64],
        cells: &[Vec<&str>],
        look: &TableLook,
    ) -> FrameId {
        let rect = Rect::new(
            origin.0,
            origin.1,
            cols.iter().sum::<f64>(),
            rows.iter().sum::<f64>(),
        );
        let border = Some(Stroke {
            color: look.border.clone(),
            width: 0.5,
            dash: Dash::Solid,
        });
        let base = CellStyle {
            fill: None,
            borders: [border.clone(), border.clone(), border.clone(), border],
            insets: Margins::uniform(4.0),
            valign: look.valign,
        };
        let table = self
            .b
            .table(Parent::Page(page), rect, rows.len(), cols.len(), &base)
            .expect("table");
        for (c, w) in cols.iter().enumerate() {
            self.b
                .exec(Command::TableSetColWidth {
                    frame: table,
                    col: c as u32,
                    width: *w,
                })
                .expect("column");
        }
        for (r, h) in rows.iter().enumerate() {
            self.b
                .exec(Command::TableSetRowHeight {
                    frame: table,
                    row: r as u32,
                    min_height: *h,
                })
                .expect("row");
        }
        for (r, row) in cells.iter().enumerate() {
            for (c, text) in row.iter().enumerate() {
                let (r32, c32) = (r as u32, c as u32);
                let head = look.header && r == 0;
                self.b.cell_text(table, r32, c32, text).expect("cell");
                let story = self
                    .b
                    .doc()
                    .frame(table)
                    .and_then(|f| f.as_table())
                    .and_then(|t| t.cell_at(r32, c32))
                    .map(|c| c.story)
                    .expect("cell story");
                let chars = if head {
                    props(SANS, look.size, true, false, WHITE)
                } else {
                    props(SANS, look.size, false, false, INK)
                };
                self.b
                    .exec(Command::ApplyCharProps {
                        story,
                        start: 0,
                        end: text.chars().count(),
                        props: chars,
                    })
                    .expect("cell text");
                let fill = if head {
                    Some(look.header_fill.clone())
                } else if look.band && r % 2 == 0 {
                    Some(look.band_fill.clone())
                } else {
                    None
                };
                if fill.is_some() {
                    let mut st = base.clone();
                    st.fill = fill;
                    self.b
                        .exec(Command::TableSetCellStyle {
                            frame: table,
                            row: r32,
                            col: c32,
                            style: st,
                        })
                        .expect("cell style");
                }
            }
        }
        table
    }

    /// Recolour one table cell's text (calendar days outside the month).
    fn cell_chars(&mut self, table: FrameId, row: u32, col: u32, chars: CharProps) {
        let story = self
            .b
            .doc()
            .frame(table)
            .and_then(|f| f.as_table())
            .and_then(|t| t.cell_at(row, col))
            .map(|c| c.story)
            .expect("cell story");
        let len = self.b.doc().story(story).map_or(0, |s| s.char_len());
        self.b
            .exec(Command::ApplyCharProps {
                story,
                start: 0,
                end: len,
                props: chars,
            })
            .expect("cell chars");
    }

    fn rotate(&mut self, frame: FrameId, degrees: f64) {
        self.b
            .exec(Command::SetRotation { frame, degrees })
            .expect("rotation");
    }

    fn name(&mut self, frame: FrameId, name: &str) {
        self.b
            .exec(Command::SetFrameName {
                frame,
                name: Some(name.into()),
            })
            .expect("name");
    }

    fn guide(&mut self, page: PageId, axis: Axis, position: f64) {
        let id = GuideId::new(&mut self.b.ids);
        self.b
            .exec(Command::AddGuide {
                owner: Owner::Page(page),
                guide: Guide { id, axis, position },
            })
            .expect("guide");
    }

    fn finish(self) -> Document {
        let mut ed = self.b.editor;
        ed.mark_saved();
        ed.doc().clone()
    }
}

fn flyer() -> Document {
    let mut t = T::new(
        101,
        PageSetup {
            size: PageSizePreset::Letter.size(),
            margins: Margins::uniform(36.0),
            bleed: 9.0,
            ..PageSetup::default()
        },
        "Flyer",
        "flyer",
    );
    let p = t.b.page(0);
    t.rect(p, Rect::new(-9.0, -9.0, 630.0, 219.0), NAVY);
    t.rect(p, Rect::new(-9.0, 210.0, 630.0, 6.0), ORANGE);
    t.text(
        p,
        Rect::new(36.0, 40.0, 540.0, 100.0),
        "Your Headline Here",
        props(SANS, 44.0, true, false, WHITE),
        para(Align::Left, 1.05, 0.0),
        Some("headline"),
        VerticalAlign::Bottom,
    );
    t.text(
        p,
        Rect::new(36.0, 144.0, 540.0, 50.0),
        "A short line that says why this matters",
        props(SANS, 18.0, false, false, CREAM),
        para(Align::Left, 1.2, 0.0),
        Some("subheadline"),
        VerticalAlign::Top,
    );
    t.image(
        p,
        Rect::new(36.0, 240.0, 540.0, 300.0),
        "hero_image",
        "Main photo, landscape (about 9:5)",
    );
    t.text(
        p,
        Rect::new(36.0, 560.0, 340.0, 170.0),
        "Tell readers what is happening, when and where, and what they will get out of it. Two or three short paragraphs work best.",
        props(SERIF, 13.0, false, false, INK),
        para(Align::Left, 1.3, 8.0),
        Some("body"),
        VerticalAlign::Top,
    );
    let cta = t.text(
        p,
        Rect::new(396.0, 560.0, 180.0, 120.0),
        "Call to action!",
        props(SANS, 20.0, true, false, WHITE),
        para(Align::Center, 1.15, 0.0),
        Some("call_to_action"),
        VerticalAlign::Middle,
    );
    t.b.style(
        cta,
        pubrs_core::FrameStyle {
            fill: Some(ORANGE),
            stroke: None,
            opacity: 1.0,
            ..Default::default()
        },
    )
    .expect("cta");
    t.text(
        p,
        Rect::new(36.0, 740.0, 540.0, 28.0),
        "Address · Phone · Website",
        props(SANS, 11.0, false, false, NAVY),
        para(Align::Center, 1.2, 0.0),
        Some("contact"),
        VerticalAlign::Middle,
    );
    t.finish()
}

fn trifold() -> Document {
    let mut t = T::new(
        102,
        PageSetup {
            size: Size::new(792.0, 612.0),
            margins: Margins::uniform(18.0),
            bleed: 9.0,
            ..PageSetup::default()
        },
        "Trifold brochure",
        "trifold_brochure",
    );
    let outside = t.b.page(0);
    let inside = t.b.add_page().expect("page");
    for page in [outside, inside] {
        t.guide(page, Axis::Vertical, 264.0);
        t.guide(page, Axis::Vertical, 528.0);
    }
    // ---- outside: [inside flap | back panel | front cover]
    t.rect(outside, Rect::new(-9.0, -9.0, 273.0, 630.0), CREAM);
    t.text(
        outside,
        Rect::new(22.0, 36.0, 220.0, 60.0),
        "About Us",
        props(SANS, 22.0, true, false, NAVY),
        para(Align::Left, 1.1, 0.0),
        Some("flap_heading"),
        VerticalAlign::Bottom,
    );
    t.line(
        Owner::Page(outside),
        Rect::new(22.0, 104.0, 60.0, 0.0),
        ORANGE,
        3.0,
    );
    t.text(
        outside,
        Rect::new(22.0, 118.0, 220.0, 450.0),
        "Introduce the business: who you are, how long you have been doing this and what makes you different. Keep it warm and personal.",
        props(SERIF, 12.0, false, false, INK),
        para(Align::Left, 1.35, 8.0),
        Some("flap_text"),
        VerticalAlign::Top,
    );
    t.rect(outside, Rect::new(264.0, -9.0, 264.0, 630.0), NAVY);
    t.text(
        outside,
        Rect::new(286.0, 300.0, 220.0, 50.0),
        "Visit Us",
        props(SANS, 22.0, true, false, WHITE),
        para(Align::Center, 1.1, 0.0),
        Some("contact_heading"),
        VerticalAlign::Bottom,
    );
    t.text(
        outside,
        Rect::new(286.0, 360.0, 220.0, 200.0),
        "Street address\nTown, postcode\nPhone number\nOpening hours\nwebsite.example",
        props(SANS, 12.0, false, false, CREAM),
        para(Align::Center, 1.5, 0.0),
        Some("contact"),
        VerticalAlign::Top,
    );
    t.image(
        outside,
        Rect::new(528.0, -9.0, 273.0, 345.0),
        "cover_image",
        "Cover photo, portrait-ish (about 4:5)",
    );
    t.rect(outside, Rect::new(528.0, 336.0, 273.0, 6.0), ORANGE);
    t.text(
        outside,
        Rect::new(546.0, 356.0, 228.0, 120.0),
        "Business Name",
        props(SANS, 30.0, true, false, NAVY),
        para(Align::Center, 1.05, 0.0),
        Some("cover_title"),
        VerticalAlign::Middle,
    );
    t.text(
        outside,
        Rect::new(546.0, 480.0, 228.0, 100.0),
        "A one-line promise to your customers",
        props(SERIF, 15.0, false, true, ORANGE),
        para(Align::Center, 1.25, 0.0),
        Some("cover_subtitle"),
        VerticalAlign::Top,
    );
    // ---- inside: [story | continued story + photo | features]
    t.text(
        inside,
        Rect::new(22.0, 30.0, 220.0, 80.0),
        "Our Story",
        props(SANS, 24.0, true, false, NAVY),
        para(Align::Left, 1.05, 0.0),
        Some("inside_heading"),
        VerticalAlign::Bottom,
    );
    t.line(
        Owner::Page(inside),
        Rect::new(22.0, 118.0, 60.0, 0.0),
        ORANGE,
        3.0,
    );
    t.text(
        inside,
        Rect::new(22.0, 132.0, 220.0, 450.0),
        "Write the main story here. If it is longer than this panel, link this frame to the frame at the top of the middle panel (inside_body_continued) and the text will flow on.",
        props(SERIF, 12.0, false, false, INK),
        para(Align::Left, 1.35, 8.0),
        Some("inside_body"),
        VerticalAlign::Top,
    );
    let cont =
        t.b.text_paragraphs(
            Parent::Page(inside),
            Rect::new(286.0, 30.0, 220.0, 280.0),
            vec![Paragraph::default()],
        )
        .expect("continued");
    t.name(cont, "inside_body_continued");
    t.image(
        inside,
        Rect::new(286.0, 330.0, 220.0, 250.0),
        "inside_image",
        "Second photo, roughly square",
    );
    t.rect(inside, Rect::new(528.0, -9.0, 273.0, 630.0), CREAM);
    t.text(
        inside,
        Rect::new(550.0, 30.0, 220.0, 80.0),
        "What We Offer",
        props(SANS, 24.0, true, false, NAVY),
        para(Align::Left, 1.05, 0.0),
        Some("features_heading"),
        VerticalAlign::Bottom,
    );
    t.line(
        Owner::Page(inside),
        Rect::new(550.0, 118.0, 60.0, 0.0),
        ORANGE,
        3.0,
    );
    t.text(
        inside,
        Rect::new(550.0, 132.0, 220.0, 450.0),
        "First product or service\nSecond product or service\nThird product or service\nFourth product or service",
        props(SANS, 13.0, false, false, INK),
        para(Align::Left, 1.3, 12.0),
        Some("features"),
        VerticalAlign::Top,
    );
    for (name, hint) in [
        ("flap_heading", "Heading for the flap panel"),
        ("contact_heading", "Heading for the contact panel"),
        ("inside_heading", "Heading for the main story"),
        (
            "features_heading",
            "Heading for the list of products or services",
        ),
    ] {
        let f = t.b.doc().placeholder(name).expect("placeholder");
        t.describe(f, hint);
    }
    t.finish()
}

fn business_card() -> Document {
    let mut t = T::new(
        103,
        PageSetup {
            size: PageSizePreset::BusinessCard.size(),
            margins: Margins::uniform(12.0),
            bleed: 9.0,
            ..PageSetup::default()
        },
        "Business card",
        "business_card",
    );
    let p = t.b.page(0);
    t.rect(p, Rect::new(-9.0, -9.0, 89.0, 162.0), NAVY);
    t.image(
        p,
        Rect::new(10.0, 42.0, 60.0, 60.0),
        "logo",
        "Square logo (SVG works well)",
    );
    t.text(
        p,
        Rect::new(92.0, 16.0, 150.0, 26.0),
        "Your Name",
        props(SANS, 15.0, true, false, NAVY),
        para(Align::Left, 1.1, 0.0),
        Some("name"),
        VerticalAlign::Bottom,
    );
    t.text(
        p,
        Rect::new(92.0, 42.0, 150.0, 16.0),
        "Job Title",
        props(SERIF, 9.5, false, true, ORANGE),
        para(Align::Left, 1.1, 0.0),
        Some("job_title"),
        VerticalAlign::Top,
    );
    t.text(
        p,
        Rect::new(92.0, 60.0, 150.0, 18.0),
        "Company Name",
        props(SANS, 10.0, true, false, INK),
        para(Align::Left, 1.1, 0.0),
        Some("company"),
        VerticalAlign::Top,
    );
    t.line(
        Owner::Page(p),
        Rect::new(96.0, 82.0, 40.0, 0.0),
        ORANGE,
        1.5,
    );
    t.text(
        p,
        Rect::new(92.0, 86.0, 150.0, 50.0),
        "phone\nemail@example.com\nwebsite.example",
        props(SANS, 8.0, false, false, INK),
        para(Align::Left, 1.3, 0.0),
        Some("contact"),
        VerticalAlign::Top,
    );
    t.finish()
}

// ---------------------------------------------------------------------------
// Everyday templates (M30)

const GREY: Color = Color::rgb(0x76, 0x76, 0x76);
const CENTER: Align = Align::Center;

fn setup(size: Size, margin: f64, bleed: f64) -> PageSetup {
    PageSetup {
        size,
        margins: Margins::uniform(margin),
        bleed,
        ..PageSetup::default()
    }
}

fn letter() -> Size {
    PageSizePreset::Letter.size()
}

fn letter_landscape() -> Size {
    let l = letter();
    Size::new(l.height, l.width)
}

/// Character overrides: size only.
fn sz(size: f64) -> CharProps {
    CharProps {
        font_size: Some(size),
        ..Default::default()
    }
}

/// Character overrides: size and colour.
fn szc(size: f64, color: impl Into<Paint>) -> CharProps {
    CharProps {
        font_size: Some(size),
        color: Some(color.into()),
        ..Default::default()
    }
}

/// Paragraph overrides: alignment only.
fn al(align: Align) -> ParaProps {
    ParaProps {
        align: Some(align),
        ..Default::default()
    }
}

/// A bulleted or numbered list paragraph.
fn list_para(kind: ListKind) -> ParaProps {
    ParaProps {
        align: Some(Align::Left),
        space_after: Some(3.0),
        list: Some(List {
            kind,
            bullet: '\u{2022}',
            format: Default::default(),
            level: 0,
            indent: 16.0,
            start: None,
        }),
        ..Default::default()
    }
}

/// Menu lines: text, a tab, then a price on a right tab with dot leaders.
fn menu_para(tab: f64) -> ParaProps {
    ParaProps {
        align: Some(Align::Left),
        space_after: Some(5.0),
        tab_stops: Some(vec![TabStop {
            pos: tab,
            align: TabAlign::Right,
            leader: TabLeader::Dot,
        }]),
        ..Default::default()
    }
}

fn italic(size: f64, family: &str, color: impl Into<Paint>) -> CharProps {
    props(family, size, false, true, color)
}

const CONTENT: VerticalAlign = VerticalAlign::Top;

fn sheet_setup(preset: &str, margin: f64, fill: pubrs_core::SheetFill) -> PageSetup {
    let layout = crate::sheet_preset(preset)
        .expect("sheet preset")
        .layout(fill);
    PageSetup {
        size: layout.label_size,
        margins: Margins::uniform(margin),
        bleed: 0.0,
        sheet: Some(layout),
        ..PageSetup::default()
    }
}

/// Quarter-fold card on one Letter portrait page. Fold guides cross at
/// (306, 396). Bottom half: back (left) and front (right), upright. Top
/// half: the inside spread, designed upright in a 612 x 396 area and then
/// turned half a revolution about the centre of the top half, so the left
/// inside page lands top right and the right inside page top left; every
/// top-half frame is rotated 180 degrees.
fn greeting_card() -> Document {
    let mut t = T::new(
        105,
        setup(letter(), 18.0, 0.0),
        "Greeting card",
        "greeting_card",
    );
    t.styles();
    let p = t.b.page(0);
    t.guide(p, Axis::Vertical, 306.0);
    t.guide(p, Axis::Horizontal, 396.0);
    // Back panel (bottom left).
    t.line(
        Owner::Page(p),
        Rect::new(126.0, 690.0, 54.0, 0.0),
        ORANGE,
        1.5,
    );
    t.text(
        p,
        Rect::new(50.0, 700.0, 206.0, 50.0),
        "Made with love by\nYour name",
        props(SANS, 9.0, false, false, INK),
        para(CENTER, 1.3, 0.0),
        Some("back_credit"),
        VerticalAlign::Top,
    );
    // Front panel (bottom right).
    t.rect(p, Rect::new(330.0, 420.0, 252.0, 348.0), NAVY);
    t.image(
        p,
        Rect::new(342.0, 432.0, 228.0, 190.0),
        "front_picture",
        "Cover picture, landscape (about 6:5)",
    );
    t.text_s(
        p,
        Rect::new(342.0, 632.0, 228.0, 64.0),
        "Happy Birthday!",
        Some("Heading 1"),
        szc(24.0, WHITE),
        al(CENTER),
        Some("front_greeting"),
        VerticalAlign::Middle,
    );
    t.line(
        Owner::Page(p),
        Rect::new(396.0, 706.0, 120.0, 0.0),
        ORANGE,
        2.0,
    );
    t.text(
        p,
        Rect::new(342.0, 716.0, 228.0, 40.0),
        "Wishing you a wonderful day",
        italic(11.0, SERIF, CREAM),
        para(CENTER, 1.2, 0.0),
        Some("front_subtitle"),
        VerticalAlign::Middle,
    );
    // Inside spread, designed upright then turned.
    let turn = |r: Rect| {
        Rect::new(
            612.0 - r.x - r.width,
            396.0 - r.y - r.height,
            r.width,
            r.height,
        )
    };
    let f = t.image(
        p,
        turn(Rect::new(36.0, 36.0, 234.0, 230.0)),
        "inside_picture",
        "Inside picture, about 1:1",
    );
    t.rotate(f, 180.0);
    let f = t.text_s(
        p,
        turn(Rect::new(36.0, 274.0, 234.0, 44.0)),
        "A few words about the picture",
        Some("Caption"),
        sz(10.0),
        al(CENTER),
        Some("inside_caption"),
        CONTENT,
    );
    t.rotate(f, 180.0);
    let f = t.text_s(
        p,
        turn(Rect::new(342.0, 48.0, 234.0, 220.0)),
        "Write your message here. Keep it short and warm.",
        Some("Body"),
        sz(15.0),
        al(CENTER),
        Some("inside_message"),
        VerticalAlign::Middle,
    );
    t.rotate(f, 180.0);
    let f = t.text(
        p,
        turn(Rect::new(342.0, 290.0, 234.0, 56.0)),
        "With love,\nYour name",
        italic(14.0, SERIF, NAVY),
        para(CENTER, 1.25, 0.0),
        Some("inside_signature"),
        CONTENT,
    );
    t.rotate(f, 180.0);
    t.finish()
}

/// 6 x 4 in landscape postcard: picture front, message/address/stamp back.
fn postcard() -> Document {
    let mut t = T::new(
        106,
        setup(Size::new(432.0, 288.0), 18.0, 9.0),
        "Postcard",
        "postcard",
    );
    t.styles();
    let front = t.b.page(0);
    let back = t.b.add_page().expect("page");
    t.image(
        front,
        Rect::new(-9.0, -9.0, 450.0, 306.0),
        "front_picture",
        "Full-bleed picture, landscape (about 3:2)",
    );
    t.rect(front, Rect::new(-9.0, 222.0, 450.0, 75.0), NAVY);
    t.text(
        front,
        Rect::new(24.0, 232.0, 384.0, 44.0),
        "Greetings from...",
        props(SANS, 24.0, true, false, WHITE),
        para(Align::Left, 1.1, 0.0),
        Some("front_caption"),
        VerticalAlign::Middle,
    );
    t.line(
        Owner::Page(back),
        Rect::new(216.0, 28.0, 0.0, 232.0),
        ORANGE,
        1.0,
    );
    t.text_s(
        back,
        Rect::new(28.0, 28.0, 172.0, 232.0),
        "Write your message here.",
        Some("Body"),
        sz(12.0),
        al(Align::Left),
        Some("message"),
        CONTENT,
    );
    t.outline(back, Rect::new(352.0, 28.0, 56.0, 66.0), NAVY, 0.75);
    t.text(
        back,
        Rect::new(352.0, 28.0, 56.0, 66.0),
        "Place\nstamp\nhere",
        props(SANS, 7.0, false, false, INK),
        para(CENTER, 1.3, 0.0),
        None,
        VerticalAlign::Middle,
    );
    t.text(
        back,
        Rect::new(236.0, 124.0, 176.0, 84.0),
        "Recipient name\nStreet address\nCity, ST 00000",
        props(SANS, 12.0, false, false, INK),
        para(Align::Left, 1.2, 8.0),
        Some("address"),
        CONTENT,
    );
    t.finish()
}

/// Letter landscape certificate with a BorderArt frame.
fn certificate() -> Document {
    let mut t = T::new(
        107,
        setup(letter_landscape(), 36.0, 0.0),
        "Certificate",
        "certificate",
    );
    t.styles();
    let p = t.b.page(0);
    let border =
        t.b.shape(
            p,
            Rect::new(36.0, 36.0, 720.0, 540.0),
            Shape::Rectangle { corner_radius: 0.0 },
        )
        .expect("border");
    t.b.style(
        border,
        pubrs_core::FrameStyle {
            border_art: Some(pubrs_core::BorderArt {
                pattern: "ropes".into(),
                width: 20.0,
            }),
            ..Default::default()
        },
    )
    .expect("border art");
    t.text_s(
        p,
        Rect::new(96.0, 84.0, 600.0, 58.0),
        "Certificate of Achievement",
        Some("Heading 1"),
        sz(38.0),
        al(CENTER),
        Some("title"),
        VerticalAlign::Middle,
    );
    t.text(
        p,
        Rect::new(96.0, 150.0, 600.0, 24.0),
        "This certificate is proudly presented to",
        props(SANS, 14.0, false, false, INK),
        para(CENTER, 1.2, 0.0),
        None,
        VerticalAlign::Middle,
    );
    t.text(
        p,
        Rect::new(96.0, 180.0, 600.0, 64.0),
        "Recipient Name",
        italic(40.0, SERIF, NAVY),
        para(CENTER, 1.1, 0.0),
        Some("recipient"),
        VerticalAlign::Middle,
    );
    t.line(
        Owner::Page(p),
        Rect::new(216.0, 248.0, 360.0, 0.0),
        ORANGE,
        1.5,
    );
    t.text_s(
        p,
        Rect::new(136.0, 262.0, 520.0, 84.0),
        "in recognition of outstanding effort, dedication and achievement.",
        Some("Body"),
        sz(16.0),
        al(CENTER),
        Some("reason"),
        VerticalAlign::Middle,
    );
    t.image(
        p,
        Rect::new(336.0, 366.0, 120.0, 120.0),
        "seal",
        "Seal or logo, square",
    );
    for (x, ph, hint, label) in [
        (116.0, "date", "Month 1, 2027", "Date"),
        (496.0, "signer", "Name, Title", "Signed"),
    ] {
        t.text(
            p,
            Rect::new(x, 440.0, 180.0, 28.0),
            hint,
            props(SANS, 12.0, false, false, INK),
            para(CENTER, 1.2, 0.0),
            Some(ph),
            VerticalAlign::Bottom,
        );
        t.line(Owner::Page(p), Rect::new(x, 472.0, 180.0, 0.0), NAVY, 1.0);
        t.text(
            p,
            Rect::new(x, 476.0, 180.0, 16.0),
            label,
            props(SANS, 9.0, false, false, INK),
            para(CENTER, 1.2, 0.0),
            None,
            CONTENT,
        );
    }
    t.finish()
}

/// Two-page Letter restaurant menu with price tabs and a bulleted list.
fn menu() -> Document {
    let mut t = T::new(108, setup(letter(), 54.0, 0.0), "Restaurant menu", "menu");
    t.styles();
    let p1 = t.b.page(0);
    let p2 = t.b.add_page().expect("page");
    t.text_s(
        p1,
        Rect::new(54.0, 54.0, 504.0, 50.0),
        "Restaurant Name",
        Some("Heading 1"),
        sz(36.0),
        al(CENTER),
        Some("restaurant_name"),
        VerticalAlign::Middle,
    );
    t.text(
        p1,
        Rect::new(54.0, 106.0, 504.0, 22.0),
        "A short line about your kitchen",
        italic(12.0, SANS, INK),
        para(CENTER, 1.2, 0.0),
        Some("tagline"),
        VerticalAlign::Middle,
    );
    t.line(
        Owner::Page(p1),
        Rect::new(126.0, 140.0, 360.0, 0.0),
        ORANGE,
        1.5,
    );
    let section =
        |t: &mut T, page: PageId, y: f64, heading: &str, ph: &str, items: &str, h: f64| {
            t.text_s(
                page,
                Rect::new(54.0, y, 504.0, 28.0),
                heading,
                Some("Heading 2"),
                sz(18.0),
                al(CENTER),
                None,
                VerticalAlign::Middle,
            );
            t.text_s(
                page,
                Rect::new(72.0, y + 34.0, 468.0, h),
                items,
                Some("Body"),
                sz(13.0),
                menu_para(460.0),
                Some(ph),
                CONTENT,
            );
        };
    section(
        &mut t,
        p1,
        158.0,
        "Starters",
        "starters",
        "Tomato soup \t6.50\nGarden salad \t7.25\nGrilled halloumi \t8.00\nSourdough and butter \t4.00\nMarinated olives \t4.50",
        126.0,
    );
    section(
        &mut t,
        p1,
        332.0,
        "Mains",
        "mains",
        "Grilled salmon with lemon \t18.00\nMushroom risotto \t15.50\nRoast chicken \t16.00\nBeef burger and chips \t14.00\nSpinach and ricotta cannelloni \t13.50\nCatch of the day \t19.00",
        148.0,
    );
    section(
        &mut t,
        p1,
        526.0,
        "Desserts",
        "desserts",
        "Lemon tart \t7.00\nChocolate pot \t6.50\nSeasonal fruit crumble \t7.25\nIce cream, two scoops \t5.00",
        106.0,
    );
    t.text_s(
        p2,
        Rect::new(54.0, 54.0, 504.0, 34.0),
        "Today's specials",
        Some("Heading 1"),
        sz(26.0),
        al(CENTER),
        None,
        VerticalAlign::Middle,
    );
    t.text_s(
        p2,
        Rect::new(90.0, 100.0, 432.0, 170.0),
        "Soup of the day with warm bread\nFish pie with buttered greens\nSlow-cooked lamb shoulder for two\nApple and blackberry sponge",
        Some("Body"),
        sz(13.0),
        list_para(ListKind::Bullet),
        Some("specials"),
        CONTENT,
    );
    t.line(
        Owner::Page(p2),
        Rect::new(126.0, 288.0, 360.0, 0.0),
        ORANGE,
        1.5,
    );
    section(
        &mut t,
        p2,
        306.0,
        "Drinks",
        "drinks",
        "House lemonade \t3.50\nSparkling water \t3.00\nFilter coffee \t3.00\nEspresso \t2.75\nPot of tea \t3.25\nGlass of house wine \t7.50",
        148.0,
    );
    t.text(
        p2,
        Rect::new(54.0, 716.0, 504.0, 28.0),
        "Please tell your server about any allergies.",
        italic(10.0, SANS, INK),
        para(CENTER, 1.2, 0.0),
        Some("footer_note"),
        VerticalAlign::Middle,
    );
    t.finish()
}

/// Letter letterhead: logo, name and address, rule, empty body, footer.
fn letterhead() -> Document {
    let mut t = T::new(109, setup(letter(), 54.0, 0.0), "Letterhead", "letterhead");
    t.styles();
    let p = t.b.page(0);
    t.image(p, Rect::new(54.0, 40.0, 64.0, 64.0), "logo", "Logo, square");
    t.text_s(
        p,
        Rect::new(130.0, 42.0, 300.0, 32.0),
        "Organisation Name",
        Some("Heading 1"),
        sz(22.0),
        ParaProps::default(),
        Some("org_name"),
        VerticalAlign::Middle,
    );
    t.text(
        p,
        Rect::new(130.0, 76.0, 300.0, 40.0),
        "123 Main Street, City, ST 00000\n(555) 555-0100\nemail@example.com",
        props(SANS, 9.0, false, false, INK),
        para(Align::Left, 1.3, 0.0),
        Some("org_details"),
        CONTENT,
    );
    t.line(
        Owner::Page(p),
        Rect::new(54.0, 124.0, 504.0, 0.0),
        ORANGE,
        1.0,
    );
    // The body starts empty; the hint says what belongs there.
    let body = t.text_s(
        p,
        Rect::new(54.0, 144.0, 504.0, 580.0),
        "",
        Some("Body"),
        sz(11.5),
        al(Align::Left),
        Some("letter_body"),
        CONTENT,
    );
    t.describe(body, "Write your letter here");
    t.line(
        Owner::Page(p),
        Rect::new(54.0, 742.0, 504.0, 0.0),
        NAVY,
        0.5,
    );
    t.text(
        p,
        Rect::new(54.0, 746.0, 504.0, 16.0),
        "Organisation Name · 123 Main Street, City, ST 00000 · www.example.com",
        props(SANS, 8.0, false, false, INK),
        para(CENTER, 1.2, 0.0),
        Some("footer"),
        VerticalAlign::Middle,
    );
    t.finish()
}

/// #10 envelope, 9.5 x 4.125 in.
fn envelope() -> Document {
    let mut t = T::new(
        110,
        setup(Size::new(684.0, 297.0), 18.0, 0.0),
        "Envelope",
        "envelope",
    );
    t.styles();
    let p = t.b.page(0);
    t.image(p, Rect::new(36.0, 30.0, 44.0, 44.0), "logo", "Logo, square");
    t.text(
        p,
        Rect::new(88.0, 30.0, 220.0, 54.0),
        "Your name\nStreet address\nCity, ST 00000",
        props(SANS, 9.0, false, false, INK),
        para(Align::Left, 1.3, 0.0),
        Some("return_address"),
        CONTENT,
    );
    t.line(
        Owner::Page(p),
        Rect::new(36.0, 92.0, 272.0, 0.0),
        ORANGE,
        1.0,
    );
    t.outline(p, Rect::new(606.0, 28.0, 48.0, 56.0), NAVY, 0.75);
    t.text(
        p,
        Rect::new(606.0, 28.0, 48.0, 56.0),
        "Stamp",
        props(SANS, 7.0, false, false, INK),
        para(CENTER, 1.2, 0.0),
        None,
        VerticalAlign::Middle,
    );
    t.text(
        p,
        Rect::new(360.0, 124.0, 270.0, 90.0),
        "Recipient name\nStreet address\nCity, ST 00000",
        props(SANS, 13.0, false, false, INK),
        para(Align::Left, 1.35, 3.0),
        Some("recipient"),
        CONTENT,
    );
    t.finish()
}

// ---- calendar date helpers (civil calendar, proleptic Gregorian) ----

/// Is `year` a leap year?
pub fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Days in `month` (1-12) of `year`.
pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if is_leap(year) {
                29
            } else {
                28
            }
        }
    }
}

/// Day of the week of a date, 0 = Sunday ... 6 = Saturday (Sakamoto).
pub(crate) fn weekday(year: i32, month: u32, day: u32) -> u32 {
    debug_assert!((1..=12).contains(&month), "month {month} out of range");
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if month < 3 { year - 1 } else { year };
    (y + y / 4 - y / 100 + y / 400 + T[month as usize - 1] + day as i32).rem_euclid(7) as u32
}

/// The civil year containing a Unix time (seconds since 1970-01-01 UTC).
pub fn year_of_unix_time(secs: i64) -> i32 {
    // Hinnant's civil_from_days.
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2)) as i32
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Twelve Letter landscape month pages for `year`; weeks start on Sunday
/// and every month shows six week rows, the spare days being the
/// neighbouring months' (grey).
fn calendar(year: i32) -> Document {
    let year = year.clamp(*CALENDAR_YEARS.start(), *CALENDAR_YEARS.end());
    let mut t = T::new(
        111,
        setup(letter_landscape(), 36.0, 0.0),
        &format!("Calendar {year}"),
        "calendar",
    );
    t.styles();
    let cols = [103.0, 103.0, 103.0, 103.0, 103.0, 103.0, 102.0];
    let mut rows = vec![22.0];
    rows.extend([44.0; 6]);
    for m in 1..=12u32 {
        let page = if m == 1 {
            t.b.page(0)
        } else {
            t.b.add_page().expect("page")
        };
        t.image(
            page,
            Rect::new(36.0, 30.0, 720.0, 170.0),
            &format!("photo_{m:02}"),
            &format!(
                "Photo for {}, landscape (about 4:1)",
                MONTHS[m as usize - 1]
            ),
        );
        t.text_s(
            page,
            Rect::new(36.0, 208.0, 720.0, 40.0),
            &format!("{} {year}", MONTHS[m as usize - 1]),
            Some("Heading 1"),
            sz(30.0),
            ParaProps::default(),
            None,
            VerticalAlign::Middle,
        );
        // Day numbers: the first of the month sits under its weekday.
        let lead = weekday(year, m, 1) as i32;
        let (py, pm) = if m == 1 {
            (year - 1, 12)
        } else {
            (year, m - 1)
        };
        let prev = days_in_month(py, pm) as i32;
        let this = days_in_month(year, m) as i32;
        let mut texts: Vec<Vec<String>> = vec![
            ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        ];
        let mut outside = Vec::new();
        for week in 0..6 {
            let mut row = Vec::new();
            for dow in 0..7 {
                let day = week * 7 + dow - lead + 1;
                let (n, out) = if day < 1 {
                    (prev + day, true)
                } else if day > this {
                    (day - this, true)
                } else {
                    (day, false)
                };
                if out {
                    outside.push((week as u32 + 1, dow as u32));
                }
                row.push(n.to_string());
            }
            texts.push(row);
        }
        let cells: Vec<Vec<&str>> = texts
            .iter()
            .map(|r| r.iter().map(String::as_str).collect())
            .collect();
        let look = TableLook {
            size: 12.0,
            band: false,
            valign: VerticalAlign::Top,
            ..TableLook::standard()
        };
        let table = t.table(page, (36.0, 254.0), &cols, &rows, &cells, &look);
        t.name(table, &format!("days_{m:02}"));
        for week in 0..6u32 {
            for dow in 0..7u32 {
                let chars = if outside.contains(&(week + 1, dow)) {
                    props(SANS, 12.0, false, false, GREY)
                } else {
                    props(SANS, 12.0, true, false, INK)
                };
                t.cell_chars(table, week + 1, dow, chars);
            }
        }
    }
    t.finish()
}

/// One Avery 5160 address label, repeated down the sheet.
fn address_labels() -> Document {
    let mut t = T::new(
        112,
        sheet_setup("avery-5160", 6.0, pubrs_core::SheetFill::Sequence),
        "Address labels",
        "address_labels",
    );
    t.styles();
    let p = t.b.page(0);
    t.rect(p, Rect::new(0.0, 0.0, 5.0, 72.0), ORANGE);
    // The address block is merge fields (M24, ADR 0023): Name, Address, then
    // City, State Zip. Load a list with merge_set_source (columns named like
    // these) and merge: one record per label on the 5160 sheet.
    let f = t.text(
        p,
        Rect::new(14.0, 10.0, 165.0, 52.0),
        "Name\nAddress\nCity, State Zip",
        props(SANS, 10.0, false, false, INK),
        para(Align::Left, 1.2, 0.0),
        Some("address"),
        VerticalAlign::Middle,
    );
    let story = t.b.story_of(f);
    let base = t.b.doc().story(story).expect("story").paragraphs[0].clone();
    let chars = base.runs[0].props.clone();
    let line = |parts: &[(&str, bool)]| Paragraph {
        style: base.style,
        props: base.props.clone(),
        runs: parts
            .iter()
            .map(|(s, field)| {
                if *field {
                    Run::field(s, None, chars.clone())
                } else {
                    Run {
                        text: (*s).into(),
                        props: chars.clone(),
                        ..Run::default()
                    }
                }
            })
            .collect(),
    };
    t.b.exec(Command::SetStory {
        story,
        paragraphs: vec![
            line(&[("Name", true)]),
            line(&[("Address", true)]),
            line(&[
                ("City", true),
                (", ", false),
                ("State", true),
                (" ", false),
                ("Zip", true),
            ]),
        ],
    })
    .expect("fields");
    t.finish()
}

/// One Avery 5371 business card, repeated on the sheet.
fn business_cards() -> Document {
    let mut t = T::new(
        113,
        sheet_setup("avery-5371", 12.0, pubrs_core::SheetFill::Repeat),
        "Business cards",
        "business_cards",
    );
    t.styles();
    let p = t.b.page(0);
    t.rect(p, Rect::new(0.0, 0.0, 252.0, 38.0), NAVY);
    t.text(
        p,
        Rect::new(14.0, 7.0, 224.0, 24.0),
        "Your Name",
        props(SANS, 16.0, true, false, WHITE),
        para(Align::Left, 1.0, 0.0),
        Some("name"),
        VerticalAlign::Middle,
    );
    t.text(
        p,
        Rect::new(14.0, 46.0, 170.0, 16.0),
        "Job Title",
        italic(10.0, SERIF, INK),
        para(Align::Left, 1.0, 0.0),
        Some("job_title"),
        VerticalAlign::Middle,
    );
    t.text(
        p,
        Rect::new(14.0, 62.0, 170.0, 18.0),
        "Company Name",
        props(SANS, 11.0, true, false, NAVY),
        para(Align::Left, 1.0, 0.0),
        Some("company"),
        VerticalAlign::Middle,
    );
    t.line(
        Owner::Page(p),
        Rect::new(16.0, 88.0, 40.0, 0.0),
        ORANGE,
        1.0,
    );
    t.text(
        p,
        Rect::new(14.0, 94.0, 170.0, 40.0),
        "(555) 555-0100\nemail@example.com\nwww.example.com",
        props(SANS, 8.5, false, false, INK),
        para(Align::Left, 1.25, 0.0),
        Some("contact"),
        CONTENT,
    );
    t.image(
        p,
        Rect::new(192.0, 60.0, 46.0, 46.0),
        "logo",
        "Logo, square",
    );
    t.finish()
}

/// Four-page Letter newsletter. The masthead band is inset 36 pt from the
/// page edges (the page has no bleed), clear of the unprintable margin.
fn newsletter() -> Document {
    let mut t = T::new(104, setup(letter(), 36.0, 0.0), "Newsletter", "newsletter");
    t.styles();
    let p1 = t.b.page(0);
    let p2 = t.b.add_page().expect("page");
    let p3 = t.b.add_page().expect("page");
    let p4 = t.b.add_page().expect("page");
    let master = MasterId::new(&mut t.b.ids);
    t.b.exec(Command::AddMaster {
        master: MasterPage {
            id: master,
            name: "Newsletter".into(),
            frames: vec![],
            guides: vec![],
        },
    })
    .expect("master");
    t.line(
        Owner::Master(master),
        Rect::new(36.0, 752.0, 540.0, 0.0),
        NAVY,
        1.0,
    );
    for p in [p1, p2, p3, p4] {
        t.b.exec(Command::SetPageMaster {
            page: p,
            master: Some(master),
        })
        .expect("apply");
    }
    let heading = |t: &mut T, page: PageId, y: f64, text: &str| {
        t.text_s(
            page,
            Rect::new(36.0, y, 540.0, 26.0),
            text,
            Some("Heading 2"),
            sz(16.0),
            ParaProps::default(),
            None,
            VerticalAlign::Middle,
        );
    };
    let justified = || ParaProps {
        align: Some(Align::Justify),
        ..Default::default()
    };
    // ---- page 1 ----
    t.rect(p1, Rect::new(36.0, 36.0, 540.0, 84.0), NAVY);
    t.text(
        p1,
        Rect::new(48.0, 42.0, 330.0, 72.0),
        "Newsletter Name",
        props(SERIF, 36.0, true, false, WHITE),
        para(Align::Left, 1.0, 0.0),
        Some("masthead"),
        VerticalAlign::Middle,
    );
    t.text(
        p1,
        Rect::new(388.0, 42.0, 176.0, 72.0),
        "Issue 1 · Month Year",
        props(SANS, 11.0, false, false, CREAM),
        para(Align::Right, 1.2, 0.0),
        Some("issue_date"),
        VerticalAlign::Middle,
    );
    t.text_s(
        p1,
        Rect::new(36.0, 132.0, 540.0, 40.0),
        "Lead Story Headline",
        Some("Heading 1"),
        sz(24.0),
        ParaProps::default(),
        Some("lead_headline"),
        VerticalAlign::Bottom,
    );
    t.image(
        p1,
        Rect::new(36.0, 180.0, 340.0, 200.0),
        "lead_image",
        "Lead photo, landscape (about 17:10)",
    );
    t.text_s(
        p1,
        Rect::new(36.0, 382.0, 340.0, 30.0),
        "Caption for the lead photo",
        Some("Caption"),
        ov(),
        ParaProps::default(),
        Some("lead_caption"),
        CONTENT,
    );
    t.rect(p1, Rect::new(392.0, 180.0, 184.0, 232.0), CREAM);
    t.text_s(
        p1,
        Rect::new(402.0, 188.0, 164.0, 26.0),
        "In Brief",
        Some("Heading 2"),
        sz(16.0),
        ParaProps::default(),
        Some("sidebar_heading"),
        VerticalAlign::Middle,
    );
    t.text_s(
        p1,
        Rect::new(402.0, 218.0, 164.0, 186.0),
        "Short news items, dates for the diary or a welcome note.",
        Some("Body"),
        sz(10.5),
        ParaProps::default(),
        Some("sidebar_text"),
        CONTENT,
    );
    let a = t.text_s(
        p1,
        Rect::new(36.0, 424.0, 255.0, 316.0),
        "Write the lead story here. It flows through both columns on this page and continues on page 2.",
        Some("Body"),
        ov(),
        justified(),
        Some("lead_story"),
        CONTENT,
    );
    let b =
        t.b.text_paragraphs(
            Parent::Page(p1),
            Rect::new(321.0, 424.0, 255.0, 316.0),
            vec![Paragraph::default()],
        )
        .expect("col2");
    t.name(b, "lead_story_column_2");
    let c =
        t.b.text_paragraphs(
            Parent::Page(p2),
            Rect::new(36.0, 36.0, 540.0, 250.0),
            vec![Paragraph::default()],
        )
        .expect("p2");
    t.name(c, "lead_story_page_2");
    t.b.exec(Command::SetTextFrame {
        frame: c,
        props: pubrs_core::TextFrameProps {
            columns: Some(2),
            column_gap: Some(18.0),
            ..Default::default()
        },
    })
    .expect("cols");
    t.b.link(a, b).expect("link");
    t.b.link(b, c).expect("link");
    // ---- page 2 ----
    t.line(
        Owner::Page(p2),
        Rect::new(96.0, 300.0, 420.0, 0.0),
        ORANGE,
        2.0,
    );
    t.text(
        p2,
        Rect::new(96.0, 306.0, 420.0, 86.0),
        "A short, striking quote from the story.",
        italic(20.0, SERIF, NAVY),
        para(CENTER, 1.2, 0.0),
        Some("pull_quote"),
        VerticalAlign::Middle,
    );
    t.line(
        Owner::Page(p2),
        Rect::new(96.0, 398.0, 420.0, 0.0),
        ORANGE,
        2.0,
    );
    heading(&mut t, p2, 414.0, "Upcoming events");
    let events = t.table(
        p2,
        (36.0, 444.0),
        &[90.0, 300.0, 150.0],
        &[26.0, 28.0, 28.0, 28.0],
        &[
            vec!["Date", "Event", "Where"],
            vec!["Mar 4", "Spring meeting", "Main hall"],
            vec!["Mar 18", "Open workshop", "Studio 2"],
            vec!["Apr 2", "Community lunch", "Garden"],
        ],
        &TableLook::standard(),
    );
    t.name(events, "events_table");
    heading(&mut t, p2, 570.0, "In brief");
    t.text_s(
        p2,
        Rect::new(36.0, 598.0, 540.0, 140.0),
        "A news item worth a line\nAn announcement\nA thank-you\nA reminder\nA date for the diary",
        Some("Body"),
        ov(),
        list_para(ListKind::Bullet),
        Some("news_list"),
        CONTENT,
    );
    // ---- page 3 ----
    t.text_s(
        p3,
        Rect::new(36.0, 36.0, 540.0, 40.0),
        "Second Story Headline",
        Some("Heading 1"),
        sz(24.0),
        ParaProps::default(),
        Some("second_headline"),
        VerticalAlign::Bottom,
    );
    t.image(
        p3,
        Rect::new(36.0, 84.0, 250.0, 170.0),
        "second_image",
        "Photo, landscape (about 3:2)",
    );
    t.text_s(
        p3,
        Rect::new(36.0, 256.0, 250.0, 30.0),
        "Caption for the photo",
        Some("Caption"),
        ov(),
        ParaProps::default(),
        Some("second_caption"),
        CONTENT,
    );
    t.text_s(
        p3,
        Rect::new(302.0, 84.0, 274.0, 200.0),
        "Write the second story here.",
        Some("Body"),
        ov(),
        justified(),
        Some("second_story"),
        CONTENT,
    );
    t.line(
        Owner::Page(p3),
        Rect::new(36.0, 300.0, 540.0, 0.0),
        ORANGE,
        2.0,
    );
    heading(&mut t, p3, 312.0, "How to take part");
    t.text_s(
        p3,
        Rect::new(36.0, 342.0, 540.0, 130.0),
        "Say hello at the front desk\nPick up a programme\nChoose a workshop\nBring a friend",
        Some("Body"),
        ov(),
        list_para(ListKind::Number),
        Some("steps"),
        CONTENT,
    );
    heading(&mut t, p3, 490.0, "Opening hours");
    let hours = t.table(
        p3,
        (36.0, 520.0),
        &[180.0, 180.0, 180.0],
        &[26.0; 5],
        &[
            vec!["Day", "Opens", "Closes"],
            vec!["Monday to Friday", "9:00 am", "5:00 pm"],
            vec!["Saturday", "10:00 am", "4:00 pm"],
            vec!["Sunday", "Closed", "Closed"],
            vec!["Holidays", "By arrangement", "By arrangement"],
        ],
        &TableLook::standard(),
    );
    t.name(hours, "hours_table");
    // ---- page 4 ----
    t.text_s(
        p4,
        Rect::new(36.0, 36.0, 540.0, 40.0),
        "Back Page Headline",
        Some("Heading 1"),
        sz(24.0),
        ParaProps::default(),
        Some("back_headline"),
        VerticalAlign::Bottom,
    );
    let s = t.text_s(
        p4,
        Rect::new(36.0, 84.0, 540.0, 190.0),
        "Write the back page story here.",
        Some("Body"),
        ov(),
        justified(),
        Some("back_story"),
        CONTENT,
    );
    t.b.exec(Command::SetTextFrame {
        frame: s,
        props: pubrs_core::TextFrameProps {
            columns: Some(2),
            column_gap: Some(18.0),
            ..Default::default()
        },
    })
    .expect("cols");
    t.rect(p4, Rect::new(36.0, 290.0, 540.0, 110.0), CREAM);
    t.text_s(
        p4,
        Rect::new(48.0, 298.0, 516.0, 26.0),
        "Get in touch",
        Some("Heading 2"),
        sz(16.0),
        ParaProps::default(),
        Some("contact_heading"),
        VerticalAlign::Middle,
    );
    t.text_s(
        p4,
        Rect::new(48.0, 326.0, 516.0, 66.0),
        "Phone, email and website\nStreet address, City, ST 00000",
        Some("Body"),
        ov(),
        ParaProps::default(),
        Some("contact_text"),
        CONTENT,
    );
    // Mailing panel: return address, stamp box and recipient.
    t.outline(p4, Rect::new(36.0, 548.0, 540.0, 192.0), NAVY, 0.5);
    t.text(
        p4,
        Rect::new(48.0, 560.0, 240.0, 56.0),
        "Your organisation\nStreet address\nCity, ST 00000",
        props(SANS, 9.0, false, false, INK),
        para(Align::Left, 1.3, 0.0),
        Some("return_address"),
        CONTENT,
    );
    t.outline(p4, Rect::new(500.0, 560.0, 64.0, 64.0), NAVY, 0.75);
    t.text(
        p4,
        Rect::new(500.0, 560.0, 64.0, 64.0),
        "Stamp",
        props(SANS, 7.0, false, false, INK),
        para(CENTER, 1.2, 0.0),
        None,
        VerticalAlign::Middle,
    );
    t.text(
        p4,
        Rect::new(300.0, 650.0, 264.0, 72.0),
        "Recipient name\nStreet address\nCity, ST 00000",
        props(SANS, 12.0, false, false, INK),
        para(Align::Left, 1.3, 2.0),
        Some("mailing_address"),
        CONTENT,
    );
    let f =
        t.b.doc()
            .placeholder("sidebar_heading")
            .expect("placeholder");
    t.describe(f, "Heading for the sidebar");
    t.finish()
}

/// Build a built-in template document with default options.
pub fn builtin(name: &str) -> Option<Document> {
    builtin_with(name, &TemplateOptions::default())
}

/// Build a built-in template document. `opts.year` applies to `calendar`.
pub fn builtin_with(name: &str, opts: &TemplateOptions) -> Option<Document> {
    Some(match name {
        "calendar" => calendar(opts.year.unwrap_or(DEFAULT_CALENDAR_YEAR)),
        "greeting_card" => greeting_card(),
        "postcard" => postcard(),
        "certificate" => certificate(),
        "menu" => menu(),
        "letterhead" => letterhead(),
        "envelope" => envelope(),
        "address_labels" => address_labels(),
        "business_cards" => business_cards(),
        "flyer" => flyer(),
        "trifold_brochure" | "trifold" => trifold(),
        "business_card" => business_card(),
        "newsletter" => newsletter(),
        _ => return None,
    })
}

/// Metadata for a template document.
pub fn info(doc: &Document, name: &str, builtin: bool) -> TemplateInfo {
    let mut placeholders = Vec::new();
    let mut named_frames = Vec::new();
    for (i, page) in doc.pages.iter().enumerate() {
        for id in doc.all_frames_of(Owner::Page(page.id)) {
            let Some(f) = doc.frame(id) else { continue };
            if let Some(p) = &f.placeholder {
                placeholders.push(PlaceholderInfo {
                    name: p.name.clone(),
                    kind: p.kind,
                    hint: p.hint.clone(),
                    page_index: i,
                    width: f.rect.width,
                    height: f.rect.height,
                });
            } else if let Some(n) = &f.name {
                named_frames.push(n.clone());
            }
        }
    }
    TemplateInfo {
        name: name.to_string(),
        title: if builtin {
            title_of(name).into()
        } else {
            doc.meta.title.clone()
        },
        description: if builtin {
            description_of(name).into()
        } else {
            "User template".into()
        },
        builtin,
        page_size: doc.setup.size,
        page_count: doc.pages.len(),
        placeholders,
        named_frames,
    }
}

/// Valid user template names: letters, digits, `_` and `-`.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Built-in and user templates.
#[derive(Debug, Clone, Default)]
pub struct TemplateLibrary {
    pub user_dir: Option<PathBuf>,
}

impl TemplateLibrary {
    pub fn new(user_dir: Option<PathBuf>) -> Self {
        Self { user_dir }
    }

    fn user_path(&self, name: &str) -> Option<PathBuf> {
        self.user_dir
            .as_ref()
            .map(|d| d.join(format!("{name}.pubrs")))
    }

    /// All templates: built-ins first, then user templates sorted by name.
    pub fn list(&self) -> Vec<TemplateInfo> {
        let mut out: Vec<TemplateInfo> = BUILTIN
            .iter()
            .filter_map(|n| builtin(n).map(|d| info(&d, n, true)))
            .collect();
        if let Some(dir) = &self.user_dir
            && let Ok(rd) = std::fs::read_dir(dir)
        {
            let mut user: Vec<TemplateInfo> = rd
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "pubrs"))
                .filter_map(|p| {
                    let name = p.file_stem()?.to_string_lossy().into_owned();
                    let doc = crate::format::load(&p).ok()?;
                    Some(info(&doc, &name, false))
                })
                .filter(|t| !BUILTIN.contains(&t.name.as_str()))
                .collect();
            user.sort_by(|a, b| a.name.cmp(&b.name));
            out.extend(user);
        }
        out
    }

    /// Load a template document by name.
    pub fn load(&self, name: &str) -> Result<Document, IoError> {
        self.load_with(name, &TemplateOptions::default())
    }

    /// Load a template document by name, with options for built-ins.
    pub fn load_with(&self, name: &str, opts: &TemplateOptions) -> Result<Document, IoError> {
        if let Some(d) = builtin_with(name, opts) {
            return Ok(d);
        }
        if valid_name(name)
            && let Some(p) = self.user_path(name)
            && p.exists()
        {
            return crate::format::load(&p);
        }
        Err(IoError::Format(format!("no template named {name:?}")))
    }

    /// Save a document as a user template.
    pub fn save(&self, name: &str, doc: &Document) -> Result<PathBuf, IoError> {
        if !valid_name(name) || BUILTIN.contains(&name) {
            return Err(IoError::Format(format!(
                "invalid template name {name:?} (letters, digits, _ and -; not a built-in name)"
            )));
        }
        let path = self
            .user_path(name)
            .ok_or_else(|| IoError::Format("no user template folder is configured".into()))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut doc = doc.clone();
        doc.meta.template = Some(name.to_string());
        crate::format::save(&doc, &path, None)?;
        Ok(path)
    }
}

/// A fresh document from a template: new document id, template name recorded.
pub fn instantiate(template: &Document, name: &str, ids: &mut dyn IdSource) -> Document {
    let mut doc = template.clone();
    doc.id = pubrs_core::DocumentId::new(ids);
    doc.meta.template = Some(name.to_string());
    doc
}

/// Does a path look like a template file?
pub fn is_template_file(p: &Path) -> bool {
    p.extension().is_some_and(|e| e == "pubrs")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_sound_and_list_placeholders() {
        for name in BUILTIN {
            let doc = builtin(name).unwrap();
            assert!(
                pubrs_core::validate::check_integrity(&doc).is_empty(),
                "{name}: {:?}",
                pubrs_core::validate::check_integrity(&doc)
            );
            let i = info(&doc, name, true);
            assert!(!i.placeholders.is_empty(), "{name}");
            let mut names: Vec<&str> = i.placeholders.iter().map(|p| p.name.as_str()).collect();
            names.sort();
            let before = names.len();
            names.dedup();
            assert_eq!(before, names.len(), "{name}: duplicate placeholder names");
            // Only unfilled-picture and empty-placeholder warnings before filling.
            for w in pubrs_core::validate::validate(&doc) {
                assert!(
                    matches!(
                        w.code,
                        pubrs_core::WarningCode::EmptyPlaceholder
                            | pubrs_core::WarningCode::EmptyImageFrame
                    ),
                    "{name}: {w:?}"
                );
            }
        }
    }

    /// Brand colours are scheme slots: the default scheme reproduces the
    /// literal colours the templates were designed with, and applying
    /// another scheme recolours them.
    #[test]
    fn templates_use_scheme_slots_and_recolour() {
        use pubrs_core::{Color, SchemeId};
        for name in BUILTIN {
            let doc = builtin(name).unwrap();
            assert_eq!(doc.swatches.len(), 7, "{name}");
            let scheme = pubrs_core::swatch::scheme(scheme_of(name)).unwrap();
            assert_eq!(doc.scheme, Some(SchemeId(scheme.id.into())), "{name}");
            let uses: usize = doc
                .swatches
                .iter()
                .map(|s| doc.swatch_uses(s.id).len())
                .sum();
            assert!(uses > 0, "{name} should use scheme slots");
            // Slot values are the scheme's colours.
            let slot = |s: SchemeSlot| doc.resolve_solid(&s.paint()).rgba;
            for s in SchemeSlot::ALL {
                assert_eq!(slot(s), scheme.color(s), "{name}");
            }
            if scheme.id == pubrs_core::swatch::DEFAULT_SCHEME {
                assert_eq!(slot(SchemeSlot::Main), Color::rgb(0x1E, 0x3A, 0x5F));
                assert_eq!(slot(SchemeSlot::Accent1), Color::rgb(0xC2, 0x41, 0x0C));
                assert_eq!(slot(SchemeSlot::Accent2), Color::rgb(0xFF, 0xF7, 0xED));
            }
            let other = if scheme.id == "coral_reef" {
                "juniper_slate"
            } else {
                "coral_reef"
            };
            let want = pubrs_core::swatch::scheme(other)
                .unwrap()
                .color(SchemeSlot::Main);
            let mut ed = pubrs_core::Editor::new(doc);
            ed.execute(Command::ApplyScheme {
                scheme: SchemeId(other.into()),
            })
            .unwrap();
            assert_eq!(ed.doc().resolve_solid(&SchemeSlot::Main.paint()).rgba, want);
        }
    }

    #[test]
    fn trifold_has_two_image_placeholders_and_a_continuation_frame() {
        let doc = builtin("trifold_brochure").unwrap();
        let i = info(&doc, "trifold_brochure", true);
        assert_eq!(i.page_count, 2);
        let images = i
            .placeholders
            .iter()
            .filter(|p| p.kind == PlaceholderKind::Image)
            .count();
        assert_eq!(images, 2);
        assert!(
            i.named_frames
                .contains(&"inside_body_continued".to_string())
        );
    }

    #[test]
    fn templates_are_deterministic() {
        for name in BUILTIN {
            assert_eq!(builtin(name), builtin(name), "{name}");
        }
    }

    #[test]
    fn every_template_has_its_own_scheme_and_seed_ids() {
        let mut schemes: Vec<&str> = BUILTIN[3..].iter().map(|n| scheme_of(n)).collect();
        schemes.sort();
        schemes.dedup();
        assert_eq!(schemes.len(), BUILTIN.len() - 3, "schemes repeat");
        let mut docs: Vec<_> = BUILTIN
            .iter()
            .map(|n| builtin(n).unwrap().pages[0].id)
            .collect();
        docs.sort();
        docs.dedup();
        assert_eq!(
            docs.len(),
            BUILTIN.len(),
            "page ids collide between templates"
        );
    }

    #[test]
    fn leap_years_and_month_lengths() {
        for (y, leap) in [
            (1900, false),
            (2000, true),
            (2024, true),
            (2100, false),
            (2027, false),
            (1600, true),
        ] {
            assert_eq!(is_leap(y), leap, "{y}");
            assert_eq!(days_in_month(y, 2), if leap { 29 } else { 28 }, "{y}");
        }
        assert_eq!(days_in_month(2027, 4), 30);
        assert_eq!(days_in_month(2027, 12), 31);
    }

    /// Days since 1970-01-01 (Hinnant), independent of `weekday`.
    fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
        let y = i64::from(y) - i64::from(m <= 2);
        let era = y.div_euclid(400);
        let yoe = y.rem_euclid(400);
        let mp = i64::from((m + 9) % 12);
        let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    #[test]
    #[should_panic]
    fn weekday_rejects_month_zero() {
        weekday(2024, 0, 1);
    }

    #[test]
    #[should_panic]
    fn weekday_rejects_month_thirteen() {
        weekday(2024, 13, 1);
    }

    #[test]
    fn weekday_agrees_with_a_day_count_for_every_day_1900_to_2100() {
        // 1970-01-01 was a Thursday (4).
        let mut known = 0;
        for y in 1900..=2100 {
            for m in 1..=12 {
                for d in 1..=days_in_month(y, m) {
                    let want = (days_from_civil(y, m, d) + 4).rem_euclid(7) as u32;
                    assert_eq!(weekday(y, m, d), want, "{y}-{m}-{d}");
                    known += 1;
                }
            }
        }
        assert_eq!(known, 73_414);
        // Anchors: leap centuries and well-known dates.
        assert_eq!(weekday(2000, 1, 1), 6);
        assert_eq!(weekday(2000, 3, 1), 3);
        assert_eq!(weekday(1900, 1, 1), 1);
        assert_eq!(weekday(1900, 3, 1), 4);
        assert_eq!(weekday(2100, 3, 1), 1);
        assert_eq!(weekday(2024, 2, 29), 4);
        assert_eq!(weekday(2027, 1, 1), 5);
        // The month bounds are the edges of the table.
        assert_eq!(weekday(2024, 12, 31), 2);
    }

    #[test]
    fn unix_time_to_year() {
        assert_eq!(year_of_unix_time(0), 1970);
        assert_eq!(year_of_unix_time(951_782_400), 2000); // 2000-02-29
        assert_eq!(year_of_unix_time(1_798_761_599), 2026); // 2026-12-31 23:59:59
        assert_eq!(year_of_unix_time(1_798_761_600), 2027);
        assert_eq!(year_of_unix_time(-1), 1969);
    }

    #[test]
    fn calendar_has_twelve_pages_and_grey_neighbours() {
        use pubrs_core::Frame;
        let doc = builtin_with("calendar", &TemplateOptions { year: Some(2026) }).unwrap();
        assert_eq!(doc.pages.len(), 12);
        assert_eq!(doc.meta.title, "Calendar 2026");
        // February 2026 starts on a Sunday and has 28 days: the table shows
        // 1..28 then March 1..14, with no empty cell.
        let page = doc.pages[1].id;
        let table = doc
            .all_frames_of(Owner::Page(page))
            .into_iter()
            .filter_map(|id| doc.frame(id))
            .find(|f: &&Frame| f.as_table().is_some())
            .unwrap();
        let t = table.as_table().unwrap();
        assert_eq!((t.row_count(), t.col_count()), (7, 7));
        let text = |r: u32, c: u32| {
            doc.story(t.cell_at(r, c).unwrap().story)
                .unwrap()
                .plain_text()
        };
        assert_eq!(text(0, 0), "Sun");
        assert_eq!(text(1, 0), "1");
        assert_eq!(text(4, 6), "28");
        assert_eq!(text(5, 0), "1");
        assert_eq!(text(6, 6), "14");
        for r in 0..7 {
            for c in 0..7 {
                assert!(!text(r, c).is_empty());
            }
        }
        // January 2027 starts on a Friday: the lead is December's 26-31.
        let doc = builtin("calendar").unwrap();
        let t = doc
            .all_frames_of(Owner::Page(doc.pages[0].id))
            .into_iter()
            .filter_map(|id| doc.frame(id))
            .find_map(|f| f.as_table())
            .unwrap();
        let text = |r: u32, c: u32| {
            doc.story(t.cell_at(r, c).unwrap().story)
                .unwrap()
                .plain_text()
        };
        assert_eq!(text(1, 0), "27");
        assert_eq!(text(1, 5), "1");
    }
}

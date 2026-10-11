//! Templates: ordinary documents whose frames carry placeholder metadata.
//!
//! Built-in templates are constructed here through commands with
//! deterministic ids; user templates are `.pubrs` files in a folder.

use std::path::{Path, PathBuf};

use pubrs_core::{
    Align, Axis, CharProps, Color, Command, DocBuilder, Document, FrameId, Guide, GuideId,
    IdSource, Margins, MasterId, MasterPage, Owner, PageId, PageSetup, PageSizePreset, Paint,
    ParaProps, Paragraph, Parent, Placeholder, PlaceholderKind, Rect, Run, SchemeSlot, SeqIds,
    Shape, Size, Stroke, TextWrap, VerticalAlign, WrapMode,
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
pub const BUILTIN: [&str; 4] = ["flyer", "trifold_brochure", "business_card", "newsletter"];

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
            "Two-page Letter newsletter: masthead, lead story flowing through two columns onto page 2, lead photo, sidebar, second story in two columns."
        }
        _ => "",
    }
}

/// Builder helpers shared by the templates.
struct T {
    b: DocBuilder<SeqIds>,
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
            // The default scheme's slot swatches, with their fixed ids.
            pubrs_core::swatch::apply_scheme(
                &mut d,
                pubrs_core::swatch::scheme(pubrs_core::swatch::DEFAULT_SCHEME)
                    .expect("default scheme"),
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
        let paragraphs: Vec<Paragraph> = text
            .split('\n')
            .map(|line| Paragraph {
                style: None,
                props: para.clone(),
                runs: vec![Run {
                    text: line.into(),
                    style: None,
                    props: chars.clone(),
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

fn newsletter() -> Document {
    let mut t = T::new(
        104,
        PageSetup {
            size: PageSizePreset::Letter.size(),
            margins: Margins::uniform(36.0),
            bleed: 0.0,
            ..PageSetup::default()
        },
        "Newsletter",
        "newsletter",
    );
    let p1 = t.b.page(0);
    let p2 = t.b.add_page().expect("page");
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
    t.b.exec(Command::SetPageMaster {
        page: p1,
        master: Some(master),
    })
    .expect("apply");
    t.b.exec(Command::SetPageMaster {
        page: p2,
        master: Some(master),
    })
    .expect("apply");
    t.rect(p1, Rect::new(0.0, 0.0, 612.0, 112.0), NAVY);
    t.text(
        p1,
        Rect::new(36.0, 22.0, 400.0, 70.0),
        "Newsletter Name",
        props(SERIF, 38.0, true, false, WHITE),
        para(Align::Left, 1.0, 0.0),
        Some("masthead"),
        VerticalAlign::Middle,
    );
    t.text(
        p1,
        Rect::new(436.0, 40.0, 140.0, 40.0),
        "Issue · Month Year",
        props(SANS, 11.0, false, false, CREAM),
        para(Align::Right, 1.2, 0.0),
        Some("issue_date"),
        VerticalAlign::Middle,
    );
    t.text(
        p1,
        Rect::new(36.0, 128.0, 540.0, 44.0),
        "Lead Story Headline",
        props(SANS, 26.0, true, false, NAVY),
        para(Align::Left, 1.05, 0.0),
        Some("lead_headline"),
        VerticalAlign::Bottom,
    );
    t.image(
        p1,
        Rect::new(36.0, 184.0, 340.0, 210.0),
        "lead_image",
        "Lead photo, landscape (about 8:5)",
    );
    t.rect(p1, Rect::new(392.0, 184.0, 184.0, 380.0), CREAM);
    t.text(
        p1,
        Rect::new(402.0, 194.0, 164.0, 40.0),
        "In Brief",
        props(SANS, 16.0, true, false, ORANGE),
        para(Align::Left, 1.1, 0.0),
        Some("sidebar_heading"),
        VerticalAlign::Top,
    );
    t.text(
        p1,
        Rect::new(402.0, 236.0, 164.0, 318.0),
        "Short news items, dates for the diary or a welcome note.",
        props(SANS, 10.5, false, false, INK),
        para(Align::Left, 1.35, 8.0),
        Some("sidebar_text"),
        VerticalAlign::Top,
    );
    let a = t.text(
        p1,
        Rect::new(36.0, 404.0, 165.0, 336.0),
        "Write the lead story here. It flows through both columns on this page and continues on page 2.",
        props(SERIF, 11.0, false, false, INK),
        para(Align::Justify, 1.35, 8.0),
        Some("lead_story"),
        VerticalAlign::Top,
    );
    let b =
        t.b.text_paragraphs(
            Parent::Page(p1),
            Rect::new(211.0, 404.0, 165.0, 336.0),
            vec![Paragraph::default()],
        )
        .expect("col2");
    t.name(b, "lead_story_column_2");
    let c =
        t.b.text_paragraphs(
            Parent::Page(p2),
            Rect::new(36.0, 36.0, 540.0, 300.0),
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
    t.line(
        Owner::Page(p2),
        Rect::new(36.0, 350.0, 540.0, 0.0),
        ORANGE,
        2.0,
    );
    t.text(
        p2,
        Rect::new(36.0, 362.0, 540.0, 40.0),
        "Second Story Headline",
        props(SANS, 22.0, true, false, NAVY),
        para(Align::Left, 1.05, 0.0),
        Some("second_headline"),
        VerticalAlign::Bottom,
    );
    let s = t.text(
        p2,
        Rect::new(36.0, 410.0, 540.0, 330.0),
        "Write the second story here.",
        props(SERIF, 11.0, false, false, INK),
        para(Align::Justify, 1.35, 8.0),
        Some("second_story"),
        VerticalAlign::Top,
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
    let f =
        t.b.doc()
            .placeholder("sidebar_heading")
            .expect("placeholder");
    t.describe(f, "Heading for the sidebar");
    t.finish()
}

/// Build a built-in template document.
pub fn builtin(name: &str) -> Option<Document> {
    Some(match name {
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
        if let Some(d) = builtin(name) {
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
            // Only empty-placeholder warnings before filling, and bands that
            // run to the page edge (desktop-printer margin check).
            for w in pubrs_core::validate::validate(&doc) {
                assert!(
                    matches!(
                        w.code,
                        pubrs_core::WarningCode::EmptyPlaceholder
                            | pubrs_core::WarningCode::ObjectInUnprintableArea
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
            assert_eq!(
                doc.scheme,
                Some(SchemeId(pubrs_core::swatch::DEFAULT_SCHEME.into()))
            );
            let uses: usize = doc
                .swatches
                .iter()
                .map(|s| doc.swatch_uses(s.id).len())
                .sum();
            assert!(uses > 0, "{name} should use scheme slots");
            // Default values equal the original literals.
            let slot = |s: SchemeSlot| doc.resolve_solid(&s.paint()).rgba;
            assert_eq!(slot(SchemeSlot::Main), Color::rgb(0x1E, 0x3A, 0x5F));
            assert_eq!(slot(SchemeSlot::Accent1), Color::rgb(0xC2, 0x41, 0x0C));
            assert_eq!(slot(SchemeSlot::Accent2), Color::rgb(0xFF, 0xF7, 0xED));
            let mut ed = pubrs_core::Editor::new(doc);
            ed.execute(Command::ApplyScheme {
                scheme: SchemeId("coral_reef".into()),
            })
            .unwrap();
            assert_eq!(
                ed.doc().resolve_solid(&SchemeSlot::Main.paint()).rgba,
                Color::rgb(0x0F, 0x5E, 0x73)
            );
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
        assert_eq!(builtin("newsletter"), builtin("newsletter"));
    }
}

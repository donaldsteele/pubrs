//! Document validation.
//!
//! Two levels:
//! - [`check_integrity`]: structural errors (dangling ids, broken chains).
//!   Loading a file that fails this is an error.
//! - [`validate`]: print-readiness warnings that need only the model
//!   (objects off the page, empty placeholders, low-resolution images).
//!   Text overflow and missing fonts need layout and are added by `pubrs-api`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::color::{Color, contrast_ratio};
use crate::geom::{PT_PER_INCH, Rect};
use crate::id::{FrameId, PageId};
use crate::model::{Document, Frame, FrameContent, ImageFit, ImageFormat, Owner, PlaceholderKind};
use crate::style::StyleKind;
use crate::swatch::{Ink, Paint, ResolvedPaint, SwatchValue, extra_paints};
use crate::table::{Cell, TableFrame};

/// Images below this effective resolution trigger a warning.
pub const MIN_IMAGE_DPI: f64 = 150.0;

/// Kinds of validation warnings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum WarningCode {
    /// Text does not fit in its frame chain.
    Overflow,
    /// A font used by the document is not installed; a fallback is used.
    MissingFont,
    /// An image prints at less than 150 dpi.
    LowResolutionImage,
    /// A frame lies entirely outside its page.
    OffPage,
    /// A frame crosses the page edge but extends past the bleed area.
    BeyondBleed,
    /// A template placeholder has not been filled.
    EmptyPlaceholder,
    /// An image frame with no picture.
    EmptyImageFrame,
    /// A paragraph asks for hyphenation in a language with no bundled
    /// patterns; its words are not hyphenated.
    HyphenationUnavailable,
    /// Characters that no bundled or installed font covers (drawn as
    /// missing-glyph boxes).
    MissingGlyphs,
    /// An RGB colour in a job meant for CMYK printing (commercial checks only).
    RgbInCmykJob,
    /// Text smaller than 6 pt.
    TextTooSmall,
    /// Text with too little contrast against its background.
    LowContrastText,
    /// A visible object within the margin many desktop printers cannot print.
    ObjectInUnprintableArea,
    /// A table with empty cells.
    EmptyTableCells,
    /// A picture with no alternative text (needed for tagged PDF).
    MissingAltText,
}

/// A validation warning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Warning {
    pub code: WarningCode,
    pub message: String,
    /// How to fix it.
    pub hint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<PageId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<FrameId>,
    /// All the objects involved when the issue spans several (`frame` is
    /// the primary one).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<FrameId>,
}

/// Structural problems, one message each. Empty means the document is sound.
pub fn check_integrity(doc: &Document) -> Vec<String> {
    let mut errs = Vec::new();
    if doc.pages.is_empty() {
        errs.push("document has no pages".into());
    }
    // Every frame is listed exactly once by exactly one container.
    let mut listed: BTreeMap<FrameId, usize> = BTreeMap::new();
    let mut lists: Vec<(&str, &Vec<FrameId>)> = Vec::new();
    for p in doc.pages.iter() {
        lists.push(("page", &p.frames));
    }
    for m in doc.masters.iter() {
        lists.push(("master", &m.frames));
        if let Some(_dup) = doc.masters.iter().filter(|o| o.id == m.id).nth(1) {
            errs.push(format!("duplicate master id {}", m.id));
        }
    }
    for f in doc.frames.values() {
        if let FrameContent::Group(g) = &f.content {
            lists.push(("group", &g.children));
        }
    }
    for (kind, list) in &lists {
        for id in *list {
            *listed.entry(*id).or_default() += 1;
            if !doc.frames.contains_key(id) {
                errs.push(format!("{kind} lists missing frame {id}"));
            }
        }
    }
    for id in doc.frames.keys() {
        match listed.get(id) {
            None => errs.push(format!("frame {id} is not on any page, master or group")),
            Some(n) if *n > 1 => errs.push(format!("frame {id} is listed {n} times")),
            _ => {}
        }
    }
    let page_ids: BTreeSet<PageId> = doc.pages.iter().map(|p| p.id).collect();
    if page_ids.len() != doc.pages.len() {
        errs.push("duplicate page ids".into());
    }
    for p in doc.pages.iter() {
        if let Some(m) = p.master
            && doc.master(m).is_none()
        {
            errs.push(format!("page {} uses missing master {m}", p.id));
        }
    }
    for (id, f) in &doc.frames {
        if f.id != *id {
            errs.push(format!("frame key {id} does not match its id {}", f.id));
        }
        match &f.content {
            FrameContent::Text(t) => match doc.story(t.story) {
                Some(s) if s.frames.contains(id) => {}
                Some(_) => errs.push(format!("frame {id} is missing from story {}", t.story)),
                None => errs.push(format!("frame {id} uses missing story {}", t.story)),
            },
            FrameContent::Image(i) => {
                if let Some(a) = i.asset
                    && !doc.assets.contains_key(&a)
                {
                    errs.push(format!("frame {id} uses missing asset {a}"));
                }
            }
            FrameContent::Table(t) => {
                if let Err(e) = t.check() {
                    errs.push(format!("table {id}: {e}"));
                }
                for c in &t.cells {
                    match doc.story(c.story) {
                        Some(s) if s.frames == [*id] => {}
                        Some(_) => errs.push(format!(
                            "table {id}: story {} must list only the table",
                            c.story
                        )),
                        None => errs.push(format!("table {id} uses missing story {}", c.story)),
                    }
                }
            }
            _ => {}
        }
    }
    for (id, s) in &doc.stories {
        if s.id != *id {
            errs.push(format!("story key {id} does not match its id {}", s.id));
        }
        if s.frames.is_empty() {
            errs.push(format!("story {id} has no frames"));
        }
        if s.paragraphs.is_empty() {
            errs.push(format!("story {id} has no paragraphs"));
        }
        for f in &s.frames {
            if doc.story_of(*f) != Some(*id) && doc.table_of_story(*id) != Some(*f) {
                errs.push(format!(
                    "story {id} lists frame {f} which shows another story"
                ));
            }
        }
        for p in &s.paragraphs {
            if let Some(st) = p.style
                && doc
                    .styles
                    .get(st)
                    .is_none_or(|s| s.kind != StyleKind::Paragraph)
            {
                errs.push(format!("story {id} uses missing paragraph style {st}"));
            }
            for r in &p.runs {
                if let Some(st) = r.style
                    && doc
                        .styles
                        .get(st)
                        .is_none_or(|s| s.kind != StyleKind::Character)
                {
                    errs.push(format!("story {id} uses missing character style {st}"));
                }
            }
        }
    }
    match doc.styles.get(doc.styles.default_paragraph) {
        Some(s) if s.kind == StyleKind::Paragraph => {}
        _ => errs.push("default paragraph style is missing".into()),
    }
    for (id, a) in &doc.assets {
        if a.id != *id {
            errs.push(format!("asset key {id} does not match its id {}", a.id));
        }
    }
    errs.extend(doc.dangling_swatches());
    errs
}

/// Effective print resolution of an image frame in dots per inch, if it shows a raster image.
pub fn effective_dpi(doc: &Document, frame: FrameId) -> Option<f64> {
    let f = doc.frame(frame)?;
    let img = f.as_image()?;
    let asset = doc.assets.get(&img.asset?)?;
    if asset.format == ImageFormat::Svg {
        return None;
    }
    let c = img.crop;
    let cw = f64::from(asset.pixel_width) * (1.0 - c.left - c.right);
    let ch = f64::from(asset.pixel_height) * (1.0 - c.top - c.bottom);
    if cw <= 0.0 || ch <= 0.0 || f.rect.width <= 0.0 || f.rect.height <= 0.0 {
        return None;
    }
    // Points per source pixel along each axis.
    let sx = f.rect.width / cw;
    let sy = f.rect.height / ch;
    let s = match img.fit {
        ImageFit::Fill => sx.max(sy),
        ImageFit::Fit => sx.min(sy),
        ImageFit::Stretch => sx.max(sy),
    };
    Some(PT_PER_INCH / s)
}

/// Text smaller than this (points) is flagged as hard to read in print.
pub const MIN_TEXT_PT: f64 = 6.0;

/// Objects closer than this to the page edge (points; 0.25 in, a typical
/// desktop printer margin) may be cut off on a desktop printer.
pub const UNPRINTABLE_MARGIN_PT: f64 = 18.0;

/// Text whose contrast ratio against its background is below this is
/// flagged (WCAG's minimum for large text).
pub const MIN_CONTRAST_RATIO: f64 = 3.0;

/// How many empty table cells a warning names.
const EMPTY_CELLS_NAMED: usize = 10;

/// What the model-level checks assume about the job.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ValidateOptions {
    /// The document is going to a commercial printer in CMYK: RGB colours
    /// are flagged, and desktop-printer margins are not.
    pub commercial: bool,
}

/// Model-level warnings, ordered by page then frame.
pub fn validate(doc: &Document) -> Vec<Warning> {
    validate_with(doc, &ValidateOptions::default())
}

/// [`validate`] with options: every page, then the document-level warnings.
pub fn validate_with(doc: &Document, opts: &ValidateOptions) -> Vec<Warning> {
    let mut out = Vec::new();
    for page in doc.pages.iter() {
        out.extend(validate_page(doc, page.id, opts));
    }
    out.extend(validate_document(doc, opts));
    out
}

/// The model-level warnings of the frames on one page (none if the page does
/// not exist). Depends only on those frames, their stories, the page setup,
/// styles, swatches and assets, so a caller can cache it per page.
pub fn validate_page(doc: &Document, page_id: PageId, opts: &ValidateOptions) -> Vec<Warning> {
    let mut out = Vec::new();
    if !doc.pages.iter().any(|p| p.id == page_id) {
        return out;
    }
    let page_box = Rect::new(0.0, 0.0, doc.setup.size.width, doc.setup.size.height);
    let bleed_box = page_box.outset(doc.setup.bleed);
    let printable_box = page_box.outset(-UNPRINTABLE_MARGIN_PT);
    let desktop_margins = !opts.commercial && doc.setup.bleed == 0.0 && doc.setup.sheet.is_none();
    // The master's frames lie beneath the page's own.
    let master = doc
        .pages
        .iter()
        .find(|p| p.id == page_id)
        .and_then(|p| p.master);
    let order: Vec<FrameId> = master
        .map(|m| doc.paint_order(Owner::Master(m)))
        .unwrap_or_default()
        .into_iter()
        .chain(doc.paint_order(Owner::Page(page_id)))
        .collect();
    for id in doc.all_frames_of(Owner::Page(page_id)) {
        let Some(f) = doc.frame(id) else { continue };
        let warn = |code, message: String, hint: &str| Warning {
            code,
            message,
            hint: hint.to_string(),
            page: Some(page_id),
            frame: Some(id),
            frames: Vec::new(),
        };
        let in_group = matches!(doc.parent_of(id), Some(crate::model::Parent::Group(_)));
        let b = f.bounds();
        let mut edge_reported = false;
        if !in_group {
            if !b.intersects(&page_box) && !(b.width == 0.0 || b.height == 0.0) {
                edge_reported = true;
                out.push(warn(
                    WarningCode::OffPage,
                    "frame lies entirely outside the page".into(),
                    "move it onto the page or delete it",
                ));
            } else if !bleed_box.contains_rect(&b.outset(-0.01)) {
                edge_reported = true;
                out.push(warn(
                    WarningCode::BeyondBleed,
                    format!(
                        "frame extends {:.1}pt past the bleed area",
                        overhang(&b, &bleed_box)
                    ),
                    "keep frames within the page plus bleed (set bleed with doc_set_bleed)",
                ));
            }
        }
        if let Some(ph) = &f.placeholder {
            let empty = match (ph.kind, &f.content) {
                (PlaceholderKind::Text, FrameContent::Text(t)) => doc
                    .story(t.story)
                    .is_none_or(|s| s.is_empty() || s.plain_text().trim() == ph.hint.trim()),
                (PlaceholderKind::Image, FrameContent::Image(i)) => i.asset.is_none(),
                _ => false,
            };
            if empty {
                out.push(warn(
                    WarningCode::EmptyPlaceholder,
                    format!("placeholder {:?} has not been filled", ph.name),
                    "set its text with text_set or place an image with image_place",
                ));
            }
        } else if let FrameContent::Image(i) = &f.content
            && i.asset.is_none()
        {
            out.push(warn(
                WarningCode::EmptyImageFrame,
                "picture frame has no image".into(),
                "place an image with image_place or delete the frame",
            ));
        }
        if let Some(dpi) = effective_dpi(doc, id)
            && dpi < MIN_IMAGE_DPI
        {
            out.push(warn(
                WarningCode::LowResolutionImage,
                format!("image prints at {dpi:.0} dpi (below {MIN_IMAGE_DPI:.0})"),
                "use a higher-resolution image or make the frame smaller",
            ));
        }

        // Design checks.
        let runs = run_infos(doc, f);
        if opts.commercial
            && let Some(m) = rgb_uses(doc, f, &runs)
        {
            out.push(warn(
                WarningCode::RgbInCmykJob,
                format!("RGB colour in a CMYK job: {m}"),
                "use a CMYK or spot swatch (Design > Swatches, swatch_create) instead of an RGB colour",
            ));
        }
        if let Some(m) = small_text(f, &runs) {
            out.push(warn(
                WarningCode::TextTooSmall,
                m,
                "raise the font size to 6 pt or more (Text > Character…, or the Size field in the Properties panel)",
            ));
        }
        let pos = order.iter().position(|o| *o == id).unwrap_or(0);
        if let Some(m) = low_contrast(doc, f, &runs, &order[..pos]) {
            out.push(warn(
                WarningCode::LowContrastText,
                m,
                "darken the text or lighten what is behind it (Text > Character…, or the frame Fill in the Properties panel); aim for a contrast of 4.5:1 or more",
            ));
        }
        if desktop_margins
            && !edge_reported
            && has_visible_content(doc, f)
            && !printable_box.contains_rect(&b)
        {
            out.push(warn(
                WarningCode::ObjectInUnprintableArea,
                format!(
                    "frame is within {UNPRINTABLE_MARGIN_PT:.0}pt of the page edge, which many desktop printers cannot print"
                ),
                "move it inside the page margins, or print through a commercial printer with bleed",
            ));
        }
        if let FrameContent::Table(t) = &f.content
            && let Some(m) = empty_cells(doc, t)
        {
            out.push(warn(
                WarningCode::EmptyTableCells,
                m,
                "type text in the empty cells, or delete the unused rows and columns",
            ));
        }
        if let FrameContent::Image(i) = &f.content
            && i.asset.is_some()
            && i.alt_text.trim().is_empty()
        {
            out.push(warn(
                WarningCode::MissingAltText,
                "picture has no alternative text".into(),
                "select the picture and fill in Alt text in the Properties panel (image_set_alt_text); tagged PDFs need it",
            ));
        }
    }
    out
}

/// Warnings that belong to stories rather than frames: paragraphs asking
/// for hyphenation in a language with no patterns.
pub fn validate_document(doc: &Document, _opts: &ValidateOptions) -> Vec<Warning> {
    let mut out = Vec::new();
    for story in doc.stories.values() {
        let mut seen: Vec<String> = Vec::new();
        for p in &story.paragraphs {
            let a = doc.styles.resolve_para(p);
            if !a.hyphenate || hyphenation_supported(&a.language) {
                continue;
            }
            if seen.contains(&a.language) {
                continue;
            }
            seen.push(a.language.clone());
            let frame = story.frames.first().copied();
            let page = frame.and_then(|f| match doc.owner_of(f) {
                Some(Owner::Page(p)) => Some(p),
                _ => None,
            });

            out.push(Warning {
                code: WarningCode::HyphenationUnavailable,
                message: format!(
                    "no hyphenation patterns for language {:?}; words are not hyphenated",
                    a.language
                ),
                hint: format!(
                    "set the paragraph language to one of: {}",
                    HYPHENATION_LANGUAGES.join(", ")
                ),
                page,
                frame,
                frames: Vec::new(),
            });
        }
    }
    out
}

/// One run of visible text, resolved.
struct RunInfo {
    /// Index of the table cell it is in.
    cell: Option<usize>,
    size: f64,
    ink: Ink,
}

/// The non-blank runs of a text frame or table, resolved through the styles.
fn run_infos(doc: &Document, f: &Frame) -> Vec<RunInfo> {
    let mut out = Vec::new();
    let mut story = |sid, cell: Option<usize>| {
        let Some(s) = doc.story(sid) else { return };
        for p in &s.paragraphs {
            for r in &p.runs {
                if r.text.trim().is_empty() {
                    continue;
                }
                let a = doc.styles.resolve_char(p, r);
                out.push(RunInfo {
                    cell,
                    size: a.font_size,
                    ink: a.color,
                });
            }
        }
    };
    match &f.content {
        FrameContent::Text(t) => story(t.story, None),
        FrameContent::Table(t) => {
            for (i, c) in t.cells.iter().enumerate() {
                story(c.story, Some(i));
            }
        }
        _ => {}
    }
    out
}

fn cell_at(f: &Frame, cell: usize) -> Option<&Cell> {
    f.as_table()?.cells.get(cell)
}

fn is_rgb_paint(doc: &Document, p: &Paint) -> bool {
    match p {
        Paint::Color(c) => c.a > 0,
        Paint::Swatch { swatch, .. } => {
            matches!(doc.swatch(*swatch), Some(s) if matches!(s.value, SwatchValue::Rgb(_)))
        }
        Paint::Gradient { stops, .. } => {
            stops.iter().any(|s| is_rgb_paint(doc, &s.color.to_paint()))
        }
    }
}

/// Where a frame uses RGB colour, if anywhere. Default black text is not
/// counted (every text frame has it); see ADR 0027.
fn rgb_uses(doc: &Document, f: &Frame, runs: &[RunInfo]) -> Option<String> {
    let mut uses: Vec<&str> = Vec::new();
    let mut add = |l: &'static str| {
        if !uses.contains(&l) {
            uses.push(l);
        }
    };
    if let Some(p) = &f.style.fill
        && is_rgb_paint(doc, p)
    {
        add(if p.is_gradient() {
            "fill (gradient stop)"
        } else {
            "fill"
        });
    }
    if let Some(s) = &f.style.stroke
        && is_rgb_paint(doc, &s.color)
    {
        add("outline");
    }
    for (what, p, _) in extra_paints(f) {
        if is_rgb_paint(doc, p) {
            add(what);
        }
    }
    if runs.iter().any(|r| match r.ink {
        Ink::Rgba(c) => c.a > 0 && !(c.a == 255 && c.r == 0 && c.g == 0 && c.b == 0),
        Ink::Swatch { id, .. } => is_rgb_paint(doc, &Paint::swatch(id)),
    }) {
        add("text colour");
    }
    (!uses.is_empty()).then(|| uses.join(", "))
}

/// The smallest text size in a frame, if below [`MIN_TEXT_PT`].
fn small_text(f: &Frame, runs: &[RunInfo]) -> Option<String> {
    if let FrameContent::WordArt(w) = &f.content {
        return (w.size < MIN_TEXT_PT && !w.text.trim().is_empty()).then(|| {
            format!(
                "WordArt is {:.1}pt, smaller than the {MIN_TEXT_PT:.0}pt minimum for print",
                w.size
            )
        });
    }
    let small = runs
        .iter()
        .filter(|r| r.size < MIN_TEXT_PT)
        .min_by(|a, b| a.size.total_cmp(&b.size))?;
    let at = small
        .cell
        .and_then(|c| cell_at(f, c))
        .map(|c| format!(" (table cell row {}, column {})", c.row + 1, c.col + 1))
        .unwrap_or_default();
    Some(format!(
        "text is {:.1}pt{at}, smaller than the {MIN_TEXT_PT:.0}pt minimum for print",
        small.size
    ))
}

/// Backgrounds (opaque, over white) a paint gives when it is opaque enough
/// to count: the colour, or every stop of a gradient.
fn backdrop(doc: &Document, p: &Paint, opacity: f64) -> Option<Vec<Color>> {
    match doc.resolve_paint(p) {
        ResolvedPaint::Solid(i) => {
            let a = f64::from(i.rgba.a) / 255.0 * opacity;
            (a >= 0.5).then(|| vec![i.rgba.over(Color::WHITE, opacity)])
        }
        ResolvedPaint::Gradient(g) => {
            let n = g.stops.len().max(1) as f64;
            let mean = g
                .stops
                .iter()
                .map(|(_, i)| f64::from(i.rgba.a) / 255.0)
                .sum::<f64>()
                / n
                * opacity;
            (mean >= 0.5).then(|| {
                g.stops
                    .iter()
                    .map(|(_, i)| i.rgba.over(Color::WHITE, opacity))
                    .collect()
            })
        }
    }
}

enum Under {
    Nothing,
    Fill(Vec<Color>),
    Unknown,
}

/// What shows behind the centre of `f`: the topmost frame below it whose
/// bounds hold that point and that is a picture (unknown) or has an opaque
/// fill.
fn under(doc: &Document, f: &Frame, below: &[FrameId]) -> Under {
    let c = f.bounds().center();
    for id in below.iter().rev() {
        let Some(g) = doc.frame(*id) else { continue };
        if !g.bounds().contains(c) {
            continue;
        }
        if g.as_image().is_some_and(|i| i.asset.is_some()) {
            return Under::Unknown;
        }
        let opacity = g.style.opacity.clamp(0.0, 1.0);
        if let Some(bg) = g
            .style
            .fill
            .as_ref()
            .and_then(|p| backdrop(doc, p, opacity))
        {
            return Under::Fill(bg);
        }
    }
    Under::Nothing
}

/// The worst text/background contrast in a frame, if below
/// [`MIN_CONTRAST_RATIO`].
fn low_contrast(doc: &Document, f: &Frame, runs: &[RunInfo], below: &[FrameId]) -> Option<String> {
    if runs.is_empty() {
        return None;
    }
    let opacity = f.style.opacity.clamp(0.0, 1.0);
    let frame_bg = match f
        .style
        .fill
        .as_ref()
        .and_then(|p| backdrop(doc, p, opacity))
    {
        Some(bg) => Some(bg),
        // Nothing opaque of its own: the topmost opaque fill under its
        // centre, if any. A picture there has unknown colours: no verdict.
        None => match under(doc, f, below) {
            Under::Unknown => return None,
            Under::Fill(bg) => Some(bg),
            Under::Nothing => None,
        },
    };
    let mut worst: Option<(f64, Color, Color)> = None;
    for r in runs {
        let fg = doc.resolve_ink(r.ink).rgba;
        let cell_bg = r
            .cell
            .and_then(|c| cell_at(f, c))
            .and_then(|c| c.fill.as_ref())
            .and_then(|p| backdrop(doc, p, opacity));
        let bgs = cell_bg
            .or_else(|| frame_bg.clone())
            .unwrap_or_else(|| vec![Color::WHITE]);
        for bg in bgs {
            let shown = fg.over(bg, opacity);
            let ratio = contrast_ratio(shown, bg);
            if worst.is_none_or(|(w, _, _)| ratio < w) {
                worst = Some((ratio, fg, bg));
            }
        }
    }
    let (ratio, fg, bg) = worst?;
    (ratio < MIN_CONTRAST_RATIO).then(|| {
        format!(
            "text colour {fg} on {bg} has a contrast ratio of {ratio:.1}:1 (below {MIN_CONTRAST_RATIO:.1}:1)"
        )
    })
}

/// Whether something would be drawn: text, a picture, a filled or stroked
/// shape, WordArt, a table.
fn has_visible_content(doc: &Document, f: &Frame) -> bool {
    match &f.content {
        FrameContent::Text(t) => doc
            .story(t.story)
            .is_some_and(|s| !s.plain_text().trim().is_empty()),
        FrameContent::Image(i) => i.asset.is_some(),
        FrameContent::Shape(_) => f.style.fill.is_some() || f.style.stroke.is_some(),
        FrameContent::WordArt(w) => !w.text.trim().is_empty(),
        FrameContent::Table(_) => true,
        FrameContent::Group(_) => false,
    }
}

fn empty_cells(doc: &Document, t: &TableFrame) -> Option<String> {
    let empty: Vec<&Cell> = t
        .cells
        .iter()
        .filter(|c| {
            doc.story(c.story)
                .is_none_or(|s| s.plain_text().trim().is_empty())
        })
        .collect();
    if empty.is_empty() {
        return None;
    }
    let named: Vec<String> = empty
        .iter()
        .take(EMPTY_CELLS_NAMED)
        .map(|c| format!("row {}, col {}", c.row + 1, c.col + 1))
        .collect();
    let more = empty.len().saturating_sub(EMPTY_CELLS_NAMED);
    Some(format!(
        "{} empty table cell{}: {}{}",
        empty.len(),
        if empty.len() == 1 { "" } else { "s" },
        named.join("; "),
        if more > 0 {
            format!("; and {more} more")
        } else {
            String::new()
        }
    ))
}

/// ISO 639-1 codes with bundled hyphenation patterns (see `ASSETS.md`).
/// `pubrs-layout` tests that this list matches the enabled `hypher`
/// languages.
pub const HYPHENATION_LANGUAGES: &[&str] = &["en", "de", "es", "fr", "pl", "pt"];

/// Whether the BCP 47 tag (e.g. "en-US", "de_DE") has bundled patterns.
pub fn hyphenation_supported(tag: &str) -> bool {
    let primary = tag
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    HYPHENATION_LANGUAGES.contains(&primary.as_str())
}

fn overhang(b: &Rect, bounds: &Rect) -> f64 {
    [
        bounds.left() - b.left(),
        bounds.top() - b.top(),
        b.right() - bounds.right(),
        b.bottom() - bounds.bottom(),
    ]
    .into_iter()
    .fold(0.0, f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;
    use crate::history::Editor;
    use crate::id::{AssetId, SeqIds};
    use crate::model::*;

    fn editor() -> (SeqIds, Editor) {
        let mut ids = SeqIds::new(11);
        let doc = Document::new(&mut ids, PageSetup::default());
        (ids, Editor::new(doc))
    }

    #[test]
    fn fresh_document_is_sound_and_clean() {
        let (_, ed) = editor();
        assert!(check_integrity(ed.doc()).is_empty());
        assert!(validate(ed.doc()).is_empty());
    }

    #[test]
    fn warns_off_page_and_low_dpi() {
        let (mut ids, mut ed) = editor();
        let page = ed.doc().pages[0].id;
        let asset = AssetId::new(&mut ids);
        ed.execute(Command::AddAsset {
            asset: Asset {
                id: asset,
                name: "small.png".into(),
                format: ImageFormat::Png,
                pixel_width: 100,
                pixel_height: 100,
            },
            data: crate::Bytes::new(vec![1]),
        })
        .unwrap();
        let img = FrameId::new(&mut ids);
        ed.execute(Command::AddFrame {
            parent: Parent::Page(page),
            index: None,
            frame: Frame::new(
                img,
                Rect::new(36.0, 36.0, 144.0, 144.0),
                FrameContent::Image(ImageFrame::new(Some(asset))),
            ),
            story: None,
        })
        .unwrap();
        let off = FrameId::new(&mut ids);
        ed.execute(Command::AddFrame {
            parent: Parent::Page(page),
            index: None,
            frame: Frame::new(
                off,
                Rect::new(1000.0, 1000.0, 10.0, 10.0),
                FrameContent::Shape(ShapeFrame {
                    shape: Shape::Ellipse,
                }),
            ),
            story: None,
        })
        .unwrap();
        let codes: Vec<WarningCode> = validate(ed.doc()).iter().map(|w| w.code).collect();
        assert_eq!(
            codes,
            vec![
                WarningCode::LowResolutionImage,
                WarningCode::MissingAltText,
                WarningCode::OffPage
            ]
        );
        assert!((effective_dpi(ed.doc(), img).unwrap() - 50.0).abs() < 1e-9);
        assert!(check_integrity(ed.doc()).is_empty());
    }

    #[test]
    fn integrity_catches_dangling_references() {
        let (mut ids, mut ed) = editor();
        let mut doc = ed.doc().clone();
        doc.pages_mut()[0].frames.push(FrameId::new(&mut ids));
        assert_eq!(check_integrity(&doc).len(), 1);
        ed.reset(doc);
    }
}

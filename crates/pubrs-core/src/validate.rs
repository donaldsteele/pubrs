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

use crate::geom::{PT_PER_INCH, Rect};
use crate::id::{FrameId, PageId};
use crate::model::{Document, FrameContent, ImageFit, ImageFormat, Owner, PlaceholderKind};
use crate::style::StyleKind;

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

/// Model-level warnings, ordered by page then frame.
pub fn validate(doc: &Document) -> Vec<Warning> {
    let mut out = Vec::new();
    let page_box = Rect::new(0.0, 0.0, doc.setup.size.width, doc.setup.size.height);
    let bleed_box = page_box.outset(doc.setup.bleed);
    for page in doc.pages.iter() {
        for id in doc.all_frames_of(Owner::Page(page.id)) {
            let Some(f) = doc.frame(id) else { continue };
            let warn = |code, message: String, hint: &str| Warning {
                code,
                message,
                hint: hint.to_string(),
                page: Some(page.id),
                frame: Some(id),
            };
            let in_group = matches!(doc.parent_of(id), Some(crate::model::Parent::Group(_)));
            let b = f.bounds();
            if !in_group {
                if !b.intersects(&page_box) && !(b.width == 0.0 || b.height == 0.0) {
                    out.push(warn(
                        WarningCode::OffPage,
                        "frame lies entirely outside the page".into(),
                        "move it onto the page or delete it",
                    ));
                } else if !bleed_box.contains_rect(&b.outset(-0.01)) {
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
        }
    }
    // Paragraphs that ask for hyphenation in a language with no patterns.
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
            });
        }
    }
    out
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
            vec![WarningCode::LowResolutionImage, WarningCode::OffPage]
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

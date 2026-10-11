//! Frames in transit: the clipboard's payload and `frame_copy_to_doc`
//! (ADR 0025). The payload is untrusted input (another program, or another
//! machine, may have put it on the clipboard): it is size-checked before and
//! after parsing, its images are re-read from their bytes, and a paste is
//! tried on a copy of the document and checked for integrity before it is
//! applied.

use std::collections::{BTreeMap, BTreeSet};

use pubrs_core::{
    Color, Command, DocumentId, Frame, FrameContent, FrameId, Owner, Paint, Paragraph, Parent,
    Story, StoryId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::images::MAX_IMAGE_BYTES;
use crate::error::{ApiError, ApiResult};
use crate::session::{Changed, Session};

/// Most frames one paste adds.
pub const MAX_CLIP_FRAMES: usize = 5_000;
/// Most stories (text frames, table cells) in a payload.
pub const MAX_CLIP_STORIES: usize = 20_000;
/// Most images in a payload.
pub const MAX_CLIP_ASSETS: usize = 256;
/// Most styles and most swatches in a payload.
pub const MAX_CLIP_STYLES: usize = 2_000;
/// Most paragraphs, runs and text bytes over all stories of a payload.
pub const MAX_CLIP_PARAGRAPHS: usize = 200_000;
pub const MAX_CLIP_RUNS: usize = 400_000;
pub const MAX_CLIP_TEXT_BYTES: usize = 32 * 1024 * 1024;
/// Largest payload JSON, in bytes.
pub const MAX_CLIP_JSON: usize = 64 * 1024 * 1024;
/// Longest frame or image name kept.
const MAX_NAME: usize = 200;

/// An asset travelling with copied frames. Only `data` is believed: the
/// format and size are read from the bytes on paste.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipAsset {
    pub asset: pubrs_core::Asset,
    pub data: pubrs_core::Bytes,
}

/// A swatch the copied frames use: id and name (to find it again), colour
/// (for a document that has no such swatch).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipSwatch {
    pub id: pubrs_core::SwatchId,
    pub name: String,
    pub color: Color,
}

/// Frames in transit: what the GUI puts on the clipboard and what
/// `frame_copy_to_doc` moves between documents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipPayload {
    /// Format version, 1.
    pub pubrs_clip: u32,
    /// The copied frames that are not inside another copied frame.
    pub roots: Vec<FrameId>,
    /// Roots and their group members, parents before children.
    pub frames: Vec<Frame>,
    /// Text frame stories and table cell stories.
    pub stories: Vec<Story>,
    pub assets: Vec<ClipAsset>,
    /// Styles the stories use, base styles first.
    pub styles: Vec<pubrs_core::Style>,
    /// Swatches the frames or text use.
    pub swatches: Vec<ClipSwatch>,
}

impl ClipPayload {
    /// The payload as JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Parse JSON written by [`ClipPayload::to_json`], refusing anything
    /// over the caps.
    pub fn parse(s: &str) -> Result<Self, String> {
        if s.len() > MAX_CLIP_JSON {
            return Err(format!(
                "the clipboard data is over {} MB",
                MAX_CLIP_JSON >> 20
            ));
        }
        let p: Self = serde_json::from_str(s).map_err(|e| e.to_string())?;
        p.check()?;
        Ok(p)
    }

    /// [`ClipPayload::parse`], `None` if it is not (valid) pubrs data.
    pub fn from_json(s: &str) -> Option<Self> {
        Self::parse(s).ok()
    }

    /// Counts against the caps.
    pub fn check(&self) -> Result<(), String> {
        if self.pubrs_clip != 1 {
            return Err("not pubrs clipboard data".into());
        }
        let over = |what: &str, n: usize, max: usize| {
            if n > max {
                Err(format!("{n} {what} (at most {max})"))
            } else {
                Ok(())
            }
        };
        if self.frames.is_empty() {
            return Err("no frames".into());
        }
        over("frames", self.frames.len(), MAX_CLIP_FRAMES)?;
        over("stories", self.stories.len(), MAX_CLIP_STORIES)?;
        over("images", self.assets.len(), MAX_CLIP_ASSETS)?;
        over("styles", self.styles.len(), MAX_CLIP_STYLES)?;
        over("swatches", self.swatches.len(), MAX_CLIP_STYLES)?;
        over("roots", self.roots.len(), MAX_CLIP_FRAMES)?;
        let (mut paras, mut runs, mut bytes) = (0usize, 0usize, 0usize);
        for s in &self.stories {
            paras += s.paragraphs.len();
            for p in &s.paragraphs {
                runs += p.runs.len();
                bytes += p.runs.iter().map(|r| r.text.len()).sum::<usize>();
            }
            if paras > MAX_CLIP_PARAGRAPHS || runs > MAX_CLIP_RUNS || bytes > MAX_CLIP_TEXT_BYTES {
                return Err("more text than a paste can carry".into());
            }
        }
        Ok(())
    }

    /// The text of the copied text frames and table cells, for a plain-text
    /// fallback (cells: tabs along a row, a line per row).
    pub fn plain_text(&self) -> String {
        let by_story: BTreeMap<StoryId, &Story> = self.stories.iter().map(|s| (s.id, s)).collect();
        let mut out = Vec::new();
        for f in &self.frames {
            if let Some(t) = f.as_text()
                && let Some(s) = by_story.get(&t.story)
                && s.frames.first() == Some(&f.id)
            {
                out.push(s.plain_text());
            }
            if let Some(t) = f.as_table() {
                let mut text = String::new();
                let mut last_row = None;
                for c in &t.cells {
                    if let Some(r) = last_row {
                        text.push(if r == c.row { '\t' } else { '\n' });
                    }
                    last_row = Some(c.row);
                    if let Some(s) = by_story.get(&c.story) {
                        text.push_str(&s.plain_text());
                    }
                }
                out.push(text);
            }
        }
        out.join("\n")
    }
}

/// Every paint a frame carries, including table cell fills and borders.
fn frame_paints_mut(f: &mut Frame) -> Vec<&mut Paint> {
    let mut v = Vec::new();
    if let Some(p) = &mut f.style.fill {
        v.push(p);
    }
    if let Some(s) = &mut f.style.stroke {
        v.push(&mut s.color);
    }
    if let Some(s) = &mut f.style.shadow {
        v.push(&mut s.color);
    }
    match &mut f.content {
        FrameContent::WordArt(w) => {
            v.push(&mut w.fill);
            if let Some(o) = &mut w.outline {
                v.push(&mut o.color);
            }
            if let Some(s) = &mut w.shadow {
                v.push(&mut s.color);
            }
        }
        FrameContent::Table(t) => {
            for c in &mut t.cells {
                if let Some(p) = &mut c.fill {
                    v.push(p);
                }
                for b in c.borders.iter_mut().flatten() {
                    v.push(&mut b.color);
                }
            }
        }
        _ => {}
    }
    v
}

fn story_paints_mut(s: &mut Story) -> Vec<&mut Paint> {
    s.paragraphs
        .iter_mut()
        .flat_map(|p| p.runs.iter_mut())
        .filter_map(|r| r.props.color.as_mut())
        .collect()
}

/// Swap references to swatches the target lacks (or names differently) for
/// their replacement: the target's swatch of that name, or the colour.
fn replace_swatches(p: &mut Paint, repl: &[(pubrs_core::SwatchId, Paint)]) {
    match p {
        Paint::Swatch { swatch, tint } => {
            if let Some((_, r)) = repl.iter().find(|(s, _)| s == swatch) {
                *p = match r {
                    Paint::Color(c) => Paint::Color(pubrs_core::swatch::apply_tint(*c, *tint)),
                    Paint::Swatch { swatch: n, .. } => Paint::Swatch {
                        swatch: *n,
                        tint: *tint,
                    },
                    other => other.clone(),
                };
            }
        }
        Paint::Gradient { .. } => {
            for (id, r) in repl {
                p.replace_swatch(*id, r);
            }
        }
        Paint::Color(_) => {}
    }
}

/// Point paragraph and run styles at the target's styles and replace
/// swatches.
fn remap_paragraphs(
    paragraphs: Vec<Paragraph>,
    style_map: &BTreeMap<pubrs_core::StyleId, Option<pubrs_core::StyleId>>,
    repl: &[(pubrs_core::SwatchId, Paint)],
) -> Vec<Paragraph> {
    let mut paragraphs = paragraphs;
    if paragraphs.is_empty() {
        paragraphs.push(Paragraph::default());
    }
    for p in &mut paragraphs {
        p.style = p.style.and_then(|s| style_map.get(&s).copied().flatten());
        if p.runs.is_empty() {
            p.runs.push(pubrs_core::Run::plain(""));
        }
        for r in &mut p.runs {
            r.style = r.style.and_then(|s| style_map.get(&s).copied().flatten());
            if let Some(c) = &mut r.props.color {
                replace_swatches(c, repl);
            }
        }
    }
    paragraphs
}

/// How a paste lands.
#[derive(Debug, Clone, Copy)]
pub struct PasteAt {
    pub parent: Parent,
    pub dx: f64,
    pub dy: f64,
}

/// What a paste made.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FrameCopyOut {
    /// The new frames (groups stay groups), in the target document.
    pub frame_ids: Vec<FrameId>,
    #[serde(flatten)]
    pub change: Changed,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Offset {
    /// Points to the right.
    pub x: f64,
    /// Points down.
    pub y: f64,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameCopyToDoc {
    /// The document the frames are in.
    pub from_doc_id: DocumentId,
    /// Frames to copy (groups come with their members; members of a copied
    /// group need not be listed).
    pub frame_ids: Vec<FrameId>,
    /// The document that receives the copies. May be the same document.
    pub to_doc_id: DocumentId,
    /// Page of the target document, counted from 0.
    pub page_index: usize,
    /// Shift of the copies in points (default 0, 0).
    #[serde(default)]
    pub offset: Option<Offset>,
}

fn bad(what: impl Into<String>) -> ApiError {
    ApiError::invalid(
        format!("cannot paste: {}", what.into()),
        "copy the frames again, or paste fewer of them",
    )
}

enum StylePlan {
    Existing(pubrs_core::StyleId),
    New(Box<pubrs_core::Style>),
    Drop,
}

impl Session {
    /// Copy frames (with everything they need) out of a document.
    pub fn clip_copy(
        &self,
        doc_id: Option<DocumentId>,
        frame_ids: &[FrameId],
    ) -> ApiResult<ClipPayload> {
        let doc = self.get(doc_id)?.doc();
        if frame_ids.is_empty() {
            return Err(ApiError::invalid(
                "no frames to copy",
                "give at least one frame id",
            ));
        }
        for id in frame_ids {
            if doc.frame(*id).is_none() {
                return Err(ApiError::not_found(
                    "frame",
                    &id.to_string(),
                    "list frames with frame_list",
                ));
            }
        }
        let wanted: BTreeSet<FrameId> = frame_ids.iter().copied().collect();
        let inside_selected = |id: FrameId| {
            let mut cur = id;
            while let Some(Parent::Group(g)) = doc.parent_of(cur) {
                if wanted.contains(&g) {
                    return true;
                }
                cur = g;
            }
            false
        };
        let mut roots: Vec<FrameId> = Vec::new();
        let mut seen_roots = BTreeSet::new();
        for id in frame_ids {
            if seen_roots.insert(*id) && !inside_selected(*id) {
                roots.push(*id);
            }
        }
        // Keep the stacking order of the source.
        let key = |id: &FrameId| {
            let parent = doc.parent_of(*id);
            let page = match doc.owner_of(*id) {
                Some(Owner::Page(p)) => doc.page_index(p).unwrap_or(usize::MAX),
                _ => usize::MAX,
            };
            let pos = parent
                .and_then(|p| doc.children(p))
                .and_then(|l| l.iter().position(|f| f == id))
                .unwrap_or(0);
            (page, pos)
        };
        roots.sort_by_key(key);
        let mut frames: Vec<Frame> = Vec::new();
        for r in &roots {
            for d in doc.descendants(*r) {
                if let Some(f) = doc.frame(d) {
                    frames.push(f.clone());
                }
            }
        }
        if frames.len() > MAX_CLIP_FRAMES {
            return Err(ApiError::invalid(
                format!("more than {MAX_CLIP_FRAMES} frames"),
                "copy fewer frames at a time",
            ));
        }
        let mut stories: Vec<Story> = Vec::new();
        let mut story_seen: BTreeSet<StoryId> = BTreeSet::new();
        let mut assets: Vec<ClipAsset> = Vec::new();
        let mut asset_seen = BTreeSet::new();
        for f in &frames {
            let mut ids: Vec<StoryId> = Vec::new();
            if let Some(t) = f.as_text() {
                ids.push(t.story);
            }
            if let Some(t) = f.as_table() {
                ids.extend(t.cells.iter().map(|c| c.story));
            }
            for sid in ids {
                if story_seen.insert(sid)
                    && let Some(s) = doc.story(sid)
                {
                    stories.push(s.clone());
                }
            }
            if let Some(a) = f.as_image().and_then(|i| i.asset)
                && asset_seen.insert(a)
                && let (Some(meta), Some(blob)) = (doc.assets.get(&a), doc.blobs.get(&a))
            {
                assets.push(ClipAsset {
                    asset: meta.clone(),
                    data: pubrs_core::Bytes(blob.clone()),
                });
            }
        }
        // Styles the text uses, base styles first.
        let mut style_ids: Vec<pubrs_core::StyleId> = Vec::new();
        fn want(
            doc: &pubrs_core::Document,
            id: pubrs_core::StyleId,
            out: &mut Vec<pubrs_core::StyleId>,
        ) {
            if out.contains(&id) || out.len() > MAX_CLIP_STYLES {
                return;
            }
            if let Some(s) = doc.styles.get(id) {
                if let Some(b) = s.based_on {
                    want(doc, b, out);
                }
                out.push(id);
            }
        }
        for s in &stories {
            for p in &s.paragraphs {
                if let Some(id) = p.style {
                    want(doc, id, &mut style_ids);
                }
                for r in &p.runs {
                    if let Some(id) = r.style {
                        want(doc, id, &mut style_ids);
                    }
                }
            }
        }
        let styles: Vec<pubrs_core::Style> = style_ids
            .iter()
            .filter_map(|id| doc.styles.get(*id).cloned())
            .collect();
        // Swatches in use, with their names and full-strength colours.
        let mut swatches: Vec<ClipSwatch> = Vec::new();
        let mut note = |p: &Paint| {
            for id in p.swatch_ids() {
                if !swatches.iter().any(|s| s.id == id)
                    && let Some(sw) = doc.swatch(id)
                {
                    swatches.push(ClipSwatch {
                        id,
                        name: sw.name.clone(),
                        color: doc.resolve_swatch(id, 100.0).rgba,
                    });
                }
            }
        };
        let mut scratch_frames = frames.clone();
        for f in &mut scratch_frames {
            frame_paints_mut(f).into_iter().for_each(|p| note(p));
        }
        let mut scratch_stories = stories.clone();
        for s in &mut scratch_stories {
            story_paints_mut(s).into_iter().for_each(|p| note(p));
        }
        for st in &styles {
            if let Some(p) = &st.character.color {
                note(p);
            }
        }
        Ok(ClipPayload {
            pubrs_clip: 1,
            roots,
            frames,
            stories,
            assets,
            styles,
            swatches,
        })
    }

    /// Paste a payload into a document as one undoable step. Frames,
    /// stories, groups and assets get new ids; images are re-read from their
    /// bytes (and refused if they are not images within the limits) and
    /// reused if the document holds the same bytes; styles and swatches are
    /// matched by name (styles added when missing, swatches turned into their
    /// colours); table cells get stories of their own. The result is tried
    /// on a copy of the document and must pass `check_integrity` before it is
    /// applied.
    pub fn clip_paste(
        &mut self,
        doc_id: Option<DocumentId>,
        payload: &ClipPayload,
        at: PasteAt,
    ) -> ApiResult<FrameCopyOut> {
        payload.check().map_err(bad)?;
        if !at.dx.is_finite() || !at.dy.is_finite() {
            return Err(ApiError::invalid("offset must be finite", "use numbers"));
        }
        let resolved = self.resolve(doc_id)?;
        let by_story: BTreeMap<StoryId, &Story> =
            payload.stories.iter().map(|s| (s.id, s)).collect();
        // ---- the images, read from their bytes ----
        let mut checked: Vec<(pubrs_core::AssetId, pubrs_core::Asset, pubrs_core::Bytes)> =
            Vec::with_capacity(payload.assets.len());
        let mut seen_assets = BTreeSet::new();
        for a in &payload.assets {
            if !seen_assets.insert(a.asset.id) {
                return Err(bad("the same image twice"));
            }
            if a.data.0.len() > MAX_IMAGE_BYTES {
                return Err(bad("an image is larger than 64 MB"));
            }
            let info = pubrs_io::image_info(&a.data.0)
                .map_err(|e| bad(format!("an image is not usable: {e}")))?;
            let name: String = a.asset.name.chars().take(MAX_NAME).collect();
            let meta = pubrs_core::Asset {
                id: a.asset.id,
                name,
                format: info.format,
                pixel_width: info.width,
                pixel_height: info.height,
            };
            checked.push((a.asset.id, meta, a.data.clone()));
        }
        // ---- read the target ----
        let doc = self.get(Some(resolved))?.doc();
        if doc.children(at.parent).is_none() {
            return Err(ApiError::not_found(
                "page",
                &format!("{:?}", at.parent),
                "list pages with page_list",
            ));
        }
        // Swatches: kept when the target has one of that id and name; else
        // the target's swatch of that name; else the colour.
        let mut repl: Vec<(pubrs_core::SwatchId, Paint)> = Vec::new();
        for s in &payload.swatches {
            let same = doc
                .swatch(s.id)
                .is_some_and(|t| t.name.eq_ignore_ascii_case(&s.name));
            if same {
                continue;
            }
            let by_name = doc
                .swatches
                .iter()
                .find(|t| t.name.eq_ignore_ascii_case(&s.name));
            repl.push((
                s.id,
                match by_name {
                    Some(t) => Paint::Swatch {
                        swatch: t.id,
                        tint: 100.0,
                    },
                    None => Paint::Color(s.color),
                },
            ));
        }
        // Styles: by kind and name; one with the id but another name is
        // not adopted.
        let mut style_plan: Vec<(pubrs_core::StyleId, StylePlan)> = Vec::new();
        for s in &payload.styles {
            let m = match doc.styles.by_name(&s.name) {
                Some(t) if t.kind == s.kind => StylePlan::Existing(t.id),
                Some(_) => StylePlan::Drop,
                None => StylePlan::New(Box::new(s.clone())),
            };
            style_plan.push((s.id, m));
        }
        let reuse: Vec<Option<pubrs_core::AssetId>> = checked
            .iter()
            .map(|(_, meta, data)| {
                doc.assets
                    .iter()
                    .find(|(id, m)| {
                        m.pixel_width == meta.pixel_width
                            && m.pixel_height == meta.pixel_height
                            && m.format == meta.format
                            && doc
                                .blobs
                                .get(id)
                                .is_some_and(|b| b.len() == data.0.len() && b[..] == data.0[..])
                    })
                    .map(|(id, _)| *id)
            })
            .collect();
        // ---- allocate every id before building commands (ADR 0005) ----
        let mut commands: Vec<Command> = Vec::new();
        let mut style_map: BTreeMap<pubrs_core::StyleId, Option<pubrs_core::StyleId>> =
            BTreeMap::new();
        for (old, m) in style_plan {
            match m {
                StylePlan::Existing(id) => {
                    style_map.insert(old, Some(id));
                }
                StylePlan::Drop => {
                    style_map.insert(old, None);
                }
                StylePlan::New(mut s) => {
                    let id = pubrs_core::StyleId::new(&mut *self.ids);
                    s.id = id;
                    s.based_on = s
                        .based_on
                        .and_then(|b| style_map.get(&b).copied().flatten());
                    if let Some(p) = &mut s.character.color {
                        replace_swatches(p, &repl);
                    }
                    style_map.insert(old, Some(id));
                    commands.push(Command::AddStyle { style: *s });
                }
            }
        }
        let mut asset_map: BTreeMap<pubrs_core::AssetId, pubrs_core::AssetId> = BTreeMap::new();
        for ((old, meta, data), found) in checked.into_iter().zip(reuse) {
            let id = match found {
                Some(id) => id,
                None => {
                    let id = pubrs_core::AssetId::new(&mut *self.ids);
                    let mut meta = meta;
                    meta.id = id;
                    commands.push(Command::AddAsset { asset: meta, data });
                    id
                }
            };
            asset_map.insert(old, id);
        }
        let mut frame_map: BTreeMap<FrameId, FrameId> = BTreeMap::new();
        for f in &payload.frames {
            let id = FrameId::new(&mut *self.ids);
            if frame_map.insert(f.id, id).is_some() {
                return Err(bad("the same frame twice"));
            }
        }
        let mut story_map: BTreeMap<StoryId, StoryId> = BTreeMap::new();
        for s in &payload.stories {
            story_map.insert(s.id, StoryId::new(&mut *self.ids));
        }
        // ---- frames ----
        let mut links: Vec<(FrameId, FrameId)> = Vec::new();
        let mut final_of: BTreeMap<FrameId, Vec<FrameId>> = BTreeMap::new();
        for f in &payload.frames {
            let new_id = frame_map[&f.id];
            if matches!(f.content, FrameContent::Group(_)) {
                continue;
            }
            let mut nf = f.clone();
            nf.id = new_id;
            nf.locked = false;
            nf.placeholder = None;
            nf.name = nf.name.map(|n| n.chars().take(MAX_NAME).collect());
            nf.rect = nf.rect.translate(at.dx, at.dy);
            for p in frame_paints_mut(&mut nf) {
                replace_swatches(p, &repl);
            }
            final_of.insert(f.id, vec![new_id]);
            if let Some(table) = nf.as_table_mut() {
                // Check the table before making a story for every cell.
                table.check().map_err(bad)?;
                let mut cell_stories = Vec::with_capacity(table.cells.len());
                for cell in &mut table.cells {
                    let sid = StoryId::new(&mut *self.ids);
                    let paragraphs = by_story
                        .get(&cell.story)
                        .map_or_else(Vec::new, |s| s.paragraphs.clone());
                    cell.story = sid;
                    cell_stories.push(Story {
                        id: sid,
                        frames: vec![new_id],
                        paragraphs: remap_paragraphs(paragraphs, &style_map, &repl),
                    });
                }
                commands.push(Command::AddTable {
                    parent: at.parent,
                    index: None,
                    frame: nf,
                    stories: cell_stories,
                });
                continue;
            }
            let mut story = None;
            match &mut nf.content {
                FrameContent::Image(i) => {
                    if let Some(a) = i.asset {
                        i.asset = Some(
                            *asset_map
                                .get(&a)
                                .ok_or_else(|| bad("a picture's image is missing"))?,
                        );
                    }
                }
                FrameContent::Text(t) => {
                    if !(1..=12).contains(&t.columns)
                        || !t.column_gap.is_finite()
                        || t.column_gap < 0.0
                        || !t.insets.is_valid()
                    {
                        return Err(bad("a text frame has invalid columns or insets"));
                    }
                    let src = by_story.get(&t.story).copied();
                    let chain: Vec<FrameId> = src
                        .map(|s| {
                            s.frames
                                .iter()
                                .copied()
                                .filter(|x| frame_map.contains_key(x))
                                .collect()
                        })
                        .unwrap_or_default();
                    let pos = chain.iter().position(|x| *x == f.id);
                    let is_head = pos.is_none_or(|p| p == 0);
                    let sid = if is_head {
                        src.and_then(|s| story_map.get(&s.id).copied())
                            .unwrap_or_else(|| StoryId::new(&mut *self.ids))
                    } else {
                        StoryId::new(&mut *self.ids)
                    };
                    let paragraphs = match (is_head, src) {
                        (true, Some(s)) => s.paragraphs.clone(),
                        _ => Vec::new(),
                    };
                    if let Some(p) = pos
                        && p > 0
                    {
                        links.push((frame_map[&chain[p - 1]], new_id));
                    }
                    t.story = sid;
                    story = Some(Story {
                        id: sid,
                        frames: vec![new_id],
                        paragraphs: remap_paragraphs(paragraphs, &style_map, &repl),
                    });
                }
                _ => {}
            }
            commands.push(Command::AddFrame {
                parent: at.parent,
                index: None,
                frame: nf,
                story,
            });
        }
        for (from, to) in links {
            commands.push(Command::LinkFrames { from, to });
        }
        // Groups, innermost first (children follow their group in
        // `payload.frames`). A group with fewer than two members left is
        // not made: its member stays on the page.
        for f in payload.frames.iter().rev() {
            let FrameContent::Group(g) = &f.content else {
                continue;
            };
            let new_id = frame_map[&f.id];
            let kids: Vec<FrameId> = g
                .children
                .iter()
                .filter_map(|c| final_of.get(c))
                .flatten()
                .copied()
                .collect();
            if kids.len() < 2 {
                final_of.insert(f.id, kids);
                continue;
            }
            commands.push(Command::Group {
                group: new_id,
                frames: kids,
            });
            let mut meta = f.clone();
            if let Some(n) = &f.name {
                commands.push(Command::SetFrameName {
                    frame: new_id,
                    name: Some(n.chars().take(MAX_NAME).collect()),
                });
            }
            for p in frame_paints_mut(&mut meta) {
                replace_swatches(p, &repl);
            }
            if meta.style != pubrs_core::FrameStyle::default() {
                commands.push(Command::SetFrameStyle {
                    frame: new_id,
                    style: meta.style,
                });
            }
            if f.wrap != pubrs_core::TextWrap::default() {
                commands.push(Command::SetWrap {
                    frame: new_id,
                    wrap: f.wrap,
                });
            }
            final_of.insert(f.id, vec![new_id]);
        }
        let frame_ids: Vec<FrameId> = payload
            .roots
            .iter()
            .filter_map(|r| final_of.get(r))
            .flatten()
            .copied()
            .collect();
        let batch = Command::Batch {
            label: "paste".into(),
            commands,
        };
        // Try it on a copy first: whatever the payload held, the document
        // never ends up inconsistent.
        {
            let mut trial = self.get(Some(resolved))?.doc().clone();
            batch.apply(&mut trial)?;
            let errs = pubrs_core::validate::check_integrity(&trial);
            if let Some(first) = errs.first() {
                return Err(bad(format!("the frames are inconsistent ({first})")));
            }
        }
        let change = self.exec(Some(resolved), batch)?;
        Ok(FrameCopyOut { frame_ids, change })
    }

    /// Copy frames from one open document into another (assets included).
    pub fn frame_copy_to_doc(&mut self, req: FrameCopyToDoc) -> ApiResult<FrameCopyOut> {
        let payload = self.clip_copy(Some(req.from_doc_id), &req.frame_ids)?;
        let page = self
            .get(Some(req.to_doc_id))?
            .doc()
            .pages
            .get(req.page_index)
            .map(|p| p.id)
            .ok_or_else(|| {
                ApiError::not_found(
                    "page",
                    &req.page_index.to_string(),
                    "page_index counts from 0; list pages with page_list",
                )
            })?;
        let o = req.offset.unwrap_or(Offset { x: 0.0, y: 0.0 });
        self.clip_paste(
            Some(req.to_doc_id),
            &payload,
            PasteAt {
                parent: Parent::Page(page),
                dx: o.x,
                dy: o.y,
            },
        )
    }
}

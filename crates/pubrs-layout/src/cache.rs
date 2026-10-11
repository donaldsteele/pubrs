//! Incremental layout: per-story layouts reused while their inputs stay the
//! same (ADR 0012).
//!
//! [`StoryLayoutCache::layout`] builds every story's key (`StoryKey`: all the
//! inputs the flow of that story reads), compares it with the key its cached
//! layout was made from, and recomposes only the stories whose key changed.
//! Entries of deleted stories are dropped. The result is a
//! [`DocumentLayout`] whose stories are shared with the cache, so
//! assembling it costs a pointer per story.
//!
//! With `PUBRS_LAYOUT_VERIFY=1` in the environment (read once per process),
//! every pass is followed by a layout from scratch, and a story that differs
//! panics with its id. Tests run with it to prove the key is complete.

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use pubrs_core::{Document, Story, StoryId, StyleSheet};

use crate::flow::{DocIndex, Layouter, Region, regions_for};
use crate::types::{DocumentLayout, StoryLayout};

/// Everything the flow of one story reads. Two equal keys give equal
/// [`StoryLayout`]s; the key is a snapshot compared by equality (no hash,
/// so no collisions). Built by [`story_key`] only.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StoryKey {
    /// Stylesheet generation: bumped whenever the document's stylesheet
    /// differs from the one the cache saw last.
    styles: u64,
    /// Font library fingerprint: (families, faces) registered with the shaper.
    fonts: (usize, usize),
    /// For each frame of the chain, whether it is a text frame (each text
    /// frame gets a [`crate::FrameText`], placed or not).
    text_frames: Vec<bool>,
    /// The flow regions, exactly as the layout receives them.
    regions: Vec<Region>,
    /// The story: chain (frame ids in order), paragraphs, runs, text,
    /// style references and local overrides. Compared last: it is the
    /// largest field.
    story: SharedStory,
}

/// A story snapshot compared by pointer first: an unchanged story keeps its
/// `Arc` across commands, so the common case costs no text comparison.
/// Different pointers fall back to full equality (undo/redo or an equal
/// rewrite still hit).
#[derive(Debug, Clone)]
struct SharedStory(Arc<Story>);

impl PartialEq for SharedStory {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || *self.0 == *other.0
    }
}

/// The cache key of `story`: every input `Layouter::layout_story` reads.
///
/// - **The story** (`doc.story`): its paragraphs and runs (text, paragraph
///   and character style ids, local overrides) and its chain of frame ids.
/// - **Resolved styles** (`doc.styles.resolve_para` / `resolve_char`):
///   through `styles`, the stylesheet generation; any stylesheet edit
///   recomposes every story.
/// - **Fonts** (`FontLibrary::ensure_family`, parley's font context):
///   through `fonts`, the number of families and faces registered.
/// - **Chain frames** (`doc.frame(f).as_text()`): whether each is a text
///   frame.
/// - **Regions** (`regions_for`, the same function the layout calls): for
///   each text frame of the chain that is on a page or master, and each
///   column, the frame-local content box (from the frame's width and height,
///   insets, column count and gap), its vertical alignment, and the wrap
///   obstacles over it (`obstacles_for`: frames painted above it on the same
///   page or master with wrap on and another story, mapped through the text
///   frame's position and rotation and outset by the wrap margin). A
///   frame's page position and rotation enter only through obstacles, since
///   layout output is frame-local.
///
/// When the flow starts reading something new, add it here; the
/// verification mode and `tests/cache.rs` fail until it is.
pub(crate) fn story_key(
    doc: &Document,
    index: &mut DocIndex,
    story: &Arc<Story>,
    styles: u64,
    fonts: (usize, usize),
) -> StoryKey {
    StoryKey {
        styles,
        fonts,
        text_frames: story
            .frames
            .iter()
            .map(|f| doc.frame(*f).and_then(|f| f.as_text()).is_some())
            .collect(),
        regions: regions_for(doc, index, &story.frames),
        story: SharedStory(Arc::clone(story)),
    }
}

/// What the last [`StoryLayoutCache::layout`] pass did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// Stories laid out again in the last pass.
    pub recomposed: usize,
    /// Stories whose cached layout was reused in the last pass.
    pub reused: usize,
    /// Entries dropped in the last pass because their story was deleted.
    pub dropped: usize,
    /// Time the last pass spent building and comparing keys.
    pub key_time: Duration,
    /// Stories recomposed since the cache was created.
    pub total_recomposed: u64,
}

struct Entry {
    key: StoryKey,
    layout: Arc<StoryLayout>,
}

/// Per-story layouts of one document. Keep one per open document; the
/// [`Layouter`] (fonts and shaping caches) can be shared between them.
pub struct StoryLayoutCache {
    entries: BTreeMap<StoryId, Entry>,
    styles: Option<StyleSheet>,
    styles_generation: u64,
    stats: CacheStats,
    verify: bool,
}

impl std::fmt::Debug for StoryLayoutCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoryLayoutCache")
            .field("stories", &self.entries.len())
            .field("stats", &self.stats)
            .field("verify", &self.verify)
            .finish()
    }
}

impl Default for StoryLayoutCache {
    fn default() -> Self {
        Self::new()
    }
}

/// `PUBRS_LAYOUT_VERIFY` set to anything but empty or `0`, read once.
pub fn verify_enabled() -> bool {
    static VERIFY: OnceLock<bool> = OnceLock::new();
    *VERIFY.get_or_init(|| {
        std::env::var("PUBRS_LAYOUT_VERIFY").is_ok_and(|v| !v.is_empty() && v != "0")
    })
}

impl StoryLayoutCache {
    /// An empty cache; verification follows `PUBRS_LAYOUT_VERIFY`.
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            styles: None,
            styles_generation: 0,
            stats: CacheStats::default(),
            verify: verify_enabled(),
        }
    }

    /// What the last pass did.
    pub fn stats(&self) -> CacheStats {
        self.stats
    }

    /// Forget every cached story.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.styles = None;
    }

    /// The layout of every story of `doc`, recomposing only stories whose
    /// key changed since the previous call. Equal to
    /// [`Layouter::layout_document`].
    pub fn layout(&mut self, layouter: &mut Layouter, doc: &Document) -> DocumentLayout {
        let started = Instant::now();
        if self.styles.as_ref() != Some(&doc.styles) {
            self.styles = Some(doc.styles.clone());
            self.styles_generation += 1;
        }
        let styles = self.styles_generation;
        let fonts = font_fingerprint(layouter);
        let before = self.entries.len();
        self.entries.retain(|id, _| doc.stories.contains_key(id));
        let dropped = before - self.entries.len();
        let mut index = DocIndex::new(doc);
        let mut key_time = started.elapsed();
        let (mut recomposed, mut reused) = (0, 0);
        let mut out = DocumentLayout::default();
        for (id, story) in &doc.stories {
            let t = Instant::now();
            let key = story_key(doc, &mut index, story, styles, fonts);
            let hit = self
                .entries
                .get(id)
                .filter(|e| key == e.key)
                .map(|e| e.layout.clone());
            key_time += t.elapsed();
            let layout = match hit {
                Some(l) => {
                    reused += 1;
                    l
                }
                None => {
                    recomposed += 1;
                    let l = Arc::new(layouter.flow_story(doc, *id, &key.regions));
                    self.entries.insert(
                        *id,
                        Entry {
                            key,
                            layout: l.clone(),
                        },
                    );
                    l
                }
            };
            out.stories.insert(*id, layout);
        }
        self.stats = CacheStats {
            recomposed,
            reused,
            dropped,
            key_time,
            total_recomposed: self.stats.total_recomposed + recomposed as u64,
        };
        if self.verify {
            verify(layouter, doc, &out);
        }
        out
    }
}

/// (families, faces) registered with the shaper. Changes when fonts are
/// added or a system family is loaded on first use.
fn font_fingerprint(layouter: &mut Layouter) -> (usize, usize) {
    let collection = &mut layouter.fonts.fcx.collection;
    let ids: Vec<_> = collection.family_ids().collect();
    let faces = ids
        .iter()
        .filter_map(|id| collection.family(*id))
        .map(|f| f.fonts().len())
        .sum();
    (ids.len(), faces)
}

/// Lay `doc` out from scratch and panic, naming the story, if any story of
/// `incremental` differs.
fn verify(layouter: &mut Layouter, doc: &Document, incremental: &DocumentLayout) {
    let full = layouter.layout_document(doc);
    let ids = full.stories.keys().chain(incremental.stories.keys());
    for id in ids {
        if full.stories.get(id) != incremental.stories.get(id) {
            panic!(
                "PUBRS_LAYOUT_VERIFY: the incremental layout of story {id} differs from a full layout"
            );
        }
    }
}

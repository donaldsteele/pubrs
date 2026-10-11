//! `pubrs-app --self-test DIR`: drives the real GUI through the Definition
//! of Done scenario with injected mouse/keyboard input and the same code
//! paths the menus use, then exits 0 on success.
//!
//! Scenario: new trifold brochure from a template → type text into the
//! story (double-click, Cmd+A, typing) → link it into the continuation frame
//! so it flows → place and crop two photos → drag a frame on the canvas and
//! undo it → export a PDF with bleed and embedded fonts → save, close,
//! reopen → identical document JSON and byte-identical page renders →
//! zoom to 800% with the keyboard and check a shape edge on screen is sharp
//! (one texel per device pixel) → type into a 50-page publication and check
//! the frame times `PUBRS_PERF` reports: no page is rasterised on the UI
//! thread, and the UI thread's own work per frame stays small. Later
//! milestones add their own steps; the last ones (M28) type a misspelt word,
//! wait for its squiggle, fix it from the right-click menu and from the
//! Spelling window (F7).
//!
//! Before each step the self-test waits until the render worker has drawn
//! the current revision, so screenshots show the page.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Instant;

use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2};
use pubrs_api::core::{Command, Crop, FrameId, Point};
use pubrs_api::render::{RasterOptions, tiny_skia};
use pubrs_api::tools::doc::DocRef;
use pubrs_api::tools::find::{FindMatch, TextFind};
use pubrs_api::tools::text::{TextLinkFrames, TextRef, TextSet};

use crate::app::PubrsApp;
use crate::keymap::Action;
use crate::textedit::TextEdit;

/// Page x (points) in the gap between the brochure's first two columns: nothing but a guide is there.
const GX: f64 = 252.0;

const STORY: &str = "Golden Crust has been baking in the same stone oven on Mill Lane since 1987. \
We mill our own rye, feed a sourdough starter older than the shop, and shape every loaf by hand.\n\
Every morning we bake sourdough, rye, seeded and spelt loaves, buttery croissants and cinnamon \
buns. On Fridays and Saturdays we add gluten-free and vegan bakes, and celebration cakes are \
made to order: tell us the occasion three days ahead and we will design something special.\n\
We also supply cafés and restaurants across the valley, delivering before eight so their \
bread is as fresh as ours. Baking classes run on the first Sunday of every month: mix, shape \
and bake your own loaf and take home a jar of our starter.\n\
Our Saturday market stall opens at seven on the square, with seasonal specials: hot cross buns at \
Easter, stollen and mince pies in December, and summer fruit tarts while the berries last. Ask \
about our bread subscription, which brings a fresh loaf to your door twice a week.\n\
Thank you for keeping a small bakery busy for more than thirty years.";

/// Frames a step waits at most for the render worker (about 15 s at 60 Hz;
/// software-rendered CI frames are slower, so this is a frame count).
const SETTLE_FRAMES: u32 = 900;

/// Seconds one step may take before the self-test fails. Software-rendered
/// CI frames (Windows WARP, llvmpipe) are slow; the 50-page steps take longest.
const STEP_TIMEOUT_SECS: u64 = 120;

/// Seconds the whole self-test may take: only a guard against a hang, since
/// the run grows with every milestone's steps (it passed 300 s on the Windows
/// runner at M16, run 38129342526).
const TOTAL_TIMEOUT_SECS: u64 = 1800;

/// At 800% a pixel-aligned shape edge must put at least this share of its
/// contrast between two neighbouring screen pixels. One texel per device
/// pixel gives 100%; the old page texture capped at 4,096 px was magnified
/// about 1.6x at 800% and spread an edge over two pixels (about 60%).
const SHARP_EDGE: f32 = 0.9;

/// Keystrokes typed into the 50-page publication.
const KEYSTROKES: usize = 24;

/// Most UI-thread time one frame may spend on its own work while typing,
/// besides the command and the relayout: the canvas, panels, thumbnail
/// checks and texture uploads (painting is not UI time, so slow software
/// GPUs do not count). Before M13 each keystroke also rasterised the page
/// here. Measured about 2.5 ms in this debug build on the development
/// machine; the bound leaves room for slow CI runners. It applies to the
/// second-worst frame: one frame may stall on a shared runner's scheduler
/// (115.7 ms once on macos-15-intel, 5.4 ms on re-run), up to
/// [`MAX_UI_OUTLIER_MS`].
const MAX_UI_OVERHEAD_MS: f64 = 50.0;

/// Most UI-thread time, besides commands and layout, of the single worst
/// frame while typing.
const MAX_UI_OUTLIER_MS: f64 = 250.0;

/// The M15 story: a first paragraph a page break will end, four one-line
/// fillers, a heading (paragraph 5) and its text (paragraph 6).
const M15_TEXT: &str = "Welcome to the bakery.\na\nb\nc\nd\nOpening hours\nOur ovens open at six \
every morning, and the first loaves are out by seven.\nLast orders at four.";

#[derive(Default)]
struct Data {
    body: Option<FrameId>,
    cont: Option<FrameId>,
    title: Option<FrameId>,
    title_rect: Option<pubrs_api::core::Rect>,
    images: Vec<FrameId>,
    json: Option<serde_json::Value>,
    pngs: Vec<Vec<u8>>,
    /// The last screenshot taken.
    shot: Option<std::sync::Arc<egui::ColorImage>>,
    /// The shape whose edge is checked at 800%.
    edge_frame: Option<FrameId>,
    /// The 50-page publication and the text frame typed into.
    big_doc: Option<pubrs_api::core::DocumentId>,
    typed_frame: Option<FrameId>,
    typed_from: u64,
    guides_before: usize,
    hist: usize,
    matches: usize,
    first_match: Option<(usize, usize)>,
    swatch: Option<pubrs_api::core::SwatchId>,
    swatch_name: Option<String>,
    probe: Option<FrameId>,
    before: Option<[u8; 4]>,
    /// M15: frame A (page 1) and frame B (page 2, two columns) of one story.
    m15: Option<(FrameId, FrameId)>,
    /// M28: the frame typed into, the flagged word, the first suggestion.
    spell_frame: Option<FrameId>,
    spell_range: Option<(usize, usize)>,
    suggestion: Option<String>,
    /// M25: the WordArt frame and the shape given a shadow and a border.
    m25: Option<(FrameId, FrameId)>,
    /// Undo steps done before the step being checked (M25).
    m25_hist: usize,
    /// M20: the table, the undo steps before a check and a column's width.
    m20: Option<FrameId>,
    m20_hist: usize,
    m20_w: f64,
    /// The table frame's height before the wrapping edit.
    m20_h: f64,
    /// The step being retried and how often.
    retry: (usize, u32),
    /// M16: the list frame and the price frame.
    m16: Option<(FrameId, FrameId)>,
    /// M17: the character formatting frame.
    m17: Option<FrameId>,
    /// M18: the text frame and the ellipse it wraps around.
    m18: Option<(FrameId, FrameId)>,
    /// M27: the publication copied from and the one pasted into.
    m27_docs: Option<(pubrs_api::core::DocumentId, pubrs_api::core::DocumentId)>,
    /// M27: the frame the input method types into and the Hebrew frame.
    m27_frames: Option<(FrameId, FrameId)>,
}

pub struct SelfTest {
    out: PathBuf,
    step: usize,
    wait: u32,
    pending: VecDeque<Vec<Event>>,
    log: Vec<String>,
    data: Data,
    shots: usize,
    started: Instant,
    /// When the current step began; each step gets its own time limit.
    step_started: Instant,
    /// File to drop on the window with the next frame.
    drop: Option<PathBuf>,
    /// Input clock, one 60 Hz tick per frame, so double-clicks and drags do
    /// not depend on how fast this machine renders (software GPUs on CI).
    clock: f64,
    /// Frames spent waiting for the render worker before the current step.
    settling: u32,
    /// The scenario ended; only the input clock still runs.
    pub finished: bool,
    /// A screenshot was asked for: taken once the canvas has caught up.
    want_shot: bool,
    /// Screenshots saved when the last one was requested, and frames waited.
    shot_wait: Option<(usize, u32)>,
}

fn png(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let mut pm = tiny_skia::Pixmap::new(w, h).expect("pixmap");
    for (i, px) in pm.pixels_mut().iter_mut().enumerate() {
        let x = (i as u32 % w) * 255 / w;
        let y = (i as u32 / w) * 255 / h;
        *px = tiny_skia::ColorU8::from_rgba(x as u8, y as u8, seed, 255).premultiply();
    }
    pm.encode_png().expect("png")
}

impl SelfTest {
    pub fn new(out: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&out);
        Self {
            out,
            step: 0,
            wait: 5,
            pending: VecDeque::new(),
            log: Vec::new(),
            data: Data::default(),
            shots: 0,
            started: Instant::now(),
            step_started: Instant::now(),
            drop: None,
            clock: 0.0,
            settling: 0,
            finished: false,
            want_shot: false,
            shot_wait: None,
        }
    }

    /// Feed queued synthetic events into egui.
    pub fn inject(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.clock += 1.0 / 60.0;
        raw.time = Some(self.clock);
        raw.predicted_dt = 1.0 / 60.0;
        if let Some(events) = self.pending.pop_front() {
            raw.events.extend(events);
        }
        if let Some(path) = self.drop.take() {
            raw.dropped_files
                .push(std::sync::Arc::new(DroppedPath(path)));
        }
    }

    fn ok(&mut self, msg: impl Into<String>) {
        let m = msg.into();
        tracing::info!("self-test: {m}");
        self.log.push(format!("ok  {m}"));
    }

    /// Take a screenshot once the render worker has drawn the current state
    /// (before the next step runs).
    fn screenshot(&mut self, _ctx: &egui::Context) {
        self.want_shot = true;
    }

    fn save_screenshots(&mut self, ctx: &egui::Context) {
        let shots: Vec<std::sync::Arc<egui::ColorImage>> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
                .collect()
        });
        for img in shots {
            self.data.shot = Some(img.clone());
            self.shots += 1;
            let [w, h] = img.size;
            if let Some(mut pm) = tiny_skia::Pixmap::new(w as u32, h as u32) {
                for (dst, c) in pm.pixels_mut().iter_mut().zip(img.pixels.iter()) {
                    let [r, g, b, a] = c.to_srgba_unmultiplied();
                    *dst = tiny_skia::ColorU8::from_rgba(r, g, b, a).premultiply();
                }
                let path = self.out.join(format!("screen-{}.png", self.shots));
                if let Ok(bytes) = pm.encode_png() {
                    let _ = std::fs::write(&path, bytes);
                }
            }
        }
    }

    /// The canvas shows the current revision at the current view: the render
    /// worker has caught up with the document, the zoom and the page, and the
    /// thumbnails the pages panel shows first are current.
    fn rendered(app: &PubrsApp) -> bool {
        let Some(v) = app.view() else {
            return true;
        };
        let Ok(open) = app.session.get(Some(v.doc_id)) else {
            return true;
        };
        let rev = open.revision();
        let pages = open.doc().pages.len();
        let canvas = v.page_tex.as_ref().is_some_and(|t| {
            t.revision == rev
                && t.key.page_index == v.page
                && v.requested.is_none_or(|(k, r)| k == t.key && r == rev)
        });
        canvas && (0..pages.min(8)).all(|i| v.thumbs.get(&i).is_some_and(|t| t.revision == rev))
    }

    /// The rendered RGBA at page point (`x`, `y`) of the page being shown.
    fn probe_pixel(app: &mut PubrsApp, x: u32, y: u32) -> Result<[u8; 4], String> {
        let id = app.doc_id().ok_or("no doc")?;
        let page = app.view().map_or(0, |v| v.page);
        let (w, _, px) = app
            .session
            .page_rgba(
                Some(id),
                page,
                &RasterOptions {
                    dpi: 72.0,
                    ..Default::default()
                },
            )
            .map_err(|e| e.message)?;
        let i = ((y * w + x) * 4) as usize;
        px.get(i..i + 4)
            .map(|p| [p[0], p[1], p[2], p[3]])
            .ok_or_else(|| "probe pixel is off the page".to_string())
    }

    fn page_size(app: &PubrsApp) -> Result<(f64, f64), String> {
        let s = app.doc().ok_or("no doc")?.setup.size;
        Ok((s.width, s.height))
    }

    /// The guides on the page being shown.
    fn page_guides(app: &PubrsApp) -> Vec<pubrs_api::core::Guide> {
        let page = app.view().map_or(0, |v| v.page);
        app.doc()
            .and_then(|d| d.pages.get(page))
            .map(|p| p.guides.clone())
            .unwrap_or_default()
    }

    /// Undo steps that are applied (not undone).
    fn done_steps(app: &mut PubrsApp) -> usize {
        app.session
            .history_list(DocRef::default())
            .map_or(0, |h| h.entries.iter().filter(|e| !e.undone).count())
    }

    fn history_len(app: &mut PubrsApp) -> usize {
        app.session
            .history_list(DocRef::default())
            .map_or(0, |h| h.entries.len())
    }

    /// The middle of a ruler strip, level with page position `at` (points).
    fn ruler_point(app: &PubrsApp, top: bool, at: f64) -> Result<Pos2, String> {
        let v = app.view().ok_or("no view")?;
        let canvas = v.canvas_rect.ok_or("no canvas")?;
        let half = crate::rulers::RULER / 2.0;
        let on_page = Self::screen_of(app, Point::new(at, at)).ok_or("no canvas")?;
        Ok(if top {
            Pos2::new(on_page.x, canvas.top() - half)
        } else {
            Pos2::new(canvas.left() - half, on_page.y)
        })
    }

    /// Matches of `query` that sit in a frame on a page.
    fn navigable_matches(app: &mut PubrsApp, query: &str) -> Result<Vec<FindMatch>, String> {
        let out = app
            .session
            .text_find(TextFind {
                doc_id: None,
                frame_id: None,
                story_id: None,
                query: query.to_string(),
                case_sensitive: false,
                whole_word: false,
                start: None,
                end: None,
            })
            .map_err(|e| e.message)?;
        Ok(out
            .matches
            .into_iter()
            .filter(|m| m.frame_id.is_some())
            .collect())
    }

    /// The lines frame `f` shows.
    fn m15_lines(
        app: &mut PubrsApp,
        f: FrameId,
    ) -> Result<Vec<pubrs_api::layout::LineBox>, String> {
        let lay = app.session.layout(app.doc_id()).map_err(|e| e.message)?;
        Ok(lay.frame(f).map(|ft| ft.lines.clone()).unwrap_or_default())
    }

    /// The lines of paragraph `para` of frame `f`'s story that `f` shows.
    fn m15_para_lines(
        app: &mut PubrsApp,
        f: FrameId,
        para: usize,
    ) -> Result<Vec<pubrs_api::layout::LineBox>, String> {
        let doc = app.doc().ok_or("no doc")?;
        let s = doc
            .story_of(f)
            .and_then(|s| doc.story(s))
            .ok_or("no story")?;
        let start = s.paragraph_start(para);
        let end = start + s.paragraphs.get(para).ok_or("no paragraph")?.char_len();
        Ok(Self::m15_lines(app, f)?
            .into_iter()
            .filter(|l| l.chars.start >= start && l.chars.end <= end)
            .collect())
    }

    /// Font size and height above the line's baseline of the glyph that
    /// shows story offset `at` in frame `f`.
    fn m17_glyph(app: &mut PubrsApp, f: FrameId, at: usize) -> Result<(f64, f64), String> {
        let lines = Self::m15_lines(app, f)?;
        for line in &lines {
            for run in &line.runs {
                if let Some(g) = run
                    .glyphs
                    .iter()
                    .find(|g| g.text.start == at && !g.text.is_empty())
                {
                    return Ok((run.font_size, line.baseline - g.y));
                }
            }
        }
        Err(format!("no glyph shows offset {at}"))
    }

    /// The list labels of every line frame `f` shows, one per labelled line.
    fn m16_labels(app: &mut PubrsApp, f: FrameId) -> Result<Vec<String>, String> {
        Ok(Self::m15_lines(app, f)?
            .into_iter()
            .filter_map(|l| l.list_label)
            .collect())
    }

    /// Screen point on the text of a line of frame `f`, clear of the
    /// frame's corner handles.
    fn m15_line_pos(
        app: &PubrsApp,
        f: FrameId,
        l: &pubrs_api::layout::LineBox,
    ) -> Result<Pos2, String> {
        let r = app
            .doc()
            .and_then(|d| d.frame(f))
            .map(|f| f.rect)
            .ok_or("no frame")?;
        let p = Point::new(r.x + l.rect.x + 20.0, r.y + l.rect.y + l.rect.height / 2.0);
        Self::screen_of(app, p).ok_or_else(|| "canvas not laid out".to_string())
    }

    fn screen_of(app: &PubrsApp, p: Point) -> Option<Pos2> {
        app.view().and_then(|v| v.xf).map(|xf| xf.to_screen(p))
    }

    fn click(&mut self, pos: Pos2, double: bool) {
        let press = |pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        self.pending.push_back(vec![Event::PointerMoved(pos)]);
        self.pending.push_back(vec![press(true)]);
        self.pending.push_back(vec![press(false)]);
        if double {
            self.pending.push_back(vec![press(true)]);
            self.pending.push_back(vec![press(false)]);
        }
    }

    /// A double-click after 0.7 s of input-clock idle: egui counts a third
    /// click within 0.6 s of an earlier one as a triple click, and the
    /// dialog clicks of the step before may be that recent.
    fn fresh_double_click(&mut self, pos: Pos2) {
        for _ in 0..42 {
            self.pending.push_back(Vec::new());
        }
        self.click(pos, true);
    }

    /// Click the centre of a Page Setup control the dialog drew this frame.
    fn click_target(&mut self, ctx: &egui::Context, name: &str) -> Result<(), String> {
        let r = crate::dialogs::target_rect(ctx, name)
            .ok_or_else(|| format!("Page Setup control '{name}' is not on screen"))?;
        self.click(r.center(), false);
        Ok(())
    }

    /// Screen centre of a table cell (an unrotated table).
    fn cell_center(app: &mut PubrsApp, frame: FrameId, row: u32, col: u32) -> Result<Pos2, String> {
        let id = app.doc_id().ok_or("no doc")?;
        let layout = app.session.layout(Some(id)).map_err(|e| e.message)?;
        let doc = app.doc().ok_or("no doc")?;
        let f = doc.frame(frame).ok_or("no table")?;
        let t = f.as_table().ok_or("not a table")?;
        let i = t
            .cells
            .iter()
            .position(|c| c.row == row && c.col == col)
            .ok_or("no such cell")?;
        let r = layout.table(frame).ok_or("no table layout")?.cells[i].rect;
        let c = Point::new(
            f.rect.x + r.x + r.width / 2.0,
            f.rect.y + r.y + r.height / 2.0,
        );
        Self::screen_of(app, c).ok_or_else(|| "canvas not laid out".to_string())
    }

    fn drag(&mut self, from: Pos2, to: Pos2) {
        self.pending.push_back(vec![Event::PointerMoved(from)]);
        self.pending.push_back(vec![Event::PointerButton {
            pos: from,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }]);
        for k in 1..=8 {
            let t = k as f32 / 8.0;
            self.pending
                .push_back(vec![Event::PointerMoved(from + (to - from) * t)]);
        }
        self.pending.push_back(vec![Event::PointerButton {
            pos: to,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }]);
        self.pending.push_back(vec![Event::PointerGone]);
    }

    fn right_click(&mut self, pos: Pos2) {
        let press = |pressed| Event::PointerButton {
            pos,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        self.pending.push_back(vec![Event::PointerMoved(pos)]);
        self.pending.push_back(vec![press(true)]);
        self.pending.push_back(vec![press(false)]);
    }

    /// Run the current step again on a later frame (waiting for a worker).
    fn again(&mut self, what: &str) -> Result<bool, String> {
        let n = if self.data.retry.0 == self.step {
            self.data.retry.1 + 1
        } else {
            1
        };
        self.data.retry = (self.step, n);
        if n > 300 {
            return Err(format!("timed out waiting for {what}"));
        }
        self.step -= 1;
        Ok(false)
    }

    /// The range of `word` in the newest spell check of the page shown, if
    /// that check is of the current revision and story text.
    fn spell_issue(app: &PubrsApp, word: &str) -> Option<(usize, usize)> {
        let doc_id = app.doc_id()?;
        let rev = app.session.get(Some(doc_id)).ok()?.revision();
        let p = app.spell.page.as_ref()?;
        if p.doc_id != doc_id || p.revision != rev || p.page_index != app.view()?.page {
            return None;
        }
        let doc = app.doc()?;
        p.stories.iter().find_map(|(id, s)| {
            let cur = doc.stories.get(id)?;
            if !std::sync::Arc::ptr_eq(cur, &s.story) {
                return None;
            }
            s.issues
                .iter()
                .find(|i| i.word == word)
                .map(|i| (i.start, i.end))
        })
    }

    /// Screen centre of a character range of a frame's story.
    fn range_center(
        app: &mut PubrsApp,
        frame: FrameId,
        start: usize,
        end: usize,
    ) -> Result<Pos2, String> {
        let id = app.doc_id().ok_or("no doc")?;
        let layout = app.session.layout(Some(id)).map_err(|e| e.message)?;
        let doc = app.doc().ok_or("no doc")?;
        let story = doc.story_of(frame).ok_or("no story")?;
        let q = crate::textedit::selection_quads(doc, &layout, story, start, end)
            .into_iter()
            .next()
            .ok_or("the word is not laid out")?;
        let c = Point::new((q[0].x + q[2].x) / 2.0, (q[0].y + q[2].y) / 2.0);
        Self::screen_of(app, c).ok_or_else(|| "canvas not laid out".to_string())
    }

    fn spell_text(app: &mut PubrsApp, frame: Option<FrameId>) -> Result<String, String> {
        let f = frame.ok_or("no frame")?;
        crate::canvas::story_text(app, f).ok_or_else(|| "no text".to_string())
    }

    fn key(&mut self, key: Key, modifiers: Modifiers) {
        self.pending.push_back(vec![Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers,
        }]);
        self.pending.push_back(vec![Event::Key {
            key,
            physical_key: Some(key),
            pressed: false,
            repeat: false,
            modifiers,
        }]);
    }

    /// Advance the scenario; `Some(result)` when finished.
    pub fn step(
        &mut self,
        app: &mut PubrsApp,
        ctx: &egui::Context,
    ) -> Option<Result<String, String>> {
        self.save_screenshots(ctx);
        // Generous: CI draws the window in software, several frames a second.
        // Each step has its own limit, so the whole run may grow with the
        // number of steps; the overall limit only guards against a hang.
        if self.step_started.elapsed().as_secs() > STEP_TIMEOUT_SECS
            || self.started.elapsed().as_secs() > TOTAL_TIMEOUT_SECS
        {
            return Some(Err(format!(
                "timed out at step {} ({} s on this step, {} s in total)\n{}",
                self.step,
                self.step_started.elapsed().as_secs(),
                self.started.elapsed().as_secs(),
                self.log.join("\n")
            )));
        }
        if !self.pending.is_empty() || self.wait > 0 {
            self.wait = self.wait.saturating_sub(1);
            return None;
        }
        // Let the render worker catch up (bounded: a step that needs the
        // pixels checks them itself).
        if let Some((n, waited)) = self.shot_wait {
            if self.shots <= n && waited < 120 {
                self.shot_wait = Some((n, waited + 1));
                return None;
            }
            self.shot_wait = None;
        }
        if !Self::rendered(app) && self.settling < SETTLE_FRAMES {
            self.settling += 1;
            return None;
        }
        self.settling = 0;
        if self.want_shot {
            self.want_shot = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            self.shot_wait = Some((self.shots, 0));
            return None;
        }
        let r = self.run_step(app, ctx);
        match r {
            Ok(true) => Some(Ok(self.log.join("\n"))),
            Ok(false) => {
                self.step += 1;
                self.step_started = Instant::now();
                self.wait = 4;
                None
            }
            Err(e) => Some(Err(format!(
                "step {}: {e}\n{}",
                self.step,
                self.log.join("\n")
            ))),
        }
    }

    fn run_step(&mut self, app: &mut PubrsApp, ctx: &egui::Context) -> Result<bool, String> {
        let find = |app: &PubrsApp, name: &str| -> Option<FrameId> {
            let doc = app.doc()?;
            doc.placeholder(name).or_else(|| {
                doc.frames
                    .values()
                    .find(|f| f.name.as_deref() == Some(name))
                    .map(|f| f.id)
            })
        };
        match self.step {
            0 => {
                app.dialog = Some(crate::dialogs::Dialog::TemplatePicker);
                self.wait = 10;
            }
            1 => {
                self.screenshot(ctx);
                self.ok("template picker shown");
            }
            2 => {
                app.dialog = None;
                app.new_from_template("trifold_brochure")
                    .ok_or("template_apply failed")?;
                let n = app.doc().map_or(0, |d| d.pages.len());
                if n != 2 {
                    return Err(format!("expected 2 pages, got {n}"));
                }
                self.ok("created a two-page trifold brochure from the template");
                self.screenshot(ctx);
            }
            3 => {
                self.data.body = find(app, "inside_body");
                self.data.cont = find(app, "inside_body_continued");
                self.data.title = find(app, "cover_title");
                self.data.images = ["cover_image", "inside_image"]
                    .iter()
                    .filter_map(|n| find(app, n))
                    .collect();
                if self.data.body.is_none()
                    || self.data.cont.is_none()
                    || self.data.images.len() != 2
                {
                    return Err("template placeholders not found".into());
                }
                if let Some(v) = app.view_mut() {
                    v.page = 1;
                }
            }
            4 => {
                // Double-click the story frame on the canvas to start editing.
                let body = self.data.body.expect("found");
                let center = app
                    .doc()
                    .and_then(|d| d.frame(body))
                    .map(|f| f.rect.center())
                    .ok_or("no body frame")?;
                let pos = Self::screen_of(app, center).ok_or("canvas not laid out")?;
                self.click(pos, true);
            }
            5 => {
                let editing = app.view().and_then(|v| v.edit.as_ref()).map(|e| e.frame);
                if editing != self.data.body {
                    return Err(format!(
                        "double-click did not start text editing (edit = {editing:?})"
                    ));
                }
                self.ok("double-click on the canvas started in-place text editing");
                // Select all and type the story.
                self.key(Key::A, Modifiers::COMMAND);
                self.pending.push_back(vec![Event::Text(STORY.to_string())]);
            }
            6 => {
                let body = self.data.body.expect("found");
                let text = crate::canvas::story_text(app, body).unwrap_or_default();
                if text != STORY {
                    return Err(format!(
                        "typed text differs: {:?}",
                        text.chars().take(80).collect::<String>()
                    ));
                }
                let undo_steps = app
                    .session
                    .history_list(DocRef::default())
                    .map_err(|e| e.message)?
                    .entries
                    .len();
                self.ok(format!(
                    "history has {undo_steps} step(s) after typing (typing merges into one)"
                ));
                self.ok("typed the story into the frame through keyboard events");
                let ov = app
                    .session
                    .text_overflow_status(DocRef::default())
                    .map_err(|e| e.message)?;
                if !ov.any_overflow {
                    return Err("expected the long story to overflow one panel".into());
                }
                self.key(Key::Escape, Modifiers::NONE);
            }
            7 => {
                // Link the story into the continuation frame (Text ▸ Link).
                let (body, cont) = (
                    self.data.body.expect("found"),
                    self.data.cont.expect("found"),
                );
                app.select(vec![body, cont]);
                app.run_action(ctx, Action::LinkTextFrames);
                let ov = app
                    .session
                    .text_overflow_status(DocRef::default())
                    .map_err(|e| e.message)?;
                if ov.any_overflow {
                    return Err("text still overflows after linking".into());
                }
                let t = app
                    .session
                    .text_get(TextRef {
                        doc_id: None,
                        frame_id: Some(body),
                        story_id: None,
                    })
                    .map_err(|e| e.message)?;
                let shown_in_cont = t
                    .frames
                    .iter()
                    .find(|f| f.frame_id == cont)
                    .map_or(0, |f| f.end - f.start);
                if shown_in_cont == 0 {
                    return Err("no text flowed into the linked frame".into());
                }
                self.ok(format!("linked frames: {shown_in_cont} characters flow into the second frame, no overflow"));
                self.screenshot(ctx);
            }
            8 => {
                // Place and crop two photos.
                for (i, frame) in self.data.images.clone().into_iter().enumerate() {
                    let path = self.out.join(format!("photo{i}.png"));
                    std::fs::write(&path, png(1800, 1350, 60 + 90 * i as u8))
                        .map_err(|e| e.to_string())?;
                    if !app.place_image(frame, &path) {
                        return Err(format!("placing photo {i} failed"));
                    }
                    let crop = Crop {
                        left: 0.1,
                        top: 0.05,
                        right: 0.1,
                        bottom: 0.05,
                    };
                    app.exec(Command::SetImageCrop { frame, crop });
                }
                let doc = app.doc().ok_or("no doc")?;
                for f in &self.data.images {
                    let img = doc
                        .frame(*f)
                        .and_then(|f| f.as_image())
                        .ok_or("not an image")?;
                    if img.asset.is_none() || img.crop.left != 0.1 {
                        return Err("photo not placed/cropped".into());
                    }
                }
                self.ok("placed and cropped two photos");
                if let Some(v) = app.view_mut() {
                    v.page = 0;
                    v.selection.clear();
                }
            }
            9 => {
                // Drag the cover title on the canvas, then undo with Cmd+Z.
                let title = self.data.title.expect("found");
                let r = app
                    .doc()
                    .and_then(|d| d.frame(title))
                    .map(|f| f.rect)
                    .ok_or("no title")?;
                self.data.title_rect = Some(r);
                let from = Self::screen_of(app, r.center()).ok_or("no canvas")?;
                let to = Self::screen_of(app, Point::new(r.center().x - 30.0, r.center().y + 20.0))
                    .ok_or("no canvas")?;
                self.drag(from, to);
            }
            10 => {
                let title = self.data.title.expect("found");
                let before = self.data.title_rect.expect("recorded");
                let now = app
                    .doc()
                    .and_then(|d| d.frame(title))
                    .map(|f| f.rect)
                    .ok_or("no title")?;
                if (now.x - before.x).abs() < 5.0 {
                    return Err(format!(
                        "dragging on the canvas did not move the frame ({before:?} → {now:?})"
                    ));
                }
                self.ok(format!(
                    "dragged a frame on the canvas by ({:.0}, {:.0}) pt",
                    now.x - before.x,
                    now.y - before.y
                ));
                self.key(Key::Z, Modifiers::COMMAND);
            }
            11 => {
                let title = self.data.title.expect("found");
                let before = self.data.title_rect.expect("recorded");
                let now = app
                    .doc()
                    .and_then(|d| d.frame(title))
                    .map(|f| f.rect)
                    .ok_or("no title")?;
                if now != before {
                    return Err("Cmd+Z did not undo the move".into());
                }
                self.ok("Cmd+Z undid the move");
                let v = app
                    .session
                    .validate(DocRef::default())
                    .map_err(|e| e.message)?;
                self.ok(format!(
                    "validate: {} warning(s) (unfilled template text is expected)",
                    v.warnings.len()
                ));
            }
            12 => {
                // Export a PDF with bleed and embedded fonts.
                let pdf = self.out.join("brochure.pdf");
                if !app.export_pdf_to(&pdf) {
                    return Err("PDF export failed".into());
                }
                let bytes = std::fs::read(&pdf).map_err(|e| e.to_string())?;
                let has = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
                if !has(b"/FontFile2") {
                    return Err("PDF has no embedded TrueType fonts".into());
                }
                if !has(b"/BleedBox") || !has(b"/TrimBox") {
                    return Err("PDF lacks BleedBox/TrimBox".into());
                }
                self.ok(format!(
                    "exported {} ({} bytes) with bleed boxes and embedded fonts",
                    pdf.display(),
                    bytes.len()
                ));
            }
            13 => {
                // Save, remember the layout, close.
                let path = self.out.join("brochure.pubrs");
                if !app.save_to(&path) {
                    return Err("save failed".into());
                }
                let id = app.doc_id().ok_or("no doc")?;
                self.data.json = Some(
                    serde_json::to_value(
                        &app.session
                            .doc_to_json(DocRef { doc_id: Some(id) })
                            .map_err(|e| e.message)?
                            .document,
                    )
                    .map_err(|e| e.to_string())?,
                );
                let pages = app.doc().map_or(0, |d| d.pages.len());
                for i in 0..pages {
                    let (_, _, png) = app
                        .session
                        .page_png(
                            Some(id),
                            i,
                            &RasterOptions {
                                dpi: 72.0,
                                ..Default::default()
                            },
                            false,
                        )
                        .map_err(|e| e.message)?;
                    self.data.pngs.push(png);
                }
                self.screenshot(ctx);
                app.close_active(false);
                if !app.views.is_empty() {
                    return Err("document did not close (unsaved changes?)".into());
                }
                self.ok("saved and closed the publication");
                // Reopen it the way a user would drag it from the file manager.
                self.drop = Some(path);
            }
            14 => {
                let id = app
                    .doc_id()
                    .ok_or("dropping the saved file on the window did not open it")?;
                let json = serde_json::to_value(
                    &app.session
                        .doc_to_json(DocRef { doc_id: Some(id) })
                        .map_err(|e| e.message)?
                        .document,
                )
                .map_err(|e| e.to_string())?;
                if Some(&json) != self.data.json.as_ref() {
                    return Err("reopened document differs from the saved one".into());
                }
                for (i, before) in self.data.pngs.iter().enumerate() {
                    let (_, _, png) = app
                        .session
                        .page_png(
                            Some(id),
                            i,
                            &RasterOptions {
                                dpi: 72.0,
                                ..Default::default()
                            },
                            false,
                        )
                        .map_err(|e| e.message)?;
                    if &png != before {
                        return Err(format!(
                            "page {} renders differently after reopening",
                            i + 1
                        ));
                    }
                }
                self.ok("reopened by dropping the file on the window: identical document JSON and byte-identical page renders");
                self.screenshot(ctx);
            }
            15 => {
                // Autosave writes a recovery copy for unsaved changes.
                let id = app.doc_id().ok_or("no doc")?;
                app.exec(Command::SetBleed { bleed: 12.0 });
                if !app.autosave.write_if_changed(&app.session, id) {
                    return Err("autosave did not write a recovery copy".into());
                }
                if app.autosave.recoverable().is_empty() {
                    return Err("recovery copy not found".into());
                }
                app.autosave.forget(id);
                self.ok("autosave wrote a recovery copy for unsaved changes");
                self.ok(format!("canvas rendering: {}", app.renderer.describe()));
            }
            16 => {
                // A dark rectangle on white near the page corner, where the
                // view lands when zooming in; then zoom with Cmd+Alt+= (Zoom In).
                if let Some(v) = app.view_mut() {
                    v.page = 0;
                    v.selection.clear();
                }
                for (fill, x, y, w, h) in [
                    ("#FFFFFF", 16.0, 16.0, 60.0, 44.0),
                    ("#1E3A8A", 24.0, 24.0, 30.0, 20.0),
                ] {
                    let req: pubrs_api::tools::frames::FrameAddShape =
                        serde_json::from_value(serde_json::json!({
                            "shape": "rectangle", "x": x, "y": y, "width": w, "height": h,
                            "fill": fill, "stroke": "none",
                        }))
                        .map_err(|e| e.to_string())?;
                    let out = app.session.frame_add_shape(req).map_err(|e| e.message)?;
                    self.data.edge_frame = Some(out.frame_id);
                }
                let zoom = app.view().map_or(1.0, |v| v.effective_zoom).max(0.05);
                let presses = ((8.0 / zoom).ln() / 1.25f32.ln()).ceil().max(0.0) as usize + 1;
                for _ in 0..presses {
                    self.key(Key::Plus, Modifiers::COMMAND | Modifiers::ALT);
                }
            }
            17 => {
                let zoom = app.view().map_or(0.0, |v| v.effective_zoom);
                if (zoom - 8.0).abs() > 1e-3 {
                    return Err(format!(
                        "Cmd+Alt+Plus reached {:.0}% zoom, not 800%",
                        zoom * 100.0
                    ));
                }
                let ppp = ctx.pixels_per_point();
                let t = app
                    .view()
                    .and_then(|v| v.page_tex.as_ref())
                    .ok_or("no page texture at 800%")?;
                let want = 72.0 * 8.0 * f64::from(ppp);
                if (t.key.dpi - want).abs() > 1e-6 {
                    return Err(format!(
                        "the texture is at {:.0} dpi, not one texel per device pixel ({want:.0} dpi)",
                        t.key.dpi
                    ));
                }
                let setup = app.doc().ok_or("no doc")?.setup.clone();
                let page_w = ((setup.size.width + 2.0 * setup.bleed) * want / 72.0).ceil();
                self.ok(format!(
                    "zoomed to 800% with Cmd+Alt+Plus: the canvas texture covers {}x{} device pixels of the {page_w:.0} px wide page, one texel per pixel (no 4,096 px cap)",
                    t.key.region.width, t.key.region.height
                ));
                self.screenshot(ctx);
            }
            18 => {
                let shot = self.data.shot.clone().ok_or("no screenshot at 800%")?;
                let edge = self.data.edge_frame.ok_or("no probe")?;
                let r = app
                    .doc()
                    .and_then(|d| d.frame(edge))
                    .map(|f| f.rect)
                    .ok_or("probe missing")?;
                let p = Self::screen_of(app, Point::new(r.x, r.y + r.height / 2.0))
                    .ok_or("no canvas")?;
                let ppp = ctx.pixels_per_point();
                let (ex, ey) = ((p.x * ppp).round() as i64, (p.y * ppp).round() as i64);
                let [w, h] = shot.size;
                let lum = |x: i64| -> Option<f32> {
                    if x < 0 || ey < 0 || x >= w as i64 || ey >= h as i64 {
                        return None;
                    }
                    let c = shot.pixels[ey as usize * w + x as usize];
                    Some(
                        0.2126 * f32::from(c.r())
                            + 0.7152 * f32::from(c.g())
                            + 0.0722 * f32::from(c.b()),
                    )
                };
                let row: Vec<f32> = (ex - 6..=ex + 6)
                    .map(lum)
                    .collect::<Option<_>>()
                    .ok_or("the probe edge is off screen")?;
                let outside = row[..3].iter().sum::<f32>() / 3.0;
                let inside = row[row.len() - 3..].iter().sum::<f32>() / 3.0;
                let contrast = (outside - inside).abs();
                if contrast < 60.0 {
                    return Err(format!(
                        "no edge where the probe should be ({contrast:.0} levels of contrast)"
                    ));
                }
                let step = row
                    .windows(2)
                    .map(|p| (p[1] - p[0]).abs())
                    .fold(0.0f32, f32::max);
                let sharp = step / contrast;
                if sharp < SHARP_EDGE {
                    return Err(format!(
                        "at 800% the edge is blurred: the largest step between neighbouring pixels is {:.0}% of its contrast (need {:.0}%)",
                        sharp * 100.0,
                        SHARP_EDGE * 100.0
                    ));
                }
                self.ok(format!(
                    "at 800% a shape edge is sharp on screen: {:.0}% of its contrast lies between two neighbouring pixels",
                    sharp * 100.0
                ));
                // Remove the probe (two undo steps) and fit the page again.
                self.key(Key::Z, Modifiers::COMMAND);
                self.key(Key::Z, Modifiers::COMMAND);
                self.key(Key::Num0, Modifiers::COMMAND);
            }
            19 => {
                let (doc, _) = pubrs_api::perf::build(&pubrs_api::perf::PerfSpec::scaled(50, 1))
                    .map_err(|e| e.message)?;
                let id = app.session.insert(doc, None);
                app.add_view(id);
                self.data.big_doc = Some(id);
                let d = app.doc().ok_or("no doc")?;
                self.data.typed_frame = d.pages[0].frames.iter().copied().find(|f| {
                    d.frame(*f)
                        .is_some_and(|f| f.kind() == pubrs_api::core::FrameKind::Text)
                });
                let n = d.pages.len();
                self.ok(format!(
                    "opened a {n}-page publication ({} frames, {} stories)",
                    d.frames.len(),
                    d.stories.len()
                ));
            }
            20 => {
                let frame = self.data.typed_frame.ok_or("no text frame on page 1")?;
                let r = app
                    .doc()
                    .and_then(|d| d.frame(frame))
                    .map(|f| f.rect)
                    .ok_or("no text frame")?;
                let pos = Self::screen_of(app, r.center()).ok_or("canvas not laid out")?;
                self.click(pos, true);
            }
            21 => {
                let editing = app.view().and_then(|v| v.edit.as_ref()).map(|e| e.frame);
                if editing != self.data.typed_frame {
                    return Err(format!(
                        "double-click did not start editing the 50-page publication ({editing:?})"
                    ));
                }
                let stats = app.perf.get_or_insert_with(crate::perf::FrameStats::fresh);
                stats.reset_totals();
                self.data.typed_from = app.session.get(None).map_or(0, |o| o.revision());
                for _ in 0..KEYSTROKES {
                    self.pending.push_back(vec![Event::Text("x".to_string())]);
                }
            }
            22 => {
                // The step only runs once the worker has drawn the last
                // keystroke (or the settle limit passed).
                if !Self::rendered(app) {
                    return Err("the canvas did not catch up with typing".into());
                }
                let rev = app.session.get(None).map_or(0, |o| o.revision());
                let typed = rev.saturating_sub(self.data.typed_from);
                if typed < KEYSTROKES as u64 {
                    return Err(format!(
                        "{typed} of {KEYSTROKES} keystrokes reached the document"
                    ));
                }
                let t = app.perf.as_ref().map(|p| p.totals()).ok_or("no stats")?;
                let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
                let line = format!(
                    "perf while typing {KEYSTROKES} keys into 50 pages: {} frames, ui {:.2} ms avg / {:.2} ms max (without commands and layout {:.2} ms max, {:.2} ms second-worst; commands {:.1} ms, layout {:.1} ms), ui-thread rasters {}, worker renders {} ({:.1} ms), uploaded {} kpx",
                    t.frames,
                    ms(t.ui) / f64::from(t.frames.max(1)),
                    ms(t.ui_max),
                    ms(t.overhead_max),
                    ms(t.overhead_second),
                    ms(t.command),
                    ms(t.layout),
                    t.ui_rasters,
                    t.worker_renders,
                    ms(t.worker_time),
                    t.uploaded_px / 1000,
                );
                tracing::info!("{line}");
                if t.ui_rasters > 0 {
                    return Err(format!(
                        "{} page(s) were rasterised on the UI thread while typing\n{line}",
                        t.ui_rasters
                    ));
                }
                if t.worker_renders == 0 {
                    return Err(format!(
                        "the render worker drew nothing while typing\n{line}"
                    ));
                }
                if ms(t.overhead_second) > MAX_UI_OVERHEAD_MS {
                    return Err(format!(
                        "two frames spent more than {MAX_UI_OVERHEAD_MS} ms of UI-thread time besides commands and layout (second-worst {:.1} ms, worst {:.1} ms)\n{line}",
                        ms(t.overhead_second),
                        ms(t.overhead_max)
                    ));
                }
                if ms(t.overhead_max) > MAX_UI_OUTLIER_MS {
                    return Err(format!(
                        "a frame spent {:.1} ms of UI-thread time besides commands and layout (limit {MAX_UI_OUTLIER_MS} ms for the worst frame)\n{line}",
                        ms(t.overhead_max)
                    ));
                }
                self.ok(format!(
                    "typed {KEYSTROKES} keys into a 50-page publication: no page rasterised on the UI thread, besides commands and layout second-worst frame {:.1} ms (limit {MAX_UI_OVERHEAD_MS} ms), worst {:.1} ms (limit {MAX_UI_OUTLIER_MS} ms)",
                    ms(t.overhead_second),
                    ms(t.overhead_max)
                ));
                self.ok(line);
            }
            23 => {
                // Page Setup → Labels: Avery 5160 on a 2.625 x 1 in page.
                app.new_blank();
                app.exec(Command::SetPageSize {
                    size: pubrs_api::core::Size::new(189.0, 72.0),
                });
                self.key(Key::P, Modifiers::COMMAND | Modifiers::SHIFT);
            }
            24 => {
                if !matches!(app.dialog, Some(crate::dialogs::Dialog::PageSetup(_))) {
                    return Err("Cmd+Shift+P did not open Page Setup".into());
                }
                self.click_target(ctx, "tab-labels")?;
            }
            25 => self.click_target(ctx, "mode-sheet")?,
            26 => self.click_target(ctx, "preset-combo")?,
            27 => self.click_target(ctx, "preset:avery-5160")?,
            28 => {
                self.screenshot(ctx);
                self.click_target(ctx, "ok")?;
            }
            29 => {
                if app.dialog.is_some() {
                    return Err("Page Setup did not close on OK".into());
                }
                let setup = app.doc().ok_or("no doc")?.setup.clone();
                let sheet = setup.sheet.ok_or("OK did not set a sheet")?;
                if sheet.preset != "avery-5160" {
                    return Err(format!("unexpected preset {}", sheet.preset));
                }
                let path = self.out.join("labels.pdf");
                if !app.export_pdf_to(&path) {
                    return Err("export_pdf failed".into());
                }
                let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                let text = String::from_utf8_lossy(&bytes);
                let boxes = text.matches("/MediaBox").count();
                let at = text.find("/MediaBox").ok_or("no MediaBox")?;
                let media: String = text[at..].chars().take_while(|c| *c != ']').collect();
                if boxes != 1 || !media.contains("612") || !media.contains("792") {
                    return Err(format!("expected one Letter sheet, got {boxes}: {media}"));
                }
                self.ok("Page Setup (keyboard + mouse): Avery 5160 chosen, exported PDF has a sheet-sized page");
            }
            // ---- rulers, guides from rulers, find and replace (M26)
            30 => {
                app.new_from_template("trifold_brochure")
                    .ok_or("template_apply failed")?;
                let (body, cont) = (
                    find(app, "inside_body").ok_or("no inside_body")?,
                    find(app, "inside_body_continued").ok_or("no continuation")?,
                );
                app.session
                    .text_set(TextSet {
                        doc_id: None,
                        frame_id: Some(body),
                        story_id: None,
                        text: STORY.to_string(),
                        markup: false,
                    })
                    .map_err(|e| e.message)?;
                app.session
                    .text_link_frames(TextLinkFrames {
                        doc_id: None,
                        from_frame_id: body,
                        to_frame_id: cont,
                    })
                    .map_err(|e| e.message)?;
                if !app.show_rulers {
                    return Err("rulers are off by default".into());
                }
                if let Some(v) = app.view_mut() {
                    v.page = 1;
                    v.selection.clear();
                    v.edit = None;
                }
            }
            31 => {
                // Drag from the top ruler onto the page: a horizontal guide.
                let (pw, _) = Self::page_size(app)?;
                self.data.guides_before = Self::page_guides(app).len();
                self.data.hist = Self::history_len(app);
                let from = Self::ruler_point(app, true, pw / 2.0)?;
                let to = Self::screen_of(app, Point::new(pw / 2.0, 200.0)).ok_or("no canvas")?;
                self.drag(from, to);
            }
            32 => {
                let guides = Self::page_guides(app);
                if guides.len() != self.data.guides_before + 1 {
                    return Err(format!(
                        "dragging from the ruler made {} guide(s)",
                        guides.len() - self.data.guides_before
                    ));
                }
                let g = guides.last().expect("one");
                if g.axis != pubrs_api::core::Axis::Horizontal || (g.position - 200.0).abs() > 0.5 {
                    return Err(format!("unexpected guide {g:?}"));
                }
                if Self::history_len(app) != self.data.hist + 1 {
                    return Err("creating a guide was not one undo step".into());
                }
                self.ok(format!(
                    "dragged from the top ruler: horizontal guide at {:.2} pt (one undo step)",
                    g.position
                ));
                self.screenshot(ctx);
                // Drag the guide itself.
                let from = Self::screen_of(app, Point::new(GX, 200.0)).ok_or("no canvas")?;
                let to = Self::screen_of(app, Point::new(GX, 260.0)).ok_or("no canvas")?;
                self.drag(from, to);
            }
            33 => {
                let guides = Self::page_guides(app);
                let g = guides.last().ok_or("guide vanished")?;
                if guides.len() != self.data.guides_before + 1 || (g.position - 260.0).abs() > 0.5 {
                    return Err(format!("the guide did not move to 260 pt: {guides:?}"));
                }
                if Self::history_len(app) != self.data.hist + 2 {
                    return Err("moving a guide was not one undo step".into());
                }
                self.ok(format!("dragged the guide to {:.2} pt", g.position));
                // Back onto the ruler deletes it.
                let (pw, _) = Self::page_size(app)?;
                let from = Self::screen_of(app, Point::new(GX, 260.0)).ok_or("no canvas")?;
                let to = Self::ruler_point(app, true, pw / 2.0)?;
                self.drag(from, to);
            }
            34 => {
                if Self::page_guides(app).len() != self.data.guides_before {
                    return Err("dragging the guide onto the ruler did not delete it".into());
                }
                self.ok("dragged the guide back onto the ruler: deleted");
                self.key(Key::Z, Modifiers::COMMAND);
            }
            35 => {
                let guides = Self::page_guides(app);
                let g = guides.last().ok_or("undo did not bring the guide back")?;
                if guides.len() != self.data.guides_before + 1 || (g.position - 260.0).abs() > 0.5 {
                    return Err(format!("undo restored {guides:?}"));
                }
                self.ok("Cmd+Z brought the deleted guide back");
                // Off the page, to the left, deletes it too.
                let from = Self::screen_of(app, Point::new(GX, 260.0)).ok_or("no canvas")?;
                let to = Self::screen_of(app, Point::new(-14.0, 260.0)).ok_or("no canvas")?;
                self.drag(from, to);
            }
            36 => {
                if Self::page_guides(app).len() != self.data.guides_before {
                    return Err("dragging the guide off the page did not delete it".into());
                }
                self.ok("dragged the guide off the page: deleted");
                self.key(Key::Z, Modifiers::COMMAND);
            }
            37 => {
                if Self::page_guides(app).len() != self.data.guides_before + 1 {
                    return Err("undo did not bring the guide back".into());
                }
                // The left ruler makes a vertical guide.
                let (_, ph) = Self::page_size(app)?;
                let from = Self::ruler_point(app, false, ph / 2.0)?;
                let to = Self::screen_of(app, Point::new(150.0, ph / 2.0)).ok_or("no canvas")?;
                self.drag(from, to);
            }
            38 => {
                let guides = Self::page_guides(app);
                let g = guides.last().ok_or("no guides")?;
                if guides.len() != self.data.guides_before + 2
                    || g.axis != pubrs_api::core::Axis::Vertical
                    || (g.position - 150.0).abs() > 0.5
                {
                    return Err(format!("left ruler drag made {guides:?}"));
                }
                self.ok(format!(
                    "dragged from the left ruler: vertical guide at {:.2} pt",
                    g.position
                ));
                self.screenshot(ctx);
                self.key(Key::Z, Modifiers::COMMAND);
            }
            39 => {
                if Self::page_guides(app).len() != self.data.guides_before + 1 {
                    return Err("undo did not remove the vertical guide".into());
                }
                // Find: Cmd+F, type, Enter.
                self.key(Key::F, Modifiers::COMMAND);
            }
            40 => {
                if app.find.mode != crate::find::Mode::Bar {
                    return Err("Cmd+F did not open the find bar".into());
                }
                self.pending
                    .push_back(vec![Event::Text("sourdough".to_string())]);
            }
            41 => {
                if app.find.query != "sourdough" {
                    return Err(format!("find query is {:?}", app.find.query));
                }
                self.key(Key::Enter, Modifiers::NONE);
            }
            42 => {
                let m = Self::navigable_matches(app, "sourdough")?;
                if m.len() < 2 {
                    return Err(format!(
                        "expected two matches in the story, got {}",
                        m.len()
                    ));
                }
                let e = app
                    .view()
                    .and_then(|v| v.edit.clone())
                    .ok_or("Enter did not select a match")?;
                if e.range() != (m[0].start, m[0].end) || Some(e.story) != m[0].story_id {
                    return Err(format!("selection {:?} is not the first match", e.range()));
                }
                self.data.first_match = Some(e.range());
                self.ok(format!(
                    "Cmd+F, typed a word, Enter: selected match 1 of {} at {:?}",
                    app.find.match_count(),
                    e.range()
                ));
                self.screenshot(ctx);
                self.key(Key::Enter, Modifiers::NONE);
            }
            43 => {
                let m = Self::navigable_matches(app, "sourdough")?;
                let e = app
                    .view()
                    .and_then(|v| v.edit.clone())
                    .ok_or("no selection")?;
                if e.range() != (m[1].start, m[1].end) || Some(e.range()) == self.data.first_match {
                    return Err(format!("Enter again selected {:?}, not match 2", e.range()));
                }
                self.ok("Enter again selected the next match");
                self.key(Key::Escape, Modifiers::NONE);
            }
            44 => {
                if app.find.mode != crate::find::Mode::Closed {
                    return Err("Escape did not close the find bar".into());
                }
                self.ok("Escape closed the find bar");
                self.key(Key::H, Modifiers::COMMAND);
            }
            45 => {
                if app.find.mode != crate::find::Mode::Replace {
                    return Err("Cmd+H did not open Find and Replace".into());
                }
                self.click_target(ctx, "find-replacement")?;
            }
            46 => self
                .pending
                .push_back(vec![Event::Text("levain".to_string())]),
            47 => {
                if app.find.replacement != "levain" {
                    return Err(format!("replacement is {:?}", app.find.replacement));
                }
                self.data.matches = Self::navigable_matches(app, "sourdough")?.len();
                self.data.hist = Self::history_len(app);
                self.click_target(ctx, "find-replace-all")?;
            }
            48 => {
                let n = self.data.matches;
                if app.find.last_replaced != Some(n) || n < 2 {
                    return Err(format!(
                        "Replace All reported {:?} for {n} matches",
                        app.find.last_replaced
                    ));
                }
                if !Self::navigable_matches(app, "sourdough")?.is_empty()
                    || Self::navigable_matches(app, "levain")?.len() < n
                {
                    return Err("Replace All left the old word or lost the new one".into());
                }
                self.ok(format!("Cmd+H, Replace All: {n} replaced"));
                self.screenshot(ctx);
                self.key(Key::Z, Modifiers::COMMAND);
            }
            49 => {
                let n = self.data.matches;
                if Self::navigable_matches(app, "sourdough")?.len() != n {
                    return Err("one Cmd+Z did not restore every replacement".into());
                }
                crate::find::close(app);
                self.ok("a single Cmd+Z restored every replaced match (one undo step)");
            }
            50 => {
                // Paragraph dialog: hyphenation on a justified story.
                app.new_blank();
                let req: pubrs_api::tools::frames::FrameAddText =
                    serde_json::from_value(serde_json::json!({
                        "x": 40.0, "y": 40.0, "width": 150.0, "height": 400.0,
                        "align": "justify",
                        "text": "Extraordinary hyphenation demonstrates comprehensive typographic \
                    craftsmanship throughout international newsletters, particularly considering \
                    unforgettable communication opportunities and administration.",
                    }))
                    .map_err(|e| e.to_string())?;
                let out = app.session.frame_add_text(req).map_err(|e| e.message)?;
                app.select(vec![out.frame_id]);
                self.key(Key::P, Modifiers::COMMAND | Modifiers::ALT);
            }
            51 => {
                if !matches!(app.dialog, Some(crate::dialogs::Dialog::Paragraph(_))) {
                    return Err(format!(
                        "Cmd+Alt+P did not open the Paragraph dialog (selection {:?}, dialog open: {})",
                        app.selection(),
                        app.dialog.is_some()
                    ));
                }
                self.click_target(ctx, "hyphenate")?;
            }
            52 => {
                self.click_target(ctx, "limit")?;
            }
            53 => {
                self.screenshot(ctx);
                // Confirm from the keyboard.
                self.key(Key::Enter, Modifiers::NONE);
            }
            54 => {
                if app.dialog.is_some() {
                    return Err("Enter did not close the Paragraph dialog".into());
                }
                let id = app.doc_id();
                let lay = app.session.layout(id).map_err(|e| e.message)?;
                let hyphenated = lay
                    .stories
                    .values()
                    .flat_map(|s| s.frames.values())
                    .flat_map(|f| f.lines.iter())
                    .filter(|l| {
                        l.runs
                            .last()
                            .and_then(|r| r.glyphs.last())
                            .is_some_and(|g| g.text.is_empty() && g.advance > 0.0)
                    })
                    .count();
                if hyphenated == 0 {
                    return Err("no hyphenated line after turning Hyphenate on".into());
                }
                self.ok(format!(
                    "Paragraph dialog (keyboard + mouse): Hyphenate on, {hyphenated} hyphenated line(s) in the layout"
                ));
            }
            55 => {
                // M21: swatches. Design → Swatches opens the section; "+ CMYK"
                // opens the editor.
                if let Some(v) = app.view_mut() {
                    v.selection.clear();
                    v.edit = None;
                }
                app.run_action(ctx, Action::ShowSwatches);
                self.wait = 6;
            }
            56 => {
                self.click_target(ctx, "swatch-add-cmyk")?;
            }
            57 => {
                if !matches!(app.dialog, Some(crate::dialogs::Dialog::Swatch(_))) {
                    return Err("+ CMYK did not open the swatch editor".into());
                }
                self.screenshot(ctx);
                self.click_target(ctx, "swatch-save")?;
            }
            58 => {
                let doc = app.doc().ok_or("no doc")?;
                let sw = doc
                    .swatches
                    .iter()
                    .find(|s| matches!(s.value, pubrs_api::core::SwatchValue::Cmyk(_)))
                    .ok_or("the CMYK swatch was not added")?;
                if app.dialog.is_some() {
                    return Err("the swatch editor did not close".into());
                }
                self.data.swatch = Some(sw.id);
                self.data.swatch_name = Some(sw.name.clone());
                // A probe rectangle on the page, selected, so the properties
                // panel shows its Fill picker.
                let doc_id = app.doc_id().ok_or("no doc")?;
                let page = app.view().map_or(0, |v| v.page);
                let page_id = app.doc().and_then(|d| d.pages.get(page)).map(|p| p.id);
                let o = app
                    .session
                    .frame_add_shape(pubrs_api::tools::frames::FrameAddShape {
                        doc_id: Some(doc_id),
                        page_id,
                        master_id: None,
                        shape: pubrs_api::tools::frames::ShapeKind::Rectangle,
                        x: 20.0,
                        y: 20.0,
                        width: 80.0,
                        height: 50.0,
                        corner_radius: None,
                        line_direction: None,
                        fill: None,
                        stroke: None,
                        stroke_width: None,
                        dash: None,
                        name: Some("colour probe".into()),
                    })
                    .map_err(|e| e.message)?;
                self.data.probe = Some(o.frame_id);
                if let Some(v) = app.view_mut() {
                    v.selection = vec![o.frame_id];
                }
                self.wait = 6;
            }
            59 => {
                let f = self.data.probe.ok_or("no probe")?;
                self.click_target(ctx, &format!("paint-fill-{f}"))?;
            }
            60 => {
                let name = self.data.swatch_name.clone().ok_or("no swatch")?;
                self.click_target(ctx, &format!("paint-swatch-{name}"))?;
            }
            61 => {
                let f = self.data.probe.ok_or("no probe")?;
                let sw = self.data.swatch.ok_or("no swatch")?;
                let fill = app
                    .doc()
                    .and_then(|d| d.frame(f))
                    .and_then(|f| f.style.fill.clone());
                if fill != Some(pubrs_api::core::Paint::swatch(sw)) {
                    return Err(format!("the picker set the fill to {fill:?}"));
                }
                // CMYK 0/100/100/0 is red by the plain formula.
                let px = Self::probe_pixel(app, 60, 45)?;
                if px[..3] != [255, 0, 0] {
                    return Err(format!("CMYK swatch fill rendered as {px:?}, not red"));
                }
                self.ok("created a CMYK swatch in the editor and filled a shape with it from the picker");
                self.screenshot(ctx);
                app.run_action(ctx, Action::ColorScheme);
            }
            62 => {
                if !matches!(app.dialog, Some(crate::dialogs::Dialog::ColorScheme)) {
                    return Err("Design > Colour Scheme did not open the dialog".into());
                }
                self.screenshot(ctx);
                self.click_target(ctx, "scheme-apply-coral_reef")?;
            }
            63 => {
                let doc = app.doc().ok_or("no doc")?;
                if doc.scheme.as_ref().map(|s| s.0.as_str()) != Some("coral_reef")
                    || doc.swatches.len() < 8
                {
                    return Err("applying the scheme did not add its slot swatches".into());
                }
                self.click_target(ctx, "scheme-close")?;
            }
            64 => {
                if app.dialog.is_some() {
                    return Err("the scheme dialog did not close".into());
                }
                // Fill the probe with the scheme's main colour from the picker.
                let f = self.data.probe.ok_or("no probe")?;
                self.click_target(ctx, &format!("paint-fill-{f}"))?;
            }
            65 => {
                self.click_target(ctx, "paint-swatch-Scheme: Main")?;
            }
            66 => {
                let px = Self::probe_pixel(app, 60, 45)?;
                if px[..3] != [0x0F, 0x5E, 0x73] {
                    return Err(format!("scheme main rendered as {px:?}, not Coral Reef's"));
                }
                self.data.before = Some(px);
                app.run_action(ctx, Action::ColorScheme);
            }
            67 => {
                self.click_target(ctx, "scheme-apply-lantern_harbor")?;
            }
            68 => {
                let px = Self::probe_pixel(app, 60, 45)?;
                if Some(px) == self.data.before || px[..3] != [0x1E, 0x3A, 0x5F] {
                    return Err(format!("another scheme left the fill at {px:?}"));
                }
                self.ok("applied two colour schemes from the dialog: the fill re-rendered in each scheme's colour");
                self.screenshot(ctx);
                self.click_target(ctx, "scheme-close")?;
            }
            69 => {
                if app.dialog.is_some() {
                    return Err("the scheme dialog did not close".into());
                }
                // File → Export PDF…: Commercial printing preset (PDF/X-4).
                app.new_from_template("flyer")
                    .ok_or("template_apply failed")?;
                app.run_action(ctx, Action::ExportPdf);
            }
            70 => {
                if !matches!(app.dialog, Some(crate::dialogs::Dialog::ExportPdf(_))) {
                    return Err("File > Export PDF did not open the Export dialog".into());
                }
                self.click_target(ctx, "preset-commercial")?;
            }
            71 => {
                let path = self.out.join("commercial.pdf");
                let _ = std::fs::remove_file(&path);
                match &mut app.dialog {
                    Some(crate::dialogs::Dialog::ExportPdf(st)) => {
                        if st.standard() != pubrs_api::tools::export::PdfStandardIn::Pdfx4
                            || !st.bleed
                            || !st.crop_marks
                        {
                            return Err(
                                "the Commercial preset did not set PDF/X-4, bleed and crop marks"
                                    .into(),
                            );
                        }
                        st.target = Some(path);
                    }
                    _ => return Err("the Export dialog closed".into()),
                }
                self.screenshot(ctx);
                self.click_target(ctx, "export-go")?;
            }
            72 => {
                if app.dialog.is_some() {
                    return Err("the Export dialog did not close after Export".into());
                }
                let bytes = std::fs::read(self.out.join("commercial.pdf"))
                    .map_err(|e| format!("no commercial.pdf: {e}"))?;
                let text = String::from_utf8_lossy(&bytes);
                if !text.contains("/S/GTS_PDFX") || !text.contains("/GTS_PDFXVersion(PDF/X-4)") {
                    return Err(
                        "the Commercial printing export has no PDF/X-4 markers (GTS_PDFX)".into(),
                    );
                }
                let issues = pubrs_api::render::pdfx::check(&bytes);
                if !issues.is_empty() {
                    return Err(format!("the PDF/X-4 export fails its check: {issues:?}"));
                }
                self.ok("Export PDF dialog: the Commercial printing preset wrote a PDF/X-4 file (GTS_PDFX) that passes the checker");
                app.run_action(ctx, Action::ExportPdf);
            }
            73 => {
                // Keyboard: Escape closes the dialog.
                if !matches!(app.dialog, Some(crate::dialogs::Dialog::ExportPdf(_))) {
                    return Err("the Export dialog did not reopen".into());
                }
                self.click_target(ctx, "preset-archive")?;
            }
            74 => {
                match &app.dialog {
                    Some(crate::dialogs::Dialog::ExportPdf(st)) => {
                        if st.standard() != pubrs_api::tools::export::PdfStandardIn::Pdfa2b
                            || st.crop_marks
                        {
                            return Err(
                                "the Archive preset did not choose PDF/A-2b without crop marks"
                                    .into(),
                            );
                        }
                    }
                    _ => return Err("the Export dialog closed".into()),
                }
                self.key(Key::Escape, Modifiers::NONE);
            }
            75 => {
                if app.dialog.is_some() {
                    return Err("Escape did not close the Export dialog".into());
                }
            }
            76 => {
                // M15: a page break typed while editing, keep with next and
                // a drop cap. Frame A on page 1 holds the whole story; frame
                // B on page 2 (two columns) continues it.
                app.new_blank();
                let id = app.doc_id();
                let p2 = app
                    .session
                    .page_add(pubrs_api::tools::pages::PageAdd {
                        doc_id: id,
                        index: None,
                        master_id: None,
                    })
                    .map_err(|e| e.message)?;
                let add = |app: &mut PubrsApp, v: serde_json::Value| -> Result<FrameId, String> {
                    let req: pubrs_api::tools::frames::FrameAddText =
                        serde_json::from_value(v).map_err(|e| e.to_string())?;
                    Ok(app
                        .session
                        .frame_add_text(req)
                        .map_err(|e| e.message)?
                        .frame_id)
                };
                let a = add(
                    app,
                    serde_json::json!({
                        "x": 40.0, "y": 40.0, "width": 300.0, "height": 400.0,
                        "text": M15_TEXT,
                    }),
                )?;
                let b = add(
                    app,
                    serde_json::json!({
                        "page_id": p2.page_id, "x": 40.0, "y": 40.0, "width": 300.0, "height": 100.0,
                    }),
                )?;
                app.session
                    .text_link_frames(TextLinkFrames {
                        doc_id: id,
                        from_frame_id: a,
                        to_frame_id: b,
                    })
                    .map_err(|e| e.message)?;
                // Six lines per column in B, so the heading ends column 1.
                let h = Self::m15_lines(app, a)?
                    .first()
                    .map(|l| l.rect.height)
                    .ok_or("frame A has no lines")?;
                app.exec(Command::SetTextFrame {
                    frame: b,
                    props: pubrs_api::core::TextFrameProps {
                        columns: Some(2),
                        column_gap: Some(12.0),
                        ..Default::default()
                    },
                });
                app.exec(Command::SetFrameRect {
                    frame: b,
                    rect: pubrs_api::core::Rect::new(40.0, 40.0, 300.0, 8.0 + 6.0 * h + 0.5),
                });
                // Selected first, so the properties panel is already open
                // when the double-click lands.
                app.select(vec![a]);
                self.data.m15 = Some((a, b));
                self.screenshot(ctx);
            }
            77 => {
                let (a, _) = self.data.m15.ok_or("no M15 frames")?;
                let lines = Self::m15_lines(app, a)?;
                if lines.len() < 8 {
                    return Err(format!(
                        "frame A shows {} lines, not the whole story",
                        lines.len()
                    ));
                }
                let pos = Self::m15_line_pos(app, a, &lines[0])?;
                self.fresh_double_click(pos);
            }
            78 => {
                let (a, _) = self.data.m15.ok_or("no M15 frames")?;
                let editing = app.view().and_then(|v| v.edit.as_ref()).map(|e| e.frame);
                if editing != Some(a) {
                    return Err(format!(
                        "double-click did not start editing frame A ({editing:?})"
                    ));
                }
                // End of the first line, then Cmd+Enter: a page break.
                self.key(Key::End, Modifiers::NONE);
                self.key(Key::Enter, Modifiers::COMMAND);
            }
            79 => {
                let (a, b) = self.data.m15.ok_or("no M15 frames")?;
                let at = M15_TEXT.find('\n').unwrap_or(0);
                let doc = app.doc().ok_or("no doc")?;
                let story = doc.story_of(a).ok_or("no story")?;
                let text = doc.story(story).map(|s| s.plain_text()).unwrap_or_default();
                if text.chars().nth(at) != Some(pubrs_api::core::text::PAGE_BREAK) {
                    return Err(format!("no page break at {at} after Cmd+Enter: {text:?}"));
                }
                let lay = app.session.layout(app.doc_id()).map_err(|e| e.message)?;
                let (fa, fb) = (lay.frame(a).ok_or("A")?, lay.frame(b).ok_or("B")?);
                if fa.chars.end != at || fb.chars.start != at + 1 {
                    return Err(format!(
                        "the text after the page break did not move to page 2: A {:?}, B {:?}",
                        fa.chars, fb.chars
                    ));
                }
                self.ok(format!(
                    "Cmd+Enter while editing inserted a page break: page 1 shows {:?}, the rest starts on page 2",
                    fa.chars
                ));
                // Leave editing and go to page 2.
                self.key(Key::Escape, Modifiers::NONE);
                self.key(Key::PageDown, Modifiers::NONE);
            }
            80 => {
                if app.view().map(|v| v.page) != Some(1) {
                    return Err("Page Down did not show page 2".into());
                }
                let why = app.unavailable(Action::InsertPageBreak);
                if why.is_none() {
                    return Err("Insert ▸ Break is available without a caret".into());
                }
                let (_, b) = self.data.m15.ok_or("no M15 frames")?;
                let heading = Self::m15_para_lines(app, b, 5)?;
                let body = Self::m15_para_lines(app, b, 6)?;
                let col = |l: &pubrs_api::layout::LineBox| usize::from(l.rect.x > 150.0);
                if heading.len() != 1 || col(&heading[0]) != 0 || body.first().map(col) != Some(1) {
                    return Err(format!(
                        "setup: the heading should end column 1 and its text start column 2 ({} heading lines)",
                        heading.len()
                    ));
                }
                let pos = Self::m15_line_pos(app, b, &heading[0])?;
                self.fresh_double_click(pos);
                self.ok(format!(
                    "Insert ▸ Break without a caret is unavailable: \"{}\"",
                    why.unwrap_or_default()
                ));
            }
            81 => {
                let (_, b) = self.data.m15.ok_or("no M15 frames")?;
                let editing = app.view().and_then(|v| v.edit.as_ref()).map(|e| e.frame);
                if editing != Some(b) {
                    return Err(format!(
                        "double-click did not start editing frame B ({editing:?})"
                    ));
                }
                self.key(Key::P, Modifiers::COMMAND | Modifiers::ALT);
            }
            82 => {
                if !matches!(app.dialog, Some(crate::dialogs::Dialog::Paragraph(_))) {
                    return Err("Cmd+Alt+P did not open the Paragraph dialog while editing".into());
                }
                self.click_target(ctx, "tab-keeps")?;
            }
            83 => {
                self.click_target(ctx, "keep-with-next")?;
            }
            84 => {
                self.screenshot(ctx);
            }
            85 => {
                self.key(Key::Enter, Modifiers::NONE);
            }
            86 => {
                if app.dialog.is_some() {
                    return Err("Enter did not close the Paragraph dialog".into());
                }
                let (_, b) = self.data.m15.ok_or("no M15 frames")?;
                let heading = Self::m15_para_lines(app, b, 5)?;
                if heading.first().is_none_or(|l| l.rect.x < 150.0) {
                    return Err("Keep with next did not move the heading to column 2".into());
                }
                self.ok("Paragraph ▸ Keeps ▸ Keep with next: the heading moved to column 2 with its text");
                // To the start of the next paragraph for the drop cap.
                self.key(Key::End, Modifiers::NONE);
                self.key(Key::ArrowRight, Modifiers::NONE);
            }
            87 => {
                let (a, _) = self.data.m15.ok_or("no M15 frames")?;
                let doc = app.doc().ok_or("no doc")?;
                let story = doc.story_of(a).ok_or("no story")?;
                let body = doc.story(story).map_or(0, |s| s.paragraph_start(6));
                let caret = app.view().and_then(|v| v.edit.as_ref()).map(|e| e.caret);
                if caret != Some(body) {
                    return Err(format!(
                        "End, Right did not reach the paragraph start {body}: {caret:?}"
                    ));
                }
                app.run_action(ctx, Action::DropCap);
            }
            88 => {
                if !matches!(app.dialog, Some(crate::dialogs::Dialog::DropCap(_))) {
                    return Err("Text ▸ Drop Cap… did not open its dialog".into());
                }
                self.screenshot(ctx);
            }
            89 => {
                self.key(Key::Enter, Modifiers::NONE);
            }
            90 => {
                if app.dialog.is_some() {
                    return Err("Enter did not close the Drop Cap dialog".into());
                }
                let (_, b) = self.data.m15.ok_or("no M15 frames")?;
                let body = Self::m15_para_lines(app, b, 6)?;
                let (Some(first), Some(third)) = (body.first(), body.get(2)) else {
                    return Err("the paragraph lost its lines".into());
                };
                let (cap, text) = match &first.runs[..] {
                    [cap, text, ..] => (cap, text),
                    _ => return Err("no drop cap run on the first line".into()),
                };
                let g = cap.glyphs.first().ok_or("empty drop cap")?;
                if cap.font_size < 2.0 * text.font_size
                    || (g.y - third.baseline).abs() > 0.01
                    || third.rect.x < g.x + g.advance
                {
                    return Err(format!(
                        "drop cap: {:.1} pt (text {:.1} pt), baseline {:.2} vs line 3 {:.2}, line 3 at x {:.1}",
                        cap.font_size, text.font_size, g.y, third.baseline, third.rect.x
                    ));
                }
                self.ok(format!(
                    "Text ▸ Drop Cap…: a {:.1} pt cap spans 3 lines, which are indented past it",
                    cap.font_size
                ));
                self.screenshot(ctx);
            }
            91 => {
                if app.dialog.is_some() {
                    return Err("a dialog is still open after the drop cap step".into());
                }
                // M28: spelling. A blank publication with one text frame.
                app.new_blank();
                let req: pubrs_api::tools::frames::FrameAddText =
                    serde_json::from_value(serde_json::json!({
                        "x": 72.0, "y": 72.0, "width": 360.0, "height": 160.0,
                        "font_size": 24.0,
                        "text": "Fresh bread every morning.",
                    }))
                    .map_err(|e| e.to_string())?;
                let out = app.session.frame_add_text(req).map_err(|e| e.message)?;
                self.data.spell_frame = Some(out.frame_id);
                app.select(vec![out.frame_id]);
                // Let the input clock pass two double-click delays since the
                // last click, so egui does not count a triple click.
                for _ in 0..40 {
                    self.pending.push_back(Vec::new());
                }
            }
            92 => {
                // Double-click the frame to edit its text, Cmd+End to its end.
                let f = self.data.spell_frame.ok_or("no frame")?;
                let center = app
                    .doc()
                    .and_then(|d| d.frame(f))
                    .map(|f| f.rect.center())
                    .ok_or("no frame")?;
                let pos = Self::screen_of(app, center).ok_or("canvas not laid out")?;
                self.click(pos, true);
            }
            93 => {
                let e = app
                    .view()
                    .and_then(|v| v.edit.clone())
                    .ok_or_else(|| {
                        format!(
                            "double-click did not start editing (selection {:?}, frame {:?}, xf {:?}, canvas {:?})",
                            app.selection(),
                            self.data.spell_frame,
                            app.view().and_then(|v| v.xf),
                            app.view().and_then(|v| v.canvas_rect)
                        )
                    })?;
                if Some(e.frame) != self.data.spell_frame {
                    return Err(format!("double-click edits frame {:?}", e.frame));
                }
                self.key(Key::End, Modifiers::COMMAND);
            }
            94 => {
                let e = app
                    .view()
                    .and_then(|v| v.edit.clone())
                    .ok_or("Cmd+End stopped text editing")?;
                let n = Self::spell_text(app, self.data.spell_frame)?
                    .chars()
                    .count();
                if e.caret != n {
                    return Err(format!("Cmd+End put the caret at {} of {n}", e.caret));
                }
                // Type a misspelt word, one character a frame.
                for c in " Delicous".chars() {
                    self.pending.push_back(vec![Event::Text(c.to_string())]);
                }
            }
            95 => {
                let text = Self::spell_text(app, self.data.spell_frame)?;
                if !text.ends_with(" Delicous") {
                    return Err(format!("the typed text is {text:?}"));
                }
                // Wait for the spell worker and the squiggle.
                let Some((start, end)) =
                    Self::spell_issue(app, "Delicous").filter(|_| app.spell.drawn > 0)
                else {
                    return self.again("a squiggle under the typed word");
                };
                self.data.spell_range = Some((start, end));
                self.ok(format!(
                    "typed a misspelt word: the spell worker flagged \"Delicous\" at {start}..{end} and the canvas draws {} squiggle(s)",
                    app.spell.drawn
                ));
                self.screenshot(ctx);
            }
            96 => {
                // Right-click the word.
                let (start, end) = self.data.spell_range.ok_or("no range")?;
                let f = self.data.spell_frame.ok_or("no frame")?;
                let pos = Self::range_center(app, f, start, end)?;
                self.right_click(pos);
            }
            97 => {
                let m = app
                    .spell
                    .menu
                    .clone()
                    .ok_or("right-click on the word did not open the spelling menu")?;
                let Some(s) = m.suggestions else {
                    return self.again("suggestions in the spelling menu");
                };
                self.data.suggestion =
                    Some(s.first().cloned().ok_or("no suggestions for Delicous")?);
                self.screenshot(ctx);
            }
            98 => {
                self.data.hist = Self::done_steps(app);
                self.click_target(ctx, "spell-suggestion-0")?;
            }
            99 => {
                let want = self.data.suggestion.clone().ok_or("no suggestion")?;
                if app.spell.menu.is_some() {
                    return Err("the spelling menu stayed open".into());
                }
                let text = Self::spell_text(app, self.data.spell_frame)?;
                if !text.ends_with(&format!(" {want}")) {
                    return Err(format!("picking {want:?} left the text {text:?}"));
                }
                let steps = Self::done_steps(app) - self.data.hist;
                if steps != 1 {
                    return Err(format!("the replacement took {steps} undo steps"));
                }
                self.ok(format!(
                    "right-click menu: picked {want:?}; the text changed in one undo step"
                ));
                self.key(Key::Z, Modifiers::COMMAND);
            }
            100 => {
                let text = Self::spell_text(app, self.data.spell_frame)?;
                if !text.ends_with(" Delicous") {
                    return Err(format!("one Cmd+Z left {text:?}"));
                }
                self.ok("one Cmd+Z restored the misspelt word");
                self.key(Key::F7, Modifiers::NONE);
            }
            101 => {
                let d = app
                    .spell
                    .dialog
                    .clone()
                    .ok_or("F7 did not open the Spelling window")?;
                let (Some(i), Some(_)) = (d.current.clone(), d.suggestions.clone()) else {
                    return self.again("a word and suggestions in the Spelling window");
                };
                if i.word != "Delicous" {
                    return Err(format!("the Spelling window shows {:?}", i.word));
                }
                let sel = app
                    .view()
                    .and_then(|v| v.edit.as_ref())
                    .map(TextEdit::range);
                if sel != Some((i.start, i.end)) {
                    return Err(format!("the window did not select the word: {sel:?}"));
                }
                self.ok(format!(
                    "F7: the Spelling window shows {:?} ({}) and selects it in the text",
                    i.word, i.language
                ));
                self.screenshot(ctx);
            }
            102 => {
                // Change, from the keyboard.
                self.data.hist = Self::done_steps(app);
                self.key(Key::Enter, Modifiers::NONE);
            }
            103 => {
                let want = self.data.suggestion.clone().ok_or("no suggestion")?;
                let text = Self::spell_text(app, self.data.spell_frame)?;
                if !text.ends_with(&format!(" {want}")) {
                    return Err(format!("Enter in the Spelling window left {text:?}"));
                }
                let d = app
                    .spell
                    .dialog
                    .clone()
                    .ok_or("the Spelling window closed")?;
                if !d.done {
                    return self.again("the end of the spelling check");
                }
                let steps = Self::done_steps(app) - self.data.hist;
                if steps != 1 {
                    return Err(format!("Change took {steps} undo steps"));
                }
                self.ok("Enter changed the word from the Spelling window (one undo step); the check is complete");
                self.screenshot(ctx);
            }
            104 => self.key(Key::Escape, Modifiers::NONE),
            105 => {
                if app.spell.dialog.is_some() {
                    return Err("Escape did not close the Spelling window".into());
                }
                self.ok("Escape closed the Spelling window");
            }
            106 => {
                // M16: a numbered list made with Bullets and Numbering… and
                // nested with Tab, then a price line with a right tab and a
                // dot leader. Frame L holds the list, frame T the price.
                app.new_blank();
                let add = |app: &mut PubrsApp, v: serde_json::Value| -> Result<FrameId, String> {
                    let req: pubrs_api::tools::frames::FrameAddText =
                        serde_json::from_value(v).map_err(|e| e.to_string())?;
                    Ok(app
                        .session
                        .frame_add_text(req)
                        .map_err(|e| e.message)?
                        .frame_id)
                };
                let l = add(
                    app,
                    serde_json::json!({
                        "x": 40.0, "y": 40.0, "width": 300.0, "height": 140.0,
                        "text": "Pick fruit\nApples\nGreen apples\nPears",
                    }),
                )?;
                let t = add(
                    app,
                    serde_json::json!({
                        "x": 40.0, "y": 220.0, "width": 300.0, "height": 60.0,
                        "text": "Tomato soup",
                    }),
                )?;
                app.select(vec![l]);
                self.data.m16 = Some((l, t));
                self.screenshot(ctx);
            }
            107 => {
                let (l, _) = self.data.m16.ok_or("no M16 frames")?;
                let lines = Self::m15_lines(app, l)?;
                let pos = Self::m15_line_pos(app, l, &lines[0])?;
                self.fresh_double_click(pos);
            }
            108 => {
                let (l, _) = self.data.m16.ok_or("no M16 frames")?;
                if app.view().and_then(|v| v.edit.as_ref()).map(|e| e.frame) != Some(l) {
                    return Err("double-click did not start editing the list frame".into());
                }
                self.key(Key::A, Modifiers::COMMAND);
            }
            109 => {
                app.run_action(ctx, Action::BulletsNumbering);
            }
            110 => {
                if !matches!(app.dialog, Some(crate::dialogs::Dialog::Lists(_))) {
                    return Err("Text ▸ Bullets and Numbering… did not open its dialog".into());
                }
                self.click_target(ctx, "list-number")?;
            }
            111 => {
                self.screenshot(ctx);
            }
            112 => {
                self.key(Key::Enter, Modifiers::NONE);
            }
            113 => {
                if app.dialog.is_some() {
                    return Err("Enter did not close the Bullets and Numbering dialog".into());
                }
                let (l, _) = self.data.m16.ok_or("no M16 frames")?;
                let got = Self::m16_labels(app, l)?;
                if got != ["1.", "2.", "3.", "4."] {
                    return Err(format!("numbered list labels are {got:?}"));
                }
                self.ok("Bullets and Numbering… made four numbered items (1. 2. 3. 4.)");
                // Click at the very start of "Green apples", then Tab.
                let lines = Self::m15_lines(app, l)?;
                let line = lines.get(2).ok_or("no third line")?;
                let r = app
                    .doc()
                    .and_then(|d| d.frame(l))
                    .map(|f| f.rect)
                    .ok_or("no frame")?;
                let p = pubrs_api::core::Point::new(
                    r.x + line.caret_x(line.chars.start),
                    r.y + line.rect.y + line.rect.height / 2.0,
                );
                let pos = Self::screen_of(app, p).ok_or("canvas not laid out")?;
                self.click(pos, false);
                self.key(Key::Tab, Modifiers::NONE);
            }
            114 => {
                let (l, _) = self.data.m16.ok_or("no M16 frames")?;
                let got = Self::m16_labels(app, l)?;
                if got != ["1.", "2.", "a.", "3."] {
                    return Err(format!("after Tab the labels are {got:?}, not 1. 2. a. 3."));
                }
                let doc = app.doc().ok_or("no doc")?;
                let text = doc
                    .story_of(l)
                    .and_then(|s| doc.story(s))
                    .map(|s| s.plain_text())
                    .unwrap_or_default();
                if text.contains('\t') {
                    return Err("Tab at the start of a list item inserted a tab".into());
                }
                self.ok("Tab at the start of an item nested it: labels 1. 2. a. 3.");
                self.screenshot(ctx);
                self.key(Key::Tab, Modifiers::SHIFT);
            }
            115 => {
                let (l, _) = self.data.m16.ok_or("no M16 frames")?;
                let got = Self::m16_labels(app, l)?;
                if got != ["1.", "2.", "3.", "4."] {
                    return Err(format!("after Shift+Tab the labels are {got:?}"));
                }
                self.ok("Shift+Tab brought it back to level 0");
                // Nest it again for the picture, then to the price line.
                self.key(Key::Tab, Modifiers::NONE);
                self.key(Key::Escape, Modifiers::NONE);
                let (_, t) = self.data.m16.ok_or("no M16 frames")?;
                app.select(vec![t]);
            }
            116 => {
                let (_, t) = self.data.m16.ok_or("no M16 frames")?;
                let lines = Self::m15_lines(app, t)?;
                let pos = Self::m15_line_pos(app, t, &lines[0])?;
                self.fresh_double_click(pos);
            }
            117 => {
                let (_, t) = self.data.m16.ok_or("no M16 frames")?;
                if app.view().and_then(|v| v.edit.as_ref()).map(|e| e.frame) != Some(t) {
                    return Err("double-click did not start editing the price frame".into());
                }
                self.key(Key::End, Modifiers::NONE);
                self.key(Key::Tab, Modifiers::NONE);
                self.pending
                    .push_back(vec![Event::Text("4.50".to_string())]);
            }
            118 => {
                let (_, t) = self.data.m16.ok_or("no M16 frames")?;
                let doc = app.doc().ok_or("no doc")?;
                let text = doc
                    .story_of(t)
                    .and_then(|s| doc.story(s))
                    .map(|s| s.plain_text())
                    .unwrap_or_default();
                if text != "Tomato soup\t4.50" {
                    return Err(format!("Tab did not insert a tab character: {text:?}"));
                }
                self.ok("Tab while editing inserted a tab character");
                self.key(Key::A, Modifiers::COMMAND);
            }
            119 => {
                app.run_action(ctx, Action::Tabs);
            }
            120 => {
                if !matches!(app.dialog, Some(crate::dialogs::Dialog::Tabs(_))) {
                    return Err("Text ▸ Tabs… did not open its dialog".into());
                }
                // Click the ruler at 250 pt: the box is 300 pt wide.
                let r = crate::dialogs::target_rect(ctx, "tab-ruler")
                    .ok_or("the ruler is not on screen")?;
                let x = r.left() + 250.0 * r.width() / 300.0;
                self.click(egui::pos2(x, r.center().y), false);
            }
            121 => {
                self.click_target(ctx, "tab-align-right")?;
            }
            122 => {
                self.click_target(ctx, "tab-leader-dot")?;
            }
            123 => {
                self.screenshot(ctx);
            }
            124 => {
                self.key(Key::Enter, Modifiers::NONE);
            }
            125 => {
                if app.dialog.is_some() {
                    return Err("Enter did not close the Tabs dialog".into());
                }
                let (_, t) = self.data.m16.ok_or("no M16 frames")?;
                let lines = Self::m15_lines(app, t)?;
                let line = lines.first().ok_or("the price frame lost its line")?;
                let dots: usize = line
                    .runs
                    .iter()
                    .filter(|r| !r.text.is_empty() && r.text.chars().all(|c| c == '.'))
                    .map(|r| r.glyphs.len())
                    .sum();
                let end = line.caret_x(line.chars.end);
                if dots < 8 || (end - 254.0).abs() > 0.6 {
                    return Err(format!(
                        "right tab with dot leader: {dots} leader dots, the price ends at x {end:.1} (stop at 254)"
                    ));
                }
                self.ok(format!(
                    "Text ▸ Tabs…: a right stop at 250 pt with a dot leader ({dots} dots); the price ends on the stop"
                ));
                self.screenshot(ctx);
            }
            126 => {
                // M17: character formatting. A blank publication with one
                // frame of 24 pt text.
                app.new_blank();
                let req: pubrs_api::tools::frames::FrameAddText =
                    serde_json::from_value(serde_json::json!({
                        "x": 40.0, "y": 40.0, "width": 420.0, "height": 120.0,
                        "font_size": 24.0,
                        "text": "Water x2 now small struck",
                    }))
                    .map_err(|e| e.to_string())?;
                let f = app
                    .session
                    .frame_add_text(req)
                    .map_err(|e| e.message)?
                    .frame_id;
                app.select(vec![f]);
                self.data.m17 = Some(f);
            }
            127 => {
                let f = self.data.m17.ok_or("no M17 frame")?;
                let lines = Self::m15_lines(app, f)?;
                let pos = Self::m15_line_pos(app, f, &lines[0])?;
                self.fresh_double_click(pos);
            }
            128 => {
                let f = self.data.m17.ok_or("no M17 frame")?;
                let editing = app.view().and_then(|v| v.edit.as_ref()).map(|e| e.frame);
                if editing != Some(f) {
                    return Err(format!("double-click did not start editing ({editing:?})"));
                }
                // Select "x2": Home, six Right, two Shift+Right.
                self.key(Key::Home, Modifiers::NONE);
                for _ in 0..6 {
                    self.key(Key::ArrowRight, Modifiers::NONE);
                }
                for _ in 0..2 {
                    self.key(Key::ArrowRight, Modifiers::SHIFT);
                }
            }
            129 => {
                let sel = app
                    .view()
                    .and_then(|v| v.edit.as_ref())
                    .map(TextEdit::range);
                if sel != Some((6, 8)) {
                    return Err(format!("the selection is {sel:?}, not x2 (6..8)"));
                }
                self.data.hist = Self::done_steps(app);
                self.click_target(ctx, "char-superscript")?;
            }
            130 => {
                let f = self.data.m17.ok_or("no M17 frame")?;
                let (size, up) = Self::m17_glyph(app, f, 7)?;
                let (base_size, base_up) = Self::m17_glyph(app, f, 0)?;
                if size >= 0.7 * base_size || up < 4.0 || base_up.abs() > 0.01 {
                    return Err(format!(
                        "after x\u{b2}: '2' is {size:.1} pt raised {up:.1}, 'W' is {base_size:.1} pt raised {base_up:.1}"
                    ));
                }
                let steps = Self::done_steps(app) - self.data.hist;
                if steps != 1 {
                    return Err(format!("the x\u{b2} button took {steps} undo steps"));
                }
                self.ok(format!(
                    "x\u{b2} button: the selected word is {size:.1} pt (neighbours {base_size:.1} pt) and raised {up:.1} pt, in one undo step"
                ));
                self.screenshot(ctx);
                // The same shortcut toggles it off again.
                self.key(Key::Plus, Modifiers::COMMAND | Modifiers::SHIFT);
            }
            131 => {
                let f = self.data.m17.ok_or("no M17 frame")?;
                let (size, up) = Self::m17_glyph(app, f, 7)?;
                if (size - 24.0).abs() > 0.01 || up.abs() > 0.01 {
                    return Err(format!(
                        "Cmd+Shift+= did not undo the superscript: {size:.1} pt raised {up:.1}"
                    ));
                }
                self.ok("Cmd+Shift+= toggled the superscript off");
                self.key(Key::Equals, Modifiers::COMMAND);
            }
            132 => {
                let f = self.data.m17.ok_or("no M17 frame")?;
                let (size, up) = Self::m17_glyph(app, f, 7)?;
                if size >= 0.7 * 24.0 || up > -2.0 {
                    return Err(format!(
                        "Cmd+= did not lower the word: {size:.1} pt raised {up:.1}"
                    ));
                }
                self.ok(format!(
                    "Cmd+= made the word a subscript ({size:.1} pt, lowered {:.1} pt)",
                    -up
                ));
                self.key(Key::A, Modifiers::COMMAND);
            }
            133 => {
                self.key(Key::C, Modifiers::COMMAND | Modifiers::ALT);
            }
            134 => {
                if !matches!(app.dialog, Some(crate::dialogs::Dialog::Character(_))) {
                    return Err("Cmd+Alt+C did not open the Character dialog".into());
                }
                self.data.hist = Self::done_steps(app);
                self.click_target(ctx, "char-caps-small")?;
            }
            135 => {
                self.click_target(ctx, "char-strike")?;
            }
            136 => {
                self.screenshot(ctx);
            }
            137 => {
                self.key(Key::Enter, Modifiers::NONE);
            }
            138 => {
                if app.dialog.is_some() {
                    return Err("Enter did not close the Character dialog".into());
                }
                let f = self.data.m17.ok_or("no M17 frame")?;
                let lines = Self::m15_lines(app, f)?;
                let line = lines.first().ok_or("the frame lost its line")?;
                // "ater" is lower case: synthesized capitals at 70% (24 pt).
                let small = line
                    .runs
                    .iter()
                    .any(|r| (r.font_size - 24.0 * 0.7).abs() < 0.01);
                if !small || line.decorations.is_empty() {
                    return Err(format!(
                        "small caps + strikethrough: sizes {:?}, {} strike rule(s)",
                        line.runs.iter().map(|r| r.font_size).collect::<Vec<_>>(),
                        line.decorations.len()
                    ));
                }
                let text = Self::spell_text(app, Some(f))?;
                if text != "Water x2 now small struck" {
                    return Err(format!("the dialog changed the text to {text:?}"));
                }
                let steps = Self::done_steps(app) - self.data.hist;
                if steps != 1 {
                    return Err(format!("the dialog took {steps} undo steps"));
                }
                self.ok(format!(
                    "Text \u{25b8} Character\u{2026}: small caps (lowercase drawn at 16.8 pt) and strikethrough ({} rules) applied in one undo step; the text is unchanged",
                    line.decorations.len()
                ));
                self.screenshot(ctx);
            }
            139 => {
                // M25: WordArt, shadows and Border Art. A blank publication
                // with a rectangle low on the page.
                app.new_blank();
                let req: pubrs_api::tools::frames::FrameAddShape =
                    serde_json::from_value(serde_json::json!({
                        "shape": "rectangle", "x": 72.0, "y": 560.0, "width": 220.0, "height": 120.0,
                        "fill": "#BFDBFE",
                    }))
                    .map_err(|e| e.to_string())?;
                let out = app.session.frame_add_shape(req).map_err(|e| e.message)?;
                self.data.m25 = Some((out.frame_id, out.frame_id));
                app.select(Vec::new());
                app.run_action(ctx, Action::InsertWordArt);
            }
            140 => {
                if !matches!(app.dialog, Some(crate::dialogs::Dialog::WordArt(_))) {
                    return Err("Insert ▸ WordArt… did not open the gallery".into());
                }
                // The gallery's previews are rendered by the real renderer.
                self.screenshot(ctx);
            }
            141 => {
                self.click_target(ctx, "wordart-style-1")?;
            }
            142 => {
                self.data.m25_hist = Self::done_steps(app);
                self.click_target(ctx, "ok")?;
            }
            143 => {
                if app.dialog.is_some() {
                    return Err("the WordArt gallery stayed open after Insert".into());
                }
                let doc = app.doc().ok_or("no doc")?;
                let art = doc
                    .frames
                    .values()
                    .find_map(|f| f.as_wordart().map(|w| (f.id, w.clone())))
                    .ok_or("Insert did not add a WordArt frame")?;
                let r = doc.frame(art.0).map(|f| f.rect).ok_or("frame")?;
                if art.1.warp != pubrs_api::core::Warp::ArchUp || art.1.text != "Your text" {
                    return Err(format!(
                        "gallery style 2 gave {:?} / {:?}",
                        art.1.warp, art.1.text
                    ));
                }
                if Self::done_steps(app) != self.data.m25_hist + 1 {
                    return Err("inserting WordArt was not one undo step".into());
                }
                if app.selection() != vec![art.0] {
                    return Err("the new WordArt is not selected".into());
                }
                let (_, rect) = self.data.m25.ok_or("no m25")?;
                self.data.m25 = Some((art.0, rect));
                // The page shows letters where the frame is.
                let mut dark = 0;
                for dx in (0..r.width as u32).step_by(3) {
                    for dy in (0..r.height as u32).step_by(3) {
                        let px = Self::probe_pixel(app, r.x as u32 + dx, r.y as u32 + dy)?;
                        if px[0] != 255 || px[1] != 255 || px[2] != 255 {
                            dark += 1;
                        }
                    }
                }
                if dark < 100 {
                    return Err(format!("the WordArt drew only {dark} sample points"));
                }
                self.ok(format!(
                    "Insert ▸ WordArt…: gallery style 2 (arch) added in one undo step and selected; the page shows {dark} inked samples"
                ));
                self.screenshot(ctx);
            }
            144 => {
                // Change the warp from the properties panel.
                self.click_target(ctx, "wordart-warp")?;
            }
            145 => {
                self.click_target(ctx, "warp:wave")?;
            }
            146 => {
                let (wa, _) = self.data.m25.ok_or("no m25")?;
                let warp = app
                    .doc()
                    .and_then(|d| d.frame(wa))
                    .and_then(|f| f.as_wordart().map(|w| w.warp))
                    .ok_or("no WordArt")?;
                if warp != pubrs_api::core::Warp::Wave {
                    return Err(format!("the Shape menu left the warp at {warp:?}"));
                }
                self.ok("WordArt panel: Shape ▸ Wave changed the warp");
                self.screenshot(ctx);
                // Select the rectangle for the shadow.
                let (_, rect) = self.data.m25.ok_or("no m25")?;
                app.select(vec![rect]);
            }
            147 => {
                let (_, rect) = self.data.m25.ok_or("no m25")?;
                self.data.m25_hist = Self::done_steps(app);
                if app.selection() != vec![rect] {
                    return Err("the rectangle is not selected".into());
                }
                self.click_target(ctx, "shadow-on")?;
            }
            148 => {
                let (_, rect) = self.data.m25.ok_or("no m25")?;
                let st = app
                    .doc()
                    .and_then(|d| d.frame(rect))
                    .map(|f| f.style.clone())
                    .ok_or("no rectangle")?;
                let Some(sh) = st.shadow else {
                    return Err("the Shadow checkbox did not add a shadow".into());
                };
                if Self::done_steps(app) != self.data.m25_hist + 1 {
                    return Err("the shadow was not one undo step".into());
                }
                // The shadow falls right of and below the rectangle: grey where
                // the page was white.
                let px = Self::probe_pixel(app, 72 + 220 + 2, 560 + 60)?;
                if px[0] > 235 || px[0] != px[1] {
                    return Err(format!("no shadow right of the rectangle: {px:?} ({sh:?})"));
                }
                self.ok(format!(
                    "Shadow checkbox: a {:.0} pt shadow, {:.0} pt blur falls beside the rectangle (pixel {px:?})",
                    sh.offset_x, sh.blur
                ));
            }
            149 => {
                self.click_target(ctx, "border-art-combo")?;
            }
            150 => {
                self.click_target(ctx, "border:stars")?;
            }
            151 => {
                let (_, rect) = self.data.m25.ok_or("no m25")?;
                let b = app
                    .doc()
                    .and_then(|d| d.frame(rect))
                    .and_then(|f| f.style.border_art.clone())
                    .ok_or("the Border art menu did not set a border")?;
                if b.pattern != "stars" {
                    return Err(format!("border pattern {:?}", b.pattern));
                }
                // Gold stars sit in the band: some pixel there is orange-ish.
                let mut gold = 0;
                for dx in 0..40u32 {
                    for dy in 0..b.width as u32 {
                        let px = Self::probe_pixel(app, 72 + dx, 560 + dy)?;
                        if px[0] > 200 && px[2] < 120 {
                            gold += 1;
                        }
                    }
                }
                if gold < 10 {
                    return Err(format!("no gold star pixels in the border band ({gold})"));
                }
                self.ok(format!(
                    "Border art menu: stars, {:.0} pt wide ({gold} gold pixels on the top edge)",
                    b.width
                ));
                self.screenshot(ctx);
                // The Border Art… dialog from the Format menu.
                app.run_action(ctx, Action::BorderArt);
            }
            152 => {
                if !matches!(app.dialog, Some(crate::dialogs::Dialog::BorderArt(_))) {
                    return Err("Format ▸ Border Art… did not open its dialog".into());
                }
                self.screenshot(ctx);
            }
            153 => {
                self.click_target(ctx, "border-art-hearts")?;
            }
            154 => {
                self.data.m25_hist = Self::done_steps(app);
                self.key(Key::Enter, Modifiers::NONE);
            }
            155 => {
                if app.dialog.is_some() {
                    return Err("Enter did not close the Border Art dialog".into());
                }
                let (_, rect) = self.data.m25.ok_or("no m25")?;
                let b = app
                    .doc()
                    .and_then(|d| d.frame(rect))
                    .and_then(|f| f.style.border_art.clone())
                    .ok_or("no border")?;
                if b.pattern != "hearts" || Self::done_steps(app) != self.data.m25_hist + 1 {
                    return Err(format!("dialog gave {b:?}"));
                }
                self.ok("Format ▸ Border Art…: picked hearts with the mouse, Enter applied it in one undo step");
                // Export PNG and PDF.
                let png = self.out.join("wordart.png");
                let pdf = self.out.join("wordart.pdf");
                let r = app
                    .session
                    .export_png(pubrs_api::tools::export::ExportPng {
                        path: Some(
                            std::path::absolute(&png)
                                .map_err(|e| e.to_string())?
                                .display()
                                .to_string(),
                        ),
                        dpi: Some(100.0),
                        ..Default::default()
                    })
                    .map_err(|e| e.message)?;
                if r.width == 0 || std::fs::metadata(&png).map_or(0, |m| m.len()) < 1000 {
                    return Err("PNG export wrote nothing".into());
                }
                if !app.export_pdf_to(&pdf) {
                    return Err("PDF export failed".into());
                }
                let bytes = std::fs::read(&pdf).map_err(|e| e.to_string())?;
                let has = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
                if !has(b"/SMask") {
                    return Err("the PDF has no soft-masked shadow image".into());
                }
                self.ok(format!(
                    "exported the page as PNG ({}x{}) and PDF ({} bytes, shadow as a soft-masked image, WordArt as outlines)",
                    r.width,
                    r.height,
                    bytes.len()
                ));
            }
            156 => {
                // M18: wrap. A blank publication with a story frame and an
                // ellipse over its middle.
                app.new_blank();
                let req: pubrs_api::tools::frames::FrameAddText =
                    serde_json::from_value(serde_json::json!({
                        "x": 40.0, "y": 40.0, "width": 420.0, "height": 300.0,
                        "font_size": 12.0,
                        "text": "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium doloremque laudantium.",
                    }))
                    .map_err(|e| e.to_string())?;
                let text = app
                    .session
                    .frame_add_text(req)
                    .map_err(|e| e.message)?
                    .frame_id;
                let req: pubrs_api::tools::frames::FrameAddShape =
                    serde_json::from_value(serde_json::json!({
                        "shape": "ellipse",
                        "x": 170.0, "y": 80.0, "width": 160.0, "height": 120.0,
                    }))
                    .map_err(|e| e.to_string())?;
                let ellipse = app
                    .session
                    .frame_add_shape(req)
                    .map_err(|e| e.message)?
                    .frame_id;
                app.select(vec![ellipse]);
                self.data.m18 = Some((text, ellipse));
            }
            157 => {
                // The panel shows the wrap list for the selected ellipse.
                self.data.hist = Self::done_steps(app);
                self.click_target(ctx, "wrap-combo")?;
            }
            158 => {
                self.click_target(ctx, "wrap-tight")?;
            }
            159 => {
                let (_, e) = self.data.m18.ok_or("no M18 frames")?;
                let w = app.doc().and_then(|d| d.frame(e)).map(|f| f.wrap);
                if w.map(|w| w.mode) != Some(pubrs_api::core::WrapMode::Tight) {
                    return Err(format!("the wrap list did not choose Tight: {w:?}"));
                }
                self.click_target(ctx, "wrap-both-sides")?;
            }
            160 => {
                let (t, e) = self.data.m18.ok_or("no M18 frames")?;
                let w = app
                    .doc()
                    .and_then(|d| d.frame(e))
                    .map(|f| f.wrap)
                    .ok_or("ellipse gone")?;
                if !w.both_sides || w.mode != pubrs_api::core::WrapMode::Tight {
                    return Err(format!("the checkbox did not set both sides: {w:?}"));
                }
                let steps = Self::done_steps(app) - self.data.hist;
                if steps != 2 {
                    return Err(format!("mode and checkbox took {steps} undo steps, not 2"));
                }
                // Ellipse at x 170..330 (text frame x 40): its slots are
                // frame-local x 4..126 on the left and 294..416 on the right.
                let lines = Self::m15_lines(app, t)?;
                let ys: Vec<f64> = lines.iter().map(|l| l.rect.y).collect();
                let row = |y: f64| lines.iter().filter(|l| l.rect.y == y).count();
                let both = ys.iter().any(|y| row(*y) >= 2);
                let left = lines
                    .iter()
                    .any(|l| l.rect.right() < 130.0 && l.rect.y > 60.0);
                let right = lines.iter().any(|l| l.rect.x > 290.0 && l.rect.y > 60.0);
                if !(both && left && right) {
                    return Err(format!(
                        "expected lines on both sides of the ellipse: two-line rows {both}, left {left}, right {right}"
                    ));
                }
                self.ok("Text wrap \u{25b8} Tight and Text on both sides (chosen in the panel): lines sit left and right of the ellipse on the same rows");
                self.screenshot(ctx);
            }
            161 => {
                // M20: tables. Insert > Table… in a blank publication.
                app.new_blank();
                app.select(Vec::new());
                // A swatch for the cell fill (a blank publication has none).
                let id = pubrs_api::core::SwatchId::new(app.session.ids());
                app.exec(Command::AddSwatch {
                    swatch: pubrs_api::core::Swatch {
                        id,
                        name: "Header blue".into(),
                        value: pubrs_api::core::SwatchValue::Rgb(pubrs_api::core::Color::rgb(
                            191, 219, 254,
                        )),
                    },
                });
                self.data.m20_hist = Self::done_steps(app);
                app.run_action(ctx, Action::InsertTable);
            }
            162 => {
                if !matches!(app.dialog, Some(crate::dialogs::Dialog::Table(_))) {
                    return Err("Insert \u{25b8} Table… did not open the dialog".into());
                }
                self.screenshot(ctx);
                // The third row and third column of the hover grid.
                self.click_target(ctx, "table-grid-2-2")?;
            }
            163 => {
                self.click_target(ctx, "ok")?;
            }
            164 => {
                if app.dialog.is_some() {
                    return Err("the Table dialog stayed open after Insert".into());
                }
                let steps_now = Self::done_steps(app);
                let doc = app.doc().ok_or("no doc")?;
                let (id, t) = doc
                    .frames
                    .values()
                    .find_map(|f| f.as_table().map(|t| (f.id, t.clone())))
                    .ok_or("Insert did not add a table")?;
                if t.rows.len() != 3 || t.cols.len() != 3 {
                    return Err(format!("the grid gave {}x{}", t.rows.len(), t.cols.len()));
                }
                if steps_now != self.data.m20_hist + 1 {
                    return Err("inserting the table was not one undo step".into());
                }
                let first = t.cells[0].story;
                let editing = app.view().and_then(|v| v.edit.as_ref()).map(|e| e.story);
                if editing != Some(first) || app.selection() != [id] {
                    return Err("the new table is not selected with its first cell in edit".into());
                }
                self.data.m20 = Some(id);
                self.ok("Insert \u{25b8} Table…: 3x3 from the hover grid, one undo step, first cell ready for typing");
                // Type into every cell with Tab between them, one character
                // at a time (typing coalesces per cell).
                for k in 0..9 {
                    for ch in format!("cell{k}").chars() {
                        self.pending.push_back(vec![Event::Text(ch.to_string())]);
                    }
                    if k < 8 {
                        self.key(Key::Tab, Modifiers::NONE);
                    }
                }
            }
            165 => {
                let id = self.data.m20.ok_or("no table")?;
                let steps = Self::done_steps(app) - self.data.m20_hist;
                let doc = app.doc().ok_or("no doc")?;
                let t = doc
                    .frame(id)
                    .and_then(|f| f.as_table())
                    .ok_or("table gone")?;
                for (i, c) in t.cells.iter().enumerate() {
                    let text = doc
                        .story(c.story)
                        .map(|s| s.plain_text())
                        .unwrap_or_default();
                    if text != format!("cell{i}") {
                        return Err(format!("cell {i} holds {text:?}, not \"cell{i}\""));
                    }
                }
                if steps != 10 {
                    return Err(format!("table + 9 cells took {steps} undo steps, not 10"));
                }
                self.ok("typed into all nine cells with Tab (no tab characters), one undo step per cell");
                // Five typed characters undo as one step.
                app.run_action(ctx, Action::Undo);
            }
            166 => {
                let id = self.data.m20.ok_or("no table")?;
                let doc = app.doc().ok_or("no doc")?;
                let t = doc
                    .frame(id)
                    .and_then(|f| f.as_table())
                    .ok_or("table gone")?;
                let last = t.cells.last().ok_or("no cells")?.story;
                let text = doc.story(last).map(|s| s.plain_text()).unwrap_or_default();
                if !text.is_empty() {
                    return Err(format!("undo left {text:?} in the last cell"));
                }
                self.ok("undo removed the five characters typed in the last cell in one step");
                app.run_action(ctx, Action::Redo);
            }
            167 => {
                // Tab in the last cell adds a row.
                let id = self.data.m20.ok_or("no table")?;
                let doc = app.doc().ok_or("no doc")?;
                let t = doc
                    .frame(id)
                    .and_then(|f| f.as_table())
                    .ok_or("table gone")?;
                let last = t.cells.last().ok_or("no cells")?.story;
                if doc.story(last).map(|s| s.plain_text()).as_deref() != Some("cell8") {
                    return Err("redo did not bring the text back".into());
                }
                self.data.m20_hist = Self::done_steps(app);
                crate::canvas::begin_edit(app, id, last, 5);
                self.key(Key::Tab, Modifiers::NONE);
            }
            168 => {
                let id = self.data.m20.ok_or("no table")?;
                let steps_now = Self::done_steps(app);
                let doc = app.doc().ok_or("no doc")?;
                let t = doc
                    .frame(id)
                    .and_then(|f| f.as_table())
                    .ok_or("table gone")?;
                if t.rows.len() != 4 {
                    return Err(format!("Tab in the last cell gave {} rows", t.rows.len()));
                }
                if steps_now != self.data.m20_hist + 1 {
                    return Err("Tab in the last cell took more than one undo step".into());
                }
                let new_first = t.cell_at(3, 0).ok_or("no new cell")?.story;
                let editing = app.view().and_then(|v| v.edit.as_ref()).map(|e| e.story);
                if editing != Some(new_first) {
                    return Err("the new row's first cell is not being edited".into());
                }
                self.ok("Tab in the last cell added a row (4 rows) and edits its first cell");
                app.run_action(ctx, Action::Undo);
            }
            169 => {
                let id = self.data.m20.ok_or("no table")?;
                let rows = app
                    .doc()
                    .and_then(|d| d.frame(id))
                    .and_then(|f| f.as_table())
                    .map(|t| t.rows.len());
                if rows != Some(3) || Self::done_steps(app) != self.data.m20_hist {
                    return Err(format!("undo of the new row left {rows:?} rows"));
                }
                self.ok("one undo removed the row again (3 rows)");
                app.select(vec![id]);
            }
            170 => {
                // Drag across the header row.
                let id = self.data.m20.ok_or("no table")?;
                let a = Self::cell_center(app, id, 0, 0)?;
                let b = Self::cell_center(app, id, 0, 2)?;
                self.drag(a, b);
            }
            171 => {
                let id = self.data.m20.ok_or("no table")?;
                let sel = app.view().and_then(|v| v.table_sel);
                match sel {
                    Some(s) if s.frame == id && (s.r0, s.c0, s.r1, s.c1) == (0, 0, 0, 2) => {}
                    other => return Err(format!("dragging across the header gave {other:?}")),
                }
                self.ok("dragging across the header row selected cells (0,0) to (0,2)");
                let pos = Self::cell_center(app, id, 0, 1)?;
                self.right_click(pos);
            }
            172 => {
                if app.table_menu.is_none() {
                    return Err("right-click in the table did not open its menu".into());
                }
                // The menu is on screen for the screenshot.
                self.screenshot(ctx);
            }
            173 => {
                self.data.m20_hist = Self::done_steps(app);
                self.click_target(ctx, "table-merge")?;
            }
            174 => {
                let id = self.data.m20.ok_or("no table")?;
                let steps_now = Self::done_steps(app);
                let y = Self::cell_center(app, id, 2, 0)?;
                let doc = app.doc().ok_or("no doc")?;
                let t = doc
                    .frame(id)
                    .and_then(|f| f.as_table())
                    .ok_or("table gone")?;
                let c = t.cell_at(0, 0).ok_or("no cell")?;
                if (c.row_span, c.col_span) != (1, 3) {
                    return Err(format!("merge gave a {}x{} span", c.row_span, c.col_span));
                }
                if steps_now != self.data.m20_hist + 1 {
                    return Err("merging took more than one undo step".into());
                }
                let text = doc
                    .story(c.story)
                    .map(|s| s.plain_text())
                    .unwrap_or_default();
                if text != "cell0\ncell1\ncell2" {
                    return Err(format!("the merged header holds {text:?}"));
                }
                self.ok("context menu \u{25b8} Merge Cells: the header is one 1x3 cell in one undo step, text kept as paragraphs");
                // Drag the border between columns 1 and 2 below the header.
                self.data.m20_w = t.cols[0];
                self.data.m20_hist = steps_now;
                let f = doc.frame(id).ok_or("frame")?.rect;
                let x =
                    Self::screen_of(app, Point::new(f.x + t.cols[0], f.y)).ok_or("no canvas")?;
                self.drag(Pos2::new(x.x, y.y), Pos2::new(x.x + 30.0, y.y));
            }
            175 => {
                let id = self.data.m20.ok_or("no table")?;
                let steps_now = Self::done_steps(app);
                let cell10 = Self::cell_center(app, id, 1, 0)?;
                let doc = app.doc().ok_or("no doc")?;
                let t = doc
                    .frame(id)
                    .and_then(|f| f.as_table())
                    .ok_or("table gone")?;
                let scale = app
                    .view()
                    .and_then(|v| v.xf)
                    .map_or(1.0, |xf| f64::from(xf.scale));
                let grown = t.cols[0] - self.data.m20_w;
                if (grown - 30.0 / scale).abs() > 1.5 {
                    return Err(format!(
                        "dragging the border 30 px changed the column by {grown:.2} pt (expected {:.2})",
                        30.0 / scale
                    ));
                }
                if steps_now != self.data.m20_hist + 1 {
                    return Err("the border drag was not one undo step".into());
                }
                let w = doc.frame(id).map_or(0.0, |f| f.rect.width);
                if (w - t.width()).abs() > 1e-6 {
                    return Err("the frame width does not follow the columns".into());
                }
                self.ok(format!(
                    "dragging a column border widened column 1 by {grown:.1} pt in one undo step"
                ));
                // Click a cell of the selected table to edit it, then set its fill.
                self.data.m20_hist = steps_now;
                self.click(cell10, false);
            }
            176 => {
                let id = self.data.m20.ok_or("no table")?;
                if app.view().and_then(|v| v.edit.as_ref()).map(|e| e.frame) != Some(id) {
                    return Err("clicking a cell of the selected table did not edit it".into());
                }
                self.click_target(ctx, &format!("paint-cell-fill-{id}"))?;
            }
            177 => {
                let name = app
                    .doc()
                    .and_then(|d| d.swatches.iter().next().map(|s| s.name.clone()))
                    .ok_or("no swatch")?;
                self.click_target(ctx, &format!("paint-swatch-{name}"))?;
            }
            178 => {
                let id = self.data.m20.ok_or("no table")?;
                let doc = app.doc().ok_or("no doc")?;
                let t = doc
                    .frame(id)
                    .and_then(|f| f.as_table())
                    .ok_or("table gone")?;
                let c = t.cell_at(1, 0).ok_or("no cell")?;
                if c.fill.is_none() || t.cell_at(1, 1).is_some_and(|o| o.fill.is_some()) {
                    return Err(format!("the panel fill gave {:?}", c.fill));
                }
                if Self::done_steps(app) != self.data.m20_hist + 1 {
                    return Err("the fill took more than one undo step".into());
                }
                self.ok(
                    "Cell section of the panel: fill set on the edited cell only, one undo step",
                );
                self.screenshot(ctx);
            }
            179 => {
                // The panel's border control: None on the edited cell.
                self.click_target(ctx, "cell-border-none")?;
            }
            180 => {
                let id = self.data.m20.ok_or("no table")?;
                let steps_now = Self::done_steps(app);
                let cell10 = Self::cell_center(app, id, 1, 0)?;
                let doc = app.doc().ok_or("no doc")?;
                let t = doc
                    .frame(id)
                    .and_then(|f| f.as_table())
                    .ok_or("table gone")?;
                let c = t.cell_at(1, 0).ok_or("no cell")?;
                let o = t.cell_at(1, 1).ok_or("no cell")?;
                if c.borders.iter().any(Option::is_some) || o.borders.iter().all(Option::is_none) {
                    return Err(format!(
                        "None gave {:?} / neighbour {:?}",
                        c.borders, o.borders
                    ));
                }
                if steps_now != self.data.m20_hist + 2 {
                    return Err("fill then no borders should be two undo steps".into());
                }
                self.ok(
                    "Cell section: Borders > None removed the four borders of the edited cell only",
                );
                // Back to the canvas so the next keys reach the cell.
                self.click(cell10, false);
            }
            181 => {
                // Select all in the cell and type text that wraps: the
                // row grows, and the frame with it, in ONE undo step.
                let id = self.data.m20.ok_or("no table")?;
                let h = app
                    .doc()
                    .and_then(|d| d.frame(id))
                    .map_or(0.0, |f| f.rect.height);
                self.data.m20_h = h;
                self.data.m20_hist = Self::done_steps(app);
                self.key(Key::A, Modifiers::COMMAND);
                self.pending.push_back(vec![Event::Text(
                    "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi"
                        .to_string(),
                )]);
            }
            182 => {
                let id = self.data.m20.ok_or("no table")?;
                let steps_now = Self::done_steps(app);
                let doc = app.doc().ok_or("no doc")?;
                let t = doc
                    .frame(id)
                    .and_then(|f| f.as_table())
                    .ok_or("table gone")?;
                let c = t.cell_at(1, 0).ok_or("no cell")?;
                let text = doc
                    .story(c.story)
                    .map(|s| s.plain_text())
                    .unwrap_or_default();
                if !text.starts_with("alpha beta") {
                    return Err(format!("the replacement text is {text:?}"));
                }
                let h = doc.frame(id).map_or(0.0, |f| f.rect.height);
                if h <= self.data.m20_h + 5.0 {
                    return Err(format!(
                        "the frame stayed {h:.1} pt tall (was {:.1})",
                        self.data.m20_h
                    ));
                }
                if steps_now != self.data.m20_hist + 1 {
                    return Err(format!(
                        "replacing text and growing the frame took {} undo steps",
                        steps_now - self.data.m20_hist
                    ));
                }
                self.ok(format!(
                    "select all + typing that wraps grew the frame {:.1} -> {h:.1} pt, in one undo step",
                    self.data.m20_h
                ));
                app.run_action(ctx, Action::Undo);
            }
            183 => {
                let id = self.data.m20.ok_or("no table")?;
                let steps_now = Self::done_steps(app);
                let doc = app.doc().ok_or("no doc")?;
                let t = doc
                    .frame(id)
                    .and_then(|f| f.as_table())
                    .ok_or("table gone")?;
                let c = t.cell_at(1, 0).ok_or("no cell")?;
                let text = doc
                    .story(c.story)
                    .map(|s| s.plain_text())
                    .unwrap_or_default();
                let h = doc.frame(id).map_or(0.0, |f| f.rect.height);
                if text != "cell3"
                    || (h - self.data.m20_h).abs() > 1e-6
                    || steps_now != self.data.m20_hist
                {
                    return Err(format!(
                        "one undo left text {text:?}, height {h:.2} (was {:.2}), {} steps",
                        self.data.m20_h,
                        steps_now - self.data.m20_hist
                    ));
                }
                self.ok("one undo restored both the text and the frame height");
            }
            184 => {
                let pdf = self.out.join("tables.pdf");
                if !app.export_pdf_to(&pdf) {
                    return Err("PDF export failed".into());
                }
                let bytes = std::fs::read(&pdf).map_err(|e| e.to_string())?;
                let has = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
                // Page objects: "/Type/Page" or "/Type /Page" not followed by 's' (Pages).
                let pages = bytes
                    .windows(12)
                    .filter(|w| {
                        let tail = if w.starts_with(b"/Type/Page") {
                            &w[10..]
                        } else if w.starts_with(b"/Type /Page") {
                            &w[11..]
                        } else {
                            return false;
                        };
                        tail.first() != Some(&b's')
                    })
                    .count();
                if bytes.len() < 1000
                    || !bytes.starts_with(b"%PDF-")
                    || !has(b"%%EOF")
                    || !has(b"startxref")
                    || pages != 1
                {
                    return Err(format!(
                        "the table PDF looks wrong ({} bytes, {pages} page objects)",
                        bytes.len()
                    ));
                }
                self.ok(format!(
                    "exported the table page as PDF ({} bytes: header, one page object, xref and EOF present; the cell text is in compressed subset-font streams, so it is not checked here)",
                    bytes.len()
                ));
            }
            185 => {
                // M27: copy frames (a text frame and a picture) in one
                // publication, then open another.
                app.new_blank();
                let a = app.doc_id().ok_or("no document")?;
                let req: pubrs_api::tools::frames::FrameAddText =
                    serde_json::from_value(serde_json::json!({
                        "x": 40.0, "y": 40.0, "width": 240.0, "height": 60.0,
                        "text": "Copy me",
                    }))
                    .map_err(|e| e.to_string())?;
                let text = app
                    .session
                    .frame_add_text(req)
                    .map_err(|e| e.message)?
                    .frame_id;
                let path = self.out.join("m27-picture.png");
                std::fs::write(&path, png(48, 32, 9)).map_err(|e| e.to_string())?;
                // The session resolves relative paths against its documents folder.
                let path = path.canonicalize().map_err(|e| e.to_string())?;
                let req: pubrs_api::tools::frames::FrameAddImage =
                    serde_json::from_value(serde_json::json!({
                        "x": 40.0, "y": 140.0, "width": 96.0, "height": 64.0,
                        "path": path.display().to_string(),
                    }))
                    .map_err(|e| e.to_string())?;
                let img = app
                    .session
                    .frame_add_image(req)
                    .map_err(|e| e.message)?
                    .frame_id;
                app.select(vec![text, img]);
                app.run_action(ctx, Action::Copy);
                app.new_blank();
                let b = app.doc_id().ok_or("no second document")?;
                self.data.m27_docs = Some((a, b));
            }
            186 => {
                // Edit > Paste into the other publication: both frames
                // arrive, with their text and their picture's bytes, as one
                // undo step; a second paste reuses the picture.
                let (a, b) = self.data.m27_docs.ok_or("no M27 documents")?;
                if app.doc_id() != Some(b) {
                    return Err("the second publication is not active".into());
                }
                let source_bytes = {
                    let d = app.session.get(Some(a)).map_err(|e| e.message)?.doc();
                    d.blobs
                        .values()
                        .next()
                        .cloned()
                        .ok_or("source has no picture")?
                };
                app.run_action(ctx, Action::Paste);
                let (n_frames, text, asset_ok) = {
                    let d = app.doc().ok_or("no document")?;
                    let page = d.pages[0].id;
                    let ids = d
                        .children(pubrs_api::core::Parent::Page(page))
                        .cloned()
                        .unwrap_or_default();
                    let text = ids
                        .iter()
                        .filter_map(|f| d.frame(*f).and_then(|f| f.as_text()))
                        .filter_map(|t| d.story(t.story))
                        .map(|s| s.plain_text())
                        .collect::<Vec<_>>();
                    let asset_ok = ids
                        .iter()
                        .filter_map(|f| {
                            d.frame(*f).and_then(|f| f.as_image()).and_then(|i| i.asset)
                        })
                        .all(|a| d.blobs.get(&a).is_some_and(|b| b[..] == source_bytes[..]))
                        && d.frames
                            .values()
                            .any(|f| f.as_image().is_some_and(|i| i.asset.is_some()));
                    (ids.len(), text, asset_ok)
                };
                if n_frames != 2 || text != ["Copy me"] || !asset_ok {
                    return Err(format!(
                        "paste into the second publication gave {n_frames} frame(s), text {text:?}, picture bytes ok {asset_ok}"
                    ));
                }
                if Self::done_steps(app) != 1 {
                    return Err(format!(
                        "paste took {} undo steps, not 1",
                        Self::done_steps(app)
                    ));
                }
                app.run_action(ctx, Action::Paste);
                let assets = app.doc().map_or(0, |d| d.assets.len());
                let frames = app.doc().map_or(0, |d| d.frames.len());
                if assets != 1 || frames != 4 {
                    return Err(format!(
                        "second paste: {assets} asset(s) (want 1, reused), {frames} frames (want 4)"
                    ));
                }
                app.run_action(ctx, Action::Undo);
                app.run_action(ctx, Action::Undo);
                let left = app.doc().map_or(9, |d| d.frames.len() + d.assets.len());
                let source = app
                    .session
                    .get(Some(a))
                    .map_err(|e| e.message)?
                    .doc()
                    .frames
                    .len();
                if left != 0 || source != 2 {
                    return Err(format!(
                        "after two undos {left} frame(s)/asset(s) remain; the source has {source} frames"
                    ));
                }
                self.ok("Edit \u{25b8} Paste put copied frames into another open publication with their picture (reused the second time), in one undo step each");
            }
            187 => {
                // A picture on the OS clipboard becomes a picture frame; one
                // above 8,000 px a side is scaled down, with a warning.
                app.platform
                    .clipboard
                    .set_image(crate::platform::ClipImage {
                        width: 9000,
                        height: 40,
                        rgba: vec![200; 9000 * 40 * 4],
                    });
                app.run_action(ctx, Action::Paste);
                let big = app.doc().and_then(|d| {
                    d.assets
                        .values()
                        .next()
                        .map(|a| (a.pixel_width, a.pixel_height))
                });
                let status = app.status.as_ref().map(|s| s.0.clone()).unwrap_or_default();
                if big != Some((8000, 36)) || !status.contains("scaled down") {
                    return Err(format!(
                        "9000x40 picture pasted as {big:?}; status {status:?}"
                    ));
                }
                app.platform
                    .clipboard
                    .set_image(crate::platform::ClipImage {
                        width: 64,
                        height: 32,
                        rgba: vec![90; 64 * 32 * 4],
                    });
                app.run_action(ctx, Action::Paste);
                let small = app.doc().is_some_and(|d| {
                    d.frames.values().any(|f| {
                        f.as_image()
                            .and_then(|i| i.asset)
                            .and_then(|a| d.assets.get(&a))
                            .is_some_and(|a| (a.pixel_width, a.pixel_height) == (64, 32))
                            && (f.rect.width - 48.0).abs() < 0.01
                    })
                });
                if !small || app.selection().len() != 1 {
                    return Err(
                        "a 64x32 picture did not paste as a 48 x 24 pt picture frame".into(),
                    );
                }
                self.ok("a picture on the clipboard pasted as a picture frame; 9000 px wide was scaled down to 8000 with a status warning");
            }
            188 => {
                // Input method: composition text, then its commit, while
                // editing text.
                let req: pubrs_api::tools::frames::FrameAddText =
                    serde_json::from_value(serde_json::json!({
                        "x": 40.0, "y": 300.0, "width": 240.0, "height": 40.0,
                        "text": "ab",
                    }))
                    .map_err(|e| e.to_string())?;
                let out = app.session.frame_add_text(req).map_err(|e| e.message)?;
                let story = out.story_id.ok_or("no story")?;
                crate::canvas::begin_edit(app, out.frame_id, story, 2);
                self.data.m27_frames = Some((out.frame_id, out.frame_id));
                self.data.hist = Self::done_steps(app);
                self.pending
                    .push_back(vec![Event::Ime(egui::ImeEvent::Preedit {
                        text: "\u{306b}\u{307b}\u{3093}".to_string(),
                        active_range_chars: Some(0..3),
                    })]);
            }
            189 => {
                // The composition is shown, not inserted: no new undo step.
                let (f, _) = self.data.m27_frames.ok_or("no M27 frame")?;
                let text = crate::canvas::story_text(app, f).unwrap_or_default();
                let pre = app
                    .view()
                    .and_then(|v| v.edit.as_ref())
                    .and_then(|e| e.preedit.clone());
                if text != "ab"
                    || pre.map(|p| p.text) != Some("\u{306b}\u{307b}\u{3093}".to_string())
                {
                    return Err(format!(
                        "preedit changed the story ({text:?}) or was not kept"
                    ));
                }
                if Self::done_steps(app) != self.data.hist {
                    return Err("composition text made an undo step".into());
                }
                self.screenshot(ctx);
                self.pending
                    .push_back(vec![Event::Ime(egui::ImeEvent::Commit(
                        "\u{65e5}\u{672c}".to_string(),
                    ))]);
            }
            190 => {
                let (f, _) = self.data.m27_frames.ok_or("no M27 frame")?;
                let text = crate::canvas::story_text(app, f).unwrap_or_default();
                let edit = app
                    .view()
                    .and_then(|v| v.edit.clone())
                    .ok_or("editing ended")?;
                if text != "ab\u{65e5}\u{672c}" || edit.preedit.is_some() || edit.caret != 4 {
                    return Err(format!(
                        "after the commit: text {text:?}, caret {}, preedit {:?}",
                        edit.caret, edit.preedit
                    ));
                }
                if Self::done_steps(app) != self.data.hist + 1 {
                    return Err(format!(
                        "the commit took {} undo steps, not 1",
                        Self::done_steps(app) - self.data.hist
                    ));
                }
                app.run_action(ctx, Action::Undo);
                let text = crate::canvas::story_text(app, f).unwrap_or_default();
                if text != "ab" {
                    return Err(format!("one undo left {text:?}, not \"ab\""));
                }
                self.ok("input method: preedit drawn but not inserted (no undo step), commit inserted \u{65e5}\u{672c} as one undo step");
            }
            191 => {
                // Right-to-left text: arrows move by position on the line.
                let req: pubrs_api::tools::frames::FrameAddText =
                    serde_json::from_value(serde_json::json!({
                        "x": 40.0, "y": 380.0, "width": 240.0, "height": 40.0,
                        "text": "\u{5e9}\u{5dc}\u{5d5}\u{5dd}",
                    }))
                    .map_err(|e| e.to_string())?;
                let out = app.session.frame_add_text(req).map_err(|e| e.message)?;
                let story = out.story_id.ok_or("no story")?;
                crate::canvas::begin_edit(app, out.frame_id, story, 4);
                self.data.m27_frames = Some((out.frame_id, out.frame_id));
                self.key(Key::ArrowRight, Modifiers::NONE);
            }
            192 => {
                // The caret was at the logical end (left edge of the line):
                // Right moves it right, which is one letter back.
                let caret = app.view().and_then(|v| v.edit.as_ref()).map(|e| e.caret);
                if caret != Some(3) {
                    return Err(format!(
                        "Right in a Hebrew word put the caret at {caret:?}, not 3"
                    ));
                }
                self.key(Key::Home, Modifiers::NONE);
            }
            193 => {
                let caret = app.view().and_then(|v| v.edit.as_ref()).map(|e| e.caret);
                if caret != Some(0) {
                    return Err(format!(
                        "Home in a Hebrew line put the caret at {caret:?}, not 0 (its right end)"
                    ));
                }
                self.key(Key::ArrowLeft, Modifiers::NONE);
            }
            194 => {
                let caret = app.view().and_then(|v| v.edit.as_ref()).map(|e| e.caret);
                if caret != Some(1) {
                    return Err(format!(
                        "Left from the right end of a Hebrew line put the caret at {caret:?}, not 1"
                    ));
                }
                self.ok("right-to-left line: Right/Left/Home follow position on the line, not the order of the text");
                self.screenshot(ctx);
                return Ok(true);
            }
            _ => return Ok(true),
        }
        Ok(false)
    }
}

/// A file "dropped" on the window by the self-test.
#[derive(Debug)]
struct DroppedPath(PathBuf);

impl egui::DroppedFile for DroppedPath {
    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

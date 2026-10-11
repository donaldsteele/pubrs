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
//! thread, and the UI thread's own work per frame stays small.
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
use pubrs_api::tools::text::TextRef;

use crate::app::PubrsApp;
use crate::keymap::Action;

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
/// machine; the bound leaves room for slow CI runners.
const MAX_UI_OVERHEAD_MS: f64 = 50.0;

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
        if self.started.elapsed().as_secs() > 300 {
            return Some(Err(format!(
                "timed out at step {}\n{}",
                self.step,
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
                // view lands when zooming in; then zoom with Cmd+Plus.
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
                    self.key(Key::Plus, Modifiers::COMMAND);
                }
            }
            17 => {
                let zoom = app.view().map_or(0.0, |v| v.effective_zoom);
                if (zoom - 8.0).abs() > 1e-3 {
                    return Err(format!(
                        "Cmd+Plus reached {:.0}% zoom, not 800%",
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
                let setup = app.doc().ok_or("no doc")?.setup;
                let page_w = ((setup.size.width + 2.0 * setup.bleed) * want / 72.0).ceil();
                self.ok(format!(
                    "zoomed to 800% with Cmd+Plus: the canvas texture covers {}x{} device pixels of the {page_w:.0} px wide page, one texel per pixel (no 4,096 px cap)",
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
                    "perf while typing {KEYSTROKES} keys into 50 pages: {} frames, ui {:.2} ms avg / {:.2} ms max (without commands and layout {:.2} ms max; commands {:.1} ms, layout {:.1} ms), ui-thread rasters {}, worker renders {} ({:.1} ms), uploaded {} kpx",
                    t.frames,
                    ms(t.ui) / f64::from(t.frames.max(1)),
                    ms(t.ui_max),
                    ms(t.overhead_max),
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
                if ms(t.overhead_max) > MAX_UI_OVERHEAD_MS {
                    return Err(format!(
                        "a frame spent {:.1} ms of UI-thread time besides commands and layout (limit {MAX_UI_OVERHEAD_MS} ms)\n{line}",
                        ms(t.overhead_max)
                    ));
                }
                self.ok(format!(
                    "typed {KEYSTROKES} keys into a 50-page publication: no page rasterised on the UI thread, worst frame {:.1} ms besides commands and layout (limit {MAX_UI_OVERHEAD_MS} ms)",
                    ms(t.overhead_max)
                ));
                self.ok(line);
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

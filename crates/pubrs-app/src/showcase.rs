//! `pubrs-app --showcase DIR`: draws the curated screenshots and renders the
//! project website uses (`scripts/site-screenshots.sh`), then exits 0.
//!
//! Each scene starts from a clean state (every document closed), builds its
//! publication from a script in `crates/pubrs-app/showcase/` (the same tool
//! calls an MCP client makes), shows it through the real GUI (menus' own
//! actions, dialogs, canvas clicks) and saves one named PNG. The window is
//! [`WINDOW_PX`] physical pixels drawn at [`ZOOM`] points per logical point,
//! so a screenshot is 2160 x 1350 with crisp text. The sample pictures are
//! the SVG files in `site/assets/showcase-content/`.
//!
//! A failing scene stops the run: the error names the scene.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Instant;

use eframe::egui::{self, Event};
use pubrs_api::core::{FrameId, WarningCode};
use pubrs_api::render::RasterOptions;
use pubrs_api::script::{Step, substitute};
use pubrs_api::{ScriptFile, call_tool};
use serde_json::{Value, json};

use crate::app::PubrsApp;
use crate::dialogs::{Dialog, MergeWizardState, target_rect};
use crate::keymap::Action;
use crate::selftest::{click_events, encode_screenshot, rendered, screenshots_in};

/// Egui points per logical point: the window is 1440 x 900 logical points.
pub const ZOOM: f32 = 1.5;
/// Window size in physical pixels (1440 x 900 logical at [`ZOOM`]).
pub const WINDOW_PX: [f32; 2] = [2160.0, 1350.0];

/// Frames a step waits at most for the render worker.
const SETTLE_FRAMES: u32 = 900;
/// Frames a scene waits at most for something to happen.
const MAX_RETRIES: u32 = 600;
/// Seconds the whole run may take (a guard against a hang).
const TOTAL_TIMEOUT_SECS: u64 = 900;

/// Where the sample pictures, the CSV and the Publisher file live.
const CONTENT_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../site/assets/showcase-content"
);
const CSV: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/showcase/labels.csv");
const PUB_FILE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/corpus/pub/poi/SampleNewsletter.pub"
);

const NEWSLETTER: &str = include_str!("../showcase/newsletter.json");
const CERTIFICATE: &str = include_str!("../showcase/certificate.json");
const MENU: &str = include_str!("../showcase/menu.json");
const POSTCARD: &str = include_str!("../showcase/postcard.json");
const CALENDAR: &str = include_str!("../showcase/calendar.json");
const LABELS: &str = include_str!("../showcase/address_labels.json");
const FLYER: &str = include_str!("../showcase/design_checker.json");

/// What a scene step asks the driver to do next.
enum Next {
    /// Run the next step of the scene.
    Go,
    /// Run this step again on a later frame (waiting for a worker).
    Again,
    /// Take the screenshot and end the scene.
    ShotDone(&'static str),
    /// End the scene.
    Done,
}

type SceneFn = fn(&mut Showcase, &mut PubrsApp, &egui::Context, usize) -> Result<Next, String>;

/// The scenes, in order. The PNG names are a contract with the website.
const SCENES: &[(&str, SceneFn)] = &[
    ("newsletter", scene_newsletter),
    ("templates", scene_templates),
    ("tables", scene_tables),
    ("mail-merge", scene_mail_merge),
    ("merged-labels", scene_merged_labels),
    ("design-checker", scene_design_checker),
    ("wordart", scene_wordart),
    ("colour", scene_colour),
    ("pub-import", scene_pub_import),
    ("export", scene_export),
    ("renders", scene_renders),
];

pub struct Showcase {
    out: PathBuf,
    scene: usize,
    phase: usize,
    wait: u32,
    pending: VecDeque<Vec<Event>>,
    clock: f64,
    settling: u32,
    retries: u32,
    /// Screenshot to take once the UI has settled.
    want_shot: Option<&'static str>,
    /// Screenshot asked for: file name and frames waited.
    awaiting: Option<(&'static str, u32)>,
    /// End the scene when the awaited screenshot is saved.
    shot_ends_scene: bool,
    started: Instant,
    /// What the built publication's script returned, by variable name.
    vars: BTreeMap<String, Value>,
    report: Vec<String>,
    /// The scene ended; only the input clock still runs.
    pub finished: bool,
}

impl Showcase {
    pub fn new(out: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&out);
        Self {
            out,
            scene: 0,
            phase: 0,
            wait: 5,
            pending: VecDeque::new(),
            clock: 0.0,
            settling: 0,
            retries: 0,
            want_shot: None,
            awaiting: None,
            shot_ends_scene: false,
            started: Instant::now(),
            vars: BTreeMap::new(),
            report: Vec::new(),
            finished: false,
        }
    }

    /// Feed queued synthetic events into egui, on a steady 60 Hz clock so
    /// double-clicks do not depend on how fast this machine draws.
    pub fn inject(&mut self, raw: &mut egui::RawInput) {
        self.clock += 1.0 / 60.0;
        raw.time = Some(self.clock);
        raw.predicted_dt = 1.0 / 60.0;
        if let Some(events) = self.pending.pop_front() {
            raw.events.extend(events);
        }
    }

    /// Advance the run; `Some(result)` when it is over.
    pub fn step(
        &mut self,
        app: &mut PubrsApp,
        ctx: &egui::Context,
    ) -> Option<Result<String, String>> {
        if self.started.elapsed().as_secs() > TOTAL_TIMEOUT_SECS {
            return Some(Err(format!(
                "timed out in scene {}",
                SCENES.get(self.scene).map_or("?", |s| s.0)
            )));
        }
        if let Some((name, waited)) = self.awaiting {
            if let Some(img) = screenshots_in(ctx).into_iter().next() {
                if let Err(e) = self.save(name, &img) {
                    return Some(Err(e));
                }
                self.awaiting = None;
                self.wait = 3;
                if std::mem::take(&mut self.shot_ends_scene) {
                    self.end_scene();
                }
            } else if waited > 300 {
                return Some(Err(format!("{name}: the screenshot never arrived")));
            } else {
                self.awaiting = Some((name, waited + 1));
            }
            return None;
        }
        if !self.pending.is_empty() || self.wait > 0 {
            self.wait = self.wait.saturating_sub(1);
            return None;
        }
        if !rendered(app) && self.settling < SETTLE_FRAMES {
            self.settling += 1;
            return None;
        }
        self.settling = 0;
        if let Some(name) = self.want_shot.take() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            self.awaiting = Some((name, 0));
            return None;
        }
        let Some((scene, run)) = SCENES.get(self.scene).copied() else {
            return Some(Ok(self.report.join("\n")));
        };
        match run(self, app, ctx, self.phase) {
            Ok(Next::Go) => {
                self.phase += 1;
                self.retries = 0;
                self.wait = 4;
            }
            Ok(Next::Again) => {
                self.retries += 1;
                self.wait = 2;
                if self.retries > MAX_RETRIES {
                    return Some(Err(format!(
                        "{scene}: timed out waiting (phase {})",
                        self.phase
                    )));
                }
            }
            Ok(Next::ShotDone(name)) => {
                self.want_shot = Some(name);
                self.shot_ends_scene = true;
                self.wait = 6;
            }
            Ok(Next::Done) => self.end_scene(),
            Err(e) => return Some(Err(format!("scene {scene}: {e}"))),
        }
        None
    }

    fn end_scene(&mut self) {
        self.scene += 1;
        self.phase = 0;
        self.retries = 0;
        self.wait = 4;
    }

    fn save(&mut self, name: &str, img: &egui::ColorImage) -> Result<(), String> {
        let bytes = encode_screenshot(img).ok_or_else(|| format!("{name}: cannot encode"))?;
        self.write(name, &bytes, img.size[0], img.size[1])
    }

    fn write(&mut self, name: &str, bytes: &[u8], w: usize, h: usize) -> Result<(), String> {
        std::fs::write(self.out.join(name), bytes).map_err(|e| format!("{name}: {e}"))?;
        self.report
            .push(format!("{name} {w}x{h} {} bytes", bytes.len()));
        Ok(())
    }

    // ------------------------------------------------------------ helpers

    /// A clean slate: no dialogs, no panels, no documents.
    fn reset(&mut self, app: &mut PubrsApp) {
        app.dialog = None;
        app.checker.open = false;
        app.swatches_open = Some(false);
        app.table_menu = None;
        app.spell.menu = None;
        self.pending.clear();
        self.vars.clear();
        for _ in 0..256 {
            if app.views.is_empty() {
                break;
            }
            if app.active.is_none() {
                app.activate(0);
            }
            app.close_active(true);
        }
        app.status = None;
    }

    /// Run a scene's script and show the document it makes in a new tab.
    /// Each placeholder of a template is reachable as `${d.ph.NAME}` (its
    /// frame) and `${d.story.NAME}` (its story), as the result of the step
    /// that is saved as `d`.
    fn build(&mut self, app: &mut PubrsApp, script: &str) -> Result<(), String> {
        let steps = serde_json::from_str::<ScriptFile>(script)
            .map_err(|e| format!("bad script: {e}"))?
            .steps();
        let mut vars: BTreeMap<String, Value> = BTreeMap::new();
        vars.insert("content".into(), json!(CONTENT_DIR));
        vars.insert("csv".into(), json!(CSV));
        for (
            i,
            Step {
                tool,
                args,
                save_as,
                ..
            },
        ) in steps.iter().enumerate()
        {
            let args =
                substitute(args, &vars).map_err(|e| format!("step {i} ({tool}): {}", e.message))?;
            let mut out = call_tool(&mut app.session, tool, args)
                .map_err(|e| format!("step {i} ({tool}): {} {}", e.message, e.hint))?;
            if let Some(list) = out.get("placeholders").and_then(Value::as_array).cloned() {
                let mut ph = serde_json::Map::new();
                let mut story = serde_json::Map::new();
                for p in &list {
                    let Some(name) = p.get("name").and_then(Value::as_str) else {
                        continue;
                    };
                    if let Some(f) = p.get("frame_id") {
                        ph.insert(name.to_string(), f.clone());
                    }
                    if let Some(s) = p.get("story_id") {
                        story.insert(name.to_string(), s.clone());
                    }
                }
                if let Some(o) = out.as_object_mut() {
                    o.insert("ph".into(), Value::Object(ph));
                    o.insert("story".into(), Value::Object(story));
                }
            }
            if let Some(name) = save_as {
                vars.insert(name.clone(), out);
            }
        }
        let id = vars
            .get("d")
            .and_then(|d| d.get("doc_id"))
            .cloned()
            .ok_or("the script saves no `d` result with a doc_id")?;
        let id = serde_json::from_value(id).map_err(|e| format!("doc_id: {e}"))?;
        app.add_view(id);
        self.vars = vars;
        Ok(())
    }

    /// A frame the script made: a dotted variable path such as `d.ph.masthead`.
    fn frame(&self, path: &str) -> Result<FrameId, String> {
        let v = substitute(&json!(format!("${{{path}}}")), &self.vars)
            .map_err(|e| format!("{path}: {}", e.message))?;
        serde_json::from_value(v).map_err(|e| format!("{path}: {e}"))
    }

    /// The page, zoom (`None` fits the page) and selection of the document shown.
    fn show(app: &mut PubrsApp, page: usize, zoom: Option<f32>, select: Vec<FrameId>) {
        if let Some(v) = app.view_mut() {
            v.page = page;
            v.zoom = zoom;
        }
        app.select(select);
        ready(app);
    }

    fn click_target(&mut self, ctx: &egui::Context, name: &str) -> Result<(), String> {
        let r =
            target_rect(ctx, name).ok_or_else(|| format!("control '{name}' is not on screen"))?;
        self.pending.extend(click_events(r.center(), false));
        Ok(())
    }
}

/// The neutral status-bar message.
fn ready(app: &mut PubrsApp) {
    app.status = Some(("Ready".to_string(), Instant::now()));
}

fn ensure(cond: bool, msg: &str) -> Result<(), String> {
    if cond { Ok(()) } else { Err(msg.to_string()) }
}

// ---------------------------------------------------------------- scenes

/// The hero: page 1 of the newsletter, its lead story selected.
fn scene_newsletter(
    s: &mut Showcase,
    app: &mut PubrsApp,
    _ctx: &egui::Context,
    phase: usize,
) -> Result<Next, String> {
    match phase {
        0 => {
            s.reset(app);
            s.build(app, NEWSLETTER)?;
            let lead = s.frame("d.ph.lead_story")?;
            Showcase::show(app, 0, None, vec![lead]);
            Ok(Next::ShotDone("newsletter.png"))
        }
        _ => Ok(Next::Done),
    }
}

/// File > New from Template with the gallery open over a finished publication.
fn scene_templates(
    s: &mut Showcase,
    app: &mut PubrsApp,
    _ctx: &egui::Context,
    phase: usize,
) -> Result<Next, String> {
    match phase {
        0 => {
            s.reset(app);
            s.build(app, NEWSLETTER)?;
            Showcase::show(app, 0, None, Vec::new());
            app.dialog = Some(Dialog::TemplatePicker);
            Ok(Next::Go)
        }
        1 => {
            ensure(
                app.template_list.as_ref().is_some_and(|l| !l.is_empty())
                    && app.template_thumbs.len() >= app.template_list.as_ref().map_or(0, Vec::len),
                "the template gallery has no thumbnails",
            )?;
            Ok(Next::ShotDone("templates.png"))
        }
        _ => Ok(Next::Done),
    }
}

/// The events table on page 2 of the newsletter, a cell selected.
fn scene_tables(
    s: &mut Showcase,
    app: &mut PubrsApp,
    _ctx: &egui::Context,
    phase: usize,
) -> Result<Next, String> {
    match phase {
        0 => {
            s.reset(app);
            s.build(app, NEWSLETTER)?;
            let table = s.frame("d.named_frames.events_table")?;
            Showcase::show(app, 1, None, vec![table]);
            Ok(Next::Go)
        }
        1 => {
            // Select the "Open workshop" cell, as a click on it does.
            let table = s.frame("d.named_frames.events_table")?;
            if let Some(v) = app.view_mut() {
                v.table_sel = Some(crate::tables::TableSel {
                    frame: table,
                    r0: 2,
                    c0: 1,
                    r1: 2,
                    c1: 1,
                });
            }
            app.focus_cell_props = true;
            s.pending.push_back(vec![Event::PointerGone]);
            ready(app);
            Ok(Next::Go)
        }
        _ => Ok(Next::ShotDone("tables.png")),
    }
}

/// The Mail Merge wizard over the label template, 30 records loaded.
fn scene_mail_merge(
    s: &mut Showcase,
    app: &mut PubrsApp,
    _ctx: &egui::Context,
    phase: usize,
) -> Result<Next, String> {
    match phase {
        0 => {
            s.reset(app);
            s.build(app, LABELS)?;
            let label = s.frame("d.ph.address")?;
            Showcase::show(app, 0, Some(2.5), vec![label]);
            let st = MergeWizardState::new(app);
            app.dialog = Some(Dialog::MergeWizard(Box::new(st)));
            Ok(Next::Go)
        }
        1 => {
            let loaded = app
                .doc()
                .and_then(|d| d.merge.as_ref())
                .map(|m| m.record_count());
            ensure(loaded == Some(30), "the label sheet has no 30 records")?;
            Ok(Next::ShotDone("mail-merge.png"))
        }
        _ => Ok(Next::Done),
    }
}

/// The merged labels: the merge to a new publication, one label per page.
fn scene_merged_labels(
    s: &mut Showcase,
    app: &mut PubrsApp,
    _ctx: &egui::Context,
    phase: usize,
) -> Result<Next, String> {
    match phase {
        0 => {
            s.reset(app);
            s.build(app, LABELS)?;
            ensure(app.merge_to_publication(), "merging the labels failed")?;
            ensure(
                app.doc().is_some_and(|d| d.pages.len() == 30),
                "the merged publication is not 30 pages",
            )?;
            Showcase::show(app, 0, Some(3.0), Vec::new());
            Ok(Next::Go)
        }
        _ => Ok(Next::ShotDone("merged-labels.png")),
    }
}

/// A flyer with four problems listed; the overflowing text is selected.
fn scene_design_checker(
    s: &mut Showcase,
    app: &mut PubrsApp,
    _ctx: &egui::Context,
    phase: usize,
) -> Result<Next, String> {
    match phase {
        0 => {
            s.reset(app);
            s.build(app, FLYER)?;
            Showcase::show(app, 0, None, Vec::new());
            app.checker.show();
            Ok(Next::Go)
        }
        1 => {
            if app.checker.pending() || !app.checker.has_results() {
                return Ok(Next::Again);
            }
            let issue = app
                .checker
                .issues()
                .into_iter()
                .find(|w| w.code == WarningCode::Overflow)
                .cloned()
                .ok_or("the checker found no overflow")?;
            ensure(
                app.checker.issues().len() == 4,
                "the checker does not list 4 issues",
            )?;
            crate::checker::goto(app, &issue);
            ready(app);
            Ok(Next::Go)
        }
        _ => Ok(Next::ShotDone("design-checker.png")),
    }
}

/// The certificate: BorderArt border, WordArt title, the WordArt selected.
fn scene_wordart(
    s: &mut Showcase,
    app: &mut PubrsApp,
    _ctx: &egui::Context,
    phase: usize,
) -> Result<Next, String> {
    match phase {
        0 => {
            s.reset(app);
            s.build(app, CERTIFICATE)?;
            let title = s.frame("wa.frame_id")?;
            Showcase::show(app, 0, None, vec![title]);
            Ok(Next::ShotDone("wordart.png"))
        }
        _ => Ok(Next::Done),
    }
}

/// Design > Colour Scheme…: a scheme applied to the newsletter, Swatches open.
fn scene_colour(
    s: &mut Showcase,
    app: &mut PubrsApp,
    ctx: &egui::Context,
    phase: usize,
) -> Result<Next, String> {
    match phase {
        0 => {
            s.reset(app);
            s.build(app, NEWSLETTER)?;
            let lead = s.frame("d.ph.lead_story")?;
            Showcase::show(app, 0, None, vec![lead]);
            app.swatches_open = Some(true);
            app.run_action(ctx, Action::ColorScheme);
            Ok(Next::Go)
        }
        1 => {
            ensure(
                matches!(app.dialog, Some(Dialog::ColorScheme)),
                "Design > Colour Scheme did not open the dialog",
            )?;
            s.click_target(ctx, "scheme-apply-plum_orchard")?;
            Ok(Next::Go)
        }
        2 => {
            let scheme = app
                .doc()
                .and_then(|d| d.scheme.as_ref().map(|c| c.0.clone()));
            ensure(
                scheme.as_deref() == Some("plum_orchard"),
                "the scheme was not applied",
            )?;
            ready(app);
            Ok(Next::ShotDone("colour.png"))
        }
        _ => Ok(Next::Done),
    }
}

/// A Publisher file imported, the import report showing.
fn scene_pub_import(
    s: &mut Showcase,
    app: &mut PubrsApp,
    _ctx: &egui::Context,
    phase: usize,
) -> Result<Next, String> {
    match phase {
        0 => {
            s.reset(app);
            let path = std::path::absolute(Path::new(PUB_FILE)).map_err(|e| e.to_string())?;
            app.import_publisher(&path);
            ensure(
                matches!(app.dialog, Some(Dialog::ImportReport(_))),
                "the import report is not showing",
            )?;
            Showcase::show(app, 0, None, Vec::new());
            Ok(Next::Go)
        }
        _ => Ok(Next::ShotDone("pub-import.png")),
    }
}

/// File > Export PDF… with the Commercial printing preset chosen.
fn scene_export(
    s: &mut Showcase,
    app: &mut PubrsApp,
    ctx: &egui::Context,
    phase: usize,
) -> Result<Next, String> {
    match phase {
        0 => {
            s.reset(app);
            s.build(app, NEWSLETTER)?;
            Showcase::show(app, 0, None, Vec::new());
            app.run_action(ctx, Action::ExportPdf);
            Ok(Next::Go)
        }
        1 => {
            ensure(
                matches!(app.dialog, Some(Dialog::ExportPdf(_))),
                "File > Export PDF did not open the dialog",
            )?;
            s.click_target(ctx, "preset-commercial")?;
            Ok(Next::Go)
        }
        2 => {
            ready(app);
            Ok(Next::ShotDone("export.png"))
        }
        _ => Ok(Next::Done),
    }
}

/// Page-1 renders of the filled publications (no window involved).
fn scene_renders(
    s: &mut Showcase,
    app: &mut PubrsApp,
    _ctx: &egui::Context,
    phase: usize,
) -> Result<Next, String> {
    const DOCS: [(&str, &str, f64); 5] = [
        ("render-newsletter.png", NEWSLETTER, 130.0),
        ("render-certificate.png", CERTIFICATE, 130.0),
        ("render-menu.png", MENU, 130.0),
        ("render-postcard.png", POSTCARD, 170.0),
        ("render-calendar.png", CALENDAR, 130.0),
    ];
    let Some((name, script, dpi)) = DOCS.get(phase).copied() else {
        s.reset(app);
        return Ok(Next::Done);
    };
    s.reset(app);
    s.build(app, script)?;
    let id = app.doc_id();
    let opts = RasterOptions {
        dpi,
        ..Default::default()
    };
    let (w, h, png) = app
        .session
        .page_png(id, 0, &opts, false)
        .map_err(|e| format!("{name}: {}", e.message))?;
    s.write(name, &png, w as usize, h as usize)?;
    Ok(Next::Go)
}

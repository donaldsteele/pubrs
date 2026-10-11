//! The application: document views over one `pubrs_api::Session`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

use eframe::egui;
use pubrs_api::core::align::{AlignEdge, DistributeAxis};
use pubrs_api::core::{Command, Document, DocumentId, FrameId, FrameKind, Owner, StoryId};
use pubrs_api::tools::doc::{DocClose, DocNew, DocOpen, DocRef, DocSaveAs};
use pubrs_api::tools::export::{ExportPdf, ExportPng, ExportPrintPackage};
use pubrs_api::tools::frames::{
    FrameAlign, FrameDistribute, FrameDuplicate, FrameGroup, FrameLock, FrameSetZorder,
    FrameUngroup, RelativeTo, ZAction,
};
use pubrs_api::tools::images::ImagePlace;
use pubrs_api::tools::pages::{PageAdd, PageDelete};
use pubrs_api::tools::templates::{TemplateApply, TemplateSave};
use pubrs_api::tools::text::{TextLinkFrames, TextUnlinkFrames};
use pubrs_api::{ApiError, ApiResult, Session, SessionConfig};

use crate::autosave::Autosave;
use crate::dialogs::Dialog;
use crate::keymap::{self, Action};
use crate::platform::menu::NativeMenu;
use crate::platform::{Platform, PrintOptions, PrintOutcome};
use crate::selftest::SelfTest;
use crate::view::DocView;
use crate::{Args, Outcome};

/// Why Insert → Break is unavailable.
const NO_CARET: &str = "Click in the text where the break should go first";
/// Why Text → Drop Cap… is unavailable.
const NO_PARAGRAPH: &str = "Click in a paragraph or select a text box first";
/// Why Text → Character… and the super/subscript buttons are unavailable.
const NO_CHARS: &str = "Select some text, or a text box, first";

/// The active creation tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Text,
    Picture,
    Rectangle,
    Ellipse,
    Line,
}

/// Measurement units for the properties panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Units {
    Inches,
    Millimetres,
    Points,
}

impl Units {
    pub fn per_point(self) -> f64 {
        match self {
            Units::Inches => 1.0 / 72.0,
            Units::Millimetres => 25.4 / 72.0,
            Units::Points => 1.0,
        }
    }

    pub fn suffix(self) -> &'static str {
        match self {
            Units::Inches => " in",
            Units::Millimetres => " mm",
            Units::Points => " pt",
        }
    }
}

/// What draws the window, for the About box and the software-drawing warning.
#[derive(Debug, Clone, Default)]
pub struct WindowRenderer {
    pub name: String,
    pub api: String,
    pub software: bool,
}

impl WindowRenderer {
    #[allow(unsafe_code)]
    fn detect(cc: &eframe::CreationContext<'_>) -> Self {
        if let Some(rs) = &cc.wgpu_render_state {
            let a = rs.adapter.get_info();
            return Self {
                software: crate::adapter::is_software(&a),
                api: format!("{:?}", a.backend),
                name: a.name,
            };
        }
        if let Some(gl) = &cc.gl {
            use eframe::glow::HasContext as _;
            // SAFETY: eframe made this context current for the creation callback.
            let name = unsafe { gl.get_parameter_string(eframe::glow::RENDERER) };
            return Self {
                software: crate::adapter::is_software_name(&name),
                api: "OpenGL".into(),
                name,
            };
        }
        Self::default()
    }
}

pub struct PubrsApp {
    pub session: Session,
    pub platform: Platform,
    pub views: Vec<DocView>,
    pub active: Option<usize>,
    pub tool: Tool,
    pub units: Units,
    pub show_guides: bool,
    pub show_rulers: bool,
    pub find: crate::find::FindState,
    pub status: Option<(String, Instant)>,
    pub dialog: Option<Dialog>,
    /// One-shot request to open (or close) the Swatches section.
    pub swatches_open: Option<bool>,
    native_menu: Option<NativeMenu>,
    menu_tried: bool,
    pub autosave: Autosave,
    pub fonts: Vec<String>,
    pub template_thumbs: HashMap<String, egui::TextureHandle>,
    pub args: Args,
    outcome: Rc<RefCell<Outcome>>,
    pub frames_drawn: u64,
    pub selftest: Option<SelfTest>,
    copied: Option<(DocumentId, Vec<FrameId>)>,
    allow_close: bool,
    saved_settings: Option<Settings>,
    pub perf: Option<crate::perf::FrameStats>,
    pub window: WindowRenderer,
    /// Draws the canvas and thumbnails off the UI thread.
    pub renderer: crate::render_worker::RenderWorker,
    /// Times the render worker was started again after its thread died.
    renderer_restarts: u32,
    /// Spell checking off the UI thread, squiggles, menu and window.
    pub spell: crate::spell::SpellState,
}

/// Failed renders of one view retried before giving up until it changes.
pub const MAX_RENDER_RETRIES: u32 = 3;
/// Times a dead render worker is started again.
const MAX_RENDERER_RESTARTS: u32 = 3;

impl PubrsApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        args: Args,
        outcome: Rc<RefCell<Outcome>>,
    ) -> Self {
        let selftest_dir = args.self_test.clone();
        let platform = match &selftest_dir {
            Some(dir) => Platform::null(dir),
            None if args.smoke_test => Platform::null(&std::env::temp_dir().join("pubrs-smoke")),
            None => Platform::native(),
        };
        // User Hunspell dictionaries and added words (M28).
        let dictionaries = platform.paths.config_dir().join("dictionaries");
        let session = Session::new(SessionConfig {
            system_fonts: platform.fonts.use_system_fonts(),
            user_templates_dir: Some(platform.paths.user_templates_dir()),
            output_dir: Some(platform.paths.documents_dir()),
            dictionaries_dir: Some(dictionaries.clone()),
            ..Default::default()
        });
        let mut visuals = egui::Visuals::light();
        visuals.window_corner_radius = egui::CornerRadius::same(8);
        cc.egui_ctx.set_visuals(visuals);
        let fonts = Vec::new();
        let autosave = Autosave::new(platform.paths.recovery_dir());
        let mut app = Self {
            session,
            platform,
            views: Vec::new(),
            active: None,
            tool: Tool::Select,
            units: Units::Inches,
            show_guides: true,
            show_rulers: true,
            find: crate::find::FindState::default(),
            status: None,
            dialog: None,
            swatches_open: None,
            native_menu: None,
            menu_tried: false,
            autosave,
            fonts,
            template_thumbs: HashMap::new(),
            selftest: selftest_dir.map(SelfTest::new),
            args: args.clone(),
            outcome,
            frames_drawn: 0,
            copied: None,
            allow_close: false,
            saved_settings: None,
            perf: crate::perf::FrameStats::from_env(),
            window: WindowRenderer::detect(cc),
            renderer: crate::render_worker::RenderWorker::new(cc.egui_ctx.clone()),
            renderer_restarts: 0,
            spell: crate::spell::SpellState::new(&cc.egui_ctx, Some(dictionaries)),
        };
        app.load_settings();
        app.fonts = app
            .session
            .fonts_list(DocRef::default())
            .map(|f| f.families.into_iter().map(|f| f.family).collect())
            .unwrap_or_default();
        let w = &app.window;
        tracing::info!(renderer = %w.name, api = %w.api, software = w.software, "window drawing");
        if w.software {
            tracing::warn!(
                "no GPU driver for the window: drawing on the CPU is slow and uses a lot of it"
            );
        }
        tracing::info!(canvas = %app.renderer.describe(), "page rendering");
        for f in &args.files {
            app.open_path(f);
        }
        if app.selftest.is_none() && !args.smoke_test {
            let recoverable = app.autosave.recoverable();
            if !recoverable.is_empty() {
                app.dialog = Some(Dialog::Recovery(recoverable));
            } else if app.views.is_empty() {
                app.dialog = Some(Dialog::TemplatePicker);
            }
        }
        app
    }

    // ------------------------------------------------------------ helpers

    pub fn view(&self) -> Option<&DocView> {
        self.active.and_then(|i| self.views.get(i))
    }

    pub fn view_mut(&mut self) -> Option<&mut DocView> {
        self.active.and_then(|i| self.views.get_mut(i))
    }

    pub fn doc_id(&self) -> Option<DocumentId> {
        self.view().map(|v| v.doc_id)
    }

    pub fn doc(&self) -> Option<&Document> {
        let id = self.doc_id()?;
        self.session.get(Some(id)).ok().map(|o| o.doc())
    }

    pub fn notify(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        tracing::info!("{msg}");
        self.status = Some((msg, Instant::now()));
    }

    /// Show an API error to the user; return the value on success.
    pub fn report<T>(&mut self, r: ApiResult<T>) -> Option<T> {
        match r {
            Ok(v) => Some(v),
            Err(e) => {
                self.show_error(&e);
                None
            }
        }
    }

    pub fn show_error(&mut self, e: &ApiError) {
        tracing::warn!(code = %e.code, "{}", e.message);
        self.status = Some((format!("{} — {}", e.message, e.hint), Instant::now()));
    }

    pub(crate) fn add_view(&mut self, doc_id: DocumentId) {
        self.views.push(DocView::new(doc_id));
        self.active = Some(self.views.len() - 1);
        self.session.set_current(doc_id);
    }

    pub fn activate(&mut self, index: usize) {
        if index < self.views.len() {
            self.active = Some(index);
            let id = self.views[index].doc_id;
            self.session.set_current(id);
        }
    }

    /// Execute a raw command on the active document (GUI edits).
    pub fn exec(&mut self, command: Command) -> bool {
        let Some(id) = self.doc_id() else {
            return false;
        };
        let t0 = Instant::now();
        let r = self.session.exec(Some(id), command);
        if let Some(p) = &mut self.perf {
            p.command(t0.elapsed());
        }
        self.report(r).is_some()
    }

    /// Execute, merging with the previous step if it had the same key.
    pub fn exec_merge(&mut self, command: Command, key: &str) -> bool {
        let Some(id) = self.doc_id() else {
            return false;
        };
        let t0 = Instant::now();
        let r = self.session.exec_coalesced(Some(id), command, key);
        if let Some(p) = &mut self.perf {
            p.command(t0.elapsed());
        }
        self.report(r).is_some()
    }

    /// Bring a document's layout up to date, timed for the frame statistics
    /// (layout is the one cost of an edit the UI thread keeps).
    pub fn layout_timed(&mut self, doc_id: DocumentId) -> Result<(), ApiError> {
        let t0 = Instant::now();
        let r = self.session.layout(Some(doc_id)).map(drop);
        if let Some(p) = &mut self.perf {
            p.layout(t0.elapsed());
        }
        r
    }

    /// Put what the render worker finished on screen: full renders replace
    /// a view's texture, damage repaints update parts of it, thumbnails
    /// replace theirs. Only uploads happen here.
    fn apply_renders(&mut self, ctx: &egui::Context) {
        use crate::render_worker::Rendered;
        use crate::view::{PageTexture, Thumb};
        let opts = egui::TextureOptions::LINEAR;
        let polled = self.renderer.poll();
        for r in polled.results {
            match r {
                Rendered::View(v) => {
                    if let Some(p) = &mut self.perf {
                        p.worker(v.spent);
                        p.upload(v.patches.iter().map(|(r, _)| r.area()).sum());
                    }
                    let Some(view) = self.views.iter_mut().find(|d| d.doc_id == v.key.doc_id)
                    else {
                        continue;
                    };
                    if v.key.generation != view.tex_generation {
                        continue;
                    }
                    view.render_failures = 0;
                    if v.full {
                        let Some((_, img)) = v.patches.into_iter().next() else {
                            continue;
                        };
                        match &mut view.page_tex {
                            Some(t) => {
                                t.texture.set(img, opts);
                                t.key = v.key;
                                t.revision = v.revision;
                            }
                            None => {
                                let texture =
                                    ctx.load_texture(format!("page-{}", v.key.doc_id), img, opts);
                                view.page_tex = Some(PageTexture {
                                    key: v.key,
                                    revision: v.revision,
                                    texture,
                                });
                            }
                        }
                    } else if let Some(t) = &mut view.page_tex
                        && t.key == v.key
                    {
                        for (r, img) in v.patches {
                            t.texture
                                .set_partial([r.x as usize, r.y as usize], img, opts);
                        }
                        t.revision = v.revision;
                    } else {
                        // Out of step with the worker: start over in full.
                        view.tex_generation += 1;
                        view.requested = None;
                    }
                }
                Rendered::Thumb(t) => {
                    if let Some(p) = &mut self.perf {
                        p.worker(t.spent);
                    }
                    let Some(view) = self.views.iter_mut().find(|d| d.doc_id == t.doc_id) else {
                        continue;
                    };
                    let texture = ctx.load_texture(
                        format!("thumb-{}-{}", t.doc_id, t.page_index),
                        t.image,
                        opts,
                    );
                    view.thumbs.insert(
                        t.page_index,
                        Thumb {
                            revision: t.revision,
                            fingerprint: t.fingerprint,
                            texture,
                        },
                    );
                }
                Rendered::Failed {
                    doc_id,
                    page_index,
                    message,
                } => {
                    tracing::warn!("page render failed: {message}");
                    let view = doc_id.and_then(|d| self.views.iter_mut().find(|v| v.doc_id == d));
                    if let Some(view) = view {
                        // Ask again (the worker now draws in full), a few
                        // times: a render that always fails must not spin.
                        if view.render_failures < MAX_RENDER_RETRIES {
                            view.render_failures += 1;
                            match page_index {
                                Some(i) => {
                                    view.thumbs_requested.remove(&i);
                                }
                                None => view.requested = None,
                            }
                        }
                    }
                    self.status = Some((
                        format!("Could not draw the page: {message}"),
                        Instant::now(),
                    ));
                }
            }
        }
        if polled.stopped {
            tracing::error!("the render worker stopped");
            if self.renderer_restarts < MAX_RENDERER_RESTARTS {
                self.renderer_restarts += 1;
                self.renderer = crate::render_worker::RenderWorker::new(ctx.clone());
                // The new worker has drawn nothing: every view asks again.
                for v in &mut self.views {
                    v.requested = None;
                    v.thumbs_requested.clear();
                    v.tex_generation += 1;
                }
                self.status = Some((
                    "Page drawing stopped unexpectedly and was restarted".into(),
                    Instant::now(),
                ));
            } else {
                self.status = Some((
                    "Page drawing stopped; save your work and restart pubrs".into(),
                    Instant::now(),
                ));
            }
        }
    }

    pub fn selection(&self) -> Vec<FrameId> {
        self.view().map(|v| v.selection.clone()).unwrap_or_default()
    }

    pub fn select(&mut self, frames: Vec<FrameId>) {
        if let Some(v) = self.view_mut() {
            v.selection = frames;
            v.edit = None;
        }
    }

    pub fn page_owner(&self) -> Option<Owner> {
        let v = self.view()?;
        let doc = self.doc()?;
        doc.pages.get(v.page).map(|p| Owner::Page(p.id))
    }

    // ------------------------------------------------------------ documents

    pub fn open_path(&mut self, path: &Path) {
        let r = self.session.doc_open(DocOpen {
            path: session_path(path),
        });
        if let Some(out) = self.report(r) {
            self.add_view(out.doc_id);
            self.notify(format!("Opened {}", path.display()));
        }
    }

    pub fn new_from_template(&mut self, name: &str) -> Option<DocumentId> {
        let r = self.session.template_apply(TemplateApply {
            name: name.to_string(),
            title: None,
        });
        let out = self.report(r)?;
        self.add_view(out.doc_id);
        self.notify(format!("New publication from the {name} template"));
        Some(out.doc_id)
    }

    pub fn new_blank(&mut self) {
        let r = self.session.doc_new(DocNew {
            title: Some("Untitled".into()),
            ..Default::default()
        });
        if let Some(out) = self.report(r) {
            self.add_view(out.doc_id);
        }
    }

    pub fn save(&mut self) -> bool {
        let Some(id) = self.doc_id() else {
            return false;
        };
        let has_path = self.session.get(Some(id)).is_ok_and(|o| o.path.is_some());
        if !has_path {
            return self.save_as();
        }
        let r = self.session.doc_save(DocRef { doc_id: Some(id) });
        match self.report(r) {
            Some(out) => {
                self.autosave.forget(id);
                self.notify(format!("Saved {}", out.path));
                true
            }
            None => false,
        }
    }

    pub fn save_as(&mut self) -> bool {
        let title = self.doc().map(|d| d.meta.title.clone()).unwrap_or_default();
        let name = pubrs_api::session::slug(&title);
        match self.platform.dialogs.save_document(&name) {
            Some(p) => self.save_to(&p),
            None => false,
        }
    }

    pub fn save_to(&mut self, path: &Path) -> bool {
        let Some(id) = self.doc_id() else {
            return false;
        };
        let r = self.session.doc_save_as(DocSaveAs {
            doc_id: Some(id),
            path: Some(session_path(path)),
        });
        match self.report(r) {
            Some(out) => {
                self.autosave.forget(id);
                self.notify(format!("Saved {}", out.path));
                true
            }
            None => false,
        }
    }

    pub fn export_pdf_to(&mut self, path: &Path) -> bool {
        let Some(id) = self.doc_id() else {
            return false;
        };
        let r = self.session.export_pdf(ExportPdf {
            doc_id: Some(id),
            path: Some(session_path(path)),
            ..Default::default()
        });
        match self.report(r) {
            Some(out) => {
                self.notify(format!(
                    "Exported {} ({} pages)",
                    path.display(),
                    out.page_count
                ));
                true
            }
            None => false,
        }
    }

    /// Export a PDF with the Export dialog's choices.
    pub fn export_pdf_with(&mut self, path: &Path, st: &crate::dialogs::ExportState) -> bool {
        let Some(id) = self.doc_id() else {
            return false;
        };
        let r = self.session.export_pdf(ExportPdf {
            doc_id: Some(id),
            path: Some(session_path(path)),
            bleed: Some(st.bleed),
            crop_marks: st.crop_marks,
            standard: st.standard(),
            tagged: st.tagged,
            ..Default::default()
        });
        match self.report(r) {
            Some(out) => {
                self.notify(format!(
                    "Exported {} ({} pages)",
                    path.display(),
                    out.page_count
                ));
                true
            }
            None => false,
        }
    }

    /// Save the zip for a commercial printer.
    pub fn export_print_package_to(&mut self, path: &Path, bleed: bool, crop_marks: bool) -> bool {
        let Some(id) = self.doc_id() else {
            return false;
        };
        let r = self.session.export_print_package(ExportPrintPackage {
            doc_id: Some(id),
            path: Some(session_path(path)),
            bleed: Some(bleed),
            crop_marks,
            output_intent_icc: None,
        });
        match self.report(r) {
            Some(_) => {
                self.notify(format!("Saved {} for a commercial printer", path.display()));
                true
            }
            None => false,
        }
    }

    fn export_png_to(&mut self, path: &Path) {
        let Some(v) = self.view() else { return };
        let page = self.doc().and_then(|d| d.pages.get(v.page).map(|p| p.id));
        let r = self.session.export_png(ExportPng {
            doc_id: Some(v.doc_id),
            page_id: page,
            dpi: Some(300.0),
            path: Some(session_path(path)),
            ..Default::default()
        });
        if self.report(r).is_some() {
            self.notify(format!("Exported {}", path.display()));
        }
    }

    /// Export a print PDF (no bleed, no marks) and hand it to the OS printer path.
    pub fn print(&mut self, options: &PrintOptions) {
        let Some(id) = self.doc_id() else { return };
        let tmp = std::env::temp_dir().join(format!("pubrs-print-{}.pdf", std::process::id()));
        let r = self.session.export_pdf(ExportPdf {
            doc_id: Some(id),
            path: Some(tmp.display().to_string()),
            bleed: Some(false),
            ..Default::default()
        });
        if self.report(r).is_none() {
            return;
        }
        let imposed = self.doc().and_then(crate::dialogs::imposition_note);
        let title = self.doc().map(|d| d.meta.title.clone()).unwrap_or_default();
        match self.platform.print.print_pdf(&tmp, &title, options) {
            Ok(PrintOutcome::Printed) => self.notify(match imposed {
                Some(n) => format!("Sent to the printer. {n}"),
                None => "Sent to the printer".into(),
            }),
            Ok(PrintOutcome::Queued(msg)) => self.notify(format!("Printing: {msg}")),
            Ok(PrintOutcome::Cancelled) => self.notify("Printing cancelled"),
            Err(e) => self.dialog = Some(Dialog::Message("Printing failed".into(), e)),
        }
    }

    /// Close the active document (asks first if it has unsaved changes).
    pub fn close_active(&mut self, discard: bool) {
        let Some(i) = self.active else { return };
        let id = self.views[i].doc_id;
        let dirty = self
            .session
            .get(Some(id))
            .is_ok_and(|o| o.editor.is_dirty());
        if dirty && !discard {
            self.dialog = Some(Dialog::ConfirmClose(id));
            return;
        }
        let _ = self.session.doc_close(DocClose {
            doc_id: Some(id),
            confirm: true,
        });
        self.autosave.forget(id);
        self.views.remove(i);
        self.active = if self.views.is_empty() {
            None
        } else {
            Some(i.min(self.views.len() - 1))
        };
        if let Some(j) = self.active {
            self.activate(j);
        }
    }

    pub fn dirty_docs(&self) -> Vec<DocumentId> {
        self.views
            .iter()
            .filter(|v| {
                self.session
                    .get(Some(v.doc_id))
                    .is_ok_and(|o| o.editor.is_dirty())
            })
            .map(|v| v.doc_id)
            .collect()
    }

    pub fn quit(&mut self, ctx: &egui::Context, force: bool) {
        if !force && !self.dirty_docs().is_empty() {
            self.dialog = Some(Dialog::ConfirmQuit);
            return;
        }
        self.allow_close = true;
        for v in &self.views {
            self.autosave.forget(v.doc_id);
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    /// Files dropped on the window: publications open, a picture goes into the
    /// selected picture frame.
    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let paths: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        for path in paths {
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            if ext == "pubrs" {
                self.open_path(&path);
                continue;
            }
            let target = match self.selection().as_slice() {
                [f] => self
                    .doc()
                    .and_then(|d| d.frame(*f))
                    .filter(|fr| fr.kind() == FrameKind::Image)
                    .map(|fr| fr.id),
                _ => None,
            };
            match target {
                Some(frame) => {
                    self.place_image(frame, &path);
                }
                None => self.notify(format!(
                    "Select one picture frame, then drop {} on the window to place it",
                    path.display()
                )),
            }
        }
    }

    // ------------------------------------------------------------ frame actions

    pub fn place_image(&mut self, frame: FrameId, path: &Path) -> bool {
        let Some(id) = self.doc_id() else {
            return false;
        };
        let r = self.session.image_place(ImagePlace {
            doc_id: Some(id),
            frame_id: frame,
            path: Some(session_path(path)),
            base64: None,
            svg: None,
            filename: None,
            fit: None,
        });
        match self.report(r) {
            Some(out) => {
                if let Some(dpi) = out.effective_dpi
                    && dpi < 150.0
                {
                    self.notify(format!(
                        "Picture placed — it will print at {dpi:.0} dpi (150+ recommended)"
                    ));
                }
                true
            }
            None => false,
        }
    }

    fn zorder(&mut self, action: ZAction) {
        let id = self.doc_id();
        for f in self.selection() {
            let r = self.session.frame_set_zorder(FrameSetZorder {
                doc_id: id,
                frame_id: f,
                action: Some(action),
                index: None,
            });
            self.report(r);
        }
    }

    fn align(&mut self, edge: AlignEdge) {
        let sel = self.selection();
        let rel = if sel.len() > 1 {
            RelativeTo::Selection
        } else {
            RelativeTo::Margins
        };
        let r = self.session.frame_align(FrameAlign {
            doc_id: self.doc_id(),
            frame_ids: sel,
            edge,
            relative_to: rel,
        });
        self.report(r);
    }

    fn distribute(&mut self, axis: DistributeAxis) {
        let r = self.session.frame_distribute(FrameDistribute {
            doc_id: self.doc_id(),
            frame_ids: self.selection(),
            axis,
            within: RelativeTo::Selection,
        });
        self.report(r);
    }

    pub fn delete_selection(&mut self) {
        let sel = self.selection();
        if sel.is_empty() {
            return;
        }
        let commands = sel
            .iter()
            .map(|f| Command::DeleteFrame { frame: *f })
            .collect();
        if self.exec(Command::Batch {
            label: "delete".into(),
            commands,
        }) {
            self.select(Vec::new());
        }
    }

    fn duplicate_selection(&mut self, page: Option<pubrs_api::core::PageId>, offset: f64) {
        let mut new = Vec::new();
        for f in self.selection() {
            let r = self.session.frame_duplicate(FrameDuplicate {
                doc_id: self.doc_id(),
                frame_id: f,
                dx: Some(offset),
                dy: Some(offset),
                page_id: page,
            });
            if let Some(out) = self.report(r) {
                new.push(out.frame_id);
            }
        }
        if !new.is_empty() {
            self.select(new);
        }
    }

    fn story_of_selection(&self) -> Option<StoryId> {
        let doc = self.doc()?;
        if let Some(e) = self.view().and_then(|v| v.edit.as_ref()) {
            return Some(e.story);
        }
        self.selection().iter().find_map(|f| doc.story_of(*f))
    }

    /// Toggle bold/italic/underline on the edited selection or selected text frames.
    pub fn toggle_char(&mut self, which: &str) {
        let Some(story) = self.story_of_selection() else {
            return;
        };
        let Some(doc) = self.doc() else { return };
        let Some(s) = doc.story(story) else { return };
        let (start, end) = match self.view().and_then(|v| v.edit.as_ref()) {
            Some(e) if e.has_selection() => e.range(),
            Some(_) => return,
            None => (0, s.char_len()),
        };
        let para = &s.paragraphs[s.locate(start).0];
        let run_fmt = s.format_at(start + 1);
        let run = pubrs_api::core::Run {
            text: String::new(),
            style: run_fmt.style,
            props: run_fmt.props,
        };
        let a = doc.styles.resolve_char(para, &run);
        let mut props = pubrs_api::core::CharProps::default();
        match which {
            "bold" => props.bold = Some(!a.bold),
            "italic" => props.italic = Some(!a.italic),
            "superscript" | "subscript" => {
                let want = if which == "superscript" {
                    pubrs_api::core::Baseline::Superscript
                } else {
                    pubrs_api::core::Baseline::Subscript
                };
                props.baseline = Some(if a.baseline == want {
                    pubrs_api::core::Baseline::Normal
                } else {
                    want
                });
            }
            _ => props.underline = Some(!a.underline),
        }
        self.exec(Command::ApplyCharProps {
            story,
            start,
            end,
            props,
        });
    }

    fn link_selected(&mut self) {
        let sel = self.selection();
        let doc = match self.doc() {
            Some(d) => d,
            None => return,
        };
        let texts: Vec<FrameId> = sel
            .iter()
            .copied()
            .filter(|f| doc.frame(*f).is_some_and(|fr| fr.kind() == FrameKind::Text))
            .collect();
        if texts.len() != 2 {
            self.notify("Select two text boxes: the first one, then the empty one to continue into (Shift+click)");
            return;
        }
        let r = self.session.text_link_frames(TextLinkFrames {
            doc_id: self.doc_id(),
            from_frame_id: texts[0],
            to_frame_id: texts[1],
        });
        if let Some(out) = self.report(r) {
            self.notify(if out.overflowing {
                "Linked — the text still does not fit; link another box or make it bigger"
            } else {
                "Linked — text now flows into the second box"
            });
        }
    }

    // ------------------------------------------------------------ dispatch

    /// Why `action` cannot run now, for its menu item's tooltip; `None`
    /// when it can.
    pub fn unavailable(&self, action: Action) -> Option<&'static str> {
        let editing = self.view().is_some_and(|v| v.edit.is_some());
        let text_selected = self
            .doc()
            .is_some_and(|d| self.selection().iter().any(|f| d.story_of(*f).is_some()));
        match action {
            Action::InsertColumnBreak | Action::InsertFrameBreak | Action::InsertPageBreak
                if !editing =>
            {
                Some(NO_CARET)
            }
            Action::DropCap if !editing && !text_selected => Some(NO_PARAGRAPH),
            Action::Character | Action::Superscript | Action::Subscript
                if !editing && !text_selected =>
            {
                Some(NO_CHARS)
            }
            Action::Tabs | Action::BulletsNumbering if !editing && !text_selected => {
                Some(NO_PARAGRAPH)
            }
            _ => None,
        }
    }

    /// Insert a column, frame or page break at the caret (replacing any
    /// selected text) as its own undo step.
    pub fn insert_break(&mut self, kind: pubrs_api::core::text::BreakKind) {
        let Some(e) = self.view().and_then(|v| v.edit.clone()) else {
            self.notify(NO_CARET);
            return;
        };
        let (s, t) = e.range();
        let text = kind.char().to_string();
        let ok = if s != t {
            self.exec(Command::ReplaceText {
                story: e.story,
                start: s,
                end: t,
                text,
            })
        } else {
            self.exec(Command::InsertText {
                story: e.story,
                at: s,
                text,
                format: None,
            })
        };
        if ok && let Some(ed) = self.view_mut().and_then(|v| v.edit.as_mut()) {
            ed.set(s + 1, false);
            ed.goal_x = None;
        }
    }

    pub fn run_action(&mut self, ctx: &egui::Context, action: Action) {
        use Action::*;
        let editing = self.view().is_some_and(|v| v.edit.is_some());
        match action {
            NewFromTemplate => self.dialog = Some(Dialog::TemplatePicker),
            NewBlank => self.new_blank(),
            Open => {
                if let Some(p) = self.platform.dialogs.open_document() {
                    self.open_path(&p);
                }
            }
            Save => {
                self.save();
            }
            SaveAs => {
                self.save_as();
            }
            SaveAsTemplate => {
                if self.doc_id().is_some() {
                    self.dialog = Some(Dialog::SaveTemplate(String::new()));
                }
            }
            ExportPdf => {
                if let Some(doc) = self.doc() {
                    self.dialog = Some(Dialog::ExportPdf(crate::dialogs::ExportState::new(doc)));
                }
            }
            SavePrintPackage => {
                let Some(doc) = self.doc() else { return };
                let has_bleed = doc.setup.bleed > 0.0;
                let name = pubrs_api::session::slug(&doc.meta.title);
                if let Some(p) = self
                    .platform
                    .dialogs
                    .save_export(&name, "zip", "print package")
                {
                    self.export_print_package_to(&p, has_bleed, true);
                }
            }
            ExportPng => {
                let name = pubrs_api::session::slug(
                    &self.doc().map(|d| d.meta.title.clone()).unwrap_or_default(),
                );
                if let Some(p) = self.platform.dialogs.save_export(&name, "png", "PNG image") {
                    self.export_png_to(&p);
                }
            }
            Paragraph => {
                if let Some(state) = crate::dialogs::ParagraphState::for_selection(self) {
                    self.dialog = Some(Dialog::Paragraph(state));
                }
            }
            PageSetup => {
                if let Some(state) = crate::dialogs::PageSetupState::for_doc(self.doc()) {
                    self.dialog = Some(Dialog::PageSetup(state));
                }
            }
            Print => {
                if self.doc_id().is_none() {
                    return;
                }
                if self.platform.print.has_system_dialog() {
                    self.print(&PrintOptions::default());
                } else {
                    let printers = self.platform.print.printers();
                    self.dialog = Some(Dialog::Print(printers, PrintOptions::default()));
                }
            }
            CloseDocument => self.close_active(false),
            Quit => self.quit(ctx, false),
            Undo => {
                if let Some(id) = self.doc_id() {
                    let r = self.session.undo(DocRef { doc_id: Some(id) });
                    if let Some(u) = self.report(r)
                        && let Some(l) = u.label
                    {
                        self.notify(format!("Undid {l}"));
                    }
                    if let Some(v) = self.view_mut() {
                        v.edit = None;
                    }
                }
            }
            Redo => {
                if let Some(id) = self.doc_id() {
                    let r = self.session.redo(DocRef { doc_id: Some(id) });
                    if let Some(u) = self.report(r)
                        && let Some(l) = u.label
                    {
                        self.notify(format!("Redid {l}"));
                    }
                    if let Some(v) = self.view_mut() {
                        v.edit = None;
                    }
                }
            }
            Cut | Copy | Paste if editing => {
                // Menu clipboard commands while editing text use the OS clipboard
                // (keyboard shortcuts arrive as egui clipboard events instead).
                if let Some(e) = self.view().and_then(|v| v.edit.clone()) {
                    if action == Paste {
                        if let Some(t) = self.platform.clipboard.get_text() {
                            crate::canvas::edit_insert(self, &e, &t);
                        }
                    } else if e.has_selection() {
                        let (s, t) = e.range();
                        let text: String = self
                            .doc()
                            .and_then(|d| d.story(e.story))
                            .map(|st| st.plain_text().chars().skip(s).take(t - s).collect())
                            .unwrap_or_default();
                        self.platform.clipboard.set_text(&text);
                        if action == Cut {
                            crate::canvas::edit_insert(self, &e, "");
                        }
                    }
                }
            }
            Cut | Copy | Paste => match action {
                Copy => self.copy_frames(),
                Cut => {
                    self.copy_frames();
                    self.delete_selection();
                }
                _ => self.paste_frames(),
            },
            Delete => {
                if !editing {
                    self.delete_selection();
                }
            }
            Duplicate => self.duplicate_selection(None, 12.0),
            SelectAll => {
                if !editing && let (Some(doc), Some(owner)) = (self.doc(), self.page_owner()) {
                    let all = doc.children(owner.into()).cloned().unwrap_or_default();
                    self.select(all);
                }
            }
            InsertText => self.tool = crate::app::Tool::Text,
            InsertPicture => self.tool = crate::app::Tool::Picture,
            InsertRectangle => self.tool = crate::app::Tool::Rectangle,
            InsertEllipse => self.tool = crate::app::Tool::Ellipse,
            InsertLine => self.tool = crate::app::Tool::Line,
            InsertPage => {
                let index = self.view().map(|v| v.page + 1);
                let r = self.session.page_add(PageAdd {
                    doc_id: self.doc_id(),
                    index,
                    master_id: None,
                });
                if let Some(out) = self.report(r)
                    && let Some(v) = self.view_mut()
                {
                    v.page = out.index;
                }
            }
            DeletePage => {
                if let (Some(v), Some(doc)) = (self.view(), self.doc())
                    && doc.pages.len() > 1
                {
                    let page = doc.pages[v.page].id;
                    self.dialog = Some(Dialog::ConfirmDeletePage(page));
                }
            }
            InsertColumnBreak | InsertFrameBreak | InsertPageBreak => {
                use pubrs_api::core::text::BreakKind;
                let kind = match action {
                    InsertColumnBreak => BreakKind::Column,
                    InsertFrameBreak => BreakKind::Frame,
                    _ => BreakKind::Page,
                };
                self.insert_break(kind);
            }
            Tabs => match crate::dialogs::TabsState::for_selection(self) {
                Some(state) => self.dialog = Some(Dialog::Tabs(state)),
                None => self.notify(NO_PARAGRAPH),
            },
            BulletsNumbering => match crate::dialogs::ListState::for_selection(self) {
                Some(state) => self.dialog = Some(Dialog::Lists(state)),
                None => self.notify(NO_PARAGRAPH),
            },
            DropCap => match crate::dialogs::DropCapState::for_selection(self) {
                Some(state) => self.dialog = Some(Dialog::DropCap(state)),
                None => self.notify(NO_PARAGRAPH),
            },
            BringToFront => self.zorder(ZAction::BringToFront),
            BringForward => self.zorder(ZAction::BringForward),
            SendBackward => self.zorder(ZAction::SendBackward),
            SendToBack => self.zorder(ZAction::SendToBack),
            Group => {
                let r = self.session.frame_group(FrameGroup {
                    doc_id: self.doc_id(),
                    frame_ids: self.selection(),
                });
                if let Some(out) = self.report(r) {
                    self.select(vec![out.group_id]);
                }
            }
            Ungroup => {
                let doc = self.doc();
                let groups: Vec<FrameId> = self
                    .selection()
                    .into_iter()
                    .filter(|f| {
                        doc.and_then(|d| d.frame(*f))
                            .is_some_and(|fr| fr.kind() == FrameKind::Group)
                    })
                    .collect();
                let mut members = Vec::new();
                for g in groups {
                    let r = self.session.frame_ungroup(FrameUngroup {
                        doc_id: self.doc_id(),
                        group_id: g,
                    });
                    if let Some(out) = self.report(r) {
                        members.extend(out.frame_ids);
                    }
                }
                if !members.is_empty() {
                    self.select(members);
                }
            }
            AlignLeft => self.align(AlignEdge::Left),
            AlignCenter => self.align(AlignEdge::Center),
            AlignRight => self.align(AlignEdge::Right),
            AlignTop => self.align(AlignEdge::Top),
            AlignMiddle => self.align(AlignEdge::Middle),
            AlignBottom => self.align(AlignEdge::Bottom),
            DistributeHorizontally => self.distribute(DistributeAxis::Horizontal),
            DistributeVertically => self.distribute(DistributeAxis::Vertical),
            ToggleLock => {
                let doc = self.doc();
                let locked = self
                    .selection()
                    .first()
                    .and_then(|f| doc.and_then(|d| d.frame(*f)))
                    .is_some_and(|f| f.locked);
                for f in self.selection() {
                    let r = self.session.frame_lock(FrameLock {
                        doc_id: self.doc_id(),
                        frame_id: f,
                        locked: !locked,
                    });
                    self.report(r);
                }
            }
            Rotate90 => {
                let doc = self.doc();
                let cmds: Vec<Command> = self
                    .selection()
                    .iter()
                    .filter_map(|f| doc.and_then(|d| d.frame(*f)))
                    .map(|f| Command::SetRotation {
                        frame: f.id,
                        degrees: f.rotation + 90.0,
                    })
                    .collect();
                self.exec(Command::Batch {
                    label: "rotate".into(),
                    commands: cmds,
                });
            }
            LinkTextFrames => self.link_selected(),
            UnlinkTextFrames => {
                if let Some(f) = self.selection().first().copied() {
                    let r = self.session.text_unlink_frames(TextUnlinkFrames {
                        doc_id: self.doc_id(),
                        frame_id: f,
                    });
                    self.report(r);
                }
            }
            Bold => self.toggle_char("bold"),
            Italic => self.toggle_char("italic"),
            Underline => self.toggle_char("underline"),
            Superscript => self.toggle_char("superscript"),
            Subscript => self.toggle_char("subscript"),
            Character => match crate::dialogs::CharState::for_selection(self) {
                Some(state) => self.dialog = Some(Dialog::Character(state)),
                None => self.notify(NO_CHARS),
            },
            ZoomIn | ZoomOut | ZoomActual | ZoomFit => {
                if let Some(v) = self.view_mut() {
                    v.zoom = match action {
                        ZoomIn => Some((v.effective_zoom * 1.25).min(8.0)),
                        ZoomOut => Some((v.effective_zoom / 1.25).max(0.1)),
                        ZoomActual => Some(1.0),
                        _ => None,
                    };
                }
            }
            NextPage | PreviousPage => {
                let n = self.doc().map_or(0, |d| d.pages.len());
                if let Some(v) = self.view_mut()
                    && v.edit.is_none()
                {
                    v.page = if action == NextPage {
                        (v.page + 1).min(n.saturating_sub(1))
                    } else {
                        v.page.saturating_sub(1)
                    };
                    v.selection.clear();
                }
            }
            ToggleGuides => self.show_guides = !self.show_guides,
            ToggleRulers => self.show_rulers = !self.show_rulers,
            Find => crate::find::open(self, crate::find::Mode::Bar),
            Replace => crate::find::open(self, crate::find::Mode::Replace),
            FindNext => crate::find::step(self, true),
            FindPrevious => crate::find::step(self, false),
            ToggleSnap => {
                if let Some(doc) = self.doc() {
                    let mut g = doc.grid;
                    g.snap_to_guides = !g.snap_to_guides;
                    self.exec(Command::SetGrid { grid: g });
                }
            }
            Validate => {
                if let Some(id) = self.doc_id() {
                    let r = self.session.validate(DocRef { doc_id: Some(id) });
                    if let Some(v) = self.report(r) {
                        self.dialog = Some(Dialog::Validation(v.warnings));
                    }
                }
            }
            ColorScheme => {
                if self.doc().is_some() {
                    self.dialog = Some(Dialog::ColorScheme);
                }
            }
            ShowSwatches => {
                if self.doc().is_some() {
                    self.swatches_open = Some(true);
                }
            }
            About => self.dialog = Some(Dialog::About),
            McpHelp => self.dialog = Some(Dialog::McpHelp),
            Spelling => crate::spell::open_dialog(self),
        }
    }

    fn copy_frames(&mut self) {
        if let (Some(id), sel) = (self.doc_id(), self.selection())
            && !sel.is_empty()
        {
            self.copied = Some((id, sel));
            self.notify("Copied");
        }
    }

    fn paste_frames(&mut self) {
        let Some((from, frames)) = self.copied.clone() else {
            return;
        };
        if Some(from) != self.doc_id() {
            self.notify("Pasting between publications is not supported yet");
            return;
        }
        let page = self.page_owner().and_then(|o| match o {
            Owner::Page(p) => Some(p),
            Owner::Master(_) => None,
        });
        self.select(frames);
        self.duplicate_selection(page, 18.0);
    }

    pub fn save_template(&mut self, name: &str) {
        let r = self.session.template_save(TemplateSave {
            doc_id: self.doc_id(),
            name: name.to_string(),
        });
        if let Some(out) = self.report(r) {
            self.notify(format!(
                "Saved template {} ({} placeholders)",
                out.name,
                out.placeholders.len()
            ));
            self.template_thumbs.remove(&out.name);
        }
    }

    pub fn delete_page(&mut self, page: pubrs_api::core::PageId) {
        let r = self.session.page_delete(PageDelete {
            doc_id: self.doc_id(),
            page_id: page,
            confirm: true,
        });
        self.report(r);
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let editing = self.view().is_some_and(|v| v.edit.is_some());
        let text_field = ctx.text_edit_focused();
        for (sc, action) in keymap::shortcut_actions() {
            if self.native_menu.as_ref().is_some_and(|m| m.handles(action)) {
                continue;
            }
            let text_keys = matches!(
                action,
                Action::SelectAll
                    | Action::Delete
                    | Action::NextPage
                    | Action::PreviousPage
                    | Action::Duplicate
            );
            if (editing || text_field) && text_keys {
                continue;
            }
            if text_field
                && matches!(
                    action,
                    Action::Undo
                        | Action::Redo
                        | Action::Bold
                        | Action::Italic
                        | Action::Underline
                        | Action::Superscript
                        | Action::Subscript
                )
            {
                continue;
            }
            if ctx.input_mut(|i| i.consume_shortcut(&sc)) {
                self.run_action(ctx, action);
            }
        }
        if !editing && !text_field {
            let backspace =
                ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace));
            if backspace {
                self.delete_selection();
            }
            let events = ctx.input(|i| i.events.clone());
            for e in events {
                match e {
                    egui::Event::Copy => self.run_action(ctx, Action::Copy),
                    egui::Event::Cut => self.run_action(ctx, Action::Cut),
                    egui::Event::Paste(_) => self.run_action(ctx, Action::Paste),
                    _ => {}
                }
            }
        }
    }
}

impl eframe::App for PubrsApp {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if let Some(t) = &mut self.selftest {
            t.inject(ctx, raw);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let started = Instant::now();
        self.frames_drawn += 1;
        if !self.menu_tried && self.selftest.is_none() && !self.args.smoke_test {
            self.menu_tried = true;
            self.native_menu = NativeMenu::install(keymap::MENUS);
        }
        if let Some(m) = &self.native_menu {
            for a in m.poll() {
                self.run_action(&ctx, a);
            }
        }
        self.apply_renders(&ctx);
        crate::spell::poll(self, &ctx);
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            if self.dirty_docs().is_empty() || self.args.smoke_test {
                self.allow_close = true;
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.dialog = Some(Dialog::ConfirmQuit);
            }
        }
        // Keep views in sync with the documents.
        for v in &mut self.views {
            if let Ok(o) = self.session.get(Some(v.doc_id)) {
                v.prune(o.doc());
            }
        }
        self.handle_shortcuts(&ctx);
        crate::spell::dialog_keys(self, &ctx);
        self.handle_dropped_files(&ctx);
        if self.native_menu.is_none() {
            crate::panels::menu_bar(self, ui);
        }
        crate::panels::toolbar(self, ui);
        crate::panels::status_bar(self, ui);
        crate::find::bar(self, ui);
        if self.view().is_some() {
            crate::panels::pages_panel(self, ui);
            crate::panels::properties_panel(self, ui);
        }
        crate::canvas::central(self, ui);
        crate::dialogs::show(self, &ctx);
        crate::find::replace_window(self, &ctx);
        crate::spell::dialog_window(self, &ctx);
        self.autosave.tick(&self.session, &self.views);
        if self.selftest.is_none() && !self.args.smoke_test {
            self.save_settings();
        }
        if let Some(mut t) = self.selftest.take() {
            if !t.finished {
                if let Some(result) = t.step(self, &ctx) {
                    self.outcome.borrow_mut().self_test = Some(result);
                    self.allow_close = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    // Keep feeding its clock until the window closes: egui
                    // must not see time go back to the real clock.
                    t.finished = true;
                }
                ctx.request_repaint();
            }
            self.selftest = Some(t);
        }
        if self.args.smoke_test {
            if self.frames_drawn >= 3 {
                self.outcome.borrow_mut().painted = true;
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ctx.request_repaint();
        }
        if let Some((_, t)) = &self.status
            && t.elapsed().as_secs() > 8
        {
            self.status = None;
        }
        if let Some(p) = &mut self.perf {
            p.frame(
                &ctx,
                started.elapsed(),
                frame.info().cpu_usage,
                self.session.rasters_drawn(),
            );
        }
    }
}

/// Preferences kept between runs in the config folder.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct Settings {
    units: String,
    show_guides: bool,
    #[serde(default = "default_true")]
    show_rulers: bool,
}

impl PubrsApp {
    fn settings_path(&self) -> PathBuf {
        self.platform.paths.config_dir().join("settings.json")
    }

    fn current_settings(&self) -> Settings {
        Settings {
            units: match self.units {
                Units::Inches => "in",
                Units::Millimetres => "mm",
                Units::Points => "pt",
            }
            .into(),
            show_guides: self.show_guides,
            show_rulers: self.show_rulers,
        }
    }

    pub fn load_settings(&mut self) {
        let Ok(bytes) = std::fs::read(self.settings_path()) else {
            return;
        };
        let Ok(s) = serde_json::from_slice::<Settings>(&bytes) else {
            return;
        };
        self.units = match s.units.as_str() {
            "mm" => Units::Millimetres,
            "pt" => Units::Points,
            _ => Units::Inches,
        };
        self.show_guides = s.show_guides;
        self.show_rulers = s.show_rulers;
        self.saved_settings = Some(s);
    }

    /// Write settings if they changed since the last write.
    pub fn save_settings(&mut self) {
        let s = self.current_settings();
        if self.saved_settings.as_ref() == Some(&s) {
            return;
        }
        let path = self.settings_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if std::fs::write(&path, serde_json::to_vec_pretty(&s).unwrap_or_default()).is_ok() {
            self.saved_settings = Some(s);
        }
    }
}

/// A path from a file dialog or the command line, made absolute against the
/// current directory: the session resolves relative paths against its output
/// folder, which is meant for MCP clients.
fn session_path(path: &Path) -> String {
    std::path::absolute(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

fn default_true() -> bool {
    true
}

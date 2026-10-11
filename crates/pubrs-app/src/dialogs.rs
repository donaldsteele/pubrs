//! Modal dialogs.

use eframe::egui::{self, Color32, RichText};
use pubrs_api::core::{DocumentId, PageId, Warning};
use pubrs_api::render::RasterOptions;

use crate::app::PubrsApp;
use crate::autosave::Recoverable;
use crate::platform::{PrintOptions, PrinterInfo};

pub enum Dialog {
    TemplatePicker,
    Validation(Vec<Warning>),
    About,
    McpHelp,
    Print(Vec<PrinterInfo>, PrintOptions),
    ConfirmClose(DocumentId),
    ConfirmQuit,
    ConfirmDeletePage(PageId),
    Recovery(Vec<Recoverable>),
    PageSetup(PageSetupState),
    Paragraph(ParagraphState),
    SaveTemplate(String),
    Message(String, String),
    /// Design → Colour Scheme…
    ColorScheme,
    /// Add or edit a swatch.
    Swatch(SwatchEditor),
    /// File → Export PDF…
    ExportPdf(ExportState),
}

/// What kind of swatch the editor makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwatchKind {
    Rgb,
    Cmyk,
    Spot,
}

impl SwatchKind {
    fn label(self) -> &'static str {
        match self {
            SwatchKind::Rgb => "RGB",
            SwatchKind::Cmyk => "CMYK",
            SwatchKind::Spot => "Spot (CMYK alternative)",
        }
    }
}

/// State of the swatch editor dialog.
pub struct SwatchEditor {
    /// `None` while adding.
    pub id: Option<pubrs_api::core::SwatchId>,
    pub name: String,
    pub kind: SwatchKind,
    pub rgb: Color32,
    /// Cyan, magenta, yellow, black: 0 to 100.
    pub cmyk: [f64; 4],
    /// The spot ink's name.
    pub ink: String,
    pub error: String,
    focused: bool,
}

impl SwatchEditor {
    /// A new swatch of `kind` with a free default name.
    pub fn add(doc: &Document, kind: SwatchKind) -> Self {
        let base = match kind {
            SwatchKind::Rgb => "Colour",
            SwatchKind::Cmyk => "CMYK",
            SwatchKind::Spot => "Spot",
        };
        let taken = |n: &str| doc.swatches.iter().any(|s| s.name.eq_ignore_ascii_case(n));
        let name = (1..)
            .map(|i| format!("{base} {i}"))
            .find(|n| !taken(n))
            .unwrap_or_default();
        let mut e = Self::blank(kind, name);
        e.ink = format!("{base} ink");
        e
    }

    fn blank(kind: SwatchKind, name: String) -> Self {
        Self {
            id: None,
            name,
            kind,
            rgb: Color32::from_rgb(0x1E, 0x3A, 0x8A),
            cmyk: [0.0, 100.0, 100.0, 0.0],
            ink: String::new(),
            error: String::new(),
            focused: false,
        }
    }

    pub fn edit(s: &Swatch) -> Self {
        let mut e = Self::blank(SwatchKind::Rgb, s.name.clone());
        e.id = Some(s.id);
        e.name = s.name.clone();
        match &s.value {
            SwatchValue::Rgb(c) => {
                e.kind = SwatchKind::Rgb;
                e.rgb = Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a);
            }
            SwatchValue::Cmyk(c) => {
                e.kind = SwatchKind::Cmyk;
                e.cmyk = [c.c, c.m, c.y, c.k];
            }
            SwatchValue::Spot { name, cmyk } => {
                e.kind = SwatchKind::Spot;
                e.ink.clone_from(name);
                e.cmyk = [cmyk.c, cmyk.m, cmyk.y, cmyk.k];
            }
        }
        e
    }

    fn value(&self) -> SwatchValue {
        let cmyk = Cmyk::new(self.cmyk[0], self.cmyk[1], self.cmyk[2], self.cmyk[3]);
        match self.kind {
            SwatchKind::Rgb => {
                let [r, g, b, a] = self.rgb.to_srgba_unmultiplied();
                SwatchValue::Rgb(Color::rgba(r, g, b, a))
            }
            SwatchKind::Cmyk => SwatchValue::Cmyk(cmyk),
            SwatchKind::Spot => SwatchValue::Spot {
                name: self.ink.clone(),
                cmyk,
            },
        }
    }
}

/// What the user chose in a dialog this frame.
enum Outcome {
    Keep,
    Close,
}

pub fn show(app: &mut PubrsApp, ctx: &egui::Context) {
    let Some(dialog) = app.dialog.take() else {
        return;
    };
    let mut dialog = dialog;
    let outcome = egui::Modal::new(egui::Id::new("dialog"))
        .show(ctx, |ui| body(app, ui, &mut dialog))
        .inner;
    if matches!(outcome, Outcome::Keep) && app.dialog.is_none() {
        app.dialog = Some(dialog);
    }
}

fn template_thumb(
    app: &mut PubrsApp,
    ctx: &egui::Context,
    name: &str,
) -> Option<egui::TextureHandle> {
    if let Some(t) = app.template_thumbs.get(name) {
        return Some(t.clone());
    }
    // Render the template's first page in a scratch session.
    let mut s = pubrs_api::Session::new(pubrs_api::SessionConfig {
        user_templates_dir: Some(app.platform.paths.user_templates_dir()),
        software_rendering: true,
        ..Default::default()
    });
    let out = s
        .template_apply(pubrs_api::tools::templates::TemplateApply {
            name: name.into(),
            title: None,
        })
        .ok()?;
    let size = s.get(Some(out.doc_id)).ok()?.doc().setup.size;
    let dpi = (200.0 * 72.0 / size.width.max(size.height)).clamp(4.0, 72.0);
    let (w, h, px) = s
        .page_rgba(
            Some(out.doc_id),
            0,
            &RasterOptions {
                dpi,
                ..Default::default()
            },
        )
        .ok()?;
    let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px);
    let tex = ctx.load_texture(format!("tpl-{name}"), img, egui::TextureOptions::LINEAR);
    app.template_thumbs.insert(name.to_string(), tex.clone());
    Some(tex)
}

fn body(app: &mut PubrsApp, ui: &mut egui::Ui, dialog: &mut Dialog) -> Outcome {
    let ctx = ui.ctx().clone();
    match dialog {
        Dialog::TemplatePicker => {
            ui.heading("New publication");
            ui.label("Choose a template. Every part of it can be changed afterwards.");
            ui.add_space(8.0);
            let templates = app
                .session
                .template_list(Default::default())
                .map(|t| t.templates)
                .unwrap_or_default();
            let mut chosen = None;
            egui::ScrollArea::vertical()
                .max_height(520.0)
                .show(ui, |ui| {
                    egui::Grid::new("templates")
                        .spacing([16.0, 16.0])
                        .show(ui, |ui| {
                            for (i, t) in templates.iter().enumerate() {
                                ui.vertical(|ui| {
                                    ui.set_width(210.0);
                                    if let Some(tex) = template_thumb(app, &ctx, &t.name) {
                                        let size = tex.size_vec2();
                                        let scale = (200.0 / size.x).min(160.0 / size.y);
                                        let img =
                                            egui::Image::new(&tex).fit_to_exact_size(size * scale);
                                        if ui.add(egui::Button::image(img)).clicked() {
                                            chosen = Some(t.name.clone());
                                        }
                                    } else if ui.button(&t.title).clicked() {
                                        chosen = Some(t.name.clone());
                                    }
                                    ui.label(RichText::new(&t.title).strong());
                                    ui.small(format!(
                                        "{} page{}",
                                        t.page_count,
                                        if t.page_count == 1 { "" } else { "s" }
                                    ));
                                });
                                if i % 3 == 2 {
                                    ui.end_row();
                                }
                            }
                        });
                });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Blank publication").clicked() {
                    chosen = Some(String::new());
                }
                if ui.button("Open…").clicked() {
                    chosen = Some("\0open".into());
                }
                if ui.button("Cancel").clicked() {
                    chosen = Some("\0cancel".into());
                }
            });
            match chosen.as_deref() {
                None => Outcome::Keep,
                Some("") => {
                    app.new_blank();
                    Outcome::Close
                }
                Some("\0open") => {
                    app.run_action(&ctx, crate::keymap::Action::Open);
                    Outcome::Close
                }
                Some("\0cancel") => Outcome::Close,
                Some(name) => {
                    app.new_from_template(name);
                    Outcome::Close
                }
            }
        }
        Dialog::Validation(warnings) => {
            ui.heading("Check for problems");
            if warnings.is_empty() {
                ui.colored_label(Color32::DARK_GREEN, "No problems found. Ready to print.");
            } else {
                ui.label(format!(
                    "{} problem{} found:",
                    warnings.len(),
                    if warnings.len() == 1 { "" } else { "s" }
                ));
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        for w in warnings.iter() {
                            ui.group(|ui| {
                                ui.label(RichText::new(&w.message).strong());
                                ui.small(&w.hint);
                                if let Some(f) = w.frame
                                    && ui.small_button("Show").clicked()
                                {
                                    if let (Some(p), Some(doc)) = (w.page, app.doc())
                                        && let Some(i) = doc.page_index(p)
                                        && let Some(v) = app.view_mut()
                                    {
                                        v.page = i;
                                    }
                                    app.select(vec![f]);
                                }
                            });
                        }
                    });
            }
            if ui.button("Close").clicked() {
                Outcome::Close
            } else {
                Outcome::Keep
            }
        }
        Dialog::About => {
            ui.heading("pubrs");
            ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
            ui.label("Desktop publishing for flyers, brochures, cards and newsletters.");
            ui.label(format!(
                "Window drawing: {} ({}{})",
                app.window.name,
                app.window.api,
                if app.window.software {
                    ", software"
                } else {
                    ""
                }
            ));
            ui.label(format!("Page rendering: {}", app.session.render_backend()));
            ui.label(format!("Printing: {}", app.platform.print.name()));
            ui.small(
                "Bundled fonts: Liberation Sans and Liberation Serif (SIL Open Font License 1.1).",
            );
            if ui.button("Close").clicked() {
                Outcome::Close
            } else {
                Outcome::Keep
            }
        }
        Dialog::McpHelp => {
            ui.heading("Use pubrs with AI assistants");
            ui.label("pubrs includes an MCP server, pubrs-mcp, so assistants such as Claude can create and edit publications.");
            ui.add_space(6.0);
            ui.label(RichText::new("Claude Desktop").strong());
            ui.label("Add this to claude_desktop_config.json and restart Claude Desktop:");
            let cfg = "{\n  \"mcpServers\": {\n    \"pubrs\": { \"command\": \"pubrs-mcp\", \"args\": [] }\n  }\n}";
            ui.code(cfg);
            if ui.button("Copy").clicked() {
                ctx.copy_text(cfg.into());
            }
            ui.label(RichText::new("Claude Code").strong());
            ui.code("claude mcp add pubrs -- pubrs-mcp");
            ui.small(format!(
                "Files the assistant saves go to {}.",
                app.platform.paths.documents_dir().display()
            ));
            if ui.button("Close").clicked() {
                Outcome::Close
            } else {
                Outcome::Keep
            }
        }
        Dialog::PageSetup(state) => page_setup_ui(app, ui, state),
        Dialog::Paragraph(state) => paragraph_ui(app, ui, state),
        Dialog::Print(printers, options) => {
            ui.heading("Print");
            if let Some(note) = app.doc().and_then(imposition_note) {
                ui.label(note);
            }
            if printers.is_empty() {
                ui.label("No printers were found (is CUPS running?). The default destination will be used.");
            } else {
                let current = options.printer.clone().unwrap_or_else(|| {
                    printers
                        .iter()
                        .find(|p| p.is_default)
                        .or(printers.first())
                        .map(|p| p.name.clone())
                        .unwrap_or_default()
                });
                egui::ComboBox::from_label("Printer")
                    .selected_text(&current)
                    .show_ui(ui, |ui| {
                        for p in printers.iter() {
                            if ui.selectable_label(current == p.name, &p.name).clicked() {
                                options.printer = Some(p.name.clone());
                            }
                        }
                    });
                if options.printer.is_none() {
                    options.printer = Some(current);
                }
            }
            ui.horizontal(|ui| {
                ui.label("Copies");
                ui.add(egui::DragValue::new(&mut options.copies).range(1..=999));
            });
            let mut out = Outcome::Keep;
            ui.horizontal(|ui| {
                if ui.button("Print").clicked() {
                    let o = options.clone();
                    app.print(&o);
                    out = Outcome::Close;
                }
                if ui.button("Cancel").clicked() {
                    out = Outcome::Close;
                }
            });
            out
        }
        Dialog::ConfirmClose(id) => {
            ui.heading("Save changes?");
            ui.label("This publication has unsaved changes.");
            let mut out = Outcome::Keep;
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    if let Some(i) = app.views.iter().position(|v| v.doc_id == *id) {
                        app.activate(i);
                    }
                    if app.save() {
                        app.close_active(true);
                    }
                    out = Outcome::Close;
                }
                if ui.button("Don't save").clicked() {
                    if let Some(i) = app.views.iter().position(|v| v.doc_id == *id) {
                        app.activate(i);
                    }
                    app.close_active(true);
                    out = Outcome::Close;
                }
                if ui.button("Cancel").clicked() {
                    out = Outcome::Close;
                }
            });
            out
        }
        Dialog::ConfirmQuit => {
            ui.heading("Quit pubrs?");
            ui.label(format!(
                "{} publication(s) have unsaved changes.",
                app.dirty_docs().len()
            ));
            let mut out = Outcome::Keep;
            ui.horizontal(|ui| {
                if ui.button("Save all and quit").clicked() {
                    let mut ok = true;
                    for id in app.dirty_docs() {
                        if let Some(i) = app.views.iter().position(|v| v.doc_id == id) {
                            app.activate(i);
                            ok &= app.save();
                        }
                    }
                    if ok {
                        app.quit(&ctx, true);
                    }
                    out = Outcome::Close;
                }
                if ui.button("Quit without saving").clicked() {
                    app.quit(&ctx, true);
                    out = Outcome::Close;
                }
                if ui.button("Cancel").clicked() {
                    out = Outcome::Close;
                }
            });
            out
        }
        Dialog::ConfirmDeletePage(page) => {
            ui.heading("Delete page?");
            ui.label("The page and everything on it will be deleted (you can undo this).");
            let mut out = Outcome::Keep;
            ui.horizontal(|ui| {
                if ui.button("Delete").clicked() {
                    app.delete_page(*page);
                    out = Outcome::Close;
                }
                if ui.button("Cancel").clicked() {
                    out = Outcome::Close;
                }
            });
            out
        }
        Dialog::Recovery(items) => {
            ui.heading("Recover unsaved work?");
            ui.label("pubrs did not shut down cleanly. These publications have unsaved changes:");
            for r in items.iter() {
                ui.label(format!("• {} ({})", r.title, r.saved_at));
            }
            let mut out = Outcome::Keep;
            ui.horizontal(|ui| {
                if ui.button("Recover").clicked() {
                    for r in items.iter() {
                        app.recover(r);
                    }
                    out = Outcome::Close;
                }
                if ui.button("Discard").clicked() {
                    for r in items.iter() {
                        app.autosave.discard(r);
                    }
                    out = Outcome::Close;
                }
            });
            out
        }
        Dialog::SaveTemplate(name) => {
            ui.heading("Save as template");
            ui.label("Name (letters, digits, - and _):");
            ui.text_edit_singleline(name);
            ui.small("Tip: mark frames as placeholders so new publications show what to fill in.");
            let mut out = Outcome::Keep;
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    let n = name.clone();
                    app.save_template(&n);
                    out = Outcome::Close;
                }
                if ui.button("Cancel").clicked() {
                    out = Outcome::Close;
                }
            });
            out
        }
        Dialog::ColorScheme => scheme_dialog(app, ui),
        Dialog::Swatch(ed) => swatch_dialog(app, ui, ed),
        Dialog::ExportPdf(st) => export_dialog(app, ui, st),
        Dialog::Message(title, body) => {
            ui.heading(title.as_str());
            ui.label(body.as_str());
            if ui.button("OK").clicked() {
                Outcome::Close
            } else {
                Outcome::Keep
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Colour scheme and swatches

fn scheme_dialog(app: &mut PubrsApp, ui: &mut egui::Ui) -> Outcome {
    ui.heading("Colour scheme");
    ui.label(
        "Applying a scheme recolours everything that uses the “Scheme:” swatches, \
         such as the colours in the built-in templates. Undo restores the old colours.",
    );
    ui.add_space(6.0);
    let current = app.doc().and_then(|d| d.scheme.clone());
    let mut apply = None;
    egui::ScrollArea::vertical()
        .max_height(420.0)
        .show(ui, |ui| {
            for s in pubrs_api::core::swatch::SCHEMES.iter() {
                let is_current = current.as_ref().is_some_and(|c| c.0 == s.id);
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [150.0, 20.0],
                        egui::Label::new(if is_current {
                            RichText::new(format!("{} (in use)", s.name)).strong()
                        } else {
                            RichText::new(s.name)
                        }),
                    );
                    for slot in SchemeSlot::ALL {
                        crate::panels::chip(ui, s.color(slot));
                    }
                    let b = ui
                        .add_enabled(!is_current, egui::Button::new("Apply"))
                        .on_disabled_hover_text("This scheme is already applied")
                        .on_hover_text(format!("Apply the {} colour scheme", s.name));
                    mark(ui, &format!("scheme-apply-{}", s.id), &b);
                    if b.clicked() {
                        apply = Some(s.id);
                    }
                });
            }
        });
    if let Some(id) = apply {
        app.exec(Command::ApplyScheme {
            scheme: SchemeId(id.to_string()),
        });
    }
    ui.add_space(6.0);
    let close = ui.button("Close");
    mark(ui, "scheme-close", &close);
    if close.clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        Outcome::Close
    } else {
        Outcome::Keep
    }
}

fn swatch_dialog(app: &mut PubrsApp, ui: &mut egui::Ui, ed: &mut SwatchEditor) -> Outcome {
    ui.heading(if ed.id.is_some() {
        "Edit swatch"
    } else {
        "New swatch"
    });
    let mut save = false;
    egui::Grid::new("swatch-edit")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("Name");
            let name = ui.text_edit_singleline(&mut ed.name);
            if !ed.focused {
                name.request_focus();
                ed.focused = true;
            }
            if name.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                save = true;
            }
            ui.end_row();
            ui.label("Kind");
            egui::ComboBox::from_id_salt("swatch-kind")
                .selected_text(ed.kind.label())
                .show_ui(ui, |ui| {
                    for k in [SwatchKind::Rgb, SwatchKind::Cmyk, SwatchKind::Spot] {
                        ui.selectable_value(&mut ed.kind, k, k.label());
                    }
                });
            ui.end_row();
            if ed.kind == SwatchKind::Rgb {
                ui.label("Colour");
                ui.color_edit_button_srgba(&mut ed.rgb);
                ui.end_row();
            } else {
                if ed.kind == SwatchKind::Spot {
                    ui.label("Ink name");
                    ui.text_edit_singleline(&mut ed.ink);
                    ui.end_row();
                }
                for (i, label) in ["Cyan", "Magenta", "Yellow", "Black"]
                    .into_iter()
                    .enumerate()
                {
                    ui.label(label);
                    ui.add(
                        egui::DragValue::new(&mut ed.cmyk[i])
                            .range(0.0..=100.0)
                            .speed(0.5)
                            .suffix("%"),
                    );
                    ui.end_row();
                }
            }
        });
    if ed.kind != SwatchKind::Rgb {
        let v = Cmyk::new(ed.cmyk[0], ed.cmyk[1], ed.cmyk[2], ed.cmyk[3]);
        ui.horizontal(|ui| {
            ui.label("On screen");
            crate::panels::chip(ui, v.to_rgb());
        });
        ui.small("CMYK is shown on screen by a simple formula, not colour-managed.");
    }
    if !ed.error.is_empty() {
        ui.colored_label(Color32::from_rgb(0xB9, 0x1C, 0x1C), &ed.error);
    }
    let mut out = Outcome::Keep;
    ui.horizontal(|ui| {
        let ok = ui.button("Save");
        mark(ui, "swatch-save", &ok);
        save |= ok.clicked();
        if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            out = Outcome::Close;
        }
    });
    if save {
        let id = ed
            .id
            .unwrap_or_else(|| pubrs_api::core::SwatchId::new(app.session.ids()));
        let swatch = Swatch {
            id,
            name: ed.name.clone(),
            value: ed.value(),
        };
        let cmd = if ed.id.is_some() {
            Command::UpdateSwatch { swatch }
        } else {
            Command::AddSwatch { swatch }
        };
        app.status = None;
        if app.exec(cmd) {
            out = Outcome::Close;
        } else {
            ed.error = app
                .status
                .take()
                .map(|(m, _)| m)
                .unwrap_or_else(|| "could not save the swatch".into());
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Page Setup

use std::collections::HashMap;

use pubrs_api::core::{
    Cmyk, Color, Command, Document, PageSetup, SchemeId, SchemeSlot, SheetFill, Size, Swatch,
    SwatchValue,
};
use pubrs_api::io::presets::{SheetPreset, sheet_preset, sheet_presets};
use pubrs_api::render::impose::{booklet_plan, sheet_plan};

/// Where the dialog's controls were drawn this frame, by name. The GUI
/// self-test reads this to aim its injected clicks.
const TARGETS: &str = "page-setup-targets";

pub fn target_rect(ctx: &egui::Context, name: &str) -> Option<egui::Rect> {
    ctx.data(|d| {
        d.get_temp::<HashMap<String, egui::Rect>>(egui::Id::new(TARGETS))
            .and_then(|m| m.get(name).copied())
    })
}

pub fn mark(ui: &egui::Ui, name: &str, r: &egui::Response) {
    let rect = r.rect;
    ui.ctx().data_mut(|d| {
        d.get_temp_mut_or_default::<HashMap<String, egui::Rect>>(egui::Id::new(TARGETS))
            .insert(name.to_string(), rect);
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SetupTab {
    Page,
    Labels,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SheetMode {
    Normal,
    Sheet,
    Booklet,
}

/// The Page Setup dialog's edits, applied when the user presses OK.
pub struct PageSetupState {
    pub tab: SetupTab,
    pub mode: SheetMode,
    pub preset: String,
    pub fill: SheetFill,
    /// Resize the page to the label when they differ.
    pub resize: bool,
}

fn sizes_agree(a: Size, b: Size) -> bool {
    (a.width - b.width).abs() <= 0.5 && (a.height - b.height).abs() <= 0.5
}

impl PageSetupState {
    pub fn for_doc(doc: Option<&Document>) -> Option<Self> {
        let d = doc?;
        let fits = sheet_presets()
            .into_iter()
            .find(|p| sizes_agree(p.label, d.setup.size))
            .map_or_else(|| "avery-5160".to_string(), |p| p.id.to_string());
        let (mode, preset, fill) = match (&d.setup.sheet, d.setup.booklet) {
            (Some(s), _) => (SheetMode::Sheet, s.preset.clone(), s.fill),
            (None, true) => (SheetMode::Booklet, fits, SheetFill::Repeat),
            (None, false) => (SheetMode::Normal, fits, SheetFill::Repeat),
        };
        Some(Self {
            tab: SetupTab::Page,
            mode,
            preset,
            fill,
            resize: true,
        })
    }

    fn preset(&self) -> Option<SheetPreset> {
        sheet_preset(&self.preset)
    }

    /// The commands that make the document match this state.
    fn commands(&self, s: &PageSetup) -> Vec<Command> {
        let mut cmds = Vec::new();
        match self.mode {
            SheetMode::Normal => {
                if s.sheet.is_some() {
                    cmds.push(Command::SetSheet { sheet: None });
                }
                if s.booklet {
                    cmds.push(Command::SetBooklet { booklet: false });
                }
            }
            SheetMode::Booklet => {
                if s.sheet.is_some() {
                    cmds.push(Command::SetSheet { sheet: None });
                }
                if !s.booklet {
                    cmds.push(Command::SetBooklet { booklet: true });
                }
            }
            SheetMode::Sheet => {
                let Some(p) = self.preset() else {
                    return cmds;
                };
                if s.booklet {
                    cmds.push(Command::SetBooklet { booklet: false });
                }
                if self.resize && !sizes_agree(p.label, s.size) {
                    cmds.push(Command::SetPageSize { size: p.label });
                }
                let layout = p.layout(self.fill);
                if s.sheet.as_ref() != Some(&layout) {
                    cmds.push(Command::SetSheet {
                        sheet: Some(layout),
                    });
                }
            }
        }
        cmds
    }
}

fn inches(v: f64) -> String {
    format!("{:.3}", v / 72.0)
}

/// One line on how a document will print, for the Print dialog and status
/// bar: `None` for ordinary pages.
pub fn imposition_note(doc: &Document) -> Option<String> {
    imposition_text(&doc.setup, doc.pages.len())
}

fn imposition_text(setup: &PageSetup, n: usize) -> Option<String> {
    use pubrs_api::render::impose::{kind, output_pages};

    let sheets = output_pages(setup, n);
    match kind(setup)? {
        "sheet" => {
            let s = setup.sheet.as_ref()?;
            let name = sheet_preset(&s.preset)
                .map_or_else(|| "a custom sheet".to_string(), |p| p.name.to_string());
            Some(format!(
                "Prints on label sheets: {name}. {sheets} sheet{} ({} labels each).",
                if sheets == 1 { "" } else { "s" },
                s.cells()
            ))
        }
        _ => Some(format!(
            "Prints as a saddle-stitched booklet: {n} page{} padded to {}, {sheets} sheet sides, 2-up. Print double-sided, flip on the short edge.",
            if n == 1 { "" } else { "s" },
            n.div_ceil(4) * 4
        )),
    }
}

fn page_setup_ui(app: &mut PubrsApp, ui: &mut egui::Ui, st: &mut PageSetupState) -> Outcome {
    ui.ctx().data_mut(|d| {
        d.remove::<HashMap<String, egui::Rect>>(egui::Id::new(TARGETS));
    });
    ui.heading("Page Setup");
    ui.horizontal(|ui| {
        let r = ui.selectable_value(&mut st.tab, SetupTab::Page, "Page");
        mark(ui, "tab-page", &r);
        let r = ui.selectable_value(&mut st.tab, SetupTab::Labels, "Labels, Cards, Booklet");
        mark(ui, "tab-labels", &r);
    });
    ui.separator();
    let Some((setup, pages)) = app.doc().map(|d| (d.setup.clone(), d.pages.len())) else {
        return Outcome::Close;
    };
    match st.tab {
        SetupTab::Page => {
            let s = &setup;
            ui.label(format!(
                "Page size: {} \u{d7} {} in",
                inches(s.size.width),
                inches(s.size.height)
            ));
            ui.label(format!(
                "Margins: {} / {} / {} / {} in (top, right, bottom, left)",
                inches(s.margins.top),
                inches(s.margins.right),
                inches(s.margins.bottom),
                inches(s.margins.left)
            ));
            ui.label(format!("Bleed: {} in", inches(s.bleed)));
            ui.small("Change these in the Properties panel with nothing selected.");
            if let Some(note) = imposition_text(&setup, pages) {
                ui.add_space(6.0);
                ui.label(note);
            }
        }
        SetupTab::Labels => {
            let r = ui.radio_value(&mut st.mode, SheetMode::Normal, "Normal pages");
            mark(ui, "mode-normal", &r);
            let r = ui.radio_value(&mut st.mode, SheetMode::Sheet, "Labels or cards on a sheet");
            mark(ui, "mode-sheet", &r);
            let r = ui.radio_value(
                &mut st.mode,
                SheetMode::Booklet,
                "Booklet (saddle stitch, 2-up)",
            );
            mark(ui, "mode-booklet", &r);
            ui.add_space(6.0);
            let sheet_on = st.mode == SheetMode::Sheet;
            let why = "Choose \"Labels or cards on a sheet\" first";
            let presets = sheet_presets();
            let selected = st
                .preset()
                .map_or_else(|| "Choose a sheet".to_string(), |p| p.name.to_string());
            let inner = ui.add_enabled_ui(sheet_on, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Sheet");
                    let combo = egui::ComboBox::from_id_salt("sheet-preset")
                        .width(360.0)
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            for p in &presets {
                                let r = ui.selectable_label(st.preset == p.id, p.name);
                                mark(ui, &format!("preset:{}", p.id), &r);
                                if r.clicked() {
                                    st.preset = p.id.to_string();
                                }
                            }
                        });
                    mark(ui, "preset-combo", &combo.response);
                });
                ui.horizontal(|ui| {
                    ui.label("Fill");
                    let r =
                        ui.radio_value(&mut st.fill, SheetFill::Repeat, "Same page on every label");
                    mark(ui, "fill-repeat", &r);
                    let r = ui.radio_value(&mut st.fill, SheetFill::Sequence, "One page per label");
                    mark(ui, "fill-sequence", &r);
                });
            });
            if !sheet_on {
                inner.response.on_disabled_hover_text(why);
            }
            let mut ok_blocked = None;
            if sheet_on && let Some(p) = st.preset() {
                if sizes_agree(p.label, setup.size) {
                    ui.small(format!(
                        "Each page is one {} \u{d7} {} in label.",
                        inches(p.label.width),
                        inches(p.label.height)
                    ));
                } else {
                    let txt = format!(
                        "Set the page size to {} \u{d7} {} in (now {} \u{d7} {} in)",
                        inches(p.label.width),
                        inches(p.label.height),
                        inches(setup.size.width),
                        inches(setup.size.height)
                    );
                    let r = ui.checkbox(&mut st.resize, txt);
                    mark(ui, "resize", &r);
                    r.on_hover_text("A page is one label, so it must be the label size");
                    if !st.resize {
                        ok_blocked =
                            Some("The page size must equal the label size: tick the box above");
                    }
                }
            }
            if sheet_on && st.preset().is_none() {
                ok_blocked = Some("Choose a sheet first");
            }
            ui.add_space(6.0);
            paint_preview(ui, st, &setup, pages);
            if st.mode == SheetMode::Booklet {
                ui.small(format!(
                    "{pages} page{} padded to {} with blanks, reordered for saddle stitch.",
                    if pages == 1 { "" } else { "s" },
                    pages.div_ceil(4) * 4
                ));
            }
            return page_setup_buttons(app, ui, st, &setup, ok_blocked);
        }
    }
    page_setup_buttons(app, ui, st, &setup, None)
}

fn page_setup_buttons(
    app: &mut PubrsApp,
    ui: &mut egui::Ui,
    st: &PageSetupState,
    setup: &PageSetup,
    blocked: Option<&str>,
) -> Outcome {
    let mut out = Outcome::Keep;
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let ok = ui.add_enabled(blocked.is_none(), egui::Button::new("OK"));
        mark(ui, "ok", &ok);
        let ok = match blocked {
            Some(why) => ok.on_disabled_hover_text(why),
            None => ok,
        };
        if ok.clicked() {
            let mut cmds = st.commands(setup);
            let ran = match cmds.len() {
                0 => true,
                1 => app.exec(cmds.remove(0)),
                _ => app.exec(Command::Batch {
                    label: "page setup".into(),
                    commands: cmds,
                }),
            };
            if ran {
                out = Outcome::Close;
            }
        }
        let c = ui.button("Cancel");
        mark(ui, "cancel", &c);
        if c.clicked() {
            out = Outcome::Close;
        }
    });
    out
}

/// A drawn sheet: the paper outline and its cells, scaled to fit `area`.
fn draw_sheet(
    painter: &egui::Painter,
    visuals: &egui::Visuals,
    area: egui::Rect,
    paper: Size,
    cells: &[(egui::Rect, Option<usize>)],
) {
    let scale = (area.width() / paper.width as f32).min(area.height() / paper.height as f32);
    let origin = area.center() - egui::vec2(paper.width as f32, paper.height as f32) * scale / 2.0;
    let sheet = egui::Rect::from_min_size(
        origin,
        egui::vec2(paper.width as f32, paper.height as f32) * scale,
    );
    painter.rect_filled(sheet, 0.0, Color32::WHITE);
    painter.rect_stroke(
        sheet,
        0.0,
        egui::Stroke::new(1.0, visuals.widgets.noninteractive.fg_stroke.color),
        egui::StrokeKind::Outside,
    );
    for (cell, page) in cells {
        let r = egui::Rect::from_min_size(origin + cell.min.to_vec2() * scale, cell.size() * scale);
        painter.rect_filled(r, 1.0, Color32::from_rgb(222, 235, 250));
        painter.rect_stroke(
            r,
            1.0,
            egui::Stroke::new(1.0, Color32::from_rgb(70, 110, 170)),
            egui::StrokeKind::Inside,
        );
        let label = page.map_or_else(|| "blank".to_string(), |p| (p + 1).to_string());
        let size = (r.height() * 0.5).clamp(6.0, 16.0);
        if r.height() >= 6.0 {
            painter.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(size),
                Color32::from_rgb(30, 50, 90),
            );
        }
    }
}

fn paint_preview(ui: &mut egui::Ui, st: &PageSetupState, setup: &PageSetup, pages: usize) {
    let (resp, painter) = ui.allocate_painter(egui::vec2(480.0, 250.0), egui::Sense::hover());
    let visuals = ui.visuals().clone();
    let area = resp.rect.shrink(4.0);
    let rect_of = |x: f64, y: f64, w: f64, h: f64| {
        egui::Rect::from_min_size(
            egui::pos2(x as f32, y as f32),
            egui::vec2(w as f32, h as f32),
        )
    };
    match st.mode {
        SheetMode::Normal => {
            painter.text(
                area.center(),
                egui::Align2::CENTER_CENTER,
                "Pages print one per sheet",
                egui::FontId::proportional(14.0),
                visuals.text_color(),
            );
        }
        SheetMode::Sheet => {
            let Some(p) = st.preset() else { return };
            let layout = p.layout(st.fill);
            let plan = sheet_plan(&layout, pages.max(1));
            let first = plan.first().cloned().unwrap_or_default();
            let cells: Vec<_> = first
                .into_iter()
                .enumerate()
                .map(|(i, page)| {
                    let o = layout.cell_origin(i);
                    (
                        rect_of(o.x, o.y, layout.label_size.width, layout.label_size.height),
                        page,
                    )
                })
                .collect();
            draw_sheet(&painter, &visuals, area, layout.sheet_size, &cells);
        }
        SheetMode::Booklet => {
            let w = setup.size.width;
            let h = setup.size.height;
            let plan = booklet_plan(pages.max(1));
            let Some(first) = plan.first() else { return };
            let halves = area.width() / 2.0;
            for (i, side) in first.iter().enumerate() {
                let sub = egui::Rect::from_min_size(
                    area.min + egui::vec2(halves * i as f32, 0.0),
                    egui::vec2(halves - 8.0, area.height() - 14.0),
                );
                let cells = [
                    (rect_of(0.0, 0.0, w, h), side[0]),
                    (rect_of(w, 0.0, w, h), side[1]),
                ];
                draw_sheet(&painter, &visuals, sub, Size::new(2.0 * w, h), &cells);
                painter.text(
                    egui::pos2(sub.center().x, area.max.y - 4.0),
                    egui::Align2::CENTER_BOTTOM,
                    if i == 0 {
                        "Sheet 1, front"
                    } else {
                        "Sheet 1, back"
                    },
                    egui::FontId::proportional(12.0),
                    visuals.text_color(),
                );
            }
        }
    }
}

// ---------------------------------------------------------------------
// Paragraph

use pubrs_api::core::{JustifyLimits, ParaAttrs, ParaProps, StoryId};

/// Languages with bundled hyphenation patterns, as (BCP 47 tag, name).
const LANGUAGES: [(&str, &str); 6] = [
    ("en-US", "English (US)"),
    ("de-DE", "German"),
    ("es-ES", "Spanish"),
    ("fr-FR", "French"),
    ("pl-PL", "Polish"),
    ("pt-PT", "Portuguese"),
];

/// The Paragraph dialog's edits, applied (as one undo step) on OK.
pub struct ParagraphState {
    story: StoryId,
    start: usize,
    end: usize,
    initial: ParaAttrs,
    line_spacing: f64,
    space_before: f64,
    space_after: f64,
    first_line_indent: f64,
    left_indent: f64,
    right_indent: f64,
    hyphenate: bool,
    language: String,
    limit: bool,
    word_pct: f64,
    letter_pct: f64,
}

impl ParagraphState {
    /// For the caret's paragraph, the selected text, or the whole story of
    /// the selected text frame.
    pub fn for_selection(app: &PubrsApp) -> Option<Self> {
        let doc = app.doc()?;
        let story = match app.view().and_then(|v| v.edit.as_ref()) {
            Some(e) => e.story,
            None => app.selection().iter().find_map(|f| doc.story_of(*f))?,
        };
        let s = doc.story(story)?;
        let (start, end) = match app.view().and_then(|v| v.edit.as_ref()) {
            Some(e) if e.has_selection() => e.range(),
            Some(e) => (e.caret, e.caret),
            None => (0, s.char_len()),
        };
        let initial = doc.styles.resolve_para(&s.paragraphs[s.locate(start).0]);
        let lim = initial.justify_limits.unwrap_or_default();
        Some(Self {
            story,
            start,
            end,
            line_spacing: initial.line_spacing,
            space_before: initial.space_before,
            space_after: initial.space_after,
            first_line_indent: initial.first_line_indent,
            left_indent: initial.left_indent,
            right_indent: initial.right_indent,
            hyphenate: initial.hyphenate,
            language: initial.language.clone(),
            limit: initial.justify_limits.is_some(),
            word_pct: lim.max_word_space * 100.0,
            letter_pct: lim.letter_spacing * 100.0,
            initial,
        })
    }

    /// The paragraph overrides that differ from what the paragraph has now.
    fn props(&self) -> ParaProps {
        let i = &self.initial;
        let changed = |new: f64, old: f64| ((new - old).abs() > 1e-9).then_some(new);
        let limits = if self.limit {
            let l = JustifyLimits {
                max_word_space: self.word_pct / 100.0,
                letter_spacing: self.letter_pct / 100.0,
                unbounded: false,
            };
            (i.justify_limits != Some(l)).then_some(l)
        } else if i.justify_limits.is_some() {
            // Unticked: override whatever the paragraph or its style set.
            Some(JustifyLimits {
                unbounded: true,
                ..JustifyLimits::default()
            })
        } else {
            None
        };
        ParaProps {
            align: None,
            line_spacing: changed(self.line_spacing, i.line_spacing),
            space_before: changed(self.space_before, i.space_before),
            space_after: changed(self.space_after, i.space_after),
            first_line_indent: changed(self.first_line_indent, i.first_line_indent),
            left_indent: changed(self.left_indent, i.left_indent),
            right_indent: changed(self.right_indent, i.right_indent),
            hyphenate: (self.hyphenate != i.hyphenate).then_some(self.hyphenate),
            language: (self.language != i.language).then(|| self.language.clone()),
            justify_limits: limits,
        }
    }
}

fn paragraph_ui(app: &mut PubrsApp, ui: &mut egui::Ui, st: &mut ParagraphState) -> Outcome {
    ui.ctx().data_mut(|d| {
        d.remove::<HashMap<String, egui::Rect>>(egui::Id::new(TARGETS));
    });
    ui.heading("Paragraph");
    let num = |ui: &mut egui::Ui,
               name: &str,
               label: &str,
               v: &mut f64,
               r: std::ops::RangeInclusive<f64>,
               suffix: &str| {
        ui.label(label);
        let resp = ui.add(egui::DragValue::new(v).speed(0.5).range(r).suffix(suffix));
        mark(ui, name, &resp);
        ui.end_row();
    };
    egui::Grid::new("paragraph-grid")
        .num_columns(2)
        .show(ui, |ui| {
            num(
                ui,
                "line-spacing",
                "Line spacing",
                &mut st.line_spacing,
                0.5..=4.0,
                "",
            );
            num(
                ui,
                "space-before",
                "Space before",
                &mut st.space_before,
                0.0..=200.0,
                " pt",
            );
            num(
                ui,
                "space-after",
                "Space after",
                &mut st.space_after,
                0.0..=200.0,
                " pt",
            );
            num(
                ui,
                "left-indent",
                "Left indent",
                &mut st.left_indent,
                0.0..=500.0,
                " pt",
            );
            num(
                ui,
                "right-indent",
                "Right indent",
                &mut st.right_indent,
                0.0..=500.0,
                " pt",
            );
            num(
                ui,
                "first-indent",
                "First line indent",
                &mut st.first_line_indent,
                -500.0..=500.0,
                " pt",
            );
        });
    ui.separator();
    let r = ui.checkbox(&mut st.hyphenate, "Hyphenate");
    mark(ui, "hyphenate", &r);
    ui.horizontal(|ui| {
        ui.label("Language");
        let name = LANGUAGES
            .iter()
            .find(|(t, _)| *t == st.language)
            .map_or(st.language.as_str(), |(_, n)| *n)
            .to_string();
        let combo = egui::ComboBox::from_id_salt("paragraph-language")
            .selected_text(name)
            .show_ui(ui, |ui| {
                for (tag, name) in LANGUAGES {
                    let r = ui.selectable_label(st.language == tag, name);
                    mark(ui, &format!("lang:{tag}"), &r);
                    if r.clicked() {
                        st.language = tag.to_string();
                    }
                }
            });
        mark(ui, "language", &combo.response);
    });
    if !pubrs_api::core::validate::hyphenation_supported(&st.language) {
        ui.small("This language has no hyphenation patterns; words will not be hyphenated.");
    }
    ui.separator();
    let r = ui.checkbox(&mut st.limit, "Limit word and letter spacing");
    mark(ui, "limit", &r);
    r.on_hover_text("Justified lines that would need more stretch than this are left short");
    if st.limit {
        egui::Grid::new("paragraph-limits")
            .num_columns(2)
            .show(ui, |ui| {
                num(
                    ui,
                    "word-pct",
                    "Word space up to",
                    &mut st.word_pct,
                    100.0..=1000.0,
                    "%",
                );
                num(
                    ui,
                    "letter-pct",
                    "Letter spacing up to",
                    &mut st.letter_pct,
                    0.0..=100.0,
                    "%",
                );
            });
    }
    ui.add_space(8.0);
    let (enter, escape) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
        )
    });
    let editing = ui.ctx().text_edit_focused();
    let mut out = Outcome::Keep;
    ui.horizontal(|ui| {
        let ok = ui.button("OK");
        mark(ui, "ok", &ok);
        if ok.clicked() || (enter && !editing) {
            let props = st.props();
            if props == ParaProps::default()
                || app.exec(Command::ApplyParaProps {
                    story: st.story,
                    start: st.start,
                    end: st.end,
                    props,
                })
            {
                out = Outcome::Close;
            }
        }
        let c = ui.button("Cancel");
        mark(ui, "cancel", &c);
        if c.clicked() || escape {
            out = Outcome::Close;
        }
    });
    out
}

// ---------------------------------------------------------------------------
// Export PDF

/// The presets of the Export PDF dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportPreset {
    Screen,
    Desktop,
    Commercial,
    Archive,
}

impl ExportPreset {
    pub const ALL: [ExportPreset; 4] = [
        ExportPreset::Screen,
        ExportPreset::Desktop,
        ExportPreset::Commercial,
        ExportPreset::Archive,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ExportPreset::Screen => "Screen",
            ExportPreset::Desktop => "Desktop printing",
            ExportPreset::Commercial => "Commercial printing (PDF/X-4)",
            ExportPreset::Archive => "Archive (PDF/A-2b)",
        }
    }

    fn id(self) -> &'static str {
        match self {
            ExportPreset::Screen => "screen",
            ExportPreset::Desktop => "desktop",
            ExportPreset::Commercial => "commercial",
            ExportPreset::Archive => "archive",
        }
    }

    fn about(self) -> &'static str {
        match self {
            ExportPreset::Screen => {
                "For reading on a screen and sharing by email. Tagged for screen readers."
            }
            ExportPreset::Desktop => {
                "For an office or home printer. Plain PDF with fonts embedded."
            }
            ExportPreset::Commercial => {
                "For a print shop: PDF/X-4 with a CMYK output profile, trim and bleed boxes. \
                 The file is checked before it is saved."
            }
            ExportPreset::Archive => "For long-term storage: PDF/A-2b, checked by the PDF writer.",
        }
    }

    /// Bleed and crop marks make sense only for printing.
    fn prints(self) -> bool {
        matches!(self, ExportPreset::Desktop | ExportPreset::Commercial)
    }
}

/// Export PDF dialog state.
pub struct ExportState {
    pub preset: ExportPreset,
    pub bleed: bool,
    pub crop_marks: bool,
    pub tagged: bool,
    /// Whether the publication has a bleed set up.
    pub has_bleed: bool,
    /// Where to save, when already decided (the GUI self-test sets this to
    /// skip the native file dialog).
    pub target: Option<std::path::PathBuf>,
}

impl ExportState {
    pub fn new(doc: &pubrs_api::core::Document) -> Self {
        let mut s = Self {
            preset: ExportPreset::Desktop,
            bleed: false,
            crop_marks: false,
            tagged: false,
            has_bleed: doc.setup.bleed > 0.0,
            target: None,
        };
        s.choose(ExportPreset::Desktop);
        s
    }

    pub fn choose(&mut self, p: ExportPreset) {
        self.preset = p;
        self.bleed = p == ExportPreset::Commercial && self.has_bleed;
        self.crop_marks = p == ExportPreset::Commercial;
        self.tagged = p == ExportPreset::Screen;
    }

    pub fn standard(&self) -> pubrs_api::tools::export::PdfStandardIn {
        use pubrs_api::tools::export::PdfStandardIn as S;
        match self.preset {
            ExportPreset::Commercial => S::Pdfx4,
            ExportPreset::Archive => S::Pdfa2b,
            _ => S::None,
        }
    }
}

fn export_dialog(app: &mut PubrsApp, ui: &mut egui::Ui, st: &mut ExportState) -> Outcome {
    ui.heading("Export PDF");
    ui.label("Choose what the PDF is for. You can still change the options below.");
    ui.add_space(6.0);
    for p in ExportPreset::ALL {
        ui.horizontal(|ui| {
            let r = ui.radio(st.preset == p, p.label());
            mark(ui, &format!("preset-{}", p.id()), &r);
            if r.clicked() {
                st.choose(p);
            }
        });
        ui.indent(p.id(), |ui| ui.small(p.about()));
    }
    ui.add_space(6.0);
    ui.separator();
    let prints = st.preset.prints();
    let bleed_ok = prints && st.has_bleed;
    if !bleed_ok {
        st.bleed = false;
    }
    if !prints {
        st.crop_marks = false;
    }
    let r = ui
        .add_enabled(
            bleed_ok,
            egui::Checkbox::new(&mut st.bleed, "Include bleed"),
        )
        .on_disabled_hover_text(if prints {
            "This publication has no bleed. Set one in Page Setup."
        } else {
            "Bleed is for printing. Choose Desktop or Commercial printing."
        })
        .on_hover_text("Extend artwork past the trim edge into the bleed area");
    mark(ui, "export-bleed", &r);
    let r = ui
        .add_enabled(
            prints,
            egui::Checkbox::new(&mut st.crop_marks, "Crop marks"),
        )
        .on_disabled_hover_text(
            "Crop marks are for printing. Choose Desktop or Commercial printing.",
        )
        .on_hover_text("Draw trim marks outside the bleed area");
    mark(ui, "export-marks", &r);
    let r = ui
        .checkbox(&mut st.tagged, "Tagged PDF (accessibility)")
        .on_hover_text("Mark up text and pictures, with picture alt text, for screen readers");
    mark(ui, "export-tagged", &r);
    ui.add_space(8.0);
    let mut go = false;
    let mut package = false;
    let mut cancel = false;
    ui.horizontal(|ui| {
        let b = ui.button("Export…");
        mark(ui, "export-go", &b);
        go = b.clicked();
        let b = ui.button("Save for a commercial printer…").on_hover_text(
            "A zip with the PDF/X-4 file, the editable publication with its pictures, and a \
                 list of the fonts and colours used",
        );
        mark(ui, "export-package", &b);
        package = b.clicked();
        let b = ui.button("Cancel");
        mark(ui, "export-cancel", &b);
        cancel = b.clicked();
    });
    if cancel || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        return Outcome::Close;
    }
    let name =
        pubrs_api::session::slug(&app.doc().map(|d| d.meta.title.clone()).unwrap_or_default());
    if go {
        let path = st
            .target
            .clone()
            .or_else(|| app.platform.dialogs.save_export(&name, "pdf", "PDF"));
        if let Some(path) = path
            && app.export_pdf_with(&path, st)
        {
            return Outcome::Close;
        }
    }
    if package {
        let path = st.target.clone().or_else(|| {
            app.platform
                .dialogs
                .save_export(&name, "zip", "print package")
        });
        if let Some(path) = path {
            let (bleed, marks) = if st.preset == ExportPreset::Commercial {
                (st.bleed, st.crop_marks)
            } else {
                (st.has_bleed, true)
            };
            if app.export_print_package_to(&path, bleed, marks) {
                return Outcome::Close;
            }
        }
    }
    Outcome::Keep
}

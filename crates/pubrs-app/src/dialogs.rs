//! Modal dialogs.

use eframe::egui::{self, Color32, RichText};
use pubrs_api::core::{DocumentId, PageId, Warning};
use pubrs_api::render::RasterOptions;

use crate::app::PubrsApp;
use crate::autosave::Recoverable;
use crate::platform::{PrintOptions, PrinterInfo};

pub enum Dialog {
    TemplatePicker,
    About,
    McpHelp,
    Print(Vec<PrinterInfo>, PrintOptions),
    ConfirmClose(DocumentId),
    ConfirmQuit,
    ConfirmDeletePage(PageId),
    Recovery(Vec<Recoverable>),
    PageSetup(PageSetupState),
    /// Text → Character…
    Character(CharState),
    Paragraph(ParagraphState),
    DropCap(DropCapState),
    /// Text → Tabs…
    Tabs(TabsState),
    /// Text → Bullets and Numbering…
    Lists(ListState),
    SaveTemplate(String),
    Message(String, String),
    /// After File → Import Publisher File…: what was imported and dropped.
    ImportReport(Box<ImportReportView>),
    /// Design → Colour Scheme…
    ColorScheme,
    /// Add or edit a swatch.
    Swatch(SwatchEditor),
    /// File → Export PDF…
    ExportPdf(ExportState),
    /// Insert → WordArt…
    WordArt(Box<WordArtState>),
    /// Format → Border Art…
    BorderArt(Box<BorderArtState>),
    /// Insert → Table…
    Table(TableState),
    /// Mailings → Mail Merge…
    MergeWizard(Box<MergeWizardState>),
    /// Insert → Merge Field…
    MergeField(MergeFieldState),
}

/// The import report as the dialog shows it.
pub struct ImportReportView {
    pub file: String,
    pub summary: String,
    /// `(kind, count, detail)`: everything dropped or substituted.
    pub entries: Vec<(String, usize, String)>,
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
    if !matches!(app.dialog, Some(Dialog::TemplatePicker)) {
        app.template_list = None;
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
            ..Default::default()
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
            // Building every template (the calendar has twelve pages) is not
            // free: list them once per time the picker is open.
            if app.template_list.is_none() {
                app.template_list = Some(
                    app.session
                        .template_list(Default::default())
                        .map(|t| t.templates)
                        .unwrap_or_default(),
                );
            }
            let templates = app.template_list.clone().unwrap_or_default();
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
            ui.small(
                "Spelling: en_US Hunspell dictionary from SCOWL, by Kevin Atkinson and others.",
            );
            ui.collapsing("Spelling dictionary copyright and licence", |ui| {
                egui::ScrollArea::vertical()
                    .max_height(220.0)
                    .show(ui, |ui| {
                        ui.small(pubrs_api::io::spell::BUNDLED_NOTICE);
                    });
            });
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
        Dialog::Character(state) => char_ui(app, ui, state),
        Dialog::Paragraph(state) => paragraph_ui(app, ui, state),
        Dialog::DropCap(state) => drop_cap_ui(app, ui, state),
        Dialog::WordArt(state) => wordart_ui(app, ui, state),
        Dialog::Table(state) => table_ui(app, ui, state),
        Dialog::BorderArt(state) => border_art_ui(app, ui, state),
        Dialog::MergeWizard(state) => merge_wizard_ui(app, ui, state),
        Dialog::MergeField(state) => merge_field_ui(app, ui, state),
        Dialog::Tabs(state) => tabs_ui(app, ui, state),
        Dialog::Lists(state) => list_ui(app, ui, state),
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
        Dialog::ImportReport(v) => {
            ui.heading("Publisher file imported");
            ui.label(RichText::new(&v.file).strong());
            ui.label(&v.summary);
            ui.add_space(6.0);
            if v.entries.is_empty() {
                ui.colored_label(Color32::DARK_GREEN, "Nothing was dropped.");
            } else {
                ui.label("Not imported, or substituted:");
                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .show(ui, |ui| {
                        for (kind, count, detail) in &v.entries {
                            ui.group(|ui| {
                                ui.label(RichText::new(format!("{kind} × {count}")).strong());
                                ui.small(detail);
                            });
                        }
                    });
            }
            ui.add_space(6.0);
            if ui.button("OK").clicked() {
                Outcome::Close
            } else {
                Outcome::Keep
            }
        }
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

use pubrs_api::core::style::{DEFAULT_WIDOW_ORPHAN, DropCap};
use pubrs_api::core::{JustifyLimits, ParaAttrs, ParaProps, StoryId};

/// The Paragraph dialog's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParaTab {
    Spacing,
    Keeps,
}

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
    pub tab: ParaTab,
    keep_with_next: bool,
    keep_together: bool,
    widow_orphan: bool,
    widow_lines: u8,
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
            tab: ParaTab::Spacing,
            keep_with_next: initial.keep_with_next,
            keep_together: initial.keep_together,
            widow_orphan: initial.widow_orphan >= 2,
            widow_lines: if initial.widow_orphan >= 2 {
                initial.widow_orphan
            } else {
                DEFAULT_WIDOW_ORPHAN
            },
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
            keep_with_next: (self.keep_with_next != i.keep_with_next)
                .then_some(self.keep_with_next),
            keep_together: (self.keep_together != i.keep_together).then_some(self.keep_together),
            widow_orphan: {
                let new = if self.widow_orphan {
                    self.widow_lines
                } else {
                    0
                };
                let old = if i.widow_orphan >= 2 {
                    i.widow_orphan
                } else {
                    0
                };
                (new != old).then_some(new)
            },
            drop_cap: None,
            tab_stops: None,
            list: None,
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
    ui.horizontal(|ui| {
        for (tab, label, name) in [
            (ParaTab::Spacing, "Indents and Spacing", "tab-spacing"),
            (ParaTab::Keeps, "Keeps", "tab-keeps"),
        ] {
            let r = ui.selectable_value(&mut st.tab, tab, label);
            mark(ui, name, &r);
        }
    });
    ui.separator();
    if st.tab == ParaTab::Keeps {
        keeps_ui(ui, st);
    } else {
        spacing_ui(ui, st, num);
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

/// The Keeps tab: how the paragraph breaks across columns and frames.
fn keeps_ui(ui: &mut egui::Ui, st: &mut ParagraphState) {
    let r = ui.checkbox(&mut st.keep_with_next, "Keep with next");
    mark(ui, "keep-with-next", &r);
    r.on_hover_text("The last line stays in the same column or frame as the next paragraph");
    let r = ui.checkbox(&mut st.keep_together, "Keep lines together");
    mark(ui, "keep-together", &r);
    r.on_hover_text("The paragraph is not split across columns or frames when it fits in one");
    ui.horizontal(|ui| {
        let r = ui.checkbox(&mut st.widow_orphan, "Widow and orphan control");
        mark(ui, "widow-orphan", &r);
        r.on_hover_text(
            "No fewer than this many lines alone at the bottom or top of a column or frame",
        );
        let lines = ui.add_enabled(
            st.widow_orphan,
            egui::DragValue::new(&mut st.widow_lines)
                .range(2..=10)
                .suffix(" lines"),
        );
        mark(ui, "widow-lines", &lines);
        lines.on_disabled_hover_text("Turn on widow and orphan control first");
    });
}

/// The Indents and Spacing tab, with hyphenation and justification limits.
fn spacing_ui(
    ui: &mut egui::Ui,
    st: &mut ParagraphState,
    num: impl Fn(&mut egui::Ui, &str, &str, &mut f64, std::ops::RangeInclusive<f64>, &str),
) {
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
}

// ---------------------------------------------------------------------
// Drop cap

/// The Drop Cap dialog's edits for the caret's paragraph, the selected
/// paragraphs, or every paragraph of the selected text box.
pub struct DropCapState {
    story: StoryId,
    start: usize,
    end: usize,
    /// The first paragraph's text, for the preview.
    sample: String,
    on: bool,
    lines: u8,
    chars: u8,
    initial: Option<DropCap>,
}

impl DropCapState {
    pub fn for_selection(app: &PubrsApp) -> Option<Self> {
        let doc = app.doc()?;
        let edit = app.view().and_then(|v| v.edit.as_ref());
        let story = match edit {
            Some(e) => e.story,
            None => app.selection().iter().find_map(|f| doc.story_of(*f))?,
        };
        let s = doc.story(story)?;
        let (start, end) = match edit {
            Some(e) => e.range(),
            None => (0, s.char_len()),
        };
        let para = &s.paragraphs[s.locate(start).0];
        let initial = doc.styles.resolve_para(para).drop_cap;
        let dc = initial.unwrap_or_default();
        Some(Self {
            story,
            start,
            end,
            sample: para.text(),
            // Opening the dialog is asking for a drop cap.
            on: true,
            lines: dc.lines.max(1),
            chars: dc.chars,
            initial,
        })
    }

    /// The override to apply, if anything changed.
    fn props(&self) -> Option<ParaProps> {
        let new = self.on.then_some(DropCap {
            lines: self.lines,
            chars: self.chars,
        });
        (new != self.initial).then(|| ParaProps {
            // `lines: 0` also removes a drop cap the style sets.
            drop_cap: Some(new.unwrap_or(DropCap { lines: 0, chars: 1 })),
            ..Default::default()
        })
    }
}

fn drop_cap_ui(app: &mut PubrsApp, ui: &mut egui::Ui, st: &mut DropCapState) -> Outcome {
    ui.ctx().data_mut(|d| {
        d.remove::<HashMap<String, egui::Rect>>(egui::Id::new(TARGETS));
    });
    ui.heading("Drop Cap");
    let r = ui.checkbox(&mut st.on, "Drop cap");
    mark(ui, "drop-cap", &r);
    r.on_hover_text("Draw the first letters large, sunk into the first lines");
    egui::Grid::new("drop-cap-grid")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("Lines");
            let r = ui.add_enabled(
                st.on,
                egui::DragValue::new(&mut st.lines).range(1..=DropCap::MAX_LINES),
            );
            mark(ui, "drop-lines", &r);
            r.on_hover_text(
                "Lines the letter spans, from the first line's top to the last one's baseline",
            )
            .on_disabled_hover_text("Turn on the drop cap first");
            ui.end_row();
            ui.label("Letters");
            let r = ui.add_enabled(
                st.on,
                egui::DragValue::new(&mut st.chars).range(1..=DropCap::MAX_CHARS),
            );
            mark(ui, "drop-chars", &r);
            r.on_hover_text("How many of the first characters are drawn large")
                .on_disabled_hover_text("Turn on the drop cap first");
            ui.end_row();
        });
    drop_cap_preview(ui, st);
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
            out = match st.props() {
                Some(props) => {
                    if app.exec(Command::ApplyParaProps {
                        story: st.story,
                        start: st.start,
                        end: st.end,
                        props,
                    }) {
                        Outcome::Close
                    } else {
                        Outcome::Keep
                    }
                }
                None => Outcome::Close,
            };
        }
        let c = ui.button("Cancel");
        mark(ui, "cancel", &c);
        if c.clicked() || escape {
            out = Outcome::Close;
        }
    });
    out
}

// ---------------------------------------------------------------------
// Tabs and Bullets and Numbering

use pubrs_api::core::style::{
    List, ListFormat, ListKind, MAX_LIST_LEVEL, TabAlign, TabLeader, TabStop,
};

/// The paragraphs a paragraph dialog edits: the caret's, the selected ones,
/// or every paragraph of the selected text box. Also the box's width in
/// points (the ruler's length).
fn para_target(app: &PubrsApp) -> Option<(StoryId, usize, usize, f64)> {
    let doc = app.doc()?;
    let edit = app.view().and_then(|v| v.edit.as_ref());
    let (story, frame) = match edit {
        Some(e) => (e.story, e.frame),
        None => app
            .selection()
            .iter()
            .find_map(|f| doc.story_of(*f).map(|s| (s, *f)))?,
    };
    let s = doc.story(story)?;
    let (start, end) = match edit {
        Some(e) => e.range(),
        None => (0, s.char_len()),
    };
    let width = doc.frame(frame).map_or(360.0, |f| f.rect.width);
    Some((story, start, end, width))
}

/// The Tabs dialog: a ruler strip over the paragraph's tab stops.
pub struct TabsState {
    story: StoryId,
    start: usize,
    end: usize,
    initial: Vec<TabStop>,
    stops: Vec<TabStop>,
    selected: Option<usize>,
    /// The stop being dragged, and whether it was pulled off the ruler.
    drag: Option<(usize, bool)>,
    /// Ruler length in points.
    width: f64,
    /// Style of the next stop added.
    new_align: TabAlign,
    new_leader: TabLeader,
}

const RULER_PX: f32 = 520.0;
const RULER_H: f32 = 34.0;

impl TabsState {
    pub fn for_selection(app: &PubrsApp) -> Option<Self> {
        let (story, start, end, width) = para_target(app)?;
        let doc = app.doc()?;
        let s = doc.story(story)?;
        let initial = doc
            .styles
            .resolve_para(&s.paragraphs[s.locate(start).0])
            .tab_stops;
        let last = initial.last().map_or(0.0, |t| t.pos);
        Some(Self {
            story,
            start,
            end,
            stops: initial.clone(),
            initial,
            selected: None,
            drag: None,
            width: width.max(last + 36.0).max(72.0),
            new_align: TabAlign::Left,
            new_leader: TabLeader::None,
        })
    }

    fn pos_at(&self, x: f32, left: f32) -> f64 {
        ((f64::from(x - left) * self.width / f64::from(RULER_PX)).round()).clamp(0.0, self.width)
    }

    fn x_of(&self, pos: f64, left: f32) -> f32 {
        left + (pos * f64::from(RULER_PX) / self.width) as f32
    }

    fn hit(&self, x: f32, left: f32) -> Option<usize> {
        self.stops
            .iter()
            .enumerate()
            .map(|(i, s)| (i, (self.x_of(s.pos, left) - x).abs()))
            .filter(|(_, d)| *d <= 7.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// Add a stop in the style of the last one used, and select it.
    fn add(&mut self, pos: f64) {
        if self.stops.len() >= 64 {
            return;
        }
        self.stops.push(TabStop {
            pos,
            align: self.new_align,
            leader: self.new_leader,
        });
        self.sort_keep(self.stops.len() - 1);
    }

    /// Sort the stops by position and keep `sel` selected.
    fn sort_keep(&mut self, sel: usize) {
        let chosen = self.stops[sel];
        self.stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
        self.selected = self.stops.iter().position(|s| *s == chosen);
    }

    fn remove_selected(&mut self) {
        if let Some(i) = self.selected.take() {
            self.stops.remove(i);
        }
    }

    fn changed(&self) -> bool {
        self.stops != self.initial
    }
}

fn align_letter(a: TabAlign) -> &'static str {
    match a {
        TabAlign::Left => "L",
        TabAlign::Center => "C",
        TabAlign::Right => "R",
        TabAlign::Decimal => "D",
    }
}

/// The ruler strip: click to add a stop, drag to move it, drag it off the
/// strip to remove it; with the strip focused, the arrow keys move the
/// selected stop and Delete removes it.
fn tabs_ruler(ui: &mut egui::Ui, st: &mut TabsState) {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(RULER_PX, RULER_H), egui::Sense::click_and_drag());
    mark(ui, "tab-ruler", &resp);
    let resp = resp.on_hover_text(
        "Click to add a tab stop, drag a stop to move it, drag it off the ruler to remove it. With the ruler focused: arrow keys move the selected stop, Delete removes it.",
    );
    let left = rect.left();
    let vis = ui.visuals().clone();
    let p = ui.painter_at(rect.expand2(egui::vec2(0.0, 12.0)));
    p.rect_filled(rect, 2.0, vis.extreme_bg_color);
    p.rect_stroke(
        rect,
        2.0,
        vis.widgets.noninteractive.bg_stroke,
        egui::StrokeKind::Inside,
    );
    let mut t = 0.0;
    while t <= st.width {
        let x = st.x_of(t, left);
        let major = (t % 36.0).abs() < 0.01;
        let h = if major { 10.0 } else { 5.0 };
        p.line_segment(
            [
                egui::pos2(x, rect.bottom()),
                egui::pos2(x, rect.bottom() - h),
            ],
            egui::Stroke::new(1.0, vis.weak_text_color()),
        );
        if major && t > 0.0 {
            p.text(
                egui::pos2(x + 2.0, rect.top() + 2.0),
                egui::Align2::LEFT_TOP,
                format!("{t:.0}"),
                egui::FontId::proportional(9.0),
                vis.weak_text_color(),
            );
        }
        t += 9.0;
    }
    let pointer = ui.input(|i| i.pointer.interact_pos());
    if resp.drag_started()
        && let Some(pt) = pointer
        && let Some(i) = st.hit(pt.x, left)
    {
        st.selected = Some(i);
        st.drag = Some((i, false));
    }
    if resp.dragged()
        && let Some((i, _)) = st.drag
        && let Some(pt) = pointer
    {
        let off = pt.y < rect.top() - 14.0 || pt.y > rect.bottom() + 14.0;
        st.stops[i].pos = st.pos_at(pt.x, left);
        st.drag = Some((i, off));
    }
    if resp.drag_stopped()
        && let Some((i, off)) = st.drag.take()
    {
        if off {
            st.selected = Some(i);
            st.remove_selected();
        } else {
            st.sort_keep(i);
        }
    }
    if resp.clicked()
        && let Some(pt) = pointer
    {
        match st.hit(pt.x, left) {
            Some(i) => st.selected = Some(i),
            None => st.add(st.pos_at(pt.x, left)),
        }
    }
    if resp.has_focus() {
        let (dx, del) = ui.input(|i| {
            let step = if i.modifiers.shift { 10.0 } else { 1.0 };
            (
                f64::from(i.key_pressed(egui::Key::ArrowRight)) * step
                    - f64::from(i.key_pressed(egui::Key::ArrowLeft)) * step,
                i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
            )
        });
        if let Some(i) = st.selected {
            if dx != 0.0 {
                st.stops[i].pos = (st.stops[i].pos + dx).clamp(0.0, st.width);
                st.sort_keep(i);
            }
            if del {
                st.remove_selected();
            }
        }
    }
    for (i, s) in st.stops.iter().enumerate() {
        let off = st.drag == Some((i, true));
        let x = st.x_of(s.pos, left);
        let sel = st.selected == Some(i);
        let col = if off {
            vis.weak_text_color()
        } else if sel {
            vis.selection.stroke.color
        } else {
            vis.text_color()
        };
        p.line_segment(
            [
                egui::pos2(x, rect.top() + 12.0),
                egui::pos2(x, rect.bottom()),
            ],
            egui::Stroke::new(if sel { 2.0 } else { 1.0 }, col),
        );
        p.text(
            egui::pos2(x, rect.top() + 11.0),
            egui::Align2::CENTER_BOTTOM,
            align_letter(s.align),
            egui::FontId::monospace(11.0),
            col,
        );
        if resp.has_focus() && sel {
            p.rect_stroke(
                egui::Rect::from_center_size(
                    egui::pos2(x, rect.center().y),
                    egui::vec2(14.0, 26.0),
                ),
                2.0,
                egui::Stroke::new(1.0, col),
                egui::StrokeKind::Outside,
            );
        }
    }
}

const NO_STOP: &str = "Select a tab stop on the ruler first";

fn tabs_ui(app: &mut PubrsApp, ui: &mut egui::Ui, st: &mut TabsState) -> Outcome {
    ui.ctx().data_mut(|d| {
        d.remove::<HashMap<String, egui::Rect>>(egui::Id::new(TARGETS));
    });
    ui.heading("Tabs");
    ui.label("Tab stops for the selected paragraphs, in points from the left edge of the text.");
    ui.add_space(4.0);
    tabs_ruler(ui, st);
    ui.add_space(6.0);
    let sel = st.selected.filter(|i| *i < st.stops.len());
    let mut stop = sel.map_or(
        TabStop {
            pos: 0.0,
            align: st.new_align,
            leader: st.new_leader,
        },
        |i| st.stops[i],
    );
    let before = stop;
    let max_pos = st.width.max(stop.pos);
    ui.add_enabled_ui(sel.is_some(), |ui| {
        egui::Grid::new("tabs-grid").num_columns(2).show(ui, |ui| {
            ui.label("Position");
            let r = ui.add(
                egui::DragValue::new(&mut stop.pos)
                    .speed(0.5)
                    .range(0.0..=max_pos)
                    .suffix(" pt"),
            );
            mark(ui, "tab-pos", &r);
            r.on_disabled_hover_text(NO_STOP);
            ui.end_row();
            ui.label("Alignment");
            ui.horizontal(|ui| {
                for (a, label, name) in [
                    (TabAlign::Left, "Left", "tab-align-left"),
                    (TabAlign::Center, "Centre", "tab-align-center"),
                    (TabAlign::Right, "Right", "tab-align-right"),
                    (TabAlign::Decimal, "Decimal", "tab-align-decimal"),
                ] {
                    let r = ui.radio_value(&mut stop.align, a, label);
                    mark(ui, name, &r);
                    r.on_disabled_hover_text(NO_STOP);
                }
            });
            ui.end_row();
            ui.label("Leader");
            ui.horizontal(|ui| {
                for (l, label, name) in [
                    (TabLeader::None, "None", "tab-leader-none"),
                    (TabLeader::Dot, "Dots ....", "tab-leader-dot"),
                    (TabLeader::Dash, "Dashes ----", "tab-leader-dash"),
                    (TabLeader::Line, "Line ____", "tab-leader-line"),
                ] {
                    let r = ui.radio_value(&mut stop.leader, l, label);
                    mark(ui, name, &r);
                    r.on_disabled_hover_text(NO_STOP);
                }
            });
            ui.end_row();
        });
    });
    if let Some(i) = sel
        && stop != before
    {
        st.stops[i] = stop;
        st.new_align = stop.align;
        st.new_leader = stop.leader;
        if stop.pos != before.pos {
            st.sort_keep(i);
        }
    }
    ui.horizontal(|ui| {
        let r = ui.button("Add stop");
        mark(ui, "tab-add", &r);
        let r = r.on_hover_text("Add a stop after the last one (the keyboard way to add)");
        if r.clicked() {
            let next = st.stops.last().map_or(36.0, |s| s.pos + 36.0).min(st.width);
            st.add(next);
        }
        let r = ui.add_enabled(sel.is_some(), egui::Button::new("Remove"));
        mark(ui, "tab-remove", &r);
        let r = r.on_disabled_hover_text(NO_STOP);
        if r.clicked() {
            st.remove_selected();
        }
        let r = ui.add_enabled(!st.stops.is_empty(), egui::Button::new("Clear all"));
        mark(ui, "tab-clear", &r);
        let r = r.on_disabled_hover_text("There are no tab stops to clear");
        if r.clicked() {
            st.stops.clear();
            st.selected = None;
        }
    });
    ui.small("Without a stop ahead, a tab goes to the next 36 pt.");
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
            out = if !st.changed()
                || app.exec(Command::ApplyParaProps {
                    story: st.story,
                    start: st.start,
                    end: st.end,
                    props: ParaProps {
                        tab_stops: Some(st.stops.clone()),
                        ..Default::default()
                    },
                }) {
                Outcome::Close
            } else {
                Outcome::Keep
            };
        }
        let c = ui.button("Cancel");
        mark(ui, "cancel", &c);
        if c.clicked() || escape {
            out = Outcome::Close;
        }
    });
    out
}

/// The Bullets and Numbering dialog.
pub struct ListState {
    story: StoryId,
    start: usize,
    end: usize,
    kind: ListKind,
    bullet: String,
    format: ListFormat,
    level: u8,
    indent: f64,
    restart: bool,
    restart_at: u32,
}

impl ListState {
    pub fn for_selection(app: &PubrsApp) -> Option<Self> {
        let (story, start, end, _) = para_target(app)?;
        let doc = app.doc()?;
        let s = doc.story(story)?;
        let list = doc
            .styles
            .resolve_para(&s.paragraphs[s.locate(start).0])
            .list;
        let l = list.unwrap_or_default();
        Some(Self {
            story,
            start,
            end,
            // Opening the dialog is asking for a list.
            kind: if l.kind == ListKind::None {
                ListKind::Bullet
            } else {
                l.kind
            },
            bullet: l.bullet.to_string(),
            format: l.format,
            level: l.level,
            indent: l.indent,
            restart: l.start.is_some(),
            restart_at: l.start.unwrap_or(1),
        })
    }

    fn bullet_char(&self) -> char {
        self.bullet
            .chars()
            .find(|c| !c.is_control() && !c.is_whitespace())
            .unwrap_or(List::default().bullet)
    }

    fn list(&self) -> List {
        List {
            kind: self.kind,
            bullet: self.bullet_char(),
            format: self.format,
            level: self.level,
            indent: self.indent,
            start: None,
        }
    }

    fn preview(&self) -> String {
        match self.kind {
            ListKind::Bullet => self.bullet_char().to_string(),
            ListKind::Number => self
                .format
                .label(if self.restart { self.restart_at } else { 1 }),
            ListKind::None => String::new(),
        }
    }

    /// The commands for OK; empty when nothing would change.
    fn commands(&self, doc: &pubrs_api::core::Document) -> Vec<Command> {
        let Some(s) = doc.story(self.story) else {
            return Vec::new();
        };
        match self.kind {
            ListKind::None => pubrs_api::core::clear_list_commands(
                self.story,
                s,
                &doc.styles,
                self.start,
                self.end,
                false,
            ),
            _ => pubrs_api::core::apply_list_commands(
                self.story,
                s,
                self.start,
                self.end,
                self.list(),
                (f64::from(self.level) + 1.0) * self.indent,
                (self.kind == ListKind::Number && self.restart).then_some(self.restart_at),
            ),
        }
    }
}

fn list_ui(app: &mut PubrsApp, ui: &mut egui::Ui, st: &mut ListState) -> Outcome {
    ui.ctx().data_mut(|d| {
        d.remove::<HashMap<String, egui::Rect>>(egui::Id::new(TARGETS));
    });
    ui.heading("Bullets and Numbering");
    ui.horizontal(|ui| {
        for (k, label, name) in [
            (ListKind::None, "None", "list-none"),
            (ListKind::Bullet, "Bullets", "list-bullet"),
            (ListKind::Number, "Numbers", "list-number"),
        ] {
            let r = ui.radio_value(&mut st.kind, k, label);
            mark(ui, name, &r);
            if k == ListKind::None {
                r.on_hover_text("Turn list items back into plain paragraphs");
            }
        }
    });
    let bullets = st.kind == ListKind::Bullet;
    let numbers = st.kind == ListKind::Number;
    let listed = st.kind != ListKind::None;
    egui::Grid::new("list-grid").num_columns(2).show(ui, |ui| {
        ui.label("Bullet");
        ui.horizontal(|ui| {
            let r = ui.add_enabled(
                bullets,
                egui::TextEdit::singleline(&mut st.bullet).desired_width(36.0),
            );
            mark(ui, "list-bullet-char", &r);
            r.on_disabled_hover_text("Choose Bullets first");
            for c in ["\u{2022}", "\u{2013}", "*", "\u{bb}", "\u{b7}"] {
                let r = ui.add_enabled(bullets, egui::Button::new(c).small());
                let r = r.on_disabled_hover_text("Choose Bullets first");
                if r.clicked() {
                    st.bullet = c.to_string();
                }
            }
        });
        ui.end_row();
        ui.label("Number format");
        let r = ui
            .add_enabled_ui(numbers, |ui| {
                egui::ComboBox::from_id_salt("list-format")
                    .selected_text(st.format.label(1))
                    .show_ui(ui, |ui| {
                        for (f, name) in [
                            (ListFormat::Decimal, "1., 2., 3."),
                            (ListFormat::LowerAlpha, "a., b., c."),
                            (ListFormat::UpperAlpha, "A., B., C."),
                            (ListFormat::LowerRoman, "i., ii., iii."),
                            (ListFormat::UpperRoman, "I., II., III."),
                        ] {
                            ui.selectable_value(&mut st.format, f, name);
                        }
                    });
            })
            .response;
        mark(ui, "list-format", &r);
        if !numbers {
            r.on_hover_text("Choose Numbers first");
        }
        ui.end_row();
        ui.label("Level");
        let r = ui.add_enabled(
            listed,
            egui::DragValue::new(&mut st.level).range(0..=MAX_LIST_LEVEL),
        );
        mark(ui, "list-level", &r);
        r.on_hover_text("Nesting level; a deeper level restarts after a shallower item")
            .on_disabled_hover_text("Choose Bullets or Numbers first");
        ui.end_row();
        ui.label("Indent");
        let r = ui.add_enabled(
            listed,
            egui::DragValue::new(&mut st.indent)
                .speed(0.5)
                .range(0.0..=720.0)
                .suffix(" pt"),
        );
        mark(ui, "list-indent", &r);
        r.on_hover_text("How far the bullet or number hangs left of the text")
            .on_disabled_hover_text("Choose Bullets or Numbers first");
        ui.end_row();
        ui.label("Start at");
        ui.horizontal(|ui| {
            let r = ui.add_enabled(numbers, egui::Checkbox::without_text(&mut st.restart));
            mark(ui, "list-restart", &r);
            r.on_hover_text("Restart numbering at this paragraph")
                .on_disabled_hover_text("Choose Numbers first");
            let r = ui.add_enabled(
                numbers && st.restart,
                egui::DragValue::new(&mut st.restart_at).range(0..=100_000),
            );
            mark(ui, "list-start", &r);
            r.on_disabled_hover_text("Turn on restart numbering first");
        });
        ui.end_row();
    });
    if listed {
        ui.add_space(4.0);
        ui.label(format!("Preview:  {}  Item text", st.preview()));
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
            let commands = app.doc().map(|d| st.commands(d)).unwrap_or_default();
            out = if commands.is_empty()
                || app.exec(Command::Batch {
                    label: "bullets and numbering".into(),
                    commands,
                }) {
                Outcome::Close
            } else {
                Outcome::Keep
            };
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
    /// Issues the checker found before a commercial export (missing fonts,
    /// RGB colour, low-resolution pictures); shown instead of the options
    /// until the user chooses.
    pub blocking: Option<Vec<Warning>>,
    /// The button that was clicked, run again after "Export anyway".
    pub asked: Option<ExportAsk>,
    /// "Export anyway" was chosen: the check is not run again.
    pub acknowledged: bool,
}

/// Which export button started the check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportAsk {
    Pdf,
    Package,
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
            blocking: None,
            asked: None,
            acknowledged: false,
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

/// What the pre-export check found: the list and the three ways out.
fn blocking_view(app: &mut PubrsApp, ui: &mut egui::Ui, st: &mut ExportState) -> Option<Outcome> {
    let list = st.blocking.clone()?;
    ui.heading("Problems found before printing");
    ui.label(
        "A print shop's preflight is likely to stop on these. Fix them first, or export anyway.",
    );
    ui.add_space(4.0);
    egui::ScrollArea::vertical()
        .max_height(260.0)
        .show(ui, |ui| {
            for code in crate::checker::BLOCKING {
                let items: Vec<_> = list.iter().filter(|w| w.code == code).collect();
                if items.is_empty() {
                    continue;
                }
                let n = items.len();
                let (what, why) = match code {
                    pubrs_api::core::WarningCode::RgbInCmykJob => {
                        ("RGB colours", "a CMYK job needs CMYK or spot colours")
                    }
                    pubrs_api::core::WarningCode::MissingFont => {
                        ("Missing fonts", "the printer cannot embed them")
                    }
                    _ => ("Low-resolution pictures", "they print soft below 300 dpi"),
                };
                let unit = if n == 1 { "object" } else { "objects" };
                let title = format!("{what} in {n} {unit} ({why})");
                egui::CollapsingHeader::new(RichText::new(title).strong())
                    .id_salt(("export-block", code))
                    .default_open(n == 1)
                    .show(ui, |ui| {
                        for w in items {
                            ui.group(|ui| {
                                ui.label(&w.message);
                                ui.small(&w.hint);
                            });
                        }
                    });
            }
        });
    ui.add_space(6.0);
    let (mut anyway, mut fix, mut cancel) = (false, false, false);
    ui.horizontal(|ui| {
        let b = ui.button("Export anyway");
        mark(ui, "export-anyway", &b);
        anyway = b.clicked();
        let b = ui.button("Fix first");
        mark(ui, "export-fix", &b);
        fix = b.clicked();
        let b = ui.button("Cancel");
        mark(ui, "export-block-cancel", &b);
        cancel = b.clicked();
    });
    if cancel || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        return Some(Outcome::Close);
    }
    if fix {
        crate::checker::fix_first(app, list.first());
        return Some(Outcome::Close);
    }
    if anyway {
        st.blocking = None;
        st.acknowledged = true;
        return None;
    }
    Some(Outcome::Keep)
}

fn export_dialog(app: &mut PubrsApp, ui: &mut egui::Ui, st: &mut ExportState) -> Outcome {
    // After "Export anyway" the original button's work goes on below.
    let mut resume = None;
    if st.blocking.is_some() {
        match blocking_view(app, ui, st) {
            Some(o) => return o,
            None => resume = st.asked.take(),
        }
    }
    if resume.is_none() {
        return export_options(app, ui, st);
    }
    export_run(
        app,
        st,
        resume == Some(ExportAsk::Pdf),
        resume == Some(ExportAsk::Package),
    )
}

fn export_options(app: &mut PubrsApp, ui: &mut egui::Ui, st: &mut ExportState) -> Outcome {
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
    // A commercial job is checked first; what a printer would reject is
    // listed, with a way to carry on.
    if (go || package) && !st.acknowledged && (st.preset == ExportPreset::Commercial || package) {
        // If the check itself fails the error is shown and the dialog stays.
        let Some(found) = crate::checker::blocking_issues(app) else {
            return Outcome::Keep;
        };
        if !found.is_empty() {
            st.asked = Some(if go {
                ExportAsk::Pdf
            } else {
                ExportAsk::Package
            });
            st.blocking = Some(found);
            return Outcome::Keep;
        }
    }
    export_run(app, st, go, package)
}

/// Save: ask where (unless the self-test decided), then write the file.
fn export_run(app: &mut PubrsApp, st: &mut ExportState, go: bool, package: bool) -> Outcome {
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

/// A sketch of the paragraph: the cap in the dialog's font spanning
/// `lines` lines, with bars for the text beside and below it.
fn drop_cap_preview(ui: &mut egui::Ui, st: &DropCapState) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(240.0, 96.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    let v = ui.visuals();
    p.rect_filled(rect, 4.0, v.extreme_bg_color);
    let pitch = 13.0;
    let inner = rect.shrink(8.0);
    let ink = v.strong_text_color();
    let bar = v.weak_text_color();
    let rows = ((inner.height() / pitch) as usize).max(1);
    let (lines, cap_w) = if st.on {
        let lines = usize::from(st.lines).min(rows);
        let text: String = st.sample.chars().take(usize::from(st.chars)).collect();
        let text = if text.trim().is_empty() {
            "A".to_string()
        } else {
            text
        };
        // Cap height is about 0.7 of the font size.
        let size = 9.0 + (lines as f32 - 1.0) * pitch / 0.7;
        let galley = p.layout_no_wrap(text, egui::FontId::proportional(size), ink);
        let baseline = inner.top() + (lines as f32 - 1.0) * pitch + 10.0;
        let w = galley.size().x;
        let ascent = galley.rows.first().map_or(size, |r| r.row.size.y * 0.8);
        p.galley(egui::pos2(inner.left(), baseline - ascent), galley, ink);
        (lines, w + 4.0)
    } else {
        (0, 0.0)
    };
    for k in 0..rows {
        let y = inner.top() + k as f32 * pitch + 6.0;
        let x0 = inner.left() + if k < lines { cap_w } else { 0.0 };
        let x1 = inner.right() - if k + 1 == rows { 60.0 } else { 0.0 };
        if x1 > x0 {
            p.rect_filled(
                egui::Rect::from_min_max(egui::pos2(x0, y), egui::pos2(x1, y + 3.0)),
                1.0,
                bar,
            );
        }
    }
}

// ---------------------------------------------------------------------
// Character

use pubrs_api::core::{Baseline, Caps, CharAttrs, CharProps};

/// Which baseline choice the Character dialog shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BaselineKind {
    Normal,
    Super,
    Sub,
    Custom,
}

/// The Character dialog's edits for the selected text (or the whole text
/// box), applied as one undo step on OK.
pub struct CharState {
    story: StoryId,
    start: usize,
    end: usize,
    initial: CharAttrs,
    family: String,
    size: f64,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    baseline: BaselineKind,
    /// Points, positive up (used when the baseline is custom).
    shift: f64,
    caps: Caps,
    ligatures: bool,
    kerning: bool,
}

impl CharState {
    /// For the selected text, or all the text of the selected text box.
    /// Without a selection while editing there is nothing to format.
    pub fn for_selection(app: &PubrsApp) -> Option<Self> {
        let doc = app.doc()?;
        let edit = app.view().and_then(|v| v.edit.as_ref());
        let story = match edit {
            Some(e) => e.story,
            None => app.selection().iter().find_map(|f| doc.story_of(*f))?,
        };
        let s = doc.story(story)?;
        let (start, end) = match edit {
            Some(e) if e.has_selection() => e.range(),
            Some(_) => return None,
            None => (0, s.char_len()),
        };
        if end <= start {
            return None;
        }
        let para = &s.paragraphs[s.locate(start).0];
        let fmt = s.format_at(start + 1);
        let run = pubrs_api::core::Run {
            text: String::new(),
            style: fmt.style,
            props: fmt.props,
            field: None,
        };
        let a = doc.styles.resolve_char(para, &run);
        let (baseline, shift) = match a.baseline {
            Baseline::Normal => (BaselineKind::Normal, 0.0),
            Baseline::Superscript => (BaselineKind::Super, 0.0),
            Baseline::Subscript => (BaselineKind::Sub, 0.0),
            Baseline::Shift(v) => (BaselineKind::Custom, v),
        };
        Some(Self {
            story,
            start,
            end,
            family: a.font_family.clone(),
            size: a.font_size,
            bold: a.bold,
            italic: a.italic,
            underline: a.underline,
            strike: a.strikethrough,
            baseline,
            shift,
            caps: a.caps,
            ligatures: a.ligatures,
            kerning: a.kerning,
            initial: a,
        })
    }

    /// The overrides that differ from what the text has now.
    fn props(&self) -> CharProps {
        let i = &self.initial;
        let baseline = match self.baseline {
            BaselineKind::Normal => Baseline::Normal,
            BaselineKind::Super => Baseline::Superscript,
            BaselineKind::Sub => Baseline::Subscript,
            BaselineKind::Custom => Baseline::Shift(self.shift),
        };
        CharProps {
            font_family: (self.family != i.font_family).then(|| self.family.clone()),
            font_size: ((self.size - i.font_size).abs() > 1e-9).then_some(self.size),
            bold: (self.bold != i.bold).then_some(self.bold),
            italic: (self.italic != i.italic).then_some(self.italic),
            underline: (self.underline != i.underline).then_some(self.underline),
            color: None,
            letter_spacing: None,
            baseline: (baseline != i.baseline).then_some(baseline),
            caps: (self.caps != i.caps).then_some(self.caps),
            strikethrough: (self.strike != i.strikethrough).then_some(self.strike),
            ligatures: (self.ligatures != i.ligatures).then_some(self.ligatures),
            kerning: (self.kerning != i.kerning).then_some(self.kerning),
        }
    }
}

fn char_ui(app: &mut PubrsApp, ui: &mut egui::Ui, st: &mut CharState) -> Outcome {
    ui.ctx().data_mut(|d| {
        d.remove::<HashMap<String, egui::Rect>>(egui::Id::new(TARGETS));
    });
    ui.heading("Character");
    egui::Grid::new("char-grid").num_columns(2).show(ui, |ui| {
        ui.label("Font");
        let fonts = app.fonts.clone();
        let r = egui::ComboBox::from_id_salt("char-font")
            .selected_text(st.family.clone())
            .height(300.0)
            .show_ui(ui, |ui| {
                for fam in &fonts {
                    ui.selectable_value(&mut st.family, fam.clone(), fam);
                }
            });
        mark(ui, "char-font", &r.response);
        ui.end_row();
        ui.label("Size");
        let r = ui.add(
            egui::DragValue::new(&mut st.size)
                .speed(0.5)
                .range(1.0..=999.0)
                .suffix(" pt"),
        );
        mark(ui, "char-size", &r);
        ui.end_row();
        ui.label("Style");
        ui.horizontal(|ui| {
            let r = ui.checkbox(&mut st.bold, "Bold");
            mark(ui, "char-bold", &r);
            let r = ui.checkbox(&mut st.italic, "Italic");
            mark(ui, "char-italic", &r);
            let r = ui.checkbox(&mut st.underline, "Underline");
            mark(ui, "char-underline", &r);
            let r = ui.checkbox(&mut st.strike, "Strikethrough");
            mark(ui, "char-strike", &r);
        });
        ui.end_row();
        ui.label("Baseline");
        ui.horizontal(|ui| {
            for (kind, label, name) in [
                (BaselineKind::Normal, "Normal", "char-base-normal"),
                (BaselineKind::Super, "Superscript", "char-base-super"),
                (BaselineKind::Sub, "Subscript", "char-base-sub"),
                (BaselineKind::Custom, "Shift", "char-base-custom"),
            ] {
                let r = ui.radio_value(&mut st.baseline, kind, label);
                mark(ui, name, &r);
            }
            let r = ui.add_enabled(
                st.baseline == BaselineKind::Custom,
                egui::DragValue::new(&mut st.shift)
                    .speed(0.25)
                    .range(-200.0..=200.0)
                    .suffix(" pt"),
            );
            mark(ui, "char-shift", &r);
            r.on_hover_text("Points above the baseline (negative lowers)")
                .on_disabled_hover_text("Choose Shift to enter an amount");
        });
        ui.end_row();
        ui.label("Capitals");
        ui.horizontal(|ui| {
            for (caps, label, name) in [
                (Caps::None, "None", "char-caps-none"),
                (Caps::Small, "Small caps", "char-caps-small"),
                (Caps::All, "All caps", "char-caps-all"),
            ] {
                let r = ui.radio_value(&mut st.caps, caps, label);
                mark(ui, name, &r);
            }
        });
        ui.end_row();
        ui.label("Typography");
        ui.horizontal(|ui| {
            let r = ui.checkbox(&mut st.ligatures, "Ligatures");
            mark(ui, "char-ligatures", &r);
            r.on_hover_text("Join letter pairs such as fi into one glyph where the font has them");
            let r = ui.checkbox(&mut st.kerning, "Kerning");
            mark(ui, "char-kerning", &r);
            r.on_hover_text("Tighten letter pairs such as AV using the font's kerning");
        });
        ui.end_row();
    });
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
            out = if props.is_empty()
                || app.exec(Command::ApplyCharProps {
                    story: st.story,
                    start: st.start,
                    end: st.end,
                    props,
                }) {
                Outcome::Close
            } else {
                Outcome::Keep
            };
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
// WordArt gallery and Border Art picker (M25, ADR 0020)

use pubrs_api::core::{
    BORDER_ART_PATTERNS, BorderArt, DocBuilder, Frame, FrameContent, FrameId, FrameStyle,
    GradientKind, GradientStop, Margins, Paint, Parent, Rect, SeqIds, Shadow, Stroke, Warp,
    WordArtFrame,
};
use pubrs_api::layout::{FontLibrary, Layouter};
use pubrs_api::render::{NoImages, RasterRenderer, build_page_scene};

/// One entry of the WordArt gallery: a warp with a fill, outline and shadow.
pub struct WordArtPreset {
    pub name: &'static str,
    pub warp: Warp,
    pub fill: Paint,
    pub outline: Option<Stroke>,
    pub shadow: Option<Shadow>,
}

fn rgb(r: u8, g: u8, b: u8) -> Paint {
    Paint::Color(Color::rgb(r, g, b))
}

fn gradient2(a: Color, b: Color, angle: f64) -> Paint {
    use pubrs_api::core::SolidPaint;
    Paint::Gradient {
        gradient: GradientKind::Linear,
        stops: vec![
            GradientStop {
                offset: 0.0,
                color: SolidPaint::Color(a),
            },
            GradientStop {
                offset: 1.0,
                color: SolidPaint::Color(b),
            },
        ],
        angle,
    }
}

fn line(r: u8, g: u8, b: u8, width: f64) -> Option<Stroke> {
    Some(Stroke {
        color: rgb(r, g, b),
        width,
        dash: pubrs_api::core::Dash::Solid,
    })
}

fn soft(dx: f64, dy: f64, blur: f64, opacity: f64, color: Paint) -> Option<Shadow> {
    Some(Shadow {
        offset_x: dx,
        offset_y: dy,
        blur,
        color,
        opacity,
    })
}

/// The twelve gallery styles.
pub fn wordart_presets() -> Vec<WordArtPreset> {
    let c = Color::rgb;
    let black = rgb(0, 0, 0);
    vec![
        WordArtPreset {
            name: "Plain",
            warp: Warp::None,
            fill: rgb(30, 58, 138),
            outline: None,
            shadow: None,
        },
        WordArtPreset {
            name: "Arch",
            warp: Warp::ArchUp,
            fill: gradient2(c(250, 204, 21), c(220, 38, 38), 90.0),
            outline: line(60, 20, 20, 1.2),
            shadow: None,
        },
        WordArtPreset {
            name: "Smile",
            warp: Warp::ArchDown,
            fill: rgb(14, 116, 144),
            outline: line(8, 51, 68, 1.0),
            shadow: soft(2.0, 3.0, 3.0, 0.4, black.clone()),
        },
        WordArtPreset {
            name: "Ring",
            warp: Warp::Circle,
            fill: rgb(22, 101, 52),
            outline: None,
            shadow: None,
        },
        WordArtPreset {
            name: "Wave",
            warp: Warp::Wave,
            fill: gradient2(c(244, 114, 182), c(124, 58, 237), 0.0),
            outline: line(76, 29, 149, 1.0),
            shadow: None,
        },
        WordArtPreset {
            name: "Climb",
            warp: Warp::SlantUp,
            fill: rgb(220, 38, 38),
            outline: None,
            shadow: soft(3.0, 3.0, 2.0, 0.5, black.clone()),
        },
        WordArtPreset {
            name: "Fall",
            warp: Warp::SlantDown,
            fill: rgb(234, 88, 12),
            outline: None,
            shadow: soft(2.0, 4.0, 5.0, 0.5, black.clone()),
        },
        WordArtPreset {
            name: "Balloon",
            warp: Warp::Inflate,
            fill: rgb(124, 58, 237),
            outline: line(221, 214, 254, 1.5),
            shadow: soft(3.0, 4.0, 4.0, 0.5, black.clone()),
        },
        WordArtPreset {
            name: "Sunset",
            warp: Warp::None,
            fill: gradient2(c(251, 146, 60), c(219, 39, 119), 90.0),
            outline: line(124, 45, 18, 1.5),
            shadow: soft(3.0, 3.0, 3.0, 0.45, black.clone()),
        },
        WordArtPreset {
            name: "Ocean",
            warp: Warp::Wave,
            fill: gradient2(c(56, 189, 248), c(30, 58, 138), 90.0),
            outline: None,
            shadow: soft(2.0, 4.0, 5.0, 0.5, black.clone()),
        },
        WordArtPreset {
            name: "Neon",
            warp: Warp::ArchUp,
            fill: rgb(34, 211, 238),
            outline: line(14, 116, 144, 2.0),
            shadow: soft(0.0, 0.0, 6.0, 0.9, rgb(34, 211, 238)),
        },
        WordArtPreset {
            name: "Retro",
            warp: Warp::SlantUp,
            fill: rgb(250, 204, 21),
            outline: line(31, 41, 55, 2.0),
            shadow: soft(4.0, 4.0, 0.0, 1.0, black),
        },
    ]
}

impl WordArtPreset {
    /// The WordArt this style makes of `text`.
    pub fn art(&self, text: &str) -> WordArtFrame {
        WordArtFrame {
            text: text.to_string(),
            font: pubrs_api::core::style::DEFAULT_FONT_FAMILY.to_string(),
            size: 96.0,
            warp: self.warp,
            fill: self.fill.clone(),
            outline: self.outline.clone(),
            shadow: self.shadow.clone(),
        }
    }
}

/// Render a preview with the real renderer: `doc_frames` adds frames to a
/// `w` x `h` point page that is rasterised at `scale` pixels per point.
fn render_preview(
    layouter: &mut Layouter,
    (w, h): (f64, f64),
    scale: f64,
    fill: Color,
    frames: impl FnOnce(&mut DocBuilder<SeqIds>, pubrs_api::core::PageId),
) -> Option<egui::ColorImage> {
    let setup = pubrs_api::core::PageSetup {
        size: pubrs_api::core::Size::new(w, h),
        margins: Margins::uniform(0.0),
        bleed: 0.0,
        ..Default::default()
    };
    let mut b = DocBuilder::new(SeqIds::new(11), setup);
    let p = b.page(0);
    frames(&mut b, p);
    let doc = b.finish();
    let layout = layouter.layout_document(&doc);
    let scene = build_page_scene(&doc, &layout, 0)?;
    let pm = RasterRenderer::new()
        .render(
            &scene,
            &NoImages,
            &RasterOptions {
                dpi: 72.0 * scale,
                include_bleed: false,
                background: Some(fill),
            },
        )
        .ok()?;
    Some(egui::ColorImage::from_rgba_premultiplied(
        [pm.width() as usize, pm.height() as usize],
        pm.data(),
    ))
}

/// Insert → WordArt…: the text and the chosen gallery style.
pub struct WordArtState {
    text: String,
    selected: usize,
    presets: Vec<WordArtPreset>,
    previews: Vec<egui::TextureHandle>,
    /// The text the previews show.
    shown: Option<String>,
    layouter: Layouter,
    focused: bool,
}

impl WordArtState {
    pub fn new() -> Self {
        Self {
            text: "Your text".into(),
            selected: 0,
            presets: wordart_presets(),
            previews: Vec::new(),
            shown: None,
            layouter: Layouter::new(FontLibrary::bundled()),
            focused: false,
        }
    }

    /// Render the twelve previews of the current text (when it changed).
    fn refresh(&mut self, ctx: &egui::Context) {
        let text = if self.text.trim().is_empty() {
            "Text".to_string()
        } else {
            self.text.clone()
        };
        if self.shown.as_deref() == Some(&text) {
            return;
        }
        self.previews.clear();
        for (i, p) in self.presets.iter().enumerate() {
            let art = p.art(&text);
            let img = render_preview(
                &mut self.layouter,
                (WORDART_THUMB.0 as f64, WORDART_THUMB.1 as f64),
                2.0,
                Color::rgb(255, 255, 255),
                |b, page| {
                    let _ = b.wordart(
                        page,
                        Rect::new(
                            2.0,
                            2.0,
                            f64::from(WORDART_THUMB.0) - 4.0,
                            f64::from(WORDART_THUMB.1) - 4.0,
                        ),
                        art,
                    );
                },
            );
            if let Some(img) = img {
                self.previews.push(ctx.load_texture(
                    format!("wordart-preview-{i}"),
                    img,
                    egui::TextureOptions::LINEAR,
                ));
            }
        }
        self.shown = Some(text);
    }

    /// Add the chosen style's WordArt, centred on the page shown.
    fn insert(&self, app: &mut PubrsApp) -> bool {
        let Some(doc) = app.doc() else { return false };
        let Some(page) = app.view().and_then(|v| doc.pages.get(v.page)) else {
            return false;
        };
        let size = doc.setup.size;
        let w = (size.width * 0.7).clamp(72.0, 360.0);
        let h = (w * 0.36).max(48.0);
        let rect = Rect::new((size.width - w) / 2.0, (size.height - h) / 2.0, w, h);
        let parent = Parent::Page(page.id);
        let text = if self.text.trim().is_empty() {
            "Your text"
        } else {
            self.text.trim()
        };
        let art = self.presets[self.selected].art(text);
        let id = app.session.new_frame_id();
        let frame = Frame::new(id, rect, FrameContent::WordArt(Box::new(art)));
        let ok = app.exec(Command::AddFrame {
            parent,
            index: None,
            frame,
            story: None,
        });
        if ok {
            app.select(vec![id]);
        }
        ok
    }
}

/// Size of a gallery preview in points.
const WORDART_THUMB: (f32, f32) = (168.0, 64.0);

fn wordart_ui(app: &mut PubrsApp, ui: &mut egui::Ui, st: &mut WordArtState) -> Outcome {
    ui.ctx().data_mut(|d| {
        d.remove::<HashMap<String, egui::Rect>>(egui::Id::new(TARGETS));
    });
    ui.heading("Insert WordArt");
    ui.label("Type your text, then choose a style. You can change everything afterwards.");
    ui.add_space(4.0);
    let r = ui.add(
        egui::TextEdit::singleline(&mut st.text)
            .id_salt("wordart-text")
            .desired_width(WORDART_THUMB.0 * 4.0 + 24.0)
            .char_limit(pubrs_api::core::command::WORDART_MAX_CHARS),
    );
    mark(ui, "wordart-text", &r);
    if !st.focused {
        r.request_focus();
        st.focused = true;
    }
    let editing = ui.ctx().text_edit_focused();
    // Arrow keys move through the gallery unless the text is being edited.
    if !editing {
        let n = st.presets.len();
        ui.input(|i| {
            let step = |k: egui::Key, d: isize, cur: usize| {
                if i.key_pressed(k) {
                    Some(((cur as isize + d).rem_euclid(n as isize)) as usize)
                } else {
                    None
                }
            };
            for (k, d) in [
                (egui::Key::ArrowRight, 1),
                (egui::Key::ArrowLeft, -1),
                (egui::Key::ArrowDown, 4),
                (egui::Key::ArrowUp, -4),
            ] {
                if let Some(s) = step(k, d, st.selected) {
                    st.selected = s;
                }
            }
        });
    }
    st.refresh(ui.ctx());
    ui.add_space(6.0);
    egui::Grid::new("wordart-gallery")
        .num_columns(4)
        .spacing([8.0, 8.0])
        .show(ui, |ui| {
            for i in 0..st.presets.len() {
                ui.vertical(|ui| {
                    if let Some(t) = st.previews.get(i) {
                        let img = egui::Image::new(egui::load::SizedTexture::new(
                            t.id(),
                            egui::vec2(WORDART_THUMB.0, WORDART_THUMB.1),
                        ));
                        let r = ui
                            .add(egui::Button::image(img).selected(st.selected == i))
                            .on_hover_text(st.presets[i].name);
                        mark(ui, &format!("wordart-style-{i}"), &r);
                        if r.clicked() {
                            st.selected = i;
                        }
                    }
                    ui.small(st.presets[i].name);
                });
                if i % 4 == 3 {
                    ui.end_row();
                }
            }
        });
    ui.add_space(8.0);
    let (enter, escape) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
        )
    });
    let mut out = Outcome::Keep;
    ui.horizontal(|ui| {
        let ok = ui.button("Insert");
        mark(ui, "ok", &ok);
        if ok.clicked() || enter {
            out = if st.insert(app) {
                Outcome::Close
            } else {
                Outcome::Keep
            };
        }
        let c = ui.button("Cancel");
        mark(ui, "cancel", &c);
        if c.clicked() || escape {
            out = Outcome::Close;
        }
    });
    out
}

/// Insert → Table…: rows and columns, from a hover grid or typed.
pub struct TableState {
    rows: u32,
    cols: u32,
    focused: bool,
}

/// The hover grid: rows by columns.
const TABLE_GRID: (u32, u32) = (8, 10);

impl TableState {
    pub fn new() -> Self {
        Self {
            rows: 3,
            cols: 3,
            focused: false,
        }
    }

    /// Add the table, centred on the page shown and about two thirds of the
    /// margin box wide, with rows of 24 pt: one undo step. The first cell is
    /// then ready for typing.
    fn insert(&self, app: &mut PubrsApp) -> bool {
        use pubrs_api::tools::tables::TableAdd;
        let Some(doc) = app.doc() else { return false };
        let Some(page) = app.view().and_then(|v| doc.pages.get(v.page)) else {
            return false;
        };
        let (size, m) = (doc.setup.size, doc.setup.margins);
        let page_id = page.id;
        let w = ((size.width - m.left - m.right) * 2.0 / 3.0).max(72.0);
        // Every row shows: the frame is rows x 24 pt, even past the page.
        let h = f64::from(self.rows) * crate::tables::ROW_HEIGHT;
        let y_top = ((size.height - h) / 2.0).max(m.top).max(0.0);
        let req = TableAdd {
            doc_id: app.doc_id(),
            page_id: Some(page_id),
            master_id: None,
            rows: self.rows,
            cols: self.cols,
            x: (size.width - w) / 2.0,
            y: y_top,
            width: w,
            height: h,
            header_fill: None,
            border_width: None,
            border_color: None,
            name: None,
        };
        let r = app.session.table_add(req);
        let Some(out) = app.report(r) else {
            return false;
        };
        app.select(vec![out.frame_id]);
        if let Some(first) = out.story_ids.first() {
            crate::canvas::begin_edit(app, out.frame_id, *first, 0);
        }
        if y_top + h > size.height {
            app.notify("The table runs past the bottom of the page: resize it or use fewer rows");
        }
        true
    }
}

fn table_ui(app: &mut PubrsApp, ui: &mut egui::Ui, st: &mut TableState) -> Outcome {
    ui.ctx().data_mut(|d| {
        d.remove::<HashMap<String, egui::Rect>>(egui::Id::new(TARGETS));
    });
    ui.heading("Insert Table");
    ui.label("Point at the grid and click, or type the numbers.");
    ui.add_space(4.0);
    let cell = 18.0;
    let (grid, _) = ui.allocate_exact_size(
        egui::vec2(
            cell * TABLE_GRID.1 as f32 + 2.0 * (TABLE_GRID.1 as f32 - 1.0),
            cell * TABLE_GRID.0 as f32 + 2.0 * (TABLE_GRID.0 as f32 - 1.0),
        ),
        egui::Sense::hover(),
    );
    let hover = ui.input(|i| i.pointer.hover_pos());
    let mut shown = (st.rows, st.cols);
    let mut clicked = None;
    for r in 0..TABLE_GRID.0 {
        for c in 0..TABLE_GRID.1 {
            let rect = egui::Rect::from_min_size(
                grid.min + egui::vec2(c as f32 * (cell + 2.0), r as f32 * (cell + 2.0)),
                egui::vec2(cell, cell),
            );
            let resp = ui.interact(
                rect,
                egui::Id::new(("table-grid", r, c)),
                egui::Sense::click(),
            );
            mark(ui, &format!("table-grid-{r}-{c}"), &resp);
            if resp.hovered() || hover.is_some_and(|h| rect.contains(h)) {
                shown = (r + 1, c + 1);
            }
            if resp.clicked() {
                clicked = Some((r + 1, c + 1));
            }
        }
    }
    if let Some((r, c)) = clicked {
        st.rows = r;
        st.cols = c;
        shown = (r, c);
    }
    let hovering = hover.is_some_and(|h| grid.contains(h));
    for r in 0..TABLE_GRID.0 {
        for c in 0..TABLE_GRID.1 {
            let rect = egui::Rect::from_min_size(
                grid.min + egui::vec2(c as f32 * (cell + 2.0), r as f32 * (cell + 2.0)),
                egui::vec2(cell, cell),
            );
            let on = r < shown.0 && c < shown.1;
            let fill = if on && hovering {
                Color32::from_rgb(147, 197, 253)
            } else if on {
                Color32::from_rgb(191, 219, 254)
            } else {
                Color32::from_gray(245)
            };
            ui.painter().rect(
                rect,
                1.0,
                fill,
                egui::Stroke::new(1.0, Color32::from_gray(150)),
                egui::StrokeKind::Inside,
            );
        }
    }
    ui.label(if hovering {
        format!("{} \u{d7} {}", shown.0, shown.1)
    } else {
        format!("{} rows \u{d7} {} columns", st.rows, st.cols)
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label("Rows");
        let r = ui.add(egui::DragValue::new(&mut st.rows).range(1..=200));
        mark(ui, "table-rows", &r);
        ui.label("Columns");
        let r = ui.add(egui::DragValue::new(&mut st.cols).range(1..=40));
        mark(ui, "table-cols", &r);
        if !st.focused {
            r.request_focus();
            st.focused = true;
        }
    });
    ui.add_space(8.0);
    let (enter, escape) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
        )
    });
    let mut out = Outcome::Keep;
    ui.horizontal(|ui| {
        let ok = ui.button("Insert");
        mark(ui, "ok", &ok);
        if ok.clicked() || enter {
            out = if st.insert(app) {
                Outcome::Close
            } else {
                Outcome::Keep
            };
        }
        let c = ui.button("Cancel");
        mark(ui, "cancel", &c);
        if c.clicked() || escape {
            out = Outcome::Close;
        }
    });
    out
}

/// Format → Border Art…: the pattern and width for the selected frames.
pub struct BorderArtState {
    frames: Vec<FrameId>,
    /// `None` removes the border.
    pattern: Option<String>,
    width: f64,
    previews: Vec<egui::TextureHandle>,
}

impl BorderArtState {
    pub fn for_selection(app: &PubrsApp, ctx: &egui::Context) -> Option<Self> {
        let doc = app.doc()?;
        let frames: Vec<FrameId> = app
            .selection()
            .into_iter()
            .filter(|f| {
                doc.frame(*f)
                    .is_some_and(|f| f.kind() != pubrs_api::core::FrameKind::Group)
            })
            .collect();
        let first = frames.first().and_then(|f| doc.frame(*f))?;
        let cur = first.style.border_art.clone();
        let mut layouter = Layouter::new(FontLibrary::bundled());
        let mut previews = Vec::new();
        for (i, name) in BORDER_ART_PATTERNS.iter().enumerate() {
            let img = render_preview(
                &mut layouter,
                (BORDER_THUMB, BORDER_THUMB),
                2.0,
                Color::rgb(255, 255, 255),
                |b, page| {
                    let f = b
                        .shape(
                            page,
                            Rect::new(0.0, 0.0, BORDER_THUMB, BORDER_THUMB),
                            pubrs_api::core::Shape::Rectangle { corner_radius: 0.0 },
                        )
                        .ok();
                    if let Some(f) = f {
                        let _ = b.style(
                            f,
                            FrameStyle {
                                border_art: Some(BorderArt {
                                    pattern: (*name).to_string(),
                                    width: 12.0,
                                }),
                                ..Default::default()
                            },
                        );
                    }
                },
            );
            if let Some(img) = img {
                previews.push(ctx.load_texture(
                    format!("border-preview-{i}"),
                    img,
                    egui::TextureOptions::LINEAR,
                ));
            }
        }
        Some(Self {
            frames,
            pattern: cur.as_ref().map(|b| b.pattern.clone()),
            width: cur.map_or(18.0, |b| b.width),
            previews,
        })
    }

    fn apply(&self, app: &mut PubrsApp) -> bool {
        let Some(doc) = app.doc() else { return false };
        let commands: Vec<Command> = self
            .frames
            .iter()
            .filter_map(|id| {
                let mut style = doc.frame(*id)?.style.clone();
                style.border_art = self.pattern.as_ref().map(|p| BorderArt {
                    pattern: p.clone(),
                    width: self.width,
                });
                Some(Command::SetFrameStyle { frame: *id, style })
            })
            .collect();
        app.exec(Command::Batch {
            label: "border art".into(),
            commands,
        })
    }
}

/// Size of a Border Art preview in points.
const BORDER_THUMB: f64 = 64.0;

fn border_art_ui(app: &mut PubrsApp, ui: &mut egui::Ui, st: &mut BorderArtState) -> Outcome {
    ui.ctx().data_mut(|d| {
        d.remove::<HashMap<String, egui::Rect>>(egui::Id::new(TARGETS));
    });
    ui.heading("Border Art");
    ui.label("A decorative border along the inside edge of the selected frames.");
    ui.add_space(4.0);
    let none = ui.selectable_label(st.pattern.is_none(), "None");
    mark(ui, "border-art-none", &none);
    if none.clicked() {
        st.pattern = None;
    }
    egui::Grid::new("border-art-gallery")
        .num_columns(4)
        .spacing([8.0, 8.0])
        .show(ui, |ui| {
            for (i, name) in BORDER_ART_PATTERNS.iter().enumerate() {
                ui.vertical(|ui| {
                    if let Some(t) = st.previews.get(i) {
                        let img = egui::Image::new(egui::load::SizedTexture::new(
                            t.id(),
                            egui::vec2(BORDER_THUMB as f32, BORDER_THUMB as f32),
                        ));
                        let selected = st.pattern.as_deref() == Some(*name);
                        let r = ui
                            .add(egui::Button::image(img).selected(selected))
                            .on_hover_text(*name);
                        mark(ui, &format!("border-art-{name}"), &r);
                        if r.clicked() {
                            st.pattern = Some((*name).to_string());
                        }
                    }
                    ui.small(*name);
                });
                if i % 4 == 3 {
                    ui.end_row();
                }
            }
        });
    ui.horizontal(|ui| {
        ui.label("Width");
        let r = ui.add(
            egui::DragValue::new(&mut st.width)
                .range(pubrs_api::core::BORDER_ART_WIDTH)
                .speed(0.5)
                .suffix(" pt"),
        );
        mark(ui, "border-art-width", &r);
        r.on_hover_text("How wide the border band is; text inside is kept clear of it");
    });
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
            out = if st.apply(app) {
                Outcome::Close
            } else {
                Outcome::Keep
            };
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
// Mail merge (M24, ADR 0023): Mailings ▸ Mail Merge… and Insert ▸ Merge Field…

use pubrs_api::tools::merge::{MergeExport, MergeFormat, MergeSetSource};

/// The steps of the Mail Merge wizard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeStep {
    Data,
    Fields,
    Preview,
    Finish,
}

impl MergeStep {
    const ALL: [(MergeStep, &'static str); 4] = [
        (MergeStep::Data, "1  Data"),
        (MergeStep::Fields, "2  Fields"),
        (MergeStep::Preview, "3  Preview"),
        (MergeStep::Finish, "4  Finish"),
    ];

    fn next(self) -> Option<MergeStep> {
        let i = Self::ALL.iter().position(|(s, _)| *s == self)?;
        Self::ALL.get(i + 1).map(|(s, _)| *s)
    }

    fn previous(self) -> Option<MergeStep> {
        let i = Self::ALL.iter().position(|(s, _)| *s == self)?;
        i.checked_sub(1).map(|i| Self::ALL[i].0)
    }
}

/// State of the Mail Merge wizard.
pub struct MergeWizardState {
    pub step: MergeStep,
    /// The data file typed or chosen.
    pub path: String,
    /// Sheet names of the chosen .xlsx file.
    pub sheets: Vec<String>,
    pub sheet: Option<String>,
    pub selected: usize,
    pub error: String,
    focused: bool,
}

impl MergeWizardState {
    pub fn new(app: &PubrsApp) -> Self {
        let remembered = app
            .doc_id()
            .and_then(|d| app.session.get(Some(d)).ok())
            .and_then(|o| o.merge_path.clone());
        let has_data = app.doc().is_some_and(|d| d.merge.is_some());
        let mut st = Self {
            step: if has_data {
                MergeStep::Fields
            } else {
                MergeStep::Data
            },
            path: String::new(),
            sheets: Vec::new(),
            sheet: None,
            selected: 0,
            error: String::new(),
            focused: false,
        };
        if let Some((p, sheet)) = remembered {
            st.path = p.display().to_string();
            st.sheet = sheet;
            st.look_at_file();
        }
        st
    }

    /// Read the sheet names of the typed path.
    fn look_at_file(&mut self) {
        self.sheets =
            pubrs_api::io::merge_source::sheet_names_of(std::path::Path::new(self.path.trim()))
                .unwrap_or_default();
        if self
            .sheet
            .as_ref()
            .is_some_and(|s| !self.sheets.contains(s))
        {
            self.sheet = None;
        }
    }

    /// Load the typed path into the document.
    fn load(&mut self, app: &mut PubrsApp) -> bool {
        let r = app.session.merge_set_source(MergeSetSource {
            doc_id: app.doc_id(),
            path: Some(self.path.trim().to_string()),
            sheet: self.sheet.clone(),
            ..Default::default()
        });
        match r {
            Ok(out) => {
                self.error.clear();
                app.notify(format!(
                    "Loaded {} records with {} fields from {}",
                    out.record_count,
                    out.fields.len(),
                    out.source_name
                ));
                self.selected = 0;
                true
            }
            Err(e) => {
                self.error = format!("{} {}", e.message, e.hint);
                false
            }
        }
    }
}

/// What the document's merge data looks like to the dialogs.
struct MergeInfo {
    source: String,
    fields: Vec<(String, String)>,
    records: usize,
    index: usize,
    preview: bool,
}

fn merge_info(app: &PubrsApp) -> Option<MergeInfo> {
    let m = app.doc()?.merge.as_ref()?;
    let idx = m.current_index();
    Some(MergeInfo {
        source: match &m.sheet {
            Some(s) => format!("{} ({s})", m.source_name),
            None => m.source_name.clone(),
        },
        fields: m
            .fields
            .iter()
            .map(|f| (f.clone(), m.value_in(idx, f).unwrap_or("").to_string()))
            .collect(),
        records: m.record_count(),
        index: idx,
        preview: m.preview,
    })
}

/// The caret of the text being edited, if there is one.
fn merge_caret(app: &PubrsApp) -> Option<crate::textedit::TextEdit> {
    app.view().and_then(|v| v.edit.clone())
}

impl PubrsApp {
    /// Insert the merge field `name` at the caret (replacing a selection)
    /// in one undo step. Returns whether it was inserted.
    pub fn insert_merge_field(&mut self, name: &str) -> bool {
        let Some(e) = merge_caret(self) else {
            self.notify("Click in a text box first, then insert the field");
            return false;
        };
        crate::canvas::edit_insert_field(self, &e, name)
    }

    /// Mailings ▸ Refresh Data: read the data file again.
    pub fn refresh_merge_data(&mut self) {
        let r = self.session.merge_set_source(MergeSetSource {
            doc_id: self.doc_id(),
            refresh: true,
            ..Default::default()
        });
        if let Some(out) = self.report(r) {
            self.notify(format!(
                "Refreshed {}: {} records",
                out.source_name, out.record_count
            ));
        }
    }

    /// Change the previewed record and switch (one undo step while the user
    /// keeps stepping).
    pub fn merge_preview(&mut self, index: Option<usize>, on: Option<bool>) {
        self.exec_merge(
            pubrs_api::core::Command::SetMergePreview { index, preview: on },
            "merge-preview",
        );
    }

    /// Merge to a PDF file with a page set per record.
    pub fn merge_to_pdf(&mut self, path: &std::path::Path) -> bool {
        let r = self.session.merge_export(MergeExport {
            doc_id: self.doc_id(),
            format: MergeFormat::Pdf,
            path: Some(
                std::path::absolute(path)
                    .unwrap_or_else(|_| path.to_path_buf())
                    .display()
                    .to_string(),
            ),
            ..Default::default()
        });
        match self.report(r) {
            Some(out) => {
                self.notify(format!(
                    "Merged {} records into {} ({} pages)",
                    out.record_count,
                    path.display(),
                    out.page_count
                ));
                true
            }
            None => false,
        }
    }

    /// Merge to a new, unsaved publication shown in its own tab.
    pub fn merge_to_publication(&mut self) -> bool {
        let Some(doc) = self.doc() else { return false };
        match pubrs_api::core::merge::merge_to_publication(doc) {
            Ok(merged) => {
                let pages = merged.pages.len();
                let id = self.session.insert(merged, None);
                self.add_view(id);
                self.notify(format!("Merged into a new publication of {pages} pages"));
                true
            }
            Err(e) => {
                self.notify(format!("{e} — {}", e.hint()));
                false
            }
        }
    }

    /// Merge to a temporary PDF and send it to the printer.
    pub fn merge_print(&mut self) {
        let tmp = std::env::temp_dir().join(format!("pubrs-merge-{}.pdf", std::process::id()));
        if !self.merge_to_pdf(&tmp) {
            return;
        }
        let title = self.doc().map(|d| d.meta.title.clone()).unwrap_or_default();
        match self
            .platform
            .print
            .print_pdf(&tmp, &title, &PrintOptions::default())
        {
            Ok(_) => self.notify("Merged pages sent to the printer"),
            Err(e) => self.dialog = Some(Dialog::Message("Printing failed".into(), e)),
        }
    }
}

fn merge_wizard_ui(app: &mut PubrsApp, ui: &mut egui::Ui, st: &mut MergeWizardState) -> Outcome {
    ui.ctx().data_mut(|d| {
        d.remove::<HashMap<String, egui::Rect>>(egui::Id::new(TARGETS));
    });
    ui.set_min_width(460.0);
    ui.heading("Mail Merge");
    ui.horizontal(|ui| {
        for (i, (step, label)) in MergeStep::ALL.iter().enumerate() {
            let r = ui.selectable_label(st.step == *step, *label);
            mark(ui, &format!("merge-step-{}", i + 1), &r);
            if r.clicked() {
                st.step = *step;
            }
        }
    });
    ui.separator();
    let info = merge_info(app);
    let mut out = Outcome::Keep;
    match st.step {
        MergeStep::Data => {
            ui.label(
                "Choose the list to merge: a .csv or .xlsx file. The first row names the fields.",
            );
            let mut path_enter = false;
            ui.horizontal(|ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut st.path)
                        .id_salt("merge-path")
                        .hint_text("path of a .csv or .xlsx file")
                        .desired_width(300.0),
                );
                mark(ui, "merge-path", &r);
                if !st.focused {
                    r.request_focus();
                    st.focused = true;
                }
                if r.changed() {
                    st.look_at_file();
                }
                path_enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let choose = ui.button("Choose file…");
                mark(ui, "merge-choose", &choose);
                if choose.clicked()
                    && let Some(p) = app.platform.dialogs.pick_data()
                {
                    st.path = p.display().to_string();
                    st.sheet = None;
                    st.look_at_file();
                }
            });
            if st.sheets.len() > 1 {
                ui.horizontal(|ui| {
                    ui.label("Sheet");
                    let cur = st.sheet.clone().unwrap_or_else(|| st.sheets[0].clone());
                    let cb = egui::ComboBox::from_id_salt("merge-sheet")
                        .selected_text(&cur)
                        .show_ui(ui, |ui| {
                            for (i, s) in st.sheets.iter().enumerate() {
                                let r = ui.selectable_label(cur == *s, s);
                                mark(ui, &format!("merge-sheet-{i}"), &r);
                                if r.clicked() {
                                    st.sheet = Some(s.clone());
                                }
                            }
                        });
                    mark(ui, "merge-sheet", &cb.response);
                });
            }
            ui.horizontal(|ui| {
                let can = !st.path.trim().is_empty();
                let load = ui.add_enabled(can, egui::Button::new("Load"));
                mark(ui, "merge-load", &load);
                if can && (load.clicked() || path_enter) && st.load(app) {
                    st.step = MergeStep::Fields;
                }
                if info.is_some()
                    && app
                        .session
                        .get(app.doc_id())
                        .is_ok_and(|o| o.merge_path.is_some())
                {
                    let refresh = ui.button("Refresh data");
                    mark(ui, "merge-refresh", &refresh);
                    if refresh.clicked() {
                        app.refresh_merge_data();
                    }
                }
            });
            if !st.error.is_empty() {
                ui.colored_label(Color32::DARK_RED, &st.error);
            }
            if let Some(i) = &info {
                ui.add_space(4.0);
                ui.label(format!(
                    "Loaded: {} — {} records, {} fields",
                    i.source,
                    i.records,
                    i.fields.len()
                ));
            }
        }
        MergeStep::Fields => match &info {
            None => {
                ui.label("There is no data yet. Choose a file on the Data step.");
            }
            Some(i) => {
                ui.label("Click in the text where the field goes, then choose it here. Fields look like this in the text: «Name».");
                let caret = merge_caret(app).is_some();
                let mut insert: Option<String> = None;
                // Arrow keys move through the list; Enter inserts.
                let n = i.fields.len();
                st.selected = st.selected.min(n.saturating_sub(1));
                if !ui.ctx().text_edit_focused() {
                    ui.input(|inp| {
                        if inp.key_pressed(egui::Key::ArrowDown) {
                            st.selected = (st.selected + 1) % n.max(1);
                        }
                        if inp.key_pressed(egui::Key::ArrowUp) {
                            st.selected = (st.selected + n.max(1) - 1) % n.max(1);
                        }
                    });
                }
                egui::ScrollArea::vertical()
                    .max_height(220.0)
                    .show(ui, |ui| {
                        for (k, (name, value)) in i.fields.iter().enumerate() {
                            let text = if value.is_empty() {
                                name.clone()
                            } else {
                                format!("{name}    {value}")
                            };
                            let r = ui.selectable_label(st.selected == k, text);
                            mark(ui, &format!("merge-field-{k}"), &r);
                            if r.clicked() {
                                st.selected = k;
                            }
                            if r.double_clicked() {
                                insert = Some(name.clone());
                            }
                        }
                    });
                ui.horizontal(|ui| {
                    let b = ui.add_enabled(caret, egui::Button::new("Insert at cursor"));
                    mark(ui, "merge-insert", &b);
                    let b = if caret {
                        b
                    } else {
                        b.on_disabled_hover_text(
                            "Close this window, click in a text box, and open it again",
                        )
                    };
                    let enter = ui.input(|inp| inp.key_pressed(egui::Key::Enter));
                    if (b.clicked() || (enter && caret))
                        && let Some((name, _)) = i.fields.get(st.selected)
                    {
                        insert = Some(name.clone());
                    }
                });
                if !caret {
                    ui.small("No text cursor: close this window, click in a text box, and open it again.");
                }
                if let Some(name) = insert
                    && app.insert_merge_field(&name)
                {
                    app.notify(format!("Inserted the field «{name}»"));
                }
            }
        },
        MergeStep::Preview => match &info {
            None => {
                ui.label("There is no data yet. Choose a file on the Data step.");
            }
            Some(i) => {
                let mut on = i.preview;
                let c = ui.checkbox(&mut on, "Show the data in the text");
                mark(ui, "merge-preview-on", &c);
                if c.changed() {
                    app.merge_preview(None, Some(on));
                }
                ui.horizontal(|ui| {
                    let last = i.records.saturating_sub(1);
                    let prev = ui.add_enabled(i.index > 0, egui::Button::new("◀"));
                    mark(ui, "merge-prev", &prev);
                    let mut shown = i.index + 1;
                    let dv = ui.add(
                        egui::DragValue::new(&mut shown)
                            .range(1..=i.records.max(1))
                            .speed(0.2),
                    );
                    mark(ui, "merge-index", &dv);
                    let next = ui.add_enabled(i.index < last, egui::Button::new("▶"));
                    mark(ui, "merge-next", &next);
                    ui.label(format!("of {}", i.records));
                    let arrows = !ui.ctx().text_edit_focused() && !dv.has_focus();
                    let (left, right) = ui.input(|inp| {
                        (
                            arrows && inp.key_pressed(egui::Key::ArrowLeft),
                            arrows && inp.key_pressed(egui::Key::ArrowRight),
                        )
                    });
                    let mut to = None;
                    if (prev.clicked() || left) && i.index > 0 {
                        to = Some(i.index - 1);
                    }
                    if (next.clicked() || right) && i.index < last {
                        to = Some(i.index + 1);
                    }
                    if dv.changed() {
                        to = Some(shown.saturating_sub(1).min(last));
                    }
                    if let Some(t) = to {
                        app.merge_preview(Some(t), Some(true));
                    }
                });
                ui.small("Left and right arrow keys step through the records.");
                ui.add_space(4.0);
                for (name, value) in &i.fields {
                    ui.label(format!("{name}: {value}"));
                }
            }
        },
        MergeStep::Finish => match &info {
            None => {
                ui.label("There is no data yet. Choose a file on the Data step.");
            }
            Some(i) => {
                ui.label(format!(
                    "{} records. Each record gets its own copy of the pages.",
                    i.records
                ));
                if let Some(note) = app.doc().and_then(imposition_note) {
                    ui.small(format!("{note} A sequence label sheet gets one record per label; otherwise each record is one sheet."));
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let pdf = ui.button("Merge to PDF…");
                    mark(ui, "merge-pdf", &pdf);
                    if pdf.clicked() {
                        let name = app
                            .doc()
                            .map(|d| format!("{}-merged", pubrs_api::session::slug(&d.meta.title)))
                            .unwrap_or_else(|| "merged".into());
                        if let Some(p) =
                            app.platform.dialogs.save_export(&name, "pdf", "merged PDF")
                            && app.merge_to_pdf(&p)
                        {
                            out = Outcome::Close;
                        }
                    }
                    let publ = ui.button("Merge to a new publication");
                    mark(ui, "merge-publication", &publ);
                    if publ.clicked() && app.merge_to_publication() {
                        out = Outcome::Close;
                    }
                    let print = ui.button("Print…");
                    mark(ui, "merge-print", &print);
                    if print.clicked() {
                        app.merge_print();
                        if app.dialog.is_none() {
                            out = Outcome::Close;
                        }
                    }
                });
            }
        },
    }
    ui.separator();
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    ui.horizontal(|ui| {
        let back = ui.add_enabled(st.step.previous().is_some(), egui::Button::new("Back"));
        mark(ui, "merge-back", &back);
        if back.clicked()
            && let Some(p) = st.step.previous()
        {
            st.step = p;
        }
        let next = ui.add_enabled(
            st.step.next().is_some() && info.is_some(),
            egui::Button::new("Next"),
        );
        mark(ui, "merge-next-step", &next);
        if next.clicked()
            && let Some(n) = st.step.next()
        {
            st.step = n;
        }
        let close = ui.button("Close");
        mark(ui, "merge-close", &close);
        if close.clicked() || escape {
            out = Outcome::Close;
        }
    });
    out
}

/// Insert ▸ Merge Field…: pick one of the data's fields for the caret.
pub struct MergeFieldState {
    selected: usize,
}

impl MergeFieldState {
    pub fn new() -> Self {
        Self { selected: 0 }
    }
}

fn merge_field_ui(app: &mut PubrsApp, ui: &mut egui::Ui, st: &mut MergeFieldState) -> Outcome {
    ui.ctx().data_mut(|d| {
        d.remove::<HashMap<String, egui::Rect>>(egui::Id::new(TARGETS));
    });
    ui.heading("Insert merge field");
    let Some(i) = merge_info(app) else {
        ui.label("There is no merge data. Use Mailings ▸ Mail Merge… to choose a list.");
        return if ui.button("Close").clicked() {
            Outcome::Close
        } else {
            Outcome::Keep
        };
    };
    ui.label("Choose a field and press Enter. It is inserted at the text cursor.");
    let n = i.fields.len().max(1);
    st.selected = st.selected.min(n - 1);
    let (down, up, enter, escape) = ui.input(|inp| {
        (
            inp.key_pressed(egui::Key::ArrowDown),
            inp.key_pressed(egui::Key::ArrowUp),
            inp.key_pressed(egui::Key::Enter),
            inp.key_pressed(egui::Key::Escape),
        )
    });
    if down {
        st.selected = (st.selected + 1) % n;
    }
    if up {
        st.selected = (st.selected + n - 1) % n;
    }
    let mut pick: Option<String> = None;
    egui::ScrollArea::vertical()
        .max_height(260.0)
        .show(ui, |ui| {
            for (k, (name, value)) in i.fields.iter().enumerate() {
                let label = if value.is_empty() {
                    name.clone()
                } else {
                    format!("{name}    {value}")
                };
                let r = ui.selectable_label(st.selected == k, label);
                mark(ui, &format!("merge-field-{k}"), &r);
                if r.clicked() {
                    st.selected = k;
                }
                if r.double_clicked() {
                    pick = Some(name.clone());
                }
            }
        });
    let mut out = Outcome::Keep;
    ui.horizontal(|ui| {
        let ok = ui.button("Insert");
        mark(ui, "ok", &ok);
        if ok.clicked() || enter {
            pick = i.fields.get(st.selected).map(|f| f.0.clone());
        }
        let c = ui.button("Cancel");
        mark(ui, "cancel", &c);
        if c.clicked() || escape {
            out = Outcome::Close;
        }
    });
    if let Some(name) = pick
        && app.insert_merge_field(&name)
    {
        out = Outcome::Close;
    }
    out
}

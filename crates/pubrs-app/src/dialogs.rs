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
    SaveTemplate(String),
    Message(String, String),
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
// Page Setup

use std::collections::HashMap;

use pubrs_api::core::{Command, Document, PageSetup, SheetFill, Size};
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

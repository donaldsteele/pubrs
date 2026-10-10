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
        Dialog::Print(printers, options) => {
            ui.heading("Print");
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

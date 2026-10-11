//! Menu bar, toolbar, status bar, pages panel and properties panel.

use eframe::egui::{self, Color32, RichText};
use pubrs_api::core::{
    Align, BORDER_ART_PATTERNS, BORDER_ART_WIDTH, BorderArt, CharProps, Color, Command, Crop, Dash,
    Document, FrameContent, FrameId, FrameKind, GradientKind, GradientStop, ImageFit, Margins,
    PageSizePreset, Paint, ParaProps, Rect, Shadow, Shape, Size, SolidPaint, Stroke, SwatchValue,
    TextFrameProps, VerticalAlign, Warp, WordArtFrame, WrapMode,
};

use crate::app::{PubrsApp, Tool, Units};
use crate::dialogs::{Dialog, SwatchEditor, SwatchKind};
use crate::keymap::{self, Action, MenuItem};

fn menu_items(app: &mut PubrsApp, ui: &mut egui::Ui, items: &[MenuItem]) {
    for item in items {
        match item {
            MenuItem::Action(label, action) => {
                let mut b = egui::Button::new(*label);
                if let Some(sc) = keymap::shortcut(*action) {
                    b = b.shortcut_text(ui.ctx().format_shortcut(&sc));
                }
                let why = app.unavailable(*action);
                let r = ui.add_enabled(why.is_none(), b);
                let r = match why {
                    Some(why) => r.on_disabled_hover_text(why),
                    None => r,
                };
                if r.clicked() {
                    let ctx = ui.ctx().clone();
                    app.run_action(&ctx, *action);
                    ui.close();
                }
            }
            MenuItem::Separator => {
                ui.separator();
            }
            MenuItem::Submenu(title, sub) => {
                ui.menu_button(*title, |ui| menu_items(app, ui, sub));
            }
        }
    }
}

pub fn menu_bar(app: &mut PubrsApp, ui: &mut egui::Ui) {
    egui::Panel::top("menu").show(ui, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            for m in keymap::MENUS {
                ui.menu_button(m.title, |ui| menu_items(app, ui, m.items));
            }
        });
    });
}

pub fn toolbar(app: &mut PubrsApp, ui: &mut egui::Ui) {
    egui::Panel::top("toolbar").show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for (tool, label, tip) in [
                (Tool::Select, "⬉ Select", "Select, move and resize (Esc)"),
                (Tool::Text, "T Text", "Draw a text box"),
                (
                    Tool::Picture,
                    "🖼 Picture",
                    "Draw a picture frame, then choose a picture",
                ),
                (Tool::Rectangle, "Rectangle", "Draw a rectangle"),
                (Tool::Ellipse, "Ellipse", "Draw an ellipse"),
                (Tool::Line, "Line", "Draw a line"),
            ] {
                if ui
                    .selectable_label(app.tool == tool, label)
                    .on_hover_text(tip)
                    .clicked()
                {
                    app.tool = tool;
                }
            }
            ui.separator();
            let ctx = ui.ctx().clone();
            if ui.button("Undo").clicked() {
                app.run_action(&ctx, Action::Undo);
            }
            if ui.button("Redo").clicked() {
                app.run_action(&ctx, Action::Redo);
            }
            ui.separator();
            if ui.button("−").on_hover_text("Zoom out").clicked() {
                app.run_action(&ctx, Action::ZoomOut);
            }
            let zoom = app.view().map_or(1.0, |v| v.effective_zoom);
            if ui
                .button(format!("{:.0}%", zoom * 100.0))
                .on_hover_text("Fit page")
                .clicked()
            {
                app.run_action(&ctx, Action::ZoomFit);
            }
            if ui.button("+").on_hover_text("Zoom in").clicked() {
                app.run_action(&ctx, Action::ZoomIn);
            }
            ui.separator();
            if ui
                .button("Check")
                .on_hover_text("Check for print problems")
                .clicked()
            {
                app.run_action(&ctx, Action::Validate);
            }
            if ui.button("Export PDF").clicked() {
                app.run_action(&ctx, Action::ExportPdf);
            }
        });
        if app.views.len() > 1 || app.view().is_some() {
            ui.horizontal(|ui| {
                let mut activate = None;
                let mut close = None;
                for (i, v) in app.views.iter().enumerate() {
                    let Ok(o) = app.session.get(Some(v.doc_id)) else {
                        continue;
                    };
                    let mut title = o.doc().meta.title.clone();
                    if title.is_empty() {
                        title = "Untitled".into();
                    }
                    if o.editor.is_dirty() {
                        title.push_str(" *");
                    }
                    if ui.selectable_label(app.active == Some(i), title).clicked() {
                        activate = Some(i);
                    }
                    if app.active == Some(i)
                        && ui.small_button("x").on_hover_text("Close").clicked()
                    {
                        close = Some(i);
                    }
                }
                if let Some(i) = activate {
                    app.activate(i);
                }
                if close.is_some() {
                    app.close_active(false);
                }
            });
        }
    });
}

pub fn status_bar(app: &mut PubrsApp, ui: &mut egui::Ui) {
    egui::Panel::bottom("status").show(ui, |ui| {
        ui.horizontal(|ui| {
            if let (Some(v), Some(doc)) = (app.view(), app.doc()) {
                ui.label(format!("Page {} of {}", v.page + 1, doc.pages.len()));
                ui.separator();
                let s = doc.setup.size;
                ui.label(format!(
                    "{:.2} × {:.2}{}",
                    s.width * app.units.per_point(),
                    s.height * app.units.per_point(),
                    app.units.suffix()
                ));
                ui.separator();
                if let Some(e) = &v.edit {
                    ui.label(format!("Editing text · position {}", e.caret));
                    ui.separator();
                }
            }
            if let Some((msg, _)) = &app.status {
                ui.label(RichText::new(msg.as_str()).color(Color32::from_rgb(30, 64, 120)));
            }
            if app.window.software {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("⚠ Drawing without a GPU").color(Color32::from_rgb(180, 83, 9)))
                        .on_hover_text(format!(
                            "No graphics driver is available to this window, so {} draws it on the CPU, \
                             which uses much more CPU than a graphics card. Install the graphics driver, or turn \
                             on 3D acceleration if this is a virtual machine.",
                            app.window.name
                        ));
                });
            }
        });
    });
}

pub fn pages_panel(app: &mut PubrsApp, ui: &mut egui::Ui) {
    egui::Panel::left("pages")
        .resizable(false)
        .exact_size(132.0)
        .show(ui, |ui| {
            ui.add_space(4.0);
            ui.label(RichText::new("Pages").strong());
            let Some(doc_id) = app.doc_id() else { return };
            let (rev, n, size) = match app.session.get(Some(doc_id)) {
                Ok(o) => (o.revision(), o.doc().pages.len(), o.doc().setup.size),
                Err(_) => return,
            };
            let current = app.view().map_or(0, |v| v.page);
            // Check at most one stale thumbnail per frame: a page the edit
            // did not reach (same fingerprint) is just marked current; a
            // changed one is drawn by the render worker, off the UI thread.
            let stale = (0..n).find(|i| {
                app.view().is_some_and(|v| {
                    v.thumbs.get(i).is_none_or(|t| t.revision != rev)
                        && v.thumbs_requested.get(i).is_none_or(|(r, _)| *r != rev)
                })
            });
            if let Some(i) = stale
                && app.layout_timed(doc_id).is_ok()
                && let Ok(snap) = app.session.page_snapshot(Some(doc_id), i, &[], None)
            {
                let fp = snap.fingerprint();
                let dpi = (96.0 * 72.0 / size.width.max(size.height)).clamp(4.0, 72.0);
                let mut ask = false;
                if let Some(v) = app.view_mut() {
                    match v.thumbs.get_mut(&i) {
                        Some(t) if t.fingerprint == fp => t.revision = rev,
                        _ => {
                            ask = v.thumbs_requested.get(&i).is_none_or(|(_, f)| *f != fp);
                            v.thumbs_requested.insert(i, (rev, fp));
                        }
                    }
                }
                if ask {
                    app.renderer
                        .request_thumb(crate::render_worker::ThumbRequest {
                            snapshot: snap,
                            dpi,
                            fingerprint: fp,
                        });
                }
                ui.ctx().request_repaint();
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                for i in 0..n {
                    let tex = app
                        .view()
                        .and_then(|v| v.thumbs.get(&i).map(|t| t.texture.clone()));
                    let w = 96.0f32;
                    let h = w * (size.height / size.width) as f32;
                    let (rect, resp) =
                        ui.allocate_exact_size(egui::vec2(w + 8.0, h + 22.0), egui::Sense::click());
                    let img_rect = egui::Rect::from_min_size(
                        rect.min + egui::vec2(4.0, 4.0),
                        egui::vec2(w, h),
                    );
                    if i == current {
                        ui.painter()
                            .rect_filled(rect, 4.0, Color32::from_rgb(210, 225, 245));
                    }
                    match tex {
                        Some(t) => {
                            ui.painter().image(
                                t.id(),
                                img_rect,
                                egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                ),
                                Color32::WHITE,
                            );
                        }
                        None => {
                            ui.painter().rect_filled(img_rect, 0.0, Color32::WHITE);
                        }
                    }
                    ui.painter().rect_stroke(
                        img_rect,
                        0.0,
                        (1.0, Color32::GRAY),
                        egui::StrokeKind::Outside,
                    );
                    ui.painter().text(
                        egui::pos2(rect.center().x, img_rect.bottom() + 9.0),
                        egui::Align2::CENTER_CENTER,
                        format!("{}", i + 1),
                        egui::FontId::proportional(12.0),
                        Color32::DARK_GRAY,
                    );
                    if resp.clicked()
                        && let Some(v) = app.view_mut()
                    {
                        v.page = i;
                        v.selection.clear();
                        v.edit = None;
                    }
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let ctx = ui.ctx().clone();
                    if ui
                        .button("+")
                        .on_hover_text("Insert page after this one")
                        .clicked()
                    {
                        app.run_action(&ctx, Action::InsertPage);
                    }
                    if ui.button("−").on_hover_text("Delete this page").clicked() {
                        app.run_action(&ctx, Action::DeletePage);
                    }
                    if ui.button("Up").on_hover_text("Move page up").clicked() {
                        move_page(app, -1);
                    }
                    if ui.button("Down").on_hover_text("Move page down").clicked() {
                        move_page(app, 1);
                    }
                });
            });
        });
}

fn move_page(app: &mut PubrsApp, dir: isize) {
    let (Some(v), Some(doc)) = (app.view(), app.doc()) else {
        return;
    };
    let to = v.page as isize + dir;
    if to < 0 || to as usize >= doc.pages.len() {
        return;
    }
    let page = doc.pages[v.page].id;
    if app.exec(Command::MovePage {
        page,
        index: to as usize,
    }) && let Some(v) = app.view_mut()
    {
        v.page = to as usize;
    }
}

/// A small colour square.
pub(crate) fn chip(ui: &mut egui::Ui, c: Color) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
    let fill = Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a);
    ui.painter().rect_filled(rect, 2.0, fill);
    ui.painter().rect_stroke(
        rect,
        2.0,
        egui::Stroke::new(1.0, Color32::from_gray(150)),
        egui::StrokeKind::Inside,
    );
}

/// What a colour picker offers.
#[derive(Clone, Copy)]
struct PaintOpts {
    none_ok: bool,
    gradient_ok: bool,
}

fn paint_label(doc: &Document, p: Option<&Paint>) -> String {
    match p {
        None => "None".into(),
        Some(Paint::Color(c)) => c.to_hex(),
        Some(Paint::Swatch { swatch, tint }) => {
            let name = doc
                .swatch(*swatch)
                .map_or("(missing swatch)", |s| s.name.as_str());
            if *tint < 100.0 {
                format!("{name} {tint:.0}%")
            } else {
                name.to_string()
            }
        }
        Some(Paint::Gradient { gradient, .. }) => match gradient {
            GradientKind::Linear => "Linear gradient".into(),
            GradientKind::Radial => "Radial gradient".into(),
        },
    }
}

fn solid_of(doc: &Document, p: Option<&Paint>) -> SolidPaint {
    match p {
        Some(Paint::Swatch { swatch, tint }) => SolidPaint::Swatch {
            swatch: *swatch,
            tint: *tint,
        },
        Some(other) => SolidPaint::Color(doc.resolve_paint(other).solid_rgba()),
        None => SolidPaint::Color(Color::BLACK),
    }
}

fn to_color(c: Color32) -> Color {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    Color::rgba(r, g, b, a)
}

fn to_color32(c: Color) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

/// A colour picker: the document's swatches first, then a custom colour
/// (and, for fills, gradients). Returns the new paint when it changed.
/// `salt` must be unique on screen; controls are registered for the GUI
/// self-test as `paint-{salt}` and `paint-swatch-{name}`.
fn paint_edit(
    ui: &mut egui::Ui,
    doc: &Document,
    salt: &str,
    cur: Option<&Paint>,
    o: PaintOpts,
) -> Option<Option<Paint>> {
    let mut out: Option<Option<Paint>> = None;
    ui.horizontal(|ui| {
        match cur {
            Some(p) => chip(ui, doc.resolve_paint(p).solid_rgba()),
            None => chip(ui, Color::TRANSPARENT),
        }
        let r = egui::ComboBox::from_id_salt(("paint", salt))
            .selected_text(paint_label(doc, cur))
            .width(150.0)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show_ui(ui, |ui| {
                if o.none_ok && ui.selectable_label(cur.is_none(), "None").clicked() {
                    out = Some(None);
                    ui.close();
                }
                if !doc.swatches.is_empty() {
                    ui.small("Swatches");
                }
                for s in doc.swatches.iter() {
                    let selected =
                        matches!(cur, Some(Paint::Swatch { swatch, .. }) if *swatch == s.id);
                    ui.horizontal(|ui| {
                        chip(ui, doc.resolve_swatch(s.id, 100.0).rgba);
                        let r = ui.selectable_label(selected, &s.name);
                        crate::dialogs::mark(ui, &format!("paint-swatch-{}", s.name), &r);
                        if r.clicked() {
                            out = Some(Some(Paint::swatch(s.id)));
                            ui.close();
                        }
                    });
                }
                ui.separator();
                ui.small("Custom colour");
                let mut c = to_color32(doc.resolve_paint_or_black(cur));
                if ui.color_edit_button_srgba(&mut c).changed() {
                    out = Some(Some(Paint::Color(to_color(c))));
                }
                if o.gradient_ok {
                    ui.separator();
                    for (kind, label) in [
                        (GradientKind::Linear, "Linear gradient"),
                        (GradientKind::Radial, "Radial gradient"),
                    ] {
                        let is = matches!(cur, Some(Paint::Gradient { gradient, .. }) if *gradient == kind);
                        let r = ui.selectable_label(is, label);
                        crate::dialogs::mark(ui, &format!("paint-{label}"), &r);
                        if r.clicked() && !is {
                            out = Some(Some(Paint::Gradient {
                                gradient: kind,
                                stops: vec![
                                    GradientStop {
                                        offset: 0.0,
                                        color: solid_of(doc, cur),
                                    },
                                    GradientStop {
                                        offset: 1.0,
                                        color: SolidPaint::Color(Color::WHITE),
                                    },
                                ],
                                angle: 0.0,
                            }));
                            ui.close();
                        }
                    }
                }
            });
        crate::dialogs::mark(ui, &format!("paint-{salt}"), &r.response);
        r.response.on_hover_text(
            "Pick a swatch from this publication, a custom colour, or none. \
             Swatches update everywhere they are used when edited.",
        );
    });
    match cur {
        Some(Paint::Swatch { swatch, tint }) => {
            let mut t = *tint;
            ui.horizontal(|ui| {
                ui.label("Tint");
                if ui
                    .add(
                        egui::DragValue::new(&mut t)
                            .range(0.0..=100.0)
                            .speed(0.5)
                            .suffix("%"),
                    )
                    .changed()
                {
                    out = Some(Some(Paint::Swatch {
                        swatch: *swatch,
                        tint: t,
                    }));
                }
            });
        }
        Some(Paint::Gradient {
            gradient,
            stops,
            angle,
        }) => {
            let mut a = *angle;
            if *gradient == GradientKind::Linear {
                ui.horizontal(|ui| {
                    ui.label("Angle");
                    if ui
                        .add(egui::DragValue::new(&mut a).speed(1.0).suffix("°"))
                        .changed()
                    {
                        out = Some(Some(Paint::Gradient {
                            gradient: *gradient,
                            stops: stops.clone(),
                            angle: a,
                        }));
                    }
                });
            }
            for (i, label) in [(0, "From"), (stops.len().saturating_sub(1), "To")] {
                let Some(stop) = stops.get(i) else { continue };
                let cur_c = doc.resolve_solid(&stop.color.to_paint()).rgba;
                let mut c = to_color32(cur_c);
                ui.horizontal(|ui| {
                    ui.label(label);
                    if ui.color_edit_button_srgba(&mut c).changed() {
                        let mut s2 = stops.clone();
                        s2[i].color = SolidPaint::Color(to_color(c));
                        out = Some(Some(Paint::Gradient {
                            gradient: *gradient,
                            stops: s2,
                            angle: a,
                        }));
                    }
                });
            }
        }
        _ => {}
    }
    out
}

/// The Swatches section of the properties panel: list, add, edit, delete.
fn swatches_section(app: &mut PubrsApp, ui: &mut egui::Ui, doc: &Document) {
    ui.separator();
    let mut h = egui::CollapsingHeader::new(RichText::new("Swatches").strong()).id_salt("swatches");
    if let Some(open) = app.swatches_open.take() {
        h = h.open(Some(open));
    }
    h.show(ui, |ui| {
        if doc.swatches.is_empty() {
            ui.small("No swatches yet. Add one below, or apply a colour scheme (Design menu).");
        }
        for s in doc.swatches.iter() {
            ui.horizontal_wrapped(|ui| {
                chip(ui, doc.resolve_swatch(s.id, 100.0).rgba);
                let kind = match &s.value {
                    SwatchValue::Rgb(_) => "",
                    SwatchValue::Cmyk(_) => " (CMYK)",
                    SwatchValue::Spot { .. } => " (spot)",
                };
                ui.label(format!("{}{kind}", s.name));
                let edit = ui
                    .small_button("Edit")
                    .on_hover_text("Change this swatch; everything using it recolours");
                if edit.clicked() {
                    app.dialog = Some(Dialog::Swatch(SwatchEditor::edit(s)));
                }
                let uses = doc.swatch_uses(s.id);
                let reason = if let Some(slot) = pubrs_api::core::swatch::slot_of(s.id) {
                    Some(format!(
                        "“{}” is a colour scheme slot; apply another scheme to change it",
                        slot.swatch_name()
                    ))
                } else if !uses.is_empty() {
                    Some(format!("In use: {}", uses.join("; ")))
                } else {
                    None
                };
                let del = ui
                    .add_enabled(reason.is_none(), egui::Button::new("Delete").small())
                    .on_hover_text("Delete this swatch");
                let del = match &reason {
                    Some(r) => del.on_disabled_hover_text(r.as_str()),
                    None => del,
                };
                if del.clicked() {
                    app.exec(Command::DeleteSwatch {
                        swatch: s.id,
                        replace_with: None,
                    });
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            for (kind, label, name) in [
                (SwatchKind::Rgb, "+ RGB", "swatch-add-rgb"),
                (SwatchKind::Cmyk, "+ CMYK", "swatch-add-cmyk"),
                (SwatchKind::Spot, "+ Spot", "swatch-add-spot"),
            ] {
                let b = ui
                    .button(label)
                    .on_hover_text("Add a swatch to this publication");
                crate::dialogs::mark(ui, name, &b);
                if b.clicked() {
                    app.dialog = Some(Dialog::Swatch(SwatchEditor::add(doc, kind)));
                }
            }
        });
        if ui.button("Colour scheme…").clicked() {
            app.dialog = Some(Dialog::ColorScheme);
        }
    });
}

fn length(ui: &mut egui::Ui, label: &str, units: Units, value: f64) -> Option<f64> {
    let k = units.per_point();
    let mut v = value * k;
    let speed = match units {
        Units::Inches => 0.01,
        Units::Millimetres => 0.5,
        Units::Points => 1.0,
    };
    let r = ui
        .horizontal(|ui| {
            ui.label(label);
            ui.add(
                egui::DragValue::new(&mut v)
                    .speed(speed)
                    .max_decimals(3)
                    .suffix(units.suffix()),
            )
        })
        .inner;
    r.changed().then_some(v / k)
}

pub fn properties_panel(app: &mut PubrsApp, ui: &mut egui::Ui) {
    egui::Panel::right("props")
        .resizable(false)
        .exact_size(272.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(4.0);
                let Some(doc) = app.doc().cloned() else {
                    return;
                };
                let selection = app.selection();
                match selection.as_slice() {
                    [] => document_props(app, ui, &doc),
                    [f] => frame_props(app, ui, &doc, *f),
                    many => {
                        ui.label(RichText::new(format!("{} items selected", many.len())).strong());
                        let ctx = ui.ctx().clone();
                        ui.horizontal_wrapped(|ui| {
                            for (label, a) in [
                                ("Align left", Action::AlignLeft),
                                ("Center", Action::AlignCenter),
                                ("Right", Action::AlignRight),
                                ("Top", Action::AlignTop),
                                ("Middle", Action::AlignMiddle),
                                ("Bottom", Action::AlignBottom),
                                ("Space across", Action::DistributeHorizontally),
                                ("Space down", Action::DistributeVertically),
                                ("Group", Action::Group),
                                ("Link text", Action::LinkTextFrames),
                            ] {
                                if ui.button(label).clicked() {
                                    app.run_action(&ctx, a);
                                }
                            }
                        });
                    }
                }
                swatches_section(app, ui, &doc);
            });
        });
}

fn document_props(app: &mut PubrsApp, ui: &mut egui::Ui, doc: &pubrs_api::core::Document) {
    ui.label(RichText::new("Publication").strong());
    let mut title = doc.meta.title.clone();
    ui.horizontal(|ui| {
        ui.label("Title");
        if ui.text_edit_singleline(&mut title).lost_focus() && title != doc.meta.title {
            app.exec(Command::SetMeta {
                title: Some(title.clone()),
                author: None,
            });
        }
    });
    ui.horizontal(|ui| {
        ui.label("Units");
        egui::ComboBox::from_id_salt("units")
            .selected_text(app.units.suffix().trim())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut app.units, Units::Inches, "in");
                ui.selectable_value(&mut app.units, Units::Millimetres, "mm");
                ui.selectable_value(&mut app.units, Units::Points, "pt");
            });
    });
    ui.separator();
    ui.label(RichText::new("Page size").strong());
    let size = doc.setup.size;
    let current = PageSizePreset::ALL.iter().find(|p| {
        let s = p.size();
        (s.width - size.width).abs() < 0.5 && (s.height - size.height).abs() < 0.5
            || (s.width - size.height).abs() < 0.5 && (s.height - size.width).abs() < 0.5
    });
    egui::ComboBox::from_id_salt("page-size")
        .selected_text(current.map_or("Custom", |p| p.name()))
        .show_ui(ui, |ui| {
            for p in PageSizePreset::ALL {
                if ui.selectable_label(current == Some(&p), p.name()).clicked() {
                    let mut s = p.size();
                    if size.width > size.height {
                        s = Size::new(s.height, s.width);
                    }
                    app.exec(Command::SetPageSize { size: s });
                }
            }
        });
    if ui.button("Swap orientation").clicked() {
        app.exec(Command::SetPageSize {
            size: Size::new(size.height, size.width),
        });
    }
    let units = app.units;
    if let Some(w) = length(ui, "Width", units, size.width) {
        app.exec_merge(
            Command::SetPageSize {
                size: Size::new(w, size.height),
            },
            "page-w",
        );
    }
    if let Some(h) = length(ui, "Height", units, size.height) {
        app.exec_merge(
            Command::SetPageSize {
                size: Size::new(size.width, h),
            },
            "page-h",
        );
    }
    ui.separator();
    ui.label(RichText::new("Margins and bleed").strong());
    let m = doc.setup.margins;
    if let Some(v) = length(ui, "Margins", units, m.top) {
        app.exec_merge(
            Command::SetMargins {
                margins: Margins::uniform(v),
            },
            "margins",
        );
    }
    if let Some(v) = length(ui, "Bleed", units, doc.setup.bleed) {
        app.exec_merge(Command::SetBleed { bleed: v }, "bleed");
    }
    ui.separator();
    if let Some(v) = app.view() {
        let page = &doc.pages[v.page];
        ui.label(RichText::new(format!("Page {}", v.page + 1)).strong());
        let master_name = page
            .master
            .and_then(|m| doc.master(m))
            .map_or("(none)".to_string(), |m| m.name.clone());
        let page_id = page.id;
        egui::ComboBox::from_label("Master page")
            .selected_text(master_name)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(page.master.is_none(), "(none)")
                    .clicked()
                {
                    app.exec(Command::SetPageMaster {
                        page: page_id,
                        master: None,
                    });
                }
                for m in doc.masters.iter() {
                    if ui
                        .selectable_label(page.master == Some(m.id), &m.name)
                        .clicked()
                    {
                        app.exec(Command::SetPageMaster {
                            page: page_id,
                            master: Some(m.id),
                        });
                    }
                }
            });
    }
    ui.separator();
    ui.small(
        "Tip: double-click a text box to type in it. Drag on the page with a tool to add items.",
    );
}

fn frame_props(
    app: &mut PubrsApp,
    ui: &mut egui::Ui,
    doc: &pubrs_api::core::Document,
    id: FrameId,
) {
    let Some(f) = doc.frame(id).cloned() else {
        return;
    };
    let kind = match f.kind() {
        FrameKind::Text => "Text box",
        FrameKind::Image => "Picture",
        FrameKind::Shape => "Shape",
        FrameKind::Group => "Group",
        FrameKind::WordArt => "WordArt",
    };
    ui.label(RichText::new(kind).strong());
    if let Some(p) = &f.placeholder {
        ui.small(format!("Placeholder “{}”", p.name));
    }
    let units = app.units;
    let r = f.rect;
    let key = |k: &str| format!("prop-{k}-{id}");
    if let Some(v) = length(ui, "X", units, r.x) {
        app.exec_merge(
            Command::SetFrameRect {
                frame: id,
                rect: Rect::new(v, r.y, r.width, r.height),
            },
            &key("x"),
        );
    }
    if let Some(v) = length(ui, "Y", units, r.y) {
        app.exec_merge(
            Command::SetFrameRect {
                frame: id,
                rect: Rect::new(r.x, v, r.width, r.height),
            },
            &key("y"),
        );
    }
    let lock = f.as_image().is_some_and(|i| i.lock_aspect);
    if let Some(v) = length(ui, "Width", units, r.width) {
        let h = if lock && r.width > 0.0 {
            v * r.height / r.width
        } else {
            r.height
        };
        app.exec_merge(
            Command::SetFrameRect {
                frame: id,
                rect: Rect::new(r.x, r.y, v.max(0.0), h),
            },
            &key("w"),
        );
    }
    if let Some(v) = length(ui, "Height", units, r.height) {
        let w = if lock && r.height > 0.0 {
            v * r.width / r.height
        } else {
            r.width
        };
        app.exec_merge(
            Command::SetFrameRect {
                frame: id,
                rect: Rect::new(r.x, r.y, w, v.max(0.0)),
            },
            &key("h"),
        );
    }
    let mut rot = f.rotation;
    ui.horizontal(|ui| {
        ui.label("Rotation");
        if ui
            .add(
                egui::DragValue::new(&mut rot)
                    .speed(1.0)
                    .suffix("°")
                    .range(-360.0..=360.0),
            )
            .changed()
        {
            app.exec_merge(
                Command::SetRotation {
                    frame: id,
                    degrees: rot,
                },
                &key("rot"),
            );
        }
    });
    let mut locked = f.locked;
    if ui.checkbox(&mut locked, "Locked").changed() {
        app.exec(Command::SetLocked { frame: id, locked });
    }
    ui.separator();
    ui.label(RichText::new("Fill and line").strong());
    let style = f.style.clone();
    ui.label("Fill");
    {
        if let Some(c) = paint_edit(
            ui,
            doc,
            &format!("fill-{id}"),
            style.fill.as_ref(),
            PaintOpts {
                none_ok: true,
                gradient_ok: true,
            },
        ) {
            let mut s = style.clone();
            s.fill = c;
            app.exec_merge(
                Command::SetFrameStyle {
                    frame: id,
                    style: s,
                },
                &key("fill"),
            );
        }
    }
    ui.label("Line");
    {
        if let Some(c) = paint_edit(
            ui,
            doc,
            &format!("stroke-{id}"),
            style.stroke.as_ref().map(|s| &s.color),
            PaintOpts {
                none_ok: true,
                gradient_ok: false,
            },
        ) {
            let mut s = style.clone();
            s.stroke = c.map(|color| Stroke {
                color,
                width: style.stroke.as_ref().map_or(1.0, |s| s.width),
                dash: style.stroke.as_ref().map(|s| s.dash).unwrap_or_default(),
            });
            app.exec_merge(
                Command::SetFrameStyle {
                    frame: id,
                    style: s,
                },
                &key("stroke"),
            );
        }
    }
    if let Some(st) = style.stroke.clone() {
        let mut w = st.width;
        let mut dash = st.dash;
        ui.horizontal(|ui| {
            ui.label("Weight");
            let a = ui
                .add(
                    egui::DragValue::new(&mut w)
                        .speed(0.25)
                        .range(0.0..=72.0)
                        .suffix(" pt"),
                )
                .changed();
            let mut b = false;
            egui::ComboBox::from_id_salt("dash")
                .selected_text(format!("{dash:?}"))
                .show_ui(ui, |ui| {
                    for d in [Dash::Solid, Dash::Dashed, Dash::Dotted] {
                        b |= ui
                            .selectable_value(&mut dash, d, format!("{d:?}"))
                            .changed();
                    }
                });
            if a || b {
                let mut s = style.clone();
                s.stroke = Some(Stroke {
                    width: w,
                    dash,
                    ..st.clone()
                });
                app.exec_merge(
                    Command::SetFrameStyle {
                        frame: id,
                        style: s,
                    },
                    &key("weight"),
                );
            }
        });
    }
    let mut opacity = style.opacity * 100.0;
    ui.horizontal(|ui| {
        ui.label("Opacity");
        if ui
            .add(egui::Slider::new(&mut opacity, 0.0..=100.0).suffix("%"))
            .changed()
        {
            let mut s = style;
            s.opacity = opacity / 100.0;
            app.exec_merge(
                Command::SetFrameStyle {
                    frame: id,
                    style: s,
                },
                &key("opacity"),
            );
        }
    });
    shadow_props(app, ui, doc, id, &f.style);
    border_art_props(app, ui, id, &f.style);
    if f.kind() != FrameKind::Text {
        wrap_section(app, ui, id, f.wrap);
    }
    ui.separator();
    match &f.content {
        FrameContent::Text(t) => text_props(app, ui, doc, id, t.story, t),
        FrameContent::Image(img) => {
            ui.label(RichText::new("Picture").strong());
            if let Some(dpi) = pubrs_api::core::validate::effective_dpi(doc, id) {
                let c = if dpi < 150.0 {
                    Color32::from_rgb(180, 60, 0)
                } else {
                    Color32::DARK_GREEN
                };
                ui.colored_label(c, format!("Prints at {dpi:.0} dpi"));
            }
            if ui
                .button(if img.asset.is_some() {
                    "Change picture…"
                } else {
                    "Insert picture…"
                })
                .clicked()
                && let Some(p) = app.platform.dialogs.pick_image()
            {
                app.place_image(id, &p);
            }
            let mut fit = img.fit;
            ui.horizontal(|ui| {
                ui.label("Fit");
                for (m, label) in [
                    (ImageFit::Fill, "Fill"),
                    (ImageFit::Fit, "Fit"),
                    (ImageFit::Stretch, "Stretch"),
                ] {
                    if ui.selectable_value(&mut fit, m, label).changed() {
                        app.exec(Command::SetImageFit { frame: id, fit });
                    }
                }
            });
            ui.label("Alt text");
            let mut alt = img.alt_text.clone();
            let r = ui
                .add(
                    egui::TextEdit::multiline(&mut alt)
                        .id_salt(format!("alt-{id}"))
                        .desired_rows(2)
                        .desired_width(f32::INFINITY)
                        .hint_text("Describe the picture for screen readers"),
                )
                .on_hover_text(
                    "What the picture shows. Written to the PDF when you export with tagged output, \
                     and read aloud by screen readers.",
                );
            crate::dialogs::mark(ui, "alt-text", &r);
            if r.changed() {
                app.exec_merge(
                    Command::SetImageAlt {
                        frame: id,
                        alt_text: alt,
                    },
                    &key("alt"),
                );
            }
            ui.label("Crop (%)");
            let c = img.crop;
            let mut vals = [
                c.left * 100.0,
                c.top * 100.0,
                c.right * 100.0,
                c.bottom * 100.0,
            ];
            let mut changed = false;
            egui::Grid::new("crop").num_columns(4).show(ui, |ui| {
                for (k, label) in ["Left", "Top", "Right", "Bottom"].iter().enumerate() {
                    ui.label(*label);
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut vals[k])
                                .speed(0.5)
                                .range(0.0..=90.0),
                        )
                        .changed();
                    if k % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
            if changed {
                let crop = Crop {
                    left: vals[0] / 100.0,
                    top: vals[1] / 100.0,
                    right: vals[2] / 100.0,
                    bottom: vals[3] / 100.0,
                };
                if crop.is_valid() {
                    app.exec_merge(Command::SetImageCrop { frame: id, crop }, &key("crop"));
                }
            }
            let mut la = img.lock_aspect;
            if ui
                .checkbox(&mut la, "Keep proportions when resizing")
                .changed()
            {
                app.exec(Command::SetLockAspect {
                    frame: id,
                    lock: la,
                });
            }
        }
        FrameContent::Shape(s) => {
            if let Shape::Rectangle { corner_radius } = s.shape {
                let mut cr = corner_radius;
                ui.horizontal(|ui| {
                    ui.label("Corner radius");
                    if ui
                        .add(
                            egui::DragValue::new(&mut cr)
                                .speed(0.5)
                                .range(0.0..=500.0)
                                .suffix(" pt"),
                        )
                        .changed()
                    {
                        app.exec_merge(
                            Command::SetShape {
                                frame: id,
                                shape: Shape::Rectangle { corner_radius: cr },
                            },
                            &key("radius"),
                        );
                    }
                });
            }
        }
        FrameContent::WordArt(w) => wordart_props(app, ui, doc, id, w),
        FrameContent::Group(g) => {
            ui.label(format!("{} items", g.children.len()));
            let ctx = ui.ctx().clone();
            if ui.button("Ungroup").clicked() {
                app.run_action(&ctx, Action::Ungroup);
            }
        }
    }
}

/// The shadow controls of a frame's style: on/off, offset, blur, opacity and
/// colour. Every edit merges with the previous one of the same control.
fn shadow_props(
    app: &mut PubrsApp,
    ui: &mut egui::Ui,
    doc: &Document,
    id: FrameId,
    style: &pubrs_api::core::FrameStyle,
) {
    let key = |k: &str| format!("prop-{k}-{id}");
    let mut on = style.shadow.is_some();
    let r = ui.checkbox(&mut on, "Shadow");
    crate::dialogs::mark(ui, "shadow-on", &r);
    let r = r.on_hover_text("A soft drop shadow under the frame (Format ▸ Shadow)");
    if r.changed() {
        let mut s = style.clone();
        s.shadow = on.then_some(Shadow {
            offset_x: 4.0,
            offset_y: 4.0,
            blur: 4.0,
            color: Paint::Color(Color::BLACK),
            opacity: 0.5,
        });
        app.exec(Command::SetFrameStyle {
            frame: id,
            style: s,
        });
    }
    let Some(sh) = style.shadow.clone() else {
        return;
    };
    if let Some(new) = shadow_edit(ui, doc, &format!("shadow-{id}"), &sh) {
        let mut s = style.clone();
        s.shadow = Some(new);
        app.exec_merge(
            Command::SetFrameStyle {
                frame: id,
                style: s,
            },
            &key("shadow"),
        );
    }
}

/// Offset, blur, opacity and colour of a shadow; `Some` when one changed.
fn shadow_edit(ui: &mut egui::Ui, doc: &Document, salt: &str, sh: &Shadow) -> Option<Shadow> {
    let mut new = sh.clone();
    let mut changed = false;
    egui::Grid::new(("shadow-grid", salt))
        .num_columns(2)
        .show(ui, |ui| {
            let mut drag = |ui: &mut egui::Ui, label: &str, v: &mut f64, range, unit: &str| {
                ui.label(label);
                let r = ui.add(
                    egui::DragValue::new(v)
                        .speed(0.25)
                        .range(range)
                        .suffix(unit),
                );
                crate::dialogs::mark(ui, &format!("shadow-{}", label.to_lowercase()), &r);
                changed |= r.changed();
                ui.end_row();
            };
            let m = pubrs_api::core::SHADOW_MAX_OFFSET;
            drag(ui, "Across", &mut new.offset_x, -m..=m, " pt");
            drag(ui, "Down", &mut new.offset_y, -m..=m, " pt");
            drag(
                ui,
                "Blur",
                &mut new.blur,
                0.0..=pubrs_api::core::SHADOW_MAX_BLUR,
                " pt",
            );
            let mut pct = new.opacity * 100.0;
            drag(ui, "Strength", &mut pct, 0.0..=100.0, "%");
            new.opacity = pct / 100.0;
        });
    ui.horizontal(|ui| {
        ui.label("Shadow colour");
        if let Some(Some(c)) = paint_edit(
            ui,
            doc,
            &format!("{salt}-colour"),
            Some(&sh.color),
            PaintOpts {
                none_ok: false,
                gradient_ok: false,
            },
        ) {
            new.color = c;
            changed = true;
        }
    });
    changed.then_some(new)
}

/// The Border Art picker of a frame's style: a pattern and its width.
fn border_art_props(
    app: &mut PubrsApp,
    ui: &mut egui::Ui,
    id: FrameId,
    style: &pubrs_api::core::FrameStyle,
) {
    let key = |k: &str| format!("prop-{k}-{id}");
    let cur = style.border_art.clone();
    ui.horizontal(|ui| {
        ui.label("Border art");
        let combo = egui::ComboBox::from_id_salt(("border-art", id))
            .selected_text(cur.as_ref().map_or("None", |b| b.pattern.as_str()))
            .show_ui(ui, |ui| {
                let r = ui.selectable_label(cur.is_none(), "None");
                crate::dialogs::mark(ui, "border:none", &r);
                if r.clicked() {
                    let mut s = style.clone();
                    s.border_art = None;
                    app.exec(Command::SetFrameStyle {
                        frame: id,
                        style: s,
                    });
                }
                for name in BORDER_ART_PATTERNS {
                    let is = cur.as_ref().is_some_and(|b| b.pattern == name);
                    let r = ui.selectable_label(is, name);
                    crate::dialogs::mark(ui, &format!("border:{name}"), &r);
                    if r.clicked() && !is {
                        let mut s = style.clone();
                        s.border_art = Some(BorderArt {
                            pattern: name.to_string(),
                            width: cur.as_ref().map_or(18.0, |b| b.width),
                        });
                        app.exec(Command::SetFrameStyle {
                            frame: id,
                            style: s,
                        });
                    }
                }
            });
        crate::dialogs::mark(ui, "border-art-combo", &combo.response);
    });
    if let Some(b) = cur {
        let mut w = b.width;
        ui.horizontal(|ui| {
            ui.label("Border width");
            if ui
                .add(
                    egui::DragValue::new(&mut w)
                        .speed(0.5)
                        .range(BORDER_ART_WIDTH)
                        .suffix(" pt"),
                )
                .changed()
            {
                let mut s = style.clone();
                s.border_art = Some(BorderArt {
                    width: w,
                    ..b.clone()
                });
                app.exec_merge(
                    Command::SetFrameStyle {
                        frame: id,
                        style: s,
                    },
                    &key("border-width"),
                );
            }
        });
    }
}

/// The WordArt panel: text, font, size, warp, fill, outline and shadow.
fn wordart_props(
    app: &mut PubrsApp,
    ui: &mut egui::Ui,
    doc: &Document,
    id: FrameId,
    w: &WordArtFrame,
) {
    let key = |k: &str| format!("prop-wordart-{k}-{id}");
    let set = |app: &mut PubrsApp, art: WordArtFrame, k: &str| {
        app.exec_merge(Command::SetWordArt { frame: id, art }, &key(k));
    };
    ui.label(RichText::new("WordArt").strong());
    let mut text = w.text.clone();
    let r = ui.add(
        egui::TextEdit::singleline(&mut text)
            .id_salt(("wordart-text", id))
            .desired_width(f32::INFINITY)
            .char_limit(pubrs_api::core::command::WORDART_MAX_CHARS)
            .hint_text("Your text"),
    );
    crate::dialogs::mark(ui, "wordart-text", &r);
    if r.changed() {
        set(app, WordArtFrame { text, ..w.clone() }, "text");
    }
    let fonts = app.fonts.clone();
    egui::ComboBox::from_label("Font")
        .selected_text(w.font.clone())
        .height(300.0)
        .show_ui(ui, |ui| {
            for fam in &fonts {
                if ui.selectable_label(*fam == w.font, fam).clicked() {
                    set(
                        app,
                        WordArtFrame {
                            font: fam.clone(),
                            ..w.clone()
                        },
                        "font",
                    );
                }
            }
        });
    let mut size = w.size;
    ui.horizontal(|ui| {
        ui.label("Size");
        let r = ui.add(
            egui::DragValue::new(&mut size)
                .speed(1.0)
                .range(1.0..=1000.0)
                .suffix(" pt"),
        );
        crate::dialogs::mark(ui, "wordart-size", &r);
        let r = r.on_hover_text("Nominal size; the text shrinks to fit the frame");
        if r.changed() {
            set(app, WordArtFrame { size, ..w.clone() }, "size");
        }
    });
    ui.horizontal(|ui| {
        ui.label("Shape");
        let combo = egui::ComboBox::from_id_salt(("wordart-warp", id))
            .selected_text(warp_label(w.warp))
            .show_ui(ui, |ui| {
                for warp in Warp::ALL {
                    let r = ui.selectable_label(w.warp == warp, warp_label(warp));
                    crate::dialogs::mark(ui, &format!("warp:{}", warp.name()), &r);
                    if r.clicked() && w.warp != warp {
                        app.exec(Command::SetWordArt {
                            frame: id,
                            art: WordArtFrame { warp, ..w.clone() },
                        });
                    }
                }
            });
        crate::dialogs::mark(ui, "wordart-warp", &combo.response);
    });
    ui.label("Letter fill");
    if let Some(Some(c)) = paint_edit(
        ui,
        doc,
        &format!("wordart-fill-{id}"),
        Some(&w.fill),
        PaintOpts {
            none_ok: false,
            gradient_ok: true,
        },
    ) {
        set(
            app,
            WordArtFrame {
                fill: c,
                ..w.clone()
            },
            "fill",
        );
    }
    ui.label("Letter outline");
    if let Some(c) = paint_edit(
        ui,
        doc,
        &format!("wordart-outline-{id}"),
        w.outline.as_ref().map(|o| &o.color),
        PaintOpts {
            none_ok: true,
            gradient_ok: false,
        },
    ) {
        let outline = c.map(|color| Stroke {
            color,
            width: w.outline.as_ref().map_or(1.5, |o| o.width),
            dash: w.outline.as_ref().map(|o| o.dash).unwrap_or_default(),
        });
        set(
            app,
            WordArtFrame {
                outline,
                ..w.clone()
            },
            "outline",
        );
    }
    if let Some(o) = w.outline.clone() {
        let mut width = o.width;
        ui.horizontal(|ui| {
            ui.label("Outline weight");
            if ui
                .add(
                    egui::DragValue::new(&mut width)
                        .speed(0.25)
                        .range(0.0..=72.0)
                        .suffix(" pt"),
                )
                .changed()
            {
                set(
                    app,
                    WordArtFrame {
                        outline: Some(Stroke { width, ..o.clone() }),
                        ..w.clone()
                    },
                    "outline-width",
                );
            }
        });
    }
    let mut on = w.shadow.is_some();
    let r = ui.checkbox(&mut on, "Letter shadow");
    crate::dialogs::mark(ui, "wordart-shadow-on", &r);
    if r.changed() {
        app.exec(Command::SetWordArt {
            frame: id,
            art: WordArtFrame {
                shadow: on.then_some(Shadow {
                    offset_x: 3.0,
                    offset_y: 3.0,
                    blur: 3.0,
                    color: Paint::Color(Color::BLACK),
                    opacity: 0.5,
                }),
                ..w.clone()
            },
        });
    }
    if let Some(sh) = &w.shadow
        && let Some(new) = shadow_edit(ui, doc, &format!("wordart-shadow-{id}"), sh)
    {
        set(
            app,
            WordArtFrame {
                shadow: Some(new),
                ..w.clone()
            },
            "shadow",
        );
    }
}

fn warp_label(w: Warp) -> &'static str {
    match w {
        Warp::None => "Straight",
        Warp::ArchUp => "Arch up",
        Warp::ArchDown => "Arch down",
        Warp::Circle => "Circle",
        Warp::Wave => "Wave",
        Warp::SlantUp => "Slant up",
        Warp::SlantDown => "Slant down",
        Warp::Inflate => "Inflate",
    }
}

fn text_props(
    app: &mut PubrsApp,
    ui: &mut egui::Ui,
    doc: &pubrs_api::core::Document,
    id: FrameId,
    story: pubrs_api::core::StoryId,
    t: &pubrs_api::core::TextFrame,
) {
    let Some(s) = doc.story(story) else { return };
    let (start, end) = match app.view().and_then(|v| v.edit.as_ref()) {
        Some(e) if e.has_selection() => e.range(),
        Some(e) => {
            // Paragraph formatting applies to the caret's paragraph; character
            // formatting needs a selection.
            (e.caret, e.caret)
        }
        None => (0, s.char_len()),
    };
    let para = &s.paragraphs[s.locate(start).0];
    let fmt = s.format_at(start + usize::from(end > start));
    let run = pubrs_api::core::Run {
        text: String::new(),
        style: fmt.style,
        props: fmt.props,
    };
    let a = doc.styles.resolve_char(para, &run);
    let pa = doc.styles.resolve_para(para);
    ui.label(
        RichText::new(
            if end > start || app.view().is_none_or(|v| v.edit.is_none()) {
                "Text"
            } else {
                "Text (select text to change characters)"
            },
        )
        .strong(),
    );
    let apply_chars = |app: &mut PubrsApp, props: CharProps| {
        if end > start {
            app.exec(Command::ApplyCharProps {
                story,
                start,
                end,
                props,
            });
        }
    };
    let apply_para = |app: &mut PubrsApp, props: ParaProps| {
        app.exec(Command::ApplyParaProps {
            story,
            start,
            end,
            props,
        });
    };
    // Paragraph style
    let style_name = para
        .style
        .and_then(|i| doc.styles.get(i))
        .map_or("Normal".to_string(), |s| s.name.clone());
    egui::ComboBox::from_label("Style")
        .selected_text(style_name)
        .show_ui(ui, |ui| {
            for st in doc
                .styles
                .styles
                .iter()
                .filter(|s| s.kind == pubrs_api::core::StyleKind::Paragraph)
            {
                if ui
                    .selectable_label(para.style == Some(st.id), &st.name)
                    .clicked()
                {
                    app.exec(Command::SetParaStyle {
                        story,
                        start,
                        end,
                        style: Some(st.id),
                    });
                }
            }
        });
    let fonts = app.fonts.clone();
    egui::ComboBox::from_label("Font")
        .selected_text(a.font_family.clone())
        .height(300.0)
        .show_ui(ui, |ui| {
            for fam in &fonts {
                if ui.selectable_label(*fam == a.font_family, fam).clicked() {
                    apply_chars(
                        app,
                        CharProps {
                            font_family: Some(fam.clone()),
                            ..Default::default()
                        },
                    );
                }
            }
        });
    let mut size = a.font_size;
    ui.horizontal(|ui| {
        ui.label("Size");
        if ui
            .add(
                egui::DragValue::new(&mut size)
                    .speed(0.5)
                    .range(1.0..=999.0)
                    .suffix(" pt"),
            )
            .changed()
            && end > start
        {
            app.exec_merge(
                Command::ApplyCharProps {
                    story,
                    start,
                    end,
                    props: CharProps {
                        font_size: Some(size),
                        ..Default::default()
                    },
                },
                &format!("size-{story}"),
            );
        }
        let ctx = ui.ctx().clone();
        if ui
            .selectable_label(a.bold, RichText::new("B").strong())
            .clicked()
        {
            app.run_action(&ctx, Action::Bold);
        }
        if ui
            .selectable_label(a.italic, RichText::new("I").italics())
            .clicked()
        {
            app.run_action(&ctx, Action::Italic);
        }
        if ui
            .selectable_label(a.underline, RichText::new("U").underline())
            .clicked()
        {
            app.run_action(&ctx, Action::Underline);
        }
        for (on, label, action, name, tip) in [
            (
                a.baseline == pubrs_api::core::Baseline::Superscript,
                "x\u{b2}",
                Action::Superscript,
                "char-superscript",
                "Superscript (Cmd+Shift+=)",
            ),
            (
                a.baseline == pubrs_api::core::Baseline::Subscript,
                "x\u{2082}",
                Action::Subscript,
                "char-subscript",
                "Subscript (Cmd+=)",
            ),
        ] {
            let r = ui.selectable_label(on, label).on_hover_text(tip);
            crate::dialogs::mark(ui, name, &r);
            if r.clicked() {
                app.run_action(&ctx, action);
            }
        }
    });
    ui.label("Colour");
    if let Some(Some(c)) = paint_edit(
        ui,
        doc,
        &format!("text-{id}"),
        Some(&Paint::from(a.color)),
        PaintOpts {
            none_ok: false,
            gradient_ok: false,
        },
    ) {
        apply_chars(
            app,
            CharProps {
                color: Some(c),
                ..Default::default()
            },
        );
    }
    ui.horizontal(|ui| {
        for (al, label) in [
            (Align::Left, "Left"),
            (Align::Center, "Center"),
            (Align::Right, "Right"),
            (Align::Justify, "Justify"),
        ] {
            if ui
                .selectable_label(pa.align == al, label)
                .on_hover_text(format!("{al:?}"))
                .clicked()
            {
                apply_para(
                    app,
                    ParaProps {
                        align: Some(al),
                        ..Default::default()
                    },
                );
            }
        }
    });
    let mut ls = pa.line_spacing;
    ui.horizontal(|ui| {
        ui.label("Line spacing");
        if ui
            .add(egui::DragValue::new(&mut ls).speed(0.01).range(0.5..=4.0))
            .changed()
        {
            app.exec_merge(
                Command::ApplyParaProps {
                    story,
                    start,
                    end,
                    props: ParaProps {
                        line_spacing: Some(ls),
                        ..Default::default()
                    },
                },
                &format!("ls-{story}"),
            );
        }
    });
    let mut sa = pa.space_after;
    ui.horizontal(|ui| {
        ui.label("Space after");
        if ui
            .add(
                egui::DragValue::new(&mut sa)
                    .speed(0.5)
                    .range(0.0..=200.0)
                    .suffix(" pt"),
            )
            .changed()
        {
            app.exec_merge(
                Command::ApplyParaProps {
                    story,
                    start,
                    end,
                    props: ParaProps {
                        space_after: Some(sa),
                        ..Default::default()
                    },
                },
                &format!("sa-{story}"),
            );
        }
    });
    ui.separator();
    ui.label(RichText::new("Text box").strong());
    let mut cols = t.columns;
    let mut va = t.vertical_align;
    ui.horizontal(|ui| {
        ui.label("Columns");
        if ui
            .add(egui::DragValue::new(&mut cols).range(1..=12))
            .changed()
        {
            app.exec(Command::SetTextFrame {
                frame: id,
                props: TextFrameProps {
                    columns: Some(cols),
                    ..Default::default()
                },
            });
        }
    });
    ui.horizontal(|ui| {
        ui.label("Vertical");
        for (v, label) in [
            (VerticalAlign::Top, "Top"),
            (VerticalAlign::Middle, "Middle"),
            (VerticalAlign::Bottom, "Bottom"),
        ] {
            if ui.selectable_value(&mut va, v, label).changed() {
                app.exec(Command::SetTextFrame {
                    frame: id,
                    props: TextFrameProps {
                        vertical_align: Some(va),
                        ..Default::default()
                    },
                });
            }
        }
    });
    let chain = &s.frames;
    if chain.len() > 1 {
        let pos = chain.iter().position(|f| *f == id).map_or(0, |p| p + 1);
        ui.small(format!("Linked text box {pos} of {}", chain.len()));
    }
    let ctx = ui.ctx().clone();
    ui.horizontal(|ui| {
        if ui
            .button("Link…")
            .on_hover_text("Select this box and an empty one (Shift+click), then Link")
            .clicked()
        {
            app.run_action(&ctx, Action::LinkTextFrames);
        }
        if chain.last() != Some(&id) && ui.button("Break link").clicked() {
            app.run_action(&ctx, Action::UnlinkTextFrames);
        }
    });
}

/// The name of a wrap mode in the panel.
fn wrap_label(m: WrapMode) -> &'static str {
    match m {
        WrapMode::None => "None",
        WrapMode::Square => "Square",
        WrapMode::Tight => "Tight",
        WrapMode::Through => "Through",
        WrapMode::TopBottom => "Top and bottom",
    }
}

/// Text wrap around the selected frame: the mode, the clearance and, when
/// text can sit beside the frame, whether to fill both sides. Every control
/// is focusable (Tab) and works from the keyboard (Space/Enter opens the
/// list, arrows choose, Space toggles the box).
fn wrap_section(
    app: &mut PubrsApp,
    ui: &mut egui::Ui,
    id: FrameId,
    wrap: pubrs_api::core::TextWrap,
) {
    let mut mode = wrap.mode;
    ui.horizontal(|ui| {
        let label = ui.label("Text wrap");
        let combo = egui::ComboBox::from_id_salt("wrap")
            .selected_text(wrap_label(mode))
            .show_ui(ui, |ui| {
                for (m, name) in [
                    (WrapMode::None, "wrap-none"),
                    (WrapMode::Square, "wrap-square"),
                    (WrapMode::Tight, "wrap-tight"),
                    (WrapMode::Through, "wrap-through"),
                    (WrapMode::TopBottom, "wrap-top-bottom"),
                ] {
                    let r = ui.selectable_value(&mut mode, m, wrap_label(m));
                    crate::dialogs::mark(ui, name, &r);
                    if r.changed() {
                        let mut w = wrap;
                        w.mode = mode;
                        app.exec(Command::SetWrap { frame: id, wrap: w });
                    }
                }
            });
        crate::dialogs::mark(ui, "wrap-combo", &combo.response);
        combo.response.labelled_by(label.id);
    });
    if wrap.mode == WrapMode::None {
        return;
    }
    ui.horizontal(|ui| {
        ui.label("Margin");
        let mut margin = wrap.margin;
        let r = ui.add(
            egui::DragValue::new(&mut margin)
                .range(0.0..=200.0)
                .speed(0.5)
                .suffix(" pt"),
        );
        crate::dialogs::mark(ui, "wrap-margin", &r);
        if r.changed() {
            let mut w = wrap;
            w.margin = margin;
            app.exec_merge(
                Command::SetWrap { frame: id, wrap: w },
                &format!("wrap-margin-{id}"),
            );
        }
    });
    if wrap.mode != WrapMode::TopBottom {
        let mut both = wrap.both_sides;
        let r = ui
            .checkbox(&mut both, "Text on both sides")
            .on_hover_text("Fill every gap beside the frame that is wider than three em");
        crate::dialogs::mark(ui, "wrap-both-sides", &r);
        if r.changed() {
            let mut w = wrap;
            w.both_sides = both;
            app.exec(Command::SetWrap { frame: id, wrap: w });
        }
    }
}

//! Menu bar, toolbar, status bar, pages panel and properties panel.

use eframe::egui::{self, Color32, RichText};
use pubrs_api::core::{
    Align, CharProps, Color, Command, Crop, Dash, FrameContent, FrameId, FrameKind, ImageFit,
    Margins, PageSizePreset, ParaProps, Rect, Shape, Size, Stroke, TextFrameProps, VerticalAlign,
    WrapMode,
};
use pubrs_api::render::RasterOptions;

use crate::app::{PubrsApp, Tool, Units};
use crate::keymap::{self, Action, MenuItem};

fn menu_items(app: &mut PubrsApp, ui: &mut egui::Ui, items: &[MenuItem]) {
    for item in items {
        match item {
            MenuItem::Action(label, action) => {
                let mut b = egui::Button::new(*label);
                if let Some(sc) = keymap::shortcut(*action) {
                    b = b.shortcut_text(ui.ctx().format_shortcut(&sc));
                }
                if ui.add(b).clicked() {
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
            // Refresh at most one stale thumbnail per frame.
            let stale = (0..n).find(|i| {
                app.view()
                    .is_some_and(|v| v.thumbs.get(i).is_none_or(|(r, _)| *r != rev))
            });
            if let Some(i) = stale {
                let dpi = (96.0 * 72.0 / size.width.max(size.height)).clamp(4.0, 72.0);
                let t0 = std::time::Instant::now();
                let r = app.session.page_rgba(
                    Some(doc_id),
                    i,
                    &RasterOptions {
                        dpi,
                        ..Default::default()
                    },
                );
                if let Some(p) = &mut app.perf {
                    p.raster(t0.elapsed());
                }
                if let Ok((w, h, px)) = r {
                    let img =
                        egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px);
                    let tex = ui.ctx().load_texture(
                        format!("thumb-{doc_id}-{i}"),
                        img,
                        egui::TextureOptions::LINEAR,
                    );
                    if let Some(v) = app.view_mut() {
                        v.thumbs.insert(i, (rev, tex));
                    }
                }
                ui.ctx().request_repaint();
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                for i in 0..n {
                    let tex = app
                        .view()
                        .and_then(|v| v.thumbs.get(&i).map(|(_, t)| t.clone()));
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

fn color_edit(ui: &mut egui::Ui, c: Option<Color>) -> Option<Option<Color>> {
    let mut on = c.is_some();
    let col = c.unwrap_or(Color::BLACK);
    let mut rgba = Color32::from_rgba_unmultiplied(col.r, col.g, col.b, col.a);
    let mut changed = false;
    ui.horizontal(|ui| {
        changed |= ui.checkbox(&mut on, "").changed();
        ui.add_enabled_ui(on, |ui| {
            changed |= ui.color_edit_button_srgba(&mut rgba).changed();
        });
    });
    changed.then(|| {
        on.then(|| {
            let [r, g, b, a] = rgba.to_srgba_unmultiplied();
            Color::rgba(r, g, b, a)
        })
    })
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
                for m in &doc.masters {
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
    let style = f.style;
    ui.horizontal(|ui| {
        ui.label("Fill");
        if let Some(c) = color_edit(ui, style.fill) {
            let mut s = style;
            s.fill = c;
            app.exec_merge(
                Command::SetFrameStyle {
                    frame: id,
                    style: s,
                },
                &key("fill"),
            );
        }
    });
    ui.horizontal(|ui| {
        ui.label("Line");
        if let Some(c) = color_edit(ui, style.stroke.map(|s| s.color)) {
            let mut s = style;
            s.stroke = c.map(|color| Stroke {
                color,
                width: style.stroke.map_or(1.0, |s| s.width),
                dash: style.stroke.map(|s| s.dash).unwrap_or_default(),
            });
            app.exec_merge(
                Command::SetFrameStyle {
                    frame: id,
                    style: s,
                },
                &key("stroke"),
            );
        }
    });
    if let Some(st) = style.stroke {
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
                let mut s = style;
                s.stroke = Some(Stroke {
                    width: w,
                    dash,
                    ..st
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
    if f.kind() != FrameKind::Text {
        let mut mode = f.wrap.mode;
        ui.horizontal(|ui| {
            ui.label("Text wrap");
            egui::ComboBox::from_id_salt("wrap")
                .selected_text(format!("{mode:?}"))
                .show_ui(ui, |ui| {
                    for m in [WrapMode::None, WrapMode::Square, WrapMode::TopBottom] {
                        if ui
                            .selectable_value(&mut mode, m, format!("{m:?}"))
                            .changed()
                        {
                            let mut w = f.wrap;
                            w.mode = mode;
                            app.exec(Command::SetWrap { frame: id, wrap: w });
                        }
                    }
                });
        });
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
        FrameContent::Group(g) => {
            ui.label(format!("{} items", g.children.len()));
            let ctx = ui.ctx().clone();
            if ui.button("Ungroup").clicked() {
                app.run_action(&ctx, Action::Ungroup);
            }
        }
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
    });
    ui.horizontal(|ui| {
        ui.label("Colour");
        if let Some(Some(c)) = color_edit(ui, Some(a.color)) {
            apply_chars(
                app,
                CharProps {
                    color: Some(c),
                    ..Default::default()
                },
            );
        }
    });
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

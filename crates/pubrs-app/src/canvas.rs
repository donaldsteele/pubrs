//! The page canvas: rendering, overlays and direct manipulation.

use eframe::egui::{self, Color32, Pos2, Sense, Stroke, Vec2};
use pubrs_api::core::{
    Color, Command, Document, FrameContent, FrameId, FrameKind, LineDirection, Owner, Point, Rect,
    RunFormat,
};
use pubrs_api::render::PixelRect;
use pubrs_api::tools::export::{PageSnapshot, viewport_region};
use pubrs_api::tools::frames::{FrameAddImage, FrameAddShape, FrameAddText, ShapeKind};
use pubrs_api::tools::text::TextRef;

use crate::app::{PubrsApp, Tool};
use crate::render_worker::{ViewKey, ViewRequest};
use crate::textedit::{self, TextEdit};
use crate::view::{
    Drag, Handle, PageXf, frames_in_rect, handle_point, hit_leaf, resize_rect, top_level,
};

const PAD: f32 = 32.0;
const HANDLE: f32 = 4.5;
const ROTATE_OFFSET_PX: f32 = 22.0;
const SELECT: Color32 = Color32::from_rgb(37, 99, 235);
const GUIDE: Color32 = Color32::from_rgb(6, 182, 212);
const MARGIN: Color32 = Color32::from_rgb(217, 70, 239);
const REMOVE: Color32 = Color32::from_rgb(220, 38, 38);

fn quad(points: [Pos2; 4], fill: Color32, stroke: Stroke) -> egui::Shape {
    egui::Shape::convex_polygon(points.to_vec(), fill, stroke)
}

fn dashed_quad(painter: &egui::Painter, pts: [Pos2; 4], stroke: Stroke) {
    for i in 0..4 {
        let a = pts[i];
        let b = pts[(i + 1) % 4];
        painter.extend(egui::Shape::dashed_line(&[a, b], stroke, 4.0, 3.0));
    }
}

pub fn central(app: &mut PubrsApp, ui: &mut egui::Ui) {
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(Color32::from_gray(214)))
        .show(ui, |ui| {
            if app.view().is_none() {
                welcome(app, ui);
                return;
            }
            let full = ui.available_rect_before_wrap();
            let show = app.show_rulers;
            let inner = crate::rulers::canvas_rect(full, show);
            if let Some(v) = app.view_mut() {
                v.canvas_rect = Some(inner);
            }
            ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
                page_view(app, ui);
            });
            if show {
                crate::rulers::show(app, ui, full);
            }
        });
}

fn welcome(app: &mut PubrsApp, ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.3);
        ui.heading("pubrs");
        ui.label("Flyers, brochures, cards and newsletters.");
        ui.add_space(12.0);
        let ctx = ui.ctx().clone();
        if ui.button("New from template…").clicked() {
            app.run_action(&ctx, crate::keymap::Action::NewFromTemplate);
        }
        if ui.button("New blank publication").clicked() {
            app.run_action(&ctx, crate::keymap::Action::NewBlank);
        }
        if ui.button("Open…").clicked() {
            app.run_action(&ctx, crate::keymap::Action::Open);
        }
    });
}

/// What the canvas shows this frame, in texture terms.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Wanted {
    doc_id: pubrs_api::core::DocumentId,
    page_index: usize,
    /// Texture pixels per inch.
    dpi: f64,
    /// The whole page raster at `dpi`.
    page_px: (u32, u32),
    /// Visible page-raster pixels at `dpi`.
    visible: PixelRect,
    generation: u64,
    /// The largest texture the GPU takes (`max_texture_side`).
    max_side: u32,
}

/// Texture resolution for a view: one texel per device pixel, unless the
/// visible pixels (`visible_w` × `visible_h` at that resolution) would not
/// fit in the largest texture; then lower, and the texture is drawn scaled.
fn texture_dpi(device_dpi: f64, visible_w: u32, visible_h: u32, max_side: u32) -> f64 {
    let largest = visible_w.max(visible_h);
    if largest <= max_side || largest == 0 {
        device_dpi
    } else {
        // A little under, so rounding the visible pixels up still fits.
        device_dpi * (f64::from(max_side) - 2.0).max(1.0) / f64::from(largest)
    }
}

/// The key to ask the worker for: the pending or shown key while it still
/// fits (same page, resolution, page raster size and texture generation,
/// and its region holds the visible pixels), else a new region around the
/// visible pixels. `None` when nothing of the page is visible.
fn choose_key(want: &Wanted, pending: Option<ViewKey>, shown: Option<ViewKey>) -> Option<ViewKey> {
    let page = PixelRect::new(0, 0, want.page_px.0, want.page_px.1);
    let visible = want.visible.intersect(&page)?;
    let fits = |k: &ViewKey| {
        k.doc_id == want.doc_id
            && k.page_index == want.page_index
            && k.dpi == want.dpi
            && k.page_px == want.page_px
            && k.generation == want.generation
            && k.region.contains(&visible)
            && page.contains(&k.region)
    };
    if let Some(k) = pending.filter(fits).or(shown.filter(fits)) {
        return Some(k);
    }
    let region = viewport_region(visible, want.page_px, want.max_side)?;
    Some(ViewKey {
        doc_id: want.doc_id,
        page_index: want.page_index,
        dpi: want.dpi,
        page_px: want.page_px,
        region,
        generation: want.generation,
    })
}

/// Ask the render worker for the page when the revision or the view
/// changed. Nothing is rasterised here.
fn request_view(app: &mut PubrsApp, snap: &PageSnapshot, want: &Wanted) {
    let Some(v) = app.view() else { return };
    let pending = v.requested.map(|(k, _)| k);
    let shown = v.page_tex.as_ref().map(|t| t.key);
    let Some(key) = choose_key(want, pending, shown) else {
        return;
    };
    if v.requested == Some((key, snap.revision)) {
        return;
    }
    let since = v.page_tex.as_ref().map_or(0, |t| t.revision);
    let changes = app
        .session
        .get(Some(snap.doc_id))
        .map(|o| o.changes_since(since))
        .unwrap_or_default();
    app.renderer.request_view(ViewRequest {
        key,
        snapshot: snap.clone(),
        changes,
    });
    if let Some(v) = app.view_mut() {
        v.requested = Some((key, snap.revision));
    }
}

fn page_view(app: &mut PubrsApp, ui: &mut egui::Ui) {
    let Some(doc_id) = app.doc_id() else { return };
    let Ok(open) = app.session.get(Some(doc_id)) else {
        return;
    };
    let page_index = app
        .view()
        .map_or(0, |v| v.page)
        .min(open.doc().pages.len() - 1);
    // The page's slice of the document (with the selection and the story
    // being edited, for the overlays), its layout and images: what the
    // render worker draws from. Costs what the page holds.
    if let Err(e) = app.layout_timed(doc_id) {
        app.show_error(&e);
        return;
    }
    let snap = {
        let view = app.view();
        let selection: Vec<FrameId> = view.map_or_else(Vec::new, |v| v.selection.clone());
        let editing = view.and_then(|v| v.edit.as_ref().map(|e| e.story));
        app.session
            .page_snapshot(Some(doc_id), page_index, &selection, editing)
    };
    let snap = match snap {
        Ok(s) => s,
        Err(e) => {
            app.show_error(&e);
            return;
        }
    };
    // Spelling: check what the page shows, off the UI thread.
    let editing = app.view().and_then(|v| v.edit.as_ref().map(|e| e.story));
    crate::spell::request_page(app, &snap, editing);
    let doc: &Document = &snap.doc;
    let page = doc.pages[0].clone();
    let owner = Owner::Page(page.id);
    let (pw, ph, bleed) = (
        doc.setup.size.width as f32,
        doc.setup.size.height as f32,
        doc.setup.bleed as f32,
    );
    let avail = ui.available_size();
    let zoom = app.view().and_then(|v| v.zoom).unwrap_or_else(|| {
        ((avail.x - 2.0 * PAD) / (pw + 2.0 * bleed))
            .min((avail.y - 2.0 * PAD) / (ph + 2.0 * bleed))
            .max(0.05)
    });
    if let Some(v) = app.view_mut() {
        v.effective_zoom = zoom;
    }
    let content = Vec2::new(
        (pw + 2.0 * bleed) * zoom + 2.0 * PAD,
        (ph + 2.0 * bleed) * zoom + 2.0 * PAD,
    );
    let ppp = ui.ctx().pixels_per_point();

    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let size = content.max(ui.available_size());
            let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
            let canvas_id = resp.id;
            // The bleed box starts on a device pixel, so the texture (one
            // texel per device pixel) is drawn unfiltered and sharp.
            let snap_px = |v: f32| (v * ppp).round() / ppp;
            let bleed_min = Pos2::new(
                snap_px(rect.center().x - (pw / 2.0 + bleed) * zoom),
                snap_px(rect.center().y - (ph / 2.0 + bleed) * zoom),
            );
            let origin = bleed_min + Vec2::splat(bleed * zoom);
            let xf = PageXf {
                origin,
                scale: zoom,
            };
            if let Some(v) = app.view_mut() {
                v.xf = Some(xf);
                // Bring a find match into view.
                if let Some(r) = v.scroll_to.take() {
                    let r = xf.rect_to_screen(r).expand(24.0);
                    ui.scroll_to_rect(r, Some(egui::Align::Center));
                }
            }
            let bleed_rect = egui::Rect::from_min_size(
                bleed_min,
                Vec2::new((pw + 2.0 * bleed) * zoom, (ph + 2.0 * bleed) * zoom),
            );
            // ---- page texture: ask the render worker for what is visible
            let visible = ui.clip_rect().intersect(bleed_rect);
            if visible.is_positive() {
                let device_dpi = 72.0 * f64::from(zoom) * f64::from(ppp);
                // The GPU's texture limit, but never more than the rasterizer renders.
                let max_side = (ui.ctx().input(|i| i.max_texture_side) as u32)
                    .min(pubrs_api::render::raster::MAX_PIXELS_PER_SIDE);
                let span = |dpi: f64| {
                    let k = dpi / device_dpi * f64::from(ppp);
                    let to_px = |a: f32, b: f32| f64::from(a - b) * k;
                    PixelRect::covering(
                        to_px(visible.min.x, bleed_min.x),
                        to_px(visible.min.y, bleed_min.y),
                        to_px(visible.max.x, bleed_min.x),
                        to_px(visible.max.y, bleed_min.y),
                    )
                };
                let full = span(device_dpi);
                let dpi = texture_dpi(device_dpi, full.width, full.height, max_side);
                let s = dpi / 72.0;
                let want = Wanted {
                    doc_id,
                    page_index,
                    dpi,
                    page_px: (
                        ((doc.setup.size.width + 2.0 * doc.setup.bleed) * s)
                            .ceil()
                            .max(1.0) as u32,
                        ((doc.setup.size.height + 2.0 * doc.setup.bleed) * s)
                            .ceil()
                            .max(1.0) as u32,
                    ),
                    visible: span(dpi),
                    generation: app.view().map_or(0, |v| v.tex_generation),
                    max_side,
                };
                request_view(app, &snap, &want);
            }
            let painter = ui.painter_at(rect);
            painter.rect_filled(
                bleed_rect.translate(Vec2::new(3.0, 4.0)),
                0.0,
                Color32::from_black_alpha(40),
            );
            // Paper first: parts the worker has not drawn yet show white.
            painter.rect_filled(bleed_rect, 0.0, Color32::WHITE);
            if let Some(t) = app.view().and_then(|v| v.page_tex.as_ref())
                && t.key.page_index == page_index
            {
                // The previous texture, scaled and offset, until the new one
                // lands; never outside the page (it may be from before a
                // page size or bleed change).
                painter.with_clip_rect(bleed_rect.intersect(rect)).image(
                    t.texture.id(),
                    t.screen_rect(bleed_min, zoom),
                    egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            let trim = xf.rect_to_screen(Rect::new(0.0, 0.0, f64::from(pw), f64::from(ph)));
            if bleed > 0.0 {
                painter.rect_stroke(
                    bleed_rect,
                    0.0,
                    Stroke::new(1.0, Color32::from_rgba_unmultiplied(220, 38, 38, 90)),
                    egui::StrokeKind::Inside,
                );
            }
            painter.rect_stroke(
                trim,
                0.0,
                Stroke::new(1.0, Color32::from_gray(90)),
                egui::StrokeKind::Outside,
            );
            if app.show_guides {
                let m = doc.setup.margins;
                let mrect =
                    xf.rect_to_screen(Rect::new(0.0, 0.0, f64::from(pw), f64::from(ph)).inset(&m));
                dashed_quad(
                    &painter,
                    [
                        mrect.left_top(),
                        mrect.right_top(),
                        mrect.right_bottom(),
                        mrect.left_bottom(),
                    ],
                    Stroke::new(0.8, MARGIN.gamma_multiply(0.7)),
                );
                let mut guides = page.guides.clone();
                if let Some(mid) = page.master
                    && let Some(m) = doc.master(mid)
                {
                    guides.extend(m.guides.iter().copied());
                }
                let gdrag = app.view().and_then(|v| v.drag.clone());
                for g in guides {
                    let (mut pos, mut color) = (g.position, GUIDE);
                    if let Some(Drag::Guide {
                        id: Some(gid),
                        pos: p,
                        inside,
                        ..
                    }) = &gdrag
                        && *gid == g.id
                    {
                        pos = *p;
                        if !*inside {
                            color = REMOVE;
                        }
                    }
                    draw_guide(&painter, xf, bleed_rect, g.axis, pos, color);
                }
                if let Some(Drag::Guide {
                    id: None,
                    axis,
                    pos,
                    inside: true,
                    ..
                }) = &gdrag
                {
                    draw_guide(&painter, xf, bleed_rect, *axis, *pos, GUIDE);
                }
            }
            let layout = Some(snap.layout.clone());
            // ---- frame outlines (text boxes and empty picture frames are otherwise invisible)
            for id in doc.paint_order(owner) {
                let Some(f) = doc.frame(id) else { continue };
                let show = match &f.content {
                    FrameContent::Text(_) => app.show_guides,
                    FrameContent::Image(i) => i.asset.is_none(),
                    _ => false,
                };
                if show {
                    dashed_quad(
                        &painter,
                        xf.corners(f.rect, f.rotation),
                        Stroke::new(0.8, Color32::from_rgba_unmultiplied(100, 116, 139, 160)),
                    );
                }
                if let FrameContent::Image(i) = &f.content
                    && i.asset.is_none()
                {
                    let c = xf.to_screen(f.rect.center());
                    let label = f
                        .placeholder
                        .as_ref()
                        .map_or("Picture", |p| p.name.as_str());
                    painter.text(
                        c,
                        egui::Align2::CENTER_CENTER,
                        format!("{label}\ndouble-click to add a picture"),
                        egui::FontId::proportional(11.0),
                        Color32::from_gray(100),
                    );
                }
            }
            // ---- overflow markers and chain connectors
            if let Some(layout) = &layout {
                for (sid, sl) in &layout.stories {
                    let Some(story) = doc.story(*sid) else {
                        continue;
                    };
                    if sl.overflow.is_some()
                        && let Some(last) = story.frames.last()
                        && doc.owner_of(*last) == Some(owner)
                        && let Some(f) = doc.frame(*last)
                    {
                        let br = xf.to_screen(handle_point(f.rect, f.rotation, Handle::SE, 0.0));
                        let r = egui::Rect::from_center_size(br, Vec2::splat(12.0));
                        painter.rect_filled(r, 2.0, Color32::from_rgb(220, 38, 38));
                        painter.text(
                            r.center(),
                            egui::Align2::CENTER_CENTER,
                            "+",
                            egui::FontId::monospace(11.0),
                            Color32::WHITE,
                        );
                    }
                    let selected_in_chain = app.view().is_some_and(|v| {
                        v.selection.iter().any(|s| story.frames.contains(s))
                            || v.edit.as_ref().is_some_and(|e| e.story == *sid)
                    });
                    if selected_in_chain {
                        for pair in story.frames.windows(2) {
                            let (Some(a), Some(b)) = (doc.frame(pair[0]), doc.frame(pair[1]))
                            else {
                                continue;
                            };
                            if doc.owner_of(a.id) != Some(owner)
                                || doc.owner_of(b.id) != Some(owner)
                            {
                                continue;
                            }
                            let p0 =
                                xf.to_screen(handle_point(a.rect, a.rotation, Handle::SE, 0.0));
                            let p1 =
                                xf.to_screen(handle_point(b.rect, b.rotation, Handle::NW, 0.0));
                            painter.extend(egui::Shape::dashed_line(
                                &[p0, p1],
                                Stroke::new(1.5, SELECT),
                                6.0,
                                4.0,
                            ));
                            painter.circle_filled(p1, 3.5, SELECT);
                        }
                    }
                }
            }
            // ---- spelling: squiggles under misspelt words
            if let Some(layout) = &layout {
                crate::spell::draw_squiggles(
                    app,
                    &painter,
                    doc_id,
                    doc,
                    layout,
                    owner,
                    xf,
                    ppp,
                    ui.visuals().error_fg_color,
                );
            }
            // ---- text editing overlays
            let edit = app.view().and_then(|v| v.edit.clone());
            if let (Some(e), Some(layout)) = (&edit, &layout) {
                let (s, t) = e.range();
                for q in textedit::selection_quads(doc, layout, e.story, s, t) {
                    painter.add(quad(
                        q.map(|p| xf.to_screen(p)),
                        Color32::from_rgba_unmultiplied(59, 130, 246, 70),
                        Stroke::NONE,
                    ));
                }
                if let Some((f, top, bottom)) =
                    textedit::caret_segment(doc, layout, e.story, e.caret)
                    && doc.owner_of(f) == Some(owner)
                {
                    let blink = (ui.input(|i| i.time) * 2.0) as i64 % 2 == 0;
                    if blink || e.has_selection() {
                        painter.line_segment(
                            [xf.to_screen(top), xf.to_screen(bottom)],
                            Stroke::new(1.5, Color32::BLACK),
                        );
                    }
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(500));
                }
            }
            // ---- selection
            let selection = app.view().map(|v| v.selection.clone()).unwrap_or_default();
            let drag = app.view().and_then(|v| v.drag.clone());
            let rotate_offset = f64::from(ROTATE_OFFSET_PX / zoom);
            for id in &selection {
                let Some(f) = doc.frame(*id) else { continue };
                let (mut r, mut rot) = (f.rect, f.rotation);
                match &drag {
                    Some(Drag::Move { frames, delta, .. })
                        if frames.iter().any(|(fid, _)| fid == id) =>
                    {
                        r = r.translate(delta.0, delta.1);
                    }
                    Some(Drag::Resize { frame, rect, .. }) if frame == id => r = *rect,
                    Some(Drag::Rotate { frame, angle, .. }) if frame == id => rot = *angle,
                    _ => {}
                }
                let corners = xf.corners(r, rot);
                painter.add(quad(
                    corners,
                    Color32::TRANSPARENT,
                    Stroke::new(1.5, SELECT),
                ));
                if selection.len() == 1 && !f.locked && edit.is_none() {
                    for h in Handle::RESIZE {
                        let p = xf.to_screen(handle_point(r, rot, h, 0.0));
                        painter.rect(
                            egui::Rect::from_center_size(p, Vec2::splat(HANDLE * 2.0)),
                            1.0,
                            Color32::WHITE,
                            Stroke::new(1.2, SELECT),
                            egui::StrokeKind::Middle,
                        );
                    }
                    let top = xf.to_screen(handle_point(r, rot, Handle::N, 0.0));
                    let rp = xf.to_screen(handle_point(r, rot, Handle::Rotate, rotate_offset));
                    painter.line_segment([top, rp], Stroke::new(1.0, SELECT));
                    painter.circle(rp, HANDLE + 0.5, Color32::WHITE, Stroke::new(1.2, SELECT));
                }
                if f.locked {
                    painter.text(
                        corners[1],
                        egui::Align2::LEFT_BOTTOM,
                        "locked",
                        egui::FontId::proportional(10.0),
                        SELECT,
                    );
                }
            }
            match &drag {
                Some(Drag::Create { start, current }) | Some(Drag::Marquee { start, current }) => {
                    let r = xf.rect_to_screen(Rect::from_points(*start, *current));
                    painter.rect_stroke(r, 0.0, Stroke::new(1.0, SELECT), egui::StrokeKind::Middle);
                    if matches!(drag, Some(Drag::Marquee { .. })) {
                        painter.rect_filled(
                            r,
                            0.0,
                            Color32::from_rgba_unmultiplied(59, 130, 246, 25),
                        );
                    }
                }
                _ => {}
            }
            handle_input(
                app,
                ui,
                &resp,
                canvas_id,
                doc,
                owner,
                xf,
                rotate_offset,
                layout.as_deref(),
            );
            crate::spell::context_menu(app, ui);
        });
}

/// The frame under the pointer for a click, and whether a handle was hit.
fn handle_at(
    doc: &Document,
    selection: &[FrameId],
    xf: PageXf,
    pos: Pos2,
    rotate_offset: f64,
) -> Option<(FrameId, Handle)> {
    let [id] = selection else { return None };
    let f = doc.frame(*id)?;
    if f.locked {
        return None;
    }
    let rp = xf.to_screen(handle_point(
        f.rect,
        f.rotation,
        Handle::Rotate,
        rotate_offset,
    ));
    if rp.distance(pos) <= HANDLE + 3.0 {
        return Some((*id, Handle::Rotate));
    }
    Handle::RESIZE
        .into_iter()
        .find(|h| {
            xf.to_screen(handle_point(f.rect, f.rotation, *h, 0.0))
                .distance(pos)
                <= HANDLE + 3.0
        })
        .map(|h| (*id, h))
}

#[allow(clippy::too_many_arguments)]
fn handle_input(
    app: &mut PubrsApp,
    ui: &mut egui::Ui,
    resp: &egui::Response,
    canvas_id: egui::Id,
    doc: &Document,
    owner: Owner,
    xf: PageXf,
    rotate_offset: f64,
    layout: Option<&pubrs_api::layout::DocumentLayout>,
) {
    let ctx = ui.ctx().clone();
    let pad = f64::from(3.0 / xf.scale);
    let modifiers = ctx.input(|i| i.modifiers);
    let selection = app.view().map(|v| v.selection.clone()).unwrap_or_default();
    // Cursor feedback.
    if let Some(pos) = resp.hover_pos() {
        if app.tool != Tool::Select {
            ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
        } else if let Some((_, h)) = handle_at(doc, &selection, xf, pos, rotate_offset) {
            ctx.set_cursor_icon(h.cursor());
        } else if app.show_guides
            && let Some((_, g)) = crate::rulers::guide_at(&doc.pages[0].guides, xf, pos, owner)
        {
            ctx.set_cursor_icon(if g.axis == pubrs_api::core::Axis::Vertical {
                egui::CursorIcon::ResizeHorizontal
            } else {
                egui::CursorIcon::ResizeVertical
            });
        } else if app.view().is_some_and(|v| v.edit.is_some())
            && hit_leaf(doc, owner, xf.to_page(pos), pad)
                .is_some_and(|f| doc.frame(f).is_some_and(|f| f.kind() == FrameKind::Text))
        {
            ctx.set_cursor_icon(egui::CursorIcon::Text);
        }
    }
    // ---- right click: the spelling menu on a misspelt word
    if resp.secondary_clicked()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let p = xf.to_page(pos);
        if let (Some(leaf), Some(layout), Some(doc_id)) =
            (hit_leaf(doc, owner, p, pad), layout, app.doc_id())
            && let Some(story) = doc.story_of(leaf)
            && let Some(at) = textedit::offset_at(doc, layout, story, leaf, p)
        {
            crate::spell::open_menu(app, doc_id, doc, story, at, pos, false);
        }
        return;
    }
    // ---- double click: edit text / place picture
    if resp.double_clicked()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let p = xf.to_page(pos);
        if let Some(leaf) = hit_leaf(doc, owner, p, pad) {
            match doc.frame(leaf).map(|f| f.kind()) {
                Some(FrameKind::Text) => {
                    if let (Some(story), Some(layout)) = (doc.story_of(leaf), layout) {
                        let at = textedit::offset_at(doc, layout, story, leaf, p).unwrap_or(0);
                        begin_edit(app, leaf, story, at);
                        resp.request_focus();
                    }
                }
                Some(FrameKind::Image) => {
                    app.select(vec![leaf]);
                    if let Some(path) = app.platform.dialogs.pick_image() {
                        app.place_image(leaf, &path);
                    }
                }
                _ => {}
            }
        }
        return;
    }
    // ---- single click
    if resp.clicked()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        resp.request_focus();
        let p = xf.to_page(pos);
        let editing = app.view().and_then(|v| v.edit.clone());
        if let (Some(e), Some(layout)) = (&editing, layout) {
            let leaf = hit_leaf(doc, owner, p, pad);
            if let Some(leaf) = leaf
                && doc.story_of(leaf) == Some(e.story)
            {
                let at = textedit::offset_at(doc, layout, e.story, leaf, p).unwrap_or(e.caret);
                if let Some(v) = app.view_mut()
                    && let Some(ed) = &mut v.edit
                {
                    ed.set(at, modifiers.shift);
                    ed.frame = leaf;
                    ed.goal_x = None;
                }
                return;
            }
        }
        match app.tool {
            Tool::Select => {
                let hit = hit_leaf(doc, owner, p, pad).map(|f| top_level(doc, f));
                let mut sel = if modifiers.shift {
                    selection.clone()
                } else {
                    Vec::new()
                };
                if let Some(h) = hit {
                    if modifiers.shift && sel.contains(&h) {
                        sel.retain(|f| *f != h);
                    } else if !sel.contains(&h) {
                        sel.push(h);
                    }
                }
                app.select(sel);
            }
            tool => create_frame(app, tool, Rect::new(p.x, p.y, 0.0, 0.0), owner),
        }
        return;
    }
    // ---- drags
    if resp.drag_started()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        resp.request_focus();
        let origin = ctx
            .input(|i| i.pointer.press_origin())
            .unwrap_or(pos - resp.drag_delta());
        let p = xf.to_page(origin);
        let editing = app.view().is_some_and(|v| v.edit.is_some());
        let drag = if app.tool != Tool::Select {
            Some(Drag::Create {
                start: p,
                current: p,
            })
        } else if let Some((id, h)) = handle_at(doc, &selection, xf, origin, rotate_offset) {
            let f = doc.frame(id).expect("selected");
            Some(if h == Handle::Rotate {
                let c = f.rect.center();
                Drag::Rotate {
                    frame: id,
                    center: c,
                    start_angle: (p.y - c.y).atan2(p.x - c.x).to_degrees(),
                    orig: f.rotation,
                    angle: f.rotation,
                }
            } else {
                let keep = f.as_image().is_some_and(|i| i.lock_aspect) || modifiers.shift;
                Drag::Resize {
                    frame: id,
                    handle: h,
                    orig: f.rect,
                    rotation: f.rotation,
                    keep_aspect: keep,
                    rect: f.rect,
                }
            })
        } else if app.show_guides
            && hit_leaf(doc, owner, p, pad).is_none()
            && let Some((o, g)) = crate::rulers::guide_at(&doc.pages[0].guides, xf, origin, owner)
        {
            Some(Drag::Guide {
                owner: o,
                id: Some(g.id),
                axis: g.axis,
                orig: Some(g.position),
                pos: g.position,
                inside: true,
            })
        } else if editing
            && hit_leaf(doc, owner, p, pad).and_then(|f| doc.story_of(f))
                == app.view().and_then(|v| v.edit.as_ref().map(|e| e.story))
        {
            Some(Drag::TextSelect)
        } else {
            match hit_leaf(doc, owner, p, pad).map(|f| top_level(doc, f)) {
                Some(hit) => {
                    let mut sel = selection.clone();
                    if !sel.contains(&hit) {
                        sel = vec![hit];
                        app.select(sel.clone());
                    }
                    let frames: Vec<(FrameId, Rect)> = sel
                        .iter()
                        .filter_map(|f| doc.frame(*f))
                        .filter(|f| !f.locked)
                        .map(|f| (f.id, f.rect))
                        .collect();
                    (!frames.is_empty()).then_some(Drag::Move {
                        frames,
                        start: p,
                        delta: (0.0, 0.0),
                    })
                }
                None => {
                    if !modifiers.shift {
                        app.select(Vec::new());
                    }
                    Some(Drag::Marquee {
                        start: p,
                        current: p,
                    })
                }
            }
        };
        if let Some(v) = app.view_mut() {
            if !matches!(drag, Some(Drag::TextSelect)) {
                v.edit = None;
            }
            v.drag = drag;
        }
    }
    if resp.dragged()
        && let Some(pos) = resp.interact_pointer_pos()
        && matches!(
            app.view().and_then(|v| v.drag.as_ref()),
            Some(Drag::Guide { .. })
        )
    {
        crate::rulers::drag_update(app, pos);
    }
    if resp.dragged()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let p = xf.to_page(pos);
        let mut drag = app.view().and_then(|v| v.drag.clone());
        match &mut drag {
            Some(Drag::Move {
                frames,
                start,
                delta,
            }) => {
                let mut d = (p.x - start.x, p.y - start.y);
                if (doc.grid.snap_to_guides || doc.grid.snap_to_grid)
                    && frames.len() == 1
                    && !modifiers.alt
                {
                    let (id, r) = frames[0];
                    let moved = r.translate(d.0, d.1);
                    let snapped =
                        pubrs_api::core::snap::snap_move(doc, owner, moved, &doc.descendants(id));
                    d = (snapped.x - r.x, snapped.y - r.y);
                }
                *delta = d;
            }
            Some(Drag::Resize {
                handle,
                orig,
                rotation,
                keep_aspect,
                rect,
                ..
            }) => {
                *rect = resize_rect(
                    *orig,
                    *rotation,
                    *handle,
                    p,
                    *keep_aspect || modifiers.shift,
                );
            }
            Some(Drag::Rotate {
                center,
                start_angle,
                orig,
                angle,
                ..
            }) => {
                let a = (p.y - center.y).atan2(p.x - center.x).to_degrees();
                let mut deg = *orig + a - *start_angle;
                if modifiers.shift {
                    deg = (deg / 15.0).round() * 15.0;
                }
                *angle = deg.rem_euclid(360.0);
            }
            Some(Drag::Create { current, .. }) | Some(Drag::Marquee { current, .. }) => {
                *current = p
            }
            Some(Drag::Guide { .. }) => {}
            Some(Drag::TextSelect) => {
                if let (Some(layout), Some(e)) = (layout, app.view().and_then(|v| v.edit.clone()))
                    && let Some(leaf) = hit_leaf(doc, owner, p, pad)
                    && doc.story_of(leaf) == Some(e.story)
                    && let Some(at) = textedit::offset_at(doc, layout, e.story, leaf, p)
                    && let Some(v) = app.view_mut()
                    && let Some(ed) = &mut v.edit
                {
                    ed.set(at, true);
                }
            }
            None => {}
        }
        if let Some(v) = app.view_mut() {
            v.drag = drag;
        }
    }
    if resp.drag_stopped()
        && matches!(
            app.view().and_then(|v| v.drag.as_ref()),
            Some(Drag::Guide { .. })
        )
    {
        crate::rulers::finish_drag(app);
    }
    if resp.drag_stopped() {
        let drag = app.view_mut().and_then(|v| v.drag.take());
        match drag {
            Some(Drag::Move { frames, delta, .. }) if delta != (0.0, 0.0) => {
                let commands = frames
                    .iter()
                    .map(|(f, r)| Command::SetFrameRect {
                        frame: *f,
                        rect: r.translate(delta.0, delta.1),
                    })
                    .collect();
                app.exec(Command::Batch {
                    label: "move".into(),
                    commands,
                });
            }
            Some(Drag::Resize {
                frame, rect, orig, ..
            }) if rect != orig => {
                app.exec(Command::SetFrameRect { frame, rect });
            }
            Some(Drag::Rotate {
                frame, angle, orig, ..
            }) if (angle - orig).abs() > 1e-6 => {
                app.exec(Command::SetRotation {
                    frame,
                    degrees: angle,
                });
            }
            Some(Drag::Create { start, current }) => {
                let tool = app.tool;
                create_frame(app, tool, Rect::from_points(start, current), owner);
            }
            Some(Drag::Marquee { start, current }) => {
                let found = frames_in_rect(doc, owner, Rect::from_points(start, current));
                let mut sel = if modifiers.shift {
                    selection.clone()
                } else {
                    Vec::new()
                };
                for f in found {
                    if !sel.contains(&f) {
                        sel.push(f);
                    }
                }
                app.select(sel);
            }
            _ => {}
        }
    }
    // ---- keyboard
    // While editing, Tab is text: egui moves the focus on Tab unless the
    // canvas already holds it, and a Tab press may be the first thing it
    // sees after a dialog closed, so take the focus back for it.
    let tab_event = app.view().is_some_and(|v| v.edit.is_some())
        && app.dialog.is_none()
        && ctx.input(|i| {
            i.events.iter().any(|e| {
                matches!(
                    e,
                    egui::Event::Key {
                        key: egui::Key::Tab,
                        ..
                    }
                )
            })
        });
    let canvas_focus = tab_event || ctx.memory(|m| m.has_focus(canvas_id) || m.focused().is_none());
    if ctx.text_edit_focused() || !canvas_focus {
        return;
    }
    // The spelling menu or the Spelling window has the keyboard.
    if app.spell.wants_keys() {
        return;
    }
    if app.dialog.is_some() {
        // A modal dialog (Paragraph…, Drop Cap… opened while editing) owns
        // the keyboard: its Enter must not also type a paragraph break.
    } else if app.view().is_some_and(|v| v.edit.is_some()) {
        ctx.memory_mut(|m| {
            // After a dialog closes nothing has focus, and egui would move
            // it on the first Tab: keep it on the canvas while editing.
            if !m.has_focus(canvas_id) {
                m.request_focus(canvas_id);
            }
            m.set_focus_lock_filter(
                canvas_id,
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            )
        });
        text_keys(app, &ctx, doc, layout);
    } else {
        object_keys(app, &ctx, doc, &selection, layout);
    }
}

fn object_keys(
    app: &mut PubrsApp,
    ctx: &egui::Context,
    doc: &Document,
    selection: &[FrameId],
    layout: Option<&pubrs_api::layout::DocumentLayout>,
) {
    let (mut dx, mut dy) = (0.0, 0.0);
    let step = if ctx.input(|i| i.modifiers.shift) {
        10.0
    } else {
        1.0
    };
    ctx.input(|i| {
        for e in &i.events {
            if let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = e
                && !modifiers.command
            {
                match key {
                    egui::Key::ArrowLeft => dx -= step,
                    egui::Key::ArrowRight => dx += step,
                    egui::Key::ArrowUp => dy -= step,
                    egui::Key::ArrowDown => dy += step,
                    _ => {}
                }
            }
        }
    });
    if (dx != 0.0 || dy != 0.0) && !selection.is_empty() {
        let commands = selection
            .iter()
            .filter_map(|f| doc.frame(*f))
            .map(|f| Command::SetFrameRect {
                frame: f.id,
                rect: f.rect.translate(dx, dy),
            })
            .collect();
        app.exec_merge(
            Command::Batch {
                label: "nudge".into(),
                commands,
            },
            "nudge",
        );
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.select(Vec::new());
        app.tool = Tool::Select;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Enter))
        && let [f] = selection
        && let Some(story) = doc.story_of(*f)
    {
        let end = doc.story(story).map_or(0, |s| s.char_len());
        let _ = layout;
        begin_edit(app, *f, story, end);
    }
}

pub fn begin_edit(app: &mut PubrsApp, frame: FrameId, story: pubrs_api::core::StoryId, at: usize) {
    if let Some(v) = app.view_mut() {
        v.selection = vec![frame];
        v.edit = Some(TextEdit::new(frame, story, at));
    }
}

fn text_keys(
    app: &mut PubrsApp,
    ctx: &egui::Context,
    doc: &Document,
    layout: Option<&pubrs_api::layout::DocumentLayout>,
) {
    let Some(layout) = layout else { return };
    let events = ctx.input(|i| i.events.clone());
    for ev in events {
        let Some(e) = app.view().and_then(|v| v.edit.clone()) else {
            return;
        };
        let story = e.story;
        let len = doc.story(story).map_or(0, |s| s.char_len());
        match ev {
            egui::Event::Text(t) => edit_insert(app, &e, &t),
            egui::Event::Paste(t) => edit_insert(app, &e, &t),
            egui::Event::Copy | egui::Event::Cut => {
                if e.has_selection() {
                    let (s, t) = e.range();
                    let text: String = doc
                        .story(story)
                        .map(|st| st.plain_text().chars().skip(s).take(t - s).collect())
                        .unwrap_or_default();
                    app.platform.clipboard.set_text(&text);
                    ctx.copy_text(text);
                    if matches!(ev, egui::Event::Cut) {
                        edit_insert(app, &e, "");
                    }
                }
            }
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                let extend = modifiers.shift;
                let word = modifiers.command || modifiers.alt;
                let chars: Vec<char> = doc
                    .story(story)
                    .map(|s| s.plain_text().chars().collect())
                    .unwrap_or_default();
                let set = |app: &mut PubrsApp, at: usize, goal: Option<f64>| {
                    if let Some(v) = app.view_mut()
                        && let Some(ed) = &mut v.edit
                    {
                        ed.set(at.min(len), extend);
                        ed.goal_x = goal;
                    }
                };
                match key {
                    egui::Key::Escape => {
                        let f = e.frame;
                        app.select(vec![f]);
                    }
                    // Cmd/Ctrl+Enter is a page break (a shortcut), not a
                    // paragraph break.
                    egui::Key::Enter if types_paragraph_break(modifiers) => {
                        edit_insert(app, &e, "\n")
                    }
                    egui::Key::F10 if modifiers.shift => {
                        // The spelling menu for the word at the caret.
                        let anchor = textedit::caret_segment(doc, layout, story, e.caret)
                            .zip(app.view().and_then(|v| v.xf))
                            .map(|((_, _, bottom), xf)| xf.to_screen(bottom));
                        if let (Some(anchor), Some(doc_id)) = (anchor, app.doc_id()) {
                            crate::spell::open_menu(app, doc_id, doc, story, e.caret, anchor, true);
                        }
                    }
                    egui::Key::Tab => tab_key(app, doc, &e, modifiers),
                    egui::Key::Backspace => edit_delete(app, &e, false, word, &chars),
                    egui::Key::Delete => edit_delete(app, &e, true, word, &chars),
                    egui::Key::ArrowLeft => {
                        let at = if e.has_selection() && !extend {
                            e.range().0
                        } else if word {
                            textedit::word_boundary(&chars, e.caret, false)
                        } else {
                            e.caret.saturating_sub(1)
                        };
                        set(app, at, None);
                    }
                    egui::Key::ArrowRight => {
                        let at = if e.has_selection() && !extend {
                            e.range().1
                        } else if word {
                            textedit::word_boundary(&chars, e.caret, true)
                        } else {
                            e.caret + 1
                        };
                        set(app, at, None);
                    }
                    egui::Key::ArrowUp | egui::Key::ArrowDown => {
                        let dir = if key == egui::Key::ArrowUp { -1 } else { 1 };
                        if let Some((at, x)) =
                            textedit::vertical(doc, layout, story, e.caret, dir, e.goal_x)
                        {
                            set(app, at, Some(x));
                        }
                    }
                    egui::Key::Home => {
                        let at = if modifiers.command {
                            0
                        } else {
                            textedit::line_edge(doc, layout, story, e.caret, false).unwrap_or(0)
                        };
                        set(app, at, None);
                    }
                    egui::Key::End => {
                        let at = if modifiers.command {
                            len
                        } else {
                            textedit::line_edge(doc, layout, story, e.caret, true).unwrap_or(len)
                        };
                        set(app, at, None);
                    }
                    egui::Key::A if modifiers.command => {
                        if let Some(v) = app.view_mut()
                            && let Some(ed) = &mut v.edit
                        {
                            ed.anchor = 0;
                            ed.caret = len;
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

/// Replace the selection (or insert at the caret) with `text`.
/// Whether Enter with `m` held types a paragraph break while editing.
/// With Cmd or Ctrl it is the page-break shortcut instead: egui consumes
/// it on Windows and Linux, and on macOS the native menu takes it, but a
/// stray event must still not split the paragraph.
fn types_paragraph_break(m: egui::Modifiers) -> bool {
    !(m.command || m.ctrl || m.mac_cmd)
}

pub fn edit_insert(app: &mut PubrsApp, e: &TextEdit, text: &str) {
    let text = text.replace('\r', "");
    let n = text.chars().count();
    let (s, t) = e.range();
    let ok = if s != t {
        app.exec(Command::ReplaceText {
            story: e.story,
            start: s,
            end: t,
            text: text.clone(),
        })
    } else if n > 0 {
        let fmt: Option<RunFormat> = None;
        app.exec_merge(
            Command::InsertText {
                story: e.story,
                at: s,
                text: text.clone(),
                format: fmt,
            },
            &format!("type-{}", e.story),
        )
    } else {
        false
    };
    if (ok || s != t)
        && let Some(v) = app.view_mut()
        && let Some(ed) = &mut v.edit
    {
        ed.set(s + n, false);
        ed.goal_x = None;
    }
}

fn edit_delete(app: &mut PubrsApp, e: &TextEdit, forward: bool, word: bool, chars: &[char]) {
    let (s, t) = if e.has_selection() {
        e.range()
    } else if forward {
        let end = if word {
            textedit::word_boundary(chars, e.caret, true)
        } else {
            e.caret + 1
        };
        (e.caret, end.min(chars.len()))
    } else {
        let start = if word {
            textedit::word_boundary(chars, e.caret, false)
        } else {
            e.caret.saturating_sub(1)
        };
        (start, e.caret)
    };
    if s == t {
        return;
    }
    if app.exec_merge(
        Command::DeleteText {
            story: e.story,
            start: s,
            end: t,
        },
        &format!("del-{}", e.story),
    ) && let Some(v) = app.view_mut()
        && let Some(ed) = &mut v.edit
    {
        ed.set(s, false);
        ed.goal_x = None;
    }
}

/// Create a frame with the active tool in `r` (a click makes a default size).
pub fn create_frame(app: &mut PubrsApp, tool: Tool, mut r: Rect, owner: Owner) {
    let Owner::Page(page) = owner else { return };
    let dragged = r.width >= 4.0 || r.height >= 4.0;
    if !dragged {
        let (w, h) = match tool {
            Tool::Line => (144.0, 0.0),
            Tool::Text => (216.0, 72.0),
            _ => (144.0, 108.0),
        };
        r = Rect::new(r.x, r.y, w, h);
    }
    let doc_id = app.doc_id();
    let created = match tool {
        Tool::Select => return,
        Tool::Text => app
            .session
            .frame_add_text(FrameAddText {
                doc_id,
                page_id: Some(page),
                master_id: None,
                x: r.x,
                y: r.y,
                width: r.width.max(24.0),
                height: r.height.max(18.0),
                text: None,
                markup: false,
                style: None,
                font_family: None,
                font_size: None,
                color: None,
                align: None,
                columns: None,
                name: None,
            })
            .map(|o| (o.frame_id, o.story_id)),
        Tool::Picture => app
            .session
            .frame_add_image(FrameAddImage {
                doc_id,
                page_id: Some(page),
                master_id: None,
                x: r.x,
                y: r.y,
                width: r.width.max(12.0),
                height: r.height.max(12.0),
                path: None,
                base64: None,
                svg: None,
                filename: None,
                fit: None,
                name: None,
            })
            .map(|o| (o.frame_id, None)),
        Tool::Rectangle | Tool::Ellipse | Tool::Line => {
            let line_dir = if tool == Tool::Line {
                // A drag from bottom-left to top-right is an up-right line.
                Some(LineDirection::DownRight)
            } else {
                None
            };
            app.session
                .frame_add_shape(FrameAddShape {
                    doc_id,
                    page_id: Some(page),
                    master_id: None,
                    shape: match tool {
                        Tool::Rectangle => ShapeKind::Rectangle,
                        Tool::Ellipse => ShapeKind::Ellipse,
                        _ => ShapeKind::Line,
                    },
                    x: r.x,
                    y: r.y,
                    width: r.width,
                    height: if tool == Tool::Line {
                        r.height
                    } else {
                        r.height.max(1.0)
                    },
                    corner_radius: None,
                    line_direction: line_dir,
                    fill: (tool != Tool::Line).then_some(pubrs_api::tools::OptColor(Some(
                        Color::rgb(0xDB, 0xEA, 0xFE).into(),
                    ))),
                    stroke: Some(pubrs_api::tools::OptColor(Some(
                        Color::rgb(0x1E, 0x3A, 0x5F).into(),
                    ))),
                    stroke_width: Some(if tool == Tool::Line { 2.0 } else { 1.0 }),
                    dash: None,
                    name: None,
                })
                .map(|o| (o.frame_id, None))
        }
    };
    app.tool = Tool::Select;
    let Some((frame, story)) = app.report(created) else {
        return;
    };
    app.select(vec![frame]);
    match tool {
        Tool::Text => {
            if let Some(s) = story {
                begin_edit(app, frame, s, 0);
            }
        }
        Tool::Picture => {
            if let Some(p) = app.platform.dialogs.pick_image() {
                app.place_image(frame, &p);
            }
        }
        _ => {}
    }
}

/// Text of a story (for the self-test and status).
pub fn story_text(app: &mut PubrsApp, frame: FrameId) -> Option<String> {
    let id = app.doc_id();
    app.session
        .text_get(TextRef {
            doc_id: id,
            frame_id: Some(frame),
            story_id: None,
        })
        .ok()
        .map(|t| t.text)
}

/// One ruler guide across the page, with its position in points on `axis`.
fn draw_guide(
    painter: &egui::Painter,
    xf: PageXf,
    bleed_rect: egui::Rect,
    axis: pubrs_api::core::Axis,
    position: f64,
    color: Color32,
) {
    match axis {
        pubrs_api::core::Axis::Vertical => {
            let x = xf.to_screen(Point::new(position, 0.0)).x;
            painter.line_segment(
                [
                    Pos2::new(x, bleed_rect.top()),
                    Pos2::new(x, bleed_rect.bottom()),
                ],
                Stroke::new(1.0, color),
            );
        }
        pubrs_api::core::Axis::Horizontal => {
            let y = xf.to_screen(Point::new(0.0, position)).y;
            painter.line_segment(
                [
                    Pos2::new(bleed_rect.left(), y),
                    Pos2::new(bleed_rect.right(), y),
                ],
                Stroke::new(1.0, color),
            );
        }
    }
}

#[cfg(test)]
mod view_key_tests {
    use pubrs_api::core::{DocumentId, SeqIds};

    use super::*;

    fn want(page_px: (u32, u32), visible: PixelRect) -> Wanted {
        Wanted {
            doc_id: DocumentId::new(&mut SeqIds::new(1)),
            page_index: 0,
            dpi: 144.0,
            page_px,
            visible,
            generation: 0,
            max_side: 8192,
        }
    }

    #[test]
    fn small_pans_keep_the_region() {
        let w = want((1224, 1584), PixelRect::new(100, 100, 800, 600));
        let k = choose_key(&w, None, None).expect("key");
        let panned = Wanted {
            visible: PixelRect::new(140, 120, 800, 600),
            ..w
        };
        assert_eq!(choose_key(&panned, None, Some(k)), Some(k));
        let far = Wanted {
            visible: PixelRect::new(400, 900, 800, 600),
            ..w
        };
        assert_ne!(choose_key(&far, None, Some(k)), Some(k));
    }

    /// Letter → A5 (or less bleed) at the same zoom is a new key whose
    /// region lies inside the new page raster.
    #[test]
    fn a_page_size_change_is_a_new_key() {
        let letter = want((1224, 1584), PixelRect::new(0, 0, 1224, 1584));
        let k = choose_key(&letter, None, None).expect("key");
        let a5 = want((840, 1191), PixelRect::new(0, 0, 840, 1191));
        let k2 = choose_key(&a5, Some(k), Some(k)).expect("key");
        assert_ne!(k2, k);
        assert_eq!(k2.page_px, (840, 1191));
        assert!(PixelRect::new(0, 0, 840, 1191).contains(&k2.region));
    }

    #[test]
    fn regions_fit_the_largest_texture() {
        // A 6K portrait screen at 2x and a high zoom: the margin gives way.
        let w = Wanted {
            max_side: 8192,
            ..want((40_000, 50_000), PixelRect::new(9000, 9000, 3384, 6016))
        };
        let k = choose_key(&w, None, None).expect("key");
        assert!(k.region.width <= 8192 && k.region.height <= 8192, "{k:?}");
        assert!(k.region.contains(&w.visible));
        // Visible pixels beyond the limit lower the texture resolution.
        assert_eq!(texture_dpi(144.0, 3000, 2000, 8192), 144.0);
        let dpi = texture_dpi(144.0, 10_000, 2000, 8192);
        assert!(dpi < 144.0 && 10_000.0 * dpi / 144.0 <= 8190.0, "{dpi}");
    }
}

#[cfg(test)]
mod enter_tests {
    use eframe::egui::{Context, Event, Key, Modifiers, RawInput};

    use super::types_paragraph_break;
    use crate::keymap::{Action, shortcut_actions};

    #[test]
    fn only_plain_or_shifted_enter_types_a_paragraph_break() {
        assert!(types_paragraph_break(Modifiers::NONE));
        assert!(types_paragraph_break(Modifiers::SHIFT));
        assert!(!types_paragraph_break(Modifiers::COMMAND));
        assert!(!types_paragraph_break(Modifiers::CTRL));
        assert!(!types_paragraph_break(Modifiers::MAC_CMD));
        assert!(!types_paragraph_break(
            Modifiers::COMMAND | Modifiers::SHIFT
        ));
    }

    /// The egui path (Windows, Linux, and macOS without the native menu):
    /// Cmd/Ctrl+Enter and Cmd/Ctrl+Shift+Enter are consumed as the page
    /// and column break shortcuts, so the canvas never sees the Enter.
    #[test]
    fn command_enter_is_consumed_as_a_break_shortcut() {
        for (mods, want) in [
            (Modifiers::COMMAND, Action::InsertPageBreak),
            (
                Modifiers::COMMAND | Modifiers::SHIFT,
                Action::InsertColumnBreak,
            ),
        ] {
            let ctx = Context::default();
            let mut raw = RawInput::default();
            raw.events.push(Event::Key {
                key: Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: mods,
            });
            let mut out = ctx.run_ui(raw, |ui| {
                let ctx = ui.ctx().clone();
                let hit = shortcut_actions()
                    .into_iter()
                    .find(|(sc, _)| ctx.input_mut(|i| i.consume_shortcut(sc)))
                    .map(|(_, a)| a);
                assert_eq!(hit, Some(want));
                let left = ctx.input(|i| {
                    i.events.iter().any(|e| {
                        matches!(
                            e,
                            Event::Key {
                                key: Key::Enter,
                                ..
                            }
                        )
                    })
                });
                assert!(!left, "{want:?}: the Enter key is still queued");
            });
            out.textures_delta.clear();
        }
    }
}

/// Tab while editing text. At the very start of a list item it makes the
/// item one level deeper, and Shift+Tab one level shallower (one undo step
/// each); anywhere else Tab inserts a tab character. Shortcuts with
/// Cmd/Ctrl or Alt are not list gestures.
fn tab_key(app: &mut PubrsApp, doc: &Document, e: &TextEdit, m: egui::Modifiers) {
    let list_item = || {
        let s = doc.story(e.story)?;
        let (pi, c) = s.locate(e.range().0);
        doc.styles.resolve_para(&s.paragraphs[pi]).list?;
        Some((s, pi, c))
    };
    if !(m.command || m.alt)
        && let Some((s, pi, c)) = list_item()
        && (m.shift || (c == 0 && !e.has_selection()))
    {
        let delta = if m.shift { -1 } else { 1 };
        if let Some(cmd) = pubrs_api::core::list_level_command(e.story, s, &doc.styles, pi, delta) {
            app.exec(cmd);
        }
        return;
    }
    if !m.shift {
        edit_insert(app, e, "\t");
    }
}

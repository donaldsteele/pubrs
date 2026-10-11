//! Rulers along the canvas, and guides dragged from them.
//!
//! The rulers sit outside the scrolling canvas and follow it: zero is the
//! page's top-left trim corner, ticks are in the user's unit and are spaced in
//! screen points (so they look the same on any display density) and snapped to
//! device pixels (so they stay sharp on HiDPI ones).

use eframe::egui::{self, Color32, Pos2, Sense, Stroke, Vec2};
use pubrs_api::core::{Axis, Command, Guide, GuideId, Owner};

use crate::app::{PubrsApp, Units};
use crate::view::{Drag, PageXf};

/// Thickness of a ruler in screen points.
pub const RULER: f32 = 20.0;
const BG: Color32 = Color32::from_gray(238);
const INK: Color32 = Color32::from_gray(70);
const TICK: Color32 = Color32::from_gray(120);
const MARK: Color32 = Color32::from_rgb(37, 99, 235);
const GUIDE: Color32 = Color32::from_rgb(6, 182, 212);
/// Smallest distance between two labelled ticks, in screen points.
const MIN_LABEL_GAP: f32 = 64.0;
/// Smallest distance between two minor ticks, in screen points.
const MIN_TICK_GAP: f32 = 5.0;
/// How near (screen points) the pointer must be to grab a guide.
pub const GRAB: f32 = 4.0;

/// (label step, minor divisions per step) in the unit, coarse to fine fits.
fn steps(units: Units) -> &'static [(f64, u32)] {
    match units {
        Units::Inches => &[
            (0.125, 2),
            (0.25, 4),
            (0.5, 2),
            (1.0, 4),
            (2.0, 4),
            (5.0, 5),
            (10.0, 10),
            (20.0, 4),
            (50.0, 5),
            (100.0, 10),
        ],
        Units::Millimetres | Units::Points => &[
            (1.0, 10),
            (2.0, 4),
            (5.0, 5),
            (10.0, 10),
            (20.0, 4),
            (50.0, 5),
            (100.0, 10),
            (200.0, 4),
            (500.0, 5),
            (1000.0, 10),
            (2000.0, 4),
            (5000.0, 5),
        ],
    }
}

/// The label step and minor divisions for `screen_per_unit` screen points per unit.
pub fn pick_step(units: Units, screen_per_unit: f64) -> (f64, u32) {
    let all = steps(units);
    all.iter()
        .copied()
        .find(|(s, _)| s * screen_per_unit >= f64::from(MIN_LABEL_GAP))
        .unwrap_or(all[all.len() - 1])
}

/// A ruler value as text: up to three decimals, no trailing zeros.
pub fn format_value(v: f64) -> String {
    if v.abs() < 5e-4 {
        return "0".into();
    }
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The area left for the canvas when the rulers take their strips.
pub fn canvas_rect(full: egui::Rect, show: bool) -> egui::Rect {
    if show {
        egui::Rect::from_min_max(full.min + Vec2::splat(RULER), full.max)
    } else {
        full
    }
}

fn snap(v: f32, ppp: f32) -> f32 {
    (v * ppp).round() / ppp
}

/// Draw one ruler. `axis_len` is along the ruler; `to_screen` maps a page
/// position in points to a coordinate along it.
struct Ruler {
    rect: egui::Rect,
    horizontal: bool,
    /// Screen coordinate of page position 0 along the ruler.
    zero: f32,
    /// Screen points per page point.
    scale: f32,
}

impl Ruler {
    fn at(&self, a: f32, b: f32) -> Pos2 {
        if self.horizontal {
            Pos2::new(a, b)
        } else {
            Pos2::new(b, a)
        }
    }

    fn paint(&self, ui: &egui::Ui, units: Units, marks: &[(f32, f32)], guide: Option<f32>) {
        let ppp = ui.ctx().pixels_per_point();
        let painter = ui.painter_at(self.rect);
        painter.rect_filled(self.rect, 0.0, BG);
        let (lo, hi) = if self.horizontal {
            (self.rect.left(), self.rect.right())
        } else {
            (self.rect.top(), self.rect.bottom())
        };
        // Selection bounds first, under the ticks.
        let across = |r: &egui::Rect| {
            if self.horizontal {
                (r.top(), r.bottom())
            } else {
                (r.left(), r.right())
            }
        };
        let (c0, c1) = across(&self.rect);
        for (a, b) in marks {
            let (a, b) = (a.max(lo), b.min(hi));
            if b >= a {
                let band = egui::Rect::from_two_pos(self.at(a, c0), self.at(b, c1));
                painter.rect_filled(band, 0.0, MARK.gamma_multiply(0.22));
            }
        }
        let per_point = units.per_point();
        let spu = f64::from(self.scale) / per_point;
        let (step, divs) = pick_step(units, spu);
        let minor = step / f64::from(divs);
        let minor_px = minor * spu;
        let to_value = |a: f32| f64::from(a - self.zero) / f64::from(self.scale) * per_point;
        let (v0, v1) = (to_value(lo), to_value(hi));
        let k0 = (v0 / minor).floor() as i64 - 1;
        let k1 = (v1 / minor).ceil() as i64 + 1;
        let font = egui::FontId::proportional(10.0);
        let ticks_visible = minor_px >= f64::from(MIN_TICK_GAP);
        for k in k0..=k1 {
            let v = k as f64 * minor;
            let a = snap(self.zero + (v / per_point) as f32 * self.scale, ppp);
            if a < lo - 1.0 || a > hi + 1.0 {
                continue;
            }
            let major = k.rem_euclid(i64::from(divs)) == 0;
            let half = divs % 2 == 0 && k.rem_euclid(i64::from(divs)) == i64::from(divs / 2);
            if !major && !ticks_visible {
                continue;
            }
            let len = if major {
                RULER * 0.62
            } else if half {
                RULER * 0.4
            } else {
                RULER * 0.24
            };
            let (b0, b1) = if self.horizontal {
                (self.rect.bottom() - len, self.rect.bottom())
            } else {
                (self.rect.right() - len, self.rect.right())
            };
            painter.line_segment(
                [self.at(a, b0), self.at(a, b1)],
                Stroke::new(1.0, if major { INK } else { TICK }),
            );
            if major {
                let text = format_value(v);
                if self.horizontal {
                    painter.text(
                        Pos2::new(a + 3.0, self.rect.top() + 2.0),
                        egui::Align2::LEFT_TOP,
                        text,
                        font.clone(),
                        INK,
                    );
                } else {
                    // Reads bottom to top, starting just below its tick.
                    let galley = painter.layout_no_wrap(text, font.clone(), INK);
                    let pos = Pos2::new(self.rect.left() + 2.0, a + 3.0 + galley.size().x);
                    painter.add(
                        egui::epaint::TextShape::new(pos, galley, INK)
                            .with_angle(-std::f32::consts::FRAC_PI_2),
                    );
                }
            }
        }
        for (a, b) in marks {
            for e in [*a, *b] {
                if (lo..=hi).contains(&e) {
                    let e = snap(e, ppp);
                    painter.line_segment([self.at(e, c0), self.at(e, c1)], Stroke::new(1.0, MARK));
                }
            }
        }
        if let Some(g) = guide
            && (lo..=hi).contains(&g)
        {
            let g = snap(g, ppp);
            painter.line_segment([self.at(g, c0), self.at(g, c1)], Stroke::new(1.0, GUIDE));
        }
        // The edge towards the canvas.
        let edge = if self.horizontal {
            [self.rect.left_bottom(), self.rect.right_bottom()]
        } else {
            [self.rect.right_top(), self.rect.right_bottom()]
        };
        painter.line_segment(edge, Stroke::new(1.0, Color32::from_gray(180)));
    }
}

/// Screen bounds of the selection along each axis, as (min, max) pairs.
type Marks = Vec<(f32, f32)>;

fn selection_marks(app: &PubrsApp, xf: PageXf) -> (Marks, Marks) {
    let (Some(v), Some(doc)) = (app.view(), app.doc()) else {
        return (Vec::new(), Vec::new());
    };
    let mut x = (f32::MAX, f32::MIN);
    let mut y = (f32::MAX, f32::MIN);
    let mut any = false;
    for id in &v.selection {
        let Some(f) = doc.frame(*id) else { continue };
        for c in xf.corners(f.rect, f.rotation) {
            x = (x.0.min(c.x), x.1.max(c.x));
            y = (y.0.min(c.y), y.1.max(c.y));
            any = true;
        }
    }
    if any {
        (vec![x], vec![y])
    } else {
        (Vec::new(), Vec::new())
    }
}

/// Draw the rulers in the strips of `full` and run guide drags that start on them.
pub fn show(app: &mut PubrsApp, ui: &mut egui::Ui, full: egui::Rect) {
    let Some(xf) = app.view().and_then(|v| v.xf) else {
        return;
    };
    let units = app.units;
    let top_rect = egui::Rect::from_min_max(
        Pos2::new(full.left() + RULER, full.top()),
        Pos2::new(full.right(), full.top() + RULER),
    );
    let left_rect = egui::Rect::from_min_max(
        Pos2::new(full.left(), full.top() + RULER),
        Pos2::new(full.left() + RULER, full.bottom()),
    );
    let (xs, ys) = selection_marks(app, xf);
    // A guide being dragged shows on both rulers.
    let guide = match app.view().and_then(|v| v.drag.as_ref()) {
        Some(Drag::Guide {
            axis, pos, inside, ..
        }) if *inside => Some((*axis, *pos)),
        _ => None,
    };
    let (gx, gy) = (
        guide
            .filter(|(a, _)| *a == Axis::Vertical)
            .map(|(_, p)| xf.origin.x + p as f32 * xf.scale),
        guide
            .filter(|(a, _)| *a == Axis::Horizontal)
            .map(|(_, p)| xf.origin.y + p as f32 * xf.scale),
    );
    Ruler {
        rect: top_rect,
        horizontal: true,
        zero: xf.origin.x,
        scale: xf.scale,
    }
    .paint(ui, units, &xs, gx);
    Ruler {
        rect: left_rect,
        horizontal: false,
        zero: xf.origin.y,
        scale: xf.scale,
    }
    .paint(ui, units, &ys, gy);
    let corner = egui::Rect::from_min_size(full.min, Vec2::splat(RULER));
    let cp = ui.painter_at(corner);
    cp.rect_filled(corner, 0.0, BG);
    cp.text(
        corner.center(),
        egui::Align2::CENTER_CENTER,
        units.suffix().trim(),
        egui::FontId::proportional(9.0),
        INK,
    );
    cp.line_segment(
        [corner.left_bottom(), corner.right_bottom()],
        Stroke::new(1.0, Color32::from_gray(180)),
    );
    cp.line_segment(
        [corner.right_top(), corner.right_bottom()],
        Stroke::new(1.0, Color32::from_gray(180)),
    );

    // Dragging out of a ruler makes a guide: the top ruler a horizontal one.
    for (rect, axis, id, cursor) in [
        (
            top_rect,
            Axis::Horizontal,
            "ruler-top",
            egui::CursorIcon::ResizeVertical,
        ),
        (
            left_rect,
            Axis::Vertical,
            "ruler-left",
            egui::CursorIcon::ResizeHorizontal,
        ),
    ] {
        let resp = ui.interact(rect, egui::Id::new(id), Sense::drag());
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(cursor);
        }
        if resp.drag_started() {
            let Some(owner) = app.page_owner() else {
                continue;
            };
            app.show_guides = true;
            if let Some(v) = app.view_mut() {
                v.drag = Some(Drag::Guide {
                    owner,
                    id: None,
                    axis,
                    orig: None,
                    pos: 0.0,
                    inside: false,
                });
                v.edit = None;
            }
        }
        if resp.dragged()
            && let Some(pos) = resp.interact_pointer_pos()
        {
            drag_update(app, pos);
        }
        if resp.drag_stopped() {
            finish_drag(app);
        }
    }
    // Escape abandons a guide drag.
    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape))
        && app
            .view()
            .is_some_and(|v| matches!(v.drag, Some(Drag::Guide { .. })))
        && let Some(v) = app.view_mut()
    {
        v.drag = None;
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Follow the pointer with the guide being dragged.
pub fn drag_update(app: &mut PubrsApp, pointer: Pos2) {
    let size = app.doc().map(|d| d.setup.size);
    let Some(v) = app.view_mut() else { return };
    let (Some(xf), Some(size)) = (v.xf, size) else {
        return;
    };
    let canvas = v.canvas_rect;
    let p = xf.to_page(pointer);
    let inside = p.x >= 0.0
        && p.y >= 0.0
        && p.x <= size.width
        && p.y <= size.height
        && canvas.is_none_or(|c| c.contains(pointer));
    if let Some(Drag::Guide {
        axis,
        pos,
        inside: ins,
        ..
    }) = &mut v.drag
    {
        *pos = round2(if *axis == Axis::Vertical { p.x } else { p.y });
        *ins = inside;
    }
}

/// Commit the guide drag as one command: add, move or (off the page) remove.
pub fn finish_drag(app: &mut PubrsApp) {
    let Some(Drag::Guide {
        owner,
        id,
        axis,
        orig,
        pos,
        inside,
    }) = app.view_mut().and_then(|v| v.drag.take())
    else {
        return;
    };
    match (id, inside) {
        (None, true) => {
            let guide = Guide {
                id: GuideId::new(app.session.ids()),
                axis,
                position: pos,
            };
            app.exec(Command::AddGuide { owner, guide });
        }
        (Some(guide), true) if orig != Some(pos) => {
            app.exec(Command::MoveGuide {
                owner,
                guide,
                position: pos,
            });
        }
        (Some(guide), false) => {
            app.exec(Command::RemoveGuide { owner, guide });
        }
        _ => {}
    }
}

/// The guide of the page under `pointer` (screen), if any.
pub fn guide_at(
    guides: &[Guide],
    xf: PageXf,
    pointer: Pos2,
    owner: Owner,
) -> Option<(Owner, Guide)> {
    guides
        .iter()
        .filter_map(|g| {
            let d = match g.axis {
                Axis::Vertical => (xf.origin.x + g.position as f32 * xf.scale - pointer.x).abs(),
                Axis::Horizontal => (xf.origin.y + g.position as f32 * xf.scale - pointer.y).abs(),
            };
            (d <= GRAB).then_some((d, g))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, g)| (owner, *g))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_scale_with_zoom() {
        // 100% zoom: 72 points per inch is 72 screen points per unit.
        let (s, _) = pick_step(Units::Inches, 72.0);
        assert_eq!(s, 1.0);
        // Zoomed far out, labels thin out instead of overlapping.
        let (s, _) = pick_step(Units::Inches, 8.0);
        assert!(s * 8.0 >= f64::from(MIN_LABEL_GAP));
        // Zoomed far in, finer steps.
        let (s, _) = pick_step(Units::Inches, 600.0);
        assert!(s < 1.0);
        let (s, _) = pick_step(Units::Millimetres, 2.83 * 4.0);
        assert!(s * 2.83 * 4.0 >= f64::from(MIN_LABEL_GAP));
    }

    #[test]
    fn values_format_cleanly() {
        assert_eq!(format_value(0.0), "0");
        assert_eq!(format_value(-0.0000001), "0");
        assert_eq!(format_value(2.5), "2.5");
        assert_eq!(format_value(10.0), "10");
        assert_eq!(format_value(0.125), "0.125");
        assert_eq!(format_value(-3.0), "-3");
    }
}

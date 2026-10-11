//! Per-document view state and page/screen geometry.

use std::collections::HashMap;

use eframe::egui::{self, Pos2, Vec2};
use pubrs_api::core::{Document, FrameId, Owner, Parent, Point, Rect};

use crate::render_worker::ViewKey;
use crate::textedit::TextEdit;

/// Page-space ↔ screen-space mapping for the page being shown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageXf {
    /// Screen position of the page's trim top-left.
    pub origin: Pos2,
    /// Screen points per page point.
    pub scale: f32,
}

impl PageXf {
    pub fn to_screen(self, p: Point) -> Pos2 {
        Pos2::new(
            self.origin.x + p.x as f32 * self.scale,
            self.origin.y + p.y as f32 * self.scale,
        )
    }

    pub fn to_page(self, s: Pos2) -> Point {
        Point::new(
            f64::from((s.x - self.origin.x) / self.scale),
            f64::from((s.y - self.origin.y) / self.scale),
        )
    }

    pub fn rect_to_screen(self, r: Rect) -> egui::Rect {
        egui::Rect::from_min_max(
            self.to_screen(Point::new(r.x, r.y)),
            self.to_screen(Point::new(r.right(), r.bottom())),
        )
    }

    /// Screen corners of a rotated frame rect (tl, tr, br, bl).
    pub fn corners(self, r: Rect, rotation: f64) -> [Pos2; 4] {
        r.rotated_corners(rotation).map(|p| self.to_screen(p))
    }
}

/// A resize handle, by position on the frame box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    NW,
    Rotate,
}

impl Handle {
    pub const RESIZE: [Handle; 8] = [
        Handle::NW,
        Handle::N,
        Handle::NE,
        Handle::E,
        Handle::SE,
        Handle::S,
        Handle::SW,
        Handle::W,
    ];

    /// Position in the unit square (0..1, 0..1) of the frame box.
    pub fn unit(self) -> (f64, f64) {
        match self {
            Handle::NW => (0.0, 0.0),
            Handle::N => (0.5, 0.0),
            Handle::NE => (1.0, 0.0),
            Handle::E => (1.0, 0.5),
            Handle::SE => (1.0, 1.0),
            Handle::S => (0.5, 1.0),
            Handle::SW => (0.0, 1.0),
            Handle::W => (0.0, 0.5),
            Handle::Rotate => (0.5, 0.0),
        }
    }

    pub fn is_corner(self) -> bool {
        matches!(self, Handle::NW | Handle::NE | Handle::SE | Handle::SW)
    }

    pub fn cursor(self) -> egui::CursorIcon {
        match self {
            Handle::N | Handle::S => egui::CursorIcon::ResizeVertical,
            Handle::E | Handle::W => egui::CursorIcon::ResizeHorizontal,
            Handle::NE | Handle::SW => egui::CursorIcon::ResizeNeSw,
            Handle::NW | Handle::SE => egui::CursorIcon::ResizeNwSe,
            Handle::Rotate => egui::CursorIcon::Alias,
        }
    }
}

/// Page point of a handle on a (rotated) frame.
pub fn handle_point(r: Rect, rotation: f64, h: Handle, rotate_offset: f64) -> Point {
    let (u, v) = h.unit();
    let mut p = Point::new(r.x + r.width * u, r.y + r.height * v);
    if h == Handle::Rotate {
        p.y -= rotate_offset;
    }
    p.rotated_about(r.center(), rotation)
}

/// New rect when dragging `handle` of `orig` (rotated by `rotation`) to page
/// point `p`, keeping the opposite side fixed on the page.
pub fn resize_rect(orig: Rect, rotation: f64, handle: Handle, p: Point, keep_aspect: bool) -> Rect {
    let c = orig.center();
    // Pointer in the frame's unrotated coordinate system.
    let local = p.rotated_about(c, -rotation);
    let (mut x0, mut y0, mut x1, mut y1) = (orig.left(), orig.top(), orig.right(), orig.bottom());
    match handle {
        Handle::N => y0 = local.y.min(y1 - 1.0),
        Handle::S => y1 = local.y.max(y0 + 1.0),
        Handle::W => x0 = local.x.min(x1 - 1.0),
        Handle::E => x1 = local.x.max(x0 + 1.0),
        Handle::NW => {
            x0 = local.x.min(x1 - 1.0);
            y0 = local.y.min(y1 - 1.0);
        }
        Handle::NE => {
            x1 = local.x.max(x0 + 1.0);
            y0 = local.y.min(y1 - 1.0);
        }
        Handle::SE => {
            x1 = local.x.max(x0 + 1.0);
            y1 = local.y.max(y0 + 1.0);
        }
        Handle::SW => {
            x0 = local.x.min(x1 - 1.0);
            y1 = local.y.max(y0 + 1.0);
        }
        Handle::Rotate => {}
    }
    if keep_aspect && handle.is_corner() && orig.width > 0.0 && orig.height > 0.0 {
        let aspect = orig.width / orig.height;
        let w = x1 - x0;
        let h = y1 - y0;
        if w / h > aspect {
            let nw = h * aspect;
            if matches!(handle, Handle::NW | Handle::SW) {
                x0 = x1 - nw;
            } else {
                x1 = x0 + nw;
            }
        } else {
            let nh = w / aspect;
            if matches!(handle, Handle::NW | Handle::NE) {
                y0 = y1 - nh;
            } else {
                y1 = y0 + nh;
            }
        }
    }
    let local_rect = Rect::new(x0, y0, x1 - x0, y1 - y0);
    // Rotate the new centre back so the fixed side stays put on the page.
    let new_center = local_rect.center().rotated_about(c, rotation);
    Rect::new(
        new_center.x - local_rect.width / 2.0,
        new_center.y - local_rect.height / 2.0,
        local_rect.width,
        local_rect.height,
    )
}

/// True if page point `p` is inside a frame (rotation aware, `pad` points of slack).
pub fn frame_contains(r: Rect, rotation: f64, p: Point, pad: f64) -> bool {
    let local = p.rotated_about(r.center(), -rotation);
    local.x >= r.left() - pad
        && local.x <= r.right() + pad
        && local.y >= r.top() - pad
        && local.y <= r.bottom() + pad
}

/// The topmost leaf frame at `p` on a page (master frames are not hit).
pub fn hit_leaf(doc: &Document, owner: Owner, p: Point, pad: f64) -> Option<FrameId> {
    doc.paint_order(owner).into_iter().rev().find(|id| {
        doc.frame(*id).is_some_and(|f| {
            let slack = if f.rect.width < 2.0 || f.rect.height < 2.0 {
                pad.max(4.0)
            } else {
                pad
            };
            frame_contains(f.rect, f.rotation, p, slack)
        })
    })
}

/// The page-level item (a group or the frame itself) containing `frame`.
pub fn top_level(doc: &Document, frame: FrameId) -> FrameId {
    let mut cur = frame;
    for _ in 0..64 {
        match doc.parent_of(cur) {
            Some(Parent::Group(g)) => cur = g,
            _ => break,
        }
    }
    cur
}

/// Frames entirely inside a page rect (top-level items).
pub fn frames_in_rect(doc: &Document, owner: Owner, area: Rect) -> Vec<FrameId> {
    doc.children(owner.into())
        .map(|l| {
            l.iter()
                .copied()
                .filter(|id| {
                    doc.frame(*id)
                        .is_some_and(|f| area.contains_rect(&f.bounds()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Current drag gesture on the canvas.
#[derive(Debug, Clone, PartialEq)]
pub enum Drag {
    Move {
        frames: Vec<(FrameId, Rect)>,
        start: Point,
        delta: (f64, f64),
    },
    Resize {
        frame: FrameId,
        handle: Handle,
        orig: Rect,
        rotation: f64,
        keep_aspect: bool,
        rect: Rect,
    },
    Rotate {
        frame: FrameId,
        center: Point,
        start_angle: f64,
        orig: f64,
        angle: f64,
    },
    Create {
        start: Point,
        current: Point,
    },
    Marquee {
        start: Point,
        current: Point,
    },
    TextSelect,
}

/// The canvas texture: a viewport-sized region of the page raster at
/// device resolution, filled by the render worker.
pub struct PageTexture {
    /// What it shows (page, resolution, region of the page raster).
    pub key: ViewKey,
    /// The document revision its pixels show.
    pub revision: u64,
    pub texture: egui::TextureHandle,
}

impl PageTexture {
    /// Screen rectangle the texture covers, for a page whose bleed box
    /// starts at `bleed_min` (screen points) shown at `zoom`. When the zoom
    /// changed since it was drawn, the old pixels are shown scaled.
    pub fn screen_rect(&self, bleed_min: Pos2, zoom: f32) -> egui::Rect {
        let k = (72.0 / self.key.dpi) as f32 * zoom;
        let r = self.key.region;
        egui::Rect::from_min_size(
            bleed_min + Vec2::new(r.x as f32 * k, r.y as f32 * k),
            Vec2::new(r.width as f32 * k, r.height as f32 * k),
        )
    }
}

/// A page thumbnail and what it shows.
pub struct Thumb {
    pub revision: u64,
    /// `PageSnapshot::fingerprint` of the page it shows.
    pub fingerprint: u64,
    pub texture: egui::TextureHandle,
}

/// View state of one open document.
pub struct DocView {
    pub doc_id: pubrs_api::core::DocumentId,
    pub page: usize,
    /// Screen points per page point; `None` = fit the page in the window.
    pub zoom: Option<f32>,
    /// Zoom used in the last frame (fit resolves to a number).
    pub effective_zoom: f32,
    pub selection: Vec<FrameId>,
    pub edit: Option<TextEdit>,
    pub drag: Option<Drag>,
    pub page_tex: Option<PageTexture>,
    /// Generation of `page_tex` (see `ViewKey::generation`).
    pub tex_generation: u64,
    /// The last viewport render asked of the worker, and for which revision.
    pub requested: Option<(ViewKey, u64)>,
    pub thumbs: HashMap<usize, Thumb>,
    /// Thumbnails asked of the worker: page → (revision, fingerprint).
    pub thumbs_requested: HashMap<usize, (u64, u64)>,
    /// Renders of this view that failed in a row (retried up to
    /// `MAX_RENDER_RETRIES` times).
    pub render_failures: u32,
    pub xf: Option<PageXf>,
}

impl DocView {
    pub fn new(doc_id: pubrs_api::core::DocumentId) -> Self {
        Self {
            doc_id,
            page: 0,
            zoom: None,
            effective_zoom: 1.0,
            selection: Vec::new(),
            edit: None,
            drag: None,
            page_tex: None,
            tex_generation: 0,
            requested: None,
            thumbs: HashMap::new(),
            thumbs_requested: HashMap::new(),
            render_failures: 0,
            xf: None,
        }
    }

    /// Selection restricted to frames that still exist.
    pub fn prune(&mut self, doc: &Document) {
        self.selection.retain(|f| doc.frame(*f).is_some());
        if self.page >= doc.pages.len() {
            self.page = doc.pages.len().saturating_sub(1);
        }
        if let Some(e) = &self.edit
            && doc.story(e.story).is_none()
        {
            self.edit = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_keeps_opposite_corner_fixed_when_rotated() {
        let r = Rect::new(100.0, 100.0, 100.0, 50.0);
        for rot in [0.0, 30.0, 90.0, 200.0] {
            let fixed = handle_point(r, rot, Handle::NW, 0.0);
            let target = handle_point(Rect::new(100.0, 100.0, 140.0, 80.0), rot, Handle::SE, 0.0);
            // Drag SE so the unrotated box would become 140x80 anchored at NW.
            let p = Point::new(100.0 + 140.0, 100.0 + 80.0).rotated_about(r.center(), rot);
            let n = resize_rect(r, rot, Handle::SE, p, false);
            let still = handle_point(n, rot, Handle::NW, 0.0);
            assert!(
                (still.x - fixed.x).abs() < 1e-6 && (still.y - fixed.y).abs() < 1e-6,
                "rot {rot}"
            );
            assert!(
                (n.width - 140.0).abs() < 1e-6 && (n.height - 80.0).abs() < 1e-6,
                "rot {rot}"
            );
            let _ = target;
        }
    }

    #[test]
    fn aspect_lock_on_corner() {
        let r = Rect::new(0.0, 0.0, 200.0, 100.0);
        let n = resize_rect(r, 0.0, Handle::SE, Point::new(300.0, 120.0), true);
        assert!((n.width / n.height - 2.0).abs() < 1e-9);
    }

    #[test]
    fn rotated_hit_test() {
        let r = Rect::new(0.0, 0.0, 100.0, 10.0);
        assert!(frame_contains(r, 90.0, Point::new(50.0, 40.0), 0.0));
        assert!(!frame_contains(r, 0.0, Point::new(50.0, 40.0), 0.0));
    }
}

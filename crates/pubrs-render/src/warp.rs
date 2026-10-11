//! WordArt warps (ADR 0020).
//!
//! The text is shaped once by `pubrs-layout` on a single baseline (x along
//! the baseline from 0 to the line width, y down, the baseline at 0). Here
//! every glyph outline is moved into that "text space" and each point is
//! sent through the warp, then the whole result is fitted to the frame.
//!
//! Warps, with `W` the line width, `em` the font size, `u = x / W`:
//!
//! - none: identity. Slant up / down: `Y = y -/+ 0.2 x` (a shear; the
//!   outlines stay curves).
//! - wave: `Y = y + 0.28 em sin(2 pi u)`.
//! - arch up / down: the baseline becomes an arc of `150 deg` and radius
//!   `W / angle`, so its length stays `W`; with `theta = (u - 1/2) angle`,
//!   up gives `r = R - y`, `(X, Y) = (r sin theta, -r cos theta)` (letter
//!   tops point away from the centre) and down gives `r = R + y`,
//!   `(X, Y) = (r sin theta, r cos theta)` (tops point at the centre).
//! - circle: arch up round `0.97` of a full turn, starting at the top.
//! - inflate: `Y = c + (y - c) g(u)`, `g = 0.5 + 0.8 sin(pi u)`, where `c`
//!   is the middle of the text's height: letters grow towards the middle.
//!
//! Warped outlines are flattened (a line is cut into pieces no longer than
//! `em / 10`, a curve into 8 or 10) because the maps are not affine.
//! The result is then scaled down, never up, so its box (grown by the
//! outline width) fits the frame, and centred in it: `size` is the
//! nominal size and the frame is the limit. The path is returned relative
//! to its box, so a gradient fill spans the letters.

use std::f64::consts::PI;

use pubrs_core::{Warp, WordArtFrame};
use pubrs_layout::WordArtLayout;
use skrifa::instance::Size;
use skrifa::outline::OutlinePen;
use skrifa::{FontRef, GlyphId, MetadataProvider};

use crate::scene::{PathData, PathSeg};

/// One outline segment in text space.
#[derive(Debug, Clone, Copy)]
enum Seg {
    Move(f64, f64),
    Line(f64, f64),
    Quad(f64, f64, f64, f64),
    Cubic(f64, f64, f64, f64, f64, f64),
    Close,
}

/// Collects a glyph outline into text space.
struct TextPen<'a> {
    out: &'a mut Vec<Seg>,
    /// glyph origin
    ox: f64,
    oy: f64,
    /// font units → points
    k: f64,
    /// horizontal shear per unit of height (synthetic italic)
    skew: f64,
}

impl TextPen<'_> {
    fn pt(&self, x: f32, y: f32) -> (f64, f64) {
        let (fx, fy) = (f64::from(x) * self.k, -f64::from(y) * self.k);
        (self.ox + fx - fy * self.skew, self.oy + fy)
    }
}

impl OutlinePen for TextPen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.pt(x, y);
        self.out.push(Seg::Move(x, y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.pt(x, y);
        self.out.push(Seg::Line(x, y));
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let (a, b) = self.pt(cx0, cy0);
        let (x, y) = self.pt(x, y);
        self.out.push(Seg::Quad(a, b, x, y));
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let (a, b) = self.pt(cx0, cy0);
        let (c, d) = self.pt(cx1, cy1);
        let (x, y) = self.pt(x, y);
        self.out.push(Seg::Cubic(a, b, c, d, x, y));
    }
    fn close(&mut self) {
        self.out.push(Seg::Close);
    }
}

/// Every glyph outline of the layout in text space.
fn text_outlines(layout: &WordArtLayout) -> Vec<Seg> {
    let mut out = Vec::new();
    for run in &layout.runs {
        let Ok(font) = FontRef::from_index(run.font.bytes(), run.font.index) else {
            continue;
        };
        let upem = f64::from(
            font.metrics(Size::unscaled(), skrifa::instance::LocationRef::default())
                .units_per_em,
        )
        .max(1.0);
        let k = run.font_size / upem;
        let skew = run.synthetic_skew.map_or(0.0, |d| d.to_radians().tan());
        let glyphs = font.outline_glyphs();
        for g in &run.glyphs {
            let Some(glyph) = glyphs.get(GlyphId::new(g.id)) else {
                continue;
            };
            let mut pen = TextPen {
                out: &mut out,
                ox: g.x,
                oy: g.y,
                k,
                skew,
            };
            let _ = glyph.draw(Size::unscaled(), &mut pen);
        }
    }
    out
}

/// Warped, fitted outlines (see [`warp_text`]).
pub struct Warped {
    /// Relative to `origin`.
    pub path: PathData,
    /// Top-left corner of the box, frame-local.
    pub origin: (f64, f64),
    /// Width and height of the box.
    pub size: (f64, f64),
}

/// A point map plus whether it is affine (curves stay curves).
struct Map {
    f: Box<dyn Fn(f64, f64) -> (f64, f64)>,
    affine: bool,
}

fn make_map(warp: Warp, w: f64, em: f64, y_mid: f64) -> Map {
    let w = w.max(1.0);
    let arch = 150.0_f64.to_radians();
    let f: Box<dyn Fn(f64, f64) -> (f64, f64)> = match warp {
        Warp::None => Box::new(|x, y| (x, y)),
        Warp::SlantUp => Box::new(|x, y| (x, y - 0.2 * x)),
        Warp::SlantDown => Box::new(|x, y| (x, y + 0.2 * x)),
        Warp::Wave => Box::new(move |x, y| (x, y + 0.28 * em * (2.0 * PI * x / w).sin())),
        Warp::ArchUp => {
            let r0 = w / arch;
            Box::new(move |x, y| {
                let th = (x / w - 0.5) * arch;
                let r = r0 - y;
                (r * th.sin(), -r * th.cos())
            })
        }
        Warp::ArchDown => {
            let r0 = w / arch;
            Box::new(move |x, y| {
                let th = (x / w - 0.5) * arch;
                let r = r0 + y;
                (r * th.sin(), r * th.cos())
            })
        }
        Warp::Circle => {
            let turn = 2.0 * PI * 0.97;
            let r0 = w / turn;
            Box::new(move |x, y| {
                let th = x / w * turn;
                let r = r0 - y;
                (r * th.sin(), -r * th.cos())
            })
        }
        Warp::Inflate => Box::new(move |x, y| {
            let g = 0.5 + 0.8 * (PI * (x / w).clamp(0.0, 1.0)).sin();
            (x, y_mid + (y - y_mid) * g)
        }),
    };
    Map {
        f,
        affine: matches!(warp, Warp::None | Warp::SlantUp | Warp::SlantDown),
    }
}

/// Warp the layout's outlines and fit them into a `w` x `h` frame.
/// `None` when there is nothing to draw.
/// The result is relative to its own box, whose top-left corner is at
/// `origin` in the frame.
pub fn warp_text(layout: &WordArtLayout, art: &WordArtFrame, w: f64, h: f64) -> Option<Warped> {
    let segs = text_outlines(layout);
    if segs.is_empty() {
        return None;
    }
    let (mut ymin, mut ymax) = (f64::MAX, f64::MIN);
    for s in &segs {
        for y in ys(s) {
            ymin = ymin.min(y);
            ymax = ymax.max(y);
        }
    }
    let map = make_map(art.warp, layout.width, art.size, (ymin + ymax) / 2.0);
    let em = art.size.max(1.0);
    let step = em / 10.0;
    let mut pts: Vec<Seg> = Vec::with_capacity(segs.len() * 4);
    let mut cur = (0.0, 0.0);
    let mut start = (0.0, 0.0);
    for s in &segs {
        match *s {
            Seg::Move(x, y) => {
                cur = (x, y);
                start = cur;
                let (a, b) = (map.f)(x, y);
                pts.push(Seg::Move(a, b));
            }
            Seg::Line(x, y) if map.affine => {
                let (a, b) = (map.f)(x, y);
                pts.push(Seg::Line(a, b));
                cur = (x, y);
            }
            Seg::Line(x, y) => {
                line_to(&map, &mut pts, cur, (x, y), step);
                cur = (x, y);
            }
            Seg::Quad(a, b, x, y) if map.affine => {
                let (a, b) = (map.f)(a, b);
                let (x2, y2) = (map.f)(x, y);
                pts.push(Seg::Quad(a, b, x2, y2));
                cur = (x, y);
            }
            Seg::Quad(a, b, x, y) => {
                for i in 1..=8 {
                    let t = f64::from(i) / 8.0;
                    let m = 1.0 - t;
                    let px = m * m * cur.0 + 2.0 * m * t * a + t * t * x;
                    let py = m * m * cur.1 + 2.0 * m * t * b + t * t * y;
                    let (px, py) = (map.f)(px, py);
                    pts.push(Seg::Line(px, py));
                }
                cur = (x, y);
            }
            Seg::Cubic(a, b, c, d, x, y) if map.affine => {
                let (a, b) = (map.f)(a, b);
                let (c, d) = (map.f)(c, d);
                let (x2, y2) = (map.f)(x, y);
                pts.push(Seg::Cubic(a, b, c, d, x2, y2));
                cur = (x, y);
            }
            Seg::Cubic(a, b, c, d, x, y) => {
                for i in 1..=10 {
                    let t = f64::from(i) / 10.0;
                    let m = 1.0 - t;
                    let px = m * m * m * cur.0
                        + 3.0 * m * m * t * a
                        + 3.0 * m * t * t * c
                        + t * t * t * x;
                    let py = m * m * m * cur.1
                        + 3.0 * m * m * t * b
                        + 3.0 * m * t * t * d
                        + t * t * t * y;
                    let (px, py) = (map.f)(px, py);
                    pts.push(Seg::Line(px, py));
                }
                cur = (x, y);
            }
            Seg::Close => {
                // A flattened outline closes with its implicit line back.
                if !map.affine {
                    line_to(&map, &mut pts, cur, start, step);
                }
                pts.push(Seg::Close);
                cur = start;
            }
        }
    }
    // Fit.
    let (mut l, mut t, mut r, mut b) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for s in &pts {
        for (x, y) in xys(s) {
            l = l.min(x);
            r = r.max(x);
            t = t.min(y);
            b = b.max(y);
        }
    }
    if !(l.is_finite() && r.is_finite() && t.is_finite() && b.is_finite()) {
        return None;
    }
    let pad = art.outline.as_ref().map_or(0.0, |o| o.width.max(0.0) * 1.5);
    let (bw, bh) = ((r - l).max(1e-6), (b - t).max(1e-6));
    let scale = ((w - 2.0 * pad) / bw)
        .min((h - 2.0 * pad) / bh)
        .clamp(1e-3, 1.0);
    let (bw, bh) = (bw * scale, bh * scale);
    let origin = ((w - bw) / 2.0, (h - bh) / 2.0);
    let fit = |x: f64, y: f64| (((x - l) * scale) as f32, ((y - t) * scale) as f32);
    let mut out = Vec::with_capacity(pts.len());
    for s in &pts {
        out.push(match *s {
            Seg::Move(x, y) => {
                let (x, y) = fit(x, y);
                PathSeg::Move(x, y)
            }
            Seg::Line(x, y) => {
                let (x, y) = fit(x, y);
                PathSeg::Line(x, y)
            }
            Seg::Quad(a, b, x, y) => {
                let (a, b) = fit(a, b);
                let (x, y) = fit(x, y);
                PathSeg::Quad(a, b, x, y)
            }
            Seg::Cubic(a, b, c, d, x, y) => {
                let (a, b) = fit(a, b);
                let (c, d) = fit(c, d);
                let (x, y) = fit(x, y);
                PathSeg::Cubic(a, b, c, d, x, y)
            }
            Seg::Close => PathSeg::Close,
        });
    }
    Some(Warped {
        path: PathData::new(out),
        origin,
        size: (bw, bh),
    })
}

/// Cut the straight edge `from -> to` into pieces no longer than `step`
/// and push the warped points after `from`.
fn line_to(map: &Map, out: &mut Vec<Seg>, from: (f64, f64), to: (f64, f64), step: f64) {
    let len = (to.0 - from.0).hypot(to.1 - from.1);
    let n = (len / step).ceil().max(1.0) as u32;
    for i in 1..=n {
        let t = f64::from(i) / f64::from(n);
        let (x, y) = (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
        let (x, y) = (map.f)(x, y);
        out.push(Seg::Line(x, y));
    }
}

fn xys(s: &Seg) -> Vec<(f64, f64)> {
    match *s {
        Seg::Move(x, y) | Seg::Line(x, y) => vec![(x, y)],
        Seg::Quad(a, b, x, y) => vec![(a, b), (x, y)],
        Seg::Cubic(a, b, c, d, x, y) => vec![(a, b), (c, d), (x, y)],
        Seg::Close => vec![],
    }
}

fn ys(s: &Seg) -> Vec<f64> {
    xys(s).into_iter().map(|p| p.1).collect()
}

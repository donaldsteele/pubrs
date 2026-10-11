//! Parallel tiled rasterisation of a page region with tiny-skia on
//! `std::thread::scope`, byte-identical to the untiled
//! [`RasterRenderer::render_region`](crate::raster::RasterRenderer::render_region).
//!
//! # Why tiles are exact
//!
//! Every tile is drawn into a scratch pixmap the size of the whole region,
//! with the region's own transform. tiny-skia therefore sees the same path
//! coordinates, the same clip (the pixmap bounds), the same pixel centres for
//! image sampling and the same layer sizes as the untiled render: nothing is
//! translated, so nothing is rounded differently and no edge is chopped at a
//! tile seam. Each pixel's value depends only on the primitives drawn over it
//! and its starting value, so a tile is exact as long as
//!
//! * its pixels start from the background (the tile is cleared first),
//! * every primitive that can touch it is drawn, in paint order (primitives
//!   whose conservative device box misses the tile are skipped), and
//! * only its pixels are read back.
//!
//! Scratch pixmaps are pooled between calls (a damage repaint allocates
//! nothing); a memory budget caps how many exist at once, so a huge region
//! is drawn by fewer threads rather than with unbounded memory. Opacity layers use a second region-sized scratch: the tile is cleared to
//! transparent, the node is drawn, and the layer is composited over the tile
//! only, with the pattern paint tiny-skia's `draw_pixmap` uses. SVG images
//! render their sub-pixmap as in the untiled path and composite only the
//! tile. Axis-aligned rectangles and image clips are cut to the tile (see
//! `cut_rect` in `raster.rs`) so big fills and images are not redrawn whole
//! for every tile. Glyphs outside a tile are skipped one by one.

use std::sync::{Mutex, PoisonError};

use resvg::tiny_skia as ts;

use crate::images::ImageResolver;
use crate::raster::{
    GlyphCache, Painter, PixelRect, RasterOptions, Region, RenderError, affine_box, composite,
    glyph_box,
};
use crate::scene::{Affine, Geom, PageScene, Prim};

/// How a region is split up and how many threads draw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileOptions {
    /// Threads drawing at once (including the calling thread).
    pub threads: usize,
    /// Tile width in pixels; `None` = the full width of the rectangle drawn.
    pub tile_width: Option<u32>,
    /// Tile height in pixels; `None` = about four bands per thread.
    pub tile_height: Option<u32>,
}

impl TileOptions {
    /// One thread per core (at most 8: more add scratch memory but no
    /// measurable speed), automatic tile sizes.
    pub fn auto() -> Self {
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        Self {
            threads: cores.clamp(1, 8),
            tile_width: None,
            tile_height: None,
        }
    }

    /// `threads` threads with fixed tile sizes (tests).
    pub fn fixed(threads: usize, tile_width: u32, tile_height: u32) -> Self {
        Self {
            threads,
            tile_width: Some(tile_width),
            tile_height: Some(tile_height),
        }
    }
}

impl Default for TileOptions {
    fn default() -> Self {
        Self::auto()
    }
}

/// Most bytes of region-sized scratch pixmaps alive at once. A region of
/// `n` bytes is drawn by at most `SCRATCH_BUDGET / n` threads (at least
/// one), or half as many when the scene has translucent nodes (each thread
/// then also needs an opacity layer).
pub const SCRATCH_BUDGET: usize = 160 << 20;

/// Scratches kept between calls: enough for damage repaints, which touch
/// one or two bands.
const KEEP_SCRATCHES: usize = 2;

/// A region-sized pixmap a thread draws its tiles into, plus the layer for
/// translucent nodes (made on first use).
struct Scratch {
    pm: ts::Pixmap,
    layer: Option<ts::Pixmap>,
}

/// Smallest automatic band height: below this, glyph lines straddling band
/// seams are drawn twice too often.
const MIN_AUTO_BAND: u32 = 48;

/// Conservative device boxes of each node and primitive of a scene.
struct Boxes {
    nodes: Vec<NodeBoxes>,
}

struct NodeBoxes {
    t: Affine,
    bbox: Option<PixelRect>,
    prims: Vec<Option<PixelRect>>,
}

fn geom_extent(g: Geom) -> (f64, f64, f64, f64) {
    match g {
        Geom::Rect { w, h, .. } | Geom::Ellipse { w, h } => (0.0, 0.0, w, h),
        Geom::Line { x0, y0, x1, y1 } => (x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)),
    }
}

fn prim_box(prim: &Prim, t: Affine, glyphs: &GlyphCache) -> Option<PixelRect> {
    let pad = |r: PixelRect| Some(r.outset(2));
    match prim {
        Prim::Fill { geom, .. } => {
            let (x0, y0, x1, y1) = geom_extent(*geom);
            pad(affine_box(t, x0, y0, x1, y1))
        }
        Prim::Stroke { geom, width, .. } => {
            // Miter joins (limit 4) reach twice the width past the outline.
            let g = 2.0 * width.max(0.0) + 1.0;
            let (x0, y0, x1, y1) = geom_extent(*geom);
            pad(affine_box(t, x0 - g, y0 - g, x1 + g, y1 + g))
        }
        Prim::Image { clip, .. } => pad(affine_box(t, clip.x, clip.y, clip.right(), clip.bottom())),
        Prim::Rect { rect, .. } => pad(affine_box(t, rect.x, rect.y, rect.right(), rect.bottom())),
        Prim::Glyphs(run) => {
            let key = run.font.key();
            let upem = glyphs.cached_upem(key);
            let k = run.font_size / f64::from(upem);
            let mut glyph_space = Affine::scale(k, k);
            if let Some(skew) = run.synthetic_skew {
                glyph_space = glyph_space.then(Affine::skew_x(skew));
            }
            let grow = if run.synthetic_bold {
                f64::from(upem) * 0.03
            } else {
                0.0
            };
            let mut out: Option<PixelRect> = None;
            for g in &run.glyphs {
                let Some(path) = glyphs.cached_outline(key, g.id) else {
                    continue;
                };
                let gt = glyph_space.then(Affine::translate(g.x, g.y)).then(t);
                if let Some(b) = glyph_box(path, gt, grow) {
                    out = Some(out.map_or(b, |o| o.union(&b)));
                }
            }
            out
        }
    }
}

fn boxes(scene: &PageScene, base: Affine, glyphs: &GlyphCache) -> Boxes {
    let nodes = scene
        .nodes
        .iter()
        .map(|node| {
            let t = node.transform.then(base);
            let prims: Vec<Option<PixelRect>> =
                node.prims.iter().map(|p| prim_box(p, t, glyphs)).collect();
            let bbox = prims
                .iter()
                .flatten()
                .fold(None, |acc: Option<PixelRect>, b| {
                    Some(acc.map_or(*b, |a| a.union(b)))
                });
            NodeBoxes { t, bbox, prims }
        })
        .collect();
    Boxes { nodes }
}

/// One horizontal band of one output rectangle, drawn by one thread.
struct Band<'a, P> {
    /// Region pixels of the band.
    rows: PixelRect,
    /// Its rows of the output (`rows.width` pixels a row).
    out: &'a mut [P],
}

fn fill_pixels(pm: &mut ts::Pixmap, r: PixelRect, px: ts::PremultipliedColorU8) {
    let w = pm.width() as usize;
    let pixels = pm.pixels_mut();
    for y in r.y..r.bottom() {
        let start = y as usize * w + r.x as usize;
        pixels[start..start + r.width as usize].fill(px);
    }
}

/// Renders page regions as parallel tiles. Keeps parsed glyph outlines and
/// a couple of scratch pixmaps between calls.
#[derive(Default)]
pub struct TileRenderer {
    glyphs: GlyphCache,
    options: TileOptions,
    /// Scratches of the last region size.
    pool: Vec<Scratch>,
}

impl TileRenderer {
    pub fn new(options: TileOptions) -> Self {
        Self {
            glyphs: GlyphCache::default(),
            options,
            pool: Vec::new(),
        }
    }

    pub fn options(&self) -> TileOptions {
        self.options
    }

    pub fn set_options(&mut self, options: TileOptions) {
        self.options = options;
    }

    /// Drop pooled scratch pixmaps (memory goes back until the next call).
    pub fn release(&mut self) {
        self.pool.clear();
    }

    /// The whole page with `opts`, tiled: equal to `RasterRenderer::render`.
    pub fn render_page(
        &mut self,
        scene: &PageScene,
        images: &(dyn ImageResolver + Sync),
        opts: &RasterOptions,
    ) -> Result<ts::Pixmap, RenderError> {
        let region = Region::page(scene, *opts);
        let full = PixelRect::new(0, 0, region.rect.width, region.rect.height);
        let mut out = self.render_rects(scene, images, &region, &[full])?;
        out.pop().map(|(_, pm)| pm).ok_or(RenderError::InvalidSize)
    }

    /// Render `rects` (region pixels; each clipped to the region) of a
    /// region. Returns each non-empty clipped rect with its premultiplied
    /// pixels, equal to the same pixels of the untiled region render.
    pub fn render_rects(
        &mut self,
        scene: &PageScene,
        images: &(dyn ImageResolver + Sync),
        region: &Region,
        rects: &[PixelRect],
    ) -> Result<Vec<(PixelRect, ts::Pixmap)>, RenderError> {
        region.check_size()?;
        let bounds = PixelRect::new(0, 0, region.rect.width, region.rect.height);
        let mut outputs: Vec<(PixelRect, ts::Pixmap)> = Vec::new();
        for r in rects.iter().filter_map(|r| r.intersect(&bounds)) {
            let pm = ts::Pixmap::new(r.width, r.height).ok_or(RenderError::InvalidSize)?;
            outputs.push((r, pm));
        }
        let mut targets: Vec<(PixelRect, &mut [[u8; 4]])> = outputs
            .iter_mut()
            .map(|(r, pm)| (*r, pm.data_mut().as_chunks_mut::<4>().0))
            .collect();
        self.render_into(scene, images, region, &mut targets, |px| px)?;
        Ok(outputs)
    }

    /// Like [`TileRenderer::render_rects`], writing each rect's pixels into
    /// a caller's buffer (`width * height` pixels, row by row) through `px`,
    /// which converts a premultiplied RGBA8 pixel; conversion runs on the
    /// drawing threads. Rects must lie inside the region.
    pub fn render_into<P: Send>(
        &mut self,
        scene: &PageScene,
        images: &(dyn ImageResolver + Sync),
        region: &Region,
        targets: &mut [(PixelRect, &mut [P])],
        px: impl Fn([u8; 4]) -> P + Sync,
    ) -> Result<(), RenderError> {
        region.check_size()?;
        let bounds = PixelRect::new(0, 0, region.rect.width, region.rect.height);
        for (r, buf) in targets.iter() {
            if !bounds.contains(r) || buf.len() != r.area() as usize {
                return Err(RenderError::InvalidSize);
            }
        }
        let total_rows: u32 = targets.iter().map(|(r, _)| r.height).sum();
        if total_rows == 0 {
            return Ok(());
        }
        // Imposed sheets clip every node to its cell. They are only made for
        // PDF and PNG output, never for the viewport, so they take the
        // untiled path (identical by construction) instead of a tiled mask.
        if scene.nodes.iter().any(|n| n.clip.is_some()) {
            let pm = crate::RasterRenderer::new().render_region(scene, images, region)?;
            let src = pm.data().as_chunks::<4>().0;
            let stride = region.rect.width as usize;
            for (r, buf) in targets.iter_mut() {
                for row in 0..r.height as usize {
                    let s = (r.y as usize + row) * stride + r.x as usize;
                    let d = row * r.width as usize;
                    let n = r.width as usize;
                    for (o, i) in buf[d..d + n].iter_mut().zip(&src[s..s + n]) {
                        *o = px(*i);
                    }
                }
            }
            return Ok(());
        }
        self.glyphs.prepare(scene);
        let base = region.transform(scene);
        let boxes = boxes(scene, base, &self.glyphs);
        let size = (region.rect.width, region.rect.height);
        let layers = scene.nodes.iter().any(|n| n.opacity < 1.0);
        let bytes = size.0 as usize * size.1 as usize * 4 * if layers { 2 } else { 1 };
        self.pool.retain(|s| (s.pm.width(), s.pm.height()) == size);
        let threads = self
            .options
            .threads
            .clamp(1, (SCRATCH_BUDGET / bytes.max(1)).max(1));
        let band_h = self
            .options
            .tile_height
            .unwrap_or_else(|| total_rows.div_ceil(4 * threads as u32).max(MIN_AUTO_BAND))
            .max(1);
        let mut bands: Vec<Band<'_, P>> = Vec::new();
        for (r, buf) in targets.iter_mut() {
            if r.is_empty() {
                continue;
            }
            let stride = r.width as usize;
            for (i, chunk) in buf.chunks_mut(stride * band_h as usize).enumerate() {
                let y = r.y + (i as u32 * band_h) as i32;
                bands.push(Band {
                    rows: PixelRect::new(r.x, y, r.width, (chunk.len() / stride) as u32),
                    out: chunk,
                });
            }
        }
        let workers = threads.min(bands.len()).max(1);
        let queue = Mutex::new(bands.into_iter());
        let failure: Mutex<Option<RenderError>> = Mutex::new(None);
        let painter = Painter {
            glyphs: &self.glyphs,
            images,
            mips: region.mips,
        };
        let job = Job {
            scene,
            boxes: &boxes,
            painter: &painter,
            size,
            background: region.background_pixel(),
            tile_w: self.options.tile_width.map(|w| w.max(1)),
        };
        let pool = Mutex::new(std::mem::take(&mut self.pool));
        let work = || {
            let mut scratch: Option<Scratch> = None;
            loop {
                let next = queue.lock().unwrap_or_else(PoisonError::into_inner).next();
                let Some(band) = next else { break };
                if scratch.is_none() {
                    scratch = pool.lock().unwrap_or_else(PoisonError::into_inner).pop();
                }
                if let Err(e) = job.band(band, &mut scratch, &px) {
                    *failure.lock().unwrap_or_else(PoisonError::into_inner) = Some(e);
                    break;
                }
            }
            if let Some(s) = scratch {
                pool.lock().unwrap_or_else(PoisonError::into_inner).push(s);
            }
        };
        std::thread::scope(|s| {
            let helpers: Vec<_> = (1..workers).map(|_| s.spawn(work)).collect();
            work();
            // Re-raise a drawing thread's panic with its own message (the
            // scope alone would only say that some thread panicked).
            for h in helpers {
                if let Err(p) = h.join() {
                    std::panic::resume_unwind(p);
                }
            }
        });
        self.pool = pool.into_inner().unwrap_or_else(PoisonError::into_inner);
        self.pool.truncate(KEEP_SCRATCHES);
        // Opacity layers are made again when needed rather than kept idle.
        for s in &mut self.pool {
            s.layer = None;
        }
        match failure.into_inner().unwrap_or_else(PoisonError::into_inner) {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

/// What every thread shares while drawing bands.
struct Job<'a, R: ImageResolver + ?Sized> {
    scene: &'a PageScene,
    boxes: &'a Boxes,
    painter: &'a Painter<'a, R>,
    size: (u32, u32),
    background: ts::PremultipliedColorU8,
    tile_w: Option<u32>,
}

impl<R: ImageResolver + ?Sized> Job<'_, R> {
    fn band<P>(
        &self,
        band: Band<'_, P>,
        scratch: &mut Option<Scratch>,
        px: &impl Fn([u8; 4]) -> P,
    ) -> Result<(), RenderError> {
        if scratch.is_none() {
            *scratch = Some(Scratch {
                pm: ts::Pixmap::new(self.size.0, self.size.1).ok_or(RenderError::InvalidSize)?,
                layer: None,
            });
        }
        let Some(Scratch { pm, layer }) = scratch.as_mut() else {
            return Err(RenderError::InvalidSize);
        };
        let rows = band.rows;
        let tw = self.tile_w.unwrap_or(rows.width).clamp(1, rows.width);
        let src_stride = self.size.0 as usize;
        let mut x = rows.x;
        while x < rows.right() {
            let w = tw.min((rows.right() - x) as u32);
            let tile = PixelRect::new(x, rows.y, w, rows.height);
            self.tile(pm, layer, tile)?;
            let src = pm.data().as_chunks::<4>().0;
            for row in 0..tile.height as usize {
                let s = (tile.y as usize + row) * src_stride + tile.x as usize;
                let d = row * rows.width as usize + (tile.x - rows.x) as usize;
                let n = tile.width as usize;
                for (o, i) in band.out[d..d + n].iter_mut().zip(&src[s..s + n]) {
                    *o = px(*i);
                }
            }
            x += w as i32;
        }
        Ok(())
    }

    fn tile(
        &self,
        pm: &mut ts::Pixmap,
        layer: &mut Option<ts::Pixmap>,
        tile: PixelRect,
    ) -> Result<(), RenderError> {
        fill_pixels(pm, tile, self.background);
        for (node, nb) in self.scene.nodes.iter().zip(&self.boxes.nodes) {
            if !nb.bbox.is_some_and(|b| b.intersect(&tile).is_some()) {
                continue;
            }
            let touching = node
                .prims
                .iter()
                .zip(&nb.prims)
                .filter(|(_, b)| b.is_some_and(|b| b.intersect(&tile).is_some()));
            if node.opacity < 1.0 {
                if layer.is_none() {
                    *layer = Some(
                        ts::Pixmap::new(self.size.0, self.size.1)
                            .ok_or(RenderError::InvalidSize)?,
                    );
                }
                let Some(l) = layer.as_mut() else {
                    return Err(RenderError::InvalidSize);
                };
                fill_pixels(l, tile, ts::PremultipliedColorU8::TRANSPARENT);
                for (p, _) in touching {
                    self.painter.draw_prim(l, p, nb.t, Some(tile));
                }
                let paint = ts::PixmapPaint {
                    opacity: node.opacity as f32,
                    ..Default::default()
                };
                composite(pm, 0, 0, l.as_ref(), &paint, tile, None);
            } else {
                for (p, _) in touching {
                    self.painter.draw_prim(pm, p, nb.t, Some(tile));
                }
            }
        }
        Ok(())
    }
}

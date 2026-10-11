//! Performance harness: a large synthetic publication measured through the
//! same paths the GUI uses (commands, layout, software raster, save, open,
//! PDF export). Run it with `pubrs perf`.
//!
//! The document: Letter pages, ten text frames a page threaded in chains of
//! fifty, filler text from a seeded xorshift generator, images placed from 25
//! generated 800x600 PNG assets, and one purple band a page. It is built
//! through `DocBuilder` commands, so building also measures the command path.
//!
//! Rows carry stable machine keys (`keystroke.total`, ...) so budgets can be
//! attached by key and page count ([`BUDGETS`]). Budgets are informational
//! until `enforced` is set; [`PerfReport::check`] fails only on enforced
//! budgets that were exceeded.

use std::fmt;
use std::time::Instant;

use pubrs_core::*;
use pubrs_render::raster::RasterOptions;

use crate::error::{ApiError, ApiResult};
use crate::{Session, SessionConfig};

/// The page count the informational budgets were recorded at (M9).
pub const BUDGET_PAGES: usize = 16;

/// An upper bound on one measurement at one page count (see [`BUDGETS`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Budget {
    /// Milliseconds the row's median must stay under.
    pub max_ms: f64,
    /// Whether exceeding the bound fails [`PerfReport::check`].
    pub enforced: bool,
}

/// Keystroke total (command + layout + render) at 16 pages. Bounds what a
/// typist feels per key. Recorded 2026-10-10 on a 16-core Linux box, release
/// build, software raster (measured about 70 ms; bound is about 2x). Not yet
/// enforced.
pub const BUDGET_KEYSTROKE_TOTAL_MS: f64 = 150.0;
/// Whole-document relayout after an edit at 16 pages. Recorded 2026-10-10,
/// same machine (measured about 47 ms; bound about 2x). Not yet enforced.
pub const BUDGET_KEYSTROKE_LAYOUT_MS: f64 = 100.0;
/// Re-render of the edited page at 144 dpi, software raster, 16 pages.
/// Recorded 2026-10-10 (measured; bound is about 2x or more). Not yet enforced.
pub const BUDGET_KEYSTROKE_RENDER_MS: f64 = 50.0;
/// One coalesced InsertText command (clone, diff, coalesce) at 16 pages.
/// Recorded 2026-10-10 (measured; bound is about 2x or more). Not yet enforced.
pub const BUDGET_KEYSTROKE_COMMAND_MS: f64 = 5.0;
/// Render of one page at 100% on a HiDPI screen (144 dpi), 16 pages.
/// Recorded 2026-10-10 (measured; bound is about 2x or more). Not yet enforced.
pub const BUDGET_RENDER_100_MS: f64 = 50.0;

const fn info(max_ms: f64) -> Budget {
    Budget {
        max_ms,
        enforced: false,
    }
}

const fn enforced(max_ms: f64) -> Budget {
    Budget {
        max_ms,
        enforced: true,
    }
}

/// Every budget: (page count, row key, bound). A row gets a budget only at
/// the page count listed. Plan Locked Decision 3: a budget is loosened only
/// by an ADR in `docs/adr/` that gives the measurement.
pub const BUDGETS: &[(usize, &str, Budget)] = &[
    // 16 pages (M9 baseline, informational): recorded 2026-10-10 before the
    // story layout cache; see the constants above.
    (16, "keystroke.total", info(BUDGET_KEYSTROKE_TOTAL_MS)),
    (16, "keystroke.layout", info(BUDGET_KEYSTROKE_LAYOUT_MS)),
    (16, "keystroke.render", info(BUDGET_KEYSTROKE_RENDER_MS)),
    (16, "keystroke.command", info(BUDGET_KEYSTROKE_COMMAND_MS)),
    (16, "render.100", info(BUDGET_RENDER_100_MS)),
    // 50 pages, enforced in CI (Linux job, `perf --pages 50 --runs 5 --check`)
    // since M11. Incremental layout recomposes the one edited story. The
    // development machine measures about 7 ms layout and 31 ms total, of
    // which about 23 ms is the re-render (ADR 0012). The M9 CI run showed the
    // runner about 1.15x slower for layout and 1.35x slower for raster.
    (50, "keystroke.layout", enforced(25.0)),
    (50, "keystroke.total", enforced(60.0)),
    // 200 pages, enforced on the development machine (16 cores) since M11.
    // Layout per keystroke stays near the cost of one story however long the
    // publication is: measured 8.5 ms, of which 1.4 ms builds the cache keys.
    (200, "keystroke.layout", enforced(20.0)),
    // M12: commands cost what they change. Frames, stories and pages are
    // shared by `Arc`, one diff per command. Measured on the development
    // machine: 0.12 ms at 50 pages (was 0.54) and 0.25 ms at 200 pages (was
    // 2.13). The CI bound keeps the plan's 2 ms; 0.5 ms is the dev-machine
    // bound at 200 pages (ADR 0001 addendum).
    (50, "keystroke.command", enforced(2.0)),
    (200, "keystroke.command", enforced(0.5)),
];

fn budget_for(key: &str, pages: usize) -> Option<Budget> {
    BUDGETS
        .iter()
        .find(|(p, k, _)| *p == pages && *k == key)
        .map(|(_, _, b)| *b)
}

/// What to build and how often to repeat each measurement.
#[derive(Debug, Clone, Copy)]
pub struct PerfSpec {
    pub pages: usize,
    /// Total characters of filler text.
    pub chars: usize,
    /// Total placed images.
    pub images: usize,
    /// Repetitions per measurement; the median is reported.
    pub runs: usize,
}

impl PerfSpec {
    /// A document of `pages` pages with text and images scaled like the
    /// 200-page evaluation document (5,000 characters and 2.5 images a page).
    pub fn scaled(pages: usize, runs: usize) -> Self {
        Self {
            pages,
            chars: pages * 5_000,
            images: (pages * 5 / 2).max(1),
            runs,
        }
    }
}

/// One measured row.
#[derive(Debug, Clone)]
pub struct PerfRow {
    /// Stable machine key, e.g. `keystroke.total`.
    pub key: &'static str,
    pub label: String,
    pub ms: f64,
    pub budget: Option<Budget>,
}

impl PerfRow {
    /// `None` when no budget applies.
    pub fn exceeded(&self) -> Option<bool> {
        self.budget.map(|b| self.ms > b.max_ms)
    }
}

#[derive(Debug, Clone, Default)]
pub struct PerfReport {
    pub header: String,
    pub rows: Vec<PerfRow>,
    /// Informational lines (overset stories, file sizes, backend).
    pub notes: Vec<String>,
}

impl PerfReport {
    pub fn row(&self, key: &str) -> Option<&PerfRow> {
        self.rows.iter().find(|r| r.key == key)
    }

    /// Rows whose enforced budget was exceeded.
    pub fn failures(&self) -> Vec<&PerfRow> {
        self.rows
            .iter()
            .filter(|r| r.budget.is_some_and(|b| b.enforced) && r.exceeded() == Some(true))
            .collect()
    }

    /// Fails only when an enforced budget was exceeded.
    pub fn check(&self) -> ApiResult<()> {
        let bad = self.failures();
        if bad.is_empty() {
            return Ok(());
        }
        let list: Vec<String> = bad
            .iter()
            .map(|r| {
                format!(
                    "{} {:.1} ms > {:.1} ms",
                    r.key,
                    r.ms,
                    r.budget.map_or(0.0, |b| b.max_ms)
                )
            })
            .collect();
        Err(ApiError::new(
            "budget_exceeded",
            format!("performance budget exceeded: {}", list.join("; ")),
            "profile the slow path or re-measure on a quieter machine",
        ))
    }
}

impl fmt::Display for PerfReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.header)?;
        writeln!(
            f,
            "  {:<58} {:>10}  {:>10}  verdict",
            "measurement", "ms", "budget"
        )?;
        for r in &self.rows {
            let (budget, verdict) = match (r.budget, r.exceeded()) {
                (Some(b), Some(over)) => (
                    if b.max_ms < 10.0 {
                        format!("{:.1}", b.max_ms)
                    } else {
                        format!("{:.0}", b.max_ms)
                    },
                    match (over, b.enforced) {
                        (false, _) => "ok",
                        (true, true) => "FAIL",
                        (true, false) => "over (informational)",
                    },
                ),
                _ => ("-".to_string(), "-"),
            };
            writeln!(
                f,
                "  {:<58} {:>10.2}  {:>10}  {}",
                r.label, r.ms, budget, verdict
            )?;
        }
        for n in &self.notes {
            writeln!(f, "  ({n})")?;
        }
        Ok(())
    }
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % 1_000_000) as f64 / 1_000_000.0
    }
    fn pick<'a>(&mut self, v: &[&'a str]) -> &'a str {
        v[((self.next() * v.len() as f64) as usize).min(v.len() - 1)]
    }
}

const WORDS: &[&str] = &[
    "the",
    "grid",
    "page",
    "margin",
    "column",
    "rhythm",
    "baseline",
    "reader",
    "story",
    "frame",
    "quiet",
    "layout",
    "type",
    "paragraph",
    "composer",
    "weighs",
    "every",
    "line",
    "break",
    "texture",
    "even",
    "hyphen",
    "image",
    "spread",
    "pulse",
    "caption",
    "colour",
    "swatch",
    "ink",
    "tint",
    "gradient",
    "style",
    "voice",
    "printer",
    "proof",
    "bleed",
    "a",
    "of",
    "and",
    "to",
    "in",
    "with",
    "for",
    "is",
    "on",
    "that",
    "by",
    "it",
    "as",
    "careful",
    "measure",
    "leading",
    "kerning",
    "tracking",
    "glyph",
    "serif",
    "italic",
    "heading",
    "running",
    "folio",
];

fn filler(r: &mut Rng, chars: usize) -> String {
    let mut s = String::with_capacity(chars + 16);
    let mut para = 0;
    while s.len() < chars {
        let w = r.pick(WORDS);
        if para == 0 {
            let mut c = w.chars();
            if let Some(f) = c.next() {
                s.extend(f.to_uppercase());
                s.push_str(c.as_str());
            }
        } else {
            s.push_str(w);
        }
        para += w.len() + 1;
        if para > 400 {
            s.push_str(".\n");
            para = 0;
        } else {
            s.push(' ');
        }
    }
    s
}

fn art_png(w: u32, h: u32, seed: f32) -> ApiResult<Vec<u8>> {
    let mut img = image::RgbaImage::new(w, h);
    for (x, y, p) in img.enumerate_pixels_mut() {
        let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
        let v = ((fx * 9.0 + seed).sin() * (fy * 7.0 - seed).cos() * 0.5 + 0.5).clamp(0.0, 1.0);
        *p = image::Rgba([
            (v * 255.0) as u8,
            ((1.0 - fy) * 200.0) as u8,
            ((fx * 0.6 + seed * 0.1).fract() * 255.0) as u8,
            255,
        ]);
    }
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| ApiError::internal(format!("encoding the test image: {e}")))?;
    Ok(out.into_inner())
}

const FRAMES_PER_PAGE: usize = 10;
const FRAMES_PER_STORY: usize = 50;
const ASSETS: usize = 25;

fn build(spec: &PerfSpec) -> ApiResult<(Document, Vec<StoryId>)> {
    let mut b = DocBuilder::new(SeqIds::new(1), PageSetup::default());
    let mut rng = Rng(0x5eed);
    let (aw, ah) = (800u32, 600u32);
    let mut assets: Vec<AssetId> = Vec::with_capacity(ASSETS);
    for k in 0..ASSETS {
        let id = AssetId::new(&mut b.ids);
        let data = art_png(aw, ah, k as f32 * 0.37)?;
        b.exec(Command::AddAsset {
            asset: Asset {
                id,
                name: format!("art-{k}.png"),
                format: ImageFormat::Png,
                pixel_width: aw,
                pixel_height: ah,
            },
            data: Bytes::new(data),
        })?;
        assets.push(id);
    }
    let total_frames = spec.pages * FRAMES_PER_PAGE;
    let stories = total_frames.div_ceil(FRAMES_PER_STORY).max(1);
    let per_story = spec.chars / stories;
    let rows = FRAMES_PER_PAGE.div_ceil(2);
    let (top, strip) = (48.0, 130.0);
    let row_h = (792.0 - 36.0 - top - strip - 8.0) / rows as f64;
    let mut prev: Option<FrameId> = None;
    let mut heads = Vec::new();
    let mut n_frame = 0usize;
    let mut placed = 0usize;
    for abs in 0..spec.pages {
        let page = if abs == 0 { b.page(0) } else { b.add_page()? };
        let band = b.shape(
            page,
            Rect::new(36.0, 20.0, 540.0, 20.0),
            Shape::Rectangle { corner_radius: 0.0 },
        )?;
        b.style(
            band,
            FrameStyle {
                fill: Some(Color::rgb(120, 40, 110)),
                stroke: None,
                opacity: 1.0,
            },
        )?;
        for k in 0..FRAMES_PER_PAGE {
            let (c, r) = (k % 2, k / 2);
            let rect = Rect::new(
                36.0 + c as f64 * 280.0,
                top + r as f64 * row_h,
                260.0,
                row_h - 6.0,
            );
            let first = n_frame.is_multiple_of(FRAMES_PER_STORY);
            let text = if first {
                filler(&mut rng, per_story)
            } else {
                String::new()
            };
            let fid = b.text(page, rect, &text)?;
            if first {
                heads.push(fid);
            } else if let Some(p) = prev {
                b.link(p, fid)?;
            }
            prev = Some(fid);
            n_frame += 1;
        }
        let want = (spec.images * (abs + 1)) / spec.pages - placed;
        for k in 0..want {
            let w = 540.0 / want as f64;
            let rect = Rect::new(36.0 + k as f64 * w, 792.0 - 36.0 - strip, w - 8.0, strip);
            b.image(page, rect, Some(assets[(placed + k) % assets.len()]))?;
        }
        placed += want;
    }
    let doc = b.finish();
    let story_ids = heads.iter().filter_map(|f| doc.story_of(*f)).collect();
    Ok((doc, story_ids))
}

fn ms_since(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v.get(v.len() / 2).copied().unwrap_or(0.0)
}

fn median_ms(runs: usize, mut f: impl FnMut() -> ApiResult<()>) -> ApiResult<f64> {
    let mut v = Vec::with_capacity(runs.max(1));
    for _ in 0..runs.max(1) {
        let t = Instant::now();
        f()?;
        v.push(ms_since(t));
    }
    Ok(median(v))
}

fn opts(dpi: f64) -> RasterOptions {
    RasterOptions {
        dpi,
        include_bleed: true,
        background: Some(Color::WHITE),
    }
}

/// Build the document and take every measurement (software raster, bundled
/// fonts only).
pub fn run(spec: PerfSpec) -> ApiResult<PerfReport> {
    if spec.pages == 0 {
        return Err(ApiError::invalid(
            "pages must be at least 1",
            "use --pages 1 or more",
        ));
    }
    let runs = spec.runs.max(1);
    let mut rep = PerfReport::default();
    let pages = spec.pages;
    let push = |rep: &mut PerfReport, key: &'static str, label: String, ms: f64| {
        rep.rows.push(PerfRow {
            key,
            label,
            ms,
            budget: budget_for(key, pages),
        });
    };

    let t = Instant::now();
    let (doc, stories) = build(&spec)?;
    let build_ms = ms_since(t);
    let chars: usize = doc.stories.values().map(|s| s.char_len()).sum();
    rep.header = format!(
        "pubrs perf: {} pages, {} frames, {} stories, {}k chars, {} images; renderer: tiny-skia (CPU)",
        doc.pages.len(),
        doc.frames.len(),
        doc.stories.len(),
        chars / 1000,
        spec.images
    );
    push(
        &mut rep,
        "build",
        "build document through commands (one undo step each)".into(),
        build_ms,
    );

    // Cold layout of every story, on a fresh layouter (includes font loading).
    let fonts = pubrs_layout::FontLibrary::bundled();
    let mut ctx = pubrs_io::ExportContext::new(fonts);
    let t = Instant::now();
    let layout = ctx.layout(&doc);
    push(
        &mut rep,
        "layout.cold",
        "layout all stories, first run (1 thread)".into(),
        ms_since(t),
    );
    let warm = median_ms(runs.min(3), || {
        drop(ctx.layout(&doc));
        Ok(())
    })?;
    push(
        &mut rep,
        "layout.warm",
        "layout all stories, warm layouter (1 thread)".into(),
        warm,
    );
    let overflow = layout
        .stories
        .values()
        .filter(|s| s.overflow.is_some())
        .count();
    rep.notes.push(format!(
        "stories with overset text: {overflow} of {}",
        layout.stories.len()
    ));

    let mut s = Session::new(SessionConfig {
        software_rendering: true,
        system_fonts: false,
        ..Default::default()
    });
    let doc_id = s.insert(doc.clone(), None);
    let mid = doc.pages.len() / 2;
    s.layout(Some(doc_id))?;
    rep.notes
        .push(format!("render backend: {}", s.render_backend()));
    // Warm image decode and glyph caches.
    s.page_rgba(Some(doc_id), mid, &opts(144.0))?;
    for (key, label, dpi) in [
        ("render.25", "render page at 25% (36 dpi)", 36.0),
        (
            "render.100",
            "render page at 100% on HiDPI (144 dpi, 1224x1584)",
            144.0,
        ),
        (
            "render.400",
            "render page at 400% on HiDPI (capped 4096 px texture)",
            4096.0 * 72.0 / 792.0,
        ),
    ] {
        let ms = median_ms(runs, || {
            s.page_rgba(Some(doc_id), mid, &opts(dpi)).map(drop)
        })?;
        push(&mut rep, key, label.into(), ms);
    }
    // Page switching: every page is a different texture.
    let mut p = 0;
    let ms = median_ms(runs, || {
        p = (p + 7) % doc.pages.len();
        s.page_rgba(Some(doc_id), p, &opts(144.0)).map(drop)
    })?;
    push(
        &mut rep,
        "render.paging",
        "render a different page each time at 144 dpi (paging)".into(),
        ms,
    );

    // Keystroke, as the GUI does it: one coalesced InsertText, then the page
    // texture is re-rendered because the revision changed.
    let story = *stories
        .get(stories.len() / 2)
        .ok_or_else(|| ApiError::internal("the test document has no stories"))?;
    let (mut t_exec, mut t_layout, mut t_raster, mut t_total) = (vec![], vec![], vec![], vec![]);
    let (mut t_keys, mut recomposed) = (vec![], 0);
    for k in 0..runs {
        let t0 = Instant::now();
        let len = s
            .get(Some(doc_id))?
            .doc()
            .story(story)
            .ok_or_else(|| ApiError::not_found("story", "perf", "internal"))?
            .char_len();
        s.exec_coalesced(
            Some(doc_id),
            Command::InsertText {
                story,
                at: len / 2 + k,
                text: "x".into(),
                format: None,
            },
            "typing",
        )?;
        let t1 = Instant::now();
        s.layout(Some(doc_id))?;
        let t2 = Instant::now();
        let stats = s.layout_stats(Some(doc_id))?;
        t_keys.push(stats.key_time.as_secs_f64() * 1000.0);
        recomposed = recomposed.max(stats.recomposed);
        s.page_rgba(Some(doc_id), mid, &opts(144.0))?;
        let t3 = Instant::now();
        t_exec.push((t1 - t0).as_secs_f64() * 1000.0);
        t_layout.push((t2 - t1).as_secs_f64() * 1000.0);
        t_raster.push((t3 - t2).as_secs_f64() * 1000.0);
        t_total.push((t3 - t0).as_secs_f64() * 1000.0);
    }
    push(
        &mut rep,
        "keystroke.command",
        "keystroke: command (clone + diff + coalesce)".into(),
        median(t_exec),
    );
    push(
        &mut rep,
        "keystroke.layout",
        "keystroke: relayout (changed stories only)".into(),
        median(t_layout),
    );
    push(
        &mut rep,
        "keystroke.layout.keys",
        "keystroke: layout cache keys (all stories, part of relayout)".into(),
        median(t_keys),
    );
    rep.notes.push(format!(
        "stories recomposed per keystroke: {recomposed} of {}",
        doc.stories.len()
    ));
    push(
        &mut rep,
        "keystroke.render",
        "keystroke: re-render page at 144 dpi".into(),
        median(t_raster),
    );
    push(
        &mut rep,
        "keystroke.total",
        "keystroke: total, as the GUI does it".into(),
        median(t_total),
    );

    // A non-coalesced edit (moving a frame).
    let some_frame = *doc.pages[mid]
        .frames
        .first()
        .ok_or_else(|| ApiError::internal("the middle page has no frames"))?;
    let ms = median_ms(runs, || {
        let mut r = s
            .get(Some(doc_id))?
            .doc()
            .frame(some_frame)
            .ok_or_else(|| ApiError::not_found("frame", "perf", "internal"))?
            .rect;
        r.x += 1.0;
        s.exec(
            Some(doc_id),
            Command::SetFrameRect {
                frame: some_frame,
                rect: r,
            },
        )
        .map(drop)
    })?;
    push(
        &mut rep,
        "move_frame",
        "move a frame (one undoable command, no relayout)".into(),
        ms,
    );
    let ms = median_ms(runs, || {
        s.get_mut(Some(doc_id))?.editor.undo();
        Ok(())
    })?;
    push(&mut rep, "undo", "undo one step".into(), ms);

    let ms = median_ms(runs.min(5), || {
        s.page_png(Some(doc_id), mid, &RasterOptions::default(), false)
            .map(drop)
    })?;
    push(
        &mut rep,
        "export.png",
        "export page PNG at 150 dpi (layout cached)".into(),
        ms,
    );
    let mut bytes = vec![];
    let ms = median_ms(runs.min(3), || {
        bytes = pubrs_io::write_bytes(&doc, None)?;
        Ok(())
    })?;
    push(
        &mut rep,
        "save",
        format!("save .pubrs ({} pages)", doc.pages.len()),
        ms,
    );
    rep.notes
        .push(format!("file size {} kB", bytes.len() / 1024));
    let ms = median_ms(runs.min(3), || {
        pubrs_io::read_bytes(&bytes)?;
        Ok(())
    })?;
    push(&mut rep, "open", "open .pubrs".into(), ms);
    let mut pdf_len = 0;
    let ms = median_ms(1, || {
        pdf_len = ctx
            .export_pdf(&doc, &pubrs_render::pdf::PdfOptions::default())?
            .len();
        Ok(())
    })?;
    push(
        &mut rep,
        "export.pdf",
        format!(
            "PDF export, all {} pages (includes layout)",
            doc.pages.len()
        ),
        ms,
    );
    rep.notes.push(format!("PDF size {} kB", pdf_len / 1024));
    Ok(rep)
}

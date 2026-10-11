//! Opt-in frame statistics: with `PUBRS_PERF` set, pubrs logs once a second
//! how many frames it drew, how long its own UI code took (and how much of
//! that was commands and layout), what the render worker drew off the UI
//! thread, what was uploaded, any page rasterised on the UI thread (there
//! should be none while editing) and what asked egui for the repaints.
//! [`Totals`] keep the same figures from a chosen moment on, for the
//! self-test's frame-time check.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use eframe::egui;

/// Figures accumulated since [`FrameStats::reset_totals`].
#[derive(Debug, Clone, Copy, Default)]
pub struct Totals {
    pub frames: u32,
    /// UI-thread time of our code per frame: sum and worst.
    pub ui: Duration,
    pub ui_max: Duration,
    /// The worst frame's UI time minus its command and layout time: what
    /// the canvas, panels and texture uploads cost on the UI thread.
    pub overhead_max: Duration,
    /// The second-worst frame's figure: the worst with one outlier (a
    /// scheduler stall on a shared CI runner) set aside.
    pub overhead_second: Duration,
    pub command: Duration,
    pub layout: Duration,
    /// Pages rasterised synchronously on the UI thread.
    pub ui_rasters: u32,
    /// Renders the worker delivered, and its time on them.
    pub worker_renders: u32,
    pub worker_time: Duration,
    /// Pixels uploaded to textures.
    pub uploaded_px: u64,
}

pub struct FrameStats {
    since: Instant,
    window: Totals,
    /// Command and layout time in the current frame.
    frame_command: Duration,
    frame_layout: Duration,
    frame_cpu: Duration,
    causes: BTreeMap<String, u32>,
    totals: Totals,
    /// The session's synchronous raster count at the last frame.
    rasters_seen: Option<u64>,
}

impl FrameStats {
    pub fn from_env() -> Option<Self> {
        std::env::var_os("PUBRS_PERF").map(|_| Self::fresh())
    }

    pub fn fresh() -> Self {
        Self {
            since: Instant::now(),
            window: Totals::default(),
            frame_command: Duration::ZERO,
            frame_layout: Duration::ZERO,
            frame_cpu: Duration::ZERO,
            causes: BTreeMap::new(),
            totals: Totals::default(),
            rasters_seen: None,
        }
    }

    fn both(&mut self, f: impl Fn(&mut Totals)) {
        f(&mut self.window);
        f(&mut self.totals);
    }

    /// A command executed on the UI thread.
    pub fn command(&mut self, spent: Duration) {
        self.frame_command += spent;
        self.both(|t| t.command += spent);
    }

    /// Layout brought up to date on the UI thread.
    pub fn layout(&mut self, spent: Duration) {
        self.frame_layout += spent;
        self.both(|t| t.layout += spent);
    }

    /// The worker delivered a render that took it `spent`.
    pub fn worker(&mut self, spent: Duration) {
        self.both(|t| {
            t.worker_renders += 1;
            t.worker_time += spent;
        });
    }

    /// Pixels handed to a texture upload.
    pub fn upload(&mut self, pixels: u64) {
        self.both(|t| t.uploaded_px += pixels);
    }

    pub fn totals(&self) -> Totals {
        self.totals
    }

    pub fn reset_totals(&mut self) {
        self.totals = Totals::default();
    }

    /// One frame's UI code took `spent`; `frame_cpu` is eframe's figure for the
    /// previous frame including painting; `rasters` is the session's count of
    /// pages rasterised on this thread so far. Log and reset once a second.
    pub fn frame(
        &mut self,
        ctx: &egui::Context,
        spent: Duration,
        frame_cpu: Option<f32>,
        rasters: u64,
    ) {
        let new = rasters.saturating_sub(self.rasters_seen.unwrap_or(rasters));
        self.rasters_seen = Some(rasters);
        self.both(|t| t.ui_rasters += new as u32);
        let overhead = spent.saturating_sub(self.frame_command + self.frame_layout);
        self.frame_command = Duration::ZERO;
        self.frame_layout = Duration::ZERO;
        self.both(|t| {
            t.frames += 1;
            t.ui += spent;
            t.ui_max = t.ui_max.max(spent);
            if overhead > t.overhead_max {
                t.overhead_second = t.overhead_max;
                t.overhead_max = overhead;
            } else {
                t.overhead_second = t.overhead_second.max(overhead);
            }
        });
        self.frame_cpu += Duration::from_secs_f32(frame_cpu.unwrap_or(0.0));
        for c in ctx.repaint_causes() {
            *self.causes.entry(c.to_string()).or_default() += 1;
        }
        let elapsed = self.since.elapsed();
        if elapsed < Duration::from_secs(1) {
            return;
        }
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let w = self.window;
        let n = f64::from(w.frames.max(1));
        tracing::info!(
            "perf: {} frames in {:.1} s, ui {:.2} ms avg / {:.2} ms max (without commands and layout {:.2} ms max; commands {:.1} ms, layout {:.1} ms), ui+paint {:.2} ms avg, ui-thread rasters {}, worker renders {} ({:.1} ms), uploaded {} kpx, repaints {:?}",
            w.frames,
            elapsed.as_secs_f64(),
            ms(w.ui) / n,
            ms(w.ui_max),
            ms(w.overhead_max),
            ms(w.command),
            ms(w.layout),
            ms(self.frame_cpu) / n,
            w.ui_rasters,
            w.worker_renders,
            ms(w.worker_time),
            w.uploaded_px / 1000,
            self.causes,
        );
        self.since = Instant::now();
        self.window = Totals::default();
        self.frame_cpu = Duration::ZERO;
        self.causes.clear();
    }
}

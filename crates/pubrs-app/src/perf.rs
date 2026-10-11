//! Opt-in frame statistics: with `PUBRS_PERF` set, pubrs logs once a second
//! how many frames it drew, how long its own UI code took, how many page
//! images it rasterised and what asked egui for the repaints.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use eframe::egui;

pub struct FrameStats {
    since: Instant,
    frames: u32,
    ui: Duration,
    worst: Duration,
    frame_cpu: Duration,
    rasters: u32,
    raster_time: Duration,
    causes: BTreeMap<String, u32>,
}

impl FrameStats {
    pub fn from_env() -> Option<Self> {
        std::env::var_os("PUBRS_PERF").map(|_| Self::fresh())
    }

    fn fresh() -> Self {
        Self {
            since: Instant::now(),
            frames: 0,
            ui: Duration::ZERO,
            worst: Duration::ZERO,
            frame_cpu: Duration::ZERO,
            rasters: 0,
            raster_time: Duration::ZERO,
            causes: BTreeMap::new(),
        }
    }

    pub fn raster(&mut self, spent: Duration) {
        self.rasters += 1;
        self.raster_time += spent;
    }

    /// One frame's UI code took `spent`; `frame_cpu` is eframe's figure for the
    /// previous frame including painting. Log and reset once a second.
    pub fn frame(&mut self, ctx: &egui::Context, spent: Duration, frame_cpu: Option<f32>) {
        self.frames += 1;
        self.frame_cpu += Duration::from_secs_f32(frame_cpu.unwrap_or(0.0));
        self.ui += spent;
        self.worst = self.worst.max(spent);
        for c in ctx.repaint_causes() {
            *self.causes.entry(c.to_string()).or_default() += 1;
        }
        let window = self.since.elapsed();
        if window < Duration::from_secs(1) {
            return;
        }
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        tracing::info!(
            "perf: {} frames in {:.1} s, ui {:.2} ms avg / {:.2} ms max, ui+paint {:.2} ms avg, {} rasters {:.1} ms, repaints {:?}",
            self.frames,
            window.as_secs_f64(),
            ms(self.ui) / f64::from(self.frames),
            ms(self.worst),
            ms(self.frame_cpu) / f64::from(self.frames),
            self.rasters,
            ms(self.raster_time),
            self.causes,
        );
        *self = Self::fresh();
    }
}

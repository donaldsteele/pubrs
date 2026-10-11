//! The render worker: one thread that rasterises the canvas viewport and
//! page thumbnails from immutable snapshots, so the UI thread never waits
//! for pixels (plan §3 steps 5–7, ADR 0013).
//!
//! * The UI sends a [`ViewRequest`] (a [`PageSnapshot`]: `Arc` document
//!   slice, `Arc` layout, image snapshot; plus what it wants drawn) whenever
//!   the revision or the view changes. Only the newest pending request is
//!   kept: an older one not yet started is dropped.
//! * The worker remembers the snapshot behind the last pixels it sent for
//!   each view key. If the key (page, resolution, region, texture
//!   generation) is unchanged, it computes the page damage between that
//!   snapshot and the new one and renders only the damaged rectangles;
//!   otherwise the whole region. Results arrive in order, so the UI applies
//!   them as they come: full results replace the texture, partial ones go
//!   through `TextureHandle::set_partial`.
//! * Thumbnails are queued behind the viewport and rendered only while no
//!   viewport request waits.
//! * The worker never touches the `Session`.
//! * A failing or panicking render (including a tile thread's panic, which
//!   `std::thread::scope` re-raises) is caught per job and reported as
//!   [`Rendered::Failed`]; the worker forgets what it last drew, so the next
//!   request is drawn in full. If the thread is gone anyway, [`RenderWorker::poll`]
//!   says so and the app starts a new one.

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, ColorImage};
use pubrs_api::core::{Color, DocumentId, Touched};
use pubrs_api::render::{
    PixelRect, RasterOptions, RasterRenderer, Region, TileOptions, TileRenderer,
};
use pubrs_api::session::touched_between;
use pubrs_api::tools::export::{PageSnapshot, damage_pixels};

/// What a viewport texture shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewKey {
    pub doc_id: DocumentId,
    pub page_index: usize,
    /// Texture pixels per inch: 72 × zoom × pixels per point, lowered only
    /// when the visible pixels exceed the largest texture.
    pub dpi: f64,
    /// Size of the whole page raster at `dpi` (trim plus bleed), so a page
    /// size or bleed change is a new key even at the same zoom.
    pub page_px: (u32, u32),
    /// Page-raster pixels the texture covers (its size).
    pub region: PixelRect,
    /// Bumped by the UI when it drops its texture, forcing a full render.
    pub generation: u64,
}

impl ViewKey {
    pub fn options(&self) -> RasterOptions {
        RasterOptions {
            dpi: self.dpi,
            include_bleed: true,
            background: Some(Color::WHITE),
        }
    }
}

/// Draw the page as `key` says, at the snapshot's revision.
pub struct ViewRequest {
    pub key: ViewKey,
    pub snapshot: PageSnapshot,
    /// The session's recorded changes after the revision the UI's texture
    /// shows (a superset of what the worker needs).
    pub changes: Vec<(u64, Touched)>,
}

/// Draw a page thumbnail.
pub struct ThumbRequest {
    pub snapshot: PageSnapshot,
    pub dpi: f64,
    /// What the page looked like when requested (see `panels.rs`).
    pub fingerprint: u64,
}

pub struct ViewResult {
    pub key: ViewKey,
    pub revision: u64,
    /// The patches replace the whole texture (else they are partial updates).
    pub full: bool,
    /// Region-pixel rectangles and their pixels.
    pub patches: Vec<(PixelRect, ColorImage)>,
    /// Time the worker spent on it.
    pub spent: Duration,
}

pub struct ThumbResult {
    pub doc_id: DocumentId,
    pub page_index: usize,
    pub revision: u64,
    pub fingerprint: u64,
    pub image: ColorImage,
    pub spent: Duration,
}

pub enum Rendered {
    View(ViewResult),
    Thumb(ThumbResult),
    /// A render failed or panicked. `page_index` is set for thumbnails.
    Failed {
        doc_id: Option<DocumentId>,
        page_index: Option<usize>,
        message: String,
    },
}

/// What [`RenderWorker::poll`] found.
pub struct Polled {
    pub results: Vec<Rendered>,
    /// The worker thread is gone (it can no longer draw anything).
    pub stopped: bool,
}

#[derive(Default)]
struct Queue {
    view: Option<ViewRequest>,
    thumbs: VecDeque<ThumbRequest>,
    quit: bool,
    #[cfg(test)]
    panic: bool,
}

type Shared = Arc<(Mutex<Queue>, Condvar)>;

pub struct RenderWorker {
    shared: Shared,
    results: Receiver<Rendered>,
    thread: Option<JoinHandle<()>>,
    threads: usize,
}

impl RenderWorker {
    /// Start the worker; it calls `ctx.request_repaint()` after each result.
    pub fn new(ctx: egui::Context) -> Self {
        let shared: Shared = Arc::default();
        let (tx, results) = channel();
        let options = TileOptions::auto();
        let threads = options.threads;
        let thread = {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name("pubrs-render".into())
                .spawn(move || run(&shared, &tx, &ctx, options))
                .map_err(|e| tracing::error!("cannot start the render worker: {e}"))
                .ok()
        };
        Self {
            shared,
            results,
            thread,
            threads,
        }
    }

    fn queue(&self) -> std::sync::MutexGuard<'_, Queue> {
        self.shared.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Ask for a viewport render; replaces any request not yet started.
    pub fn request_view(&self, req: ViewRequest) {
        self.queue().view = Some(req);
        self.shared.1.notify_one();
    }

    /// Ask for a thumbnail; replaces a waiting one for the same page.
    pub fn request_thumb(&self, req: ThumbRequest) {
        let mut q = self.queue();
        let same = |t: &ThumbRequest| {
            t.snapshot.doc_id == req.snapshot.doc_id
                && t.snapshot.page_index == req.snapshot.page_index
        };
        q.thumbs.retain(|t| !same(t));
        q.thumbs.push_back(req);
        drop(q);
        self.shared.1.notify_one();
    }

    /// Results that arrived, oldest first, and whether the thread is gone.
    pub fn poll(&self) -> Polled {
        let mut results = Vec::new();
        loop {
            match self.results.try_recv() {
                Ok(r) => results.push(r),
                Err(TryRecvError::Empty) => {
                    return Polled {
                        results,
                        stopped: false,
                    };
                }
                Err(TryRecvError::Disconnected) => {
                    return Polled {
                        results,
                        stopped: true,
                    };
                }
            }
        }
    }

    /// Make the worker panic inside a tile thread (tests).
    #[cfg(test)]
    pub fn inject_panic(&self) {
        self.queue().panic = true;
        self.shared.1.notify_one();
    }

    pub fn describe(&self) -> String {
        format!("tiny-skia, {} tile threads off the UI thread", self.threads)
    }
}

impl Drop for RenderWorker {
    fn drop(&mut self) {
        self.queue().quit = true;
        self.shared.1.notify_all();
        if let Some(t) = self.thread.take()
            && t.join().is_err()
        {
            tracing::error!("the render worker thread had panicked");
        }
    }
}

enum Job {
    View(ViewRequest),
    Thumb(ThumbRequest),
    #[cfg(test)]
    Panic,
}

/// The text of a caught panic.
fn panic_message(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_string())
}

fn run(shared: &Shared, tx: &Sender<Rendered>, ctx: &egui::Context, options: TileOptions) {
    let mut state = State {
        tiles: TileRenderer::new(options),
        thumbs: RasterRenderer::new(),
        last: None,
    };
    loop {
        let job = {
            let mut q = shared.0.lock().unwrap_or_else(PoisonError::into_inner);
            loop {
                if q.quit {
                    return;
                }
                #[cfg(test)]
                if std::mem::take(&mut q.panic) {
                    break Job::Panic;
                }
                if let Some(v) = q.view.take() {
                    break Job::View(v);
                }
                if let Some(t) = q.thumbs.pop_front() {
                    break Job::Thumb(t);
                }
                q = shared.1.wait(q).unwrap_or_else(PoisonError::into_inner);
            }
        };
        let (doc_id, page_index) = match &job {
            Job::View(v) => (Some(v.key.doc_id), None),
            Job::Thumb(t) => (Some(t.snapshot.doc_id), Some(t.snapshot.page_index)),
            #[cfg(test)]
            Job::Panic => (None, None),
        };
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match job {
            Job::View(v) => state.view(v),
            Job::Thumb(t) => state.thumb(t),
            #[cfg(test)]
            Job::Panic => std::thread::scope(|s| {
                // As a tile thread's panic reaches the worker.
                let tile = s.spawn(|| -> Rendered { panic!("injected render panic") });
                tile.join().unwrap_or_else(|p| std::panic::resume_unwind(p))
            }),
        }));
        let out = caught.unwrap_or_else(|p| {
            // Whatever the worker drew last is unknown now: start over in full.
            state.last = None;
            Rendered::Failed {
                doc_id,
                page_index,
                message: format!("the page renderer panicked: {}", panic_message(&*p)),
            }
        });
        if tx.send(out).is_err() {
            return;
        }
        ctx.request_repaint();
    }
}

struct State {
    tiles: TileRenderer,
    thumbs: RasterRenderer,
    /// The view key and snapshot behind the last pixels sent.
    last: Option<(ViewKey, PageSnapshot)>,
}

fn color32([r, g, b, a]: [u8; 4]) -> Color32 {
    Color32::from_rgba_premultiplied(r, g, b, a)
}

impl State {
    fn view(&mut self, req: ViewRequest) -> Rendered {
        let t0 = Instant::now();
        let scene = match req.snapshot.scene() {
            Ok(s) => s,
            Err(e) => {
                return Rendered::Failed {
                    doc_id: Some(req.snapshot.doc_id),
                    page_index: None,
                    message: e.message,
                };
            }
        };
        let key = req.key;
        let region = Region {
            opts: key.options(),
            rect: key.region,
            mips: true,
        };
        let whole = PixelRect::new(0, 0, key.region.width, key.region.height);
        let damaged = match &self.last {
            Some((k, base)) if *k == key => {
                let touched = touched_between(&req.changes, base.revision, req.snapshot.revision);
                let damage = req.snapshot.damage_since(base, touched.as_ref());
                damage_pixels(&damage, &scene, &region)
            }
            _ => None,
        };
        let full = damaged.is_none();
        let rects = damaged.unwrap_or_else(|| vec![whole]);
        let mut images: Vec<ColorImage> = rects
            .iter()
            .map(|r| {
                ColorImage::filled([r.width as usize, r.height as usize], Color32::TRANSPARENT)
            })
            .collect();
        let mut targets: Vec<(PixelRect, &mut [Color32])> = rects
            .iter()
            .zip(images.iter_mut())
            .map(|(r, img)| (*r, img.pixels.as_mut_slice()))
            .collect();
        if let Err(e) =
            self.tiles
                .render_into(&scene, &req.snapshot.images, &region, &mut targets, color32)
        {
            self.last = None;
            return Rendered::Failed {
                doc_id: Some(key.doc_id),
                page_index: None,
                message: e.to_string(),
            };
        }
        let revision = req.snapshot.revision;
        self.last = Some((key, req.snapshot));
        Rendered::View(ViewResult {
            key,
            revision,
            full,
            patches: rects.into_iter().zip(images).collect(),
            spent: t0.elapsed(),
        })
    }

    fn thumb(&mut self, req: ThumbRequest) -> Rendered {
        let t0 = Instant::now();
        let scene = match req.snapshot.scene() {
            Ok(s) => s,
            Err(e) => {
                return Rendered::Failed {
                    doc_id: Some(req.snapshot.doc_id),
                    page_index: Some(req.snapshot.page_index),
                    message: e.message,
                };
            }
        };
        let opts = RasterOptions {
            dpi: req.dpi,
            ..Default::default()
        };
        let region = Region {
            mips: true,
            ..Region::page(&scene, opts)
        };
        match self
            .thumbs
            .render_region(&scene, &req.snapshot.images, &region)
        {
            Ok(pm) => {
                let size = [pm.width() as usize, pm.height() as usize];
                Rendered::Thumb(ThumbResult {
                    doc_id: req.snapshot.doc_id,
                    page_index: req.snapshot.page_index,
                    revision: req.snapshot.revision,
                    fingerprint: req.fingerprint,
                    image: ColorImage::from_rgba_premultiplied(size, pm.data()),
                    spent: t0.elapsed(),
                })
            }
            Err(e) => Rendered::Failed {
                doc_id: Some(req.snapshot.doc_id),
                page_index: Some(req.snapshot.page_index),
                message: e.to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use pubrs_api::core::{DocBuilder, PageSetup, Rect, SeqIds};
    use pubrs_api::render::PixelRect;
    use pubrs_api::{Session, SessionConfig};

    use super::*;

    fn wait(worker: &RenderWorker) -> (Rendered, bool) {
        let t0 = Instant::now();
        loop {
            let mut polled = worker.poll();
            if let Some(r) = polled.results.pop() {
                return (r, polled.stopped);
            }
            assert!(!polled.stopped, "the worker thread died");
            assert!(t0.elapsed() < Duration::from_secs(60), "no result");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// A panic while rendering (here in a tile thread, re-raised by
    /// `thread::scope`) is reported as `Failed`, the thread lives on, and
    /// the next request is drawn in full.
    #[test]
    fn a_panicking_render_is_reported_and_the_worker_keeps_going() {
        let worker = RenderWorker::new(egui::Context::default());
        worker.inject_panic();
        match wait(&worker) {
            (Rendered::Failed { message, .. }, false) => {
                assert!(message.contains("injected render panic"), "{message}");
            }
            _ => panic!("expected a reported failure"),
        }
        let mut b = DocBuilder::new(SeqIds::new(1), PageSetup::default());
        let p = b.page(0);
        b.text(p, Rect::new(72.0, 72.0, 200.0, 100.0), "Still drawing")
            .expect("text");
        let mut s = Session::new(SessionConfig {
            software_rendering: true,
            ..Default::default()
        });
        let id = s.insert(b.finish(), None);
        let snapshot = s.page_snapshot(Some(id), 0, &[], None).expect("snapshot");
        let key = ViewKey {
            doc_id: id,
            page_index: 0,
            dpi: 72.0,
            page_px: (612, 792),
            region: PixelRect::new(0, 0, 612, 792),
            generation: 0,
        };
        worker.request_view(ViewRequest {
            key,
            snapshot,
            changes: Vec::new(),
        });
        match wait(&worker) {
            (Rendered::View(v), false) => {
                assert!(v.full, "drawn in full after the failure");
                assert_eq!(v.key, key);
                let [w, h] = v.patches[0].1.size;
                assert_eq!((w, h), (612, 792));
            }
            _ => panic!("expected a page render"),
        }
    }
}

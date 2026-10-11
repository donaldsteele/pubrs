# M13: Viewport rendering off the UI thread

What works:

- The canvas no longer rasterises on the UI thread. A render worker (`crates/pubrs-app/src/render_worker.rs`)
  draws from immutable snapshots (`Session::page_snapshot`: the page's slice of the document as an
  `Arc<Document>`, the shared `Arc<DocumentLayout>` and an `ImageSnapshot`) and never touches the `Session`.
  The newest request wins. Until new pixels land, the previous texture is shown, scaled and offset.
- The texture is the visible part of the page plus a 25% margin, at device resolution (one texel per screen
  pixel; the page origin is snapped to device pixels). The 4,096 px cap is gone, so 800% is sharp.
- Damage repaint: after an edit the worker draws only what changed (`pubrs-render/src/damage.rs`, from the
  commands' `Touched` and the old and new layouts) and the UI uploads those rectangles with
  `TextureHandle::set_partial`. Typing one character redraws about 1% of the page.
- Parallel tiles (`pubrs-render/src/tiles.rs`) are byte-identical to the single-threaded render: each tile is
  drawn with the region's own transform and clip into a region-sized scratch, then only its pixels are kept
  (ADR 0013 explains why integer-offset tiles are not exact with tiny-skia and how opacity layers, images and
  SVGs are handled).
- Page thumbnails are drawn by the worker too, and only when the page changed (`page_fingerprint`).
- Images keep half-size mip levels for zoomed-out drawing in the GUI (viewport and page thumbnails); exports, the
  thumbnail stored in saved files and goldens never use them.
- vello is no longer used by the GUI; it remains behind the `gpu` feature for `Session::page_rgba` (ADR 0013).
- `PUBRS_PERF=1` now reports UI-thread time without commands and layout, pages rasterised on the UI thread
  (none while editing), worker renders and uploaded pixels.

See it:

```
cargo test -p pubrs-render --test tiles
cargo run --release -p pubrs-cli -- perf --pages 200 --runs 5 --check
xvfb-run -a -s "-screen 0 1600x1000x24" cargo run -p pubrs-app -- --self-test target/gui-selftest
PUBRS_PERF=1 cargo run --release -p pubrs-app
```

The self-test now zooms to 800% with Cmd+Plus and checks that a shape edge on screen is sharp (100% of its
contrast between two neighbouring pixels; the bound is 90%), then types 24 keys into a 50-page publication and
checks the frame statistics: no page rasterised on the UI thread, at most 50 ms of UI-thread work per frame
besides commands and layout (2.3 to 2.7 ms measured in the debug build).

Median ms, release build, 16-core Linux box, `pubrs perf --runs 5`:

| Measurement | 50 pages | 200 pages | Budget |
|-------------|---------:|----------:|--------|
| viewport 2880x1800 at 100% on HiDPI (8 threads) | 6.26 | 6.41 | 40 (CI, 50 pages), 16 (200 pages) |
| viewport 2880x1800 at 400% on HiDPI | 19.69 | 19.38 | informational |
| keystroke: command | 0.06 | 0.15 | 2 / 0.5 |
| keystroke: layout | 6.94 | 7.55 | 25 / 20 |
| keystroke: damage repaint (tiled) | 0.70 | 0.70 | - |
| keystroke: total | 7.80 | 8.39 | 60 / 30 |

Runs vary with machine load: the viewport row measured 6.4 to 9.4 ms at 200 pages over several runs.

Before M13 the keystroke total was 30.8 ms at 50 pages and 31.4 ms at 200 on this machine (48.9 ms at 50 pages
in CI), most of it the full-page re-render (about 23 ms) on the UI thread.

# M11: Incremental layout

What works:

- `StoryLayoutCache` (`crates/pubrs-layout/src/cache.rs`) keeps each story's layout with a snapshot of everything
  its flow reads (ADR 0012). A pass builds every story's key, compares it by equality and recomposes only stories
  whose key changed. Deleted stories are dropped. `Session::layout` uses one cache per open document, and
  `DocumentLayout.stories` shares the cached layouts (`Arc<StoryLayout>`).
- Typing in one story recomposes that story alone. A frame move recomposes a story only if it changes that story's
  wrap obstacles or the frame's own size. Stylesheet edits recompose every story. Undo recomputes correctly.
- Carets come from one table of cluster offsets per paragraph. They are bit-identical to parley's
  `Cursor::geometry` (unit test), and the per-line rescans of the paragraph are gone. A full layout is about 2×
  faster.
- Export and the GUI no longer copy the document. `page_png`, `page_rgba` and `export_pdf` borrow it next to the
  render caches, and PDF export reuses the cached layout. The canvas reads a snapshot of the current page only.
- `PUBRS_LAYOUT_VERIFY=1` lays out from scratch after every incremental pass and panics with the id of any story
  that differs. `crates/pubrs-layout/tests/cache.rs` has a proptest of random edits with undo and redo, plus
  deterministic tests (an untouched story is not recomposed, moving a wrapping frame invalidates the story it
  wraps, undo recomputes, stylesheet edits, deleted stories).
- Budgets are a table keyed by page count and row (`BUDGETS` in `crates/pubrs-api/src/perf.rs`):
  - 50 pages, enforced in CI: `keystroke.layout` ≤ 25 ms and `keystroke.total` ≤ 60 ms. The Linux CI job runs
    `perf --pages 50 --runs 5 --check`.
  - 200 pages, enforced on the development machine: `keystroke.layout` ≤ 20 ms.
  - 16 pages: unchanged and informational.
- New perf row `keystroke.layout.keys`: time spent building and comparing cache keys, which is part of the
  relayout.

See it:

```
cargo run --release -p pubrs-cli -- perf --pages 50 --runs 5 --check
cargo run --release -p pubrs-cli -- perf --pages 200 --runs 5 --check
PUBRS_LAYOUT_VERIFY=1 cargo test -p pubrs-layout --test cache
```

Before (M9) and after (M11). Release build, software raster, 16-core Linux box, median ms, `--runs 5`
(16 pages before: `--runs 3`).

| Measurement | 16 before | 16 after | 50 before | 50 after | 200 before | 200 after |
|-------------|----------:|---------:|----------:|---------:|-----------:|----------:|
| layout, all stories (warm) | 46.39 | 21.27 | 147.92 | 68.07 | 598.17 | 282.78 |
| keystroke: command | 0.20 | 0.19 | 0.56 | 0.51 | 2.37 | 2.25 |
| keystroke: layout | 47.14 | 6.83 | 146.70 | 7.23 | 594.46 | 8.54 |
| of which cache keys | - | 0.10 | - | 0.32 | - | 1.41 |
| keystroke: render | 23.25 | 22.74 | 23.89 | 23.59 | 24.17 | 23.16 |
| keystroke: total | 70.54 | 29.77 | 171.13 | 31.32 | 621.01 | 33.97 |
| PDF export (includes layout) | 441.53 | 412.14 | 559.58 | 477.31 | 1071.18 | 774.00 |

Each keystroke recomposes 1 story (4, 10 and 40 stories in the three documents). Enforced budgets pass:
- 50 pages: layout 7.23 ms against 25 ms, and total 31.32 ms against 60 ms.
- 200 pages: layout 8.54 ms against 20 ms.

What is left: the render step (about 23 ms of the 50-page total) is M13 (T5, viewport rendering off the UI
thread). The command step at 200 pages (2.3 ms) is M12 (T4).

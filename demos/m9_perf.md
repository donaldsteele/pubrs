# M9: Performance harness in the tree

What works:

- `pubrs perf` builds a synthetic publication (Letter pages, 10 text frames a page threaded in chains of 50,
  seeded filler text, images from 25 generated PNGs, one band a page) through commands and measures build, layout,
  rendering at three zoom levels, paging, the keystroke path (command, relayout, re-render), move, undo, PNG export,
  save, open and PDF export. Software rendering only, bundled fonts only.
- Rows have stable keys (`keystroke.total`, ...); budgets live in `crates/pubrs-api/src/perf.rs` and are all
  informational for now (`enforced: false`). `--check` exits 1 only if an enforced budget is exceeded.
- `--chars` and `--images` scale with `--pages` from the 200-page spec; `--pages 16` is the 16-page evaluation document.
- CI prints the 16-page table on Linux.

See it:

```
cargo run --release -p pubrs-cli -- perf --pages 16 --runs 3
cargo run --release -p pubrs-cli -- perf --pages 200 --runs 5 --check
```

16 pages (release, 16-core Linux box):

```
pubrs perf: 16 pages, 216 frames, 4 stories, 80k chars, 40 images; renderer: tiny-skia (CPU)
  measurement                                                        ms      budget  verdict
  build document through commands (one undo step each)           178.98           -  -
  layout all stories, first run (1 thread)                        47.73           -  -
  layout all stories, warm layouter (1 thread)                    46.39           -  -
  render page at 25% (36 dpi)                                      5.64           -  -
  render page at 100% on HiDPI (144 dpi, 1224x1584)               23.08          50  ok
  render page at 400% on HiDPI (capped 4096 px texture)           97.65           -  -
  render a different page each time at 144 dpi (paging)           24.12           -  -
  keystroke: command (clone + diff + coalesce)                     0.20           5  ok
  keystroke: relayout (whole document)                            47.14         100  ok
  keystroke: re-render page at 144 dpi                            23.25          50  ok
  keystroke: total, as the GUI does it                            70.54         150  ok
  move a frame (one undoable command, no relayout)                 0.08           -  -
  undo one step                                                    0.00           -  -
  export page PNG at 150 dpi (layout cached)                      64.10           -  -
  save .pubrs (16 pages)                                           3.54           -  -
  open .pubrs                                                      1.47           -  -
  PDF export, all 16 pages (includes layout)                     441.53           -  -
  (stories with overset text: 4 of 4)
  (render backend: tiny-skia (software))
  (file size 5472 kB)
  (PDF size 10961 kB)

```

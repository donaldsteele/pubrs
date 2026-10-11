# 0012 Story layout cache: per-story snapshot keys, compared by equality

- Context: `Session::layout` laid out every story again after any change. With the `pubrs perf` document (25,000-character stories, each threaded through 50 frames on 5 pages), one keystroke cost 594 ms of layout at 200 pages and 147 ms at 50 (M9). Plan Locked Decision 3 requires that no edit does work proportional to the whole document. Profiling showed a second problem inside each story: caret positions took 76% of layout time, because parley's `Cursor::geometry` walks the line from its start for every caret.
- Decision:
  - `pubrs_layout::cache::StoryLayoutCache` (one per open document, in `OpenDoc`; the `Layouter` with its fonts and shaping caches stays shared in the session) keeps an `Arc<StoryLayout>` per story with the key it was made from. Each pass builds every story's key and recomposes only stories whose key differs. Entries of deleted stories are dropped. `DocumentLayout.stories` holds `Arc<StoryLayout>`, so assembling a layout costs one pointer per story.
  - **The key is a snapshot of the inputs, compared by equality, not a hash.** Equality has no collision risk. Comparing an unchanged 25,000-character story is a memory compare, cheaper than hashing it. After M12 (T4) stories become `Arc<Story>`, so the compare can first check the pointer. `story_key` in `cache.rs` is the one function that builds it, and its doc comment lists every input:
    - the story itself: paragraphs, runs, text, style references, local overrides and the chain of frame ids;
    - a stylesheet generation, bumped when `doc.styles` differs from the last one seen, so any stylesheet edit recomposes every story;
    - a font fingerprint: the families and faces registered with the shaper;
    - whether each chain frame is a text frame;
    - the flow regions from `regions_for`, the same function the layout flows into. Each region holds a frame-local content box (from the frame's size, insets, columns and gap), its vertical alignment, and the wrap obstacles over it (`obstacles_for`, mapped through the text frame's position and rotation).
  - Layout output is frame-local, so moving or rotating a chain frame recomposes its story only when the change alters the wrap obstacles.
  - `DocIndex` answers `owner_of` and `paint_order` from one O(frames) pass, so building every key stays cheap. Carets come from a per-paragraph table of cluster offsets, built with the same f32 additions in the same order as parley's `Cluster::visual_offset`. A unit test checks every caret, bit for bit, against `Cursor::geometry`, including wrapped, justified, mixed-style, combining-mark and right-to-left text. Line character offsets are counted forward once per paragraph instead of being rescanned from the paragraph start for every line.
  - **Verify mode.** With `PUBRS_LAYOUT_VERIFY=1` (read once per process), every pass lays the document out again from scratch and panics with the id of any story that differs. `crates/pubrs-layout/tests/cache.rs` adds two kinds of test. A proptest runs 64 random sequences of up to 31 edits through `Editor` (typing, deleting, frame moves, resizes and rotation, character and paragraph formatting, style assignment, stylesheet edits, link and unlink, wrap mode and margin, text-frame settings, z-order, page margins, adding and deleting frames, undo and redo) and checks incremental against full layout after every step. Deterministic tests show that an untouched story is not recomposed and stays the same allocation, that moving a wrapping frame recomposes the story it wraps, that undo recomputes, and that deleted stories are dropped.
- Consequence:
  - Measured on the development machine (16 cores, release build, `pubrs perf --runs 5`, median):

    | Pages | Keystroke layout | Of which cache keys | Keystroke total |
    | ---: | ---: | ---: | ---: |
    | 16 | 47.1 → 6.8 ms | 0.10 ms | 70.5 → 29.8 ms |
    | 50 | 146.7 → 7.2 ms | 0.32 ms | 171.1 → 31.3 ms |
    | 200 | 594.5 → 8.5 ms | 1.41 ms | 621.0 → 34.0 ms |

    A full layout from scratch also fell from 598 to 283 ms at 200 pages, because of the caret change.
  - `pubrs perf --check` enforces these budgets: at 50 pages in CI, `keystroke.layout` ≤ 25 ms and `keystroke.total` ≤ 60 ms; at 200 pages on the development machine, `keystroke.layout` ≤ 20 ms. The M9 CI run measured the runner about 1.15× slower than this machine for layout and 1.35× slower for raster, so about 43 ms is expected there against the 60 ms total.
  - A keystroke still recomposes the whole edited story (about 6 ms for 25,000 characters). Paragraph-level reuse is possible later if longer stories need it.
  - The cache holds a copy of every story's text. That is about 1 MB at 200 pages, and it shrinks to pointers once T4 makes stories `Arc`.

# 0001 Patch-based undo

- Context: every command must be invertible; hand-writing an inverse for ~50 commands is error-prone.
- Decision: the Editor snapshots, applies, and diffs before/after into forward/backward `Patch`es (themselves a serializable `patch` command); undo/redo apply patches, never re-run command logic.
- Consequence: inverses are exact and atomic by construction; cost is one document clone + diff per command (sub-millisecond at publication sizes).

## Addendum (M12): Arc sharing, one diff, measurements

- Context: the original cost, one deep document clone plus a diff per command, grew with the publication: a coalesced keystroke made two full clones and three whole-document diffs (2.13 ms at 200 pages, 0.54 ms at 50).
- Decision, model: `Document.frames` and `Document.stories` hold `Arc<Frame>` and `Arc<Story>`, and `pages` and `masters` are `Arc<Vec<_>>`. Mutation goes through `Arc::make_mut` inside `Document::frame_mut`, `story_mut`, `page_mut`, `pages_mut`, `masters_mut` (copy on write). Serde's `rc` feature keeps the JSON byte-identical, which `crates/pubrs-core/tests/fixtures/patch_corpus.json` pins (patch JSON for every command, coalesced typing, undo and redo, blessed on the pre-refactor code).
- Decision, patches: patch maps hold `Option<Arc<_>>`, so patches and history share objects with the documents instead of copying them.
- Decision, diff: `Patch::diff_pair` walks both documents once, skips entries that are `Arc::ptr_eq`, and returns the forward and backward patches together. A coalesced keystroke diffs the step once and folds it into the history entry with `Patch::compose_pair`, which prunes entries that returned to their starting value, so the result equals a diff from the entry's start state. The `before` snapshot is a shallow clone, `check()` clones shallowly, and `changed_pages` is skipped when the page list is the same `Arc`.
- Consequence: a command costs what it changes plus one pass of pointer comparisons over the frame and story maps. A command that makes a copy-on-write copy of a frame and leaves it equal is still correct: equality is the fallback.
- Measured (release, 16-core dev machine, `perf --runs 5`): `keystroke.command` 0.54 to 0.12 ms at 50 pages and 2.13 to 0.23 ms at 200 pages. Enforced: 2 ms at 50 pages (CI), 0.5 ms at 200 pages (dev machine). The layout cache compares stories by `Arc::ptr_eq` first (ADR 0012).

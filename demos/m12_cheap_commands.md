# M12: Commands cost what they change

What works:

- `Document.frames` and `Document.stories` hold `Arc<Frame>` and `Arc<Story>`; `pages` and `masters` are
  `Arc<Vec<_>>`. Edits copy on write (`Arc::make_mut` inside `frame_mut`, `story_mut`, `page_mut`, `pages_mut`,
  `masters_mut`), so a command copies only the objects it changes. Patches hold `Arc`s too, so history shares
  objects with the document.
- The JSON is unchanged (serde `rc`). `patch_json_matches_the_golden_corpus` in
  `crates/pubrs-core/tests/commands.rs` compares the forward and backward patch JSON of every command, coalesced
  typing, undo and redo with `crates/pubrs-core/tests/fixtures/patch_corpus.json`. The fixture was written
  (`PUBRS_BLESS=1`) on the code before the refactor and has not changed since. `MCP.md` and the golden images
  are untouched.
- One diff per command. `Patch::diff_pair` makes the forward and backward patches in a single walk that skips
  `Arc::ptr_eq` entries. Coalesced typing folds each step into the history entry with `Patch::compose_pair`
  (no second snapshot, no re-diff). `check()` clones shallowly. Undo and redo skip the page comparison when the
  page list is the same `Arc`.
- The layout cache compares a story by pointer first and falls back to equality, so an untouched story costs no
  text comparison (ADR 0012). The remaining cost of building keys at 200 pages (about 1.2 ms) is the region and
  obstacle computation, not story comparison.
- A property test in `crates/pubrs-core/tests/props.rs` checks that after any command, every frame and story the
  change did not touch is the same allocation as before, next to the undo-all and redo-all checks.
- Budgets (`BUDGETS` in `crates/pubrs-api/src/perf.rs`): `keystroke.command` at most 2 ms at 50 pages (CI) and
  0.5 ms at 200 pages (development machine).

See it:

```
cargo run --release -p pubrs-cli -- perf --pages 50 --runs 5 --check
cargo run --release -p pubrs-cli -- perf --pages 200 --runs 5 --check
cargo test -p pubrs-core
```

Median ms, release build, 16-core Linux box, `--runs 5`:

| Measurement | 50 pages before | 50 pages after | 200 pages before | 200 pages after |
|-------------|----------------:|---------------:|-----------------:|----------------:|
| keystroke: command (apply + diff + coalesce) | 0.54 | 0.12 | 2.13 | 0.23 |
| keystroke: layout cache keys | 0.30 | 0.30 | 1.36 | 1.24 |
| keystroke: total | 31.3 | 30.8 | 34.0 | 31.5 |

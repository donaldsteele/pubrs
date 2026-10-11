# M10: Robustness and guard rails

What works:

- A panic inside a command no longer leaves a half-edited document: `Editor` catches it, restores the snapshot and
  returns `CommandError::Internal { message }` (API code `internal`). No history entry, no revision bump.
- Property tests (`proptest`, 64 cases each, no persistence files):
  - `crates/pubrs-core/tests/props.rs`: random mixes of typing, deletes, moves, resizes, rotation, style edits,
    link/unlink, wrap, add/delete page and frame, many of them failing. Failed commands leave the document untouched;
    undo-all equals the start; redo-all equals the end.
  - `crates/pubrs-io/tests/fuzz.rs`: save/load round-trip equality (and deterministic bytes), plus `read_bytes`,
    `read_thumbnail` and `migrate` never panic on random bytes, truncated files, bit flips, mutated `document.json`
    and arbitrary JSON.
- The round-trip property found a real bug: JSON floats were off by one ULP after a save/load. Fixed by enabling
  `serde_json`'s `float_roundtrip` feature.
- Text wrap now works around a rotated text frame: obstacle outlines are mapped through the inverse of the text
  frame's rotation before wrapping. Unrotated frames take the old path, so goldens are byte-identical.
- Workspace lints: `unsafe_code = deny` (allowed only in `pubrs-app` `platform` and the GL renderer probe in `app.rs`),
  clippy `unwrap_used`, `dbg_macro`, `todo`, `undocumented_unsafe_blocks`. Tests may unwrap (`clippy.toml`).
- `crates/pubrs-api/tests/layering.rs` reads `cargo metadata` and fails on any normal dependency that is not strictly
  downward: core < layout < render < io < api < {mcp, cli, app}.

See it:

```
cargo test -p pubrs-core --test props
cargo test -p pubrs-io --test fuzz
cargo test -p pubrs-api --test layering
cargo clippy --workspace --all-targets -- -D warnings
```

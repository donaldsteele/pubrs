# M0: workspace skeleton

What works:

- Cargo workspace with eight crates in the fixed dependency order.
- `pubrs-core` command system: ~50 serializable commands with stable names,
  atomic application, undo/redo via patches, typing coalescing, revision counter.
- GUI shell opens a window (wgpu, falling back to OpenGL) and exits on `--smoke-test`.
- CI matrix: Linux, Windows, macOS arm64, macOS Intel; fmt, clippy, tests, GUI smoke launch.

See it:

```
cargo test -p pubrs-core
cargo run -p pubrs-app -- --smoke-test     # Linux headless: xvfb-run -a cargo run -p pubrs-app -- --smoke-test
cargo run -p pubrs-app
```

# M5: pubrs-api Session and CLI

What works:

- `Session` with ~65 tools covering documents, pages, masters, guides, frames (add/move/resize/rotate/z-order/
  group/lock/align/distribute/style/wrap/placeholders/duplicate), text (set with markup, insert, replace, character
  and paragraph formatting, link/unlink, get, overflow, frame options), styles, images (place/crop/fit/replace),
  export (PDF/PNG/preview), undo/redo/history, doc_to_json, validate, fonts_list.
- Tool registry with JSON schemas generated from the Rust types; structured `{code, message, hint}` errors.
- Script replay (`examples/scripts/flyer.json`) with `${var}` references; deterministic with a fixed id seed.
- `pubrs` CLI: `render`, `export`, `validate`, `script`, `tools`.

See it:

```
cargo run -p pubrs-cli -- script examples/scripts/flyer.json --out out/
cargo run -p pubrs-cli -- validate out/flyer.pubrs
cargo run -p pubrs-cli -- render out/flyer.pubrs --dpi 100 -o out/
cargo run -p pubrs-cli -- export out/flyer.pubrs --crop-marks -o out/flyer-print.pdf
cargo run -p pubrs-cli -- tools
```

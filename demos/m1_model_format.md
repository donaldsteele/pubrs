# M1: document model and file format

What works:

- Full document model: pages, master pages, guides, grid/snap settings,
  text/image/shape/group frames, stories for linked text, paragraph and
  character styles with inheritance, embedded image assets.
- Snapping (page edges, margins, guides, other frames, grid), align and
  distribute geometry, structural integrity checks and model-level
  validation warnings (off page, beyond bleed, empty placeholders, low dpi).
- `.pubrs` zip format: `mimetype`, `document.json`, `assets/<id>.<ext>`,
  optional `thumbnail.png`. Deterministic output, atomic saves, schema
  migration (unversioned → v1), newer-version rejection.
- Round-trip test: a document touching every model feature saves, loads and
  compares equal.

See it:

```
cargo test -p pubrs-core -p pubrs-io
cargo run -p pubrs-io --example write_sample -- sample.pubrs
unzip -l sample.pubrs && unzip -p sample.pubrs document.json | head -40
```

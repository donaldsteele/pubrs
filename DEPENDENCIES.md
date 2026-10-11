# DEPENDENCIES

The stack named in PLAN.md is used as-is unless a blocker is recorded here.
Any dependency outside that stack has a one-line justification below.

## Outside the prescribed stack

- `tracing-subscriber`: log output for the three binaries (companion of the prescribed `tracing`).
- `clap`: argument parsing for the `pubrs` CLI and `pubrs-mcp` flags.
- `base64`: encoding PNG previews and image uploads in the API/MCP layer.
- `lopdf` (dev): parses exported PDFs back in tests (page count, boxes, embedded fonts).
- `pollster` (optional, `gpu` feature): blocks on wgpu adapter/device futures for the vello path.
- `objc2-foundation`, `objc2-pdf-kit` (macOS only): NSURL and PDFDocument for printing the exported PDF through NSPrintOperation.

## Version pins and blockers

- `resvg`/`usvg` pinned to 0.47 (not 0.48): `krilla-svg` 0.8.1 accepts only usvg 0.47 `Tree`s; one usvg version keeps SVG raster and PDF paths on the same parsed tree.
- `fontdb` pinned to 0.23 to match usvg 0.47 (one font database type for SVG text and font discovery).
- `skrifa` pinned to 0.44 to match parley 0.12's internal version (no duplicate outline code).
- `parley` pinned to `=0.12.0`: M14 (hyphenation) relies on its `#[doc(hidden)]` `BreakLines::set_prior_line_width` to give a hyphenated or capped line its own alignment width (ADR 0014). Re-check the layout goldens and `crates/pubrs-layout/tests/hyphenation.rs` when upgrading.
- `hayro` (dev, pubrs-io): rasterises exported PDFs in tests so PDF output is compared against the PNG renderer.
- `axum` (pubrs-mcp): hosts rmcp's streamable-HTTP tower service for `pubrs-mcp --http`.
- `tokio-util` (pubrs-mcp): `CancellationToken` for graceful HTTP shutdown (rmcp's own type).
- `muda` (macOS only): native NSMenu menu bar from the shared keymap table (egui has no native menus).
- `windows` features `Data_Pdf`, `Graphics_Imaging`, `Storage*`: Windows printing rasterises the exported PDF with the OS PDF renderer (WinRT) and prints through GDI after the system print dialog.
- `proptest` (dev, pubrs-core and pubrs-io): property tests for command/undo sequences, save/load round-trips and loader fuzzing.
- `cfb` (pubrs-io, MIT): reads the OLE compound file container of Microsoft Publisher `.pub` files (T23).
- `hypher` 0.1.8 (pubrs-layout, MIT OR Apache-2.0): TeX-pattern hyphenation with patterns compiled in at build time; `default-features = false` with only the languages whose pattern licences are recorded in `ASSETS.md` (en, de, es, fr, pl, pt). Used by Typst.
- `sha2` (dev, pubrs-io, MIT OR Apache-2.0): verifies the SHA-256 hashes in `ASSETS.md` in the assets listing test; already in the tree through lopdf.
- `hayro-syntax` 0.8 (pubrs-render, MIT OR Apache-2.0; `default-features = false`): parses an exported PDF's objects and content streams so `pdfx::check` can find device RGB under a CMYK output intent (PDF/X-4). Already in the tree through the dev-only `hayro`.

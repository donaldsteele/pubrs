# Architecture

pubrs is a desktop publishing app (flyers, newsletters, brochures, cards) with
two equal clients over one headless core: a GUI and an MCP server. A headless
CLI is a third client.

## Crates

Dependencies point strictly downward in this list.

| Crate | Role | May depend on |
|-------|------|---------------|
| `pubrs-core` | Document model, ids, geometry, styles, commands, undo/redo, validation. No I/O, no UI, no rendering. Builds with `--no-default-features`. | serde, uuid, thiserror, schemars (optional) |
| `pubrs-layout` | Text shaping and flow: linked frame chains, columns, overflow, wrap around objects, styles → positioned glyph runs. | core, parley, fontique, fontdb, skrifa |
| `pubrs-render` | Builds a display list from a laid-out document and renders it to raster (tiny-skia; vello on GPU) or PDF (krilla). Never mutates the document. | core, layout, resvg/usvg, krilla, vello (feature `gpu`) |
| `pubrs-io` | `.pubrs` file format (zip: `document.json` + `assets/`), schema migration, image decoding, SVG, templates, export orchestration. | core, layout, render, zip, image |
| `pubrs-api` | `Session`: open documents + command dispatch + queries. **The single entry point** for every client. Tool registry (names, JSON schemas, handlers). | core, layout, render, io, schemars |
| `pubrs-mcp` | MCP server (rmcp) over stdio or streamable HTTP. Maps tools 1:1 onto the `pubrs-api` registry; only schema and error mapping. | api, rmcp, tokio |
| `pubrs-cli` | `pubrs render / export / validate / script`. | api, clap |
| `pubrs-app` | egui/eframe GUI. Platform services behind traits in `src/platform/`. | api, eframe, rfd, dirs, arboard, windows / objc2 |

Rule: if a feature is not in `pubrs-api`, it does not exist for any client.

Platform-specific code (`#[cfg(windows)]`, `#[cfg(target_os = ...)]`) is only
allowed in `pubrs-app` and `pubrs-cli`.

## Commands and undo

Every mutation is a `pubrs_core::Command`: serde-serializable data with a
stable snake_case name (the `op` tag, e.g. `insert_text`, `set_frame_rect`).
Commands carry the ids of everything they create, so applying one is
deterministic and a command log replays exactly.

`Editor::execute` snapshots the document, applies the command, and on error
restores the snapshot (commands are atomic). On success it diffs before/after
into a forward and a backward `Patch` (changed pages, frames, stories, styles,
assets). Undo applies the backward patch, redo the forward one. A `Patch` is
itself a command (`op: "patch"`), so the inverse of every command is a
serializable command. `Batch` groups several commands into one undo step.
See `docs/adr/0001-patch-based-undo.md`.

Each successful change bumps a per-document `revision` counter. Every API/MCP
call returns the revision and the ids it touched, so agents can detect stale
state.

## Document model

- Units are points (1/72 in). Origin is the top-left of the page trim box, y down.
- `Document` holds the page setup (size, margins, bleed), grid/snap settings,
  pages, master pages, a flat id-keyed map of frames, stories, a style sheet
  and asset metadata. Asset bytes live beside the model (`blobs`) and in the
  file's `assets/` folder.
- Pages, masters and groups hold ordered frame-id lists (back to front).
- Frames: text, image, shape (rectangle, ellipse, line) or group. Each has a
  rect, rotation (degrees clockwise about its centre), lock flag, fill/stroke/
  opacity, text-wrap setting and optional template placeholder metadata.
- Text lives in `Story` objects. A story lists the chain of text frames it
  flows through; linked frames share one story. Paragraphs carry a paragraph
  style plus overrides; runs carry a character style plus overrides.
- Text positions in commands and the API are character offsets (Unicode
  scalar values) into the story's plain text, paragraphs joined by `\n`.

## Rendering paths

- Raster: tiny-skia, deterministic, used for PNG export, previews, golden
  tests and the GUI when no GPU adapter exists. vello (wgpu) is used by the
  GUI when an adapter is available; the active path is logged.
- PDF: krilla, with subset/embedded fonts, bleed, crop marks and
  MediaBox/BleedBox/TrimBox/CropBox set. Printing on every OS prints this PDF.

## Fonts

pubrs bundles OFL fonts (Liberation Sans and Serif, metric-compatible with
Arial and Times New Roman). System fonts are discovered with fontdb and
registered lazily. A document that names a missing font renders with the
bundled fallback and `validate` warns about it. Golden tests use only the
fonts in `tests/fonts/`.

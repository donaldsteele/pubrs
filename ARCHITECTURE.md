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

## Layout pipeline

`pubrs-layout` turns stories into positioned glyph runs, frame-local
(relative to each frame's unrotated rect; renderers apply position and
rotation).

1. **Regions.** `regions_for` splits each text frame of a story's chain into
   column regions (frame size minus insets, columns and gap). Each region
   carries its vertical alignment and the wrap obstacles over it: frames
   painted above it on the same page or master with wrap on and another story,
   mapped through the text frame's position and rotation and outset by the wrap
   margin (`obstacles_for`). A `DocIndex` built once per pass answers
   `owner_of` and `paint_order`, so this costs O(frames).
2. **Flow.** Each paragraph is one parley layout (ADR 0007), shaped with its
   resolved paragraph and character styles and broken line by line into the
   current region around the obstacles. Text that does not fit is reported as
   overflow. Each line records its story character range and a caret x for
   every character boundary, computed from one table of cluster offsets per
   paragraph.
3. **Cache.** `StoryLayoutCache` (one per open document; ADR 0012) keeps an
   `Arc<StoryLayout>` per story with the inputs it was made from: the story,
   a stylesheet generation, a font fingerprint, the chain's text frames and
   the regions above. Each pass builds every story's key (`story_key`) and
   compares it by equality. Only stories whose key changed are recomposed, so
   a keystroke costs about one story whatever the page count. The resulting
   `DocumentLayout` shares the cached stories.
   `PUBRS_LAYOUT_VERIFY=1` re-lays out from scratch after every pass and
   panics on any difference.
4. **Use.** `Session::layout` returns the cached layout of the current
   revision. Rendering, PDF export, the canvas overlays (`textedit.rs`) and
   the text tools read it. `Layouter::layout_document` (from scratch) remains
   for one-shot exports and tests.

## Rendering paths

- Raster: tiny-skia, deterministic. `RasterRenderer` draws a page (or a
  `Region` of the page raster) on one thread: PNG export, previews,
  thumbnails stored in files and golden tests. `TileRenderer`
  (`pubrs-render/src/tiles.rs`) draws a region as parallel tiles on
  `std::thread::scope` and is byte-identical to the untiled render (plan
  Locked Decision 4, `crates/pubrs-render/tests/tiles.rs`). Every tile is
  drawn into a scratch the size of the whole region with the region's own
  transform, so tiny-skia sees the same coordinates, clip and pixel centres
  as the untiled render; primitives that miss a tile are skipped, axis-aligned
  rectangles and image clips are cut to the tile, and opacity layers and SVGs
  are composited over the tile only. A memory budget caps how many
  region-sized scratches exist at once (ADR 0013).
- vello (wgpu, feature `gpu`): only `Session::page_rgba`, one-shot whole
  pages for API clients. The GUI does not use it (ADR 0013).
- PDF: krilla, with subset/embedded fonts, bleed, crop marks and
  MediaBox/BleedBox/TrimBox/CropBox set. Printing on every OS prints this PDF.

## Fonts

pubrs bundles OFL fonts (Liberation Sans and Serif, metric-compatible with
Arial and Times New Roman). System fonts are discovered with fontdb and
registered lazily. A document that names a missing font renders with the
bundled fallback and `validate` warns about it. Golden tests use only the
fonts in `tests/fonts/`.

## The API layer

`pubrs-api::Session` owns open documents (keyed by a per-session `doc_id`),
one `Editor` per document, a per-document story layout cache (see "Layout
pipeline") plus the layout of the current revision, and the shared export
caches (fonts, decoded images, glyph outlines). Export and drawing borrow the
document next to these caches; they never copy it. Every operation is a
typed method `fn tool_name(&mut self, Request) -> ApiResult<Response>`; request
and response types derive `JsonSchema`, so the JSON Schemas published over MCP
come straight from the Rust types.

`registry::TOOLS` lists every operation by name with its schemas, description
and annotations (read-only, destructive, image output). The MCP server and
`pubrs script` both dispatch through `call_tool(session, name, json)`, so a
script is exactly a recorded sequence of MCP calls. Errors are always
`{code, message, hint}`; tool panics are caught and reported as `internal`.
Destructive tools require `confirm: true`. `doc_id` defaults to the most
recently used document.

## MCP server

`pubrs-mcp` wraps one `Session` in `Arc<Mutex<_>>` and implements rmcp's
`ServerHandler` by hand: `tools/list` publishes `registry::TOOLS` with their
generated input and output schemas and annotations; `tools/call` runs the
registry handler on a blocking thread and returns structured content (plus
an image block for `render_preview`), or a tool error whose text is
`{code, message, hint}`. Resources (`pubrs://fonts`, `pubrs://templates/{name}`,
`pubrs://doc/{id}`, `pubrs://doc/{id}/page/{n}.png`) and prompts
(`make_flyer`, `make_trifold`, `make_business_card`) are the only MCP-specific
logic. Transports: stdio (default) and streamable HTTP via axum.

MCP.md is generated from the registry and `docs/mcp-examples.json`; the
integration test executes every example through an in-process rmcp client.

## Templates

Templates are ordinary documents whose frames carry `placeholder` metadata
(name, kind, hint). Built-ins (`flyer`, `trifold_brochure`, `business_card`,
`newsletter`) are built in `pubrs-io::templates` through normal commands with
deterministic ids; user templates are `.pubrs` files in the user template
folder. A text placeholder counts as unfilled while its text equals its hint.

## GUI

`pubrs-app` (egui/eframe) is a client of the same `Session`: every edit is an
API call or a `Command` executed through `Session::exec` (typing uses
`exec_coalesced` so a burst of keystrokes is one undo step). Selection
handles, guides, the text caret and selection are egui overlays computed from
the layout (`textedit.rs`).

### Render pipeline (M13, ADR 0013)

No page is rasterised on the UI thread while editing. Per edit:

1. The command is applied to the `Arc`-shared model; `Change.touched` names
   what it changed and `Session` keeps the last 256 in a change log
   (`OpenDoc::changes_since`).
2. The layout cache recomposes the changed stories (ADR 0012).
3. Each egui frame the canvas takes a `PageSnapshot` (`Session::page_snapshot`):
   an `Arc<Document>` holding only the page, its master, their frames, the
   stories they show, the selection and the story being edited, plus the
   `Arc<DocumentLayout>` and an `ImageSnapshot` of the decoded images.
   It costs what the page holds.
4. When the revision, page, zoom or visible area changes, the canvas sends a
   `ViewRequest` to the render worker (`render_worker.rs`, one thread,
   newest request wins). The request names a region of the page raster at
   device resolution (72 × zoom × pixels per point): the visible pixels plus
   25% of the viewport on each side, clipped to the page and to the GPU's
   largest texture (the margin gives way first). The texture keeps its
   region while the visible pixels stay inside it and the page raster size
   is unchanged.
5. The worker keeps the snapshot behind the pixels it last sent. For the same
   region it computes the page damage (`pubrs-render/src/damage.rs`) from the
   logged `Touched` and the old and new layouts and renders only the damaged
   rectangles as tiles; otherwise the whole region. It never touches the
   `Session`. Each job runs under `catch_unwind`: a failure is reported and
   the next request is drawn in full; a dead worker thread is restarted.
6. The UI applies results in order: a full render replaces the texture, a
   damage repaint updates parts of it with `TextureHandle::set_partial`.
   Until a new texture lands the previous one is drawn, scaled and offset.
   The page origin is snapped to device pixels, so the texture is one texel
   per screen pixel (sharp at 800%; there is no texture size cap).
7. Page thumbnails are drawn by the same worker behind the viewport. A
   thumbnail is redrawn only when the page's fingerprint
   (`page_fingerprint`) changed.

Images keep half-size mip levels (`RasterImage::mips`, built on first use)
that only the GUI (the viewport and the page panel's thumbnails) samples, when
an image is drawn at less than half its size; exports, the thumbnail stored in
saved files and goldens never do.

- `keymap.rs` is the single table of actions, shortcuts (`Modifiers::COMMAND`
  = Cmd on macOS, Ctrl elsewhere) and menus; the egui menu bar and the native
  macOS menu (muda) are both built from it.
- `platform/` holds one trait per OS concern (`FileDialogs`, `AppPaths`,
  `Clipboard`, `SystemFonts`, `PrintBackend`) with per-OS implementations and
  a null implementation for headless runs. Printing always prints the PDF the
  exporter produced: CUPS `lpr` on Linux, `NSPrintOperation` (PDFKit) on
  macOS, the print dialog + GDI with WinRT's PDF renderer on Windows.
- `autosave.rs` writes recovery copies of unsaved documents every minute and
  offers them after a crash.
- `--self-test DIR` drives the real window with injected input through the
  Definition of Done scenario; CI runs it on every OS.

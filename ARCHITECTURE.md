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
`exec_coalesced` so a burst of keystrokes is one undo step). Pages are
rendered by `Session::page_rgba` (vello on the GPU when available, else
tiny-skia) into a texture; selection handles, guides, the text caret and
selection are egui overlays computed from the layout (`textedit.rs`). Each
egui frame, the canvas reads a snapshot of the current page only (the page,
its master, their frames, the stories they show, the selection and the story
being edited), never a copy of the whole document.

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

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
| `pubrs-io` | `.pubrs` file format (zip: `document.json` + `assets/`), schema migration, image decoding, SVG, templates, sheet presets, export orchestration, Publisher (`.pub`) import, mail-merge list readers (CSV, XLSX), spell checking. | core, layout, render, zip, image, cfb, calamine, spellbook |
| `pubrs-api` | `Session`: open documents + command dispatch + queries. **The single entry point** for every client. Tool registry (names, JSON schemas, handlers). | core, layout, render, io, schemars |
| `pubrs-mcp` | MCP server (rmcp) over stdio or streamable HTTP. Maps tools 1:1 onto the `pubrs-api` registry; only schema and error mapping. | api, rmcp, tokio |
| `pubrs-cli` | `pubrs render / export / validate / import / merge / perf / script / tools`. | api, clap |
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
(name, kind, hint). Built-ins (13: `flyer`, `trifold_brochure`, `business_card`, `newsletter`,
`greeting_card`, `postcard`, `certificate`, `menu`, `letterhead`, `envelope`,
`calendar`, `address_labels`, `business_cards`; ADR 0028) are built in
`pubrs-io::templates` through normal commands with deterministic ids; user
templates are `.pubrs` files in the user template folder. A built-in name
always wins: `TemplateStore::list` hides a user file with a built-in name,
`load` returns the built-in first, and `save` refuses those names. A text placeholder counts as unfilled while its text equals its hint.

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

## Design checker (M29, ADR 0027)

`validate` is split so a client can check part of a document.
`pubrs_core::validate_page(doc, page, opts)` returns the frame-level warnings
of one page; `validate_document(doc, opts)` the story-level ones;
`Session::validate_parts(doc, commercial, pages)` adds the layout-based ones
(overflow, missing fonts and glyphs, undecodable images) for just the pages
asked for. The warning codes include `rgb_in_cmyk_job` (only in a commercial
request), `text_too_small`, `low_contrast_text`, `object_in_unprintable_area`,
`empty_table_cells`, `missing_alt_text` and `merge_field_missing`. The GUI panel
(`checker.rs`, Review > Design Checker) debounces: a change starts a 400 ms
timer, and after it the panel re-checks only the pages whose frames or stories
changed (compared as `Arc`s), never on a keystroke and not at all while
closed. A commercial export runs the checks first and offers Export anyway,
Fix first or Cancel for missing fonts, RGB colour and low-resolution pictures.
Dangling swatch references are integrity errors (`check_integrity`), not
warnings.

## Mail merge (M24, ADR 0023)

`Document.merge: Option<MergeSource>` stores the list (fields, records as an
`Arc<Vec<Vec<String>>>`, preview index and switch) inside the document, so a
file reopens without its CSV or XLSX. `pubrs-io/merge_source.rs` reads the
lists: CSV in-tree (RFC 4180, delimiter sniffed), XLSX with `calamine` after
the workbook has been re-written as a sanitised zip (part counts, expansion
ratios and shared-string counts are checked, and `quick-xml` re-serialises the
shared strings) so no number a file declares is trusted. Values are sanitised
to one line (`merge::sanitize_value`) and limits are enforced by
`SetMergeSource` and `check_integrity`. A field is one U+FFFC character in a run
of its own with `field: Some(name)`, which `Run::same_format` never merges.
Layout shows the previewed record's value, or «Name», through an edit table on
the shadow text (the same one that holds soft hyphens), keyed in the story
cache by the field values, so stepping a record recomposes only stories with
fields. Merge to PDF lays out one document clone per record through one cache
and imposes the whole run once (so 30 records on a 5160 sheet in `Sequence`
fill are one sheet); `merge_to_publication` copies the pages with the fields
replaced by their values (`realise_story`) and ids derived from the originals.

## Spelling (M28)

`pubrs-io/spell.rs` wraps `spellbook` (Hunspell-compatible; en_US built in,
other languages from the user's dictionaries folder) and splits text into words
per paragraph language. In the app (`spell.rs`) one worker thread owns the
checker; the UI thread never checks a word. Requests are newest-wins, answers
carry the `Arc<Story>` they checked so squiggles are drawn only while that is
still the story on screen, and unchanged paragraphs come from a cache. A dead
worker is restarted (up to three times). The `spell_*` tools use the same
checker from a `Session`.

## Colour, CMYK and PDF/X output (M21, M22; ADR 0016, 0018)

Fills, strokes and text hold a `Paint`: a hex colour, a swatch reference with a
tint, or (fills only) a gradient. `Document.swatches` holds RGB, CMYK and spot
swatches and the colour scheme slots; colour is resolved at draw time
(`ResolvedPaint` / `ResolvedInk` carry the exact definition), so a swatch edit
repaints and never re-lays out. The PDF writer maps RGB to RGB, CMYK to
DeviceCMYK and spot colours to `/Separation`. PDF/X-4 is krilla's PDF 1.6 with
a CMYK profile and `no_device_cs`, plus an incremental update (`pdfx.rs`:
output intent, ICC stream, XMP, Info keys); `pdfx::check` is the gate every
PDF/X-4 export passes (veraPDF has no PDF/X profile). PDF/A-2b is krilla's
validator. Colour management and soft proofing are out of scope.

## Performance harness (M9)

`pubrs perf` (`pubrs-api/src/perf.rs`) builds a synthetic publication through
`DocBuilder` commands (letter pages, chains of text frames, generated PNGs)
and times what the GUI does: keystroke (command, layout, render), relayout,
tile render, save, open and PDF export, as medians over `--runs`. Rows have
stable keys; budgets are attached by key and page count, and only enforced ones
make `--check` exit 1. CI runs `--pages 50 --runs 5 --check` on Linux.

## Publisher (`.pub`) import (M31, M32; ADR 0017, 0026)

`pubrs-io/src/pubfile/` is an own parser (no libmspub code). `container.rs`
reads the OLE compound file with `cfb` under size caps; `records.rs` parses the
Contents, Quill and Escher streams with every count and offset clamped to the
stream it points into; `text.rs` decodes story text and formatting;
`images.rs` reads the BLIP store (PNG, JPEG, GIF; metafiles are reported, not
drawn); `mod.rs` maps it all onto a `Document` through `DocBuilder` commands
(pages, linked text frames, shapes, tables, colour scheme swatches, masters) and
returns an `ImportReport` of what was dropped or substituted. `import` catches
panics. Entry points: `doc_import` (API and MCP), `pubrs import`, File > Import
Publisher File… and dropping a `.pub` file on the window.

## Architecture decision records

| ADR | Decision |
|-----|----------|
| [0001](docs/adr/0001-patch-based-undo.md) | Patch-based undo |
| [0002](docs/adr/0002-macos-intel-runner.md) | macOS Intel CI runner |
| [0003](docs/adr/0003-license.md) | Project licence |
| [0004](docs/adr/0004-gui-renderer-fallback.md) | GUI renderer fallback |
| [0005](docs/adr/0005-ids-in-commands.md) | Ids are allocated before commands are built |
| [0006](docs/adr/0006-wrap-bounding-box.md) | Text wrap uses bounding boxes |
| [0007](docs/adr/0007-paragraph-shaping.md) | One parley layout per paragraph |
| [0008](docs/adr/0008-golden-tolerance.md) | Golden images compare with a tolerance |
| [0009](docs/adr/0009-image-decoding-in-io.md) | Image decoding lives in pubrs-io |
| [0010](docs/adr/0010-packaging.md) | Installers: one release workflow, per-OS native tools |
| [0011](docs/adr/0011-gui-adapter.md) | GUI adapter: hardware on any API first, then bundled Mesa llvmpipe |
| [0012](docs/adr/0012-story-layout-cache.md) | Story layout cache: per-story snapshot keys, compared by equality |
| [0013](docs/adr/0013-viewport-rendering.md) | Viewport rendering: exact parallel tiles off the UI thread; vello leaves the GUI |
| [0014](docs/adr/0014-hyphenation.md) | Hyphenation and bounded justification: a shadow text with soft hyphens |
| [0015](docs/adr/0015-break-characters.md) | Break characters, keeps and drop caps |
| [0016](docs/adr/0016-paint-and-swatches.md) | Paint, swatches and resolving colour at draw time |
| [0017](docs/adr/0017-pub-import.md) | Publisher (`.pub`) import: own parser, bounded reads, three file generations |
| [0018](docs/adr/0018-pdfx.md) | Commercial print output: CMYK and spot colour, PDF/X-4 as an incremental update, PDF/A-2b |
| [0019](docs/adr/0019-tabs-and-lists.md) | Tab stops, leaders, bullets and numbering |
| [0020](docs/adr/0020-wordart-shadows-borderart.md) | WordArt, drop shadows and BorderArt |
| [0021](docs/adr/0021-character-formatting.md) | Character formatting and missing glyphs |
| [0022](docs/adr/0022-tight-wrap.md) | Tight, through and both-sides wrap |
| [0023](docs/adr/0023-mail-merge.md) | Mail merge |
| [0024](docs/adr/0024-tables.md) | Tables |
| [0025](docs/adr/0025-clipboard-ime-rtl.md) | Clipboard, input methods and right-to-left carets |
| [0026](docs/adr/0026-pub-import-part-2.md) | Publisher (`.pub`) import, part 2: pictures, shapes, colours, tables, masters and entry points |
| [0027](docs/adr/0027-design-checker.md) | Design Checker: warnings, commercial context, incremental panel |
| [0028](docs/adr/0028-everyday-templates.md) | Everyday templates (M30) |

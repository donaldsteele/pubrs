# Project: `pubrs` — Rust desktop publishing app (MS Publisher replacement)

You are building this project end to end, autonomously, in one run. Do not
stop for review. Make decisions, record them, keep going. If the session is
interrupted, a later session must be able to resume from PROGRESS.md alone.

## Context

Microsoft Publisher reached end of life in October 2026. I need a native,
cross-platform desktop publishing app in Rust covering the workflows
Publisher users rely on: flyers, newsletters, brochures, cards, labels, small
print jobs. Target user: non-designer, small business/home. Simplicity over
power. Not competing with InDesign/Affinity.

Windows, macOS, and Linux are equal first-class targets. Nothing ships
unless it works on all three; "compiles on Linux" is not done.

Second, equal-priority goal: **every document operation must be reachable
through an MCP server** so an AI agent can create, edit, inspect, render, and
export Publisher-style documents with no human and no GUI. The GUI and the
MCP server are two clients of the same headless core.

## Deliverables (all required)

1. Cargo workspace. Runs natively on Windows 10/11 (x64, arm64), macOS 12+
   (Apple Silicon + Intel, universal binary), Linux (x64, arm64; X11 and
   Wayland).
2. Working GUI app.
3. `pubrs-mcp` binary: MCP server over stdio (default) and streamable HTTP
   (`--http <addr>`), implemented with the official `rmcp` crate.
4. Headless CLI: `pubrs render`, `pubrs export`, `pubrs validate`,
   `pubrs script <file.json>` (replays a command list — the same commands
   MCP uses).
5. Docs: ARCHITECTURE.md, MCP.md (full tool reference with JSON examples),
   FILE_FORMAT.md (schema spec), USER_GUIDE.md (short, with per-OS install
   section), RELEASE.md, PROGRESS.md, TODO.md, DEPENDENCIES.md.
6. Tests: unit (core/layout/io), golden-image render tests, PDF validation
   tests, MCP integration test that drives the server as a client.
7. Example MCP configs for Claude Desktop and Claude Code in `examples/mcp/`.
8. Installers built by CI for every tagged release:
   Windows: MSI via `cargo-wix` (code-signing hook left as a documented
   placeholder). macOS: signed-ready `.app` bundle + `.dmg` via
   `cargo-bundle`, with `Info.plist` document-type association for `.pubrs`.
   Linux: AppImage + `.deb`, plus a Flatpak manifest in `packaging/flatpak/`.
   All three register the `.pubrs` file type and an icon.

## Architecture (fixed; do not deviate without recording why in ARCHITECTURE.md)

Workspace crates, dependency direction strictly downward:

- `pubrs-core`     Document model, IDs, geometry, styles, command system,
                   undo/redo, validation. No I/O, no UI, no rendering deps.
- `pubrs-layout`   Text shaping and flow. Linked frame chains, overflow,
                   text wrap around objects, paragraph/char styles → glyph runs.
- `pubrs-render`   Scene → raster (PNG) and → vector (PDF). Takes laid-out
                   document, never mutates it.
- `pubrs-io`       Own file format (zip: `document.json` + `assets/`), image
                   decode, SVG, template packs, PDF/PNG export orchestration.
- `pubrs-api`      Thin façade: `Session` struct = open documents + command
                   dispatch + query functions. THE single entry point used by
                   both GUI and MCP. If a feature isn't in `pubrs-api`, it
                   doesn't exist for either client.
- `pubrs-mcp`      MCP server. Maps tools 1:1 onto `pubrs-api`. No logic of
                   its own beyond JSON schema and error mapping.
- `pubrs-cli`      Headless CLI over `pubrs-api`.
- `pubrs-app`      GUI over `pubrs-api`.

Rules:

- Every mutation is a `Command` (serde-serializable, invertible, has a stable
  string name). Undo/redo = command stack. MCP tools, GUI actions, and CLI
  scripts all emit the same Commands. This is non-negotiable; it is what
  makes AI automation and undo both free.
- Every object has a stable UUID. MCP addresses objects by ID, never by index.
- Document JSON schema is versioned (`schema_version`), with a migration
  function from every past version.
- `pubrs-core` must compile with `--no-default-features` and no OS deps.
- No `#[cfg(windows)]`-, `#[cfg(target_os = "macos")]`-, or
  `#[cfg(target_os = "linux")]`-only code paths outside `pubrs-app` and
  `pubrs-cli`. Core, layout, render, io, api, mcp are platform-agnostic.

## Core features (MVP, build in this order)

1. Document model: multi-page, page sizes (Letter, A4, A5, custom), margins,
   bleed, master pages, ruler guides, snap-to-guide/grid.
2. Frames: text, image, shapes (rect, ellipse, line), groups, z-order,
   rotate/resize, lock, align/distribute.
3. Text: rich text runs (font family, size, bold/italic, color), paragraph
   styles, alignment, line spacing, **linked text frames with overflow
   flow**, text wrap around objects.
4. Images: PNG/JPEG/SVG import, crop, scale, aspect lock.
5. Templates: flyer, trifold brochure, business card, newsletter; save user
   templates. Templates are ordinary documents with placeholder metadata.
6. Output: PDF (print-ready: embedded/subset fonts, bleed, crop marks, PDF
   boxes set correctly), PNG, system print dialog.
7. File format: zipped JSON + assets, versioned.
8. Undo/redo for everything, autosave, crash recovery.
9. MCP server exposing all of the above (see MCP spec below).

## Later phases (do NOT build; design so they fit; list in TODO.md)

Mail merge from CSV · .pub import (ref: libmspub) · CMYK/spot, PDF/X ·
Tables · WordArt-style effects · Avery label sheet presets.

## MCP server spec

Use `rmcp` (official Rust MCP SDK). Implement tools, resources, and prompts.

Transport: stdio default; `--http 127.0.0.1:PORT` for streamable HTTP.
Server holds a `Session` (multiple open documents keyed by `doc_id`).

Design rules for tools:

- Flat, verb-first names. Small, composable. Every tool returns the affected
  object IDs and a `revision` counter so agents can detect stale state.
- Every tool input/output has a JSON Schema generated from the Rust types
  (`schemars`). Descriptions written for an LLM reader: units (points),
  coordinate origin (top-left of page, y down), defaults.
- Errors are structured: `{code, message, hint}`. Never panic across the
  MCP boundary.
- Idempotent where possible; destructive tools (delete, close without save)
  require `confirm: true`.

Required tools (minimum; extend if a core feature isn't reachable):

```
Document:   doc_new, doc_open, doc_save, doc_save_as, doc_close, doc_list,
            doc_info, doc_set_page_size, doc_set_margins, doc_set_bleed
Pages:      page_add, page_delete, page_reorder, page_list, page_info,
            master_create, master_apply, guide_add, guide_remove
Frames:     frame_add_text, frame_add_image, frame_add_shape, frame_delete,
            frame_move, frame_resize, frame_rotate, frame_set_zorder,
            frame_group, frame_ungroup, frame_lock, frame_align,
            frame_distribute, frame_set_style (fill/stroke/opacity),
            frame_list (filterable by page/type), frame_get
Text:       text_set (whole frame, plain or minimal markup), text_insert,
            text_replace_range, text_apply_char_style, text_apply_para_style,
            text_link_frames, text_unlink_frames, text_get (with run/style
            info), text_overflow_status (which chains overflow, by how much)
Styles:     style_list, style_create, style_update, style_delete
Images:     image_place (path or base64), image_crop, image_fit (fill/fit/
            stretch), image_replace
Templates:  template_list, template_apply (returns placeholder IDs with
            semantic names like "headline", "hero_image"), template_save
Export:     export_pdf (options: bleed, crop_marks, font_embedding),
            export_png (page, dpi), render_preview (page → PNG base64, low
            dpi, so an agent can LOOK at its work)
History:    undo, redo, history_list
Query:      doc_to_json (full document), validate (returns warnings:
            overflow, missing fonts, images below 150 dpi, objects off page,
            empty placeholders), fonts_list
```

Resources (read-only URIs):

- `pubrs://templates/{name}`            template metadata + placeholder list
- `pubrs://doc/{doc_id}`                current document JSON
- `pubrs://doc/{doc_id}/page/{n}.png`   preview image
- `pubrs://fonts`                       available fonts

Prompts:

- `make_flyer`, `make_trifold`, `make_business_card`: guided workflows that
  tell an agent which tools to call in which order.

Acceptance test (must pass, run in CI): an `rmcp` client, in-process, with no
GUI, calls tools to: apply trifold template → fill all placeholders → place
two images → link frames and verify `text_overflow_status` is clean → run
`validate` with zero warnings → `export_pdf` → save as `.pubrs` → `doc_open`
the saved file → `doc_to_json` equals original.

## Tech constraints

- Rust stable, edition 2024. `cargo clippy --all-targets -- -D warnings`
  clean, `cargo fmt` clean, `cargo test --workspace` green at every
  milestone boundary.
- Stack (use these unless you record a concrete blocker in DEPENDENCIES.md):
  - GUI: egui + eframe (fastest path to a working canvas; wrap it so the
    `pubrs-app` crate could be swapped later)
  - Rendering: vello (GPU) with tiny-skia fallback for headless/CI PNG
  - Text: parley + swash for shaping/layout; fontdb + fontique for fonts
  - PDF: krilla (handles font subsetting/embedding)
  - Images: image, resvg, usvg
  - MCP: rmcp, schemars, serde, serde_json, tokio
  - Platform: rfd, dirs, arboard, windows (Windows only), objc2 +
    objc2-app-kit (macOS only)
  - Misc: uuid, thiserror, anyhow (binaries only), tracing, zip, camino
- Any other dependency: allowed only with a one-line justification appended
  to DEPENDENCIES.md. No deps with <1k downloads/month, no unmaintained
  crates, no git deps.
- Platform rules:
  - Platform-specific behaviour lives behind one trait per concern in
    `pubrs-app/src/platform/` (`PrintBackend`, `FileDialogs`,
    `SystemFonts`, `AppPaths`, `Clipboard`), one impl per OS, selected at
    compile time. A `NullPlatform` impl exists for headless tests.
  - Printing: Windows via the system print dialog + GDI/XPS through the
    `windows` crate; macOS via `NSPrintOperation` through `objc2` /
    `objc2-app-kit`; Linux via CUPS (`lpr` with PDF). All three paths print
    the same PDF the exporter produces; do not write three rasterizers.
  - File dialogs: `rfd` (native on all three). Paths: `dirs` for config,
    autosave, and user-template locations per OS convention.
  - Fonts: `fontdb` system scan on each OS, plus bundled fallback fonts
    (OFL-licensed) so a document renders identically on a machine missing
    the original font, with a "missing font" warning from `validate`.
  - GPU: vello on wgpu (DX12 / Metal / Vulkan). Must fall back to tiny-skia
    software rendering when no GPU adapter is available (VMs, CI, remote
    desktop) and log which path is active.
  - HiDPI: all three scale factors handled; no hardcoded 96 dpi in UI code.
  - Line endings, path separators, case-sensitive filesystems: tests must
    pass on all three. Use `camino` or `PathBuf` correctly; never string-
    concatenate paths. Zip entries in `.pubrs` always use `/`.
  - Menus and shortcuts: Cmd on macOS, Ctrl elsewhere, from one keymap
    table. Native menu bar on macOS.
- CI (GitHub Actions): matrix `windows-latest`, `macos-latest` (arm64),
  `macos-13` (Intel), `ubuntu-latest`. Every job runs fmt, clippy, full
  test suite, headless render goldens, MCP integration test. Linux job
  also runs under `xvfb` to smoke-launch the GUI and exit. Release workflow
  on tag builds all installers from Deliverable 8 and uploads them as
  artifacts.

## Process (autonomous)

Milestones. Each ends with: build green, tests green, clippy clean, a
`demos/mN_*.md` note with what works and a command to see it, PROGRESS.md
updated with milestone status and the next concrete step.

```
M0  Workspace skeleton, 4-job CI matrix green on a hello-world GUI that
    opens a window and exits on all three OSes, ARCHITECTURE.md,
    FILE_FORMAT.md draft, command system + undo/redo with tests.
M1  Core document model + file format round-trip (save → load → equal).
M2  Layout: single text frame shaping; then linked chains and overflow.
    Golden tests with fixed fonts bundled in `tests/fonts/`.
M3  Render to PNG (headless), golden-image tests with tolerance.
M4  PDF export via krilla; test asserts page count, MediaBox/BleedBox/
    TrimBox, fonts embedded (parse the PDF back, don't trust the writer).
M5  `pubrs-api` Session façade + CLI. Script replay test.
M6  MCP server: all tools, resources, prompts. Integration test above.
    MCP.md generated from schemas plus hand-written examples.
M7  GUI: canvas, selection, handles, properties panel, pages panel, text
    editing in place, image placement, undo/redo, autosave, recovery.
M8  Templates (4 built-in), print dialog on all three platforms, installers
    for all three built by release workflow, USER_GUIDE.md with per-OS
    install section, RELEASE.md, polish pass, final verification against
    Definition of Done.
```

Rules:

- Never stub a feature silently. Partial = entry in TODO.md with what's
  missing and why.
- Write the test before or with the feature, not after the milestone.
- When a design question arises that isn't settled above: pick the simpler
  option, write a 3-line ADR in `docs/adr/NNNN-title.md`, continue.
- Do not refactor earlier milestones for style. Only touch them to fix a
  real defect or to add an interface the current milestone needs.
- You can only run tests on the OS you are on. CI is the verifier for the
  other two. Treat a red CI job on another OS as a blocking defect for the
  current milestone; fix it from the logs before moving on.
- Keep PROGRESS.md current after every milestone and before any long
  operation. Format: milestone table (status), "next step" line, known
  failing tests, open questions.
- If interrupted and restarted: read PROGRESS.md, TODO.md, run the test
  suite, continue from "next step".

## Definition of done (verify explicitly at the end, report each item)

1. GUI: create a two-page trifold brochure from a template, edit text that
   flows across linked frames, place and crop photos, export a PDF with
   bleed and embedded fonts, close, reopen, identical layout.
2. MCP: the acceptance test above passes, and a human can configure Claude
   Desktop with `examples/mcp/claude_desktop_config.json` and ask it to
   "make me a trifold brochure for a bakery" with no further help.
3. `cargo test --workspace` green on Windows, macOS (arm64 + Intel), Linux
   in CI. Golden-image tests produce byte-identical PNGs across OSes (use
   bundled fonts and the software renderer for goldens).
4. Every tool in MCP.md has a working example that the integration test
   actually executes.
5. Release workflow produces MSI, DMG, AppImage, and .deb from one tag.
   Each installer installs, launches, opens a sample `.pubrs`, exports a
   PDF, and uninstalls cleanly. Document the manual check in RELEASE.md.

Report at the end: what's done, what's in TODO.md, what the test counts are,
and the exact commands to run the GUI, the MCP server, and the tests on each
OS.

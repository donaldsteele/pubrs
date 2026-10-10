# M7: GUI

What works:

- Window with menus from one keymap table (Cmd on macOS / Ctrl elsewhere; native NSMenu on macOS), toolbar
  tools (select, text, picture, rectangle, ellipse, line), document tabs, pages panel with thumbnails,
  properties panel (position/size/rotation in in/mm/pt, fill, line, opacity, wrap, picture fit/crop, text
  style/font/size/bold/italic/underline/colour/alignment/spacing, columns, vertical alignment, page setup).
- Canvas: GPU (vello) or software page rendering, margins/guides/bleed overlays, selection with resize and
  rotation handles (rotation aware, aspect lock for pictures), marquee, snapping, nudge, overflow markers,
  linked-chain connectors, double-click to edit text in place (caret, selection, arrows/Home/End/word moves,
  typing merged into one undo step, clipboard), double-click a picture frame to place a picture.
- Undo/redo, templates picker, validation dialog, save/open/export PDF/PNG, print dialog per OS,
  autosave + crash recovery, settings persisted per OS convention.
- `--self-test` drives the GUI end to end (Definition of Done scenario) and is run by CI on every OS.

See it:

```
cargo run -p pubrs-app
xvfb-run -a cargo run -p pubrs-app -- --self-test /tmp/pubrs-selftest   # Linux headless; screenshots in that folder
```

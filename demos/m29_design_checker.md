# M29: Design checker

What works:

- **Six new warning codes** in `validate` (and `pubrs validate`, MCP `validate`), each with a hint, the page and the
  object ids: `rgb_in_cmyk_job` (commercial only), `text_too_small` (under 6 pt), `low_contrast_text` (WCAG
  contrast ratio under 3:1, against the frame's fill, a frame beneath, or white), `object_in_unprintable_area`
  (within 0.25 in of the edge; desktop jobs without bleed), `empty_table_cells`, `missing_alt_text`; the earlier
  `missing_glyphs` (names the characters) is listed too. `merge_field_missing` is pending mail merge (ADR 0027).
- **Commercial flag**: `validate {commercial: true}` / `pubrs validate FILE --commercial` also flags RGB colours.
  The document has no colour mode, so this is a request flag; the panel's box and the pre-export check set it.
- **Design Checker panel** (Review → Design Checker; the toolbar Check button and View → Check for Problems… open
  it): a bottom panel (between the pages and properties panels, so the page keeps its width) listing issues under
  "Page N (count)" headers and "Publication". Click an issue to go to its page and select the object.
- **Cheap**: nothing runs on a keystroke. A change starts a 400 ms timer; once quiet, only the pages whose frames or
  stories changed are checked again (a one-letter edit on a 20-page document checks 1 page; unit-tested).
- **Before a commercial export**: Export… and Save for a commercial printer… list missing fonts, RGB colour and
  low-resolution pictures first, with Export anyway / Fix first / Cancel.
- **Integrity**: `check_integrity` now flags references to swatches that do not exist (fills, outlines, shadows,
  WordArt, table cells, run and style colours, gradient stops).
- **Follow-ups**: Replace All in a table is one undo step that grows the table (a batch through `App::exec`);
  Change All in the spell window refuses a stale report; the CLI and MCP server use the app's dictionaries folder;
  over-large clipboard pictures (200 MP) are refused before they are copied.

See it:

```
cargo run -p pubrs-cli -- validate my.pubrs --commercial   # any .pubrs file
cargo run -p pubrs-app                                                                          # Review → Design Checker
cargo run -p pubrs-app -- --self-test target/gui-selftest                                       # steps 198-208
```

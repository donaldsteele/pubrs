# M19: Tables

What works:

- **Tables are frames.** `table_add` puts a rows × columns table on a page: equal columns and rows filling the
  rectangle, hairline borders, 4 pt insets, an optional header fill. Every cell is an ordinary text story, so
  `text_get`, `text_set`, `style_apply`, `text_find` and `validate` work on a cell through its `story_id`
  (`table_get` lists them). A text tool given the table's `frame_id` says to use a cell's story instead.
- **Rows grow with their text.** A row is as tall as its tallest cell, at least its minimum height. Rows stop at the
  frame's bottom; text that no longer fits is dropped from view and reported by `text_overflow_status`, `frame_get`
  and the overflow warning of `validate`, with the cell named by its story.
- **Rows, columns, merging.** `table_insert_rows`/`_cols`, `table_delete_rows`/`_cols` (confirm), `table_merge_cells`
  (the other cells' text is appended as paragraphs), `table_split_cell`, `table_set_col_width` (the frame follows the
  column total), `table_set_row_height`. All of it is one undo step each.
- **Cell look.** `table_set_cell_style` sets fill, each of the four borders (colour, width, dash), insets and vertical
  alignment (top, middle, bottom), for one cell or a range. A border two cells share is drawn once, the later
  cell winning.
- **Output.** Cell fills, text and borders are drawn like any frame, on screen, in PNG and in PDF (fills and borders as
  artifacts, text as extractable text; no Table structure tags yet).
- **Schema 11.** Identity migration; `tests/fixtures/schema_v11.pubrs` holds a table with a merged header.
- **GUI.** A table shows on the canvas and the properties panel says "Table: R×C". Double-click a cell to type in it;
  everything else (picker, selecting cells, merge and split from menus) is T12.

Golden: `table_merged_header` (6 rows × 4 columns: merged header, fills, thick and dashed borders, insets, middle and
bottom alignment, a merged 2 × 1 cell and a row grown by wrapped text). Run `cargo test -p pubrs-core --test tables`,
`cargo test -p pubrs-layout --test tables` and `cargo test -p pubrs-io --test render_golden`.

Limits (ADR 0024, TODO.md): splitting only undoes a merge; a cell shows whole lines only; the frame's height is a limit,
not a size.

Try it:

```
claude mcp add pubrs -- pubrs-mcp
# "Add a 4-row, 3-column price table under the headline with a blue header, merge the header row, and fill it in."
```

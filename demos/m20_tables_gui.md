# M20: Tables in the GUI

What works (everything is also reachable through the tools of M19; this is the by-hand side):

- **Insert → Table…** A grid picker (8 rows by 10 columns of squares, hover and click) plus Rows (1-200) and Columns
  (1-40) fields; Insert makes one undo step through `Session::table_add`: centred on the page, two thirds of the margin
  box wide, 24 pt rows, 0.5 pt black borders. The new table is selected with its first cell in edit.
- **Selecting.** First click selects the table like any frame; a click in a cell of the selected table, or a double
  click, edits that cell at the clicked offset. Dragging from one cell into another (or Shift+click a second cell while editing) selects a
  rectangle of cells (`DocView.table_sel`, extended to cover merged cells, drawn as a translucent overlay); Esc or a click
  elsewhere clears it. Alt+drag moves a selected table.
- **Tab** and **Shift+Tab** move to the next or previous cell in row-major order (merged-over positions have no cell),
  with the caret at the end of the text. Tab in the last cell inserts a row (stories allocated first, ADR 0005) and edits
  its first cell: one undo step. Control+Tab (Control on macOS too) types a tab.
- **Resizing.** Within 4 screen pixels of an interior column border or a row's bottom the pointer becomes a resize
  cursor (cached layout only; borders inside merged cells are not borders). `Drag::TableCol` and `Drag::TableRow` preview a
  guide line; release commits one `TableSetColWidth` (the column left of the border; the table's width changes) or
  `TableSetRowHeight`, clamped to at least 6 pt.
- **Context menu.** Right-click in a table: Insert Row Above/Below, Insert Column Left/Right, Delete Row(s)/Column(s)
  (the model's "keeps at least one row" error shows in the status bar), Merge Cells (more than one cell selected),
  Split Cell (merged cell), Cell Properties… (scrolls the panel to the Cell section). On a misspelt word the spelling
  items come first, then a separator, then these. Each item is one undo step.
- **Cell properties** (properties panel, "Cell"): fill (swatch, custom colour or none), borders (All/Top/Right/Bottom/Left,
  colour, width, None), vertical alignment, insets (all, or each side), plus the width of the current column and the
  minimum height of the current row. Edits apply to the cell being edited, the selected range, or every cell, as one
  batch; sliders merge into one step per control.
- **The frame follows the rows.** M19 never grows a table frame, so after structural edits, row-height changes and while
  typing makes a row taller, the GUI grows the frame to the layout's natural height inside the same undo step.
- **Typing** coalesces per cell story ("type-{story}"): five characters typed then undone is one step.

Self-test steps 161-179: Insert → Table… (grid click, 3x3), type "cell0".."cell8" with Tab through the nine cells,
undo and redo of the last cell's five characters, Tab in the last cell (4 rows) and undo (3 rows), a drag across the header
row, right-click → Merge Cells (1x3 span, text kept), a column border drag (width changed by the pointer's travel), the
fill from the panel, and a PDF export that parses. Screenshots: the dialog, the context menu and the finished table.

```
xvfb-run -a -s "-screen 0 1600x1000x24" cargo run -p pubrs-app -- --self-test target/gui-selftest
```

Or by hand: `cargo run -p pubrs-app`, New → Blank, Insert → Table…, Insert, type, Tab, right-click.

# M23: Label and card sheets, and booklets

What works:

- 26 sheet presets as data in `crates/pubrs-io/src/presets.rs`: Avery US 5160, 5161, 5162, 5163, 5164, 5167, 5371,
  5376, 5377, 8160, 8163, 22805, 5305, 8315; A4 L7160, L7161, L7162, L7163, L7173, L7651, C32011, J8160; generic
  2-up and 4-up on Letter and on A4. Each is a sheet size, rows, columns, label size, first-label corner and
  pitch, written in inches or millimetres. A test proves every grid fits on its sheet.
- `PageSetup.sheet` (`SheetLayout`) and `PageSetup.booklet`, set by the invertible commands `SetSheet` and
  `SetBooklet`. A sheet whose label size differs from the page by more than 0.5 pt is refused with the hint
  "set the page size to 2.625 x 1.000 in first". A document is either a sheet or a booklet.
- Imposition (`crates/pubrs-render/src/impose.rs`) works on finished page scenes. A label sheet is one
  sheet-sized scene whose nodes are the label page's nodes moved to their cell and clipped to it (`Node.clip`,
  honoured by the PDF writer and the rasteriser). `repeat` puts one page in every cell of its own sheet;
  `sequence` fills cells with pages in order, which is where mail merge will plug in one record per label.
  Booklets pad to a multiple of 4 and use the saddle-stitch order: 8 pages give [8,1] [2,7] [6,3] [4,5], each
  side its own PDF page, twice the page width.
- `export_pdf` and printing impose by default (`PdfOptions.flat` / `impose: false` opts out). Per-page PNG
  export stays per page. Printing sends the same PDF, so the OS print path needs no change.
- MCP: `sheet_presets_list`, `doc_set_sheet` (preset, custom grid, or none to clear; fill), `doc_set_booklet`.
  `doc_info` reports `sheet` and `booklet`; `export_pdf` reports `imposed`.
- GUI: File > Page Setup... (Ctrl/Cmd+Shift+P), tab "Labels, Cards, Booklet": mode radios, preset picker, fill,
  a resize-page-to-label checkbox and a live sheet or booklet preview with page numbers. Disabled controls say
  why on hover. The Print dialog and the status bar say which sheet or booklet is printed.
- Schema 2 (identity migration). `tests/fixtures/schema_v1.pubrs` (the bakery sample) and
  `tests/fixtures/schema_v2.pubrs` (a 5160 sheet) open and round-trip; the bakery sample renders to the same
  golden as before.

See it:

```
cargo test -p pubrs-render --test impose
cargo test -p pubrs-io --test render_golden imposed
xvfb-run -a cargo run -p pubrs-app -- --self-test target/gui-selftest
```

Goldens: `tests/golden/render/imposed_sheet_avery_5160.png`, `imposed_booklet_front_1.png`,
`imposed_booklet_back_1.png`, `bakery_trifold_schema1.png`.

# TODO

Deferred and partial work. Each entry says what is missing and why.

## Later phases (designed for, not built)

- Mail merge from CSV.
- `.pub` import beyond M31 part 1 (reference: libmspub).
- CMYK / spot colours, PDF/X output.
- Tables.
- Avery label sheet presets.

## Partial features

- Character formatting caches each face's OS/2 and `smcp` data per (family, bold, italic) in the `Layouter` and never clears it; once fonts can be added at runtime, clear it (or key it on a font-library generation) so a newly added family replaces the fallback face's data.
- Sheet presets Avery 5376, 5377, 22805, 5305, 8315, C32011, L7651 and the L7161/L7162 margins are from memory of published specs; check them against vendor datasheets.
- Imposed sheets carry no bleed, and `crop_marks` marks the whole sheet rather than each label.
- `SetPageSize` does not re-check an existing label sheet; a mismatch is reported at PDF export.
- The GPU renderer ignores `Node.clip` (only imposed sheets use it, and those are never drawn on the GPU).
- The `validate` tool still clones the whole document on every call (`crates/pubrs-api/src/tools/query.rs`); `Session::validate_parts` (M29) still clones the `Document` (maps of `Arc`s) per call, once per refresh of the Design Checker panel, not per keystroke. (The canvas page snapshot now shares frames and stories by `Arc` and is built by `Session::page_snapshot`, M13.)
- vello is no longer used by the GUI (ADR 0013), but `crates/pubrs-app/Cargo.toml` still enables `pubrs-api/gpu`, so the app compiles and links it. Dropping the feature from the app was outside M13's files.
- Character formatting (M17, ADR 0021): real `smcp` is detected from the GSUB feature list only (not per-letter coverage); a strikethrough through synthesized small caps is stepped; the Character dialog has no preview and no letter-spacing or colour fields (the text panel has them); the super/subscript buttons are in the text panel, not a toolbar.
- Windows and macOS printing are compile-checked here; they need the manual check in RELEASE.md (no printers on CI).
- macOS: double-clicking a `.pubrs` file in Finder launches pubrs but does not open the file, because winit does not deliver `application:openURLs:` events. Workaround: File → Open, or drag the file onto the pubrs window (ADR 0010).
- Installers are unsigned unless the signing secrets in RELEASE.md are set; macOS notarization is a manual step.
- Flatpak: manifest and AppStream metadata only. It is not built in CI, and Flathub submission needs generated `cargo-sources.json`.
- The Windows ARM64 MSI is built but not install-checked in CI (no ARM64 Windows runner in the release workflow).
- The Windows ARM64 MSI has no bundled Mesa (mesa-dist-win publishes x64 and x86 only), so ARM64 Windows without a GPU driver still draws with WARP: about 0.5 CPU-seconds per frame (ADR 0011).
- Bundled Mesa is pinned to mesa-dist-win 26.2.4. Updating it means a new version and SHA-256 in `packaging/windows/fetch-mesa.ps1` and `packaging/windows/mesa/THIRD-PARTY-NOTICES.txt`.
- Find and replace reaches WordArt text (M25), but the GUI selects the WordArt frame (it has no caret), Replace replaces only the first match in the selected frame, and "selection only" never includes WordArt; table cells are covered because they are stories.
- Guides on master pages are drawn on the pages using them but can only be moved through `guide_move` (there is no master-editing view in the GUI).

## Hyphenation (M14)

- Hyphenation languages left out (licence unclear or not allowed): Afrikaans, Danish, Greek, Estonian, Croatian, Icelandic, Italian, Georgian, Kurmanji, Latin, Mongolian, Dutch, Russian, Sanskrit, Serbian (Cyrillic), Slovenian, Swedish, Turkish and Ukrainian (LPPL-only or LPPL with MIT alternatives not yet checked); Slovak (GPL). Possible additions once checked: Belarusian, Bengali, Assamese, Gujarati, Hindi, Kannada, Malayalam, Marathi, Odia, Punjabi, Tamil, Telugu, Turkmen, Albanian, Czech and Finnish (headers say MIT, or MIT alongside GPL/LGPL).
- English US pattern licence (Kuiken's notice-preserving grant) is outside the LD5 list; the owner must confirm (ASSETS.md).
- The built-in styles leave `justify_limits` unset (v0.1 behaviour, LD2); the M30 templates enable limits in their Body style (ADR 0028); the defaults of a blank document are unchanged.
- Letter-spaced justification spaces characters, not grapheme clusters, and uses the mean font size of the line for its cap.
- The Paragraph dialog is under Text → Paragraph… (Ctrl/Cmd+Alt+P); there is no Format menu.

## Paragraph controls and breaks (M15)

- Autoflow (adding pages and linked frames for overflowing text) is not implemented; a page break at the end of a chain overflows.
- The built-in styles leave `widow_orphan` unset (v0.1 layout, LD2); the M30 templates enable `widow_orphan = 2` in their Body style (ADR 0028); the defaults of a blank document are unchanged.
- A keep-with-next paragraph that itself spans two columns or frames is not moved, and its last lines are not split off to join the next paragraph (ADR 0015).
- Keeps (keep together, widows and orphans) are skipped for a paragraph that contains a break character.
- Keeps never cost visible text: a move that makes its own paragraphs overflow is undone, and when moves push later text out of the chain the whole story is laid out without keeps (all or nothing, ADR 0015). Finer recovery (dropping only the move that hid the text) is not done.
- With a drop cap, the first word is hyphenated without its cap letters (the patterns see "ourselves" for "Ourselves" with a one-letter cap).
- A drop cap is drawn in the style of its first character even when its characters differ; right-to-left paragraphs get the cap on the left. Clicks beside the cap on lines 2..N land on those lines (the cap is hit through the first line).
- Insert ▸ Break ▸ Frame Break has no shortcut (Page: Ctrl/Cmd+Enter, Column: Ctrl/Cmd+Shift+Enter). Drop Cap… is under Text, as there is no Format menu.

## Tab stops and lists (M16)

- Lines are broken before tabs are placed, so text after a tab that a stop pushes past the right edge overhangs instead of wrapping (ADR 0019).
- A paragraph with a tab is set flush left whatever its alignment: centre, right and justified paragraphs with tabs are not supported.
- A paragraph with a drop cap shows no list label; the first-line indent is ignored for list items.
- List labels use the first character's style and cannot be styled on their own (no label font, colour or bullet picture).
- Tab stops are measured from the column's text edge; there is no ruler on the canvas to drag them (use Text ▸ Tabs…), and no "tab stops from the ruler" while editing.
- Numbered lists cannot be continued from another story or restarted with a dialog "continue" option; use Start at.
- Multilevel numbering does not yet join levels ("1.a."); every level shows its own number.
- The Tabs dialog's ruler is as wide as the text box; a stop past its right end is only reachable with the position field.

## `.pub` import (M31)

- `.pub` margin guides are not read; imported pages use 36 pt margins.
- Blank `.pub` pages are dropped on import (they can't be told apart from placeholder pages).
- Publisher 98 text is read as Windows-1252; non-Western code pages import garbled.
- (M32, ADR 0026) `.pub` Windows metafile (WMF/EMF) pictures are not drawn: they leave an empty picture frame and a `picture-wmf`/`picture-emf` report entry. The corpus WMFs are vector only, so a WMF renderer (or a bitmap extractor for files that embed one) is open.
- `.pub` pictures in Publisher 98 and 2000 files are not imported (no corpus sample; the picture shape markers are unknown). Picture crop properties are not read; pictures stretch to the frame.
- `.pub` shapes of Escher types other than rectangle, rounded rectangle, ellipse and line (the corpus has types 6 and 136) are reported as `shape` and not drawn; rounded-rectangle `adjust` values, dashes, arrowheads, gradient/pattern/picture fills and shadows are not read.
- `.pub` tables: only plain row-by-column grids with one paragraph per cell import (merged cells, cell borders and fills are not read); other tables keep their text as a text frame and are reported as `table`.
- `.pub` masters: the master reference is read for 2002+ files only, and no corpus master has a shape, so a master with frames is covered by the shared frame pipeline but by no corpus file.
- `.pub` colour scheme slots (palette entries 0 to 6 = Main, Accent 1 to 4, Hyperlink, Followed Hyperlink; 7 = Background) are inferred from three schemes.
- macOS has no Apple-event handler for opened files: `.pubrs` and `.pub` files arrive only as command-line paths (File, Import Publisher File… and drag and drop work).
- The macOS `.pub` document type and the Windows "Open with" registration for `.pub` are not built or run in this environment; CI and the release workflow are the first to build them.
- Linked-chain flow order in 98/2000 `.pub` files is page order; prev/next ids are read only for 2002+.
- The `.pub` oracle (libmspub) counts a chained story once per frame; recall is measured that way (ADR 0017).
- Colour (M21, resolved in M22): the PDF now writes CMYK as DeviceCMYK and spot colours as Separation.
- Colour (M21): `validate` does not warn about a reference to a missing swatch (it draws magenta); the design checker (M29) should.
- Colour (M21): the gradient editor in the GUI edits the angle and the first and last stop colours only (custom colours, no swatches or extra stops in the dialog); the API takes up to 16 stops with swatches.
- Colour (M21): the GPU gradient path (vello) is compile-checked only; CI has no GPU adapter.
- Colour (M21): the swatch tools take a swatch id or name, but `{swatch: ...}` paints in other tools need the id.

## Commercial print output (M22)

- Colour is quantised to 8 bits per component (krilla); a value read back can differ from the swatch by up to 0.2 percent (ADR 0018).
- A gradient that mixes RGB with CMYK or spot stops is written in RGB; stops of one spot stay Separation, CMYK and spot mixes become DeviceCMYK.
- PDF/X-4 and PDF/A-2b write CMYK as ICCBased against the output profile (same numbers as DeviceCMYK); the plain export writes DeviceCMYK.
- PDF/X-4 is checked by `pdfx::check` only (veraPDF has no PDF/X profile); trapping, overprint and total ink coverage are not checked.
- The OutputCondition of a custom `output_intent_icc` is the fixed text "Custom CMYK output profile" (the profile's description is not read).
- The Export dialog has no field for a custom output profile; use `export_pdf` with `output_intent_icc`.
- `validate` does not yet warn about RGB colours in a PDF/X job (`rgb_in_cmyk_job`) or pictures without alt text (`missing_alt_text`); the design checker (M29) should.
- Tagged PDF has no heading or list structure: text is `P` per frame, pictures are `Figure`, everything else is an artifact.

## Spelling (M28)

- Only American English (en_US, SCOWL size 60) is built in. Other languages, and British or other English variants, need a user Hunspell `.dic`/`.aff` pair in the dictionaries folder; a paragraph whose language has none is skipped (`unchecked_languages`).
- `pubrs-mcp` passes no dictionaries folder (its `main.rs` was outside M28's files): over MCP, added words last for the session and user dictionaries are not loaded. The app and the `pubrs` CLI use `<config dir>/pubrs/dictionaries`.
- `spellbook` 0.4 is alpha: no phonetic suggestions, and dictionaries with complex compounding rules (German, Hungarian, Finnish) may check less well than in Hunspell.
- The word being typed is flagged at once (no wait until the word is finished), and there is no setting to turn checking as you type off.
- Squiggles are drawn for text frames on the page shown, not for text on its master page.
- Ignore accepts the word everywhere for the session (there is no "ignore once"; Next in the Spelling window skips one occurrence).
- WordArt text (M25) is not spell checked: the spell worker's reports, squiggles and Change are all keyed to stories and a warped line has nowhere to draw a squiggle; table cells (M19) will be, because they are stories.

## WordArt, shadows and BorderArt (M25)

- BorderArt pattern colours are fixed in the SVGs; there is no colour option and no way to add a user pattern.
- The WordArt text is edited in the properties panel and the gallery, not in place on the canvas, and WordArt has no per-letter formatting (one paragraph warped as a whole, Owner decision 8).
- Shadow offsets are rounded to whole device pixels and the blur is three box passes (a Gaussian approximation). In PDF a shadow is a 150 dpi bitmap (ADR 0020), so even a hard shadow has a 150 dpi edge.
- The GPU backend (`gpu` feature) has no blur: `Rasterizer` draws pages with a shadow with tiny-skia. Pages with a shadowed frame also skip the parallel tile path in the GUI's viewport (the untiled render is used, so output stays exact).
- WordArt warps are fixed shapes (no adjustable handles), the text is scaled down but never up to fit the frame, and `circle` closes at 97% of a turn.
- The Format menu holds Shadow and Border Art… only; the plan's Format menu (Paragraph…, Tabs…, Bullets and Numbering…, Drop Cap…, Character…) lives in the Text menu for now.

## Text wrap (M18)

- Wrap (ADR 0022): slots in one row share a top, not a forced baseline (mixed sizes in a row differ); a word wider than its slot overflows it; widow fixes look at the first slot of a row; frames inside groups wrap by their bounding box. WordArt frames (M25) wrap by their frame rectangle in every mode (their warped path is not an obstacle outline yet).

## Tables (M19)

- Splitting a cell only undoes a merge; there is no "split into N rows and M columns" that adds grid lines. Insert rows and columns instead.
- Table text is clipped to whole lines at the frame's bottom; a table never grows its frame, and rows below the bottom have no height.
- Tagged PDF has no Table, Row or Cell structure elements (cells are artifacts and text spans).
- Up/Down inside a cell use the cell's own lines; the arrow keys never leave a cell (Tab and Shift+Tab move between cells).
- Wrap obstacles do not affect cell text (a cell never wraps around a frame above it); a table with wrap set is an obstacle by its rectangle.
- A cell's fill is a solid colour or swatch (no gradients).

## Tables in the GUI (M20)

- A selected table moves only with Alt+drag (a plain drag in a selected table selects text or cells); there is no move handle on the table's border.
- A column or row border drag shows a guide line and applies on release; the table does not reflow live.
- Text menu, Format and character shortcuts act on the cell being edited only; with cells selected (or only the table) they are unavailable. Delete and typing do nothing on a cell range, and copy, cut and paste of cell ranges are not done.
- The frame grows with its rows (typing, row inserts, row heights) but never shrinks; shrinking rows leaves the frame as tall as it was, and resizing the frame is how to trim it.
- Insert Row/Column insert one line at a time (the count of selected rows is not used); no "distribute columns evenly".
- Shift+F10 opens the spelling menu in a cell without the table items; only a right-click shows them.
- The frame grows with its rows only for edits the GUI makes (typing, replace, character and paragraph formats, row, column and cell-style commands). Changes through the MCP tools do not grow the frame; `table_set_row_height` etc. leave the frame as it was.

## Wrap (M18 review)

- A drop cap beside a multi-slot (both-sides/through) row can be overlapped by the second slot.
- Up/Down move by story line, not by visual row, in multi-slot rows.
- Clipboard (M27, ADR 0025): frames go on the OS clipboard as HTML with the pubrs payload in a comment (plus a PNG and the text alternative) because arboard replaces the clipboard on every set; a native multi-format offer per OS (pubrs flavour, PNG and text together) is not done.
- Clipboard (M27): RTF is not read (arboard has no RTF format); only HTML, text and images are.
- Clipboard (M27): Ctrl/Cmd+V with only an image on the clipboard does nothing, on Windows and Linux, because egui-winit 0.36.2 swallows the paste shortcut (it reads clipboard text, sends `Event::Paste` only if non-empty, and returns without a key event), so the app cannot see the keypress at all; Edit > Paste works, and on macOS the native menu's Cmd+V does too. Needs a key hook in the window layer (own winit event handling or an egui-winit change).
- Clipboard (M27): a pasted picture is always a PNG, and large clipboard payloads (HTML over 48 MB) are kept only inside the running app.
- IME (M27): the preedit is drawn with the UI font, which lacks CJK glyphs unless the system provides them to egui, and not with the document's font and size; IME events cannot be verified end to end here (no input method in the test environment), only the egui events are injected.
- Right-to-left (M27): tab stops in right-to-left paragraphs are measured from the left edge, not the right; Left/Right with a selection collapse it by text order, not by position; where two offsets share one place (Hebrew word edges in English text) an arrow walk reaches only one of them in each direction.
- Clipboard (M27): `paste_image` encodes, scales and places a clipboard picture on the UI thread (up to 8,000 px a side, so a visible pause for a big picture); it belongs on a worker.
- Clipboard (M27): copying renders the whole page at 96 dpi to crop a preview of the selection; a small selection on a big page, or a very long copy, pays for the full page.
- Clipboard (M27): a pasted group keeps its name, style and wrap but not its rotation (setting a group's rotation rotates its members again).
- Design Checker (M29): `missing_alt_text` covers pictures only; WordArt and shapes have no alt-text field (and tagged PDF writes them as artifacts).
- Design Checker (M29): frames stacked beneath a text frame are judged one level deep by their own fill (the topmost one holding the text frame's centre); text over a picture, a gradient or several partly covering fills is not judged exactly, and rotated frames use their bounds.
- Design Checker (M29): `object_in_unprintable_area` assumes 0.25 in on every side and skips label sheets; real printer margins (and each label's own) are not modelled. The built-in newsletter's full-width masthead band is flagged for a desktop job, by design.
- Design Checker (M29): the model-level checks run on the UI thread after the debounce (they need the session's layout); a refresh of every page after a style or swatch change is unmeasured on very large documents.
- Design Checker (M29): the pre-export check covers missing fonts, RGB colour and low-resolution pictures only; ink coverage, overprint and trapping are not checked.
- Clipboard (M29): a picture over 200 megapixels is refused before it is copied, but arboard has already decoded it by then.
- Tab leaders (M30 menu template): the leader fill can start flush against the text (dots sit on a fixed grid), so the menu template appends a space before each tab; the layout should leave a minimum gap after the text itself.

## Mail merge (M24)

- Catalog merge and e-mail merge are out of scope.
- A field's value is one line: line breaks and tabs in a cell become spaces, so a multi-line address needs one field per line.
- Copying text that contains a field through the clipboard gives plain text: the pasted U+FFFC is an ordinary character (a missing-glyph box) until the field is inserted again. Duplicating a frame or page keeps its fields.
- Fields on a master page merge in a PDF (each record is previewed) but stay «Name» in a merged publication (masters are shared).
- A booklet is imposed over the whole merged run; records are not padded to a multiple of 4 each. Merge to PDF holds every page scene in memory until the file is written (100,000 records is large).
- XLSX: only `.xlsx` and `.xlsm` (by extension), one sheet at a time, cached formula results as displayed; no `.xls`/`.ods`. CSV must be UTF-8.
- The wizard is a modal dialog: the text cursor has to be placed before it is opened (the Fields step says so), and Print sends the merged PDF to the default destination without a per-job dialog beyond what the platform shows.
- The preview index and switch are undoable commands (the GUI coalesces a walk through the records into one step); `Refresh data` only works for a list loaded in the same session (the path is not stored).
- The merge fields tint is drawn by the app over the page image, so it is not in exported PNG or PDF (intended); there is no setting to hide it.
- Merge limits (ADR 0023): 64 MiB per file, 100,000 records, 512 fields, 5,000,000 cells and 20,000 output pages; larger lists must be split. A workbook cell beyond column 512 or row 100,001 below the header is an error, not ignored.
- Mail merge (M24): a text line that holds a merge field has no per-character boxes for right-to-left hit testing (`caret_positions` returns no `CharSpan`s for it); carets themselves are right. Fields in table cells preview and merge (cell stories are ordinary stories), but the GUI's row growth runs only for edits, not for preview steps.

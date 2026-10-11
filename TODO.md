# TODO

Deferred and partial work. Each entry says what is missing and why.

## Later phases (designed for, not built)

- Mail merge from CSV.
- `.pub` import beyond M31 part 1 (reference: libmspub).
- CMYK / spot colours, PDF/X output.
- Tables.
- WordArt-style text effects.
- Avery label sheet presets.

## Partial features

- Character formatting caches each face's OS/2 and `smcp` data per (family, bold, italic) in the `Layouter` and never clears it; once fonts can be added at runtime, clear it (or key it on a font-library generation) so a newly added family replaces the fallback face's data.
- Sheet presets Avery 5376, 5377, 22805, 5305, 8315, C32011, L7651 and the L7161/L7162 margins are from memory of published specs; check them against vendor datasheets.
- Imposed sheets carry no bleed, and `crop_marks` marks the whole sheet rather than each label.
- `SetPageSize` does not re-check an existing label sheet; a mismatch is reported at PDF export.
- The GPU renderer ignores `Node.clip` (only imposed sheets use it, and those are never drawn on the GPU).
- Text wrap: `square` and `top_bottom` only, around bounding boxes. Contour ("tight") wrap and text on both sides of an obstacle are not implemented (ADR 0006).
- The `validate` tool still clones the whole document on every call (`crates/pubrs-api/src/tools/query.rs`); the design checker (M29) replaces it. (The canvas page snapshot now shares frames and stories by `Arc` and is built by `Session::page_snapshot`, M13.)
- vello is no longer used by the GUI (ADR 0013), but `crates/pubrs-app/Cargo.toml` still enables `pubrs-api/gpu`, so the app compiles and links it. Dropping the feature from the app was outside M13's files.
- Right-to-left scripts: shaping works through parley, but caret/hit-testing assumes left-to-right lines.
- GUI: copy/paste of frames works within one publication only (no cross-document or OS-clipboard frame paste).
- GUI: text editing has no IME composition support (direct key/text input only).
- Character formatting (M17, ADR 0021): real `smcp` is detected from the GSUB feature list only (not per-letter coverage); a strikethrough through synthesized small caps is stepped; the Character dialog has no preview and no letter-spacing or colour fields (the text panel has them); the super/subscript buttons are in the text panel, not a toolbar.
- Windows and macOS printing are compile-checked here; they need the manual check in RELEASE.md (no printers on CI).
- macOS: double-clicking a `.pubrs` file in Finder launches pubrs but does not open the file, because winit does not deliver `application:openURLs:` events. Workaround: File → Open, or drag the file onto the pubrs window (ADR 0010).
- Installers are unsigned unless the signing secrets in RELEASE.md are set; macOS notarization is a manual step.
- Flatpak: manifest and AppStream metadata only. It is not built in CI, and Flathub submission needs generated `cargo-sources.json`.
- The Windows ARM64 MSI is built but not install-checked in CI (no ARM64 Windows runner in the release workflow).
- The Windows ARM64 MSI has no bundled Mesa (mesa-dist-win publishes x64 and x86 only), so ARM64 Windows without a GPU driver still draws with WARP: about 0.5 CPU-seconds per frame (ADR 0011).
- Bundled Mesa is pinned to mesa-dist-win 26.2.4. Updating it means a new version and SHA-256 in `packaging/windows/fetch-mesa.ps1` and `packaging/windows/mesa/THIRD-PARTY-NOTICES.txt`.
- Find and replace does not search WordArt text yet (M25 must add it in `crates/pubrs-api/src/tools/find.rs`); table cells are covered because they are stories.
- Guides on master pages are drawn on the pages using them but can only be moved through `guide_move` (there is no master-editing view in the GUI).

## Hyphenation (M14)

- Hyphenation languages left out (licence unclear or not allowed): Afrikaans, Danish, Greek, Estonian, Croatian, Icelandic, Italian, Georgian, Kurmanji, Latin, Mongolian, Dutch, Russian, Sanskrit, Serbian (Cyrillic), Slovenian, Swedish, Turkish and Ukrainian (LPPL-only or LPPL with MIT alternatives not yet checked); Slovak (GPL). Possible additions once checked: Belarusian, Bengali, Assamese, Gujarati, Hindi, Kannada, Malayalam, Marathi, Odia, Punjabi, Tamil, Telugu, Turkmen, Albanian, Czech and Finnish (headers say MIT, or MIT alongside GPL/LGPL).
- English US pattern licence (Kuiken's notice-preserving grant) is outside the LD5 list; the owner must confirm (ASSETS.md).
- The built-in styles leave `justify_limits` unset (v0.1 behaviour, LD2); the M30 templates should enable limits in their body styles.
- Letter-spaced justification spaces characters, not grapheme clusters, and uses the mean font size of the line for its cap.
- The Paragraph dialog is under Text → Paragraph… (Ctrl/Cmd+Alt+P); there is no Format menu.

## Paragraph controls and breaks (M15)

- Autoflow (adding pages and linked frames for overflowing text) is not implemented; a page break at the end of a chain overflows.
- The built-in styles leave `widow_orphan` unset (v0.1 layout, LD2); the M30 templates should enable `widow_orphan = 2` in their body styles.
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
- Masters, pictures, shapes, tables and colour schemes in `.pub` files are dropped and reported until M32.
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
- WordArt text (M25) is not checked yet (it must be added where `check_document` walks stories); table cells (M19) will be, because they are stories.

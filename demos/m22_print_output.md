# M22: commercial print output (CMYK, spot, PDF/X-4, PDF/A-2b)

What it does: the PDF now carries your CMYK and spot colours exactly (DeviceCMYK, and a separate plate for each spot colour), and can be written as PDF/X-4 for a print shop or PDF/A-2b for archiving. Pictures can carry alt text, and one click saves everything a printer needs.

## In the app
1. **File > Export PDF...** opens the Export dialog. Pick a preset: **Screen**, **Desktop printing**, **Commercial printing (PDF/X-4)** or **Archive (PDF/A-2b)**. Bleed, crop marks and tagged PDF are options under the presets; an option that does not apply says why when you hover over it (for example, bleed needs a bleed set in Page Setup). Press **Export...**, choose a file. Escape closes the dialog; Tab moves between the controls.
2. **Commercial printing** writes a PDF 1.6 file with a CMYK output intent, TrimBox and BleedBox on every page, and checks the result before saving. If the check fails you get an error that names what is wrong, and no file.
3. **Save for a commercial printer...** (in the dialog and in the File menu) saves a zip with the PDF/X-4 file, your `.pubrs` with its pictures, and a README listing the fonts used and every swatch with its CMYK or spot values.
4. Select a picture: the right-hand panel has an **Alt text** box. With tagged PDF on (the Screen preset turns it on), the picture becomes a figure with that description.

## From a script or an AI assistant
```json
{"tool": "image_set_alt_text", "args": {"frame_id": "...", "alt_text": "Two loaves of sourdough on a board"}}
{"tool": "export_pdf", "args": {"path": "flyer-x4.pdf", "standard": "pdfx4", "crop_marks": true}}
{"tool": "export_pdf", "args": {"path": "flyer-archive.pdf", "standard": "pdfa2b"}}
{"tool": "export_pdf", "args": {"path": "flyer-tagged.pdf", "tagged": true}}
{"tool": "export_print_package", "args": {"path": "flyer-print.zip"}}
```
`output_intent_icc` takes the path of your printer's CMYK output profile for `pdfx4`; anything that is not a CMYK output profile is refused with a hint. Without it the bundled generic CMYK (SWOP-like) profile is used.

## Checking it
- `cargo test -p pubrs-io --test pdf_export`: reads the PDF back with lopdf and asserts DeviceCMYK operators with tints applied, `/Separation /PANTONE 186 C`, the OutputIntent, the profile bytes, `GTS_PDFXVersion`, XMP, boxes, `/Alt` and the print package.
- `cargo test -p pubrs-io --test fixtures_pdf`: `pdfx::check` finds nothing in the PDF/X-4 export of the newsletter, a label sheet, a booklet and the colour document, and writes `target/pdf-standards/{a2b,x4}.pdf`. CI runs veraPDF on `a2b.pdf` (Linux).
- The GUI self-test opens the Export dialog, chooses Commercial printing, exports and checks the file for `GTS_PDFX`.

Known limits: colour is stored with 8 bits per component; a gradient that mixes RGB with CMYK or spot stops is written in RGB; PDF/X-4 does not check trapping or ink coverage; PDF/X and PDF/A files use ICC-based CMYK (same numbers) rather than DeviceCMYK. See `docs/adr/0018-pdfx.md`.

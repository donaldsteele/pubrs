# M4: PDF export

What works:

- krilla PDF writer: one PDF page per document page; MediaBox/CropBox = trim + slug, BleedBox = trim + bleed,
  TrimBox = trim. Artwork is clipped to the bleed area.
- Bleed on/off, crop marks in registration colour outside the bleed, font embedding as subsets (default) or
  text converted to outlines, title/author metadata, deterministic output.
- Pre-shaped glyphs from the layout engine with ToUnicode text mapping; synthetic bold (fill + stroke) and
  italic (shear); JPEG pass-through (DCTDecode), PNG/RGBA otherwise; SVG via krilla-svg (vector, with text).
- Tests parse the PDF back with lopdf (page count, every box, fonts embedded + subset, text extraction, JPEG
  filter) and rasterise it with hayro to compare against the PNG renderer.

See it:

```
cargo test -p pubrs-io --test pdf_export
cargo run -p pubrs-io --example export_sample -- out/   # writes out/sample.pdf with bleed and crop marks
```

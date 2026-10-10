# M2: text layout

What works:

- `FontLibrary`: bundled Liberation Sans/Serif (OFL), optional lazy system fonts (fontdb), deterministic test sets from a directory.
- `Layouter`: per-paragraph parley shaping, styles resolved from the style sheet, bold/italic faces or synthesis,
  underline, letter spacing, alignment (left/center/right/justify), indents, space before/after, line spacing.
- Flow through linked frame chains and columns with per-line widths; overflow reported as a character offset and hidden count.
- Text wrap around frames above (square / top-bottom), vertical alignment (top/middle/bottom).
- Caret positions per line for hit testing; glyph → source text ranges for PDF text extraction.
- Golden layout snapshot (`tests/golden/layout/reference.json`) using only `tests/fonts/`.

See it:

```
cargo test -p pubrs-layout
PUBRS_BLESS=1 cargo test -p pubrs-layout --test golden   # regenerate the golden after an intended change
```

# TODO

Deferred and partial work. Each entry says what is missing and why.

## Later phases (designed for, not built)

- Mail merge from CSV.
- `.pub` import (reference: libmspub).
- CMYK / spot colours, PDF/X output.
- Tables.
- WordArt-style text effects.
- Avery label sheet presets.

## Not yet implemented (scheduled)


## Partial features

- Text wrap: `square` and `top_bottom` only, around bounding boxes. Contour ("tight") wrap and text on both sides of an obstacle are not implemented (ADR 0006).
- Right-to-left scripts: shaping works through parley, but caret/hit-testing assumes left-to-right lines.
- Characters no bundled or installed font covers render as .notdef boxes; `validate` does not yet name them (layout counts them in `missing_glyphs`).
- AppImage: `--mcp` dispatch to the bundled `pubrs-mcp` is promised in examples/mcp/README.md (M8 packaging).

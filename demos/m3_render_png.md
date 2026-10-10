# M3: rendering to PNG

What works:

- Display list per page (`pubrs-render::scene`): master frames beneath page frames, fills, strokes
  (solid/dashed/dotted), rounded rectangles, ellipses, lines, rotation, frame opacity, images (fill/fit/stretch,
  crop, SVG with bundled-font text), glyph runs with synthetic bold/italic, underlines.
- tiny-skia rasteriser (deterministic) with glyph outline cache; PNG export at any dpi, with or without bleed.
- vello GPU rasteriser (feature `gpu`) with automatic fallback to tiny-skia; logged active path.
- Golden PNGs under `tests/golden/render/` compared with a tolerance; GPU vs CPU comparison test.

See it:

```
cargo test -p pubrs-io --test render_golden
cargo test -p pubrs-render --features gpu --test gpu -- --nocapture
ls tests/golden/render/
```

# M18: Tight, through and both-sides wrap

What works:

- **Tight wrap.** `frame_set_wrap` with `mode: "tight"` makes text follow the real outline of the frame above it:
  ellipses, rounded rectangles, lines (their stroke), rotated frames and placed PNGs with transparency (the visible
  pixels, alpha 50% or more). On each line, the outer hull of the shape is blocked, so text hugs a circle's curve
  instead of its box. Pictures without transparency, JPEG, SVG and pictures not yet decoded wrap by their rectangle.
- **Through wrap.** `mode: "through"` blocks only the covered parts of each line, so text runs into notches and holes of
  the outline (the inside of an arch or a C shape) and beside it.
- **Text on both sides.** `both_sides: true` fills every gap beside the frame that is wider than three em, left to
  right, in story order: the left slot continues into the right slot. Without it, text uses the widest gap as before.
  `square` with `both_sides` works the same way around the bounding box.
- **Margin.** The clearance in points is kept around the outline, not the box.
- **Unchanged.** `square` and `top_bottom` without `both_sides` lay out exactly as before; no golden changed.
- **MCP.** `frame_set_wrap` takes `tight`, `through`, `both_sides` and `margin`; two new examples.
- **GUI.** Select a shape or picture: Text wrap offers None, Square, Tight, Through and Top and bottom, with a Margin
  field and a "Text on both sides" checkbox. Everything is reachable with Tab, Space and the arrow keys.
- **Schema 9.** `v8_to_v9` stamps the version; `tests/fixtures/schema_v9.pubrs` holds the new modes.

Golden: `tight_wrap` (an ellipse and a disc-shaped PNG with text on both sides, an arch-shaped PNG with through wrap).
Run `cargo test -p pubrs-layout --test wrap` and `cargo test -p pubrs-io --test render_golden`.

Limits (ADR 0022, TODO.md): a word wider than its slot overflows it; slots of one row share a top, not a forced
baseline; frames inside groups wrap by their bounding box.

Try it:

```
claude mcp add pubrs -- pubrs-mcp
# "Put an ellipse in the middle of the page, make the text wrap tight around it on both sides, margin 8."
```

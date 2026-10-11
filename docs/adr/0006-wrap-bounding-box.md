# 0006 Text wrap uses bounding boxes

- Context: Publisher offers square, tight and through wrap; tight wrap needs shape contours and per-line multi-segment flow.
- Decision: `square` wraps around the obstacle's rotated bounding box (plus margin) on the wider free side; `top_bottom` skips the band; only frames above the text frame in z-order on the same page count.
- Consequence: one text segment per line keeps flow simple and fast; contour ("tight") wrap is listed in TODO.md.

## Addendum (M18, ADR 0022)

- `tight` and `through` wrap now exist, following the true outline of the obstacle, and `both_sides` lets a line take several slots. `square` and `top_bottom` are as decided above: `square` still means the rotated bounding box on the wider free side, `top_bottom` still skips the band, and a frame with the new fields unset lays out exactly as before.
- The "one text segment per line" consequence no longer holds for `both_sides` and `through`; ADR 0022 describes the flow, its cost and its limits. The TODO.md entry for contour wrap is closed.

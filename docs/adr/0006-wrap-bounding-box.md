# 0006 Text wrap uses bounding boxes

- Context: Publisher offers square, tight and through wrap; tight wrap needs shape contours and per-line multi-segment flow.
- Decision: `square` wraps around the obstacle's rotated bounding box (plus margin) on the wider free side; `top_bottom` skips the band; only frames above the text frame in z-order on the same page count.
- Consequence: one text segment per line keeps flow simple and fast; contour ("tight") wrap is listed in TODO.md.

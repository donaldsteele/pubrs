# The `.pubrs` file format

Status: version 1 (M1). The model is defined by the Rust types in
`crates/pubrs-core/src/model.rs`; a JSON Schema generated from them is
published with the MCP docs.

## Container

A `.pubrs` file is a ZIP archive:

```
mimetype              "application/vnd.pubrs+zip" (stored, first entry)
document.json         the document (UTF-8 JSON)
assets/<asset-id>.<ext>   embedded images (png, jpg, svg)
thumbnail.png         optional first-page preview
```

Entry names always use `/`. Readers ignore unknown entries.

## `document.json`

Top-level object:

| Field | Type | Notes |
|-------|------|-------|
| `schema_version` | integer | Currently `1`. Older versions are migrated on load. Newer versions are rejected. |
| `id` | uuid | Document id. |
| `meta` | object | `title`, `author`, optional `template`. |
| `setup` | object | `size {width,height}`, `margins {top,right,bottom,left}`, `bleed` (all points). |
| `grid` | object | `spacing`, `show_grid`, `snap_to_grid`, `snap_to_guides`, `snap_tolerance`. |
| `pages` | array | Pages in order: `id`, optional `master`, `frames` (ids, back to front), `guides`. |
| `masters` | array | `id`, `name`, `frames`, `guides`. |
| `frames` | object | Frame id → frame. |
| `stories` | object | Story id → story. |
| `styles` | object | `default_paragraph` (style id) and `styles` (array). |
| `assets` | object | Asset id → `{id, name, format, pixel_width, pixel_height}`. |

Units are points (1/72 inch). Coordinates have their origin at the top-left
of the page trim box, x to the right, y down. Colours are `#RRGGBB` or
`#RRGGBBAA` strings. Ids are UUID strings.

### Frame

```json
{
  "id": "…",
  "rect": {"x": 36, "y": 36, "width": 200, "height": 100},
  "rotation": 0,
  "locked": false,
  "style": {"fill": "#FFFFFF", "stroke": {"color": "#000000", "width": 1, "dash": "solid"}, "opacity": 1},
  "wrap": {"mode": "none", "margin": 0},
  "placeholder": {"name": "headline", "kind": "text", "hint": "Your headline"},
  "content": {"type": "text", "story": "…", "insets": {…}, "columns": 1, "column_gap": 12, "vertical_align": "top"}
}
```

`content.type` is one of:

- `text`: `story`, `insets`, `columns`, `column_gap`, `vertical_align` (`top|middle|bottom`).
- `image`: optional `asset`, `fit` (`fill|fit|stretch`), `crop` (fractions trimmed from `left/top/right/bottom`), `lock_aspect`.
- `shape`: `shape` = `{"kind":"rectangle","corner_radius":0}`, `{"kind":"ellipse"}` or `{"kind":"line","direction":"down_right|up_right"}`.
- `group`: `children` (ids, back to front). Children keep absolute geometry.

### Story

```json
{
  "id": "…",
  "frames": ["frame-a", "frame-b"],
  "paragraphs": [
    {"style": "…", "align": "center", "runs": [{"text": "Hello ", "bold": true}, {"text": "world"}]}
  ]
}
```

`frames` is the linked chain in flow order. Paragraph objects may carry
overrides `align`, `line_spacing`, `space_before`, `space_after`,
`first_line_indent`, `left_indent`, `right_indent`. Run objects may carry
`style` (character style id) and overrides `font_family`, `font_size`, `bold`,
`italic`, `underline`, `color`, `letter_spacing`.

### Style

```json
{"id": "…", "name": "Heading 1", "kind": "paragraph", "based_on": "…",
 "paragraph": {"space_before": 12}, "character": {"font_size": 20, "bold": true}}
```

Formatting resolves as: built-in defaults → default paragraph style chain →
paragraph style chain → character style chain → run overrides.

## Versioning

`schema_version` increases whenever the JSON shape changes incompatibly.
`pubrs-io` contains a migration function from every past version to the
next; loading applies them in sequence. Writers always write the current
version.

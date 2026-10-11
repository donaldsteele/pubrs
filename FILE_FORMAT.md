# The `.pubrs` file format

Status: version 3 (M14). The model is defined by the Rust types in
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
| `schema_version` | integer | Currently `3`. Older versions are migrated on load. Newer versions are rejected. |
| `id` | uuid | Document id. |
| `meta` | object | `title`, `author`, optional `template`. |
| `setup` | object | `size {width,height}`, `margins {top,right,bottom,left}`, `bleed` (all points); optional `sheet` (see below) and `booklet` (bool, omitted when false). |
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
`first_line_indent`, `left_indent`, `right_indent`, `hyphenate` (bool, default
false), `language` (BCP 47, default `en-US`) and `justify_limits`
(`{"max_word_space": 1.33, "letter_spacing": 0.05}`, both optional; see below).
Run objects may carry
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

### Version history

| Version | Change |
| --- | --- |
| 0 | Unversioned or hand-written JSON; `schema_version`, `id`, `meta` and `grid` may be missing. |
| 1 | The first released shape. |
| 2 | `setup.sheet` and `setup.booklet` (labels, cards, booklets). Both are optional, so a version 1 file is a valid version 2 file; migration only stamps the version. |
| 3 | Paragraph fields `hyphenate`, `language` and `justify_limits` (on paragraphs and paragraph styles). All are optional, so version 1 and 2 files are valid version 3 files; migration only stamps the version. |

### `setup.sheet` and `setup.booklet` (version 2)

A document page is one label or card. When `setup.sheet` is present, PDF
export and printing put the pages N-up on the sheet; `setup.booklet: true`
imposes a saddle-stitched booklet instead. The two are mutually exclusive.
Neither changes how pages are laid out, edited or saved, and per-page PNG
export stays per page.

`setup.sheet` fields, all points unless noted: `preset` (an id such as
`avery-5160`, or `custom`), `sheet_size {width,height}`, `rows`, `cols`,
`label_size {width,height}` (must equal `setup.size` within 0.5 pt),
`origin {x,y}` (top-left corner of the first label), `pitch {width,height}`
(corner-to-corner distance between neighbouring labels) and `fill`
(`repeat`: the page in every cell of its own sheet; `sequence`: one page per
cell in reading order).

Booklets pad the page count to a multiple of four with blank pages, then
reorder for saddle stitch, 2-up on a sheet twice the page width: for 8 pages
sheet 1 front is [8, 1], back [2, 7]; sheet 2 front [6, 3], back [4, 5]. Each
side is one PDF page.

### Hyphenation and justify limits (version 3)

`hyphenate: true` lets the line breaker break words at hyphenation points when
a line would otherwise be loose; the line then ends with the font's hyphen
glyph. The story text never contains the hyphen (offsets stay in characters).
`language` picks the patterns (`en`, `de`, `es`, `fr`, `pl`, `pt`; the region
is ignored); another language gets no hyphens and a `hyphenation_unavailable`
note from `validate`. A soft hyphen (U+00AD) typed in the text is always a
break opportunity.

`justify_limits` is absent in every older file, and **absent means
unbounded justification, as before** (Locked Decision 2). When present,
`max_word_space` (default 1.33) caps a word space as a multiple of a normal
space and `letter_spacing` (default 0.05) caps letter spacing as a fraction of
the font size; word spaces stretch first, then letters, and a line that would
need more is left short. `{}` means both defaults. `"unbounded": true` (inside
`justify_limits`) means no limits and overrides limits inherited from a
paragraph style. The last line of a
paragraph is never stretched.

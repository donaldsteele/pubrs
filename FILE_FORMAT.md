# The `.pubrs` file format

Status: version 4 (M21). The model is defined by the Rust types in
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
| `schema_version` | integer | Currently `7`. Older versions are migrated on load. Newer versions are rejected. |
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
| `swatches` | array | Optional (omitted when empty). Named colours: `{id, name, value}` (version 4). |
| `scheme` | string | Optional. Id of the colour scheme last applied (version 4). |

Units are points (1/72 inch). Coordinates have their origin at the top-left
of the page trim box, x to the right, y down. Colours are `#RRGGBB` or
`#RRGGBBAA` strings, or a paint (see Paint below) wherever a fill, outline or
text colour is stored. Ids are UUID strings.

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
- `image`: optional `asset`, `fit` (`fill|fit|stretch`), `crop` (fractions trimmed from `left/top/right/bottom`), `lock_aspect`, `alt_text` (what the picture shows; empty or absent for none).
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
false), `language` (BCP 47, default `en-US`), `justify_limits`
(`{"max_word_space": 1.33, "letter_spacing": 0.05}`, both optional; see below),
`keep_with_next`, `keep_together` (bools, default false), `widow_orphan`
(lines, default off) and `drop_cap` (`{"lines": 3, "chars": 1}`; see below).
Run text may contain the column, frame and page break characters (see below).
Run objects may carry
`style` (character style id) and overrides `font_family`, `font_size`, `bold`,
`italic`, `underline`, `color`, `letter_spacing`, `baseline`, `caps`, `strikethrough`,
`ligatures`, `kerning` (the last five are described below).

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
| 4 | `swatches` and `scheme` on the document, and paints (swatch references and gradients) wherever a colour is stored. All optional, and a hex string is still a valid paint, so older files are valid version 4 files and are only stamped. |
| 5 | `alt_text` on picture frames (written to tagged PDF). Optional and omitted when empty, so older files are valid version 5 files and are only stamped. |
| 6 | Paragraph fields `keep_with_next`, `keep_together`, `widow_orphan` and `drop_cap` (on paragraphs and paragraph styles), and the break characters U+000E (column), U+001D (frame) and U+000C (page) in run text. All fields are optional and older files hold no break characters, so older files are valid version 6 files; migration only stamps the version. |
| 7 | Paragraph fields `tab_stops` and `list` (on paragraphs and paragraph styles); tab characters in run text now move to tab stops. Both fields are optional, so older files are valid version 7 files; migration only stamps the version. |
| 8 | Character fields `baseline`, `caps`, `strikethrough`, `ligatures` and `kerning` (on runs and character styles). All are optional, so older files are valid version 8 files; migration only stamps the version. |
| 9 | `type: "word_art"` frame content, and the frame style fields `shadow` and `border_art`. All are optional and omitted when unset, so older files are valid version 9 files; migration only stamps the version (ADR 0020). |
| 10 | Wrap modes `tight` and `through`, and `wrap.both_sides` on frames. All optional (`both_sides` is omitted when false), so older files are valid version 9 files; migration only stamps the version. |
| 11 | `type: "table"` frame content (`cols`, `rows`, `cells`; cell text is stories whose chain is the table frame). A new variant, so older files hold none; migration only stamps the version (ADR 0024). |

### Swatches, schemes and paints (version 4)

`swatches` entries are `{"id", "name", "value"}` where `value` is one of:

- `{"rgb": "#RRGGBB"}`
- `{"cmyk": {"c": 0-100, "m": 0-100, "y": 0-100, "k": 0-100}}`
- `{"spot": {"name": "PANTONE 186 C", "cmyk": {...}}}` (the CMYK alternative is required)

A **paint** is what `style.fill`, `style.stroke.color` and a run's `color`
store. It is one of:

- a hex string, `"#1E3A5F"` (the only form before version 4, still valid);
- `{"swatch": id, "tint": 0-100}` (`tint` defaults to 100 and is omitted at 100);
- a gradient, **frame fills only**: `{"gradient": "linear"|"radial", "stops": [{"offset": 0-1, "color": hex-or-swatch}], "angle": degrees}`.
  `angle` is clockwise from left-to-right (linear only). Stops are in increasing offset order, 2 to 16 of them.

Swatch values are resolved when drawing, so editing a swatch recolours
everything that refers to it without relaying out text. Screen colour for CMYK
is the plain formula `r = 255 (1 - c)(1 - k)` (likewise g, b); a spot colour
shows its CMYK alternative; a tint mixes towards white, `rgb' = 255 - t (255 - rgb)`.

The object forms are strict: an unknown key (for example `tint_pct` for `tint`)
is an error that names the key, and a bad hex string is an error that says what
a colour looks like. In the PDF, RGB is DeviceRGB, a CMYK swatch is DeviceCMYK
with the tint applied to each component, and a spot colour is a `/Separation`
named after the spot (ADR 0018).

`scheme` names one of twelve built-in colour schemes. Applying a scheme sets
seven swatches with fixed ids, named `Scheme: Main`, `Scheme: Accent 1` to `4`,
`Scheme: Hyperlink` and `Scheme: Followed Hyperlink`; everything that refers to them recolours.

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

### WordArt, shadows and BorderArt (version 9)

A frame whose content is `{"type": "word_art", ...}` holds one line of
decorative text, warped as a whole:

| Field | Meaning |
| --- | --- |
| `text` | The words (up to 200 characters; control characters draw as spaces). |
| `font` | Font family name. |
| `size` | Nominal font size in points. The warped text is scaled down, never up, to fit the frame, so a large size fills it. |
| `warp` | `none` (default), `arch_up`, `arch_down`, `circle`, `wave`, `slant_up`, `slant_down` or `inflate`. |
| `fill` | A paint: hex string, `{"swatch", "tint"}` or a gradient (which spans the letters' box). |
| `outline` | Optional stroke `{"color", "width", "dash"}`; the colour is a hex string or swatch. |
| `shadow` | Optional shadow of the letters (below). |

A frame's `style` may also hold:

- `shadow`: `{"offset_x", "offset_y", "blur", "color", "opacity"}`, points
  (offsets within 200, blur 0 to 50), a hex or swatch colour and an opacity
  from 0 to 1. It is cast by everything the frame draws. A WordArt frame with
  both uses its style's shadow.
- `border_art`: `{"pattern", "width"}`: one of `stars`, `hearts`, `dots`,
  `waves`, `zigzag`, `diamonds`, `leaves`, `ropes`, `checks`, `scallops`,
  `arrows` or `snowflakes`, and a width of 4 to 72 points. The border runs along
  the inside of the frame's edge; text in a text frame is inset by its width.

All three are absent in every older file, and absent means the version 8
rendering, unchanged. `tests/fixtures/schema_v9.pubrs` holds each of them.

### Keeps, drop caps and breaks (version 6)

All four paragraph fields are absent in every older file, and **absent means
the version 5 layout, unchanged** (Locked Decision 2).

- `keep_with_next: true` keeps the paragraph's last line in the same column
  or frame as the next paragraph's first line; `keep_together: true` keeps
  all its lines in one column or frame. A paragraph is moved at most one
  column or frame on, and only when that can help (ADR 0015).
- `widow_orphan` is the fewest lines of the paragraph left alone at the top
  (widows) or bottom (orphans) of a column or frame. `0` or `1` turns control
  off and overrides a style; readers also accept `true` (2 lines) and `false`
  (0). Writers write the number.
- `drop_cap: {"lines": n, "chars": k}` draws the first `k` characters (1-15,
  default 1) as one large glyph run, sized so its cap height reaches from the
  first line's cap height to the baseline of line `n` (1-10, default 3); those
  lines are indented past it. `"lines": 0` removes a drop cap set by a style.
  The characters stay in the story text.
- Breaks are single characters in run text, so offsets stay in characters:

  | Character | Break | The text after it continues in |
  | --- | --- | --- |
  | U+000E | column | the next column, or the next frame of the chain after the last column |
  | U+001D | frame | the next frame of the chain |
  | U+000C | page | the next frame of the chain that is on a later page |

  JSON writes them as `"\u000e"`, `"\u001d"` and `"\f"`. They draw nothing.
  Pages are never added: with no frame to continue in, the rest of the story
  overflows.

### Tab stops and lists (version 7)

Both fields are absent in every older file, and **absent means the version 6
layout, unchanged** (Locked Decision 2). A tab character in text now moves the
text after it to a tab stop; before version 7 it was only a glyph.

- `tab_stops: [{"pos": 200, "align": "right", "leader": "dot"}]`: `pos` is in
  points from the left edge of the column's text area; `align` is `left`
  (default), `center`, `right` or `decimal`; `leader` is `none` (default),
  `dot`, `dash` or `line`. A paragraph's list replaces its style's; `[]`
  clears stops a style sets. A tab with no stop ahead goes to the next
  multiple of 36 pt.
- `list: {"kind": "number", "format": "a.", "level": 1, "indent": 18,
  "start": 5, "bullet": "•"}`: `kind` is `bullet`, `number` or `none` (not a
  list; overrides a style); `format` is `1.` (default), `a.`, `A.`, `i.` or
  `I.`; `level` is 0-8; `indent` is how far the label hangs left of the text
  (default 18); `start` restarts numbering at that value on this paragraph;
  `bullet` is the bullet character (default U+2022). The text is at
  `max(left_indent, indent)`. Labels are generated, never stored in run text.
  Numbering counts per level through the story, continuing across paragraphs
  that are not list items and across linked frames (ADR 0019).

### Character formatting (version 8)

Five optional fields on a run's `props` and a character style's `character`.
**Absent means the version 7 layout, unchanged** (Locked Decision 2).

- `baseline`: `"normal"`, `"superscript"`, `"subscript"` or `{"shift": 3.5}`
  (points, positive raises). Super and subscript use the font's OS/2 size and
  offset, else 58% size and a 33% shift. A raised run makes its line taller.
- `caps`: `"none"`, `"small"` or `"all"`. Display only: the text is stored as
  typed. Small caps use the font's `smcp` feature, else lowercase letters are
  drawn as capitals at 70% size.
- `strikethrough`, `ligatures` (default on), `kerning` (default on): booleans.

See ADR 0021.

### Text wrap (version 10)

`wrap` on a frame is `{"mode", "margin", "both_sides"}`. `mode` is `none`,
`square`, `top_bottom`, and now `tight` and `through`; `both_sides` (default
false, omitted when false) fills every gap beside the frame that is wider than
three em instead of only the widest. **Absent or default means the version 8
layout, unchanged.**

- `tight` follows the outline of ellipses, rounded rectangles, lines, rotated
  frames and the visible pixels of a PNG with transparency (alpha of 50% or
  more), blocking the outer hull of the shape on each line.
- `through` blocks only the covered parts of each line, so text enters notches
  and holes, and uses every slot wide enough (as with `both_sides`).
- Outlines are never stored: they are computed from the shape, or traced from
  the image once per asset, when the layout is made. A picture that is not
  decoded or has no transparency wraps by its rectangle.

See ADR 0022.

### Tables (version 11)

A frame with `"type": "table"` has `cols` (column widths in points), `rows`
(`[{"min_height"}]`) and `cells`. Each cell is `{"row", "col", "row_span",
"col_span", "story", "fill", "borders", "insets", "valign"}`: `row` and `col`
are the top-left grid position, the spans default to 1, `borders` is
`[top, right, bottom, left]` with each side a stroke or `null`, and `story` is
the id of an ordinary story in `stories` whose `frames` is `[the table frame]`.
Every grid position is covered by exactly one cell, and cells are listed in
row-major order of their top-left position. Row heights as laid out are never
stored: a row is as tall as its tallest cell, at least `min_height`, and rows
stop at the frame's bottom.

See ADR 0024.

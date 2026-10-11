# pubrs MCP server

`pubrs-mcp` lets an AI agent create, edit, inspect, render and export print
documents (flyers, brochures, business cards, newsletters) with no GUI. It is
built on the official Rust MCP SDK (`rmcp`) and exposes the same operations
as the pubrs GUI and the `pubrs script` command, one tool per operation.

> This file is generated: tool sections come from the JSON Schemas of the
> Rust types, and every example below was executed by the integration test
> `crates/pubrs-mcp/tests/examples.rs` (`PUBRS_WRITE_DOCS=1 cargo test -p
> pubrs-mcp --test examples` regenerates it). Hand-written parts live in
> `docs/mcp-intro.md`; examples live in `docs/mcp-examples.json`.

## Running

```
pubrs-mcp                         # stdio (default) — for Claude Desktop / Claude Code
pubrs-mcp --http 127.0.0.1:8765   # streamable HTTP at http://127.0.0.1:8765/mcp
```

Options:

| Flag | Meaning |
|---|---|
| `--output-dir DIR` | Folder for files saved or exported with a relative path, and with no path at all (default: `Documents/pubrs`). |
| `--templates-dir DIR` | Folder for user templates (default: the platform data folder, e.g. `~/.local/share/pubrs/templates`). |
| `--no-system-fonts` | Use only the fonts bundled with pubrs. |
| `--http ADDR` | Serve streamable HTTP instead of stdio. Only loopback `Host` headers are accepted. |

Logs go to stderr (`RUST_LOG=debug` for more). Example client configurations
are in `examples/mcp/`.

### Claude Desktop

Add to `claude_desktop_config.json` (see `examples/mcp/claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "pubrs": { "command": "pubrs-mcp", "args": [] }
  }
}
```

### Claude Code

```
claude mcp add pubrs -- pubrs-mcp
```

or commit `examples/mcp/.mcp.json` to a project.

## Conventions

- **Units** are points (1/72 inch). **Coordinates** start at the top-left
  corner of the page's trim box; x grows to the right, **y grows down**.
  Letter is 612 × 792 pt, A4 595.3 × 841.9 pt.
- **Ids**: every page, frame, story, style, guide, master and image has a
  UUID that never changes. Address objects by id, never by position.
- **`doc_id`** is optional on every tool and defaults to the most recently
  used document. Each open document gets a new `doc_id` for this session
  (the file's own id is in `doc_to_json`).
- **Revisions**: every tool that changes a document returns `doc_id`,
  `revision` (increases by one per change) and `affected_ids`. Queries
  return the current `revision`, so you can detect stale state.
- **Text offsets** count characters (Unicode scalar values); each paragraph
  break counts as one `"\n"`.
- **Undo**: each mutating call is one undo step (`undo`, `redo`, `history_list`).
- **Destructive tools** (`doc_close` with unsaved changes, `page_delete`,
  `frame_delete`, `style_delete`, `master_delete`) require `"confirm": true`.
- **Colours** are `"#RRGGBB"` or `"#RRGGBBAA"`; fill/stroke also accept `"none"`.
- **Images** can be a file path, base64 bytes or SVG markup (`svg`). SVG is
  the easiest way for an agent to supply artwork when no photo is available.

## Errors

Tool failures come back as a tool result with `isError: true` and one text
block holding JSON:

```json
{"code": "locked", "message": "frame … is locked", "hint": "unlock it first with frame_lock (locked: false)"}
```

Codes: `not_found`, `invalid_argument`, `conflict`, `locked`,
`confirmation_required`, `no_document`, `file_not_found`, `io_error`,
`invalid_file`, `invalid_image`, `export_failed`, `internal`. An unknown tool
name is a protocol error (JSON-RPC `-32602`). The server never panics across
the MCP boundary.

## Resources

| URI | Content |
|---|---|
| `pubrs://fonts` | Available font families (JSON). |
| `pubrs://templates/{name}` | Template metadata: page size and placeholders (JSON). |
| `pubrs://doc/{doc_id}` | The complete document (JSON, same as `doc_to_json`). |
| `pubrs://doc/{doc_id}/page/{n}.png` | PNG preview of page `n` (1-based), 72 dpi. |

## Prompts

| Prompt | Arguments | What it does |
|---|---|---|
| `make_flyer` | `subject`, `details`, `images` | Flyer from the `flyer` template, step by step. |
| `make_trifold` | `business`, `details`, `images` | Trifold brochure from `trifold_brochure`, including linking the story frames. |
| `make_business_card` | `name`, `job_title`, `company`, `contact`, `logo` | Business card from `business_card`. |

Each prompt ends with the same checks: `text_overflow_status` clean,
`validate` ok, `render_preview` of every page, then `doc_save_as` and
`export_pdf`.

## A typical session

1. `template_list` → pick a template.
2. `template_apply {name}` → note the placeholders (`frame_id`, `kind`, size, hint).
3. `text_set {frame_id, text}` for each text placeholder (template styling is kept).
4. `image_place {frame_id, path | base64 | svg}` for each image placeholder.
5. `text_overflow_status` → fix overflow (shorten, smaller text, or `text_link_frames`).
6. `validate` → fix every warning (each has a hint).
7. `render_preview` → look at each page.
8. `doc_save_as {path}` and `export_pdf {path}`.

# Tool reference

## Document

### `doc_new`

Create a new empty document and make it current. Sizes are in points (72 per inch). Defaults: letter portrait, 36pt margins, no bleed, 1 page. Returns doc_id and page ids.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `bleed` | number |  | Bleed in points (default 0; 9pt = 0.125in is typical for print). |
| `margins` | object |  | Margins (default 36pt = 0.5in on every side). |
| `page_size` | object |  | Page size (default letter portrait). |
| `pages` | integer |  | Number of pages (default 1). |
| `title` | string |  |  |

**Result fields:** `doc_id`, `page_ids`, `revision`

**Example**

```json
{
  "arguments": {
    "bleed": 9,
    "page_size": {
      "preset": "letter"
    },
    "title": "Spring Fair"
  },
  "name": "doc_new"
}
```

Result:

```json
{
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "page_ids": [
    "00000000-0000-87ea-8000-000000000002"
  ],
  "revision": 1
}
```

**Example**

```json
{
  "arguments": {
    "page_size": {
      "height": 72,
      "width": 189
    },
    "pages": 1,
    "title": "Address labels"
  },
  "name": "doc_new"
}
```

Result:

```json
{
  "doc_id": "00000000-0000-87ea-8000-00000000002d",
  "page_ids": [
    "00000000-0000-87ea-8000-000000000025"
  ],
  "revision": 1
}
```

### `doc_open`

Open a .pubrs file from disk and make it current. Returns a new doc_id for this session.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `path` | string | yes | Path of a .pubrs file. |

**Result fields:** `doc_id`, `page_ids`, `path`, `revision`, `title`

**Example**

```json
{
  "arguments": {
    "path": "<output_dir>/spring-fair.pubrs"
  },
  "name": "doc_open"
}
```

Result:

```json
{
  "doc_id": "00000000-0000-87ea-8000-000000000023",
  "page_ids": [
    "00000000-0000-87ea-8000-000000000002",
    "00000000-0000-87ea-8000-00000000000b"
  ],
  "path": "<output_dir>/spring-fair.pubrs",
  "revision": 0,
  "title": "Spring Fair"
}
```

### `doc_save`

Save the document to the path it was opened from or last saved to. Use doc_save_as for new documents.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |

**Result fields:** `bytes`, `doc_id`, `path`, `revision`

**Example** — A new document has no path yet: doc_save fails until doc_save_as is used.

```json
{
  "arguments": {},
  "name": "doc_save"
}
```

Error:

```json
{
  "code": "invalid_argument",
  "hint": "use doc_save_as with a path",
  "message": "the document has never been saved"
}
```

**Example**

```json
{
  "arguments": {},
  "name": "doc_save"
}
```

Result:

```json
{
  "bytes": 159280,
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "path": "<output_dir>/spring-fair.pubrs",
  "revision": 67
}
```

### `doc_save_as`

Save the document as a .pubrs file at a new path (the extension is added if missing). The file is written atomically.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `path` | string |  | Destination path; ".pubrs" is appended if missing. Relative paths and the default (the document title) go in the server's output folder. |

**Result fields:** `bytes`, `doc_id`, `path`, `revision`

**Example**

```json
{
  "arguments": {
    "path": "spring-fair"
  },
  "name": "doc_save_as"
}
```

Result:

```json
{
  "bytes": 159280,
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "path": "<output_dir>/spring-fair.pubrs",
  "revision": 67
}
```

### `doc_close`

Close a document. If it has unsaved changes, confirm: true is required and the changes are discarded.

_destructive (needs `confirm: true`)_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `confirm` | boolean |  | Required when the document has unsaved changes (they are discarded). |
| `doc_id` | string |  | Document id (default: the most recently used document). |

**Result fields:** `closed`, `discarded_changes`, `doc_id`

**Example**

```json
{
  "arguments": {
    "confirm": true,
    "doc_id": "00000000-0000-87ea-8000-000000000022"
  },
  "name": "doc_close"
}
```

Result:

```json
{
  "closed": true,
  "discarded_changes": false,
  "doc_id": "00000000-0000-87ea-8000-000000000022"
}
```

**Example**

```json
{
  "arguments": {
    "confirm": true,
    "doc_id": "00000000-0000-87ea-8000-00000000002d"
  },
  "name": "doc_close"
}
```

Result:

```json
{
  "closed": true,
  "discarded_changes": true,
  "doc_id": "00000000-0000-87ea-8000-00000000002d"
}
```

### `doc_list`

List open documents with their ids, titles, paths, revision and unsaved-changes flag.

_read-only_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |

**Result fields:** `documents`

**Example**

```json
{
  "arguments": {},
  "name": "doc_list"
}
```

Result:

```json
{
  "documents": [
    {
      "current": true,
      "dirty": false,
      "doc_id": "00000000-0000-87ea-8000-00000000000a",
      "page_count": 1,
      "…": "2 more keys"
    }
  ]
}
```

### `doc_info`

Describe a document: page size, margins and bleed (points), page ids, master pages, counts of frames, stories, styles and images.

_read-only_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |

**Result fields:** `asset_count`, `author`, `bleed`, `booklet`, `dirty`, `doc_id`, `frame_count`, `grid`, `margins`, `masters`, `page_ids`, `page_size`, `path`, `revision`, `sheet`, `story_count`, `style_count`, `template`, `title`

**Example**

```json
{
  "arguments": {
    "doc_id": "00000000-0000-87ea-8000-00000000000a"
  },
  "name": "doc_info"
}
```

Result:

```json
{
  "asset_count": 0,
  "author": "",
  "bleed": 9.0,
  "dirty": false,
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "frame_count": 0,
  "grid": {
    "show_grid": false,
    "snap_to_grid": false,
    "snap_to_guides": true,
    "snap_tolerance": 6.0,
    "spacing": 18.0
  },
  "margins": {
    "bottom": 36.0,
    "left": 36.0,
    "right": 36.0,
    "top": 36.0
  },
  "masters": [],
  "page_ids": [
    "00000000-0000-87ea-8000-000000000002"
  ],
  "page_size": {
    "height": 792.0,
    "width": 612.0
  },
  "revision": 1,
  "story_count": 0,
  "style_count": 7,
  "title": "Spring Fair"
}
```

### `doc_set_page_size`

Change the trim size of every page: a preset (letter, a4, business_card, ...) with optional landscape orientation, or width/height in points. Frames are not moved.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `height` | number |  | Custom height in points (used when no preset is given). |
| `orientation` | "portrait" \| "landscape" |  | Swap width and height for landscape presets. Default portrait. |
| `preset` | "letter" \| "legal" \| "tabloid" \| "a3" \| "a4" \| "a5" \| "a6" \| "business_card" \| "postcard" |  | Named size: letter (8.5x11 in), legal, tabloid, a3, a4, a5, a6, business_card (3.5x2 in), postcard (4x6 in). |
| `width` | number |  | Custom width in points (used when no preset is given). |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "orientation": "portrait",
    "preset": "a4"
  },
  "name": "doc_set_page_size"
}
```

Result:

```json
{
  "affected_ids": [],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 2
}
```

### `doc_set_margins`

Set the margin guides in points: `all` and/or individual sides. Margins are guides only; nothing is clipped.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `all` | number |  |  |
| `bottom` | number |  |  |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `left` | number |  |  |
| `right` | number |  |  |
| `top` | number |  |  |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "all": 36,
    "bottom": 54
  },
  "name": "doc_set_margins"
}
```

Result:

```json
{
  "affected_ids": [],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 3
}
```

### `doc_set_bleed`

Set the bleed in points (area outside the trim that is printed and cut off). 9pt (0.125in) is standard for print. Frames meant to bleed should extend to -bleed.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `bleed` | number | yes | Bleed in points on every side (0..=144). 9pt = 0.125in is standard. |
| `doc_id` | string |  | Document id (default: the most recently used document). |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "bleed": 9
  },
  "name": "doc_set_bleed"
}
```

Result:

```json
{
  "affected_ids": [],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 3
}
```

### `doc_set_info`

Set the document title and/or author (written to exported PDFs).

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `author` | string |  |  |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `title` | string |  |  |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "author": "Parish Council",
    "title": "Spring Fair"
  },
  "name": "doc_set_info"
}
```

Result:

```json
{
  "affected_ids": [],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 4
}
```

### `doc_set_grid`

Configure the layout grid and snapping: spacing in points, show_grid, snap_to_grid, snap_to_guides, snap_tolerance.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `show_grid` | boolean |  |  |
| `snap_to_grid` | boolean |  |  |
| `snap_to_guides` | boolean |  |  |
| `snap_tolerance` | number |  | Snap distance in points. |
| `spacing` | number |  | Grid spacing in points. |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "snap_to_guides": true,
    "spacing": 18
  },
  "name": "doc_set_grid"
}
```

Result:

```json
{
  "affected_ids": [],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 4
}
```

## Pages

### `page_add`

Add an empty page (optionally at a 0-based index and with a master page). Returns the new page_id.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `index` | integer |  | Insert position, 0-based (default: after the last page). |
| `master_id` | string |  | Master page to apply to the new page. |

**Result fields:** `affected_ids`, `doc_id`, `index`, `page_id`, `revision`

**Example**

```json
{
  "arguments": {},
  "name": "page_add"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-00000000000b"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "index": 1,
  "page_id": "00000000-0000-87ea-8000-00000000000b",
  "revision": 5
}
```

### `page_delete`

Delete a page and every frame on it. Requires confirm: true. The last page cannot be deleted.

_destructive (needs `confirm: true`)_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `confirm` | boolean |  | Must be true: the page and all its frames are deleted. |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `page_id` | uuid | yes | Identifies a page. |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "confirm": true,
    "page_id": "00000000-0000-87ea-8000-00000000000b"
  },
  "name": "page_delete"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-00000000000b"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 68
}
```

### `page_reorder`

Move a page to a new 0-based position.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `index` | integer | yes | New 0-based position. |
| `page_id` | uuid | yes | Identifies a page. |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "index": 0,
    "page_id": "00000000-0000-87ea-8000-00000000000b"
  },
  "name": "page_reorder"
}
```

Result:

```json
{
  "affected_ids": [],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 6
}
```

**Example**

```json
{
  "arguments": {
    "index": 1,
    "page_id": "00000000-0000-87ea-8000-00000000000b"
  },
  "name": "page_reorder"
}
```

Result:

```json
{
  "affected_ids": [],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 7
}
```

### `page_list`

List pages in order with ids, master page and frame counts.

_read-only_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |

**Result fields:** `page_size`, `pages`, `revision`

**Example**

```json
{
  "arguments": {},
  "name": "page_list"
}
```

Result:

```json
{
  "page_size": {
    "height": 841.8897637795277,
    "width": 595.2755905511812
  },
  "pages": [
    {
      "frame_count": 0,
      "index": 0,
      "page_id": "00000000-0000-87ea-8000-000000000002"
    },
    {
      "frame_count": 0,
      "index": 1,
      "page_id": "00000000-0000-87ea-8000-00000000000b"
    }
  ],
  "revision": 5
}
```

### `page_info`

Describe one page: its frames (back to front, with kind, position in points, text preview) and guides.

_read-only_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `page_id` | uuid | yes | Identifies a page. |

**Result fields:** `frames`, `guides`, `index`, `master_id`, `page_id`, `page_size`, `revision`

**Example**

```json
{
  "arguments": {
    "page_id": "00000000-0000-87ea-8000-000000000002"
  },
  "name": "page_info"
}
```

Result:

```json
{
  "frames": [
    {
      "frame_id": "00000000-0000-87ea-8000-00000000000f",
      "kind": "text",
      "locked": false,
      "owner": {
        "id": "00000000-0000-87ea-8000-000000000002",
        "kind": "page"
      },
      "…": "6 more keys"
    },
    {
      "frame_id": "00000000-0000-87ea-8000-000000000011",
      "kind": "shape",
      "locked": false,
      "owner": {
        "id": "00000000-0000-87ea-8000-000000000002",
        "kind": "page"
      },
      "…": "3 more keys"
    },
    {
      "frame_id": "00000000-0000-87ea-8000-000000000015",
      "kind": "text",
      "locked": false,
      "owner": {
        "id": "00000000-0000-87ea-8000-000000000002",
        "kind": "page"
      },
      "…": "5 more keys"
    },
    {
      "frame_id": "00000000-0000-87ea-8000-000000000017",
      "kind": "text",
      "locked": false,
      "owner": {
        "id": "00000000-0000-87ea-8000-000000000002",
        "kind": "page"
      },
      "…": "5 more keys"
    },
    "… 2 more"
  ],
  "guides": [],
  "index": 0,
  "master_id": "00000000-0000-87ea-8000-00000000000c",
  "page_id": "00000000-0000-87ea-8000-000000000002",
  "page_size": {
    "height": 841.8897637795277,
    "width": 595.2755905511812
  },
  "revision": 67
}
```

### `master_create`

Create an empty master page. Add frames to it with frame_add_* and master_id; apply it with master_apply. Master frames draw beneath page frames.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `name` | string | yes | Display name, e.g. "Footer". |

**Result fields:** `affected_ids`, `doc_id`, `master_id`, `revision`

**Example**

```json
{
  "arguments": {
    "name": "Footer"
  },
  "name": "master_create"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-00000000000c"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "master_id": "00000000-0000-87ea-8000-00000000000c",
  "revision": 8
}
```

### `master_apply`

Apply a master page to pages (default all), or pass master_id null to remove it.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `master_id` | string |  | Master to apply, or null to remove the master from the pages. |
| `page_ids` | array |  | Pages to change (default: every page). |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "master_id": "00000000-0000-87ea-8000-00000000000c"
  },
  "name": "master_apply"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000002",
    "00000000-0000-87ea-8000-00000000000b"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 10
}
```

### `master_delete`

Delete a master page and its frames; pages using it lose their master. Requires confirm: true.

_destructive (needs `confirm: true`)_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `confirm` | boolean |  | Must be true: the master and its frames are deleted. |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `master_id` | uuid | yes | Identifies a master page. |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "confirm": true,
    "master_id": "00000000-0000-87ea-8000-00000000000c"
  },
  "name": "master_delete"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000002",
    "00000000-0000-87ea-8000-00000000000d"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 69
}
```

### `guide_add`

Add a ruler guide to a page or master: axis horizontal (position = y) or vertical (position = x), in points.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `axis` | "horizontal" \| "vertical" | yes | "horizontal" (position is y) or "vertical" (position is x). |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `master_id` | string |  | Identifies a master page. |
| `page_id` | string |  | Page to add the guide to (or give master_id). |
| `position` | number | yes | Points from the page's top (horizontal) or left (vertical) edge. |

**Result fields:** `affected_ids`, `doc_id`, `guide_id`, `revision`

**Example**

```json
{
  "arguments": {
    "axis": "vertical",
    "page_id": "00000000-0000-87ea-8000-000000000002",
    "position": 297.6
  },
  "name": "guide_add"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000002",
    "00000000-0000-87ea-8000-00000000000e"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "guide_id": "00000000-0000-87ea-8000-00000000000e",
  "revision": 11
}
```

### `guide_move`

Move a ruler guide to a new position in points (found by guide_id; page_id or master_id optional).

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `guide_id` | uuid | yes | Identifies a ruler guide. |
| `master_id` | string |  | Identifies a master page. |
| `page_id` | string |  | Page or master owning the guide (optional: found by guide_id). |
| `position` | number | yes | New position in points from the page's top (horizontal) or left (vertical) edge. |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "guide_id": "00000000-0000-87ea-8000-00000000000e",
    "position": 310
  },
  "name": "guide_move"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000002"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 12
}
```

### `guide_remove`

Remove a ruler guide.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `guide_id` | uuid | yes | Identifies a ruler guide. |
| `master_id` | string |  | Identifies a master page. |
| `page_id` | string |  | Identifies a page. |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "guide_id": "00000000-0000-87ea-8000-00000000000e"
  },
  "name": "guide_remove"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000002"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 13
}
```

## Frames

### `frame_add_text`

Add a text frame. Position/size in points from the page's top-left (y down). Optional initial text ("\n" = new paragraph, markup: true for # headings and **bold**/*italic*), paragraph style, font, size, colour, alignment and columns. Returns frame_id and story_id.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `align` | "left" \| "center" \| "right" \| "justify" \| null |  | Horizontal paragraph alignment. |
| `color` | string \| object \| object |  | Text colour: hex or `{"swatch": id, "tint": 0-100}`. |
| `columns` | integer |  | Number of columns (1-12). |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `font_family` | string |  | Font family for the text, e.g. "Liberation Serif" (see fonts_list). |
| `font_size` | number |  | Font size in points. |
| `height` | number | yes | Height in points. |
| `markup` | boolean |  | Interpret markup in `text` (see text). |
| `master_id` | string |  | Identifies a master page. |
| `name` | string |  | Display name shown in frame lists. |
| `page_id` | string |  | Page to place the frame on (default: first page). Or give master_id. |
| `style` | string |  | Paragraph style name for all paragraphs (e.g. "Body", "Heading 1"). |
| `text` | string |  | Initial text; "\n" separates paragraphs. With markup=true, `# `/`## ` headings and **bold**, *italic*, __underline__ are recognised. |
| `width` | number | yes | Width in points. |
| `x` | number | yes | Left edge (points from the page's left edge). |
| `y` | number | yes | Top edge (points from the page's top edge, y grows down). |

**Result fields:** `affected_ids`, `asset_id`, `doc_id`, `frame_id`, `revision`, `story_id`

**Example**

```json
{
  "arguments": {
    "color": "#1E3A5F",
    "height": 90,
    "markup": true,
    "page_id": "00000000-0000-87ea-8000-000000000002",
    "text": "# Spring Fair\nSaturday 12 May · Village Green",
    "width": 523,
    "x": 36,
    "y": 36
  },
  "name": "frame_add_text"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000002",
    "00000000-0000-87ea-8000-00000000000f",
    "00000000-0000-87ea-8000-000000000010"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "frame_id": "00000000-0000-87ea-8000-00000000000f",
  "revision": 14,
  "story_id": "00000000-0000-87ea-8000-000000000010"
}
```

**Example**

```json
{
  "arguments": {
    "height": 120,
    "page_id": "00000000-0000-87ea-8000-000000000002",
    "style": "Body",
    "text": "Stalls, games and the famous cake competition. Bring the who… (227 characters)",
    "width": 250,
    "x": 36,
    "y": 370
  },
  "name": "frame_add_text"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000002",
    "00000000-0000-87ea-8000-000000000015",
    "00000000-0000-87ea-8000-000000000016"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "frame_id": "00000000-0000-87ea-8000-000000000015",
  "revision": 18,
  "story_id": "00000000-0000-87ea-8000-000000000016"
}
```

### `frame_add_image`

Add a picture frame, optionally with an image from a file path or base64 (PNG, JPEG, SVG). fit: fill (default, covers the frame), fit or stretch.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `base64` | string |  | Image bytes, base64. |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `filename` | string |  | Display name for base64 images. |
| `fit` | "fill" \| "fit" \| "stretch" |  | fill (default, covers the frame), fit (letterbox) or stretch. |
| `height` | number | yes | Height in points. |
| `master_id` | string |  | Identifies a master page. |
| `name` | string |  | Display name shown in frame lists. |
| `page_id` | string |  | Identifies a page. |
| `path` | string |  | Image file (PNG, JPEG or SVG). Omit both path and base64 for an empty picture frame. |
| `svg` | string |  | SVG markup as text (drawn artwork when no photo is available). |
| `width` | number | yes | Width in points. |
| `x` | number | yes | Left edge in points from the page's left edge. |
| `y` | number | yes | Top edge in points from the page's top edge (y grows down). |

**Result fields:** `affected_ids`, `asset_id`, `doc_id`, `frame_id`, `revision`, `story_id`

**Example**

```json
{
  "arguments": {
    "height": 200,
    "page_id": "00000000-0000-87ea-8000-000000000002",
    "path": "<assets>/bread.jpg",
    "width": 300,
    "x": 36,
    "y": 150
  },
  "name": "frame_add_image"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000002",
    "00000000-0000-87ea-8000-000000000012",
    "00000000-0000-87ea-8000-000000000013"
  ],
  "asset_id": "00000000-0000-87ea-8000-000000000012",
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "frame_id": "00000000-0000-87ea-8000-000000000013",
  "revision": 16
}
```

**Example**

```json
{
  "arguments": {
    "height": 199,
    "page_id": "00000000-0000-87ea-8000-000000000002",
    "width": 199,
    "x": 360,
    "y": 150
  },
  "name": "frame_add_image"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000002",
    "00000000-0000-87ea-8000-000000000014"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "frame_id": "00000000-0000-87ea-8000-000000000014",
  "revision": 17
}
```

### `frame_add_shape`

Add a rectangle (optional corner_radius), ellipse or line. Lines run corner to corner of the box (height 0 = horizontal). fill/stroke are colours or "none".

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `corner_radius` | number |  | Rectangles: corner radius in points. |
| `dash` | "solid" \| "dashed" \| "dotted" \| null |  | Dash pattern of a stroke. |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `fill` | string \| string \| object \| object |  | Fill colour (default none). |
| `height` | number | yes | Height in points. |
| `line_direction` | "down_right" \| "up_right" |  | Lines: down_right (top-left to bottom-right, default) or up_right. |
| `master_id` | string |  | Identifies a master page. |
| `name` | string |  | Display name shown in frame lists. |
| `page_id` | string |  | Identifies a page. |
| `shape` | "rectangle" \| "ellipse" \| "line" | yes | Shape type. |
| `stroke` | string \| string \| object \| object |  | Outline colour (default black for lines, none otherwise). |
| `stroke_width` | number |  | Outline width in points (default 1). |
| `width` | number | yes | Width in points. |
| `x` | number | yes | Bounding box. A line runs corner to corner; height 0 = horizontal line. Left edge in points from the page's left edge. |
| `y` | number | yes | Top edge in points from the page's top edge (y grows down). |

**Result fields:** `affected_ids`, `asset_id`, `doc_id`, `frame_id`, `revision`, `story_id`

**Example**

```json
{
  "arguments": {
    "height": 0,
    "master_id": "00000000-0000-87ea-8000-00000000000c",
    "shape": "line",
    "stroke": "#1E3A5F",
    "stroke_width": 1,
    "width": 523,
    "x": 36,
    "y": 800
  },
  "name": "frame_add_shape"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-00000000000d"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "frame_id": "00000000-0000-87ea-8000-00000000000d",
  "revision": 9
}
```

**Example**

```json
{
  "arguments": {
    "fill": "#C2410C",
    "height": 4,
    "page_id": "00000000-0000-87ea-8000-000000000002",
    "shape": "rectangle",
    "width": 523,
    "x": 36,
    "y": 132
  },
  "name": "frame_add_shape"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000002",
    "00000000-0000-87ea-8000-000000000011"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "frame_id": "00000000-0000-87ea-8000-000000000011",
  "revision": 15
}
```

### `frame_delete`

Delete a frame (groups delete their members). Requires confirm: true. Locked frames must be unlocked first.

_destructive (needs `confirm: true`)_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `confirm` | boolean |  | Must be true. |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `frame_id` | uuid | yes | Identifies a frame (text, image, shape or group). |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "confirm": true,
    "frame_id": "00000000-0000-87ea-8000-000000000020"
  },
  "name": "frame_delete"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000002",
    "00000000-0000-87ea-8000-000000000020"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 65
}
```

### `frame_move`

Move a frame: absolute x/y (top-left, points) or relative dx/dy. snap: true snaps to guides, margins, page edges and other frames.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `dx` | number |  | Move right by this many points (relative). |
| `dy` | number |  | Move down by this many points (relative). |
| `frame_id` | uuid | yes | Identifies a frame (text, image, shape or group). |
| `snap` | boolean |  | Snap to guides, margins, page edges and other frames (and the grid if enabled). |
| `x` | number |  | New left edge in points (absolute). Or use dx. |
| `y` | number |  | New top edge in points (absolute). Or use dy. |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "dy": 2,
    "frame_id": "00000000-0000-87ea-8000-000000000011"
  },
  "name": "frame_move"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000011"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 45
}
```

### `frame_resize`

Resize a frame to width/height in points, anchored at its top-left (default) or centre. Image frames keep their aspect ratio unless keep_aspect is false or a height is given.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `anchor` | "top_left" \| "center" |  | Fixed point when resizing. |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `frame_id` | uuid | yes | Identifies a frame (text, image, shape or group). |
| `height` | number |  | New height in points. With keep_aspect it is recomputed from width. |
| `keep_aspect` | boolean |  | Keep the current aspect ratio (default: true for image frames with lock_aspect, false otherwise). |
| `width` | number | yes | New width in points. |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "frame_id": "00000000-0000-87ea-8000-000000000013",
    "width": 290
  },
  "name": "frame_resize"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000013"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 46
}
```

### `frame_rotate`

Set a frame's clockwise rotation in degrees about its centre (absolute, not relative).

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `degrees` | number | yes | Absolute clockwise rotation in degrees. |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `frame_id` | uuid | yes | Identifies a frame (text, image, shape or group). |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "degrees": 4,
    "frame_id": "00000000-0000-87ea-8000-000000000014"
  },
  "name": "frame_rotate"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000014"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 47
}
```

### `frame_set_zorder`

Change stacking order: bring_to_front, send_to_back, bring_forward, send_backward, or an explicit index (0 = back).

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `action` | "bring_to_front" \| "send_to_back" \| "bring_forward" \| "send_backward" \| null |  | Z-order change. |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `frame_id` | uuid | yes | Identifies a frame (text, image, shape or group). |
| `index` | integer |  | Explicit position among siblings, 0 = back (instead of action). |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "action": "bring_to_front",
    "frame_id": "00000000-0000-87ea-8000-000000000013"
  },
  "name": "frame_set_zorder"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000002"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 48
}
```

### `frame_group`

Group two or more frames on the same page so they move together. Returns group_id.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `frame_ids` | array of uuid | yes | At least two frames on the same page. |

**Result fields:** `affected_ids`, `doc_id`, `group_id`, `revision`

**Example**

```json
{
  "arguments": {
    "frame_ids": [
      "00000000-0000-87ea-8000-000000000013",
      "00000000-0000-87ea-8000-000000000014"
    ]
  },
  "name": "frame_group"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000002",
    "00000000-0000-87ea-8000-00000000001f"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "group_id": "00000000-0000-87ea-8000-00000000001f",
  "revision": 62
}
```

### `frame_ungroup`

Dissolve a group; its members keep their positions.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `group_id` | uuid | yes | Identifies a frame (text, image, shape or group). |

**Result fields:** `affected_ids`, `doc_id`, `frame_ids`, `revision`

**Example**

```json
{
  "arguments": {
    "group_id": "00000000-0000-87ea-8000-00000000001f"
  },
  "name": "frame_ungroup"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000002",
    "00000000-0000-87ea-8000-00000000001f"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "frame_ids": [
    "00000000-0000-87ea-8000-000000000014",
    "00000000-0000-87ea-8000-000000000013"
  ],
  "revision": 63
}
```

### `frame_lock`

Lock (locked: true) or unlock a frame. Locked frames cannot be moved, resized, rotated or deleted; their text can still be edited.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `frame_id` | uuid | yes | Identifies a frame (text, image, shape or group). |
| `locked` | boolean |  | true locks (cannot be moved, resized, rotated or deleted), false unlocks. |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "frame_id": "00000000-0000-87ea-8000-000000000011",
    "locked": true
  },
  "name": "frame_lock"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000011"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 58
}
```

**Example**

```json
{
  "arguments": {
    "frame_id": "00000000-0000-87ea-8000-000000000011",
    "locked": false
  },
  "name": "frame_lock"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000011"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 59
}
```

### `frame_align`

Align frames' left/center/right/top/middle/bottom edges to the selection's bounds, the page or the margins.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `edge` | "left" \| "right" \| "top" \| "bottom" \| "center" \| "middle" | yes | left, center, right, top, middle or bottom. |
| `frame_ids` | array of uuid | yes |  |
| `relative_to` | "selection" \| "page" \| "margins" |  | What to align/distribute relative to. |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "edge": "top",
    "frame_ids": [
      "00000000-0000-87ea-8000-000000000013",
      "00000000-0000-87ea-8000-000000000014"
    ]
  },
  "name": "frame_align"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000013"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 60
}
```

### `frame_distribute`

Space frames evenly horizontally or vertically, within their own span (default), the page or the margins.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `axis` | "horizontal" \| "vertical" | yes | horizontal or vertical. |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `frame_ids` | array of uuid | yes | At least two frames. |
| `within` | "selection" \| "page" \| "margins" |  | selection (default: outer frames stay put), page or margins. |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "axis": "horizontal",
    "frame_ids": [
      "00000000-0000-87ea-8000-000000000013",
      "00000000-0000-87ea-8000-000000000014"
    ],
    "within": "margins"
  },
  "name": "frame_distribute"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000014"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 61
}
```

### `frame_set_style`

Set a frame's fill colour, outline (stroke colour, width in points, dash solid/dashed/dotted), opacity (0-1) and display name. Use "none" to remove fill or outline.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `dash` | "solid" \| "dashed" \| "dotted" \| null |  | Dash pattern of a stroke. |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `fill` | string \| string \| object \| object |  | Background fill, or "none". |
| `frame_id` | uuid | yes | Identifies a frame (text, image, shape or group). |
| `name` | string |  | Display name (empty string clears). Display name shown in frame lists. |
| `opacity` | number |  | 0 (invisible) to 1 (opaque). |
| `stroke` | string \| string \| object \| object |  | Outline colour, or "none". |
| `stroke_width` | number |  | Outline width in points. |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "fill": "#FFF7ED",
    "frame_id": "00000000-0000-87ea-8000-000000000017",
    "stroke": "#1E3A5F",
    "stroke_width": 1
  },
  "name": "frame_set_style"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000017"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 49
}
```

**Example** — Fills, outlines and text colours accept hex, a swatch with a tint, or (fills only) a gradient.

```json
{
  "arguments": {
    "fill": {
      "swatch": "00000000-0000-87ea-8000-00000000001d",
      "tint": 25
    },
    "frame_id": "00000000-0000-87ea-8000-000000000017",
    "stroke": {
      "swatch": "00000000-0000-87ea-8000-00000000001e"
    }
  },
  "name": "frame_set_style"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000017"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 53
}
```

### `frame_set_wrap`

Set how text in frames below this one wraps around it: none, square (text flows beside it) or top_bottom, with a margin in points. Pictures default to square.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `frame_id` | uuid | yes | Identifies a frame (text, image, shape or group). |
| `margin` | number |  | Clearance in points (default 6). |
| `mode` | "none" \| "square" \| "top_bottom" | yes | none, square (text flows around the frame) or top_bottom. |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "frame_id": "00000000-0000-87ea-8000-000000000014",
    "margin": 6,
    "mode": "square"
  },
  "name": "frame_set_wrap"
}
```

Result:

```json
{
  "affected_ids": [],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 56
}
```

### `frame_set_placeholder`

Mark a text or picture frame as a named template placeholder (e.g. "headline"), or pass name null to clear. Used when saving templates.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `frame_id` | uuid | yes | Identifies a frame (text, image, shape or group). |
| `hint` | string |  | Sample text / instruction shown until filled. |
| `name` | string |  | Semantic name like "headline" or "hero_image"; null removes the placeholder. |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "frame_id": "00000000-0000-87ea-8000-00000000000f",
    "hint": "Event name and date",
    "name": "headline"
  },
  "name": "frame_set_placeholder"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-00000000000f"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 57
}
```

### `frame_list`

List frames (all pages and masters, or one page/master), optionally only one kind: text, image, shape, group. Includes position, z-index, placeholder names and text previews.

_read-only_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `kind` | "text" \| "image" \| "shape" \| "group" \| null |  | Only frames of this kind: text, image, shape or group. |
| `master_id` | string |  | Only frames on this master page. |
| `page_id` | string |  | Only frames on this page. |

**Result fields:** `frames`, `revision`

**Example**

```json
{
  "arguments": {
    "kind": "text"
  },
  "name": "frame_list"
}
```

Result:

```json
{
  "frames": [
    {
      "frame_id": "00000000-0000-87ea-8000-00000000000f",
      "kind": "text",
      "locked": false,
      "owner": {
        "id": "00000000-0000-87ea-8000-000000000002",
        "kind": "page"
      },
      "…": "6 more keys"
    },
    {
      "frame_id": "00000000-0000-87ea-8000-000000000015",
      "kind": "text",
      "locked": false,
      "owner": {
        "id": "00000000-0000-87ea-8000-000000000002",
        "kind": "page"
      },
      "…": "5 more keys"
    },
    {
      "frame_id": "00000000-0000-87ea-8000-000000000017",
      "kind": "text",
      "locked": false,
      "owner": {
        "id": "00000000-0000-87ea-8000-000000000002",
        "kind": "page"
      },
      "…": "5 more keys"
    }
  ],
  "revision": 65
}
```

### `frame_get`

Get one frame in full (geometry, style, wrap, content), plus overflow for text frames and effective dpi for images.

_read-only_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `frame_id` | uuid | yes | Identifies a frame (text, image, shape or group). |

**Result fields:** `effective_dpi`, `frame`, `overflowing`, `revision`, `summary`

**Example**

```json
{
  "arguments": {
    "frame_id": "00000000-0000-87ea-8000-000000000013"
  },
  "name": "frame_get"
}
```

Result:

```json
{
  "effective_dpi": 446.8965517241379,
  "frame": {
    "content": {
      "alt_text": "Steaming cup of coffee on a wooden table",
      "asset": "00000000-0000-87ea-8000-00000000001b",
      "crop": {
        "bottom": 0.0,
        "left": 0.0,
        "right": 0.0,
        "top": 0.0
      },
      "fit": "fill",
      "…": "2 more keys"
    },
    "id": "00000000-0000-87ea-8000-000000000013",
    "locked": false,
    "rect": {
      "height": 193.33333333333334,
      "width": 290.0,
      "x": 36.0,
      "y": 143.30160786160704
    },
    "rotation": 0.0,
    "style": {
      "opacity": 1.0
    },
    "wrap": {
      "margin": 6.0,
      "mode": "square"
    }
  },
  "revision": 65,
  "summary": {
    "asset_id": "00000000-0000-87ea-8000-00000000001b",
    "frame_id": "00000000-0000-87ea-8000-000000000013",
    "kind": "image",
    "locked": false,
    "owner": {
      "id": "00000000-0000-87ea-8000-000000000002",
      "kind": "page"
    },
    "rect": {
      "height": 193.33333333333334,
      "width": 290.0,
      "x": 36.0,
      "y": 143.30160786160704
    },
    "rotation": 0.0,
    "z_index": 5
  }
}
```

### `frame_duplicate`

Copy a frame (text frames get a copy of their text) offset by dx/dy points, optionally onto another page.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `dx` | number |  | Offset of the copy in points (default 12, 12). |
| `dy` | number |  |  |
| `frame_id` | uuid | yes | Identifies a frame (text, image, shape or group). |
| `page_id` | string |  | Put the copy on another page. |

**Result fields:** `affected_ids`, `asset_id`, `doc_id`, `frame_id`, `revision`, `story_id`

**Example**

```json
{
  "arguments": {
    "dy": 360,
    "frame_id": "00000000-0000-87ea-8000-000000000011"
  },
  "name": "frame_duplicate"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000002",
    "00000000-0000-87ea-8000-000000000020"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "frame_id": "00000000-0000-87ea-8000-000000000020",
  "revision": 64
}
```

## Text

### `text_set`

Replace all text of a text frame's story (the whole linked chain). Keeps the existing first-paragraph formatting, so filling a template placeholder keeps its style. markup: true enables # headings and **bold**/*italic*/__underline__. Reports overflow.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `frame_id` | string |  | A text frame (its whole chain's story is replaced). Or give story_id. |
| `markup` | boolean |  | Interpret `# `/`## ` headings and **bold**, *italic*, __underline__. |
| `story_id` | string |  | Identifies a story: the text that flows through a chain of linked text frames. |
| `text` | string | yes | New text; "\n" separates paragraphs. Formatting of the first paragraph/run is kept (so template styling survives). |

**Result fields:** `affected_ids`, `doc_id`, `length`, `overflowing`, `revision`, `story_id`

**Example**

```json
{
  "arguments": {
    "frame_id": "00000000-0000-87ea-8000-00000000000f",
    "markup": true,
    "text": "# Spring Fair\nSaturday 12 May · **Village Green**"
  },
  "name": "text_set"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000010"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "length": 43,
  "overflowing": true,
  "revision": 35,
  "story_id": "00000000-0000-87ea-8000-000000000010"
}
```

### `text_insert`

Insert text at a character offset (default: end). Offsets count characters; each paragraph break is one "\n".

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `frame_id` | string |  | Identifies a frame (text, image, shape or group). |
| `offset` | integer |  | Character offset (0 = start). Paragraphs count as one "\n" character. Omit to append at the end. |
| `story_id` | string |  | Identifies a story: the text that flows through a chain of linked text frames. |
| `text` | string | yes |  |

**Result fields:** `affected_ids`, `doc_id`, `length`, `overflowing`, `revision`, `story_id`

**Example**

```json
{
  "arguments": {
    "frame_id": "00000000-0000-87ea-8000-00000000000f",
    "offset": 0,
    "text": "The "
  },
  "name": "text_insert"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000010"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "length": 47,
  "overflowing": false,
  "revision": 21,
  "story_id": "00000000-0000-87ea-8000-000000000010"
}
```

### `text_insert_break`

Insert a column, frame or page break at a character offset (default: end). A break is one character. column continues in the next column or frame; frame in the next frame of the chain; page in the next frame of the chain on a later page (pages are not added: with none, the rest overflows).

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `frame_id` | string |  | Identifies a frame (text, image, shape or group). |
| `kind` | "column" \| "frame" \| "page" | yes | column (continue in the next column or frame), frame (the next frame of the chain) or page (the next frame of the chain on a later page; with none, the rest overflows). |
| `offset` | integer |  | Character offset (0 = start). Omit to append at the end. |
| `story_id` | string |  | Identifies a story: the text that flows through a chain of linked text frames. |

**Result fields:** `affected_ids`, `doc_id`, `length`, `overflowing`, `revision`, `story_id`

**Example** — A break is one character in the story (column U+000E, frame U+001D, page U+000C). Without an offset it goes at the end, so what follows starts in the next frame of the chain.

```json
{
  "arguments": {
    "frame_id": "00000000-0000-87ea-8000-000000000015",
    "kind": "frame"
  },
  "name": "text_insert_break"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000016"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "length": 220,
  "overflowing": false,
  "revision": 29,
  "story_id": "00000000-0000-87ea-8000-000000000016"
}
```

### `text_replace_range`

Replace characters start..end (end exclusive) with new text; empty text deletes. The replaced text's formatting is kept.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `end` | integer | yes | End of the range (exclusive). start == end inserts. |
| `frame_id` | string |  | Identifies a frame (text, image, shape or group). |
| `start` | integer | yes | First character to replace (inclusive). |
| `story_id` | string |  | Identifies a story: the text that flows through a chain of linked text frames. |
| `text` | string | yes | Replacement text (may be empty to delete). |

**Result fields:** `affected_ids`, `doc_id`, `length`, `overflowing`, `revision`, `story_id`

**Example**

```json
{
  "arguments": {
    "end": 4,
    "frame_id": "00000000-0000-87ea-8000-00000000000f",
    "start": 0,
    "text": ""
  },
  "name": "text_replace_range"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000010"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "length": 43,
  "overflowing": false,
  "revision": 23,
  "story_id": "00000000-0000-87ea-8000-000000000010"
}
```

### `text_apply_char_style`

Format characters start..end (default all): character style by name, font_family, font_size (points), bold, italic, underline, colour, letter_spacing. clear: true removes local formatting first.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `bold` | boolean |  |  |
| `clear` | boolean |  | Remove local character formatting first. |
| `color` | string \| object \| object |  | Text colour: hex or `{"swatch": id, "tint": 0-100}`. |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `end` | integer |  | Range end (exclusive). Default: end of story. |
| `font_family` | string |  |  |
| `font_size` | number |  | Points. |
| `frame_id` | string |  | Identifies a frame (text, image, shape or group). |
| `italic` | boolean |  |  |
| `letter_spacing` | number |  | Extra letter spacing in points. |
| `start` | integer |  | Range start (character offset). Default 0. |
| `story_id` | string |  | Identifies a story: the text that flows through a chain of linked text frames. |
| `style` | string |  | Character style name or id ("" removes the character style). |
| `underline` | boolean |  |  |

**Result fields:** `affected_ids`, `doc_id`, `length`, `overflowing`, `revision`, `story_id`

**Example**

```json
{
  "arguments": {
    "color": "#C2410C",
    "end": 11,
    "font_size": 40,
    "frame_id": "00000000-0000-87ea-8000-00000000000f",
    "start": 0
  },
  "name": "text_apply_char_style"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000010"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "length": 43,
  "overflowing": false,
  "revision": 24,
  "story_id": "00000000-0000-87ea-8000-000000000010"
}
```

**Example**

```json
{
  "arguments": {
    "color": {
      "swatch": "00000000-0000-87ea-8000-00000000001e",
      "tint": 80
    },
    "frame_id": "00000000-0000-87ea-8000-000000000017"
  },
  "name": "text_apply_char_style"
}
```

Result:

```json
{
  "affected_ids": [],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "length": 0,
  "overflowing": false,
  "revision": 54,
  "story_id": "00000000-0000-87ea-8000-000000000019"
}
```

### `text_apply_para_style`

Format paragraphs touching start..end (default all): paragraph style by name (e.g. "Heading 1", "Body"), align left/center/right/justify, line_spacing (multiple of font size), space_before/after and indents in points, hyphenation, keep_with_next, keep_together, widow_orphan (true = 2 lines), drop_cap {lines, chars}, tab_stops and list (see text_set_tabs and text_apply_list).

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `align` | "left" \| "center" \| "right" \| "justify" \| null |  | Horizontal paragraph alignment. |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `drop_cap` | object |  | Drop cap `{lines, chars}` (defaults 3 and 1); `{"lines": 0}` removes it. |
| `end` | integer |  |  |
| `first_line_indent` | number |  |  |
| `frame_id` | string |  | Identifies a frame (text, image, shape or group). |
| `hyphenate` | boolean |  | Break words at hyphenation points on loose lines (default off). |
| `justify_limits` | object |  | Caps on justified stretch: `{max_word_space, letter_spacing}` (defaults 1.33 and 0.05). Unset leaves it as is (inherited, or unbounded as in v0.1); `{"unbounded": true}` removes limits a paragraph style sets. |
| `keep_together` | boolean |  | Keep all lines in one column or frame (default off). |
| `keep_with_next` | boolean |  | Keep the last line in the same column or frame as the next paragraph's first line (default off). |
| `language` | string |  | BCP 47 language tag for hyphenation (default "en-US"). |
| `left_indent` | number |  |  |
| `line_spacing` | number |  | Line height as a multiple of the font size (1.2 = normal). |
| `list` | object |  | Bullet or numbered list `{kind: bullet\|number\|none, bullet, format, level, indent, start}`; `{"kind": "none"}` removes it. See text_apply_list. |
| `right_indent` | number |  |  |
| `space_after` | number |  | Points. |
| `space_before` | number |  | Points. |
| `start` | integer |  | Paragraphs touching start..end are changed. Default: all paragraphs. |
| `story_id` | string |  | Identifies a story: the text that flows through a chain of linked text frames. |
| `style` | string |  | Paragraph style name or id (e.g. "Heading 1", "Body"). |
| `tab_stops` | array |  | Tab stops `[{pos, align: left\|center\|right\|decimal, leader: none\|dot\|dash\|line}]` (pos in points from the text edge); `[]` clears them. See text_set_tabs. |
| `widow_orphan` | boolean \| integer |  | Widow and orphan control: `true` (2 lines), a line count, or `false`/0 to turn it off (default off). |

**Result fields:** `affected_ids`, `doc_id`, `length`, `overflowing`, `revision`, `story_id`

**Example**

```json
{
  "arguments": {
    "align": "justify",
    "frame_id": "00000000-0000-87ea-8000-000000000015",
    "space_after": 6
  },
  "name": "text_apply_para_style"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000016"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "length": 219,
  "overflowing": false,
  "revision": 25,
  "story_id": "00000000-0000-87ea-8000-000000000016"
}
```

**Example** — Hyphenation on, and justification limited to the defaults (word spaces up to 133%, then letter spacing up to 5%). Without justify_limits, justification is unbounded as in v0.1; typed soft hyphens (U+00AD) are honoured either way.

```json
{
  "arguments": {
    "align": "justify",
    "frame_id": "00000000-0000-87ea-8000-000000000015",
    "hyphenate": true,
    "justify_limits": {},
    "language": "en-US"
  },
  "name": "text_apply_para_style"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000016"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "length": 219,
  "overflowing": false,
  "revision": 26,
  "story_id": "00000000-0000-87ea-8000-000000000016"
}
```

### `text_set_tabs`

Set the tab stops of the paragraphs touching start..end (default all): stops [{pos, align left/center/right/decimal, leader none/dot/dash/line}], pos in points from the text edge. An empty list clears them. A tab with no stop ahead goes to the next 36 pt. Decimal stops align on the first . (or , in languages that write a decimal comma). Leaders fill the gap before the text and line up from line to line.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `end` | integer |  |  |
| `frame_id` | string |  | Identifies a frame (text, image, shape or group). |
| `start` | integer |  | Paragraphs touching start..end are changed. Default: all paragraphs. |
| `stops` | array of object | yes | The tab stops, replacing the paragraphs' own: `pos` in points from the text edge, `align` left (default), center, right or decimal, `leader` none (default), dot, dash or line. An empty list clears them. Without stops a tab goes to the next 36 pt; past the last stop too. |
| `story_id` | string |  | Identifies a story: the text that flows through a chain of linked text frames. |

**Result fields:** `affected_ids`, `doc_id`, `length`, `overflowing`, `revision`, `story_id`

**Example** — A right-aligned stop 200 pt from the text edge with a dot leader: "Soup<TAB>4.50" fills the gap with dots, and leaders line up from line to line. Align is left (default), center, right or decimal; leader is none (default), dot, dash or line. A tab past the last stop (or with no stops) goes to the next 36 pt.

```json
{
  "arguments": {
    "end": 0,
    "frame_id": "00000000-0000-87ea-8000-000000000015",
    "start": 0,
    "stops": [
      {
        "align": "right",
        "leader": "dot",
        "pos": 200
      }
    ]
  },
  "name": "text_set_tabs"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000016"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "length": 220,
  "overflowing": false,
  "revision": 30,
  "story_id": "00000000-0000-87ea-8000-000000000016"
}
```

**Example** — An empty list clears the stops.

```json
{
  "arguments": {
    "end": 0,
    "frame_id": "00000000-0000-87ea-8000-000000000015",
    "start": 0,
    "stops": []
  },
  "name": "text_set_tabs"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000016"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "length": 220,
  "overflowing": false,
  "revision": 31,
  "story_id": "00000000-0000-87ea-8000-000000000016"
}
```

### `text_apply_list`

Make the paragraphs touching start..end (default all) bullet or numbered list items with a hanging indent: kind, format (1. a. A. i. I.), bullet character, level 0-8, indent (points the label hangs left of the text), restart_at (number the first paragraph from this value). Numbering continues across paragraphs and linked frames; a deeper level restarts after a shallower item. Labels are generated text: they are not in the story and do not move offsets (text_get lists them as list_label).

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `bullet` | string |  | Bullet character (default U+2022). |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `end` | integer |  |  |
| `format` | "1." \| "a." \| "A." \| "i." \| "I." \| null |  | Number format: "1." (default), "a.", "A.", "i." or "I.". |
| `frame_id` | string |  | Identifies a frame (text, image, shape or group). |
| `indent` | number |  | Hanging indent in points: the label sits this far left of the text (default 18). |
| `kind` | "bullet" \| "number" \| "none" | yes | bullet or number. |
| `left_indent` | number |  | Left indent of the text in points (default: (level + 1) x indent). |
| `level` | integer |  | Nesting level 0-8 (default 0). A deeper level restarts when a shallower item appears. |
| `restart_at` | integer |  | Number the first paragraph of the range from this value instead of continuing the count. |
| `start` | integer |  | Paragraphs touching start..end become list items. Default: all paragraphs. |
| `story_id` | string |  | Identifies a story: the text that flows through a chain of linked text frames. |

**Result fields:** `affected_ids`, `doc_id`, `length`, `overflowing`, `revision`, `story_id`

**Example** — Numbered list items with a hanging indent: the number sits 18 pt (indent) left of the text. Numbering continues across paragraphs and linked frames; level 1 items restart after a level 0 item. The label is generated text, not part of the story: text_get shows it as list_label.

```json
{
  "arguments": {
    "end": 0,
    "format": "1.",
    "frame_id": "00000000-0000-87ea-8000-000000000015",
    "kind": "number",
    "restart_at": 1,
    "start": 0
  },
  "name": "text_apply_list"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000016"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "length": 220,
  "overflowing": false,
  "revision": 32,
  "story_id": "00000000-0000-87ea-8000-000000000016"
}
```

**Example** — kind is bullet or number; text_clear_list removes a list.

```json
{
  "arguments": {
    "end": 0,
    "frame_id": "00000000-0000-87ea-8000-000000000015",
    "kind": "none",
    "start": 0
  },
  "name": "text_apply_list"
}
```

Error:

```json
{
  "code": "invalid_argument",
  "hint": "use text_clear_list to remove a list",
  "message": "kind is bullet or number"
}
```

### `text_clear_list`

Turn list items touching start..end (default all) back into plain paragraphs. Resets the left indent to 0 unless keep_indent is true.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `end` | integer |  |  |
| `frame_id` | string |  | Identifies a frame (text, image, shape or group). |
| `keep_indent` | boolean |  | Leave the left indent as it is (default: it is reset to 0). |
| `start` | integer |  | Paragraphs touching start..end stop being list items. Default: all paragraphs. |
| `story_id` | string |  | Identifies a story: the text that flows through a chain of linked text frames. |

**Result fields:** `affected_ids`, `doc_id`, `length`, `overflowing`, `revision`, `story_id`

**Example**

```json
{
  "arguments": {
    "end": 0,
    "frame_id": "00000000-0000-87ea-8000-000000000015",
    "start": 0
  },
  "name": "text_clear_list"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000016"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "length": 220,
  "overflowing": false,
  "revision": 33,
  "story_id": "00000000-0000-87ea-8000-000000000016"
}
```

### `text_link_frames`

Link two text frames so text overflowing from_frame_id continues in to_frame_id (which must not already be linked). Reports whether the chain still overflows.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `from_frame_id` | uuid | yes | Text frame whose overflow continues into `to_frame_id`. |
| `to_frame_id` | uuid | yes | An unlinked text frame (its text, if any, is appended to the chain). |

**Result fields:** `affected_ids`, `doc_id`, `length`, `overflowing`, `revision`, `story_id`

**Example**

```json
{
  "arguments": {
    "from_frame_id": "00000000-0000-87ea-8000-000000000015",
    "to_frame_id": "00000000-0000-87ea-8000-000000000017"
  },
  "name": "text_link_frames"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000016",
    "00000000-0000-87ea-8000-000000000017",
    "00000000-0000-87ea-8000-000000000018"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "length": 227,
  "overflowing": false,
  "revision": 20,
  "story_id": "00000000-0000-87ea-8000-000000000016"
}
```

### `text_unlink_frames`

Break the link after a frame. The text stays in the first part of the chain; later frames get a new empty story.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `frame_id` | uuid | yes | The link after this frame is broken; later frames get a new empty story. |

**Result fields:** `affected_ids`, `doc_id`, `new_story_id`, `revision`

**Example**

```json
{
  "arguments": {
    "frame_id": "00000000-0000-87ea-8000-000000000015"
  },
  "name": "text_unlink_frames"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000016",
    "00000000-0000-87ea-8000-000000000017",
    "00000000-0000-87ea-8000-000000000019"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "new_story_id": "00000000-0000-87ea-8000-000000000019",
  "revision": 36
}
```

### `text_find`

Find text in the whole document (every story) or one story. Options: case_sensitive, whole_word, and a start/end character range for one story. Returns story_id, the frame showing the match start, character offsets and a context snippet. An empty query is an error.

_read-only_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `case_sensitive` | boolean |  | Match letter case exactly (default false). |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `end` | integer |  | Only match before this character offset. Needs frame_id or story_id. |
| `frame_id` | string |  | Limit the search to this text frame's story. Omit both frame_id and story_id to search every story in the document. |
| `query` | string | yes | Text to find. A "\n" matches a paragraph break. |
| `start` | integer |  | Only match at or after this character offset. Needs frame_id or story_id. |
| `story_id` | string |  | Identifies a story: the text that flows through a chain of linked text frames. |
| `whole_word` | boolean |  | Match whole words only (default false). |

**Result fields:** `count`, `matches`

**Example**

```json
{
  "arguments": {
    "case_sensitive": false,
    "query": "family",
    "whole_word": true
  },
  "name": "text_find"
}
```

Result:

```json
{
  "count": 1,
  "matches": [
    {
      "context": "on. Bring the whole family for a day on the gr",
      "end": 69,
      "frame_id": "00000000-0000-87ea-8000-000000000015",
      "start": 63,
      "…": "1 more keys"
    }
  ]
}
```

### `text_replace_all`

Replace every match of query with replacement across the document or one story (same options as text_find). One undo step. Returns the count; zero matches changes nothing. An empty query is an error.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `case_sensitive` | boolean |  |  |
| `doc_id` | string |  | Identifies a document within a session. |
| `end` | integer |  | Only replace before this character offset. Needs frame_id or story_id. |
| `frame_id` | string |  | Identifies a frame (text, image, shape or group). |
| `query` | string | yes |  |
| `replacement` | string | yes | Replacement text (empty deletes the matches). |
| `start` | integer |  | Only replace at or after this character offset. Needs frame_id or story_id. |
| `story_id` | string |  | Identifies a story: the text that flows through a chain of linked text frames. |
| `whole_word` | boolean |  |  |

**Result fields:** `affected_ids`, `count`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "query": "Bring the whole family",
    "replacement": "Bring everyone"
  },
  "name": "text_replace_all"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000016"
  ],
  "count": 1,
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 22
}
```

### `text_get`

Get a story's text with character offsets per paragraph and run, styles, effective fonts, the range each linked frame shows and how many characters overflow.

_read-only_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `frame_id` | string |  | Identifies a frame (text, image, shape or group). |
| `story_id` | string |  | Identifies a story: the text that flows through a chain of linked text frames. |

**Result fields:** `frames`, `hidden_chars`, `length`, `paragraphs`, `revision`, `story_id`, `text`

**Example**

```json
{
  "arguments": {
    "frame_id": "00000000-0000-87ea-8000-00000000000f"
  },
  "name": "text_get"
}
```

Result:

```json
{
  "frames": [
    {
      "end": 43,
      "frame_id": "00000000-0000-87ea-8000-00000000000f",
      "start": 0
    }
  ],
  "hidden_chars": 0,
  "length": 43,
  "paragraphs": [
    {
      "align": "left",
      "end": 11,
      "index": 0,
      "props": {},
      "…": "3 more keys"
    },
    {
      "align": "left",
      "end": 43,
      "index": 1,
      "props": {},
      "…": "3 more keys"
    }
  ],
  "revision": 20,
  "story_id": "00000000-0000-87ea-8000-000000000010",
  "text": "Spring Fair\nSaturday 12 May · Village Green"
}
```

### `text_overflow_status`

Report every story: its frame chain, whether it overflows and how many characters are hidden. Fix overflow by enlarging the last frame, shortening text or linking another frame.

_read-only_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |

**Result fields:** `any_overflow`, `revision`, `stories`

**Example**

```json
{
  "arguments": {},
  "name": "text_overflow_status"
}
```

Result:

```json
{
  "any_overflow": false,
  "revision": 20,
  "stories": [
    {
      "frame_ids": [
        "00000000-0000-87ea-8000-00000000000f"
      ],
      "hidden_chars": 0,
      "last_frame_id": "00000000-0000-87ea-8000-00000000000f",
      "overflowing": false,
      "…": "1 more keys"
    },
    {
      "frame_ids": [
        "00000000-0000-87ea-8000-000000000015",
        "00000000-0000-87ea-8000-000000000017"
      ],
      "hidden_chars": 0,
      "last_frame_id": "00000000-0000-87ea-8000-000000000017",
      "overflowing": false,
      "…": "1 more keys"
    }
  ]
}
```

### `text_set_frame_options`

Set a text frame's columns (1-12), column gap and inner inset in points, and vertical alignment (top/middle/bottom).

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `column_gap` | number |  | Gap between columns in points. |
| `columns` | integer |  | Columns inside the frame (1-12). |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `frame_id` | uuid | yes | Identifies a frame (text, image, shape or group). |
| `inset` | number |  | Inner padding in points on every side. |
| `vertical_align` | "top" \| "middle" \| "bottom" \| null |  | top, middle or bottom. |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "frame_id": "00000000-0000-87ea-8000-000000000017",
    "inset": 6,
    "vertical_align": "top"
  },
  "name": "text_set_frame_options"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000017"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 34
}
```

## Spelling

### `spell_check`

Find misspelt words in the whole document (reading order) or in one story (frame_id or story_id). Each paragraph is checked in its own language (paragraph `language`, BCP 47): en-US is built in, other languages need a user Hunspell dictionary; paragraphs in a language without one are skipped and listed in unchecked_languages. Words with digits, web and e-mail addresses, merge fields and (by default) words in capitals only are not checked. Returns story character ranges like text_find; fix a word with text_replace_range.

_read-only_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `frame_id` | string |  | Check only this text frame's story. Omit both frame_id and story_id to check every story in the document. |
| `ignore_all_caps` | boolean |  | Skip words written in capitals only, such as acronyms (default true). |
| `story_id` | string |  | Identifies a story: the text that flows through a chain of linked text frames. |

**Result fields:** `count`, `dictionaries`, `issues`, `unchecked_languages`

**Example** — Misspelt words with story offsets. The French paragraph has no dictionary (only en_US is built in), so it is skipped and listed in unchecked_languages.

```json
{
  "arguments": {
    "frame_id": "00000000-0000-87ea-8000-000000000038"
  },
  "name": "spell_check"
}
```

Result:

```json
{
  "count": 3,
  "dictionaries": [
    "en_US"
  ],
  "issues": [
    {
      "end": 24,
      "frame_id": "00000000-0000-87ea-8000-000000000038",
      "language": "en-US",
      "start": 16,
      "…": "2 more keys"
    },
    {
      "end": 37,
      "frame_id": "00000000-0000-87ea-8000-000000000038",
      "language": "en-US",
      "start": 31,
      "…": "2 more keys"
    },
    {
      "end": 57,
      "frame_id": "00000000-0000-87ea-8000-000000000038",
      "language": "en-US",
      "start": 52,
      "…": "2 more keys"
    }
  ],
  "unchecked_languages": [
    "fr-FR"
  ]
}
```

**Example** — The whole document, after adding pubrs.

```json
{
  "arguments": {},
  "name": "spell_check"
}
```

Result:

```json
{
  "count": 2,
  "dictionaries": [
    "en_US"
  ],
  "issues": [
    {
      "end": 24,
      "frame_id": "00000000-0000-87ea-8000-000000000038",
      "language": "en-US",
      "start": 16,
      "…": "2 more keys"
    },
    {
      "end": 37,
      "frame_id": "00000000-0000-87ea-8000-000000000038",
      "language": "en-US",
      "start": 31,
      "…": "2 more keys"
    }
  ],
  "unchecked_languages": [
    "fr-FR"
  ]
}
```

### `spell_suggest`

Suggest corrections for a word in a language (default en-US), best first, and say whether the word is already correct.

_read-only_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `language` | string |  | BCP 47 language tag, such as a spell_check issue's language (default "en-US"). |
| `word` | string | yes | The word to correct. |

**Result fields:** `correct`, `dictionary`, `suggestions`, `word`

**Example**

```json
{
  "arguments": {
    "language": "en-US",
    "word": "mornng"
  },
  "name": "spell_suggest"
}
```

Result:

```json
{
  "correct": false,
  "dictionary": "en_US",
  "suggestions": [
    "morning",
    "morn"
  ],
  "word": "mornng"
}
```

**Example** — No dictionary for French: add fr_FR.dic and fr_FR.aff to the dictionaries folder.

```json
{
  "arguments": {
    "language": "fr-FR",
    "word": "chaton"
  },
  "name": "spell_suggest"
}
```

Error:

```json
{
  "code": "not_found",
  "hint": "available: en_US; add a Hunspell .dic/.aff pair to the dictionaries folder for other languages",
  "message": "dictionary for \"fr-FR\" not found"
}
```

### `spell_add_word`

Add a word to the user dictionary so spell_check accepts it in every language (capitalised and all-capitals forms too). The word is saved to the user dictionary file when the session has a dictionaries folder (the app and the pubrs CLI do); otherwise it lasts for this session.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `word` | string | yes | One word without spaces (at most 64 characters). From now on it is accepted in every language. |

**Result fields:** `added`, `path`, `word`

**Example** — Without a dictionaries folder (as in this example session) the word lasts for the session; the app and the CLI save it to user-words.txt.

```json
{
  "arguments": {
    "word": "pubrs"
  },
  "name": "spell_add_word"
}
```

Result:

```json
{
  "added": true,
  "word": "pubrs"
}
```

## Styles

### `style_list`

List paragraph and character styles with their properties and parents. Built-ins: Normal, Title, Heading 1, Heading 2, Body, Caption, Emphasis.

_read-only_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `kind` | "paragraph" \| "character" \| null |  | Only paragraph or character styles. |

**Result fields:** `revision`, `styles`

**Example**

```json
{
  "arguments": {
    "kind": "paragraph"
  },
  "name": "style_list"
}
```

Result:

```json
{
  "revision": 36,
  "styles": [
    {
      "character": {
        "bold": false,
        "color": "#000000",
        "font_family": "Liberation Sans",
        "font_size": 11.0,
        "…": "3 more keys"
      },
      "is_default": true,
      "kind": "paragraph",
      "name": "Normal",
      "…": "2 more keys"
    },
    {
      "based_on": "Normal",
      "character": {
        "bold": true,
        "font_size": 36.0
      },
      "is_default": false,
      "kind": "paragraph",
      "…": "3 more keys"
    },
    {
      "based_on": "Normal",
      "character": {
        "bold": true,
        "font_size": 20.0
      },
      "is_default": false,
      "kind": "paragraph",
      "…": "3 more keys"
    },
    {
      "based_on": "Normal",
      "character": {
        "bold": true,
        "font_size": 14.0
      },
      "is_default": false,
      "kind": "paragraph",
      "…": "3 more keys"
    },
    "… 2 more"
  ]
}
```

### `style_create`

Create a named paragraph (default) or character style, optionally based_on another style. Properties: paragraph {align, line_spacing, space_before, space_after, indents}, character {font_family, font_size, bold, italic, underline, color, letter_spacing}.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `based_on` | string |  | Parent style name or id of the same kind; unset properties inherit from it. |
| `character` | object |  | Character formatting overrides. Unset fields inherit. |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `kind` | "paragraph" \| "character" \| null |  | paragraph (default) or character. |
| `name` | string | yes | Unique name. |
| `paragraph` | object |  | Paragraph formatting overrides. Unset fields inherit. |

**Result fields:** `affected_ids`, `doc_id`, `revision`, `style_id`

**Example**

```json
{
  "arguments": {
    "based_on": "Body",
    "character": {
      "bold": true,
      "font_size": 16
    },
    "name": "Price"
  },
  "name": "style_create"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-00000000001a"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 37,
  "style_id": "00000000-0000-87ea-8000-00000000001a"
}
```

### `style_update`

Rename a style, change its parent, or merge new paragraph/character properties into it. All text using it updates.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `based_on` | string |  | New parent name or id ("" removes the parent). |
| `character` | object |  | Character properties to merge in (set fields win). |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `name` | string |  | Rename. |
| `paragraph` | object |  | Paragraph properties to merge in (set fields win). |
| `style` | string | yes | Style name or id. |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "character": {
      "color": "#C2410C"
    },
    "style": "Price"
  },
  "name": "style_update"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-00000000001a"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 38
}
```

**Example** — Headings stay with the paragraph after them, and in one piece.

```json
{
  "arguments": {
    "paragraph": {
      "keep_together": true,
      "keep_with_next": true
    },
    "style": "Heading 1"
  },
  "name": "style_update"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000005"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 39
}
```

### `style_delete`

Delete a style (not the default Normal style). Text using it falls back. Requires confirm: true.

_destructive (needs `confirm: true`)_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `confirm` | boolean |  | Must be true. |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `style` | string | yes | Style name or id. Text using it falls back to its parent / default. |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "confirm": true,
    "style": "Price"
  },
  "name": "style_delete"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-00000000001a"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 40
}
```

## Colour

### `swatch_list`

List the document's swatches (rgb, cmyk or spot) with their screen colour, where each is used, and the applied colour scheme. Use a swatch anywhere a colour is accepted as {"swatch": id, "tint": 0-100}.

_read-only_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |

**Result fields:** `revision`, `scheme`, `swatches`

**Example**

```json
{
  "arguments": {},
  "name": "swatch_list"
}
```

Result:

```json
{
  "revision": 54,
  "swatches": [
    {
      "name": "Brand Cyan",
      "screen": "#17ACE6",
      "swatch_id": "00000000-0000-87ea-8000-00000000001d",
      "used_by": [
        "frame 00000000-0000-87ea-8000-000000000017 fill"
      ],
      "…": "1 more keys"
    },
    {
      "name": "Brand Red",
      "screen": "#F5002F",
      "swatch_id": "00000000-0000-87ea-8000-00000000001e",
      "used_by": [
        "frame 00000000-0000-87ea-8000-000000000017 outline"
      ],
      "…": "1 more keys"
    }
  ]
}
```

### `swatch_create`

Create a named swatch: value {"rgb": "#1E3A8A"}, {"cmyk": {c,m,y,k}} (0-100) or {"spot": {name, cmyk}}. A spot colour must have a CMYK alternative. Returns swatch_id.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `name` | string | yes | Unique name. |
| `value` | object \| object \| object | yes | `{"rgb": "#1E3A8A"}`, `{"cmyk": {"c":100,"m":60,"y":0,"k":20}}` or `{"spot": {"name": "PANTONE 186 C", "cmyk": {...}}}`. |

**Result fields:** `affected_ids`, `doc_id`, `revision`, `swatch_id`

**Example** — CMYK is shown on screen by a plain formula; the PDF keeps the values.

```json
{
  "arguments": {
    "name": "Brand Cyan",
    "value": {
      "cmyk": {
        "c": 100,
        "k": 10,
        "m": 30,
        "y": 0
      }
    }
  },
  "name": "swatch_create"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-00000000001d"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 50,
  "swatch_id": "00000000-0000-87ea-8000-00000000001d"
}
```

**Example**

```json
{
  "arguments": {
    "name": "Brand Red",
    "value": {
      "spot": {
        "cmyk": {
          "c": 0,
          "k": 4,
          "m": 100,
          "y": 81
        },
        "name": "PANTONE 186 C"
      }
    }
  },
  "name": "swatch_create"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-00000000001e"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 51,
  "swatch_id": "00000000-0000-87ea-8000-00000000001e"
}
```

### `swatch_update`

Rename a swatch or change its value; every frame fill, outline and text colour using it recolours at once.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `name` | string |  | Rename. |
| `swatch` | string | yes | Swatch name or id. |
| `value` | object \| object \| object |  | New value. Everything using the swatch recolours. |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "swatch": "Brand Cyan",
    "value": {
      "cmyk": {
        "c": 90,
        "k": 10,
        "m": 25,
        "y": 0
      }
    }
  },
  "name": "swatch_update"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-00000000001d"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 52
}
```

### `swatch_delete`

Delete a swatch. One that is in use is refused (the error lists where) unless replace_with gives a colour or swatch to use instead. Scheme slot swatches cannot be deleted. Requires confirm: true.

_destructive (needs `confirm: true`)_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `confirm` | boolean |  | Must be true. |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `replace_with` | string \| object \| object |  | What to use where the swatch is used (hex or `{"swatch": id, "tint"}`; a gradient only if the swatch is used by fills alone). Required when the swatch is in use. |
| `swatch` | string | yes | Swatch name or id. |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example** — A swatch in use is refused and the error lists where.

```json
{
  "arguments": {
    "confirm": true,
    "swatch": "Brand Cyan"
  },
  "name": "swatch_delete"
}
```

Error:

```json
{
  "code": "conflict",
  "hint": "pass replace_with (a colour or swatch) to recolour those places, or change them first",
  "message": "conflict: swatch is in use (frame 00000000-0000-87ea-8000-000000000017 fill); pass replace_with to recolour those places first"
}
```

**Example**

```json
{
  "arguments": {
    "confirm": true,
    "replace_with": "#1E3A5F",
    "swatch": "Brand Cyan"
  },
  "name": "swatch_delete"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000017",
    "00000000-0000-87ea-8000-00000000001d"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 56
}
```

### `scheme_list`

List the twelve built-in colour schemes (main, accent 1-4, hyperlink, followed hyperlink) and which one the document uses.

_read-only_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id; marks which scheme is applied (default: the current document, if any). |

**Result fields:** `schemes`

**Example**

```json
{
  "arguments": {},
  "name": "scheme_list"
}
```

Result:

```json
{
  "schemes": [
    {
      "applied": false,
      "colors": {
        "accent_1": "#C2410C",
        "accent_2": "#FFF7ED",
        "accent_3": "#0F766E",
        "accent_4": "#CA8A04",
        "…": "3 more keys"
      },
      "name": "Lantern Harbor",
      "scheme_id": "lantern_harbor"
    },
    {
      "applied": false,
      "colors": {
        "accent_1": "#D97706",
        "accent_2": "#F4EFE6",
        "accent_3": "#6B8F71",
        "accent_4": "#8C4A2F",
        "…": "3 more keys"
      },
      "name": "Juniper Slate",
      "scheme_id": "juniper_slate"
    },
    {
      "applied": false,
      "colors": {
        "accent_1": "#E0A526",
        "accent_2": "#F6F3DC",
        "accent_3": "#8E5B3A",
        "accent_4": "#4C8BB3",
        "…": "3 more keys"
      },
      "name": "Meadowlark",
      "scheme_id": "meadowlark"
    },
    {
      "applied": false,
      "colors": {
        "accent_1": "#E6862B",
        "accent_2": "#FBEFE0",
        "accent_3": "#4A5A6A",
        "accent_4": "#B8A05A",
        "…": "3 more keys"
      },
      "name": "Copper Dawn",
      "scheme_id": "copper_dawn"
    },
    "… 8 more"
  ]
}
```

### `scheme_apply`

Apply a built-in colour scheme: sets the seven "Scheme: ..." slot swatches, so everything that uses them (the built-in templates do) recolours. One undo step.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `scheme` | string | yes | Scheme id or name (see scheme_list). |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example** — Sets the seven Scheme swatches; everything using them recolours (the built-in templates do).

```json
{
  "arguments": {
    "scheme": "coral_reef"
  },
  "name": "scheme_apply"
}
```

Result:

```json
{
  "affected_ids": [
    "70756272-732d-7363-6865-6d652d736c00",
    "70756272-732d-7363-6865-6d652d736c01",
    "70756272-732d-7363-6865-6d652d736c02",
    "70756272-732d-7363-6865-6d652d736c03",
    "… 3 more"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 55
}
```

## Images

### `image_place`

Put an image (file path or base64; PNG, JPEG or SVG) into a picture frame or image placeholder. Returns asset_id and effective print dpi (aim for 150+).

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `base64` | string |  | Image bytes, base64-encoded. |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `filename` | string |  | Display name for base64 images. |
| `fit` | "fill" \| "fit" \| "stretch" |  | fill (default), fit or stretch. |
| `frame_id` | uuid | yes | An image (picture) frame; create one with frame_add_image. |
| `path` | string |  | PNG, JPEG or SVG file path. Or give base64. |
| `svg` | string |  | SVG markup as text (e.g. "<svg xmlns=..."). Handy for drawn artwork when no photo is available; scales without losing quality. |

**Result fields:** `affected_ids`, `asset_id`, `doc_id`, `effective_dpi`, `frame_id`, `pixel_height`, `pixel_width`, `revision`

**Example**

```json
{
  "arguments": {
    "frame_id": "00000000-0000-87ea-8000-000000000014",
    "svg": "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100… (164 characters)"
  },
  "name": "image_place"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000014",
    "00000000-0000-87ea-8000-00000000001c"
  ],
  "asset_id": "00000000-0000-87ea-8000-00000000001c",
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "frame_id": "00000000-0000-87ea-8000-000000000014",
  "pixel_height": 100,
  "pixel_width": 100,
  "revision": 44
}
```

### `image_crop`

Crop the image in a picture frame: fractions 0..1 of the image trimmed from left, top, right and bottom (e.g. left 0.1 trims 10%).

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `bottom` | number |  | Fraction (0..1) of the image height trimmed from the bottom. |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `frame_id` | uuid | yes | Identifies a frame (text, image, shape or group). |
| `left` | number |  | Fraction (0..1) of the image width trimmed from the left. |
| `right` | number |  | Fraction (0..1) of the image width trimmed from the right. |
| `top` | number |  | Fraction (0..1) of the image height trimmed from the top. |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "frame_id": "00000000-0000-87ea-8000-000000000013",
    "left": 0.1,
    "right": 0.1
  },
  "name": "image_crop"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000013"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 41
}
```

### `image_fit`

Set how the image fills its frame: fill (cover, may crop), fit (all visible) or stretch. resize_frame: true makes the frame match the image's aspect ratio.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `frame_id` | uuid | yes | Identifies a frame (text, image, shape or group). |
| `mode` | "fill" \| "fit" \| "stretch" | yes | fill: cover the frame (may crop); fit: whole image visible; stretch: distort to fill. |
| `resize_frame` | boolean |  | Also resize the frame (keeping its width) to the cropped image's aspect ratio. |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "frame_id": "00000000-0000-87ea-8000-000000000013",
    "mode": "fill"
  },
  "name": "image_fit"
}
```

Result:

```json
{
  "affected_ids": [],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 41
}
```

### `image_replace`

Swap the image in a picture frame for another file/base64 image, keeping the frame and fit mode (crop is reset).

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `base64` | string |  | Image bytes, base64-encoded. |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `filename` | string |  | Display name for base64 images. |
| `fit` | "fill" \| "fit" \| "stretch" |  | fill (default), fit or stretch. |
| `frame_id` | uuid | yes | An image (picture) frame; create one with frame_add_image. |
| `path` | string |  | PNG, JPEG or SVG file path. Or give base64. |
| `svg` | string |  | SVG markup as text (e.g. "<svg xmlns=..."). Handy for drawn artwork when no photo is available; scales without losing quality. |

**Result fields:** `affected_ids`, `asset_id`, `doc_id`, `effective_dpi`, `frame_id`, `pixel_height`, `pixel_width`, `revision`

**Example**

```json
{
  "arguments": {
    "frame_id": "00000000-0000-87ea-8000-000000000013",
    "path": "<assets>/coffee.png"
  },
  "name": "image_replace"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000013",
    "00000000-0000-87ea-8000-00000000001b"
  ],
  "asset_id": "00000000-0000-87ea-8000-00000000001b",
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "effective_dpi": 432.0,
  "frame_id": "00000000-0000-87ea-8000-000000000013",
  "pixel_height": 1200,
  "pixel_width": 1800,
  "revision": 42
}
```

### `image_set_alt_text`

Set the alt text of a picture frame: what the picture shows, for screen readers. It is written to the PDF as /Alt when export_pdf is called with tagged: true. An empty string clears it.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `alt_text` | string | yes | What the picture shows, for screen readers (written to tagged PDF as /Alt). An empty string clears it. |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `frame_id` | uuid | yes | An image (picture) frame. |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example** — Written to the PDF as /Alt when export_pdf is called with tagged: true. An empty string clears it.

```json
{
  "arguments": {
    "alt_text": "Steaming cup of coffee on a wooden table",
    "frame_id": "00000000-0000-87ea-8000-000000000013"
  },
  "name": "image_set_alt_text"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000013"
  ],
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "revision": 43
}
```

## Templates

### `template_list`

List templates (built-in: flyer, trifold_brochure, business_card, newsletter; plus user templates) with page size and every placeholder's name, kind, hint and size.

_read-only_

**Arguments**

_No arguments._

**Result fields:** `templates`

**Example** — Built-in and user templates with their placeholders.

```json
{
  "arguments": {},
  "name": "template_list"
}
```

Result:

```json
{
  "templates": [
    {
      "builtin": true,
      "description": "Letter portrait flyer with a colour header, headline, hero photo, body text, call-to-action box and contact line. 9pt bleed.",
      "name": "flyer",
      "named_frames": [],
      "…": "4 more keys"
    },
    {
      "builtin": true,
      "description": "Letter landscape, two sides, three panels each (folds at 264… (259 characters)",
      "name": "trifold_brochure",
      "named_frames": [
        "inside_body_continued"
      ],
      "…": "4 more keys"
    },
    {
      "builtin": true,
      "description": "3.5 x 2 in business card with logo strip, name, job title, company and contact lines. 9pt bleed.",
      "name": "business_card",
      "named_frames": [],
      "…": "4 more keys"
    },
    {
      "builtin": true,
      "description": "Two-page Letter newsletter: masthead, lead story flowing through two columns onto page 2, lead photo, sidebar, second story in two columns.",
      "name": "newsletter",
      "named_frames": [
        "lead_story_column_2",
        "lead_story_page_2"
      ],
      "…": "4 more keys"
    }
  ]
}
```

### `template_apply`

Create a new document from a template and make it current. Returns placeholder frame ids by semantic name (e.g. headline, cover_image). Fill text placeholders with text_set (keeps the template styling) and image placeholders with image_place.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `name` | string | yes | Template name from template_list, e.g. "trifold_brochure", "flyer", "business_card", "newsletter". |
| `title` | string |  | Title for the new document. |

**Result fields:** `doc_id`, `named_frames`, `page_ids`, `placeholders`, `revision`

**Example**

```json
{
  "arguments": {
    "name": "flyer",
    "title": "Bake Sale"
  },
  "name": "template_apply"
}
```

Result:

```json
{
  "doc_id": "00000000-0000-87ea-8000-000000000022",
  "named_frames": {},
  "page_ids": [
    "00000000-0000-8065-8000-000000000002"
  ],
  "placeholders": [
    {
      "frame_id": "00000000-0000-8065-8000-00000000000c",
      "height": 100.0,
      "hint": "Your Headline Here",
      "kind": "text",
      "…": "4 more keys"
    },
    {
      "frame_id": "00000000-0000-8065-8000-00000000000e",
      "height": 50.0,
      "hint": "A short line that says why this matters",
      "kind": "text",
      "…": "4 more keys"
    },
    {
      "frame_id": "00000000-0000-8065-8000-000000000010",
      "height": 300.0,
      "hint": "Main photo, landscape (about 9:5)",
      "kind": "image",
      "…": "3 more keys"
    },
    {
      "frame_id": "00000000-0000-8065-8000-000000000011",
      "height": 170.0,
      "hint": "Tell readers what is happening, when and where, and what they will get out of it. Two or three short paragraphs work best.",
      "kind": "text",
      "…": "4 more keys"
    },
    "… 2 more"
  ],
  "revision": 0
}
```

### `template_save`

Save the document as a reusable user template. Mark frames as placeholders first with frame_set_placeholder.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `name` | string | yes | Template name: letters, digits, "_" and "-". |

**Result fields:** `name`, `path`, `placeholders`

**Example**

```json
{
  "arguments": {
    "name": "spring_fair"
  },
  "name": "template_save"
}
```

Result:

```json
{
  "name": "spring_fair",
  "path": "<output_dir>/templates/spring_fair.pubrs",
  "placeholders": [
    {
      "height": 90.0,
      "hint": "Event name and date",
      "kind": "text",
      "name": "headline",
      "…": "2 more keys"
    }
  ]
}
```

## Labels

### `sheet_presets_list`

List label, card and postcard sheet presets (Avery US 5160 ..., A4 L7160 ..., generic 2-up/4-up) with the paper size, grid, and the label size the document page must have.

_read-only_

**Arguments**

_No arguments._

**Result fields:** `presets`

**Example**

```json
{
  "arguments": {},
  "name": "sheet_presets_list"
}
```

Result:

```json
{
  "presets": [
    {
      "cols": 3,
      "count": 30,
      "h_pitch": 198.0,
      "id": "avery-5160",
      "…": "9 more keys"
    },
    {
      "cols": 2,
      "count": 20,
      "h_pitch": 301.5,
      "id": "avery-5161",
      "…": "9 more keys"
    },
    {
      "cols": 2,
      "count": 14,
      "h_pitch": 301.5,
      "id": "avery-5162",
      "…": "9 more keys"
    },
    {
      "cols": 2,
      "count": 10,
      "h_pitch": 301.5,
      "id": "avery-5163",
      "…": "9 more keys"
    },
    "… 22 more"
  ]
}
```

### `doc_set_sheet`

Print the document as a label or card sheet: PDF export and print put the page N-up on the sheet. Give a preset id from sheet_presets_list or a custom grid (points); omit both to clear. fill: repeat (the page in every cell, default) or sequence (one page per label, in order). The page size must equal the label size, otherwise the call is refused with a hint.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `custom` | object |  | A custom grid instead of a preset. |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `fill` | "repeat" \| "sequence" |  | How pages fill the cells. |
| `preset` | string |  | Preset id from sheet_presets_list, e.g. "avery-5160". Omit both `preset` and `custom` (or pass null) to clear the sheet. |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example** — Page is 2.625 x 1 in, the Avery 5160 label size, so export_pdf puts it 30-up on a Letter sheet.

```json
{
  "arguments": {
    "preset": "avery-5160"
  },
  "name": "doc_set_sheet"
}
```

Result:

```json
{
  "affected_ids": [],
  "doc_id": "00000000-0000-87ea-8000-00000000002d",
  "revision": 2
}
```

**Example** — A4 L7160 labels are 63.5 x 38.1 mm: the call is refused with a hint to set the page size first.

```json
{
  "arguments": {
    "preset": "l7160"
  },
  "name": "doc_set_sheet"
}
```

Error:

```json
{
  "code": "invalid_argument",
  "hint": "check the argument ranges in the tool's input schema",
  "message": "invalid argument: the label is 2.500 × 1.500 in but the page is 2.625 × 1.000 in; set the page size to 2.500 × 1.500 in first"
}
```

### `doc_set_booklet`

Print the document as a saddle-stitched booklet: pages are padded to a multiple of 4, reordered and put 2-up on sheets twice the page width (front and back are separate PDF pages). Not combinable with a label sheet.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `booklet` | boolean | yes |  |
| `doc_id` | string |  | Document id (default: the most recently used document). |

**Result fields:** `affected_ids`, `doc_id`, `revision`

**Example**

```json
{
  "arguments": {
    "booklet": true
  },
  "name": "doc_set_booklet"
}
```

Result:

```json
{
  "affected_ids": [],
  "doc_id": "00000000-0000-87ea-8000-00000000002d",
  "revision": 5
}
```

## Export

### `export_pdf`

Export the whole document as a print-ready PDF (embedded subset fonts, TrimBox/BleedBox set). Options: bleed, crop_marks, font_embedding subset|outlines, impose (default true: documents with a label sheet or booklet are imposed), standard none|pdfx4|pdfa2b (pdfx4 = commercial printing: PDF 1.6, CMYK output intent, checked before it is returned; pdfa2b = archive), output_intent_icc (path of a CMYK output profile for pdfx4), tagged (tagged PDF with image alt text). CMYK and spot swatches are written as DeviceCMYK and Separation colours. Writes to path, or returns base64 when no path is given.

_read-only_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `bleed` | boolean |  | Include the document's bleed (default: true when the document has bleed). |
| `crop_marks` | boolean |  | Add crop marks outside the bleed (default false). |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `font_embedding` | "subset" \| "outlines" |  | Font handling in PDFs. |
| `impose` | boolean |  | Impose the pages onto label sheets or booklet sheets when the document has a sheet or booklet set (default true). false exports the plain pages. |
| `output_intent_icc` | string |  | pdfx4 only: path of a CMYK output profile (.icc, class prtr) to use instead of the bundled generic CMYK (SWOP-like) profile. |
| `path` | string |  | Output file path. Without a path the PDF is returned as base64. |
| `return_base64` | boolean |  | Also return the bytes as base64 when a path is given. |
| `standard` | "none" \| "pdfx4" \| "pdfa2b" |  | none (default), pdfx4 (commercial printing: PDF 1.6 with a CMYK output intent, checked before it is returned) or pdfa2b (archive). |
| `tagged` | boolean |  | Write a tagged PDF: text as paragraphs, pictures as figures with their alt text (set with image_set_alt_text), the rest as artifacts. |

**Result fields:** `base64`, `bytes`, `doc_id`, `imposed`, `page_count`, `path`, `revision`

**Example**

```json
{
  "arguments": {
    "crop_marks": true,
    "path": "spring-fair.pdf"
  },
  "name": "export_pdf"
}
```

Result:

```json
{
  "bytes": 94850,
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "page_count": 2,
  "path": "<output_dir>/spring-fair.pdf",
  "revision": 67
}
```

**Example** — Commercial printing: PDF/X-4 (PDF 1.6, CMYK output intent, TrimBox and BleedBox). The result is checked before it is returned; a failed check is an export error that names the issues.

```json
{
  "arguments": {
    "crop_marks": true,
    "path": "spring-fair-x4.pdf",
    "standard": "pdfx4"
  },
  "name": "export_pdf"
}
```

Result:

```json
{
  "bytes": 514441,
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "page_count": 2,
  "path": "<output_dir>/spring-fair-x4.pdf",
  "revision": 67
}
```

### `export_print_package`

Save for a commercial printer: a zip with the PDF/X-4 file, the .pubrs document with its images, and a README listing the fonts used and the colours (swatches with their CMYK and spot values). Options: bleed, crop_marks, output_intent_icc. Writes to path, or returns base64 when no path is given.

_read-only_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `bleed` | boolean |  | Include the document's bleed (default: true when the document has bleed). |
| `crop_marks` | boolean |  | Add crop marks outside the bleed (default false). |
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `output_intent_icc` | string |  | Path of a CMYK output profile to use instead of the bundled one. |
| `path` | string |  | Output .zip path. Without a path the zip is returned as base64. |

**Result fields:** `base64`, `bytes`, `doc_id`, `files`, `path`, `revision`

**Example** — Save for a commercial printer: a zip with the PDF/X-4 file, the .pubrs with its images and a README listing the fonts and colours.

```json
{
  "arguments": {
    "crop_marks": true,
    "path": "spring-fair-print.zip"
  },
  "name": "export_print_package"
}
```

Result:

```json
{
  "bytes": 668612,
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "files": [
    "spring-fair-print-pdfx4.pdf",
    "spring-fair-print.pubrs",
    "README.txt"
  ],
  "path": "<output_dir>/spring-fair-print.zip",
  "revision": 67
}
```

### `export_png`

Render one page to PNG at a dpi (default 150), optionally including bleed. Writes to path or returns base64.

_read-only_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `dpi` | number |  | Resolution in dots per inch (default 150). |
| `include_bleed` | boolean |  | Include the bleed area. |
| `page_id` | string |  | Page to render (default: first page). |
| `path` | string |  | Output file path. Without a path the PNG is returned as base64. |
| `return_base64` | boolean |  |  |

**Result fields:** `base64`, `bytes`, `doc_id`, `height`, `page_id`, `path`, `revision`, `width`

**Example**

```json
{
  "arguments": {
    "dpi": 72,
    "page_id": "00000000-0000-87ea-8000-000000000002",
    "path": "spring-fair.png"
  },
  "name": "export_png"
}
```

Result:

```json
{
  "bytes": 23361,
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "height": 842,
  "page_id": "00000000-0000-87ea-8000-000000000002",
  "path": "<output_dir>/spring-fair.png",
  "revision": 67,
  "width": 596
}
```

### `render_preview`

Render a low-resolution PNG preview of a page (default 60 dpi) so you can look at your work. Returned as an image.

_read-only, returns an image_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |
| `dpi` | number |  | Resolution (default 60 dpi, max 150) — low so the image is cheap to view. |
| `page_id` | string |  | Page to look at (default: first page). |

**Result fields:** `doc_id`, `height`, `mime_type`, `page_id`, `png_base64`, `revision`, `width`

**Example**

```json
{
  "arguments": {
    "dpi": 30,
    "page_id": "00000000-0000-87ea-8000-000000000002"
  },
  "name": "render_preview"
}
```

Result:

```json
{
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "height": 351,
  "mime_type": "image/png",
  "page_id": "00000000-0000-87ea-8000-000000000002",
  "png_base64": "iVBORw0KGgoAAAANSUhEUgAAAPkAAAFfCAYAAACMfWrXAAAf3ElEQVR4nO2d… (10952 characters)",
  "revision": 67,
  "width": 249
}
```

## History

### `undo`

Undo the last change to the document. Every tool call that changed the document is one step.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |

**Result fields:** `affected_ids`, `can_redo`, `can_undo`, `doc_id`, `done`, `label`, `revision`

**Example**

```json
{
  "arguments": {},
  "name": "undo"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000002",
    "00000000-0000-87ea-8000-000000000020"
  ],
  "can_redo": true,
  "can_undo": true,
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "done": true,
  "label": "delete frame",
  "revision": 66
}
```

### `redo`

Redo the last undone change.

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |

**Result fields:** `affected_ids`, `can_redo`, `can_undo`, `doc_id`, `done`, `label`, `revision`

**Example**

```json
{
  "arguments": {},
  "name": "redo"
}
```

Result:

```json
{
  "affected_ids": [
    "00000000-0000-87ea-8000-000000000002",
    "00000000-0000-87ea-8000-000000000020"
  ],
  "can_redo": false,
  "can_undo": true,
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "done": true,
  "label": "delete frame",
  "revision": 67
}
```

### `history_list`

List undo history, oldest first; undone entries can be redone.

_read-only_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |

**Result fields:** `doc_id`, `entries`, `revision`

**Example**

```json
{
  "arguments": {},
  "name": "history_list"
}
```

Result:

```json
{
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "entries": [
    {
      "label": "set page size",
      "name": "set_page_size",
      "undone": false
    },
    {
      "label": "set margins",
      "name": "set_margins",
      "undone": false
    },
    {
      "label": "set meta",
      "name": "set_meta",
      "undone": false
    },
    {
      "label": "add page",
      "name": "add_page",
      "undone": false
    },
    "… 60 more"
  ],
  "revision": 65
}
```

## Query

### `doc_to_json`

Return the complete document model as JSON (pages, frames, stories, styles, asset metadata). This is the document.json stored in .pubrs files.

_read-only_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |

**Result fields:** `doc_id`, `document`, `revision`

**Example**

```json
{
  "arguments": {},
  "name": "doc_to_json"
}
```

Result:

```json
{
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "document": {
    "assets": {
      "00000000-0000-87ea-8000-000000000012": {
        "format": "jpeg",
        "id": "00000000-0000-87ea-8000-000000000012",
        "name": "bread.jpg",
        "pixel_height": 1200,
        "…": "1 more keys"
      },
      "00000000-0000-87ea-8000-00000000001b": {
        "format": "png",
        "id": "00000000-0000-87ea-8000-00000000001b",
        "name": "coffee.png",
        "pixel_height": 1200,
        "…": "1 more keys"
      },
      "00000000-0000-87ea-8000-00000000001c": {
        "format": "svg",
        "id": "00000000-0000-87ea-8000-00000000001c",
        "name": "artwork.svg",
        "pixel_height": 100,
        "…": "1 more keys"
      }
    },
    "frames": {
      "00000000-0000-87ea-8000-00000000000d": {
        "content": {
          "shape": {
            "direction": "down_right",
            "kind": "line"
          },
          "type": "shape"
        },
        "id": "00000000-0000-87ea-8000-00000000000d",
        "locked": false,
        "rect": {
          "height": 0.0,
          "width": 523.0,
          "x": 36.0,
          "y": 800.0
        },
        "…": "3 more keys"
      },
      "00000000-0000-87ea-8000-00000000000f": {
        "content": {
          "column_gap": 12.0,
          "columns": 1,
          "insets": {
            "bottom": 4.0,
            "left": 4.0,
            "right": 4.0,
            "top": 4.0
          },
          "story": "00000000-0000-87ea-8000-000000000010",
          "…": "2 more keys"
        },
        "id": "00000000-0000-87ea-8000-00000000000f",
        "locked": false,
        "placeholder": {
          "hint": "Event name and date",
          "kind": "text",
          "name": "headline"
        },
        "…": "4 more keys"
      },
      "00000000-0000-87ea-8000-000000000011": {
        "content": {
          "shape": {
            "corner_radius": 0.0,
            "kind": "rectangle"
          },
          "type": "shape"
        },
        "id": "00000000-0000-87ea-8000-000000000011",
        "locked": false,
        "rect": {
          "height": 4.0,
          "width": 523.0,
          "x": 36.0,
          "y": 134.0
        },
        "…": "3 more keys"
      },
      "00000000-0000-87ea-8000-000000000013": {
        "content": {
          "alt_text": "Steaming cup of coffee on a wooden table",
          "asset": "00000000-0000-87ea-8000-00000000001b",
          "crop": {
            "bottom": 0.0,
            "left": 0.0,
            "right": 0.0,
            "top": 0.0
          },
          "fit": "fill",
          "…": "2 more keys"
        },
        "id": "00000000-0000-87ea-8000-000000000013",
        "locked": false,
        "rect": {
          "height": 193.33333333333334,
          "width": 290.0,
          "x": 36.0,
          "y": 143.30160786160704
        },
        "…": "3 more keys"
      },
      "…": "3 more keys"
    },
    "grid": {
      "show_grid": false,
      "snap_to_grid": false,
      "snap_to_guides": true,
      "snap_tolerance": 6.0,
      "…": "1 more keys"
    },
    "id": "00000000-0000-87ea-8000-000000000001",
    "masters": [
      {
        "frames": [
          "00000000-0000-87ea-8000-00000000000d"
        ],
        "guides": [],
        "id": "00000000-0000-87ea-8000-00000000000c",
        "name": "Footer"
      }
    ],
    "meta": {
      "author": "Parish Council",
      "title": "Spring Fair"
    },
    "pages": [
      {
        "frames": [
          "00000000-0000-87ea-8000-00000000000f",
          "00000000-0000-87ea-8000-000000000011",
          "… 4 more"
        ],
        "guides": [],
        "id": "00000000-0000-87ea-8000-000000000002",
        "master": "00000000-0000-87ea-8000-00000000000c"
      },
      {
        "frames": [],
        "guides": [],
        "id": "00000000-0000-87ea-8000-00000000000b",
        "master": "00000000-0000-87ea-8000-00000000000c"
      }
    ],
    "schema_version": 7,
    "scheme": "coral_reef",
    "setup": {
      "bleed": 9.0,
      "margins": {
        "bottom": 54.0,
        "left": 36.0,
        "right": 36.0,
        "top": 36.0
      },
      "size": {
        "height": 841.8897637795277,
        "width": 595.2755905511812
      }
    },
    "stories": {
      "00000000-0000-87ea-8000-000000000010": {
        "frames": [
          "00000000-0000-87ea-8000-00000000000f"
        ],
        "id": "00000000-0000-87ea-8000-000000000010",
        "paragraphs": [
          {
            "runs": [
              {
                "color": "#C2410C",
                "font_size": 40.0,
                "text": "Spring Fair"
              }
            ],
            "style": "00000000-0000-87ea-8000-000000000005"
          },
          {
            "runs": [
              {
                "color": "#C2410C",
                "font_size": 40.0,
                "text": "Saturday 12 May · "
              },
              {
                "bold": true,
                "color": "#C2410C",
                "font_size": 40.0,
                "text": "Village Green"
              }
            ],
            "style": "00000000-0000-87ea-8000-000000000005"
          }
        ]
      },
      "00000000-0000-87ea-8000-000000000016": {
        "frames": [
          "00000000-0000-87ea-8000-000000000015"
        ],
        "id": "00000000-0000-87ea-8000-000000000016",
        "paragraphs": [
          {
            "align": "justify",
            "drop_cap": {
              "chars": 1,
              "lines": 3
            },
            "hyphenate": true,
            "justify_limits": {
              "letter_spacing": 0.05,
              "max_word_space": 1.33
            },
            "…": "9 more keys"
          },
          {
            "align": "justify",
            "hyphenate": true,
            "justify_limits": {
              "letter_spacing": 0.05,
              "max_word_space": 1.33
            },
            "keep_together": true,
            "…": "5 more keys"
          }
        ]
      },
      "00000000-0000-87ea-8000-000000000019": {
        "frames": [
          "00000000-0000-87ea-8000-000000000017"
        ],
        "id": "00000000-0000-87ea-8000-000000000019",
        "paragraphs": [
          {
            "align": "justify",
            "drop_cap": {
              "chars": 1,
              "lines": 3
            },
            "hyphenate": true,
            "justify_limits": {
              "letter_spacing": 0.05,
              "max_word_space": 1.33
            },
            "…": "9 more keys"
          }
        ]
      }
    },
    "styles": {
      "default_paragraph": "00000000-0000-87ea-8000-000000000003",
      "styles": [
        {
          "character": {
            "bold": false,
            "color": "#000000",
            "font_family": "Liberation Sans",
            "font_size": 11.0,
            "…": "3 more keys"
          },
          "id": "00000000-0000-87ea-8000-000000000003",
          "kind": "paragraph",
          "name": "Normal",
          "…": "1 more keys"
        },
        {
          "based_on": "00000000-0000-87ea-8000-000000000003",
          "character": {
            "bold": true,
            "font_size": 36.0
          },
          "id": "00000000-0000-87ea-8000-000000000004",
          "kind": "paragraph",
          "…": "2 more keys"
        },
        "… 5 more"
      ]
    },
    "swatches": [
      {
        "id": "00000000-0000-87ea-8000-00000000001e",
        "name": "Brand Red",
        "value": {
          "spot": {
            "cmyk": {
              "c": 0.0,
              "k": 4.0,
              "m": 100.0,
              "y": 81.0
            },
            "name": "PANTONE 186 C"
          }
        }
      },
      {
        "id": "70756272-732d-7363-6865-6d652d736c00",
        "name": "Scheme: Main",
        "value": {
          "rgb": "#0F5E73"
        }
      },
      "… 6 more"
    ]
  },
  "revision": 67
}
```

### `validate`

Check print readiness. Warnings: overflow, missing_font, low_resolution_image (<150 dpi), off_page, beyond_bleed, empty_placeholder, empty_image_frame — each with a hint.

_read-only_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |

**Result fields:** `doc_id`, `ok`, `revision`, `warnings`

**Example**

```json
{
  "arguments": {},
  "name": "validate"
}
```

Result:

```json
{
  "doc_id": "00000000-0000-87ea-8000-00000000000a",
  "ok": false,
  "revision": 67,
  "warnings": [
    {
      "code": "overflow",
      "frame": "00000000-0000-87ea-8000-00000000000f",
      "hint": "enlarge the frame, reduce the text or font size, or link another frame with text_link_frames",
      "message": "31 characters do not fit in the text frame chain",
      "…": "1 more keys"
    },
    {
      "code": "overflow",
      "frame": "00000000-0000-87ea-8000-000000000015",
      "hint": "enlarge the frame, reduce the text or font size, or link another frame with text_link_frames",
      "message": "0 characters do not fit in the text frame chain",
      "…": "1 more keys"
    }
  ]
}
```

### `fonts_list`

List available font families (bundled ones are always available and embed in PDFs).

_read-only_

**Arguments**

| Field | Type | Required | Description |
|---|---|---|---|
| `doc_id` | string |  | Document id (default: the most recently used document). |

**Result fields:** `fallback`, `families`

**Example** — Families every document can use without warnings.

```json
{
  "arguments": {},
  "name": "fonts_list"
}
```

Result:

```json
{
  "fallback": "Liberation Sans",
  "families": [
    {
      "family": "Liberation Sans",
      "source": "added"
    },
    {
      "family": "Liberation Serif",
      "source": "added"
    }
  ]
}
```

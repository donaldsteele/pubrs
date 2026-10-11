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
| `make_newsletter` | `topic`, `audience`, `details`, `images` | Four-page newsletter from `newsletter`: placeholders, the two tables (`table_set_cell_text`), the bulleted and numbered lists, images with alt text. |
| `make_greeting_card` | `occasion`, `recipient`, `message`, `images` | Quarter-fold card from `greeting_card`, including its rotated inside spread. |
| `make_mail_merge` | `piece`, `data`, `sheet`, `details` | Merge a CSV or XLSX list into a letter or card: `merge_set_source`, fields, preview, one page per record. |
| `make_labels` | `data`, `preset`, `sheet`, `lines` | Address labels from a list on a label sheet, one record per label. |

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

# Using pubrs from Claude

`pubrs-mcp` is installed next to the pubrs app (see USER_GUIDE.md). The
configs here assume it is on your `PATH`; otherwise put the full path in
`command`:

| OS | Installed path |
|---|---|
| Windows | `C:\\Program Files\\pubrs\\bin\\pubrs-mcp.exe` |
| macOS | `/Applications/pubrs.app/Contents/MacOS/pubrs-mcp` |
| Linux (.deb) | `/usr/bin/pubrs-mcp` |
| Linux (AppImage) | `pubrs-x86_64.AppImage --mcp` (or extract the AppImage) |

Files the assistant saves without a full path go to `Documents/pubrs`
(change with `"args": ["--output-dir", "/some/folder"]`).

## Claude Desktop

Merge `claude_desktop_config.json` into Claude Desktop's config file and
restart Claude Desktop:

- macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`
- Windows: `%APPDATA%\Claude\claude_desktop_config.json`

Then ask, for example: *"Make me a trifold brochure for a bakery."* Claude
uses the `make_trifold` workflow: it applies the template, writes the copy,
draws artwork if you have no photos, checks overflow and validation, looks at
previews and exports a print-ready PDF.

## Claude Code

Either run `claude mcp add pubrs -- pubrs-mcp`, or copy `.mcp.json` into
your project folder. For a long-running shared server use HTTP:

```
pubrs-mcp --http 127.0.0.1:8765
```

and `claude_code_http.mcp.json` as the project's `.mcp.json`.

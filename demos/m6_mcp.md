# M6: MCP server

What works:

- `pubrs-mcp` (rmcp 3.5): 71 tools mapped 1:1 onto the `pubrs-api` registry with generated JSON Schemas
  (input and output), annotations, structured results, image content for previews and `{code,message,hint}`
  errors; resources and three guided prompts; stdio and streamable HTTP transports.
- Four built-in templates (flyer, trifold brochure, business card, newsletter) plus user templates.
- Acceptance test: in-process rmcp client applies the trifold template, fills all placeholders, places two
  images, links frames, overflow clean, validate with zero warnings, exports a PDF, saves, reopens, and
  `doc_to_json` is identical.
- MCP.md generated from the schemas and 77 examples that the integration test executes.

See it:

```
cargo test -p pubrs-mcp
cargo run -p pubrs-mcp -- --http 127.0.0.1:8765        # streamable HTTP endpoint at /mcp
PUBRS_WRITE_DOCS=1 cargo test -p pubrs-mcp --test examples   # regenerate MCP.md
```

Claude Desktop/Code configs: `examples/mcp/`.

# PROGRESS

Spec: `PLAN.md` (CLAUDE.md calls it PROMPT.md; same document). Read this file first.

| Milestone | Status | Notes |
|-----------|--------|-------|
| M0 Workspace skeleton, CI, command system + undo | done locally; CI blocked (billing) | 36 core tests; GUI smoke launch passes on Linux/xvfb |
| M1 Core document model + file format round-trip | done locally; CI blocked (billing) | +7 core, +11 io tests; Windows/macOS cross clippy clean |
| M2 Layout: shaping, linked chains, overflow | done locally; CI blocked (billing) | +15 layout tests incl. golden snapshot |
| M3 Render to PNG, golden images | done locally; CI blocked (billing) | +5 golden render tests, GPU vs CPU test (feature gpu) |
| M4 PDF export via krilla | done locally; CI blocked (billing) | +8 PDF tests (lopdf parse-back, hayro raster comparison) |
| M5 pubrs-api Session + CLI | in progress | |
| M6 MCP server | pending | |
| M7 GUI | pending | |
| M8 Templates, printing, installers, docs, DoD | pending | |

**Next step:** M5: `pubrs-api` Session (open docs keyed by doc_id, Editor per doc, ExportContext), typed request/response structs with schemars, tool registry (name → schema → handler) covering every MCP tool, structured errors {code,message,hint}; `pubrs` CLI (render, export, validate, script) with a script replay test.

## Verification per milestone

- Linux (this machine): `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `xvfb-run -a cargo run -p pubrs-app -- --smoke-test`.
- Windows/macOS: `scripts/cross-check.sh` (compile + clippy only) until CI runs.

## Known failing tests

None.

## Open questions

- **CI cannot start.** The repo `donaldsteele/pubrs` was created private (no remote existed). GitHub refuses to start
  Actions jobs: "recent account payments have failed or your spending limit needs to be increased". Fix by updating
  billing or making the repo public (public repos get free standard runners). Until then Windows/macOS are only
  compile-checked locally.
- Pushing uses SSH (`git remote set-url --push origin git@github.com:...`) because the gh OAuth token lacks the
  `workflow` scope needed to push `.github/workflows/*`.

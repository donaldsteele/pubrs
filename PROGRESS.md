# PROGRESS

Spec: `PLAN.md` (CLAUDE.md calls it PROMPT.md; same document). Read this file first.

| Milestone | Status | Notes |
|-----------|--------|-------|
| M0 Workspace skeleton, CI, command system + undo | done locally; CI blocked (billing) | 36 core tests; GUI smoke launch passes on Linux/xvfb |
| M1 Core document model + file format round-trip | done locally; CI blocked (billing) | +7 core, +11 io tests; Windows/macOS cross clippy clean |
| M2 Layout: shaping, linked chains, overflow | in progress | |
| M3 Render to PNG, golden images | pending | |
| M4 PDF export via krilla | pending | |
| M5 pubrs-api Session + CLI | pending | |
| M6 MCP server | pending | |
| M7 GUI | pending | |
| M8 Templates, printing, installers, docs, DoD | pending | |

**Next step:** M2: `pubrs-layout` FontLibrary (bundled Liberation fonts, lazy system fonts via fontdb) and per-paragraph parley layout flowing through chain regions (frames × columns) with wrap exclusions and overflow; layout golden tests from `tests/fonts/`.

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

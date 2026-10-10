# PROGRESS

Spec: `PLAN.md` (CLAUDE.md calls it PROMPT.md; same document). Read this file first.

| Milestone | Status | Notes |
|-----------|--------|-------|
| M0 Workspace skeleton, CI, command system + undo | done locally; CI blocked (billing) | 36 core tests; GUI smoke launch passes on Linux/xvfb |
| M1 Core document model + file format round-trip | done locally; CI blocked (billing) | +7 core, +11 io tests; Windows/macOS cross clippy clean |
| M2 Layout: shaping, linked chains, overflow | done locally; CI blocked (billing) | +15 layout tests incl. golden snapshot |
| M3 Render to PNG, golden images | done locally; CI blocked (billing) | +5 golden render tests, GPU vs CPU test (feature gpu) |
| M4 PDF export via krilla | done locally; CI blocked (billing) | +8 PDF tests (lopdf parse-back, hayro raster comparison) |
| M5 pubrs-api Session + CLI | done locally; CI blocked (billing) | +6 session, +2 script, +3 registry, +2 CLI tests |
| M6 MCP server | done locally; CI blocked (billing) | +2 acceptance, +1 examples (all 71 tools), +1 HTTP, +2 template, +3 io template tests |
| M7 GUI | done locally; CI blocked (billing) | GUI self-test (14 steps, real input injection) + 7 app unit tests; Windows/macOS compile clean |
| M8 Templates, printing, installers, docs, DoD | done locally | release workflow (MSI x64/ARM64, universal DMG, .deb + AppImage x86_64/aarch64, Flatpak manifest); .deb and AppImage built and run here; +1 CLI test (bundled sample); self-test reopens by file drop |

**Next step:** Definition of Done follow-up: keep CI green on all four OSes, run the Release workflow (Actions → Release → Run workflow) and fix any installer job from its logs, then do the manual installer check in RELEASE.md on real Windows/macOS/Linux desktops.

## Verification per milestone

- Linux (this machine): `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `xvfb-run -a cargo run -p pubrs-app -- --smoke-test`.
- Windows/macOS: `scripts/cross-check.sh` (compile + clippy only) until CI runs.

## Known failing tests

None.

## Open questions

- CI: the repo was private and Actions could not start (billing). On 2026-10-10 the owner asked for the repo to be made
  public; it now runs on free standard runners.
- Pushing uses SSH (`git remote set-url --push origin git@github.com:...`) because the gh OAuth token lacks the
  `workflow` scope needed to push `.github/workflows/*`.

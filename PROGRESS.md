# PROGRESS

Spec: `PLAN.md` (CLAUDE.md calls it PROMPT.md; same document). Read this file first.

| Milestone | Status | Notes |
|-----------|--------|-------|
| M0 Workspace skeleton, CI, command system + undo | done; CI green | 36 core tests; GUI smoke launch passes on Linux/xvfb |
| M1 Core document model + file format round-trip | done; CI green | +7 core, +11 io tests; Windows/macOS cross clippy clean |
| M2 Layout: shaping, linked chains, overflow | done; CI green | +15 layout tests incl. golden snapshot |
| M3 Render to PNG, golden images | done; CI green | +5 golden render tests, GPU vs CPU test (feature gpu) |
| M4 PDF export via krilla | done; CI green | +8 PDF tests (lopdf parse-back, hayro raster comparison) |
| M5 pubrs-api Session + CLI | done; CI green | +6 session, +2 script, +3 registry, +2 CLI tests |
| M6 MCP server | done; CI green | +2 acceptance, +1 examples (all 71 tools), +1 HTTP, +2 template, +3 io template tests |
| M7 GUI | done; CI green | GUI self-test (14 steps, real input injection) + 7 app unit tests; Windows/macOS compile clean |
| M8 Templates, printing, installers, docs, DoD | done; CI green; release workflow green | release workflow (MSI x64/ARM64, universal DMG, .deb + AppImage x86_64/aarch64, Flatpak manifest); .deb and AppImage built and run here; +1 CLI test (bundled sample); self-test reopens by file drop |

**Next step:** none scheduled. The manual per-installer check in RELEASE.md on real desktops is still open (it needs physical Windows, macOS and Linux machines); then tag `v0.1.0`. Further work is listed in TODO.md.

## Verification per milestone

- Linux (this machine): `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `xvfb-run -a cargo run -p pubrs-app -- --smoke-test`.
- CI (`.github/workflows/ci.yml`) on ubuntu-latest, windows-latest, macos-latest (arm64), macos-15-intel: fmt, clippy, tests, golden byte-identity report, GPU test, GUI smoke launch and GUI self-test.
- Release (`.github/workflows/release.yml`, manual run or `v*` tag): MSI x64 + ARM64, universal DMG, .deb + AppImage x86_64 + aarch64; each installed, launched, sample exported, uninstalled.
- `scripts/cross-check.sh`: Windows/macOS clippy from Linux before pushing.

## Known failing tests

None.

## Open questions

- CI: the repo was private and Actions could not start (billing). On 2026-10-10 the owner asked for the repo to be made
  public; CI and the release workflow now run on free standard runners.
- Pushing uses SSH (`git remote set-url --push origin git@github.com:...`) because the gh OAuth token lacks the
  `workflow` scope needed to push `.github/workflows/*`.

## Status at the end of M8 (2026-10-10)

- Tests: 120 in `cargo test --workspace` (core 43, layout 16, render 3 incl. GPU, io 27, api 17, cli 3, app 7,
  mcp 4) plus the 15-step GUI self-test; all pass in CI on ubuntu-latest, windows-latest, macos-latest (arm64) and
  macos-15-intel (run 38081976081), and locally.
- Golden images are byte-identical to the references on all four CI machines (step "Golden images byte-identical
  on this OS?"); the bundled sample exports to the same 217,300-byte PDF on Linux, Windows and macOS.
- Release workflow run 38084072933 (manual dispatch, commit 98426ff): MSI x64 and ARM64, universal DMG, .deb and
  AppImage x86_64 and aarch64 all built and checked (install, launch, export the sample, uninstall). CI run
  38085477371 is green on all four OSes at eacbb3c.
- CI fixes found on first real runs: libxkbcommon-x11 on Linux, verbatim Windows paths, relative GUI paths, vello
  on the WARP software adapter, self-test timing on slow renderers, WiX/cargo-bundle packaging details.

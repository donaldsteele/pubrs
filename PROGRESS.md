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

**Next step:** Phase 2, M14 (T6) Hyphenation and bounded justification, per the plan on branch `plan/publisher-parity`.


## Phase 2: Publisher parity

Plan: `~/.claude/plans/pubrs-publisher-parity.md`, branch `plan/publisher-parity`.

| Milestone | Status | Notes |
|-----------|--------|-------|
| M9 Performance harness in the tree | done; CI green (run 38110112006) | `pubrs perf` (crates/pubrs-api/src/perf.rs), informational budgets, CI table on Linux; +1 CLI test (129 tests total, was 128) |
| M10 Robustness and guard rails | done; CI green (run 38110596754) | panic rollback (`CommandError::Internal`), proptest suites (undo/redo, save/load, loader fuzz), wrap around rotated text frames fixed, workspace lints (`unsafe_code` deny, `unwrap_used`, `dbg_macro`, `todo`, `undocumented_unsafe_blocks`), crate layering test; +12 tests (141 tests total, was 129) |
| M11 Incremental layout | done; CI green (run 38112507559) | `StoryLayoutCache` (per-story snapshot keys compared by equality, ADR 0012), `PUBRS_LAYOUT_VERIFY=1`, carets from one cluster-offset table per paragraph (bit-identical to parley), no whole-document clones in export or the canvas, budgets keyed by page count and enforced (CI: `perf --pages 50 --check`); keystroke layout 8.54 ms at 200 pages (budget 20); +7 layout tests (148 tests total, was 141) |
| M12 Commands cost what they change | done | frames, stories, pages and masters shared by `Arc` (serde `rc`, JSON unchanged, pinned by a golden patch corpus `crates/pubrs-core/tests/fixtures/patch_corpus.json` blessed before the refactor), `Patch::diff_pair` and `compose_pair` (one diff per command, coalesced typing included), shallow `check()`, layout keys compare stories by `Arc::ptr_eq`; keystroke command 0.12 ms at 50 pages (was 0.54) and 0.23 ms at 200 (was 2.13), enforced 2 ms at 50 and 0.5 ms at 200; +1 core test, plus a sharing property in the undo/redo proptest (149 tests total, was 148) |
| M13 Viewport rendering off the UI thread | done | render worker thread (newest request wins) draws the canvas from `Arc` page snapshots, never touching `Session`; viewport-sized textures at device resolution (no 4,096 px cap, sharp at 800%), previous texture shown scaled until the new one lands; damage repaint from `Change.touched` and old/new layouts (`pubrs-render/src/damage.rs`) uploaded with `set_partial`; parallel tiles byte-identical to the untiled render (`pubrs-render/src/tiles.rs`, every golden page tiled at 1/2/7/16 threads and odd tile sizes); image mips for the viewport only; thumbnails on the worker; vello leaves the GUI (ADR 0013); viewport at 100% 6.4 ms at 200 pages (budget 16) and 6.3 ms at 50 (CI budget 40), keystroke total 8.4 ms at 200 pages (budget 30, was 31.4) and 7.8 ms at 50 (CI budget 60); self-test checks 800% sharpness and typing frame times; +25 tests (194 tests total; was 169 after the M31 part-1 fast-forward b1d609d, 149 at M12) |
| M23 Label and card sheets, and booklets | done | 26 sheet presets as data (`pubrs-io/src/presets.rs`); `PageSetup.sheet`/`booklet` with `SetSheet`/`SetBooklet` (schema 2, identity migration, `tests/fixtures/schema_v1`/`v2`, LD2 bakery test and golden); `pubrs-render/src/impose.rs` (repeat/sequence N-up with per-cell clip, saddle-stitch booklets); PDF export and printing impose by default (`impose: false` opts out); MCP `sheet_presets_list`, `doc_set_sheet`, `doc_set_booklet`; Page Setup dialog with sheet preview, GUI self-test steps; built on a parallel branch and rebased onto M13; 215 tests total (was 194) |

### Baseline performance (M9)

Release build, software raster, 16-core Linux box, median ms. `pubrs perf --pages N --runs R` (16: R=3; 50 and 200: R=5).

| Measurement | 16 pages | 50 pages | 200 pages |
|-------------|---------:|---------:|----------:|
| build document | 178.98 | 296.39 | 2499.63 |
| layout (warm) | 46.39 | 147.92 | 598.17 |
| render page at 100% (144 dpi) | 23.08 | 23.82 | 24.09 |
| keystroke: command | 0.20 | 0.56 | 2.37 |
| keystroke: layout | 47.14 | 146.70 | 594.46 |
| keystroke: render | 23.25 | 23.89 | 24.17 |
| keystroke: total | 70.54 | 171.13 | 621.01 |
| move frame | 0.08 | 0.20 | 0.94 |
| undo | 0.00 | 0.01 | 0.05 |
| save | 3.54 | 7.89 | 28.49 |
| open | 1.47 | 3.56 | 15.57 |
| PDF export | 441.53 | 559.58 | 1071.18 |

### After M11 (incremental layout)

Same machine and command, `--runs 5`. Full table and before/after: `demos/m11_incremental_layout.md`.

| Measurement | 16 pages | 50 pages | 200 pages |
|-------------|---------:|---------:|----------:|
| layout (warm, all stories) | 21.27 | 68.07 | 282.78 |
| keystroke: command | 0.19 | 0.51 | 2.25 |
| keystroke: layout | 6.83 | 7.23 | 8.54 |
| of which cache keys | 0.10 | 0.32 | 1.41 |
| keystroke: render | 22.74 | 23.59 | 23.16 |
| keystroke: total | 29.77 | 31.32 | 33.97 |
| PDF export | 412.14 | 477.31 | 774.00 |

Layout per keystroke at 200 pages on the development machine: 8.54 ms (budget 20 ms, enforced by `perf --pages 200 --check`). At 50 pages: layout 7.23 ms (budget 25) and total 31.32 ms (budget 60), enforced in CI.

### After M13 (viewport rendering off the UI thread)

Same machine and command, `--runs 5`. Full table: `demos/m13_viewport_rendering.md`.

| Measurement | 50 pages | 200 pages |
|-------------|---------:|----------:|
| viewport 2880x1800 at 100% on HiDPI (8 tile threads) | 6.26 | 6.41 |
| keystroke: command | 0.06 | 0.15 |
| keystroke: layout | 6.94 | 7.55 |
| keystroke: damage repaint (tiled, off the UI thread) | 0.70 | 0.70 |
| keystroke: total | 7.80 | 8.39 |

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

## After 0.1.0: high CPU on Windows (2026-10-11)

- Report: about 80% CPU on Windows. Reproduced on the windows-latest runner, which has no GPU. While the cursor
  moves, egui draws on WARP at about 5 fps and 0.5 CPU-seconds per frame, using 76% of the machine. Idle is 0%.
  Measurements and the options tried are in ADR 0011.
- Fix: the GUI adapter is chosen across Vulkan/Metal/DX12/OpenGL, hardware first (`crates/pubrs-app/src/adapter.rs`,
  +5 app tests, 125 tests in total). Help → About and the status bar show when the window is drawn in software.
  `PUBRS_PERF=1` logs frame statistics.
- Then (requested): the x64 MSI bundles Mesa llvmpipe (mesa-dist-win 26.2.4) in `bin\mesa`. With no hardware adapter, pubrs restarts
  from the copy of itself there (`crates/pubrs-app/src/mesa.rs`). CI runs the GUI self-test through that restart on
  the GPU-less Windows runner, and the release workflow checks that the installed MSI draws with llvmpipe. +3 app
  tests (128 in total). ARM64 has no Mesa build (TODO.md).
- Then (requested): `pubrs-app.exe` no longer opens a command prompt on Windows (GUI subsystem). It attaches to the
  terminal it was started from, if any, for logs, `--help` and `--version`. It shows fatal start-up errors in a dialog
  when there is no terminal. CI and the release workflow check the PE subsystem of each exe (ADR 0010 addendum).

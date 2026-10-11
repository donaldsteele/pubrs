# TODO

Deferred and partial work. Each entry says what is missing and why.

## Later phases (designed for, not built)

- Mail merge from CSV.
- `.pub` import (reference: libmspub).
- CMYK / spot colours, PDF/X output.
- Tables.
- WordArt-style text effects.
- Avery label sheet presets.

## Partial features

- Sheet presets Avery 5376, 5377, 22805, 5305, 8315, C32011, L7651 and the L7161/L7162 margins are from memory of published specs; check them against vendor datasheets.
- Imposed sheets carry no bleed, and `crop_marks` marks the whole sheet rather than each label.
- `SetPageSize` does not re-check an existing label sheet; a mismatch is reported at PDF export.
- The GPU renderer ignores `Node.clip` (only imposed sheets use it, and those are never drawn on the GPU).
- Text wrap: `square` and `top_bottom` only, around bounding boxes. Contour ("tight") wrap and text on both sides of an obstacle are not implemented (ADR 0006).
- The `validate` tool still clones the whole document on every call (`crates/pubrs-api/src/tools/query.rs`); the design checker (M29) replaces it. (The canvas page snapshot now shares frames and stories by `Arc` and is built by `Session::page_snapshot`, M13.)
- vello is no longer used by the GUI (ADR 0013), but `crates/pubrs-app/Cargo.toml` still enables `pubrs-api/gpu`, so the app compiles and links it. Dropping the feature from the app was outside M13's files.
- Right-to-left scripts: shaping works through parley, but caret/hit-testing assumes left-to-right lines.
- Characters no bundled or installed font covers render as .notdef boxes; `validate` does not yet name them (layout counts them in `missing_glyphs`).
- GUI: copy/paste of frames works within one publication only (no cross-document or OS-clipboard frame paste).
- GUI: no rulers or guide dragging from rulers yet (guides are added through the API/MCP `guide_add`).
- GUI: text editing has no IME composition support (direct key/text input only) and no spell checking.
- Windows and macOS printing are compile-checked here; they need the manual check in RELEASE.md (no printers on CI).
- macOS: double-clicking a `.pubrs` file in Finder launches pubrs but does not open the file, because winit does not deliver `application:openURLs:` events. Workaround: File → Open, or drag the file onto the pubrs window (ADR 0010).
- Installers are unsigned unless the signing secrets in RELEASE.md are set; macOS notarization is a manual step.
- Flatpak: manifest and AppStream metadata only. It is not built in CI, and Flathub submission needs generated `cargo-sources.json`.
- The Windows ARM64 MSI is built but not install-checked in CI (no ARM64 Windows runner in the release workflow).
- The Windows ARM64 MSI has no bundled Mesa (mesa-dist-win publishes x64 and x86 only), so ARM64 Windows without a GPU driver still draws with WARP: about 0.5 CPU-seconds per frame (ADR 0011).
- Bundled Mesa is pinned to mesa-dist-win 26.2.4. Updating it means a new version and SHA-256 in `packaging/windows/fetch-mesa.ps1` and `packaging/windows/mesa/THIRD-PARTY-NOTICES.txt`.

# 0010 Installers: one release workflow, per-OS native tools

- Context: the spec asks for MSI, a macOS .app/.dmg with `.pubrs` document types, AppImage, .deb and a Flatpak manifest from one tag, each registering `.pubrs` and an icon.
- Decision:
  - Windows: cargo-wix with a hand-written `crates/pubrs-app/wix/main.wxs` (WiX 3.14, per-machine, `bin` on PATH, ProgId `pubrs.Publication`, sample in `samples\`). Built for x64 and ARM64.
  - macOS: cargo-bundle builds the `.app` per architecture; `packaging/macos/build-dmg.sh` joins them with `lipo`, merges `document-types.plist` into Info.plist with PlistBuddy (cargo-bundle cannot express `CFBundleDocumentTypes`/`UTExportedTypeDeclarations`), signs ad hoc or with `MACOS_SIGNING_IDENTITY`, and makes a UDZO DMG with an Applications link.
  - Linux: cargo-deb from `[package.metadata.deb]` (dlopen'd X11/Wayland/GL libraries listed by hand) and `packaging/linux/build-appimage.sh` (appimagetool, `AppRun` dispatches `--cli`/`--mcp`). Built on Ubuntu 22.04 runners for an older glibc baseline.
  - Flatpak: manifest and AppStream metainfo only; Flathub needs vendored cargo sources, which the release workflow does not generate.
  - Icons are generated from code (`cargo run -p pubrs-io --example make_icons`) so PNG/ICO/ICNS stay in sync.
- Consequence: no installer framework beyond each platform's usual tool; every installer job installs, runs `--smoke-test`, exports the bundled sample and uninstalls. macOS Finder "open document" events are not delivered by winit, so double-clicking a `.pubrs` file launches pubrs without opening it (TODO.md).

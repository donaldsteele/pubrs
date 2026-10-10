# Releasing pubrs

## Prerequisites

- GitHub Actions must be able to run on the repository. Private repositories
  need Actions minutes (macOS minutes are billed at 10×); public repositories
  run on free standard runners.
- Optional signing secrets (the workflow skips signing without them):
  - `WINDOWS_CERT_BASE64`, `WINDOWS_CERT_PASSWORD`: an Authenticode `.pfx`,
    base64-encoded. The MSI is signed with `signtool` after it is built.
  - `MACOS_SIGNING_IDENTITY`: a "Developer ID Application" identity in the
    runner keychain (import it in an extra step before `build-dmg.sh`).
    Without it the app is ad-hoc signed.

## Cutting a release

1. Update `version` in the workspace `Cargo.toml` (and the AppStream
   `<release>` in `packaging/flatpak/*.metainfo.xml`), commit.
2. Make sure CI is green on all four jobs.
3. Tag and push: `git tag v0.1.0 && git push origin v0.1.0`. GitHub does not start the
   workflow for a tag on a commit whose message contains `[skip ci]`; in that case run it
   on the tag by hand: `gh workflow run release.yml --ref v0.1.0` (it drafts the release
   the same way).
4. The **Release** workflow builds, checks and uploads:
   - `pubrs-<v>-x86_64-pc-windows-msvc.msi`, `pubrs-<v>-aarch64-pc-windows-msvc.msi` (cargo-wix, WiX 3.14)
   - `pubrs-<v>-macos-universal.dmg` (cargo-bundle + lipo, arm64 + x86_64, `.pubrs` document type)
   - `pubrs_<v>_amd64.deb`, `pubrs_<v>_arm64.deb` (cargo-deb)
   - `pubrs-<v>-x86_64.AppImage`, `pubrs-<v>-aarch64.AppImage` (appimagetool)
   and drafts a GitHub release with all of them. Review the notes and
   publish the draft.
5. macOS notarization (manual until credentials are configured):
   `xcrun notarytool submit pubrs-<v>-macos-universal.dmg --apple-id … --team-id … --wait`
   then `xcrun stapler staple` the DMG and re-upload it.
6. Flatpak: `flatpak-builder --user --install --force-clean build packaging/flatpak/io.github.donaldsteele.pubrs.yml`
   (for Flathub, generate `cargo-sources.json` with flatpak-cargo-generator).

The workflow can also be started by hand (**Actions → Release → Run
workflow**) to build installers without publishing.

## What the workflow checks automatically

Each installer job installs (or mounts) the package and then:

| Check | Windows MSI (x64) | macOS DMG | Linux .deb | AppImage |
|---|---|---|---|---|
| Installs | `msiexec /i /qn` | mount + copy `.app` | `apt install ./*.deb` | run in place |
| Launches the GUI | `pubrs-app --smoke-test` | `pubrs-app --smoke-test` | `xvfb-run pubrs-app --smoke-test` | `--smoke-test` under xvfb |
| Opens the sample and exports a PDF | `pubrs export samples\bakery-trifold.pubrs` | `pubrs export Resources/samples/…` | `pubrs export /usr/share/doc/pubrs/examples/…` | `--cli export …` |
| File type registered | `assoc .pubrs` = `pubrs.Publication` | `CFBundleDocumentTypes` in Info.plist | mime XML installed | — |
| Uninstalls cleanly | `msiexec /x /qn`, binaries gone | remove `.app` | `apt remove`, binaries gone | delete file |

The ARM64 Windows MSI and the aarch64 Linux packages are built and the
Linux ones are checked on ARM runners; the ARM64 MSI cannot run on the x64
runner and needs the manual check below.

## Manual check per installer (before publishing)

Do this on a real desktop for each OS (Windows 10 or 11, macOS 12+ on Apple
Silicon and on Intel, a GNOME or KDE Linux desktop on X11 and on Wayland):

1. **Install** with the normal double-click flow.
2. **Launch** pubrs from the Start menu / Applications / app launcher. The
   template picker appears.
3. **Open the sample**: double-click `bakery-trifold.pubrs` in the sample
   folder (Windows, Linux) or drag it onto the pubrs window (macOS). Both pages show the
   bakery brochure with two photos.
4. **Edit**: double-click the "Our Story" text and type a sentence; the text
   flows into the middle panel. Undo with Ctrl/⌘+Z.
5. **Export PDF** (Ctrl/⌘+E) and open it in the system PDF viewer: two
   landscape pages, text selectable, photos sharp.
6. **Print** (Ctrl/⌘+P): the system print dialog (Windows, macOS) or the
   pubrs printer chooser (Linux/CUPS) appears; print to PDF or a real
   printer.
7. **MCP**: `pubrs-mcp --help` runs from a terminal (Windows: new terminal
   after install so PATH is updated).
8. **Uninstall**: Settings → Apps (Windows), drag to Bin (macOS),
   `sudo apt remove pubrs` (Linux). The program files are gone; your
   documents and `Documents/pubrs` are untouched.

Record the results (OS version, installer, pass/fail per step) in the
release notes.

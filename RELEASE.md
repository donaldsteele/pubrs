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

1. Update `version` in the workspace `Cargo.toml`, `version` under
   `[package.metadata.bundle]` in `crates/pubrs-app/Cargo.toml` (the macOS app),
   and add a `<release>` to `packaging/flatpak/*.metainfo.xml`. Then refresh
   `Cargo.lock` (`cargo update -w`, or any build) and commit it too: the release
   workflow builds with `--locked` and fails if the lock file does not match the
   version. Commit without `[skip ci]` in the message, or the tag will not
   start the release workflow.
2. Make sure CI is green on all four jobs.
3. Tag and push: `git tag v0.2.0 && git push origin v0.2.0`. GitHub does not start the
   workflow for a tag on a commit whose message contains `[skip ci]`; in that case run it
   on the tag by hand: `gh workflow run release.yml --ref v0.2.0` (it drafts the release
   the same way).
4. The **Release** workflow builds, checks and uploads:
   - `pubrs-<v>-x86_64-pc-windows-msvc.msi`, `pubrs-<v>-aarch64-pc-windows-msvc.msi` (cargo-wix, WiX 3.14); the x64 MSI includes Mesa llvmpipe in `bin\mesa` for PCs without a GPU driver (ADR 0011)
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
workflow**, or `gh workflow run release.yml --ref <branch>`) to build installers
without publishing. A manual dispatch on a branch builds, checks and uploads
every installer (Windows x64 and ARM64 MSI, macOS DMG, Linux .deb and AppImage
for x86_64 and aarch64) as workflow artifacts, named with the version in
`Cargo.toml`. It does not create a release or a tag: the `release` job runs only
when the ref is a tag (`refs/tags/v*`). The workflow file must exist on that
branch. Use this to rehearse a release (for 0.2.0, on `plan/publisher-parity`)
before tagging; the artifacts are kept for the repository's retention period.

## What the workflow checks automatically

Each installer job installs (or mounts) the package and then:

| Check | Windows MSI (x64) | macOS DMG | Linux .deb | AppImage |
|---|---|---|---|---|
| Installs | `msiexec /i /qn` | mount + copy `.app` | `apt install ./*.deb` | run in place |
| Launches the GUI | `pubrs-app --smoke-test`, which must restart with the bundled Mesa llvmpipe (the runner has no GPU) | `pubrs-app --smoke-test` | `xvfb-run pubrs-app --smoke-test` | `--smoke-test` under xvfb |
| Opens the sample and exports a PDF | `pubrs export samples\bakery-trifold.pubrs` | `pubrs export Resources/samples/…` | `pubrs export /usr/share/doc/pubrs/examples/…` | `--cli export …` |
| No console window | `check-subsystem.ps1`: pubrs-app GUI, pubrs and pubrs-mcp console | — | — | — |
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
   template picker appears, and on Windows no command prompt window opens.
3. **Open the sample**: double-click `bakery-trifold.pubrs` in the sample
   folder (Windows, Linux) or drag it onto the pubrs window (macOS). Both pages show the
   bakery brochure with two photos.
4. **Edit**: double-click the "Our Story" text and type a sentence; the text
   flows into the middle panel. Undo with Ctrl/⌘+Z.
5. **Help → About**: "Window drawing" names the GPU. On a Windows PC or VM
   without a graphics driver it says llvmpipe (x64) and the status bar shows
   "Drawing without a GPU"; moving the mouse over the page stays smooth.
6. **Export PDF** (Ctrl/⌘+E) and open it in the system PDF viewer: two
   landscape pages, text selectable, photos sharp.
7. **Print** (Ctrl/⌘+P): the system print dialog (Windows, macOS) or the
   pubrs printer chooser (Linux/CUPS) appears; print to PDF or a real
   printer.
8. **MCP**: `pubrs-mcp --help` runs from a terminal (Windows: new terminal
   after install so PATH is updated).
9. **Uninstall**: Settings → Apps (Windows), drag to Bin (macOS),
   `sudo apt remove pubrs` (Linux). The program files are gone; your
   documents and `Documents/pubrs` are untouched.

Printing and print-shop output need a person and paper, so they are checked by
hand once per release (on at least one OS per printing backend: Windows GDI,
macOS PDFKit, Linux CUPS):

10. **Print a sheet of 30 labels.** New from template → Address labels, then
    Mailings → Mail Merge… with a CSV of 30 rows (Name, Address, City, State,
    Zip) and **Print…** (or Merge to PDF and print the PDF). Print at actual
    size (no "fit to page", no scaling) on plain paper, then lay it over a
    real Avery 5160 sheet against a window: every address should sit inside
    its label with equal margins. On Windows, note whether the printer's
    printable area shrank the page (see USER_GUIDE.md, Printing). Repeat
    with another preset if you have the stock, and look at the presets
    listed in ASSETS.md as unchecked.
11. **Print an 8-page booklet.** Make an 8-page publication, Page Setup →
    Labels, Cards, Booklet → Booklet (saddle stitch), **Print…** double-sided,
    flipping on the short edge. Fold the stack in half: the pages must read
    1 to 8 in order, with page 1 on the outside front and page 8 on the back.
12. **Open the PDF/X-4 export in a preflight tool.** Export the newsletter
    (or the flyer after switching its colours to CMYK) with **Commercial
    printing (PDF/X-4)** and open it in Acrobat Pro's Preflight (PDF/X-4
    profile) or another PDF/X checker such as callas pdfToolbox. It should
    pass or show only findings you accept (for example the generic output
    profile). pubrs's own checker does not look at trapping, overprint or
    ink coverage.

Record the results (OS version, installer, pass/fail per step) in the
release notes.

## 0.2.0 release notes

Paste this into the GitHub release draft and trim it.

**New in 0.2.0 (publisher parity)**

- *Text.* Hyphenation with bounded justification (English, German, Spanish,
  French, Polish, Portuguese); keep with next, keep lines together, widow and
  orphan control; drop caps; tab stops with leaders; bullets and numbering with
  nested levels; character formatting (small and all caps, strikethrough,
  superscript, subscript, baseline shift, ligatures, kerning); tight, through
  and both-sides text wrap; text that follows into linked frames and columns
  with page, column and frame breaks.
- *Objects.* Tables (insert, type, merge and split cells, resize, cell
  properties, Tab between cells); WordArt, drop shadows and BorderArt.
- *Colour and print.* Swatches (RGB, CMYK, spot), twelve colour schemes, tints
  and gradients; PDF/X-4 and PDF/A-2b export with bleed, crop marks and a
  "Save for a commercial printer" package; a Design Checker panel that
  updates as you edit, with a check before a commercial export.
- *Labels, cards and booklets.* 26 sheet presets (Avery and A4 labels and
  cards, generic 2-up and 4-up), page setup for sheets and saddle-stitch
  booklets, imposed in PDF and print.
- *Mail merge.* A wizard for CSV and XLSX lists, merge fields, a preview, and
  merge to PDF, to a new publication or to the printer; `pubrs merge`;
  matching MCP tools.
- *Spelling.* Check as you type, a right-click menu, a Spelling window (F7) and
  user dictionaries (American English is built in).
- *Everyday templates.* Greeting card, postcard, certificate, menu, letterhead,
  envelope, annual calendar, address labels and business cards (sheet).
- *Import.* Microsoft Publisher (`.pub`) files: pages, text, pictures (PNG,
  JPEG, GIF), shapes, tables, colours and masters, with a report of what was
  left out (File → Import Publisher File…, `pubrs import`, MCP `doc_import`).
- *Clipboard and input.* Copy and paste frames between publications, rich text
  and pictures from other programs; Chinese, Japanese and Korean input;
  right-to-left text (Hebrew, Arabic) carets and selection.
- *Speed.* Tiled, off-thread page drawing and an incremental layout cache; `pubrs
  perf` measures it (a 200-page publication checks within budget).
- *File format.* Schema version 12. Files saved by 0.1 open unchanged; files
  saved by 0.2 do not open in 0.1.

**Template names.** The nine new built-in templates (`greeting_card`,
`postcard`, `certificate`, `menu`, `letterhead`, `envelope`, `calendar`,
`address_labels`, `business_cards`) take precedence over any template of your
own saved earlier under the same name: the built-in is listed and used, and
yours is hidden (its file is untouched; save it under another name to use it).

**Known limitations** (see TODO.md for the full list)

- Installers are not code-signed or notarized; Windows SmartScreen and macOS
  Gatekeeper warn on first run (USER_GUIDE.md says how to proceed).
- macOS: double-clicking a `.pubrs` or `.pub` file starts pubrs without opening
  it; use File → Open or drag the file onto the window.
- Windows ARM64: the MSI is built but not install-tested, and has no bundled
  Mesa (a PC without a GPU driver draws slowly).
- Printing on Windows and macOS is untested on real printers by CI; label
  sheet alignment (several presets entered from memory: Avery 5376, 5377, 22805,
  5305, 8315, C32011, L7651 and the L7161/L7162 margins) needs a test print.
  On Windows a page larger than the printer's printable area is shrunk to fit.
- Hyphenation covers six languages; spelling needs a Hunspell dictionary for
  anything but American English.
- Windows metafile pictures in `.pub` files and pictures in Publisher 98 and
  2000 files are not imported.
- Ctrl+V cannot paste an image-only clipboard on Windows and Linux (use Edit →
  Paste); the text of an input method's composition is drawn in the window's
  font.
- Flatpak is manifest only, not built in CI.


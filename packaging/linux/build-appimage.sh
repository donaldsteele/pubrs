#!/usr/bin/env bash
# Build pubrs-<version>-<arch>.AppImage from release binaries.
# Usage: packaging/linux/build-appimage.sh <target-dir-with-binaries> <version> <arch> [out-dir]
set -euo pipefail
BIN=$1; VERSION=$2; ARCH=$3; OUT=${4:-dist}
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
APPDIR="$(mktemp -d)/pubrs.AppDir"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" "$APPDIR/usr/share/mime/packages" "$OUT"
for b in pubrs-app pubrs pubrs-mcp; do install -m755 "$BIN/$b" "$APPDIR/usr/bin/$b"; done
install -m644 "$ROOT/packaging/linux/pubrs.desktop" "$APPDIR/pubrs.desktop"
install -m644 "$ROOT/packaging/linux/pubrs.desktop" "$APPDIR/usr/share/applications/pubrs.desktop"
install -m644 "$ROOT/packaging/linux/pubrs-mime.xml" "$APPDIR/usr/share/mime/packages/pubrs.xml"
install -m644 "$ROOT/packaging/icons/pubrs-256.png" "$APPDIR/pubrs.png"
for s in 16 32 48 64 128 256 512; do
    mkdir -p "$APPDIR/usr/share/icons/hicolor/${s}x${s}/apps" "$APPDIR/usr/share/icons/hicolor/${s}x${s}/mimetypes"
    install -m644 "$ROOT/packaging/icons/pubrs-$s.png" "$APPDIR/usr/share/icons/hicolor/${s}x${s}/apps/pubrs.png"
    install -m644 "$ROOT/packaging/icons/pubrs-document-$s.png" "$APPDIR/usr/share/icons/hicolor/${s}x${s}/mimetypes/application-vnd.pubrs+zip.png"
done
install -m755 "$ROOT/packaging/linux/AppRun" "$APPDIR/AppRun"
TOOL="${APPIMAGETOOL:-appimagetool}"
if ! command -v "$TOOL" >/dev/null 2>&1; then
    TOOL="$(mktemp -d)/appimagetool"
    curl -fsSL -o "$TOOL" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$ARCH.AppImage"
    chmod +x "$TOOL"
fi
ARCH="$ARCH" "$TOOL" --appimage-extract-and-run --no-appstream "$APPDIR" "$OUT/pubrs-$VERSION-$ARCH.AppImage"
echo "$OUT/pubrs-$VERSION-$ARCH.AppImage"

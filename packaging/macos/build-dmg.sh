#!/usr/bin/env bash
# Build a universal (arm64 + x86_64) pubrs.app with cargo-bundle and wrap it
# in a DMG. Usage: packaging/macos/build-dmg.sh <version> [out-dir]
#
# Signing: set MACOS_SIGNING_IDENTITY (e.g. "Developer ID Application: …") to
# sign with hardened runtime; otherwise the app is ad-hoc signed. Notarization
# is a documented manual step in RELEASE.md (needs Apple credentials).
set -euo pipefail
VERSION=$1; OUT=${2:-dist}
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
mkdir -p "$OUT"
for t in aarch64-apple-darwin x86_64-apple-darwin; do
    rustup target add "$t" >/dev/null
    cargo build --release --locked --target "$t" -p pubrs-app -p pubrs-cli -p pubrs-mcp
done
cargo bundle --release -p pubrs-app --target aarch64-apple-darwin --format osx
APP="target/aarch64-apple-darwin/release/bundle/osx/pubrs.app"
test -d "$APP"
for b in pubrs-app pubrs pubrs-mcp; do
    lipo -create -output "$APP/Contents/MacOS/$b" \
        "target/aarch64-apple-darwin/release/$b" "target/x86_64-apple-darwin/release/$b"
done
lipo -info "$APP/Contents/MacOS/pubrs-app"
cp packaging/icons/pubrs-document.icns "$APP/Contents/Resources/"
mkdir -p "$APP/Contents/Resources/samples"
cp examples/samples/bakery-trifold.pubrs "$APP/Contents/Resources/samples/"
/usr/libexec/PlistBuddy -c "Merge packaging/macos/document-types.plist" "$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" "$APP/Contents/Info.plist" || true
plutil -lint "$APP/Contents/Info.plist"
if [ -n "${MACOS_SIGNING_IDENTITY:-}" ]; then
    codesign --force --deep --options runtime --timestamp -s "$MACOS_SIGNING_IDENTITY" "$APP"
else
    codesign --force --deep -s - "$APP"
fi
codesign --verify --verbose "$APP"
STAGE="$(mktemp -d)"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
DMG="$OUT/pubrs-$VERSION-macos-universal.dmg"
hdiutil create -volname "pubrs $VERSION" -srcfolder "$STAGE" -ov -format UDZO "$DMG"
echo "$DMG"

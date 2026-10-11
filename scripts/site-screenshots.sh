#!/usr/bin/env bash
# Regenerates the project website's images in site/assets/screens/.
#
#   scripts/site-screenshots.sh
#
# Builds pubrs-app in release mode and runs it under a virtual X display with
# `--showcase DIR` (crates/pubrs-app/src/showcase.rs). The app drives its own
# GUI through a fixed list of scenes and saves one PNG per scene:
#   newsletter, templates, tables, mail-merge, merged-labels, design-checker,
#   wordart, colour, pub-import, export       (window screenshots)
#   render-newsletter, render-certificate, render-menu, render-postcard,
#   render-calendar                            (page-1 renders of the filled documents)
# The window is 2160x1350 physical pixels at 1.5 points per logical point
# (1440x900 logical). The documents are built from the scripts in
# crates/pubrs-app/showcase/ with the SVG pictures in
# site/assets/showcase-content/ (our own work, MIT licensed).
#
# Needs xvfb-run (apt install xvfb), a Rust toolchain and Mesa's software GL.
# Output is deterministic apart from anti-aliasing differences between GL
# drivers. Run it from anywhere; paths are relative to the repository root.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

width=2160
height=1350
dest="$root/site/assets/screens"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

if ! command -v xvfb-run >/dev/null 2>&1; then
    echo "xvfb-run not found: install xvfb" >&2
    exit 1
fi

cargo build --release -p pubrs-app
xvfb-run -a -s "-screen 0 ${width}x${height}x24" \
    cargo run --release -p pubrs-app -- --showcase "$tmp"

mkdir -p "$dest"
count=0
for png in "$tmp"/*.png; do
    cp "$png" "$dest/"
    count=$((count + 1))
done
echo "copied $count images to ${dest#"$root"/}"

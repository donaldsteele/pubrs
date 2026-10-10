#!/usr/bin/env sh
# Compile-check (clippy, -D warnings) the workspace for Windows and macOS from
# any host. This catches cfg-gated code that the host OS never builds; it does
# not run tests. CI on real runners remains the verifier.
set -eu
for target in x86_64-pc-windows-msvc aarch64-apple-darwin; do
    rustup target add "$target" >/dev/null
    echo "== $target"
    cargo clippy --workspace --all-targets --all-features --target "$target" -- -D warnings
done

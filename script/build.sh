#!/usr/bin/env bash
# Build all release binaries into dist/ (mirrors the CI matrix).
# Requires: cargo-zigbuild and zig (https://github.com/ziglang/zig).
set -euo pipefail

dist="dist"
mkdir -p "$dist"

build() {
  local target="$1" outfile="$2"
  echo "Building $target..."
  cargo zigbuild --release --target "$target" --bin livetube
  cp "target/$target/release/livetube" "$dist/$outfile"
}

build x86_64-unknown-linux-musl  livetube-linux-x64
build aarch64-unknown-linux-musl livetube-linux-arm64
build x86_64-apple-darwin        livetube-macos-x64
build aarch64-apple-darwin       livetube-macos-arm64
build x86_64-pc-windows-gnu      livetube-windows-x64.exe

echo "✔ Binaries built in $dist:"
ls -lh "$dist"

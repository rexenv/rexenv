#!/bin/sh
# Stage the `rex` CLI as Tauri external binaries (sidecars).
#
# Tauri bundles `src-tauri/binaries/rex-<target-triple>` into the app
# (macOS: Contents/MacOS/rex, codesigned with the bundle) and copies it next
# to the dev binary on `tauri dev`. Wired into beforeDevCommand /
# beforeBuildCommand in tauri.conf.json — never run cargo against the app
# crate here (the tauri CLI owns that build).
set -e
cd "$(dirname "$0")/.."
mkdir -p src-tauri/binaries

case "$(uname -s)" in
  Darwin)
    # Both slices + a lipo'd universal, so any bundle target
    # (aarch64 / x86_64 / universal-apple-darwin) finds its sidecar.
    cargo build --manifest-path cli/Cargo.toml --release --target aarch64-apple-darwin
    cargo build --manifest-path cli/Cargo.toml --release --target x86_64-apple-darwin
    cp cli/target/aarch64-apple-darwin/release/rex src-tauri/binaries/rex-aarch64-apple-darwin
    cp cli/target/x86_64-apple-darwin/release/rex src-tauri/binaries/rex-x86_64-apple-darwin
    lipo -create -output src-tauri/binaries/rex-universal-apple-darwin \
      src-tauri/binaries/rex-aarch64-apple-darwin \
      src-tauri/binaries/rex-x86_64-apple-darwin
    echo "build-cli: staged rex sidecars (aarch64, x86_64, universal)"
    ;;
  *)
    # Phase 4: Windows/Linux staging lands with their platform impls.
    echo "build-cli: only macOS staging is implemented" >&2
    exit 1
    ;;
esac

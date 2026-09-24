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
  MINGW*|MSYS*|CYGWIN*)
    # Windows (Git Bash on a Windows build host, W8 S4, ledger #633): the x64 sidecar, named for its target
    # triple so Tauri bundles it beside rexenv.exe as rex.exe — the name `core::cli::bundled_rex` looks for.
    cargo build --manifest-path cli/Cargo.toml --release --target x86_64-pc-windows-msvc
    cp cli/target/x86_64-pc-windows-msvc/release/rex.exe src-tauri/binaries/rex-x86_64-pc-windows-msvc.exe
    echo "build-cli: staged rex sidecar (x86_64-pc-windows-msvc)"
    ;;
  Linux)
    # Linux (docs/PLAN-linux-port.md L3): the HOST triple's sidecar — `tauri build` on an
    # x86_64 box bundles `rex-x86_64-unknown-linux-gnu`, on an arm64 box the aarch64 one.
    # `rustc -vV` names the host; a hardcoded x86_64 would stage the wrong name on the
    # aarch64 machines this port is exercised on (the Mac's Docker containers).
    triple="$(rustc -vV | sed -n 's/^host: //p')"
    cargo build --manifest-path cli/Cargo.toml --release --target "$triple"
    cp "cli/target/$triple/release/rex" "src-tauri/binaries/rex-$triple"
    echo "build-cli: staged rex sidecar ($triple)"
    ;;
  *)
    echo "build-cli: only macOS, Windows and Linux staging are implemented" >&2
    exit 1
    ;;
esac

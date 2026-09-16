#!/bin/bash
# THE way to cross-build the Windows app on the Mac — for the Dell, and later for a release.
#
# It exists because the invocation has ONE non-obvious flag and forgetting it fails in a way
# that looks like a broken machine rather than a wrong build:
#
#   `--features tauri/custom-protocol`
#
# Without it a debug build serves `devUrl` (http://localhost:1420) instead of the embedded
# `frontendDist`, so the app launches, the backend comes up healthy — pipe held, DNS agent
# running, the log clean — and the window shows the browser's "can't reach this page".
# `docs/TESTING.md` (W6's done-when) and `docs/PLAN-windows-port.md` §5 both record the flag;
# on 16 Sep 2026 a build was hand-rolled without reading them and shipped to the Dell exactly
# that way, which cost the owner a working app until it was rebuilt. Hence a script: the
# command is no longer something anyone has to remember.
#
# The check at the end is the load-bearing part. It greps the built exe for the hashed asset
# name Vite just emitted, so "it built" can never be reported as "it works" — if the assets
# are not inside the binary, this exits non-zero and says so.
#
# Usage:  ./scripts/windows-app-build.sh            # debug (what the Dell runs)
#         ./scripts/windows-app-build.sh --release  # optimised
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"
TARGET="x86_64-pc-windows-msvc"
PROFILE_DIR="debug"
CARGO_PROFILE_ARGS=()
# NOTE the `${arr[@]+...}` form below: macOS ships bash 3.2, where `set -u` treats an
# EMPTY array expansion as unbound and kills the script. It bit this file on its first run.
if [ "${1:-}" = "--release" ]; then
  PROFILE_DIR="release"
  CARGO_PROFILE_ARGS=(--release)
fi

: "${XWIN_ACCEPT_LICENSE:?set XWIN_ACCEPT_LICENSE=1 — cargo-xwin downloads Microsoft's CRT and SDK, which is Microsoft's licence to accept}"

SIDECAR="src-tauri/binaries/rex-$TARGET.exe"
REX="cli/target/xwin/$TARGET/$PROFILE_DIR/rex.exe"

echo "==> rex sidecar ($PROFILE_DIR)"
# tauri_build validates `externalBin` against the HOST triple at build time, and build.rs only
# stages the macOS ones (`#[cfg(target_os = "macos")]`), so the Windows sidecar must be here
# before the app compiles or the build refuses with a missing-binary error.
if [ ! -f "$REX" ]; then
  (cd cli && CARGO_TARGET_DIR=target/xwin cargo xwin build ${CARGO_PROFILE_ARGS[@]+"${CARGO_PROFILE_ARGS[@]}"} --target "$TARGET")
fi
cp "$REX" "$SIDECAR"
trap 'rm -f "$ROOT/$SIDECAR"' EXIT

echo "==> frontend"
pnpm build

ASSET="$(ls dist/assets | grep -E '^index-.*\.js$' | head -1)"
[ -n "$ASSET" ] || { echo "windows-app-build: dist/assets has no index-*.js — did the frontend build?" >&2; exit 1; }
echo "    asset: $ASSET"

echo "==> app ($PROFILE_DIR, with tauri/custom-protocol)"
(cd src-tauri && CARGO_TARGET_DIR=target/xwin \
  cargo xwin build --bin rexenv --features tauri/custom-protocol ${CARGO_PROFILE_ARGS[@]+"${CARGO_PROFILE_ARGS[@]}"} --target "$TARGET")

EXE="src-tauri/target/xwin/$TARGET/$PROFILE_DIR/rexenv.exe"

# The verdict. A binary without the assets is the devUrl build, whatever the exit codes said.
if ! LC_ALL=C grep -aq -- "$ASSET" "$EXE"; then
  cat >&2 <<MSG
windows-app-build: the exe does NOT embed the frontend.

  Looked for: $ASSET
  In:         $EXE

  That is the devUrl build — it will show "can't reach this page" on a machine with no
  dev server. Check that --features tauri/custom-protocol reached cargo.
MSG
  exit 1
fi

echo "windows-app-build: $EXE"
echo "  $(wc -c < "$EXE" | tr -d ' ') bytes, embeds $ASSET"
echo "  sidecar: $(wc -c < "$REX" | tr -d ' ') bytes"

#!/bin/bash
# Does the tree COMPILE for Windows x64? — cross-checked from the Mac with cargo-xwin.
#
# docs/PLAN-windows-port.md W0. The owner ruled on 12 Sep 2026 that this runs locally,
# not as a GitHub Actions job: the repo is private and has never run Actions, and the
# release build is local for the same billing reason (docs/RELEASING.md).
#
# What it proves and what it does not. `cargo check` type-checks every target
# (`--all-targets`: lib, bins, examples, tests) against the Windows std and the
# Windows-only dependency tree, with the MSVC CRT/SDK that xwin fetches so C build
# scripts (`ring`, bundled sqlite) run. It does NOT link, does NOT run a single test,
# and says nothing about behaviour — a green here means "the Windows stubs are
# reachable", not "rexenv works on Windows". That proof needs a Windows machine (plan §7).
#
# Part of verify.sh since it first went green (W1 complete, 12 Sep 2026). On a machine
# without the toolchain or the licence consent it exits 3, and verify.sh prints that as
# a SKIPPED line instead of failing; any other non-zero exit fails the bar.
#
#   ./scripts/windows-check.sh              check both crates, print the error inventory
#   ./scripts/windows-check.sh > f 2>&1     the sanctioned way to keep its output
#
# Requires: `cargo install cargo-xwin --locked`, `brew install llvm lld` (clang-cl,
# lld-link), `rustup target add x86_64-pc-windows-msvc`, and XWIN_ACCEPT_LICENSE=1 —
# xwin downloads Microsoft's CRT and Windows SDK, which is Microsoft's licence to accept.
# This script never accepts it on anyone's behalf — and cargo-xwin itself does not ask,
# so the refusal below is the ONLY place consent is checked (ledger #583).
set -uo pipefail

# Same rule as verify.sh, same reason: a pipe replaces this script's exit code.
if [ -p /dev/stdout ] && [ "${REXENV_ALLOW_PIPE:-0}" != "1" ]; then
  echo "windows-check.sh: refusing to run with stdout piped — redirect to a file instead." >&2
  exit 2
fi

cd "$(dirname "$0")/.."

TARGET=x86_64-pc-windows-msvc
SIDECAR="src-tauri/binaries/rex-$TARGET.exe"
PLACEHOLDER_MARK='PLACEHOLDER staged by scripts/windows-check.sh — not a rex binary'

# A placeholder left by a run this script never got to clean up (SIGKILL skips every
# trap) is removed FIRST, before anything can refuse: a later run would otherwise see
# the file, take it for a real `rex.exe`, never stage — and never clean — it, and a
# real Windows bundle would ship it. Only OUR file, recognised by its first line; a
# real sidecar at that path is left alone (ledger #582).
if [ -f "$SIDECAR" ] && [ "$(head -n1 "$SIDECAR" 2>/dev/null)" = "$PLACEHOLDER_MARK" ]; then
  rm -f "$SIDECAR"
fi

missing=()
command -v cargo-xwin >/dev/null 2>&1 || missing+=("cargo install cargo-xwin --locked")
LLVM_BIN="$(brew --prefix llvm 2>/dev/null)/bin"
LLD_BIN="$(brew --prefix lld 2>/dev/null)/bin"
[ -x "$LLVM_BIN/clang-cl" ] || missing+=("brew install llvm")
[ -x "$LLD_BIN/lld-link" ] || missing+=("brew install lld")
rustup target list --installed 2>/dev/null | grep -qx "$TARGET" || missing+=("rustup target add $TARGET")
if [ "${#missing[@]}" -gt 0 ]; then
  echo "windows-check: missing toolchain — run:" >&2
  printf '    %s\n' "${missing[@]}" >&2
  exit 3
fi
if [ "${XWIN_ACCEPT_LICENSE:-}" != "1" ]; then
  cat >&2 <<'MSG'
windows-check: XWIN_ACCEPT_LICENSE is not set.

  cargo-xwin downloads Microsoft's CRT and Windows SDK, which are licensed by Microsoft.
  Read the terms (https://go.microsoft.com/fwlink/?LinkId=2086102), and if you accept
  them, re-run with XWIN_ACCEPT_LICENSE=1.
MSG
  exit 3
fi

export PATH="$LLVM_BIN:$LLD_BIN:$PATH"

# One target dir per crate, apart from the macOS one: sharing it would make every
# verify.sh run after this (and this after it) rebuild build-script outputs for the
# other target's environment.
#
# The `rex` sidecar. tauri_build refuses to run unless `bundle.externalBin` exists for
# the TARGET triple, and the real `rex.exe` cannot exist yet — the cli crate is one of
# the things that does not compile for Windows (W8). So this script stages an empty,
# clearly-labelled placeholder for the length of the run and removes it on every exit.
# Deliberately NOT in build.rs: a placeholder the build itself creates is a placeholder
# a real Windows bundle would ship, silently, as the user's `rex`.
SIDECAR="src-tauri/binaries/rex-$TARGET.exe"
staged=0
cleanup() { [ "$staged" -eq 1 ] && rm -f "$SIDECAR"; }
trap cleanup EXIT
if [ ! -e "$SIDECAR" ]; then
  mkdir -p "$(dirname "$SIDECAR")"
  printf 'PLACEHOLDER staged by scripts/windows-check.sh — not a rex binary\n' > "$SIDECAR"
  staged=1
fi

# `--keep-going`: without it cargo stops scheduling at the first failed crate, so a
# dev-dependency that cannot build for Windows (objc2, test targets only) would hide
# every error in the library behind it — an inventory of one line.
fail=0
total=0
for crate in src-tauri cli; do
  dir="$crate/target/xwin"
  mkdir -p "$dir"
  log="$dir/windows-check.log"
  (cd "$crate" && CARGO_TARGET_DIR=target/xwin cargo xwin check --all-targets --keep-going --target "$TARGET") > "$log" 2>&1
  code=$?

  # The inventory: every error header in OUR sources, with its first location.
  # Headers whose location is inside std (`/rustc/…`) are the cascade of an error
  # already listed, not a separate place to fix.
  inventory="$(awk '
    /^error(\[E[0-9]+\])?: / && !/could not compile/ { msg = $0; want = 1; next }
    want && /^ +--> / {
      loc = $2
      if (loc !~ /^\/rustc\//) print loc "\t" msg
      want = 0
    }
  ' "$log" | sort -u)"
  n=0
  [ -n "$inventory" ] && n=$(printf '%s\n' "$inventory" | wc -l | tr -d ' ')
  total=$((total + n))

  if [ "$code" -eq 0 ]; then
    echo "windows-check: $crate — compiles for $TARGET"
  else
    fail=1
    echo "windows-check: $crate — RED (cargo exit $code, $n error sites; full log: $log)"
    [ -n "$inventory" ] && printf '%s\n' "$inventory" | sed 's/^/    /'
    [ "$n" -eq 0 ] && tail -20 "$log" | sed 's/^/    /'
  fi
done

if [ "$fail" -eq 0 ]; then
  echo "windows-check: all green"
  exit 0
fi
echo "windows-check: RED — $total error sites across both crates"
exit 1

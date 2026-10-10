#!/bin/bash
# Keep the dev Mac's disk from filling under the build (owner, 10 Oct 2026).
#
#   scripts/disk-guard.sh after    # end of verify.sh / live-checks.sh: drop what the next run relinks
#   scripts/disk-guard.sh before   # start of a long build: make room first when the disk is low
#   scripts/disk-guard.sh selftest # the target/-only rule, checked (verify.sh runs it)
#
# Why it exists: on 10 Oct 2026 the disk hit ZERO twice in one afternoon. Each time a
# verify or an example run died with ENOSPC mid-link (a state of the machine, never a
# verdict on the code), and the second time even Claude Code's own shell could not
# open its output file, so nothing could be run until a human freed space by hand.
# The examples alone are ~8 GB of linked binaries that `cargo build --examples`
# rebuilds from cached deps in minutes.
#
# What it deletes — ONLY build output under the two crates' `target/` folders, never
# a source file, never app data, never a fixture:
#   always (after):   target/debug/examples, and the Windows cross-build's examples
#   when free < LOW:  also the incremental caches and deps/build entries untouched
#                     for 3+ days (stale hashes; cargo rebuilds what is missing)
# REXENV_KEEP_EXAMPLES=1 skips the "always" step (verify-full.sh sets it between
# verify and the live-check tier, which runs the examples verify just built).
set -uo pipefail
cd "$(dirname "$0")/.."
mode="${1:-after}"
LOW_GB="${REXENV_DISK_LOW_GB:-15}"

free_gb() { df -g . | awk 'NR==2 {print $4}'; }
before=$(free_gb)
dropped=()

drop() { # <path> — only under a target/ folder of this repo
  case "$1" in
    src-tauri/target/*|cli/target/*) ;;
    *) echo "disk-guard: refusing to delete $1 (not build output)" >&2; return ;;
  esac
  [ -e "$1" ] || return
  rm -rf "$1" && dropped+=("$1")
}

# `selftest`: the one rule that matters — a path outside target/ is never deleted
# (ledger #840). Run by verify.sh; a wider `case` in `drop` makes it red.
if [ "$mode" = "selftest" ]; then
  probe="$(mktemp -d "${TMPDIR:-/tmp}/disk-guard-probe.XXXXXX")"
  mkdir -p src-tauri/target/disk-guard-probe
  drop "$probe" 2>/dev/null
  drop src-tauri/target/disk-guard-probe
  if [ -d "$probe" ] && [ ! -e src-tauri/target/disk-guard-probe ]; then
    rmdir "$probe"; echo "disk-guard: selftest ok (outside target/ kept, inside dropped)"; exit 0
  fi
  rm -rf "$probe" src-tauri/target/disk-guard-probe
  echo "disk-guard: selftest RED — the target/-only rule does not hold" >&2
  exit 1
fi

if [ "$mode" = "after" ] && [ "${REXENV_KEEP_EXAMPLES:-0}" != "1" ]; then
  drop src-tauri/target/debug/examples
  drop src-tauri/target/xwin/x86_64-pc-windows-msvc/debug/examples
fi

if [ "$(free_gb)" -lt "$LOW_GB" ]; then
  for t in src-tauri/target cli/target; do
    for inc in "$t/debug/incremental" "$t/xwin/x86_64-pc-windows-msvc/debug/incremental"; do
      drop "$inc"
    done
    for d in "$t/debug/deps" "$t/debug/build" "$t/xwin/x86_64-pc-windows-msvc/debug/deps" "$t/xwin/x86_64-pc-windows-msvc/debug/build"; do
      [ -d "$d" ] && find "$d" -mindepth 1 -maxdepth 1 -mtime +3 -exec rm -rf {} + 2>/dev/null && dropped+=("$d/<3+ days old>")
    done
  done
fi

after=$(free_gb)
if [ "${#dropped[@]}" -gt 0 ]; then
  echo "disk-guard($mode): ${before} GB → ${after} GB free (dropped: ${dropped[*]})"
else
  echo "disk-guard($mode): ${after} GB free, nothing to drop"
fi
exit 0

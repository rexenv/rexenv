#!/bin/bash
# Does the tree COMPILE for Linux? — linted inside an Ubuntu 22.04 container on the Mac.
#
# docs/PLAN-linux-port.md L0. The Windows gate cross-compiles with cargo-xwin; Linux
# cannot be cross-checked that way, because Tauri links webkit2gtk/gtk3/libsoup through
# pkg-config and nothing packages a GTK sysroot for a Mac host. So this runs the SAME
# question — `cargo clippy --all-targets -- -D warnings`, both crates — inside the image
# `scripts/linux-check/Dockerfile` builds (Ubuntu 22.04, the floor rexenv supports on
# Linux; the toolchain pinned to the Mac's rustc). The container's arch is the host's
# (aarch64 on Apple silicon): a compile gate, not an x86_64 proof.
#
# What it proves and what it does not: that the code type-checks and lints against
# Linux's std, the Linux-only dependency tree and 22.04's libraries. It does NOT run a
# test, does NOT link a binary, and says nothing about behaviour — a green here means
# "the Linux impls are reachable", never "rexenv works on Ubuntu". That proof needs a
# VM (plan §7).
#
# Part of verify.sh, with the same contract as windows-check.sh: exit 3 when it cannot
# run on this machine (no docker, or its daemon is down — Docker Desktop is closed on the
# dev Mac most of the time) and verify.sh prints SKIPPED; any other non-zero exit is a
# Linux break and fails the bar. The source tree is bind-mounted; the target dir and the
# crate registry live in two named volumes so the second run is incremental.
#
#   ./scripts/linux-check.sh              lint both crates, print the error inventory
#   ./scripts/linux-check.sh > f 2>&1     the sanctioned way to keep its output
#   REXENV_LINUX_CHECK_REBUILD=1 …        rebuild the image first (a Dockerfile change)
set -uo pipefail

# Same rule as verify.sh, same reason: a pipe replaces this script's exit code.
if [ -p /dev/stdout ] && [ "${REXENV_ALLOW_PIPE:-0}" != "1" ]; then
  echo "linux-check.sh: refusing to run with stdout piped — redirect to a file instead." >&2
  exit 2
fi

cd "$(dirname "$0")/.."

IMAGE=rexenv-linux-check:ubuntu22
TARGET_VOL=rexenv-linux-check-target
CARGO_VOL=rexenv-linux-check-cargo
PLACEHOLDER_MARK='PLACEHOLDER staged by scripts/linux-check.sh — not a rex binary'

# On a LINUX host the container is beside the point: the compiler targets Linux natively.
if [ "$(uname -s)" = "Linux" ] && [ "${REXENV_FORCE_DOCKER:-0}" != "1" ]; then
  fail=0
  for crate in src-tauri cli; do
    log="$crate/target/linux-check.log"
    mkdir -p "$(dirname "$log")"
    (cd "$crate" && cargo clippy --all-targets --keep-going -- -D warnings) > "$log" 2>&1
    code=$?
    if [ "$code" -eq 0 ]; then
      echo "linux-check: $crate — compiles for Linux (native host)"
    else
      fail=1
      echo "linux-check: $crate — RED (cargo exit $code; full log: $log)"
      tail -20 "$log" | sed 's/^/    /'
    fi
  done
  [ "$fail" -eq 0 ] && { echo "linux-check: all green"; exit 0; }
  echo "linux-check: RED on the native host"
  exit 1
fi

if ! command -v docker >/dev/null 2>&1; then
  echo "linux-check: docker is not installed — install Docker Desktop to run the Linux gate" >&2
  exit 3
fi
if ! docker info >/dev/null 2>&1; then
  echo "linux-check: the docker daemon is not running — start Docker Desktop to run the Linux gate" >&2
  exit 3
fi

# The image, built once from the checked-in Dockerfile (a Dockerfile change = rebuild).
# `docker image inspect` is asked TWICE: on 24 Sep 2026 a verify.sh run saw it fail once
# while the image was there (the daemon mid-something), took that for "no image", tried to
# rebuild on a 2 GB-free disk, and reported the tree RED — for a state of the machine, not
# of the code. A rebuild that fails is the same kind of thing, so it exits 3 (SKIPPED in
# verify.sh) with the reason, never 1.
#
# The Rust version is rust-toolchain.toml's, passed in as a build arg, and an image built
# with a different rustc is rebuilt: after the pin moves, a container still on the old
# version would answer for a toolchain nothing else here uses.
RUST_VERSION="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml)"
if [ -z "$RUST_VERSION" ]; then
  echo "linux-check: no channel in rust-toolchain.toml" >&2
  exit 1
fi
have_image() { docker image inspect "$IMAGE" >/dev/null 2>&1; }
stale_image() { ! docker run --rm "$IMAGE" rustc --version 2>/dev/null | grep -qF "rustc $RUST_VERSION "; }
if [ "${REXENV_LINUX_CHECK_REBUILD:-0}" = "1" ] || { ! have_image && sleep 3 && ! have_image; } || stale_image; then
  if ! docker build --build-arg "RUST_VERSION=$RUST_VERSION" -t "$IMAGE" scripts/linux-check > /dev/null 2>&1; then
    echo "linux-check: could not build $IMAGE from scripts/linux-check/Dockerfile (disk? network?) — not a code verdict" >&2
    exit 3
  fi
fi

# The `rex` sidecar. tauri_build refuses to run unless `bundle.externalBin` exists for the
# TARGET triple, and a lint need not build the real `rex`. The container's triple is the
# host's arch; a placeholder is staged for the length of the run and removed on every exit
# — the same discipline, and the same reason it is NOT in build.rs, as windows-check.sh
# (a placeholder the build itself creates is one a real bundle would ship). A leftover
# from a killed run is removed first, recognised by its first line only.
staged=()
cleanup() { for f in "${staged[@]:-}"; do [ -n "$f" ] && rm -f "$f"; done; }
trap cleanup EXIT
for triple in aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu; do
  sidecar="src-tauri/binaries/rex-$triple"
  if [ -f "$sidecar" ] && [ "$(head -n1 "$sidecar" 2>/dev/null)" = "$PLACEHOLDER_MARK" ]; then
    rm -f "$sidecar"
  fi
  if [ ! -e "$sidecar" ]; then
    mkdir -p src-tauri/binaries
    printf '%s\n' "$PLACEHOLDER_MARK" > "$sidecar"
    staged+=("$sidecar")
  fi
done

fail=0
total=0
for crate in src-tauri cli; do
  mkdir -p "$crate/target"
  log="$crate/target/linux-check.log"
  # `--keep-going`: without it cargo stops scheduling at the first failed crate and the
  # inventory is one line long. The named volumes keep the build between runs; the tree
  # is mounted read-write because build scripts write beside their sources only in
  # OUT_DIR, which is under the target volume — nothing of ours is touched.
  docker run --rm \
    -v "$PWD:/work" \
    -v "$TARGET_VOL:/target" \
    -v "$CARGO_VOL:/usr/local/cargo/registry" \
    -e CARGO_TARGET_DIR="/target/$crate" \
    -e CARGO_INCREMENTAL=0 \
    -w "/work/$crate" \
    "$IMAGE" cargo clippy --all-targets --keep-going -- -D warnings > "$log" 2>&1
  code=$?

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
    echo "linux-check: $crate — compiles for Linux (Ubuntu 22.04, $(docker run --rm "$IMAGE" uname -m))"
  else
    fail=1
    echo "linux-check: $crate — RED (cargo exit $code, $n error sites; full log: $log)"
    [ -n "$inventory" ] && printf '%s\n' "$inventory" | sed 's/^/    /'
    [ "$n" -eq 0 ] && tail -20 "$log" | sed 's/^/    /'
  fi
done

if [ "$fail" -eq 0 ]; then
  echo "linux-check: all green"
  exit 0
fi
echo "linux-check: RED — $total error sites across both crates"
exit 1

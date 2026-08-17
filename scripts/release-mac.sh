#!/bin/bash
# The macOS release build, with the pre-clean `tauri build` needs and does not do.
#
#   ./scripts/release-mac.sh          (this is what `npm run release:mac` runs)
#
# # Why a wrapper exists at all
#
# `tauri build` shells out to a generated `bundle_dmg.sh`, which creates a
# read-write `rw.<pid>.<name>.dmg`, ATTACHES it, copies the app in, and detaches.
# When that sequence dies partway — and it does — the image stays attached and the
# temp file stays on disk. Every later build then fails at the same step with
# nothing but:
#
#     failed to bundle project: error running bundle_dmg.sh
#
# which names neither the mounted volume nor the file. It cost two builds on
# 18 Aug 2026, and worse, each failure left a volume mounted on the developer's
# Mac that they had to notice and eject by hand. A build tool that fails is
# tolerable; one that quietly leaves state behind and then blames itself is not.
#
# # What this cleans, and what it deliberately does not
#
# CLEANS: attached images and `rw.*.dmg` files **whose backing path is inside this
# repo's target directory**. That scoping is the whole safety argument — this Mac
# has iOS simulator runtimes mounted, and a `detach` that matched on `/Volumes/dmg.*`
# would eject somebody else's disk. Never widen the match to a volume NAME; the
# name is random and tells you nothing about the owner.
#
# DOES NOT clean: the previous finished `.dmg`. A build that fails after we deleted
# it would leave the developer with neither, and "the old artefact survived a failed
# build" is worth more than a tidy directory. It is only WARNED about, because
# §A0's "exactly one dmg" check reads that directory.
set -euo pipefail

cd "$(dirname "$0")/.."
BUNDLE="src-tauri/target/universal-apple-darwin/release/bundle"
MACOS_DIR="$BUNDLE/macos"
DMG_DIR="$BUNDLE/dmg"
TARGET_ABS="$(cd src-tauri/target 2>/dev/null && pwd || true)"

detached=0
if [ -n "$TARGET_ABS" ]; then
  # One block per attached image; `image-path` comes first, the /dev/diskN lines
  # after. Emit devices ONLY for blocks whose image lives under our target dir.
  while read -r dev; do
    [ -n "$dev" ] || continue
    # Already gone? Detaching the whole disk takes its slices with it, and
    # `hdiutil info` lists both — the first version of this treated the
    # now-vanished slice as a fatal error and refused to build at all. Found by
    # planting the exact stale-volume state this script exists to clear, which is
    # the only way it would have been found before a developer hit it.
    if ! diskutil info "$dev" >/dev/null 2>&1; then
      continue
    fi
    echo "pre-clean: detaching stale build volume $dev"
    if ! hdiutil detach "$dev" -force >/dev/null 2>&1; then
      # Re-check rather than trust the exit code: a detach that races with the
      # parent disk going away "fails" having achieved the goal.
      if diskutil info "$dev" >/dev/null 2>&1; then
        echo "pre-clean: could not detach $dev — eject it in Finder and re-run" >&2
        exit 1
      fi
    fi
    detached=$((detached + 1))
  done < <(hdiutil info | awk -v pfx="$TARGET_ABS" '
      /^={4,}/          { ours = 0; next }
      /^image-path/     { ours = (index($0, pfx) > 0); next }
      # WHOLE DISK only (/dev/diskN, never /dev/diskNsM): detaching the disk
      # takes its slices, and asking for the slice afterwards is asking about
      # something that no longer exists.
      ours && /^\/dev\/disk[0-9]+[[:space:]]/ { print $1 }
  ')
fi

removed=0
if [ -d "$MACOS_DIR" ]; then
  while IFS= read -r f; do
    [ -n "$f" ] || continue
    echo "pre-clean: removing stale temp image $(basename "$f")"
    rm -f "$f"
    removed=$((removed + 1))
  done < <(find "$MACOS_DIR" -maxdepth 1 -name 'rw.*.dmg' 2>/dev/null)
fi

# §A0 asserts exactly one dmg in that directory. A leftover from another VERSION
# would fail it for a reason that has nothing to do with this build, so say so
# now rather than letting the integrity check take the blame.
if [ -d "$DMG_DIR" ]; then
  n="$(find "$DMG_DIR" -maxdepth 1 -name '*.dmg' | wc -l | tr -d ' ')"
  if [ "$n" -gt 1 ]; then
    echo "pre-clean: WARNING — $n dmgs already in $DMG_DIR:" >&2
    find "$DMG_DIR" -maxdepth 1 -name '*.dmg' -exec basename {} \; >&2
    echo "  §A0 requires exactly one. Delete the stale version(s) before publishing." >&2
  fi
fi

if [ "$detached" -eq 0 ] && [ "$removed" -eq 0 ]; then
  echo "pre-clean: nothing stale"
fi

exec npx tauri build --target universal-apple-darwin "$@"

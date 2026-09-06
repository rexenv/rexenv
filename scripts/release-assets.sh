#!/bin/bash
# Everything a release needs BESIDE the dmg, and the checks that make it safe to
# publish. Run automatically by `scripts/release-mac.sh` after `tauri build`.
#
#   scripts/release-assets.sh            # after a real build
#   scripts/release-assets.sh --check     # verify what is already there; build nothing
#
# # What it produces, and why the app cannot update from the dmg
#
# `rexenv_<V>_universal.app.tar.gz` — the bundle itself, tarred. A self-update
# replaces a DIRECTORY, so it needs the directory; a dmg would mean mounting a
# disk image inside the app, keeping it mounted across the swap, and a
# quarantine-aware copy tool. The tar is a byte copy of the SIGNED bundle, so a
# universal build stays universal and the ad-hoc signature travels with it.
#
# # The layout is load-bearing, not a detail
#
# The extractor strips exactly one leading component, so the archive must hold
# exactly one top-level `rexenv.app/` entry. A `./` prefix, a wrapper directory,
# or AppleDouble `._` members (what bsdtar writes without COPYFILE_DISABLE) each
# produce a broken bundle or a failed install — upstream has issues for both
# shapes. Asserted below rather than assumed.
#
# # And it re-runs §A0 on the EXTRACTED bundle
#
# PUBLISH-TESTING §A0 checks the .app that `tauri build` produced. That is not
# the thing users of the updater receive: they receive whatever comes back out
# of this archive. Checking the artefact and shipping a different one is the
# gap this closes — the same reason the cask's hash is computed from the
# DOWNLOADED asset rather than the local build.
set -euo pipefail

cd "$(dirname "$0")/.."

CHECK_ONLY=0
[ "${1:-}" = "--check" ] && CHECK_ONLY=1

BUNDLE="src-tauri/target/universal-apple-darwin/release/bundle"
MACOS_DIR="$BUNDLE/macos"
DMG_DIR="$BUNDLE/dmg"
APP="$MACOS_DIR/rexenv.app"

V="$(sed -n 's/^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' src-tauri/tauri.conf.json | head -1)"
TAR="$MACOS_DIR/rexenv_${V}_universal.app.tar.gz"
DMG="$DMG_DIR/rexenv_${V}_universal.dmg"

fail() { echo "release-assets: $*" >&2; exit 1; }

[ -d "$APP" ] || fail "no built bundle at $APP — run pnpm release:mac"

# ── 1. One version, everywhere, INCLUDING the bundle that was just built ──────
./scripts/check-versions.sh --built "$V"

# ── 2. Exactly one of each, so nothing stale is published by accident ─────────
n_app=$(find "$MACOS_DIR" -maxdepth 1 -name '*.app' | wc -l | tr -d ' ')
[ "$n_app" = "1" ] || fail "$n_app .app bundles in $MACOS_DIR — §A0 requires exactly one"
if [ -d "$DMG_DIR" ]; then
  n_dmg=$(find "$DMG_DIR" -maxdepth 1 -name '*.dmg' | wc -l | tr -d ' ')
  [ "$n_dmg" -le 1 ] || fail "$n_dmg dmgs in $DMG_DIR — delete the stale one before publishing"
fi

# ── 3. The archive ───────────────────────────────────────────────────────────
if [ "$CHECK_ONLY" = "0" ]; then
  rm -f "$MACOS_DIR"/rexenv_*_universal.app.tar.gz
  # COPYFILE_DISABLE: without it bsdtar writes an AppleDouble `._rexenv.app`
  # member, which the in-app extractor cannot place and which fails the whole
  # install. `-C` so the archive root is the bundle itself, never `./`.
  COPYFILE_DISABLE=1 tar -C "$MACOS_DIR" -czf "$TAR" "rexenv.app"
fi
[ -f "$TAR" ] || fail "no archive at $TAR"

first="$(tar -tzf "$TAR" | head -1)"
[ "$first" = "rexenv.app/" ] || fail "the archive's first entry is '$first', not 'rexenv.app/'"
if tar -tzf "$TAR" | grep -q '/\._\|^\._'; then
  fail "the archive carries AppleDouble (._) members — rebuild with COPYFILE_DISABLE=1"
fi
tops="$(tar -tzf "$TAR" | awk -F/ '{print $1}' | sort -u | wc -l | tr -d ' ')"
[ "$tops" = "1" ] || fail "the archive has $tops top-level entries — the extractor strips exactly one"

# ── 4. §A0, on the bundle that comes back OUT of the archive ─────────────────
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
tar -xzf "$TAR" -C "$TMP"
OUT="$TMP/rexenv.app"
[ -d "$OUT" ] || fail "the archive did not extract to a rexenv.app"

for bin in rexenv rex; do
  archs="$(lipo -archs "$OUT/Contents/MacOS/$bin" 2>/dev/null || true)"
  [ "$archs" = "x86_64 arm64" ] || fail "extracted Contents/MacOS/$bin is '$archs', not universal"
done
# Per SLICE, not merely somewhere in the fat binary: a half-populated universal
# is invisible to `lipo -archs`, which is the arm64-dmg mistake in a subtler form.
for arch in arm64 x86_64; do
  lipo -thin "$arch" "$OUT/Contents/MacOS/rexenv" -output "$TMP/rexenv-$arch"
  n=$(strings -a "$TMP/rexenv-$arch" | grep -c Dist_Archive_Command || true)
  [ "$n" -gt 0 ] || fail "Dist_Archive_Command missing from the extracted $arch slice"
  # The update key must be in BOTH slices too: a build whose Intel half cannot
  # verify a descriptor is a build that silently never updates on Intel.
  k=$(strings -a "$TMP/rexenv-$arch" | grep -c "$(sed -n 's/^const RELEASE_PUBKEY: &str = "\(.*\)";/\1/p' src-tauri/src/core/updates.rs | head -1)" || true)
  [ "$k" -gt 0 ] || fail "the update public key is missing from the extracted $arch slice"
  rm -f "$TMP/rexenv-$arch"
done
codesign --verify --deep --strict "$OUT" || fail "the extracted bundle fails its own signature"
plist_v="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$OUT/Contents/Info.plist")"
[ "$plist_v" = "$V" ] || fail "the extracted bundle says $plist_v, the release is $V"

# ── 5. Sidecars, and the exact publish line ──────────────────────────────────
if [ "$CHECK_ONLY" = "0" ]; then
  ( cd "$MACOS_DIR" && shasum -a 256 "$(basename "$TAR")" \
      | awk '{print $1 "  " $2}' > "$(basename "$TAR").sha256" )
fi
TAR_SHA="$(shasum -a 256 "$TAR" | awk '{print $1}')"

echo
echo "release-assets: all green"
echo "  version   $V"
echo "  archive   $TAR"
echo "  sha256    $TAR_SHA"
echo "  size      $(stat -f%z "$TAR") bytes"
echo
echo "Publish (assets never end in _universal.dmg except the dmg — the tap's"
echo "update-cask.yml selects by that suffix):"
echo
echo "  gh release create \"v$V\" --repo rexenv/homebrew-tap --draft \\"
echo "    --title \"rexenv $V\" \\"
echo "    \"$DMG\" \\"
echo "    \"$DMG.sha256\" \\"
echo "    \"$TAR\" \\"
echo "    \"$TAR.sha256\""
echo
echo "Then: publish the draft (that IS the §A sign-off), and run"
echo "rexenv/runtimes → Actions → 'Publish app update manifest' (dry run first)."
echo "Finally: scripts/check-app-manifest.sh"

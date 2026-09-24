#!/bin/bash
# Artefact integrity for the Linux release — §A0's Linux half.
#
#   ./scripts/release-linux-check.sh           (release-linux.sh ends with this)
#   ./scripts/release-linux-check.sh --check   (verify what is there; build nothing)
#
# # What a Linux release is, and what the app will do with it
#
# Two artefacts per arch, both made by `tauri build` on a host of that arch:
#
#   rexenv_<V>_<amd64|arm64>.deb         installed by dpkg — and REPLACED by dpkg in the
#                                        in-app update (`dpkg -i` through the polkit step)
#   rexenv_<V>_<amd64|aarch64>.AppImage  one user-owned file — replaced in place by the
#                                        in-app update (`renameat2` exchange)
#
# The in-app updater verifies the SAME facts this script asserts, on the user's
# machine, before it swaps anything (`platform/linux/app_bundle.rs`): the package is
# `rexenv` at the released version for this arch and carries `usr/bin/rexenv` +
# `usr/bin/rex`; the image answers `--print-version` with the released version. So a
# release that fails here is one every installed copy would have refused — better to
# learn that from the build than from a user's "update failed" dialog. Tauri's deb
# bundler spells members WITHOUT a leading `./` (dpkg-deb spells them with it); the
# app accepts both, and so does this.
#
# macOS checks that a universal binary carries both slices; Windows reads the PE
# machine field. Here `file(1)` reads the ELF's, and the arch is the HOST's — nothing
# cross-compiles, so a build claims exactly the arch it ran on.
#
# Add a line here whenever something new is compiled into the binary or bundled
# beside it.
set -euo pipefail

cd "$(dirname "$0")/.."
CHECK_ONLY=0
[ "${1:-}" = "--check" ] && CHECK_ONLY=1

REL="src-tauri/target/release"
BUNDLE="$REL/bundle"
VERSION="$(node -p "require('./src-tauri/tauri.conf.json').version")"

fail() { echo "::error::$*" >&2; exit 1; }

case "$(uname -m)" in
  x86_64)  DEB_ARCH=amd64; IMG_ARCH=amd64;   ELF_WORD="x86-64" ;;
  aarch64) DEB_ARCH=arm64; IMG_ARCH=aarch64; ELF_WORD="aarch64" ;;
  *) fail "unsupported host arch $(uname -m)" ;;
esac
DEB="$BUNDLE/deb/rexenv_${VERSION}_${DEB_ARCH}.deb"
IMG="$BUNDLE/appimage/rexenv_${VERSION}_${IMG_ARCH}.AppImage"

for tool in dpkg-deb file; do
  command -v "$tool" >/dev/null 2>&1 || fail "$tool is missing"
done

# The update key must be in the shipped binary, or the build silently never
# updates. Read from the source so the two cannot drift.
PUBKEY="$(sed -n 's/^const RELEASE_PUBKEY: &str = "\(.*\)";/\1/p' src-tauri/src/core/updates.rs | head -1)"
[ -n "$PUBKEY" ] || fail "could not read RELEASE_PUBKEY from src-tauri/src/core/updates.rs"

# (1) Exactly one deb and one AppImage, named for the version and THIS arch. Two of
#     either would leave the release step to pick, and it would pick wrong eventually.
[ -f "$DEB" ] || { ls -la "$BUNDLE/deb" 2>/dev/null >&2; fail "expected package not found: $DEB"; }
[ -f "$IMG" ] || { ls -la "$BUNDLE/appimage" 2>/dev/null >&2; fail "expected image not found: $IMG"; }
n=$(find "$BUNDLE/deb" -maxdepth 1 -name '*.deb' | wc -l | tr -d ' ')
[ "$n" -eq 1 ] || fail "expected exactly one .deb in $BUNDLE/deb, found $n"
n=$(find "$BUNDLE/appimage" -maxdepth 1 -name '*.AppImage' | wc -l | tr -d ' ')
[ "$n" -eq 1 ] || fail "expected exactly one .AppImage in $BUNDLE/appimage, found $n"

# (2) The `rex` sidecar rode along. Tauri copies `binaries/rex-<triple>` to `rex`
#     beside the app (`build.rs` stages it from `build-cli.sh`); if either half
#     silently did nothing the app ships without its CLI and nothing else notices.
[ -f "$REL/rex" ] || fail "the rex sidecar is missing from $REL — build-cli.sh or build.rs did nothing"

# (3) This arch, read off the ELF header rather than off the filename.
for f in "$REL/rexenv" "$REL/rex"; do
  file "$f" | grep -q "$ELF_WORD" || fail "$f is not $ELF_WORD: $(file "$f")"
done
echo "ELF: $ELF_WORD (rexenv, rex)"

# (4) The payloads compiled INTO the binary are in it: the vendored `wp dist-archive`
#     tree (`include_bytes!` via build.rs) and the update public key.
[ "$(grep -ac "Dist_Archive_Command" "$REL/rexenv" || true)" -gt 0 ] || fail "Dist_Archive_Command missing from rexenv — the vendored wp tree did not embed"
[ "$(grep -ac "$PUBKEY" "$REL/rexenv" || true)" -gt 0 ] || fail "the update public key is missing from rexenv — this build would never update"
echo "embedded: Dist_Archive_Command + RELEASE_PUBKEY present"

# (5) The deb: what the in-app updater reads before `dpkg -i` (control fields and the
#     two members), plus the polkit action file — without it every privileged step
#     shows polkit's generic sentence instead of rexenv's (docs/INSTALL.md promises
#     rexenv's), and the `Depends` line names the runtime libraries the webview and
#     the tray need (a deb without them installs and then fails to start).
control="$(dpkg-deb -f "$DEB")"
field() { printf '%s\n' "$control" | sed -n "s/^$1: *//p" | head -1; }
[ "$(field Package)" = "rexenv" ] || fail "the package is '$(field Package)', not rexenv"
[ "$(field Version)" = "$VERSION" ] || fail "the package says Version '$(field Version)', the release is $VERSION"
[ "$(field Architecture)" = "$DEB_ARCH" ] || fail "the package is built for '$(field Architecture)', this host is $DEB_ARCH"
members="$(dpkg-deb -c "$DEB" | awk '{print $NF}' | sed 's#^\./##')"
for m in usr/bin/rexenv usr/bin/rex usr/share/polkit-1/actions/dev.rexenv.rexenv.policy usr/share/applications/rexenv.desktop; do
  printf '%s\n' "$members" | grep -qx "$m" || fail "the package is missing $m"
done
for dep in libwebkit2gtk-4.1-0 libayatana-appindicator3-1 policykit-1 libnss3-tools; do
  printf '%s' "$(field Depends)" | grep -q "$dep" || fail "the package's Depends lacks $dep: $(field Depends)"
done
echo "deb: rexenv $VERSION $DEB_ARCH — usr/bin/{rexenv,rex}, the polkit action, the desktop entry, Depends complete"

# (6) The AppImage: a type-2 image (ELF with the magic at offset 8) that ANSWERS
#     `--print-version` with the released version — the exact check the updater runs
#     on a staged image, and run the way the updater runs it (extract-and-run, so a
#     host without libfuse2 still answers).
magic="$(dd if="$IMG" bs=1 skip=8 count=3 2>/dev/null | od -An -c | tr -d ' ')"
[ "$magic" = 'AI002' ] || fail "$IMG is not a type-2 AppImage (magic at offset 8: '$magic')"
file "$IMG" | grep -q "$ELF_WORD" || fail "$IMG is not $ELF_WORD: $(file "$IMG")"
chmod +x "$IMG"
pv="$(APPIMAGE_EXTRACT_AND_RUN=1 timeout 60 "$IMG" --print-version 2>/dev/null | tr -d '\r' | tail -1 || true)"
[ "$pv" = "$VERSION" ] || fail "the AppImage answers --print-version '$pv', the release is $VERSION"
echo "AppImage: type 2, $ELF_WORD, --print-version $pv"

# (7) The deb's binary answers the same — extracted, never installed (this runs
#     beside a real install on the release machine).
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
dpkg-deb -x "$DEB" "$TMP"
pv="$("$TMP/usr/bin/rexenv" --print-version 2>/dev/null | tail -1 || true)"
[ "$pv" = "$VERSION" ] || fail "the packaged rexenv answers --print-version '$pv', the release is $VERSION"
[ "$(grep -ac "$PUBKEY" "$TMP/usr/bin/rexenv" || true)" -gt 0 ] || fail "the update public key is missing from the packaged rexenv"

# (8) Digest sidecars, the names the tap release carries.
if [ "$CHECK_ONLY" = "0" ]; then
  ( cd "$BUNDLE/deb" && sha256sum "$(basename "$DEB")" > "$(basename "$DEB").sha256" )
  ( cd "$BUNDLE/appimage" && sha256sum "$(basename "$IMG")" > "$(basename "$IMG").sha256" )
fi

echo "§A0-linux: all green"
echo "  deb       $DEB  $(sha256sum "$DEB" | cut -c1-64)  ($(( $(wc -c < "$DEB") / 1048576 )) MB)"
echo "  AppImage  $IMG  $(sha256sum "$IMG" | cut -c1-64)  ($(( $(wc -c < "$IMG") / 1048576 )) MB)"

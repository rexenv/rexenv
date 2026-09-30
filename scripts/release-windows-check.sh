#!/bin/bash
# Artefact integrity for the Windows release — §A0's Windows half.
#
#   ./scripts/release-windows-check.sh           (release-windows.sh ends with this)
#   ./scripts/release-windows-check.sh --check   (verify what is there; build nothing)
#
# # What it produces beside the installer, and why the app cannot update from it
#
# `rexenv_<V>_x64.zip` — the install DIRECTORY's contents, zipped flat: `rexenv.exe`
# and `rex.exe` at the archive root, nothing else. A self-update replaces that
# directory (`platform/windows/app_bundle.rs`), so it needs the files, not an
# installer it would have to run; and `uninstall.exe` is deliberately ABSENT — the
# installer writes it at install time and the swap carries the installed one across.
# The extractor strips nothing, so a wrapper directory in the archive would land the
# executable one level too deep. Asserted below rather than assumed, and the archive
# is extracted and re-checked, because what a user receives is what comes OUT of it.
#
# macOS checks that a UNIVERSAL binary really carries both slices and that anything
# compiled in is in EACH of them. Windows ships one architecture, so the question is
# different but the principle is the same: prove the things that are supposed to be
# inside the artefact are inside it, rather than trusting that the build said so.
#
# Add a line here whenever something new is compiled into the binary or bundled
# beside it.
set -euo pipefail

cd "$(dirname "$0")/.."
CHECK_ONLY=0
[ "${1:-}" = "--check" ] && CHECK_ONLY=1

REL="src-tauri/target/release"
BUNDLE="$REL/bundle/nsis"
VERSION="$(node -p "require('./src-tauri/tauri.conf.json').version")"
SETUP="$BUNDLE/rexenv_${VERSION}_x64-setup.exe"
ZIP="$BUNDLE/rexenv_${VERSION}_x64.zip"

fail() { echo "::error::$*" >&2; exit 1; }

# The update key must be in the shipped binary, or the build silently never
# updates. Read from the source so the two cannot drift.
PUBKEY="$(sed -n 's/^const RELEASE_PUBKEY: &str = "\(.*\)";/\1/p' src-tauri/src/core/updates.rs | head -1)"
[ -n "$PUBKEY" ] || fail "could not read RELEASE_PUBKEY from src-tauri/src/core/updates.rs"

# (1) Exactly one installer, named for the version being released. Two would leave
#     the release step to pick, and it would pick wrong eventually.
[ -f "$SETUP" ] || { ls -la "$BUNDLE" 2>/dev/null >&2; fail "expected installer not found: $SETUP"; }
n=$(find "$BUNDLE" -maxdepth 1 -name '*-setup.exe' | wc -l | tr -d ' ')
[ "$n" -eq 1 ] || fail "expected exactly one installer in $BUNDLE, found $n"

# (2) The `rex` sidecar rode along. Tauri copies `binaries/rex-<triple>.exe` to
#     `rex.exe` beside the app, which is where `core::cli::bundled_rex` looks — and
#     `build.rs` stages it from `build-cli.sh`. If either half silently did nothing
#     the app ships without its CLI and nothing else notices.
[ -f "$REL/rex.exe" ] || fail "the rex sidecar is missing from $REL — build-cli.sh or build.rs did nothing"

# (3) x64, read off the PE header rather than off the filename. `lipo -archs` is
#     the macOS equivalent; here the machine field is two bytes after "PE\0\0",
#     whose offset lives at 0x3C. 0x8664 is AMD64.
pe_machine() {
  local f="$1"
  local off
  off=$(od -An -t u4 -j 60 -N 4 "$f" | tr -d ' ')
  od -An -t x2 -j $((off + 4)) -N 2 "$f" | tr -d ' '
}
for f in "$REL/rexenv.exe" "$REL/rex.exe"; do
  m="$(pe_machine "$f")"
  [ "$m" = "8664" ] || fail "$f is not x64 (PE machine 0x$m)"
done
echo "PE machine: x64 (rexenv.exe, rex.exe)"

# (4) The payloads compiled INTO the binary are in it. Today's list: the vendored
#     `wp dist-archive` tree, which `build.rs` turns into `include_bytes!`. Grepped
#     as binary — `strings` is not guaranteed on a Git Bash host.
hits=$(grep -ac "Dist_Archive_Command" "$REL/rexenv.exe" || true)
[ "${hits:-0}" -gt 0 ] || fail "Dist_Archive_Command missing from rexenv.exe — the vendored wp tree did not embed"
echo "embedded: Dist_Archive_Command present"

# (5) UNSIGNED, deliberately — and asserted, because `docs/INSTALL.md` promises the
#     user a specific dialog ("Unknown Publisher", Run on the first screen) and that
#     page becomes a lie the day a certificate appears without it being rewritten.
#     D5, ruled 19 Sep 2026: rexenv is open source, earns nothing, spends nothing.
sig=$(powershell -NoProfile -Command "(Get-AuthenticodeSignature '$SETUP').Status" 2>/dev/null | tr -d '\r')
[ "$sig" = "NotSigned" ] || fail "installer signature is '$sig', not NotSigned — D5 says unsigned, and docs/INSTALL.md describes the unsigned dialog"
echo "signature: NotSigned (as ruled)"
k=$(grep -ac "$PUBKEY" "$REL/rexenv.exe" || true)
[ "${k:-0}" -gt 0 ] || fail "the update public key is missing from rexenv.exe — this build would never update"
echo "embedded: RELEASE_PUBKEY present"

# (6) The update archive: flat, exactly the two binaries, and re-checked from
#     what comes back out of it. Built with Python's zipfile — present on the Dell
#     and on windows-latest, where Git Bash has no `zip`. A fixed member order so
#     the same inputs give the same archive.
if [ "$CHECK_ONLY" = "0" ]; then
  rm -f "$BUNDLE"/rexenv_*_x64.zip "$BUNDLE"/rexenv_*_x64.zip.sha256
  # Absolute BEFORE the subshell's cd — resolved inside it, the path doubled
  # (`…/target/release/src-tauri/target/release/…`) on the first Dell run.
  ZIP_ABS="$(pwd -W 2>/dev/null || pwd)/$ZIP"
  ( cd "$REL" && python -c "
import zipfile, sys
with zipfile.ZipFile(sys.argv[1], 'w', zipfile.ZIP_DEFLATED) as z:
    for name in ('rexenv.exe', 'rex.exe'):
        z.write(name, name)
" "$ZIP_ABS" )
fi
[ -f "$ZIP" ] || fail "no update archive at $ZIP"
# `tr -d '\r'`: Python on Windows prints CRLF, and "rexenv.exe\r" is not "rexenv.exe"
# — the first Dell run refused its own correct archive on exactly that.
members="$(python -c "import zipfile,sys; print('\n'.join(zipfile.ZipFile(sys.argv[1]).namelist()))" "$ZIP" | tr -d '\r')"
[ "$members" = $'rexenv.exe\nrex.exe' ] || fail "the archive's members are not exactly rexenv.exe + rex.exe (flat):
$members"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
python -c "import zipfile,sys; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])" "$ZIP" "$TMP"
for f in rexenv.exe rex.exe; do
  [ -f "$TMP/$f" ] || fail "extracted archive lacks $f"
  m="$(pe_machine "$TMP/$f")"
  [ "$m" = "8664" ] || fail "extracted $f is not x64 (PE machine 0x$m)"
done
[ "$(grep -ac Dist_Archive_Command "$TMP/rexenv.exe" || true)" -gt 0 ] || fail "Dist_Archive_Command missing from the extracted rexenv.exe"
[ "$(grep -ac "$PUBKEY" "$TMP/rexenv.exe" || true)" -gt 0 ] || fail "the update public key is missing from the extracted rexenv.exe"
# The version the swap will read: the executable's OWN VERSIONINFO, the same
# words `app_bundle.rs` reads, so a wrong stamp is caught here rather than after
# a user's download.
pv="$(powershell -NoProfile -Command "(Get-Item '$(cd "$TMP" && pwd -W 2>/dev/null || echo "$TMP")\\rexenv.exe').VersionInfo.ProductVersion" 2>/dev/null | tr -d '\r')"
[ "$pv" = "$VERSION" ] || fail "the extracted rexenv.exe says ProductVersion '$pv', the release is $VERSION"
pn="$(powershell -NoProfile -Command "(Get-Item '$(cd "$TMP" && pwd -W 2>/dev/null || echo "$TMP")\\rexenv.exe').VersionInfo.ProductName" 2>/dev/null | tr -d '\r')"
[ "$pn" = "rexenv" ] || fail "the extracted rexenv.exe says ProductName '$pn', not rexenv — the swap verifies this word"
echo "archive: flat, x64, version $pv, product $pn"

if [ "$CHECK_ONLY" = "0" ]; then
  # The NAME is spelled here, never taken from sha256sum's second column: Git Bash's
  # sha256sum hashes in binary mode on Windows and prints `<hash> *name`, and `$2`
  # carried that `*` into the file — release.yml's publish job then ran `sha256sum -c`
  # on the Linux runner, which read a file called `*rexenv_0.8.8_x64.zip` and refused
  # the whole draft (run 36324734214, 27 Sep 2026). Two spaces = GNU text mode.
  ( cd "$BUNDLE" && sha256sum "$(basename "$ZIP")" | awk -v name="$(basename "$ZIP")" '{print $1 "  " name}' > "$(basename "$ZIP").sha256" )
fi
ZIP_SHA="$(sha256sum "$ZIP" | awk '{print $1}')"

echo "§A0-windows: all green — $(basename "$SETUP") ($(( $(wc -c < "$SETUP") / 1048576 )) MB)"
echo "  update archive  $ZIP"
echo "  sha256          $ZIP_SHA"
echo "  size            $(wc -c < "$ZIP" | tr -d ' ') bytes"
echo
echo "Publish beside the macOS assets on the SAME tag (the tap's update-cask.yml"
echo "selects the dmg by its _universal.dmg suffix; nothing here ends in that):"
echo "  gh release upload \"v$VERSION\" --repo rexenv/rexenv \\"
echo "    \"$SETUP\" \"$SETUP.sha256\" \"$ZIP\" \"$ZIP.sha256\""
echo "Then: rexenv/runtimes → 'Publish app update manifest' for WINDOWS"
echo "(app-manifest-windows.json), and scripts/check-app-manifest.sh --windows"

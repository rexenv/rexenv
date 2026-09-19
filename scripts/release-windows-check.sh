#!/bin/bash
# Artefact integrity for the Windows release — §A0's Windows half.
#
#   ./scripts/release-windows-check.sh        (release-windows.sh ends with this)
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
REL="src-tauri/target/release"
BUNDLE="$REL/bundle/nsis"
VERSION="$(node -p "require('./src-tauri/tauri.conf.json').version")"
SETUP="$BUNDLE/rexenv_${VERSION}_x64-setup.exe"

fail() { echo "::error::$*" >&2; exit 1; }

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

echo "§A0-windows: all green — $(basename "$SETUP") ($(( $(wc -c < "$SETUP") / 1048576 )) MB)"

#!/bin/bash
# Offline proof that scripts/check-app-manifest.sh tells CDN lag from a forgotten
# publish (ledger #594).
#
# Releasing 0.7.1 the check read raw.githubusercontent.com's cached copy two
# minutes after a correct publish and told the releaser to publish again. A real
# CDN window cannot be summoned on demand, so this builds one: two descriptors
# signed with a throwaway ed25519 key, served over file:// — one as "what the CDN
# returns", one as "what is committed" — and the tap's latest version given
# directly. No network, no gh, no release key (tests must never hold its private
# half). Exit code is the verdict.
set -euo pipefail

# ── Which `openssl`: one that knows `pkeyutl -rawin` (ed25519 over raw bytes). macOS ships
# LibreSSL as `openssl`, which does not — the GitHub macos-14 runner failed here with
# "pkeyutl: Option unknown option -rawin" (27 Sep 2026) while every developer Mac had
# Homebrew's OpenSSL 3 first on PATH. Asked, not assumed: the first candidate whose
# `pkeyutl -help` lists `-rawin` wins; none is a named refusal, never a confusing exit.
OPENSSL=""
# Probed by DOING, not by name or help text: the two earlier probes each read something
# other than the capability — `-help` (OpenSSL 3.0.2 has -rawin and does not list it) and
# `version` (matched, and the macOS runner still signed with LibreSSL, 27 Sep 2026). A
# throwaway ed25519 key signed with `pkeyutl -sign -rawin` either works or it does not.
_probe="$(mktemp -d)"
for c in openssl /opt/homebrew/opt/openssl@3/bin/openssl /usr/local/opt/openssl@3/bin/openssl /opt/homebrew/bin/openssl; do
  command -v "$c" >/dev/null 2>&1 || continue
  if "$c" genpkey -algorithm ed25519 -out "$_probe/k.pem" >/dev/null 2>&1 \
     && printf 'x' > "$_probe/m" \
     && "$c" pkeyutl -sign -inkey "$_probe/k.pem" -rawin -in "$_probe/m" -out "$_probe/s" >/dev/null 2>&1 \
     && [ -s "$_probe/s" ]; then
    OPENSSL="$c"; break
  fi
done
rm -rf "$_probe"
[ -n "$OPENSSL" ] || { echo "$(basename "$0"): no openssl with 'pkeyutl -rawin' found (macOS's LibreSSL lacks it) — brew install openssl@3" >&2; exit 1; }


cd "$(dirname "$0")/.."

T="$(mktemp -d)"
trap 'rm -rf "$T"' EXIT

"$OPENSSL" genpkey -algorithm ed25519 -out "$T/key.pem" 2>/dev/null
PUB="$("$OPENSSL" pkey -in "$T/key.pem" -pubout -outform DER | tail -c 32 | xxd -p -c 64)"
[ "${#PUB}" = "64" ] || { echo "check-app-manifest-test: could not derive a test public key" >&2; exit 1; }

# descriptor <dir> <serial> <version> [floor]: a signed document shaped like the published one.
descriptor() {
  local floor="${4:-13.0}"
  mkdir -p "$1"
  cat > "$1/app-manifest.json" <<EOF
{
  "serial": $2,
  "generatedAt": "2026-09-13T10:37:29Z",
  "release": {
    "version": "$3",
    "url": "https://github.com/rexenv/rexenv/releases/download/v$3/rexenv_$3_universal.app.tar.gz",
    "sha256": "e21b3525e7d1de3d04f27f29d04560a60cc84ce476137b1d45eed095c9fce859",
    "sizeBytes": 29014289,
    "minAppVersion": "",
    "minimumSystemVersion": "$floor",
    "notes": "",
    "publishedAt": "2026-09-13T10:34:13Z"
  }
}
EOF
  "$OPENSSL" pkeyutl -sign -inkey "$T/key.pem" -rawin -in "$1/app-manifest.json" | xxd -p -c 256 > "$1/app-manifest.json.sig"
}

descriptor "$T/old" 3 0.7.0
descriptor "$T/new" 4 0.7.1
# The floor the publisher restated by hand, and got wrong (0.8.7, 24 Sep 2026).
descriptor "$T/floor" 4 0.7.1 15.0
# A committed file whose signature does not match its bytes.
mkdir -p "$T/badsig" && cp "$T/new/app-manifest.json" "$T/badsig/" && cp "$T/old/app-manifest.json.sig" "$T/badsig/"

# A file:// URL for a path on THIS host. Under Git Bash the fixture path is a POSIX
# one (/tmp/…) that native curl.exe resolves against the drive root — so every
# fixture URL fetches nothing, and the failure is SILENT in the worst possible way:
# "the CDN has nothing published" is a green exit here, so the whole test would have
# passed while measuring nothing. Measured on the Dell 17 Sep 2026: plain
# `file:///tmp/x` returns empty, `file:///$(cygpath -m /tmp/x)` returns the bytes.
# `cygpath` exists only on MSYS, so macOS keeps the plain form.
file_url() {
  if command -v cygpath >/dev/null 2>&1; then
    printf 'file:///%s' "$(cygpath -m "$1")"
  else
    printf 'file://%s' "$1"
  fi
}

# run <cdn dir> <committed dir> <tap latest>
run() {
  CHECK_APP_MANIFEST_PUBKEY="$PUB" CHECK_APP_MANIFEST_OFFLINE=1 CHECK_APP_MANIFEST_TAP_LATEST="$3" \
  CHECK_APP_MANIFEST_FLOOR=13.0 \
  CHECK_APP_MANIFEST_DOC_URL="$(file_url "$1/app-manifest.json")" \
  CHECK_APP_MANIFEST_API_DOC_URL="$(file_url "$2/app-manifest.json")" \
    ./scripts/check-app-manifest.sh 2>&1
}

FAILS=0
check() { # <case> <what> <condition 0/1>
  if [ "$3" = "1" ]; then echo "  ✓ $1: $2"; else echo "  ✗ $1: $2"; FAILS=$((FAILS + 1)); fi
}
has() { grep -qF -- "$2" <<<"$1" && echo 1 || echo 0; }
lacks() { grep -qF -- "$2" <<<"$1" && echo 0 || echo 1; }

code=0; out="$(run "$T/old" "$T/new" 0.7.1)" || code=$?
check "CDN behind a correct publish" "says only the CDN is behind" "$(has "$out" "only the CDN is behind")"
check "CDN behind a correct publish" "does NOT tell anyone to publish again" "$(lacks "$out" "forgotten-second-click")"
check "CDN behind a correct publish" "does not call it all green" "$(lacks "$out" "all green")"
check "CDN behind a correct publish" "exit 0" "$([ "$code" = 0 ] && echo 1 || echo 0)"

code=0; out="$(run "$T/old" "$T/old" 0.7.1)" || code=$?
check "publish really forgotten" "names the forgotten second click" "$(has "$out" "forgotten-second-click")"
check "publish really forgotten" "does not blame the CDN" "$(lacks "$out" "only the CDN is behind")"
check "publish really forgotten" "says why it is not CDN lag" "$(has "$out" "so this is not CDN lag")"

code=0; out="$(run "$T/old" "$T/nonexistent" 0.7.1)" || code=$?
check "committed file unreadable" "still warns about the missing publish" "$(has "$out" "forgotten-second-click")"
check "committed file unreadable" "says CDN lag was not ruled out" "$(has "$out" "CDN lag is not ruled out")"

code=0; out="$(run "$T/new" "$T/new" 0.7.1)" || code=$?
check "everything current" "all green" "$(has "$out" "all green")"
check "everything current" "no warning" "$(lacks "$out" "WARNING")"

code=0; out="$(run "$T/old" "$T/badsig" 0.7.1)" || code=$?
check "committed file badly signed" "fails instead of reassuring" "$([ "$code" != 0 ] && echo 1 || echo 0)"
check "committed file badly signed" "says the committed descriptor does not verify" "$(has "$out" "COMMITTED descriptor")"

code=0; out="$(run "$T/floor" "$T/floor" 0.7.1)" || code=$?
check "floor drifted from tauri.conf.json" "fails instead of reassuring" "$([ "$code" != 0 ] && echo 1 || echo 0)"
check "floor drifted from tauri.conf.json" "names both numbers" "$(has "$out" "is '15.0'; tauri.conf.json declares '13.0'")"
check "floor drifted from tauri.conf.json" "points at MIN_MACOS" "$(has "$out" "MIN_MACOS")"

if [ "$FAILS" -gt 0 ]; then
  echo "check-app-manifest-test: $FAILS check(s) FAILED"
  exit 1
fi
echo "check-app-manifest-test: all green"

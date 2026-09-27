#!/bin/bash
# Is the PUBLISHED app-update descriptor real, current, and signed by the key
# this build trusts?
#
#   scripts/check-app-manifest.sh
#
# # The failure this exists for
#
# Publishing a release takes two clicks in two repositories: the tap release
# (which is the §A sign-off) and, in `rexenv/runtimes`, the workflow that signs
# and commits the descriptor. Forget the second and everything looks finished —
# the dmg is downloadable, the cask bumps itself, the website updates — while
# every installed rexenv keeps answering "0.5.0 is what you are running". Nothing
# fails, nobody is told, and the feature that was just shipped is off.
#
# So this asks the three questions a person cannot answer by looking:
#
#   1. Does the descriptor verify against the key compiled into THIS source tree?
#      (A rotated key, or a publisher signing with the wrong half, is otherwise
#      invisible until a user's update fails.)
#   2. Does it name the version the tap actually published?
#   3. Is the artifact it names really there, with that digest?
#   4. Is its `minimumSystemVersion` the floor THIS source tree declares?
#      (The publisher cannot read our tauri.conf.json — a private repo — so it
#      restates the number by hand. 0.8.7 (24 Sep 2026) lowered the floor to 13.0
#      and the publisher stayed at 15.0: serial 13 was signed telling every 13/14
#      host that no later release will ever fit it. `NoOffer::NeedsNewerMacos`,
#      silently, forever. APP-MANIFEST.md had claimed this check existed.)
#
# # And the false alarm it used to raise
#
# Question 2 reads what apps read: raw.githubusercontent.com, which caches this
# file for up to five minutes. Releasing 0.7.1 (13 Sep 2026) this check ran two
# minutes after a correct publish, got the CDN's previous serial, and printed
# "this is the forgotten-second-click: run … 'Publish app update manifest'" —
# telling the releaser to publish a second time. The CDN served the new file
# 3 min 10 s after the commit. So before blaming a missed publish, this now reads
# the COMMITTED file through the contents API, verifies its signature too, and
# says "only the CDN is behind" when that is the truth (ledger #594).
#
# The same shape as `scripts/check-php-pins.sh`: the one check only this repo can
# make, because only this repo holds the pinned public key.
#
# Test seams (scripts/check-app-manifest-test.sh drives them offline, with file://
# fixtures and a throwaway key): CHECK_APP_MANIFEST_DOC_URL, _SIG_URL,
# _API_DOC_URL, _API_SIG_URL, _PUBKEY, _TAP_LATEST, _FLOOR, and _OFFLINE=1 (skip gh). A
# run with any of them set says so first, because it says nothing about the real
# descriptor.
set -euo pipefail

# ── Which `openssl`: one that knows `pkeyutl -rawin` (ed25519 over raw bytes). macOS ships
# LibreSSL as `openssl`, which does not — the GitHub macos-14 runner failed here with
# "pkeyutl: Option unknown option -rawin" (27 Sep 2026) while every developer Mac had
# Homebrew's OpenSSL 3 first on PATH. Asked, not assumed: the first candidate whose
# `pkeyutl -help` lists `-rawin` wins; none is a named refusal, never a confusing exit.
OPENSSL=""
for c in openssl /opt/homebrew/opt/openssl@3/bin/openssl /usr/local/opt/openssl@3/bin/openssl /opt/homebrew/bin/openssl; do
  # CAPTURED and matched, never `| grep -q`: under `set -o pipefail` a grep that exits at
  # the first match leaves the writer dying of SIGPIPE, the pipeline reports 141 and the
  # `if` reads "no -rawin" — how ubuntu-22.04's and windows-latest's OpenSSL 3, which have
  # it, were refused on 27 Sep 2026 (the nm|grep trap the ledger already records).
  command -v "$c" >/dev/null 2>&1 || continue
  # By NAME and version, not by grepping `-help`: OpenSSL 3.0.2's pkeyutl help does not list
  # `-rawin` although the flag works (ubuntu-22.04, 27 Sep 2026 — the Rust test that signs
  # through the same binary passed in the same run). `-rawin` exists in every OpenSSL since
  # 1.1.1; LibreSSL, which macOS calls `openssl`, has never had it.
  ver="$("$c" version 2>/dev/null || true)"
  case "$ver" in
    "OpenSSL 1.1.1"*|"OpenSSL 3"*|"OpenSSL 4"*) OPENSSL="$c"; break ;;
  esac
done
[ -n "$OPENSSL" ] || { echo "$(basename "$0"): no openssl with 'pkeyutl -rawin' found (macOS's LibreSSL lacks it) — brew install openssl@3" >&2; exit 1; }


cd "$(dirname "$0")/.."

# `--windows`: the SECOND descriptor. Each OS reads its own (`core::app_update::
# manifest_urls_on`), because one `release` names one artifact and the macOS one is
# a universal .app.tar.gz no Windows machine can use. Same key, same rules, same
# forgotten-second-click — so the same check, pointed at the other document.
# `--linux <deb|appimage> <x86_64|aarch64>`: one document per package kind and arch
# (`core::app_update::manifest_urls_for`, docs/PLAN-linux-port.md L7) — a `.deb` and an
# AppImage are different bytes, and so are the two archs. Empty floor, as Windows.
DOC_NAME="app-manifest.json"
case "${1:-}" in
  --windows) DOC_NAME="app-manifest-windows.json" ;;
  --linux)
    case "${2:-}/${3:-}" in
      deb/x86_64|deb/aarch64|appimage/x86_64|appimage/aarch64) DOC_NAME="app-manifest-linux-$2-$3.json" ;;
      *) echo "check-app-manifest: --linux takes <deb|appimage> <x86_64|aarch64>" >&2; exit 2 ;;
    esac ;;
esac

DOC_URL="${CHECK_APP_MANIFEST_DOC_URL:-https://raw.githubusercontent.com/rexenv/runtimes/main/$DOC_NAME}"
SIG_URL="${CHECK_APP_MANIFEST_SIG_URL:-$DOC_URL.sig}"
# The committed file itself, not the CDN's copy of it (API cache: 60 s, raw: 300 s).
API_DOC_URL="${CHECK_APP_MANIFEST_API_DOC_URL:-https://api.github.com/repos/rexenv/runtimes/contents/$DOC_NAME}"
API_SIG_URL="${CHECK_APP_MANIFEST_API_SIG_URL:-$API_DOC_URL.sig}"
TAP="rexenv/homebrew-tap"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail() { echo "check-app-manifest: $*" >&2; exit 1; }
WARNED=0
warn() { echo "check-app-manifest: WARNING — $*" >&2; WARNED=1; }

if env | grep -q '^CHECK_APP_MANIFEST_'; then
  echo "check-app-manifest: TEST overrides in effect — this run says nothing about the real descriptor" >&2
fi

jstr() { sed -n "s/.*\"$1\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p" "$2" | head -1; }
jnum() { sed -n "s/.*\"$1\"[[:space:]]*:[[:space:]]*\([0-9]*\).*/\1/p" "$2" | head -1; }

# ── The key this build trusts, read from the source rather than retyped ──────
PUB="${CHECK_APP_MANIFEST_PUBKEY:-$(sed -n 's/^const RELEASE_PUBKEY: &str = "\(.*\)";/\1/p' src-tauri/src/core/updates.rs | head -1)}"
[ -n "$PUB" ] || fail "could not read RELEASE_PUBKEY out of src-tauri/src/core/updates.rs"
[ "${#PUB}" = "64" ] || fail "RELEASE_PUBKEY is ${#PUB} hex chars, not 64"

# A raw 32-byte ed25519 public key becomes a PEM by prepending the fixed SPKI
# DER header — the same 12 bytes for every ed25519 key, which is why this can be
# a constant rather than a dependency.
{
  printf -- "-----BEGIN PUBLIC KEY-----\n"
  printf '302a300506032b6570032100%s' "$PUB" | xxd -r -p | base64
  printf -- "-----END PUBLIC KEY-----\n"
} > "$TMP/pub.pem"

# verified <doc> <hex sig file>: exit 0 only when the signature verifies.
verified() {
  tr -d '[:space:]' < "$2" | xxd -r -p > "$2.bin" 2>/dev/null || return 1
  "$OPENSSL" pkeyutl -verify -pubin -inkey "$TMP/pub.pem" -rawin \
    -in "$1" -sigfile "$2.bin" >/dev/null 2>&1
}

# ── Fetch what is published ─────────────────────────────────────────────────
if ! curl -fsS "$DOC_URL" -o "$TMP/doc.json" 2>/dev/null; then
  echo "check-app-manifest: nothing is published yet ($DOC_URL is 404)."
  echo "  That is the state before the first release that carries a descriptor."
  echo "  Installed copies will offer no update until rexenv/runtimes publishes one."
  exit 0
fi
curl -fsS "$SIG_URL" -o "$TMP/sig.hex" 2>/dev/null || fail "the document is published but its .sig is not"

# ── 1. Verify, with openssl, against the pinned key ──────────────────────────
if ! verified "$TMP/doc.json" "$TMP/sig.hex"; then
  fail "the published descriptor does NOT verify against this build's key.
  Either the publisher signed with a different key, or the document was changed
  after signing. Every installed rexenv is refusing this document right now."
fi

VERSION="$(jstr version "$TMP/doc.json")"
SERIAL="$(jnum serial "$TMP/doc.json")"
SHA="$(jstr sha256 "$TMP/doc.json")"
URL="$(jstr url "$TMP/doc.json")"
echo "check-app-manifest: signature OK — serial $SERIAL names rexenv $VERSION"

# ── 2. Does it match what the tap actually published? ───────────────────────
HAVE_GH=0
if [ "${CHECK_APP_MANIFEST_OFFLINE:-0}" != "1" ] && command -v gh >/dev/null 2>&1; then
  HAVE_GH=1
fi
LATEST="${CHECK_APP_MANIFEST_TAP_LATEST:-}"
if [ -z "$LATEST" ] && [ "$HAVE_GH" = "1" ]; then
  LATEST="$(gh release list --repo "$TAP" --limit 1 --exclude-drafts --exclude-pre-releases \
              --json tagName --jq '.[0].tagName // ""' 2>/dev/null || true)"
fi
LATEST="${LATEST#v}"

forgotten() {
  warn "the tap's latest release is $LATEST but the descriptor still names $VERSION.
  This is the forgotten-second-click: run rexenv/runtimes → Actions →
  'Publish app update manifest'. Until then no installed rexenv will be offered $LATEST.
  $1"
}

if [ -z "$LATEST" ] && [ "$HAVE_GH" = "0" ] && [ -z "${CHECK_APP_MANIFEST_TAP_LATEST:-}" ]; then
  warn "gh is not installed — checked the signature only, not what the tap published"
elif [ -n "$LATEST" ] && [ "$LATEST" != "$VERSION" ]; then
  # Before saying "publish again", read the committed file (see the header).
  if curl -fsS -H "Accept: application/vnd.github.raw" "$API_DOC_URL" -o "$TMP/committed.json" 2>/dev/null \
     && curl -fsS -H "Accept: application/vnd.github.raw" "$API_SIG_URL" -o "$TMP/committed.hex" 2>/dev/null; then
    C_VERSION="$(jstr version "$TMP/committed.json")"
    C_SERIAL="$(jnum serial "$TMP/committed.json")"
    if [ "$C_VERSION" = "$LATEST" ] && [ "${C_SERIAL:-0}" -gt "${SERIAL:-0}" ]; then
      verified "$TMP/committed.json" "$TMP/committed.hex" \
        || fail "the COMMITTED descriptor (serial $C_SERIAL, $C_VERSION) does NOT verify against this build's key.
  The CDN is about to serve a document every installed rexenv will refuse."
      warn "the publish already happened — only the CDN is behind.
  committed (contents API): serial $C_SERIAL names rexenv $C_VERSION, signature OK
  CDN (raw.githubusercontent.com, what installed apps fetch): still serial $SERIAL naming $VERSION
  raw caches this file for up to 5 minutes (3 min 10 s on 13 Sep 2026).
  Do NOT publish again — re-run this check in a few minutes."
    else
      forgotten "(The committed file names ${C_VERSION:-nothing} at serial ${C_SERIAL:-?} too, so this is not CDN lag.)"
    fi
  else
    forgotten "(Could not read the committed file through the contents API, so CDN lag is not ruled out.)"
  fi
fi

# ── 3. Is the artifact really there, with that digest? ───────────────────────
if [ "$HAVE_GH" = "1" ] && [ -n "$URL" ]; then
  ASSET="${URL##*/}"
  DIGEST="$(gh api "repos/$TAP/releases/tags/v$VERSION" \
              --jq ".assets[] | select(.name == \"$ASSET\") | .digest // \"\"" 2>/dev/null | head -1 || true)"
  DIGEST="${DIGEST#sha256:}"
  if [ -z "$DIGEST" ]; then
    warn "the descriptor names $ASSET on v$VERSION, and no such asset is there"
  elif [ "$DIGEST" != "$SHA" ]; then
    fail "the descriptor's sha256 and the published asset's digest DISAGREE.
  descriptor: $SHA
  published:  $DIGEST
  Every update would be downloaded and then discarded on the digest check."
  else
    echo "check-app-manifest: the named asset exists and its digest matches"
  fi
fi

# ── 4. Is the floor it declares the one this source tree declares? ──────────
# The macOS descriptor must say what tauri.conf.json says: higher, and hosts the
# app supports are never offered an update; lower, and they are offered a bundle
# LaunchServices refuses to open. The Windows descriptor carries an EMPTY floor —
# the Windows app has no host version to compare (APP-MANIFEST.md §0).
FLOOR="$(jstr minimumSystemVersion "$TMP/doc.json")"
if [ "$DOC_NAME" != "app-manifest.json" ]; then
  [ -z "$FLOOR" ] || fail "the $DOC_NAME descriptor declares minimumSystemVersion '$FLOOR'; it must be empty (only macOS compares a host version)"
else
  WANT="${CHECK_APP_MANIFEST_FLOOR:-$(jstr minimumSystemVersion src-tauri/tauri.conf.json)}"
  [ -n "$WANT" ] || fail "could not read minimumSystemVersion out of src-tauri/tauri.conf.json"
  if [ "$FLOOR" != "$WANT" ]; then
    fail "the descriptor's minimumSystemVersion is '$FLOOR'; tauri.conf.json declares '$WANT'.
  MIN_MACOS in rexenv/runtimes scripts/publish-app-manifest.sh is a hand-copied number —
  set it to $WANT and publish again. Until then a host between the two floors is either
  never offered this release (descriptor higher) or offered one it cannot launch (lower)."
  fi
  echo "check-app-manifest: minimumSystemVersion $FLOOR matches tauri.conf.json"
fi

if [ "$WARNED" = "1" ]; then
  echo "check-app-manifest: signature checks passed — read the WARNING above before calling this release done"
else
  echo "check-app-manifest: all green"
fi

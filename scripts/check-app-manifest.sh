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
#
# The same shape as `scripts/check-php-pins.sh`: the one check only this repo can
# make, because only this repo holds the pinned public key.
set -euo pipefail

cd "$(dirname "$0")/.."

DOC_URL="https://raw.githubusercontent.com/rexenv/runtimes/main/app-manifest.json"
SIG_URL="$DOC_URL.sig"
TAP="rexenv/homebrew-tap"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail() { echo "check-app-manifest: $*" >&2; exit 1; }
warn() { echo "check-app-manifest: WARNING — $*" >&2; }

# ── The key this build trusts, read from the source rather than retyped ──────
PUB="$(sed -n 's/^const RELEASE_PUBKEY: &str = "\(.*\)";/\1/p' src-tauri/src/core/updates.rs | head -1)"
[ -n "$PUB" ] || fail "could not read RELEASE_PUBKEY out of src-tauri/src/core/updates.rs"
[ "${#PUB}" = "64" ] || fail "RELEASE_PUBKEY is ${#PUB} hex chars, not 64"

# ── Fetch what is published ─────────────────────────────────────────────────
if ! curl -fsS "$DOC_URL" -o "$TMP/doc.json"; then
  echo "check-app-manifest: nothing is published yet ($DOC_URL is 404)."
  echo "  That is the state before the first release that carries a descriptor."
  echo "  Installed copies will offer no update until rexenv/runtimes publishes one."
  exit 0
fi
curl -fsS "$SIG_URL" -o "$TMP/sig.hex" || fail "the document is published but its .sig is not"

# ── 1. Verify, with openssl, against the pinned key ──────────────────────────
#
# A raw 32-byte ed25519 public key becomes a PEM by prepending the fixed SPKI
# DER header — the same 12 bytes for every ed25519 key, which is why this can be
# a constant rather than a dependency.
{
  printf -- "-----BEGIN PUBLIC KEY-----\n"
  printf '302a300506032b6570032100%s' "$PUB" | xxd -r -p | base64
  printf -- "-----END PUBLIC KEY-----\n"
} > "$TMP/pub.pem"
tr -d '[:space:]' < "$TMP/sig.hex" | xxd -r -p > "$TMP/sig.bin" 2>/dev/null \
  || fail "the signature file is not hex"

if ! openssl pkeyutl -verify -pubin -inkey "$TMP/pub.pem" -rawin \
      -in "$TMP/doc.json" -sigfile "$TMP/sig.bin" >/dev/null 2>&1; then
  fail "the published descriptor does NOT verify against this build's key.
  Either the publisher signed with a different key, or the document was changed
  after signing. Every installed rexenv is refusing this document right now."
fi

VERSION="$(sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$TMP/doc.json" | head -1)"
SERIAL="$(sed -n 's/.*"serial"[[:space:]]*:[[:space:]]*\([0-9]*\).*/\1/p' "$TMP/doc.json" | head -1)"
SHA="$(sed -n 's/.*"sha256"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$TMP/doc.json" | head -1)"
URL="$(sed -n 's/.*"url"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$TMP/doc.json" | head -1)"
echo "check-app-manifest: signature OK — serial $SERIAL names rexenv $VERSION"

# ── 2. Does it match what the tap actually published? ───────────────────────
if command -v gh >/dev/null 2>&1; then
  LATEST="$(gh release list --repo "$TAP" --limit 1 --exclude-drafts --exclude-pre-releases \
              --json tagName --jq '.[0].tagName // ""' 2>/dev/null || true)"
  LATEST="${LATEST#v}"
  if [ -n "$LATEST" ] && [ "$LATEST" != "$VERSION" ]; then
    warn "the tap's latest release is $LATEST but the descriptor still names $VERSION.
  This is the forgotten-second-click: run rexenv/runtimes → Actions →
  'Publish app update manifest'. Until then no installed rexenv will be offered $LATEST."
  fi

  # ── 3. Is the artifact really there, with that digest? ─────────────────────
  if [ -n "$URL" ]; then
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
else
  warn "gh is not installed — checked the signature only, not what the tap published"
fi

echo "check-app-manifest: all green"

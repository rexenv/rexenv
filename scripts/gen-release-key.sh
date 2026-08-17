#!/bin/bash
# Generate the ed25519 keypair that signs the PHP update manifest.
#
# RUN THIS ONCE, BY HAND, ON A MACHINE YOU TRUST. It is deliberately not wired
# into any build, any CI job, or verify.sh: a key that a pipeline can mint is a
# key an attacker who reaches the pipeline can mint, and the whole point of the
# compiled-in public half is that rotating it costs an app release.
#
# WHAT COMES OUT
#   - a PUBLIC key (hex) → paste into `RELEASE_PUBKEY` in
#     src-tauri/src/core/updates.rs and commit. Until you do, the app trusts no
#     manifest and the Update button never appears, by construction.
#   - a PRIVATE key (PEM) → save to ~/.rexenv/manifest-key.pem, chmod 600.
#     NOT a CI secret: releases are published from your machine already (to avoid
#     a cross-repo credential), so the key has no reason to leave it — and keeping
#     it off CI means a GitHub account compromise does not get an attacker the
#     signing key. Strictly stronger, one less moving part.
#
# WHAT THIS KEY IS WORTH, STATED PLAINLY
#
# Whoever holds the private half can make any rexenv install download and run
# arbitrary bytes as the user — and that user's machine has a trusted local CA
# whose private key is readable by anything running as them. This is the most
# valuable secret in the project. It outranks the app signing identity, which
# does not exist yet.
#
# Kept on your machine (see above), the signature defends against a compromised
# CDN or mirror, tampering past TLS, and a repo compromise. It does NOT defend
# against your own machine being compromised — that is what a hardware token would
# buy, and moving to one later costs a key rotation, NOT a redesign, because the
# app only ever holds the public half.
set -euo pipefail

command -v openssl >/dev/null || { echo "openssl is required" >&2; exit 1; }

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
chmod 700 "$TMP"

openssl genpkey -algorithm ed25519 -out "$TMP/key.pem" 2>/dev/null

# ring wants raw pkcs8 v1; openssl writes exactly that in DER.
openssl pkey -in "$TMP/key.pem" -outform DER -out "$TMP/key.pk8" 2>/dev/null
# The public half, raw 32 bytes: strip the 12-byte SPKI prefix ed25519 always has.
openssl pkey -in "$TMP/key.pem" -pubout -outform DER 2>/dev/null \
  | tail -c 32 | xxd -p -c 64 > "$TMP/pub.hex"

PUB="$(cat "$TMP/pub.hex")"
PRIV="$(cat "$TMP/key.pem")"

[ "${#PUB}" -eq 64 ] || { echo "public key is ${#PUB} hex chars, expected 64" >&2; exit 1; }

cat <<EOF

────────────────────────────────────────────────────────────────────────
PUBLIC KEY — commit this, in src-tauri/src/core/updates.rs:

    const RELEASE_PUBKEY: &str = "$PUB";

PRIVATE KEY — save to ~/.rexenv/manifest-key.pem and chmod 600. Do not
commit it, do not put it in a CI secret, and do not paste it into an issue,
a chat, or an AI tool's transcript:

$PRIV

────────────────────────────────────────────────────────────────────────

NEXT, in this order — the order matters:

  1. mkdir -p ~/.rexenv && chmod 700 ~/.rexenv
     Save the PEM above to ~/.rexenv/manifest-key.pem, chmod 600.
  2. Pin the public key in src-tauri/src/core/updates.rs and commit.
  3. ./scripts/publish-php-manifest.sh <patch>   — signs and prints the
     publish command. Do this BEFORE the build reaches anyone, or the button
     appears with nothing to offer.

     (2 and 3 in that order: the script refuses to publish if the key does
     not match what the app pins, which is the check that catches a rotation
     done backwards.)

TO ROTATE (a leak, or moving to a hardware token): run this again, replace
~/.rexenv/manifest-key.pem, pin the new public key, and ship a release. Old manifests stop
verifying the moment the app updates — which is the property that makes a
stolen key survivable, and the reason the public half is compiled in rather
than fetched.
EOF

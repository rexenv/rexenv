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
#   - a PRIVATE key (base64 pkcs8) → paste into the GitHub Actions secret
#     REXENV_MANIFEST_KEY on the repo that publishes the manifest.
#
# WHAT THIS KEY IS WORTH, STATED PLAINLY
#
# Whoever holds the private half can make any rexenv install download and run
# arbitrary bytes as the user — and that user's machine has a trusted local CA
# whose private key is readable by anything running as them. This is the most
# valuable secret in the project. It outranks the app signing identity, which
# does not exist yet.
#
# With the key in a CI secret, one compromise of that account takes the app, the
# manifest AND the key together, so the signature buys you: a compromised CDN or
# mirror cannot forge a manifest, and neither can tampering past TLS. It does NOT
# buy you protection from whoever can push to the repo. That is a real, narrower
# property — and moving the private half to a hardware token later costs a key
# rotation, NOT a redesign, because the app only ever holds the public half.
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
PRIV="$(base64 < "$TMP/key.pk8" | tr -d '\n')"

[ "${#PUB}" -eq 64 ] || { echo "public key is ${#PUB} hex chars, expected 64" >&2; exit 1; }

cat <<EOF

────────────────────────────────────────────────────────────────────────
PUBLIC KEY — commit this, in src-tauri/src/core/updates.rs:

    const RELEASE_PUBKEY: &str = "$PUB";

PRIVATE KEY — GitHub Actions secret REXENV_MANIFEST_KEY. Paste it once,
then close this terminal. Do not commit it, do not paste it into an issue,
a chat, or an AI tool's transcript:

$PRIV

────────────────────────────────────────────────────────────────────────

NEXT, in this order — the order matters:

  1. Store the PRIVATE key in the CI secret FIRST, and confirm the signing
     job can read it.
  2. THEN pin the public key and commit. Doing it the other way round ships
     an app that trusts a key nobody can sign with, so the first user to
     press Update gets a failure with no cause they can see.
  3. Publish a signed manifest before that build reaches anyone, or the
     button appears and has nothing to offer.

TO ROTATE (a leak, or moving to a hardware token): run this again, update the
CI secret, pin the new public key, and ship a release. Old manifests stop
verifying the moment the app updates — which is the property that makes a
stolen key survivable, and the reason the public half is compiled in rather
than fetched.
EOF

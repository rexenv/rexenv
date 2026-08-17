#!/bin/bash
# Build, SIGN and publish the PHP update manifest.
#
#   ./scripts/publish-php-manifest.sh 8.3.32 8.4.24
#
# For each patch given: downloads all four artifacts (cli/fpm × arm64/amd64),
# hashes them, emits a manifest entry, signs the whole document, VERIFIES its own
# signature, and prints the one command that publishes it.
#
# # Why this runs on your machine and not in CI
#
# The dmg is already built and published locally, because uploading from the
# private source repo to a public one would need a cross-repo credential — the
# PAT this pipeline was designed to avoid (docs/RELEASING.md). The manifest goes
# to a public repo too, so it inherits the same answer.
#
# That is a SECURITY WIN, not just consistency: with no CI secret holding the
# signing key, compromising the GitHub account does not get an attacker the key.
# The signature then defends against a compromised CDN or mirror, tampering past
# TLS, AND a repo compromise. Keep the key off CI for exactly that reason.
#
# # What the key is worth
#
# Whoever holds it can make any rexenv install download and run arbitrary bytes as
# the user — on a machine whose trusted local CA private key that user can read.
# It is the most valuable secret in this project. Treat losing it as an incident:
# rotate (`scripts/gen-release-key.sh`), pin the new public half, ship a release.
# Old manifests stop verifying as soon as the app updates, which is the property
# that makes a stolen key survivable and the reason the public half is compiled in.
set -euo pipefail

KEY="${REXENV_MANIFEST_KEY_FILE:-$HOME/.rexenv/manifest-key.pem}"
REPO="${REXENV_MANIFEST_REPO:-rexenv/runtimes}"
TAG="manifest"
MIN_APP="${REXENV_MANIFEST_MIN_APP:-0.3.0}"

[ $# -ge 1 ] || { echo "usage: $0 <patch> [patch...]   e.g. $0 8.3.32" >&2; exit 2; }
[ -f "$KEY" ] || {
  cat >&2 <<EOF
No signing key at $KEY

  Mint one ONCE with:   ./scripts/gen-release-key.sh
  Save the PEM it makes to $KEY (chmod 600), and pin the public half in
  src-tauri/src/core/updates.rs. Do NOT put it in a CI secret — see the header.
EOF
  exit 1
}
command -v openssl >/dev/null || { echo "openssl is required" >&2; exit 1; }
command -v gh >/dev/null || { echo "gh is required (to read the current serial)" >&2; exit 1; }

WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT; chmod 700 "$WORK"

# ── The serial must INCREASE, or the app refuses the document as a replay ──────
# Read the published one rather than trusting a local counter: the app compares
# against the highest serial it has ever ACCEPTED, so a number that goes backwards
# is a manifest nobody can install. Absent (first publish) starts at 1.
CUR=0
if gh release view "$TAG" --repo "$REPO" >/dev/null 2>&1; then
  if gh release download "$TAG" --repo "$REPO" --pattern manifest.json --dir "$WORK" 2>/dev/null; then
    CUR="$(sed -n 's/.*"serial"[[:space:]]*:[[:space:]]*\([0-9]\{1,\}\).*/\1/p' "$WORK/manifest.json" | head -1)"
    CUR="${CUR:-0}"
  fi
fi
SERIAL=$((CUR + 1))
echo "current published serial: $CUR → publishing $SERIAL"

# ── Hash every artifact, from the URL the app itself will use ─────────────────
# Not from a local build directory. The digest must describe the bytes a USER
# receives, and the only way to be sure of that is to fetch what they fetch.
ENTRIES=""
for V in "$@"; do
  case "$V" in
    [0-9]*.[0-9]*.[0-9]*) ;;
    *) echo "not an x.y.z patch: $V" >&2; exit 1 ;;
  esac
  for KIND in cli fpm; do
    for ARCH_APP in arm64 x86_64; do
      # static-php.dev names arm64 `aarch64`; the app's own `php_arch` does the
      # same mapping. Two spellings of one fact, and this is the second one.
      case "$ARCH_APP" in
        arm64) ARCH_URL=aarch64 ;;
        x86_64) ARCH_URL=x86_64 ;;
      esac
      URL="https://dl.static-php.dev/static-php-cli/bulk/php-${V}-${KIND}-macos-${ARCH_URL}.tar.gz"
      OUT="$WORK/${V}-${KIND}-${ARCH_APP}.tar.gz"
      printf '  fetching %s %s %s … ' "$V" "$KIND" "$ARCH_APP"
      if ! curl -fsSL --retry 3 -o "$OUT" "$URL"; then
        echo "FAILED"
        echo "  $URL did not answer — static-php.dev may not have built $V yet." >&2
        echo "  That is the normal case for days or weeks after an upstream release." >&2
        exit 1
      fi
      SHA="$(shasum -a 256 "$OUT" | awk '{print $1}')"
      echo "$SHA"
      NAME=$([ "$KIND" = fpm ] && echo php-fpm || echo php)
      ENTRIES="${ENTRIES:+$ENTRIES,}$(printf '{"name":"%s","version":"%s","arch":"%s","url":"%s","sha256":"%s"}' \
        "$NAME" "$V" "$ARCH_APP" "$URL" "$SHA")"
    done
  done
done

printf '{"serial":%d,"generatedAt":"%s","minAppVersion":"%s","artifacts":[%s]}' \
  "$SERIAL" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$MIN_APP" "$ENTRIES" > "$WORK/manifest.json"

# ── Sign, then VERIFY OUR OWN SIGNATURE before anyone else has to ─────────────
# A signature nobody checked is a release nobody can install, and the failure
# would surface on a user's machine as "the manifest signature does not verify"
# with no way for them to tell whose fault it is.
openssl pkeyutl -sign -inkey "$KEY" -rawin -in "$WORK/manifest.json" -out "$WORK/sig.bin"
xxd -p -c 256 < "$WORK/sig.bin" | tr -d '\n' > "$WORK/manifest.json.sig"

openssl pkeyutl -verify -pubin \
  -inkey <(openssl pkey -in "$KEY" -pubout) \
  -rawin -in "$WORK/manifest.json" -sigfile "$WORK/sig.bin" >/dev/null \
  || { echo "our own signature does not verify — refusing to publish" >&2; exit 1; }

PUB="$(openssl pkey -in "$KEY" -pubout -outform DER | tail -c 32 | xxd -p -c 64)"
PINNED="$(sed -n 's/.*RELEASE_PUBKEY: &str = "\([0-9a-f]*\)".*/\1/p' src-tauri/src/core/updates.rs | head -1)"
if [ -z "$PINNED" ]; then
  echo
  echo "WARNING: RELEASE_PUBKEY is EMPTY in src-tauri/src/core/updates.rs."
  echo "Every shipped build will ignore this manifest. Pin it:"
  echo "    const RELEASE_PUBKEY: &str = \"$PUB\";"
elif [ "$PINNED" != "$PUB" ]; then
  echo "the key signing this ($PUB) is NOT the one the app pins ($PINNED)" >&2
  echo "Rotating? Ship the app release with the new pubkey FIRST." >&2
  exit 1
else
  echo "signing key matches the pinned RELEASE_PUBKEY ✓"
fi

cp "$WORK/manifest.json" "$WORK/manifest.json.sig" .
echo
echo "Wrote manifest.json (serial $SERIAL) + manifest.json.sig"
echo
echo "Publish (the tag is MOVED — safe here and nowhere else, because these bytes"
echo "are trusted for their SIGNATURE, not for their URL):"
echo
echo "    gh release delete '$TAG' --repo $REPO --yes 2>/dev/null || true"
echo "    gh release create '$TAG' --repo $REPO --title 'PHP update manifest' \\"
echo "      manifest.json manifest.json.sig"

#!/bin/bash
# Is the PUBLISHED apt repository signed by its own key, whole, and current?
#
#   scripts/check-apt-repo.sh
#
# # The failure this exists for
#
# The apt repository (github.com/rexenv/apt, docs/PLAN-apt-repo.md) is published by its own
# workflow AFTER a release, behind an approval — the same shape as the app-update descriptors,
# and the same way to forget it: the tap release is public, the cask bumps, and every
# `apt upgrade` keeps answering "rexenv is already the newest version". So this answers what
# a person cannot see by looking:
#
#   1. Does InRelease verify against the key the repository publishes, and is that key the
#      fingerprint this tree expects (a swapped key signs perfectly well)?
#   2. Does every Packages index match the hash InRelease signs for it?
#   3. Is the newest rexenv in each architecture the tap's latest published release?
#
# Exit 0 only when all three hold. Needs curl and gpgv.
set -euo pipefail

APT_URL="https://rexenv.github.io/apt"
RELEASES_LATEST="https://github.com/rexenv/rexenv/releases/latest"
# The repository's key (github.com/rexenv/apt KEY_FINGERPRINT). install.sh in the tap carries the
# same key; a rotation changes all three together (docs/PLAN-apt-repo.md §5).
EXPECTED_FPR="139CC1A4A1971376FC6B586BD2F2070D6DFA60C9"

say() { printf 'check-apt-repo: %s\n' "$*"; }
fail() {
  printf 'check-apt-repo: %s\n' "$*" >&2
  exit 1
}
command -v gpgv >/dev/null || fail "needs gpgv (brew install gnupg / apt install gpgv)"

T="$(mktemp -d)"
trap 'rm -rf "$T"' EXIT
get() { curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location --retry 2 -o "$2" "$1" || fail "could not download $1"; }

get "$APT_URL/rexenv.gpg" "$T/rexenv.gpg"
get "$APT_URL/dists/stable/InRelease" "$T/InRelease"

# 1. The signature, by the published key — and the published key is the expected one.
if ! status="$(gpgv --status-fd 1 --keyring "$T/rexenv.gpg" "$T/InRelease" 2>/dev/null)"; then
  fail "InRelease does not verify against ${APT_URL}/rexenv.gpg"
fi
signer="$(printf '%s\n' "$status" | awk '/VALIDSIG/ {print $NF; exit}')"
[ "$signer" = "$EXPECTED_FPR" ] || fail "InRelease is signed by ${signer:-nobody}, expected ${EXPECTED_FPR}"
say "InRelease: good signature by ${signer}"

latest="$(curl --proto '=https' --tlsv1.2 --fail --silent --show-error -o /dev/null -w '%{redirect_url}' "$RELEASES_LATEST")"
latest="${latest##*/v}"
[[ $latest =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "could not read the tap's latest release"

sha256_of() { if command -v sha256sum >/dev/null; then sha256sum "$1" | awk '{print $1}'; else shasum -a 256 "$1" | awk '{print $1}'; fi; }
stale=0
for arch in amd64 arm64; do
  rel="main/binary-${arch}/Packages"
  get "$APT_URL/dists/stable/$rel" "$T/Packages.$arch"
  # 2. The index is the one InRelease signed (its SHA256 section: "<hash> <size> <path>").
  want="$(awk -v p="$rel" '/^SHA256:/ {s=1; next} /^[A-Za-z]/ {s=0} s && $3 == p {print $1}' "$T/InRelease")"
  got="$(sha256_of "$T/Packages.$arch")"
  [ -n "$want" ] && [ "$want" = "$got" ] || fail "${rel}: the file's sha256 is ${got}, InRelease signs ${want:-nothing}"
  # 3. Its newest rexenv is the latest release.
  newest="$(awk '/^Package: rexenv$/ {p=1; next} /^Package:/ {p=0} p && /^Version:/ {print $2}' "$T/Packages.$arch" | sort -V | tail -1)"
  if [ "$newest" = "$latest" ]; then
    say "${arch}: rexenv ${newest} (the latest release)"
  else
    say "${arch}: the repository's newest is ${newest:-nothing}, the tap's latest release is ${latest}"
    stale=1
  fi
done
if [ "$stale" -ne 0 ]; then
  fail "the repository lags the release — run github.com/rexenv/apt → Actions → 'Publish apt repository' and approve it"
fi
say "all green"

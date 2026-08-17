#!/bin/bash
# Does rexenv/runtimes know about every PHP minor this app ships?
#
#   ./scripts/check-php-pins.sh
#
# # Why this exists, and why it replaced a second publisher
#
# Publishing the update manifest is ONE command, and it lives where the artifacts
# and the signing key do:
#
#   rexenv/runtimes → Actions → "Publish PHP update manifest" → Run workflow
#   (or, locally: cd ../runtimes && ./scripts/publish-manifest.sh --dry-run)
#
# This repo used to carry its own copy of that script. Two implementations of one
# document format, in two repos, is the drift this codebase removes everywhere
# else — and the copy here could not see the `PINS` list that drives discovery,
# so it was the weaker half. It is gone; the runtimes one is canonical.
#
# What is left is the one check only THIS repo can make. `publish-manifest.sh`
# carries a `PINS` list that must mirror `PHP_VERSIONS` in `core/binaries.rs`,
# duplicated because the app repo is private and runtimes cannot read it.
#
# Most drift is harmless: discovery starts AT the pin, so a stale patch can only
# offer something the app already has, which the app then filters out as not newer
# than its own pin. A wasted probe, never a wrong install.
#
# **A missing MINOR is not harmless, and it is silent.** Discovery never probes a
# minor it has never heard of, so shipping 8.6 without adding it to `PINS` means
# 8.6 gets no in-app updates at all, forever, with nothing anywhere saying so.
# That is the case this script exists to catch.
set -euo pipefail

cd "$(dirname "$0")/.."

PINS_URL="${REXENV_PINS_URL:-https://raw.githubusercontent.com/rexenv/runtimes/main/scripts/publish-manifest.sh}"

# Ours, from the source of truth rather than a list typed twice.
ours="$(sed -n '/^pub const PHP_VERSIONS/,/\];/p' src-tauri/src/core/binaries.rs \
  | grep -oE '"[0-9]+\.[0-9]+\.[0-9]+"' | tr -d '"' | sort -V)"
[ -n "$ours" ] || { echo "could not read PHP_VERSIONS from core/binaries.rs" >&2; exit 1; }

remote="$(curl -fsSL --max-time 30 "$PINS_URL")" || {
  echo "could not fetch runtimes' publish-manifest.sh from $PINS_URL" >&2
  exit 1
}
theirs="$(printf '%s\n' "$remote" | sed -n '/^PINS=(/,/^)/p' \
  | grep -oE '"[0-9]+\.[0-9]+:[0-9]+\.[0-9]+\.[0-9]+"' | tr -d '"' | sort -V)"
[ -n "$theirs" ] || { echo "no PINS block found in the fetched script — did it move?" >&2; exit 1; }

printf 'app  PHP_VERSIONS : %s\n' "$(printf '%s ' $ours)"
printf 'runtimes PINS     : %s\n\n' "$(printf '%s ' $theirs)"

status=0
for patch in $ours; do
  minor="${patch%.*}"
  their_patch="$(printf '%s\n' "$theirs" | sed -n "s/^${minor}://p")"
  if [ -z "$their_patch" ]; then
    # The unbounded case.
    echo "MISSING  $minor — runtimes has no entry, so $minor can NEVER be offered an update."
    echo "         Add \"$minor:$patch\" to PINS in rexenv/runtimes scripts/publish-manifest.sh."
    status=1
  elif [ "$their_patch" != "$patch" ]; then
    # The bounded case: worth saying, not worth failing.
    echo "stale    $minor — app pins $patch, runtimes says $their_patch."
    echo "         Harmless (discovery starts at the pin and the app floors anything older),"
    echo "         but update it while you are there."
  fi
done

# The reverse direction: an entry for a minor this app does not ship. The app
# drops those per-entry, so it costs probes and nothing else.
for entry in $theirs; do
  minor="${entry%%:*}"
  printf '%s\n' "$ours" | grep -q "^${minor}\." || \
    echo "extra    $minor — runtimes probes it; this app ships no such minor, so entries are dropped."
done

[ "$status" -eq 0 ] && echo "pins: every minor this app ships can receive updates"
exit "$status"

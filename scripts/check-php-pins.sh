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

PINS_REPO="${REXENV_PINS_REPO:-rexenv/runtimes}"
PINS_PATH="scripts/publish-manifest.sh"
PINS_URL="${REXENV_PINS_URL:-https://raw.githubusercontent.com/$PINS_REPO/main/$PINS_PATH}"

# Ours, from the source of truth rather than a list typed twice.
ours="$(sed -n '/^pub const PHP_VERSIONS/,/\];/p' src-tauri/src/core/binaries.rs \
  | grep -oE '"[0-9]+\.[0-9]+\.[0-9]+"' | tr -d '"' | sort -V)"
[ -n "$ours" ] || { echo "could not read PHP_VERSIONS from core/binaries.rs" >&2; exit 1; }

# Through the API when `gh` is here, `raw.githubusercontent.com` otherwise.
# The order matters: raw is CDN-cached for minutes, so right after a push it
# serves the OLD script — and this check would then report a mismatch that was
# just fixed, or (worse) miss one that was just introduced. A drift check reading
# a stale copy is a drift check that lies in both directions.
remote=""
if command -v gh >/dev/null 2>&1; then
  remote="$(gh api "repos/$PINS_REPO/contents/$PINS_PATH" --jq '.content' 2>/dev/null \
    | base64 -d 2>/dev/null || true)"
fi
if [ -z "$remote" ]; then
  remote="$(curl -fsSL --max-time 30 "$PINS_URL")" || {
    echo "could not fetch runtimes' publish-manifest.sh from $PINS_URL" >&2
    exit 1
  }
  echo "(read through raw.githubusercontent.com, which is CDN-cached — if this" >&2
  echo " disagrees with a push you just made, re-run in a few minutes)" >&2
fi
theirs="$(printf '%s\n' "$remote" | sed -n '/^PINS=(/,/^)/p' \
  | grep -oE '"[0-9]+\.[0-9]+:[0-9]+\.[0-9]+\.[0-9]+"' | tr -d '"' | sort -V)"
[ -n "$theirs" ] || { echo "no PINS block found in the fetched script — did it move?" >&2; exit 1; }

# ── Adminer: the pin, and the CEILING, which is the one that must match ───────
#
# `ADMINER_MAX_MAJOR` exists in both repos and both copies are EVIDENCE — the
# app's number is the newest Adminer major whose plugin API has been run against
# rexenv's wrapper. A publisher ceiling ABOVE the app's publishes entries every
# installed app silently drops (the version is offered by nobody and the run
# looks successful); a publisher ceiling BELOW the app's quietly withholds
# versions the app would happily take. Neither is visible from either side alone,
# which is why this is the check that exits non-zero.
app_adminer="$(sed -n 's/^pub const ADMINER_VERSION: &str = "\([^"]*\)".*/\1/p' \
  src-tauri/src/core/binaries.rs | head -1)"
app_ceiling="$(sed -n 's/^pub const ADMINER_MAX_MAJOR: u32 = \([0-9]*\).*/\1/p' \
  src-tauri/src/core/updates.rs | head -1)"
their_adminer="$(printf '%s\n' "$remote" | sed -n 's/^ADMINER_PIN="\([^"]*\)".*/\1/p' | head -1)"
their_ceiling="$(printf '%s\n' "$remote" | sed -n 's/^ADMINER_MAX_MAJOR="\([0-9]*\)".*/\1/p' | head -1)"
[ -n "$app_adminer" ] && [ -n "$app_ceiling" ] \
  || { echo "could not read ADMINER_VERSION / ADMINER_MAX_MAJOR from the app" >&2; exit 1; }
[ -n "$their_adminer" ] && [ -n "$their_ceiling" ] \
  || { echo "no ADMINER_PIN / ADMINER_MAX_MAJOR in the fetched script — did they move?" >&2; exit 1; }


printf 'app  PHP_VERSIONS : %s\n' "$(printf '%s ' $ours)"
printf 'runtimes PINS     : %s\n' "$(printf '%s ' $theirs)"
printf 'app  adminer      : %s (ceiling %s)\n' "$app_adminer" "$app_ceiling"
printf 'runtimes adminer  : %s (ceiling %s)\n\n' "$their_adminer" "$their_ceiling"

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

if [ "$app_ceiling" != "$their_ceiling" ]; then
  echo "MISMATCH adminer ceiling — app says $app_ceiling, runtimes says $their_ceiling."
  echo "         Both are EVIDENCE: the app's is the newest major whose plugin API has"
  echo "         been run against rexenv's wrapper. A higher publisher ceiling publishes"
  echo "         entries every app drops; a lower one withholds versions the app accepts."
  echo "         Raise the app's only after running the probe (adminer_check)."
  status=1
fi
if [ "$app_adminer" != "$their_adminer" ]; then
  echo "stale    adminer — app pins $app_adminer, runtimes says $their_adminer."
  echo "         Harmless (discovery starts at the pin and the app floors anything older)."
fi

[ "$status" -eq 0 ] && echo "pins: every minor this app ships can receive updates, and the adminer ceilings agree"
exit "$status"

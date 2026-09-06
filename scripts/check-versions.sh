#!/bin/bash
# One version, everywhere — checked LOCALLY, where releases are actually cut.
#
#   scripts/check-versions.sh              # the four manifests must agree
#   scripts/check-versions.sh <X.Y.Z>      # …and must all equal this
#   scripts/check-versions.sh --built      # also check the BUILT bundle's Info.plist
#
# # Why this exists as a script and not only in CI
#
# The four-manifest guard has always lived in `.github/workflows/release.yml`,
# which runs on a tag push — and the pre-push hook refuses those while
# `rexenv/rexenv` is private. So the guard has never run on a real release:
# every one of 0.1.0 through 0.5.0 was built on this Mac, where nothing checked.
# It cost nothing so far because one person bumped four files carefully. That is
# the shape this project stops relying on rather than the shape it trusts.
#
# Self-update raises the stakes: the version now decides what a user is OFFERED,
# what the descriptor names, and — through Homebrew's `auto_updates` comparison —
# whether `brew upgrade` thinks a self-updated app is current. A dmg whose
# filename disagrees with its Info.plist is a release that installs and then
# offers itself an update forever.
#
# `--built` is the check CI cannot do at all: it reads the version out of the
# bundle that was just produced, which is the only copy a user ever sees.
set -euo pipefail

cd "$(dirname "$0")/.."

WANT=""
BUILT=0
for arg in "$@"; do
  case "$arg" in
    --built) BUILT=1 ;;
    -h|--help) sed -n '2,8p' "$0"; exit 0 ;;
    *) WANT="${arg#v}" ;;
  esac
done

read_json() { # read_json <file> — the top-level "version" field
  sed -n 's/^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$1" | head -1
}
read_toml() { # read_toml <file> — the FIRST version= line ([package] is first)
  sed -n 's/^version = "\(.*\)"/\1/p' "$1" | head -1
}

declare -a NAMES VALUES
add() { NAMES+=("$1"); VALUES+=("$2"); }

add "src-tauri/tauri.conf.json" "$(read_json src-tauri/tauri.conf.json)"
add "package.json"             "$(read_json package.json)"
add "src-tauri/Cargo.toml"     "$(read_toml src-tauri/Cargo.toml)"
add "cli/Cargo.toml"           "$(read_toml cli/Cargo.toml)"

if [ "$BUILT" = "1" ]; then
  APP="src-tauri/target/universal-apple-darwin/release/bundle/macos/rexenv.app"
  if [ ! -d "$APP" ]; then
    echo "check-versions: --built asked for, but $APP does not exist (build first)" >&2
    exit 1
  fi
  # The bundle a USER gets. Homebrew reads this exact key to decide whether an
  # `auto_updates` cask is behind, and the updater compares it to what the
  # signed descriptor names — so a mismatch here is not cosmetic.
  add "$APP/Contents/Info.plist (CFBundleShortVersionString)" \
    "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$APP/Contents/Info.plist" 2>/dev/null || echo '?')"
  add "$APP/Contents/Info.plist (CFBundleVersion)" \
    "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$APP/Contents/Info.plist" 2>/dev/null || echo '?')"
fi

# The reference is the first value unless one was given on the command line.
REF="${WANT:-${VALUES[0]}}"
FAIL=0
for i in "${!NAMES[@]}"; do
  if [ "${VALUES[$i]}" != "$REF" ]; then
    echo "check-versions: ${NAMES[$i]} says '${VALUES[$i]}', expected '$REF'" >&2
    FAIL=1
  fi
done

# Plain semver, and nothing else. A `v` prefix or a build suffix would break
# Homebrew's Info.plist comparison and the descriptor's three-segment rule at
# the same time, in ways that read as "the update just does not appear".
if ! printf '%s' "$REF" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  echo "check-versions: '$REF' is not plain three-segment semver" >&2
  echo "  no 'v' prefix, no -rc/-beta, no build suffix: brew compares this string against" >&2
  echo "  the cask's version, and the update descriptor refuses anything else." >&2
  FAIL=1
fi

if [ "$FAIL" = "1" ]; then
  echo >&2
  echo "  Every one of these must carry the same plain semver. Fix them and re-run." >&2
  exit 1
fi

echo "check-versions: everything agrees on $REF (${#NAMES[@]} places)"

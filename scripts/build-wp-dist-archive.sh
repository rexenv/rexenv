#!/bin/bash
# Rebuild the vendored `wp dist-archive` package tree.
#
# You do NOT need to run this to build rexenv — the tree is committed under
# src-tauri/resources/wp-dist-archive/vendor and compiled into the binary. This
# script exists so that BUMPING it is one command with a recorded result, rather
# than an archaeology exercise in two years. That is the whole point: a vendored
# dependency with no recorded provenance is the thing nobody dares touch.
#
#   ./scripts/build-wp-dist-archive.sh              rebuild at the pinned version
#   DIST_ARCHIVE_VERSION=3.2.0 ./scripts/…          bump (see the constraint below)
#
# After a bump, three things must move together or the build fails:
#   1. this tree                       (regenerated here)
#   2. DIST_ARCHIVE_VERSION            (src-tauri/src/core/wp_packages.rs)
#   3. THIRD-PARTY-NOTICES.md          (the two PHP rows carry the version)
# `wp_packages::the_vendored_tree_is_the_version_we_pinned` reads the version out
# of the tree itself and fails if (1) and (2) disagree, so a silent bump is a
# build error rather than a behaviour change.
#
# ── THE WP-CLI COUPLING, found the hard way (4 Aug 2026) ──────────────────────
# dist-archive-command declares a dependency on wp-cli/wp-cli, and the versions
# are coupled to OUR pinned phar:
#
#   v3.1.0 requires wp-cli/wp-cli ^2      → works with our 2.12.0
#   v3.2.0 requires wp-cli/wp-cli ^2.13   → REFUSES to resolve against 2.12.0
#
# So bumping this package can require bumping WP_CLI_VERSION first, and composer
# will say so plainly rather than producing something that half-works. This is
# why the root package below is NAMED `wp-cli/wp-cli` at our pinned version —
# the same trick wp-cli's own package installer uses, so the requirement is
# satisfied by the phar we ship instead of vendoring a second copy of WP-CLI
# (which is what a naive composer.json does: 6 packages, several MB).
set -euo pipefail
cd "$(dirname "$0")/.."

DIST_ARCHIVE_VERSION="${DIST_ARCHIVE_VERSION:-3.1.0}"
# Must match binaries::WP_CLI_VERSION — the phar this tree is resolved against.
WP_CLI_VERSION="${WP_CLI_VERSION:-2.12.0}"

DEST="src-tauri/resources/wp-dist-archive"

# Prefer the binaries rexenv already downloaded (same PHP the app will run this
# code with); fall back to whatever is on PATH so a fresh clone can still bump.
APPDATA="$HOME/Library/Application Support/dev.rexenv.rexenv/bin"
PHP=""
COMPOSER=""
for candidate in "$APPDATA"/php-8.3.*/php "$APPDATA"/php-8.4.*/php; do
  [ -x "$candidate" ] && PHP="$candidate" && break
done
[ -z "$PHP" ] && PHP="$(command -v php || true)"
for candidate in "$APPDATA"/composer-*/composer.phar; do
  [ -f "$candidate" ] && COMPOSER="$candidate" && break
done

if [ -z "$PHP" ]; then
  echo "build-wp-dist-archive: no PHP found (neither rexenv's bundle nor PATH)." >&2
  echo "  Start rexenv once so it downloads PHP, or install php." >&2
  exit 1
fi
if [ -z "$COMPOSER" ]; then
  COMPOSER_BIN="$(command -v composer || true)"
  if [ -z "$COMPOSER_BIN" ]; then
    echo "build-wp-dist-archive: no composer found (neither rexenv's bundle nor PATH)." >&2
    exit 1
  fi
  COMPOSER="$COMPOSER_BIN"
  RUN_COMPOSER=("$COMPOSER")
else
  RUN_COMPOSER=("$PHP" "$COMPOSER")
fi

echo "php:      $PHP"
echo "composer: $COMPOSER"
echo "pinning:  wp-cli/dist-archive-command $DIST_ARCHIVE_VERSION against wp-cli $WP_CLI_VERSION"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

cat > "$WORK/composer.json" <<EOF
{
    "name": "wp-cli/wp-cli",
    "version": "$WP_CLI_VERSION",
    "description": "rexenv's vendored dist-archive command. The root package is deliberately named wp-cli/wp-cli so the bundled phar satisfies that requirement instead of a second copy being vendored. See scripts/build-wp-dist-archive.sh.",
    "require": {
        "wp-cli/dist-archive-command": "$DIST_ARCHIVE_VERSION"
    },
    "config": {
        "secure-http": true,
        "optimize-autoloader": true
    }
}
EOF

( cd "$WORK" && COMPOSER_HOME="$WORK/.composer" "${RUN_COMPOSER[@]}" \
    install --no-dev --no-interaction --no-progress )

rm -rf "$DEST/vendor" "$DEST/composer.json" "$DEST/composer.lock"
mkdir -p "$DEST"
cp "$WORK/composer.json" "$WORK/composer.lock" "$DEST/"
cp -R "$WORK/vendor" "$DEST/vendor"

echo
echo "vendored into $DEST:"
find "$DEST/vendor" -type f | wc -l | tr -d ' ' | sed 's/^/  files: /'
du -sh "$DEST/vendor" | sed 's/^/  size:  /'
echo
echo "packages (from composer.lock — this is the provenance record):"
"$PHP" -r '
$l = json_decode(file_get_contents($argv[1]), true);
foreach ($l["packages"] as $p) {
    printf("  %-40s %-10s %s %s\n", $p["name"], $p["version"],
        substr($p["dist"]["reference"], 0, 12), implode(",", $p["license"]));
}
printf("  content-hash: %s\n", $l["content-hash"]);
' "$DEST/composer.lock"
echo
echo "Next: update DIST_ARCHIVE_VERSION in src-tauri/src/core/wp_packages.rs and the"
echo "two rows in THIRD-PARTY-NOTICES.md, then run scripts/verify.sh."

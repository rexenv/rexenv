#!/bin/bash
# Render the three winget manifests for a rexenv release — the Windows counterpart
# of the Homebrew cask, and like the cask, derived from the PUBLISHED asset rather
# than typed.
#
#   scripts/winget-manifest.sh                       the tap's latest release (needs gh)
#   scripts/winget-manifest.sh 0.7.1                 that version's release
#   scripts/winget-manifest.sh --local <setup.exe> <version> [--url <url>]
#                                                    render from a local installer, for
#                                                    `winget validate` before a release exists
#
# Output: src-tauri/target/winget/<version>/rexenv.rexenv{,.installer,.locale.en-US}.yaml —
# the exact files a winget-pkgs PR carries under manifests/r/rexenv/rexenv/<version>/.
#
# # What is derived, and from where
#
#   • the installer URL and its digest — from the release's own asset, then the
#     asset is DOWNLOADED and hashed here, because a hash nobody in this run
#     computed is a hash this run is merely repeating (the runtimes publisher's rule);
#   • the version — the release tag, checked to be plain three-segment semver;
#   • every URL points at something the PUBLIC can open. `rexenv/rexenv` is a
#     PRIVATE repo, so `PackageUrl`/`PublisherSupportUrl`/`LicenseUrl` pointing
#     there answered 404 to everyone including winget's own URL validation
#     (measured 20 Sep 2026, before the first submission). They name the tap,
#     which is where the installer itself lives; `LicenseUrl` is omitted rather
#     than pointed at a file nobody can read — the `License` field still says
#     Apache-2.0. When the source repo goes public, both move back (docs/TODO.md);
#   • the fields winget needs to recognise an installed copy — read off what the
#     NSIS installer actually writes: `AppsAndFeaturesEntries` names the uninstall
#     entry (`DisplayName` rexenv, `ProductCode` rexenv — the key's leaf under
#     HKCU\…\Uninstall, which is what `platform/windows/app_bundle_rules.rs`
#     names too), and `Scope: user` because that is the only mode rexenv ships (D5).
#
# # What winget-pkgs will do with it, read from their docs on 19 Sep 2026
#
# No Authenticode requirement exists. The pipeline's "SmartScreen validation" is
# about the URL's reputation, not the binary's signature, and "binary validation"
# is static analysis, hash and malware scanning — so an unsigned rexenv is
# submittable (docs/TODO.md, W11). The cost stays where D5 put it: on the user,
# who meets SmartScreen on first run.
set -euo pipefail

cd "$(dirname "$0")/.."

RELEASES="${RELEASES_REPO:-rexenv/rexenv}"
ID="rexenv.rexenv"
MANIFEST_VERSION="1.9.0"

fail() { echo "winget-manifest: $*" >&2; exit 1; }

LOCAL=""
WANT=""
URL=""
while [ $# -gt 0 ]; do
  case "$1" in
    --local) LOCAL="$2"; WANT="${3#v}"; shift 3 ;;
    --url) URL="$2"; shift 2 ;;
    -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
    *) WANT="${1#v}"; shift ;;
  esac
done

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

if [ -n "$LOCAL" ]; then
  [ -f "$LOCAL" ] || fail "no such installer: $LOCAL"
  V="$WANT"
  [ -n "$V" ] || fail "--local needs the version: --local <setup.exe> <version>"
  ASSET="rexenv_${V}_x64-setup.exe"
  URL="${URL:-https://github.com/$RELEASES/releases/download/v$V/$ASSET}"
  cp "$LOCAL" "$WORK/$ASSET"
  PUBLISHED_AT="$(date -u +%Y-%m-%d)"
else
  command -v gh >/dev/null || fail "gh is required to read the release (or use --local)"
  if [ -n "$WANT" ]; then
    TAG="v$WANT"
  else
    TAG="$(gh release list --repo "$RELEASES" --limit 1 --exclude-drafts --exclude-pre-releases \
             --json tagName --jq '.[0].tagName // ""')"
    [ -n "$TAG" ] || fail "no published release on $RELEASES"
  fi
  V="${TAG#v}"
  ASSET="rexenv_${V}_x64-setup.exe"
  REL="$(gh api "repos/$RELEASES/releases/tags/$TAG" 2>/dev/null)" \
    || fail "no release $TAG on $RELEASES (a draft is invisible here, which is the point)"
  URL="$(printf '%s' "$REL" | jq -r --arg n "$ASSET" 'first(.assets[] | select(.name == $n) | .browser_download_url) // ""')"
  DIGEST="$(printf '%s' "$REL" | jq -r --arg n "$ASSET" 'first(.assets[] | select(.name == $n) | .digest) // ""')"
  DIGEST="${DIGEST#sha256:}"
  PUBLISHED_AT="$(printf '%s' "$REL" | jq -r '.published_at // ""' | cut -c1-10)"
  [ -n "$URL" ] || fail "release $TAG carries no $ASSET — build it with pnpm release:win first"
  echo "downloading $ASSET …"
  curl -fsSL "$URL" -o "$WORK/$ASSET" || fail "could not download $URL"
fi

printf '%s' "$V" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' \
  || fail "'$V' is not plain three-segment semver"

# Hash what a user would download. Never the API's digest alone.
if command -v sha256sum >/dev/null; then
  SHA="$(sha256sum "$WORK/$ASSET" | awk '{print $1}')"
else
  SHA="$(shasum -a 256 "$WORK/$ASSET" | awk '{print $1}')"
fi
SHA="$(printf '%s' "$SHA" | tr 'a-f' 'A-F')"
if [ -n "${DIGEST:-}" ] && [ "$(printf '%s' "$DIGEST" | tr 'a-f' 'A-F')" != "$SHA" ]; then
  fail "the asset's API digest and its bytes DISAGREE — refusing to render"
fi
# The installer must be what D5 says it is: unsigned NSIS, per user. Its PE
# header is the honest check for "an executable at all".
head -c 2 "$WORK/$ASSET" | grep -q "MZ" || fail "$ASSET is not an executable"

OUT="src-tauri/target/winget/$V"
mkdir -p "$OUT"

cat > "$OUT/$ID.yaml" <<YAML
# Created with scripts/winget-manifest.sh (rexenv) — derived from the published release, not typed.
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.version.${MANIFEST_VERSION}.schema.json
PackageIdentifier: $ID
PackageVersion: $V
DefaultLocale: en-US
ManifestType: version
ManifestVersion: $MANIFEST_VERSION
YAML

cat > "$OUT/$ID.installer.yaml" <<YAML
# Created with scripts/winget-manifest.sh (rexenv) — derived from the published release, not typed.
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.installer.${MANIFEST_VERSION}.schema.json
PackageIdentifier: $ID
PackageVersion: $V
InstallerType: nullsoft
Scope: user
InstallModes:
- interactive
- silent
- silentWithProgress
InstallerSwitches:
  Silent: /S
  SilentWithProgress: /S
UpgradeBehavior: install
ReleaseDate: $PUBLISHED_AT
AppsAndFeaturesEntries:
- DisplayName: rexenv
  Publisher: rexenv
  DisplayVersion: $V
  ProductCode: rexenv
Installers:
- Architecture: x64
  InstallerUrl: $URL
  InstallerSha256: $SHA
ManifestType: installer
ManifestVersion: $MANIFEST_VERSION
YAML

cat > "$OUT/$ID.locale.en-US.yaml" <<YAML
# Created with scripts/winget-manifest.sh (rexenv) — derived from the published release, not typed.
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.defaultLocale.${MANIFEST_VERSION}.schema.json
PackageIdentifier: $ID
PackageVersion: $V
PackageLocale: en-US
Publisher: rexenv
PublisherUrl: https://github.com/rexenv
PublisherSupportUrl: https://github.com/$RELEASES/issues
PackageName: rexenv
PackageUrl: https://github.com/$RELEASES
License: Apache-2.0
ShortDescription: A native, lightweight, no-Docker local development environment for web & WordPress developers.
Description: rexenv runs the whole local stack — edge proxy with auto-HTTPS, shared web server, multi-version PHP, MySQL/PostgreSQL, one-click WordPress, .rex DNS, mail catching, tunnels — from one window, with native binaries and no Docker.
Moniker: rexenv
Tags:
- wordpress
- php
- local-development
- laravel
ManifestType: defaultLocale
ManifestVersion: $MANIFEST_VERSION
YAML

echo
echo "winget-manifest: rendered $ID $V"
echo "  installer  $URL"
echo "  sha256     $SHA"
echo "  files      $OUT/"
echo
echo "Validate on a Windows machine:  winget validate --manifest \"$OUT\""
echo "Submit: copy $OUT/* to manifests/r/rexenv/rexenv/$V/ in a fork of microsoft/winget-pkgs,"
echo "one version per PR (their rule)."

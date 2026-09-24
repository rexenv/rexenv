#!/bin/bash
# The Linux release build (docs/PLAN-linux-port.md L3): `tauri build` on an Ubuntu host.
#
#   ./scripts/release-linux.sh        (this is what `npm run release:linux` runs)
#
# # Why a wrapper exists at all
#
# The same reason the macOS and Windows ones do: a piece of state `tauri build` needs
# and does not handle. On Linux the app's DNS agent is the app's OWN binary
# (`rexenv --dns-agent`) under a systemd USER unit with `Restart=always`, and a
# development build registers `target/release/rexenv` as that unit's program. A link
# step that replaces a running executable succeeds on Linux (the old inode lives on),
# so the build does not fail — but the agent keeps running the OLD binary until the
# unit restarts, and the release-gate machine then tests a resolver that is not the
# build it thinks it is. So the agent is restarted after the build, when its program
# is the new file.
#
# CLEANS: nothing before the build. Linux does not lock a running executable.
# DOES NOT clean: the previous `.deb`/`.AppImage`. A build that fails after deleting
# them would leave the developer with neither; the artefact count below reads the
# directory as it is.
#
# Builds ON Linux only: the `.deb` and AppImage steps run `dpkg-deb`/`linuxdeploy` on
# the host, and the CLI sidecar is built for the host triple by build-cli.sh. Cross
# builds are for examples (docs/TESTING.md "Proving a Linux claim").
set -euo pipefail

cd "$(dirname "$0")/.."

if [ "$(uname -s)" != "Linux" ]; then
  echo "release-linux.sh: this builds ON Linux (Ubuntu 22.04+)." >&2
  echo "  The deb and AppImage bundlers run on the host; nothing here cross-compiles." >&2
  exit 1
fi

for tool in dpkg-deb; do
  command -v "$tool" >/dev/null 2>&1 || { echo "release-linux.sh: $tool is missing (apt install dpkg)"; exit 1; }
done

BUNDLE="src-tauri/target/release/bundle"
npx tauri build

# The DNS agent, if a dev launch registered one on the binary this build just replaced.
if systemctl --user is-active rexenv-dns.service >/dev/null 2>&1; then
  systemctl --user restart rexenv-dns.service && echo "post-build: restarted the rexenv-dns user unit on the new binary"
fi

echo "release-linux: artefacts"
find "$BUNDLE/deb" "$BUNDLE/appimage" "$BUNDLE/rpm" -maxdepth 1 -type f \( -name '*.deb' -o -name '*.AppImage' -o -name '*.rpm' \) 2>/dev/null | while read -r f; do
  printf '  %s  %s\n' "$(sha256sum "$f" | cut -c1-64)" "$f"
done

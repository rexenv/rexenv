#!/bin/bash
# The Windows release build, with the pre-clean `tauri build` needs and does not do.
#
#   ./scripts/release-windows.sh        (this is what `npm run release:win` runs)
#
# # Why a wrapper exists at all
#
# The same reason `release-mac.sh` has one, with a different piece of state left
# behind. On Windows the app's DNS agent is the app's OWN binary
# (`rexenv.exe --dns-agent`), it OUTLIVES the app by design, and its watchdog puts
# it back. So a build whose link step has to replace `rexenv.exe` dies with:
#
#     error: failed to remove file `…\target\release\rexenv.exe`
#     Caused by: Access is denied. (os error 5)
#
# which names neither the holder nor the fact that closing the app does not release
# it. Measured 19 Sep 2026: `Stop-Process rexenv` was not enough — the agent came
# straight back, and the kill and the link had to happen in one breath.
#
# CLEANS: rexenv processes that would hold the output binary — the app, its DNS
# agent, and any elevated step — by IMAGE NAME, which is safe because that name is
# ours. Then it verifies the file is actually writable before starting a 20-minute
# build that would otherwise fail at the end.
#
# DOES NOT clean: the previous installer. A build that fails after we deleted it
# would leave the developer with neither, and the artefact check below wants to
# count what is there.
set -euo pipefail

cd "$(dirname "$0")/.."
BUNDLE="src-tauri/target/release/bundle/nsis"
EXE="src-tauri/target/release/rexenv.exe"

case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*) ;;
  *)
    echo "release-windows.sh: this builds ON Windows (Git Bash). Cross-compiling a" >&2
    echo "  bundle is not what tauri does — the NSIS step runs makensis on the host." >&2
    exit 1
    ;;
esac

# The DNS agent is not a stray: it is supposed to be running. Stopping it here is
# the build's business and the app puts it back on next launch.
if taskkill //F //IM rexenv.exe //T >/dev/null 2>&1; then
  echo "pre-clean: stopped a running rexenv (app and/or DNS agent)"
  sleep 1
else
  echo "pre-clean: no rexenv running"
fi

# Prove the lock is gone BEFORE the build, not after. A release build is ~20
# minutes; discovering the hold at the link step wastes all of it.
if [ -e "$EXE" ] && ! (rm -f "$EXE" 2>/dev/null); then
  echo "release-windows: $EXE is still locked — something holds it." >&2
  echo "  Find it with:  Get-Process | Where-Object { \$_.Path -eq '$(pwd -W 2>/dev/null || pwd)\\$EXE' }" >&2
  exit 1
fi

if [ -d "$BUNDLE" ]; then
  n=$(find "$BUNDLE" -maxdepth 1 -name '*-setup.exe' | wc -l | tr -d ' ')
  if [ "$n" -gt 0 ]; then
    echo "pre-clean: WARNING — $n installer(s) already in $BUNDLE:" >&2
    find "$BUNDLE" -maxdepth 1 -name '*-setup.exe' -exec basename {} \; >&2
    echo "  The artefact check requires exactly one. Delete the stale version(s)." >&2
  fi
fi

# NSIS only. `bundle.targets` is "all", which on Windows also means an MSI — and
# WiX installs per-machine and wants admin, the opposite of what D5 ruled for this
# installer. Narrowing `targets` in the config would have changed the macOS build
# for a Windows reason, so the narrowing lives here, where the reason is.
npx tauri build --bundles nsis "$@"

exec "$(dirname "$0")/release-windows-check.sh"

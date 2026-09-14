#!/bin/bash
# Cross-build one Windows example on the Mac, run it on a Windows test machine over SSH, remove it:
#
#   scripts/probes/windows-example.sh dell@192.168.0.107 windows_cgi_breaker_check
#   scripts/probes/windows-example.sh w10.rex.bd windows_cgi_churn_probe      # away from home
#
# 1. `cargo xwin build --example <name>` (the sidecar placeholder staged exactly as
#    windows-files-check.sh stages it), then scp the exe to the SSH user's home.
# 2. Run it in one PowerShell session. The example owns its fixture and removes it.
# 3. Remove the exe — also on failure (trap).
#
# BUILD_ONLY=1 stops after the build (no SSH at all). The verdict is the example's own
# `PASS` line on the Windows machine; this script's exit code is only ssh's.
set -uo pipefail
host="${1:?usage: windows-example.sh <user@host> <example>}"
name="${2:?usage: windows-example.sh <user@host> <example>}"
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
[ -f "$repo/src-tauri/examples/$name.rs" ] || { echo "no example $name"; exit 1; }
O=(-o BatchMode=yes -o ConnectTimeout=20 -o ServerAliveInterval=15 -o ServerAliveCountMax=8)
F='post-quantum|store now|upgraded'
EXE="$name.exe"

ps() {
  local encoded
  encoded="$(printf '%s' "\$ProgressPreference = 'SilentlyContinue'
$1" | iconv -f UTF-8 -t UTF-16LE | base64 | tr -d '\n')"
  ssh "${O[@]}" "$host" "powershell -NoProfile -NonInteractive -EncodedCommand $encoded" 2>&1 | tr -d '\r' | grep -v -E "$F"
}

TARGET=x86_64-pc-windows-msvc
SIDECAR="$repo/src-tauri/binaries/rex-$TARGET.exe"
MARK='PLACEHOLDER staged by scripts/windows-check.sh — not a rex binary'
staged=0
copied=0
cleanup() {
  [ "$staged" -eq 1 ] && rm -f "$SIDECAR"
  [ "$copied" -eq 1 ] && ps "Remove-Item -Force \"\$HOME\\$EXE\" -ErrorAction SilentlyContinue
\"# cleaned: exe=\$(Test-Path \"\$HOME\\$EXE\")\""
}
trap cleanup EXIT
[ -e "$SIDECAR" ] || { printf '%s\n' "$MARK" > "$SIDECAR"; staged=1; }
export PATH="$(brew --prefix llvm)/bin:$(brew --prefix lld)/bin:$PATH"
log="$(mktemp)"
if ! (cd "$repo/src-tauri" && XWIN_ACCEPT_LICENSE=1 CARGO_TARGET_DIR=target/xwin cargo xwin build --example "$name" --target "$TARGET") > "$log" 2>&1; then
  echo "BUILD FAILED"; grep -E '^error' -A12 "$log" | head -60; exit 1
fi
[ "$staged" -eq 1 ] && { rm -f "$SIDECAR"; staged=0; }
grep -E "^warning" -A6 "$log" | grep -B3 -A3 "examples/$name.rs" | head -30
if [ "${BUILD_ONLY:-0}" = 1 ]; then echo "## built (BUILD_ONLY)"; exit 0; fi
scp -q "${O[@]}" "$repo/src-tauri/target/xwin/$TARGET/debug/examples/$EXE" "$host:$EXE" || { echo "copy failed"; exit 1; }
copied=1
echo "## built and copied"
ps "& \"\$HOME\\$EXE\"; \"exit=\$LASTEXITCODE\""

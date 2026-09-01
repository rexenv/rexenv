#!/bin/bash
# One-shot evidence for the menu-bar login leg (SMOKE-TEST "The menu bar",
# ledger #439). Run it AFTER logging back in, before opening rexenv's window.
#
# What it must show: rexenv running, launched WITH --hidden, sitting as an
# accessory app (no window, no dock tile), its services up and its sockets
# answering — and the log line that says the decision was taken deliberately.
cd "$(dirname "$0")/.."
echo "=== process (should be running, with --hidden) ==="
ps ax -o pid,lstart,command | grep -E "/rexenv$|rexenv --hidden|MacOS/rexenv" | grep -v grep | grep -v dns-agent

echo
echo "=== activation policy (UIElement = no dock tile, i.e. no window up) ==="
lsappinfo list 2>/dev/null | grep -B 2 -A 4 "rexenv.app\"" | grep -E "bundle path|pid = " | head -4

echo
echo "=== the app's own verdict, from its log ==="
grep -E "launched at login" "$HOME/Library/Application Support/dev.rexenv.rexenv/logs/rexenv.log" | tail -3

echo
echo "=== control planes with no window ==="
./src-tauri/binaries/rex-universal-apple-darwin status 2>&1 | head -3
ls "$HOME/Library/Application Support/dev.rexenv.rexenv/config/"*.sock 2>/dev/null | xargs -n1 basename

echo
echo "=== the login plist that produced all this ==="
grep -A 4 ProgramArguments "$HOME/Library/LaunchAgents/dev.rexenv.rexenv.plist"

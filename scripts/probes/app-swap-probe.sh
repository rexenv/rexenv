#!/bin/bash
# T0 of docs/PLAN-self-update.md — build a throwaway ad-hoc-signed app in /Applications,
# launch it the way Finder does, and let it measure whether it can replace ITSELF.
#
#   scripts/probes/app-swap-probe.sh                # the ordinary leg
#   scripts/probes/app-swap-probe.sh --quarantine   # simulate a browser-downloaded copy
#   scripts/probes/app-swap-probe.sh --keep         # leave the fixture on disk to inspect
#   scripts/probes/app-swap-probe.sh --clean        # remove a fixture a previous run left
#
# # Why this exists
#
# rexenv's self-update replaces /Applications/rexenv.app with a verified copy of itself.
# macOS App Management blocks bundle modification by anything not signed with the same
# Team ID — and rexenv is AD-HOC signed, with no Team ID, a case Apple documents nowhere.
# Rust maps EACCES and EPERM to one error kind, so a policy refusal and a permissions
# problem are indistinguishable without measuring the errno. The plan's error handling —
# and in outcome O4 whether an in-app install ships at all — depends on the answer, so it
# is measured BEFORE the code is designed. That ordering is the ledger's standing first
# step for any claim about a third party's behaviour.
#
# # What it will and will not touch
#
# CREATES and REMOVES exactly two things: /Applications/RexSwapProbe.app and
# /Applications/.rexswapprobe-stage-<pid>. The probe binary refuses any other path and
# aborts on a path naming rexenv; this script refuses to run if an argument names rexenv,
# and its cleanup matches the same two prefixes and nothing else. The real rexenv.app is
# never touched, never quit, and never read.
#
# # What only a human can see, and must watch for
#
# A "RexSwapProbe was prevented from modifying apps on your Mac" notification, a
# Gatekeeper "cannot be opened" dialog, or an App Management prompt in System Settings.
# None of them appear in any log this script can capture, and their presence changes the
# outcome letter. The script prints a WATCH FOR block before launching, and asks at the
# end.
set -euo pipefail

APP="/Applications/RexSwapProbe.app"
STAGE_PREFIX="/Applications/.rexswapprobe-stage-"
BUNDLE_ID="dev.rexenv.rexswapprobe"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
RUNDIR="${TMPDIR:-/tmp}/rexswapprobe"
TS="$(date +%Y%m%d-%H%M%S)"
LOG="$RUNDIR/probe-$TS.log"
TCC_LOG="$RUNDIR/tcc-$TS.log"

QUARANTINE=0
KEEP=0
CLEAN_ONLY=0

for arg in "$@"; do
  # A probe that writes into /Applications must never be pointed at the real app, not
  # even by accident of a shell history line.
  case "$(printf '%s' "$arg" | tr '[:upper:]' '[:lower:]')" in
    *rexenv*) echo "refusing: an argument names rexenv ($arg). This probe only ever touches RexSwapProbe.app." >&2; exit 2 ;;
  esac
  case "$arg" in
    --quarantine) QUARANTINE=1 ;;
    --keep) KEEP=1 ;;
    --clean) CLEAN_ONLY=1 ;;
    -h|--help) sed -n '2,10p' "$0"; exit 0 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

[ "$(uname -s)" = "Darwin" ] || { echo "macOS only." >&2; exit 2; }

# ── Cleanup, scoped by construction ───────────────────────────────────────────
# Never a variable that could be empty, never a glob wider than the fixture, and a
# last-line check on the literal prefix before every rm.
cleanup_fixture() {
  if [ -d "$APP" ]; then
    case "$APP" in
      /Applications/RexSwapProbe.app) rm -rf "$APP" && echo "cleaned: $APP" ;;
      *) echo "refusing to remove $APP" >&2 ;;
    esac
  fi
  for d in /Applications/.rexswapprobe-stage-*; do
    [ -e "$d" ] || continue
    case "$d" in
      /Applications/.rexswapprobe-stage-*) rm -rf "$d" && echo "cleaned: $d" ;;
    esac
  done
}

if [ "$CLEAN_ONLY" = "1" ]; then
  cleanup_fixture
  exit 0
fi

if [ -e "$APP" ]; then
  echo "refusing: $APP already exists — a previous run did not clean up." >&2
  echo "  inspect it, then: $0 --clean" >&2
  exit 1
fi

mkdir -p "$RUNDIR"

# ── Build the probe binary ────────────────────────────────────────────────────
echo "building the probe binary…"
cargo build --manifest-path "$ROOT/src-tauri/Cargo.toml" --example app_swap_probe
BIN="$ROOT/src-tauri/target/debug/examples/app_swap_probe"
[ -x "$BIN" ] || { echo "probe binary not found at $BIN" >&2; exit 1; }

# ── Assemble a throwaway .app around it ───────────────────────────────────────
# LSUIElement so the probe never steals focus or a Dock tile; its own bundle id so
# LaunchServices can never confuse it with rexenv; version 1.0.0 so the staged copy's
# 2.0.0 proves which one is running after a swap.
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/RexSwapProbe"
cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleName</key><string>RexSwapProbe</string>
	<key>CFBundleDisplayName</key><string>RexSwapProbe</string>
	<key>CFBundleExecutable</key><string>RexSwapProbe</string>
	<key>CFBundleIdentifier</key><string>dev.rexenv.rexswapprobe</string>
	<key>CFBundlePackageType</key><string>APPL</string>
	<key>CFBundleShortVersionString</key><string>1.0.0</string>
	<key>CFBundleVersion</key><string>1.0.0</string>
	<key>LSMinimumSystemVersion</key><string>15.0</string>
	<key>LSUIElement</key><true/>
	<key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# Ad-hoc, exactly like the shipped rexenv bundle (tauri.conf.json signingIdentity "-").
codesign -f -s - "$APP"
codesign -dv "$APP" 2>&1 | grep -E 'Signature|TeamIdentifier|flags' || true

if [ "$QUARANTINE" = "1" ]; then
  # The same simulated-browser-download attribute PUBLISH-TESTING §A uses. This leg also
  # measures App Translocation: a quarantined bundle launched through LaunchServices and
  # never Finder-moved runs from a read-only copy, which the probe reports.
  xattr -w com.apple.quarantine "0083;$(printf '%x' "$(date +%s)");Safari;$(uuidgen)" "$APP"
  echo "quarantine attribute written (translocation is expected on this leg)"
fi

# ── Capture the TCC log alongside ─────────────────────────────────────────────
# App Management denials are logged under com.apple.TCC as
# kTCCServiceSystemPolicyAppBundles. Without sudo the interesting fields may be redacted
# as <private>, so an empty capture is NOT proof that nothing was denied — the errno the
# probe records is the primary evidence and this is corroboration.
log stream --style compact --predicate 'subsystem == "com.apple.TCC"' > "$TCC_LOG" 2>&1 &
TCC_PID=$!
trap 'kill "$TCC_PID" 2>/dev/null || true' EXIT
sleep 1

cat <<BANNER

────────────────────────────────────────────────────────────────────────────────
WATCH FOR — none of this reaches a log, and each one changes the outcome letter:

  • a notification "RexSwapProbe was prevented from modifying apps on your Mac"
  • a Gatekeeper dialog ("cannot be opened", "is damaged")
  • System Settings › Privacy & Security › App Management gaining a RexSwapProbe row

The probe is a background app (no Dock icon, no window). It writes to:
  $LOG
────────────────────────────────────────────────────────────────────────────────

BANNER

# ── Launch it the way a double-click does ─────────────────────────────────────
echo "launching through LaunchServices…"
if ! open "$APP" --args measure "$LOG"; then
  echo "open refused to launch the probe — that is itself a finding (Gatekeeper)." >&2
fi

wait_for() { # wait_for <marker> <seconds>
  local marker="$1" limit="$2" waited=0
  while [ "$waited" -lt "$limit" ]; do
    [ -f "$LOG" ] && grep -q "$marker" "$LOG" && return 0
    sleep 1
    waited=$((waited + 1))
  done
  return 1
}

if wait_for "DONE run=1" 120; then
  echo "run 1 finished."
else
  echo "run 1 did not finish within 120s — see $LOG (it may not have launched at all)." >&2
fi

if grep -q '^RESULT relaunch_helper_spawned=ok' "$LOG" 2>/dev/null; then
  echo "waiting for the relaunched copy…"
  wait_for "DONE run=2" 60 || echo "the relaunch never reported in — a finding, see below." >&2
fi

sleep 1
kill "$TCC_PID" 2>/dev/null || true

# ── Summary ───────────────────────────────────────────────────────────────────
echo
echo "=== RESULTS ($LOG) ==="
grep '^RESULT ' "$LOG" 2>/dev/null || echo "(no results — the probe never ran)"

echo
echo "=== TCC lines mentioning app-bundle policy ($TCC_LOG) ==="
if grep -i 'SystemPolicyAppBundles' "$TCC_LOG" 2>/dev/null; then
  :
else
  echo "(none — but without sudo these fields are often <private>; the errnos above are the evidence)"
fi

# The suggested letter is arithmetic on the errnos; the HUMAN owns the final call,
# because two of the seven outcomes turn on a notification no log can see.
r() { grep "^RESULT $1=" "$LOG" 2>/dev/null | tail -1 | cut -d= -f2-; }
SWAP="$(r renamex_np_swap)"; ASIDE="$(r rename_aside)"; IN="$(r rename_in)"
RELAUNCH="$(r relaunch_started)"; MKDIR="$(r stage_mkdir)"; TRANS="$(r translocated)"

echo
echo "=== SUGGESTED OUTCOME (docs/PLAN-self-update.md §6.5) ==="
if [ "$TRANS" = "true" ]; then
  echo "  translocated launch — this leg measured the refusal path (R6), not the swap."
elif [ "${MKDIR:-}" != "ok" ]; then
  echo "  O7 — the staging mkdir in /Applications failed (${MKDIR:-no result})."
elif [ "$SWAP" = "ok" ]; then
  if [ "$RELAUNCH" = "yes" ]; then
    echo "  O1 — the atomic swap worked and the new copy relaunched."
    echo "       (O5 instead if you SAW the 'prevented from modifying apps' notification.)"
  else
    echo "  O6 candidate — the swap worked but the relaunch did not report in."
  fi
elif [ "$ASIDE" = "ok" ] && [ "$IN" = "ok" ]; then
  echo "  O2 — RENAME_SWAP was refused ($SWAP) but the rename pair works."
elif [ "$(r o3_rename_in)" = "ok" ]; then
  echo "  O3 — both rename shapes refused; delete-then-create worked."
else
  echo "  O4 — nothing could replace the bundle. In-app install does not ship;"
  echo "       the check, the offer and the release flow still do."
fi
echo
echo "Record the letter, the raw log and the macOS build in docs/PLAN-self-update.md §T0,"
echo "then rewrite §6 to match what was measured."

if [ "$KEEP" = "1" ]; then
  echo
  echo "--keep: leaving the fixture in place. Remove it with: $0 --clean"
else
  echo
  cleanup_fixture
fi

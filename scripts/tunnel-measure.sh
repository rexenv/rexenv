#!/bin/bash
# Timing recorder for the tunnel probe sessions (docs/TODO.md "PROBE SESSION",
# docs/PLAN-tunnel-lifecycle.md). The physical part stays human — sharing,
# `kill -9`, toggling wifi; THIS records the second-by-second facts so the
# session produces timings instead of badge-reading and guessing.
#
# Usage:
#   scripts/tunnel-measure.sh <trycloudflare-url-or-host> [duration-seconds]
#
# Start it the MOMENT the share card shows the URL — t0 is the banner anchor
# every delta is measured from. Default duration 180s; Ctrl-C ends early with
# the summary intact. Press ENTER at any time to drop a MARK row (type a note
# first, e.g. `killed -9<Enter>` or `wifi off<Enter>`) — marks timestamp the
# physical actions so the windows around them are attributable.
#
# Per tick (~1s), one CSV row + console line:
#   cf1111  A answer at 1.1.1.1 (the resolver Phase A trusts)
#   auth    A answer at trycloudflare.com's own authoritative NS
#   sys     A answer via the SYSTEM resolver (the router — the negative-cache
#           victim; trycloudflare SOA MINIMUM = 1800s, measured 28 Jul 2026)
#   http    HTTPS status of GET / ("000" = transport error; 530 = edge 1033)
#   cfd     local cloudflared process count (the kill -9 subject)
#
# The three owed sessions, all with this one script:
#   fresh share    → run with no marks; summary's banner→cf1111/auth/sys
#                    deltas ARE the propagation gaps.
#   kill -9        → mark at the kill; watch http flip to 530/000 and the app's
#                    card — the mark→530 and mark→card-change gaps are the
#                    Broken-window numbers.
#   wifi blip      → mark off/on; whether http recovers on the SAME hostname
#                    answers URL stability.
# NOT set -e: probes failing is DATA here (a dead resolver, a 000, a missing
# NS are all rows), and bare `[ … ] && record` lists must not kill the loop.
set -uo pipefail

if [ $# -lt 1 ]; then
  echo "usage: tunnel-measure.sh <trycloudflare-url-or-host> [seconds]" >&2
  exit 2
fi
HOST="$1"
HOST="${HOST#https://}"
HOST="${HOST#http://}"
HOST="${HOST%%/*}"
DURATION="${2:-180}"

OUT="tunnel-measure-${HOST%%.*}-$(date +%Y%m%d-%H%M%S).csv"
echo "t,wall,cf1111,auth,sys,http,cfd,mark" >"$OUT"

# Authoritative NS discovered once — queried directly it can't be cached.
AUTH_NS="$(dig NS trycloudflare.com @1.1.1.1 +short +time=2 +tries=1 2>/dev/null | head -1)"
if [ -z "$AUTH_NS" ]; then
  echo "note: could not discover trycloudflare.com NS via 1.1.1.1 — auth column will read '-'"
fi

T0="$(date +%s)"
echo "t0 (banner anchor) = $(date '+%H:%M:%S') · host = $HOST · auth NS = ${AUTH_NS:-—}"
echo "recording to $OUT — ENTER after typing a note drops a MARK (e.g. 'killed -9')"

probe_a() { # resolver → first A answer or "-"
  local at="$1"
  local ans
  ans="$(dig A "$HOST" "@$at" +short +time=1 +tries=1 2>/dev/null | grep -E '^[0-9.]+$' | head -1)"
  echo "${ans:--}"
}
probe_sys() {
  local ans
  ans="$(dig A "$HOST" +short +time=1 +tries=1 2>/dev/null | grep -E '^[0-9.]+$' | head -1)"
  echo "${ans:--}"
}

# First-seen transition trackers (bash-3.2-safe plain vars).
first_cf="" first_auth="" first_sys="" first_http="" first_ok="" first_530=""
first_530_after_ok="" recovered="" proc_drop="" proc_back=""
last_procs=""

summary_line() { # name value(seconds-from-t0 or empty)
  if [ -n "$2" ]; then
    printf "  %-34s t0+%ss\n" "$1" "$2"
  else
    printf "  %-34s never seen\n" "$1"
  fi
}

finish() {
  echo
  echo "── summary (deltas from the banner anchor) ──"
  summary_line "1.1.1.1 first answers (Phase A view)" "$first_cf"
  summary_line "authoritative first answers" "$first_auth"
  summary_line "SYSTEM resolver first answers" "$first_sys"
  summary_line "first HTTP response of any kind" "$first_http"
  summary_line "first non-530 response (path proven)" "$first_ok"
  summary_line "first 530 (edge 1033)" "$first_530"
  summary_line "530 AFTER being live (break began)" "$first_530_after_ok"
  summary_line "recovery after 530" "$recovered"
  summary_line "cloudflared count dropped" "$proc_drop"
  summary_line "cloudflared count came back" "$proc_back"
  echo
  echo "context: trycloudflare SOA MINIMUM = 1800s — a system-resolver NXDOMAIN"
  echo "taken before 'authoritative first answers' negative-caches the ROUTER for"
  echo "30 min (every device on the LAN). The banner→authoritative gap above is"
  echo "the window a human-speed click races. Full per-second data: $OUT"
  exit 0
}
trap finish INT

while :; do
  now="$(date +%s)"
  t=$((now - T0))
  [ "$t" -ge "$DURATION" ] && finish

  cf="$(probe_a 1.1.1.1)"
  auth="-"
  [ -n "$AUTH_NS" ] && auth="$(probe_a "$AUTH_NS")"
  sys="$(probe_sys)"
  # curl writes the -w code ("000") even on transport failure — only the exit
  # status needs suppressing under set -e, never a second echo.
  http="$(curl -s -o /dev/null -m 3 -w '%{http_code}' "https://$HOST/" 2>/dev/null)" || true
  procs="$(pgrep -x cloudflared 2>/dev/null | wc -l | tr -d ' ')"

  # Mark capture doubles as the tick sleep. On a dead/EOF stdin (detached
  # run) `read` returns instantly — fall back to a plain sleep so the loop
  # keeps its 1s cadence instead of spinning.
  mark=""
  if [ "${stdin_dead:-0}" = 1 ]; then
    sleep 1
  else
    read -r -t 1 mark
    rc=$?
    if [ "$rc" -eq 0 ]; then
      mark="${mark:-MARK}"
    elif [ "$rc" -le 128 ]; then
      stdin_dead=1 # EOF (timeout returns >128)
      sleep 1
    fi
  fi

  # Transitions (recorded once, flagged on the console line).
  flag=""
  [ -z "$first_cf" ] && [ "$cf" != "-" ] && first_cf="$t" && flag="$flag ★cf1111"
  [ -z "$first_auth" ] && [ "$auth" != "-" ] && first_auth="$t" && flag="$flag ★auth"
  [ -z "$first_sys" ] && [ "$sys" != "-" ] && first_sys="$t" && flag="$flag ★sys"
  [ -z "$first_http" ] && [ "$http" != "000" ] && first_http="$t" && flag="$flag ★http"
  if [ -z "$first_ok" ] && [ "$http" != "000" ] && [ "$http" != "530" ]; then
    first_ok="$t" && flag="$flag ★path-proven"
  fi
  if [ "$http" = "530" ]; then
    [ -z "$first_530" ] && first_530="$t" && flag="$flag ★530"
    if [ -n "$first_ok" ] && [ -z "$first_530_after_ok" ]; then
      first_530_after_ok="$t" && flag="$flag ★break"
    fi
  fi
  if [ -n "$first_530_after_ok" ] && [ -z "$recovered" ] && [ "$http" != "000" ] && [ "$http" != "530" ]; then
    recovered="$t" && flag="$flag ★recovered"
  fi
  if [ -n "$last_procs" ]; then
    [ -z "$proc_drop" ] && [ "$procs" -lt "$last_procs" ] && proc_drop="$t" && flag="$flag ★cfd-drop"
    [ -n "$proc_drop" ] && [ -z "$proc_back" ] && [ "$procs" -gt "$last_procs" ] && proc_back="$t" && flag="$flag ★cfd-back"
  fi
  last_procs="$procs"

  printf "%4ss  cf1111=%-15s auth=%-15s sys=%-15s http=%s cfd=%s%s%s\n" \
    "$t" "$cf" "$auth" "$sys" "$http" "$procs" \
    "${mark:+  ◀ MARK: $mark}" "$flag"
  echo "$t,$(date '+%H:%M:%S'),$cf,$auth,$sys,$http,$procs,$mark" >>"$OUT"
done

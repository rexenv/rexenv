#!/bin/bash
# Run a PowerShell probe on a Windows test machine over SSH, from the Mac.
#
#   scripts/probes/windows-probe.sh <user@host> <script.ps1>
#   scripts/probes/windows-probe.sh dell@192.168.0.107 scripts/probes/windows-inventory.ps1
#
# # Why the script is sent encoded
#
# The Dell's OpenSSH default shell is Windows PowerShell 5.1 (docs/PLAN-windows-port.md §7).
# The first attempt piped the script into `powershell -Command -`, which reads stdin LINE BY
# LINE: a `foreach { … }` spanning lines broke, and everything after it vanished with no
# error (13 Sep 2026). `-EncodedCommand` hands PowerShell the whole script at once, as
# base64 of UTF-16LE — its documented input form.
#
# Key authentication only (BatchMode): a probe must never hang on a password prompt. The
# first connection to a host accepts its key (accept-new) — fine on the owner's LAN, and
# said here so it is a decision, not an accident. The session carries an ELEVATED token.
#
# stdout is the probe's output. The OpenSSH post-quantum notice and PowerShell's CLIXML
# progress records go to stderr and are dropped. Exit code is ssh's.
set -euo pipefail

if [ $# -ne 2 ]; then
  echo "usage: windows-probe.sh <user@host> <script.ps1>" >&2
  exit 2
fi
host="$1"
script="$2"
[ -f "$script" ] || { echo "windows-probe: no such script: $script" >&2; exit 2; }

encoded="$(iconv -f UTF-8 -t UTF-16LE "$script" | base64 | tr -d '\n')"
# The encoded form must fit Windows' 32 767-character command line with room for the rest.
if [ "${#encoded}" -gt 30000 ]; then
  echo "windows-probe: $script encodes to ${#encoded} characters — too long for -EncodedCommand" >&2
  exit 2
fi

ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new -o ConnectTimeout=10 "$host" \
  "powershell -NoProfile -NonInteractive -EncodedCommand $encoded" 2>/dev/null

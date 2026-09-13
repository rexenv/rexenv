#!/bin/bash
# Run a PowerShell probe on a Windows test machine under a NON-ELEVATED token, from the Mac.
#
#   scripts/probes/windows-limited-token.sh <user@host> <probe.ps1>
#   scripts/probes/windows-limited-token.sh dell@192.168.0.107 scripts/probes/windows-bind-matrix.ps1
#
# # Why a scheduled task
#
# Every SSH session on the Dell gets the admin's ELEVATED token (measured 13 Sep 2026,
# docs/PLAN-windows-port.md §7), and a desktop user runs with the filtered one. To get the
# filtered token without a person at the keyboard, Task Scheduler starts the probe with an
# INTERACTIVE logon and RunLevel Limited, so it runs in the logged-on user's desktop session
# with that session's filtered (Medium) token. windows-limited-run.ps1 prints the token it
# really received at the top of the output — trust those lines, not the request.
#
# S4U was tried first and does NOT work: an S4U task with RunLevel Limited ran with
# `token elevated: True` and a High integrity label (13 Sep 2026) — UAC splits the token only
# for interactive logons. Interactive has two costs, said here so they are not surprises: it
# needs the user logged on to the desktop (the Dell's `dell` is, console session 2), and a
# PowerShell window may flash on that desktop even with -WindowStyle Hidden.
#
# # Why short sessions, keepalives and scp
#
# The first version held ONE SSH session open while the task ran and streamed the output
# back through it. On 13 Sep 2026 that session sat silent for minutes, the connection died
# without either side noticing, and the Mac waited 40 minutes for output the Dell had
# already produced and cleaned away. So: every ssh/scp carries ServerAliveInterval (a dead
# link fails within a minute), each step is its own short session, the Mac polls, and the
# output file is copied back with scp rather than trusted to a stream. The Dell is on 2.4 GHz
# Wi-Fi with adapter power saving on, and SSH sessions to it do drop for minutes at a time.
#
# # Why the account name comes from the token
#
# The second version asked for "$env:USERDOMAIN\$env:USERNAME". In an SSH session on a
# workgroup machine USERDOMAIN is `WORKGROUP`, not the computer, so the task's principal named
# an account that does not exist ("No mapping between account names and security IDs was
# done"). Register-ScheduledTask reported that as a NON-terminating error, the script printed
# 'started' anyway, and ten minutes of polling watched a task that was never created. So the
# account is the process token's own name, and every task call now runs with -ErrorAction
# Stop inside a try that says what failed.
#
# # What it changes on the machine, and undoes
#
# Creates %LOCALAPPDATA%\Temp\rexenv-probe (the wrapper, the probe, its output) and ONE
# scheduled task, `rexenv-probe-limited`. Both are removed by a trap on EXIT, so a failure or
# an interrupt at any step still cleans up; a run killed with SIGKILL leaves them for the next
# run to overwrite and remove. Owner's go for creating the task: 13 Sep 2026.
#
# stdout: the probe output (wrapper header first), then `# task result:` and `# cleaned:`.
# stderr: one `# poll N:` line per poll.
set -euo pipefail

if [ $# -ne 2 ]; then
  echo "usage: windows-limited-token.sh <user@host> <probe.ps1>" >&2
  exit 2
fi
host="$1"
probe="$2"
here="$(cd "$(dirname "$0")" && pwd)"
[ -f "$probe" ] || { echo "windows-limited-token: no such probe: $probe" >&2; exit 2; }

opts=(-o BatchMode=yes -o StrictHostKeyChecking=accept-new -o ConnectTimeout=10
      -o ServerAliveInterval=15 -o ServerAliveCountMax=4)
work="$(mktemp -d)"

# run_ps <powershell source>: one short session, the script sent via -EncodedCommand.
run_ps() {
  local encoded
  encoded="$(printf '%s' "\$ProgressPreference = 'SilentlyContinue'
$1" | iconv -f UTF-8 -t UTF-16LE | base64 | tr -d '\n')"
  ssh "${opts[@]}" "$host" "powershell -NoProfile -NonInteractive -EncodedCommand $encoded" 2>/dev/null | tr -d '\r'
}

DIR_PS="\$dir = Join-Path \$env:LOCALAPPDATA 'Temp\\rexenv-probe'; \$name = 'rexenv-probe-limited'"

cleanup() {
  run_ps "$DIR_PS
Unregister-ScheduledTask -TaskName \$name -Confirm:\$false -ErrorAction SilentlyContinue
Remove-Item \$dir -Recurse -Force -ErrorAction SilentlyContinue
\"# cleaned: task present=\$([bool](Get-ScheduledTask -TaskName \$name -ErrorAction SilentlyContinue)) folder present=\$(Test-Path \$dir)\"" || echo "# cleanup session FAILED — check the task and folder by hand"
  rm -rf "$work"
}
trap cleanup EXIT

# A: the folder, the files, the task.
remote_dir="$(run_ps "$DIR_PS
Unregister-ScheduledTask -TaskName \$name -Confirm:\$false -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force \$dir | Out-Null
\$dir" | tail -1)"
[ -n "$remote_dir" ] || { echo "windows-limited-token: could not create the remote folder" >&2; exit 1; }
scp_dir="${remote_dir//\\//}"
scp -q "${opts[@]}" "$here/windows-limited-run.ps1" "$host:$scp_dir/windows-limited-run.ps1"
scp -q "${opts[@]}" "$probe" "$host:$scp_dir/probe.ps1"

started="$(run_ps "$DIR_PS
try {
  \$account = [Security.Principal.WindowsIdentity]::GetCurrent().Name
  \$action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument \"-NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File \`\"\$dir\\windows-limited-run.ps1\`\"\"
  \$principal = New-ScheduledTaskPrincipal -UserId \$account -LogonType Interactive -RunLevel Limited
  \$settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes 10) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
  Register-ScheduledTask -TaskName \$name -Action \$action -Principal \$principal -Settings \$settings -Force -ErrorAction Stop | Out-Null
  Start-ScheduledTask -TaskName \$name -ErrorAction Stop
  \"account: \$account\"
  'started'
} catch {
  \"REGISTER FAILED: \$(\$_.Exception.Message -replace '\\s+', ' ')\"
}")" || started="start session failed"
echo "# $started" | tr '\n' ' ' >&2; echo >&2
[ "$(printf '%s' "$started" | tail -1)" = "started" ] || { echo "windows-limited-token: the task did not start: $started" >&2; exit 1; }

# B: poll in short sessions until the wrapper has written its output and the task is idle.
# Every poll's state goes to stderr, and one failed poll session is a retry, not the end of
# the run: the 13 Sep 2026 run died on a single ssh 255 after ten silent minutes, with no
# record of what the task had been doing.
state=""
for poll in $(seq 1 60); do
  sleep 10
  if polled="$(run_ps "$DIR_PS
\$t = Get-ScheduledTask -TaskName \$name -ErrorAction SilentlyContinue
\$i = Get-ScheduledTaskInfo -TaskName \$name -ErrorAction SilentlyContinue
'{0} output={1} lastresult={2} lastrun={3}' -f \$(if (\$t) { \$t.State } else { 'Absent' }), (Test-Path (Join-Path \$dir 'output.txt')), \$i.LastTaskResult, \$i.LastRunTime" | tail -1)"; then
    state="$polled"
  else
    state="poll session failed"
  fi
  echo "# poll $poll: $state" >&2
  case "$state" in
    "Ready output=True"*|"Disabled output=True"*) break ;;
    Absent*) echo "windows-limited-token: the task disappeared — nothing to wait for" >&2; exit 1 ;;
  esac
done

# C: the output, copied back rather than streamed.
if scp -q "${opts[@]}" "$host:$scp_dir/output.txt" "$work/output.txt" 2>/dev/null; then
  tr -d '\r' < "$work/output.txt" | sed '1s/^\xEF\xBB\xBF//'
else
  echo "# NO OUTPUT — last task state: ${state:-unknown}"
fi
run_ps "\$i = Get-ScheduledTaskInfo -TaskName 'rexenv-probe-limited' -ErrorAction SilentlyContinue
\"# task result: \$(if (\$i) { \$i.LastTaskResult } else { 'no task' })\"" || echo "# task result: session failed"
# D: the trap removes the task and the folder.

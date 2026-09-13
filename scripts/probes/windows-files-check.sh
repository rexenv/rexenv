#!/bin/bash
# Ledger #597 and #598 on the Dell, driven from the Mac:
#
#   scripts/probes/windows-files-check.sh dell@192.168.0.107
#
# 1. Cross-build examples/windows_files_check.rs and copy it over.
# 2. `acl`: the example writes owner-only files beside a control under
#    C:\Users\Public\rexenv-acl-check (a folder another account can reach, so a denial is the
#    file's DACL and not the path's).
# 3. Get-Acl reads each DACL back: protected, one rule, the owner.
# 4. A scheduled task running as NT AUTHORITY\LOCAL SERVICE — another account, no user created —
#    reads every file: the control must be READ, each private file DENIED.
# 5. `motw`: Mark of the Web removal, image refusals, the tree check, the download scan.
# 6. Removes the fixture folder, the task and the exe — also on failure (trap).
#
# Why a scheduled task: "owner-only" is a claim about OTHER accounts, and the only one
# available without creating a user is a built-in service account; Task Scheduler is how a
# process gets to run as one (the same mechanism windows-limited-token.sh uses, owner go
# 13 Sep 2026).
set -uo pipefail
host="${1:?usage: windows-files-check.sh <user@host>}"
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
O=(-o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=15 -o ServerAliveCountMax=8)
F='post-quantum|store now|upgraded'
DIR='C:\Users\Public\rexenv-acl-check'
TASK='rexenv-acl-read-as'

ps() {
  local encoded
  encoded="$(printf '%s' "\$ProgressPreference = 'SilentlyContinue'
$1" | iconv -f UTF-8 -t UTF-16LE | base64 | tr -d '\n')"
  ssh "${O[@]}" "$host" "powershell -NoProfile -NonInteractive -EncodedCommand $encoded" 2>&1 | tr -d '\r' | grep -v -E "$F"
}

cleanup() {
  ps "Unregister-ScheduledTask -TaskName '$TASK' -Confirm:\$false -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force '$DIR' -ErrorAction SilentlyContinue
Remove-Item -Force \"\$HOME\\windows_files_check.exe\" -ErrorAction SilentlyContinue
\"# cleaned: task=\$([bool](Get-ScheduledTask -TaskName '$TASK' -ErrorAction SilentlyContinue)) dir=\$(Test-Path '$DIR') exe=\$(Test-Path \"\$HOME\\windows_files_check.exe\")\""
}

# 1. Build and copy.
TARGET=x86_64-pc-windows-msvc
SIDECAR="$repo/src-tauri/binaries/rex-$TARGET.exe"
MARK='PLACEHOLDER staged by scripts/windows-check.sh — not a rex binary'
staged=0
trap '[ "$staged" -eq 1 ] && rm -f "$SIDECAR"; cleanup' EXIT
[ -e "$SIDECAR" ] || { printf '%s\n' "$MARK" > "$SIDECAR"; staged=1; }
export PATH="$(brew --prefix llvm)/bin:$(brew --prefix lld)/bin:$PATH"
log="$(mktemp)"
if ! (cd "$repo/src-tauri" && XWIN_ACCEPT_LICENSE=1 CARGO_TARGET_DIR=target/xwin cargo xwin build --example windows_files_check --target "$TARGET") > "$log" 2>&1; then
  echo "BUILD FAILED"; grep -E '^error' -A10 "$log" | head -40; exit 1
fi
[ "$staged" -eq 1 ] && { rm -f "$SIDECAR"; staged=0; }
scp -q "${O[@]}" "$repo/src-tauri/target/xwin/$TARGET/debug/examples/windows_files_check.exe" "$host:windows_files_check.exe" || { echo "copy failed"; exit 1; }
echo "## built and copied"

# 2. Write the files (the SSH user = the owner).
echo "## acl"
ps "& \"\$HOME\\windows_files_check.exe\" acl '$DIR'; \"exit=\$LASTEXITCODE\""

# 3. Read the DACLs back.
echo "## Get-Acl"
ps "foreach (\$n in 'control.txt','born.txt','hardened.txt','rewritten.txt') {
  \$a = Get-Acl (Join-Path '$DIR' \$n)
  \$rules = @(\$a.Access)
  \"\$n protected=\$(\$a.AreAccessRulesProtected) rules=\$(\$rules.Count) owner=\$(\$a.Owner) :: \$((\$rules | ForEach-Object { \"\$(\$_.IdentityReference) \$(\$_.AccessControlType) \$(\$_.FileSystemRights) inherited=\$(\$_.IsInherited)\" }) -join ' | ')\"
}"

# 4. Read them as LOCAL SERVICE.
echo "## read as another account"
ps "\$ErrorActionPreference = 'Stop'
try {
  New-Item -ItemType Directory -Force (Join-Path '$DIR' 'out') | Out-Null
  # LOCAL SERVICE (S-1-5-19) may write its answer here and nowhere else in the fixture.
  icacls (Join-Path '$DIR' 'out') /grant '*S-1-5-19:(OI)(CI)M' | Out-Null
} catch { \"prep failed: \$(\$_.Exception.Message)\" }"
scp -q "${O[@]}" "$here/windows-files-read-as.ps1" "$host:C:/Users/Public/rexenv-acl-check/read-as.ps1" || echo "copy of read-as.ps1 failed"
ps "\$ErrorActionPreference = 'Stop'
try {
  \$action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument \"-NoProfile -NonInteractive -ExecutionPolicy Bypass -File \`\"$DIR\\read-as.ps1\`\" -Dir \`\"$DIR\`\"\"
  \$principal = New-ScheduledTaskPrincipal -UserId 'NT AUTHORITY\\LOCALSERVICE' -LogonType ServiceAccount -RunLevel Limited
  Register-ScheduledTask -TaskName '$TASK' -Action \$action -Principal \$principal -Force | Out-Null
  Start-ScheduledTask -TaskName '$TASK'
  'started'
} catch { \"TASK FAILED: \$(\$_.Exception.Message)\" }"
for i in $(seq 1 30); do
  sleep 3
  state="$(ps "\$i = Get-ScheduledTaskInfo -TaskName '$TASK' -ErrorAction SilentlyContinue; '{0} out={1} result={2}' -f (Get-ScheduledTask -TaskName '$TASK').State, (Test-Path (Join-Path '$DIR' 'out\\read-as.txt')), \$i.LastTaskResult" | tail -1)"
  case "$state" in "Ready out=True"*) break ;; esac
done
echo "# task: $state"
ps "Get-Content (Join-Path '$DIR' 'out\\read-as.txt') -ErrorAction SilentlyContinue"

# 5. Mark of the Web and the image checks.
echo "## motw"
ps "& \"\$HOME\\windows_files_check.exe\" motw (Join-Path \$env:TEMP 'rexenv-motw-check'); \"exit=\$LASTEXITCODE\""

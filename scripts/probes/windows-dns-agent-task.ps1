# windows_dns_agent_task_check in the logged-on DESKTOP session (ledger #616): the DNS agent's logon
# task registered, run, kept alive and removed by the desktop user's own (Medium) token. Run through
# windows-limited-token.sh:
#
#   BUILD_ONLY=1 scripts/probes/windows-example.sh <host> windows_dns_agent_task_check
#   scp src-tauri/target/xwin/x86_64-pc-windows-msvc/debug/examples/windows_dns_agent_task_check.exe <host>:
#   scripts/probes/windows-limited-token.sh <host> scripts/probes/windows-dns-agent-task.ps1
#
# CHANGES the machine while it runs: registers \rexenv\dns-agent and serves 127.0.0.1:53, both removed
# by the check (and its guard). Needs a logged-on user. Owner's go: 15 Sep 2026. The exe is removed at
# the end. ASCII only (PowerShell 5.1).

$ErrorActionPreference = 'Continue'
$exe = Join-Path $HOME 'windows_dns_agent_task_check.exe'
if (-not (Test-Path $exe)) {
  "# no $exe - copy it first (see the header)"
  return
}
& $exe 2>&1 | ForEach-Object { "$_" }
"exit=$LASTEXITCODE"
Start-Sleep -Seconds 2
Remove-Item -Force $exe -ErrorAction SilentlyContinue
"# exe removed: $(-not (Test-Path $exe))"

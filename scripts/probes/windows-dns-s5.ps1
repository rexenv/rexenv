# W6 S5 (plan: DNS done-when): what the REAL app did to DNS on this machine, read only - nothing here
# starts, stops, adds or removes anything. Run it over SSH between the owner's steps:
#
#   scp scripts/probes/windows-dns-s5.ps1 <host>:rexenv-s5/
#   ssh <host> "powershell -NoProfile -ExecutionPolicy Bypass -File rexenv-s5\windows-dns-s5.ps1"
#
# Copied, not -EncodedCommand: encoded, this script is past the Windows command-line limit and nothing
# runs - with no error on the SSH side (measured 15 Sep 2026).
#
# Prints: the app and agent processes, who holds 127.0.0.1:53, the \rexenv\dns-agent task, every NRPT
# rule, a .rex name through Windows' own resolver and straight at 127.0.0.1, the rexenv CA in this
# user's Root store, and the DNS lines of the app, health and agent logs. ASCII only (PowerShell 5.1).
# Over SSH the "user" is the SSH account: run it as the same account that is logged on at the desktop.

$ProgressPreference = 'SilentlyContinue'
$ErrorActionPreference = 'Continue'
$data = Join-Path $env:LOCALAPPDATA 'rexenv\rexenv\data'
$logs = Join-Path $data 'logs'
"# at $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')  boot $((Get-CimInstance Win32_OperatingSystem).LastBootUpTime)"
"# desktop user: $((Get-Process explorer -IncludeUserName -ErrorAction SilentlyContinue | Select-Object -First 1).UserName)"

"## processes"
Get-CimInstance Win32_Process -Filter "Name = 'rexenv.exe'" | ForEach-Object {
  "  pid $($_.ProcessId) started $($_.CreationDate) : $($_.CommandLine)"
}

"## 127.0.0.1:53"
Get-NetUDPEndpoint -LocalPort 53 -ErrorAction SilentlyContinue | ForEach-Object {
  $p = Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue
  "  $($_.LocalAddress):53 pid $($_.OwningProcess) $($p.ProcessName)"
}

"## task \rexenv\dns-agent"
$t = Get-ScheduledTask -TaskPath '\rexenv\' -TaskName 'dns-agent' -ErrorAction SilentlyContinue
if ($t) {
  $i = $t | Get-ScheduledTaskInfo
  "  state $($t.State) last run $($i.LastRunTime) result $($i.LastTaskResult) next $($i.NextRunTime)"
  "  action: $($t.Actions | ForEach-Object { $_.Execute + ' ' + $_.Arguments })"
} else { "  (no task)" }

"## NRPT rules"
$rules = @(Get-DnsClientNrptRule)
if ($rules.Count -eq 0) { "  (none)" }
$rules | ForEach-Object { "  $($_.Name) namespace [$($_.Namespace -join ', ')] servers [$($_.NameServers -join ', ')] comment '$($_.Comment)'" }

"## .rex resolution"
foreach ($name in 's5probe.rex', 'a.b.s5probe.rex') {
  $sys = Resolve-DnsName $name -Type A -DnsOnly -QuickTimeout -ErrorAction SilentlyContinue | Where-Object Type -eq 'A' | Select-Object -First 1
  $direct = Resolve-DnsName $name -Type A -Server 127.0.0.1 -DnsOnly -QuickTimeout -ErrorAction SilentlyContinue | Where-Object Type -eq 'A' | Select-Object -First 1
  "  $name  windows resolver: $(if ($sys) { $sys.IPAddress } else { 'no answer' })  127.0.0.1:53: $(if ($direct) { $direct.IPAddress } else { 'no answer' })"
}
$build = Resolve-DnsName '_build.rexenv-agent.rex' -Type TXT -Server 127.0.0.1 -DnsOnly -QuickTimeout -ErrorAction SilentlyContinue | Where-Object Type -eq 'TXT' | Select-Object -First 1
"  build identity: $(if ($build) { $build.Strings -join ' ' } else { 'no answer' })"

"## rexenv CA in CurrentUser Root"
$ca = @(Get-ChildItem Cert:\CurrentUser\Root | Where-Object { $_.Subject -match 'rexenv' })
if ($ca.Count -eq 0) { "  (none)" }
$ca | ForEach-Object { "  $($_.Thumbprint) $($_.Subject) until $($_.NotAfter)" }

"## logs ($logs)"
foreach ($f in 'rexenv.log', 'health.log', 'dns-agent.log') {
  $p = Join-Path $logs $f
  if (-not (Test-Path -LiteralPath $p)) { "  -- ${f}: (none)"; continue }
  "  -- $f ($((Get-Item -LiteralPath $p).Length) bytes)"
  $lines = Get-Content -LiteralPath $p -ErrorAction SilentlyContinue
  if ($f -eq 'rexenv.log') { $lines = $lines | Select-String -Pattern 'dns|resolver|panick|unported' | ForEach-Object { $_.Line } }
  $lines | Select-Object -Last 25 | ForEach-Object { "    $_" }
}

# W6 S2 measurement (plan section 5): can the desktop user, WITHOUT elevation, register a logon
# Scheduled Task for itself shaped like the DNS agent's - and does Task Scheduler keep its settings,
# run it, restart it when its process is killed, end it and delete it? Run through
# windows-limited-token.sh (Medium token, the logged-on session):
#
#   scripts/probes/windows-limited-token.sh <host> scripts/probes/windows-logon-task.ps1
#
# Creates ONE task, \rexenv\dns-agent-probe, whose action is a hidden PowerShell that sleeps; deletes it
# in a finally block. Owner's go: 15 Sep 2026. ASCII only (PowerShell 5.1).

$ErrorActionPreference = 'Continue'
$ProgressPreference = 'SilentlyContinue'
$name = '\rexenv\dns-agent-probe'
$marker = 'rexenv-dns-agent-probe'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$xmlPath = Join-Path $here 'task.xml'

function Probe-Pids {
  @(Get-CimInstance Win32_Process -Filter "Name='powershell.exe'" | Where-Object { $_.CommandLine -like "*$marker*" } | ForEach-Object { $_.ProcessId })
}
function Task-State {
  $o = schtasks /Query /TN $name /V /FO LIST 2>&1 | Out-String
  (($o -split "`r?`n") | Where-Object { $_ -match '^(Status|Last Result|Last Run Time):' }) -join ' | '
}

$sid = ([Security.Principal.WindowsIdentity]::GetCurrent()).User.Value
$user = ([Security.Principal.WindowsIdentity]::GetCurrent()).Name
"user $user sid $sid"
$action = "-NoProfile -NonInteractive -WindowStyle Hidden -Command Start-Sleep -Seconds 900; '$marker'"

$xml = @"
<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>rexenv DNS agent probe</Description></RegistrationInfo>
  <Triggers>
    <LogonTrigger><Enabled>true</Enabled><UserId>$sid</UserId></LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author"><UserId>$sid</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <IdleSettings><StopOnIdleEnd>false</StopOnIdleEnd><RestartOnIdle>false</RestartOnIdle></IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>true</Hidden>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
    <RestartOnFailure><Interval>PT1M</Interval><Count>999</Count></RestartOnFailure>
  </Settings>
  <Actions Context="Author">
    <Exec><Command>powershell.exe</Command><Arguments>$action</Arguments></Exec>
  </Actions>
</Task>
"@
$xml | Out-File -Encoding Unicode $xmlPath

try {
  '--- create (schtasks /Create /XML, no elevation) ---'
  $o = schtasks /Create /TN $name /XML $xmlPath 2>&1 | Out-String
  "exit=$LASTEXITCODE $($o.Trim())"
  if ($LASTEXITCODE -ne 0) {
    '--- create at the root folder instead ---'
    $name = '\rexenv-dns-agent-probe'
    $o = schtasks /Create /TN $name /XML $xmlPath 2>&1 | Out-String
    "exit=$LASTEXITCODE $($o.Trim())"
  }
  '--- what Task Scheduler kept ---'
  $back = schtasks /Query /TN $name /XML 2>&1 | Out-String
  foreach ($tag in 'LogonType', 'RunLevel', 'UserId', 'Hidden', 'ExecutionTimeLimit', 'DisallowStartIfOnBatteries', 'StopIfGoingOnBatteries', 'MultipleInstancesPolicy', 'Interval', 'Count') {
    $m = [regex]::Matches($back, "<$tag>([^<]*)</$tag>") | ForEach-Object { $_.Groups[1].Value }
    "$tag = $($m -join ',')"
  }
  "state: $(Task-State)"

  '--- run ---'
  $o = schtasks /Run /TN $name 2>&1 | Out-String
  "exit=$LASTEXITCODE $($o.Trim())"
  Start-Sleep -Seconds 5
  $pids = Probe-Pids
  "running pids: $($pids -join ',')  state: $(Task-State)"

  '--- kill the process; does Task Scheduler restart it? (wait up to 150 s) ---'
  foreach ($p in $pids) { Stop-Process -Id $p -Force }
  $killedAt = Get-Date
  Start-Sleep -Seconds 3
  "after kill: pids $((Probe-Pids) -join ',')  state: $(Task-State)"
  $restarted = $false
  while (((Get-Date) - $killedAt).TotalSeconds -lt 150) {
    $now = Probe-Pids
    if ($now.Count -gt 0) { $restarted = $true; "restarted after $([int]((Get-Date) - $killedAt).TotalSeconds) s: pids $($now -join ',')"; break }
    Start-Sleep -Seconds 5
  }
  if (-not $restarted) { "not restarted within 150 s  state: $(Task-State)" }

  '--- a second /Run while one runs (IgnoreNew) ---'
  if (-not $restarted) { schtasks /Run /TN $name 2>&1 | Out-Null; Start-Sleep -Seconds 5 }
  $before = (Probe-Pids).Count
  $o = schtasks /Run /TN $name 2>&1 | Out-String
  Start-Sleep -Seconds 5
  "instances before $before, after a second run $((Probe-Pids).Count)  ($($o.Trim()))"

  '--- end ---'
  $o = schtasks /End /TN $name 2>&1 | Out-String
  "exit=$LASTEXITCODE $($o.Trim())"
  Start-Sleep -Seconds 3
  "after end: pids $((Probe-Pids) -join ',')  state: $(Task-State)"
} finally {
  '--- delete ---'
  $o = schtasks /Delete /TN $name /F 2>&1 | Out-String
  "exit=$LASTEXITCODE $($o.Trim())"
  foreach ($p in Probe-Pids) { Stop-Process -Id $p -Force -ErrorAction SilentlyContinue }
  schtasks /Query /TN $name 2>&1 | Out-Null
  "task still present: $($LASTEXITCODE -eq 0)"
  $folder = schtasks /Query /TN '\rexenv\' 2>&1 | Out-String
  "rexenv folder query: $(($folder -split "`r?`n" | Select-Object -First 2) -join ' ')"
  Remove-Item -Force $xmlPath -ErrorAction SilentlyContinue
}

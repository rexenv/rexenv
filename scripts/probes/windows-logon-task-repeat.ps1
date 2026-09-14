# W6 S2 measurement, second part (plan section 5): the owner ruled a time trigger repeating every
# minute as the DNS agent's keep-alive, because RestartOnFailure did not restart a killed action.
# Does it work - without a manual /Run, does the trigger start the task; after the action is killed,
# is it back within about a minute; and does IgnoreNew keep it to one instance? Run through
# windows-limited-token.sh (Medium token, the logged-on session):
#
#   scripts/probes/windows-limited-token.sh <host> scripts/probes/windows-logon-task-repeat.ps1
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
  (($o -split "`r?`n") | Where-Object { $_ -match '^(Status|Last Result|Last Run Time|Next Run Time):' } | ForEach-Object { $_ -replace '\s{2,}', ' ' }) -join ' | '
}
function Wait-For([scriptblock]$cond, [int]$seconds, [string]$label) {
  $t0 = Get-Date
  while (((Get-Date) - $t0).TotalSeconds -lt $seconds) {
    if (& $cond) { return "$label after $([int]((Get-Date) - $t0).TotalSeconds) s" }
    Start-Sleep -Seconds 5
  }
  "NOT $label within $seconds s"
}

$sid = ([Security.Principal.WindowsIdentity]::GetCurrent()).User.Value
$action = "-NoProfile -NonInteractive -WindowStyle Hidden -Command Start-Sleep -Seconds 900; '$marker'"
$start = (Get-Date).AddMinutes(-5).ToString('yyyy-MM-ddTHH:mm:ss')

$xml = @"
<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>rexenv DNS agent probe</Description></RegistrationInfo>
  <Triggers>
    <LogonTrigger><Enabled>true</Enabled><UserId>$sid</UserId></LogonTrigger>
    <TimeTrigger>
      <Repetition><Interval>PT1M</Interval><StopAtDurationEnd>false</StopAtDurationEnd></Repetition>
      <StartBoundary>$start</StartBoundary>
      <Enabled>true</Enabled>
    </TimeTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author"><UserId>$sid</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <IdleSettings><StopOnIdleEnd>false</StopOnIdleEnd><RestartOnIdle>false</RestartOnIdle></IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>true</Hidden>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec><Command>powershell.exe</Command><Arguments>$action</Arguments></Exec>
  </Actions>
</Task>
"@
$xml | Out-File -Encoding Unicode $xmlPath

try {
  '--- create (no elevation), no manual run ---'
  $o = schtasks /Create /TN $name /XML $xmlPath 2>&1 | Out-String
  "exit=$LASTEXITCODE $($o.Trim())"
  if ($LASTEXITCODE -ne 0) { return }
  $back = schtasks /Query /TN $name /XML 2>&1 | Out-String
  foreach ($tag in 'Interval', 'StopAtDurationEnd', 'StartBoundary', 'StartWhenAvailable', 'MultipleInstancesPolicy') {
    $m = [regex]::Matches($back, "<$tag>([^<]*)</$tag>") | ForEach-Object { $_.Groups[1].Value }
    "$tag = $($m -join ',')"
  }
  "state: $(Task-State)"

  '--- does the trigger start it on its own? (up to 75 s) ---'
  Wait-For { (Probe-Pids).Count -gt 0 } 75 'started by the trigger'
  "pids: $((Probe-Pids) -join ',')  state: $(Task-State)"

  '--- one instance across another trigger tick (IgnoreNew) ---'
  Start-Sleep -Seconds 65
  "instances after ~1 tick: $((Probe-Pids).Count)  state: $(Task-State)"

  '--- kill it; is it back? (up to 75 s) ---'
  foreach ($p in Probe-Pids) { Stop-Process -Id $p -Force }
  Start-Sleep -Seconds 2
  "after kill: pids $((Probe-Pids) -join ',')"
  Wait-For { (Probe-Pids).Count -gt 0 } 75 'back after the kill'
  "pids: $((Probe-Pids) -join ',')  state: $(Task-State)"

  '--- /End: does the next tick bring it back? (up to 75 s) ---'
  $o = schtasks /End /TN $name 2>&1 | Out-String
  "end exit=$LASTEXITCODE $($o.Trim())"
  Start-Sleep -Seconds 2
  "after end: pids $((Probe-Pids) -join ',')"
  Wait-For { (Probe-Pids).Count -gt 0 } 75 'back after /End'
} finally {
  '--- delete ---'
  $o = schtasks /Delete /TN $name /F 2>&1 | Out-String
  "exit=$LASTEXITCODE $($o.Trim())"
  Start-Sleep -Seconds 2
  foreach ($p in Probe-Pids) { Stop-Process -Id $p -Force -ErrorAction SilentlyContinue }
  schtasks /Query /TN $name 2>&1 | Out-Null
  "task still present: $($LASTEXITCODE -eq 0)"
  "probe processes left: $((Probe-Pids).Count)"
  Remove-Item -Force $xmlPath -ErrorAction SilentlyContinue
}

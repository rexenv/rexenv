# W6 S4 measurement (plan section 5): what an NRPT rule for `.rex` does on Windows before rexenv writes
# one - where it is stored, what Get-DnsClientNrptRule reports (the ownership signature), whether Windows'
# resolver sends `probe.rex` and `a.b.probe.rex` to 127.0.0.1:53 at once or only after a cache flush,
# which APIs honour it, and what removing it leaves. Run over SSH (the elevated token - adding a rule
# needs an administrator), with the agent exe copied to $HOME first:
#
#   scp src-tauri/target/xwin/x86_64-pc-windows-msvc/debug/examples/windows_dns_agent_task_check.exe <host>:
#   (then send this script with -EncodedCommand)
#
# CHANGES the machine for about a minute: adds ONE NRPT rule (namespace .rex, comment "rexenv-probe")
# and runs the resolver on 127.0.0.1:53; both removed in a finally block. Owner's go: 15 Sep 2026.
# ASCII only (PowerShell 5.1).

$ErrorActionPreference = 'Continue'
$ProgressPreference = 'SilentlyContinue'
$exe = Join-Path $HOME 'windows_dns_agent_task_check.exe'
$policy = 'HKLM:\SYSTEM\CurrentControlSet\Services\Dnscache\Parameters\DnsPolicyConfig'

function Rules([string]$when) {
  "--- NRPT rules, $when ---"
  $all = @(Get-DnsClientNrptRule)
  if ($all.Count -eq 0) { 'none' }
  foreach ($r in $all) {
    "name=$($r.Name) namespace=$($r.Namespace -join ',') servers=$($r.NameServers -join ',') comment=$($r.Comment) display=$($r.DisplayName) version=$($r.Version)"
  }
}
function Ask([string]$name) {
  $t = [Diagnostics.Stopwatch]::StartNew()
  try {
    $a = Resolve-DnsName $name -Type A -QuickTimeout -ErrorAction Stop | Where-Object Type -eq 'A'
    "  Resolve-DnsName $name -> $(($a.IPAddress) -join ',') ($($t.ElapsedMilliseconds) ms)"
  } catch {
    "  Resolve-DnsName $name -> ERR $($_.Exception.Message) ($($t.ElapsedMilliseconds) ms)"
  }
  try {
    $b = [System.Net.Dns]::GetHostAddresses($name) | ForEach-Object { $_.IPAddressToString }
    "  .NET GetHostAddresses $name -> $($b -join ',')"
  } catch {
    "  .NET GetHostAddresses $name -> ERR $($_.Exception.InnerException.Message)"
  }
}

$agent = $null
$rule = $null
try {
  Rules 'before'
  "policy key exists: $(Test-Path $policy)"
  if (Test-Path $policy) { Get-ChildItem $policy | ForEach-Object { "  key $($_.PSChildName)" } }
  "effective policy before: $((Get-DnsClientNrptPolicy | ForEach-Object { $_.Namespace }) -join ',')"

  if (-not (Test-Path $exe)) { "# no $exe - copy it first"; return }
  $agent = Start-Process -FilePath $exe -ArgumentList '--dns-agent' -WindowStyle Hidden -PassThru
  Start-Sleep -Seconds 2
  try {
    $direct = (Resolve-DnsName check.rex -Server 127.0.0.1 -DnsOnly -QuickTimeout -Type A -ErrorAction Stop | Where-Object Type -eq 'A').IPAddress
  } catch { $direct = "ERR $($_.Exception.Message)" }
  "agent pid $($agent.Id) answers directly: $direct"

  '--- system resolver BEFORE the rule ---'
  Ask 'probe.rex'

  '--- add the rule ---'
  $rule = Add-DnsClientNrptRule -Namespace '.rex' -NameServers '127.0.0.1' -Comment 'rexenv-probe' -DisplayName 'rexenv-probe .rex' -PassThru
  "added: name=$($rule.Name) namespace=$($rule.Namespace -join ',') servers=$($rule.NameServers -join ',')"
  Rules 'after add'
  if (Test-Path $policy) {
    Get-ChildItem $policy | ForEach-Object {
      $p = Get-ItemProperty $_.PSPath
      "  key $($_.PSChildName): " + (($p.PSObject.Properties | Where-Object { $_.Name -notlike 'PS*' } | ForEach-Object { "$($_.Name)=$($_.Value -join ',')" }) -join '; ')
    }
  }
  "effective policy after: $((Get-DnsClientNrptPolicy | ForEach-Object { "$($_.Namespace)->$($_.NameServers -join ',')" }) -join ' | ')"

  '--- system resolver AT ONCE after the rule ---'
  Ask 'probe.rex'
  Ask 'a.b.probe.rex'
  Ask 'probe.test'
  '--- after Clear-DnsClientCache ---'
  Clear-DnsClientCache
  Ask 'probe.rex'
  '--- ping (getaddrinfo) ---'
  ((ping -n 1 -w 500 sub.probe.rex 2>&1 | Out-String) -split "`r?`n" | Select-Object -First 2) -join ' / '
} finally {
  '--- remove the rule ---'
  if ($rule) {
    Remove-DnsClientNrptRule -Name $rule.Name -Force
    "removed exit ok: $?"
  }
  Rules 'after remove'
  Clear-DnsClientCache
  '--- system resolver AFTER removal ---'
  Ask 'probe.rex'
  if ($agent) { Stop-Process -Id $agent.Id -Force -ErrorAction SilentlyContinue; "agent stopped" }
  "policy key exists after: $(Test-Path $policy)"
  if (Test-Path $policy) { Get-ChildItem $policy | ForEach-Object { "  key $($_.PSChildName)" } }
  Remove-Item -Force $exe -ErrorAction SilentlyContinue
  "exe removed: $(-not (Test-Path $exe))"
}

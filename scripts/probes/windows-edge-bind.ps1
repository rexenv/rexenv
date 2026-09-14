# W5: can the DESKTOP user's filtered token run the edge on :443/:80, and does binding every
# interface raise Windows Defender Firewall's prompt? (docs/PLAN-windows-port.md §5 W5)
#
#   scripts/probes/windows-limited-token.sh <user@host> scripts/probes/windows-edge-bind.ps1
#
# Run through windows-limited-token.sh so it has the logged-on user's Medium token, in their
# desktop session - the token and session rexenv itself runs with. An SSH session's token is
# elevated and has no desktop, so neither answer can come from there.
#
# For each bind - all interfaces (Caddy's default, what rexenv's Caddyfile does today) and
# 127.0.0.1 only (`default_bind`) - it starts the pinned caddy.exe from rexenv's binary cache
# with a minimal config (admin off, plain HTTP on :443 and :80: the BIND is the question, not
# TLS), then records: did :443 and :80 answer, what netstat lists, and whether a window titled
# like the firewall's security alert appeared in this session. Every caddy it starts is stopped.
# Nothing is written outside %TEMP%\rexenv-edge-bind, which is removed.
$ErrorActionPreference = 'Continue'
$work = Join-Path $env:TEMP 'rexenv-edge-bind'
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $work | Out-Null

$caddy = Get-ChildItem (Join-Path $env:LOCALAPPDATA 'rexenv') -Recurse -Filter caddy.exe -ErrorAction SilentlyContinue |
  Select-Object -First 1 -ExpandProperty FullName
"caddy: $caddy"
if (-not $caddy) { 'NO caddy.exe in the binary cache - run windows_edge_probe first'; exit 1 }

function Test-Answers([int]$port) {
  try {
    $c = New-Object Net.Sockets.TcpClient
    $ok = $c.ConnectAsync('127.0.0.1', $port).Wait(1500)
    $c.Close()
    return $ok
  } catch { return $false }
}

function Get-AlertWindows {
  Get-Process -ErrorAction SilentlyContinue |
    Where-Object { $_.MainWindowTitle -match 'Security Alert|Firewall|allow' } |
    ForEach-Object { "$($_.ProcessName) pid=$($_.Id) title='$($_.MainWindowTitle)'" }
}

"before: alert windows = $(@(Get-AlertWindows).Count); :443 answers $(Test-Answers 443); :80 answers $(Test-Answers 80)"

foreach ($case in @(
    @{ label = 'all interfaces (today)'; bind = '' },
    @{ label = '127.0.0.1 only';         bind = "`tdefault_bind 127.0.0.1`n" }
  )) {
  "## $($case.label)"
  $conf = Join-Path $work 'Caddyfile'
  $body = "{`n`tadmin off`n`tauto_https off`n$($case.bind)}`n:443 {`n`trespond `"edge-bind-443`"`n}`n:80 {`n`trespond `"edge-bind-80`"`n}`n"
  Set-Content -Path $conf -Value $body -Encoding ascii
  $log = Join-Path $work 'caddy.log'
  $p = Start-Process -FilePath $caddy -ArgumentList @('run', '--config', "`"$conf`"", '--adapter', 'caddyfile') `
        -RedirectStandardError $log -WindowStyle Hidden -PassThru
  Start-Sleep -Seconds 6
  "  caddy alive: $(-not $p.HasExited)"
  "  :443 answers: $(Test-Answers 443)   :80 answers: $(Test-Answers 80)"
  netstat -ano -p TCP | Select-String -Pattern ':443 |:80 ' | Select-String LISTENING | ForEach-Object { "  netstat: $($_.Line.Trim())" }
  $alerts = @(Get-AlertWindows)
  "  alert windows now: $($alerts.Count)"
  $alerts | ForEach-Object { "    $_" }
  if ($p.HasExited) {
    "  caddy exited with $($p.ExitCode); its log tail:"
    Get-Content $log -Tail 6 -ErrorAction SilentlyContinue | ForEach-Object { "    $_" }
  } else {
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 2
  }
  "  after stop: :443 answers $(Test-Answers 443)"
}

Get-Process caddy -ErrorAction SilentlyContinue | ForEach-Object { "LEFTOVER caddy pid=$($_.Id) - stopping"; Stop-Process -Id $_.Id -Force }
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
"cleaned: $(-not (Test-Path $work))"

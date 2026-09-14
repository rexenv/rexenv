# D2 (plan section 3): who holds port 53, and who answers it, while Windows' Mobile hotspot is ON.
# Run in the logged-on desktop session through windows-limited-token.sh:
#
#   scripts/probes/windows-limited-token.sh <host> scripts/probes/windows-hotspot-dns53.ps1
#
# It CHANGES the machine for about a minute: Mobile hotspot is turned on (the Internet connection is
# shared over Wi-Fi), measured, and turned off again in a finally block. The Dell has one Wi-Fi
# adapter, so the SSH link may drop while the hotspot is up; the script runs on the machine and writes
# its output to a file, and the polling side retries. If the hotspot was already on, it is left on.
# Owner's go: 14 Sep 2026. ASCII only (PowerShell 5.1).

$ErrorActionPreference = 'Continue'
$ProgressPreference = 'SilentlyContinue'

function Show-Port53([string]$when) {
  "--- port 53, $when ---"
  $rows = @(Get-NetUDPEndpoint -LocalPort 53 -ErrorAction SilentlyContinue | ForEach-Object { [pscustomobject]@{ P = 'udp'; A = $_.LocalAddress; Id = $_.OwningProcess } })
  $rows += @(Get-NetTCPConnection -LocalPort 53 -State Listen -ErrorAction SilentlyContinue | ForEach-Object { [pscustomobject]@{ P = 'tcp'; A = $_.LocalAddress; Id = $_.OwningProcess } })
  if ($rows.Count -eq 0) { 'nobody'; return }
  foreach ($r in $rows) {
    $name = (Get-Process -Id $r.Id -ErrorAction SilentlyContinue).ProcessName
    $svc = (Get-CimInstance Win32_Service -Filter "ProcessId=$($r.Id)" -ErrorAction SilentlyContinue | ForEach-Object Name) -join ','
    "{0} {1}:53 pid {2} {3} services [{4}]" -f $r.P, $r.A, $r.Id, $name, $svc
  }
}

function Ask([string]$server, [string]$name, [switch]$Tcp) {
  try {
    $a = Resolve-DnsName $name -Server $server -DnsOnly -QuickTimeout -Type A -TcpOnly:$Tcp -ErrorAction Stop | Where-Object Type -eq 'A'
    "resolve $name via $server $(if ($Tcp) { 'tcp' } else { 'udp' }): $(($a.IPAddress) -join ',')"
  } catch {
    "resolve $name via $server $(if ($Tcp) { 'tcp' } else { 'udp' }): ERR $($_.Exception.Message)"
  }
}

# Can the agent's shape still bind: 127.0.0.1:53, exclusive, UDP and TCP?
function Try-AgentBind {
  foreach ($proto in 'udp', 'tcp') {
    try {
      if ($proto -eq 'udp') {
        $s = New-Object System.Net.Sockets.Socket([System.Net.Sockets.AddressFamily]::InterNetwork, [System.Net.Sockets.SocketType]::Dgram, [System.Net.Sockets.ProtocolType]::Udp)
      } else {
        $s = New-Object System.Net.Sockets.Socket([System.Net.Sockets.AddressFamily]::InterNetwork, [System.Net.Sockets.SocketType]::Stream, [System.Net.Sockets.ProtocolType]::Tcp)
      }
      $s.ExclusiveAddressUse = $true
      $s.Bind((New-Object System.Net.IPEndPoint([System.Net.IPAddress]::Parse('127.0.0.1'), 53)))
      if ($proto -eq 'tcp') { $s.Listen(4) }
      "agent-shaped bind 127.0.0.1:53 $proto exclusive: ok"
      $s.Close()
    } catch {
      "agent-shaped bind 127.0.0.1:53 $proto exclusive: FAILED $($_.Exception.InnerException.SocketErrorCode) $($_.Exception.Message)"
    }
  }
}

Add-Type -AssemblyName System.Runtime.WindowsRuntime
$asTask = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
  $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1'
})[0]
function Await($op, [Type]$type) {
  $t = $asTask.MakeGenericMethod($type).Invoke($null, @($op))
  $null = $t.Wait(60000)
  $t.Result
}

[Windows.Networking.Connectivity.NetworkInformation, Windows.Networking.Connectivity, ContentType = WindowsRuntime] | Out-Null
[Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager, Windows.Networking.NetworkOperators, ContentType = WindowsRuntime] | Out-Null
[Windows.Networking.NetworkOperators.NetworkOperatorTetheringOperationResult, Windows.Networking.NetworkOperators, ContentType = WindowsRuntime] | Out-Null

Show-Port53 'before'
$netProfile = [Windows.Networking.Connectivity.NetworkInformation]::GetInternetConnectionProfile()
"internet profile: $($netProfile.ProfileName)"
$tm = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager]::CreateFromConnectionProfile($netProfile)
$wasOn = ($tm.TetheringOperationalState -eq 'On')
"tethering state before: $($tm.TetheringOperationalState)"

try {
  if (-not $wasOn) {
    $r = Await ($tm.StartTetheringAsync()) ([Windows.Networking.NetworkOperators.NetworkOperatorTetheringOperationResult])
    "start: $($r.Status) $($r.AdditionalErrorMessage)"
  }
  "tethering state: $($tm.TetheringOperationalState)"
  # The ICS DNS proxy may bind a moment after the hotspot comes up: wait up to 20 s for a :53 row.
  $deadline = (Get-Date).AddSeconds(20)
  while ((Get-Date) -lt $deadline) {
    if (Get-NetUDPEndpoint -LocalPort 53 -ErrorAction SilentlyContinue) { break }
    Start-Sleep -Milliseconds 500
  }
  Start-Sleep -Seconds 2
  Show-Port53 'hotspot on'
  Try-AgentBind
  '--- hotspot addresses ---'
  Get-NetIPAddress -AddressFamily IPv4 -ErrorAction SilentlyContinue | Where-Object { $_.InterfaceAlias -like 'Local Area Connection*' -or $_.IPAddress -like '192.168.137.*' } | ForEach-Object { "$($_.InterfaceAlias) $($_.IPAddress)" }
  '--- who answers ---'
  Ask '127.0.0.1' 'probe.rex'
  Ask '127.0.0.1' 'probe.rex' -Tcp
  Ask '127.0.0.1' 'www.microsoft.com'
  $ics = (Get-NetIPAddress -AddressFamily IPv4 -ErrorAction SilentlyContinue | Where-Object IPAddress -like '192.168.137.*' | Select-Object -First 1).IPAddress
  if ($ics) {
    Ask $ics 'www.microsoft.com'
    Ask $ics 'www.microsoft.com' -Tcp
  } else { 'no 192.168.137.x address' }
} finally {
  if (-not $wasOn) {
    $s = Await ($tm.StopTetheringAsync()) ([Windows.Networking.NetworkOperators.NetworkOperatorTetheringOperationResult])
    "stop: $($s.Status) $($s.AdditionalErrorMessage)"
  }
  Start-Sleep -Seconds 3
  "tethering state after: $($tm.TetheringOperationalState)"
  Show-Port53 'after'
  Try-AgentBind
}

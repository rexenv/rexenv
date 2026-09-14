# D2 (plan section 3): who holds port 53, and who answers it, while a WSL 2 distribution RUNS (NAT
# networking), and after WSL shuts down. Run over SSH:
#
#   REMOTE_SCRIPT=... scripts/probes/windows-probe.sh <host> scripts/probes/windows-wsl-dns53.ps1
#   (or send it with -EncodedCommand)
#
# It starts the `Ubuntu` distribution (installed at the owner's go, 14 Sep 2026) with a sleeping shell
# as root, measures, then runs `wsl --shutdown`. Nothing is installed or configured by this script.
# ASCII only (PowerShell 5.1).

$ErrorActionPreference = 'Continue'
$ProgressPreference = 'SilentlyContinue'
$wsl = Join-Path $env:ProgramFiles 'WSL\wsl.exe'

function Clean([string]$s) { ($s -replace "`0", '').Trim() }

function Show-Port53([string]$when) {
  "--- port 53, $when ---"
  $rows = @(Get-NetUDPEndpoint -LocalPort 53 -ErrorAction SilentlyContinue | ForEach-Object { [pscustomobject]@{ P = 'udp'; A = $_.LocalAddress; Id = $_.OwningProcess } })
  $rows += @(Get-NetTCPConnection -LocalPort 53 -State Listen -ErrorAction SilentlyContinue | ForEach-Object { [pscustomobject]@{ P = 'tcp'; A = $_.LocalAddress; Id = $_.OwningProcess } })
  if ($rows.Count -eq 0) { 'nobody' }
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

'--- wsl ---'
Clean (& $wsl -l -v 2>&1 | Out-String)
Show-Port53 'before WSL starts'
Try-AgentBind

$runner = Start-Process -FilePath $wsl -ArgumentList '-d', 'Ubuntu', '-u', 'root', '--', 'sleep', '120' -WindowStyle Hidden -PassThru
Start-Sleep -Seconds 15
'--- wsl running ---'
Clean (& $wsl -l -v 2>&1 | Out-String)
"resolv.conf inside: $(Clean (& $wsl -d Ubuntu -u root -- cat /etc/resolv.conf 2>&1 | Out-String))"
'--- WSL adapter ---'
Get-NetIPAddress -AddressFamily IPv4 -ErrorAction SilentlyContinue | Where-Object InterfaceAlias -like 'vEthernet*' | ForEach-Object { "$($_.InterfaceAlias) $($_.IPAddress)/$($_.PrefixLength)" }
Show-Port53 'WSL running'
Try-AgentBind
'--- who answers ---'
Ask '127.0.0.1' 'probe.rex'
Ask '127.0.0.1' 'probe.rex' -Tcp
$v = (Get-NetIPAddress -AddressFamily IPv4 -ErrorAction SilentlyContinue | Where-Object InterfaceAlias -like 'vEthernet (WSL*' | Select-Object -First 1).IPAddress
if ($v) {
  Ask $v 'www.microsoft.com'
  Ask $v 'www.microsoft.com' -Tcp
} else { 'no vEthernet (WSL) address' }
"name lookup from inside WSL: $(Clean (& $wsl -d Ubuntu -u root -- getent hosts www.microsoft.com 2>&1 | Out-String))"

'--- shutdown ---'
Clean (& $wsl --shutdown 2>&1 | Out-String)
if ($runner -and -not $runner.HasExited) { Stop-Process -Id $runner.Id -Force -ErrorAction SilentlyContinue }
Start-Sleep -Seconds 5
Clean (& $wsl -l -v 2>&1 | Out-String)
Show-Port53 'after wsl --shutdown'
Try-AgentBind

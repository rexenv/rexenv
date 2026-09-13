# The Windows socket bind matrix — docs/PLAN-windows-port.md §6, measured before
# core/ports::ensure_free is written for Windows.
#
#   scripts/probes/windows-probe.sh dell@192.168.0.107 scripts/probes/windows-bind-matrix.ps1 > matrix.csv
#
# # Why this exists
#
# ensure_free decides "is this port free?" with a TRIAL BIND on 127.0.0.1 (what Rust's
# TcpListener::bind does, default options). On macOS a shadow bind already fooled a
# listen check once (Herd on 127.0.0.1:443). Windows adds SO_REUSEADDR, which php-cgi's
# fcgi_listen sets, and SO_EXCLUSIVEADDRUSE, and its rules for a specific address beside
# a wildcard differ from BSD's. So the rule the port relies on is measured, not assumed.
#
# First run, 13 Sep 2026, the Dell (Windows 10 22H2, elevated SSH token, one user): the
# trial bind reports FREE under a 0.0.0.0 or [::] holder and then receives its 127.0.0.1
# traffic; TCP and UDP identical. The table and its consequences are in the plan, §6.
#
# # What it will and will not touch
#
# Opens and closes sockets on loopback only (the wildcard binds included — nothing is
# sent to another host), ports 47001-47288, one pair at a time; changes no setting and
# writes no file. Output: CSV on stdout.
#
#   proto,A,B,b_bind,answers
#   A, B     = <address>-<option>: v4any 0.0.0.0, v4lo 127.0.0.1, v6dual [::] dual-stack,
#              v6only [::] IPv6-only; def (no option), reuse (SO_REUSEADDR),
#              excl (SO_EXCLUSIVEADDRUSE). A binds (and listens) first, then B.
#   b_bind   = OK, or FAIL:<SocketError> (AddressAlreadyInUse, AccessDenied, ...)
#   answers  = when both bound: which socket a connection / datagram to 127.0.0.1
#              reached — A, B, both, or neither.
#
# # What it does not measure
#
# A holder running as another account (a service as LocalService or SYSTEM), a
# non-elevated desktop token, and Windows 11 — run it again under those before relying
# on them.

$ProgressPreference = 'SilentlyContinue'
$ErrorActionPreference = 'Stop'

$addrs = [ordered]@{
  'v4any'  = @{ ip = [Net.IPAddress]::Any;      fam = [Net.Sockets.AddressFamily]::InterNetwork;   dual = $false }
  'v4lo'   = @{ ip = [Net.IPAddress]::Loopback; fam = [Net.Sockets.AddressFamily]::InterNetwork;   dual = $false }
  'v6dual' = @{ ip = [Net.IPAddress]::IPv6Any;  fam = [Net.Sockets.AddressFamily]::InterNetworkV6; dual = $true }
  'v6only' = @{ ip = [Net.IPAddress]::IPv6Any;  fam = [Net.Sockets.AddressFamily]::InterNetworkV6; dual = $false }
}
$opts = 'def', 'reuse', 'excl'

function New-Socket($proto, $addr, $opt) {
  if ($proto -eq 'tcp') {
    $s = New-Object Net.Sockets.Socket($addr.fam, [Net.Sockets.SocketType]::Stream, [Net.Sockets.ProtocolType]::Tcp)
  } else {
    $s = New-Object Net.Sockets.Socket($addr.fam, [Net.Sockets.SocketType]::Dgram, [Net.Sockets.ProtocolType]::Udp)
  }
  if ($addr.fam -eq [Net.Sockets.AddressFamily]::InterNetworkV6) { $s.DualMode = $addr.dual }
  if ($opt -eq 'reuse') { $s.SetSocketOption([Net.Sockets.SocketOptionLevel]::Socket, [Net.Sockets.SocketOptionName]::ReuseAddress, $true) }
  if ($opt -eq 'excl') { $s.ExclusiveAddressUse = $true }
  return $s
}

function Get-SocketErrorCode($err) {
  $code = $err.Exception.InnerException.SocketErrorCode
  if (-not $code) { $code = $err.Exception.SocketErrorCode }
  return $code
}

$port = 47000
"proto,A,B,b_bind,answers"
foreach ($proto in 'tcp', 'udp') {
  foreach ($an in $addrs.Keys) { foreach ($ao in $opts) {
    foreach ($bn in $addrs.Keys) { foreach ($bo in $opts) {
      $port++
      $A = $null; $B = $null; $bres = ''; $who = ''
      try {
        $A = New-Socket $proto $addrs[$an] $ao
        $A.Bind((New-Object Net.IPEndPoint($addrs[$an].ip, $port)))
        if ($proto -eq 'tcp') { $A.Listen(8) }
      } catch {
        "$proto,$an-$ao,$bn-$bo,A_FAILED:$(Get-SocketErrorCode $_),"
        if ($A) { $A.Close() }
        continue
      }
      try {
        $B = New-Socket $proto $addrs[$bn] $bo
        $B.Bind((New-Object Net.IPEndPoint($addrs[$bn].ip, $port)))
        if ($proto -eq 'tcp') { $B.Listen(8) }
        $bres = 'OK'
      } catch {
        $bres = "FAIL:$(Get-SocketErrorCode $_)"
      }
      if ($bres -eq 'OK') {
        try {
          if ($proto -eq 'tcp') {
            $c = New-Object Net.Sockets.TcpClient
            $c.Connect([Net.IPAddress]::Loopback, $port)
            Start-Sleep -Milliseconds 30
            $ra = $A.Poll(100000, [Net.Sockets.SelectMode]::SelectRead)
            $rb = $B.Poll(100000, [Net.Sockets.SelectMode]::SelectRead)
            $c.Close()
          } else {
            $c = New-Object Net.Sockets.UdpClient
            [void]$c.Send([byte[]](1, 2, 3), 3, (New-Object Net.IPEndPoint([Net.IPAddress]::Loopback, $port)))
            Start-Sleep -Milliseconds 30
            $ra = $A.Available -gt 0
            $rb = $B.Available -gt 0
            $c.Close()
          }
          $who = if ($ra -and $rb) { 'both' } elseif ($ra) { 'A' } elseif ($rb) { 'B' } else { 'neither' }
        } catch {
          $who = "probe_err:$($_.Exception.Message -replace ',', ';')"
        }
      }
      "$proto,$an-$ao,$bn-$bo,$bres,$who"
      if ($B) { $B.Close() }
      if ($A) { $A.Close() }
    } }
  } }
}
"== done"

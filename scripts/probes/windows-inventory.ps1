# A read-only inventory of a Windows test machine — what the port's measurements depend on.
#
#   scripts/probes/windows-probe.sh dell@192.168.0.107 scripts/probes/windows-inventory.ps1
#
# Reads, never changes: the OS build (docs/PLAN-windows-port.md D6), whether this session's
# token is elevated (an SSH session's is, a desktop user's is not — token-sensitive results
# must be measured from both), %LOCALAPPDATA% (the app-data root, W3), the C: volume, long
# paths, who holds port 53 (D2), the services that can (ICS, Hyper-V, WSL, Docker), the
# excluded port ranges ensure_free must name (§6), whether any of rexenv's own ports are
# taken, and Defender's real-time state (first-start scans).
#
# First run, 13 Sep 2026, the Dell: Windows 10 Pro 22H2 (19045), elevated token, nothing on
# 53, ICS running but idle, no Hyper-V, no Docker, WSL present with no distribution, only
# 50000-50059 excluded, rexenv's ports free, Defender on.

$ProgressPreference = 'SilentlyContinue'
$ErrorActionPreference = 'Continue'

"== os"
$os = Get-CimInstance Win32_OperatingSystem
"{0} | version {1} | build {2} | {3}" -f $os.Caption, $os.Version, $os.BuildNumber, $os.OSArchitecture
"display version: $((Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' -ErrorAction SilentlyContinue).DisplayVersion)"
"powershell: $($PSVersionTable.PSVersion)"
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
"cpu: $($cpu.Name.Trim()) | logical: $($cpu.NumberOfLogicalProcessors)"
"ram GB total/free: {0:N1} / {1:N1}" -f ($os.TotalVisibleMemorySize / 1MB), ($os.FreePhysicalMemory / 1MB)

"== user"
"whoami: $(whoami)"
$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
"elevated admin token: $($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator))"
"LOCALAPPDATA: $env:LOCALAPPDATA"
$drive = Get-PSDrive C
"C: free GB: {0:N1} of {1:N1}" -f ($drive.Free / 1GB), (($drive.Used + $drive.Free) / 1GB)
"C: filesystem: $((Get-Volume -DriveLetter C).FileSystem)"
"LongPathsEnabled: $((Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem' -ErrorAction SilentlyContinue).LongPathsEnabled)"

"== port 53"
$udp53 = Get-NetUDPEndpoint -LocalPort 53 -ErrorAction SilentlyContinue
if ($udp53) {
  foreach ($e in $udp53) { "udp {0}:53 pid {1} {2}" -f $e.LocalAddress, $e.OwningProcess, (Get-Process -Id $e.OwningProcess -ErrorAction SilentlyContinue).ProcessName }
} else { "udp 53: nobody" }
$tcp53 = Get-NetTCPConnection -LocalPort 53 -State Listen -ErrorAction SilentlyContinue
if ($tcp53) {
  foreach ($e in $tcp53) { "tcp {0}:53 pid {1} {2}" -f $e.LocalAddress, $e.OwningProcess, (Get-Process -Id $e.OwningProcess -ErrorAction SilentlyContinue).ProcessName }
} else { "tcp 53 listen: nobody" }

"== services"
foreach ($n in 'SharedAccess', 'vmms', 'hns', 'WSLService', 'LxssManager', 'com.docker.service', 'Dnscache', 'iphlpsvc') {
  $s = Get-Service -Name $n -ErrorAction SilentlyContinue
  if ($s) { "{0}: {1} ({2})" -f $n, $s.Status, $s.StartType } else { "${n}: not installed" }
}
"wsl.exe present: $(Test-Path "$env:WINDIR\System32\wsl.exe")"
"docker cli on PATH: $([bool](Get-Command docker -ErrorAction SilentlyContinue))"

"== excluded port ranges"
netsh int ipv4 show excludedportrange protocol=tcp
netsh int ipv4 show excludedportrange protocol=udp

"== rexenv ports held now"
$ports = 80, 443, 11025, 13306, 15432, 18025, 18088, 9774, 9780, 9781, 9782, 9783, 9784, 9785
$held = Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | Where-Object { $ports -contains $_.LocalPort }
if ($held) {
  foreach ($h in $held) { "tcp {0}:{1} pid {2} {3}" -f $h.LocalAddress, $h.LocalPort, $h.OwningProcess, (Get-Process -Id $h.OwningProcess -ErrorAction SilentlyContinue).ProcessName }
} else { "none of rexenv's TCP ports are listened on" }
"udp 15353: $(if (Get-NetUDPEndpoint -LocalPort 15353 -ErrorAction SilentlyContinue) { 'held' } else { 'free' })"

"== defender"
try {
  $m = Get-MpComputerStatus -ErrorAction Stop
  "RealTimeProtectionEnabled: $($m.RealTimeProtectionEnabled) | AntivirusEnabled: $($m.AntivirusEnabled)"
} catch { "defender status unreadable: $($_.Exception.Message)" }
"== done"

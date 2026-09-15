# windows_desktop_probe in the logged-on DESKTOP session (plan §5 W7, measure first): a named pipe as a
# single-instance lock, directory junctions, and - only when REXENV_PROBE_OPEN is set below - the shell's
# open and reveal, which put windows on the screen of whoever is at the machine.
# Run through windows-limited-token.sh:
#
#   BUILD_ONLY=1 scripts/probes/windows-example.sh <host> windows_desktop_probe
#   scp src-tauri/target/xwin/x86_64-pc-windows-msvc/debug/examples/windows_desktop_probe.exe <host>:
#   scripts/probes/windows-limited-token.sh <host> scripts/probes/windows-desktop-probe.ps1
#
# Changes nothing outside its own fixture folder under %TEMP%, which it removes. The exe is removed at the
# end. ASCII only (PowerShell 5.1).

$ErrorActionPreference = 'Continue'
$exe = Join-Path $HOME 'windows_desktop_probe.exe'
if (-not (Test-Path $exe)) {
  "# no $exe - copy it first (see the header)"
  return
}
# Opening windows is opt-in: set to '1' only with someone at the screen who agreed to it.
$env:REXENV_PROBE_OPEN = '1'
& $exe 2>&1 | ForEach-Object { "$_" }
"exit=$LASTEXITCODE"
Start-Sleep -Seconds 2
Remove-Item -Force $exe -ErrorAction SilentlyContinue
"# exe removed: $(-not (Test-Path $exe))"

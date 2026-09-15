# windows_shell_open_check in the logged-on DESKTOP session (ledger #621, W7 S2): ShellRunner::open and
# reveal through the real platform. Windows appear on the screen and are closed again - run it with someone
# at the machine who agreed to that.
# Run through windows-limited-token.sh:
#
#   BUILD_ONLY=1 scripts/probes/windows-example.sh <host> windows_shell_open_check
#   scp src-tauri/target/xwin/x86_64-pc-windows-msvc/debug/examples/windows_shell_open_check.exe <host>:
#   scripts/probes/windows-limited-token.sh <host> scripts/probes/windows-shell-open.ps1
#
# Changes nothing outside its fixture folder under %TEMP%, which it removes. The exe is removed at the end.
# ASCII only (PowerShell 5.1).

$ErrorActionPreference = 'Continue'
$exe = Join-Path $HOME 'windows_shell_open_check.exe'
if (-not (Test-Path $exe)) {
  "# no $exe - copy it first (see the header)"
  return
}
& $exe 2>&1 | ForEach-Object { "$_" }
"exit=$LASTEXITCODE"
Start-Sleep -Seconds 2
Remove-Item -Force $exe -ErrorAction SilentlyContinue
"# exe removed: $(-not (Test-Path $exe))"

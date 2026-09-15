# windows_cli_pipe_probe in the logged-on DESKTOP session (plan section 5 W8, measure first): the running app's
# lock pipe reached from a Medium-integrity token, a busy pipe, progress order, and a rex mcp-shaped bridge.
# Run through windows-limited-token.sh, after the same exe has run once from the elevated SSH token:
#
#   BUILD_ONLY=1 scripts/probes/windows-example.sh <host> windows_cli_pipe_probe
#   scp src-tauri/target/xwin/x86_64-pc-windows-msvc/debug/examples/windows_cli_pipe_probe.exe <host>:
#   ssh <host> .\windows_cli_pipe_probe.exe
#   scripts/probes/windows-limited-token.sh <host> scripts/probes/windows-cli-pipe-probe.ps1
#
# Changes nothing: the fixture pipes die with the process, and the app's pipe gets one request no build
# acts on. The exe is removed at the end. ASCII only (PowerShell 5.1).

$ErrorActionPreference = 'Continue'
$exe = Join-Path $HOME 'windows_cli_pipe_probe.exe'
if (-not (Test-Path $exe)) {
  "# no $exe - copy it first (see the header)"
  return
}
& $exe 2>&1 | ForEach-Object { "$_" }
"exit=$LASTEXITCODE"
Start-Sleep -Seconds 2
Remove-Item -Force $exe -ErrorAction SilentlyContinue
"# exe removed: $(-not (Test-Path $exe))"

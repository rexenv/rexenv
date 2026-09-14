# windows_browser_lock_check in the logged-on DESKTOP session (ledger #614), where Windows' certificate
# prompts can be answered: install Yes, then delete Yes. Run through windows-limited-token.sh, with
# someone at the machine:
#
#   BUILD_ONLY=1 scripts/probes/windows-example.sh <host> windows_browser_lock_check
#   scp src-tauri/target/xwin/x86_64-pc-windows-msvc/debug/examples/windows_browser_lock_check.exe <host>:
#   scripts/probes/windows-limited-token.sh <host> scripts/probes/windows-browser-lock.ps1
#
# CHANGES this user's Root certificate store (a fixture CA added, then removed) and binds :443/:80
# while it runs. Owner's go: 14 Sep 2026. The exe is removed at the end. ASCII only (PowerShell 5.1).

$ErrorActionPreference = 'Continue'
$exe = Join-Path $HOME 'windows_browser_lock_check.exe'
if (-not (Test-Path $exe)) {
  "# no $exe - copy it first (see the header)"
  return
}
$env:REXENV_CERT_TRUST_WRITE = '1'
& $exe 2>&1 | ForEach-Object { "$_" }
"exit=$LASTEXITCODE"
Remove-Item -Force $exe -ErrorAction SilentlyContinue
"# exe removed: $(-not (Test-Path $exe))"

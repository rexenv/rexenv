# The write phase of windows_cert_trust_check in the logged-on DESKTOP session (ledger #613), where
# Windows' "Security Warning: You are about to install a certificate..." prompt can be seen and
# answered. Run through windows-limited-token.sh, with someone at the machine to answer it:
#
#   BUILD_ONLY=1 scripts/probes/windows-example.sh <host> windows_cert_trust_check
#   scp src-tauri/target/xwin/x86_64-pc-windows-msvc/debug/examples/windows_cert_trust_check.exe <host>:
#   scripts/probes/windows-limited-token.sh <host> scripts/probes/windows-cert-trust-desktop.ps1
#
# It CHANGES this user's Root certificate store: a fixture CA is added, then removed (two prompts).
# Owner's go: 14 Sep 2026. The exe is removed at the end. ASCII only: PowerShell 5.1 reads a
# BOM-less script as the ANSI code page.

$ErrorActionPreference = 'Continue'
$exe = Join-Path $HOME 'windows_cert_trust_check.exe'
if (-not (Test-Path $exe)) {
  "# no $exe - copy it first (see the header)"
  return
}
$env:REXENV_CERT_TRUST_WRITE = '1'
& $exe 2>&1 | ForEach-Object { "$_" }
"exit=$LASTEXITCODE"
Remove-Item -Force $exe -ErrorAction SilentlyContinue
"# exe removed: $(-not (Test-Path $exe))"

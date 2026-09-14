# windows_uac_step_check in the logged-on DESKTOP session (ledger #619): rexenv's own dialog, then UAC for
# rexenv.exe, answered by the person at the machine in the order the check prints -
#   1. OK, Yes   2. OK, Yes   3. Cancel   4. OK, No
# Run through windows-limited-token.sh:
#
#   BUILD_ONLY=1 scripts/probes/windows-example.sh <host> windows_uac_step_check
#   scp src-tauri/target/xwin/x86_64-pc-windows-msvc/debug/examples/windows_uac_step_check.exe <host>:
#   scripts/probes/windows-limited-token.sh <host> scripts/probes/windows-uac-step.ps1
#
# CHANGES the machine while it runs: one NRPT rule for the test TLD .rexuaccheck, added and removed.
# Owner's go: 15 Sep 2026. The exe is removed at the end. ASCII only (PowerShell 5.1).

$ErrorActionPreference = 'Continue'
$exe = Join-Path $HOME 'windows_uac_step_check.exe'
if (-not (Test-Path $exe)) {
  "# no $exe - copy it first (see the header)"
  return
}
& $exe 2>&1 | ForEach-Object { "$_" }
"exit=$LASTEXITCODE"
Start-Sleep -Seconds 2
Remove-Item -Force $exe -ErrorAction SilentlyContinue
"# exe removed: $(-not (Test-Path $exe))"

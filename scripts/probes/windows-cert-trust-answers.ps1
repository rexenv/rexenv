# windows_cert_trust_check's ANSWERED phase in the logged-on desktop session (ledger #613): the person
# at the machine answers Windows' four certificate prompts in this order -
#   1. install: No   2. install: Yes   3. delete: No   4. delete: Yes
# and the check holds each answer to its outcome. The window watcher prints each prompt's title and
# text. Run exactly like windows-cert-trust-desktop.ps1 (copy the exe first, then
# windows-limited-token.sh). CHANGES this user's Root certificate store; the fixture CA is removed at
# the last Yes. Owner's go: 14 Sep 2026. ASCII only (PowerShell 5.1).

$ErrorActionPreference = 'Continue'
$exe = Join-Path $HOME 'windows_cert_trust_check.exe'
if (-not (Test-Path $exe)) {
  "# no $exe - copy it first"
  return
}
$env:REXENV_CERT_TRUST_WRITE = '1'
$env:REXENV_CERT_TRUST_ANSWERS = 'no-yes'
& $exe 2>&1 | ForEach-Object { "$_" }
"exit=$LASTEXITCODE"
Remove-Item -Force $exe -ErrorAction SilentlyContinue
"# exe removed: $(-not (Test-Path $exe))"

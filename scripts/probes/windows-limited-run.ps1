# Runs beside a copied probe INSIDE a scheduled task, under a non-elevated token —
# started by scripts/probes/windows-limited-token.sh, never by hand.
#
# An SSH session on the Dell carries an ELEVATED admin token; a desktop user's is filtered.
# Socket and ACL behaviour can differ between the two, so a result that matters must be
# measured under both (docs/PLAN-windows-port.md §6, §7). This wrapper records WHICH token
# it actually got before running the probe — a result is only "non-elevated" if these
# lines say so, not because the task was asked for it.
#
# Output, next to this file as output.txt:
#   # user: <domain\user>
#   # token elevated: True|False
#   # integrity: <the Mandatory Label group whoami reports>
#   <the probe's own output>

$ProgressPreference = 'SilentlyContinue'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
$integrity = (whoami /groups | Select-String 'Mandatory Label') -join ' '
$header = @(
  "# user: $(whoami)",
  "# token elevated: $($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator))",
  "# integrity: $($integrity -replace '\s+', ' ')"
)
try {
  $body = & (Join-Path $here 'probe.ps1') 2>&1 | ForEach-Object { "$_" }
} catch {
  $body = @("# probe failed: $($_.Exception.Message)")
}
($header + $body) | Out-File -Encoding utf8 (Join-Path $here 'output.txt')

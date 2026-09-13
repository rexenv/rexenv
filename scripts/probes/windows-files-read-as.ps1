# Run by a scheduled task as ANOTHER account (windows-files-check.sh uses NT AUTHORITY\LOCAL
# SERVICE): try to read each fixture file and record what happened, one line per file, in
# out\read-as.txt. The fixture directory is passed as the only argument.
param([string]$Dir)
$out = Join-Path $Dir 'out\read-as.txt'
$lines = @("# account: $([Security.Principal.WindowsIdentity]::GetCurrent().Name)")
foreach ($name in 'control.txt', 'born.txt', 'hardened.txt', 'rewritten.txt') {
  try {
    $text = [IO.File]::ReadAllText((Join-Path $Dir $name))
    $lines += "$name READ ($($text.Length) chars)"
  } catch {
    $lines += "$name DENIED ($($_.Exception.InnerException.GetType().Name): $($_.Exception.InnerException.Message))"
  }
}
$lines | Set-Content -Path $out -Encoding UTF8

# windows_app_open_check in the logged-on DESKTOP session (ledger #622, W7 S3): the editors, browsers and
# terminals ShellRunner detects and opens, through the real platform. Windows appear on the screen; the
# ones naming the check's fixture are closed again, and the browser window it opens is left for the person
# at the machine to close.
# Run through windows-limited-token.sh:
#
#   BUILD_ONLY=1 scripts/probes/windows-example.sh <host> windows_app_open_check
#   scp src-tauri/target/xwin/x86_64-pc-windows-msvc/debug/examples/windows_app_open_check.exe <host>:
#   scripts/probes/windows-limited-token.sh <host> scripts/probes/windows-app-open.ps1
#
# The expected lists below are the Dell Inspiron's (inventory of 15 Sep 2026, plan section 5 W7). On another
# machine, change them or remove them - without them the check prints what it found and asserts the rest.
# ASCII only (PowerShell 5.1).

$ErrorActionPreference = 'Continue'
$exe = Join-Path $HOME 'windows_app_open_check.exe'
if (-not (Test-Path $exe)) {
  "# no $exe - copy it first (see the header)"
  return
}
$env:REXENV_EXPECT_EDITORS = 'vscode,phpstorm'
$env:REXENV_EXPECT_BROWSERS = 'chrome,firefox,brave,edge'
$env:REXENV_EXPECT_TERMINALS = 'git-bash,powershell,cmd'
$env:REXENV_EXPECT_DEFAULT_BROWSER = 'chrome'
& $exe 2>&1 | ForEach-Object { "$_" }
"exit=$LASTEXITCODE"
Start-Sleep -Seconds 2
Remove-Item -Force $exe -ErrorAction SilentlyContinue
"# exe removed: $(-not (Test-Path $exe))"

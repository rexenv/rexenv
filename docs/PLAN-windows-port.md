# PLAN — rexenv on Windows: the port, the decisions it forces, and the launch

**Status:** IN PROGRESS — W0 and W1 done 12 Sep 2026: both crates compile for Windows, and
`scripts/windows-check.sh` runs inside `verify.sh`. W2 next; W3 onward waits on the owner's
rulings D1, D2 and D4–D6 (§3; D3 is ruled). Planned against `e0d287c`. Open work is tracked as the
"Windows launch" row in `docs/TODO.md`; this file is the reasoning behind it.

macOS is stable and feature-rich (Phases 1–3 shipped). The owner wants a Windows release.
This plan records what that costs, measured against the tree and against the upstreams,
so the first person to pick it up does not start from the belief the docs used to state.

---

## 1. The belief this plan corrects

CLAUDE.md, `docs/ARCHITECTURE.md` and the README all said: *adding an OS = filling the
`todo!()` stubs, NOT restructuring.* Measured on 12 Sep 2026, that is **true for the 12
traits and false for the tree**:

- The stubs are real and bounded: **49 `todo!()`** in `src-tauri/src/platform/windows/mod.rs`.
- But Unix-only PRODUCTION code lives outside `platform/` (§2.1) — the Windows build does
  not compile until it moves — and three subsystems rest on a mechanism Windows does not
  have at all (php-fpm, `/etc/resolver`, unix sockets; §3). Those are design work, not stubs.
- Ledger #163's guard (`core_production_code_never_names_an_os_or_carries_an_os_cfg`) is
  green throughout, because it scans for `platform::{macos,…}` and `#[cfg(target_os` — not
  for `std::os::unix`, `Command::new("kill")`, or a hardcoded `macos-` URL. A guard that
  checks one place inside the surface it claims; W1 widens it.

## 2. What the tree says (measured 12 Sep 2026, test modules excluded)

### 2.1 Unix-only production code outside `platform/`

| Where | What | Why it breaks on Windows |
|---|---|---|
| `src-tauri/src/cli_server.rs` (35–1986) | `UnixListener`/`UnixStream`, `PermissionsExt` 0600 | no `std::os::unix` / tokio unix on Windows — does not compile |
| `src-tauri/src/mcp_server.rs` (88–324) | same, for the MCP socket | same |
| `cli/src/main.rs` (12, 281, 389, 562, 654) | the `rex` CLI connects over `UnixStream` | same — the CLI crate does not compile |
| `src-tauri/src/core/proxy.rs:68, :220` | `OsStrExt`; Caddy admin socket probe | same |
| `src-tauri/src/core/dbsource.rs:296` | `UnixStream` to a database socket | same |
| `src-tauri/src/core/cli.rs:99` | `std::os::unix::fs::symlink` | ~~same; `ShellRunner::symlink_dir` already exists~~ **Fixed 12 Sep 2026 (W1) — and not with `symlink_dir`**, which this row first named: the `rex` link is a FILE, and a Windows directory junction cannot point at one. A new `ShellRunner::symlink_file` (default unsupported). The link is now tried BEFORE anything at the destination is removed — the old `cfg(not(unix))` arm never touched the disk, and a remove-then-link order would have deleted a file on a platform that then cannot link |
| `src-tauri/src/core/confrewrite.rs:85` | `OpenOptionsExt` mode 0600 | **Fixed 12 Sep 2026 (W1):** the temp is born through `PermissionManager::write_private`; the sync moved to a write handle (Windows will not flush a read-only one). `atomic_write_preserving_mode` takes the `Platform` now (ledger #130) |
| `src-tauri/src/core/proc.rs:88, :108`, `core/services.rs:875`, `core/dbdump.rs:864`, `core/dbrestore.rs:182` | `Command::new("kill")` | compiles, fails at runtime — there is no `kill` and no signals. **Fixed 12 Sep 2026 (W1):** three copies of the TERM → 2s → KILL loop, the nginx SIGHUP and the adopted-pid `kill -0` became `ProcessSupervisor::terminate_child` / `signal_reload` / `pid_alive`. macOS overrides each with the exact old behaviour; the defaults are the portable floor (kill + wait, "no reload signal", has-a-command-line), so Windows compiles and falls back honestly until W3 decides what graceful means per service |

The table above was a grep. **W0's compiler run is the authoritative list**
(`scripts/windows-check.sh`, first run 12 Sep 2026: RED, 29 error sites — 19 in
`src-tauri`, 10 in `cli`). It confirmed every socket row and found four the grep missed:

- ~~**`#[cfg(unix)]` modules used ungated.** `mcp_server` (`lib.rs:12`) and `commands::mcp`
  (`commands/mod.rs:14`) are compiled out on Windows, yet `lib.rs:223, :973, :1367–1368,
  :1858, :2165` and `commands/scratch.rs:24` name them.~~ **Fixed for MCP, 12 Sep 2026
  (W1):** the module gate was wider than the thing that is unix-specific. Both modules
  and `AppState.mcp` now compile on every OS; only the transport (`socket_path`,
  `bind_socket`, `start`, `serve`, and the test that binds) is `cfg(unix)`, and off unix
  `start` refuses so the toggle never reads on (ledger #203's scope note). Un-gating
  surfaced one more: `mcp_server` called `cli_server::repo_job_settled`, a pure predicate
  stranded in the `cfg(unix)` CLI server — moved beside `RepoJobState` in
  `commands/repo.rs`. Windows run 56 → 45. **Then `cli_server` got the same
  treatment:** only its socket transport (`claim`, `bind`, `serve`, `spawn`, the hand-off)
  is `cfg(unix)`; `handle_request` and `dispatch` compile everywhere — three examples drive
  them in-process today, and W8's named pipe will reach the same dispatch. Ledger #163's scan looks for
  `cfg(target_os`, not `cfg(unix)`, so none of this ever failed it.
- ~~**`QUIT_MENU_ID`** is defined under `cfg(macos)` (`lib.rs:2255`) and used outside it
  (`lib.rs:2289, :2298`).~~ **Fixed 12 Sep 2026 (W1):** the `cfg` had landed between
  `install_about_menu_item`'s doc comment and the const's, gating the const and not the
  function. The attribute now sits on the function and the const lives inside it, its
  only user; Windows run 58 → 56, no `QUIT_MENU_ID` site left.
- **The `objc2` dev-dependencies** (`src-tauri/Cargo.toml` `[dev-dependencies]`:
  `objc2`, `objc2-foundation`, `objc2-web-kit`) are not target-gated, so every test and
  example target fails for Windows before a line of ours is checked. The `[dependencies]`
  copies are correctly under `cfg(target_os = "macos")`.
- **Build plumbing.** `tauri_build` refuses a missing `binaries/rex-<triple>.exe`, and
  `build.rs`'s sidecar staging is `#[cfg(target_os = "macos")]` — which in a build script
  means the HOST, so a Mac cross-check stages nothing for Windows. `build.rs` also shells
  to `sh` and `date`, which a Windows HOST build (W12) does not have.

A first-layer list, not a final one: most of the E0282/E0277 sites are the cascade of
an unresolved import, and fixing resolve errors routinely uncovers type errors behind them.

**W1's first fix raised the count, and that is the fix working.** With the `objc2`
dev-dependencies target-gated (12 Sep 2026) the run went from 29 to **58** error sites:
the objc2 failure had stopped cargo before any `#[cfg(test)]` module was checked, so the
first number counted production code only. The new sites are Unix APIs inside TEST
modules — `std::os::unix::fs::symlink` / `PermissionsExt` / `ExitStatusExt` /
`CommandExt::process_group` in `core/{confrewrite,devtools,dist_archive,laravel,localwp,
sites,valet,wordpress}.rs` and `commands/valet_import.rs`, a test `UnixListener` in
`core/dbsource.rs` — plus two more ungated uses: `mcp_server` in `state/db.rs`
(:1554, :1584) and the cfg-gated `UNPINNED_PROVISIONERS` in `core/sites.rs` (:4973,
:4993). Test-only sites need a Windows arm or a `cfg(unix)` on the test, never a
production change; they are counted so the number stays honest, not because a user
meets them.

**And 58 still undercounts.** `--keep-going` does not reach a target whose dependency
failed: the log shows `rexenv (lib)` failing on 18 errors and `rexenv (lib test)` on 48,
and NO example or bin unit checked at all — every one of them links the lib. So the
134 examples and `main.rs` are invisible until the lib compiles for Windows, and one
suspicion rides with them: `examples/webview_dialogs_check.rs` is gated with a
file-level `#![cfg(target_os = "macos")]`, which on Windows leaves a crate with no
`main` (E0601). Expect the count to rise again the day the lib goes green.

**The lib went green on 12 Sep 2026, and the count rose exactly as predicted: 45 → 92.**
With `proxy::admin_alive` and `dbsource::probe_socket` dialling through `LocalIpc`, the
`rexenv` library AND the app binary (`main.rs`) compile for `x86_64-pc-windows-msvc` — the
failed-unit list no longer names `(lib)` or a bin. What is left is where the production
code is not:

| Unit | Error sites | What they are |
|---|---|---|
| `rexenv (lib test)` | 24 | Unix APIs inside `#[cfg(test)]` modules (symlink/permission fixtures, `ExitStatusExt`, `process_group`) + `UNPINNED_PROVISIONERS` |
| 17 examples | 58 | mostly `mcp_*`/`cli_*` checks that bind the unix socket themselves (`mcp_control_check` alone has 10); `webview_dialogs_check` is the predicted E0601, and a second E0601 rides along |
| `cli` crate | 10 | the `rex` client's `UnixStream` — waits for D3's named pipe (W8) |

None of these is code a Windows user runs. They still matter — `verify.sh` builds the
examples, so a Windows bar will too — but the thing that ships compiles.

**The guard now holds the line (12 Sep 2026).** With the last `cfg(unix)` out of `core/`
(the rewrite's temp file → `write_private`, the CLI link → `ShellRunner::symlink_file`),
ledger #163's scan was widened to refuse `std::os::unix`, `#[cfg(unix)]`, `cfg(not(unix))`
and `Command::new("kill")` in `core/` production — so a Unix API cannot walk back in
unnoticed. Landing it exposed a bug in the scanner every tree-wide guard shares:
`copy_scan::production_lines` found a test module's end by COUNTING braces, including braces
inside strings, so `core/localwp.rs`'s `format!("{{{},{}}}")` and `core/repo.rs`'s multi-line
raw JSON closed their test modules early and the tails read as production. The first fix
(per-line string skipping) failed on the multi-line case; the second carries string, raw-string
and block-comment state across lines. The error was in the loud direction — test code
flagged as production — but every guard on that function had been reading those tails.

**Every example compiles for Windows (12 Sep 2026).** Windows run 78 → 31, all of it the
`lib test` unit (21) and the `cli` crate (10). No example changed behaviour on macOS; the
fixes came in three kinds. Unix file-mode and symlink fixtures moved behind
`examples/common` helpers (`mode_bits`, `set_mode`, `symlink` — inert off unix). The six
checks that talk to the app over its unix socket (`mcp_*`, `cli_socket_check`) and the two
whose subject is macOS-only (`app_relaunch_check`, `tunnel_parent_death_check`) keep their
real `main` behind a cfg and print a skip line elsewhere. And `webview_dialogs_check` traded
its file-level `#![cfg(target_os = "macos")]` — which on any other OS leaves a crate with no
`main` (E0601) — for per-item gates. The three examples that only drive `handle_request`
in-process needed nothing: gating `cli_server`'s transport instead of the whole module was
the whole fix.

**`src-tauri` compiles for Windows, every target (12 Sep 2026).** The last 21 sites were
inside `#[cfg(test)]` modules: unix symlink and file-mode fixtures, a `/bin/sh` fake
supervisor's `process_group`, and an `ExitStatusExt::from_raw`. They now go through
`src/test_support.rs` — deliberately outside `core/`, because a helper FILE reads as
production to ledger #163's scan even when only tests call it. The one test that runs
`#!/bin/sh` scripts is `cfg(unix)`, and the fake supervisor sets its process group only
on unix. One more misplaced `cfg` turned up, the `QUIT_MENU_ID` shape again:
`#[cfg(target_os = "macos")]` above a stray doc comment in `core/sites.rs` gated the
`UNPINNED_PROVISIONERS` const while the test that reads it was ungated — the test only
reads example files, so the attribute went. Windows run 31 → 10, all in the `cli` crate.

**W1 done: both crates compile for Windows (12 Sep 2026).** The `rex` client's four
`UnixStream::connect` calls go through one `connect`; off unix a stub `Stream` fails every
method, so nothing is dialled — and `socket_path` already refuses on non-macOS hosts first.
`windows-check.sh` went green on both crates and joined `verify.sh` by owner ruling: the
pre-commit bar, not only the release gate, with a SKIPPED line where the toolchain or the
licence consent is missing and a red bar on any real break (ledger #584). The Windows
count, from first run to last: 29 → 58 → 56 → 45 → 92 → 89 → 78 → 31 → 10 → 0 — every rise
a unit that had been hidden behind a failure becoming visible.

### 2.2 The binary catalog has no OS dimension

`src-tauri/src/core/binaries.rs` keys pins by `Arch` alone, and the URLs spell macOS into
the format string: `…-macos-{arch}.tar.gz` (PHP, :940/:943; debug PHP, :1311), nginx
(:1466), `apple-darwin` (PostgreSQL, :1493), `darwin` (Mailpit :1517, cloudflared :1531),
Homebrew `arm64_sonoma` bottles (Redis, httpd, Xdebug). Every pin needs an `(os, arch)` key
and its own checksum. The arch-name helpers (`caddy_arch`, `php_arch`, …) become per-OS
too: upstreams spell Windows differently (`windows_amd64`, `windows-x86_64`,
`x86_64-pc-windows-msvc`).

### 2.3 Compiles fine, behaves wrong

- **Mail.** Pools and the CLI set `sendmail_path` (`core/wordpress.rs:278`,
  `core/service_manager.rs`). Windows PHP ignores `sendmail_path` and talks SMTP itself:
  it needs `SMTP=127.0.0.1` + `smtp_port=11025` instead.
- **Already correctly gated:** tray/dock/About-menu (`lib.rs`), `webview_dialogs`
  (WebView2 draws its own JS dialogs), tunnel guard, relauncher, `activate_app` — all
  `cfg(macos)`. Nothing to undo there; the Windows equivalents are new work (W7, W11).
- `portable-pty` (the terminal) is cross-platform already (ConPTY).

## 3. Decisions the owner rules on before W3

**D1 — PHP process model (Windows has no php-fpm).** Official Windows PHP ships
`php-cgi.exe`; `PHP_FCGI_CHILDREN` does not fork workers on Windows, so one process serves
one request at a time. The non-negotiable "one php-fpm pool per PHP version" becomes
"one supervised php-cgi *group* per PHP minor".
*Recommendation:* rexenv spawns a small fixed number of `php-cgi.exe` workers per minor
(e.g. 4), each on its own port, fronted by an nginx `upstream`; a worker that exits
(`PHP_FCGI_MAX_REQUESTS`, a crash) is respawned by the supervisor. Needs a new port block
in `docs/PORTS.md` — the `9700 + major*10 + minor` formula has one slot per minor.

**D2 — `.rex` DNS (no `/etc/resolver`).** Windows routes a suffix to a server with an NRPT
rule (`Add-DnsClientNrptRule -Namespace .rex -NameServers 127.0.0.1`, admin). **NRPT has
no port parameter** (verified against the cmdlet reference, 12 Sep 2026), so the DNS
agent must answer on **127.0.0.1:53**, not 15353.
*Recommendation:* agent on 127.0.0.1:53 + one NRPT rule per TLD (one UAC prompt).
Fallback when 53 is taken: per-site `hosts` entries (no wildcards — subdomain multisite
degrades, and the UI must say so). *Unmeasured:* whether anything commonly binds loopback
:53 on a developer's Windows machine (ICS/SharedAccess, Hyper-V, WSL, Docker Desktop).

**D3 — Local IPC (the CLI, MCP and Caddy admin sockets).** **RULED 12 Sep 2026 by the
owner: named pipes with a current-user ACL, behind a new 13th trait `LocalIpc`** (chosen
over a method on `ProcessSupervisor`, which would have kept the count and mixed a transport
into the process trait). First use landed in W1: `proxy::admin_alive` and
`dbsource::probe_socket` dial through it. The Caddy admin measurement below still stands
before W5. `std`/tokio have no unix
sockets on Windows.
*Recommendation:* the CLI and MCP servers use named pipes
(`tokio::net::windows::named_pipe`) with a security descriptor that admits only the
current user — the Windows equivalent of `0600`. Behind a new platform trait (say
`LocalIpc`), so `cli_server`/`mcp_server`/`cli` stop naming a transport. **The Caddy admin
rule stands: never TCP `:2019`.** Measure first whether Caddy on Windows accepts a
`unix//` admin address (Go supports AF_UNIX on Windows 10 1803+; unconfirmed for Caddy's
admin listener). If not, this is a blocker to escalate, not a rule to relax.

**D4 — What ships in Windows v1.** Official, checksum-lockable artifacts exist for most
of the stack (§4). Redis has no official Windows build; Apache on Windows means Apache
Lounge (a third-party trust decision); Xdebug DLLs must match PHP's NTS + compiler.
*Recommendation:* v1 = Caddy, nginx, PHP 7.4–8.5, MySQL, PostgreSQL, Mailpit, Adminer,
WP-CLI, Composer, cloudflared; FrankenPHP if W4 proves it. Redis, Apache and Xdebug are
refused in CORE on Windows with an honest message — the shape OpenLiteSpeed already
uses (`ensure_server_available`) — until each is proven.

**D5 — Signing, installer, updates, distribution.**
Unsigned Windows installers hit SmartScreen's "Windows protected your PC".
*Recommendation:* NSIS installer (per-user, no admin to install); Authenticode signing
(Azure Trusted Signing is the cheapest route — needs an account, like Apple's
`docs/SIGNING.md`); updates through rexenv's OWN signed-manifest channel with a Windows
`AppBundle` (a running `.exe` is locked — stage, exit, swap, relaunch). The reasons
`docs/archive/PLAN-self-update.md` P1 gave for refusing `tauri-plugin-updater` were
macOS-specific; re-check them for Windows, do not inherit the ruling blind.
Distribution: GitHub release + a winget manifest (the Homebrew tap's counterpart).

**D6 — Supported Windows and architectures.**
Windows 10 left mainstream support on 14 Oct 2025. Official PHP Windows builds are
x86/x64 only (no arm64), and PostgreSQL's portable build is x64 only.
*Recommendation:* Windows 11 x64 supported; Windows 10 22H2 best-effort; arm64 runs the
x64 build under emulation, unsupported.

## 4. Upstream availability (measured 12 Sep 2026)

✓ = the asset was listed at the pinned tag or version; *unchecked* = believed to exist, not
looked at yet (W2 measures and hashes every row before it is pinned).

| Binary | Windows artifact | Checked |
|---|---|---|
| Caddy 2.11.4 | `caddy_2.11.4_windows_amd64.zip` (+ `.sig`) | ✓ |
| Mailpit 1.30.3 | `mailpit-windows-amd64.zip`, `-arm64.zip` | ✓ |
| cloudflared 2026.6.1 | `cloudflared-windows-amd64.exe` / `.msi` | ✓ |
| FrankenPHP 1.12.4 | `frankenphp-windows-x86_64.zip` | ✓ (whether it runs our overrides: W4) |
| PostgreSQL 18.6.0 | theseus-rs `x86_64-pc-windows-msvc` `.zip`/`.tar.gz` + `.sha256` | ✓ (16/17 pins unchecked) |
| PHP | php.net `php-X.Y.Z-nts-Win32-vs16/vs17-x64.zip` + `sha256sum.txt`; current dir lists 7.4.33, 8.0.30, 8.1.34, 8.2.33, 8.3.33, 8.4.25, 8.5.10; **no arm64** | ✓ — note 8.2/8.3/8.4/8.5 patches differ from the macOS pins, so `php::pdo_pgsql_supported`'s per-PATCH answer must be per-OS too |
| nginx | nginx.org Windows zip (upstream calls it beta: `select()`, limited connections — fine for local dev) | unchecked |
| MySQL 8.4 / 8.0 | Oracle Windows `noinstall` zip | unchecked |
| Xdebug | xdebug.org DLLs per PHP minor / NTS / compiler | unchecked |
| Redis | no official build | D4 |
| Apache httpd | Apache Lounge | D4 |
| WP-CLI, Composer, Adminer | `.phar` / `.php`, OS-agnostic | — |

## 5. Tasks

Each ends in something observable. W0–W2 change nothing a macOS user sees.

- **W0 — Windows compile check, on the Mac.** Owner ruling 12 Sep 2026: local
  `cargo xwin check --all-targets --target x86_64-pc-windows-msvc` for `src-tauri` and
  `cli`, not a GitHub Actions job — this private repo has never run Actions (release
  builds are local for the same billing reason, `docs/RELEASING.md`), and xwin brings the
  MSVC CRT/SDK that `ring` and the other C build scripts need (Homebrew `llvm` + `lld`
  supply `clang-cl`/`lld-link`; the owner accepted Microsoft's SDK licence). A separate
  `CARGO_TARGET_DIR` so it never invalidates `verify.sh`'s macOS cache. It proves
  COMPILATION only — no Windows test runs, no link. *Done when:* a script runs it from its
  own cwd with a load-bearing exit code, and its error list replaces §2.1 as the
  authoritative inventory. A CI job stays possible later (W12) at the owner's call.
- **W1 — Move the leaks behind traits.** Sockets → `LocalIpc` (D3); `kill` → a
  `ProcessSupervisor` method; `core/cli.rs` symlink → `ShellRunner::symlink_file` (not `symlink_dir` — a junction
  cannot point at a file);
  `confrewrite` → `PermissionManager::write_private`. Widen #163's scan to refuse
  `std::os::unix` and `Command::new("kill")` in `core/` production lines, plant-proven.
  *Done when:* `scripts/verify.sh` is green on macOS and W0 compiles the lib against the
  stubs.
- **W2 — OS dimension in the binary catalog.** `(os, arch)` pins, Windows URLs and
  checksums for §4's official rows, `manifest_sweep_check` covering them.
  *Done when:* L0 URL tests pass per OS and the sweep hashes every Windows artifact.
  **Measured 12 Sep 2026 — half of this already existed:** `manifest(name, version, os,
  arch)` and `bundle_manifest` take the OS, and every resolve passes
  `std::env::consts::OS`; only `macos` arms exist. What W2 actually needs: (1) zip
  extraction — every Windows artifact except PostgreSQL (tar.gz) and cloudflared (raw
  `.exe`) is a zip, and the tree had no zip reader (done: `zip` crate by owner ruling,
  `Archive::Zip` / `ZipTree { strip }`, ledger #585); (2) `.exe` naming — a single
  binary publishes at `dir/<name>`, and Windows will not run a file without the
  extension; (3) per-OS shapes — `shape_of(name)` says nginx and php are single
  binaries, while their Windows zips are trees; (4) the arms, all x64 (Windows ARM runs
  them under emulation — there is no arm64 PHP, PostgreSQL or MySQL build); (5) the
  sweep. Every Windows artifact was downloaded and hashed on 12 Sep 2026; Caddy's
  SHA-512 and PostgreSQL's SHA-256 matched their publishers', the rest have no published
  digest (php.net's archive has no `sha256sum.txt`, Mailpit and nginx.org publish none,
  MySQL publishes MD5).
- **W3 — Foundations.** `Paths` (`%LOCALAPPDATA%\rexenv`), `PermissionManager` (owner-only
  ACLs), `BinaryProvider` (strip the `Zone.Identifier` stream, no codesign),
  `ProcessSupervisor` (hidden + detached spawn so services OUTLIVE the app, graceful
  per-service stop, pid → exe/cmdline for ownership, listener lookup via
  `GetExtendedTcpTable`, conflict help naming HTTP.sys and the Hyper-V excluded port
  ranges). *Done when:* MySQL and Mailpit start, survive an app quit, and are adopted on
  relaunch — on a real Windows machine.
- **W4 — Serve a WordPress site.** D1's php-cgi groups, nginx Windows config (forward
  slashes, every path quoted), mail through the SMTP ini keys, WP-CLI/Composer via the
  site's PHP. *Done when:* a one-click WordPress site loads through nginx and its mail
  lands in Mailpit.
- **W5 — HTTPS edge.** Caddy on :443 (Windows has no privileged ports, so the edge need
  not run elevated — record why in ARCHITECTURE), admin per D3, `EdgeSupervisor` for that
  shape; `CertTrustManager` into the CurrentUser Root store (Windows shows its own
  confirmation) plus the Firefox enterprise-roots path. *Done when:* `https://<site>.rex`
  shows a valid lock in Edge, Chrome and Firefox.
- **W6 — DNS + privileges.** D2's agent and NRPT rules; `PrivilegeManager` as a UAC
  elevation that says what it is for (the macOS dialog rule, ledger #579's family);
  `DnsAgentManager` as a logon Scheduled Task with restart. *Done when:* `*.rex` resolves
  after a reboot with the app closed.
- **W7 — Desktop integration.** `ShellRunner` (open/reveal, editors, browsers — closes
  the Windows half of the browser-stub row in TODO — terminals, `git_preflight` naming
  Git for Windows), `AutostartManager` (HKCU Run key), tray and close-to-tray behaviour.
- **W8 — `rex` CLI + MCP on Windows.** Over D3's transport; `rex.exe` sidecar on the
  user PATH. *Done when:* `rex` commands from a new PowerShell reach the running app.
- **W9 — Frontend on WebView2.** Windows paths (`C:\…`) in inputs and display,
  Cmd → Ctrl shortcuts, font metrics; divergences into `docs/DESIGN.md`.
- **W10 — Feature gates per D4.** Refused in core, honest in the UI, one arm to enable later.
- **W11 — Packaging and updates per D5.** NSIS bundle, signing, a Windows job in
  `.github/workflows/release.yml`, Windows `AppBundle`, winget manifest.
- **W12 — Launch gates.** `verify.sh` runnable on the Windows runner (Git Bash);
  macOS-only examples tiered or ported; a Windows section in `docs/SMOKE-TEST.md` run on
  a clean Windows 11 VM; a Windows section in `docs/INSTALL.md` (SmartScreen, the UAC
  prompts, what Defender does to first start).

## 6. Windows hazards to design for, not discover

- **File locking:** an open file cannot be renamed or deleted — atomic binary publish,
  log rotation, database files and the self-update swap all assume Unix semantics.
- **MAX_PATH (260):** deep WordPress / `node_modules` paths; enable long-path support and
  test with it off.
- **Symlinks need Developer Mode or admin:** linked sites and imports use directory
  junctions instead.
- **Case-insensitive paths**, CRLF in generated configs, a console window flashing for
  every spawned process (`CREATE_NO_WINDOW`), Defender scanning a freshly unpacked
  binary tree (slow first start — measure before calling it a bug).
- **Reserved ports:** HTTP.sys (PID 4) can hold 80/443; Hyper-V/WinNAT reserve dynamic
  TCP ranges that can swallow a fixed port like 13306 or 15432 (`netsh int ipv4 show
  excludedportrange protocol=tcp`). `ensure_free` must name both.

## 7. What an agent on the macOS dev machine cannot prove

The Mac proves COMPILATION for Windows (W0's cross-check) and nothing else — no Windows
test, no link, no run. Everything past W2 needs Windows itself. The owner's machines
(12 Sep 2026): a **Dell Inspiron 3543** (i7-5500U, 8 GB — real x64, low-end, not on
Windows 11's supported-CPU list, too slow to be the build box: build elsewhere, run
there) is the release-gate machine; a Windows 11 ARM VM on the M3 Pro Mac runs the x64
build under emulation for the day-to-day loop, and never counts as the x64 proof. Both
are driven over OpenSSH from the Mac; dialogs are read by a human. `docs/TESTING.md` gains a Windows column when W0 lands — not before, so it never
claims coverage that does not run.

## 8. Docs this changes as it lands

`docs/ARCHITECTURE.md` (the OS rule, per-OS process model, DNS, IPC), `docs/MAP.md`
(new trait, Windows modules), `docs/PORTS.md` (php-cgi block, DNS :53 on Windows,
Windows pins), `docs/CLAIM-LEDGER.md` (#163 widened; every new "never"), `docs/TESTING.md`,
`docs/SMOKE-TEST.md`, `docs/INSTALL.md`, `docs/RELEASING.md`, `docs/DESIGN.md`,
`docs/CLI-ROADMAP.md` (named-pipe transport).

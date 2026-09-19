# PLAN — rexenv on Windows: the port, the decisions it forces, and the launch

**Status:** IN PROGRESS — W0, W1 and W2 done 12 Sep 2026: both crates compile for Windows,
`scripts/windows-check.sh` runs inside `verify.sh`, and the Windows x64 artifacts are pinned
and swept. **The owner ruled D1, D2, D4 and D6 on 13 Sep 2026** (D3 on 12 Sep); D5's signing
waits on an unsigned-installer measurement. D1's supervision and worker count, and the owner's
four pre-W3 questions, are answered in writing in §3 and §3a; W3 starts with §3a's step 0.
§3b ruled the same day: a separate signed document per OS (`manifest-<os>.json`), with a
publisher guard and a reader test landing before any non-macOS entry is published. MariaDB
is out of v1 (D4). The only open decision left is D5's signing, which waits on its measurement.
Planned against `e0d287c`. Open work is tracked as the
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

**D1 — PHP process model (Windows has no php-fpm).** **RULED 13 Sep 2026: accepted — one
supervised php-cgi *group* per PHP minor replaces "one php-fpm pool per PHP version" — with
(a) supervision and (b) the worker count settled in writing here, before W3, rather than
discovered in W4.** Official Windows PHP ships `php-cgi.exe`, no php-fpm.

**Correction, from php-src the same day.** This section used to say `PHP_FCGI_CHILDREN` does
not fork workers on Windows, so rexenv would spawn N workers itself, one port each. **Wrong.**
`sapi/cgi/cgi_main.c` and `main/fastcgi.c` (PHP-7.4 and PHP-8.3 branches, read 13 Sep 2026)
carry a Windows arm: started as `php-cgi.exe -b 127.0.0.1:<port>` with
`PHP_FCGI_CHILDREN=N`, the process binds the port once (`fcgi_listen`, backlog 128 or
`PHP_FCGI_BACKLOG`), then becomes a **parent** that `CreateProcessW`s N children (capped at
64) on its own command line, handing each the listening socket as its stdin with
stdout/stderr invalid. A child therefore sees `fcgi_is_fastcgi()` true, **ignores `-b`**
(`case 'b': if (!fastcgi)`), detects a socket rather than a pipe (`!GetNamedPipeInfo`) and
`accept()`s on the shared socket — one accept queue, so the next connection goes to whichever
child is idle. The parent puts every child in a Job Object with
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` and loops on `WaitForMultipleObjects`, respawning any
child that exits (`PHP_FCGI_MAX_REQUESTS`, a crash). That is php-fpm's master in miniature,
**read from source and not yet run**: every *measure* below is W4's first hour on the Dell,
before anything is built on it.

*(a) Supervision — who is master, who supervises while the app is closed, what identifies ours.*
- **Master = php-cgi's own parent, one per minor, on the minor's existing pool port**
  (`9700 + major*10 + minor`). No new port block — the old draft's per-worker ports go with
  its per-worker processes. rexenv supervises the PARENT exactly as it supervises the php-fpm
  master on macOS; the parent supervises its children. With the app closed, children still
  come back after `PHP_FCGI_MAX_REQUESTS` because the parent respawns them, not rexenv — the
  same reason a macOS pool survives a quit.
- **Outliving the app.** rexenv spawns the parent detached, hidden (`CREATE_NO_WINDOW`) and
  **outside any job the app itself sits in** (`CREATE_BREAKAWAY_FROM_JOB`) — a terminal or
  IDE that launched rexenv inside a kill-on-close job would otherwise take every pool down
  with the app. The parent gets a real stderr (the pool log): started with stdout and stderr
  both invalid and stdin valid, it would take itself for a child. *Measure:* whether
  breakaway is allowed from the launch contexts that matter (Explorer, Start, the logon task,
  Windows Terminal). Where it is refused, the start fails loud and names why; it never
  silently becomes a pool that dies with the app. **One context measured, 14 Sep 2026: a Task
  Scheduler task REFUSES it.** A task with an interactive logon and RunLevel Limited (the shape
  `scripts/probes/windows-limited-token.sh` registers, default settings) ran
  `windows_browser_lock_check`; `proxy::start` came back ACCESS_DENIED with the start error written
  for exactly this ("…started inside a job that forbids its services to outlive it…"). An SSH session
  allows it (#600). So rexenv must not be LAUNCHED by a scheduled task — autostart stays the HKCU Run
  key (W7), and W6's logon task may run the DNS agent (it spawns no service) but never the app.
  Explorer, Start and Windows Terminal remain unmeasured. **Explorer measured 15 Sep 2026 (W7 S4's sign-in,
  ledger #623):** the real app started by `explorer.exe` from the Run value at sign-in started Mailpit, and
  Mailpit kept running after the app quit — breakaway is allowed there. Start and Windows Terminal remain
  unmeasured.
- **Orphan workers — closed by the OS, with one hole.** The children live in the parent's
  kill-on-close job: the parent dies by any means, `TerminateProcess` included, the job
  handle closes, and Windows kills the children. The macOS class (a SIGKILLed master leaking
  workers that hold the port) cannot happen — **unless `AssignProcessToJobObject` fails,
  where php-cgi only prints to stderr and keeps the child.** rexenv reads the parent's
  stderr, and "unable to assign child process to job object" fails the start: the group is
  stopped and the line quoted. The orphan sweep stays anyway; it is what shows the hole stays
  closed.
- **Positive identification — never a bare pid or port.** The group is ours only if all four
  hold, the macOS `owned_listeners` / `owned_master` shape with Windows sources:
  1. a listener on our pool port (`GetExtendedTcpTable` → owning pid);
  2. that process's image (`QueryFullProcessImageNameW`) is the cached `php-cgi.exe` for
     this minor under our app-data `bin\`;
  3. its command line carries our marker, `-c "<app-data>\config\php-cgi-<minor>.ini"` — the
     role php-fpm's rewritten title plays on macOS. Children run the parent's command line
     verbatim (`GetCommandLineW`), so the same marker identifies them;
  4. the master is the root of that set: a member whose parent is also a member is a child.
     Windows keeps a dead parent's pid in the child's record and reuses pids, so a parent pid
     counts only if that process was created before the child (`process_start_token` on
     Windows = creation time).
  *Measure:* which pid the TCP table reports for a listener its creator handed to children
  (expected: the parent; the root rule in 4 gives the same answer either way), and that
  reading a same-user process's command line needs no elevation.
  **Measured 14 Sep 2026 on the Dell** (official PHP 8.3.32 NTS x64, checksum matched,
  `php-cgi.exe -n -b 127.0.0.1:<port>`, `PHP_FCGI_CHILDREN=4`, `PHP_FCGI_MAX_REQUESTS=3`, over
  SSH — the elevated token; the command-line read under the desktop token stays open):
  - one parent + 4 children; **the TCP table names the PARENT alone as the listener**; every
    child's command line is byte-identical to the parent's — the marker identifies all five;
  - 10 sequential FastCGI requests were answered round-robin by the four children, and each
    child was replaced by a new pid after its 3rd request; killing one child → a replacement
    within 2 s;
  - 6 parallel 2-second requests against 4 children finished at 2004, 2004, 2004, 2005, 4020,
    4020 ms — the excess QUEUES, it does not fail: the "slow page, then 504" D1(b) predicts;
  - idle child (`-n`, no extensions): ~6 MB private, ~11 MB working set;
  - **`TerminateProcess` on the parent → all four children gone within 2 s, the port free** —
    the kill-on-close job holds; the parent's stderr stayed empty (no "unable to assign");
  - preflight `php-cgi -n -m`: exit 0 in 185 ms. **A missing extension ALSO exits 0**, with only
    `PHP Startup: Unable to load dynamic library …` on the output — so the preflight must read
    the output for that line; the exit code alone would pass a broken ini.

*(c) The pool's contents — RULED 14 Sep 2026 (owner), after comparing the builds:*
- **Extensions: the 26 the official zip ships that rexenv's macOS build also has** — bz2 curl dba
  exif fileinfo ftp gd gmp imap intl mbstring mysqli opcache openssl pdo_mysql pdo_pgsql
  pdo_sqlite pgsql shmop soap sockets sodium sqlite3 sysvshm xsl zip (plus what PHP compiles in).
  pcntl, posix, sysvmsg and sysvsem do not exist on Windows. The PECL ones the macOS build carries
  — apcu, imagick, redis, event, swoole, protobuf, opentelemetry — are NOT in v1: each would be a
  third-party DLL pin, a trust decision and a sweep target; Settings says so honestly, D4's shape.
- **Structure: the platform names the pool MODEL, `core` renders both.** A
  `PoolModel::{Fpm, CgiGroup}` answer from the platform; the php-fpm conf and the php-cgi ini + env
  are both `core` renderers tested on every host, and the pool lifecycle (ensure, adopt, reap,
  stop) stays one implementation.
- **The PHP CLI gets its extensions from a `php.ini` beside `php.exe` in the resolved tree
  (RULED 14 Sep 2026, owner).** php.exe with no ini loads no extension; PHP reads the ini in its
  own folder by default, so every CLI spawn — WP-CLI, Composer, artisan, Adminer, the terminal, and
  any spawn not yet written — gets the same extensions the group loads. Written by `core` at the
  one resolve every PHP tree passes through. Chosen over `-c`/`PHPRC` at each call site, where a
  forgotten spawn would run without extensions silently.
- **No CA bundle for now (RULED 14 Sep 2026, owner).** Measured on the Dell: `file_get_contents`
  over HTTPS succeeds with no CA configured (PHP's openssl stream uses the Windows store); curl
  without a CA fails `unable to get local issuer certificate` and succeeds with
  `CURLSSLOPT_NATIVE_CA`, which only a handle can set. WordPress, WP-CLI and Composer carry their own
  bundles, so W4's path should not need one; a plugin's bare curl call will fail — a TODO row.
- **The ini is rexenv's own:** `php-cgi -n -c <config>\php-cgi-<minor>.ini` — `extension_dir`, the
  extension lines, the user's settings, the SMTP keys for mail; everything else PHP's built-in
  defaults, as the macOS static build runs with no php.ini at all. Not a copy of
  `php.ini-development`, whose values would differ from macOS and move with every patch.
- **Adopt on relaunch:** `adopt_startup` finds the group by 1–4 and adopts the master. A
  master whose children do not answer is not running — ownership AND liveness.
- **Hazards, written down now:**
  - **The respawn loop has no backoff — designed now, built in W4** (owner asked 13 Sep 2026
    whether W3 would catch it; it would not, since W3 has no php-cgi, and W4 would have
    discovered it). Read again in the source first, and narrower than first written:
    `cgi_sapi_module.startup` — ini parsing and extension loading — runs in the PARENT
    (`cgi_main.c:1878`) before any child exists (`:2084`), and children run the same binary
    with the same ini. So a bad ini or an extension that will not load fails, or warns, in
    the parent, once. The spins that remain: (1) `CreateProcessW` itself failing — Defender
    or another scanner blocking the child, a commit-limit or handle failure — leaves a NULL
    in the handle array, `WaitForMultipleObjects` then fails immediately, and the loop spins
    at full speed printing "unable to spawn"; (2) a child that dies on its first request,
    which respawns once per request rather than spinning, but serves nothing. The design,
    three parts:
    - *Preflight, before the group exists:* run the same `php-cgi.exe -c <ini>` once without
      `PHP_FCGI_CHILDREN` and with `-m`, stdout and stderr captured, under a timeout. Exit 0,
      the modules the pool needs listed, and no `PHP Startup:` / "Unable to load dynamic
      library" line — or the start is refused with that text quoted. One process, so it can
      never loop. (*Measure:* that `-m` exercises the same startup path the pool takes.)
    - *Churn breaker, after the start — measured, ruled and built 14 Sep 2026 (ledger #605).*
      First written as "a child younger than a few seconds replaced more than 2 × workers times
      in 10 s, or `unable to spawn` in the parent's stderr at all". The Dell measured that shape
      wrong before it was built (`examples/windows_cgi_churn_probe.rs`, PHP 8.3.32, 10 workers,
      500 requests each):
      - **Idle:** 10 children, one process-table read 8 ms, the parent at 0.0% CPU. php-cgi
        answers a FastCGI `GET_VALUES` record itself (`FCGI_MAX_CONNS 1`, `FCGI_MAX_REQS 1`,
        `FCGI_MPXS_CONNS 0`), so a serving check that runs no script exists.
      - **Legitimate churn is not rare:** one client sending 1722 requests a second for 15 s
        recycled 45 children — about 30 per 10 s, already past "2 × workers" — with the parent at
        0.9% CPU. One look 10 s in saw 10 new children: a 10 s watchdog can never count past the
        worker count, so the count threshold was both wrong and unmeasurable from where it runs.
      - **A script that kills its own worker is not a loop:** 75 requests, 70 children born, the
        parent at 0.6% CPU, a healthy script answering 15 of 15 meanwhile, 10 children 2 s later.
        Stopping the group for it would take every site on that minor down for one broken page.
      - **A worker that cannot be spawned spins** (php-cgi.exe renamed under a running parent, one
        child killed): the parent at 96.8% of a core, 14 045 `unable to spawn: [0x00000002]: The
        system cannot find the file specified` lines and ~1 MB of log a second — while the port
        still accepted and the 9 surviving children still served, so no serving check sees it.
      **Rulings (owner, 14 Sep 2026):** the signal is the PARENT's CPU — at least 25% of one core
      over a window of at least 5 s (`php_cgi::SPIN_CPU_SHARE`, `SPIN_MIN_WINDOW`); a worker-killing
      script does not stop the group; a tripped group is stopped and reported `gave-up` with the
      last `unable to spawn` line from its OWN output — one output log per minor, where every minor
      had shared one file — and is never restarted automatically, since a respawn would spin again.
      *Done when (W4):* on the Dell, a group whose worker cannot be spawned is stopped within two
      watchdog ticks with that line in the reason and no php-cgi left, while legitimate churn and a
      worker-killing script leave it serving.
    - *A serving check, not a process check:* readiness and health are a FastCGI round trip,
      so processes alive with nothing answering is "not running" — D1's ownership-AND-liveness
      rule. Measured since: a worker-killing script leaves the other workers answering, so it is
      not this case; the pool's health probe is still a TCP connect (`services::fpm_running`), and
      `GET_VALUES` is the script-free round trip to replace it with. **Measured 14 Sep 2026, before
      building it** (`examples/pool_get_values_probe.rs`, php-fpm on the Mac and the php-cgi group on
      the Dell, 10 workers each): idle, `GET_VALUES` answered in 0 ms on both; with 12 requests
      sleeping 12 s, **neither answered within 3 s** while the TCP connect still succeeded — all 12
      requests then completed in 24 s, so the pool was busy and healthy, not dead; after the sleeps,
      0 ms again; a pool frozen with `SIGSTOP` (macOS) also did not answer. PHP answers the record
      inside `fcgi_read_request`, after a WORKER's `accept()` (main/fastcgi.c), so "no answer" means
      "no free worker", and busy and frozen look the same. A miss on `GET_VALUES` alone would restart
      a pool in the middle of an import. **Ruled (owner, 14 Sep 2026): the busy-workers signal of
      D1(b) first, then `GET_VALUES` as health — a no-answer counts as a miss only while the pool is
      NOT busy.** **"Busy", measured the same day** (the probe, now printing
      `ProcessSupervisor::established_on` — ESTABLISHED connections whose LOCAL end is the pool's
      port): idle 0 on both; with 12 sleepers the Dell's table counted **12** at 2 s and at 8 s (the 10
      held AND the 2 queued), while macOS `lsof` counted **4** at 2 s and 10 at 8 s — only accepted
      connections belong to a process, and php-fpm's `pm = dynamic` was still spawning workers, with
      `GET_VALUES` unanswered throughout. So "established ≥ workers" would have read the ramping php-fpm
      pool as not busy and counted a miss. The health gate is therefore **no answer AND nothing
      held** (`php::pool_serving`, #607): any held connection spares the pool; the "all workers busy"
      display of D1(b) keeps its own ≥-workers rule. Cost, accepted: a frozen pool that is still holding
      a stuck request is not restarted (nginx's 504 is what the user sees).
  - `fcgi_listen` sets **`SO_REUSEADDR`**, so php-cgi can "start" on a port another program
    holds and another program can bind ours. That is one case of the general rule in §6 — a
    successful bind proves nothing on Windows; who answers does — which `ensure_free`, every
    readiness check and D2's :53 all follow.
  - **Stop is `TerminateProcess` on the parent**: the children have no console, so php-cgi's
    Ctrl+C handler is unreachable, and in-flight requests are cut. Acceptable for local
    development; ARCHITECTURE says so when W4 lands.
  - nginx on Windows is `select()`-based (upstream calls the build beta) — fine for one
    developer, recorded so a later "slow" report checks it.

*(b) Worker count — measured or assumed, and can a user tell?*
- **"4" was assumed.** Nothing measured it. The pool it replaces is `pm = dynamic`,
  `pm.max_children = 10` (`core/services.rs:127`), so 4 would have been a Windows-only
  downgrade nobody chose. And `PHP_FCGI_CHILDREN` is static: N processes always, no spare
  range.
- **Why a small N is worse than it looks for WordPress:** a request holds its worker for its
  whole life, and WordPress calls itself. The block editor fires several REST requests in
  parallel on load, WP-Cron's spawn takes a worker, heartbeat polls, and the theme/plugin
  file editor's save makes a *blocking* loopback request while its own worker waits — N=1
  deadlocks that outright, and any N stalls once N requests each wait on a loopback. Excess
  connections do not fail: they sit in the listen backlog, so the user sees a slow page, then
  nginx's 504 after `fastcgi_read_timeout` — which reads as "rexenv is slow on Windows".
- **Default: parity with macOS, 10 per minor**, lowered only by a measurement. *Measure on
  the Dell:* private bytes per idle and per busy child; peak concurrent FastCGI connections
  during a block-editor load of a fresh site and a WooCommerce admin page; 10 × that memory
  against 8 GB with two minors running.
- **Can a user tell it is worker exhaustion? Not today — on macOS either.** php-fpm writes
  "server reached pm.max_children" to its own log and rexenv surfaces nothing (grep, 13 Sep
  2026: `max_children` appears only in the pool template and its test). php-cgi writes
  nothing at all. **The signal, both OSes, with W4:** sample the ESTABLISHED connections to
  the pool port (the TCP table W3's listener lookup reads; `lsof` on macOS) against the
  worker count; sustained at or above it, the Services row says "PHP 8.3: all 10 workers busy
  — requests are queuing" and the health log records it with the site host from nginx's
  access log. *Done when:* N+2 parallel `sleep(5)` requests to a fixture site turn it on, and
  it turns off after. A per-minor worker setting follows only if the Dell's numbers say 10
  is wrong for somebody. **Built 14 Sep 2026 (ledger #608)** — `core::pool_busy`, sampled in the
  Services status poll (only while a view polls), held ≥ workers for two samples in a row to turn on
  and two below to turn off. Measured on the way (`pool_busy_check`, 12 × `sleep(15)` on 10 workers):
  the Dell's table read 12 from the first second and the note was on at 2 s; macOS `lsof` counted one
  more connection a second as `pm = dynamic` spawned workers (2, 3 … 10 at 9 s), on at 9.6 s — so the
  "Done when" `sleep(5)` would never have held all ten php-fpm workers, and the check sleeps 15 s;
  both off at 17 s. The health log's host is **"recently served"**, not the cause: nginx writes an
  access-log line when a request ENDS, so the requests holding the workers are not in the log yet.

**D2 — `.rex` DNS (no `/etc/resolver`).** **RULED 13 Sep 2026: the agent on 127.0.0.1:53 +
one NRPT rule per TLD is accepted; the `hosts` fallback is REFUSED for now.**
Windows routes a suffix to a server with an NRPT
rule (`Add-DnsClientNrptRule -Namespace .rex -NameServers 127.0.0.1`, admin). **NRPT has
no port parameter** (verified against the cmdlet reference, 12 Sep 2026), so the DNS
agent must answer on **127.0.0.1:53**, not 15353 — one UAC prompt for the rules.
*Why the fallback is refused (owner):* `hosts` is an OS-level file every tool on the machine
shares, and rexenv's rule is never to overwrite a file somebody else owns — `/etc/resolver`
got its consent-gated takeover for exactly that reason. And `hosts` has no wildcards, so
subdomain multisite degrades: the fallback would be a different product, not a degraded
mode of this one. `hosts` is a later conversation.
*When :53 is taken:* **refuse**, in the `port_conflict_help` shape — the holder by name (the
process image, plus the service name when the pid is a `svchost.exe`) and what to do about it.
*Measure first (a W6 prerequisite, on the Dell and the VM) — the earlier draft called this
unmeasured and still proposed a fallback for it.* Who holds loopback :53 in each state: a
clean install; Mobile hotspot / ICS (`SharedAccess`) on; Hyper-V with the Default Switch up;
WSL2 running (NAT, and mirrored networking with `dnsTunneling`); Docker Desktop running. For
each, `Get-NetUDPEndpoint -LocalPort 53` and `Get-NetTCPConnection -LocalPort 53` (address
and owning pid → process and service). **And who ANSWERS, not only who binds:** on macOS,
Herd shadow-binds 127.0.0.1:443 with no bind error, and Windows lets a specific-address bind
coexist with another process's wildcard bind unless one side set `SO_EXCLUSIVEADDRUSE` — so
each state also runs `Resolve-DnsName probe.rex -Server 127.0.0.1` against a bound agent
and records which process replied. The agent binds with `SO_EXCLUSIVEADDRUSE`. If no common
state holds loopback :53, the fallback question closes itself.
**Measured 13 Sep 2026, the Dell's clean state:** nothing on UDP or TCP 53. `SharedAccess` (ICS)
is running but idle — its DNS proxy binds only while sharing is on; Hyper-V's `vmms` and Docker
are not installed; WSL is present (`LxssManager`, `hns` running) with **no distribution and no
WSL 2 kernel**, so the WSL states cannot be measured here without installing one. **Installed 14 Sep
2026 at the owner's go** (hotspot and WSL2 NAT ruled in, Hyper-V and Docker out): `wsl --update
--web-download` put WSL 2.7.14.0 (kernel 6.18.33.2-2) in as an MSI + Appx package — then, from the SSH
session, the inbox `C:\Windows\System32\wsl.exe` (10.0.19041) failed with "The file cannot be accessed
by the system" while `C:\Program Files\WSL\wsl.exe` answered, so anything rexenv ever runs against WSL
should not assume the System32 copy works in every session. WSL 2 mirrored networking and
`dnsTunneling` are documented by Microsoft as Windows 11-only (22H2+) — not measured on this Windows
10 machine, and left to the Windows 11 VM.
**Measured, WSL 2 (NAT) with the `Ubuntu` distribution, 14 Sep 2026** (`scripts/probes/windows-wsl-dns53.ps1`
over SSH). **Once WSL 2 is installed, ICS holds UDP `0.0.0.0:53`** — `svchost` hosting `SharedAccess`
(LocalSystem) — before the distribution starts, while it runs and after `wsl --shutdown`; nothing on TCP
53. The same service held nothing at the start of the day, before `wsl --update` and the distribution
install (what bound it — the install, or the WSL NAT network hns created — is not separated). Inside the
distribution `/etc/resolv.conf` names `172.21.144.1`, the `vEthernet (WSL)` address, where that proxy
answered UDP (`www.microsoft.com`) and refused TCP. On loopback it answers nothing: a UDP query to
`127.0.0.1:53` came back port-unreachable. And an agent-shaped bind — `127.0.0.1:53`, exclusive, UDP and
TCP — succeeded in every phase. So the common "WSL is installed" machine has a wildcard :53 holder of
ANOTHER account, and it does not block the agent.
**And the agent ANSWERS beside it** — `windows_dns53_probe` rerun the same evening with ICS holding UDP
`0.0.0.0:53`: the exclusive `127.0.0.1` agent bound and `Resolve-DnsName -Server 127.0.0.1` got the
agent's marker over UDP and TCP. ICS's socket changes the other cases in one way: a same-account UDP
`0.0.0.0:53` bind is now refused (10048) — ICS did not share it — while `[::]` binds, TCP binds and the
agent's `127.0.0.1` bind still succeed; so the exclusive-wildcard case split by protocol (UDP answered by
the agent, whose holder could not bind; TCP by the exclusive holder, the agent refused with 10013). The
refusals stay the two already measured: a `127.0.0.1` holder (10048) and an exclusive wildcard (10013).
**Measured, Mobile hotspot on, 14 Sep 2026** (`scripts/probes/windows-hotspot-dns53.ps1` in the desktop
session, Medium token; the script turned the hotspot on and off itself, both `Success`, and the SSH
link survived). Turning it on added NO :53 row: the same `SharedAccess` socket (pid 4272, UDP
`0.0.0.0:53`) served the hotspot's `192.168.137.1`, answering UDP (`www.microsoft.com`) and refusing
TCP. On loopback a UDP query now TIMED OUT — with WSL alone it had come back port-unreachable — so
while the hotspot shares, ICS takes loopback datagrams and answers none. The agent-shaped exclusive
`127.0.0.1:53` bind succeeded with the hotspot on and after it was off. Not measured: the agent
ANSWERING while the hotspot shares (bound only).
**What D2 now says:** in every state measured on the Dell — clean, WSL 2 installed, WSL 2 running, the
hotspot sharing — the agent's `127.0.0.1:53` exclusive bind succeeds; the one real holder, ICS, is a
wildcard UDP socket of another account that answers the WSL and hotspot adapters, not loopback. The
fallback question has not reopened. Still open: mirrored WSL and `dnsTunneling` (Windows 11 VM), and
Hyper-V and Docker (ruled out of the Dell's measurements by the owner). The §6 matrix
already answers "who answers": a `127.0.0.1` bind wins loopback traffic over a `0.0.0.0` or
`[::]` holder, so a wildcard :53 holder would not stop the agent — only a `127.0.0.1` holder or
an exclusive wildcard would, and those are the ones to refuse by name.
**Measured for :53 itself, UDP and TCP, 14 Sep 2026** (`examples/windows_dns53_probe.rs` on the Dell,
over SSH; every holder a separate process of the same account, answering A queries with its own
marker address; queries through Windows' resolver, `Resolve-DnsName -Server 127.0.0.1 -DnsOnly`,
plus `-TcpOnly`). An agent bound `127.0.0.1:53` with `SO_EXCLUSIVEADDRUSE` bound and answered — on
both protocols — alone, after a `0.0.0.0` holder, before one (the later wildcard still bound, and the
agent still answered), after a `[::]` IPv6-only holder and after a `[::]` dual-stack one. It could
NOT bind after a `127.0.0.1` holder (WSAEADDRINUSE, 10048 — the holder answered) or after an
exclusive `0.0.0.0` holder (WSAEACCES, 10013 — the holder answered). So the rule stands for UDP as
measured, and the agent's bind error tells the two refusals apart. Not measured: a holder that is
ANOTHER account (a LocalSystem service such as ICS's DNS proxy) — that is what the real states show.

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
**Measured 14 Sep 2026 on the Dell — it DOES, with one slash, and the rule stands**
(`examples/windows_edge_probe.rs`, Caddy 2.11.4, the Caddyfile `proxy::generate_caddyfile` writes,
`:443`/`:80`, a site on the local CA's certificate): Caddy splits an address at its FIRST slash, so
rexenv's `unix//C:\…\caddy-admin.sock` named the socket `/C:\…` — Caddy exited with "cannot reuse
socket /C:\… : unix socket is already in use by another process" (its Windows reuse check dials the
path first and reads any error but "connection refused" as in use), and the `|0600` and
forward-slash variants failed the same way. `unix/C:\…|0600` started; the socket file existed; Rust
reached the admin API over AF_UNIX (Winsock `socket(AF_UNIX)` + `connect`, `GET /config/` → 200);
`:443` served the site with the local CA's certificate and `:80` answered 308. Its `caddy reload` /
`caddy stop` then failed for the same slash in rexenv's `--address` ("dial unix /C:\…: An invalid
argument was supplied"). Also recorded: Caddy bound `0.0.0.0:443` and `0.0.0.0:80` (all interfaces —
what raises Windows Firewall's prompt for a desktop user; not measured under that token yet), and
the socket file carried its folder's inherited ACL (SYSTEM, Administrators, the user — full), so the
`|0600` mode does nothing there. Fix: `proxy::admin_address` gives a path that does not start with
`/` one slash, and the macOS string is unchanged byte for byte.
**And under the desktop user's token, the same day** (`scripts/probes/windows-edge-bind.ps1` through
`windows-limited-token.sh`: Medium integrity, not elevated, in the logged-on desktop session): the
pinned `caddy.exe` bound `:443` and `:80` — no elevation, so W5's edge needs no UAC. Bound on all
interfaces (Caddy's default, and rexenv's Caddyfile today) it raised **"Windows Security Alert"**
(`rundll32`, Windows Defender Firewall's allow prompt) on the desktop; bound with `default_bind
127.0.0.1` it listened on `127.0.0.1:443`/`:80` only and no second alert window appeared (the one
counted was the first case's, still open). **Ruled (owner, 14 Sep 2026): on Windows the edge binds
127.0.0.1 only** (`default_bind 127.0.0.1`) — no firewall prompt, sites open from this computer only,
tunnels unaffected (cloudflared dials localhost); macOS keeps binding all interfaces.

**D4 — What ships in Windows v1.** **RULED 13 Sep 2026: accepted as written.**
Official, checksum-lockable artifacts exist for most
of the stack (§4). Redis has no official Windows build; Apache on Windows means Apache
Lounge (a third-party trust decision); Xdebug DLLs must match PHP's NTS + compiler.
*Recommendation:* v1 = Caddy, nginx, PHP 7.4–8.5, MySQL, PostgreSQL, Mailpit, Adminer,
WP-CLI, Composer, cloudflared; FrankenPHP if W4 proves it. Redis, Apache and Xdebug are
refused in CORE on Windows with an honest message — the shape OpenLiteSpeed already
uses (`ensure_server_available_on`) — until each is proven.
**MariaDB — RULED 13 Sep 2026: not in v1.** macOS ships it (bundle pins), there is no
Windows pin, and it stays on the refusal list with an honest message (W10). *Why (owner):*
MySQL 8.4 and 8.0 both ship on Windows, so MariaDB is redundant for v1, and a new pin is a
new trust decision, a notices row and a sweep target. Windows users asking for it is the
evidence that would add it.

**D5 — Signing, installer, updates, distribution.** **RULED 19 Sep 2026: UNSIGNED, and the
question is closed.** The owner: rexenv is open source and earns nothing, so it spends
nothing — the same answer macOS got, for the same reason, and not a judgement about the
cost of SmartScreen. Azure Trusted Signing is out. **This unblocks W11**, which was waiting
on a decision that no longer needs a measurement to make.
*What this does NOT touch:* the updater's signed manifest. That is rexenv's OWN Ed25519 key
signing `latest.json` — free, already how macOS ships, and unrelated to the paid
Authenticode certificate this ruling declines. "Signed" means two different things in this
section and conflating them would read as "no self-update on Windows".
*What survives the ruling, as DOCUMENTATION rather than a decision:* the measurement below.
Every Windows user will meet SmartScreen, so `docs/INSTALL.md` has to say exactly what they
will see and how to get past it — which needs the same numbers, taken after the installer
exists instead of before. It is no longer a W11 prerequisite; it is a W11 output.
**The superseded ruling, kept because the reasoning is still the reasoning:** *RULED 13 Sep
2026: signing is NOT decided — measure first.* NSIS (per-user, no admin to install) and the
distribution line stand as the direction.
*Why not decide now (owner):* Azure Trusted Signing needs an account — a recurring
commitment — and macOS got the opposite ruling: no $99 Developer ID, ad-hoc signing plus
de-quarantine. But the cost of going unsigned is higher on Windows: macOS pays one `xattr`
per install, while Windows shows "Windows protected your PC" to every user on every
download, and an unsigned file's SmartScreen reputation is per hash, so each release starts
from zero.
*Measure (a W11 prerequisite, on the Dell) — the cost falls on every user, so it needs
numbers (owner):* build an unsigned NSIS installer; download it through Edge and through
Chrome, so it carries the Mark of the Web; record, per browser: (1) **the clicks** from the
download finishing to rexenv's first window, each one named; (2) **every message verbatim** —
the browser's download warning and SmartScreen's dialog word for word, with a screenshot;
(3) **whether it can be got past without "More info"** — whether "Run anyway" is on the first
screen or only behind that link, since a user who does not know to click it reads the first
screen as a dead end; (4) whether a second build (a new hash) repeats all of it. macOS's
equivalent was measured as one `xattr` command; this is the Windows number. Then the owner
decides.
*Self-update — P1's refusal reasons re-checked, not inherited* (read against
`docs/archive/PLAN-self-update.md` P1, 13 Sep 2026; the plugin's Windows code path is W11's
to read): (1) the install that `rm -rf`s the app as root through osascript — macOS-only,
does not carry; (2) `restart()` bypassing the ONE quit gate — Tauri's behaviour, not macOS's:
carries; (3) unsigned `latest.json` whose version is not bound to the signed bytes, a
rollback — OS-independent: carries; (4) the signing key forced onto the build machine —
OS-independent: carries; (5) `rustls-platform-verifier` trusting rexenv's own CA — on Windows
it asks CryptoAPI, which reads the CurrentUser Root store W5 installs into: carries. Four of
five hold from the text alone, so rexenv's OWN signed-manifest channel is the expected
answer; what is new is the swap. **That sentence used to read "a running `.exe` cannot be
replaced: stage, exit through the gate, a relauncher swaps and starts" — and its premise
is wrong, measured on the Dell 19 Sep 2026.** A running `.exe` cannot be DELETED or
OVERWRITTEN, but it CAN be renamed, and a new file moves into the vacated name while the
old process keeps running from the renamed file:

| operation on a running `.exe` | result |
|---|---|
| delete | refused |
| overwrite in place | refused |
| **rename away** | **ok** |
| **move a new file into the vacated name** | **ok** |
| the renamed-away process | keeps running |
| delete the renamed-away file while it runs | refused |

So Windows gets the SAME swap macOS has — `AppBundle::swap`'s contract already says
"put the staged bundle at the install path and the installed one in staging… nothing is
deleted on any path", and the last row is why that last clause is not merely tidy there:
the old file cannot be deleted until its process exits. The relauncher is still needed,
but only to RESTART, not to swap.
Distribution: GitHub release + a winget manifest (the Homebrew tap's counterpart).

**D6 — Supported Windows and architectures.** **RULED 13 Sep 2026: accepted** — Windows 11
x64 supported; Windows 10 22H2 best-effort; arm64 runs the x64 build under emulation,
unsupported.
Windows 10 left mainstream support on 14 Oct 2025. Official PHP Windows builds are
x86/x64 only (no arm64), and PostgreSQL's portable build is x64 only.

## 3a. Answered before W3 (the owner's questions, 13 Sep 2026)

**Q1 — How far does a half-ported build get on Windows, and what does it say when it
stops?** Read from the tree, not run — there is no Windows host yet.

- `main` → `run()`: the single-instance claim is `cfg(unix)`, so on Windows **nothing stops
  a second instance** today (W8's named pipe becomes the claim). `mark_app_process`, the
  plugins and the URI-scheme handler touch no stub. `setup` calls `platform::current()` — a
  struct of unit stubs, fine — and the first stub it reaches is
  `platform.paths().log_dir()` (`lib.rs:199`) → `todo!("windows log_dir")`. The `.ok()`
  around it never runs: `todo!` panics, it does not return `Err`. (A login launch reaches
  `dns().resolver_path` one step earlier, same outcome.)
- **What the user sees:** a release build carries `windows_subsystem = "windows"`
  (`main.rs`) — no console — and the tree installs no panic hook (grep `set_hook`: none).
  The message `not yet implemented: windows log_dir` goes to a stderr nobody has. `setup`
  runs inside the event loop's callback, and a panic unwinding out of that `extern "system"`
  frame aborts. Expected, to be confirmed on the first launch: **no window, no process, and
  nothing on screen says why** — at most an Application Error in Event Viewer. A debug build
  (`tauri dev`) has a console and prints the message.
- **A stub reached later, from an IPC command,** panics on a tokio worker. tauri 2.11.3's
  source has no `catch_unwind` (grep), so the task dies and the frontend's promise never
  settles: **a spinner that never ends**, again with no message. That is the worst shape for
  W3–W12, when the half-ported app is run between every task.
- **RULED 13 Sep 2026: accepted as W3 step 0, before any stub is filled** (owner: a silent
  exit is the worst failure mode, and a spinner that never ends is worse still, because the
  user waits): (1) a panic hook installed first
  thing in `main` on every OS: message, location and backtrace appended to `crash.log` in the
  log dir when `Paths` answers and in `std::env::temp_dir()` when it does not, plus a native
  message box naming the file on a Windows release build — a half-ported build fails out
  loud; (2) Windows stubs stop being `todo!()`: one that returns `Result` returns a named
  `Error::Unported("windows log_dir")` reading "rexenv on Windows: … is not ported yet", so a
  half-ported *feature* is an ordinary error toast while the rest of the app runs; one that
  cannot return an error panics through a single `unported!` macro with the same wording,
  which the hook then records; (3) a ledger row — no `todo!()` under `platform/windows/`,
  scan-enforced, and the hook's file write plant-proven. Today: 50 `todo!()` in
  `platform/windows/mod.rs`. **Progress 13 Sep 2026: (2) and (3) landed** — no `todo!()` is left
  there (`Error::Unported`, or `unported!` where a trait method cannot return an error; ledger
  #595). **(1) landed the same day — step 0 is done:** `crash.rs` installs the hook as `main`'s
  first line, every panic is appended to `<log_dir>/crash.log` (temp dir when `Paths` cannot
  answer — exactly the half-ported case), and the first one per process raises a
  `MessageBoxW` on a Windows release build through `platform::fatal_notice` (ledger #596).
  What only Windows can show: that box appearing, and a `Result` stub's `Unported` error
  reaching the screen.

**Q2 — PostgreSQL's publisher digests.** An omission, not an exception; our own downloads now
match all three. §5 W2 and ledger #335.

**Q3 — Which per-version answers must be per-OS?** The "one fact, two places" sweep: every
function the PHP version row, the site guards and the updater consult about a version, read
13 Sep 2026. None takes an OS today. The row the owner called the read-only "exists" row
matched no function by that name; the version row (`PhpVersionView`, `core/php.rs`) is where
these answers meet, and every field it carries is below.

| Answer | Where (callers) | Keyed by today | True on Windows? | Must become |
|---|---|---|---|---|
| PostgreSQL driver present — `pdo_pgsql_supported` → `php_has_pdo_pgsql` | `binaries.rs:866` (`sites.rs:163` refusal, `php.rs:387` switch guard, the row's `postgres_supported`, `oldest_pdo_pgsql_minor`) | the version has a rexenv self-hosted tag, i.e. is one of our macOS builds | **Wrong basis.** The tag exists for the version string, so Windows would say yes for 8.1–8.5 and no for 7.4/8.0 — about php.net's build, which ships `ext/php_pdo_pgsql.dll` for every version and loads it only if our ini enables it | `(os, version)`, answered from the artifact that OS resolves; W4 measures the DLL loading per minor |
| Xdebug available, why not, which version — `xdebug_supported`, `xdebug_unavailable_reason`, `xdebug_version_for` | `binaries.rs:589`–`625` (`service_manager.rs:710` debug pools, `php.rs:252` debug port, `commands/sites.rs:1030`, the row) | minor → a Homebrew `arm64_sonoma` bottle | **Wrong.** Says available for 8.1+, while D4 refuses Xdebug on Windows. The "static build exports no Zend symbols" sentence describes our static macOS builds; php.net's PHP loads DLL extensions | `(os, minor)`; Windows gives D4's refusal, never the static-build sentence |
| curl's resolver — `wp_dns::resolver_for` | `wp_dns.rs:78` (the c-ares exposure count `:109`, its notice `:378`) | minor → the measured macOS builds (7.4 threaded, 8.x c-ares) | **Unmeasured, and the test would lie:** `every_pinned_php_has_a_measured_curl_resolver` passes on Windows with macOS measurements | `(os, minor)`, with a Windows measurement per minor once NRPT exists (W6) |
| Update offered, its cost, the artifact installed — `newer_than`, `artifact`, `update_cost`, `catalog_arch` | `updates.rs:245`–`350` (`commands/php.rs:35`, `commands/database.rs:242`, `binaries.rs:941`, `:1263`) | `arch` only: `arm64` / `x86_64` | **Dangerous.** A Windows x64 host matches the macOS Intel rows: offered a macOS patch, downloads a macOS tarball, the digest matches, the spawn fails | an OS dimension that is **not a new `os` field** — `Artifact` has no `deny_unknown_fields`, so every shipped Intel Mac app would ignore the field and take a Windows `x86_64` row as its own. Measured against every shipped release and proposed in §3b; the owner rules before W3 |
| Cache-marker staleness — `cache_matches_pin` | `binaries.rs:1444` | self-hosted tag per version | Harmless: an unmarked Windows 8.1–8.5 cache re-downloads once | `(os, version)`, with row 1 |
| Pool port / debug port — `fpm_port`, `debug_fpm_port` | `core/php.rs` | minor | The pool port holds under D1(a) (one group per minor); debug pools do not exist there (D4) | pool port unchanged; debug port refused per OS |
| Security-support end — `eol_since` | `php.rs:190` | minor → php.net's lifecycle dates | Yes — a fact about PHP, not about a build | unchanged |
| Upstream has a newer patch — `php_upstream::is_newer` | `core/php.rs` `list_versions` | php.net source releases | Yes as a fact. windows.php.net lags a source release by days, so any sentence around it saying rexenv has "not built" it is macOS wording | unchanged; the copy is checked in W9 |
| Pinned patch per minor — `PHP_VERSIONS` | `binaries.rs` | minor | Yes — W2 pinned the Windows zips at the macOS patches (php.net's `archives/` keeps them), so it is one fact until a signed update moves one OS (row 4) | unchanged until then |

**The rule for W4 onward:** an answer about a BUILD takes `(os, version)`; an answer about PHP
itself stays keyed by version. Each per-OS function gets a test asserting the Windows answer
differs wherever the builds do, so a macOS measurement cannot pass for Windows again.

**Q4 — THIRD-PARTY-NOTICES.** The count drift was **not** in TODO — the file said "due before
the next release" and nothing tracked it. Measured 13 Sep 2026 both directions: no table row
is outside the graph, but the macOS arm64 graph links 409 crates against 395 rows — 14
missing from the app that ships today (`mysql_async` and what it pulls in), 15 on Intel. Now
a TODO row. For Windows: the download sources (php.net's Windows builds, nginx.org, Oracle,
theseus-rs, and the three GitHub releases) are named in the notices, which now say the
"rexenv's own build" sections are macOS-only; the Windows app's own Rust graph (410 crates,
50 beyond the table) needs its table before a Windows release, in that same row.
**Owner, the same day: fix the macOS half now.** Done — 17 rows (the CLI sidecar's graph
added two the arm64/Intel count had not shown), and `scripts/notices-check.py` gates both
tables in `verify.sh` (ledger #592).

## 3b. Update catalogs across OSes — measured against every shipped release (13 Sep 2026)

**Owner, 13 Sep 2026:** settle this before W3, because the answer may change the catalog's
FORMAT, and that has to be negotiated with apps already released. *Measure whether old apps
skip an unknown row or crash; do not assume.*

**What ships today.** Two signed documents on `rexenv/runtimes` `main`, fetched by every
installed app: `manifest.json` — PHP, `php-fpm`, `php-licenses` and Adminer rows keyed by
`name` + `version` + `arch` ∈ {`arm64`, `x86_64`, `any`} (live: serial 5, 37 rows) — and
`app-manifest.json`, ONE `release` object naming the universal macOS `.app.tar.gz` (live:
serial 3). Neither has an OS anywhere. The PHP catalog shipped in 0.3.0 (0.1.1 and 0.2.0
never fetch it); the app descriptor in 0.6.0.

**How it was measured.** Each distinct released version of the parsing code — `updates.rs`
at v0.3.0 (identical in v0.4.0), v0.5.0, v0.6.1 (= v0.6.0) and v0.7.0; `app_update.rs` at
v0.6.1 and v0.7.0 — checked out in a worktree, a probe test added inside that release's own
`tests` module, and run. It signs crafted documents with a test keypair and feeds them to
the release's `verify_with`, the function its fetch path calls once the signature is
checked. Controls: a plain `arm64` row and a plain `x86_64` row are retained by every
release, so a "dropped" below is the filter's doing, not the probe's. Not exercised: the real
key and the network fetch, which none of these shapes touch.

| Shape a Windows entry could take | 0.3.0–0.4.0 | 0.5.0 | 0.6.0–0.6.1 | 0.7.0 |
|---|---|---|---|---|
| A — `"arch":"x86_64"` plus a new `"os":"windows"` field | **kept, as an x86_64 row** | **kept** | **kept** | **kept** |
| E — `"arch":"x86_64"` plus a nested `"platform":{…}` object | **kept** | **kept** | **kept** | **kept** |
| B — `"arch":"windows-x86_64"` | dropped | dropped | dropped | dropped |
| C — `"name":"php-windows"` | dropped | dropped | dropped | dropped |
| D — rows under a new top-level key (`windowsArtifacts`) | ignored; document accepted | ignored | ignored | ignored |
| App descriptor: `"os":"windows"` on `release` | — | — | **accepted as the Mac release** | **accepted** |
| App descriptor: a new top-level `windows` object or `releases` array | — | — | ignored | ignored |

**Reading it.** No release crashes or refuses the document — every shape parsed.
(i) Unknown fields are ignored (no `deny_unknown_fields` anywhere), so **any entry that keeps
`arch: "x86_64"` is taken by every shipped Intel Mac as its own**: offered, downloaded,
digest-verified, then spawned as a macOS binary. The break is already shipped; it fires the
day such a row is published. (ii) An entry whose `arch` or `name` an old app does not know is
silently dropped by `retain(acceptable)`, present since 0.3.0 — the rule that one bad row
cannot deny every other update, working in our favour here. (iii) New top-level keys are
invisible to old apps. (iv) The app descriptor has exactly one `release`, so a Windows release
can never share it.

**Options.**
1. **A separate document per OS** — `manifest-windows.json` and `app-manifest-windows.json`
   (each with its `.sig`) on the same branch, the same key, their own serials. Only a Windows
   build fetches them, and their schema can carry `os` from day one. Old apps never request
   those URLs, so nothing relies on how they filter. Cost: two more files per publish, and
   `check-app-manifest.sh` checks them too.
2. **The same document, arch tokens old apps cannot match** (`windows-x86_64`, and
   `windows-any` for Adminer) — relies on (ii), measured true in all four releases. Cost:
   every future macOS build must keep dropping those tokens (a test), Adminer needs a Windows
   copy of its `any` row, and one document carries both OSes.
3. **The same document, rows under a new top-level key** (`windows: {artifacts: […]}`) —
   relies on (iii). Cost: two schemas in one document, and one shared serial, so a
   Windows-only publish moves every Mac's serial.
4. **Anything that keeps `arch: "x86_64"` and adds an OS marker, or puts `os` on the app
   descriptor's `release`** — **refused by the measurement.** Not a release-ordering problem
   either: an Intel Mac that never updates would stay broken by it for good.

**Recommendation: 1.** It is the only option whose safety does not rest on how released
builds filter rows; each serial keeps meaning one thing; nothing has to ship in a particular
order; and for the app descriptor there is no alternative anyway (iv). Whatever is ruled, two
guards land with it: a test in the current tree pinning (ii) and (iii), so macOS readers keep
dropping foreign entries if one is ever published into their document by mistake, and a
refusal in the `rexenv/runtimes` publisher of any `manifest.json` entry carrying an OS marker.

**RULED 13 Sep 2026: option 1, a separate signed document per OS, with both guards.** *Why
(owner):* it is the only option that relies on nothing about released apps' filtering, and
the measurement shows that filtering wrong in the worst place — an Intel Mac taking
`arch:"x86_64"` + `os:"windows"` as its own is not a crash, it is a Mac downloading a Windows
binary and trying to run it. And an Intel Mac that never updates stays broken forever, so
there is no choice: the separate document must exist before any Windows entry is published.
The publisher guard matters more than the test — it is what stops a future "one document is
simpler" from sending a Windows binary to an Intel Mac.

**Naming, fixed now for three OSes.**

| Document | macOS | Windows | Linux |
|---|---|---|---|
| PHP / Adminer catalog | `manifest.json` | `manifest-windows.json` | `manifest-linux.json` |
| App release descriptor | `app-manifest.json` | `app-manifest-windows.json` | `app-manifest-linux.json` |
| Signature | the document's name + `.sig`, always | | |

- **The unsuffixed names are macOS's, frozen.** Their URLs are compiled into every shipped
  build (`updates.rs` `MANIFEST_URL`, the app descriptor's likewise), so they can never be
  renamed; and there is never a `manifest-macos.json` — a second macOS document would be a
  second serial for the same machines.
- **The suffix is Rust's `std::env::consts::OS`** (`windows`, `linux`) — the token
  `binaries::manifest(name, version, os, arch)` already keys every pin on, so one spelling runs
  from the pin table to the URL. A build picks its URL from that constant in one function;
  it never fetches another OS's document.
- **Inside a per-OS document** rows keep the `arch` tokens `catalog_arch` spells (`arm64`,
  `x86_64`, `any`) and carry a required `os` equal to the document's; a reader drops any row
  whose `os` is not its own, so even a mis-published file cannot cross OSes. Serials are per
  document; the signing key is the same one.
- **Linux specifically:** its `arm64` / `x86_64` collide with macOS's tokens exactly as
  Windows' `x86_64` does — that collision is the reason for the rule, so "one document with an
  `os` field" stays refused for Linux too, and so does any variant of it. If Linux ever needs a
  libc split (glibc / musl), it goes in the arch token *inside* `manifest-linux.json`
  (`x86_64-musl`), never into a shared document.

**Guards, before any non-macOS entry is published** (TODO "Update catalogs across OSes"):
(1) in this tree, tests pinning (ii) and (iii) for the macOS readers, plus a reader change so
FUTURE macOS builds also drop a row carrying any `os` other than `macos` — defence for the
builds we can still change; (2) in `rexenv/runtimes`' publisher, a refusal of any
`manifest.json` / `app-manifest.json` entry carrying an OS marker or an arch outside
`arm64` / `x86_64` / `any`, and of any per-OS document row whose `os` is not the file's — one
publisher run writes one document.

## 4. Upstream availability (measured 12 Sep 2026)

✓ = the asset was listed at the pinned tag or version; *unchecked* = believed to exist, not
looked at yet (W2 measures and hashes every row before it is pinned).

| Binary | Windows artifact | Checked |
|---|---|---|
| Caddy 2.11.4 | `caddy_2.11.4_windows_amd64.zip` (+ `.sig`) | ✓ |
| Mailpit 1.30.3 | `mailpit-windows-amd64.zip`, `-arm64.zip` | ✓ |
| cloudflared 2026.6.1 | `cloudflared-windows-amd64.exe` / `.msi` | ✓ |
| FrankenPHP 1.12.4 | `frankenphp-windows-x86_64.zip` | ✓ (whether it runs our overrides: W4) |
| PostgreSQL 18.6.0 | theseus-rs `x86_64-pc-windows-msvc` `.zip`/`.tar.gz` + `.sha256` | ✓ (all three pinned, and hashed by us — §5 W2) |
| PHP | php.net `php-X.Y.Z-nts-Win32-vs16/vs17-x64.zip` + `sha256sum.txt`; current dir lists 7.4.33, 8.0.30, 8.1.34, 8.2.33, 8.3.33, 8.4.25, 8.5.10; **no arm64** | ✓ — note 8.2/8.3/8.4/8.5 patches differ from the macOS pins, so `php::pdo_pgsql_supported`'s per-PATCH answer must be per-OS too (W2 then pinned the Windows zips at the macOS patches from `archives/`; the per-OS sweep is §3a Q3) |
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
  binaries, while their Windows zips are trees; (4) the arms, all x64 artifacts (Windows ARM runs
  them under emulation — there is no arm64 PHP, PostgreSQL or MySQL build; three images inside
  them turned out x86 when W3 measured every file — `nginx.exe`, PHP 7.4's ICU data DLL, MySQL
  8.4's configurator — ledger #598); (5) the
  sweep. Every Windows artifact except PostgreSQL's was downloaded and hashed on 12 Sep
  2026 (PostgreSQL's three were pinned from the publisher's `.sha256` and only streamed for
  their layout — see (5)); Caddy's SHA-512 matched its publisher's, the rest have no published
  digest (php.net's archive has no `sha256sum.txt`, Mailpit and nginx.org publish none,
  MySQL publishes MD5). **(2)–(4) landed the same day:** `exe_name` publishes a single
  binary as `name.exe` on Windows, `shape_of_on(name, os)` makes `php`/`nginx` trees there,
  and the seven arms are in with PORTS.md's Windows table. A test that asserted Windows
  had NO Caddy arm (`manifest_unknown_is_none`) failed on cue and now asserts an unknown
  version and an arm-less OS instead. The PostgreSQL tarball's layout
  (`postgresql-<v>-x86_64-pc-windows-msvc/bin/postgres.exe`) was confirmed by streaming it.
  **(5) the sweep, and W2 is done:** `manifest_sweep_check` enumerates the Windows x64 set
  (one target per pin — both `Arch` values resolve the same URL) and PASSED on 12 Sep 2026:
  104 targets answering, 88 re-hashed. Windows: Caddy, nginx, Mailpit and the seven PHP
  zips re-hashed and matching; cloudflared, both MySQL zips and the three PostgreSQL
  tarballs are over the 40 MB cap and HEAD-only — named. PostgreSQL's digests were the
  publisher's alone until **13 Sep 2026, when the owner asked whether that was a deliberate
  exception** to the macOS rule (a published sum is documentation; the pin is ours). It was
  not — an omission. All three tarballs were then downloaded in full and hashed: 18.6.0
  `7da44c2d…`, 17.11.0 `a013f0e0…`, 16.15.0 `157bd732…` — each MATCHES the `.sha256`. The
  cap stays: over-cap trees are HEAD-only on macOS too, so raising it for Windows alone
  would be the exception; what the ledger now says is WHICH hash a pin rests on and when
  ours was taken (#335). **What W2 does not prove:** that any of these runs.
  `resolve` on a real Windows host still calls `set_executable` / `prepare_binary`, which
  are unported there — that is W3 (since step 0 they return `Error::Unported`, not `todo!()`).
- **W3 — Foundations.** **Step 0 first (§3a Q1): the panic hook and `Unported` stubs, so every
  build from here on fails out loud.** Then `Paths` (`%LOCALAPPDATA%\rexenv\rexenv\data` —
  measured 13 Sep 2026 in `directories` 5's source: it resolves the Local AppData known folder
  and drops the qualifier on Windows; the draft's `%LOCALAPPDATA%\rexenv` was a guess),
  `PermissionManager` (owner-only ACLs — **written 13 Sep 2026, compile-checked from the Mac,
  not yet run:** a protected DACL with one full-access entry for the process's user — no SYSTEM entry, by
  owner ruling the same day — set
  through the file handle, and `write_private` creating the file WITH that descriptor;
  ledger #597), `BinaryProvider` (strip the `Zone.Identifier` stream, no codesign — **written
  13 Sep 2026, compile-checked:** the stream is removed if present, and every image must be one
  x64 Windows can run — x64, or x86 under WOW64 — before it is published; the header parser is
  L0-tested and ran against a real cross-built `rex.exe` → x64; ledger #598. **Owner ruling the
  same day: directory trees are checked too** — a new `BinaryProvider::prepare_binary_dir`, a
  no-op on macOS (MySQL stays Oracle-signed, untouched), called by `resolve_dir` before publish.
  Measuring every Windows artifact first changed the rule: 3 of 1,197 images are x86
  (`nginx.exe`, PHP 7.4's ICU data DLL, MySQL 8.4's configurator), so an x64-only check would
  have refused nginx, PHP 7.4 and MySQL 8.4 outright),
  `ProcessSupervisor` (hidden + detached spawn so services OUTLIVE the app, graceful
  per-service stop, pid → exe/cmdline for ownership, listener lookup via
  `GetExtendedTcpTable`, conflict help naming HTTP.sys and the Hyper-V excluded port
  ranges). *Done when:* MySQL and Mailpit start, survive an app quit, and are adopted on
  relaunch — on a real Windows machine.
  **Measured before spawn/stop is written — the Dell, 13 Sep 2026, pinned MySQL 8.4.6 and
  Mailpit 1.30.3 (checksums matched), fixture ports, all processes removed afterwards:**
  - *An SSH session runs inside a job with `KILL_ON_JOB_CLOSE` and `BREAKAWAY_OK`.* Two Mailpits
    spawned from one session through `CreateProcessW`: the one with `CREATE_BREAKAWAY_FROM_JOB`
    was still listening from a NEW session; the one without died with the session. So
    "outlives the app" depends on the flag whenever the launcher sits in such a job — and the
    SSH session is a real fixture for proving it.
  - *MySQL 8.4 on Windows is TWO processes by default.* `mysqld` starts as a restart MONITOR
    (`sql/restart_monitor_win.cc`) that `CreateProcess`es the real server as its child — no job
    object — and waits on it. Measured: spawned pid 8660 = monitor; the listener on the port =
    child 9308. `TerminateProcess` on the monitor left the child alive and LISTENING — the macOS
    orphan-worker class, on Windows. The child's clean-shutdown event is named after the MONITOR:
    `mysqld8660_shutdown` existed, `MYSQLShutdown<pid>` did not; `SetEvent` on it → "Normal
    shutdown … Shutdown complete" in 571 ms.
  - *With `--no-monitor` it is ONE process:* spawned pid = listener = 12056, event
    `MYSQLShutdown12056`, `SetEvent` → clean shutdown in 906 ms. The monitor only exists to
    serve the SQL `RESTART` statement; rexenv supervises the server itself.
  - *Mailpit:* one process, the listener is the spawned pid; `TerminateProcess` → exited in 110 ms,
    port released.
  - Rust's `Command` on Windows appends `.exe` to an absolute path that lacks it when that file
    exists (`resolve_exe`, std 1.93.1), so `core`'s `bin/mysqld` needs no per-OS name.
  - Go programs (Mailpit, Caddy, cloudflared) turn CTRL_C and CTRL_BREAK into SIGINT
    (`runtime/os_windows.go`) — a graceful path for them exists, not measured, and needs a shared
    console to deliver.
  **Rulings on these measurements (owner, 13 Sep 2026):** MySQL on Windows runs with
  `--no-monitor` (one process; the flag reaches `core::database::start` through
  `ProcessSupervisor::mysqld_supervision_args`, since macOS's `mysqld` has no such option), and
  `stop` asks a process through its own clean-shutdown channel with a grace, then
  `TerminateProcess` — the event for `mysqld`, termination for everything else until a
  service needs more (Go's CTRL_BREAK path waits for Caddy, W5). Written the same day:
  `platform/windows/stop_policy.rs` (the sequencing, tested on every host) +
  `process.rs::Stoppable` (holds the process handle for the whole stop) + `WindowsSupervisor`'s
  spawn family; ledger #600. The Dell run of the "Done when" is
  `examples/windows_supervision_check.rs` (two phases, two SSH sessions). **MET 14 Sep 2026**
  (phase 1 PASS and its session closed by itself in 20 s; phase 2 PASS). It took a third fix the
  measurements above did not predict: the services inherited ~20 handles the launching process was
  born with from sshd and held its session open until they were stopped — found by reading the
  whole handle table while it hung, fixed by clearing every inheritable handle before a service
  spawn (`windows/handles.rs`).
- **W4 — Serve a WordPress site.** **nginx measured first — the Dell, 14 Sep 2026** (official
  nginx 1.30.4 zip, checksum matched, a prefix with a space, a php-cgi group behind it):
  - **Backslash paths break the config:** inside a quoted nginx string `\r` and `\n` are escapes —
    `…\rexenv-probe-w3\ngx prefix\nginx.pid` came out `exenv-probe-w3 / gx prefix / ginx.pid`
    and `nginx -t` failed; the same config with FORWARD slashes passed. Every path rexenv writes into
    nginx.conf must be forward-slashed — `display()` alone is wrong there.
  - **nginx is a master and a worker with byte-identical command lines, and the socket table names
    the WORKER as the listener** — so a root-of-the-listeners `owned_master` adopts the worker.
  - **`TerminateProcess` on the master leaves the worker alive, listening and serving** — the
    orphan-worker class again. `nginx -s quit` ended both in 331 ms; the master publishes
    `ngx_quit_<pid>`, `ngx_stop_<pid>`, `ngx_reload_<pid>` and `ngx_reopen_<pid>` events.
  - `-s reload` exited 0 and replaced the worker; static and PHP-through-nginx both served;
    PHP answered with and without `REDIRECT_STATUS` — FastCGI mode does not enforce
    `cgi.force_redirect`, so the template needs no new param.
- **W4 (cont.)** D1's php-cgi group — its measurements first (§3 D1(a)), then the group, its
  positive-ID chain and D1(b)'s busy-workers signal; nginx Windows config (forward
  slashes, every path quoted), mail through the SMTP ini keys, WP-CLI/Composer via the
  site's PHP. *Done when:* a one-click WordPress site loads through nginx and its mail
  lands in Mailpit. **MET 14 Sep 2026 on the Dell** (`examples/windows_wp_site_check.rs`, ledger #606,
  24 checks): MySQL, Mailpit and the 8.3 group with the catch on; `sites::provision`;
  `install_for_site` through `php.exe` and the pinned WP-CLI in 103 s, `wp core verify-checksums`
  passing; `rebuild_configs` and the shared nginx serving the homepage (200, the site's title) and the
  login form to requests shaped as the edge sends them; a password-reset mail from a page request
  (the group's SMTP keys) and a `wp eval wp_mail()` (WP-CLI's `sendmail_path` shim) both in Mailpit;
  every port free after the stops. **Found by the first run:** the site-folder check refused every
  Windows path, judging `\` across the whole string — now judged per folder name (#303). **Arrived,
  not yet trusted:** WP-CLI's mail rode `mail::sendmail_path_cli`, which escapes for `/bin/sh`; it
  delivered from `C:\Users\DELL\…`, and **with a space in the path it is lost while `mail()` answers
  `true`** — measured the same day (`windows_cli_mail_probe`: the sh-escaped space and a quoted path
  both failed in cmd.exe; PHP's `SMTP`/`smtp_port` keys delivered with no shell involved). **Fixed the
  same day (#407):** on a php-cgi platform WP-CLI gets the keys and no `sendmail_path` flag at all — an
  emptied `-d sendmail_path=` is `""` to PHP, not NULL, and lost the mail with `true` again (measured). The edge
  (W5) and `.rex` names (W6) are not part of this proof. **Composer through the site's PHP — done
  14 Sep 2026 (#609), which closes W4's list.** Found first: Windows had no streamed step
  (`spawn_streamed`/`stop_group` answered Unsupported) and no user environment (`login_shell_env`), so
  no Laravel or Git site could be provisioned at all. **Ruled (owner):** the environment is read fresh
  from the registry (the system's and the user's `Environment` keys over the process's variables, the
  user `Path` appended), and a step dies with rexenv (a kill-on-close Job Object; macOS lets it
  run on). Measured on the Dell (`windows_streamed_step_check`, 14 checks): a value written to
  `HKCU\Environment` after launch was in the environment; cancel ended the step and the grandchild it
  started in 0.2 s; the idle limit killed both; a launcher exiting without stopping its step took both
  with it; `composer create-project laravel/laravel` through `php.exe` installed Laravel in 112 s.
- **W5 — HTTPS edge.** Caddy on :443 (Windows has no privileged ports, so the edge need
  not run elevated — record why in ARCHITECTURE), admin per D3, `EdgeSupervisor` for that
  shape; `CertTrustManager` into the CurrentUser Root store (Windows shows its own
  confirmation) plus the Firefox enterprise-roots path. *Done when:* `https://<site>.rex`
  shows a valid lock in Edge, Chrome and Firefox.
  **Edge start path — done 14 Sep 2026 (ledger #611).** `PrivilegeManager::port_needs_privilege`
  (default below 1024; Windows false) decides `EdgePlan::privileged`, so Windows takes the
  `proxy::start` child branch; `WindowsEdge` has no supervisor (`is_installed` false) and a
  `default_bind` of `127.0.0.1` (the owner's ruling, written into the Caddyfile's global block);
  `WindowsLocalIpc::connect` dials AF_UNIX, telling `NotFound` from `ConnectionRefused` by the socket
  file. Measured on the Dell (`windows_edge_start_check`, 28 checks, PASS first run): unprivileged plan;
  `admin_alive` true over AF_UNIX; `:443`/`:80` on `127.0.0.1` only, the LAN address refusing; TLS on
  the local CA; a second manager adopted the live edge and reloaded it in place; after `taskkill /F`
  the socket file stayed, the connect said `ConnectionRefused`, and a fresh start came up over it;
  `stop_all` released both ports. Remaining for W5: `CertTrustManager`, Firefox, the three browsers.
  **Firefox's profiles root — done 14 Sep 2026 (ledger #612).** `WindowsCertTrust::firefox_profiles_root`
  = `%APPDATA%\Mozilla\Firefox` (Roaming, via `directories`) when a `profiles.ini` file is there. The
  Dell's `profiles.ini` turned out UTF-16LE with a BOM — `core::firefox` read it as UTF-8, found no
  profile and would have told Settings Firefox was absent — so `firefox::ini_text` settles the
  encoding. Measured (`windows_firefox_profiles_check`, 12 checks): the real root and profile found
  without writing; the installed Firefox 105 (below 120, so the pref defaults off), headless on a
  fixture profile, saved `security.enterprise_roots.enabled` true from rexenv's `user.js` and did not on
  a control. Still open for Firefox: whether it then trusts rexenv's CA from the CurrentUser Root store
  — that waits on `CertTrustManager`; the Store build's folder.
  **`CertTrustManager` — in, half measured 14 Sep 2026 (ledger #613).** `cert_store.rs` calls CryptoAPI
  in-process: `CertOpenStore` on the CurrentUser `Root` system store, `CertAddEncodedCertificateToStore`
  (Windows then asks the user), `CertFindCertificateInStore(CERT_FIND_EXISTING)` for `is_trusted` (the
  logical view, machine roots included, no prompt), `CertDeleteCertificateFromStore` for untrust (asks
  again). Already present → Ok without a prompt; absent on untrust → Ok. **Measured from the SSH
  session** (owner's ruling: measure both ways): the add returned `ERROR_NOT_SUPPORTED` (0x32) in 0.0 s —
  no hang and no silent add; the store held no rexenv certificate before or after. So the prompt is
  Windows' own and needs an interactive desktop; rexenv words 0x32 as that. **Measured in the desktop
  session, the same day** (Task Scheduler, interactive logon, Medium token; the owner at the Dell
  answering Yes to both prompts): `trust_ca` returned Ok after 6.6 s and `is_trusted` was true; a
  second `trust_ca` returned Ok in 0.0 s (no prompt); `untrust_ca` returned Ok and `is_trusted` was
  false. No elevation is needed. The untrust's elapsed time read 3883.7 s, past the check's 180 s
  deadline, while the Dell was unreachable over both the tunnel and the LAN — not explained. **Every
  answer, the same evening** (`windows-cert-trust-answers.ps1`; the owner answered No, Yes, No, Yes):
  a No to the install and a No to the delete each came back as rexenv's cancel wording and left the
  store as it was; each Yes did what it said. The prompts are Windows' own, shown in rexenv's process
  (the check's watcher found them among its own windows): "Security Warning" — "You are about to install
  a certificate from a certification authority (CA) claiming to represent: rexenv Local CA … Thumbprint
  (sha1) …" — and "Root Certificate Store" — "Do you want to DELETE the following certificate from the
  Root Store?" with the subject, validity, serial and thumbprints. They name the CA, not the app; there is
  no hook to reword them.
  **W5's done-when — measured 14 Sep 2026 (ledger #614).** `windows_browser_lock_check` in the Dell's
  desktop session (Medium token; the owner answered Yes to both prompts): the Caddyfile rexenv writes
  served `lockcheck.rex` on the local CA's leaf; Edge 153, Chrome 152 and Firefox 105 ran headless, each on
  a fresh profile, three times. Before `trust_ca` none of them sent its request (a certificate the
  browser rejects never gets one); after it all three fetched the page and the same-origin image it
  names; after `untrust_ca` none did again. Chromium read the CurrentUser Root store directly, Firefox
  through the `user.js` rexenv writes. What it rests on: "accepted" is the request reaching the edge's
  upstream, not a rendered padlock; names resolved inside the browsers (`--host-resolver-rules`,
  `network.dns.localDomains`) because `.rex` DNS is W6; and Caddy ran as the check's own child, since the
  scheduled task the desktop session needs forbids job breakaway (§3 D1). Headless Firefox on a rejected
  certificate did not exit and was killed at 60 s.
- **W6 — DNS + privileges.** D2's :53 measurement first, then the agent, the NRPT rules and the refusal that names a
  :53 holder; `PrivilegeManager` as a UAC
  elevation that says what it is for (the macOS dialog rule, ledger #579's family);
  `DnsAgentManager` as a logon Scheduled Task with restart. *Done when:* `*.rex` resolves
  after a reboot with the app closed.
  **W6 design — proposed 14 Sep 2026; RULED the same day (R1–R4 at the end).** *Order as built:* S1
  first (self-contained, measurable without changing the Dell), then S2, then S0 together with S4 — the
  S0 panics sit on `resolver_path`, which R1's route trait replaces — then S3 and S5.
  *Where Windows stands:* the DNS server itself is platform-free (`core/dns.rs`, hickory, UDP on
  `127.0.0.1`, answers every A with loopback, a TXT build identity) and `--dns-agent` is dispatched on
  every OS (`main.rs`). Everything around it is macOS-shaped, and the Windows build reaches the
  stubs: the launch `install` fails into in-process DNS, then `spawn_dns_handoff` calls
  `DnsAgentManager::is_installed` about 20 s after launch — an `unported!` panic — and `dns_status` and
  `login_launch_needs_window` call `DnsManager::resolver_path`, another. The shapes that do not fit:
  (a) the agent's port is `DEFAULT_DNS_PORT` = 15353 everywhere, while NRPT has no port (D2) — Windows
  needs 53, with `SO_EXCLUSIVEADDRUSE`; (b) `DnsManager` and the core around it are FILE-shaped —
  `resolver_path`/`resolver_contents` as the ownership signature, backups that copy the user's file,
  `installed_tlds` a directory scan of `resolver_path("rex").parent()`; (c) `DnsAgentManager` names a
  plist; (d) `PrivilegeManager::run_privileged` takes a `/bin/sh` script, and teardown joins the edge's
  and the resolvers' commands with `" ; "`; (e) UAC's dialog shows the elevated PROGRAM's name and
  publisher, never a sentence — ledger #578's rule (every admin prompt says what it is for) cannot be met
  inside it.
  *Steps, each its own commit with its proof:*
  - **S0 — no panic on the paths the Windows build already walks.** The agent and resolver stubs answer
    as "not installed / not configured" instead of `unported!` where the caller is a status read, so the
    app runs in-process DNS without dying at 20 s. L0 scan + a Dell launch.
  - **S1 — the agent on :53.** The resolver's port and its bind come from the platform (macOS 15353, a
    plain bind, byte-identical; Windows 53, `SO_EXCLUSIVEADDRUSE`); `run_agent` and `start_default` use
    them; a taken :53 refuses by name through `port_holders` (#599), with D2's two measured refusals
    (10048 a loopback holder, 10013 an exclusive wildcard) worded apart. L1 on the Dell: the agent answers
    `Resolve-DnsName x.rex -Server 127.0.0.1`, beside ICS's wildcard holder, and refuses a planted holder.
    **S1 done 14 Sep 2026 (ledger #615).** `platform::RESOLVER_PORT` (53 / 15353) and
    `platform::bind_resolver_udp` (Windows: `SO_EXCLUSIVEADDRUSE`, `SIO_UDP_CONNRESET` off); `serve_udp`,
    `port_bound`, `run_agent` and `start_default` use them. The gate moved: `start_default` binds first,
    and `ports::refused_bind` names the refusal from LOOPBACK rows for address-in-use
    (`port_conflict_help_on`) — the unfiltered help had named ICS's `0.0.0.0:53` and offered
    `Stop-Service SharedAccess`, measured on the first Dell run. `WindowsDnsAgent::is_installed` = false
    until S2. Dell `windows_dns_agent_check` PASS, 20 checks, beside ICS. Plants on the Dell: the
    table-first gate, port 15353 and naming from every row each failed the check (5, 8 and 4 checks);
    `SO_EXCLUSIVEADDRUSE` and `SIO_UDP_CONNRESET` planted out each PASSED — Windows' default already
    refuses a same-account `SO_REUSEADDR` bind, and hickory kept answering past vanishing clients — so
    both stay as guards the check does not certify. Not in S1: the app's launch
    path on Windows end to end (it still installs no agent, so it runs in-process), `rex doctor`'s port
    list (`ports::default_ports` still table-gates :53 — the CLI is W8).
  - **S2 measured first, 15 Sep 2026** (`scripts/probes/windows-logon-task.ps1` through
    `windows-limited-token.sh`: Medium token, the desktop session). The user registered
    `\rexenv\dns-agent-probe` from task XML with `schtasks /Create /XML` — NO elevation — with a
    `LogonTrigger` for its own SID, `InteractiveToken`, and Task Scheduler kept every setting asked for:
    `Hidden`, `ExecutionTimeLimit PT0S`, both battery stops off, `IgnoreNew`, `RestartOnFailure PT1M ×999`.
    `/Run` started it; a second `/Run` while it ran started nothing (`IgnoreNew`); `/End` ended it;
    `/Delete` removed it and the `\rexenv\` folder with it. **`RestartOnFailure` did NOT restart a killed
    action process** — `Stop-Process` on it left the task `Ready`, Last Result -1, nothing running 150 s
    later: that setting covers a task that fails to START, not an action that dies. So a logon task alone
    is not launchd's `KeepAlive`; an agent that crashes with the app closed would stay down until the next
    logon. **Ruled (owner, 15 Sep 2026): a time trigger repeating every minute on the same task** —
    `IgnoreNew` makes it a no-op while the agent runs, and a dead agent is back within about a minute; no
    new code, only the task XML (over an agent that supervises itself, or the app's watchdog alone). To be
    measured before it is built. **No result yet:** the first run (`windows-logon-task-repeat.ps1`) was cut
    off when the Dell, on battery, hit "Critical Battery Trigger Met" (00:25) and the user was logged off
    (00:28) — the wrapper and the probe task's action ended with 0x40010004 and no output, which is the
    session ending, not Task Scheduler's answer; the rerun never started (267011, "has not run") because an
    interactive-token task needs a logged-on user. Found the same way: the limited-token wrapper wrote its
    output only at the end, so a killed run lost everything — it now appends as the probe runs.
    **S2 done 15 Sep 2026 (ledger #616)**, measured with the product code rather than the probe: Dell
    `windows_dns_agent_task_check` in the desktop session (Medium token, the owner logged on), 17 checks —
    `install` registered `\rexenv\dns-agent` without elevation and the agent answered in 0.5 s, naming this
    build, its bind line in the `--log` file; a second unchanged `install` left the same process;
    `kickstart` gave a new process; the agent killed with `taskkill /F` was answering again 6 s later — the
    next minute's tick; `uninstall` left no task, no agent, no definition file, nothing on :53. Not
    measured: the worst case of the keep-alive (a kill just after a tick — up to a minute), the task after
    a reboot (S5's done-when), a sleep/resume, and the app's own launch path installing it.
  - **S2 — `DnsAgentManager` as a logon task.** A pure task-XML builder (AtLogOn for this user,
    `ExecutionTimeLimit` PT0S, no battery stop, restart on failure, hidden) replacing `plist_*`;
    install/kickstart/uninstall through Task Scheduler as the user, no elevation — measured first on the
    Dell under the Medium token. L0 XML test; L1 register → runs → `kickstart` → `uninstall`.
  - **S4 measured first, 15 Sep 2026** (`scripts/probes/windows-nrpt.ps1`, elevated SSH, the agent on
    127.0.0.1:53): the Dell had no NRPT rule (`DnsPolicyConfig` existed, empty). `Add-DnsClientNrptRule
    -Namespace .rex -NameServers 127.0.0.1 -Comment … -DisplayName …` created
    `HKLM\SYSTEM\CurrentControlSet\Services\Dnscache\Parameters\DnsPolicyConfig\{GUID}` with `Name=.rex`,
    `GenericDNSServers=127.0.0.1`, `Comment`, `DisplayName`, `ConfigOptions=8`, `Version=2`, empty
    `IPSECCARestriction`; `Get-DnsClientNrptPolicy` showed it at once. **It took effect immediately, no
    cache flush:** `Resolve-DnsName probe.rex` (no `-Server`) and `a.b.probe.rex` got 127.0.0.1 in 1–8 ms,
    and so did .NET `GetHostAddresses` and `ping` (getaddrinfo); `probe.test` stayed NXDOMAIN. Removing the
    rule stopped resolution at once, and removing the LAST rule deleted the `DnsPolicyConfig` key itself.
    **The values' types and a non-elevated read, the same day** (two rules added over SSH — ours on
    `.rex`, a "foreign" one on `.test` + `.example` with servers `127.0.0.1` and `::1` — then read from the
    desktop session's Medium token, then removed): `Name` is `REG_MULTI_SZ` (one rule, several namespaces),
    `GenericDNSServers` a `REG_SZ` joined by `;`, `Comment`/`DisplayName` `REG_SZ`, `ConfigOptions` and
    `Version` `REG_DWORD`. The Medium token read the keys and `Get-DnsClientNrptRule` listed both rules, so
    ownership and status need no elevation. No Group Policy NRPT path existed on the Dell
    (`HKLM\SOFTWARE\Policies\Microsoft\Windows NT\DNSClient\DnsPolicyConfig`). A foreign rule can name
    several namespaces at once — a takeover of one TLD must not drop the others.
  - **S4, first half — the route trait, 15 Sep 2026 (ledger #617).** `DnsManager` no longer speaks
    files: `route_label`, `route_contents`, `route_owner`, `our_route_tlds`, `foreign_route_tlds` and the
    install/uninstall/restore builders; `ResolverOwner` moved to `platform::traits` (re-exported from
    `core::dns`). The file half — `owner_of` and the two signature scans, with their tests — moved to
    `platform/resolver_files.rs`, which `MacosDns` and the core's test platforms share; `resolver_path` and
    `resolver_contents` are `MacosDns`'s own methods. Callers (`dns_status`, a login launch, the MCP stack
    snapshot, the takeover preview) ask `route_owner != Absent` — what `path.exists()` answered on macOS.
    Windows' `WindowsDns` answers Absent, no TLDs and empty commands (never run: `run_privileged` is
    unported until S3) — so S0's `resolver_path` panics are gone. Next half: the NRPT reads (registry,
    measured readable without elevation) and PowerShell builders, with R3's takeover of a rule that may name
    several namespaces.
  - **S4, second half — NRPT, 15 Sep 2026 (ledger #618).** `process::read_nrpt_rules` reads every rule
    (local and Group Policy keys; `Name` as a multi-string, servers split on `;`); `windows/nrpt_rules.rs`
    holds the rest, tested on every host: ours = a local rule, comment `rexenv`, one namespace `.<tld>`, one
    server `127.0.0.1`; any other rule naming the TLD → Foreign with those rules as JSON. `install_script`
    strips `.<tld>` from every rule naming it (removing a rule only when nothing is left) and adds ours;
    `uninstall_script` removes only our exact rules; `restore_script` reads the backup with
    `ConvertFrom-Json` and `Set`s the namespaces back on the same rule key, or adds the rule when its key is
    gone. **Measured** (`windows_nrpt_route_check`, 21 checks, `.rexnrptcheck`/`.rexnrptother`, the elevated
    SSH token standing in for S3): install → ours, `a.rexnrptcheck` → 127.0.0.1 through Windows' resolver;
    uninstall → absent and unresolved; another tool's rule on both namespaces (servers `10.9.9.9`) → foreign,
    both in `foreign_route_tlds`; the takeover → ours, their rule (same key) now naming only
    `.rexnrptother`, `b.rexnrptcheck` → 127.0.0.1; `uninstall ; restore` joined as the core joins them →
    their rule (same key) naming both again, no rexenv rule. The first run failed two checks on its own
    harness: PowerShell's progress records on stderr were read into an answer that was 127.0.0.1. Not
    measured: a Group Policy rule (none on the Dell), `rexenv`-commented rules another tool wrote, the
    scripts under S3's UAC path.
  - **S3 — done 15 Sep 2026 (ledger #619).** A security question found
    while designing it went to the owner: `rexenv.exe --elevated-step <script>` would let any process on the
    machine run its own script behind a UAC prompt that names rexenv. **Ruled: the step runs only rexenv's
    ops** — `WindowsDns`'s commands became `nrpt-install <tld>` / `nrpt-remove <tlds>` / `nrpt-restore <tlds>`,
    parsed on both sides (valid TLD labels only), the restore reading only rexenv's own `resolver-backups`
    and refusing a backup that does not route its TLD, the PowerShell built inside the elevated process.
    **And ruled: rexenv's own dialog is a native message box** (titled rexenv, the `PromptReason` sentence,
    "Windows will ask for your permission next.", OK/Cancel) rather than an in-app sheet. The step writes its
    result only to a `rexenv-elevated-*.txt` directly in the user's temp directory. Over SSH the ops path
    PASSED (`windows_nrpt_route_check`, 21 checks, through `run_elevated_ops_in_this_process`). In the desktop
    session with the owner answering, `windows_uac_step_check` PASSED (10 checks) on its second run: a non-op
    refused with no window; install OK+Yes → ours; remove OK+Yes → absent; Cancel and OK+No → the cancel
    wording, absent. The first run failed four checks on its harness: the refusal's wording was pre-empted by
    the label check (the verb is now checked first), and one fixed reason gave the removal the install's
    words — that run's removal came back "cancelled" after 5.3 s, which button unknown. Not measured: that
    UAC was on screen for "No" (the secure desktop cannot be read), administrator credentials typed by a
    standard user, the app binary's own `main.rs` path, Windows 11.
  - **S3 — `PrivilegeManager` on Windows** (per R2): one UAC prompt per batch, the platform owning how
    commands join; output and exit code back to the caller. L1 on the Dell with the owner answering.
  - **S4 — resolver routes, NRPT** (per R1, R3): one rule per TLD, `-Namespace .<tld> -NameServers
    127.0.0.1`, ownership by a rexenv comment; list/owner/install/uninstall; a DNS cache flush; teardown.
    L0 for the script builders and ownership classification; L1 with the owner: `x.rex` resolves through
    Windows' resolver with no `-Server`, and teardown leaves no rule.
  - **S5 — done-when.** First-run setup, watchdog, handoff and `dns_status` on Windows end to end; the Dell
    rebooted with the app closed resolves `*.rex`.
    **S5 done 15 Sep 2026 — W6 is done.** Measured with the REAL app, not an example (owner's ruling the same
    day): `rexenv.exe` cross-built by `./scripts/windows-app-build.sh` (16 Sep 2026, ledger #646 -- it carries the
    `--features tauri/custom-protocol` flag and asserts the assets really are inside the exe; the `rex`
    sidecar a placeholder, as `windows-example.sh` stages it), copied to a stable folder on the Dell,
    opened by the owner in the desktop session; `scripts/probes/windows-dns-s5.ps1` read the machine over
    SSH between steps (read only). From a clean baseline (no task, rule, CA, logs; ICS on `0.0.0.0:53`):
    **launch** → the task registered without elevation and the app in Agent mode ("resolver agent serving
    on udp 53"), the agent naming this build; **Onboarding's setup**, the owner answering rexenv's dialog,
    UAC and the certificate warning → the `.rex` NRPT rule ours through the real `--elevated-step`, the CA in
    CurrentUser Root, `s5probe.rex` → 127.0.0.1 through Windows' own resolver; **watchdog** — the agent
    killed just after a minute tick, the app's watchdog kickstarted it 6 s later (`health.log`), answering
    at 8.4 s, before the next tick could; **handoff** — the app ended, its agent held down (the minute tick
    started it 8 times while the owner relaunched), the relaunched app served :53 in-process from 3.2 s and
    handed it back to a new agent at 23.2 s ("attempt 1"), answering throughout at 0.7 s sampling;
    **`dns_status`** — Settings' DNS card read "*.rex → 127.0.0.1 · agent … resolves even when rexenv is
    closed" and the CA trusted (the owner's screenshot); **reboot** with the app closed → after logon the
    agent up at 13:37:16, :53 its own, `.rex` resolving, no app process. Found on the way: the probe first
    ran as `-EncodedCommand` and nothing ran, silently — past the command-line limit; the Windows screens
    still say "login keychain", "your Mac" and "password" (W9's row). Not measured: sleep/resume, a
    fast-user switch, teardown from the app on Windows, a second Windows account, Windows 11.
  *Rulings asked:* **R1** the resolver trait — a neutral "resolver route" shape the core uses without
  assuming files (macOS behaviour and bytes unchanged), or the file-shaped trait kept with NRPT rules
  mapped to pseudo-paths; **R2** the UAC prompt — rexenv explains the change in its own window first and
  UAC elevates `rexenv.exe` itself for the step (the dialog names rexenv; "Unknown publisher" until D5
  signing), or UAC elevates `powershell.exe` (the dialog names Windows PowerShell, Microsoft-verified);
  **R3** a `.rex` NRPT rule that is not ours — refuse naming it, or macOS's backup-first takeover; **R4**
  the agent's protocols — UDP only as on macOS, or UDP and TCP.
- **W7 — Desktop integration.** `ShellRunner` (open/reveal, editors, browsers — closes
  the Windows half of the browser-stub row in TODO — terminals, `git_preflight` naming
  Git for Windows), `AutostartManager` (HKCU Run key), tray and close-to-tray behaviour.
  **W7 design — proposed and RULED 15 Sep 2026.**
  *Where Windows stands* (read from the tree the same day): nothing panics, but most of the desktop fails
  when used. `ShellRunner::open` is `Unported`, and it backs 19 UI call sites (every site link, "Open
  folder", Adminer, Mail, magic login, and the fallback when no editor is found); `reveal` backs 8 "Show in
  Finder" actions; `detect_editors`/`detect_browsers`/`detect_terminals` are the trait's empty defaults,
  so "Open in editor" says no editor was found and falls back to the failing `open`, the browser chevron
  shows nothing and the terminal chevron is hidden; `AutostartManager` is `Unported`, so Settings' "Open
  rexenv at login" reads off without saying why and the launch `refresh` is skipped; `git_preflight` is the
  default OK and the missing-git hint names `xcode-select`; `symlink_dir`/`remove_symlink` are
  `Unsupported`, so "Link folder" and deleting a linked plugin fail. `symlink_file` stays unsupported —
  the Windows CLI install is a PATH entry (W8). `run` has no callers. **The single-instance lock is
  `#[cfg(unix)]`** (`cli_server::claim_at_startup`, the CLI socket is the lock): on Windows a second
  double-click boots a second full app — a second SQLite writer, a second watchdog, adoption racing the
  first. The tray already installs on every OS, but with macOS's template glyph (`icon_as_template`) and a
  menu on any click. The macOS catalogs (10 editors, 16 browsers, 7 terminals) are private to `MacosShell`
  and detect `<Name>.app` folders; the frontend has no hardcoded ids (icons from `icon`, lucide fallbacks),
  but its words are macOS's ("Show in Finder", "sign in to your Mac", the `xcode-select` hint).
  *The Dell, inventoried read-only the same day:* editors VS Code (per-user install, `code.cmd` on PATH) and
  PhpStorm 2025.1.1 (uninstall entries: `InstallLocation`, `DisplayIcon`); browsers Chrome, Firefox, Edge,
  Brave (App Paths + `StartMenuInternet`), Opera (App Paths, empty), Maxthon and Internet Explorer
  (`StartMenuInternet`); the default `http`/`https` handler `ChromeHTML` (`UrlAssociations\…\UserChoice`);
  no Windows Terminal, no PowerShell 7; Git for Windows at `C:\Program Files\Git`; Developer Mode off (so a
  symbolic link needs elevation — junctions); the HKCU Run key already starts Slack and Edge.
  *Rulings (owner, 15 Sep 2026):* **Q1** the single-instance lock is built IN W7 as a named pipe (D3's
  transport, current-user ACL) that understands only `app.open` — a second launch connects, brings the
  first window to the front and exits; W8 grows the same pipe into the CLI and MCP. **Q2** the tray opens
  the window on a LEFT click and the menu on a RIGHT click (the Windows convention), with the colour app
  icon — the template glyph is macOS's; macOS unchanged. **Q3** the words a feature shows come from the
  platform ("Show in Explorer" / "Show in Finder", "when you sign in to Windows", the Git for Windows hint),
  as `route_label` already does for the DNS route, and change with each W7 feature; Onboarding's and the
  keychain's words stay W9's row.
  *Measure first on the Dell, before building on it:* (a) the named pipe as a lock — the first server made
  with `FILE_FLAG_FIRST_PIPE_INSTANCE` and a current-user DACL, a second process's create refused, its
  connect succeeding, and another account's connect refused; (b) Tauri 2's tray on Windows 10 — a left
  click reaching `on_tray_icon_event` with the menu off for it, the menu on a right click, the colour icon
  on a light and a dark taskbar; (c) the Run-key launch context — the app started by Explorer at logon with
  `--hidden`: whether `CREATE_BREAKAWAY_FROM_JOB` is allowed there (D1's launch contexts: Explorer and Start
  still unmeasured; a scheduled task was measured to refuse it); (d) a directory junction made by the
  desktop user without elevation, what `symlink_metadata().file_type().is_symlink()` says of it, and that
  removing the link leaves the target's files; (e) `ShellExecuteW` "open" on a folder and a URL, and
  `explorer.exe /select,` versus `SHOpenFolderAndSelectItems` for reveal, from the desktop token.
  **(a), (d) and (e) measured 15 Sep 2026** (`examples/windows_desktop_probe.rs` through
  `scripts/probes/windows-desktop-probe.ps1` and `windows-limited-token.sh`: Medium token, the desktop
  session, the owner at the screen). **(a)** A server pipe made with `FILE_FLAG_FIRST_PIPE_INSTANCE` and
  the DACL `D:P(A;;GA;;;<user SID>)`: a SECOND process's first-instance create of the same name was
  refused with **error 5** (access denied — the lock answer), and the same process then connected as a
  client, sent `{"cmd":"app.open","args":{}}` and read the server's reply; once the holder's handle closed,
  a new first-instance create succeeded — the lock dies with its holder, as the unix socket's does. A
  backslash inside the pipe name was accepted. Not measured: another account's connect (none on the Dell).
  **(d)** `mklink /J` by the desktop token, no elevation: **`symlink_metadata(..).file_type().is_symlink()`
  is TRUE for a junction** (and `is_dir` false) — so `core::repo::partition_symlink_deletes` already sees
  one; `read_link` gave the target. `remove_file` on a junction was **refused, error 5**, link still there —
  the macOS `remove_symlink` shape does not work on Windows; `remove_dir` removed the junction and left the
  target's file, and so did `remove_dir_all` on the junction (it did not walk in). `symlink_dir` was
  refused with **1314** (privilege not held) — Developer Mode off, so junctions, as ruled. **(e)**
  `ShellExecuteW("open")` returned 42 (success) for the fixture folder and for an `http` URL, and after
  `explorer.exe /select,` two Explorer windows (`CabinetWClass`) titled with the folder were on screen.
  `SHOpenFolderAndSelectItems` was not tried (it needs COM). (b) and (c) need the real app — S5 and S4.
  *Steps, each its own commit with its proof:*
  - **S1 — single instance (Q1).** The lock is the pipe: its name derived from the app-data directory (one
    rexenv per app-data directory, as on macOS), created first-instance-only with a current-user DACL
    before Tauri boots; a refused create means another instance runs, so the launch connects, sends
    `app.open` and exits; a pipe it cannot interpret starts the app (refusing to launch is the worse
    failure). L0 the name and the decision; L1 on the Dell: two double-clicks → one process, the window in
    front.
    **S1 done 15 Sep 2026 (ledger #620).** `platform/windows/app_pipe_rules.rs` (the name — a digest of the
    lower-cased config folder —, the claim decision, `app.open` only; tested on every host, plants 3/3) and
    `app_pipe.rs` (a plain `CreateNamedPipeW` before any runtime, overlapped and remote clients rejected,
    the owner-only descriptor from `acl.rs`; later instances from tokio's `ServerOptions`; the hand-off
    connect with a short busy retry and a 1.5 s wait for the reply); `cli_server::claim_pipe_at_startup` /
    `spawn_pipe` beside the unix claim in `lib.rs` `run()`. The server makes the next instance before it lets
    the connected one go. **Measured with the real app on the Dell:** opened by the owner → one app process,
    the pipe present, "app pipe: holding" logged; a second copy from SSH → exited in 0.14 s with the hand-off
    line, one app process, the window came to the front (the owner); the window hidden and the exe
    double-clicked again → one app process, the window in front. Not measured: another account's connect,
    two launches in the same instant, the Start menu as the second launch (W11). Seen on the way: the app
    log says "this Mac's macOS version could not be read" for the update check on Windows (W11's).
  - **S2 — `open` and `reveal`.** `ShellExecuteW` for a path or an `http(s)` URL; reveal selects the item in
    Explorer. L1: a folder, a file and a URL opened from the desktop token.
    **Ruled 15 Sep 2026 (owner), found while designing it:** `open_external` passes any string to `open`,
    and `ShellExecuteW("open")` runs a `.bat`/`.ps1`/`.lnk`/`.php` by association — so Windows `open` takes
    `http(s)` URLs, existing folders and existing files with a reading extension, and refuses the rest by
    name (the callers: link and folder buttons, and SiteLogs' "open log"). **S2 done the same day (ledger
    #621).** `windows/shell_rules.rs` (the classification, bare URI schemes refused too, explorer's
    `/select,` argument; plants 4/4 — the first scheme plant PASSED on a test whose cases other rules also
    refused, and the stronger test found `ms-settings:`/`shell:` passing a `://` check) and
    `WindowsShell::open`/`reveal`. Dell `windows_shell_open_check` PASS (7 checks, desktop session): a
    folder and a `.log` opened in their windows, a `.bat` refused and never run, missing paths refused,
    reveal selected the log in Explorer, the fixture's windows closed.
  - **S3 — editors, browsers, terminals.** A Windows catalog behind the same trait shapes, detected from App
    Paths, uninstall entries and `StartMenuInternet` (never a guessed path alone), the default browser from
    the `UserChoice` ProgId; opening runs the detected executable with the folder or URL as one argument,
    browsers `http(s)` only and their private flags (`--incognito`, `-private-window`, `--inprivate`);
    terminals Windows Terminal (`wt -d`) when present, PowerShell 7, Windows PowerShell and Git Bash at the
    folder. Icons may start as `None` (the lucide fallbacks). L0 the registry parsers on the Dell's shapes;
    L1 on the Dell: VS Code and PhpStorm open a site, the chevron lists the browsers with Chrome as default.
    **S3 done 15 Sep 2026 (ledger #622).** `windows/app_registry.rs` reads App Paths, the uninstall entries,
    `StartMenuInternet` and the `https` ProgId; `windows/app_catalog.rs` holds the catalogs (macOS's ids),
    detection (registered first, known folders after, existing executables only), the default browser, and
    each open's process; `WindowsShell` starts it. Plants 6/6 on the Mac. **The Dell took four runs, and
    each of the first three PASSED over a real bug:** the PowerShell check matched VS Code's window while the
    new console wrote into the check's output (`std::process::Command` hands down standard handles even with
    `CREATE_NEW_CONSOLE` → `CreateProcessW` with none); a fresh Chrome kept the check's pipe open with its logs
    (→ NUL standard handles); and the same Chrome still held it silently, because `std` makes every child
    inherit ALL inheritable handles (→ inherit flags cleared before the spawn, #600's rule). The fourth PASSED,
    11 checks, and ended by itself in 41 s with Chrome started fresh. Not measured: PhpStorm, Git Bash, Firefox,
    Brave and Edge actually opening; icons (`None`); the real app's menus.
  - **S4 — autostart.** The HKCU Run value `rexenv` = `"<exe>" --hidden`; `is_enabled` reads it;
    `refresh` rewrites only a changed value and never re-points it at a dev build (the macOS rule). L0 the
    value; L1 a real sign-out/sign-in: rexenv in the tray, window hidden, services started (the breakaway
    measurement), and `login_launch_needs_window` still showing the window when setup is incomplete.
    **S4, registry half done 15 Sep 2026 (ledger #623).** `windows/autostart_rules.rs` (the value, Task
    Manager's disable in `StartupApproved\Run`, a dev build, the refresh decision; plants 3/3) and
    `windows/autostart.rs` (HKCU read/write, no elevation). Decided while building it: enabled = the Run value
    AND not disabled in Task Manager; `enable` clears Task Manager's disable (the user's explicit choice in
    rexenv); `refresh` never touches it. Dell `windows_autostart_check` PASS (9 checks, real Run key, `reg.exe`
    read-back, guard left nothing). **The sign-in half is paired with S5** — both need the real app and the
    owner signing out and in: the app started by Explorer from the value, hidden, and (c) whether its services
    may break away from that launch context. **Sign-in measured the same day with the real app:** the Settings
    toggle wrote the Run value; after a sign-out and sign-in `rexenv.exe --hidden` was started by `explorer.exe`,
    logged "launched at login — staying in the menu bar" and showed no window; Mailpit started from it outlived
    the app's quit — (c) answered for Explorer.
  - **S5 — the tray (Q2).** Colour icon and the left/right split on Windows; close keeps hiding the window
    (already on every OS), so the tray and S1's second launch are the ways back. L1 with the owner.
    **S5 done 15 Sep 2026 (ledger #624).** `install_tray`'s `cfg(windows)` branch: `WINDOWS_TRAY_ICON`
    (`icons/32x32.png`), `show_menu_on_left_click(false)`, a left-button release → `show_main_window`; macOS's
    branch untouched. A source-scan test with plants 2/2. On the Dell with the real app: a left click brought a
    hidden window back and a right click opened the menu — both from a double-clicked launch and from the
    login-launched one, whose tray Quit ended the app; the owner confirmed the icon drew in colour. Afterwards,
    at the owner's word, the login item was removed and the test Mailpit stopped.
  - **S6 — linked folders as junctions.** `symlink_dir` makes a junction; `remove_symlink` recognises a
    junction and removes only the link; the delete guard that must never walk into a linked checkout
    (`core/repo.rs`, `is_symlink`) is held to a junction with its own ledger row and plant — its blast
    radius is the user's real code. L0 + L1 on the Dell: link a checkout, delete the linked plugin, the
    checkout's files all still there.
    **S6 done 15 Sep 2026 (ledger #625).** `windows/junction_rules.rs` (the target rule — local drive paths
    only, a share refused by name — and the mount-point reparse data; plants 3/3) and `windows/junction.rs`
    (`FSCTL_SET_REPARSE_POINT` on a folder it creates and removes again on failure; the link-only removal with
    `remove_dir`). Dell `windows_junction_check` PASS (12 checks, the desktop user's token): a checkout linked
    and written through, the delete guard seeing the junction as a link, a real folder refused by
    `remove_symlink`, the link removed with every checkout file intact. On the way: the first share plant
    PASSED (the drive check refused shares too, in the wrong words), and a later run of it hit a full disk and
    left the source planted until it was found and restored byte-identical — plant runs now get a `df` first.
  - **S7 — git and the words (Q3).** `git_preflight` finds `git.exe` on the login environment's PATH and
    names Git for Windows when it is missing; the platform's words for reveal, the login item and the git
    hint reach the frontend through one read, replacing the macOS strings in those features.
    **S7 done 15 Sep 2026 (ledger #626).** `platform/words.rs` (`MACOS` byte-identical to the old text,
    `WINDOWS`), the `platform_words` command and `usePlatformWords()`; "Show in Finder" ×8, the editor
    fallback's Finder, the login item's description and the git/Node/Bun/node-gyp hints now come from it. A
    scan test over the whole frontend fails on "Finder", `xcode-select` or `brew install` written outside the
    words; its first run found the checksum-cleanup confirm calling Thumbs.db "Finder clutter". `git_preflight`
    stays the default on Windows (no shim to guard; `resolve_git` names Git for Windows when git is missing).
    Dell: the real app's Settings read "rexenv launches when you sign in to Windows …".
    **W7's done-when, where it stands:** ✓ login item → sign-in → tray, services break away, a left click opens
    the window (S4, S5); ✓ a second launch → one process, window in front (S1); ✓ a folder and a reveal land in
    Explorer (S2); ✓ VS Code opens a site folder (S3); ✓ PowerShell opens in a new console at the folder (S3);
    ✓ a linked folder's removal leaves the checkout (S6, through the platform); ✓ Chrome the default among
    the detected browsers (S3). **The rest measured 15 Sep 2026 with the real app and the WordPress site
    `w7check`:** ✓ PhpStorm opened the site (its `.idea` created, the project its last opened); ✓ Firefox,
    Brave and Edge each showed the site's window from the chevron; ✓ "From Git" cloned
    `WordPress/classic-editor` through Git for Windows; ✓ a folder linked as a plugin and deleted from the
    WordPress screen left its checkout byte-identical. **W7 is done.** Measuring it found three bugs, each
    fixed first: the download plans named `php-fpm` on Windows, so no site could begin (ledger #627); tool
    lookup split `PATH` on `:`, so git and node read as missing (ledger #628); the UI split paths on `/`, so
    "Link folder" offered the whole path as the name (ledger #629).
  *Done when* (the Dell, the owner at the desktop): "Open rexenv at login" on, then sign out and in —
  rexenv in the tray with its services up, a left click opens the window; a second double-click leaves one
  process with its window in front; "Open folder" and "Show in Explorer" land in Explorer; "Open in editor"
  opens a site in VS Code and PhpStorm; the browser chevron lists the installed browsers with Chrome as
  the default and opens a site in each; "Open in terminal" opens PowerShell at the site folder; a linked
  plugin deleted leaves its checkout intact; a git clone runs through Git for Windows.
- **W8 — `rex` CLI + MCP on Windows.** Over D3's transport; `rex.exe` sidecar on the
  user PATH. *Done when:* `rex` commands from a new PowerShell reach the running app.
  *Where it starts (read 15 Sep 2026):* `cli_server`'s request model, `handle_request` and `dispatch` compile
  everywhere; only the unix socket transport (`claim`, `bind`, `serve`, `spawn`, the hand-off) is `cfg(unix)`.
  W7 left a named pipe `\\.\pipe\rexenv-app-<first 10 bytes of SHA-256 of the lower-cased config dir>`,
  owner-only, first-instance, that serves only `app.open` (`app_pipe_rules::serves`). `mcp_server::start`
  refuses off unix, so the Settings toggle never reads on (ledger #203). The `rex` crate has one dependency
  (`serde_json`), `connect` fails off unix, and `socket_path`/`mcp_socket_path` exit "not supported" off
  macOS. `scripts/build-cli.sh` stages sidecars on macOS only; `core::cli` installs a symlink at
  `Paths::cli_symlink_path` (the trait's default is `Unsupported`) and looks for `rex` next to the app.
  Nine examples (`cli_*`, `mcp_*`) bind the unix socket themselves.
  *Rulings (owner, 15 Sep 2026):* **Q1** `rex` computes the pipe name itself, the app's way: the `rex` crate
  gains `sha2` (hashing only — ledger #54's "never links the app library" is untouched), and one test
  vector binds the app's name to rex's. **Q2** MCP gets its own pipe, `rexenv-mcp-<same digest>`, created only
  while the toggle is on and closed when it goes off — the macOS socket's rule (#203); the lock pipe grows
  into the CLI only (this narrows W7 Q1's "the same pipe grows into the CLI and MCP"). **Q3** install copies
  `rex.exe` into a folder of its own, `%LOCALAPPDATA%\rexenv\bin`, and adds only that folder to the user's
  `Path` (HKCU, no UAC); each launch refreshes the copy from the app's own `rex.exe`, never moving it to a dev
  build (the autostart rule, ledger #623). **Q4** install is the Settings card's button, as on macOS — the
  user's `Path` changes only when they ask.
  *Measure first on the Dell (a probe, before building on it):* (a) a client in a new desktop PowerShell
  (Medium integrity) reaches an owner-only pipe served by an Explorer-launched process, and one from the
  elevated SSH token does too; (b) with every instance connected a client gets `ERROR_PIPE_BUSY` (231) and
  `WaitNamedPipeW` lets it in; (c) progress lines written before the envelope arrive in order on a pipe the
  server reads and writes through one handle; (d) **a `rex mcp`-shaped bridge — one thread blocked reading,
  another writing — on a synchronous client handle and its duplicate:** Windows serializes synchronous I/O
  on one file object, so a blocked `ReadFile` may hold every `WriteFile` (if it does, the bridge needs
  overlapped I/O); (e) the config dir `rex` would build from `%LOCALAPPDATA%` equals the app's
  (`directories`, the Known Folder) on the Dell; (f) a folder appended to HKCU `Path` plus
  `WM_SETTINGCHANGE` is on `PATH` in a PowerShell started afterwards from the Start menu.
  *Measured 15 Sep 2026 on the Dell* (`windows_cli_pipe_probe`, the same results from the elevated SSH token
  and the desktop session's Medium token): **(a)+(e)** the config dir from `%LOCALAPPDATA%` equals the Known
  Folder's, the name computed from it is the running app's pipe (`rexenv-app-ee611f15ee7aa1a3cf20`), and the
  app answered a request over it from both tokens; **(b)** with the one instance held a client's open
  answers 231 ("All pipe instances are busy"), `WaitNamedPipeW` waits out its timeout while held (error 121)
  and returns when the server disconnects and listens again (700 ms, as freed); **(c)** three progress lines
  written 150 ms apart arrived 150 ms apart and in order, the envelope last; **(d) the blocking bridge
  deadlocks:** with one thread blocked reading a synchronous handle, a write on its `try_clone` returned only
  when that read did (3708 ms, when the server's unprompted line came) and the next write never returned in
  8 s — Windows serializes synchronous I/O on one file object; **(d2)** a tokio client split into halves wrote
  in 0 ms with its read pending, each echo read as it came. Also seen: the app's reply read through a
  PowerShell 5.1 pipe showed its em dash as `ΓÇö` (PowerShell decodes a program's redirected output with the
  console's OEM code page) — a console write is not affected; `rex` output piped in PowerShell 5.1 will be.
  *Ruling (owner, 15 Sep 2026, after (d)):* **Q5** the `rex` crate takes tokio on Windows only
  (`cfg(windows)`: net, io-util, rt) for the pipe — the shape (d2) measured; macOS's `rex` is unchanged.
  *Steps:* **S1** the `rex` transport on Windows — the pipe name (Q1), connect with the busy wait, the
  socket paths' Windows arms, the refusal words; **S2** the lock pipe serves every CLI request through the
  unix `serve`'s exchange (progress before the envelope, the command outliving its client), one exchange
  shape for both transports; **S3** the MCP pipe (Q2) and the `rex mcp` bridge over it, per (d); **S4**
  `rex.exe` staged as the Windows sidecar (`build-cli.sh`, `externalBin`, `bundled_rex` finding `rex.exe`);
  **S5** the Settings install (Q3, Q4) — status, install, the launch refresh, the card's words from the
  platform; **S6** the `cli_socket_check`/`mcp_socket_check` legs on the Dell and the done-when.
  **S1 done 15 Sep 2026 (ledger #630).** `cli/src/main.rs` `pipe_name` / `windows_config_dir` /
  `open_waiting_out_busy` (tested on every host, one vector shared with `app_pipe_rules.rs`) and
  `cli/src/pipe.rs` (tokio's overlapped client behind `UnixStream`'s method shape; `shutdown` refused — a
  pipe has no half-close, which S3's bridge must design around: the app sees end-of-input only when the
  whole pipe closes). `cli/Cargo.toml` gained `sha2` and, on Windows, tokio; its libc is pinned to the app's
  0.2.186 so the shipped macOS graph did not grow a second libc. Found on the way: #54's guard read only
  `[dependencies]` and would have passed a banned crate under the Windows table — it reads every table now.
  Dell, both tokens, against the W7 app: `rex open` exit 0, `rex status` exit 1 with the pipe's sentence,
  `rex mcp` exit 1, a name nothing serves exit 2.
  **S2 done 15 Sep 2026 (ledger #631), L0; the Dell run next.** `cli_server::serve_connection` is the one
  exchange — request line, opt-in progress, one envelope last, the command in its own task — and the unix
  `serve` and Windows' `spawn_pipe` (both its serving paths, including the next-instance-failure branch, which
  unsplits the pipe to listen on it again) hand every connection to it. W7's app.open-only rule, its refusal
  and their re-exports are removed; ledger #620's claim is narrowed to W7. A test over `tokio::io::duplex`
  holds the framing and the outlive-the-client rule on every host, and a scan holds both transports to it.
  **S3 done 15 Sep 2026 (ledger #632), L0; the Dell run next.** `app_pipe_rules::mcp_pipe_name` (the lock's
  digest, the `mcp` name) and `app_pipe::create_mcp` (the lock's `create_first`); `mcp_server::start` on
  Windows creates the pipe synchronously before it spawns `serve_pipe` (the toggle runs off the runtime), and
  `serve_pipe` is the unix `serve`'s shape — next instance first, each connection to `session`, out on the
  toggle's signal. `rex mcp` dials `rexenv-mcp-…`, and at end of input waits until nothing is pending
  (`PendingIds::is_empty`) instead of half-closing; the pump now marks a reply answered only after writing it,
  so that wait cannot end the process between the two. Found while designing: `rex mcp` told a person whose app
  was open with MCP off that rexenv "isn't running" — on macOS too; it now asks the CLI endpoint and says the
  endpoint is off. The pipe `Stream` lost its `shutdown`, now unused.
  **S2 and S3 measured on the Dell, 15 Sep 2026** (the app from `83f9ebe`; ledgers #631, #632): from the elevated
  SSH token and the desktop session's Medium token, `rex status`, `rex site list` (plain and `--json`),
  `rex version` and `rex open` answered from the running app. MCP off by default → `rex mcp` "endpoint is off",
  exit 2; on → `initialize` and `tools/list` (50 tools) answered and the bridge ended by itself ~120 ms after
  stdin closed; a held session was dropped when the owner turned the toggle off, and a new `rex mcp` said the
  endpoint is off. The check's pipe listing (`GetFiles` on `\\.\pipe\`) did not show the MCP pipe even while it
  served, so "the name is gone" rests on the refused connect. Not run: a streaming command (both make a site).
  **S4 done 15 Sep 2026 (ledger #633), L0.** `core::cli::sidecar_file_name(EXE_SUFFIX)` is the one spelling of
  the sidecar's name — `bundled_rex` looked for a bare `rex`, which never exists on Windows — and
  `scripts/build-cli.sh` gained a Git Bash arm that builds `rex.exe` and stages it as
  `binaries/rex-x86_64-pc-windows-msvc.exe`. Not run: that arm (no Windows build host yet; the Dell's `rex.exe`
  is cross-built with cargo-xwin). The card itself stays hidden on Windows until S5 gives it an install shape.
  **Measured for S5, 15 Sep 2026, on the Dell** (the owner agreed to the Path change; both restored):
  **(f)** from the DESKTOP session (Medium token, session 3 — a broadcast from the SSH session would not reach
  Explorer), a folder appended to the user's `Path` and `WM_SETTINGCHANGE("Environment")` sent with
  `SendMessageTimeout` (returned 1): a PowerShell then opened from the Start menu found a `.cmd` in that folder
  ("found on PATH", the owner). The Dell's user `Path` is **`REG_SZ`**, not `REG_EXPAND_SZ` (595 characters) —
  so an install must write the value back in the kind it found; the probe's restore wrote the exact data and
  kind back (verified) and removed its folder. **(g)** a running program's file (a copy of `ping.exe` named
  `rex.exe`, standing in for a `rex mcp` bridge an agent keeps open): copying over it FAILS ("being used by
  another process") and so does deleting it, but RENAMING it to `rex.exe.old` works, a new `rex.exe` can then
  be copied in, and `rex.exe.old` can be deleted only once the process has exited — so S5's install and launch
  refresh rename the old copy aside, copy, and sweep a leftover `.old` later.
  **S5 done 16 Sep 2026 (ledger #634).** `Paths::cli_install` names the shape — `CliInstall::Symlink` on macOS
  (unchanged), `CliInstall::CopyOnUserPath(%LOCALAPPDATA%\rexenv\bin)` on Windows — and `ShellRunner` gained
  the user-`Path` operations: `platform/windows/user_path_rules.rs` (which entry is the folder, adding it once,
  removing only it — tested on every host) over `user_path.rs` (the registry value in its own kind, then the
  broadcast). `core::cli` installs a copy by renaming a different one aside first, reads "current" as the
  copy's bytes, refreshes an installed copy at launch (`lib.rs`, after the login item's refresh) and removes
  copy and entry on teardown. The card's three strings became platform words. Plants 13/13. **Dell:** the
  owner clicked Install; the `Path` went from 10 to 11 entries, still `REG_SZ`, the old 595 characters intact;
  the copy matched the sidecar; a Start-menu PowerShell ran `rex status`. Not run: the launch refresh, the
  off-Path sentence, the teardown.
  **S6, first run (16 Sep 2026):** `rex site create w8stream.rex` from a fresh desktop process, through the
  installed `rex`, streamed its progress records as they came (36% at 00:26:07, 73% at 00:27:56, 89% at
  00:28:04) — and then failed at "starting to serve": the app had ADOPTED the stack an earlier instance
  started, and `adopt_startup` looked for `caddy-<v>/caddy`, which on Windows is `caddy.exe`, so the manager's
  resolved binaries stayed empty and the reload refused ("services not started"). Fixed through
  `binaries::cached_bin` (ledger #635), with a scan against the bug class. The site was deleted with
  `rex site delete --yes` (the owner agreed to create and delete). The owner ruled out running the teardown
  (it removes the DNS route and CA trust too).
  **S6, after the fix (16 Sep 2026):** the app built with #635, launched over the stack an earlier instance
  started: `rex site create w8stream.rex` streamed and ended "✓ created", exit 0, and `rex site delete --yes`
  removed it. The same night, with the owner: the CLI folder taken off the user's `Path`, Reinstall put it back
  (11 entries, the other ten intact); a different `rex.exe` beside the app, and the next launch replaced the
  installed copy and left the old one as `rex.exe.old`. Not seen on screen: the card's and toast's words.
  **W8 is done (16 Sep 2026).** The done-when, clause by clause, on the Dell: after Install a PowerShell opened
  from the Start menu ran `rex status` (the owner), and fresh desktop-session processes ran `rex site list` and
  the streaming `rex site create` (created, then deleted); `rex mcp` answered `initialize` and `tools/list` with
  the toggle on and said the endpoint is off with it off (S3); with the app quit the installed `rex` said
  "rexenv isn't running", exit 2, and the next launch swept `rex.exe.old`. Found and fixed on the way: a
  blocking bridge would deadlock (measured first, so the `rex` crate took tokio on Windows), #54's guard read
  one dependency table, `rex mcp` called an open app "not running", and an adopting launch left caddy
  unresolved on Windows (#635). Not certified anywhere in W8: the Command-line tool card's words read on
  screen, a replace while a `rex mcp` keeps the old copy running, the teardown (ruled out), Windows 11.
  *Done when, measured:* after Install, a PowerShell opened from the Start menu runs `rex status`,
  `rex site list` and a streaming command against the running app; `rex mcp` answers `initialize` and
  `tools/list` with the toggle on and says rexenv's endpoint is off with it off; with the app quit, `rex`
  says it isn't running and exits 2.
- **W9 — Frontend on WebView2.** Windows paths (`C:\…`) in inputs and display,
  Cmd → Ctrl shortcuts, font metrics; divergences into `docs/DESIGN.md`.
  *Measured first, 16 Sep 2026, the real app on the Dell (the owner at the screen, one screenshot):*
  **fonts need no work** — `@fontsource` Inter, JetBrains Mono and Space Grotesk are bundled and imported by
  `globals.css`, and the screenshot shows all three rendering (the UI stack's `-apple-system` / `SF Pro Text`
  simply fall through to the bundled Inter); the Dell has none of them installed, which is exactly why
  bundling is what makes Windows and macOS look alike. **Ctrl+R reloads the whole app** — wry 0.55's
  `browser_accelerator_keys` defaults to WebView2's own behaviour and Tauri 2.11 does not surface the switch,
  so Ctrl+R/F5/Ctrl+P/Ctrl+F/F12 are all live in a packaged build. **The window has Windows' own title bar**
  (`titleBarStyle: "Overlay"` and `hiddenTitle` are macOS-only), and under it the sidebar still reserves a
  ~12px row for macOS's traffic lights.
  *Rulings (owner, 16 Sep 2026):* **Q1** keep the native title bar on Windows and drop the reserved row there —
  no frameless window, no drawn controls. **Q2** turn the browser accelerator keys off through
  `WebviewWindow::with_webview` + `ICoreWebView2Settings3::SetAreBrowserAcceleratorKeysEnabled(false)` (the
  hatch `platform/macos/webview_dialogs.rs` already uses on its side); `webview2-com` and `windows` become
  direct Windows dependencies — they are already in the lock through wry. **Q3** fonts: bundle all three —
  already true, so nothing to build.
  *The rows (measured or read, 15–16 Sep 2026):* **words the frontend still writes as macOS's** — Settings'
  two "Looked in /Applications and ~/Applications …" tooltips, its `~/rexenv/Sites` tooltip, "trusted · login
  keychain", "Local CA re-trusted in your login keychain.", "Reinstall rexenv's certificate authority in your
  system keychain.", "asks for your password once", the teardown's "/etc/resolver" sentences; SiteDetail's
  "register it with macOS"; Onboarding's "to your Mac"; Import's `~/.config/valet` and `/etc/resolver/test`
  (and what that whole Valet/Herd card means on Windows at all); AgentsMcpCard's `~/.cursor/mcp.json`.
  **And in Rust** — `core/proxy.rs`'s "is answering HTTPS on this Mac", `core/dns.rs`'s "so .{tld} sites open
  on this Mac" (pinned by a `traits.rs` test), `core/app_update.rs`'s "needs a newer macOS than this Mac's"
  and its "this Mac's macOS version could not be read" (seen in the Dell's log), `mcp_server/user_sites.rs`'s
  five "(macOS will also ask for your password)" descriptions, `lib.rs`'s "staying in the menu bar" log line,
  and `traits.rs`'s "macOS refused the operation".
  *Steps:* **S1** the window on Windows — the reserved traffic-light row gone, the drag region still ours
  (`startDragging`, double-click to maximise), one look on the Dell; **S2** the accelerator keys off, and a
  Dell run of Ctrl+R/F5/Ctrl+P/Ctrl+F/F12 to say which of them the setting actually kills; **S3** the words —
  every row above through `platform/words.rs` (the shape W7 S7 built), with the frontend scan widened to the
  new words; **S4** the paths shown in the UI — `%LOCALAPPDATA%`-shaped examples where a path is illustrative,
  and the Import card's Valet/Herd half told honestly on Windows; **S5** `docs/DESIGN.md` gets a Windows
  section: the native title bar, the accelerator keys, and anything the screenshot shows diverging.
  **S1 and S2 done 16 Sep 2026 (ledgers #636, #637), L0.** `PlatformWords` grew one non-string fact —
  `window_controls_in_content`, macOS true / Windows false — and the sidebar renders its ~12px spacer behind
  it (a scan of `Sidebar.tsx` holds that; the first paint still reserves the row for one frame, until
  `usePlatformWords` answers, which is recorded rather than hidden). `lib.rs` asks the Windows webview for
  `SetAreBrowserAcceleratorKeysEnabled(false)` inside `with_webview`, best-effort; `webview2-com` 0.38 and
  `windows` 0.61 became direct Windows dependencies and the lock did not fork (they were already there
  through wry). Plants 6/6; verify green. Found on the way: a bare `assert!` on a const is
  `clippy::assertions_on_constants`, which the bar denies — the flag test asserts through locals.
  **Both measured on the Dell the same day** (the owner at the keyboard, one screenshot against the earlier
  one): Ctrl+R, F5, Ctrl+P, Ctrl+F and F12 do nothing now, while Ctrl+A/C/V/Z still work in a field; the
  wordmark sits ~14px higher, so the strip under Windows' own title bar is gone. Also learned, and not
  hidden: the Mac ran out of disk mid-verify (the Windows caches held 19 GB) — the owner ruled they be
  cleared, which is why that verify was re-run from cold.
  **S3a done 16 Sep 2026 (ledger #638), L0.** Five words joined `PlatformWords` — `trust_store`,
  `privileged_prompt`, `os_name`, `ca_target`, `elevation_note` — and the screens ask for them: Settings'
  Local CA card (its re-trust toast, its "trusted · …" line and the Re-trust description), Settings' TLD
  copy, SiteDetail's domain dialog, Onboarding's first screen, and the five MCP consent descriptions in
  `user_sites.rs`. The frontend scan grew "keychain", "your Mac" and "with macOS", which immediately caught
  two of my own type comments quoting macOS examples. Plants 5/5. Left for S3b: the Rust half — "this Mac"
  (`core/dns.rs`'s prompt reason and the onboarding copy `core/proxy.rs` guards), `lib.rs`'s "staying in the
  menu bar", `traits.rs`'s "macOS refused the operation", Settings' two "/Applications and ~/Applications"
  tooltips — and a Rust-side scan, since nothing yet stops a literal being typed back there.
  **S3b done 16 Sep 2026 (ledger #639), L0.** Three more words — `host` ("this Mac" / "this PC"), `tray_home`
  ("menu bar" / "notification area") and `app_search` (`/Applications and ~/Applications` / the installed-
  programs list, Program Files and `%LOCALAPPDATA%`, which is where `app_catalog.rs` actually looks) — and
  the eight sentences that wrote them: the resolver prompt, the disk-shortfall message, `SwapFailure`'s
  Display, the login-launch log line, Onboarding's edge-conflict notice, the zip panel, Settings' two
  detection tooltips and its three "on this Mac" lines. `settings_access.rs`'s denial reason is a
  `&'static str`, so it was reworded ("hold rexenv on a superseded build") rather than made dynamic.
  The new guard is the frontend scan's twin over `src-tauri/src` + `cli/src`; its first run read doc comments
  as violations, so it strips comment lines the same way. Three exceptions are named with their reason, all
  macOS-only code paths. Left for the owner: the Dell look at the Windows wording once S4 and S5 land.
  **S4 done 16 Sep 2026 (ledger #640), L0.** Three path words — `routes_label` ("file under /etc/resolver" /
  "NRPT rule", the name `WindowsDns::route_label` already used), `home_prefix` ("~" / "%USERPROFILE%") and
  `import_search` — and the five places that wrote a path themselves: the teardown confirmation, the uninstall
  toast (whose "run `valet install`" advice became "the other tool will need to put its own back", Valet not
  being a Windows thing), the MCP card's config path, Import's empty state, and the sites-folder tooltip.
  That last needed no word: `default_sites_dir` is `<home>/rexenv/Sites` on every OS and the resolved path is
  already on screen above the button, so the literal was simply dropped. The frontend scan now forbids `~/`
  and `/etc/resolver`, which immediately caught two comments spelling `/etc/resolver/test`. Plants 5/5.
  **Measured, and open for the owner:** the Valet/Herd/Local importer cannot work on Windows as written —
  `valet_homes`, `herd_home` and `local_home` all build macOS layouts (`Library/Application Support`), while
  Herd on Windows keeps its tree under `%LOCALAPPDATA%`. On the Dell that card will always say "none found".
  Its words were honest already; **the owner ruled 16 Sep 2026 — say so now, port later**, done
  the same day (ledger #641): `PlatformWords::imports_other_tools` is false on Windows, `scan_valet_import`
  skips BOTH discoveries there, and `ImportScan::unsupported` carries one sentence — built in core from
  `os_name`, so the screen cannot drift from it — which the empty state shows instead of "none found". The
  consent cards and the leftover-dumps card still render there: leftover routes and leftover dumps are real
  on Windows. Plants 4/4, one of which first came back as an ANCHOR MATCHED 2 TIMES non-verdict (the string
  also appears in the guard's own assertion) and was re-run with a unique anchor. The port of the Windows
  layouts (`%LOCALAPPDATA%\Herd`, Local's AppData tree) stays open.
  *Done when:* on the Dell, no screen names a Mac thing; Ctrl+R does not reload; the window looks like a
  Windows app (native title bar, no dead row); and a path shown as an example reads `C:\…`.
- **W10 — Feature gates per D4.** Refused in core, honest in the UI, one arm to enable later.
  MariaDB joins Redis, Apache and Xdebug unless ruled in. §3a Q3's per-OS answers land with
  the feature each one gates, not here in a batch.
  **Core half done 16 Sep 2026 (ledger #642), L0.** The gate is not a list: `binaries::ships_on`
  asks `manifest`/`bundle_manifest`, which are already keyed by OS, so D4's Windows v1 set IS the
  pins — the day a Redis pin lands the engine appears with no gate edited. Four call sites:
  `DbEngine::available_on` (the one filter `db_status`, the port list, adoption, the download plan
  and the MCP context already share), `ensure_server_available_on`, `xdebug_status_on` and
  `spawn_db`. Each takes the os as a parameter, because the bar only `cargo check`s for Windows —
  a gate that read `std::env::consts::OS` would have an untestable half until W12's runner, and an
  untestable refusal is one nobody has seen refuse. Plants 6/6.
  **Two traps, both self-inflicted, recorded because they cost a chain each:** the os rule inside
  `xdebug_status` closed a cycle through `bundle_manifest` and blew the test binary's stack (hence
  `xdebug_row`, the table, separate from the policy); and the plant harness counted multi-line
  anchors with `grep -cF`, which treats each newline as a pattern, so three plants reported 2/198/360
  matches and were skipped — non-verdicts that would have read as done.
  **UI half done 16 Sep 2026 (ledger #643), L0.** `core::sites::offered_web_servers_on` derives the
  picker's options from the same gate that refuses, a new `offered_web_servers` command carries them (with
  its own DISPOSITIONS ruling — an agent names a server and is refused by core, it does not pick from a
  menu), and `NewSiteDialog` renders that set, resets a choice a blueprint made that this build cannot
  serve, and says why a server is absent. Plants 3/3. The list takes the os as a parameter for the reason
  the rest of W10 does — the first version read `std::env::consts::OS` and its Windows half could not have
  failed here. **Still unmeasured on Windows:** every W10 claim is L0 on the Mac; the Dell sees it when it
  is back.
- **W11 — Packaging and updates per D5.** NSIS bundle, signing (only after D5's measurement and ruling), a Windows job in
  `.github/workflows/release.yml`, Windows `AppBundle`, winget manifest.
- **W12 — Launch gates.** **The script side was SURVEYED 16 Sep 2026** (read, not run — there is
  still no Windows host). What breaks under Git Bash, and what was done about each:
  **Fixed here, because each is portable BY CONSTRUCTION and stays green on the Mac:**
  `verify.sh`'s `mktemp -t rexenv-windows-check` (BSD appends the `XXXXXX`, GNU refuses a template
  without them — the template now carries its own, valid on both, and under `set -euo pipefail` the
  GNU error would have killed the bar before the receipt); `status.py`'s four `read_text()` calls
  (Windows decodes cp1252 and dies on TODO.md's em-dashes and emoji), its `write_text` (now
  `newline="\n"`, so a Windows regeneration is not a whole-file diff) and its two `sh(["./scripts/…"])`
  calls (`CreateProcess` cannot execute a shebang script — they go through `bash` now); and a
  `.gitattributes` with `eol=lf`, because a default Git for Windows checkout rewrites every file
  with CRLF and the doc gates read line endings as CONTENT — `doc-counts` greps
  `^#[tauri::command]$` and would find zero commands, which reads as a baffling failure rather than
  a line ending. The tree carries no CRLF today (checked), so the file adds no diff.
  **MEASURED ON THE HOST 17 Sep 2026, and half the survey's list was wrong.** The Dell was read
  instead of reasoned about (Git Bash 5.2.26, MINGW64), and the four items above that "needed the
  host to verify" split three ways:
  *Guessed wrong, no fix needed:* `xxd` IS in Git Bash (`/usr/bin/xxd`), its OpenSSL is 3.2.1 and
  signs Ed25519 with `pkeyutl -rawin` (measured, 64-byte signature), and `jq` — which the survey
  never flagged and which Git Bash lacks — is not a dependency at all: every `--jq` in the tree is
  `gh`'s own built-in.
  *Guessed right, fixed here:* `shasum` is absent and `sha256sum` present, so `verify-receipt.sh`
  resolves the hasher once and **fails loudly** when neither exists — the old `|| true` degraded
  into an empty fingerprint, which does not read as "this host cannot hash" but as "the code
  changed", on every commit forever. And `check-app-manifest-test.sh`'s `file://` fixtures: native
  `curl.exe` resolves a POSIX path against the drive root, so every fixture URL fetched NOTHING —
  and silently, because "the CDN has nothing published" is a green exit, so the whole test would
  have passed while measuring nothing. `cygpath -m` where it exists, the plain form on macOS.
  *Fixed here, both needing the host to prove:* `build.rs` stages the sidecar on a Windows host
  too, through `scripts/build-cli.sh`'s existing MINGW arm — never a placeholder, for the reason
  `windows-check.sh` states about its own; and `windows-check.sh` grew a native arm, because the
  one machine that could check Windows natively was the one machine where the gate was INERT (no
  `brew` → exit 3 → SKIPPED). `sh`, `date` and `uname` are on the PATH a build script inherits from
  bare cmd.exe, so the two arms differ only in which staged file they look for.
  **The real unknown is not a script question, and it is being measured for the first time:**
  `windows-check.sh` only ever proved `cargo check` — it does not link and runs no test — so
  whether the 1376 lib tests and the CLI's 25 pass on a Windows host had never been measured.
  `cli_server.rs` alone has 18 `UnixStream` uses. The Dell needed no toolchain install for this
  (rustup, MSVC Build Tools 2022 and the 10.0.22621 SDK were already there, and `rustc` links);
  the tree reached it as a `git bundle`, so no credentials and no GitHub. Expect that run, not the
  script edits, to be the bulk of W12.
  **THE RUN HAPPENED, 17 Sep 2026: `1232 passed; 79 failed` of 1311**, plus the `cli` crate's 23
  all green. Two things had to be fixed before a single test could start, and both are the kind of
  defect only a host finds: the sidecar staging (#652), and an application manifest for the test
  binary. The second is worth the detail, because the obvious fix is the wrong one — the test
  binary died at load with STATUS_ENTRYPOINT_NOT_FOUND looking for comctl32's `TaskDialogIndirect`
  (Common-Controls v6, which a binary must ask for in a manifest; rfd under tauri-plugin-dialog
  imports it; Tauri gives the APP exe that manifest and a `cargo test` binary has none). Putting the
  linker arg in `build.rs` runs the tests AND kills the app binary with LNK1123 against Tauri's own
  manifest resource; `rustc-link-arg-tests` is refused by cargo because the crate has no `[[test]]`
  target. The arg belongs on the test INVOCATION, which is where `verify.sh`'s Windows arm now puts
  it (#653).
  **The 79 failures are six families, and in most of them the code is right and the test is old:**
  symlink fixtures (22, all through `test_support::symlink`; measured on the Dell the same day,
  `SeCreateSymbolicLinkPrivilege` IS available to an elevated task, so a Windows arm there would
  really run — but a developer without Developer Mode or elevation would skip, so the helper must
  try-and-skip rather than claim); test paths spelled `/logs/x` where production does
  `log_dir.join(key)` (~10 — of 126 path comparisons against a `/` literal in the tree, exactly 2
  are in production code, so this is a fixture assumption, not a port bug); fixtures spawning
  `yes`/`sleep`/`sh` (~10; `yes` never exits, so two tests hang until the process is killed, and
  `sh` gives `193: not a valid Win32 application`); tests demanding macOS-pinned artifacts (~10,
  e.g. `plan_for_php_maps_minor_to_pinned_fpm_and_cli` wanting a `php-fpm` pin on an OS with no
  php-fpm — the code is correct and the test encodes the old world); macOS-only tool scans (~6:
  valet, localwp, firefox, devtools); macOS home paths (2). Owner's ruling: family by family,
  largest first, one commit each, measured on the Dell.
  **The find that is worse than a red test:** `state/store.rs`'s SQL scan compares paths against
  `"src/state/"` and matched NOTHING on Windows — it went red only because it carries a canary
  (`only 0 SQL hits — the matcher is broken`). The same shape without a canary passes while seeing
  nothing: `mcp_server/user_sites.rs` (14 such comparisons) and `mcp_server/scratch.rs` (6) have
  none. `platform/words.rs` is the counter-example that shows it was already known there — it
  normalises with `replace('\\', "/")` before every comparison.
  **Still host-blocked, and the owner ruled on 17 Sep to install both:** Node is v14.17.6 (tsc 5.7
  and eslint need 18+), so `npx tsc` / `npx eslint` cannot run there yet; and `python3` on PATH is
  the Microsoft Store alias stub, so both `.py` gates fail on the name while `python` 3.9.7 works.
  Until those land, `verify.sh` has never completed on Windows, which is why #652's other three
  arms (`sha256sum`, `cygpath`, `windows-check`'s native arm) are still unproven there.
  Still required: `verify.sh` runnable on the Windows runner (Git Bash);
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
- **A successful bind proves nothing — the general Windows rule (owner, 13 Sep 2026).**
  Windows lets a later socket bind a port already in use when it sets `SO_REUSEADDR` (php-cgi's
  `fcgi_listen` does), and lets a specific-address bind coexist with another process's
  wildcard bind unless one side set `SO_EXCLUSIVEADDRUSE`. "We could bind it" and "it is ours"
  are different facts for every port rexenv uses — D1's php-cgi and D2's :53 are two cases of
  this one rule, not two rules. Consequences, W3 unless noted:
  - `core/ports::is_free` is a trial bind on `127.0.0.1` (`ports.rs:58`); on Windows it can
    say "free" while another process listens on `0.0.0.0` or `[::]` of the same port. There,
    `ensure_free` reads the TCP/UDP tables (`GetExtendedTcpTable` / `GetExtendedUdpTable`)
    for ANY local address on the port and names the holder from them.
    **Written 13 Sep 2026 (ledger #599) and run on the Dell the same day — `windows_port_gate_check: PASS`:**
    `ports::is_free` now takes the platform and refuses a port any row names —
    `ProcessSupervisor::port_holders`, both address families — BEFORE its trial bind, so no
    caller can reach the bind-only answer. The bind stays as a second refusal, not a
    verdict, for whatever refuses a bind without a table row. **Measured the same day, an
    administered excluded range is not one of those:** inside the Dell's 50000–50059 (`*`),
    `127.0.0.1`, `0.0.0.0` and `[::]` all listened and answered and UDP bound — an exclusion
    stops Windows handing the ports out, not a program asking for one; WinNAT's run-time
    ranges are reported to refuse binds, unmeasured here (no Hyper-V/WSL distro/Docker on the
    Dell). The first Dell run had asserted the opposite and failed on it. The conflict names the ROOT holder (parent links dropped when the parent is
    younger than the child — pid reuse, D1(a) rule 4) by image, by hosted service for a
    `svchost.exe`, as HTTP.sys for pid 4, or — with no holder — as the excluded range from
    `netsh interface ipv4 show excludedportrange`, parsed by row shape so a localized netsh
    reads the same. Our own leftover is found by the app-data path on its command line,
    ignoring case, and gets `Stop-Process -Id`, not `kill`. The pure rules live in
    `platform/windows/port_table.rs` (tested on every host), the Win32 reads in
    `process.rs`; `examples/windows_port_gate_check.rs` is the Dell run.
    **macOS turned out to have the same hole** (measured the same day on 26.6.2: TCP only — UDP
    and a `127.0.0.1` holder are refused by the bind) and got `port_holders` from `lsof` at the
    owner's go, so the rule is one rule on both OSes, not a Windows special case.
  - A start is confirmed by who ANSWERS with our identity — a FastCGI round trip for a
    php-cgi group, the admin pipe for Caddy, the handshake for MySQL/PostgreSQL, a marker
    record for the DNS agent (W6) — never by a bind or a listen. That is the existing
    "ownership AND liveness" non-negotiable; on Windows the bind is one more thing it must
    not trust.
  - Sockets rexenv opens itself (the DNS agent, probe listeners) set `SO_EXCLUSIVEADDRUSE`, so
    nothing shadows them. A third-party server's socket options are its own, which is why the
    identity check is the rule and not the option.
  - **Measured 13 Sep 2026 on the Dell** (Windows 10 22H2, over SSH — an elevated admin token,
    one user; `scripts/probes/windows-bind-matrix.ps1`, 288 binds): A binds first, B second, on
    the same port, then a connection or datagram goes to `127.0.0.1`. TCP and UDP came out
    IDENTICAL. `x` = in use, `acc` = access denied, `ok:A/B` = both bound and that one received:

    | A \ B | 0.0.0.0 | 127.0.0.1 | 127.0.0.1 +REUSEADDR | 127.0.0.1 +EXCLUSIVE |
    |---|---|---|---|---|
    | 0.0.0.0 | x | **ok:B** | **ok:B** | ok:B |
    | 0.0.0.0 +EXCLUSIVE | x | acc | acc | acc |
    | 127.0.0.1 | ok:A | x | acc | x |
    | 127.0.0.1 +EXCLUSIVE | ok:A | x | acc | x |
    | `[::]` dual-stack | ok:B | **ok:B** | ok:B | ok:B |
    | `[::]` dual-stack +EXCLUSIVE | acc | acc | acc | acc |
    | `[::]` v6-only | ok:B | **ok:B** | ok:B | ok:B |

    Three consequences. (1) **`is_free`'s trial bind (`127.0.0.1`, default options — what
    Rust's `TcpListener::bind` does) reports FREE while another process holds the port on
    `0.0.0.0` or `[::]`, and once rexenv binds, `127.0.0.1` traffic goes to rexenv's socket,
    the more specific one** — a developer's own MySQL on `0.0.0.0:13306` would lose its
    localhost clients to ours without any conflict being reported. It fails correctly only
    when the holder is on `127.0.0.1` itself or bound its wildcard EXCLUSIVE. (2) php-cgi's
    `SO_REUSEADDR` cannot take `127.0.0.1` from a server bound there with default options
    (access denied); against a wildcard holder it wins localhost exactly as a default bind
    does — the option adds nothing there. (3) `SO_EXCLUSIVEADDRUSE` on `127.0.0.1` refuses
    every later bind of that address and keeps localhost traffic when a wildcard binds after
    it — the right option for the DNS agent and rexenv's own listeners. **The desktop user's token
    gives the same answer:** re-run the same day as an Interactive, RunLevel Limited task
    (`scripts/probes/windows-limited-token.sh`; the wrapper recorded `token elevated: False`,
    Medium integrity), all 288 rows came back identical to the elevated run. **Not measured:**
    a holder running as another account (a service as LocalService/SYSTEM), and Windows 11.
  Not new to Windows in kind: on macOS, Herd shadow-binds 127.0.0.1:443 with no bind error,
  and what caught it was checking who answered.

## 7. What an agent on the macOS dev machine cannot prove

The Mac proves COMPILATION for Windows (W0's cross-check) and nothing else — no Windows
test, no link, no run. Everything past W2 needs Windows itself. The owner's machines
(12 Sep 2026): a **Dell Inspiron 3543** (i7-5500U, 8 GB — real x64, low-end, not on
Windows 11's supported-CPU list, too slow to be the build box: build elsewhere, run
there) is the release-gate machine — **measured 13 Sep 2026: Windows 10 Pro 22H2 (build 19045),
not 11**, which D6 calls best-effort, so it cannot be the gate for the "supported" Windows 11
alone. Reached from the Mac over OpenSSH (key auth; the default shell is Windows PowerShell 5.1,
so probes go through `-EncodedCommand`); an SSH session carries an ELEVATED admin token, unlike a
desktop user's filtered one, so anything token-sensitive must also be measured from the desktop's token —
`scripts/probes/windows-limited-token.sh` runs a probe as an Interactive, RunLevel Limited
scheduled task in the logged-on session, which measured Medium integrity (an S4U task with
Limited still ran High). The Dell is on 2.4 GHz Wi-Fi, and SSH sessions to it dropped for
minutes at a time, so probes use short sessions and keepalives. **Its adapter power saving was
turned off 13 Sep 2026 at the owner's go:** "allow the computer to turn off this device" went
from on to off through `root\wmi` `MSPower_DeviceEnable` (no adapter restart, the link stayed
up; set `Enable` back to `$true` to undo); the power plan's wireless Power Saving Mode was
already Maximum Performance on AC and battery, and the driver's Minimum Power Consumption was
already off, so neither was touched; a Windows 11 ARM VM on the M3 Pro Mac runs the x64
build under emulation for the day-to-day loop, and never counts as the x64 proof. Both
are driven over OpenSSH from the Mac; dialogs are read by a human. `docs/TESTING.md` gains a Windows column when W0 lands — not before, so it never
claims coverage that does not run.

## 8. Docs this changes as it lands

`docs/ARCHITECTURE.md` (the OS rule, per-OS process model, DNS, IPC), `docs/MAP.md`
(new trait, Windows modules), `docs/PORTS.md` (the php-cgi group on the existing pool ports — D1(a), no new block; DNS :53 on Windows,
Windows pins), `docs/CLAIM-LEDGER.md` (#163 widened; every new "never"), `docs/TESTING.md`,
`docs/SMOKE-TEST.md`, `docs/INSTALL.md`, `docs/RELEASING.md`, `docs/DESIGN.md`,
`docs/CLI-ROADMAP.md` (named-pipe transport).

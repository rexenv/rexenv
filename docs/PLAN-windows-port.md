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
  silently becomes a pool that dies with the app.
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
    - *Churn breaker, after the start:* each health tick already snapshots the group for
      positive ID (pids + creation times). A child younger than a few seconds being replaced
      again, more than 2 × workers times in 10 s — or "unable to spawn" appearing in the
      parent's stderr at all — stops the group: `TerminateProcess` on the parent (the job
      reaps the children), the service marked failed, the parent's stderr quoted. Legitimate
      churn from `PHP_FCGI_MAX_REQUESTS` needs that many requests per child; at 10 workers
      the threshold means thousands of requests in 10 s, not a developer's load. The number
      is a Dell measurement, not a guess to keep.
    - *A serving check, not a process check:* readiness and health are a FastCGI round trip,
      so case (2) — processes alive, nothing answering — is "not running", which is D1's
      ownership-AND-liveness rule already. *Done when (W4):* a fixture whose child cannot
      be spawned and one whose script kills the worker each end in a stopped group with the
      reason on screen, within the breaker's window, CPU idle afterwards.
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
  is wrong for somebody.

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
WSL 2 kernel**, so the WSL states cannot be measured here without installing one. The §6 matrix
already answers "who answers": a `127.0.0.1` bind wins loopback traffic over a `0.0.0.0` or
`[::]` holder, so a wildcard :53 holder would not stop the agent — only a `127.0.0.1` holder or
an exclusive wildcard would, and those are the ones to refuse by name.

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

**D4 — What ships in Windows v1.** **RULED 13 Sep 2026: accepted as written.**
Official, checksum-lockable artifacts exist for most
of the stack (§4). Redis has no official Windows build; Apache on Windows means Apache
Lounge (a third-party trust decision); Xdebug DLLs must match PHP's NTS + compiler.
*Recommendation:* v1 = Caddy, nginx, PHP 7.4–8.5, MySQL, PostgreSQL, Mailpit, Adminer,
WP-CLI, Composer, cloudflared; FrankenPHP if W4 proves it. Redis, Apache and Xdebug are
refused in CORE on Windows with an honest message — the shape OpenLiteSpeed already
uses (`ensure_server_available`) — until each is proven.
**MariaDB — RULED 13 Sep 2026: not in v1.** macOS ships it (bundle pins), there is no
Windows pin, and it stays on the refusal list with an honest message (W10). *Why (owner):*
MySQL 8.4 and 8.0 both ship on Windows, so MariaDB is redundant for v1, and a new pin is a
new trust decision, a notices row and a sweep target. Windows users asking for it is the
evidence that would add it.

**D5 — Signing, installer, updates, distribution.** **RULED 13 Sep 2026: signing is NOT
decided — measure first.** NSIS (per-user, no admin to install) and the distribution line
stand as the direction.
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
answer; what is new is the swap (a running `.exe` cannot be replaced: stage, exit through
the gate, a relauncher swaps and starts).
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
- **W4 — Serve a WordPress site.** D1's php-cgi group — its measurements first (§3 D1(a)), then the group, its
  positive-ID chain and D1(b)'s busy-workers signal; nginx Windows config (forward
  slashes, every path quoted), mail through the SMTP ini keys, WP-CLI/Composer via the
  site's PHP. *Done when:* a one-click WordPress site loads through nginx and its mail
  lands in Mailpit.
- **W5 — HTTPS edge.** Caddy on :443 (Windows has no privileged ports, so the edge need
  not run elevated — record why in ARCHITECTURE), admin per D3, `EdgeSupervisor` for that
  shape; `CertTrustManager` into the CurrentUser Root store (Windows shows its own
  confirmation) plus the Firefox enterprise-roots path. *Done when:* `https://<site>.rex`
  shows a valid lock in Edge, Chrome and Firefox.
- **W6 — DNS + privileges.** D2's :53 measurement first, then the agent, the NRPT rules and the refusal that names a
  :53 holder; `PrivilegeManager` as a UAC
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
  MariaDB joins Redis, Apache and Xdebug unless ruled in. §3a Q3's per-OS answers land with
  the feature each one gates, not here in a batch.
- **W11 — Packaging and updates per D5.** NSIS bundle, signing (only after D5's measurement and ruling), a Windows job in
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

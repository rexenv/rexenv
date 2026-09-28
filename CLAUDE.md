# CLAUDE.md — rexenv

rexenv is a native, **no-Docker** local development environment for web & WordPress
developers: a Tauri 2 desktop app (Rust backend + React/TS frontend) that runs the whole
local stack — edge proxy with auto-HTTPS, shared web server, multi-version PHP, MySQL/
PostgreSQL, one-click WordPress, `.rex` DNS, mail catching, tunnels — from one UI.
**rexenv is built for macOS, Windows AND Linux (Ubuntu 22.04+)** — one tag builds all three
(`docs/RELEASING.md`), and every new feature or fix is built for all three IN THE SAME CHANGE
(see "All three platforms" below; the per-OS reference is `docs/PLATFORMS.md`). No platform
has a `todo!()` left: a piece an OS cannot do yet fails as `Error::Unported`, never silently.

This file is a ROUTER. Read only what the task needs (table at the bottom).
The system mental model lives in `docs/ARCHITECTURE.md` — read it for any feature or bug.

## Start here — an agent's first three commands

```
./scripts/status.py            # what is open, by section, with TODO.md line numbers; ledger tally; plans in flight
cat docs/STATUS.md             # the same, committed (verify.sh + pre-commit fail when it drifts)
cat docs/archive/README.md     # what every historical file records, incl. the shipped design records
```

Then the router at the bottom for the doc the task needs. Never answer "what is
pending" from a doc's prose — three reconciles found rows open only on paper and
headers saying "not started" about shipped features. Derived beats typed.

## Project skills (`.claude/skills/`) — the repeated workflows, written down once

| Skill | Use it when |
|---|---|
| `status` | session start; "what's open / ki baki" |
| `finish-task` | a task is done and about to be committed — the docs-table walk, the tick, STATUS regen, verify, explicit staging |
| `verify` | before any commit touching `src/`, `src-tauri/`, `cli/`, `scripts/` — how to run the bar so its verdict counts |
| `reconcile-todo` | TODO.md has ticked rows, or after a release — `scripts/todo-reconcile.py` + the four judgement shapes |
| `ledger-row` | you wrote or changed an invariant comment — the row, the verdict, the tally, same commit |
| `live-check` | an L1 proof is owed, or "run it live" — examples by tier, fixture-owned everything |
| `release` | bump / tag / release — the order, the four manifests, the CI build of every OS, the human gates the agent cannot run |
| `all-platforms` | designing or building ANY feature or fix — the common/per-OS split, the three-OS checklist, per-OS proof |

## All three platforms, in the same change (non-negotiable)

**"All OSes" means exactly three: macOS, Windows AND Linux** — in every doc, script and
skill here, never two of them. A feature that works on one OS is a third of a feature. All
three are build targets, so "Windows/Linux later" is not a plan — it is a bug with a date on it. Owner
ruling, 20 Sep 2026 (macOS + Windows then), after a week in which four Windows-only defects shipped to a
user because the work had been written for macOS and compiled for Windows; widened to Linux
when the Linux build landed (24–27 Sep 2026). **Read `docs/PLATFORMS.md` before designing
any feature** — §1 what is common, §2 where a difference goes, §3 the per-OS mechanism
table, §4 the checklist, §6–§8 each OS's traps. The `all-platforms` skill walks it.

**Common is written ONCE; different is written PER OS, all three filled:**

- **Common → `core/` (or the frontend), once.** Product rules, topology, config generation,
  SQLite, port gating, the binary catalog's pins. `core/` may not name an OS — no
  `cfg(target_os)` there (a guard enforces it, ledger #163): ask the platform for a
  CAPABILITY instead — `may_spawn_with_admin_token` is the shape (#698).
- **A different VALUE → `platform/words.rs`** (`PlatformWords`: `MACOS`, `WINDOWS`, `LINUX` —
  all three filled). A path, a URL or origin, a command line, a refusal, a user-facing
  sentence. Spelling macOS's answer inline and "handling Windows later" is how the Database
  Browser shipped pointing at `rexdb://localhost`, a URL no Windows webview can load (#703)
  — the file even carried a "Windows … Phase 4" comment.
- **A different BEHAVIOUR → a trait method in `platform/traits.rs`**, implemented in
  `platform/{macos,windows,linux}/`; its pure text/rules in a `*_rules.rs`-style module
  tested on any host. An OS that cannot do it yet returns `Error::Unported` / `unported!` and
  gets a TODO row — never `todo!()`, never a silent no-op.
- **A UI string that describes a RULE lives in Rust**, beside the rule, and the TSX renders
  what it is sent (the consent and restart sentences; `copy_scan` enforces it).
- **Tests assert the platform's answer**, never one OS's literal — the first Windows run of
  the bar failed two tests that asserted `$ sudo chown` and a `:`-joined PATH.
- **Both ends of a policy, not one.** The Database Browser needed the app's own `frame-src`,
  Adminer's replayed `frame-ancestors` AND the frame's URL to be right (#699, #702, #703):
  fixing two of three still rendered an empty panel.
- **A look-alike mechanism is not the same mechanism.** Linux's first DNS route (a global
  resolved drop-in, shaped like `/etc/resolver`) sent the WHOLE internet to loopback (#717).
  Read the OS's column in `docs/PLATFORMS.md` §3 before reusing another OS's design.
- **`windows-check` and `linux-check` are COMPILE gates, not verdicts.** They say the build
  exists, never that the feature works there. A behaviour claim needs a RUN on that OS —
  `docs/TESTING.md` §"Proving a Windows claim" / §"Proving a Linux claim" (the trap: a bare
  `cargo`/`cargo-xwin` `rexenv.exe` is a DEV build whose webview loads `localhost:1420`;
  only `tauri build` produces something a GUI can be tested in).
- **The ledger row says which OS the proof came from.** "Proven" on one OS only is a `◐`,
  and `docs/SMOKE-TEST.md`'s section for each unproven OS carries the row a human must run.

## Architecture rule (non-negotiable)

- `commands/` are **thin** Tauri IPC handlers — translate calls, invoke `core/`, nothing else.
- `core/` is **platform-agnostic** ("the what") — never imports OS-specific code.
- ALL OS-specific code lives ONLY in `src-tauri/src/platform/`, behind the 13 Rust traits
  in `platform/traits.rs`, impls selected via `#[cfg(target_os)]`: `macos/`, `windows/`,
  `linux/`. All three are real for everything that ships; a piece still missing on an OS
  returns `Error::Unported` or panics via `unported!` (never `todo!()` — ledger #595, widened
  to Linux #716). Adding an OS = filling stubs, NOT restructuring — the Linux port proved it
  (24 Sep 2026, two days, `core/` untouched). The few `cfg(target_os)` branches outside
  `platform/` are app-shell wiring (`lib.rs`/`main.rs`), inventoried in
  `docs/PLAN-linux-port.md` §1.1 — a new one gets a row there saying what the other OSes get.
  Design records: `docs/PLAN-windows-port.md`, `docs/PLAN-linux-port.md`.

## Non-negotiables (foundational — don't relitigate)

Common to every OS. Where a rule is kept by a different mechanism per OS, the line names
the promise and `docs/PLATFORMS.md` §3 names the mechanism; the OS-only rules follow.

- **No Docker.** Native static binaries, downloaded on demand through `BinaryProvider`
  (pinned versions, checksum-locked, one catalog arm per OS + arch). Each OS prepares a
  binary its own way (`prepare_binary`) — PLATFORMS §3.
- **Request topology (default sites):** browser → Caddy `:443` (TLS, local-CA certs,
  auto-HTTPS disabled) → ONE shared Nginx `:18088` (vhost by `server_name`) → one PHP pool
  **per PHP version** (not per site; php-fpm on macOS/Linux, a php-cgi group on Windows) →
  WordPress → DB. No direct Caddy→PHP.
  FrankenPHP per-site overrides are loopback backends only — never the edge, never `:443`.
- **Edge admin = private local socket (`0600` / owner-only), NEVER TCP `:2019`** — a TCP
  admin on a root Caddy is arbitrary file r/w as root. rexenv never binds nor queries TCP
  2019, so it can never touch a developer's own Caddy. (Unix socket on macOS/Linux; the same
  AF_UNIX socket through Winsock on Windows, #611.)
- **Services OUTLIVE the app.** Closing rexenv stops nothing; on launch `adopt_startup`
  adopts rexenv-owned survivors (ownership = our fixed port + app-data marker on the
  cmdline; root edge via our admin socket).
- **"Running" = ownership AND liveness — never trust a bare port-listen.** The
  ServiceManager, not the monitor, is the source of truth for status.
- **Locking rule:** never hold the services lock across a wait — spawn under the lock,
  return `ReadyCheck`s, `await_ready` after dropping it. Status polls `try_lock` a
  snapshot; only `ServiceManager` sits behind an async Mutex (AppState = field-level locks).
- Embedded DNS (hickory) is **always-on and OUTLIVES the app** — `*.rex → 127.0.0.1` (any
  configured TLD; `.rex` is the always-installed backbone) on `platform::RESOLVER_PORT`
  (UDP 15353; 53 on Windows), served by a per-user `--dns-agent` with no privilege
  (LaunchAgent / logon scheduled task / systemd user unit); in-process only as automatic
  fallback; not a ServiceManager service; watchdog kickstarts/restarts (max 3).
- **System changes only through platform traits.** The per-TLD DNS route = privileged op via
  `PrivilegeManager` (`/etc/resolver/<tld>` / an NRPT rule / the `rexenv0` link, PLATFORMS
  §3); CA trust via `CertTrustManager`, into the USER's store wherever the OS has one.
  Privileged prompts must run foreground. A file somebody else owns (`hosts`, a global
  resolver config) is never overwritten. TLS leaves ≤398 days (Safari cap, enforced for all).
- **SQLite** for all app state. Config generator keeps three rewrite templates:
  single / subdomain-multisite / subdirectory-multisite. **Quote all paths** in generated
  Caddy/Nginx configs (app-data paths contain spaces).
- Every service start is port-gated via `core/ports::ensure_free`; conflicts name the
  holder + a copy-paste fix.

**OS-only non-negotiables** (the full trap lists: `docs/PLATFORMS.md` §6–§8):
- **macOS** — `prepare_binary` order: de-quarantine → relink Homebrew dylibs to `/usr/lib` →
  ad-hoc codesign **LAST**. CA trust in the **login** keychain (the System keychain is
  impossible from detached-root osascript). The UI engine is WKWebView — check UI in WebKit.
- **Windows** — the resolver is on :53 (NRPT has no port); trust goes to
  `CurrentUser\Root`, never LocalMachine (#613); the edge binds `127.0.0.1` only; `hosts` is
  never touched; only `tauri build` makes a testable app.
- **Linux** — never a global systemd-resolved drop-in (#717: it routed the whole internet to
  loopback); `pkexec` strips `PATH`, so privileged commands are absolute-pathed; trust goes
  to the system store AND the NSS dbs (snap browsers included, #726); release builds on
  22.04's glibc.

## Tech stack & conventions

- Rust: Tauri 2, tokio, hickory-dns, rcgen, reqwest, rusqlite, sysinfo.
  Frontend: React + TS (**strict**), Vite, Tailwind + shadcn/ui, TanStack Query (server
  state), Zustand (UI state), xterm.js.
- All IPC through typed wrappers in `src/lib/ipc/` — UI never calls raw `invoke`.
- Design tokens from `src/styles/tokens.css` + Tailwind theme — never hardcode hex.
  JetBrains Mono for ALL technical values; Space Grotesk hero/onboarding only; Inter body.
- `src/routes/` map 1:1 to screens (Sites, SiteDetail, Services, Databases, Mail,
  Tunnels, Settings, Onboarding).
- Verification: **`scripts/verify.sh` is the pre-commit bar** (lib tests + `cli` tests +
  example builds + clippy `--all-targets` at zero in BOTH crates + tsc + eslint (two
  react-hooks rules, NOT a style linter — see `eslint.config.js`) + the
  generated-doc gates, `ledger-tally.sh`, `doc-counts.sh` — the latter also fails
  on any `docs/*.md` path the tree cites that does not exist — `notices-check.py`
  (THIRD-PARTY-NOTICES' Rust, npm and vendored-composer tables against what ships, both directions),
  `check-app-manifest-test.sh` (the release check tells CDN lag from a missed publish, offline) and `status.py --check`,
  which fails when `docs/STATUS.md` no longer matches TODO.md / the ledger / the plans) +
  the two cross-OS COMPILE gates: `windows-check.sh`, the Windows x64 compile of both crates
  — `verify: windows-check SKIPPED` on a machine without cargo-xwin, llvm/lld or
  `XWIN_ACCEPT_LICENSE=1`, a red bar on any real Windows break (ledger #584) — and
  `linux-check.sh`, both crates inside an Ubuntu 22.04 container — SKIPPED without a running
  Docker (ledger #719). A SKIPPED gate proved nothing for that OS; say so. A green verdict comes ONLY from the script's own
  `verify: all green` line — an ad-hoc `cargo test`/`tsc` invocation is never a gate:
  it can silently run from the wrong cwd (shell state resets between tool calls) and
  a `&&`-chain then passes on partial checks, exactly as a piped exit code once
  masked a failing tsc. The script sets its own cwd and its exit code is the
  verdict. Live-check `examples/*.rs` — examples run against
  REAL app data and processes, so anything they write/spawn/delete MUST be
  fixture-owned: `common::sandbox()` for a throwaway `Platform`, `common::Reaped`
  for spawned services. Read the invariant in `examples/common/mod.rs` FIRST. Every
  example declares a tier in `scripts/live-checks.sh` (sandbox tier = safe with the
  stack running; `verify-full.sh` = release gate: verify + sandbox tier + wk-checks).
  Work in small verifiable
  steps, one task at a time; commit per task; tick finished items in `docs/TODO.md` with
  ✓ evidence. Surface assumptions before non-trivial work; keep changes surgical.

## Docs ship WITH the code, in the same commit (non-negotiable)

A commit that changes behaviour and leaves the docs describing yesterday is not
finished — it is a commit plus a debt nobody is tracking. This project has already
paid for that twice (a doc asserting a guard that no longer existed; a tally stale
within a day of being written), and the rule that failed was "remember to update
the docs later". So: **before every commit, walk the list below and update what the
change touched — in that same commit, not a follow-up.**

| If the change touches… | Update |
|---|---|
| any behaviour a user sees or a subsystem's mental model | `docs/ARCHITECTURE.md` |
| a new/renamed/deleted module, or a new entry point | `docs/MAP.md` (+ README's structure tree) |
| an invariant comment ("never", "always", "the ONE place") | `docs/CLAIM-LEDGER.md` — row + verdict + the tally (`scripts/ledger-tally.sh`, enforced by verify.sh) |
| what a layer can/can't prove, or a new probe/example/tier | `docs/TESTING.md`, `scripts/live-checks.sh` |
| a flow a release must be tested against by hand | `docs/SMOKE-TEST.md` (or `docs/PUBLISH-TESTING.md` for publish gates) |
| a port, a pinned binary version, a checksum | `docs/PORTS.md` |
| an open item finished, or a new one discovered | `docs/TODO.md` — tick with ✓ evidence, or add the row — then `./scripts/status.py --write` (the pre-commit hook refuses a stale `docs/STATUS.md`) |
| install/first-run behaviour, or a user-facing prompt | `docs/INSTALL.md` |
| the release pipeline or the cask | `docs/RELEASING.md` (cask lives in `rexenv/homebrew-tap`) |
| a design token, a component rule, an honest-UI promise | `docs/DESIGN.md` |
| a value that DIFFERS per OS (path, URL/origin, command, refusal, sentence) | `platform/words.rs` (all three constants) or a `platform/traits.rs` capability — never inline, never one OS's spelling (see "All three platforms") |
| a per-OS mechanism (how an OS keeps a promise), a new proof host, a new OS trap | `docs/PLATFORMS.md` §3 table / that OS's §6–§8 |
| a feature or fix any user will meet | `docs/SMOKE-TEST.md` — the main body (macOS) AND the Windows and Linux sections wherever the flow differs there — + `docs/INSTALL.md`'s section per OS if install/first run changed + the ledger row naming which OS(es) the proof came from |
| a `cfg(target_os)` outside `platform/` | a row in `docs/PLAN-linux-port.md` §1.1 saying what each other OS gets |
| a `rex` command or its IPC | `docs/CLI-ROADMAP.md` |

Two rules with teeth, learned the hard way:
- **A doc that is now WRONG outranks a doc that is merely incomplete.** When a fix
  closes a gap some doc lists as open, correcting that entry is part of the fix —
  a stale "zero coverage here" sends the next reader to build what already exists.
- **Say what it cost, not just what it does.** The durable half of these files is
  the failure that motivated the rule; a line that records only the current
  behaviour gets deleted by the next person who finds it obvious.

## Router — for X, read Y

| Task | Read |
|---|---|
| Any feature/bug — system mental model | `docs/ARCHITECTURE.md` (written against macOS) + `docs/PLATFORMS.md` (common vs per-OS, the three-OS checklist) |
| **Any new feature or fix — how it must work on macOS, Windows AND Linux** | **`docs/PLATFORMS.md`** — §4 checklist; per-OS traps §6 macOS · §7 Windows · §8 Linux |
| Why the Windows / Linux builds are shaped the way they are | `docs/PLAN-windows-port.md` / `docs/PLAN-linux-port.md` |
| Proving a claim on Windows / Linux (hosts, SSH, the traps) | `docs/TESTING.md` §"Proving a Windows claim" / §"Proving a Linux claim" |
| "Where does X live" — subsystem → files → entry points | `docs/MAP.md` |
| Testing: which layer proves what, the gate tiers | `docs/TESTING.md` |
| A "must never"/safety claim — is it proven? add one? | `docs/CLAIM-LEDGER.md` — **an invariant comment isn't finished until its ledger row + verdict land in the SAME commit** (a drifted ledger is worse than none); backlog is worked by the ledger's blast-radius tiers, top first |
| Ports, pinned binary versions, checksums | `docs/PORTS.md` (macOS pins + a Windows and a Linux artifact table) |
| What's open / what state is the project in | **`./scripts/status.py`** (generated; committed copy `docs/STATUS.md`) → `docs/TODO.md` (open items ONLY; shipped evidence logs: `docs/archive/SHIPPED-2026-07.md`, `-08.md`, `-09.md`) |
| Conventions for human contributors | `CONTRIBUTING.md` |
| rex CLI — future commands, IPC-exists tags | `docs/CLI-ROADMAP.md` |
| Module/file map | `README.md` ("Project structure") |
| Xdebug debug-PHP build (blocked item) | `docs/xdebug-debug-build.md` |
| Developer ID signing + notarization (blocked on a paid Apple account; runbook ready) | `docs/SIGNING.md` |
| **Why a shipped feature is shaped the way it is** — every design record (linked sites, Valet/Herd stages 1–3, Local import, git-site clone, PHP 7.4, MCP server + parity, dist-archive, menu-bar tray, binary/Adminer updates, browser preference, per-site lifecycle, webview-dialog proofs, in-app self-update) | `docs/archive/README.md` table → `docs/archive/PLAN-*.md`. A plan lives in `docs/` only while in flight; `status.py` lists those. |
| User-facing install / first-run prompts | `docs/INSTALL.md` — one section per OS (macOS · Windows · Linux) |
| Cutting a release — CI pipeline, draft gate, tap auto-bump | `docs/RELEASING.md` (cask itself lives in `rexenv/homebrew-tap`) |
| Release QA checklist, per OS | `docs/SMOKE-TEST.md` — main body = clean Mac; § Windows and § Linux say what changes on each |
| Design system, honest-UI rules, comp divergences | `docs/DESIGN.md` (comps removed from tree — in git history) |
| Why a past decision / phase evidence / audit trail | `docs/archive/` — **historical, may contradict current code; never trust without checking** |

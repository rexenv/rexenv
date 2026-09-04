# CLAUDE.md — rexenv

rexenv is a native, **no-Docker** local development environment for web & WordPress
developers: a Tauri 2 desktop app (Rust backend + React/TS frontend) that runs the whole
local stack — edge proxy with auto-HTTPS, shared web server, multi-version PHP, MySQL/
PostgreSQL, one-click WordPress, `.rex` DNS, mail catching, tunnels — from one UI.
**macOS is complete** (Phases 1–3 shipped); Windows/Linux are `todo!()` stubs (Phase 4).

This file is a ROUTER. Read only what the task needs (table at the bottom).
The system mental model lives in `docs/ARCHITECTURE.md` — read it for any feature or bug.

## Architecture rule (non-negotiable)

- `commands/` are **thin** Tauri IPC handlers — translate calls, invoke `core/`, nothing else.
- `core/` is **platform-agnostic** ("the what") — never imports OS-specific code.
- ALL OS-specific code lives ONLY in `src-tauri/src/platform/`, behind the 11 Rust traits
  in `platform/traits.rs`, impls selected via `#[cfg(target_os)]`. macOS impls are real;
  `windows/`/`linux/` stay `todo!()` — adding an OS = filling stubs, NOT restructuring.

## Non-negotiables (foundational — don't relitigate)

- **No Docker.** Native static binaries, downloaded on demand through `BinaryProvider`
  (pinned versions, checksum-locked). macOS `prepare_binary` order: de-quarantine →
  relink Homebrew dylibs to `/usr/lib` → ad-hoc codesign **LAST**.
- **Request topology (default sites):** browser → Caddy `:443` (TLS, local-CA certs,
  auto-HTTPS disabled) → ONE shared Nginx `:18088` (vhost by `server_name`) → one php-fpm
  pool **per PHP version** (not per site) → WordPress → DB. No direct Caddy→php-fpm.
  FrankenPHP per-site overrides are loopback backends only — never the edge, never `:443`.
- **Edge admin = private unix socket (`0600`), NEVER TCP `:2019`** — a TCP admin on a
  root Caddy is arbitrary file r/w as root. rexenv never binds nor queries TCP 2019, so
  it can never touch a developer's own Caddy.
- **Services OUTLIVE the app.** Closing rexenv stops nothing; on launch `adopt_startup`
  adopts rexenv-owned survivors (ownership = our fixed port + app-data marker on the
  cmdline; root edge via our admin socket).
- **"Running" = ownership AND liveness — never trust a bare port-listen.** The
  ServiceManager, not the monitor, is the source of truth for status.
- **Locking rule:** never hold the services lock across a wait — spawn under the lock,
  return `ReadyCheck`s, `await_ready` after dropping it. Status polls `try_lock` a
  snapshot; only `ServiceManager` sits behind an async Mutex (AppState = field-level locks).
- Embedded DNS (hickory) is **always-on and OUTLIVES the app** — `*.rex → 127.0.0.1` (any configured TLD; `.rex` is the always-installed backbone)
  on UDP 15353, served by a per-user LaunchAgent (`--dns-agent`, KeepAlive, no
  privilege); in-process only as automatic fallback; not a ServiceManager service;
  watchdog kickstarts/restarts (max 3).
- **System changes only through platform traits.** `/etc/resolver/rex` (+ one per extra TLD) = root op via
  `PrivilegeManager`; CA trust = USER op via `CertTrustManager` (login keychain — System
  keychain is impossible from detached-root osascript). Privileged prompts must run
  foreground. TLS leaves ≤398 days (Safari cap).
- **SQLite** for all app state. Config generator keeps three rewrite templates:
  single / subdomain-multisite / subdirectory-multisite. **Quote all paths** in generated
  Caddy/Nginx configs (app-data paths contain spaces).
- Every service start is port-gated via `core/ports::ensure_free`; conflicts name the
  holder + a copy-paste fix.

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
  react-hooks rules, NOT a style linter — see `eslint.config.js`) + the two
  generated-doc gates, `ledger-tally.sh` and `doc-counts.sh` — the latter also fails
  on any `docs/*.md` path the tree cites that does not exist). A green verdict comes ONLY from the script's own
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
| an open item finished, or a new one discovered | `docs/TODO.md` — tick with ✓ evidence, or add the row |
| install/first-run behaviour, or a user-facing prompt | `docs/INSTALL.md` |
| the release pipeline or the cask | `docs/RELEASING.md` (cask lives in `rexenv/homebrew-tap`) |
| a design token, a component rule, an honest-UI promise | `docs/DESIGN.md` |
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
| Any feature/bug — system mental model | `docs/ARCHITECTURE.md` |
| "Where does X live" — subsystem → files → entry points | `docs/MAP.md` |
| Testing: which layer proves what, the gate tiers | `docs/TESTING.md` |
| A "must never"/safety claim — is it proven? add one? | `docs/CLAIM-LEDGER.md` — **an invariant comment isn't finished until its ledger row + verdict land in the SAME commit** (a drifted ledger is worse than none); backlog is worked by the ledger's blast-radius tiers, top first |
| Ports, pinned binary versions, checksums | `docs/PORTS.md` |
| What's open / pick up work | `docs/TODO.md` (open items ONLY; shipped evidence logs: `docs/archive/SHIPPED-2026-07.md`, `docs/archive/SHIPPED-2026-08.md`) |
| Conventions for human contributors | `CONTRIBUTING.md` |
| rex CLI — future commands, IPC-exists tags | `docs/CLI-ROADMAP.md` |
| Module/file map | `README.md` ("Project structure") |
| Xdebug debug-PHP build (blocked item) | `docs/xdebug-debug-build.md` |
| Valet/Herd migration — ALL FOUR STAGES SHIPPED; the empirical research (layouts, conflicts, engine compat, dump flags) lives here | `docs/PLAN-valet-herd-migration.md` |
| Link an existing folder / serve a docroot outside the sites dir | `docs/PLAN-linked-sites.md` |
| Create a site FROM a git repo (Laravel first) — clone, `.env`, composer, migrate | `docs/PLAN-git-site-clone.md` |
| Valet/Herd import — scan, resolver consent, import loop (Stage 1) | `docs/PLAN-valet-herd-import.md` |
| Valet/Herd database import — dump/restore, provenance, credentials (Stage 2) | `docs/PLAN-valet-herd-db-import.md` |
| Valet/Herd connection rewrite — diff/consent, backup, connected fact (Stage 3) | `docs/PLAN-valet-herd-rewrite.md` |
| PHP 7.4 — SHIPPED 15 Aug 2026; where the binary comes from, self-build + hosting, EOL honesty | `docs/PLAN-php-74-support.md` |
| MCP server — M1/M2a/M2b/M3 ALL SHIPPED; agents drive rexenv, scratch sites, capability tiers; `db_query` on a user's site is SELECT-only at the Agent access dial's Read (D16 retired the grant) | `docs/PLAN-mcp-server.md` |
| MCP parity — the leftovers audit + the TODO for making EVERY app function agent-drivable (real sites, scoped grants, third registry) | `docs/PLAN-mcp-parity.md` |
| `wp dist-archive` — SHIPPED 5 Aug 2026; distributable zip from a repo asset | `docs/PLAN-dist-archive.md` |
| Menu-bar app (tray) — why the CLI/MCP sockets die with the window, the no-dock-icon ruling and what Accessory costs | `docs/PLAN-menubar-tray.md` |
| In-app PHP/engine patch updates — SHIPPED 17–18 Aug 2026; signed manifest, trust model | `docs/PLAN-binary-updates.md` |
| Adminer in-app updates — SHIPPED 18 Aug 2026; the SECOND manifest family, `updates::Family`, the binding probe | `docs/PLAN-adminer-updates.md` |
| Preferred browser + real app icons — shipped 11 Aug 2026, kept as the design record | `docs/PLAN-browser-preference.md` |
| WebKit/wry dialog + custom-scheme claims — why they are NOT L2-provable, where each leg lives | `docs/PLAN-webview-dialog-proofs.md` |
| User-facing install / first-run prompts | `docs/INSTALL.md` |
| Cutting a release — CI pipeline, draft gate, tap auto-bump | `docs/RELEASING.md` (cask itself lives in `rexenv/homebrew-tap`) |
| Release QA checklist (clean Mac) | `docs/SMOKE-TEST.md` |
| Design system, honest-UI rules, comp divergences | `docs/DESIGN.md` (comps removed from tree — in git history) |
| Why a past decision / phase evidence / audit trail | `docs/archive/` — **historical, may contradict current code; never trust without checking** |

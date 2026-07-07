# CLAUDE.md — rexenv

rexenv is a native, **no-Docker** local development environment for web & WordPress
developers: a Tauri 2 desktop app (Rust backend + React/TS frontend) that runs the whole
local stack — edge proxy with auto-HTTPS, shared web server, multi-version PHP, MySQL/
PostgreSQL, one-click WordPress, `.test` DNS, mail catching, tunnels — from one UI.
**macOS is complete** (Phases 1–3 shipped); Windows/Linux are `todo!()` stubs (Phase 4).

This file is a ROUTER. Read only what the task needs (table at the bottom).
The system mental model lives in `docs/ARCHITECTURE.md` — read it for any feature or bug.

## Architecture rule (non-negotiable)

- `commands/` are **thin** Tauri IPC handlers — translate calls, invoke `core/`, nothing else.
- `core/` is **platform-agnostic** ("the what") — never imports OS-specific code.
- ALL OS-specific code lives ONLY in `src-tauri/src/platform/`, behind the 9 Rust traits
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
- Embedded DNS (hickory) is **always-on, in-process** — `*.test → 127.0.0.1` on UDP
  15353; not a ServiceManager service; watchdog-restarted (max 3).
- **System changes only through platform traits.** `/etc/resolver/test` = root op via
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
- Verification: `cargo test --lib` + live-check `examples/*.rs`. Work in small verifiable
  steps, one task at a time; commit per task; tick finished items in `docs/TODO.md` with
  ✓ evidence. Surface assumptions before non-trivial work; keep changes surgical.

## Router — for X, read Y

| Task | Read |
|---|---|
| Any feature/bug — system mental model | `docs/ARCHITECTURE.md` |
| Ports, pinned binary versions, checksums | `docs/PORTS.md` |
| What's open / pick up work | `docs/TODO.md` |
| Module/file map | `README.md` ("Project structure") |
| Xdebug debug-PHP build (blocked item) | `docs/xdebug-debug-build.md` |
| User-facing install / first-run prompts | `docs/INSTALL.md` |
| Release QA checklist (clean Mac) | `docs/SMOKE-TEST.md` |
| Design tokens, screen specs | `docs/archive/DESIGN_BRIEF.md` + `design/*.dc.html` |
| Why a past decision / phase evidence / audit trail | `docs/archive/` — **historical, may contradict current code; never trust without checking** |

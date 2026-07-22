# rexenv

A native, lightweight, limitless local development environment for web & WordPress developers. **No Docker.** macOS first, then Windows and Linux.

> **Status:** Phases 1–3 and the deferred-services family are **complete on macOS** —
> one-click WordPress on real `https://*.rex`, multi-PHP (8.0–8.5), Nginx + per-site
> FrankenPHP/Apache, MySQL/MariaDB per site + PostgreSQL/Redis, per-engine DB version
> switching, WordPress Manager (incl. multisite), Mailpit, Adminer, logs, terminal,
> Cloudflare tunnels, blueprints, autostart. Open work is tracked in `docs/TODO.md`;
> Windows/Linux ports are `todo!()` stubs by design.

---

## What it is

One app to run your entire local stack — web servers (**Nginx**, per-site **FrankenPHP**
or **Apache** with `.htaccess`; OpenLiteSpeed blocked upstream — no macOS binary exists),
multiple PHP versions (8.0–8.5), databases (**MySQL** or **MariaDB** per site,
**PostgreSQL**, **Redis** — each engine switchable between pinned versions with
per-version data dirs), one-click WordPress with a full plugin/theme/user/
**multisite** manager, local `.rex` domains with auto-HTTPS, mail catching (Mailpit), a DB
browser (Adminer deep-link), log viewer, per-site terminal, and public sharing (Cloudflare
quick tunnels) — all native and lightweight, from one UI — plus the **`rex` CLI**
(bundled; Settings installs it on PATH), which remote-controls the running app for
nearly everything: `rex status|start|stop`, full site lifecycle incl. `site create
--multisite`, logs with `--follow`, `db export/import`, PHP/Xdebug switches, the
whole WP plugin/theme/user manager, and `rex doctor`.

## Docs

- **`docs/ARCHITECTURE.md`** — how rexenv works today, end-to-end. *Read this first.*
- **`docs/PORTS.md`** — the full port map + pinned binary versions.
- **`docs/TODO.md`** — all open work (the single active-work file).
- **`docs/CLI-ROADMAP.md`** — the `rex` CLI: shipped command surface + remaining items.
- **`CLAUDE.md`** — agent router: non-negotiable rules + "for X read Y" index.
- **`docs/INSTALL.md`** / **`docs/SMOKE-TEST.md`** — user install guide / clean-Mac release checklist.
- **`docs/archive/`** — historical: founding spec, design brief, phase task logs, audit record. May contradict current code.

## Tech stack

Tauri 2 (Rust backend + web frontend) · React + TypeScript + Vite · Tailwind CSS + shadcn/ui · native static binaries (no Docker) · SQLite for app state.

## Prerequisites (macOS, for development)

- **Rust** (stable) — for the Tauri backend
- **Node.js** (LTS) — for the frontend
- **Xcode Command Line Tools** — Tauri's macOS prerequisite (`xcode-select --install`)

## Getting started

```bash
# install frontend deps
pnpm install

# run the app in dev (Tauri + Vite)
pnpm tauri dev

# checks
pnpm build                                 # strict tsc + vite build
cargo test --lib   # in src-tauri/        # ~261 unit tests
cargo run --example <name>                 # live verification binaries (see src-tauri/examples/)
```

First launch routes to Onboarding, which performs system setup (installs the
`/etc/resolver/rex` resolver — one admin prompt — and trusts the local CA in your
login keychain). Service binaries (Caddy, Nginx, PHP, MySQL, …) are downloaded on
demand, checksum-pinned, and prepared for macOS automatically.

---

## Project structure (as built)

**Key principle:** `commands/` are thin and call `core/`; `core/` is platform-agnostic ("the what") and calls `platform/` traits ("the how"); OS-specific code lives **only** in `platform/`.

```
rexenv/
├── README.md                   # this file
├── CLAUDE.md                   # agent router: rules + doc index
├── design/                     # reference comps (*.dc.html) — one per screen
├── docs/                       # ARCHITECTURE.md · PORTS.md · TODO.md · INSTALL.md
│   │                           #   SMOKE-TEST.md · xdebug-debug-build.md
│   └── archive/                # historical: spec, design brief, task logs, audit
├── package.json · tsconfig.json · vite.config.ts
├── tailwind.config.js · postcss.config.js · index.html
│
├── cli/                        # ── `rex` CLI (bin) — remote control ONLY ──
│   └── src/main.rs             # never links the app lib: one JSON line over the
│                               #   app's private 0600 socket; app not running → exit 2
├── scripts/build-cli.sh        # stages rex as Tauri sidecars (bundled into the app)
│
├── src/                        # ── FRONTEND (React + TS) ──
│   ├── main.tsx                # React entry
│   ├── App.tsx                 # router, first-run gate, fatal-error screen
│   ├── routes/                 # one file per screen (1:1 with DESIGN_BRIEF)
│   │   ├── Sites.tsx · SiteDetail.tsx · Services.tsx · Databases.tsx
│   │   ├── Mail.tsx · Tunnels.tsx · Settings.tsx · Onboarding.tsx
│   ├── components/
│   │   ├── ui/                 # shadcn/ui primitives (button, dialog, menu, …)
│   │   ├── shell/              # AppShell, Sidebar, TopBar, StatusFooter
│   │   ├── common/             # StatusPill, StartStopToggle, Placeholder, …
│   │   ├── sites/              # NewSiteDialog (+ blueprint picker)
│   │   ├── wordpress/          # WordPressManager (plugins/themes/users/network/tools)
│   │   ├── database/           # AdminerFrame
│   │   └── terminal/           # SiteTerminal (xterm.js)
│   ├── lib/
│   │   ├── ipc/                # typed wrappers around Tauri invoke — the ONLY bridge
│   │   ├── adminer.ts · siteType.ts · theme.ts · toast.ts · utils.ts
│   │   └── mock.ts             # browser-only dev fallback data (not used in Tauri)
│   ├── types/                  # shared TS types (mirror the Rust DTOs)
│   └── styles/                 # tokens.css (design tokens) + Tailwind layers
│
└── src-tauri/                  # ── BACKEND (Rust) — Tauri convention ──
    ├── Cargo.toml · tauri.conf.json · build.rs
    ├── capabilities/           # Tauri 2 permission definitions
    ├── examples/               # live verification binaries (task evidence)
    └── src/
        ├── main.rs             # binary entry (calls lib::run)
        ├── lib.rs              # Tauri builder; registers all commands
        ├── cli_server.rs       # `rex` socket server — dispatches to the SAME
        │                       #   commands::* fns the UI calls (one code path)
        ├── error.rs            # shared error type (serializes for the UI)
        │
        ├── commands/           # Tauri IPC handlers (THIN — just call core/)
        │   ├── system.rs       # status, setup, DNS/SSL, autostart, open-external
        │   ├── sites.rs · services.rs · database.rs · php.rs · settings.rs
        │   └── wordpress.rs · mail.rs · logs.rs · terminal.rs · tunnels.rs · blueprints.rs
        │
        ├── core/               # domain logic (PLATFORM-AGNOSTIC — "the what")
        │   ├── service_manager.rs  # owns the stack: dbs, pools, overrides, mail, edge
        │   ├── sites.rs        # provision / rebuild configs / switches / teardown
        │   ├── services.rs     # nginx + php-fpm config gen & control
        │   ├── php.rs          # multi-version pool registry (8.0–8.5)
        │   ├── frankenphp.rs · apache.rs   # per-site override backends (loopback, never the edge)
        │   ├── proxy.rs        # Caddy edge (unix-socket admin, stale-edge recovery)
        │   ├── database.rs · mariadb.rs · postgres.rs · redis.rs · db.rs   # engines + DbEngine
        │   ├── wordpress.rs · wp_login.rs          # WP-CLI ops, magic login link
        │   ├── dns.rs · ssl.rs                     # hickory-dns resolver, rcgen CA
        │   ├── mail.rs · adminer.rs · logs.rs · terminal.rs · tunnels.rs
        │   ├── blueprints.rs · setup.rs · ports.rs · monitor.rs
        │   └── binaries.rs     # BinaryProvider: pinned manifest, checksum, prepare
        │
        ├── platform/           # OS-SPECIFIC impls behind traits (CRITICAL)
        │   ├── traits.rs       # DnsManager, CertTrustManager, PrivilegeManager,
        │   │                   #   ProcessSupervisor, AutostartManager, PermissionManager,
        │   │                   #   ShellRunner, Paths, BinaryProvider, EdgeSupervisor,
        │   │                   #   DnsAgentManager
        │   ├── macos/          # all 11 impls real
        │   └── windows/ · linux/   # todo!() stubs (fill later, no restructuring)
        │
        ├── state/              # app state
        │   ├── db.rs           # SQLite + migrations (v1–v14)
        │   ├── models.rs · store.rs
        │   └── app.rs          # AppState (db + platform + CA + ServiceManager + …)
        │
        └── templates/          # (config templates are generated in core/ today)
```

### Why this shape
- **`platform/` is the whole cross-platform strategy.** Every OS difference (DNS, trust store, privileges, process supervision, paths, binaries, permissions, shell) is a trait with per-OS impls. Build the macOS impls now and leave Windows/Linux as `todo!()` — adding them later means filling stubs, **not** restructuring.
- **`core/` never imports OS-specific code** — it talks to `platform/` traits only. This keeps the Windows/Linux ports clean.
- **`commands/` stay thin** — they translate IPC calls into `core/` calls, so the business logic is testable without the UI.
- **Frontend mirrors the design** — `routes/` map 1:1 to the screens in `docs/archive/DESIGN_BRIEF.md`; `components/shell/` is the app shell; `lib/ipc/` is the typed bridge to Rust.

---

## Build phases (summary)

1. **Phase 1 (macOS MVP)** — ✅ done. Embedded DNS + local CA → Caddy edge → shared Nginx + PHP-FPM → site create/list → MySQL → one-click WordPress on `https://*.test` (now `*.rex`).
2. **Phase 2 core** — ✅ done. Multi-PHP (8.0–8.5), per-site FrankenPHP override, PostgreSQL via `DbEngine`, resource monitor, edge recovery. *The once-deferred services shipped later via Homebrew-bottle **bundles** (`resolve_bundle` + `prepare_binary_tree` dylib relinking): **Redis**, **MariaDB** (+ per-site MySQL/MariaDB choice at create), **Apache** override, and per-engine DB **version switching** (per-series data dirs). OpenLiteSpeed stays blocked upstream — no macOS binary exists (`docs/TODO.md` "Blocked").*
3. **Phase 3** — ✅ done. WordPress Manager (plugins/themes/users/network incl. multisite), Adminer deep-link, Mailpit, log viewer, terminal, Cloudflare Tunnel, blueprints, autostart. *(Xdebug toggle blocked upstream on a static-php debug build; recipe in `docs/xdebug-debug-build.md`.)*
4. **Release** — hardening + `.dmg` packaging done except the clean-Mac verification (`docs/TODO.md`); audit history in `docs/archive/`.
5. **Phase 4/5** — Windows, then Linux ports (fill the `platform/` stubs).

Founding spec (historical): `docs/archive/PROJECT_SPEC.md`. Current system reference: `docs/ARCHITECTURE.md`.

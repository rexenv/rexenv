# rexenv

A native, lightweight, limitless local development environment for web & WordPress developers. **No Docker.** macOS first, then Windows and Linux.

> **Status:** planning → building. This repo currently holds the spec and design brief; the app is scaffolded in phases (see `PROJECT_SPEC.md`, section 5).

---

## What it is

One app to run your entire local stack — multiple web servers (Nginx / Apache / OpenLiteSpeed), multiple PHP versions, databases (MySQL / MariaDB / PostgreSQL / Redis), one-click WordPress with a full plugin/theme/**multisite** manager, local `.test` domains with auto-HTTPS, mail catching, and public sharing — all native and lightweight, from one UI.

## Docs

- **`PROJECT_SPEC.md`** — architecture decisions, feature list, tech stack, the platform-abstraction plan, and the build phases. *Read this first.*
- **`DESIGN_BRIEF.md`** — the Claude Design brief: design DNA + paste-ready prompts for every screen.

## Tech stack

Tauri 2 (Rust backend + web frontend) · React + TypeScript + Vite · Tailwind CSS + shadcn/ui · native static binaries (no Docker) · SQLite for app state.

## Prerequisites (macOS, for development)

- **Rust** (stable) — for the Tauri backend
- **Node.js** (LTS) — for the frontend
- **Xcode Command Line Tools** — Tauri's macOS prerequisite (`xcode-select --install`)

## Getting started

> Filled in once the Phase 1 scaffold lands.

```bash
# install frontend deps
pnpm install        # or: npm install

# run the app in dev (Tauri + Vite)
pnpm tauri dev
```

---

## Project structure (target)

The layout Claude Code should build toward. **Key principle:** `commands/` are thin and call `core/`; `core/` is platform-agnostic ("the what") and calls `platform/` traits ("the how"); OS-specific code lives **only** in `platform/`.

```
rexenv/
├── README.md                   # this file
├── PROJECT_SPEC.md             # architecture + features + roadmap
├── DESIGN_BRIEF.md             # Claude Design brief
├── package.json
├── pnpm-lock.yaml              # (or package-lock.json)
├── tsconfig.json
├── vite.config.ts
├── tailwind.config.js
├── postcss.config.js
├── components.json             # shadcn/ui config
├── index.html                  # Vite entry
├── .gitignore
│
├── src/                        # ── FRONTEND (React + TS) ──
│   ├── main.tsx                # React entry
│   ├── App.tsx                 # router + mounts the app shell
│   ├── routes/                 # one file per screen (matches the design)
│   │   ├── Sites.tsx
│   │   ├── SiteDetail.tsx
│   │   ├── Services.tsx
│   │   ├── Databases.tsx
│   │   ├── Mail.tsx
│   │   ├── Tunnels.tsx
│   │   ├── Settings.tsx
│   │   └── Onboarding.tsx
│   ├── components/
│   │   ├── ui/                 # shadcn/ui primitives (button, dialog, ...)
│   │   ├── shell/              # Sidebar, TopBar, StatusFooter (the app shell)
│   │   └── common/             # StatusPill, SiteRow, ServiceRow, Badge, ...
│   ├── features/               # feature-scoped UI + logic (co-located)
│   │   ├── sites/
│   │   ├── services/
│   │   ├── wordpress/          # WordPress Manager (plugins/themes/users/network)
│   │   ├── database/
│   │   ├── mail/
│   │   ├── tunnels/
│   │   └── onboarding/
│   ├── lib/
│   │   ├── ipc/                # typed wrappers around Tauri commands (invoke)
│   │   └── utils.ts
│   ├── hooks/                  # React hooks (TanStack Query)
│   ├── stores/                 # Zustand stores
│   ├── types/                  # shared TS types (mirror the Rust types)
│   ├── styles/                 # global.css + Tailwind layers + design tokens
│   └── assets/                 # fonts (Space Grotesk, JetBrains Mono), icons
│
└── src-tauri/                  # ── BACKEND (Rust) — Tauri convention ──
    ├── Cargo.toml
    ├── tauri.conf.json
    ├── build.rs
    ├── icons/                  # app icons (all sizes / platforms)
    ├── capabilities/           # Tauri 2 permission definitions
    └── src/
        ├── main.rs             # binary entry (calls lib::run)
        ├── lib.rs              # Tauri builder; registers all commands
        ├── error.rs            # shared error types
        │
        ├── commands/           # Tauri IPC handlers (THIN — just call core/)
        │   ├── mod.rs
        │   ├── sites.rs
        │   ├── services.rs
        │   ├── database.rs
        │   ├── wordpress.rs
        │   ├── tunnels.rs
        │   ├── mail.rs
        │   ├── settings.rs
        │   └── system.rs
        │
        ├── core/               # domain logic (PLATFORM-AGNOSTIC — "the what")
        │   ├── mod.rs
        │   ├── sites/          # site model, lifecycle, vhost/config generation
        │   ├── services/       # supervisor, PHP-FPM pools, web servers
        │   ├── database/       # db service management
        │   ├── wordpress/      # WP-CLI wrapper, plugin/theme/multisite logic
        │   ├── dns/            # embedded DNS resolver (hickory-dns)
        │   ├── ssl/            # local CA + cert generation (rcgen)
        │   ├── proxy/          # edge router (Caddy) config + control
        │   ├── tunnels/        # cloudflared control
        │   ├── mail/           # mailpit control
        │   └── binaries/       # BinaryProvider: manifest, download, extract
        │
        ├── platform/           # OS-SPECIFIC impls behind traits (CRITICAL)
        │   ├── mod.rs          # selects impl via #[cfg(target_os = "...")]
        │   ├── traits.rs       # DnsManager, CertTrustManager, PrivilegeManager,
        │   │                   #   ProcessSupervisor, AutostartManager,
        │   │                   #   PermissionManager, ShellRunner, Paths, BinaryProvider
        │   ├── macos/          # macOS implementations  (build these first)
        │   ├── windows/        # Windows impls          (start as todo!() stubs)
        │   └── linux/          # Linux impls            (start as todo!() stubs)
        │
        ├── state/              # app state
        │   ├── mod.rs
        │   ├── db.rs           # SQLite (rusqlite / sqlx)
        │   ├── models.rs
        │   └── store.rs
        │
        ├── templates/          # config templates (nginx / apache / caddy / php-fpm / wp)
        │
        └── utils/              # helpers
```

### Why this shape
- **`platform/` is the whole cross-platform strategy.** Every OS difference (DNS, trust store, privileges, process supervision, paths, binaries, permissions, shell) is a trait with per-OS impls. Build the macOS impls now and leave Windows/Linux as `todo!()` — adding them later means filling stubs, **not** restructuring.
- **`core/` never imports OS-specific code** — it talks to `platform/` traits only. This keeps the Windows/Linux ports clean.
- **`commands/` stay thin** — they translate IPC calls into `core/` calls, so the business logic is testable without the UI.
- **Frontend mirrors the design** — `routes/` map 1:1 to the screens in `DESIGN_BRIEF.md`; `components/shell/` is the app shell; `lib/ipc/` is the typed bridge to Rust.

---

## Build phases (summary)

1. **Phase 1 (macOS MVP):** scaffold → embedded DNS + local CA → Caddy edge router → one Nginx + one PHP → site create/list → MySQL → one-click WordPress. *Goal: a WP site on HTTPS.*
2. **Phase 2:** multi-PHP, Apache + OpenLiteSpeed, MariaDB + PostgreSQL + Redis, resource monitor.
3. **Phase 3:** WordPress Manager (incl. Multisite), Adminer, Mailpit, Xdebug toggle, log viewer, Cloudflare Tunnel, terminal.
4. **Phase 4:** Windows port (fill the `platform/windows/` stubs).
5. **Phase 5:** Linux port (fill the `platform/linux/` stubs).

See `PROJECT_SPEC.md` for the full detail.

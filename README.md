# rexenv

A native, lightweight, limitless local development environment for web & WordPress developers. **No Docker.** On **macOS, Windows and Linux (Ubuntu 22.04+)**.

> **Status:** shipping on all three — macOS since 0.1.0, Windows since 0.8.0 (19 Sep 2026),
> Linux since 0.8.8 (27 Sep 2026) — and since 27 Sep 2026 one tag builds all three on
> GitHub Actions (`docs/RELEASING.md`). Phases 1–3 and the deferred-services family are
> **complete**: one-click WordPress on real `https://*.rex`, multi-PHP (7.4–8.5), Nginx +
> per-site FrankenPHP/Apache, MySQL/MariaDB per site + PostgreSQL/Redis, per-engine DB version
> switching, WordPress Manager (incl. multisite), Mailpit, Adminer, logs, terminal,
> Cloudflare tunnels, blueprints, autostart — plus, since July 2026: linked sites
> (serve any existing folder in place), full Valet/Herd migration (scan → import →
> database copy → consent-gated connection rewrite, all reversible), plugin/theme
> add-from-Git with streamed jobs and watchers, per-site Xdebug, the 40+-command
> `rex` CLI, and an in-app self-update on every OS. A component an OS has no build of is
> refused there with a message that says why — Windows: Apache, FrankenPHP, Xdebug, Redis,
> MariaDB; Linux: Apache, Xdebug, Redis, MariaDB (`docs/INSTALL.md`). What is common and
> what differs per OS: `docs/PLATFORMS.md`. Open work: `docs/TODO.md`.

## Install

```sh
curl -fsSL https://rexenv.rex.bd/install.sh | bash      # macOS 13+, Ubuntu 22.04+ (amd64, arm64)
```

```powershell
irm https://rexenv.rex.bd/install.ps1 | iex             # Windows 10/11 x64, in PowerShell
```

It installs the latest release and starts it: `rexenv.app` into `/Applications` on macOS, the
per-user installer run silently on Windows (no administrator prompt), the `rexenv` package
from rexenv's signed apt repository on Ubuntu (`sudo` asks once; later updates also arrive with
`sudo apt upgrade`; without apt, the AppImage into `~/Applications`). Every
download is checked against the release's own `.sha256` first, and an installed rexenv is left
alone — it updates itself (Settings → About → Check now). The command path meets no Gatekeeper
or SmartScreen dialog: both check a mark the *downloading* program writes, and `curl` and
PowerShell's `Invoke-WebRequest` write none. The scripts are short and public —
[`install.sh`](https://github.com/rexenv/homebrew-tap/blob/main/install.sh),
[`install.ps1`](https://github.com/rexenv/homebrew-tap/blob/main/install.ps1) — and the
design behind them is `docs/archive/PLAN-install-scripts.md`.

Other routes: Homebrew on macOS (`brew tap rexenv/tap && brew trust rexenv/tap && brew install
--cask rexenv`), or the `.dmg`, `setup.exe`, `.deb` and AppImage on the
[releases page](https://github.com/rexenv/rexenv/releases). rexenv is open source and
not code-signed, so a BROWSER download meets Gatekeeper's or SmartScreen's dialog once;
`docs/INSTALL.md` has each one word for word, the first-run prompts per OS, and uninstalling
(do the in-app step first). One user account per machine: `:443`, the `.rex` route and the
resolver port are machine-wide.

---

## What it is

One app to run your entire local stack — web servers (**Nginx**, per-site **FrankenPHP**
**Apache** with `.htaccess`, or **OpenLiteSpeed** with LSCache — rexenv's own build, macOS + Linux),
multiple PHP versions (7.4–8.5), databases (**MySQL** or **MariaDB** per site,
**PostgreSQL**, **Redis** — each engine switchable between pinned versions with
per-version data dirs), one-click WordPress with a full plugin/theme/user/
**multisite** manager, local `.rex` domains with auto-HTTPS, mail catching (Mailpit), a DB
browser (Adminer deep-link), log viewer, per-site terminal, and public sharing (Cloudflare
quick tunnels) — all native and lightweight, from one UI — plus the **`rex` CLI**
(bundled; Settings installs it on PATH), which remote-controls the running app for
nearly everything: `rex status|start|stop`, full site lifecycle incl. `site create
--multisite`, logs with `--follow`, `db export/import`, PHP/Xdebug switches, the
whole WP plugin/theme/user manager, and `rex doctor`.

There is also an **opt-in MCP endpoint** (off by default, a second `0600` unix socket,
connected with `claude mcp add rexenv -- rex mcp`) so an AI agent can diagnose your
sites read-only and work in disposable **scratch sites** it owns — with a TTL, a cap, a
reaper that never touches a site of yours, and an activity feed of everything it did.
It is deliberately **not** described as a sandbox: an agent running your code in its own
site is still your machine's power, which is why it ships off and says so where you turn
it on (`docs/ARCHITECTURE.md` §8.3).

## Docs

- **`docs/ARCHITECTURE.md`** — how rexenv works today, end-to-end. *Read this first.*
- **`docs/MAP.md`** — where everything lives: subsystem → files → entry points.
- **`docs/PLATFORMS.md`** — one feature, three OSes: what is common, where an OS difference goes, each OS's mechanism and traps, the checklist every feature walks.
- **`CONTRIBUTING.md`** — build/run, the verification gate, conventions, deliberate decisions.
- **`docs/TESTING.md`** / **`docs/CLAIM-LEDGER.md`** — the layer model + the claim
  inventory that is the project's test metric.
- **`docs/PORTS.md`** — the full port map + pinned binary versions.
- **`docs/TODO.md`** — all open work (open items only; the shipped evidence log is archived).
- **`docs/CLI-ROADMAP.md`** — the `rex` CLI: shipped command surface + remaining items.
- **`CLAUDE.md`** — agent router: non-negotiable rules + "for X read Y" index.
- **`docs/INSTALL.md`** / **`docs/SMOKE-TEST.md`** — user install guide (macOS + Windows + Linux) / release checklist (clean Mac, with Windows and Linux sections).
- **`docs/STATUS.md`** — generated (`scripts/status.py`): open work by section, ledger tally, plans in flight.
- **`docs/archive/PLAN-*.md`** — design records for every shipped feature (why it is shaped the way it is); indexed in `docs/archive/README.md`. A plan lives in `docs/` only while in flight.
- **`docs/archive/`** — historical: founding spec, design brief, phase task logs, audit record. May contradict current code.

## Tech stack

Tauri 2 (Rust backend + web frontend) · React + TypeScript + Vite · Tailwind CSS + shadcn/ui · native static binaries (no Docker) · SQLite for app state.

## Prerequisites (macOS, for development)

- **Rust** (stable) — for the Tauri backend
- **Node.js** (LTS) + **pnpm** — for the frontend
- **Xcode Command Line Tools** — Tauri's macOS prerequisite (`xcode-select --install`)

## Getting started

```bash
# install frontend deps
pnpm install

# run the app in dev (Tauri + Vite)
pnpm tauri dev

# THE pre-commit bar (lib tests + example builds + clippy at zero + tsc):
scripts/verify.sh

# deeper tiers (see CONTRIBUTING.md and docs/TESTING.md):
scripts/live-checks.sh                     # tiered live checks against real binaries
scripts/verify-full.sh                     # release gate: verify + sandbox tier + WebKit harness
scripts/windows-check.sh                   # does it COMPILE for Windows x64? (cargo-xwin; also run by verify.sh)
scripts/linux-check.sh                     # does it COMPILE for Linux? (Ubuntu 22.04 container; also run by verify.sh)
cargo run --example <name>                 # a single live check (src-tauri/examples/)
```

First launch routes to Onboarding, which performs system setup — on macOS it installs the
`/etc/resolver/rex` resolver (one admin prompt) and trusts the local CA in your login
keychain; Windows (one UAC prompt, the CurrentUser Root store) and Linux (polkit, the system
store and your browsers' NSS databases) do the same by their own mechanisms
(`docs/INSTALL.md`, `docs/PLATFORMS.md` §3). Service binaries (Caddy, Nginx, PHP, MySQL, …)
are downloaded on demand, checksum-pinned, and prepared for each OS automatically.

---

## Project structure (as built)

**Key principle:** `commands/` are thin and call `core/`; `core/` is platform-agnostic ("the what") and calls `platform/` traits ("the how"); OS-specific code lives **only** in `platform/`.

```
rexenv/
├── README.md                   # this file
├── LICENSE · NOTICE · THIRD-PARTY-NOTICES.md · SECURITY.md
├── CLAUDE.md                   # agent router: rules + doc index
├── CONTRIBUTING.md             # build/run · the gate · conventions · deliberate decisions · DCO
├── docs/                       # ARCHITECTURE · MAP · PLATFORMS · TESTING · CLAIM-LEDGER · PORTS
│   │                           #   TODO · INSTALL · SMOKE-TEST · PUBLISH-TESTING
│   │                           #   CLI-ROADMAP · SIGNING · DESIGN · STATUS (generated)
│   └── archive/                # historical: spec, design brief, task logs, audit,
│                               #   shipped evidence logs, PLAN-*.md design records — may contradict current code
├── package.json · tsconfig.json · vite.config.ts
├── tailwind.config.js · postcss.config.js · index.html
│
├── cli/                        # ── `rex` CLI (bin) — remote control ONLY ──
│   └── src/main.rs             # never links the app lib: one JSON line over the
│                               #   app's private 0600 socket; app not running → exit 2
│       pipe.rs                 # Windows: the app's owner-only named pipe (tokio)
│   └── tests/closed_reader.rs  # the built `rex` on a pipe whose reader left → exit 0, silent
├── scripts/                    # verify.sh (THE pre-commit bar) · verify-full.sh
│   │                           #   live-checks.sh (tiered L1 runner) · build-cli.sh
│   │                           #   windows-check.sh (Windows x64 compile check, cargo-xwin)
│   ├── wk-checks/              # Playwright WebKit render checks (L2) + README
│   └── video/                  # narrated tutorials, launch intro + feature tour, recorded from the real UI (dev only) + README
│
│   # The Homebrew cask is NOT in this repo — it lives in github.com/rexenv/homebrew-tap
│   # (`brew tap rexenv/tap`), which is the single source of truth for it.
│
├── src/                        # ── FRONTEND (React + TS) ──
│   ├── main.tsx                # React entry
│   ├── App.tsx                 # router, first-run gate, fatal-error screen
│   ├── routes/                 # one file per screen
│   │   ├── Sites.tsx · SiteDetail.tsx · Services.tsx · Databases.tsx
│   │   ├── Mail.tsx · Tunnels.tsx · Import.tsx · Settings.tsx · Onboarding.tsx
│   │   ├── DevGitPanel.tsx · DevUiReview.tsx   # dev-only harnesses (tree-shaken)
│   ├── components/
│   │   ├── ui/                 # shadcn/ui primitives (button, dialog, menu, …)
│   │   ├── shell/              # AppShell, Sidebar, TopBar, StatusFooter
│   │   ├── common/             # StatusPill, StartStopToggle, Placeholder, …
│   │   ├── sites/              # NewSiteDialog (+ blueprint picker)
│   │   ├── wordpress/          # WordPressManager (plugins/themes/users/network/tools)
│   │   │                       #   add sources: ZipAddPanel · GitAddPanel · LinkFolderPanel
│   │   ├── database/           # AdminerFrame
│   │   └── terminal/           # SiteTerminal (xterm.js)
│   ├── lib/
│   │   ├── ipc/                # typed wrappers around Tauri invoke — the ONLY bridge
│   │   ├── adminer.ts · siteType.ts · theme.ts · toast.ts · utils.ts
│   │   └── mock.ts             # browser-only dev fallback data (not used in Tauri)
│   ├── types/                  # shared TS types (mirror the Rust DTOs)
│   └── styles/                 # tokens.css (design tokens) + Tailwind layers
├── companion/rexenv-sync/      # the WordPress plugin for live ↔ local sync (so far: the request
│                               #   signature + vectors shared with core/live_sync — docs/rexsync-protocol.md)
│
└── src-tauri/                  # ── BACKEND (Rust) — Tauri convention ──
    ├── Cargo.toml · tauri.conf.json · build.rs
    ├── capabilities/           # Tauri 2 permission definitions
    ├── examples/               # live checks (L1) — READ examples/common/mod.rs's
    │                           #   invariant first; tiers in scripts/live-checks.sh
    └── src/
        ├── main.rs             # binary entry (calls lib::run)
        ├── lib.rs              # Tauri builder; registers all commands
        ├── cli_server.rs       # `rex` socket server — dispatches to the SAME
        │                       #   commands::* fns the UI calls (one code path)
        ├── error.rs            # shared error type (serializes for the UI)
        ├── crash.rs            # panic hook — crash.log (+ a message box on Windows release)
        ├── test_support.rs     # test-only OS fixtures (symlink, mode, exit status) — kept
        │                       #   out of core/ so ledger #163's scan stays clean
        │
        ├── commands/           # Tauri IPC handlers (THIN — just call core/)
        │   ├── system.rs       # status, setup, DNS/SSL, autostart, open-external
        │   ├── sites.rs · site_provision.rs · services.rs · database.rs · php.rs
        │   ├── wordpress.rs · wp_install.rs · repo.rs · blueprints.rs · worktree.rs
        │   ├── valet_import.rs · db_import.rs · rewrite.rs · downloads.rs
        │   ├── mail.rs · logs.rs · terminal.rs · tunnels.rs · settings.rs
        │   └── mcp.rs · scratch.rs      # the MCP surface + agent scratch sites
        │
        ├── core/               # domain logic (PLATFORM-AGNOSTIC — "the what");
        │   │                   #   every module has a //! doc header; index: docs/MAP.md
        │   ├── service_manager.rs  # owns the stack: dbs, pools, overrides, mail, edge
        │   ├── sites.rs · site_env.rs · site_metrics.rs   # lifecycle, env vars, metrics
        │   ├── services.rs     # nginx + php-fpm config gen & control
        │   ├── php.rs · phpconf.rs  # multi-version pools (7.4–8.5); wp-config/.env readers
        │   ├── frankenphp.rs · apache.rs · openlitespeed.rs   # per-site override backends (loopback, never the edge)
        │   ├── proxy.rs        # Caddy edge (unix-socket admin, root daemon, adoption)
        │   ├── database.rs · mariadb.rs · postgres.rs · redis.rs · db.rs   # engines + DbEngine
        │   ├── wordpress.rs · wp_login.rs · wporg.rs      # WP-CLI ops, magic login, wp.org search
        │   ├── wp_dns.rs      # mu-plugin so a site can reach itself (c-ares vs /etc/resolver)
        │   ├── dns.rs · tld.rs · ssl.rs · firefox.rs      # resolver + TLD policy, CA
        │   ├── tunnels.rs · wp_tunnel.rs                  # cloudflared shares + URL rewrite
        │   ├── valet.rs        # read-only Valet/Herd discovery (import Stage 1)
        │   ├── localwp.rs      # read-only Local (WP Engine) discovery — the registry, re-homing .local
        │   ├── dbsource.rs · dbcompat.rs · dbdump.rs · dbrestore.rs · dbmirror.rs · dbimport.rs
        │   ├── dbclone.rs      # same-server copy of a site's database (a worktree child's start)
        │   ├── agent_db.rs      # MCP agent DB principals (names + provisioning SQL)
        │   ├── agent_query.rs   # MCP agent DB reads — native driver, never the bundled client
        │   │                   # database import (Stage 2): identify → gate → dump → restore → mirror
        │   ├── confedit.rs · confverify.rs · confrewrite.rs  # connection rewrite (Stage 3)
        │   ├── repo.rs · devtools.rs · dist_archive.rs    # add-from-Git, toolchain, `wp dist-archive`
        │   ├── worktree.rs     # git worktrees as sites — the never-rm-a-worktree guard (docs/PLAN-git-worktrees.md)
        │   ├── live_sync/      # WordPress live ↔ local sync — so far the rexsync1 signature (docs/rexsync-protocol.md)
        │   ├── laravel.rs · dotenv.rs                     # Laravel create/install, `.env` read+write
        │   ├── starter.rs      # Blank-PHP starter page + seeded table (templates/starter/)
        │   ├── scratch.rs · wp_mailtag.rs                 # agent scratch sites (TTL, cap, reaper) + their mail stamp
        │   ├── updates.rs · php_upstream.rs               # signed update manifests; "a newer patch exists"
        │   ├── app_update.rs # the app's OWN signed release descriptor (self-update)
        │   ├── wp_packages.rs · copy_scan.rs · proc.rs    # WP-CLI package pin; UI-copy guards; process ownership
        │   ├── mail.rs · adminer.rs · logs.rs · terminal.rs · monitor.rs
        │   ├── macho.rs      # the macOS version a pinned binary DECLARES (minos); which archs it holds
        │   ├── blueprints.rs · setup.rs · ports.rs · stack_guard.rs · cli.rs
        │   ├── prompt.rs       # while_prompting — an admin/keychain dialog never holds the async runtime
        │   ├── downloads.rs    # download-manager hub (prefetch-before-lock)
        │   └── binaries.rs     # BinaryProvider: pinned manifest, checksum, prepare
        │
        ├── platform/           # OS-SPECIFIC impls behind traits (CRITICAL)
        │   ├── durable.rs      # power-cut-safe writes (macOS + Linux): flushed privileged steps, durable boot files
        │   ├── traits.rs       # DnsManager, CertTrustManager, PrivilegeManager,
        │   │                   #   ProcessSupervisor, AutostartManager, PermissionManager,
        │   │                   #   ShellRunner, Paths, BinaryProvider, EdgeSupervisor,
        │   │                   #   DnsAgentManager, AppBundle, LocalIpc
        │   ├── macos/          # all 13 impls real (+ app_bundle.rs, relauncher.rs, webview_dialogs.rs, parent_death_guard.rs, activation.rs, prompt_applet.rs, keychain_trust.rs)
        │   ├── windows/        # all impls real (docs/PLAN-windows-port.md)
        │   └── linux/          # all impls real since 24 Sep 2026, run on Ubuntu 22.04 (docs/PLAN-linux-port.md);
        │                       #   pure halves proc_table/resolved/units/desktop/trust tested on every host
        │
        └── state/              # app state
            ├── db.rs           # SQLite + migrations (v1–v45)
            ├── models.rs · store.rs
            └── app.rs          # AppState (db + platform + CA + ServiceManager + …)
```

### Why this shape
- **`platform/` is the whole cross-platform strategy.** Every OS difference (DNS, trust store, privileges, process supervision, paths, binaries, permissions, shell) is a trait with per-OS impls in `macos/`, `windows/` and `linux/`, and every differing value or sentence is a `platform/words.rs` field with all three answers. macOS came first; Windows (Sep 2026) and then Linux (24 Sep 2026, two days, `core/` untouched) were added by filling stubs, not restructuring. Every new feature is built for all three in the same change — `docs/PLATFORMS.md`.
- **`core/` never imports OS-specific code** — it talks to `platform/` traits only (a scan test fails the build otherwise, ledger #163). That is what keeps one feature one change on three OSes.
- **`commands/` stay thin** — they translate IPC calls into `core/` calls, so the business logic is testable without the UI.
- **Frontend mirrors the design** — `routes/` map 1:1 to screens (`docs/DESIGN.md` holds the design system + the comps' intentional divergences); `components/shell/` is the app shell; `lib/ipc/` is the typed bridge to Rust.

---

## Build phases (summary)

1. **Phase 1 (macOS MVP)** — ✅ done. Embedded DNS + local CA → Caddy edge → shared Nginx + PHP-FPM → site create/list → MySQL → one-click WordPress on `https://*.test` (now `*.rex`).
2. **Phase 2 core** — ✅ done. Multi-PHP (7.4–8.5; 7.4 is rexenv's OWN build, hosted in `rexenv/runtimes` — static-php.dev publishes none), per-site FrankenPHP override, PostgreSQL via `DbEngine`, resource monitor, edge recovery. *The once-deferred services shipped later via Homebrew-bottle **bundles** (`resolve_bundle` + `prepare_binary_tree` dylib relinking): **Redis**, **MariaDB** (+ per-site MySQL/MariaDB choice at create), **Apache** override, and per-engine DB **version switching** (per-series data dirs). OpenLiteSpeed shipped 4 Oct 2026 as the third override kind — rexenv's own build for macOS + Linux, hosted in `rexenv/runtimes` (`docs/archive/PLAN-openlitespeed.md`).*
3. **Phase 3** — ✅ done. WordPress Manager (plugins/themes/users/network incl. multisite), Adminer deep-link, Mailpit, log viewer, terminal, Cloudflare Tunnel, blueprints, autostart. *(Xdebug toggle blocked upstream on a static-php debug build; recipe in `docs/xdebug-debug-build.md`.)*
4. **Release** — ✅ shipping since 0.1.0. Since 27 Sep 2026 one tag builds macOS, Windows and Linux on GitHub Actions and drafts one release — on `rexenv/rexenv` since 0.8.11 (30 Sep 2026; on the tap before, while this repo was private); a human publishes (`docs/RELEASING.md`). The app updates itself on every OS, and the one-command install (`install.sh` / `install.ps1`, Sep 2026) sits beside the cask. Audit history in `docs/archive/`.
5. **Phase 4/5** — ✅ Windows (first installer 0.8.0, 19 Sep 2026) and Linux (the port 24 Sep 2026, first release 0.8.8 on 27 Sep), each by filling the `platform/` stubs with `core/` untouched — `docs/PLAN-windows-port.md`, `docs/PLAN-linux-port.md`.

Founding spec (historical): `docs/archive/PROJECT_SPEC.md`. Current system reference: `docs/ARCHITECTURE.md`.

---

## Licence

rexenv is licensed under the **Apache License 2.0** (`LICENSE`). Contributions
are accepted under the same licence with a DCO sign-off (`CONTRIBUTING.md`).

What ships in the app (Rust crates, npm packages, fonts, bundled SQLite) is
inventoried in **`THIRD-PARTY-NOTICES.md`**. Most server binaries rexenv
downloads at runtime (PHP **8.0–8.5**, MySQL, MariaDB, PostgreSQL, Redis, nginx,
Caddy, FrankenPHP, Apache httpd, Mailpit, Adminer, cloudflared, WP-CLI, Composer,
Xdebug) are **not redistributed by rexenv** — they are fetched checksum-pinned
from their own distributors (official upstreams, static-php.dev,
jirutka/nginx-binaries, theseus-rs, Homebrew's bottle registry — the full
source-and-version table is `docs/PORTS.md`) and remain under their own
licences on your machine. Trusting rexenv therefore includes trusting those
build sources; the pinned checksums are the enforcement.

**PHP 7.4.33 is the one exception, and it cuts the other way:** nobody publishes
a portable 7.4, so rexenv **builds and hosts it** (`rexenv/runtimes`) and is
therefore its distributor. It carries the PHP License 3.01 and the licences of
everything statically linked into it — reproduced in `THIRD-PARTY-NOTICES.md`,
published beside the artifacts as `licenses-<arch>.tar.gz`, and **installed onto
your machine next to the interpreter** at `bin/php-7.4.33/licenses/`.

Vulnerabilities: report privately first — see `SECURITY.md`.

# CLAUDE.md — rexenv

rexenv is a native, lightweight, NO-Docker local development environment for web &
WordPress developers. Tauri 2 desktop app (Rust backend + React/TS frontend). It runs
a developer's whole local stack — web servers, multiple PHP versions, databases,
one-click WordPress, local `.test` domains with auto-HTTPS, mail catching, public
sharing — from one UI. **macOS first**, then Windows, then Linux.

## Status — Phase 1 COMPLETE · Phase 2 core COMPLETE · Phase 3 COMPLETE (macOS)
- **Phase 1 (MVP) — done, verified on real :443:** one-click WordPress at `https://wpdemo.test`
  over browser → Caddy (local-CA TLS) → shared Nginx (by `server_name`) → php-fpm → WordPress →
  MySQL. Per-task evidence in **TASKS.md**.
- **Phase 2 core — done** (per-task evidence in **TASKS-PHASE2.md**):
  - **Multi-PHP** — 8.1/8.2/8.3, one php-fpm pool per version, per-site one-click switch, registry
    + Settings UI, New Site dialog.
  - **Per-site server override** — **FrankenPHP** (single static binary) as a loopback backend
    behind the edge; live Nginx↔FrankenPHP switch + UI.
  - **Databases** — **PostgreSQL** alongside MySQL via a `DbEngine` abstraction; Databases UI
    (live status, start/stop).
  - **Resource monitor** — live RAM/CPU for every supervised service (pools, FrankenPHP backends,
    DB engines).
  - **Edge recovery** — a stale Caddy on `:2019` is auto-stopped via its admin API on startup
    (no more `sudo pkill`).
- **Phase 3 — done** (per-task evidence in **TASKS-PHASE3.md**):
  - **WordPress Manager** — plugins/themes/users (incl. one-time, single-use, loopback-only
    "Log in as" magic link), Tools (WP_DEBUG, search-replace, permalinks, core update/reinstall).
  - **Mailpit** — sendmail-shim → SMTP `:1025` sink; inbox UI reads its HTTP API `:8025`.
  - **Adminer** — internal `adminer.rexenv.test` vhost (never a tunnel origin) + **per-site
    deep-link** wrapper (passwordless loopback auto-login straight into the site's DB).
  - **Logs viewer** (per-service tail) · **Terminal** (xterm + PTY, bundled `php`/`wp` on PATH).
  - **Cloudflare Tunnel** — per-site quick tunnel (`cloudflared`), scoped to ONE site Host.
  - **Multisite** — convert subdomain/subdirectory; wildcard cert + edge route for subdomain
    (doesn't shadow other sites); Network UI (sub-sites CRUD, network-activate, super-admins).
  - **Settings** — DNS & SSL (status, re-trust CA, regenerate certs) + **autostart** (launchd
    LaunchAgent; the last stubbed Phase-1 trait, now real) + **site blueprints** (reusable presets
    incl. multisite, applied on one-click create).
- **Deferred to §7 (need a macOS dylib-tree-bundling step):** Apache, MariaDB, Redis, OpenLiteSpeed;
  plus DB multi-version switch. None has a clean portable macOS binary (MariaDB ships none; Apache/
  Redis link non-system dylibs like openssl@3/apr). FrankenPHP + PostgreSQL prove the patterns.
- **Also pending (external/packaging-era):** 10.4 SMAppService single-prompt helper; Phase-3 §8.2
  per-site **Xdebug** toggle — BLOCKED on §11.2 (no hosted static-php build with Xdebug exists; the
  `php-debug` variant + `spc` build recipe are wired in `docs/xdebug-debug-build.md`, awaiting a
  maintainer build/host + checksum pin).
- **Next:** Phase 4 / polish (PROJECT_SPEC.md §5) — Windows + Linux platform impls, packaging/signing.

## Architecture rule (non-negotiable)
- `commands/` are **thin** Tauri IPC handlers — they only translate calls and invoke `core/`.
- `core/` is **platform-agnostic** ("the what") — domain logic, never imports OS-specific code.
- ALL OS-specific code lives ONLY in `src-tauri/src/platform/`, behind **Rust traits**
  (`traits.rs`), with impls selected via `#[cfg(target_os = "...")]`.
- Build the **macOS** impls now. Leave `platform/windows/` and `platform/linux/` as
  `todo!()` stubs — adding those OSes later means filling stubs, NOT restructuring.
- Platform traits: `DnsManager`, `CertTrustManager`, `PrivilegeManager`,
  `ProcessSupervisor`, `AutostartManager`, `PermissionManager`, `ShellRunner`,
  `Paths`, `BinaryProvider`.

## Non-negotiables (foundational decisions — don't relitigate)
- **No Docker.** Services are **native static binaries**, downloaded on demand (small installer).
- All binaries go through **`BinaryProvider`** (manifest: os+arch+version → url+checksum;
  checksum is SHA-256 or SHA-512 depending on source). **macOS `prepare_binary`:**
  de-quarantine (`xattr -d com.apple.quarantine`), **relink any Homebrew dylib deps to macOS
  system libs** (`install_name_tool -change … /usr/lib/…`, so the binary is self-contained — no
  Homebrew at runtime), then ad-hoc code-sign LAST (`codesign --force --sign -`; relinking
  invalidates the signature, and Apple Silicon kills unsigned binaries).
- **Request topology (default sites):** browser → **Caddy** (:443, TLS termination with
  local-CA certs; auto-HTTPS/internal issuer DISABLED) → one shared **Nginx** on an internal
  HTTP port (vhost by `server_name`) → **php-fpm** (FastCGI). Caddy proxies all `*.test` to
  the single Nginx port. **No direct Caddy→php-fpm path for default sites.**
- One shared **Nginx** process (a server block per site) is the default site server.
- **One PHP-FPM pool per PHP version** (not per site) — shared master, low memory.
- Caddy is the Phase 1 edge router (invisible to the user; may become custom Rust/Pingora later).
- Embedded DNS via **hickory-dns** resolves `*.test → 127.0.0.1` (wildcard ⇒ multisite works
  free). Runs as a managed task on a fixed loopback port (`DEFAULT_DNS_PORT`).
- Local **CA via rcgen** issues a trusted cert per domain; must support wildcard SAN (`*.site.test`).
- **System changes go through platform traits, never direct.** The `/etc/resolver/test` write
  is a root op via **`PrivilegeManager`** (macOS osascript admin prompt). The CA **trust** is a
  USER op via **`CertTrustManager`** (macOS: login keychain — `security add-trusted-cert` shows
  its own native dialog, no root; System-keychain trust can't be set from a detached-root
  osascript session). So system setup is ~2 prompts; a true single prompt needs a privileged
  helper (SMAppService) — deferred.
- **SQLite** for all app state (site list, settings, per-site config).
- Leave room in the config generator for THREE rewrite templates from the start:
  single / subdomain-multisite / subdirectory-multisite.

## Tech stack
- Backend: Tauri 2, Rust, tokio, hickory-dns, rcgen, reqwest, rusqlite/sqlx, sysinfo, which, notify.
- Frontend: React + TypeScript + Vite, Tailwind + shadcn/ui (Radix), TanStack Query
  (IPC/server state), Zustand (UI state), lucide-react, xterm.js.
- **Full folder tree: see README.md** ("Project structure"). Don't duplicate it here.

## Conventions
- **TypeScript strict** mode.
- All Tauri IPC goes through **typed wrappers in `src/lib/ipc/`** — UI never calls raw `invoke`.
- Design tokens come from **DESIGN_BRIEF.md** (palette + 3 type roles); surfaced as
  CSS vars in `src/styles/tokens.css` and the Tailwind theme. Don't hardcode hex values.
- `src/routes/` map **1:1** to the screens in DESIGN_BRIEF.md
  (Sites, SiteDetail, Services, Databases, Mail, Tunnels, Settings, Onboarding).
- JetBrains Mono for ALL technical values (domains, paths, versions, ports, commands).
  Space Grotesk for hero/onboarding only. Inter (SF Pro Text) for UI/body.

## Implementation notes (as built — macOS)
- **Fixed loopback ports:** DNS `15353` (`DEFAULT_DNS_PORT`), shared Nginx `8088`, php-fpm `9783`,
  MySQL `13306`; edge Caddy on real `:80`/`:443`. Every service start is port-gated via
  `core/ports::ensure_free`.
- **Pinned, checksum-locked binaries** (`core/binaries.rs`): Caddy `2.11.4`, PHP `8.3.31`
  (**static-php "bulk" build — "common" lacks `mysqli`, which WP requires**), Nginx `1.30.3`
  (jirutka static), MySQL `8.4.6` (official), WP-CLI `2.12.0`.
- **`prepare_binary` order: de-quarantine → relink Homebrew dylibs → codesign LAST.** Nginx
  (jirutka) links Homebrew `libpcre2` → relinked to `/usr/lib`. MySQL is a dir tree
  (`Archive::TarGzTree` + `resolve_dir`), Oracle-signed (no re-sign), pulled from a direct CDN
  URL with a browser UA (the redirector 403s reqwest). WP-CLI `.phar` uses `resolve_file` —
  NO chmod/codesign (not a Mach-O).
- **WP-CLI runs PHP with `-d memory_limit=512M`** — WP core extraction OOMs at the 128M default.
- **Quote all paths in generated Caddy/Nginx configs** — app-data paths contain spaces.
- **CA trust is login-keychain (dev), not System keychain** — osascript detached-root can't set
  `SecTrustSettings`. System-wide trust + single prompt = SMAppService (10.4, deferred).
- **Backgrounded osascript can't show the admin dialog** — run privileged steps foreground / `sudo`.
- **Long-running processes spawn via `ProcessSupervisor::spawn_logged`** → per-service
  `<log_dir>/<svc>-stdout.log`.
- **Verification pattern:** lib unit tests + standalone `src-tauri/examples/*.rs` for live checks.
- **Multi-PHP (Phase 2):** pinned `8.1.34` / `8.2.31` / `8.3.31`; one php-fpm pool per minor on
  `9781`/`9782`/`9783` (`9700 + major*10 + minor`, so 8.3 keeps the Phase-1 port); the installed set
  lives in the `php_versions` table (migration v2); a site's nginx block `fastcgi_pass`es its version's
  pool. Switching a site's version **or** server = config regen + reload, never a docroot/cert/DB rebuild.
- **Per-site override (Phase 2):** **FrankenPHP `1.12.4`** (one static binary, embeds its OWN PHP — not the
  §1 pools) runs as a loopback backend on a per-site port in `8200..8300` (FNV-1a of the domain), with
  `auto_https off` + `admin off` — it must never be the edge, never bind `:443`/`:2019`. The edge routes an
  override site's Host → its backend; default sites stay on the shared Nginx pool. `ServiceManager` owns the
  per-site backends (`reconcile_overrides` on start/reload).
- **Databases (Phase 2):** `core/db.rs` `DbEngine` unifies engines (MySQL delegates to `core/database`).
  **PostgreSQL `18.4.0`** (theseus-rs portable, `TarGzTree`, runs unsigned on Apple Silicon) on `15432`,
  **TCP-only** (`unix_socket_directories=` empty — sidesteps macOS's ~104-char Unix-socket-path limit).
  MariaDB/Redis deferred (§7) — no clean macOS binary.
- **Edge recovery (§7.3):** before binding the edge, `proxy::recover_stale_edge` stops a leftover Caddy on
  `:2019` via the admin API (`caddy stop`) — works on a **root** edge with no privilege (admin API has no
  owner check), then errors clearly if the port still can't be freed.
- **Services OUTLIVE the app:** closing rexenv does NOT stop the stack; on launch
  `ServiceManager::adopt_startup` ADOPTS rexenv-owned survivors (ownership = our fixed port + app-data
  marker on the cmdline → masters only; root edge via OUR admin unix socket) as pid-based `Proc::Adopted`
  handles — status/Stop all/Start all treat them like spawned children. The old stop-orphans-at-boot
  (`reconcile_startup`) remains only as an explicit cleanup path.
- **Phase 3 fixed ports:** Mailpit SMTP `1025` / HTTP-API `8025` (`core/mail`). Adminer + the tunnel origin
  reuse the shared nginx + php-fpm stack (no new inbound port); `cloudflared` is outbound-only.
- **Mail routing:** php-fpm `sendmail_path` (DOUBLE-quoted in the pool ini — the parser strips bare quotes
  and app-data paths contain spaces) → Mailpit's `sendmail -t -S 127.0.0.1:1025` shim → SMTP sink.
- **Tunnel discrimination:** behind the edge `REMOTE_ADDR` is always `127.0.0.1`, so loopback-only enforcement
  (e.g. the "Log in as" mu-plugin) keys off `CF-*` headers + leftmost `X-Forwarded-For` + `Host`, never the IP.
- **Tunnel URL rewrite (§9.3):** `core/wp_tunnel` bakes the public origin into an auto-managed mu-plugin on
  tunnel start (removed on stop) — CF-marked requests get `HTTP_HOST`/`HTTPS` overrides + siteurl/home/content
  filters + an output-buffer rewrite (plain/JSON-escaped/%-encoded), local `.test` requests stay untouched.
- **Multisite (§10):** mode in `sites.multisite` (migration v3) → `RewriteMode` (Phase 1 §6.2 templates);
  subdomain adds a wildcard `server_name`/Caddy host (`mysite.test, *.mysite.test`) over the per-site wildcard
  SAN cert — exact hosts still win, so it never shadows other `.test` sites.
- **Adminer deep-link (§11.4):** served via a generated `index.php` wrapper (`adminer_object()` hook) that
  `require`s `adminer.php`, accepts a **passwordless login for loopback servers only**, and auto-submits
  Adminer's own CSRF-tokened + CSP-nonced form on `?rexenv_auto`. Internal vhost, never a tunnel origin.
- **Autostart (§11.1):** `MacosAutostart` writes a per-user launchd LaunchAgent (`~/Library/LaunchAgents/
  dev.rexenv.rexenv.plist`, `RunAtLoad`) — launches the app at login (not headless services: the edge still
  needs the `:443` root prompt; true headless = SMAppService, deferred).
- **Blueprints (§11.3):** `blueprints` table (migration v4, JSON `spec`); `create_site(blueprint_id)` applies
  plugins/themes/WP_DEBUG + multisite convert AFTER the one-click install.

## Module map (as built)
- `core/`: `binaries` · `dns` · `ssl` · `proxy` (Caddy edge + stale-edge recovery) ·
  `services` (nginx + php-fpm) · `php` (multi-version pools + registry) · `frankenphp` (per-site
  override backend) · `database` (MySQL) · `postgres` · `db` (`DbEngine` abstraction) · `wordpress` ·
  `wp_login` (single-use loopback magic link) · `wp_tunnel` (public-URL rewrite while shared) ·
  `mail` (Mailpit + HTTP API) · `adminer` (internal vhost
  + deep-link wrapper) · `logs` (per-service tail) · `terminal` (PTY) · `tunnels` (cloudflared) ·
  `blueprints` (apply preset) · `sites` (provision / rebuild_configs / set_php_version / set_web_server /
  convert_multisite / teardown) · `setup` · `ports` · `monitor` (sysinfo) · `service_manager` (owns the
  whole stack: dbs, pools, overrides, mail, adminer, edge).
- `state/`: `db` (migrations v1–v4) · `models` (incl. `Blueprint*`) · `store` (repo) · `app` (`AppState` =
  db + platform + monitor + CA + `ServiceManager` + Terminals/Tunnels registries — **field-level locks**,
  not one big mutex; only `ServiceManager` is behind an async Mutex, status polls read a `try_lock` cache).
- **Locking rule (M4):** never hold the services lock across a wait — manager methods **spawn** under the
  lock and return `ReadyCheck` probes; commands `await_ready(checks)` AFTER dropping it (concurrent,
  M3-style named-service errors). Same two-phase shape as `prepare_edge` (privileged edge prompt).
- `commands/` (thin): `system` (incl. DNS/SSL/autostart) · `sites` · `services` · `database` · `php` ·
  `settings` · `wordpress` · `mail` · `logs` · `terminal` · `tunnels` · `blueprints`.
- `platform/macos/mod.rs`: all 9 trait impls real (incl. `AutostartManager`); `windows`/`linux` = `todo!()`.

## Pointers
- Full detail in **PROJECT_SPEC.md**. Screen designs in **DESIGN_BRIEF.md** and **design/** (.dc.html).
- Phase plan: PROJECT_SPEC.md §5. **Phase 1 · Phase 2 core · Phase 3 all COMPLETE** (see Status above).
  Task logs + "Done when" evidence: **TASKS.md** (Phase 1), **TASKS-PHASE2.md** (Phase 2),
  **TASKS-PHASE3.md** (Phase 3). Xdebug build recipe: **docs/xdebug-debug-build.md** (§11.2, awaiting host).

## Working rule
- Work in **small, verifiable steps. One task at a time. Verify before moving on.**
- Surface assumptions before non-trivial work; keep changes surgical (scope discipline).

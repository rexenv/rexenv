# CLAUDE.md — rexenv

rexenv is a native, lightweight, NO-Docker local development environment for web &
WordPress developers. Tauri 2 desktop app (Rust backend + React/TS frontend). It runs
a developer's whole local stack — web servers, multiple PHP versions, databases,
one-click WordPress, local `.test` domains with auto-HTTPS, mail catching, public
sharing — from one UI. **macOS first**, then Windows, then Linux.

## Status — Phase 1 (macOS MVP) COMPLETE
- **Phase 1 goal MET, verified on real :443:** one-click WordPress at `https://wpdemo.test`
  over the full chain browser → Caddy (:443, local-CA TLS) → shared Nginx (by `server_name`)
  → php-fpm → WordPress → MySQL (homepage HTTP 200, served cert issuer = our CA).
- **Done:** scaffold + design tokens + app shell; SQLite store; embedded DNS (managed, `:15353`)
  + `/etc/resolver/test`; local CA + per-site wildcard certs + login-keychain trust; Caddy edge;
  static PHP + one php-fpm pool; shared Nginx; site create/list/start/stop/delete; sysinfo
  metrics; MySQL; WP-CLI + one-click WordPress; port allocator; configurable sites folder;
  per-service logs; app service manager (owns the stack). Per-task "Done when" evidence in **TASKS.md**.
- **Pending:** 10.4 privileged helper (SMAppService → single prompt + System-keychain trust) —
  ⏸ packaging-era, needs a signed+notarized bundle. New-Site UI form (backend create flow done).
  In-window visual pass via `pnpm tauri dev` (stack so far verified via `examples/`).
- **Next:** Phase 2 (multi-PHP, Apache/OpenLiteSpeed, MariaDB/PostgreSQL/Redis) — PROJECT_SPEC.md §5.

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

## Module map (as built)
- `core/`: `binaries` · `dns` · `ssl` · `proxy` (Caddy) · `services` (nginx + php-fpm) ·
  `database` (MySQL) · `wordpress` · `sites` (provision / rebuild_configs / teardown) ·
  `setup` (system setup) · `ports` · `monitor` (sysinfo) · `service_manager` (owns the stack).
- `state/`: `db` (migrations) · `models` · `store` (repo) · `app` (`AppState` = db + platform +
  monitor + CA + `ServiceManager`, behind an async Mutex).
- `commands/` (thin): `system` · `sites` · `services` · `settings`.
- `platform/macos/mod.rs`: all 9 trait impls real; `windows`/`linux` = `todo!()`.

## Pointers
- Full detail in **PROJECT_SPEC.md**. Screen designs in **DESIGN_BRIEF.md** and **design/** (.dc.html).
- Phase plan: PROJECT_SPEC.md §5. **Phase 1 (macOS MVP) COMPLETE** (see Status above); next is
  Phase 2. Full task log + "Done when" evidence in **TASKS.md**.

## Working rule
- Work in **small, verifiable steps. One task at a time. Verify before moving on.**
- Surface assumptions before non-trivial work; keep changes surgical (scope discipline).

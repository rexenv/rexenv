# TASKS — Phase 1 (macOS MVP)

> **Phase goal:** a WordPress site running locally over HTTPS at `https://<name>.test`.
> Scope: macOS only. Build `platform/` macOS impls; leave windows/linux as `todo!()`.
> Work top-to-bottom, one task at a time. Check the box only when "Done when" passes.
> Source: PROJECT_SPEC.md §5.

Status: `[ ]` todo · `[~]` in progress · `[x]` done

---

## 0. Scaffold & foundation

- [x] **0.1 Tauri 2 + React + TS + Vite project**
  *Done when:* `pnpm tauri dev` opens a window rendering a React page; `pnpm build` and `cargo check` (in `src-tauri/`) both pass. ✓ build + cargo check pass.
- [x] **0.2 Tailwind + shadcn/ui + design tokens**
  *Done when:* `src/styles/tokens.css` holds the DESIGN_BRIEF palette as CSS vars, Tailwind theme maps to them, the 3 fonts load, and one shadcn component renders with brand violet. ✓ tokens.css + tailwind theme + @fontsource fonts + Button primitive.
- [x] **0.3 Folder structure (README target)**
  *Done when:* `src/{routes,components/{ui,shell,common},features,lib/ipc,hooks,stores,types,styles,assets}` and `src-tauri/src/{commands,core,platform,state,templates,utils}` exist; `cargo check` passes with module stubs wired in `lib.rs`/`mod.rs`. ✓ (core/* submodules start as files; grow into folders per task).
- [x] **0.4 `platform/` traits + macOS stubs**
  *Done when:* `platform/traits.rs` defines all 9 traits; `platform/macos/` has structs implementing them (real or stub), `windows/`+`linux/` are `todo!()`; `platform/mod.rs` selects via `#[cfg(target_os)]`; `cargo check` passes on macOS. ✓ Paths/Permissions/Supervisor/Shell real on macOS; rest `todo!()`.
- [x] **0.5 Typed IPC bridge**
  *Done when:* one round-trip command (e.g. `ping`/`app_version`) is callable from the UI **only** through `src/lib/ipc/`, returns typed data, and the type mirrors the Rust struct. ✓ `app_info` cmd ↔ `getAppInfo()` wrapper (AppInfo type mirrors Rust struct).
- [x] **0.6 Static app shell (sidebar + top bar + status footer)**
  *Done when:* shell matches `design/rexenv App Shell.dc.html` (220px sidebar, 84px header, nav items, live status footer) with mock data; routes navigate; no backend calls yet. ✓ Sidebar + TopBar + StatusFooter + 8 routes, mock data.

## 1. SQLite app state

- [x] **1.1 SQLite store + schema**
  *Done when:* DB file is created in the macOS app-data path (via `Paths` trait); `sites` + `settings` tables exist via migration; insert/read round-trips in a Rust test. ✓ `state/db.rs` user_version migration runner + `open_for_platform`; 4 passing tests (schema, round-trip, idempotent, file-persist).
- [ ] **1.2 Site model + repository**
  *Done when:* `Site` struct (name, domain, php_version, server, path, status) persists and lists from SQLite through `core/sites`.

## 2. DNS (embedded resolver)

- [ ] **2.1 hickory-dns resolver for `*.test`**
  *Done when:* the embedded resolver answers A queries for any `*.test` host with `127.0.0.1` (verified with `dig @127.0.0.1 -p <port> foo.test`).
- [ ] **2.2 macOS `DnsManager` (resolver hookup)**
  *Done when:* `/etc/resolver/test` is written so the OS routes `.test` to our resolver; `ping foo.test` resolves to `127.0.0.1`. (windows/linux = `todo!()`.)

## 3. Local CA & certificates

- [ ] **3.1 Local CA generation (rcgen)**
  *Done when:* a root CA key+cert is generated once and stored under app-data; regeneration is idempotent.
- [ ] **3.2 Per-site cert with wildcard SAN**
  *Done when:* given `mysite.test`, a cert signed by the CA is issued with SAN `mysite.test` + `*.mysite.test` (verify with `openssl x509 -text`).
- [ ] **3.3 macOS `CertTrustManager`**
  *Done when:* `security add-trusted-cert` adds the CA to the login keychain as trusted; a served site shows a valid lock in the browser. (windows/linux = `todo!()`.)

## 4. Edge router (Caddy)

- [ ] **4.1 Caddy binary provider**
  *Done when:* `BinaryProvider` downloads the correct macOS (arm64/x86_64) Caddy binary on demand, verifies checksum, and caches it under app-data.
- [ ] **4.2 Caddy config generation + supervise**
  *Done when:* `core/proxy` writes a Caddyfile and `ProcessSupervisor` starts/stops Caddy holding :80/:443; a hardcoded route proxies to a test backend over HTTPS with the trusted cert.

## 5. PHP-FPM (one version)

- [ ] **5.1 PHP static binary provider**
  *Done when:* a static PHP (static-php-cli build) for macOS is downloaded/cached and `php -v` runs from the app-data path.
- [ ] **5.2 One PHP-FPM pool**
  *Done when:* a single php-fpm master starts on a loopback port via `ProcessSupervisor`, with a generated pool config; status reflects in the UI/services layer.

## 6. Web server (Nginx)

- [ ] **6.1 Nginx binary provider**
  *Done when:* prebuilt Nginx for macOS is downloaded/cached and `nginx -v` runs.
- [ ] **6.2 Shared Nginx + per-site server block**
  *Done when:* one Nginx process serves a site from its docroot via a generated server block (FastCGI → php-fpm); the config generator leaves slots for single / subdomain / subdirectory rewrite templates.

## 7. Site create / list (end-to-end wiring)

- [ ] **7.1 `create site` flow (Blank PHP)**
  *Done when:* creating a site writes the DB row, makes the docroot, issues the cert, adds the Nginx server block + Caddy route, reloads both; `https://<name>.test` serves a PHP `phpinfo()` page with a valid lock.
- [ ] **7.2 Sites list + start/stop**
  *Done when:* the Sites screen lists real sites from SQLite (via typed IPC) and start/stop toggles drive the backend, reflecting real status.

## 8. Database service (MySQL)

- [ ] **8.1 MySQL binary provider + lifecycle**
  *Done when:* MySQL is downloaded/cached, initialized to an app-data datadir, and `ProcessSupervisor` starts/stops it on a known port; a client connects.

## 9. One-click WordPress (phase goal)

- [ ] **9.1 WP-CLI via bundled PHP**
  *Done when:* `wp-cli.phar` runs through the bundled PHP (`wp --info`) from the app.
- [ ] **9.2 One-click WordPress install**
  *Done when:* choosing "WordPress" on create downloads core, creates a MySQL DB, writes `wp-config.php`, runs `wp core install`, and `https://<name>.test` loads a working WP site reachable in the browser with a trusted cert. **← Phase 1 goal met.**

---

## Notes / decisions
- Keep `commands/` thin → `core/` (platform-agnostic) → `platform/` traits. No OS code in `core/`.
- Every binary goes through `BinaryProvider` (manifest: os+arch+version → url+checksum). No bundling.
- Config generator must support 3 rewrite modes from the start (single / subdomain / subdirectory).

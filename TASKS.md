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
- [x] **1.2 Site model + repository**
  *Done when:* `Site` struct (name, domain, php_version, server, path, status) persists and lists from SQLite through `core/sites`. ✓ `state/models.rs` (Site/NewSite + enums, serde camelCase mirroring TS), `state/store.rs` repository, `core/sites.rs` create/list/get/delete. 4 passing tests (round-trip, dup-domain reject, get/delete, enum TEXT storage).

## 2. DNS (embedded resolver) & privileges

- [x] **2.1 hickory-dns resolver for `*.test`**
  *Done when:* the embedded resolver answers A queries for any `*.test` host with `127.0.0.1` (verified with `dig @127.0.0.1 -p <port> foo.test`). ✓ `core/dns.rs` `DnsHandler` + `serve_udp`; 3 passing UDP tests; dig on port 15353 returns 127.0.0.1 for `foo.test` & `site1.mysite.test`, NXDOMAIN for `example.com`. Manual check: `cargo run --example dns_serve`.
- [ ] **2.2 Run the embedded resolver as a managed service**
  *Done when:* the resolver binds a **fixed** loopback UDP port (>1024, no privilege — the `DEFAULT_DNS_PORT` constant) and is started on app launch via `ProcessSupervisor` / a managed background task, and stopped cleanly on exit. 2.4's `/etc/resolver/test` references **this exact port**. With the app running (not the manual example), `dig @127.0.0.1 -p <port> foo.test` returns `127.0.0.1`; after 3.4, `ping foo.test` works. Depends on 2.1.
- [ ] **2.3 macOS `PrivilegeManager` (admin auth prompt)** — prerequisite for 2.4, 3.3, 3.4
  *Done when:* `PrivilegeManager` runs a privileged shell op via a macOS admin auth prompt (e.g. `osascript -e 'do shell script "…" with administrator privileges'` or a privileged helper); a no-op privileged command (e.g. writing a root-owned temp file) succeeds after a single password prompt, and success/failure is reported cleanly. (windows/linux = `todo!()`.)
- [ ] **2.4 macOS `DnsManager` (resolver file → PrivilegeManager)**
  *Done when:* `DnsManager` generates the correct `/etc/resolver/test` contents (`nameserver 127.0.0.1` + `port <resolver port>`, the **fixed port from 2.2**) and installs/removes it **through `PrivilegeManager`** (never writing `/etc` directly); file contents asserted in a unit test (no sudo). Live install + `ping foo.test` → `127.0.0.1` is verified in the batched system-setup step (3.4). Depends on 2.2, 2.3. (windows/linux = `todo!()`.)

## 3. Local CA, certificates & system trust setup

- [ ] **3.1 Local CA generation (rcgen)**
  *Done when:* a root CA key+cert is generated once and stored under app-data; regeneration is idempotent.
- [ ] **3.2 Per-site cert with wildcard SAN**
  *Done when:* given `mysite.test`, a cert signed by the CA is issued with SAN `mysite.test` + `*.mysite.test` (verify with `openssl x509 -text`).
- [ ] **3.3 macOS `CertTrustManager` (trust CA → PrivilegeManager)**
  *Done when:* `CertTrustManager` builds the `security add-trusted-cert …` (and untrust) invocation for the local CA and runs it **through `PrivilegeManager`** (no direct privileged calls); command construction is unit-tested. Actual keychain trust + the browser "valid lock" check happen in the batched system-setup step (3.4). Depends on 2.3, 3.1. (windows/linux = `todo!()`.)
- [ ] **3.4 Batched system setup (single auth prompt)**
  *Done when:* one "system setup" action runs the DnsManager resolver-file install (2.4) **and** the CertTrustManager CA trust (3.3) together through a **single** `PrivilegeManager` elevation — the user is prompted for a password **once**. Afterwards `ping foo.test` resolves to `127.0.0.1` and a served `.test` site shows a valid lock in the browser. Depends on 2.3, 2.4, 3.1–3.3.

## 4. Edge router (Caddy)

- [ ] **4.1 Caddy binary provider**
  *Done when:* `BinaryProvider` downloads the correct macOS (arm64/x86_64) Caddy binary on demand, verifies checksum, caches it under app-data, then **ad-hoc code-signs (`codesign --force --sign - <path>`) and de-quarantines (`xattr -d com.apple.quarantine <path>` if present) before first exec** (else Apple Silicon kills it); `caddy version` runs from the cached path.
- [ ] **4.2 Caddy config generation + supervise**
  *Done when:* `core/proxy` writes a Caddyfile and `ProcessSupervisor` starts/stops Caddy holding :80/:443. Caddy terminates TLS using the **per-site certs issued by our local CA (3.2)** via explicit `tls <cert> <key>` directives, with Caddy's **automatic HTTPS / internal issuer DISABLED** (so the chain the browser sees matches the CA trusted in 3.4). A hardcoded `*.test` route proxies to a test backend; `openssl s_client -connect <host>:443` (or browser cert inspect) shows the served cert's **issuer is our local CA**.

## 5. PHP-FPM (one version)

- [ ] **5.1 PHP static binary provider**
  *Done when:* a static PHP (static-php-cli build) for macOS is downloaded/cached, **signed + de-quarantined via `BinaryProvider` (see 4.1)**, and `php -v` runs from the app-data path.
- [ ] **5.2 One PHP-FPM pool**
  *Done when:* a single php-fpm master starts on a loopback port via `ProcessSupervisor`, with a generated pool config; status reflects in the UI/services layer.

## 6. Web server (Nginx)

- [ ] **6.1 Nginx binary provider**
  *Done when:* prebuilt Nginx for macOS is downloaded/cached, **signed + de-quarantined via `BinaryProvider` (see 4.1)**, and `nginx -v` runs.
- [ ] **6.2 Shared Nginx + per-site server block**
  *Done when:* one shared Nginx process (listening on an **internal HTTP port**, plain HTTP — TLS is Caddy's job) serves a site from its docroot via a generated server block selected by `server_name` (FastCGI → php-fpm); the config generator leaves slots for single / subdomain / subdirectory rewrite templates.

## 7. Site create / list (end-to-end wiring)

- [ ] **7.1 `create site` flow (Blank PHP)**
  *Done when:* creating a site writes the DB row, makes the docroot, issues the cert, adds the Nginx server block (internal HTTP) + a Caddy `*.test`→Nginx TLS route, reloads both; the request flows **browser → Caddy (TLS) → shared Nginx (by `server_name`) → php-fpm**, and `https://<name>.test` serves a PHP `phpinfo()` page with a valid lock (issuer = local CA).
- [ ] **7.2 Sites list + start/stop**
  *Done when:* the Sites screen lists real sites from SQLite (via typed IPC) and start/stop toggles drive the backend, reflecting real status.
- [ ] **7.3 Delete site (full teardown)**
  *Done when:* deleting a site removes its DB row, generated configs (Nginx server block + Caddy route), its cert, and its docroot, then reloads Caddy + Nginx; the site leaves the list and `https://<name>.test` no longer serves. Site management is now full create / list / start / stop / delete.
- [ ] **7.4 Resource monitor (real metrics via `sysinfo`)**
  *Done when:* the `sysinfo` crate provides real per-service RAM/CPU (by supervised PID) plus totals, surfaced through typed IPC; the sidebar status footer and the Services view show **live** values, replacing the mock data in `StatusFooter`/`mock.ts`.

## 8. Database service (MySQL)

- [ ] **8.1 MySQL binary provider + lifecycle**
  *Done when:* MySQL is downloaded/cached (**signed + de-quarantined via `BinaryProvider`, see 4.1**), initialized to an app-data datadir, and `ProcessSupervisor` starts/stops it on a known port; a client connects.

## 9. One-click WordPress (phase goal)

- [ ] **9.1 WP-CLI via bundled PHP**
  *Done when:* `wp-cli.phar` runs through the bundled PHP (`wp --info`) from the app.
- [ ] **9.2 One-click WordPress install**
  *Done when:* choosing "WordPress" on create downloads core, creates a MySQL DB, writes `wp-config.php`, runs `wp core install`, and `https://<name>.test` loads a working WP site reachable in the browser with a trusted cert. **← Phase 1 goal met.**

---

## 10. Optional / later (Phase 1+)

> Not required for the Phase 1 goal, but cheap to design for early. Pick up when the listed prerequisite lands.

- [ ] **10.1 Port allocation + conflict detection**
  *Done when:* a central allocator hands out / records ports for :80/:443 and per-service loopback ports, detects an already-bound port before spawn, and surfaces a clear error instead of a silent crash. *Ideally designed in from the first spawned service (§4).*
- [ ] **10.2 Configurable sites folder (Paths + settings)**
  *Done when:* the sites root is read from a `settings` value (falling back to the `Paths` default), is editable in Settings, and new sites are created under it. *Touches `Paths` + `state` (§1) and the create flow (§7).*
- [ ] **10.3 Per-service log capture**
  *Done when:* every supervised process has its stdout/stderr redirected to a per-service log file under the `Paths` log dir from the moment it spawns, ready for the Phase 3 log viewer. *Ideally wired into `ProcessSupervisor` from the first spawned service (§4).*

---

## Notes / decisions
- Keep `commands/` thin → `core/` (platform-agnostic) → `platform/` traits. No OS code in `core/`.
- Every binary goes through `BinaryProvider` (manifest: os+arch+version → url+checksum). No bundling. **macOS:** after download/extract, `BinaryProvider` must ad-hoc code-sign (`codesign --force --sign - <path>`) and de-quarantine (`xattr -d com.apple.quarantine <path>` if present) before first exec — unsigned binaries are killed on Apple Silicon.
- All privileged OS changes (writing `/etc/resolver/test`, trusting the CA) go **through `PrivilegeManager`**, never direct — and are batched so the user authenticates **once** (see 3.4).
- **Request topology (default sites):** browser → Caddy (`:443`, TLS termination with local-CA certs) → one **shared Nginx** on an internal HTTP port (vhost by `server_name`) → php-fpm (FastCGI). Caddy proxies all `*.test` to the single Nginx port; Nginx dispatches by `server_name`. **No direct Caddy→php-fpm path for default sites.** (4.2, 6.2, 7.1 follow this.)
- Config generator must support 3 rewrite modes from the start (single / subdomain / subdirectory).

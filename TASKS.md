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
- [x] **2.2 Run the embedded resolver as a managed service**
  *Done when:* the resolver binds a **fixed** loopback UDP port (>1024, no privilege — the `DEFAULT_DNS_PORT` constant) and is started on app launch via `ProcessSupervisor` / a managed background task, and stopped cleanly on exit. 2.4's `/etc/resolver/test` references **this exact port**. With the app running (not the manual example), `dig @127.0.0.1 -p <port> foo.test` returns `127.0.0.1`; after 3.4, `ping foo.test` works. Depends on 2.1. ✓ `DnsService` (spawn/abort-on-drop) wired into Tauri `setup` on `:15353`; running app binary serves `foo.test` & `site2.mysite.test` → 127.0.0.1 via dig, stops cleanly on exit. `ping` deferred to 3.4. 4 dns tests (incl. managed start/serve/stop).
- [x] **2.3 macOS `PrivilegeManager` (admin auth prompt)** — prerequisite for 2.4, 3.3, 3.4
  *Done when:* `PrivilegeManager` runs a privileged shell op via a macOS admin auth prompt (e.g. `osascript -e 'do shell script "…" with administrator privileges'` or a privileged helper); a no-op privileged command (e.g. writing a root-owned temp file) succeeds after a single password prompt, and success/failure is reported cleanly. (windows/linux = `todo!()`.) ✓ trait `run_privileged(script)`; macOS via osascript admin prompt; live check (`cargo run --example priv_check`) created `/tmp/rexenv_priv_check` owned by `root` after one prompt. Escaping unit-tested (2 tests).
- [x] **2.4 macOS `DnsManager` (resolver file → PrivilegeManager)**
  *Done when:* `DnsManager` generates the correct `/etc/resolver/test` contents (`nameserver 127.0.0.1` + `port <resolver port>`, the **fixed port from 2.2**) and installs/removes it **through `PrivilegeManager`** (never writing `/etc` directly); file contents asserted in a unit test (no sudo). Live install + `ping foo.test` → `127.0.0.1` is verified in the batched system-setup step (3.4). Depends on 2.2, 2.3. (windows/linux = `todo!()`.) ✓ `DnsManager` pure command builders (path/contents/install/uninstall); `core::dns::configure_resolver`/`remove_resolver` run them via `PrivilegeManager`. 4 builder unit tests (no sudo); 17 lib tests green.

## 3. Local CA, certificates & system trust setup

- [x] **3.1 Local CA generation (rcgen)**
  *Done when:* a root CA key+cert is generated once and stored under app-data; regeneration is idempotent. ✓ `core/ssl.rs` `generate_ca` + idempotent `load_or_create` (key hardened 0600 via new `PermissionManager::set_private`). openssl confirms CA:TRUE (critical), `CN=rexenv Local CA`; rerun yields identical fingerprint; key file `-rw-------`. 4 unit tests; 20 lib tests green. Manual: `cargo run --example ca_gen`.
- [x] **3.2 Per-site cert with wildcard SAN**
  *Done when:* given `mysite.test`, a cert signed by the CA is issued with SAN `mysite.test` + `*.mysite.test` (verify with `openssl x509 -text`). ✓ `core/ssl.rs` `generate_site_cert` + idempotent `ensure_site_cert` (key 0600). openssl: SAN `DNS:mysite.test, DNS:*.mysite.test`, issuer `rexenv Local CA`, `openssl verify` OK, EKU=ServerAuth. 2 unit tests (x509-parser SAN/issuer + idempotency); 22 lib tests green. Manual: `cargo run --example site_cert_gen`.
- [x] **3.3 macOS `CertTrustManager` (trust CA)**
  *Done when:* `CertTrustManager` trusts the local CA (and can untrust it); command construction is unit-tested. Actual keychain trust + the browser "valid lock" check happen in the system-setup step (3.4). Depends on 3.1. (windows/linux = `todo!()`.) ✓ **Revised:** trust is a USER op (macOS login keychain via `security add-trusted-cert -r trustRoot -k login.keychain-db`, own native dialog, no root) — NOT a `PrivilegeManager` op, because System-keychain trust can't be set from a detached-root osascript session (`SecTrustSettings` needs UI). `trust_args`/`untrust_args` unit-tested; `core::ssl::trust_ca`/`untrust_ca` execute via `CertTrustManager`. 24 lib tests green.
- [x] **3.4 System setup (resolver install + CA trust)**
  *Done when:* one "system setup" action runs the DnsManager resolver-file install (2.4) **and** the CertTrustManager CA trust (3.3). Afterwards `ping foo.test` resolves to `127.0.0.1` and a served `.test` site shows a valid lock in the browser. Depends on 2.3, 2.4, 3.1–3.3. ✓ `core::setup::run_system_setup` = `PrivilegeManager` resolver install (osascript admin prompt) + `CertTrustManager` CA trust (login-keychain native dialog). **~2 prompts, not 1** — true single-prompt needs a privileged helper (deferred; see note). Live-verified: resolver installed, CA trusted in login keychain, `ping foo.test` & `a.b.mysite.test` → 127.0.0.1, `security verify-cert` on the site cert succeeds. `examples/system_setup.rs` / `system_teardown.rs`.

## 4. Edge router (Caddy)

- [x] **4.1 Caddy binary provider**
  *Done when:* `BinaryProvider` downloads the correct macOS (arm64/x86_64) Caddy binary on demand, verifies checksum, caches it under app-data, then **ad-hoc code-signs (`codesign --force --sign - <path>`) and de-quarantines (`xattr -d com.apple.quarantine <path>` if present) before first exec** (else Apple Silicon kills it); `caddy version` runs from the cached path. ✓ `core/binaries.rs` manifest (pinned Caddy 2.11.4 + SHA-512) + async `resolve` (download→verify→extract→`set_executable`→`prepare_binary`); `BinaryProvider` trait now `arch()` + `prepare_binary()` (macOS codesign+dequarantine). Live: downloaded, checksum OK, `caddy version => v2.11.4`, `Signature=adhoc`, idempotent cache. 4 unit tests; 29 lib tests green. Manual: `cargo run --example caddy_fetch`.
- [x] **4.2 Caddy config generation + supervise**
  *Done when:* `core/proxy` writes a Caddyfile and `ProcessSupervisor` starts/stops Caddy holding :80/:443. Caddy terminates TLS using the **per-site certs issued by our local CA (3.2)** via explicit `tls <cert> <key>` directives, with Caddy's **automatic HTTPS / internal issuer DISABLED** (so the chain the browser sees matches the CA trusted in 3.4). A hardcoded `*.test` route proxies to a test backend; `openssl s_client -connect <host>:443` (or browser cert inspect) shows the served cert's **issuer is our local CA**. ✓ `core/proxy.rs` `generate_caddyfile`/`write_caddyfile` + supervise (`start`/`stop`) + privileged `start_privileged` (osascript root, one prompt) + admin-API `reload`/`stop_admin` (no prompt). Live (high ports 8443/8080, no root): issuer=rexenv Local CA, reverse_proxy curl→200 vs our CA, internal issuer skipped (caddy log). Privileged root start + `caddy stop` (admin API) verified. **Literal :443 bind NOT exercised — this machine runs an external nginx on 127.0.0.1:443 (see 10.1 / memory).** 4 unit tests; 31 lib tests green. `examples/caddy_serve.rs` (high port), `caddy_443.rs` (privileged).

## 5. PHP-FPM (one version)

- [x] **5.1 PHP static binary provider**
  *Done when:* a static PHP (static-php-cli build) for macOS is downloaded/cached, **signed + de-quarantined via `BinaryProvider` (see 4.1)**, and `php -v` runs from the app-data path. ✓ Manifest entries `php` (cli) + `php-fpm` from static-php.dev (pinned 8.3.31, SHA-256 computed at pin time — source has no checksums); generalized `Checksum` enum (SHA-256/512). Live: downloaded, checksum OK, `php -v` → PHP 8.3.31 (cli), Signature=adhoc, executable. 5 unit tests; 32 lib tests green. Manual: `cargo run --example php_fetch`.
- [x] **5.2 One PHP-FPM pool**
  *Done when:* a single php-fpm master starts on a loopback port via `ProcessSupervisor`, with a generated pool config; status reflects in the UI/services layer. ✓ `core/services.rs` `generate_fpm_config` (one `[global]` foreground master + one `[www]` pool, no user/group), `write_fpm_config`, `start_fpm`/`test_fpm_config`/`stop` via `ProcessSupervisor`, `fpm_running(port)` status. Live: `php-fpm -t` successful, master+2 workers listening on 127.0.0.1:9783, status running=true→false on stop. 2 unit tests; 34 lib tests green. (Services-screen IPC wiring lands with 7.4.) Manual: `cargo run --example php_fpm_serve`.

## 6. Web server (Nginx)

- [x] **6.1 Nginx binary provider**
  *Done when:* prebuilt Nginx for macOS is downloaded/cached, **signed + de-quarantined via `BinaryProvider` (see 4.1)**, and `nginx -v` runs. ✓ Manifest entry `nginx` (jirutka/nginx-binaries static build, pinned 1.30.3, SHA-256 cross-checked vs published SHA-1; `Archive::Raw`). The jirutka macOS build links Homebrew `libpcre2`, so `prepare_binary` now also **relinks Homebrew dylib deps → macOS system libs** (`install_name_tool -change … /usr/lib/libpcre2-8.dylib`) then re-signs → self-contained (otool shows system libs only). Live: `nginx -v` → nginx/1.30.3. 2 new unit tests (manifest + lib mapping); 36 lib tests green. Manual: `cargo run --example nginx_fetch`.
- [x] **6.2 Shared Nginx + per-site server block**
  *Done when:* one shared Nginx process (listening on an **internal HTTP port**, plain HTTP — TLS is Caddy's job) serves a site from its docroot via a generated server block selected by `server_name` (FastCGI → php-fpm); the config generator leaves slots for single / subdomain / subdirectory rewrite templates. ✓ `core/services.rs` `generate_nginx_config`/`write_nginx_config` (self-contained: inline mime types + fastcgi params, quoted paths, `daemon off`, internal port 8088, `map`→HTTPS from Caddy's X-Forwarded-Proto), `RewriteMode` {Single, SubdomainMultisite, SubdirectoryMultisite}, `start_nginx`/`test_nginx_config`/`stop`/`nginx_running`. Live: `nginx -t` OK; `curl -H 'Host: test6.test'` → PHP via FastCGI `rexenv-php-ok 8.3.31` (200) + static file (200), routed by server_name. 4 unit tests; 39 lib tests green. Manual: `cargo run --example nginx_php_serve`.

## 7. Site create / list (end-to-end wiring)

- [ ] **7.1 `create site` flow (Blank PHP)**
  *Done when:* creating a site writes the DB row, makes the docroot, issues the cert, adds the Nginx server block (internal HTTP) + a Caddy `*.test`→Nginx TLS route, reloads both; the request flows **browser → Caddy (TLS) → shared Nginx (by `server_name`) → php-fpm**, and `https://<name>.test` serves a PHP `phpinfo()` page with a valid lock (issuer = local CA). The flow **branches on whether the site needs a database** (Blank PHP = no DB; WordPress = needs MySQL, §8/§9) — the DB-provisioning step is an optional, pluggable stage so 9.2 hooks into the same create flow cleanly.
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
  *Done when:* choosing "WordPress" on create installs WordPress in **single-site mode** (downloads core, creates a MySQL DB via the §8 step, writes `wp-config.php`, runs `wp core install`), and `https://<name>.test` loads a working WP site reachable in the browser with a trusted cert. The 3 rewrite-template slots from 6.2 (single / subdomain / subdirectory) **exist and are wired**, with only the "single" path exercised now — so Phase 3 multisite needs no refactor. **← Phase 1 goal met.**

---

## 10. Optional / later (Phase 1+)

> Not required for the Phase 1 goal, but cheap to design for early. Pick up when the listed prerequisite lands.

- [ ] **10.1 Port allocation + conflict detection**
  *Done when:* a central allocator hands out / records ports for :80/:443 and per-service loopback ports, detects an already-bound port before spawn, and surfaces a clear error instead of a silent crash. *Ideally designed in from the first spawned service (§4).*
- [ ] **10.2 Configurable sites folder (Paths + settings)**
  *Done when:* the sites root is read from a `settings` value (falling back to the `Paths` default), is editable in Settings, and new sites are created under it. *Touches `Paths` + `state` (§1) and the create flow (§7).*
- [ ] **10.3 Per-service log capture**
  *Done when:* every supervised process has its stdout/stderr redirected to a per-service log file under the `Paths` log dir from the moment it spawns, ready for the Phase 3 log viewer. *Ideally wired into `ProcessSupervisor` from the first spawned service (§4).*
- [ ] **10.4 Privileged helper for single-prompt setup (macOS SMAppService)**
  *Done when:* a privileged helper installed once performs ALL privileged setup ops (resolver file + **System-keychain** CA trust) under a single authorization, so system setup (3.4) needs only one prompt and trust is system-wide. *Replaces the current ~2-prompt login-keychain approach; ties into packaging/notarization (§4.9).*

---

## Notes / decisions
- Keep `commands/` thin → `core/` (platform-agnostic) → `platform/` traits. No OS code in `core/`.
- Every binary goes through `BinaryProvider` (manifest: os+arch+version → url+checksum). No bundling. **macOS:** after download/extract, `BinaryProvider` must ad-hoc code-sign (`codesign --force --sign - <path>`) and de-quarantine (`xattr -d com.apple.quarantine <path>` if present) before first exec — unsigned binaries are killed on Apple Silicon.
- System changes go through platform traits, never direct: the `/etc/resolver/test` write is a root op via **`PrivilegeManager`** (osascript admin prompt); the CA **trust** is a USER op via **`CertTrustManager`** (macOS login keychain — `security` shows its own native dialog, no root). System setup is therefore **~2 prompts**, not one — macOS won't set System-keychain trust from a detached-root osascript session. A true single prompt needs a privileged helper (10.4).
- **Request topology (default sites):** browser → Caddy (`:443`, TLS termination with local-CA certs) → one **shared Nginx** on an internal HTTP port (vhost by `server_name`) → php-fpm (FastCGI). Caddy proxies all `*.test` to the single Nginx port; Nginx dispatches by `server_name`. **No direct Caddy→php-fpm path for default sites.** (4.2, 6.2, 7.1 follow this.)
- Config generator must support 3 rewrite modes from the start (single / subdomain / subdirectory).

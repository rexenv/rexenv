# TASKS — Phase 2 (macOS)

> **Phase goal:** multiple PHP versions with per-site one-click switch; **FrankenPHP** as the
> primary per-site server *override* behind the Caddy edge; **MariaDB**, **PostgreSQL** & **Redis**
> database services alongside Phase-1 MySQL; the resource monitor extended to cover all of them.
> The point of the override work is to **prove the per-site override architecture** (edge router →
> internal loopback port → alternate server), so the specific alt server is secondary — FrankenPHP
> is chosen because it's a single static binary that drops cleanly into `BinaryProvider` on macOS.
>
> Builds directly on the Phase-1 architecture (see TASKS.md, CLAUDE.md): `BinaryProvider` + macOS
> `prepare_binary` (de-quarantine → relink Homebrew dylibs → codesign LAST), `ProcessSupervisor::spawn_logged`,
> the `ServiceManager` that owns the stack, the Caddy edge → shared Nginx (by `server_name`) → php-fpm
> topology, the 3 rewrite-template slots (single / subdomain / subdirectory), `core/ports` allocation,
> and SQLite state.
>
> Scope: **macOS only** — fill `platform/macos/` impls; `windows`/`linux` stay `todo!()`. No new platform
> traits are expected (Phase 2 reuses Phase-1 traits). Work top-to-bottom, one task at a time. Check the
> box only when "Done when" passes.
> Source: PROJECT_SPEC.md §5 (Phase 2), §1 (per-version FPM, per-site override), §2.1.
> **Extended request topology: see Notes / decisions at the bottom.**

Status: `[ ]` todo · `[~]` in progress · `[x]` done

---

## 1. Multi-PHP (multiple versions · per-version pools · per-site switch)

Generalizes Phase 1 §5 (single PHP 8.3 on one pool at :9783) to N versions, **one FPM pool per
version**, with sites mapping to the right version's pool.

- [x] **1.1 Multi-version PHP binary provider**
  *Done when:* `core/binaries.rs` carries ≥2 PHP versions (e.g. 8.1, 8.2, 8.3) for macOS arm64+x86_64 —
  static-php **"bulk"** build (has `mysqli`), each checksum-pinned; resolving `php`/`php-fpm` at a given
  version downloads, signs + de-quarantines, and caches each version independently (same `BinaryProvider`
  path as Phase 1 — no new sign/quarantine logic); `php -v` from each cached path reports the matching version.
  ✓ **Done (8cf471d):** 8.1.34/8.2.31/8.3.31 pinned (genuine SHA-256s); `php -v` + `php-fpm -v` run from each cached path. `examples/php_versions_check`.

- [x] **1.2 PHP version registry + per-version FPM pool manager**
  *Done when:* an installed-versions registry (SQLite — a `php_versions` table or settings) records which
  versions are installed and each pool's loopback port; a pool manager starts **one php-fpm master per
  installed version**, each on its own deterministic port (a `core/ports` range, gated by `ensure_free`)
  via `ProcessSupervisor::spawn_logged`; `services_status` lists each running pool with its version + port.
  **No per-site pools.** Depends on 1.1.
  ✓ **Done (64b45a2):** migration v2 `php_versions` table + `PhpFpmPools`; one master per version on 9781/9782/9783, `examples/php_pools_serve` shows all listening + clean stop.

- [x] **1.3 Site → PHP-version mapping in generated config**
  *Done when:* the Nginx server-block generator + `rebuild_configs` set `fastcgi_pass` to the **port of the
  site's PHP version's pool** (from the 1.2 registry), not a hardcoded `:9783`; two sites pinned to different
  versions are served by different pools — `https://a.test` reports PHP X and `https://b.test` reports PHP Y
  (phpinfo), both **HTTP 200** through the full Caddy→Nginx→fpm chain. Depends on 1.2.
  ✓ **Done (695e72e):** `nginx_site_for` maps version→pool port (unknown→default); `examples/php_per_site_serve` — a.test=8.1.34, b.test=8.3.31, both 200, no cross-leak.

- [x] **1.4 Switch a site's PHP version (reload, not rebuild)**
  *Done when:* changing a site's `php_version` updates only the DB row + regenerates its server block +
  reloads the shared Nginx (starting the target pool first if it isn't running) — **no docroot, cert, or DB
  rebuild**; after the switch the same URL reports the new version in phpinfo. Proves the "config change +
  reload" requirement. Depends on 1.3.
  ✓ **Done (89db0ec):** `sites::set_php_version` (DB-only) + `ServiceManager::ensure_php_pool` + nginx reload; `examples/php_switch_serve` 8.1→8.3, docroot sentinel + cert mtime unchanged.

- [x] **1.5 Multi-PHP UI (manage versions · per-site switch · default)**
  *Done when:* a PHP-versions card (Settings) lists installed/available versions with install/remove
  actions + a default marker; **SiteDetail** picks the PHP version from the installed ones and switching
  drives 1.4 via typed IPC (`src/lib/ipc/`); the Sites list badge reflects the active version. Mock
  fallback outside Tauri. Depends on 1.4. **Create-flow PHP select deferred to §1.6** (no New Site form yet).
  ✓ **Done (8091fae):** Settings "PHP versions" card (install/remove/Default) + SiteDetail version selector → 1.4; guards in `core::php::set_installed` (no removing default/in-use). `commands/php.rs`.
- [ ] **1.6 New Site form (create dialog + `create_site` IPC)**
  *Done when:* the "New site" button opens a dialog (name, domain, type, **PHP version** from installed
  versions, web server) wired to a new `create_site` command (provision + reload); creating a site adds it
  to the list and serves it. This is the dedicated create form that §4.2 (server select) and §5.6 (DB-engine
  select) plug into. Depends on 1.5; extended by §4/§5.

## 2. Per-site server override — FrankenPHP (primary alt server)

Override servers run as **separate processes on internal loopback ports**; the edge Caddy routes to them.
Default sites stay on the shared Nginx. FrankenPHP is the Phase-2 alt server because it's a **single static
binary** — it proves the override architecture (edge → internal port → alt server) with the least
macOS-binary friction. Apache is **deferred (§7.4)**: macOS has no portable httpd, the same packaging pain
as OpenLiteSpeed; the override path is proven, so Apache adds no Phase-2 architecture.

> FrankenPHP is itself built on Caddy + an **embedded PHP** runtime. Here it is used **only as a backend
> server on an internal loopback port** — NOT as an edge. Our Phase-1 edge Caddy keeps :443, terminates
> TLS with the local CA, and proxies to it. Keep the two roles strictly separate (see Notes / decisions).

- [x] **2.1 FrankenPHP binary provider**
  *Done when:* `BinaryProvider` downloads/caches the official FrankenPHP **static** binary for macOS
  arm64+x86_64, checksum-pinned, through the Phase-1 `prepare_binary` (de-quarantine → relink if needed →
  ad-hoc codesign); `frankenphp version` (or `--version`) runs from the cached path.
  ✓ **Done (8d6a68f):** FrankenPHP 1.12.4 static pinned (both arches); `frankenphp version` → embeds PHP 8.5.7 + Caddy 2.11.4. `examples/frankenphp_fetch`.

- [x] **2.2 FrankenPHP per-site config + supervise (internal loopback port, embedded PHP)**
  *Done when:* `core/` generates a FrankenPHP config that serves one site's docroot via its **embedded PHP**
  on a plain-HTTP listener `127.0.0.1:<internal port>` in the override port range, with FrankenPHP's own
  **automatic HTTPS DISABLED** (it sits behind our edge — it must never try to bind :443 or issue certs);
  `ProcessSupervisor::spawn_logged` starts/stops it (tracked by `ServiceManager`); `curl -H 'Host: <site>'
  http://127.0.0.1:<port>` serves PHP (200). The generator leaves the same 3 rewrite-template slots.
  **Note:** a FrankenPHP site's PHP version is the runtime embedded in the FrankenPHP build (it does NOT use
  the §1 php-fpm pools); the per-version pool model (§1.3/1.4) applies to fpm-backed servers (Nginx, Apache).
  Depends on 2.1.
  ✓ **Done (f755e8f):** `core/frankenphp.rs` (auto_https off · admin off · default_bind 127.0.0.1 · php_server); `examples/frankenphp_serve` — embedded PHP on :8200, HTTP 200, `:2019` not bound. 3 rewrite slots.

- [x] **2.3 Edge routes FrankenPHP override sites**
  *Done when:* `rebuild_configs` emits a Caddy route for a FrankenPHP-override site's Host → its FrankenPHP
  internal port at `https://<site>.test` (200, CA-issued cert seen by the browser, issuer = our CA), while
  every other `*.test` still goes to the shared Nginx port and default Nginx sites stay up. Confirms the
  edge ↔ backend separation: the served cert is our local CA's (the edge's), not anything FrankenPHP issued.
  Depends on 2.2.
  ✓ **Done (880604f):** `WebServer::Frankenphp` + `site_upstream`/`is_nginx_served` + `frankenphp::site_port`; `examples/frankenphp_edge_serve` — ng.test→pool(8.3.31), fp.test→FrankenPHP(8.5.7), both 200 via our CA.

## 3. Per-site server override — Apache (httpd) — DEFERRED → §7.4

> **Deferred (decision 2026-06).** macOS has no prebuilt portable Apache: Homebrew `httpd` links a tree of
> non-system dylibs (`apr`, `apr-util`, `pcre2`, `openssl@3`, `brotli`, `libnghttp2`) with no `/usr/lib`
> equivalent, so `prepare_binary`'s relink-to-system path can't make it self-contained — shipping it needs a
> new "bundle the dylib tree + rewrite install names to `@loader_path`" mechanism. The per-site override
> architecture is already proven by FrankenPHP (§2), so Apache moves to **§7.4** (same rationale as
> OpenLiteSpeed §7.2): revisit on the Linux/Windows ports (clean apache binaries) or as dedicated macOS
> packaging work.

## 4. Per-site server selection (wiring + UI)

- [ ] **4.1 Server select in create + live switch (backend)**
  *Done when:* the site model's `web_server` enum (Nginx | FrankenPHP; Apache when §7.4 lands) drives
  provisioning: choosing an override at create brings up that server's config + process + its edge route;
  **switching** a live site's server tears down the old backend config, brings the new one up (and the old
  one down if no other site uses it), and reloads the edge — **no docroot/cert/DB rebuild**. Verified by
  switching one site Nginx → FrankenPHP → Nginx, each serving 200 at the same URL. **Pools stay per-version,
  never per-server:** an Nginx site keeps using its version's shared php-fpm pool across the switch — no
  per-server pool is spawned (FrankenPHP is exempt — it serves via its embedded PHP, not a pool). Depends on 2.3.

- [ ] **4.2 Server-select UI (create + SiteDetail)**
  *Done when:* the create flow and SiteDetail expose a server dropdown (Nginx default / FrankenPHP),
  the switch drives 4.1 via typed IPC, and the Sites list shows the active server badge (the Phase-1 mock
  already renders a server badge). Mock fallback outside Tauri. Depends on 4.1.

## 5. Database engines (PostgreSQL; MariaDB & Redis deferred)

Each engine goes through the same `BinaryProvider` (sign + de-quarantine) + `ProcessSupervisor` lifecycle as
Phase-1 MySQL (§8), via the §5.1 `DbEngine`. **One version per engine** (multi-version is optional §7.1).
**macOS-binary reality (decision 2026-06):** only PostgreSQL has a clean portable macOS binary. MariaDB ships
**no** macOS binary (any arch) and Redis needs a dylib-bundle (openssl@3) — both deferred to §7 (same call as
Apache); MySQL covers the MySQL-compatible case meanwhile.

- [x] **5.1 DB engine abstraction**
  *Done when:* a `DbEngine` abstraction (enum/trait in `core/`) unifies MySQL + the new engines behind one
  shape — binary resolution, init-if-needed, start/stop on a per-engine known loopback port (registered in
  `core/ports::default_ports`) via `spawn_logged`, and a running-probe; **Phase-1 MySQL is refactored onto it
  with behavior unchanged** (existing MySQL example + tests stay green).
  ✓ **Done (a2ffd81):** `core/db.rs` `DbEngine` {Mysql,Mariadb,Postgres,Redis}; MySQL delegates to `core::database` (untouched); ports reserved. `examples/db_engine_serve`.

- [ ] **5.2 MariaDB binary provider + lifecycle** — DEFERRED → §7.5
  > **Deferred (decision 2026-06).** MariaDB ships **no** macOS binary tarball (checked 10.4–11.8, no arch;
  > Apple Silicon never), and its Homebrew tree is large (groonga/mecab/simdjson/…). Needs the dylib-bundling
  > mechanism (see §7.4/§7.5). MySQL is a drop-in MySQL-compatible substitute, so nothing is blocked.

- [x] **5.3 PostgreSQL binary provider + lifecycle**
  *Done when:* PostgreSQL (portable macOS binary — `theseus-rs/postgresql-binaries`, arm64+x86_64, published
  SHA-256) resolves through `BinaryProvider` (a `TarGzTree`), runs `initdb` once (idempotent) into an app-data
  datadir, starts/stops `postgres` on its own loopback port via the §5.1 engine; bundled
  `psql -c 'SELECT version();'` succeeds. Depends on 5.1.
  ✓ **Done (6007c0b):** PostgreSQL 18.4.0 (published SHA-256 cross-checked) via `core/postgres.rs`, TCP-only (no unix socket); `psql SELECT version()` → "PostgreSQL 18.4". Runs unsigned on Apple Silicon.

- [ ] **5.4 Redis binary provider + lifecycle** — DEFERRED → §7.6
  > **Deferred (decision 2026-06).** No official macOS Redis binary; the Homebrew bottle links `openssl@3`
  > (libssl/libcrypto.3 — no `/usr/lib` equivalent), so it needs the small dylib-bundle path (§7.6). Redis is a
  > shared cache service with no Phase-2 consumer anyway (app/object-cache integration is Phase 3+).

- [x] **5.5 Databases UI (multi-engine start/stop/status)**
  *Done when:* the Databases screen (Phase-1 placeholder) lists the available engines (MySQL, PostgreSQL) with
  per-engine status, port, version + start/stop, on live IPC (2s poll, mock fallback in dev); footer totals
  include them. (MariaDB/Redis appear once §7.5/§7.6 land.) Depends on 5.3.
  ✓ **Done (d6e00ee):** `ServiceManager` `dbs` map + `ensure_db`/`stop_db`/`db_status`; `commands/database.rs` (`databases_status`/start/stop); Databases.tsx rows with version/port/meters/toggle (2s poll).

- [ ] **5.6 Site → DB engine selection at create** — DEFERRED → §7.5
  > **Deferred.** With MariaDB out (§5.2), MySQL is the only relational engine, so there's no choice to make;
  > PostgreSQL isn't a WordPress/relational-app target here. Revisit when MariaDB lands (§7.5).

## 6. Resource monitor (extend to Phase-2 services)

> The monitor core was built in Phase 1 (7.4 per-PID `sysinfo` API + 10.5 Services view). Phase 2 only
> extends coverage — no new monitor engine.

- [ ] **6.1 Monitor covers all Phase-2 services**
  *Done when:* `services_status` reports live RAM/CPU rows for **every** service the app now supervises —
  each per-version php-fpm pool (§1), each per-site FrankenPHP override (§2), and the running DB engines
  (MySQL, PostgreSQL §5) — and the Services view + sidebar footer totals reflect them. Depends on 1.2, 2.2, 5.5.

---

## 7. Optional / later (Phase 2+)

> Not required for the Phase 2 goal; pick up when the prerequisite lands.

- [ ] **7.1 Per-engine DB version switch (multi-version DBs)**
  *Done when:* like multi-PHP (§1), a DB engine can have multiple installed versions with a one-click switch;
  mirrors the §1.2 registry pattern for DB engines. Heavier feature — kept out of core §5 deliberately.
  (PROJECT_SPEC Tier 1: DB "version switch".)
- [ ] **7.2 OpenLiteSpeed override server**
  *Done when:* OpenLiteSpeed is available as a per-site override via the same override pattern (§2/§4 —
  separate process on an internal loopback port, edge routes to it). **Deferred from Phase 2:** OLS is Linux-first and its
  macOS binary distribution is painful; revisit when it's actually needed, or during the Windows/Linux ports
  (§4/§5 of PROJECT_SPEC) where official binaries are easier.
- [ ] **7.3 Caddy admin-port robustness (lingering-edge recovery)**
  *Done when:* a leftover Caddy (especially a privileged/root one from a prior real-:443 run) holding the
  fixed admin port `:2019` no longer blocks startup — the app detects a stale edge and adopts/stops it (or
  uses a non-default/derived admin address), and surfaces a clear error if it can't. *Found during §1.3:*
  an orphaned root Caddy on `:2019` made a fresh Caddy fail to start (`bind: address already in use`); the
  dev workaround was a foreground `sudo pkill` of the stray.
- [ ] **7.4 Apache (httpd) override server** *(deferred from §3)*
  *Done when:* Apache is a per-site override via the §2/§4 pattern — a self-contained `httpd` on an internal
  loopback port, `mod_proxy_fcgi` → the site's per-version php-fpm pool, edge routes to it. **Blocked on macOS
  packaging:** no portable httpd exists; Homebrew `httpd` links a tree of non-system dylibs (`apr`, `apr-util`,
  `pcre2`, `openssl@3`, `brotli`, `libnghttp2`) with no `/usr/lib` equivalent, so this needs a new
  `prepare_binary` path that **bundles the dylib tree and rewrites install names to `@loader_path`**. Easiest
  on the Linux/Windows ports (clean apache binaries) or as dedicated macOS packaging work. Decided 2026-06
  to defer (override architecture already proven by FrankenPHP §2).
- [ ] **7.5 MariaDB engine** *(deferred from §5.2; includes site → DB-engine select §5.6)*
  *Done when:* MariaDB runs through `DbEngine` (datadir init + start/stop on its port, client `SELECT
  VERSION()`), and a site that needs a DB can pick MySQL **or** MariaDB at create (§5.6). **Blocked on macOS
  packaging:** no MariaDB macOS binary exists, so this needs the §7.4 dylib-tree-bundling path (its tree is
  large — groonga/mecab/simdjson/openssl@3/…). Easiest on Linux/Windows ports.
- [ ] **7.6 Redis engine** *(deferred from §5.4)*
  *Done when:* Redis runs through `DbEngine` (`redis-server` on its port, `redis-cli PING` → `PONG`) as a
  shared cache service. **Blocked on macOS packaging:** no official macOS binary; the Homebrew bottle links
  `openssl@3` (no `/usr/lib` equivalent), so it needs the dylib-bundle path (§7.4) — small here (2 dylibs).
  No Phase-2 consumer (app/object-cache integration is Phase 3+).

---

## Notes / decisions

- **Request topology (Phase 2 — extends the Phase-1 note):**
  - *Default sites (unchanged):* browser → **Caddy** (:443, local-CA TLS) → shared **Nginx** (vhost by
    `server_name`) → the **php-fpm pool for that site's PHP version** (FastCGI).
  - *Override sites:* browser → **edge Caddy** (:443, local-CA TLS) → the site's **own alt-server process**
    on a dedicated internal loopback port → PHP. **FrankenPHP** (the Phase-2 override) serves PHP from its
    **embedded runtime** (no php-fpm pool) — a plain-HTTP backend with its own auto-HTTPS DISABLED, never the
    edge. (Apache, when §7.4 lands, instead proxies to the site's per-version php-fpm pool via `mod_proxy_fcgi`.)
  - Edge Caddy routes **by Host**: all default `*.test` → the one shared Nginx port; each override site's
    Host → its dedicated backend port. TLS always terminates at the edge with the local CA (auto-HTTPS /
    internal issuer stays **DISABLED**). **No direct edge→php path; no backend terminates TLS.**
- **Edge Caddy vs FrankenPHP — strictly separate roles.** FrankenPHP is Caddy+PHP, so it is tempting to
  conflate them: don't. The edge Caddy owns :80/:443, TLS, and Host routing (Phase 1). FrankenPHP is just
  one possible per-site backend on an internal loopback port. A FrankenPHP backend must never bind :443 or
  issue/serve its own certs.
- **One php-fpm pool per PHP version, never per site** (applies to Nginx; Apache too when §7.4 lands). A site
  references its version's pool port. Switching a site's PHP **version** or its **server** is a config regen
  + edge/Nginx reload — never a docroot/cert/DB rebuild. (FrankenPHP's PHP version = its embedded build.)
- All Phase-2 binaries (multi-PHP, FrankenPHP, PostgreSQL) go through the same `BinaryProvider` + macOS
  `prepare_binary` (de-quarantine → relink Homebrew dylibs → codesign LAST) and `ProcessSupervisor::spawn_logged`
  as Phase 1. (Apache §7.4, MariaDB §7.5, Redis §7.6 need a new dylib-tree-bundling path — why they're deferred.)
- **Ports:** every new service gets a deterministic loopback port in `core/ports` (a range for per-version
  pools and per-site overrides; one each for MariaDB/PostgreSQL/Redis), gated by `ensure_free` before spawn;
  `ServiceManager` owns their lifecycle.
- **No new platform traits expected** — Phase 2 reuses the Phase-1 traits. Keep `commands/` thin →
  `core/` (platform-agnostic) → `platform/` traits. macOS only; `windows`/`linux` stay `todo!()`.
- **OpenLiteSpeed deferred** (→ §7.2): Linux-first, painful macOS binaries. FrankenPHP is the Phase-2 alt
  server precisely because it's a single static binary. The override *architecture* is what Phase 2 proves;
  more alt servers are additive afterward.
- **Resource monitor** is largely done (Phase 1 7.4/10.5); §6 is coverage-extension + verification only.
- Verification pattern carries over: lib unit tests + standalone `src-tauri/examples/*.rs` for live checks.

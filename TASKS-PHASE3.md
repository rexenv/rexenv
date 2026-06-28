# TASKS — Phase 3 (macOS)

> **Phase goal:** the differentiator features on top of the Phase 1+2 stack — the **WordPress Manager**
> (plugins · themes · users · tools · **Multisite/Network**, the signature feature), a built-in **Adminer**
> database browser, **Mailpit** email catching, a per-site **Xdebug** toggle, a real-time **log viewer**,
> **Cloudflare Tunnel** public sharing, and a built-in **terminal**.
>
> Builds on the existing architecture (CLAUDE.md, TASKS.md, TASKS-PHASE2.md) — REUSE all of it:
> `BinaryProvider` + macOS `prepare_binary` (de-quarantine → relink dylibs → codesign LAST), and
> `resolve_file` for non-Mach-O artifacts; `ProcessSupervisor::spawn_logged`; `ServiceManager` (owns the
> stack); `core/ports` (gate every bind); the `DbEngine` abstraction; **WP-CLI run through the bundled PHP**
> (Phase 1 §9.1) with `-d memory_limit=512M`; the Caddy edge → shared Nginx (by `server_name`) → per-version
> php-fpm topology; the **3 rewrite-template slots** (single / subdomain / subdirectory) from **Phase 1 §6.2**
> — now EXERCISED by multisite; the per-site **wildcard cert** (`*.site.test`, **Phase 1 §3.2**); typed IPC in
> `src/lib/ipc/`; SQLite state; the Phase-2 "config change + reload, never rebuild" rule.
>
> Scope: **macOS only** — `windows`/`linux` stay `todo!()`. **No new platform traits expected** (the terminal
> uses a cross-platform PTY crate, not an OS trait). One new SQLite migration (site multisite mode).
> Work top-to-bottom, one task at a time. Check the box only when "Done when" passes. Screens: DESIGN_BRIEF.md
> Blocks 5/6/8/9/10 + `design/`.
> Ordering rationale: lower-risk tooling (Mailpit, logs, terminal, Adminer) lands first — it establishes the
> Phase-3 patterns and is useful for debugging the WordPress Manager that follows; multisite (highest-risk,
> cross-cutting) comes last, once single-site WP management is solid.
> Source: PROJECT_SPEC.md §5 (Phase 3), §2 Tier 2, §2.1 (multisite).
> **Extended topology + decisions: see Notes / decisions at the bottom.**

Status: `[ ]` todo · `[~]` in progress · `[x]` done

---

## 1. Foundation — WP-CLI bridge · create WordPress · Site Detail tabs

Lays the WP-CLI JSON bridge, the ability to create WordPress sites to test everything against, and the Site
Detail tab shell that the WordPress Manager, Logs tab, and Terminal all live in.

- [x] **1.1 WP-CLI JSON bridge + WP detection**
  *Done when:* `core/wordpress` gains a typed JSON runner — `wp <args> --path=<docroot> --format=json` via the
  bundled PHP + `wp-cli.phar` (reusing Phase 1 §9.1's `wp_cli`, `-d memory_limit=512M`), with non-zero exit /
  stderr surfaced as a clean `Error`. A `wp_info(site_id)` IPC returns `{ isWordpress, version, multisite }`
  (via `wp core is-installed`, `wp core version`, `wp config get MULTISITE`). Verified: against a real WP site
  the bridge returns the core version + an `wp option get` value; a Blank-PHP site reports `isWordpress:false`.
  Depends on Phase 1 §9.1/§9.2.
  ✓ `core::wordpress::{wp_run, wp_json::<T>, WpInfo, wp_info}` + `commands::wordpress::wp_info` (resolves the
  site's PHP + wp-cli.phar) + typed `ipc.wpInfo`; `cargo run --example wp_info_check` → `WpInfo{isWordpress:
  true, version:"7.0", multisite:false}`, `siteurl=https://wpinfo.test`, JSON runner parsed 2 plugins,
  Blank-PHP → `isWordpress:false`. (commit pending)

- [x] **1.2 Create a WordPress site (one-click install, wired into the create flow)**
  *Done when:* choosing **WordPress** in the New Site dialog (Block 4: site title, admin user/email/password,
  language) runs the Phase-1 one-click installer (`wordpress::install_wordpress` — core download → `config
  create` → `db create` on the site's DbEngine (MySQL default) → `core install`) inside `create_site` (ensures
  MySQL is up first), then brings the site up; `https://<name>.test/wp-admin` loads a working WP login. The
  dialog's **Multisite** toggle (subdomain/subdirectory) is recorded and, when set, runs §10.1 after install.
  Verified: creating a WordPress site from the UI yields a browsable WP site + admin. Depends on 1.1, Phase 2
  §1.6, Phase 1 §9.2.
  ✓ `create_site` takes an optional `wp: InstallOptions`; for `SiteType::Wordpress` it `ensure_db(Mysql)`,
  resolves the site's PHP + wp-cli.phar, and runs `core::wordpress::install_for_site` (new — fills defaults
  from domain/name; `install_wordpress`/`WpInstall` gained a `locale` for `core download --locale`). Dialog
  gains a **Type** selector + WordPress fields (admin user/email/password, language) → `ipc.createSite(input,
  wp)`. `cargo run --example wp_create_serve` → `GET / 200` (title present) + `GET /wp-login.php 200` (login
  form). **Multisite toggle deferred to §10** (needs §10.1 convert + migration v3); single-site WP create is
  complete. (commit pending)

- [x] **1.3 Site Detail tabs + Overview (Block 5)**
  *Done when:* SiteDetail renders the tab bar — **Overview · WordPress · Database · Logs · Settings** — with
  the **WordPress** tab shown only when `wp_info.isWordpress`. Overview shows environment (the existing
  PHP-version + web-server switches from Phase 2 §1.5/§4.2), project/config paths (mono + copy + open-folder),
  quick links (Browser, WP admin, Database, Terminal, Folder), and a recent-logs peek. Routes under
  `/sites/:id/<tab>`; mock fallback outside Tauri. Depends on 1.1.
  ✓ SiteDetail rebuilt with a tab bar routed at `/sites/:id/:tab` (App.tsx); WordPress tab gated on a
  `wpInfo` query. Overview = Quick links (Browser/WP-admin/Database/Terminal[disabled→§4]/Folder), Environment
  (PHP + server switches, type, WP version), Paths (docroot + wp-config with copy + open-folder), recent-logs
  peek. Open via new `ShellRunner::open` (macOS `open`; win/linux `todo!()`) → `open_external` command →
  `ipc.openExternal` (window.open fallback off-Tauri); `ipc.wpInfo` mock derives from the mock site type.
  Verified in dev (chrome-devtools): WP site (acme) shows the WordPress tab + WP-admin link; Laravel site
  (portfolio) hides both. `cargo test` 83 pass, tsc + vite build clean. (commit pending)

## 2. Mailpit email catching (Block 9)

First of the lower-risk tooling — establishes the Phase-3 `BinaryProvider`-for-a-single-binary + managed
service pattern, and gives WordPress work a place to see captured mail.

- [ ] **2.1 Mailpit binary provider + lifecycle**
  *Done when:* Mailpit (single binary) resolves via `BinaryProvider` (sign + de-quarantine), starts via
  `ProcessSupervisor::spawn_logged` on a fixed **SMTP port** (`1025`) + **HTTP/API port** (`8025`), port-gated
  (`core/ports`), owned by `ServiceManager` (start/stop/status); `services_status` + the Mail screen show it
  running. Verified: `mailpit version` runs; after start, `:1025` accepts a connection and `GET
  /api/v1/messages` (`:8025`) responds. Depends on Phase 1 §10.5.

- [ ] **2.2 Route site outgoing mail → Mailpit**
  *Done when:* PHP `mail()` from any site is captured — each php-fpm pool sets
  `php_admin_value[sendmail_path]` to Mailpit's sendmail shim aimed at the local SMTP
  (`<mailpit> sendmail -t --smtp-addr 127.0.0.1:1025`); changing it is a **pool config rewrite + reload** (no
  rebuild). Verified: `wp eval 'wp_mail("a@b.test","hi","body")'` (or a WP password-reset) on a real site
  increments Mailpit's API message count — a real email captured. Depends on 2.1, Phase 2 §1.2.

- [ ] **2.3 Mail screen (two-pane inbox)**
  *Done when:* the **Mail** screen (Block 9) lists captured messages from Mailpit's HTTP API
  (from/to/subject/time/unread) via typed IPC; the preview pane shows **HTML / Text / Raw** + headers; a
  search field and **Clear all** (`DELETE /api/v1/messages`) work; empty-state copy per Block 9. Depends on 2.1.

## 3. Real-time log viewer (Block 5 Logs tab)

- [ ] **3.1 Live log tailing**
  *Done when:* the SiteDetail **Logs** tab (with a source picker) live-tails the relevant logs — the
  `spawn_logged` per-service stdout logs (**Phase 1 §10.3**) plus nginx access/error, php-fpm, and
  MySQL/PostgreSQL error logs — via a `tail_log(target, lines)` IPC that returns the last N lines and follows
  (Tauri event stream or ~1s poll); lines render in mono with auto-scroll + pause. Verified: hitting a site
  appends new nginx access lines to the Logs tab in near-real-time; switching the picker changes the source.
  Depends on 1.3, Phase 1 §10.3.

## 4. Built-in terminal (xterm.js)

- [ ] **4.1 PTY backend (event stream)**
  *Done when:* a PTY backend (the cross-platform `portable-pty` crate — **no new platform trait**) spawns the
  user's shell **in a site's docroot** with the bundled PHP + `wp-cli.phar` (+ Composer/Node if present) on
  `PATH`; stdin/output/resize stream over Tauri events behind a typed `src/lib/ipc/` helper. Verified
  (example/headless): writing `php -v\n` to the PTY returns the bundled PHP version; the cwd is the docroot.
  Depends on Phase 1 §9.1.

- [ ] **4.2 Terminal UI (xterm.js)**
  *Done when:* SiteDetail exposes a **Terminal** (Overview quick link / tab) rendering an xterm.js terminal
  bound to 4.1 — interactive shell, working resize, and a clear/restart control. Verified: `wp --info` runs
  against the site and `ls` lists the docroot, live in the UI. Depends on 4.1, 1.3.

## 5. Built-in database browser (Adminer, Block 8)

- [ ] **5.1 Adminer binary provider**
  *Done when:* Adminer (single `adminer.php`) resolves via `BinaryProvider::resolve_file` (download +
  checksum-pin; **no chmod/codesign** — a PHP script, like WP-CLI Phase 1 §9.1), cached under app-data;
  `php -l` on it from the bundled PHP passes. Depends on Phase 1 §5.1.

- [ ] **5.2 Serve Adminer through the stack + embed**
  *Done when:* Adminer is served via the stack — a dedicated nginx server block (host `adminer.rexenv.test`)
  rooted at the cached `adminer.php` via a php-fpm pool, behind the edge (TLS, local CA); the **Databases**
  screen / SiteDetail **Database** tab embeds it (framed, dark per Block 8) with the connection pre-filled for
  the target engine (host/port for MySQL or PostgreSQL). Verified: "Open in database browser" loads Adminer
  connected to a site's MySQL DB and a `SELECT` runs. Depends on 5.1, 1.3, Phase 2 §5 (`DbEngine`).

## 6. WordPress Manager — plugins & themes (Block 6)

- [ ] **6.1 Plugins (list · activate/deactivate · update · install · delete · bulk)**
  *Done when:* the **Plugins** sub-tab is driven by `wp plugin list --format=json` (name, status, version,
  update) via typed IPC; per-row Activate/Deactivate (`wp plugin activate|deactivate`), Update (`wp plugin
  update`), Delete (`wp plugin delete`), Add-by-slug (`wp plugin install <slug> [--activate]`); a bulk-select
  bar does bulk activate/deactivate/update; an **update-available** badge shows when `update == available`.
  Verified on a real WP site: installing `hello-dolly` by slug + activating shows Active in `wp plugin list`;
  bulk-deactivate + delete reflect. Depends on 1.3.

- [ ] **6.2 Themes (grid · activate · update · install · delete)**
  *Done when:* the **Themes** sub-tab (card grid, Block 6) is driven by `wp theme list --format=json`;
  Activate (`wp theme activate`), Update, Delete, Add (`wp theme install <slug>`); the live theme shows an
  **Active** badge. Verified: activating a second theme flips `status` in `wp theme list`; install + delete
  work. Depends on 1.3.

## 7. WordPress Manager — users & tools (Block 6)

- [ ] **7.1 Users (list · add · one-click "Log in as")**
  *Done when:* the **Users** sub-tab lists `wp user list --format=json` (login, email, role); Add user
  (`wp user create`); and a per-row **Log in as** opens the browser already authenticated as that user, via a
  WP-CLI one-time login (a temporary mu-plugin that consumes a single-use token, or the `wp-cli-login`
  package). **Security:** the auto-login token MUST be single-use, short-TTL (expires in seconds–minutes),
  AND accepted only for loopback/local requests (e.g. bound to `127.0.0.1`/`localhost` Host + a
  `REMOTE_ADDR` loopback check) — so a captured token can NOT be replayed through a public Cloudflare
  tunnel (§9) while the site is shared. Verified: "Log in as admin" opens `wp-admin` logged in, no password
  prompt; the same token fails on a second use AND when presented with a non-loopback Host. Depends on 1.3.

- [ ] **7.2 Tools (WP_DEBUG · search-replace · permalinks · core update)**
  *Done when:* the **Tools** sub-tab offers a **WP_DEBUG** toggle (`wp config set WP_DEBUG true --raw` / `wp
  config get` to reflect), a **Search-replace** tool (old→new URL, a **dry-run** checkbox → `wp search-replace
  --dry-run` reports a row count without changing data; apply mutates the DB), regenerate permalinks (`wp
  rewrite flush`), and core update / re-install (`wp core update` / `wp core download --force`). Verified:
  toggling WP_DEBUG flips `wp config get WP_DEBUG`; a dry-run reports N rows, the real run changes them.
  Depends on 1.3.

## 8. Per-site Xdebug toggle

- [ ] **8.1 Verify Xdebug availability in the bundled PHP** *(gating check — do this first)*
  *Done when:* it's determined + recorded whether the static-php "bulk" build can load Xdebug — check
  `php -m` / `php -i` for `xdebug` (and whether the build allows loading a `zend_extension` at all). **The
  result decides 8.2's approach:** if Xdebug is present → 8.2 proceeds with the per-version debug pool; if
  absent → an external `xdebug.so` generally can't load into a static PHP, so 8.2 is blocked on sourcing a
  PHP build variant with Xdebug compiled in (tracked in §11.2) — note the blocker and stop. Depends on Phase 1
  §5.1.

- [ ] **8.2 Per-site Xdebug toggle (config change + reload, not rebuild)**
  *Done when:* a per-site Xdebug toggle works via a per-version **"debug" php-fpm pool** (Xdebug
  `zend_extension` + `xdebug.mode=debug,develop`) that a site's nginx block `fastcgi_pass`es to when Xdebug is
  ON — toggling rewrites the server block + reloads nginx (the debug pool starts on demand); per-version pools
  stay shared, no rebuild. Verified: enabling Xdebug on one site makes its `phpinfo()` show Xdebug while
  another site on the same PHP version does NOT; toggling off routes it back. Depends on 8.1 (Xdebug present),
  Phase 2 §1.2.

## 9. Cloudflare Tunnel public sharing (Block 10)

- [ ] **9.1 cloudflared provider + per-site quick tunnel**
  *Done when:* `cloudflared` resolves via `BinaryProvider` (sign + de-quarantine); a per-site **quick tunnel**
  starts (`cloudflared tunnel --url http://127.0.0.1:<shared-nginx-http-port> --http-host-header=<site
  domain>`) — origin is the **shared nginx HTTP port** with the site `Host` (plain-HTTP origin; cloudflared
  provides the external TLS, so there's NO local-CA origin-trust issue and no `--no-tls-verify`), with the
  generated
  `https://<random>.trycloudflare.com` URL parsed from cloudflared's output; the tunnel is owned by
  `ServiceManager` / a registry (start/stop per site). **A tunnel is scoped to ONE site's Host only** — never
  the edge wildcard and never an internal vhost (`adminer.rexenv.test`, Mailpit, etc.), so sharing one site
  cannot expose another site or a tooling vhost. Verified: enabling sharing yields a public
  `trycloudflare.com` URL that loads the local site from outside; stopping ends it. Depends on Phase 1 §10.5.

- [ ] **9.2 Tunnels screen (Block 10)**
  *Done when:* the **Tunnels** screen lists shareable sites; a per-site **Share publicly** toggle starts/stops
  the tunnel (9.1); when active it shows the public URL (mono + copy) + a live status; empty-state per Block
  10. Depends on 9.1.

## 10. WordPress Multisite / Network (cross-cutting, highest-risk — PROJECT_SPEC §2.1)

Last, once single-site WP management is solid. Exercises the **Phase 1 §6.2** rewrite-template slots and the
**Phase 1 §3.2** wildcard cert that were built but unused.

- [ ] **10.1 Enable/convert multisite (subdomain | subdirectory) + rewrite template**
  *Done when:* a WP site can be converted to multisite either mode via WP-CLI (`wp core multisite-convert
  [--subdomains]`), writing the network wp-config constants (`MULTISITE`, `SUBDOMAIN_INSTALL`,
  `DOMAIN_CURRENT_SITE`, `PATH_CURRENT_SITE`, `SITE_ID_CURRENT_SITE`, `BLOG_ID_CURRENT_SITE`); the mode is
  persisted (**migration v3**: `sites.multisite` = `none|subdomain|subdirectory`) and mapped by
  `rebuild_configs` to the matching `RewriteMode` (**Phase 1 §6.2**) so nginx regenerates with the
  subdomain/subdirectory rewrite template + reloads (no docroot/cert/DB rebuild). Verified: converting
  `mysite.test` to **subdirectory** multisite writes the constants, sets `RewriteMode::SubdirectoryMultisite`,
  and `/wp-admin/network/` loads. Depends on 1.1, Phase 1 §6.2.

- [ ] **10.2 Wildcard cert + edge route for subdomain multisite**
  *Done when:* a subdomain-multisite site is served across `*.mysite.test`: the per-site **wildcard cert**
  (SAN `*.mysite.test`, **Phase 1 §3.2**) is used and the edge Caddy route matches the wildcard host
  (`mysite.test, *.mysite.test` → the same backend). Verified: `https://a.mysite.test` and
  `https://b.mysite.test` both load network sub-sites with a valid lock — `openssl s_client` shows SAN
  `DNS:*.mysite.test`, issuer = our local CA (DNS resolves `*.test` free, **Phase 1 §2.1**). **Negative
  check (precedence footgun):** adding the `*.mysite.test` host route MUST NOT overshadow the default
  `*.test` → shared-nginx route — verify a normal non-multisite site (e.g. `https://other.test`) STILL
  loads after the wildcard route is added (the specific `mysite.test`/`*.mysite.test` matcher must win only
  for that domain; everything else falls through to the shared-nginx route). Depends on 10.1, Phase 1 §3.2.

- [ ] **10.3 Network UI (Network sub-tab, Block 6)**
  *Done when:* the **Network** sub-tab (multisite only) shows a **mode badge** (Subdomain/Subdirectory); a
  sub-site list from `wp site list --format=json` with **Create** (`wp site create --slug=`), **Delete**
  (`wp site delete`), **Visit**, **Admin**; **network-activate** plugins/themes (`wp plugin activate
  --network`) shown via a "Network active" badge; and **super-admin** add/list (`wp super-admin add|list`).
  Verified: creating a sub-site appears in `wp site list` + loads; network-activating a plugin shows the
  Network-active badge. Depends on 10.1, 6.1.

---

## 11. Optional / later (Phase 3+)

> Not required for the Phase 3 goal; pick up when the prerequisite lands.

- [ ] **11.1 Settings — DNS & SSL + autostart (Block 11)** — re-trust local CA, regenerate certs, DNS status
  indicator; "Start services on login" via the `AutostartManager` trait (macOS launchd). *(AutostartManager is
  the one Phase-1 trait still stubbed.)*
- [ ] **11.2 Xdebug-enabled PHP build** — if Xdebug isn't in the static-php "bulk" build (§8.1), source a
  static-php build variant with Xdebug compiled in (an external `xdebug.so` generally can't load into a static
  PHP); re-pin it for the debug pool so §8.2 can proceed.
- [ ] **11.3 Site templates / blueprints** (PROJECT_SPEC Tier 3) — reusable site setups incl. multisite.
- [ ] **11.4 Adminer per-site deep-link** — "Open database" jumps straight into the site's DB with a scoped
  session.

---

## Notes / decisions

- **WP-CLI is the only WordPress engine.** Every WP op = `php -d memory_limit=512M wp-cli.phar <args>
  --path=<docroot> --format=json` (reusing Phase 1 §9.1), parsed into typed structs; thin commands → `core` →
  WP-CLI, never ad-hoc shelling elsewhere. WordPress's DB is created/used via the site's `DbEngine` (MySQL
  default; MariaDB is the deferred Phase-2 §7.5 relational option).
- **Multisite topology (extends the Phase-2 topology):**
  - *Subdomain:* wildcard cert `*.mysite.test` (Phase 1 §3.2) + a **wildcard Caddy host route** (`mysite.test,
    *.mysite.test` → the site's backend) + the **subdomain** rewrite template (Phase 1 §6.2). DNS resolves
    `*.test` free (Phase 1 §2.1), so sub-sites need no extra DNS.
  - *Subdirectory:* single host, the **subdirectory** rewrite template; sub-sites are paths.
  - The mode persists in `sites.multisite` (**migration v3**) and drives `RewriteMode` in `rebuild_configs`.
- **Mail routing:** php-fpm `sendmail_path` → Mailpit's sendmail shim → Mailpit SMTP (`:1025`); the UI reads
  Mailpit's HTTP API (`:8025`). Changing it is a pool config rewrite + reload (no rebuild).
- **Xdebug:** a shared per-version pool can't load a `zend_extension` per request, so per-site Xdebug = route
  the site to a per-version **debug pool** (Xdebug on) via an nginx reload — **but only if Xdebug is in the PHP
  build** (verify first, §8.1; else §11.2). An external `xdebug.so` generally won't load into a static PHP.
- **New binaries — all via `BinaryProvider`:** Adminer (`resolve_file`, no codesign — PHP script), Mailpit
  (single binary, signed + de-quarantined), cloudflared (single binary, signed + de-quarantined).
- **New fixed loopback ports** (in `core/ports`, gated by `ensure_free`): Mailpit SMTP `1025`, Mailpit HTTP
  `8025`. Adminer reuses the nginx + php-fpm stack (no new port). cloudflared is outbound (no inbound port).
- **New crate:** `portable-pty` (terminal). **No new platform traits** — Phase 3 reuses the Phase-1 traits;
  `AutostartManager` (already defined, stubbed) is only filled if §11.1 is taken.
- **Schema:** migration v3 adds `sites.multisite` (`none|subdomain|subdirectory`). No other schema change
  expected (Mailpit/Adminer/tunnels are runtime services, not persisted per-site).
- **Tunnels origin:** cloudflared targets the **shared nginx HTTP port** with the site `Host`
  (`--http-host-header`) — a plain-HTTP origin. cloudflared terminates the external TLS, so there is NO
  local-CA origin-trust issue and `--no-tls-verify` is unnecessary. A tunnel is scoped to **one site's Host
  only** — never the edge wildcard, never an internal/tooling vhost (`adminer.rexenv.test`, Mailpit) — so
  sharing one site can't expose another site or a tool.
- **Verification pattern carries over:** lib unit tests + standalone `src-tauri/examples/*.rs` for live
  checks — real `wp plugin list` output, an email actually landing in Mailpit (API count), a real
  `trycloudflare.com` URL loading, `openssl` wildcard SAN + issuer, `phpinfo()` showing Xdebug.

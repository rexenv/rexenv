# ARCHITECTURE — how rexenv works today (as built, macOS)

The current-truth reference. Read this before any feature or bug task; it replaces
reading the codebase end-to-end. Line numbers are anchors, not contracts — trust the
file, verify the line. Ports + pinned versions: `docs/PORTS.md`. Open work: `docs/TODO.md`.

rexenv = native, no-Docker local dev environment for web/WordPress developers.
Tauri 2 desktop app: Rust backend + React/TS frontend. Runs the whole stack — edge
proxy, shared web server, multi-version PHP, databases, one-click WordPress, local-TLD
DNS (`.rex` backbone, configurable) + auto-HTTPS, mail catching, tunnels — from one UI. macOS complete; Windows/Linux
are `todo!()` stubs.

## 1. Layering (non-negotiable)

```
src/ (React/TS)  →  typed IPC wrappers (src/lib/ipc/index.ts, never raw invoke)
        ↓ Tauri IPC
commands/   thin translators only — parse args, call core, map errors
        ↓
core/       platform-agnostic domain logic ("the what") — no OS-specific code, ever
        ↓
platform/   ALL OS-specific code, behind 11 traits (platform/traits.rs):
            DnsManager · CertTrustManager · PrivilegeManager · ProcessSupervisor ·
            AutostartManager · PermissionManager · ShellRunner · Paths · BinaryProvider ·
            EdgeSupervisor · DnsAgentManager
```

- `platform/macos/mod.rs` — all 11 traits real. `platform/windows/`, `platform/linux/` —
  every method `todo!()`. Adding an OS = filling stubs, never restructuring.
- The only non-platform `todo!`-ish code is a defensive `unreachable!` in
  `core/binaries.rs`. `core/`, `commands/`, `state/` are macOS-complete.

**Module map** — the per-subsystem file/entry-point index is `docs/MAP.md`; every
module also carries a `//!` doc header stating its job. Quick inventory:
- `core/`: adminer · apache · binaries · blueprints · cli · confedit · confrewrite ·
  confverify · database (MySQL) · db (`DbEngine`) · dbcompat · dbdump · dbimport ·
  dbmirror · dbrestore · dbsource · devtools · dns · downloads · firefox ·
  frankenphp · logs · mail · mariadb · monitor · php · phpconf · ports · postgres ·
  proc · proxy · redis · repo · service_manager · services · setup · site_env ·
  site_metrics · sites · ssl · stack_guard · terminal · tld · tunnels · valet ·
  wordpress · wp_login · wp_tunnel · wporg
- `commands/`: blueprints · database · db_import · downloads · logs · mail · php ·
  repo · rewrite · services · settings · site_provision · sites · system · terminal ·
  tunnels · valet_import · wordpress · wp_install
- `state/`: app (AppState) · db (migrations) · models · store (repo)
- `src/routes/`: Sites · SiteDetail · Services · Databases · Mail · Tunnels ·
  Import · Settings · Onboarding (+ dev-only `/dev/git-panel`, `/dev/ui-review`)

## 2. Request & TLS topology

Default site:

```
browser ──HTTPS──▶ Caddy edge :443 (TLS terminate, local-CA cert per domain)
                     │  proxies ALL site hosts (any local TLD) by Host
                     ▼
                shared Nginx :18088 (ONE process, a server block per site, vhost by server_name)
                     │  fastcgi_pass → the site's PHP version's pool
                     ▼
                php-fpm pool 978x (ONE pool per PHP minor, not per site)
                     ▼
                WordPress → the site's DB ENGINE: MySQL :13306 or MariaDB :13307
                (per-site `sites.db_engine`, chosen at create, immutable after —
                the DB lives in that engine's datadir). PostgreSQL :15432 and
                Redis :16379 are optional engines on the Databases page.
```

- **No direct Caddy→php-fpm path for default sites.** Caddy is invisible plumbing.
- Local CA via `rcgen` (`core/ssl.rs`); per-domain leaf certs, wildcard SAN for
  multisite (`*.site.rex`). **Leaves must stay ≤398 days** — Safari/WebKit rejects
  longer ones even with the CA trusted. Caddy's auto-HTTPS/internal issuer is DISABLED;
  it serves our certs only.
- CA trust = **login keychain** (user op, `CertTrustManager`) — a detached-root osascript
  can't write System-keychain trust settings. `/etc/resolver/<tld>` files = root op
  (`PrivilegeManager`; onboarding installs the `.rex` backbone, other TLDs on first use). Hence ~2 setup prompts; true single prompt = SMAppService (deferred).
- **Per-site server overrides** (`OverrideKind` in the manager — one seam, two kinds
  today, OLS drops in later if a macOS artifact ever exists):
  - **FrankenPHP** (single static binary, embeds its own PHP): loopback backend on a
    per-site port in 8200–8299, `auto_https off` + `admin off`.
  - **Apache httpd** (`core/apache.rs`, bottle bundle): loopback backend on
    8300–8399 (a disjoint range, so a FrankenPHP↔Apache switch on one site can never
    collide with itself). NO embedded PHP: `.php` goes to the site's SHARED
    php-fpm pool via `mod_proxy_fcgi`, so per-version PHP settings apply identically.
    `AllowOverride All` — `.htaccess` works (the point of Apache); subdirectory
    multisite mirrors WP's canonical network rules in server context
    (`%{DOCUMENT_ROOT}%{REQUEST_URI}` existence checks — `REQUEST_FILENAME` isn't
    mapped yet in the vhost rewrite phase). Only the 10 conf-loaded modules are
    bundled; the bottle's compiled-in default paths are `@@HOMEBREW_PREFIX@@`
    placeholders and are never trusted — every path (pidfile, runtime dir, logs,
    mime map) is explicit + quoted.
  Each backend port is **recorded, not derived** (B20 §4): allocated collision-free
  (lowest free in the range) at create / web-server switch, stored in `sites.override_port`,
  and read verbatim by every consumer (edge route, spawn, adopt, status) — never
  re-hashed, so a domain change can't orphan a running backend (same "derived once,
  never re-derived" guarantee as `db_name`). FNV-1a of the domain survives ONLY as the
  one-time migration-backfill basis (`sites::backfill_override_ports`), which records
  each pre-existing site's exact current derived port (zero disruption) and resolves any
  pre-existing collision. `override_port_conflict` (recorded) stays as a belt.
  Neither may EVER be the edge, bind `:443`, or expose an admin endpoint. The edge
  routes an override site's Host to its backend; all other sites stay on the shared
  Nginx. `ServiceManager::reconcile_overrides` keeps backends in sync on start/reload
  (config-diff restart on docroot/rewrite/env/pool change; a KIND change stops the old
  backend — different port + binary). OpenLiteSpeed is refused in CORE at create AND
  switch (`ensure_server_available`) — no macOS binary exists (see TODO "Blocked").
- **Multisite** (`sites.multisite`: none/subdomain/subdirectory → `RewriteMode`): the
  config generator has three rewrite templates. Subdomain adds `mysite.rex, *.mysite.rex`
  to both the Nginx `server_name` and the Caddy host list over the wildcard-SAN cert;
  exact hosts always win, so a wildcard never shadows other sites.
- **Quote every path in generated Caddy/Nginx configs** — app-data paths contain spaces.
- **Per-site env vars (§1.6) never touch the shared pools.** Three delivery paths:
  Nginx sites get `fastcgi_param` lines in their server block (per-REQUEST); Apache
  sites get `SetEnv` lines (mod_env → subprocess env → mod_proxy_fcgi forwards them
  as FCGI params — the same per-request class as nginx); FrankenPHP overrides get
  config `env` lines **plus real process env at spawn** — per-site by construction,
  one backend process per site. Live-verified visibility: `getenv()`, `$_SERVER`
  **and `$_ENV`** all work — nginx and Apache both ride the php-fpm path (Apache's
  `SetEnv` verified in `$_SERVER` via `apache_site_check`); FrankenPHP differs — two
  mechanisms, each with a footgun:
  - php-fpm: `$_ENV` only works because our static PHP builds load NO php.ini, so
    `variables_order` is the compiled default `EGPCS` (E on) and the FPM SAPI imports
    the FastCGI request params into `$_ENV`. **Shipping a php.ini with the stock
    `variables_order = "GPCS"` would silently break `$_ENV` here** — don't, or re-add E.
  - FrankenPHP: its SAPI's `getenv()` reads ONLY the process environ (config `env`
    lines land in `$_SERVER` alone — verified: config-lines-only gave getenv()=false,
    $_ENV=null). Hence the spawn-time process env; a respawn (watchdog) must pass it
    too or vars vanish on a crash-restart.
  `core::site_env` is the trust boundary: names must be identifiers and non-reserved
  (every template FastCGI param — test-enforced against `TEMPLATE_FCGI_PARAMS` —
  + `PHP_VALUE`/`PHP_ADMIN_VALUE` + `PATH` + the `HTTP_` prefix — header forgery);
  values reject the UNESCAPABLE (`$` — nginx interpolates it in quoted strings;
  `{`/`}` — Caddy expands placeholders in quoted strings; control chars) and escape
  `\`/`"`. Plain text in generated configs — not a secrets store.

## 3. Caddy edge lifecycle (`core/proxy.rs`)

- Edge binds real `:80`/`:443` → needs one privileged install (foreground admin prompt —
  a backgrounded osascript can't show the dialog).
- **The edge runs under an OS supervisor that keeps it alive — `CaddyHandle::Daemon`**
  (`EdgeSupervisor` trait; macOS = a **root LaunchDaemon**, `dev.rexenv.rexenv.edge`,
  `KeepAlive=true` + `RunAtLoad=true`). launchd relaunches the edge on ANY death —
  external SIGTERM, crash, sleep/wake, logout, reboot — so the edge is the one service
  that recovers WITHOUT the health watchdog (which can't clear a privileged start's
  prompt). `proxy::start_edge_daemon` stages the plist + launcher-wrapper CONTENTS
  unprivileged, then runs ONE privileged `install_command` (`cp` into the root tree +
  `launchctl bootstrap system`); an already-installed daemon takes the lighter
  `start_command` (enable + `kickstart`, which re-execs caddy so it re-reads the current
  Caddyfile). **Security: the daemon executes a `root:wheel 0755` COPY of caddy under
  `/Library/Application Support/dev.rexenv.rexenv/bin/`, never the user-writable download
  cache** — re-execing a user-writable file as root is an LPE. The plist is `root:wheel
  0644` (launchd refuses a group/other-writable daemon plist). The wrapper hands the 0600
  admin socket back to the invoking user, then `exec`s caddy (so launchd tracks the real
  edge PID), keeping reload/stop promptless.
- **Admin API = private unix socket, never TCP `:2019`** (finding H5): the Caddyfile
  emits `admin "unix//<config>/caddy-admin.sock|0600"`; the privileged edge chowns the
  socket to the invoking user. A TCP admin on a root Caddy would let any local process
  POST config = arbitrary file read/write as root.
- `admin_alive()` actually **connects** (the socket file outlives a crash; a stat would lie).
- **A live edge is adopted, never killed.** `prepare_edge()` first probes OUR socket:
  if it answers, the edge is adopted (`Daemon` if the daemon is installed, else legacy
  `Privileged`) and the current config is pushed via `caddy reload` — no stop, no
  re-prompt, sites never drop. The health watchdog heals the reverse way too: an edge
  answering while the manager says stopped is re-adopted (`"adopted"` event). A **`Daemon`
  edge whose socket goes dead gets a BOUNDED grace window** (`EDGE_SUPERVISOR_GRACE_POLLS`
  = 3 × 10s; KeepAlive's throttle is ~10s): ONE `"edge-restarting"` info on first
  detection, silence while waiting, then — if launchd didn't bring it back — a DIAGNOSED
  `"edge-down"` (daemon uninstalled / label disabled via `EdgeSupervisor::is_enabled` /
  `:443` blocked or crash loop) with the handle flipped to Stopped. Never an unbounded
  "restarting" reassurance. A legacy `Privileged`/`Child` edge that dies becomes
  `"edge-down"` immediately (no OS supervisor). **`prepare_edge` trusts liveness, not the
  handle (H2):** a non-Stopped handle whose socket is dead is a STALE handle — reset and
  fall through to a fresh start, never a silent skip (regression-tested:
  `prepare_edge_restarts_over_a_stale_daemon_handle`,
  `watchdog_bounds_edge_restarting_and_diagnoses_the_giveup`).
- **Explicit stop costs one prompt, and boots out FIRST.** With `KeepAlive` a graceful
  `caddy stop` is instantly relaunched, so Stop-all removes the daemon from launchd:
  `stop_services` runs `proxy::stop_edge_daemon` (privileged `disable` + `bootout`,
  OUTSIDE the services lock, M4) BEFORE `stop_all` touches any manager state — while the
  auth prompt sits open the handle stays truthful (`Daemon` && alive), so the watchdog
  cannot re-adopt a doomed edge mid-stop (the race that once left a stale `Daemon`
  handle: edge-restarting spam + Start-all skipping the edge). A cancelled prompt stops
  nothing (all-or-nothing). `stop_all` then skips the admin-stop for a `Daemon` edge
  (already down); `disable` keeps it down across reboots until the next Start-all
  (whose `install_command` re-`enable`s before `bootstrap`).
- `recover_stale_edge()` (fallback, only when a live edge refuses the reload): probe OUR
  socket; a live leftover rexenv edge gets `caddy stop` over it (no privilege needed),
  polled up to 10×500ms, then a clear error if `:443` still isn't free. Ownership-gated:
  rexenv never binds nor queries TCP `2019`, so a developer's own Caddy (e.g. Herd's) is
  never touched (M1 invariant, `d35db62`).
- `stop_edge()` (legacy/non-daemon path): graceful admin-API stop, then reap processes
  running OUR caddy binary path (`owned_pids(marker)`). A root remnant may survive —
  logged; it holds no ports.

## 4. DNS — survives the app (`core/dns.rs`, `lib.rs`, `DnsAgentManager`)

- Embedded hickory-dns resolver answering ANY A query with `127.0.0.1` (TTL 60) on
  UDP **15353**. Deliberately TLD-agnostic: WHICH TLDs reach it is scoped by which
  `/etc/resolver/<tld>` files exist (one per TLD; no restart to add one). Safe only
  because it binds loopback and only our resolver files route to it (`core/dns.rs`).
- **The resolver runs OUTSIDE the app** — a per-user LaunchAgent
  (`dev.rexenv.rexenv.dns`, `KeepAlive` + `RunAtLoad`) running `<app binary>
  --dns-agent` (headless: no Tauri/SQLite/services; `dns::run_agent`). Rationale
  (observed live): the data plane outlives a quit, but the OLD in-process resolver died
  with the app — sites coasted ~1h40m on client caches/persistent connections, then went
  dark until relaunch. All unprivileged (`~/Library/LaunchAgents`, high loopback port;
  `launchctl load/unload -w` — no prompt). The agent never exits on a busy port: it
  retries every 10s, so an old in-process holder hands off seamlessly.
- **App launch = adopt-or-install-or-fall-back** (`lib.rs`): probe
  `dns::answers_as_ours` (a REAL A query must return `127.0.0.1` — ownership AND
  liveness, H2 — never a bare port probe); refresh the plist every launch so it tracks
  the current binary (dev ↔ installed hand off); if the agent can't come up, fall back
  to the legacy IN-PROCESS task (`DnsState { service, mode: Agent | InProcess | Down }`)
  so DNS never regresses — Settings surfaces the degraded mode (`dns_status.mode`).
- Health watchdog (`lib.rs`), mode-aware and **bounded to 3 attempts**: Agent → wire
  probe, dead agent gets a `launchctl` kickstart, and after 3 failed kicks ONE in-process
  fallback (sites resolve now, mode says it won't survive quits); InProcess → restart the
  task in place. Emits `service-health` events. A resolver that never started (port
  conflict at launch) is NOT auto-restarted — surfaces in Settings. Teardown
  (`run_system_teardown`) also uninstalls the agent.
- The OS-side `/etc/resolver/<tld>` files are a separate privileged step, independent
  of the in-process server: onboarding installs the `.rex` backbone (`core/setup.rs`);
  any other TLD (`.test` included) installs on first use (`dns::ensure_resolver`, one
  prompt per TLD); uninstall sweeps every file matching our content signature.

## 5. Service lifecycle & source of truth (`core/service_manager.rs`)

- `ServiceManager` owns the whole stack: DB engines (MySQL/MariaDB/PostgreSQL/Redis),
  php-fpm pools, shared Nginx, per-site override backends (FrankenPHP/Apache —
  `OverrideKind` dispatch across reconcile/spawn/watchdog/adopt/status/ports/serving),
  Mailpit, Adminer, edge. Commands mirror DB state into it before starts (same pattern
  for all three): `set_php_settings`, `set_site_env`, and `set_db_versions` (the
  per-engine SELECTED version — so the watchdog respawns a crashed engine on the
  selected version, not the default pin).
- **Services OUTLIVE the app.** Closing rexenv stops nothing. On launch,
  `adopt_startup()` ADOPTS rexenv-owned survivors as pid-based `Proc::Adopted` handles —
  status/Start all/Stop all treat them like spawned children. Ownership gate = process
  holds one of OUR fixed ports AND references our app-data dir on its cmdline
  (`owned_listeners(port, marker)`, lowest pid = master). The root edge is invisible to
  unprivileged lsof → adopted iff OUR admin unix socket answers. If anything was adopted,
  cached nginx/caddy binary paths are wired strictly offline (existence-checked, no download).
- `reconcile_startup()` = the OLD stop-orphans-at-boot, now only an explicit cleanup path
  (e.g. `examples/stack_stop`), never run automatically.
- **"Running" is ownership AND liveness, never a bare port-listen** (finding H2):
  `status()` computes e.g. Nginx = handle present && `nginx_running(port)`; Caddy =
  not-Stopped && `admin_alive()`; DB = handle && `engine.running()`. Sites derive status
  from the actual stack (H1) — there is no fake per-site toggle.
- **Locking rule (M4):** `AppState` uses field-level locks; only `ServiceManager` sits
  behind an async Mutex. NEVER hold that lock across a wait: manager methods spawn under
  the lock and return `ReadyCheck` probes; commands `await_ready(checks)` AFTER dropping
  it (concurrent, named-service errors — M3). Same two-phase shape as `prepare_edge`.
  Status polls use `try_lock` with a cached-snapshot fallback (`state/app.rs`) so a long
  start/stop never blocks the UI.
- **Prefetch-before-lock invariant (download manager):** any command that can resolve a
  binary while holding the services lock MUST prefetch the needed binaries FIRST
  (unlocked), via the download hub (`core/downloads.rs` — plan → `prefetch` → then lock,
  whose resolves are then cache hits). Downloading under the lock blocks all status reads
  on a cold cache — the silent-hang bug. Current prefetch sites: `start_services`,
  `start_database`, `start_mail`, `set_php_version_installed`, `create_site`,
  `set_site_web_server`, `set_site_php_version`, `delete_site`, `set_db_engine_version`.
  Add new binary-resolving commands to this list.
- Long-running children spawn via `ProcessSupervisor::spawn_logged` →
  `<log_dir>/<svc>-stdout.log`. `stop` escalates to SIGKILL after a grace window (L3).
  Beware orphan workers after a SIGKILLed master: title-rewritten fpm/nginx workers can
  hold ports and defeat probes — the health watchdog + `Proc::terminate` guard this;
  check `logs/health.log` first.

## 6. Resource monitor (`core/monitor.rs`, `commands/services.rs`)

- `sysinfo`-backed `Monitor { sys: System }` in AppState; CPU% is a delta between
  refreshes, so the `System` must persist.
- Poll flow: frontend → `services_status` → `enriched_status` → `service_infos()`
  (ServiceManager = truth for running/pid) → **ONE** `refresh_processes()` sweep per poll
  (M6) → per-pid `tree(pid)`.
- Metrics are per process TREE (master + descendants via parent-pid walk) — workers fold
  into the service row. CPU is per-core percent (can exceed 100, Activity-Monitor style).
- `ram_mb == 0` (cross-user unreadable, e.g. root edge) → fall back to supervisor
  `resource_usage(pid)` ps accounting; the pid-less privileged edge is resolved via
  `owned_pids(caddy_marker)`.

## 7. Binaries (`core/binaries.rs`, `BinaryProvider`)

- No Docker; everything is a native static binary downloaded on demand, pinned +
  checksum-locked in a manifest (os+arch+version → url+checksum). Versions: `docs/PORTS.md`.
- macOS `prepare_binary` order (non-negotiable): **de-quarantine → relink Homebrew dylibs
  to `/usr/lib` → ad-hoc codesign LAST.**
- Shapes: single Mach-O (`resolve`) · plain file like WP-CLI `.phar` (`resolve_file`, no
  chmod/codesign) · dir tree like MySQL/PostgreSQL (`Archive::TarGzTree` + `resolve_dir`;
  extraction guards against path/symlink escapes, L2) · **bottle BUNDLE** — Redis
  (+ openssl@3), MariaDB (server/clients/bootstrap-SQL/errmsg/charsets + openssl@3 +
  pcre2; plugins excluded so groonga/lz4/lzo/xz/zstd never enter the closure), Apache
  httpd (server + the 10 conf-loaded modules + mime.types + apr + apr-util + pcre2;
  mod_ssl/mod_http2/mod_brotli excluded so openssl/nghttp2/brotli stay out)
  (`bundle_manifest` + `resolve_bundle`): services with no portable static build are
  assembled from Homebrew-bottle ghcr blobs (content-addressed — the URL embeds the
  pinned digest, so bytes can never drift under a URL; anonymous bearer auth), an
  include-filtered strip-2 extract merges them into one tree, then
  `BinaryProvider::prepare_binary_tree` rewrites every Mach-O's non-system load command
  (`@@HOMEBREW_*@@` placeholders) to `@loader_path`-relative paths into the bundle's
  `lib/`, errors loudly on any dep NOT bundled, and ad-hoc re-signs each Mach-O LAST.
  Resolves stage + atomically publish so a failed prepare can't poison the cache (H4).
- MySQL is Oracle-signed (never re-sign) and needs a direct CDN URL + browser UA.
- **Multi-version engines (per-engine DB version switch):** `MYSQL/POSTGRES/MARIADB/
  REDIS_VERSIONS` are the offered sets (default first); the mysql/postgres manifests
  are version-templated like PHP's, mariadb pins a second bottle bundle
  (`mariadb@11.4` — ghcr maps `@` → `/`). The selection lives in the
  `db_version_<engine>` settings KV (validated in core; an orphaned selection falls
  back to the default pin). **Each version SERIES keeps its OWN datadir** — never an
  in-place upgrade/downgrade (PG major datadirs are mutually incompatible;
  MySQL/MariaDB downgrades unsupported): the default pin's series keeps the legacy
  `<engine>/data` path (existing data never moves), other series live under
  `<engine>/<series>/data` (`DbEngine::series_of`/`data_dir`). One server per engine
  at a time, always on the engine's fixed port — adoption/status stay version-agnostic.
- **MariaDB has no `--initialize-insecure`** and its `mariadb-install-db` is a shell
  script full of baked brew paths — `core/mariadb.rs::initialize` drives
  `mariadbd --bootstrap` DIRECTLY, feeding the bundled SQL over stdin with
  `@auth_root_socket=NULL` (passwordless root@localhost + root@127.0.0.1, the MySQL
  model); share data via EXPLICIT `--lc-messages-dir`/`--character-sets-dir` (the
  compiled-in defaults are placeholders). A failed bootstrap removes the half-written
  datadir so the `mysql/`-dir marker can't lie.
- The `php-debug` (Xdebug) variant is fully wired but returns `None` from `manifest()`
  until its checksums are pinned — see `docs/xdebug-debug-build.md`.

## 8. Data & app state

- **SQLite for all app state** (`state/db.rs`), `user_version` migrations, currently 25:
  v1 `sites` + `settings` · v2 `php_versions` registry · v3 `sites.multisite` ·
  v4 `blueprints` (JSON `spec`) · v5 `php_settings` · v6 `sites.db_name` (stored, never
  re-derived) · v7 `site_env` · v8/v9 `default_tld` seed + `.rex` flip ·
  v10 `sites.db_engine` (TEXT, default `mysql` — exact backfill, every pre-v10 site
  lives in MySQL's datadir) · v11 `sites.xdebug` · v12 `site_git_assets` · v13
  `site_git_assets.source` · v14 `sites.override_port` (nullable INTEGER, NO UNIQUE —
  the recorded override backend port; uniqueness enforced by the allocator, existing
  rows backfilled once at startup by `sites::backfill_override_ports`, B20 §4) · v15
  `site_git_assets` install fingerprints · v16 `sites.provisioned` (DEFAULT 1) ·
  v17 `sites.docroot_managed` (see below) · v18 `resolver_takeovers` (see below) ·
  v19 `sites.db_created` (import provenance, see below) · v20 `db_imports` (the ONE
  settled import fact) · v21 `db_imports.verified` + `config_rewrites` (see below) ·
  v22 `config_rewrites.written_digest` · v23 `tunnels` (spawn-time share rows —
  see the tunnels entry in §9) · v24 `sites.content_dir` (the recorded content-dir
  rel, below) · v25 `sites.mu_dir_created` (set-once when a writer creates
  `mu-plugins/`; delete removes the dir only when recorded ours + empty).
  Per-engine DB versions are settings-KV rows (`db_version_<engine>`), not a migration.
- **The content dir is RECORDED, never re-derived at write time** (v24
  `sites.content_dir`, decided once at create/backfill from filesystem markers,
  poison-resistant): every writer that builds a `wp-content`-relative path itself —
  mu-plugin writers, asset destinations, the unlink-delete guard, theme
  screenshots, the debug-log reader — takes the recorded rel. Bedrock (`web/
  app/`) is why: deriving at write time silently wrote where WordPress never
  loads. Anything the record can't answer reads honestly `indeterminate`.
- **Docroot ownership is RECORDED, never inferred from the path** (v17
  `sites.docroot_managed`): `true` = rexenv created the folder and teardown may
  remove it; `false` = a folder the user LINKED, or one moved outside the sites
  folder — never deleted. Decided where the fact is known (create / link / move)
  and monotonic toward safety (only ever 1 → 0; linked sites refuse
  `move_site_docroot` outright, since a cross-volume move copies then DELETES the
  source). Such a folder still relocates — the user moves it, then
  `relink_site_docroot` RECORDS the new path and reloads the config, touching no
  file (`check_docroot_relink` only proves the destination is a real, servable
  directory). Refusing to move it is about who may write files, not about
  freezing an imported site where it landed. Nullable with NO default, because a default would have to guess for
  moved-out rows whose confirm dialog promises they are kept; a Rust backfill
  (`sites::backfill_docroot_managed`, the v14 pattern) evaluates the legacy
  lexical sites-dir test ONCE per existing row and freezes it, so upgrades change
  nothing. Deleting at delete time used to re-derive this from the MUTABLE
  `sites_dir` setting — pointing the Sites folder at `~/code` silently made an
  unrelated project deletable. `teardown` returns `{ existed, docroot_removed }`
  so "your folder is still there" is never ambiguous.
- **Resolver files can be BORROWED, and must be returnable** (v18
  `resolver_takeovers`). Ownership of `/etc/resolver/<tld>` is content equality
  (`resolver_contents` doubles as the signature), which has a sharp
  consequence: overwriting Valet's file makes it indistinguishable from one we
  created, so the teardown sweep would delete it and the user would be left with
  neither their config nor ours. `ensure_resolver` therefore REFUSES a foreign
  file outright; the only path that may replace one is `take_over_resolver`,
  which writes a 0600 backup and its row BEFORE the privileged write (a
  cancelled prompt rolls both back). Teardown then decides per TLD:
  ours+record → restore theirs; ours+no record → remove; foreign+record → they
  reclaimed it, touch nothing and drop the record; foreign+no record → invisible,
  as always. Restore `cp`s the backup (their arbitrary bytes never enter a root
  shell string) with the app-data path `sh_quote`d. Backups are named per TLD,
  not per timestamp, so at most one can exist per TLD BY CONSTRUCTION; the row
  owns its file and both die together, with a startup sweep covering a crash
  between the two. `dns::drifted_takeovers` reports a borrowed file another tool
  reclaimed — checked at startup and in `rex doctor`, because our resolver keeps
  answering so every health probe stays green while those sites go dark.
- **Valet/Herd import** (`core/valet.rs` + `commands/valet_import.rs`,
  `/import`): a strictly read-only scan of their config, symlink farm and
  per-site confs — nothing of theirs is written, started or stopped, and no file
  inside a user project is opened. Deliberately tolerant of real installations
  (dangling symlinks, confs with no site, proxies, a conf on a TLD the config
  never mentions, the pre-2.1 `domain` key, duplicate parked paths, and all
  three isolation-marker formats), surfacing every case as a row or a note
  rather than dropping it. Herd wins a duplicate domain. Import reuses the
  ORDINARY create path per site (`site_provision::start` + poll), sequentially
  and CONTINUE-ON-FAILURE, with resolver consent and `php::set_installed` +
  prefetch settled BEFORE the loop. `examples/valet_scan_check` fingerprints
  their trees before and after to prove the scan wrote nothing.
- **Database import copies their database and RECORDS what it did** (Stage 2:
  `core/{dbsource,dbcompat,dbdump,dbrestore,dbmirror,dbimport}`, v19 `sites.db_created`,
  v20 `db_imports`). Their side is read-only: engines identified from the pre-auth
  handshake (never a plist's `Status`), dumps are non-locking single-transaction to a
  0600 artifact + manifest, and their server is never started or stopped. Restore is
  provenance-FIRST: `db_created` (1 = this import created it, 0 = pre-existed → never
  dropped by any path, NULL = legacy provisioning) is written before `CREATE DATABASE`.
  Credentials are mirrored loopback-only (`localhost`+`127.0.0.1`, never `'%'`, never
  root — reserved accounts refuse as an outcome). The settled fact is ONE serialized
  row (`db_imports`): badge, summary and panel all render it, so they cannot disagree,
  and its `state` closed set has a `connected` value ONLY the rewrite's verification
  can write. Secrets: in memory for the job only, never argv (0600 defaults file,
  Drop-deleted), never logged, never persisted.
- **The connection rewrite writes ONE user file, provably** (Stage 3:
  `core/{confedit,confverify,confrewrite}`, `commands/rewrite.rs`, v21/v22).
  `RewriteKey` is a closed enum (Host/Port/User) — no password key exists, so no plan,
  diff or write can stage one; wp-config edits are OUR value-span editor (not wp-cli:
  the preview must BE the write). The diff is derived from the produced bytes; apply
  is gated on a whole-file sha256 fingerprint from preview time; writes refuse MORE
  than reads (any unclosed quote, equal-value duplicates, heredocs, commented-out
  keys → tell-only with the reason). Runtime order: backup (first-backup-wins, PK
  (site_id,file)) → row → temp+rename write (mode preserved) → digest → sign-in
  verification. `connected` is minted only via `confverify::Verified`, constructible
  only by a sign-in that succeeded against the RE-READ file (the HTTP probe can only
  upgrade a proof, never create one). Revert classifies via `written_digest` (NULL =
  can't-prove → conservative), clears the fact BEFORE restoring (crashes land in the
  under-claim direction), and every ugly case is a named state — backup-missing keeps
  `connected` (still true) and drops the row. Site delete: mirrored users drop by the
  RECORD (never re-derived), non-WordPress databases drop only on `db_created = 1`,
  and the connected-site confirm names both outcomes (revert-then-delete default).
  Both delete confirms (and site reset, and db-import overwrite) are gated on typing
  the site's domain, shown with a copy button — see the honest-UI rule in
  `docs/DESIGN.md`.
  Pools pin `mysqli.default_socket` at our MySQL socket (compiled default EMPTY →
  strictly additive for `DB_HOST=localhost` imports); `pdo_mysql` stays out
  permanently (its compiled default is Homebrew's `/tmp/mysql.sock` — an override
  would silently redirect a working site).
- **Linked sites are ADOPTED, never provisioned into.** A non-empty
  `NewSite.path` means "serve this folder in place": `core::sites::provision`
  validates it (`validate_linked_docroot` — canonicalized once so a later symlink
  swap can't redirect what we serve; refuses overlap with another site, our own
  app data, the sites folder, and blast-radius roots `/`, `/Users`, `$HOME`,
  `~/Desktop|Documents|Downloads`, volume roots — a docroot can be published by
  the tunnel feature) and creates nothing. `phase_defs` gives it
  prepare/fetch/serve whatever its type: `configure` is only half idempotent
  (skips `wp config create` when wp-config.php exists, then creates the database
  unconditionally), so running the WordPress phases over an existing install
  would leave a stray empty database. `core::sites::detect_project` classifies a
  folder by filesystem probes ONLY — never by interpreting Valet's PHP drivers,
  which would mean executing the user's code during a scan — and knows the
  docroot is often a subfolder (Bedrock `web/`, Laravel/Symfony `public/`, Craft
  `web/`, Magento `pub/`).
- **The served root is not always the site root.** `sites.docroot_subdir` (v32) +
  `Site::served_root()` are the ONE place the two are combined — the nginx vhost,
  the FrankenPHP/Apache override and the desired-state map all call it. `path`
  stays the project root: what teardown removes and where Composer/artisan run.
  A **created Laravel** site is the reason: `composer create-project` puts the
  front controller in `public/` and `.env` — the site's database credentials —
  one level above it, so serving the project root would publish that file. A
  **linked** project's stored path already points at the folder to serve, so it
  keeps an empty subdir (v32 backfills `public` only for `docroot_managed = 1`).
- **Laravel sites are installed, not just served.** `phase_defs` gives a managed
  Laravel site db → `app_install` (`core::laravel::create_project` —
  `composer create-project laravel/laravel` run through the SITE's bundled PHP,
  never a system composer, which may be a wrapper rather than a phar) →
  `configure` (create the database, `wire_env` the `.env`, then re-run the
  migrations). The re-run is load-bearing: the skeleton's own post-create script
  runs `artisan migrate` while `.env` still says sqlite, so the tables land in
  `database/database.sqlite` and the site's MySQL database would otherwise stay
  empty. `db_created` is recorded at create so delete drops that database
  (non-WordPress types drop only on that explicit provenance).
- `AppState` (`state/app.rs`) = db + platform + monitor + CA + ServiceManager + Terminals/
  Tunnels registries, **field-level locks** (see §5 locking rule).
- Every service start is gated by `core/ports::ensure_free`; a conflict names the holding
  process + a copy-paste free command.

## 8.1 `rex` CLI (`cli/`, `src-tauri/src/cli_server.rs`)

- **Remote control ONLY — the app stays the single brain.** The `cli/` crate (bin
  `rex`) never links the app lib: it cannot open SQLite or spawn/stop services, only
  write one JSON request line to the app's private socket and print the reply. That
  makes the examples-stop-the-edge bug class impossible at compile time.
- **Socket:** `<config>/rexenv-cli.sock`, `0600`, next to `caddy-admin.sock` — same
  trust boundary (same-user processes already own our SQLite/processes; other users
  are locked out). Never TCP. Stale files are unlinked at bind; the CLI *connects*
  to detect liveness (a stat would lie — same lesson as `admin_alive`).
- **One code path:** each request dispatches to the SAME `commands::*` fn the UI
  invokes — never a parallel implementation. The surface (42 commands as of 16 Jul
  2026) covers lifecycle, sites (create incl. `--blueprint`/`--multisite`, delete,
  info/open/login, settings switches), logs (`--follow`), PHP/Xdebug, databases
  (export/import/reset/versions), the WP plugin/theme/user manager + singles,
  mail/tunnels/tld, `doctor`, and completions — the authoritative list with
  per-command evidence lives in `docs/CLI-ROADMAP.md` (and `rex help`).
  Conventions: destructive ops confirm + `--yes` (`db reset` requires TYPING the
  domain), passwords are generated + printed once (never argv), long ops hold the
  connection, `--json` everywhere, exit codes 0/1/2.
- **Packaging:** `rex` ships as a Tauri sidecar (`bundle.externalBin`, staged by
  `scripts/build-cli.sh` — aarch64 + x86_64 + lipo'd universal; `build.rs`
  self-stages for bare cargo builds) → `Contents/MacOS/rex`, signed with the
  bundle. PATH install = one symlink `/usr/local/bin/rex → <bundle>/rex`
  (`Paths::cli_symlink_path`; unprivileged attempt first, one admin prompt
  fallback) from the Settings "Command-line tool" card; teardown removes the
  link only when it is ours (content-checked).
- **App not running → hard error, exit 2** ("open the app first"). Deliberate: a
  headless CLI-spawned backend would be a second ServiceManager/SQLite writer/
  watchdog racing the GUI — the exact second-brain class the stack guard exists to
  kill. Protocol: newline-delimited JSON, one request per connection,
  `{"ok":true,"data":…}` / `{"ok":false,"error":…}`; `--json` for scripting.

## 9. WordPress layer

- Provision (`core/sites.rs`): docroot + cert + DB + config gen + WP core install via
  WP-CLI. WP-CLI always runs PHP with `-d memory_limit=512M` (core extraction OOMs at
  128M). Switching PHP version or web server = config regen + reload, never a
  docroot/cert/DB rebuild. Domains are validated in core before becoming a
  path/config/cert/DB name (M7).
- **wp-cli argument hygiene (extends M7):** anything that lands in wp-cli argv from IPC
  is whitelisted in core (`DEBUG_FLAGS`, `PERMALINK_STRUCTURES`, `USER_ROLES`); names
  that can't be whitelisted because they're site-defined (cron hooks) pass as a single
  argv element, never through a shell. Guards live in CORE, not the UI: the primary
  administrator (lowest-ID admin — the one-click-login anchor) is refused by
  `user_set_role` itself, so no IPC path can demote it.
- **Bundled-client rule (recurring trap):** `wp db create/export/import` shell out to a
  PATH `mysql`/`mysqldump` that a Finder-launched app doesn't have (bare launchd PATH).
  Any DB feature must use the bundled clients with shell-free I/O — `--result-file`
  for output, stdin for input — never `wp db …`, never shell redirection (app-data
  paths contain spaces). **Engine- and version-aware since the per-site engine work:**
  `core/database.rs` fns take the client/dump BINARY (not a tree) — MariaDB speaks the
  same protocol, only the binaries and port differ — and every site DB op resolves
  them via `DbEngine::sql_client_bins(platform, effective_version)` from the site's
  `db_engine` + the engine's selected version, so the client always matches the
  running server. Status polls use `cached_sql_client` (strictly offline — never a
  download from a poll). Start-all spawns MariaDB exactly when some site's database
  lives there; per-site Adminer deep links carry the site's engine (the wrapper's
  loopback gate covers both ports).
- **Tool results ≠ app errors:** `wp core verify-checksums` exits 0 even with
  "should not exist" extras (verified live) — verdicts derive from PARSED findings,
  never exit codes alone; extras triage as benign only when the basename is known OS
  noise (`.DS_Store`, `._*`, …), and unknown warnings stay loud.
- **Mail:** php-fpm `sendmail_path` (DOUBLE-quoted in the pool ini — the parser strips
  bare quotes and app-data paths contain spaces) → Mailpit's `sendmail -t -S
  127.0.0.1:11025` shim → SMTP sink; inbox UI reads the HTTP API on 18025 (`core/mail.rs`).
- **"Log in as"** (`core/wp_login.rs`): one-time, single-use, loopback-only magic link
  via a mu-plugin.
- **A site can reach ITSELF** (`core/wp_dns.rs`, 10 Aug 2026): the bundled static-php
  builds link libcurl against **c-ares**, which resolves from `/etc/resolv.conf` ALONE
  and never reads macOS split-DNS (`/etc/resolver/<tld>`) — where rexenv publishes every
  TLD it serves. So inside php-fpm `gethostbyname("x.rex")` answered `127.0.0.1` while
  `curl` to the same host died with errno 6, and **WP-Cron stopped on every hosted
  WordPress site with nothing logged** (it spawns itself with a fire-and-forget HTTP
  request and never checks the result). Site Health loopbacks, REST self-calls and
  sibling-site requests failed the same way; WP-CLI hid it (cron events run in-process,
  no HTTP). The fix is an auto-managed mu-plugin (`rexenv-dns.php`) that, on
  `http_api_curl`, hands cURL the address the SYSTEM resolver already has
  (`CURLOPT_RESOLVE`) — but ONLY for hosts whose TLD has an `/etc/resolver/` file naming
  a **loopback** nameserver, so a VPN's split-DNS and all public DNS are untouched. It
  bakes in nothing per-site (no domain, no TLD list), which is why a domain change or a
  new TLD needs no rewrite; it no-ops on threaded-resolver builds (FrankenPHP) by its
  first guard. Installed at provision, re-installed after a rename (the mu-plugin sweep
  removes it), and swept for every WordPress site at launch — the launch pass is what
  makes it true for sites that predate it. The real elimination of the bug class is a
  PHP build using curl's threaded resolver (open, `docs/TODO.md`).
- **Tunnels** (`core/tunnels.rs` + `core/wp_tunnel.rs` + `commands/tunnels.rs`):
  per-site cloudflared quick tunnel, scoped to ONE site Host, outbound-only. Behind
  the edge `REMOTE_ADDR` is always `127.0.0.1`, so loopback-only enforcement keys off
  `CF-*` headers + leftmost `X-Forwarded-For` + `Host`, never the IP. On tunnel start
  an auto-managed mu-plugin bakes in the public origin (HOST/HTTPS overrides +
  siteurl/home filters + output-buffer rewrite for plain/JSON-escaped/%-encoded);
  removed on stop; local requests untouched.
  - **Tunnels DIE WITH THE APP** (ruled 28 Jul 2026 — the deliberate opposite of
    services-outlive-the-app: a public share must not outlive the thing supervising
    it). The v23 `tunnels` row is claimed atomically BEFORE spawn (the row IS the
    double-start guard; sentinel pid until the child exists), `RunEvent::Exit` kills
    from rows, and the launch sweep kills only on positive argv identity — plus a
    rowless backstop for cloudflared processes carrying our argv identity with no
    row (app-data reset class), which stops them with a loud WARN.
  - **Share health is its own tri-state** (Live / Unverified / Broken) probed via
    bounded HEADs of the public URL: any non-530 answer proves the path, 530×3 =
    Broken (sticky — only an HTTP answer clears it), transport errors are
    non-evidence. Phase A after start probes via 1.1.1.1 + pinned-address edge
    checks ONLY — never the system resolver, whose negative cache would poison the
    LAN for 30 minutes (trycloudflare SOA MINIMUM = 1800s, measured).
  - **A share guards its site for the share's LIFETIME:** web-server switch,
    multisite convert, docroot move, db-import/rewrite and provision-retry all
    refuse while shared, naming the exposure (and tunnel start refuses while those
    run). rexenv never auto-stops a share on the user's behalf; quitting with live
    shares gets a native confirm naming the honest count.
- **Adminer** (`core/adminer.rs`): internal vhost `adminer.rexenv.rex` on the shared
  stack — never a tunnel origin. Per-site deep link via a generated `index.php` wrapper
  (`adminer_object()` hook): passwordless login for loopback servers only, auto-submits
  Adminer's own CSRF-tokened + CSP-nonced form on `?rexenv_auto`.
- **Blueprints** (`core/blueprints.rs`): reusable presets (plugins/themes/WP_DEBUG/
  multisite) applied AFTER the one-click install.
- **Plugin/theme lists are TWO passes** (`useWpPlugins`/`useWpThemes`): an instant
  `--skip-update-check` list, then a background list WITH the api.wordpress.org
  check (seconds when slow, a hang when offline) that supplies the `update` badge
  **and `update_version` — the version the update installs**, rendered as
  `v10.8.1 → 10.9.0`. The arrow is drawn from that field alone, never inferred
  from the badge: the fast pass genuinely does not know the target, and a guessed
  version is worse than none. `update_version` is NOT in wp-cli's default theme
  field set, so `theme_list` names its fields explicitly — the silent-empty trap.
  **A row may claim an update only when the offered version is NEWER than the one
  on disk** (`verdict` + `isNewerVersion`, the ONE place either list decides it):
  `--skip-update-check` does not mean "no update info", it means "read the update
  transient without refreshing it", so BOTH passes hand up a claim that can be
  stale. After a finished update the stale claim is `update → the version we just
  installed`, and it kept the badge and the arrow on a plugin already at that
  version — unrenderable now, whatever the source (an in-flight pre-update check,
  or a premium plugin's own updater caching its answer for hours). Compared
  segment-by-numeric-segment: a string compare says `1.1.11` is older than
  `1.1.3.8`.
- **A finished update SETTLES both caches before it refetches** (`settleAfterUpdate`).
  The checked pass on a real site takes tens of seconds to over a minute (every
  plugin against wp.org, plus every premium plugin's own API), so a check that
  started BEFORE the update is usually still running when the update ends — and its
  answer, describing the old disk, landed on top of the fix and put the badge back
  for as long as the NEXT check took. So: cancel the in-flight check first, then
  erase what both caches claim about the updated items (wp-cli exited 0 — they are
  at the version it installed), and only then invalidate, leaving the refetch as the
  only writer. Core does the same by invalidating `wp-info`, which is where the core
  version every other card shows comes from.
- **Every update STREAMS — plugins, themes and core** (`core::wordpress::
  update_streamed` + `UpdateTracker`, keyed by `UpdateKind` →
  `wp-update://{plugins,themes,core}/<siteId>`): one wp-cli call, its stdout pumped
  line-by-line through `repo::run_step_streamed`, parsed into (item, phase, step)
  and emitted per line. One parser, not three: it is WP's one `WP_Upgrader` wearing
  a noun, so only the SETTLED line differs (plus core's own two, and core is a
  single self-named item). **A language-pack pass runs INSIDE a plugin/theme
  update** — its "Translation updated successfully." is a step, never an item
  finishing; counting it banked a plugin per translation and ran the bar ahead of
  the work. Bounded by SILENCE (`UPDATE_IDLE_LIMIT`, 420s), not by
  a total cap — deliberately past WP's own 300s `download_url` attempt cap so WP's
  error is the one the user reads instead of our kill. ONE command, not a UI variant:
  CLI/MCP callers just have no listener. The captured `plugin_update` (hard total
  cap) stays for callers with no sink.
- **Adding a plugin/theme has FOUR sources, and two of them are the same job**
  (`SourceTabs`): WordPress.org search, **Upload zip**, From Git, Link folder.
  wp.org and zip both run `commands/wp_install.rs` — same streamed card, same
  Cancel, same per-job log — because wp-cli takes a slug and a local archive in
  the same positional slot. What differs is the GATE, and it is two gates
  rather than one loosened one: `ensure_slugs` (wp.org: `^[a-z0-9][a-z0-9-]*$`,
  so a URL/path/zip is still refused there) and `ensure_zip_paths` (absolute +
  `.zip` + an existing regular file). The zip is read where it sits — nothing
  is copied, unpacked or uploaded by rexenv; WordPress's own installer does the
  unpacking, which is why a badly-shaped archive fails exactly as it would in
  wp-admin. **One honest consequence, stated rather than hidden:** wp-cli
  prints its per-item `Installing name (version)` header only on the wp.org
  path, so a zip job's attempt cursor can never advance — the card omits the
  cursor instead of parking it at "item 1 of N", and the bar runs on the
  unpack/install/activate milestones alone (`InstallProgress` may run BEHIND
  the work, never ahead). The `rex` CLI keeps slugs only.
- **Add plugin/theme from Git** (`core/repo.rs` + `core/devtools.rs` +
  `commands/repo.rs`): paste URL (https/ssh/scp/`owner/repo`; forge `/tree/`
  URLs preselect the branch) → `ls-remote` probe (URL+auth validated BEFORE
  any clone; 30s cap) → streamed clone into `wp-content/{plugins,themes}` →
  read-only detection (packageManager field > lockfile > npm; composer.json;
  WP header; `.nvmrc`/engines warning) → EXPLICIT one-click steps. Rules:
  - **Deliberate bundled-client-rule departure:** builds run the DEVELOPER'S
    toolchain (their nvm node, their ssh-agent), resolved from a cached
    login-shell env snapshot (`ShellRunner::login_shell_env` — `$SHELL -ilc`
    with a NUL-marker protocol; a Finder-launched app has the bare launchd
    PATH and nvm is rc-file init, so naive PATH detection misses node for
    every JS dev). Missing tool = honest `$`-fix error; `git_preflight`
    probes the CLT quietly so `/usr/bin/git`'s shim can never pop a GUI
    dialog from a background task. EXCEPTION: composer is ALWAYS our pinned
    phar run by the SITE's PHP version — platform checks (`php`, `ext-*`)
    then match reality, and system "composer" can be a non-phar wrapper.
  - **Repo scripts never run implicitly.** Clone+detect executes no repo
    code; composer/install/build are one explicit click each, with the
    disclosure line above the buttons. `GIT_TERMINAL_PROMPT=0` forced — a
    hidden credential prompt fails fast with a mapped error, never hangs.
  - **Every child runs in its OWN process group** (`spawn_streamed`/
    `stop_group`, pkill/pgrep `-g`): npm/git spawn worker trees, and cancel
    must kill the TREE (§5 orphan-workers lesson). Jobs die WITH the app
    (RunEvent::Exit hook) — the deliberate OPPOSITE of services-outlive-
    the-app: installs are interactive actions, and an orphaned npm would
    keep writing into wp-content. Output streams line-wise
    (`repo-job://state|output/<id>` events) + a flat
    `logs/repo-<domain>-<dir>.log` (readable via the existing log IPC).
  - **Dotfile guard in ALL THREE vhost templates** (nginx regex location
    before the `.php` location; Apache mod_rewrite R=404 before WP routing;
    FrankenPHP two-RE2-matcher respond) — a cloned `.git/`/`.env` in a
    served docroot was readable, and tunnels make docroots PUBLIC. Root
    `/.well-known/` stays exempt.
  - Provenance in v12 `site_git_assets` (+v13 `source`: cloned|adopted|
    linked — badge, RepoPanel, update-pull/watch seam). Collisions refused
    before any network traffic; a failed/cancelled clone removes only the
    dir it created.
  - **Linked assets + the unlink-only delete guard:** "Link folder" symlinks
    an existing checkout into wp-content (validated: no self-link, no
    docroot-containing cycle). `wp plugin|theme delete` would walk INTO a
    symlink and destroy the user's real checkout — so `wp_*_delete`
    partitions names on FILESYSTEM truth (`symlink_metadata`, NOT
    provenance — covers never-adopted manual links): symlinks are removed
    via `ShellRunner::remove_symlink` (which itself refuses non-symlinks),
    everything else goes to wp-cli; the active theme's link is refused.
    Delete confirms are status-driven (`loss_warning`: exact changed/
    untracked/unpushed counts; linked = calm "removes only the link").
    Watchers (`RepoWatches`) are session processes: process-grouped, die
    with the app, Restart-never-auto.
- **Autostart** (`AutostartManager`, macOS): per-user LaunchAgent
  `~/Library/LaunchAgents/dev.rexenv.rexenv.plist`, `RunAtLoad` — launches the app at
  login (not headless services; the edge still needs its `:443` prompt).

## 10. Verification pattern

Layer model + gate tiers: `docs/TESTING.md`. Claim inventory (the test metric):
`docs/CLAIM-LEDGER.md`. The pre-commit bar is `scripts/verify.sh` (lib tests +
example builds + clippy at zero + tsc — green ONLY from its own final line);
`scripts/verify-full.sh` adds the sandbox live-check tier + the WebKit harness.

- `cargo test --lib` in `src-tauri/` — unit tests on pure functions (539 and growing).
- Live checks = standalone `src-tauri/examples/*.rs` binaries (spawn real services,
  probe real ports) — the repo's convention instead of mocked integration tests.
  Each declares a tier in `scripts/live-checks.sh`; read the invariant in
  `examples/common/mod.rs` before writing one.
  They share the REAL app-data dir (cache + admin socket) by design, so
  `core::stack_guard` protects the user's running stack: a non-app process may
  stop only what it SPAWNED — adopted survivors, `proxy::stop_edge` /
  `recover_stale_edge`, and the orphan sweep are refused unless the process is
  the app (`mark_app_process` in `lib::run`) or opted in
  (`allow_real_stack_control()` / `REXENV_CONTROL_REAL_STACK=1` — only for
  deliberate stack-control utilities like `stack_stop`). Guard verified live by
  `examples/stack_guard_check`.
- Manual release gate: `docs/SMOKE-TEST.md` on a clean Mac.
- Work in small verifiable steps; one task at a time; commit per task; tick the item in
  `docs/TODO.md` with ✓ evidence.

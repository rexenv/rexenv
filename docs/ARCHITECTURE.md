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

**Module map** (file names; see README "Project structure" for the annotated tree):
- `core/`: adminer · binaries · blueprints · database (MySQL) · db (`DbEngine`
  abstraction) · dns · frankenphp · logs · mail · monitor · php · ports · postgres ·
  proc · proxy · service_manager · services · setup · site_metrics · sites · ssl ·
  terminal · tunnels · wordpress · wp_login · wp_tunnel
- `commands/`: blueprints · database · logs · mail · php · services · settings · sites ·
  system · terminal · tunnels · wordpress
- `state/`: app (AppState) · db (migrations) · models · store (repo)
- `src/routes/`: Sites · SiteDetail · Services · Databases · Mail · Tunnels · Settings ·
  Onboarding (1:1 with DESIGN_BRIEF screens)

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
                WordPress → MySQL :13306 (or PostgreSQL :15432)
```

- **No direct Caddy→php-fpm path for default sites.** Caddy is invisible plumbing.
- Local CA via `rcgen` (`core/ssl.rs`); per-domain leaf certs, wildcard SAN for
  multisite (`*.site.rex`). **Leaves must stay ≤398 days** — Safari/WebKit rejects
  longer ones even with the CA trusted. Caddy's auto-HTTPS/internal issuer is DISABLED;
  it serves our certs only.
- CA trust = **login keychain** (user op, `CertTrustManager`) — a detached-root osascript
  can't write System-keychain trust settings. `/etc/resolver/<tld>` files = root op
  (`PrivilegeManager`; onboarding installs the `.rex` backbone, other TLDs on first use). Hence ~2 setup prompts; true single prompt = SMAppService (deferred).
- **Per-site server override:** FrankenPHP (single static binary, embeds its own PHP)
  runs as a loopback backend on a per-site port in 8200–8299 (`core/frankenphp.rs`,
  FNV-1a of the domain), `auto_https off` + `admin off` — it must NEVER be the edge,
  bind `:443`, or expose an admin endpoint. The edge routes that site's Host to its
  backend; all other sites stay on the shared Nginx. `ServiceManager::reconcile_overrides`
  keeps backends in sync on start/reload.
- **Multisite** (`sites.multisite`: none/subdomain/subdirectory → `RewriteMode`): the
  config generator has three rewrite templates. Subdomain adds `mysite.rex, *.mysite.rex`
  to both the Nginx `server_name` and the Caddy host list over the wildcard-SAN cert;
  exact hosts always win, so a wildcard never shadows other sites.
- **Quote every path in generated Caddy/Nginx configs** — app-data paths contain spaces.
- **Per-site env vars (§1.6) never touch the shared pools.** Two delivery paths:
  Nginx sites get `fastcgi_param` lines in their server block (per-REQUEST);
  FrankenPHP overrides get config `env` lines **plus real process env at spawn** —
  per-site by construction, one backend process per site. Live-verified visibility:
  `getenv()`, `$_SERVER` **and `$_ENV`** all work on both servers — but for two
  different reasons, each with a footgun:
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

- `ServiceManager` owns the whole stack: DB engines, php-fpm pools, shared Nginx,
  per-site FrankenPHP backends, Mailpit, Adminer, edge.
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
  `start_database`, `set_php_version_installed`, `create_site`, `set_site_web_server`,
  `set_site_php_version`, `delete_site`. Add new binary-resolving commands to this list.
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
  extraction guards against path/symlink escapes, L2). Resolves stage + atomically
  publish so a failed prepare can't poison the cache (H4).
- MySQL is Oracle-signed (never re-sign) and needs a direct CDN URL + browser UA.
- The `php-debug` (Xdebug) variant is fully wired but returns `None` from `manifest()`
  until its checksums are pinned — see `docs/xdebug-debug-build.md`.

## 8. Data & app state

- **SQLite for all app state** (`state/db.rs`), `user_version` migrations, currently 4:
  v1 `sites` + `settings` · v2 `php_versions` registry · v3 `sites.multisite` ·
  v4 `blueprints` (JSON `spec`).
- `AppState` (`state/app.rs`) = db + platform + monitor + CA + ServiceManager + Terminals/
  Tunnels registries, **field-level locks** (see §5 locking rule).
- Every service start is gated by `core/ports::ensure_free`; a conflict names the holding
  process + a copy-paste free command.

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
  Any DB feature must use the bundled clients from the extracted MySQL tree with
  shell-free I/O — `--result-file` for output, stdin for input — never `wp db …`,
  never shell redirection (app-data paths contain spaces).
- **Tool results ≠ app errors:** `wp core verify-checksums` exits 0 even with
  "should not exist" extras (verified live) — verdicts derive from PARSED findings,
  never exit codes alone; extras triage as benign only when the basename is known OS
  noise (`.DS_Store`, `._*`, …), and unknown warnings stay loud.
- **Mail:** php-fpm `sendmail_path` (DOUBLE-quoted in the pool ini — the parser strips
  bare quotes and app-data paths contain spaces) → Mailpit's `sendmail -t -S
  127.0.0.1:11025` shim → SMTP sink; inbox UI reads the HTTP API on 18025 (`core/mail.rs`).
- **"Log in as"** (`core/wp_login.rs`): one-time, single-use, loopback-only magic link
  via a mu-plugin.
- **Tunnels** (`core/tunnels.rs` + `core/wp_tunnel.rs`): per-site cloudflared quick
  tunnel, scoped to ONE site Host, outbound-only. Behind the edge `REMOTE_ADDR` is always
  `127.0.0.1`, so loopback-only enforcement keys off `CF-*` headers + leftmost
  `X-Forwarded-For` + `Host`, never the IP. On tunnel start an auto-managed mu-plugin
  bakes in the public origin (HOST/HTTPS overrides + siteurl/home filters + output-buffer
  rewrite for plain/JSON-escaped/%-encoded); removed on stop; local requests untouched.
- **Adminer** (`core/adminer.rs`): internal vhost `adminer.rexenv.rex` on the shared
  stack — never a tunnel origin. Per-site deep link via a generated `index.php` wrapper
  (`adminer_object()` hook): passwordless login for loopback servers only, auto-submits
  Adminer's own CSRF-tokened + CSP-nonced form on `?rexenv_auto`.
- **Blueprints** (`core/blueprints.rs`): reusable presets (plugins/themes/WP_DEBUG/
  multisite) applied AFTER the one-click install.
- **Autostart** (`AutostartManager`, macOS): per-user LaunchAgent
  `~/Library/LaunchAgents/dev.rexenv.rexenv.plist`, `RunAtLoad` — launches the app at
  login (not headless services; the edge still needs its `:443` prompt).

## 10. Verification pattern

- `cargo test --lib` in `src-tauri/` — unit tests on pure functions (~172 and growing).
- Live checks = standalone `src-tauri/examples/*.rs` binaries (spawn real services,
  probe real ports) — the repo's convention instead of mocked integration tests.
- Manual release gate: `docs/SMOKE-TEST.md` on a clean Mac.
- Work in small verifiable steps; one task at a time; commit per task; tick the item in
  `docs/TODO.md` with ✓ evidence.

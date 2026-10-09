# ARCHITECTURE — how rexenv works today (as built, macOS)

The current-truth reference. Read this before any feature or bug task; it replaces
reading the codebase end-to-end. Line numbers are anchors, not contracts — trust the
file, verify the line. Ports + pinned versions: `docs/PORTS.md`. Open work: `docs/TODO.md`.
**Written against macOS's mechanisms.** The behaviour described here is common to every OS;
how Windows and Linux keep each promise (DNS route, edge, trust, prompts, autostart, IPC) is
`docs/PLATFORMS.md` §3, and a new feature walks that file's §4 checklist for all three.

rexenv = native, no-Docker local dev environment for web/WordPress developers.
Tauri 2 desktop app: Rust backend + React/TS frontend. Runs the whole stack — edge
proxy, shared web server, multi-version PHP, databases, one-click WordPress, local-TLD
DNS (`.rex` backbone, configurable) + auto-HTTPS, mail catching, tunnels — from one UI. macOS complete; Windows
shipping; Linux since 24 Sep 2026 (`docs/PLAN-linux-port.md` — every impl real and RUN on
the Ubuntu 22.04 VM through the installed `.deb`, self-update included, and the x86_64 build +
its swap and PHP checks on the Dell's WSL2 Ubuntu; the GUI rows in SMOKE-TEST are what remains).

## 1. Layering (non-negotiable)

```
src/ (React/TS)  →  typed IPC wrappers (src/lib/ipc/index.ts, never raw invoke)
        ↓ Tauri IPC
commands/   thin translators only — parse args, call core, map errors
        ↓
core/       platform-agnostic domain logic ("the what") — no OS-specific code, ever
        ↓
platform/   ALL OS-specific code, behind 13 traits (platform/traits.rs):
            DnsManager · CertTrustManager · PrivilegeManager · ProcessSupervisor ·
            AutostartManager · PermissionManager · ShellRunner · Paths · BinaryProvider ·
            EdgeSupervisor · DnsAgentManager · AppBundle · LocalIpc
```

- `platform/macos/mod.rs` — all 13 traits real. **`LocalIpc` is the 13th (12 Sep 2026,
  Windows port W1):** local non-TCP transport — a unix socket here, a named pipe with a
  current-user ACL on Windows (owner ruling D3). It only dials today (`proxy::admin_alive`,
  `dbsource::probe_socket`, which used to open `UnixStream`s inside `core/`); the CLI and
  MCP listeners move behind it in W8. It is also the one `Platform` accessor with a
  default — the build OS's implementation — so the dozen stub platforms in tests and
  examples did not each grow a field. `platform/windows/` — every unfilled
  method returns `Error::Unported`, or panics through `unported!` where the trait cannot return
  an error, and never `todo!()` (W3 step 0, ledger #595: a release build has no console, so
  `todo!()`'s message reached nobody); `platform/linux/` — every method `todo!()`.
  **Windows `Paths` and `PermissionManager` are written** (13 Sep 2026, W3; compile-checked
  from the Mac, not yet run): app data in `%LOCALAPPDATA%\rexenv\rexenv\data` (Local, never
  roaming — it holds binaries and datadirs; the namespace constants in `platform/mod.rs` are
  shared with macOS), and "owner-only" means a PROTECTED DACL with a single full-access entry
  for the process's user — no SYSTEM, no Administrators, nothing inherited — applied through the
  file handle, with `write_private` creating the file already carrying it (ledger #597).
  `BinaryProvider` there needs no relink and no signature: `prepare_binary` removes the Mark
  of the Web if present (reqwest downloads and our zip extraction are believed not to set it —
  unmeasured) and refuses any image x64 Windows cannot run (x64 and x86 pass; nginx.org's `nginx.exe` is
  x86) before it is published. Directory trees (`resolve_dir`: PHP, nginx, MySQL, PostgreSQL)
  go through `prepare_binary_dir`, the same check for every file — a no-op on macOS, where
  MySQL is Oracle-signed and must stay untouched (owner ruling 13 Sep 2026, ledger #598). The design goal is "adding an OS = filling stubs". **Measured 12
  Sep 2026 the tree did not meet it**, and ledger #163's scan was green anyway — it looked
  for OS module names, not Unix APIs. Windows port W1 (`docs/PLAN-windows-port.md`) closed
  the `core/` half the same day: its unix sockets, `kill`, `cfg(unix)` and `std::os::unix`
  moved behind `LocalIpc`, `ProcessSupervisor`, `PermissionManager` and `ShellRunner`, the
  scan now refuses them, and the app library and binary compile for Windows. **Still
  Unix-only, by design until W8:** the socket TRANSPORTS of `cli_server` and `mcp_server`
  (their request handling compiles everywhere). The `rex` client reaches the app's Windows
  pipe since W8 S1 (`cli/src/pipe.rs`, ledger #630); the app's pipe answers only `app.open`
  until S2. Still macOS-only
  DATA: the `macos-`/`darwin` URLs in `core/binaries.rs` (W2). And three mechanisms —
  php-fpm, `/etc/resolver`, unix-socket IPC — do not exist on Windows at all (plan §3).
- The only non-platform `todo!`-ish code is a defensive `unreachable!` in
  `core/binaries.rs`. `core/`, `commands/`, `state/` are macOS-complete.

**Module map** — the per-subsystem file/entry-point index is `docs/MAP.md`; every
module also carries a `//!` doc header stating its job. Quick inventory:
- `core/`: adminer · apache · binaries · blueprints · cli · confedit · confrewrite ·
  confverify · copy_scan · database (MySQL) · db (`DbEngine`) · dbcompat · dbdump ·
  dbimport · dbmirror · dbrestore · dbsource · devtools · dist_archive · dns · dotenv ·
  downloads · firefox · frankenphp · laravel · logs · mail · mariadb · monitor · php ·
  php_upstream · phpconf · ports · postgres · proc · proxy · redis · repo · scratch ·
  service_manager · services · setup · site_env · site_metrics · sites · ssl ·
  stack_guard · terminal · tld · tunnels · updates · valet · wordpress · wp_dns ·
  wp_login · wp_mailtag · wp_packages · wp_tunnel · wporg
- `commands/`: blueprints · database · db_import · downloads · logs · mail · mcp · php ·
  repo · rewrite · scratch · services · settings · site_provision · sites · system ·
  terminal · tunnels · valet_import · wordpress · wp_install
- `mcp_server.rs` + `mcp_server/`: feed · readctx · scratch · tools · view — the SECOND
  IPC surface (§8.3), and the only one that executes
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
                php-fpm pool 9774 / 978x (ONE pool per PHP minor, not per site)
                     ▼
                WordPress → the site's DB ENGINE: MySQL :13306 or MariaDB :13307
                (PostgreSQL :15432 backs Laravel / Blank PHP sites on PHP 8.1+ —
                WordPress never (`wpdb` speaks MySQL alone), and not on 7.4/8.0,
                whose builds have no working `pdo_pgsql`; both refused in core
                and the New-site dialog renders those refusals rather than
                restating them, ledger #543/#545/#549;
                per-site `sites.db_engine`, chosen at create, immutable after —
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
- **The admin dialog is rexenv's, not osascript's** (`platform/macos/prompt_applet.rs`,
  #577). SecurityAgent names the dialog after the asking process's bundle, so
  `/usr/bin/osascript` produced "osascript wants to make changes." over a plain lock —
  beside Local's branded prompt, the one a careful user refuses (owner, 12 Sep 2026).
  `run_privileged` now compiles the script INTO a throwaway applet bundle named
  `rexenv` carrying our icon (per-process `$TMPDIR/rexenv-prompt-<pid>`, 0700), opens
  it with `open -n -W`, and reads `ok`/`error <n>` back from a result file. The icon
  only shows once the applet's `Assets.car`/`CFBundleIconName` are gone (they outrank
  `applet.icns` — measured). The script is a literal in `main.scpt`, so root reads no
  file after approval. osascript remains the fallback ONLY when no dialog was shown
  (build/launch failed); an applet that ran and reported nothing is an error, never a
  second prompt.
- **Every admin dialog says what it is for** (`PromptReason`, #578): "rexenv wants to add
  a DNS resolver so .rex sites open on this Mac." / "… start its HTTPS server on ports 80
  and 443." / "… remove its system changes …" instead of the OS default "wants to make
  changes.", in the branded applet and the osascript fallback alike. The reason is a
  required typed argument of `run_privileged`, not a `&str`: beside the script, swapped
  strings would compile and run the sentence as root.
- **What a privileged step writes is on disk before its prompt returns** (`platform/durable.rs`,
  #740). On macOS and Linux `run_privileged` hands the shell `( script ) ; /bin/sync ; exit <the
  script's status>`, and the four boot/login files the app writes as the user (the login item and
  DNS agent plists, the XDG autostart entry, the DNS user unit) go through `write_durable` (temp,
  fsync, rename, fsync the folder). Measured 29 Sep 2026 on the Ubuntu VM: a unit written by a plain
  `cp` and a power cut 10 s later came back ZERO bytes — systemd reads that as masked, and the edge
  never started again until a Start all; written through the wrapper it came back whole. Windows
  needs neither — registry, NRPT, certificate store and Task Scheduler are the OS's own durable
  stores.
- **Stale rexenv CAs are swept, at trust time and at teardown** (`CertTrustManager::
  untrust_stale`, `keychain_trust::untrust_stale`, #678, 18 Sep 2026). Every fresh app-data
  folder mints a NEW local CA and trusts it; the ones earlier folders trusted stayed trusted
  roots for private keys that no longer exist — four of them in one VM's login keychain
  after four hand "resets", and "Remove rexenv's system changes" untrusted only the current
  one. Now `ssl::trust_ca` sweeps the others best-effort right after the trust dialog (its
  authorization is still warm, so in practice no extra prompt; a cancelled sweep never
  fails the trust), and `ssl::untrust_ca` sweeps them as part of the uninstall. The sweep
  finds them by subject (`ssl::CA_COMMON_NAME`) through `security find-certificate`, makes
  the trust change in-process (the dialog is titled after the caller, #579), and deletes
  the keychain item. Windows sweeps the same way since 18 Sep 2026
  (`platform/windows/cert_store.rs::untrust_stale`), with two differences worth knowing
  before you touch it: CryptoAPI can only search the Root store by subject SUBSTRING, so the
  hits are CANDIDATES and an exact common-name match (`cert_rules::is_rexenv_ca`) decides
  what may be deleted — "rexenv Local CA Inc", someone else's certificate, would otherwise
  be swept; and there is no warm authorization to ride, so Windows raises its own
  confirmation per certificate — MEASURED on a real store 19 Sep 2026, two dialogs for the two
  changes, so four stale CAs there mean four. The live check is
  `examples/windows_stale_ca_sweep.rs` (system tier, Windows): it trusts a throwaway CA from the
  production generator beside the machine's real root, names the REAL one as `current` — a pair of
  throwaways would make the machine's own root a third certificate for the sweep to delete — and
  reads the store back through PowerShell rather than through the module under test.
- **The keychain (CA trust) dialog is rexenv's too** (`platform/macos/keychain_trust.rs`,
  #579). It is titled after the process that calls the trust-settings API, so
  `security add-trusted-cert` read "security" — and running that same command inside
  the rexenv applet changed nothing (measured). rexenv now calls
  `SecCertificateAddToKeychain` (login keychain, no dialog) +
  `SecTrustSettingsSetTrustSettings`/`RemoveTrustSettings` (user domain, the dialog)
  itself: the packaged `rexenv.app` gets its name and logo, a bundle-less `cargo run` its
  executable name. A dismissed dialog returns -60006 and reads as a cancel. Cancel still
  leaves the CA imported, untrusted — as `security` did.
  **Untrust also deletes the item** (#813): "Remove system changes" removes the trust and
  then `security delete-certificate -Z` every copy of the current CA from the login keychain —
  until 9 Oct 2026 it removed only the trust and left an inert `rexenv Local CA` item that the
  cask's `--zap` cannot reach (found by the first `--zap`, 15.8 VM). Windows and Linux already
  take the certificate out of their stores in the same call.
- **Firefox keeps its own trust store** (`core/firefox.rs`, #154): rexenv forces
  `security.enterprise_roots.enabled` in each profile's `user.js` so Firefox imports the OS roots
  (default on only from Firefox 120). Only the profiles ROOT is per-OS
  (`CertTrustManager::firefox_profiles_root`): `~/Library/Application Support/Firefox` on macOS,
  `%APPDATA%\Mozilla\Firefox` on Windows (#612; the Store build's virtualized copy is not looked in).
  `profiles.ini` is read in whatever encoding it was saved — the Dell's is **UTF-16LE with a BOM**
  (the Mac's is plain UTF-8), and read as UTF-8 it yielded no profile, so Settings called Firefox
  not installed and the trust step wrote nowhere, silently.
- **A typed bare `name.rex` is a SEARCH in every browser by default** (#706): the address bar
  decides URL-or-search against the public TLD list BEFORE any DNS lookup, so the query never
  reaches rexenv's resolver — while `name.rex/` or `https://name.rex` opens. Nothing server-side
  can change that. Firefox has a per-suffix pref, `browser.fixup.domainsuffixwhitelist.<tld>`,
  and `core/firefox.rs` writes it into the same `user.js` when a TLD's route is installed
  (`dns::configure_resolver`), on the CA trust (`ssl::trust_ca`, every TLD rexenv answers on)
  and from Settings (a Firefox added later). Best-effort like the trust pref; a `false` the user
  wrote is theirs and is never flipped. Chrome, Safari and Edge have no such setting, so the UI
  says "add a slash" instead — Settings' default-TLD card and the SiteDetail domain's tooltip,
  naming the browsers from `PlatformWords::searching_browsers` (Safari on macOS, Edge on Windows).
- **CA trust on Windows = this user's Root certificate store** (`platform/windows/cert_store.rs`, #613):
  the CurrentUser `Root` store, never LocalMachine's (per-user, no elevation — the login keychain's
  counterpart). The add and the delete make WINDOWS ask the user ("Security Warning: You are about to
  install a certificate…"), and the store call waits on the answer, so callers go through
  `while_prompting` exactly as on macOS; a No reads as a cancel. `is_trusted` is a prompt-free lookup
  in the logical store (machine roots count). With no interactive desktop the add fails at once with
  0x32 — measured over SSH — and is worded as that, not as a raw number. Chrome and Edge read this
  store; Firefox reads it through `security.enterprise_roots.enabled` (above) — measured end to end
  (#614): Edge 153, Chrome 152 and Firefox 105 rejected a `.rex` site before the CA was in the store,
  accepted it once it was, and rejected it again after it was removed. The prompts are Windows'
  own dialogs inside rexenv's process, and they name the CA, not the app: "Security Warning … claiming to
  represent: rexenv Local CA … Thumbprint (sha1)" to add, "Root Certificate Store — Do you want to
  DELETE the following certificate from the Root Store?" to remove (measured; unlike macOS, there is no
  API to title them). So the CA's subject, `rexenv Local CA`, is the only name a Windows user sees there.
- **Per-site server overrides** (`OverrideKind` in the manager — one seam, three kinds;
  OpenLiteSpeed dropped in as the one new arm the seam promised, 4 Oct 2026):
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
  - **OpenLiteSpeed** (`core/openlitespeed.rs`, macOS + Linux; rexenv's OWN build from
    `rexenv/runtimes`, six patches — `docs/archive/PLAN-openlitespeed.md`): loopback backend on
    8400–8499. Like Apache, NO PHP of its own — `.php` goes to the site's shared pool
    (`extProcessor … type fcgi … autoStart 0`). The reason to pick it is LSCache (the
    module ships ON, so the LiteSpeed Cache plugin works as on a LiteSpeed host) plus
    `.htaccess`. The binary is shared from the cache; each site has its OWN server root
    under app-data (`openlitespeed/<domain>/`, passed as `LSWS_HOME`), holding the ONE
    generated `conf/httpd_config.conf` (vhost inline, so the reconcile diff sees every
    input), `run/` (`LSWS_TMP_DIR` — pid, swap, status; never the machine-wide
    `/tmp/lshttpd`), `shm/` and `lscache/`. `disableWebAdmin 1` and `noRemoteFetch 1`
    (no admin console, no release check, no quic.cloud). Per-site env vars reach PHP as
    request params through rewrite `E=` flags (single- or double-quoted, `\` and `%`
    escaped); a value with BOTH quote characters cannot be written and is refused when
    saved for, or switched to, an OLS site. **Routing steps aside for a site's own
    `.htaccess`**: OpenLiteSpeed runs vhost rules BEFORE the directory's, so a vhost
    front controller rewrote every missing path to `/index.php` and a plugin's
    `.htaccess` redirect never saw its path — with a root `.htaccess` the site routes
    itself (WordPress/Laravel ship one, as on any LiteSpeed host), without one the vhost's
    per-mode rules route it; dotfiles 404 BEFORE either. **OpenLiteSpeed reads a
    directory's `.htaccess` ONCE** — a file created later, or an edit, is not seen until a
    restart. So the step-aside is decided when the config is rendered (from the file the
    server will load), the root `.htaccess`'s fingerprint (size + mtime) is rendered into
    the config, and the health watchdog restarts an OLS backend whose config no longer
    matches the one it loaded — within one tick of WordPress's permalink save or LSCache's
    settings save (ledger #783). Subdirectory `.htaccess` edits need `rex site restart`.
    **Process shape:** started with `-n` (no daemon, crash guard on): the event loop stops
    itself when ITS parent changes, so under `-d` every OLS site died ~1 s after the app
    quit; under `-n` the loop runs in a worker forked by the main (ledger #784).
    **Ownership:** OpenLiteSpeed overwrites `argv[0]`, so the app-data marker is passed
    twice after `-n` (macOS `ps` drops the last argument once the overwrite splits
    argv[0]); the worker's title loses it, so `owned_master` resolves to the main — the pid
    rexenv holds. **Adoption needs the real per-site state:** the launch loads the
    manager's env/alias/ini mirrors (`commands::services::load_mirrors`) before `adopt_startup` (ledger #785) — before, the OLS
    watchdog restarted an adopted site without its env vars. **Windows: refused** — no OpenLiteSpeed
    build exists there; the refusal is `PlatformWords::openlitespeed_unavailable`.
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
  backend — different port + binary). A server with no pin on this OS (Apache and FrankenPHP
  on Windows; OpenLiteSpeed on Windows, permanently) is refused in CORE at create AND switch
  (`ensure_server_available_on`, through `binaries::ships_on`).
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

### Stopping ONE site (v44, `docs/archive/PLAN-per-site-lifecycle.md`)

Read the topology above and the question answers itself: a default site has **no
process of its own**. The web server is one shared nginx, and the php-fpm pool is
shared by every site on that PHP minor. So the two obvious implementations of
"stop this site" are both wrong — stopping the pool stops every site on that
version (which is why `restart_site`'s `--pool` is opt-in and reports how many),
and stopping nginx or the edge is "Stop all", which already exists.

**Stopping a site is therefore a change to the SERVING SURFACE, not to any
shared process.** The recorded switch is `sites.enabled` (v44, user-owned — one
writer, `store::set_site_enabled`), and the config rebuild reads it:

| Leg | A stopped site |
|---|---|
| nginx server block | no SERVING block (`sites::gets_nginx_block`) — a **stopped block** in its place: `return 503` on every path, the stopped page via `error_page`, no `fastcgi_pass` |
| Caddy route | **kept**, with its `tls` line and certificate — answers 503 with rexenv's own stopped PAGE (`core/stopped_page.rs`) instead of proxying |
| its OWN override backend (FrankenPHP/Apache) | actually stopped: `OverrideKind::wanted_by` returns `None`, and `reconcile_overrides` stops what is no longer wanted |
| shared nginx, php-fpm pools, DB, edge | untouched |

Two decisions worth keeping:

- **The 503 is a page, not a line of text.** The reader is a developer whose own
  site stopped loading, and a bare `respond` line reads like a server that fell
  over — the one thing this response must not say. `core/stopped_page.rs` renders
  rexenv's mark, palette and both ways back, and is WRITTEN to the config dir
  because Caddy cannot `respond` with a file and a Caddyfile string cannot hold
  CSS (every `{` would be a placeholder). The edge serves it through
  `error 503` + `handle_errors { rewrite; file_server }`, which keeps the status.
  **One page PER SITE, with the domain baked in.** The first cut shared one file
  and read `location.hostname` in the browser; over a tunnel that is the
  trycloudflare host, so the page announced a name that is not the site and
  offered `rex site start <tunnel host>` — a command nobody can run (ledger
  #512). A page rendered by the server must not ask the client what it is about.
  Each stopped site's directory is keyed by site id, written by the same rebuild
  that writes the configs (so a rename lands for free), and swept when the site
  starts again.
- **A stopped site still answers in nginx, and that is the correction this
  design needed.** The first version emitted no block at all, reasoning that the
  503 belongs at the edge. **nginx answers a name it has no block for from its
  DEFAULT server — another site** — and the edge is not the only way in: a public
  tunnel proxies straight to the shared nginx with `--http-host-header`. Sharing
  a stopped site therefore published a NEIGHBOUR's site to the internet, found
  live by the owner within a day (ledger #511). The rule that replaced it:
  **every name rexenv knows must answer for ITSELF in every tier that can be
  reached directly.** An "absent" is not a behaviour — ask what the tier does
  with a request it cannot match.
- **The route stays.** Dropping it hands the browser a TLS failure — or, next to
  a subdomain-multisite block, somebody ELSE's site at this site's address (the
  fallthrough `override_fallthrough_check` measured). A 503 that says the site is
  stopped is the honest answer, and it keeps the certificate warm so starting the
  site again is a reload rather than a name the browser has never been given a
  cert for.
- **The switch is in the DATABASE**, not the ServiceManager, because services
  outlive the app: a site the user stopped must still be stopped after a
  relaunch, and the rebuild is reached from start, reload, startup adoption, site
  edits and the scratch reaper.

Starting is the inverse plus one thing: with the stack up, it ensures the site's
PHP minor pool. It never starts the stack — a user who stopped everything meant
it — so the report carries `serving` (read from `site_serving`, the ONE status
derivation) and the reason when a started site still is not answering. Status
carries `disabled` beside `serving` for the same reason: "the stack is down" and
"you stopped this one" have different fixes, and one word for both sends people
to start a stack that is already running. A site whose setup never FINISHED is a
third answer and never `serving` (no vhost, so the edge refuses its name — the
derivation requires `provisioned`, #748); `rex site info` renders the MCP's full
classification (#200) rather than that boolean.

**All of them at once** is the same mechanism batched: `set_all_enabled` writes
every row, then does ONE config rebuild and ONE reload — a per-site loop meant N
reloads and N chances of a half-applied state — and skips (counting) sites whose
setup never finished, so one half-built site cannot fail the action for the rest.
It performs no scratch promotion, because a bulk action is not a decision about
any one site.

Reachable from the Sites row menu and the site page, the Sites page's "All sites"
menu, `rex site start|stop <domain>` (`--all` for every site), and
`site_configure {action: "enabled"}` / `stack {action: "start_sites"|"stop_sites"}`
(`manage`, not `system`: no service is touched and no password dialog comes) (existing `manage` scope —
through the MECHANISM, never the Tauri command, which promotes a scratch site).
Live-proven end to end by `site_stop_start_check`.

## 3. Caddy edge lifecycle (`core/proxy.rs`)

- Edge binds real `:80`/`:443` → needs one privileged install (foreground admin prompt —
  a backgrounded osascript can't show the dialog).
- **The edge runs under an OS supervisor that keeps it alive — `CaddyHandle::Daemon`**
  (`EdgeSupervisor` trait; macOS = a **root LaunchDaemon**, `dev.rexenv.rexenv.edge`,
  `KeepAlive = {PathState: <root-owned ON switch>}`, no `RunAtLoad` — #773, 3 Oct 2026; it was
  `KeepAlive=true` + `RunAtLoad=true` before). While the switch exists launchd relaunches the
  edge on ANY death — external SIGTERM, crash, sleep/wake, logout, reboot — and starts it at
  boot; with the switch down (Stop all) it leaves it alone, across reboots too. So the edge is the one service
  that recovers WITHOUT the health watchdog (which can't clear a privileged start's
  prompt). `proxy::start_edge_daemon` stages the plist + launcher-wrapper CONTENTS
  unprivileged, then runs ONE privileged `install_command` — which, when the binary, the
  wrapper and the plist on disk are byte-identical and the label is loaded, is `enable` +
  `launchctl kickstart -k` (re-execs caddy so it re-reads the current Caddyfile, and registers
  nothing new with Background Task Management — #772, 2 Oct 2026: every `bootstrap` had been a
  fresh registration and a "Background Items Added" card, eleven on the 15.8 VM), and otherwise
  the `cp` into the root tree + bootout → wait → `launchctl bootstrap system` (#744). (Until 2
  Oct 2026 this paragraph described a lighter `start_command` path that nothing called: Start
  all always reinstalled.) **Security: the daemon executes a `root:wheel 0755` COPY of caddy under
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
  re-prompt, sites never drop. An edge the manager ALREADY tracks gets the same reload: until
  9 Oct 2026 a live tracked edge returned "up" without one, so a Start all run by a
  site create or import while only the boot edge was running (Partial 1/N) left the
  new site with no route and no certificate on the edge while its card said Running
  (#806, seen on the 15.8 VM; TEXT-guarded). A tracked edge keeps its handle; one
  that refuses the reload is replaced by the fresh-start path. The health watchdog heals the reverse way too: an edge
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
- **A slow edge is not a foreign proxy (#811).** The watchdog's wire probe is read finer than
  `edge_wire` (`proxy::WireReading`): only a response WITHOUT our marker (or nothing listening)
  counts toward "blocked"; a poll that completes no exchange in time feeds its own one-minute
  debounce and an own-edge sentence. And no copy-paste command is offered for a holder it could
  not name — our own edge is always one of the listeners there, and on macOS the unnamed-holder
  fallback is a blanket `sudo kill` that took rexenv's root caddy with it (seen on the 15.8 VM
  under a 1 GB database copy, 9 Oct 2026).
- **Explicit stop costs one prompt, and lowers the switch FIRST.** With `KeepAlive` a graceful
  `caddy stop` is instantly relaunched, so Stop-all goes through the supervisor's own switch:
  `stop_services` runs `proxy::stop_edge_daemon` whenever the handle says Daemon OR the daemon is
  installed and enabled (`edge_daemon_needs_stop`, #790 — the handle lagged a slow boot once) (privileged `rm <flag>` + a bounded `kill TERM`
  loop — the job stays LOADED, so the next Start all is a `touch` + `kickstart` and Background
  Task Management hears nothing, #773; a plist from before the switch is still `disable` +
  `bootout`, since `kill` alone is respawned there — measured), OUTSIDE the services lock (M4)
  BEFORE `stop_all` touches any manager state — while the
  auth prompt sits open the handle stays truthful (`Daemon` && alive), so the watchdog
  cannot re-adopt a doomed edge mid-stop (the race that once left a stale `Daemon`
  handle: edge-restarting spam + Start-all skipping the edge). A cancelled prompt stops
  nothing (all-or-nothing). `stop_all` then skips the admin-stop for a `Daemon` edge
  (already down); the lowered switch keeps it down across reboots until the next Start-all
  (whose `install_command` raises it — and `enable`s, for a label a pre-#773 stop disabled).
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
  (`dev.rexenv.rexenv.dns`, `KeepAlive` + `RunAtLoad`) whose program is
  `<app data>/dns-agent/rexenv <app binary>`: a launcher script named after the app
  (Login Items shows the program's file name — a `/bin/sh -c` read as "sh", #764) that
  execs `<app binary> --dns-agent` once the bundle's signature verifies (#761; headless:
  no Tauri/SQLite/services; `dns::run_agent`). Rationale
  (observed live): the data plane outlives a quit, but the OLD in-process resolver died
  with the app — sites coasted ~1h40m on client caches/persistent connections, then went
  dark until relaunch. All unprivileged (`~/Library/LaunchAgents`, high loopback port;
  `launchctl load/unload -w` — no prompt). The agent never exits on a busy port: it
  retries every 10s, so an old in-process holder hands off seamlessly.
- **On Windows the resolver is on `127.0.0.1:53`** (#615, W6 S1): `DEFAULT_DNS_PORT` is
  `platform::RESOLVER_PORT` — 15353 on macOS, where the resolver file names a port, 53 on Windows,
  where an NRPT rule cannot (plan §3 D2). The socket comes from `platform::bind_resolver_udp`: a plain
  loopback bind on macOS; on Windows `SO_EXCLUSIVEADDRUSE` (plan §3 D2's ruling) and UDP
  connection-reset reports off — both kept as guards, neither shown load-bearing: planted out, a
  same-account `SO_REUSEADDR` bind was still refused and twenty vanishing clients still left the agent
  answering. `DnsService::start_default` BINDS FIRST and only names a refusal (`ports::refused_bind`): the
  table-first gate (#599) would refuse every Windows machine with WSL 2, where ICS holds UDP
  `0.0.0.0:53` for good and the exclusive loopback bind succeeds and answers beside it. The refusal names
  the socket that caused it — for address-in-use only a LOOPBACK row
  (`ProcessSupervisor::port_conflict_help_on`) — because the unfiltered help blamed ICS and offered
  `Stop-Service SharedAccess` as the fix while the real holder sat on `127.0.0.1`.
- **On Windows the agent is a logon Scheduled Task** (`\rexenv\dns-agent`, #616, W6 S2), registered by the
  user with `schtasks` — no elevation (measured) — from `windows/logon_task.rs`'s XML: this user's logon
  trigger plus a time trigger repeating every minute, `IgnoreNew`, `InteractiveToken`, no execution time
  limit, no battery stops, hidden. The repeating trigger is the keep-alive (owner's ruling): Task
  Scheduler's `RestartOnFailure` did not restart a killed action (measured), so a dead agent comes back on
  the next tick instead. `install` skips an unchanged definition (a fixed past `StartBoundary` keeps the
  bytes stable, as the plist is on macOS), otherwise re-registers, ends and runs; `kickstart` ends and
  runs; `uninstall` ends, deletes, and removes rexenv's copy. Task Scheduler gives the action no stdout or
  stderr, so the definition passes `--dns-agent --log <path>` and `main.rs` points both handles at that
  file first (`platform::send_output_to`). The trait's methods are `definition_path`/`definition_contents`
  — the plist on macOS, the task XML on Windows. An interactive-token task runs only while the user is
  logged on: the agent is up from logon, not from boot.
- **A TLD's route is the platform's, not a file the core reads** (`DnsManager`, #617, W6 ruling R1):
  the core asks `route_owner` (Absent / Ours / Foreign, by rexenv's exact signature), `our_route_tlds`,
  `foreign_route_tlds`, `route_label` (the words a message uses) and `route_contents` (what a takeover
  preview shows beside theirs), and gets privileged command strings — it no longer reads
  `/etc/resolver/<tld>` or scans that directory itself. macOS answers exactly as before, from its resolver
  files (`platform/resolver_files.rs`: `owner_of`, the two signature scans, and their tests). **On Linux
  "Ours" also needs the route LIVE** (#741): the marker and the current script, AND `resolvectl status
  rexenv0` showing a DNS scope, our server and `~tld` — the link's settings are bus calls resolved can
  lose (29 Sep 2026 something reverted them after a re-setup while status said "resolver installed"),
  and a lost route reading `Absent` is what sends the app to its setup step to re-apply it. The file
  and the NRPT rule need no such check: on macOS and Windows they ARE the live state.
  **And liveness, not the shape on disk, decides "installed"** (#769, 2 Oct 2026): 0.8.11 read 0.8.10's
  unit as not installed while the route routed, and opened Welcome over an install with sites. A
  script or unit that is not this build's is `DnsManager::route_notice` — a sentence beside
  "installed" in Settings and `rex status`, re-applied with one polkit by the repair verb
  (`core::dns::repair_resolver`: installed / re-applied / unchanged) — and `ensure_resolver` leaves a
  working route alone, so an update never prompts on its own; only a machine without `resolvectl`
  keeps the older rule (an older script reads Absent).
- **On Windows a TLD's route is an NRPT rule** (#618, W6 S4): `WindowsDns` reads every rule from the
  registry (`DnsPolicyConfig`, local and Group Policy — readable without elevation, measured) and
  `windows/nrpt_rules.rs` decides. Ours is exactly: a local rule, comment `rexenv`, the one namespace
  `.<tld>`, the one server `127.0.0.1` (NRPT has no port — hence the resolver on :53); any other rule naming
  the TLD makes it Foreign, and the content a takeover backs up is those whole rules as JSON. A rule takes
  effect at once (measured). One rule may name several namespaces, so `install_command` removes only
  `.<tld>` from another rule (removing the rule only when nothing else is left in it) before adding ours,
  and `restore_command` reads the backup file (never pastes it into the script) and sets the namespaces
  back on the SAME rule when it still exists, adding it again only when it does not. What `WindowsDns`
  hands `PrivilegeManager` is rexenv OPS (`nrpt-install test`, `nrpt-remove …`, `nrpt-restore …`), not
  PowerShell — the script is built on the far side of the elevation (#619).
- **Privileged steps on Windows** (`platform/windows/elevation.rs` + `elevation_rules.rs`, #619, W6 S3; owner's
  rulings R2 and 15 Sep 2026): `run_privileged` first parses the ops and refuses anything else before any
  dialog; then rexenv's own message box, titled rexenv, says the reason (`PromptReason::sentence`) and that
  Windows asks next; then `ShellExecuteExW` with the `runas` verb starts `rexenv.exe --elevated-step
  <base64 ops> "<result>"` — UAC names the program it elevates, so the prompt says rexenv — and waits for
  it. The elevated process (dispatched first thing in `main.rs`) accepts only rexenv's ops with valid TLD
  labels, reads restores only from rexenv's own `resolver-backups`, writes its result only to a
  `rexenv-elevated-*.txt` directly in the user's temp directory, and builds the PowerShell itself. Why ops
  and not a script: any process on the machine could otherwise start `rexenv.exe --elevated-step <its own
  script>` behind a UAC prompt that says "rexenv". A Cancel or a No reads as the cancel wording.
- **App launch = adopt-or-install-or-fall-back** (`lib.rs`): probe
  `dns::answers_as_ours` (a REAL A query must return `127.0.0.1` — ownership AND
  liveness, H2 — never a bare port probe); refresh the plist every launch so it tracks
  the current binary (dev ↔ installed hand off); if the agent can't come up, fall back
  to the legacy IN-PROCESS task (`DnsState { service, mode: Agent | InProcess | Down }`)
  so DNS never regresses — Settings surfaces the degraded mode (`dns_status.mode`).
- **The agent SAYS which build it is, and a stale one is kickstarted** (`_build.rexenv-agent.rex`
  TXT, ledger #533). Refreshing the plist is not enough, and the gap was invisible: after any
  bundle replacement — a self-update, or a user dragging a new copy over the old one — the
  path did not move, so the plist is byte-identical and `install` skips the reload, while the
  watchdog only kicks when the probe FAILS, which it does not, because the OLD binary answers
  A records perfectly from an inode that no longer has a name. So the agent answers one TXT
  name with `<version> <commit>` (TTL 0, loopback like everything else, nothing about the
  machine), the app asks at launch, and a mismatch — **including no answer at all, which is
  what every rexenv shipped before this does** — is `kickstart -k`ed in place. In place, never
  unload/load: a reload re-registers with Background Task Management and macOS posts an "App
  Background Activity" notification each time. Costs a sub-second `.rex` resolution gap.
- **A lost startup race is undone afterwards** (`spawn_dns_handoff`, ledger #442). The
  fallback above used to be unreachable in practice: the app was launched by hand long
  after login, when the agent already answered. **Start on login made both start
  together**, the 2s probe is less than a cold agent needs (it is the whole binary
  booting into `--dns-agent`), and once the app holds the port the agent — retrying every
  10s — can never win it back, so DNS dies with the app for the whole session. Measured
  on a real login, 1 Sep 2026. While in-process, the app therefore tries to GIVE the port
  back: release it, `kickstart` the agent so its bind happens now rather than on its own
  cadence, probe, and rebind in-process if it did not take — and a rebind that fails
  because the agent took the port just after the probe window is read as the late
  success it is, not latched as `Down` — and `Down` itself is a snapshot the watchdog
  re-probes every poll, never a verdict: on the macOS 13→15 upgrade run (23 Sep 2026) the
  agent held the port without answering yet at the one instant that re-probe fired, and an
  empty `Down` arm then reported a dead resolver for two hours while `.rex` resolved (#442
  leg 4). The watchdog's own fallback into in-process
  mode spawns the same handoff (it was the second door, and had none until 3 Sep
  2026). The release AWAITS the aborted task, so the port is free when the agent is
  kicked, not "soon". Bounded (5 attempts) and loud
  on giving up. A longer startup wait was rejected: it is a guess that taxes every launch
  and still loses on a slow one — the race is not something to win, it is something to
  undo.
- Health watchdog (`lib.rs`), mode-aware and **bounded to 3 attempts**: Agent → wire
  probe, dead agent gets a `launchctl` kickstart, and after 3 failed kicks ONE in-process
  fallback (sites resolve now, mode says it won't survive quits); InProcess → restart the
  task in place. Emits `service-health` events. A resolver that never started (port
  conflict at launch) is NOT auto-restarted — surfaces in Settings. Teardown
  (`run_system_teardown`) also uninstalls the agent, and the command then sets
  `DnsMode::Removed`, the one mode the watchdog never probes: without it the clean-15
  smoke (23 Sep 2026) watched the app kick the uninstalled agent three times and then
  serve DNS in-process — resurrecting what the user had just removed (#715). `Down` is
  different — it means we FAILED, and is re-probed every poll.
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
- **A cloned repository is judged at the clone phase, before any database** (#796, #802): its
  `require.php` against the site's minor, and every `ext-*` it or a locked package requires
  against the site's REAL PHP (`php -m` — the same on every OS, no list to keep in step). Each
  refusal names the fix (a minor to switch to; the missing extension and who requires it).
  `deps` asks both again, at the step that needs the answer.
- **Start all refuses up front when a runtime the servers need is missing**
  (`BinaryProvider::runtime_problem`, #798 — Windows' Visual C++ Redistributable; `None` on macOS
  and Linux). `start_stack`'s first step, so the refusal names the runtime and Microsoft's link
  before any download or spawn, rather than MySQL's exit code half-way through. The same verdict
  is the Services screen's banner (re-asked every 30 s — the fix happens outside rexenv) and
  `rex doctor`'s Runtime line. An OLDER runtime is a warning, never a block.
- **Stop all is refused while a site is being provisioned** (`stack_guard::stop_refusal`, ledger
  #795, 8 Oct 2026). Every Stop all — footer, tray, `rex stop`, `rex restart`, the MCP stack tool —
  goes through `stop_services`, which asks `ProvisionJobs::running_domains` first and stops nothing
  while one runs, naming the site and the way out (wait, or Cancel on its card). Why: a user's
  `rex restart` mid-provision stopped MySQL under `artisan migrate` (`1317 Query execution was
  interrupted`), the migration was left half-applied, and every Retry then died `1050 Table …
  already exists`. Start all needs no guard: it skips a running engine (`spawn_db`).
- **"Restart this site" has no single meaning, and the API says so**
  (`restart_site_backend` → `SiteRestartOutcome`, 2 Sep 2026). The default topology
  gives a site NO process of its own: shared Nginx, and one php-fpm pool per PHP
  MINOR. So only an override site (FrankenPHP/Apache on its loopback port) has
  something to bounce — it is stopped and respawned on its RECORDED port, so the edge
  route it already has still points at it. A default site gets its config rebuilt and
  the web tier reloaded, which is what actually makes it pick up a change, and the
  answer names the pool it shares. The third outcome is a REFUSAL: an adopted backend
  a non-app process may not stop is reported, never silently skipped. Restarting the
  pool would stop every other site on that minor, so it is opt-in (`rex site restart
  <domain> --pool`) and the report carries how many sites that covers whether or not
  the flag was passed — the number is what makes the flag a choice instead of a dare.
- **A site can answer on more than one hostname** (v42 `site_domains`). The primary
  stays on the site row and the extras are aliases; a hostname reaches exactly ONE
  site, and the half SQL cannot express (an alias equal to some site's primary) is
  refused in `core::sites::validate_alias`. Serving is deliberately singular
  everywhere: one nginx `server_name` list on one server block, extra addresses on
  the site's own Caddy block, ONE certificate covering every name — and the cert
  cache compares the recorded NAME SET, because "the files exist" leaves a valid
  certificate for yesterday's names while the browser shows an interstitial on the
  one just added. A subdomain network's alias carries the wildcard on both tiers.
  Every path that ISSUES a cert carries the alias list — first issue, domain
  change, and both Regenerate actions (the last two forgot, and a reissue that
  covered the primary alone left the recorded set describing the wider one, so
  the narrow cert was judged covered forever; fixed 3 Sep 2026). Anything that
  RECORDS an alias outside the add command (the Valet import) must refresh the
  manager's alias mirror and reload, because configs regenerate from the
  mirror, not the table. A resolver file is never removed automatically when the
  last name on a TLD goes (housekeeping must not prompt for a password); `rex tld
  --remove <tld>` is the explicit, ours-only, not-in-use-only verb (3 Sep 2026).
  An extra domain on a WordPress site REACHES it and then redirects to the
  primary — WordPress owns its canonical address (`siteurl`), and rexenv does not
  rewrite it. The card and `rex site domains` say so rather than leaving the
  redirect to be discovered.
- **The CLI socket streams progress, but only to a client that asked**
  (`Progress`, `stream: true`, 2 Sep 2026). The handler runs in its OWN task, so a
  client that hangs up mid-stream does not abandon the command half-way (it did,
  until 3 Sep 2026 — `| head -1` on a multisite create left the convert unrun), and a
  panicking arm is answered with an error envelope rather than a closed socket. The
  framing is: one request line in,
  zero or more `{"progress": …}` lines, then EXACTLY ONE `{"ok": …}` envelope,
  always last. Opt-in is the compatibility hinge — an older `rex` reads one line
  and treats it as the reply, so a server that streamed unasked would hand it a
  progress record as the result of the command. Records are identified by KEY,
  not by position, so a client can skip ones it does not understand and still
  know which line ends the exchange. Commands report progress unconditionally
  (`Progress::none()` discards), so no command has to know who is asking. The
  first consumers are `site.create` and `site.retry`, which poll the SAME
  provisioning state the app's card renders rather than inventing a second
  progress channel that could disagree with the first.
- **The web tier has no single-service STOP, only restart** (`WebTarget`,
  `restart_web_service`, 2 Sep 2026). One nginx serves every default site, one pool
  every site on a PHP minor, one edge everything — so "stop nginx" is every default
  site 502-ing with nothing on screen to explain it, and stopping the stack is the
  honest way to stop serving. A restart always regenerates the config first (coming
  back up on the config you already had is the state a restart is meant to escape),
  a service that is NOT running is reported rather than started (order lives in
  `start_all`: pools → nginx → edge), and the edge is RELOADED — it is a root
  KeepAlive daemon whose stop is a privileged `disable` + `bootout` with :443 dark
  in between.
- **The APP outlives the window (menu-bar app).** Closing the window hides it —
  `prevent_close` + `hide`, confirming nothing, because it stops nothing. The process
  is what the CONTROL plane lives in: `rex` and the MCP server are remote controls for
  a running app, their `0600` sockets are opened in `setup()` and die with it, so a
  quit used to end every agent session and every CLI command while the services below
  carried on. A quit is now reached through the tray's **Quit rexenv** or the app
  menu's Cmd+Q — both a bare `app.exit(0)`, because the share confirm lives on
  `RunEvent::ExitRequested`, the one gate every quit raises. The Cmd+Q item is CUSTOM
  for that reason: the predefined one is `terminate:`, which never raises the event
  (it bypassed the gate until 3 Sep 2026). **The dock follows the window** (`dock_follows_window`):
  `ActivationPolicy::Regular` while a window is up, `Accessory` the moment it closes, so
  the status item is the whole presence when no window is — and a visible window still
  has a tile to Cmd-Tab to. Ordering is load-bearing on both edges (Regular → Accessory
  hides windows): hide first then switch, switch first then show. And the policy is set
  ONCE at launch, to the state that launch is actually in — a hidden launch is Accessory,
  a normal launch is Regular from the start. Flipping it twice inside the same
  millisecond (Accessory in `setup`, Regular from `show_main_window` a few lines later)
  produced an app with its menu in the menu bar, `lsappinfo` reporting `Foreground`, and
  NO DOCK TILE: macOS does not reliably add the tile for a switch made while the app is
  still launching. The runtime transition later — tray Open from a hidden app — DOES add
  it; measured both ways 2 Sep 2026. While Accessory there is no application
  menu (no About item, no Edit menu) and nothing activates the app on the user's behalf —
  which is why every path that draws our OWN UI (the tray's Open, the quit confirm) calls
  `platform::activate_app()` first, and why A9's measurement still matters: the webview
  keeps Cmd-C/V without that menu, so a window shown from a hidden app is usable before
  the Regular switch has settled. Prompts drawn by `osascript` are excluded on
  purpose: SecurityAgent is a separate process and fronts itself.
  The MENU itself is data: `core/tray.rs` is a pure `TrayModel -> MenuSpec` with no
  Tauri types, and `lib.rs` only renders it — status line, Start/Stop all, a capped
  Sites submenu, the five routes, the MCP checkmark, About, Open, Quit. It measures
  nothing: the verdict and count come from `commands::system::summarize` (the sidebar
  footer's own function) over a `try_lock` snapshot, and every click goes through the
  command the UI uses, so the tray can never be a second answer or a second path. About
  raises the app menu's own `menu://about` event, because two doors must not become two
  About screens — and it is in the tray at all because a window-less rexenv is Accessory
  and has no app menu to reach. Refreshed on a ~5s tick in THREE gears, because macOS
  closes an open menu the moment its items are replaced: identical spec → nothing; same
  SHAPE (`MenuSpec::same_shape` — same ids, same tree) → the titles, enabled flags and
  checkmarks are written onto the LIVE items, which an open menu survives; a row added or
  removed → a real rebuild, which no open menu can survive and no API can avoid. "Only
  rebuild when the spec differs" was the first attempt and was not enough: the status
  line moves on its own (the count, the `· updating…` suffix), so a menu held open closed
  itself a few seconds later — reported from the developer's machine, 8 Sep 2026.
  `docs/archive/PLAN-menubar-tray.md`.
  **On Windows** (W7 S5, owner's ruling Q2, ledger #624) the same menu renders from the same spec, but the
  icon and the clicks are Windows': the colour app icon (`WINDOWS_TRAY_ICON`, `icons/32x32.png`) — the
  notification area draws an icon as it is, and the template glyph is a black mark on a dark taskbar — and
  `show_menu_on_left_click(false)` with a left-button release calling `show_main_window`, so a LEFT click
  brings the window back and a RIGHT click opens the menu. There is no dock to follow: a hidden window leaves
  the taskbar, so the tray and a second launch (the single-instance pipe, #620) are the ways back.
- **Services OUTLIVE the app.** Closing rexenv stops nothing. On launch,
  `adopt_startup()` ADOPTS rexenv-owned survivors as pid-based `Proc::Adopted` handles —
  status/Start all/Stop all treat them like spawned children. Ownership gate = process
  holds one of OUR fixed ports AND references our app-data dir on its cmdline
  (`owned_listeners(port, marker)`, lowest pid = master). The root edge is invisible to
  unprivileged lsof → adopted iff OUR admin unix socket answers. If anything was adopted,
  cached nginx/caddy binary paths are wired strictly offline (existence-checked, no download).
  **On Windows** (#600, 13 Sep 2026) "outlives" needs a flag, not just a missing kill: a
  service is spawned with `CREATE_BREAKAWAY_FROM_JOB`, because a launcher inside a
  kill-on-close job — an SSH session measurably is one, a terminal or IDE can be — takes
  every child without it down with the job; plus `CREATE_NO_WINDOW` and its own process
  group. A HELPER the app runs and waits on (a DB client or dump, WP-CLI, `nginx -t`,
  `pg_ctl`) is not a service and does not break away, but it must not get a console either:
  `core/` builds every one through `platform::command`, which adds `CREATE_NO_WINDOW` on
  Windows (#705). Before that, all 22 were bare `Command::new`s written on macOS, and on the
  VM Start all left an empty `pg_ctl.exe` terminal on the desktop that PostgreSQL itself was
  attached to — close it and the database stopped. A launcher whose job forbids breakaway gets a start error that says so, never a
  service that silently dies with the app — measured 14 Sep 2026 from a Task Scheduler task, which
  forbids it: `proxy::start` returned ACCESS_DENIED with that wording. So rexenv is never launched by a
  scheduled task (plan §3 D1). **The installer's own "Run rexenv" was such a launcher** — measured
  19 Sep 2026 on the first installed copy (clean Windows 11 VM): Edge → `setup.exe` → NSIS's
  `RunAsUser` left rexenv in a job with limits `0x0`, and Start all failed on the first service.
  So before Tauri boots, `platform::relaunch_outside_confining_job` reads its OWN job's limits
  (`QueryInformationJobObject(NULL, …)`); confined and not started by Explorer, it asks the
  running Explorer to open its executable and exits — ONE hop, fail-open, before the app pipe
  or a window exists (`windows/job_guard.rs`, #692). Stop has no SIGTERM to send: a process with its
  own clean-shutdown channel is asked through it and given 10 s — `mysqld`, run with
  `--no-monitor` so the pid rexenv holds IS the server (the default restart monitor's child
  survived the monitor's termination still holding the port, measured), via its
  `MYSQLShutdown<pid>` event — and everything else, or anything that ignores the request,
  is ended with `TerminateProcess` (Mailpit today; owner ruling 13 Sep 2026). A service also
  never inherits a handle rexenv holds: `std` spawns with handle inheritance on, and the Dell
  showed services carrying 20–25 handles the launcher was born with (from sshd — not just its
  stdio) and holding its SSH session open until they were stopped; the same would pin a
  terminal's pipe for a service's life. Every inheritable handle in rexenv's own snapshot is
  cleared ONCE, first thing in `main.rs` (`windows/handles.rs`) — not beside each spawn, where
  it lived until 27 Sep 2026: a sweep on one thread cleared the pipe ends another thread's
  spawn had just made, and that child started with no stdio (#600, measured 119/400).
  **A PHP minor on Windows is a php-cgi GROUP, not a php-fpm pool** (#601, W4): the platform
  answers `ProcessSupervisor::php_pool_model()` with `PoolModel::CgiGroup` and the extensions its
  PHP build loads; `core::php_cgi` renders rexenv's own ini (`php-cgi -n -c`: `extension_dir`,
  the 26 extensions, the user's settings, SMTP keys to Mailpit) and the environment
  (`PHP_FCGI_CHILDREN=10`, `PHP_FCGI_MAX_REQUESTS=500`, the Laravel mail set), refuses to start on
  a failed preflight — read from the OUTPUT, since php-cgi exits 0 when an extension fails to load —
  and spawns the parent on the minor's usual pool port. `PhpFpmPools` runs the same ensure / adopt /
  reap / stop for both models; the settings gate tests a candidate for whichever model runs. The
  Xdebug pool is refused there (D4). The PHP CLI on the same platform (WP-CLI, Composer, artisan,
  Adminer) loads the same extensions from a `php.ini` `core` writes beside `php.exe` whenever the
  tree is resolved (#603) — php.exe with no ini loads none, and PHP reads that file by default.
  **The churn breaker** (#605): php-cgi's parent respawns a dead worker at once and has no backoff,
  so when a worker cannot be spawned at all it retries at full speed — measured on the Dell at
  96.8% of a core and ~1 MB of log a second, with the port and the surviving workers still serving.
  Each watchdog tick reads the parent's CPU (`php_cgi::cpu_ms`); at 25% of a core or more over at
  least 5 s, `PhpFpmPools::trip_spinning` stops the group and the watchdog reports `gave-up`,
  quoting the last `unable to spawn` line from that minor's own output log
  (`php-cgi-<minor>-output.log`), and never restarts it — a respawn would spin again. Legitimate
  recycling (0.9% at 1722 requests a second) and a script that kills its own worker (0.6%, the other
  workers still answering) never trip it: counting children was the first design, and a 10 s
  look cannot count past the worker count.
  **A one-click WordPress site serves on Windows** (#606, W4's "Done when"): the same core create
  path — `sites::provision`, `install_for_site` through `php.exe` and WP-CLI, `rebuild_configs`, the
  shared nginx over the php-cgi group — with mail from a page and from WP-CLI both reaching Mailpit.
  A site folder is checked for config-breaking characters inside each folder NAME
  (`sites::config_breaking_char`, #303), so Windows' `\` separator passes while a `\` inside a Unix
  folder name is still refused; judged across the raw string, it had refused every Windows path.
  **WP-CLI's mail on Windows goes by PHP's SMTP keys, not the sendmail shim** (#407): the shim is a
  cmd.exe command line there, and a Mailpit path with a space was lost in it while `mail()` answered
  `true`. `wordpress::finish_wp_argv` gives a php-cgi platform `-d SMTP=127.0.0.1 -d smtp_port=…` and
  NO `sendmail_path` flag — an emptied one is `""` to PHP, not the NULL that selects SMTP.
  **A pool's health is a FastCGI answer OR held requests, on both OSes** (#607): the watchdog's
  `PhpFpmPools::reap_dead` counts a pool as serving when its port accepts AND it answers a
  `GET_VALUES` record (`services::pool_answers`, 1 s) OR connections are held on its port
  (`ProcessSupervisor::established_on`). PHP answers `GET_VALUES` in a worker after `accept()`, so a
  pool with every worker busy does not answer — measured on the Mac and the Dell, its requests all
  completing — and restarting it on that would cut them; what remains a miss is a pool that accepts,
  answers nothing and holds nothing (frozen, or workerless). A count the platform cannot read falls back
  to the TCP connect. A restarted pool's readiness is the answer alone (it is idle). The Services
  status row still reads the TCP connect.
  **"All 10 workers busy — requests are queuing"** (#608): a pool whose every worker holds a request
  makes the next one wait — a slow page, then nginx's 504 — and neither PHP says so where a user
  looks. `commands::services::enriched_status` samples `established_on` for every running shared pool
  on each status poll (so only while a view polls) and feeds `core::pool_busy::BusyTracker`: held ≥
  the pool's workers (`php::pool_workers`, the same constants the configs are written from) for two
  samples in a row sets the row's `busyNote`, two below clears it, an unreadable count changes
  nothing. Each change writes a health-log line, with the hosts in the tail of nginx's access log as
  "recently served" — nginx logs a request when it ends, so the busy ones are not there yet.
  **Streamed steps on Windows — Composer, git, npm** (#609, owner rulings 14 Sep 2026): the
  provisioning and repo jobs run tools through `core::repo::run_step_streamed`, which needs the
  user's environment (`ShellRunner::login_shell_env`) and a step that can be stopped with everything
  it started (`spawn_streamed` / `stop_group`); Windows had neither, so a Laravel or Git site could
  not be provisioned there at all. The environment is read FRESH from the registry on every refresh —
  the system's and the user's `Environment` keys laid over this process's own variables, the user's
  `Path` appended to the system's, `REG_EXPAND_SZ` expanded (`windows/login_env.rs`) — so a tool
  installed while rexenv runs is found, as re-running the login shell finds it on macOS. Each step is
  spawned SUSPENDED into its own kill-on-close Job Object and only then resumed, so its grandchildren
  are in the job from their first instant; `stop_group` terminates the job and waits for it to empty,
  and a job that cannot be made refuses the step. Unlike macOS, a step does not outlive rexenv: the
  job's last handle closing ends it (the ruling — no half-finished install running unseen).
  **The edge's admin socket on Windows** (#610, W5 in progress): the never-TCP-`:2019` rule holds —
  Caddy serves its admin API on a unix socket on Windows too — but its address takes ONE slash before a
  drive path. Caddy splits an address at its first slash, so `unix//C:\…` named the socket `/C:\…`,
  which Caddy refused to start on and its CLI could not dial; `proxy::admin_address` writes
  `unix/C:\…` for such a path and keeps macOS's `unix///Users/…` byte for byte (a running edge's admin
  listener is keyed by that string).
  **The edge's start on Windows** (#611): an ordinary service process, like nginx — no LaunchDaemon, no
  UAC. `PrivilegeManager::port_needs_privilege` answers false there (the desktop user's token bound
  `:443`/`:80`, measured), so `prepare_edge` plans an unprivileged start and `start_services` takes the
  `proxy::start` child branch; `WindowsEdge::is_installed` is false, so adopt and the watchdog treat a
  survivor as the non-daemon edge and stop it with `stop_edge`. The Caddyfile carries `default_bind
  127.0.0.1` from `EdgeSupervisor::default_bind` — Caddy's all-interfaces bind raised Windows Defender
  Firewall's allow prompt on the desktop, so on Windows sites open from this computer only (owner
  ruling; cloudflared tunnels dial localhost and are unaffected); macOS writes no bind and is unchanged.
  `LocalIpc::connect` dials the admin socket over Winsock AF_UNIX (`windows/af_unix.rs`); Windows
  answers "connection refused" whether or not the socket file exists, so the file's presence tells
  `NotFound` from `ConnectionRefused` (`windows/ipc_rules.rs`). A killed Caddy leaves its socket file,
  and a fresh start comes up over it.
  **The shared nginx on Windows** (#602): every path in nginx.conf goes through `services::nginx_path`
  (forward slashes — a backslash in a quoted nginx string is an escape, measured); the binary is
  `binaries::resolve_program`, which answers a single binary or `nginx.exe` inside its tree; the socket
  table names nginx's WORKER, so `owned_master` climbs to the marked master; reload is nginx's
  `ngx_reload_<pid>` event and stop asks `ngx_quit_<pid>` first, because a terminated master leaves
  its worker serving.
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
  **A plan names the binary the start resolves, from the same place** (ledger #627): the pool's PHP comes
  from `PoolModel::catalog_name()` — `php-fpm` on macOS, the `php` tree a Windows php-cgi group runs — in
  `PhpFpmPools::ensure` and in every plan that stages a pool (`plan_for_start_with`, `plan_for_pool_with`,
  and `plan_for_php_with` / `plan_for_php_patch` through `pool_and_cli`, which lists the CLI `php` once where
  it IS the pool binary); the update apply's manifest check asks it too. The plans named `php-fpm` on every
  OS, so a Windows first run asked for a build that does not exist ("no binary manifest for php-fpm
  8.3.32", the Dell, 15 Sep 2026) and no site could begin.
- **The download bridge cannot be watched with a gap, and a gap is closed on the next
  mount** (`Snapshot::seq`, `useDownloads`, 18 Sep 2026). The hub pushes full snapshots as
  `download-progress` events; the UI seeds once from `downloads_state` and then only
  listens. Onboarding's Install step listens, its Domains step does not, and the footer
  mounts minutes later — every event in between was lost and the footer then read the
  cache the Install step had left (`staleTime: Infinity`): two rows "downloading 0 B" for
  a batch that had entirely failed, with nothing left to emit, forever (clean VM). Now a
  snapshot carries the hub's monotonic `seq`, a mounting listener registers FIRST and
  refetches the seed second, and the cache keeps the higher seq — a seed answered before an
  event but applied after it can no longer wind the UI back.
- **A batch is over when every row has SETTLED, however it settled** (`BatchSnapshot::
  failed` + `in_flight`, 18 Sep 2026). The footer's "still moving" test was `done <
  total` with `done` counting only successes, so a first run whose six downloads all gave
  up (clean VM, dead DNS relay) read `Downloading 1 of 6` over a full indeterminate bar,
  beside six failed rows. Now `done + failed < total`; the panel header says `n failed`.
- **A planned download row never outlives its resolve** (`Hub::item_settled`, called
  by `prefetch` for each item the moment its resolve returns). The resolvers' early
  returns — the cache hit, a failure before the first byte — report nothing, and a
  batch plans rows `Pending`. Until 11 Sep 2026 that left first-run users with rows
  reading `queued` and no Retry until a restart: the Install step's downloads were
  still running when they clicked on to "Create your first site", that create
  planned them as missing, they landed while its certificate was issued, and the
  new batch's resolves were cache hits. Settling never overwrites what a resolver
  did report (`Done` stays `Done`; its own failure text stays). The same wizard's
  Domains step waits on its admin prompt and keychain dialog inside `spawn_blocking`
  (`system_setup`, #567): as a plain call in the `async fn` that wait held a tokio
  worker — one the Install step's downloads share — for as long as the dialog stayed
  open.
- **The wizard's last step derives its verdict from the Install rows** (`useCoreComponents`,
  18 Sep 2026): "Everything's installed" only when every planned component is cached or
  done; otherwise "n of m ready" or "n failed" + Retry. It was static copy, and the first
  clean-VM run showed it green over six failed rows (`docs/DESIGN.md`, honest-UI rules).
- **A switch's binary is fetched before its row is written** (`set_site_web_server`,
  `switch_php_version`, #680, 18 Sep 2026). The record used to change first and the
  prefetch ran second, so a switch whose download failed — a stale FrankenPHP pin on the
  clean VM — left `server = frankenphp` in the row while nginx went on serving, and every
  later edge reload failed on the binary that was never there. The row is a promise about
  what serves the site; it is made once the thing that will serve it exists on disk.
  **And the refusals come before the fetch** (`core::sites::check_switch_on`, #786, 4 Oct
  2026): the prefetch used to run first, so switching a Windows site to OpenLiteSpeed (or a
  Linux one to Apache) failed with "1 of 1 downloads failed — fix the connection … no binary
  manifest" and left a failed row in the download panel, instead of the OS's refusal sentence.
- **Every call that can raise a privileged prompt waits off the runtime, from an
  async caller** (`core::prompt::while_prompting`, #568). The admin-password and
  keychain dialogs stay open as long as the user takes; the platform call waits with
  them on a plain thread. In an `async fn` that wait held a tokio worker — and every
  task queued on it; in a SYNC command Tauri ran it on the main thread and the
  whole window froze (`repair_resolver`, `remove_resolver`, `trust_local_ca` were
  sync until 11 Sep 2026). `while_prompting` is `block_in_place` on the
  multi-threaded runtime (the worker's queue moves to another thread) and a plain
  call anywhere else. `every_prompt_call_waits_off_the_runtime` DERIVES the prompt
  primitives from `core/` and fails the build for an unwrapped call or a sync
  command that prompts. **Nor is the database locked across a prompt** (#569):
  `dns::take_over_resolver`, `dns::hand_back_resolver` and `setup::run_system_teardown`
  take `&Mutex<Connection>` and lock it per step — plan, then the dialog with the lock
  released, then record — where they used to be handed a held guard, so every command
  that reads the database waited on the user for as long as the dialog stayed open.
- Long-running children spawn via `ProcessSupervisor::spawn_logged` →
  `<log_dir>/<svc>-stdout.log`. `stop` escalates to SIGKILL after a grace window (L3).
  Beware orphan workers after a SIGKILLed master: title-rewritten fpm/nginx workers can
  hold ports and defeat probes — the health watchdog + `Proc::terminate` guard this;
  check `logs/health.log` first.
- **What AI agents did is `<log_dir>/mcp.log`** (5 Sep 2026) — the `agent_actions` feed
  as a file, one line per row, written by the feed's ONE writer from the same clamped
  values the row gets (#516), so the Logs tab's **"AI agents (MCP)"** tab
  (`LogCategory::Agents`, offered once the file exists) can never say something the
  Settings card does not. Asked for because the card shows twenty rows and the table
  keeps two thousand, and nothing showed the rest. Refusals are WARN lines; the site is
  named by its domain at the time; rotates at 2 MB keeping one `.1`. **The file is COMMON,
  not per-site** (the feed is one table, and `list_sites` names no site) — the tab's
  "Only this site" toggle is a client-side filter by the site's ID (both line shapes),
  the chip shows `shown/total`, and its empty state says the other lines exist
  (`agentlog.js`).
- **rexenv's OWN log is `<log_dir>/rexenv.log`**, in every build, and it has its own
  Logs tab (`LogCategory::App`) — when a service did not start, the reason is there and
  not in that service's empty file. Its own tab because every other source reports what a
  SERVICE did and this reports what rexenv DECIDED; filed under `Server` it sat beneath a
  heading, "Server (nginx/PHP)", that was not true of it. It goes in `log_dir` beside every service log rather than in
  macOS's `~/Library/Logs`, because splitting a diagnosis across two directories costs
  more than the convention is worth. `tauri_plugin_log` was installed inside
  `if cfg!(debug_assertions)` until 18 Aug 2026, so an INSTALLED app wrote no `log::`
  output at all — including both "skipped the sweep" warnings that are the cache GC's
  safety valves. The sink set is a value (`lib.rs::log_sinks`) so the rule is testable
  without a running Tauri app; `debug` only ADDS stdout. Rotation `KeepSome(3)` at 2 MB.
  Ledger #359.
- **A panic writes `<log_dir>/crash.log`** (13 Sep 2026, W3 step 0, ledger #596). `crash.rs`
  installs a panic hook as the first line of `main`, so the DNS agent, the tunnel guard, the
  relauncher and the app are all covered: version, OS, thread, message, location and a
  backtrace are appended, the file rotates to `crash.log.old` past 1 MB, and when `Paths`
  cannot answer (a half-ported platform) it goes to the temp dir instead. The first panic of a
  process also calls `platform::fatal_notice` — a native message box on a Windows release
  build, a no-op elsewhere. Before this, a release build (no console) lost every panic
  message: the window never appeared, or an IPC command's promise never settled. The hook
  cannot tell a caught panic from an uncaught one; production code catches none.

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
- **Which pin set a host gets is a TIER, derived from the host's macOS version at every
  launch and never stored** (`BinaryTier`, ledger #707; `docs/PLAN-macos-13-floor.md`). macOS
  15 is the STANDARD — every feature, the compiled-in `*_VERSION` pins. The app also runs on
  13 and 14 (`Legacy13` / `Legacy14`), where a component whose standard build declares a
  `minos` above the host resolves the last build that loads there, or is refused with the
  macOS it needs. The platform derives the tier (`Platform::binary_tier`; default `Standard`,
  because a tier is a macOS fact and `core/` may not name an OS); `lib.rs` installs it once;
  `tauri.conf.json` states the LOWEST tier's floor (13.0); `macos_floor_check` holds every
  tier's `default_stack(tier)` under its own floor. A legacy host is told once, at onboarding,
  what it gets (`legacy_notice`, derived from the tier's refusals). The signed update catalog
  carries `minMacos` per Mach-O entry; a legacy host is offered and may resolve only an entry
  whose declared floor it meets — an entry with none is never offered there (ledger #711).
  `binaries::pins()` — `PinSet::for_tier(tier())` — is the ONLY door to a version: the
  `*_VERSION` constants are private to `binaries.rs`, so a consumer that spells one fails to
  compile (ledger #708). The legacy sets are `..STANDARD_PINS` with the measured rows
  overridden (`docs/PORTS.md` §"Legacy tiers"); the manifest tables themselves are
  tier-blind — a legacy pin is its own row under its own version, so a standard host can
  sweep, relink and run it (ledger #709). **A feature the tier cannot serve is refused with
  the macOS it needs** — derived as the lowest tier that offers it, said in one Rust-built
  sentence (`PlatformWords::needs_newer_os`), and LISTED disabled with it on Settings, Databases
  and New Site rather than omitted; `rex` and the MCP server reach the same two core gates
  (`php::set_installed`, `sites::ensure_engine_available_on`) and say the same words (ledger #710).
- macOS `prepare_binary` order (non-negotiable): **de-quarantine → relink Homebrew dylibs
  to `/usr/lib` → ad-hoc codesign LAST.**
- **`prepare_binary` needs NO Xcode Command Line Tools on the common path** (ledger #676).
  A Mach-O's dylib list is read from its own load commands (`core/macho.rs::linked_dylibs`),
  never asked of `otool` — `/usr/bin/otool` is a CLT shim that pops the "install developer
  tools?" dialog and fails on every Mac without the tools, i.e. every clean Mac. That is
  what 0.7.0–0.7.2 shipped: the first clean-VM smoke test (18 Sep 2026) failed all six
  components at `otool -L failed: xcode-select: error: Unable to get active developer
  directory`, invisible for two months because every dev machine has the tools. No pinned
  single binary has a foreign dep, so caddy/nginx/php/mailpit/mysql/adminer now touch no
  tool at all. `install_name_tool` (also CLT) is reached only when a rewrite is actually
  needed — the Homebrew-bottle BUNDLES (redis/mariadb/httpd/xdebug) — and is preceded by a
  quiet `xcode-select -p` probe (`clt_preflight`, the `git_preflight` shape) so a missing
  toolchain is a plain error naming `xcode-select --install`, never a surprise dialog from
  a background download. Bundles on a CLT-less Mac are the open half: `docs/TODO.md`.
- Shapes: single Mach-O (`resolve`) · plain file like WP-CLI `.phar` (`resolve_file`, no
  chmod/codesign) · dir tree like MySQL/PostgreSQL (`Archive::TarGzTree` + `resolve_dir`;
  extraction guards against path/symlink escapes, L2) · **Windows zips** (12 Sep 2026, port
  W2) — `Archive::Zip` for a single `.exe` (Caddy, Mailpit) and `Archive::ZipTree { strip }`
  for trees (PHP flat, nginx/MySQL with a top dir), through the same `resolve`/`resolve_dir`;
  every entry's RAW name is checked before any strip and symlink entries are refused
  (ledger #585 — the zip crate's own `enclosed_name` sanitizes an absolute name instead of
  refusing it) · **bottle BUNDLE** — Redis
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
- **`www.php.net` is the ONE read-only egress of the BINARIES layer** (`core/php_upstream.rs`)
  — it was the app's only routine outbound request when it was written, and since the signed
  PHP manifest (below) and the app's own update descriptor (below) it is one of three. A
  launch-time,
  best-effort GET of `releases/active.php?json` whose ONLY output is a version string per
  minor, rendered beside the pin as `8.3.33 exists`. It selects nothing — `source`,
  `sha256` and the rest of that document are never read, and a guard asserts exactly one
  field is. This is what shipped INSTEAD of a signed manifest (`docs/archive/PLAN-binary-updates.md`
  §12): no key, no button, no new trust surface. **The copy is load-bearing** — php.net is
  ahead of static-php.dev (where 8.x actually comes from) by weeks, so a newer patch can
  exist that rexenv cannot install; "exists" stays true where "update available" would not,
  and `core::copy_scan` bans the latter. Offline: `cached()` never touches the network and
  a never-successful check says so rather than implying currency. Ledger #343.
- **One cache predicate, one dispatch.** `binaries::shape_of` is the ONE name→resolver
  mapping (`Single`/`File`/`Dir`/`Bundle`) — `downloads::resolve_any` and the planner both
  read it, after they disagreed about `composer` (member `composer.phar`, consumers call
  `resolve_file`, planner routed it to `resolve` → the same artifact downloaded twice).
  `binaries::cached_path` is the ONE answer to "would this resolve without downloading":
  member present + pin marker matches + licence texts satisfied, per shape. `is_cached`
  (planner), `cached_bin` (sync adoption) and every resolve's fast path return it — they
  used to answer differently, so the planner promised "cached" about trees `resolve` then
  deleted and re-fetched with no hub row. `needs_repair` separates *never downloaded* from
  *downloaded, now incomplete*; the launch task repairs the second, which is what keeps
  login-start strictly offline (#175) without weakening the predicate. Ledger #341.
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
- **PHP patch bumps ride app releases (Option A), and the launch sweep keeps what RUNS.**
  A release that moves a pin makes `seed_registry` report the bumped minors; `lib.rs`
  then prefetches, restarts each bumped minor's live pool, and sweeps the superseded
  `php-<patch>/` trees (~136–208MB per minor — two trees, cli + fpm). **The bump loop
  is per-minor**, and **which minors need one is a LIVE question**: after adoption, each
  running master's EXECUTABLE path is read (`lsof -d txt` — argv is a rewritten title
  that names only the minor) and compared to the pin. There is no stored patch to
  consume, so a failed bump is simply asked again next launch. `php_versions.patch` was
  that store and was deleted in migration v36: it mirrored a compile-time pin, and the
  mirror produced two live bugs in one statement (#339 the consumed bump signal, #340 the
  user's default reset every launch). The Settings row says **both** the patch the minor
  will run and, when they differ, the one actually serving (`PhpVersionView::serving`) —
  without that a derived row could only show one, which is the same silent lie in a nicer
  place. One minor's failure `continue`s rather than abandoning the rest. Ledger #338/#342.
  The sweep's keep-set is `{each minor's EFFECTIVE patch} ∪ {every patch a pool is live
  on}` (`php_caches_to_keep`): it also runs on the path where a pool restart failed
  partway and later minors still serve from their old masters, and unlinking a running
  master's tree leaves it alive on the inode but unrestartable. The union is not
  belt-and-braces — with the stack stopped, "what is running" is an empty set rather than
  "I don't know", so a live-only keep-set deletes the tree the user just installed.
  **The compiled-in pins are NOT a third half**, and were: the argument that "a floor
  whose bytes were deleted is not a floor" does not survive `updates::floored`, which
  returns the pin only when the pin is what the minor will RUN — and then the pin already
  IS that minor's effective patch. Alongside a higher selection nothing resolves the pin,
  so the tree was ~180 MB per updated minor of pure weight (found on a user's disk at
  358 MB, in a `bin/` of 3.3 GB). An unreadable registry gives an EMPTY effective set,
  which `php_caches_to_keep` refuses to answer — it returns `None` and the sweep is
  skipped, because without the pins half an empty keep-set means "delete every PHP tree
  on the machine". Ledger #338/#358.
- **A user can also move a minor forward BETWEEN app releases, from a signed manifest.**
  `core::updates` fetches `manifest.json` + `.sig` — **two files on `rexenv/runtimes`'
  default branch**, written by a commit rather than a release (a moved tag broke in
  production: GitHub burns a tag name once an immutable release on it is deleted, and
  delete-then-create is an availability hole even when it works — #368) — verifies an
  ed25519 signature over the exact bytes against a **public key compiled into the app**
  (never TLS: rexenv's digest gate compares bytes to whoever supplied the digest, so an
  attacker-chosen URL paired with an attacker-chosen hash matches perfectly), and keeps
  only entries surviving four structural limits, **each per FAMILY**
  (`updates::Family`): a declared name (`caddy` runs as a root LaunchDaemon, so it is
  not one — nor is any other service, tool or bottle), https from an allowlisted host,
  a version on a track the family accepts (PHP: a patch of a minor already shipped;
  Adminer: a major whose plugin API has been probed against rexenv's wrapper),
  lowercase 64-hex. A **monotonic serial** refuses a replayed older document,
  and the compiled-in pins stay a **floor**, so a manifest can only move a minor forward.
  The choice lands in `php_versions.selected_patch` (v37) and `php::patch_to_run` is the
  ONE answer to "which interpreter" — pool, planner, terminal, WP-CLI, composer, the
  agent tools and the ini `-t` gate all read it, enforced by a source scan (#353), because
  the pin needs no `Connection` and so compiles anywhere it does not belong. The publish
  side is one command in the runtimes repo (`scripts/publish-manifest.sh`). Ledger
  #348–#355; design in `docs/archive/PLAN-binary-updates.md`.
- **And the APP can move forward between releases the same way** (`core/app_update.rs`,
  `docs/archive/PLAN-self-update.md`). A SECOND signed document — `app-manifest.json` + `.sig`, two
  more files on `rexenv/runtimes`' default branch — names one release: version, artifact URL,
  SHA-256, size, and the macOS and rexenv floors it needs. It rides the SAME compiled-in
  `RELEASE_PUBKEY` through the same ed25519 seam (`updates::verify_signed_bytes` — one
  signature check in the codebase, not two), and repeats the rules that document earned:
  re-verified on every read, its OWN monotonic serial as rollback protection, and an artifact
  URL that must sit under a compiled-in `releases/download/` PATH prefix, checked before the
  redirect because GitHub's last hop is an expiring CDN URL.
  **Why not a `Family` in the PHP manifest**: that document is locked to artifacts flowing
  through `binaries::resolve*` (an unknown name there would be chmod-ed, `prepare_binary`-ed
  and spawned as a service), and the app's grant is strictly larger than PHP's — the same
  native code as the user, PLUS the binary that re-execs as the DNS agent and the tunnel
  guard, PLUS the process that enforces the agent dial and `settings_access`. A separate
  document keeps `only_the_declared_families_are_nameable` intact and carries fields the PHP
  rows have no slot for.
  **An offer is a live comparison, never a stored flag**: strictly newer by numeric segment,
  three all-digit segments (so a prerelease is not representable rather than filtered),
  floors satisfied, and not the skipped version — which is stored as a VERSION and compared,
  because a "skipped" boolean would hide every later release too. An unreadable host macOS
  version fails closed.
  **Cadence and consent** (T2): the check rides the launch sweep — one more GET pair to a
  host the sweep already talks to — and a 6 h `sleep` loop covers the machine left running,
  since a menu-bar app outlives its window by design. `app_update_auto_check` is read
  BEFORE any I/O, so turning it off is a switch on the REQUEST rather than on what is done
  with the answer, and "Check now" always works. The switch is in the card and in
  `rex config`; "on" is the ABSENT row a fresh install has, never the string `"true"`,
  so the key has one spelling of on rather than two for its reader to agree with by luck. The request carries `rexenv/<version>` and
  nothing else: no arch, no identifier. The fetch enforces its OWN total deadline over reqwest's, and the CALLER chooses it —
6s when a person pressed "Check now", 12s for the poller and the launch sweep (ledger #540).
A client timeout is not a guarantee, and a person watching a spinner is the caller the
original "nobody is waiting on this" comment forgot. Every failure is a log line and an
honest footer —
  `checked N ago` reads ONE stored value written only after a success, so a failed check
  keeps yesterday's timestamp instead of aging into a lie, and nothing ever says "up to
  date".
  **A cancelled quit is a state, not a non-event** (7 Sep 2026, found by §M): the swap has
  already happened when the quit gate asks, so choosing "Keep sharing" leaves the new bundle
  on disk under the old process. The card reads that from the notice row the swap writes —
  which is why it survives the window being closed — suppresses the offer, empties the tray
  snapshot, and says the only true thing left: it takes effect the next time rexenv opens.
  Ledger #539.
  **The swap** (T3, `AppBundle` — the 12th platform trait) stages a whole new bundle as a
  SIBLING of the installed one, which makes a cross-device rename impossible by
  construction rather than by a check, verifies it (version, identifier, executable name,
  the `rex` sidecar, both architectures in every Mach-O, `codesign --verify --deep
  --strict`), and exchanges the two paths with ONE `renamex_np(RENAME_SWAP)` — so no
  instant exists where the install path holds nothing, which is the state a KeepAlive
  LaunchAgent would respawn into. Nothing is written inside the launched bundle and nothing
  is copied over it: replacing a running Mach-O in place invalidates pages the kernel is
  executing, and a copy-over leaves a hybrid of two builds. **The previous bundle stays on
  disk** until the new app has launched and confirmed its own version, and leftovers are
  classified by the version INSIDE them rather than by a marker, so a crash between the
  swap and any write cannot mislead the sweep. **No path here is privileged**: an
  unwritable folder, a translocated or `/Volumes` launch, a symlinked path or a foreign
  owner is a refusal that names the consequence and carries a copy-paste fix. The same
  translocation predicate guards the two launchd files the app writes for itself — the DNS
  agent's plist and the login item — so a zipped launch never records a path that dies
  with it (#753).
  **On Windows the same shape, minus the atomic exchange** (`platform/windows/app_bundle.rs`,
  19 Sep 2026). The "bundle" is the install DIRECTORY the NSIS installer makes —
  `rexenv.exe`, `rex.exe`, `uninstall.exe`, nothing else — and it is swapped whole, because a
  running `.exe` cannot be deleted or overwritten but it and its directory CAN be renamed,
  and the process keeps running from the renamed file (measured on the Dell; the plan had
  assumed the opposite and would have built a relauncher that swapped after exit). So the
  swap is macOS's `RenamePair` fallback as the only path, restore included. Verification
  reads the executable's OWN `VERSIONINFO` (product version and `ProductName`) rather than a
  file written beside it, and the PE machine field rather than the filename; `codesign` is
  off by ruling, since the build is unsigned (D5). Two Windows-only facts shape the rest:
  `uninstall.exe` is written by the installer, not the build, so the staged directory gets
  the installed one copied in before the swap and the HKCU uninstall entry's
  `DisplayVersion` rewritten after it; and each OS fetches its OWN descriptor
  (`app-manifest-windows.json`, same schema, same key) because one `release` names one
  artifact and the macOS one is a universal `.app.tar.gz` — and each document's accepted
  serial, bytes and signature sit under their own settings keys (`app_update::doc_key`), since
  28 Sep 2026: a machine that had read another document's serial refused its own document
  as a replay forever (#519). `InstallKind` grew
  `ProgramsPerUser` (the ordinary case) and `ProgramFiles` (per-machine — refused with the
  per-user reinstall named, never a UAC prompt); the vocabulary was made platform-neutral
  for this rather than bolted on, because Linux is next.
  **On Linux, two install shapes, two swaps** (`platform/linux/app_bundle.rs`, owner ruling
  24 Sep 2026 — D-L7 reversed, self-update IS in v1). A `.deb` install is `/usr/bin/rexenv`,
  root-owned: the user's side stages the downloaded package in its own app-data `updates/`
  folder (`InstallKind::SystemPackage` — the preflight asks the STAGING folder for room and
  writability, since root's ownership of `/usr/bin` is by design), reads its `Package`,
  `Version` and `Architecture` and its `usr/bin/{rexenv,rex}` members with `dpkg-deb`, and
  then the ONE privileged command an update runs — `/usr/bin/dpkg -i '<staged>'` — goes
  through the same polkit step as the DNS route (one prompt, rexenv's own sentence). An
  AppImage is ONE user-owned file (`$APPIMAGE`, `InstallKind::PortableFile`): it is the
  macOS shape exactly — staged beside itself, verified by running the staged file with
  `--print-version` (the flag `main.rs` answers before anything else loads) and comparing
  to the signed release, then `renameat2(RENAME_EXCHANGE)` with the rename pair as the
  fallback, no prompt. A `cargo` build in `target/` and a bare binary anywhere else are
  refused with both shapes named. Each build reads its OWN descriptor,
  `app-manifest-linux-<deb|appimage>-<x86_64|aarch64>.json`, chosen by what the process IS;
  a build that is neither asks for none and `fetch` refuses rather than guess (ledger #729,
  #730; `linux_app_swap_check` is the L1). The relauncher is the same `--relaunch-after`
  self-exec, waiting on a pidfd and then starting `/usr/bin/rexenv` or the image detached.
  **Every OS's relauncher leaves `logs/relaunch.log`** under rexenv's data folder (#768, 2 Oct
  2026): what it was told, how the wait ended (`ParentWait` — exited, already gone, still alive
  at the cap, unwatchable — with the seconds), how the launch ended (`open`'s exit code, or the
  spawn error). The 30 Sep reopen that did not happen on the 15.8 VM had left no trace at all:
  the helper's stderr is `/dev/null` by design. The live checks pass `--log <fixture dir>` so
  the real file is never a test's.
  **The relaunch** (T4) is a QUIT: `app_update_apply` ends in `app.exit(0)`, so
  `ExitRequested` runs the same live-share confirm every other quit passes through, and
  `RunEvent::Exit` — reached only once that gate agreed — spawns a detached helper
  (`--relaunch-after <pid> <token> <bundle>`, the self-exec shape the DNS agent and tunnel
  guard already use). The helper waits on kqueue `NOTE_EXIT` for THAT pid, identity-checked
  by its start token, then `open`s the bundle PATH. The OLD app spawns it, but from
  `current_exe()` — a path the swap has already re-pointed — so on macOS the NEW binary runs it
  (measured 6 Oct 2026); `--relaunch-after`'s argv is what the two versions must agree on, and
  `relaunch.log` comes from the new side. `AppHandle::restart` is never called: it
  skips the gate on the main thread, can be cancelled off it (leaving every later quit a
  silent relaunch), and spawns the child before exiting — which races the single-instance
  socket. Waiting removes that race instead of arguing about it. An apply does NOT refuse a
  busy app, deliberately: an update IS a quit, and refusing here would be stricter than
  Cmd+Q for an identical consequence — the consent sentence says instead that terminals and
  jobs close with it. At the next launch `finish_at_launch` reports the version the new
  process reads from ITSELF, sweeps the leftovers, and only then deletes the previous
  bundle. **Between the swap and the restart the card says only what is true** (#771, 2 Oct
  2026): "you chose to keep rexenv running" needs `restart_declined` — the gate told no while a
  relaunch was armed; cleared by the next OK — and a swapped build still waiting for its answer
  is `pending`, the header stating the fact and nothing added.
  **The check is a read everywhere; the install is a click and nowhere else.** `rex status`
  prints one line naming the version when a verified offer exists, `--json` and the MCP
  `stack_status` tool carry the same field, and all three read the in-process snapshot — a
  status call verifies no signature and touches no network. There is no `rex update` and no
  MCP tool that installs: an agent asking for one would be asking to replace the process
  that enforces the dial that bounds it, and the relaunch kills the socket its answer would
  return on. Ledger #517–#536.
- **ADMINER is the SECOND family in that manifest**, and the limits are per family
  (`updates::Family`). Its grant is strictly below PHP's — `Shape::File` →
  `resolve_file`, no chmod, no codesign, never spawned, interpreted by an already-running
  pool as the user — so admitting it raises no ceiling a key-holder already had. What IS
  worse is **control ownership**: rexenv's login gate and frame protections for the
  database console live inside Adminer's OWN plugin API (the wrapper subclasses
  `\Adminer\Adminer`), so an Adminer update can switch off a control rexenv wrote,
  silently, with the console still serving. Two things answer that and neither is
  optional — `ADMINER_MAX_MAJOR`, a ceiling that is EVIDENCE (the newest major actually
  run against the wrapper, measured by running it), and `adminer::verify_pair`, which
  runs each candidate before an apply commits. The real console is staged as a DOTFILE
  (`.adminer.php`): every `.php` in that docroot is directly executable, so
  `/adminer.php` used to serve Adminer with **no wrapper at all** — no login gate, no
  frame bound. `adminer::effective_version` is the ONE answer to "which Adminer", the row
  lives on the Databases screen (its only entry point), and there is no "exists" chip
  because rexenv downloads Adminer's own release asset. **The frame bound has TWO forms,
  because the console is served two ways:** the wrapper bakes `frame-ancestors` for the
  DIRECT https vhost (which a page in the user's browser can also reach — the clickjacking
  case), while the `rexdb:` proxy substitutes every origin rexenv's own webview is served
  from before replaying the header. Without that substitution the Database Browser rendered
  EMPTY on Windows, where the app runs at `http://tauri.localhost` and the baked list named
  only macOS's origins (reported 20 Sep 2026; a blocked frame is silent). Nothing else in
  the CSP is touched — the script nonce is in it. Ledger #361–#369, #699; design in
  `docs/archive/PLAN-adminer-updates.md`.
- **A PHP version resolves to the source that PUBLISHES it.** Most come from
  static-php.dev's bulk builds; the ones nobody publishes portably are built by
  `rexenv/runtimes` CI and hosted as GitHub Release assets (`php_url` /
  `php_self_hosted_tag`). The self-hosted URL carries the **full immutable release
  tag**, not a stable base + version const: static-php.dev and FrankenPHP both
  rebuild assets in place, so our own host is the one place a pin can be made
  permanent — a rebuild is a NEW tag, never a re-upload, so a pin may 404 but can
  never resolve to different bytes. An EMPTY checksum const reads as unpinned, which
  is what keeps a version wired-but-unresolvable until its artifact exists
  (`docs/archive/PLAN-php-74-support.md`).
- **"In-tree" for a bundle means the load command RESOLVES under the bundle root**,
  not that it starts with `@loader_path/`. Treating the prefix as proof let deps
  spelled `@loader_path/../../../../opt/<formula>/lib/…` through both the rewrite and
  the verify loop, so `prepare_binary_tree` reported success over a tree dyld then
  refused — and `resolve_bundle` caches on the member file EXISTING, so the dead tree
  never re-downloaded (that half is still open, `docs/TODO.md` S0.3). Ledger #319.

## 8. Data & app state

- **An older build refuses newer data** (13 Sep 2026, ledger #593): `state::db::open` reads
  `user_version` before any pragma, and a value above this build's migration count stops the
  launch on its own screen ("this copy of rexenv is older than its data", both schema
  numbers, nothing changed). Migrations only walk forward, so without it an older copy
  sharing a data folder with a newer one — a hand-installed older dmg, or a dev build next
  to a release — would read and write tables whose shape it does not know. Builds up to
  0.7.1 predate the check and never refuse.
- **SQLite for all app state** (`state/db.rs`), `user_version` migrations, currently 45:
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
  `mu-plugins/`; delete removes the dir only when recorded ours + empty) ·
  v26 `agent_actions` (the MCP accountability feed) · v27 `sites.origin` +
  `agent_client` + `expires_at` (the scratch-site tier boundary) ·
  v28 `agent_actions.actor` · v29 `scratch_packages` · v30
  `agent_actions.args_summary` · v31 `db_imports.skipped_tables` (an incomplete
  copy must not read as a complete one) · v32 `sites.docroot_subdir` (what the
  vhost roots at, so Laravel's `.env` is never a public URL — read ONLY through
  `Site::served_root()`) · v33 `sites.git_url` + `git_ref` (the repository a
  cloned site's code came from; NULL is EXACT for pre-v33 rows because nothing
  could clone into a docroot before it — `docs/archive/PLAN-git-site-clone.md`) ·
  v34 `sites.git_migrate` (NULL = ON; recorded because Retry rebuilds the phase
  list from the row) · v35 `sites.git_build_assets` (NULL = OFF — the opposite
  default, and equally exact: no provisioning ever ran a package manager before it). ·
  v36 DROPS `php_versions.patch` (it MIRRORED the compile-time pin — #339/#340) ·
  v37 `php_versions.selected_patch` (NULL = follow the pin; the user's choice, which
  is why it is stored where v36's mirror could not be) · v38 `agent_db_grants` (one
  agent's permission to read one site's DB — `site_id`/`client`/`db_user` scoped,
  with `expires_at` STORED rather than a duration added at read time, and
  `revoked_at` instead of DELETE so a revoked grant stays as evidence of what an
  agent could see and until when) · v39 the grants index · v40 `agent_db_grants.auto_granted` (did a PERSON click
  Allow, or did auto-allow answer? Recorded, because auto-allow is session-scoped and will
  usually be off by the time anyone reads the list). **D16, 4 Sep 2026: v38–v40 are no longer a consent record — the Agent access dial answers reads; `db_query` still writes a row per (site, principal) — after a successful provision — as the RECORD of the account it made, so a rename cannot orphan the account (#398, #403).** · v41 `sites.starter_db` (did a
  Blank-PHP site ask for a starter database? INTENT, written at the insert, beside
  v19's `db_created` PROVENANCE, written by the job after `CREATE DATABASE` — one
  column could not hold both without lying in the window a failed job leaves the
  user sitting in, holding Retry) · v42 `site_domains` (the extra hostnames a site
  also answers on — Valet compatibility, §the multi-name entry above) · v43
  `agent_site_grants` (kept as HISTORY: D17 retired every reader and writer, and a
  source guard bans the symbols) · v44 `sites.enabled` (**is this site served?** —
  `false` is the user stopping ONE site, which in this topology is a serving-surface
  change and not a process one: no SERVING nginx block — a STOPPED one instead, 503 with
  the stop page, because a name with no block is answered by nginx's default server
  (#511) — a Caddy route that keeps its certificate and answers 503, and only a site's
  OWN override backend stopped. In the
  database, not the ServiceManager, because services outlive the app and a stopped site
  that came back on after a relaunch would be the user's decision quietly reversed.
  DEFAULT 1 with no backfill, and that is exact rather than safe: before the column,
  stopping one site was impossible — `docs/archive/PLAN-per-site-lifecycle.md`). · v45
  `site_worktrees` (a site that is a git WORKTREE of another site's checkout —
  `docs/PLAN-git-worktrees.md` §2.3: `parent_id`, `shape` asset|site, the worktree path,
  `adopted`; the branch is NOT stored, git answers it live. `parent_id` has no
  `ON DELETE`, so with `foreign_keys=ON` the database itself refuses deleting a parent
  that still has children, #815)
  Per-engine DB versions are settings-KV rows (`db_version_<engine>`), not a migration.
  *(This list read "currently 25" for eight migrations — restored 11 Aug 2026.
  A count is the one part of a list that goes wrong silently, so check it
  against `MIGRATIONS.len()` rather than trusting the prose.)*
- **The database name is derived ONCE from the site's TYPE and domain**
  (`wordpress::db_name_for` + `db_name_prefix`, `sites::unique_db_name`): `wp_` for
  WordPress, `lv_` for Laravel, `php_` for a plain PHP site — `blog.rex` → `wp_blog_rex`,
  a Laravel `myapp.rex` → `lv_myapp_rex`. Until 13 Aug 2026 the rule took only a domain,
  so EVERY type got `wp_`: a Laravel app owned `wp_myapp_rex`, a WordPress label on a
  database WordPress never touches — and that string is what a developer reads in
  Adminer/TablePlus and in `.env`. The type is a required parameter now, so a new call
  site cannot silently inherit `wp_`. Prefixes stay short because they spend the same
  64-char identifier budget as the domain slug (`DB_NAME_MAX`; a colliding or
  overflowing base falls back to the FNV hash suffix, B21). **Existing sites keep the
  name they stored** — the rule is creation-time only and every runtime op reads
  `sites.db_name`, so nothing renames a database under a live site (and the v6 backfill
  stays `wp_` for exactly that reason).
- **The content dir is RECORDED, never re-derived at write time** (v24
  `sites.content_dir`, decided once at create/backfill from filesystem markers,
  poison-resistant): every writer that builds a `wp-content`-relative path itself —
  mu-plugin writers, asset destinations, the unlink-delete guard, theme
  screenshots, the debug-log reader — takes the recorded rel. Bedrock (`web/
  app/`) is why: deriving at write time silently wrote where WordPress never
  loads. Anything the record can't answer reads honestly `indeterminate`.
  **`indeterminate` is a floor, not a resting place** (24 Aug 2026, ledger #391): the
  debug-log reader now reads an env-configured install's `WP_DEBUG`/`WP_DEBUG_LOG` out of
  its `.env` — where `config/application.php` reads them from — instead of reporting the
  whole Bedrock layout unknowable. Explicit keys only: a missing one stays indeterminate
  rather than becoming "off", and a value hardcoded in the PHP instead of `.env` stays
  indeterminate too. The project root needs a `.env` AND a `config/application.php`, since
  a bare `.env` says nothing about WordPress.
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
- **A folder holding a git worktree is never `remove_dir_all`ed — even one rexenv owns**
  (#814, 9 Oct 2026, W1 of `docs/PLAN-git-worktrees.md`). A worktree's `.git` is a FILE
  pointing at a repository that lives elsewhere; deleting the folder destroys
  uncommitted work that repository will never miss. `core::worktree::
  foreign_checkout_under` finds any `.git` file under the docroot whose gitdir resolves
  OUTSIDE it (a submodule of a repo inside the tree is not foreign; an unreadable
  pointer counts as foreign). Two defences: `sites::delete_preflight` refuses the delete
  BEFORE the tunnel stop and the database drop with a `$ git worktree remove "<dir>"`
  fix line (git itself refuses a dirty worktree), and `teardown` keeps such a docroot
  even if reached without the preflight. The same preflight refuses deleting a site that
  still has worktree children (#815), naming them.
- **A worktree child's database is a COPY made on the same server** (`core::dbclone`, #817,
  W3). It is not the import pipeline: `dbdump::gate` refuses rexenv's own server on
  purpose, and that refusal stays. The copy is built from the import's witnessed pieces.
  `record_provenance` writes the child's `db_created` before `CREATE DATABASE`.
  `prepare_target` drops and recreates only a target the record says is OURS. A target
  that exists and is not ours is refused. Membership of every source table is checked
  after the import, and the private (0600) dump file is removed on every path. The dump
  uses the export's flags, so stored routines and events are not copied; WordPress uses
  neither.
- **A plugin/theme worktree child is a COPY of its parent's WordPress, with one folder a
  git worktree** (#819, W4/W5, built 9 Oct 2026). It is not yet reachable from the UI: the
  command is unregistered until W8. `commands::worktree::start` validates the request
  against the parent row. A subdomain network or a Bedrock layout is refused for now. It
  derives the domain and hands a `NewSite` to the ONE provisioning path. The relation
  (`site_worktrees`) is written by `start_with`'s `AfterInsert` hook, under the same lock
  as the row. The job then runs its own list:
  1. `copy` — `copy_site_tree` into a fresh staging dir, renamed over the empty docroot.
     Links are listed, never followed. The tunnel/login/scratch-mail mu-plugins are
     stripped.
  2. `worktree` — `git worktree add` from the parent's checkout, on the system git.
  3. `deps` — the plugin's own composer.json.
  4. `db`, then `db_copy` — `core::dbclone`.
  5. `configure` — `wp config set` for `DB_NAME`, plus only the URL/network constants the
     file defines.
  6. `urls` — `rehome_urls_on_copy`, parent → child, through #818's token.
  7. `serve`.

  Each step is skipped on Retry once its result is on disk.
- **Deleting a worktree child goes through git first** (#820, W6). `delete_site_owned`'s
  step 0 is `commands::worktree::release_for_delete`. It runs `git worktree remove` from
  the PARENT's repository, never `rm`. While `git status --porcelain` lists anything, the
  delete is refused with those files named. Only then does the preflight run, and the
  ordinary delete (tunnel, database, row, docroot). git refuses a dirty worktree itself
  too, so the pre-check exists to name the files, and git is the second wall. The branch
  is never deleted. An adopted worktree (§2.7) is never git-removed.
- **The worktree UI** (W8, 10 Oct 2026) sits under a git plugin's or theme's repo panel
  (`components/wordpress/AssetWorktrees.tsx`). It lists that checkout's worktree sites
  with git's LIVE branch and change count (`worktree_children`), and holds the
  New-worktree dialog. The dialog previews the domain and every refusal through
  `worktree_preview`, which runs the same `plan_child` the create runs. Create hands off to
  the Sites page, whose card adopts the running job. A child's page header says what it is
  a worktree of (`WorktreeOfLine`, `worktree_of`). The Sites list's branch chip reads the
  recorded request (`worktree_relations`) and no git, so a list render never waits on N
  git processes. Every worktree query sits under the `["sites", …]` key, so one invalidation
  refreshes them all. A refused (dirty) remove is never forced without a second, explicit
  yes that quotes the files (#821).
- **Delete and rename sweep every file named after the site — including its
  job logs.** Fixed-name files (override configs/logs, the tunnel log) have a
  `log_path(domain)` owner; the per-RUN logs (`site-provision-`, `wp-install-`,
  `db-import-`, `repo-` + `<domain>-<run>.log`) are `logs::RUN_LOG_FAMILIES`,
  removed by `logs::remove_run_logs` from both sweeps. They were in neither
  until 11 Sep 2026: one dev machine still held 284 deleted sites' provision
  logs. Ownership is not a prefix match — domains contain `-`, so
  `repo-foo.rex-` also starts `foo.rex-2.rex`'s logs; a job-id run must be 8
  hex, and a Git run goes to the LONGEST living domain it reads as. The same
  rule decides which Git logs `logs::targets_for_site` LISTS for a site — the
  closed key set MCP `site_logs` opens under `read` on that site — so a
  neighbour's Git output is not one of them.
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
  The Sites banner reads `dns::drifted_takeover_records` (each TLD with its
  `taken_at`) and keys a dismissal on `<tld>@<takenAt>`: every re-take refreshes
  `taken_at`, so the next loss is a new key and shows again wherever the re-take
  was made (#807 — the bare-TLD key was only cleared when the banner itself saw
  the TLD read as ours, so a re-take on Import hid the second loss; the 15.8 VM,
  9 Oct 2026). Settings' hand-back row says a DRIFTED borrow was taken back,
  never "rexenv answers this" (#808).
  **The consent renders where the refusal lands (5 Sep 2026).** The takeover card
  (`ResolverConsent`: their file beside ours, unticked checkbox, the way out named)
  lived only on the Import page, and only for TLDs the scan derived from Valet's own
  SITES — so a user who had left Valet typed `shop.test` into Change domain, got
  "rexenv can take that TLD over" as a toast, and had no button anywhere that did:
  the scan listed nothing, and Settings' Repair for a foreign TLD refused by design
  (a button that could only fail). Now `resolver_tld_status(tld)` answers ownership
  for a TYPED TLD (policy-gated, since `resolver_path` joins the string onto
  `/etc/resolver`), and `ResolverConsentFor` drops the card under the Change-domain
  input (the button waits for it), under the default-TLD setting (the first site
  created there is what would hit the refusal) and in place of Repair on a foreign
  row of the "can't be resolved" card. The write is still the one consented
  `take_over_resolver`; a success invalidates every reader of who-owns-this-TLD.
  **And the import scan lists the FILE, not just their sites** (`dns::foreign_tlds`,
  same day): its TLD list came from Valet/Herd's candidates alone, so a leftover
  `/etc/resolver/test` from an uninstalled Valet — no site behind it — appeared on no
  page at all; now every non-ours valid-label file in the resolver dir and every
  borrowed TLD is a row, and the Import page renders the consent even in its "no sites
  found" state, with copy that does not promise "these sites" when there are none.
  The complement is filtered by the same label rule as ours-by-signature: a name
  rexenv could never create is a name it must never offer to take over, because the
  offer ends in a privileged write to that path.
- **Valet/Herd import** (`core/valet.rs` + `commands/valet_import.rs`,
  `/import`): a strictly read-only scan of their config, symlink farm and
  per-site confs — nothing of theirs is written, started or stopped, and no file
  inside a user project is opened by the SCAN. The import itself reads one: a
  WordPress row's wp-config text (`core::sites::network_mode_on_disk` — `MULTISITE`,
  `SUBDOMAIN_INSTALL`; no PHP runs), because Valet and Herd record a network nowhere
  else, and until 13 Sep 2026 their networks landed as single sites (sub-sites unserved,
  Convert offered on a live network). A network found there is adopted like Local's and
  the batch reload covers it; Convert on a docroot that already is a network records the
  file's mode instead of converting, which is also how a network imported before the
  fix is repaired from the WordPress tab (#591). Deliberately tolerant of real installations
  (dangling symlinks, confs with no site, proxies, a conf on a TLD the config
  never mentions, the pre-2.1 `domain` key, duplicate parked paths, and all
  three isolation-marker formats), surfacing every case as a row or a note
  rather than dropping it. Herd wins a duplicate domain. Import reuses the
  ORDINARY create path per site (`site_provision::start` + poll), sequentially
  and CONTINUE-ON-FAILURE, with resolver consent and `php::set_installed` +
  prefetch settled BEFORE the loop. `examples/valet_scan_check` fingerprints
  their trees before and after to prove the scan wrote nothing. Import is its own
  sidebar page (`nav.ts`) since 12 Sep 2026 — the owner reversed the original "no
  permanent nav item" decision once Local joined, and removed the entry point that sat
  inside Settings → DNS & SSL. DNS & SSL now carries only DNS: the borrowed-resolver
  hand-back rows; the leftover-dumps card lives on the Import page with the imports
  that leave it. A link farm is ONE row whose other names show under it ("also
  answers on …", `extraDomains`) — until 12 Sep 2026 the fold carried them and the
  import added them, but the row never displayed them, so the owner's real farm
  (`tr` + `tr-www`) read as a single name and looked lost. On a WordPress site
  those extra names reach it and redirect to the primary (the multi-name entry above).
- **Local import** (`core/localwp.rs`; the same `/import` screen, scan and run —
  design record `docs/archive/PLAN-local-import.md`). Local keeps a REGISTRY
  (`~/Library/Application Support/Local/sites.json`), not a symlink farm; the scan
  reads it plus existence probes of `<folder>/app/public` — never Local's
  `site-statuses.json` (a label about what runs, the DBngin lesson) and never a file
  inside the project (#570). `.local` is refused by TLD policy (Bonjour), so a site on
  a refused TLD is RE-HOMED onto the default TLD in the scan itself (`ea.local` →
  `ea.rex`, shown on the row as "was ea.local"), before `enrich` or site creation see
  it (#571); an allowed TLD keeps its name. A multisite NETWORK imports too (12 Sep 2026,
  `docs/PLAN-local-multisite.md`): its mode comes from the registry's `multiSite` and is
  RECORDED after the ordinary provision (`core::sites::adopt_multisite`) — never by
  running `multisite-convert`, which every other path to a network runs and which would
  rewrite a live network's wp-config and tables. The batch's one reload serves it (the
  network rewrites, and `*.domain` for subdomains). Refused with the reason: a subsite on
  a mapped domain (a `multiSiteDomains` host outside the network's name). The row shows
  the network and its subsites under the imported name.
  Already-imported is decided by FOLDER first for every source (a re-homed name
  cannot be matched by name, and a Valet site whose domain was later changed in
  rexenv is still that folder); a Local row whose name a Valet/Herd row already
  claims is refused rather than listed twice. A row pinned to a PHP minor rexenv
  doesn't ship (Local pins patches like 7.3.5) carries `php_choice` and gets a
  version picker on the row — for every source — and `choose_php` refuses such a
  row in the run unless a version was explicitly chosen: the run used to fall back
  to the default PHP for it, the silent substitution the scan exists to refuse.
  The database import finds a Local site's data through Local's REGISTRY, not
  wp-config's literal host (#572): when the served folder is a registered Local
  site's `app/public` AND the config host is a bare `localhost` (Local's socket), the
  source is that site's own mysqld, reached over its SOCKET for the pre-auth probe,
  the sign-in and the dump (a 0600 defaults file with `protocol=SOCKET`), re-read
  from `sites.json` on every run. Not TCP: Local's mysqld answers a connect from
  127.0.0.1 with ERR 1130 IN PLACE of its handshake (`skip-name-resolve`,
  `root@localhost` only) — the first real Local import died unidentified on exactly
  that, 11 Sep 2026; the port is used only when no socket file exists. An explicit host or port always wins. A stopped
  Local site fails with "start it in Local" — Local runs a site's database only while
  that site is started, and rexenv never starts it. `app/sql/local.sql` is never used
  (a stale snapshot). A Local copy then gets a sixth job phase, `urls` (`skipped` for
  every other import, and a success never repaints it `ok`): `http(s)://old` →
  `https://new`, the JSON-escaped spelling, then the bare name, through wp-cli's
  serialization-aware `search-replace --all-tables` — against rexenv's COPY only, via a
  0600 `--require` file defining our engine's address, passwordless root and the
  restored database name (PHP keeps the first definition; wp-config still names
  Local's socket), `--skip-plugins --skip-themes`. It runs before the import is
  recorded, and `siteurl` must re-read as `https://<new>` or the job fails (#573).
  **When the new name CONTAINS the old one** (`shop.rex` → `feature-x.shop.rex`, or a
  rename to `myshop.rex`), the bare pass runs FIRST and through a token
  (`wordpress::REHOME_TOKEN`). In the plain order it would rewrite the `https://new` the
  URL passes had just written, giving `https://feature-x.feature-x.shop.rex` (#818, found
  9 Oct 2026 building worktree sites). Change domain uses this same pair list since that
  fix; until then it ran its own `https://old` + bare passes and carried the bug for any
  rename to a longer name ending in the old one.
  A NETWORK copy (the site recorded as multisite) first lists its blogs — a mapped-domain
  blog fails the job before any replace — moves each subsite's `http(s)://sub.old` onto
  `https://sub.new` BEFORE the bare network rename (the network's own pairs never match
  a subsite, and the bare pass alone leaves it on `http://`), runs its proof with
  `--url=https://<new>/` (their wp-config still gives `DOMAIN_CURRENT_SITE` as the old
  host, and a run with no URL finds no such site once `wp_site` moved), and requires
  every blog's URL to read `https://` on the new name. Pinning the constant in the
  require file was the first design and measured unnecessary — `--url` is what works.
  A Local copy is named after the site, `local_<domain>`, from the first import (12 Sep
  2026): every Local database is `local`, so the bare name only ever collided — with the
  next Local import, or with a failed attempt's orphan (#586) — and the connect's diff
  therefore always moves `DB_NAME` too (#574). Ledger #587.
  A re-homed row whose PICKED name another rexenv site already holds (`tr.local` →
  `tr.test` with a `tr.test` site present) carries `domain_choice` and gets a name field
  on the row — the label only, on the row's re-homed TLD, checked live against the
  scan's `takenDomains` and the other rows. The run's `choose_domain` re-checks with the
  backend's own hostname validator, `domain_taken_by` and the names this batch already
  claimed, and refuses the row without a name. Only a re-homed row can be renamed (a
  Valet/Herd name is the source's own). The outcome keeps the row's scanned name as its
  key and reports `servedAs`. Before 12 Sep 2026 such a row had no way out at all.
  **Until connected, a Local import reads NO database** — unlike a Valet/Herd import,
  which keeps reading DBngin/Homebrew: its `localhost` lands on rexenv's own socket
  (`mysqli.default_socket`), where Local's `root`/`root` is refused, so the site serves
  WordPress's database error. The rewrite preview carries that derived fact
  (`oldDatabaseUnreachable` — a registered Local docroot + a bare-`localhost` config),
  the Database panel and the job's settle line say "can't load until you connect it"
  instead of "still reads its old database", and the Sites badge asserts neither (#575).
  Measured on two real imports, 12 Sep 2026.
  **Once connected, Local serves the site from rexenv's copy too** — the linked folder
  is shared, so Local's own PHP reads the rewritten wp-config and signs in to
  `127.0.0.1:13306` as the dedicated user while rexenv's MySQL runs; its pages then
  link to the `.rex` name (measured through Local's own ports: 200, 32 `ab12.rex`
  links, 0 `ab12.local`). It does NOT break in Local, as an earlier explanation
  assumed; it follows the copy. Revert restores the byte-identical wp-config and Local
  reads its own database again (same probe: 0 `.rex`, 32 `.local`). Round trip —
  import, connect, both sites 200 on `.rex` with `signin+http`, revert, both 500 again
  — run over MCP on `ab12.local` and `tr.local` (the renamed `local_tr_local_rex`
  copy), 12 Sep 2026.
  **The batch can connect Local rows itself** (`ImportRequest.connect_local`, #576):
  after a LOCAL row's database settles `imported`, the run calls the SAME
  `rewrite_preview` → `rewrite_apply(fingerprint)` the Database tab calls (backup
  first, sign-in verified, revertable), and the row reports `connect`. Local rows
  only — a Valet/Herd site keeps reading its old server, so its connect stays the
  per-site opt-in Stage 3 settled. The screen ticks "also connect Local sites" by
  default, saying on the box that it edits wp-config; the wire default is off, and
  over MCP the flag additionally claims `destroy` (what `connection_rewrite apply`
  needs), so `run` alone can never reach a project write.
- **Database import copies their database and RECORDS what it did** (Stage 2:
  `core/{dbsource,dbcompat,dbdump,dbrestore,dbmirror,dbimport}`, v19 `sites.db_created`,
  v20 `db_imports`). Their side is read-only: engines identified from the pre-auth
  handshake (never a plist's `Status`), dumps are non-locking single-transaction to a
  0600 artifact + manifest, and their server is never started or stopped. Restore is
  provenance-FIRST: `db_created` (1 = this import created it, 0 = pre-existed → never
  dropped by any path, NULL = legacy provisioning) is written before `CREATE DATABASE`.
  A job that fails after that point drops its partial copy (ours only) from every step
  up to `finish` — the row is pointed at the restored name only on success, so until
  12 Sep 2026 a failed-then-deleted import left the copy on our engine under the
  SOURCE's name, and the next import of that name (every Local site's is `local`) met
  "no rexenv site owns it" (#586). A crash mid-job can still leave one; Retry reuses it.
  Credentials are mirrored loopback-only (`localhost`+`127.0.0.1`, never `'%'`, never
  root — reserved accounts refuse as an outcome). The settled fact is ONE serialized
  row (`db_imports`): badge, summary and panel all render it, so they cannot disagree,
  and its `state` closed set has a `connected` value ONLY the rewrite's verification
  can write. Secrets: in memory for the job only, never argv (0600 defaults file,
  Drop-deleted), never logged, never persisted.
- **The connection rewrite writes ONE user file, provably** (Stage 3:
  `core/{confedit,confverify,confrewrite}`, `commands/rewrite.rs`, v21/v22).
  `RewriteKey` is a closed enum (Host/Port/User/Name — Name, the database NAME, added
  12 Sep 2026 so a collision-renamed copy such as a second Local site's
  `local_<domain>` connects in one click instead of tell-only; ledger #574; and
  NetworkDomain, a multisite network's `DOMAIN_CURRENT_SITE`, added the same day so an
  imported network's logins work under its rexenv name — wp-config only, staged only
  when the site row says network AND the file's value differs, and the preview says the
  source tool can't load the network under its old name while connected; ledger #581) — no
  password key exists, so no plan,
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
  the tunnel feature) and creates nothing. The roots are named per OS
  (`validate_linked_docroot_on`): on Windows they are the drive root and the
  profile parent (`C:\`, `C:\Users`). **Every one of those comparisons puts a
  canonical path on both sides** — `canonicalize` returns a verbatim path
  (`\\?\C:\…`) on Windows while `BaseDirs`/`ProjectDirs`/the sites-dir setting do
  not, so the plain comparison answered false there and the home-folder and
  app-data refusals did not fire at all (measured on the Dell, 17 Sep 2026). `phase_defs` gives it
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
  **"Installed" means `vendor/autoload.php` too** (`laravel::is_installed`):
  Composer extracts `artisan` and `public/index.php` before it resolves anything,
  so a `create-project` that fails leaves a skeleton with no dependencies, and a
  Retry used to read that as "already present" and fail forever. A Retry now
  empties the folder first — only when rexenv made it (`docroot_managed`) and it is
  exactly a failed skeleton (`laravel::is_failed_skeleton`: the skeleton's own
  `composer.json` name, no `vendor/` at all) — so `create-project` runs again on
  the site's CURRENT PHP. The case that found it: on PHP 8.0/8.1 Composer resolves
  Laravel 9/10, whose framework releases are all blocked by Packagist security
  advisories; `repo::map_composer_error` names that (package + "switch the site's
  PHP") ahead of its PHP-version branch, and rexenv does not turn the block off.
- **A create refused for its SHAPE leaves nothing behind.** `sites::provision_with`
  runs `refuse_unbuildable` (server available, server runs this PHP, engine
  supported on the effective patch) before it makes the docroot, writes the
  Blank-PHP page or issues the certificate; the insert chokepoint runs it again for
  every other path. Before 11 Sep 2026 only the chokepoint did, after all three.
- `AppState` (`state/app.rs`) = db + platform + monitor + CA + ServiceManager + Terminals/
  Tunnels registries, **field-level locks** (see §5 locking rule).
- Every service start is gated by `core/ports::ensure_free`; a conflict names the holding
  process + a copy-paste free command. Attribution names the managing app (Herd, Valet —
  #449), and since 12 Sep 2026 Local's ROUTER (its nginx on :80/:443 in "Site Domains"
  mode, identified by Local's bundled nginx loading a config under `run/router/`), with
  the way out that keeps Local running: Router Mode → localhost. Quitting Local also
  stops the per-site database a Local import reads. The MCP `site_status` edge-blocked
  verdict names the same holder instead of "can't identify" (#588).
  **The gate reads the platform's socket tables before it trusts a trial bind** (#599,
  13 Sep 2026): `ports::is_free(platform, …)` refuses any port `port_holders` lists on ANY
  local address, then binds. On Windows a trial bind on `127.0.0.1` succeeds beside another
  process's `0.0.0.0`/`[::]` listener and takes its localhost traffic (plan §6, measured), so
  the bind alone would call a developer's own all-interfaces MySQL free. **macOS had the same
  hole for TCP** (measured the same day: an `SO_REUSEADDR` `127.0.0.1` bind succeeds beside a
  wildcard holder and takes its `127.0.0.1` traffic) and now reads `lsof -Fpn`, counting only
  sockets whose LOCAL end is on the port — a client talking TO the port is not a holder. A
  root holder is invisible to unprivileged lsof; the trial bind stands for it. Any local
  address counts, so a listener on a LAN-only address is refused too, by name. Windows names holders by image, hosted service,
  HTTP.sys (pid 4) or the excluded port range a holderless port sits in, with PowerShell
  commands — run on the Dell (Windows 10) 13 Sep 2026, `windows_port_gate_check: PASS`.

### 8.2 Which app opens a link — one choke point

Two settings pick the app rexenv hands things to: `preferred_editor` ("Open in
editor" → `open -a <editor> <site folder>`, so the folder lands as a PROJECT) and
`preferred_browser` (every `http(s)` link). Both are stored ids over
`ShellRunner::detect_editors` / `detect_browsers`, whose macOS impls scan
`/Applications` + `~/Applications` for a fixed table of bundles. **On Windows** (`platform/windows/
app_catalog.rs` over `app_registry.rs`, W7 S3, ledger #622) the tables carry the SAME ids, so a stored
preference names the same app on both OSes, and detection reads what installers register — App Paths, the
uninstall entries (`DisplayIcon`, `InstallLocation`), `StartMenuInternet` — before a known install folder,
keeping only executables that exist on disk; the default browser is the `https` handler's `UserChoice`
ProgId matched by prefix (Firefox's carries an install hash). Every open starts the detected executable
with its arguments and no shell in between; a browser gets `http(s)` only (`app_catalog::web_url_only`,
checked before the lookup), a private window only with a known flag, a terminal only an existing folder.
Icons are `None` for now — the UI draws its glyph.
**The words around them are the platform's too** (`platform/words.rs`, W7 S7, owner's ruling Q3, ledger
#626): "Show in Finder" / "Show in Explorer", the file manager's name, the login item's description, and the
fix a missing git, Node, Bun or native compiler gets. `PlatformWords` holds `MACOS` and `WINDOWS` as data on
every host; `current()` is this build's; the UI reads it once (`platform_words` → `usePlatformWords()`), and
`core/devtools.rs` / `core/repo.rs` build their messages from it — so no Windows screen tells a user to open
Finder or run `brew`. The macOS words are the text the app showed before, byte for byte. A test reads every
frontend source file and fails on "Finder", `xcode-select` or `brew install` written outside the platform's
words (the browser shell's mock and two dev review pages excepted).

- **The browser preference is applied in the BACKEND, inside `open_external`** —
  not by the UI. Links are opened from about a dozen call sites (site header,
  quick tiles, Sites rows, Tunnels, Mail, Adminer, magic login, WordPress
  plugin/theme rows…); a UI-side rule would mean the next call site anyone adds
  silently opens in the system default. Non-`http(s)` targets (docroots, log
  files) keep going to the OS handler unchanged — on macOS. **On Windows the handler is narrower**
  (`WindowsShell::open` over `platform/windows/shell_rules.rs`, W7 S2, ledger #621; owner's ruling 15 Sep
  2026): `ShellExecuteW("open")` RUNS whatever a file's association says — a `.bat`, a `.ps1`, a `.lnk`,
  a `.php` wired to `php.exe` — silently, so the shell gets only `http(s)` URLs, existing folders and
  existing files with a reading extension (`.log .txt .conf .ini .json .xml .md .csv .sql .yml .yaml`);
  any other file, any other URI scheme (bare ones too — `ms-settings:`, `shell:`), and a missing path are
  refused by name, pointing at Show in Explorer. A drive letter and a UNC share are paths, not schemes.
  `reveal` requires the path to exist first — explorer's exit code means nothing (1 on success, measured).
- **A network's sub-sites get the header's two actions, not two plain icons** (13 Sep
  2026, owner report): each Network-tab row has Visit and Magic Login, both with the
  browser chooser (`useBrowserMenu`). Sub-site sign-in needed a backend change, not a
  button: the login token lives in an option, and a sub-site's request reads ITS blog's
  options, so a token minted on the main site was invisible there — the row could only
  open a plain `/wp-admin/`. `wp_admin_login_url` takes an optional `blog_id`, looks the
  blog's URL up in the network's own `wp site list` (never the caller's string), accepts
  it only on this site's host or a subdomain of it (`wp_login::subsite_login_base`), and
  mints the token with `--url` on that blog (#589).
- **Installed-ness is re-checked at every open, never once at save time** — a
  browser can be dragged to the Trash any day. A preference that no longer
  resolves logs and falls back to the OS handler: the link still opens, and the
  Settings picker shows "System default" again.
- **A chosen browser takes URLs only.** `open -a <browser> <path>` displays a
  local FILE, so `open_in_browser` refuses anything that isn't `http(s)` before
  it even looks the browser up (CLAIM-LEDGER #301).
- **A third kind of app: the user's own terminal.** `ShellRunner::detect_terminals`
  scans the same folders for a small `TERMINALS` table, and the chevron beside
  every built-in Terminal control opens that folder in one of them. It is not a
  choice between equals like the browser/editor menus — the plain click still
  opens rexenv's built-in tab, and the menu is a different destination — so this
  menu appears even with ONE app detected, and there is no `preferred_terminal`
  setting to drift out of date. That shell is the user's plain login shell: no
  bundled PHP dir, no `wp` wrapper on PATH, so `php -v` there answers with
  whatever their machine has. The built-in tab stays the one that answers with
  the SITE's PHP version.
- **The bundle search includes `/System/Applications/Utilities`** for one app
  every Mac has and no Mac can move: Terminal.app. Since macOS 11 the stock apps
  live on the sealed system volume, so the `/Applications` + `~/Applications`
  search that serves editors and browsers reported "no terminal installed" on a
  stock machine (CLAIM-LEDGER #541).
- **A chosen terminal takes a DIRECTORY only.** `open -a Terminal <file>` runs
  the file as a script, so `open_in_terminal` refuses anything that is not an
  existing directory before it even looks the terminal up — the same shape as the
  browser's http(s)-only guard. The directory itself is resolved in the BACKEND
  from the site id (and the plugin/theme slug); no path crosses IPC.
- **How a terminal is told where to start is per-app and honest.** Terminal.app
  and iTerm take the folder as a document argument; the rest get their documented
  cwd flag through `open -na … --args`, where `-n` is load-bearing for the same
  reason as the private-window flags — macOS drops `--args` for an already-running
  app, and a dropped `--working-directory` opens a window in `$HOME` under a
  control that said "this plugin's folder". An app whose cwd handling we have not
  established is simply left out of the table: a missing row costs a menu entry, a
  wrong row costs a command typed in the wrong directory.
- **The chevron next to "Open in browser" is one-time.** It opens THIS url
  elsewhere and changes no setting — the default moves in Settings only.
- **Each row of that menu has a SECOND target: the same url in that browser's
  private window** (`open -na <app> --args --incognito|-private-window <url>`) —
  a logged-out look at the site without signing out of the session you are
  working in. `-n` is load-bearing: for an already-running app macOS drops
  `--args` entirely, so the url would land in an ordinary tab under a control
  that said private. It is offered ONLY for browsers whose private flag is in
  the table and has been seen to work; Safari has no private-window command line
  and its row shows nothing, and the backend errors rather than falling back to
  a normal window if one is ever asked for anyway (CLAIM-LEDGER #309). The
  private path is an ARGUMENT to `open_in_browser`, not a second function, so
  the URL-only guard is one check covering both modes.
- **Icons are the apps' real icons**, extracted from the installed bundle
  (`CFBundleIconFile` → `.icns` → `sips` → PNG data URI, cached per process), not
  a hand-drawn brand table that would hardcode vendor hex and rot on every
  rebrand. An app that ships its icon only in a compiled asset catalog yields
  `None`, and the UI draws its own monochrome glyph — honest, not invented.
  `browser_detect_check` (L1) proves detection, the default-handler read, the
  URL guard, and that the icons really decode as PNGs.

### macOS app menu → the app's OWN About (`lib.rs`, `App.tsx`, `Settings.tsx`)

- **"About rexenv" opens Settings → About, not the native panel.** The native
  macOS panel can show a name, a version and a copyright line — no commit, no
  build date, no bundled licences, no links. Settings → About already answers
  "which build is this?" (`app_info`: version · commit · built-at · platform ·
  Tauri), which is the question a stale install once turned into a whole
  misdiagnosis. Two About surfaces where one is strictly poorer is a screen that
  lies by omission, so there is one.
- **The default menu is EDITED, not replaced** (`install_about_menu_item`:
  remove index 0 of the app submenu, insert ours). Rebuilding a menu from
  scratch is how an app loses the Edit menu it never wrote — Cmd-C/V/Z come from
  the default menu, and nothing else in the app provides them. macOS-only, since
  the app submenu is macOS's; other platforms keep the default menu untouched.
- **"Check for Updates…" sits under About**, where every Mac app puts it, and does two
  things: navigates to the card AND re-checks. Someone who chose that item is ASKING, so
  landing them on yesterday's answer would be a menu item that technically worked. The
  frontend invalidates rather than calling the check itself — the card owns that mutation
  and its failure copy, and a second caller would be a second place deciding what a failed
  check looks like.
- **The update item is the ONE thing in the menu about the app itself** (ledger #535). It
  appears only when a verified offer exists, reads `Update to 0.6.0…`, and OPENS Settings →
  About — the ellipsis is the promise. It never installs: the consent sentence and the button
  live in one place, and a menu item that skipped them would be a second path past the only
  sentence telling a user what pressing it costs. The version rides in the item's id, like a
  site's domain, because the menu is rebuilt every few seconds and an index would act on
  whatever slid into that position. The model reads an in-process SNAPSHOT the app publishes
  after every successful check — never the database (drawing a menu must not verify a
  signature) and never the network.
- **The click emits `menu://about`, after `show()` + `set_focus()`.** The window
  may be hidden or behind another app; an About that opens out of sight reads as
  a dead menu item. `AboutMenuWatch` (app root, so it works from any screen)
  routes to `/settings?section=about` — a deep link Settings honours on mount
  and on change, falling back to General for an unknown value rather than
  rendering an empty pane.

## 8.1 `rex` CLI (`cli/`, `src-tauri/src/cli_server.rs`)

- **Remote control ONLY — the app stays the single brain.** The `cli/` crate (bin
  `rex`) never links the app lib: it cannot open SQLite or spawn/stop services, only
  write one JSON request line to the app's private socket and print the reply. That
  makes the examples-stop-the-edge bug class impossible at compile time.
- **Socket:** `<config>/rexenv-cli.sock`, `0600`, next to `caddy-admin.sock` — same
  trust boundary (same-user processes already own our SQLite/processes; other users
  are locked out). Never TCP. Stale files are unlinked at bind; the CLI *connects*
  to detect liveness (a stat would lie — same lesson as `admin_alive`).
- **On Windows `rex` dials the app's named pipe** (W8 S1, ledger #630; plan §5 W8 rulings Q1,
  Q5): `\\.\pipe\rexenv-app-<first 10 bytes of SHA-256 of the lower-cased config folder>` —
  the single-instance lock's own name, computed by `rex` from `%LOCALAPPDATA%` exactly as the
  app computes it (the folder measured equal to the app's Known Folder on the Dell; one test
  vector is asserted on both sides, and `rex`'s test reads the app's). An open answering 231
  ("All pipe instances are busy") is the moment between two server instances, not "not
  running": it is retried for 5 s; any other error is "rexenv isn't running", exit 2. The
  client is tokio's overlapped one (`cli/src/pipe.rs`), because a blocking handle
  DEADLOCKS `rex mcp` — measured (`windows_cli_pipe_probe` (d)): with one thread blocked
  reading, a write on the handle's clone returned only when the read did, and the next never.
  A pipe has no half-close, so the Windows `Stream` has no `shutdown` at all; `rex mcp` ends a session
  by waiting for its answers and closing the pipe (ledger #632). The crate's dependencies grew for
  it — `sha2` (the name) and, on Windows only, tokio — and #54's guard now reads every
  dependency table, not only `[dependencies]` (it would have passed a banned crate added under
  the Windows table). Piped through PowerShell 5.1, `rex`'s ✓ and — print as `Γ£ô` and `ΓÇö`
  (it decodes a program's redirected output with the OEM code page); a console is unaffected.
- **A connect proves a listener, not an answer** (learned 12 Aug 2026, ledger #300).
  A stale App-Translocated instance owned the socket, accepted connections and
  replied to nothing, so "is the app running?" said yes and `rex` blocked in
  `recvfrom` with no output and no bound. The two waits are now deliberately
  different: `soft_request` (`--version`, which must work without the app) is capped
  at 2s and degrades to CLI-version-only, while `request` stays UNBOUNDED — the app
  writes a finished reply in one write at the end, so "no bytes yet" cannot tell a
  three-minute `site create` from a dead app — and instead prints one stall notice
  after 10s. Bounding that one would break long commands to fix a rare wedge.
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
  link only when it is ours (content-checked) — unprivileged where the directory
  allows it, otherwise as an `rm -f` batched into teardown's ONE admin prompt
  (ledger #677, 18 Sep 2026: on a clean Mac `/usr/local/bin` does not exist, the
  install's `mkdir -p` creates it as root, and the "best-effort" unlink then failed
  silently on every clean uninstall, leaving a link into a deleted app).
  **On Windows the sidecar is `rex.exe` beside `rexenv.exe`** (W8 S4, ledger #633):
  `scripts/build-cli.sh` stages `binaries/rex-x86_64-pc-windows-msvc.exe` on a Windows
  host, and `core::cli::bundled_rex` looks for `sidecar_file_name(EXE_SUFFIX)` — a bare
  `rex` never exists there, and the card would have stayed hidden. The Windows install
  is not a link: a copy on the user's `Path` (plan §5 W8 rulings Q3, Q4; S5).
  **The Windows install** (W8 S5, ledger #634): `Paths::cli_install` answers
  `CliInstall::CopyOnUserPath(%LOCALAPPDATA%\rexenv\bin)` — a folder of rexenv's own,
  beside the app-data tree, so the `Path` entry exposes `rex.exe` and nothing else.
  Install copies the sidecar there and adds the folder to the user's own `Path`
  (`HKEY_CURRENT_USER\Environment`) through `ShellRunner::add_to_user_path`: appended
  once, every other entry kept as written, the value written back in the kind it had —
  the Dell's is `REG_SZ`, not the `REG_EXPAND_SZ` a fresh profile gets, measured — then
  `WM_SETTINGCHANGE`, so a terminal opened afterwards has it (one already open does not;
  the toast says "open a new terminal"). No prompt. A differing copy is renamed aside to
  `rex.exe.old` first, because a running `rex.exe` — a `rex mcp` an agent keeps open —
  cannot be overwritten or deleted, only renamed (measured); the `.old` is swept later.
  "Current" is the copy's bytes, since a copy points nowhere. A launch refreshes an
  INSTALLED copy — never installs, never touches the `Path`, never serves a dev build's
  `rex` (the login item's rule) — and teardown removes the copy and the entry.
- **App not running → hard error, exit 2** ("open the app first"). Deliberate: a
  headless CLI-spawned backend would be a second ServiceManager/SQLite writer/
  watchdog racing the GUI — the exact second-brain class the stack guard exists to
  kill. Protocol: newline-delimited JSON, one request per connection,
  `{"ok":true,"data":…}` / `{"ok":false,"error":…}`; `--json` for scripting.

## 8.3 MCP server — a second socket, and the one that EXECUTES (`src-tauri/src/mcp_server.rs`, `mcp_server/`)

Added to this file 21 Aug 2026, three weeks after M1 shipped. Until then the file
CLAUDE.md calls "read this before any feature or bug" did not know rexenv had a second
IPC surface — which is how a reader ends up designing against a system with one.

- **What it is.** An opt-in MCP endpoint so a developer's AI agent can drive rexenv:
  read-only diagnosis (M1), and disposable WordPress **scratch sites** the agent owns
  outright (M2a/M2b). In-process beside `cli_server`, second `0600` unix socket, bridged
  by `rex mcp` — a pipe that constructs no request and interprets no method but, since
  parity P6.2, reads the ids of the requests it forwards so that when the app quits
  mid-call the pending request is answered in-band with "rexenv stopped … the outcome is
  unknown … check before retrying" rather than a bare EOF a model would guess past. A call
  that carries the spec's `_meta.progressToken` gets `notifications/progress` lines while a
  provision, a repo job or a database import runs — the card's own phase labels and
  percentages, every one written before the reply — and a call without the token gets
  exactly what it got before.
  M3 (database access, agent principals, the first consent dialog)
  SHIPPED 24–25 Aug 2026 (#398–#404) — this bullet said "not built" until 3 Sep 2026,
  nine days after, while the `db_query` bullet below described the shipped thing. Parity
  (`docs/archive/PLAN-mcp-parity.md`) shipped 3 Sep 2026, P1–P7; D15 replaced its per-site grants
  with one dial the same day, and **D16 (4 Sep 2026) retired M3's consent dialog and the
  mail sub-toggle** — both were reads, and Read is what the endpoint being on means.
- **The honest guarantee, first, because it constrains everything below.** This is **not
  a sandbox** (ledger #197). A read tool cannot mutate rexenv's state; an executing tool
  runs the user's own code — `wp eval`, `wp db query`, `wp plugin install --force` — as
  the user, inside a site the agent owns. The tier bounds **which site**, never **what
  code**. `core/scratch.rs` says it in one line: *"It is not containment."* What the
  tiers deliver is a safe paved road and no silent amplifier.
- **Transport.** `<config>/rexenv-mcp.sock`, `0600`, sibling of `rexenv-cli.sock`, never
  TCP; newline-delimited JSON-RPC 2.0 with a 4 MB line cap. Unlike the CLI socket — one
  request per connection — an MCP connection is a **long-lived session**. The protocol
  core (`initialize`, `tools/list`, `tools/call`, `ping`) is hand-rolled; there is no SDK
  dependency. The unlink at teardown uses the path the listener actually BOUND
  (`local_addr`), never a re-derived one.
  **It does not share `cli_server::bind`**, though a comment and ledger #198 said so for
  weeks: that binder returns a tokio listener and panics off-runtime, which was a
  packaged-build enable crash. The *convention* is shared; the binder is its own, and a
  test pins that binding needs no ambient runtime.
  **On Windows the endpoint is its own named pipe** (W8 S3, ledger #632; plan §5 W8 ruling Q2):
  `\\.\pipe\rexenv-mcp-<the lock pipe's digest>`, owner-only, remote clients rejected. `start`
  creates its first instance synchronously (`platform::create_mcp_pipe`, the lock's own
  `create_first`) — `mcp_set_enabled` runs off the runtime, the same lesson as the socket binder
  — so a failure reaches the toggle and it stays off. `serve_pipe` makes the next instance
  before a connected one goes to its `session`, and leaves its loop on the toggle's signal,
  which every session shares: once they have ended no instance of the name exists and a
  connect finds nothing, the Windows form of the socket file being unlinked. Access denied on
  the create means an instance already exists — with the lock held, something other than this
  app — and the endpoint does not start. `rex mcp` tells an endpoint that is off from an app
  that is not running (it asks the CLI endpoint), on both OSes, and on Windows ends a session
  at end of input by waiting for every sent request's answer and closing the pipe — the pump
  marks a reply answered only after writing it, so the process cannot end between the two.
- **A create that half-builds tells the agent WHY.** `site_create` and `scratch_create_site`
  name the site that now exists (so the agent does not make another) AND carry the
  provision job's own reason through `view::create_failure_reason` — the app's
  `full log:` path and its "Retry it from the app" line dropped, every other line
  through the path scrubber. Until 11 Sep 2026 both dropped the whole error to keep that
  path out and pointed at `tail_log`, which reads only a WordPress debug log, so no tool
  held the reason at all (ledger #558).
- **Parity is a test, not a table in a plan.** `mcp_server/parity.rs` derives which
  registered commands the MCP source reaches (`commands::<module>::<name>`, as a call or a
  function value) and demands a written ruling for every other one: the `Tool` that reaches
  the same core function, the reason it is `Never` a tool, or a known `Gap` (the TODO row).
  A new command cannot ship unruled, and a ruling for a command that no longer exists — or
  that the MCP source now reaches — fails as stale. Built 11 Sep 2026 after fifteen
  post-parity commands had arrived with no one asking what an agent may do with them;
  the first run ruled 95 of the 238 commands (the other 143 the MCP source reaches
  directly): 32 reachable through a tool, 33 never, 30 gaps (#560). Counted off the table,
  not typed — the parity plan's own tool counts were wrong twice for that reason. Closing a
  gap is building its tool — the guard then demands the row go. The first four, same day
  (#561): `stack` `catch_mail` (manage) / `stop_catching_mail` (system — every site's mail
  goes to real addresses) with `stack_status.mailCatchAll`; `tld` `status` / `policy`
  (read; owner and site count, never the resolver file's path or contents); `open`
  `terminal` (the site's own folder by site id). Then the health reads (#562):
  `stack_status` gains the running build (`app`), start-at-login, unresolvable TLDs,
  Firefox trust (counts only) and FrankenPHP's PHP; `site_info` gains the site's
  resources from the Sites page's own computation — minus the database size, because
  that runs a database client and a read tool runs nothing (`resources_of`'s
  `include_db_sizes`). Both computations moved into core (`core::app_info`,
  `core::site_metrics::resources_of`) rather than the read bridge reaching
  `commands::`: the M1 boundary test failed the first version that did. Then (#563):
  `wp_info` `core_versions` (WordPress.org releases) and `cli_packages` (the directory as
  `<home>/…`); `site_logs` `clear` — `destroy`, because the server, PHP and database logs
  are shared and clearing one empties every site's lines; and `repo` `watch_log`, only for
  a watch running on THAT site, lines scrubbed. The last eleven (#564) are `stack` actions
  scoped per action: reads `adminer`, `downloads`, `edge_conflict`; manage
  `adminer_update_check`, `adminer_theme`, `prefetch`, `retry_download`; system
  `set_engine_version` (a running engine restarts and every site on it is offline meanwhile),
  `adminer_update_apply`, `autostart`; with `stack_status.engines` naming the offered and
  selected versions. The table has no `Gap` row since. `init_error` and `startup_notices` were re-ruled `Never`: with no
  AppState the server answers "still starting", and reading the notices drains the
  person's toasts.
- **Three registries, and the registry IS the capability.** `mcp_server/tools.rs` holds the
  eleven read-only tools (`list_sites` — widened with parity to carry owner / multisite /
  xdebug / aliases / setup-complete / linked, `site_status`, `tail_log`, and from parity
  P2.3 `site_info` — cert validity without its directory, packages without their source
  path, the M1 verdict — and `site_inspect_folder`, the New Site dialog's own preflight,
  which classifies a folder and runs nothing, `wp_org_search`, a public network read, and
  `stack_status` — `rex doctor`'s composite with no path, pid or socket in it, `settings_get`
  — one key through the CLI's allow-list, a denied key refused with the reason, and
  `php_settings` — a pool's ini overrides, `blueprints_list` and `agent_activity` — the
  user's own audit view, readable by the agent too);
  `mcp_server/scratch.rs`
  holds the ten executing ones (`scratch_create_site`, `scratch_delete_site`,
  `scratch_add_package`, `scratch_sync_package`, `wp_run`, `set_php_version`,
  `scratch_login_url` — D2 settled 5 Sep 2026: the app's own magic login, single-use, no
  password, minted for scratch sites here and for the user's sites through `wp_user` at
  Read — the free level, by the owner's ruling: what the session may then do is
  WordPress's own capability model — never in the feed (#515) — `db_query`, `mail_list`, `mail_get`); `mcp_server/user_sites.rs` (MCP parity, 3 Sep 2026) holds the
  tools that act on the USER's own sites and the stack — `site_create` (`manage` on rexenv
  itself; a WordPress / blank-PHP / Laravel site through the app's own provision job,
  recorded as the USER's via `Ownership::UserByAgent`, which never prompts) and
  `site_delete` (`destroy`, session-only; the app's full delete), `site_configure` (`manage`
  on the site; twelve actions, each exactly one app command — rename, php, server, xdebug,
  env set/unset as a merge whose VALUES never come back, domain change, extra domains,
  move, relink, cert), `site_restart` (#444's three outcomes in words, ports dropped) and
  `site_retry` (the job's own text through the one scrubber, and the SETTLED outcome —
  `site_provision::settle`, the wait `create_site_owned_with` also uses; until 11 Sep 2026
  it answered with the job's first snapshot, `running`, #559), and from P3 the grouped
  WordPress tools on the user's own sites — `wp_info` (nine reads under `read`),
  `wp_plugin` and `wp_theme` (the scope decided PER ACTION from one table: `list` reads,
  `delete` destroys, the rest manage; a scratch or non-WordPress site refused BEFORE any
  ask; every action exactly one `commands::wordpress` command, so the vetted argument
  hygiene in `core::wordpress` applies unchanged), `wp_user` (list/super-admins read;
  create with a password rexenv generates and shows ONCE, roles, a one-time login link
  and super-admin manage; a password reset and a delete destroy — the #446 posts fork is
  restated BEFORE any ask), `wp_option` (six vetted writes, all manage) and `wp_maintain`
  (flushes, cron, checksum cleanup, core update/reinstall/repair manage — `core_repair` is
  the Site screen's Repair core files, #793; `core_switch` destroys;
  every wp-cli reply through the one scrubber), `wp_data` (exports named by FILE and
  located in words; a dry-run search-replace manages, a live one destroys; import and
  reset destroy), `wp_network`, and `site_wp_run` — the raw runner on the USER's site
  behind `run`, which is `scratch::wp_run`'s resolver, target screen, runner and scrubber
  reused rather than copied, and from P7 its Laravel twin `site_artisan` (the project's own
  `artisan` on the pool's PHP, `--no-interaction` last, stdin null; refused on the row for a
  non-Laravel, scratch or half-installed site) and `composer_link` (a `path` repository that
  is an explicit SYMLINK — D14, the S1 question re-run for Composer and ruled the other way
  because the site is the user's and `run` was their answer; the name read from the source's
  manifest, the source behind the scratch clone's blast-radius rule, the write-back truth
  said in the reply), `site_logs` (every source the site's Logs tab shows, by key
  from the site's own closed list, under `read`) and `mail_inbox` (the user's whole
  Mailpit inbox: `read` on rexenv itself for list/get/raw — free at the dial's Read
  since D16, no switch in front — `manage`/`destroy` for mark-read/delete/clear; the
  one scrubber redacts login tokens, WordPress reset keys and cookie headers and the
  reply says what it does not remove), and from P4 `stack` — start/stop of
  the whole stack under `system` (the ONE arm that reaches `run_privileged`; the macOS
  dialog it raises is a second consent the agent cannot give, and the tool says so), a
  web-tier restart, an engine or the mail catcher under `manage`; `php` (install/uninstall,
  ini overrides, update check under `manage`; the default version and a pool swap under
  `system`); `settings` (a write through `rex config`'s own allow-list, refused with its
  reason BEFORE the gate for a read-only or denied key, under `system`); `tld` (set / repair /
  remove — three resolver writes, `system` plus the password dialog); and `open` (the site's
  own URL or folder in the user's browser, editor or Finder, under `manage`; never an
  arbitrary URL or path); and `share` (D6's reopening conditions met one by one: the
  owner's demand, `run` on the site — after D17 that is the DIAL at Full, the level that
  already says an agent may run code of its choosing as the user; there is no per-call
  prompt left anywhere in the app — and a
  bounded auto-stop the app runs, ≤60 minutes, dying with the app like every tunnel; `status`
  under `read`), and `blueprints` (save `manage`, delete `destroy`, by NAME, the spec validated
  on shape first), and `repo` (the site's Repo tab — twelve reads under `read` on the site,
  two under rexenv itself, and everything that clones, links, runs git, scripts, install
  steps, archives or watches under `run`; job-shaped actions block until the job settles
  by the CLI's own rule and reply with steps, the archive's FILE NAME and the scrubbed log,
  never a path); `valet_import` (scan/drift `read`, the import `run`, the two resolver
  writes `system`); `connection_rewrite` (preview `read` with the diff scrubbed and the
  file NAMED, apply and revert `destroy`, the fingerprint binding apply to the previewed
  bytes); and `db_import` (status `read`, start `destroy` — it DROPS the site's database —
  blocking until settled, leftovers deleted by NAME only) — each through a
  `Granted<S>` scope witness (`core::agent_grants`) — **minted by the Agent access dial and
  nothing else (D15, 3 Sep 2026; D17 took publishing, the last exception)**: one global level, Read / Changes / Full, with a duration (this session, 7
  days, always); `manage` and `system` need Changes, `destroy` and `run` need Full, reads are
  free whenever MCP is on. The grant row, the ask list and `authorize` are retired (#468-#474); `agent_site_grants` stays in the schema as history. Shape refusals
  (a bad type, a taken domain, `multisite` on a PHP site) come BEFORE the gate and record no
  ask, so a typo is answered as a typo and never as a permission prompt.
  What a tool may do is decided by **which registry its name came from** —
  never by a field the tool sets about itself. **One enumeration** (`every_tool`) feeds
  dispatch, `tools/list`, the leak sweep and the disjointness guard, and a source guard
  fails the build on a registry module missing from it — because the old shape (each
  consumer naming "both" registries by hand) is exactly what a third registry would have
  let drift: listed but not swept, or swept but not dispatched. A guard proves the registries are disjoint and, on
  a collision, names the offender and the file it belongs in.
- **`db_query` is the one scratch-registry tool that also reaches a site the agent does
  not own** (M3; D16 changed WHO answers, not WHAT). Ownership decides the principal, not
  the domain: a scratch site the agent created gets `rex_agent_*` with `ALL` on its own
  disposable schema, and the USER's own site gets `rex_ro_*` with `SELECT` on that one
  database — answered by the **Agent access dial at Read**, the dial's floor while the
  endpoint is on, so there is no prompt, no per-client grant and no expiry in the
  decision. It is still one pure function (`core::agent_db::authorize`) that runs before
  anything is opened; the refusal it would give below Read names the dial. The query
  goes through a native MySQL driver (`core::agent_query`) and never the bundled client —
  that client interprets `system`/`\!`/`source`/`tee` before the server sees a statement,
  so feeding a `GRANT SELECT` principal through it would be shell-exec and file-write on a
  real site. A source guard holds that apart. What the user consented to is said ONCE, in
  the paragraph above the endpoint toggle — every site's database, read-only, password
  hashes and API keys included — and pinned there by the copy guard. **The M3 consent
  dialog, `agent_db_grants` as a grant, and the database auto-allow (#402, #404, #408) were
  retired by D16 (4 Sep 2026)**: the owner's brief was that a local dev tool's reads need
  no door, and the parity auto-allow had already gone the same way (#470). The table
  survives as the record of which principal was provisioned for which site (one row per principal, written after the provision), so
  the site's delete path still drops an account made under a domain the site no longer has.
- **The read-only boundary is a TYPE, and its scope is the handler.** A read handler
  receives a `ReadCtx` (`mcp_server/readctx.rs`) — one private `&AppState`, five read
  methods, no mutating method to reach. A source scan over both `tools.rs` and
  `readctx.rs` fails on `core::`, `commands::`, `ServiceManager`, `Command`, `std::fs`,
  `run_privileged` and their neighbours. **Stated precisely because the wider reading is
  false:** every `tools/call` writes two rexenv records — the activity feed row and the
  named scratch site's TTL touch — from the SESSION layer, which no handler can reach. "An
  M1 call writes nothing" is not the claim and is said nowhere.
- **Scratch sites: recorded ownership, a witness, a TTL and a cap.** `sites.origin` (v27)
  is written at INSERT and never derived from a name or a path; anything that is not
  `agent` reads as the user's. An executing tool can only reach a site through
  `ScratchSite`, a witness whose field is private and whose only constructor is `claim()`
  — applying an agent tool to a user's site is a **compile error**, not a runtime refusal
  (ledger #208). `still_the_agents` re-reads immediately before each destructive step,
  because the user may have pressed Keep since. TTL is 24h and the cap is 5 BY DEFAULT — since
  MCP parity P4.3 (3 Sep 2026) both are gated settings (`scratch_ttl_hours` 1–168,
  `scratch_cap` 1–20, `core::scratch::{scratch_ttl_hours, scratch_cap}`, validating setters
  in `GATED_SETTERS`, so `rex config set` and the agent's `settings` tool share the range
  check); a stored nonsense value reads as the default, never as zero.
  A TTL touch can only MOVE an expiry, never start one, and its SQL carries
  `AND origin='agent'`.
- **The reaper: skip, never stop.** A launch sweep plus an hourly loop deletes expired
  scratch sites, at most five per pass, and **skips a site that is currently shared
  through a tunnel** rather than stopping the share (#29) — then surfaces it. Its feed
  rows are `actor='rexenv'`, and the banner says "Nothing of yours was touched".
  **Users promote; agents cannot.** Any user-facing site mutation promotes through one
  choke point (`promote_if_scratch`); `set_php_version` deliberately routes around it,
  and a guard asserts it stays that way — an agent promoting its own site would clear the
  expiry and free a cap slot, making switch→create unbounded (ledger #223).
- **Opt-in twice — the endpoint and the dial — and nothing else asks; never ambient.**
  `mcp_enabled` (absent = off) BINDS FIRST and persists second, so the toggle can never
  read on while nothing listens; disabling drops the accept loop and every live session
  mid-idle, then unlinks the socket file. **The scratch-mail stamp rides the endpoint**
  (D16, 4 Sep 2026; the separate mail sub-toggle it rode before was a second consent for
  a read): `sync_scratch_mail_stamps` writes the `wp_mailtag` stamp into every scratch
  site after a successful bind and again at launch when MCP is already on — the backfill
  for a machine upgraded with it on — and removes it at disable, which is what eliminates
  "this site predates the feature" as a category. The scratch tools' fail-closed
  direction is unchanged: `mail_list`/`mail_get` return only stamped mail and refuse an
  unstamped site rather than guess (one predicate filters the list and gates the fetch,
  because Mailpit ids are global); the whole inbox is `mail_inbox`, a Read, scrubbed and
  labelled as such. With the endpoint OFF the card renders nothing below the toggle.
  The second consent surface (MCP parity, 3 Sep 2026)
  is the **Agent access dial** (`core::agent_access`, D15): NOT a switch plus per-site
  prompts — that shape shipped first, and the live run the same day needed six clicks for
  one site's ordinary work — but one global level with a duration. Read (the default, free),
  Changes (`manage` + `system`; the macOS dialog stays the second consent), Full (`destroy` +
  `run`; a shell as the user, which is why it is not in Changes). This session dies at the
  next launch, 7 days carries an expiry stamp and reads as Read once passed, always is a
  durable setting; an agent's `settings` tool and the CLI refuse the three keys. The one
  consent still a click is publishing a site (`share`): the `agent_site_grants` row (v43),
  session-only, asked for in the card's **Site access** section and revocable there. The
  dial's label is one Rust constant the refusal and the card both use.
- **The feed is complete by construction.** Every `tools/call` outcome is recorded at ONE
  place in the session loop — including unknown tools and unparseable messages — before
  the reply is written. Rows are typed (`agent_actions`, v26/v28/v30): an unrecognised
  actor reads as **rexenv**, never as the agent; a row names a site only if such a row
  exists; the only argument stored is the target, and rexenv's own knowledge of what it
  acted on beats what the agent asked for. `args_summary` is clamped **at the writer** to
  two `[a-z][a-z0-9-]{0,19}` tokens — a security property, not tidiness: the charset
  excludes every character a forged `rexenv · automatic` row would need. Cap: 2000 rows,
  pruned on every write. **The same writer appends one line to `<log_dir>/mcp.log`**
  (5 Sep 2026, #516) from the values the INSERT got — actor, client, tool, the clamped
  summary, outcome, the site's domain, the bounded detail — so the file is the feed's
  readable form, never a second rendering of a call; the Logs tab shows it as "AI agents
  (MCP)", and `rex logs mcp.log` tails it. Unset (lib tests, the `rex` process) the
  table is the only carrier.
- **One scrubber, and it says what it does not cover.** `view::scrub_log_line` is the
  single redactor (login tokens, cookie values, and rexenv's own path prefixes derived
  from `Paths` rather than a hand list); a guard asserts there is exactly one definition
  and that both output doors call it. Its claim is "rexenv's own paths are removed",
  never "no path escapes". A leak sweep runs EVERY registered tool in both registries
  against planted secrets, and a tool cannot be registered without declaring the args
  that sweep uses.
- **Proof:** ledger #197–#227 and #301; `mcp_socket_check` (stack — a spec-literal
  handshake over the real socket), `mcp_scratch_check` and `mcp_secret_sweep` (sandbox),
  `mcp_control_check` (sandbox — the OFF switch drops a live session and removes the
  socket FILE), `mcp_mail_check` (service — it brings its own Mailpit rather than planting
  test mail in the user's real store). **What is NOT proven here is the human half**:
  `docs/SMOKE-TEST.md` §M2a/§M2b carry four HOLDs that have never been recorded as run.

## 9. WordPress layer

- **An offered PHP that gets no security fixes says so, where it is chosen.**
  `core::php::security_end` holds php.net's published END DATES and `eol_since`
  compares against today, so the status is computed rather than remembered — rexenv
  offered 8.0 from Nov 2023 and 8.1 from Dec 2025 with no tell of any kind. It rides
  the registry row (`PhpVersionView`, derived per read) into the Settings badge, the
  create-dialog note and the site's own Environment card; the client formats and
  decides nothing. For WordPress the note names WP's own outdated-PHP notice in
  advance, because the same sentence met first from us is information and met first
  from WordPress is a bug report. A new minor without a date fails the build.
- Provision (`core/sites.rs`): docroot + cert + DB + config gen + WP core install via
  WP-CLI. WP-CLI always runs PHP with `-d memory_limit=512M` (core extraction OOMs at
  PHP's 128M; the pools run rexenv's own defaults — 1G / 5G uploads / 8G post / 5000
  input vars / 60 s — written into every pool config, `php::effective_settings`, owner
  ruling 19 Sep 2026). Switching PHP version or web server = config regen + reload, never a
  docroot/cert/DB rebuild. Domains are validated in core before becoming a
  path/config/cert/DB name (M7).
- **dist-archive's scratch has no whitespace in its path** (`dist_archive::scratch_root`, #679,
  18 Sep 2026). The tool hands `zip` an unquoted `-i@<TMPDIR file>`; under app-data that
  path is `…/Library/Application Support/…` and every Build zip on every Mac died at the
  space — invisible on the dev machine because the examples build under `temp_dir()`. The
  per-run scratch now lives under the OS temp dir (checked whitespace-free; a machine with
  no such dir is refused with the reason), swept by the same `Drop` guard as before (#232).
- **The COMMAND SET is pinned, not just the phar (#228):** every `wp` rexenv runs FOR
  A USER has `WP_CLI_PACKAGES_DIR` pointed at a rexenv-owned path, so a user's
  `~/.wp-cli/packages` never extends it. Without that, a bug in any wp-dependent
  feature could depend on a directory appearing in no log, diff or bug report —
  measured: `wp dist-archive` answering from a package installed on a laptop in
  Dec 2021. Three things make it hold: (1) there is ONE argv builder
  (`wordpress::wp_argv_prefix`) and ONE `Command::new(php_bin)`
  (`wordpress::wp_command`), because the coverage claim must be a property of the
  tree — the ledger row listed four spawn sites when there were seven; (2) the
  pinned value is a FILE beside the phar, so `wp package install` cannot quietly
  turn it back into a real packages dir; (3) `core/terminal.rs`'s `wp` wrapper is
  DELIBERATELY exempt — that is the user's own command line, pinning it would break
  `wp package install` from inside rexenv, and the exemption is what makes the tell
  ("they still work in rexenv's terminal") true. Streamed spawns take the pin
  through `wp_packages::with_pinned_packages`, which REMOVES any inbound value
  rather than relying on last-wins ordering.
- **…and the TELL that makes the pin a fix rather than a trade (#301):** taking a
  capability away silently is the same unreproducibility pointed the other way, so
  when a packages dir exists that WOULD have contributed, rexenv says so — a standing
  Settings card (the fact) and an explanation APPENDED to `not a registered wp command`
  at the two sites a user or agent meets it (the answer). Appended, never substituted:
  WP-CLI's own line is what gets pasted into a search box. Names come from that dir's
  `composer.json`; every way that read can fail yields NO names and a variant claiming
  no count, because "the 0 packages" invites a false conclusion about the reader's own
  machine. The card reads the LOGIN-SHELL `WP_CLI_PACKAGES_DIR`, not the app's — rexenv
  is Finder-launched, so a user's export is invisible here and visible to every
  streamed spawn.
- **STDOUT is the command's, at both ends (#316 tail, #317 head):** wp-cli's stdout is
  not only wp-cli's, and it gets written to from both directions. Both arrived as the
  same report — "the WordPress tab is dead on this site" — and they need different
  fixes, so they are recorded separately.
  **The TAIL (#316):** a plugin can write from a shutdown hook, AFTER the command's own
  output. Measured 14 Aug 2026: Elementor 4.2.2 registers its own WP-CLI logger and
  prints the notices it collected (`Manager::shutdown` → `Cli_Logger::save_log` →
  `WP_CLI::log` → `fwrite(STDOUT)`), so `wp plugin list --format=json` returned valid
  JSON with a deprecation notice glued to it, the WordPress screens said `bad JSON:
  trailing characters at line 1 column 814`, and nothing on that site could be managed
  (`wp option get home` was wrong the same way — a stdout problem, not a JSON one). Two
  plausible fixes were measured and neither touches this half: `-d
  display_errors=stderr` (the write is not PHP's error display) and an output buffer
  opened at shutdown (the write is not `echo`). What holds whatever a plugin writes WITH
  is POSITION: every CAPTURED spawn passes rexenv's own `--require` file, wp-cli loads
  it before WordPress and before any plugin, so its `register_shutdown_function` is
  first in the queue and its marker prints after the command's output and before
  anything a later hook writes. Captured stdout is cut there and the tail is APPENDED TO
  STDERR, never dropped.
  **The HEAD (#317):** PHP's CLI SAPI prints its own diagnostics to STDOUT, so a
  deprecation raised before wp-cli prints anything lands in FRONT of the answer.
  Measured on PHP 8.5.8 with the pinned 2.12.0 phar: `Deprecated: Case statements
  followed by a semicolon (;) … react/promise/src/functions.php on line 369` — the
  phar's own vendored code, on every command, so every 8.5 site broke with no plugin
  involved. Here `-d display_errors=stderr` IS the fix, in the shared argv prefix: it
  covers any diagnostic from any file at any moment, which no marker can, and loses
  nothing (streamed steps merge both streams into one live log).
  **The THIRD door (#380, 23 Aug 2026): a plugin that plainly `echo`es while the
  command runs.** Neither fix reaches it — `echo` is not PHP's error display, and the
  bytes land in FRONT of the answer, where a shutdown marker cannot cut. It is still
  not RECOVERED from, for the reason `json_from_wp` already gave: skipping to the first
  brace means guessing which one starts the answer, and a wrong guess returns plausible
  data instead of an error. What changed is that the failure stopped being anonymous.
  If a valid value parses from a later offset, that is proof the output had junk in
  front of it — so the guess drives the MESSAGE and never the data: the error says how
  many bytes preceded the answer, quotes them (bounded, control characters flattened),
  names the likely cause and says why rexenv will not skip past. All three doors
  arrived as the same report — "the WordPress tab is dead on this site" — and this is
  the one that can only ever be reported.
  Scope stated rather than implied: the marker cut is the captured path only (streamed
  output is a live log, where a marker line would be the defect); the JSON reads
  additionally tolerate trailing
  bytes as the belt for a machine where the require file could not be written; and
  `wp cli info`/`--info` never run shutdown functions and so get no marker — harmless,
  because that path never loads WordPress.
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
  paths contain spaces). **Engine-dispatched since PostgreSQL joined the site
  engines (9 Sep 2026, `docs/archive/PLAN-postgres-sites.md`):** the five site-DB operations
  — create / drop / import / export / sizes — are METHODS ON `DbEngine`, and
  `core/database.rs`'s free functions are `pub(crate)` so nothing outside the crate
  can pick a wire protocol by hand. MySQL and MariaDB share one implementation
  (same protocol, same flags, same SQL — only binaries and port differ); PostgreSQL
  has its own in `core/postgres.rs`, because it has no `CREATE DATABASE IF NOT
  EXISTS`, needs `WITH (FORCE)` to drop a database something is connected to, quotes
  identifiers with `"`, and — the one that produces a WRONG answer rather than an
  error — **`psql` exits 0 on SQL errors unless `ON_ERROR_STOP=1` is set**, so every
  `psql` invocation carries it. The `SqlClient` type carries the engine it was
  resolved for and the ops dispatch on THAT, so a `psql` handed to a MySQL op is a
  named refusal rather than a subprocess failing on foreign flags. Clients and dump
  tools still come from `DbEngine::sql_client_bins(platform, effective_version)` —
  the site's `db_engine` + the engine's selected version, so the client always
  matches the running server. Status polls use `cached_sql_client` (strictly offline — never a
  download from a poll). Start-all spawns MariaDB **or PostgreSQL** exactly when some
  site's database lives there — one loop over the optional engines, since asking
  about only one of them is what shipped a PostgreSQL site with nothing listening
  on 15432 (#549); the download planner asks the same question, because a spawn
  under the services lock must hit cache (#175). Per-site Adminer deep links
  carry the site's engine (the wrapper's loopback gate covers all three ports).
- **PostgreSQL is started through `pg_ctl` where a spawn can carry an admin token.**
  `postgres.exe` REFUSES to run under an Administrators token ("Execution of PostgreSQL by
  a user with administrative permissions is not permitted"), and on Windows every process
  gets one when UAC is off — measured 20 Sep 2026 on a clean VM with `EnableLUA=0`, where
  the engine respawn-looped to `gave-up` while the UI said only "did not start within 15s".
  `initdb` survives it by re-executing itself under a restricted token; `postgres.exe` has
  no such code, and `pg_ctl` is where PostgreSQL keeps it, so rexenv launches through
  `pg_ctl` rather than reimplementing `CreateRestrictedToken`. It detaches, so the handle is
  `Proc::Detached` — a pid with the SPAWN TIME, because it is ours: `Adopted` looked right and
  cost a shipped bug, since an adopted process is never "starting" and the watchdog respawned a
  cluster still coming up, then refused its own port (#701). Shutdown is
  `pg_ctl stop -m fast` — a hard kill of the postmaster
  there leaves its backend processes holding the datadir and the port. The switch is a
  platform CAPABILITY (`ProcessSupervisor::may_spawn_with_admin_token`), never a `cfg`:
  `core/` may not name an OS. Ledger #698.
- **Tool results ≠ app errors:** `wp core verify-checksums` exits 0 even with
  "should not exist" extras (verified live) — verdicts derive from PARSED findings,
  never exit codes alone; extras triage as benign only when the basename is known OS
  noise (`.DS_Store`, `._*`, …), and unknown warnings stay loud.
- **Core downloads are always a `.zip`, and the tarball's casualties are found** (ledger #604,
  #793). WP-CLI's default `.tar.gz` is extracted by PHP's PharData, which cuts every member
  name past 100 bytes (`wordpress/` counted) — rexenv 0.4.0–0.7.1 created sites missing the
  long-named `php-ai-client` classes, silently on macOS. Since 0.7.2 every `core download` is a
  zip (`core_zip_url`, one place). For the sites already made, a WordPress site's Overview
  shows a warning with **Repair core files** (`CoreFilesBanner` → `wp_core_cut_names` →
  `wp_core_repair_cut_names`: `core_reinstall`, the version's no-content zip, when a core file
  is missing) and `rex doctor` prints a `WP core` finding.
  The check compares the docroot with wordpress.org's file list for its version
  (`wporg::core_file_list`, one GET per version per run): an offline "looks cut" heuristic is
  impossible because real names sit at exactly 100 bytes. Offline the banner stays silent and
  doctor says `not checked`, never clean. The no-content zip carries no `wp-content/`, so the
  cut default-theme fonts and patterns (14 on 7.1) come back one file at a time instead
  (`wp_core_repair_cut_names`): `wporg::release_file` fetches each from the release's tag on
  `core.svn.wordpress.org` and `wordpress::restore_release_file` writes it only if it is
  absent, only if its MD5 is the checksums list's, never through a symlinked folder. A theme
  whose folder the user deleted is not counted. That host answers 403 to a request with no
  User-Agent, which the `wporg` client now always sends.
- **WordPress detection survives a stopped stack** (`core::wordpress::wp_info` +
  `wp_presence`, 5 Sep 2026): `core is-installed` is the ONE probe of the three that
  needs the database — `core version` reads `wp-includes/version.php`, `config get`
  parses `wp-config.php`, both fine with MySQL down — and it exits 1 for "not
  WordPress", "database dropped" AND "database server unreachable" alike. Reading the
  exit code alone therefore called every WordPress site not-WordPress the moment the
  stack stopped, and the SiteDetail page cached that `false` for 60s with no focus
  refetch, so the report was: open a WordPress site while stopped, Start all, and the
  WordPress tab and Magic Login stay gone until you navigate away. Two halves now:
  the backend reads stderr and treats WordPress's own "Error establishing a database
  connection" sentence as files-present (whether the INSTALL finished is unknowable
  without the database, so on-disk is the honest answer in that state), and the page
  re-asks `wp-info` on a KNOWN not-serving→serving transition of that site (the
  2s `sites-serving` poll it already runs) so version/multisite are re-read with the
  database up. The tab/Magic Login rule is also `wp ? wp.isWordpress : site.type ===
  "wordpress"` — only a live answer that ARRIVED overrides the recorded type; a failed
  fetch used to count as resolved and read as not-WordPress.
- **Mail:** php-fpm `sendmail_path` (DOUBLE-quoted in the pool ini — the parser strips
  bare quotes and app-data paths contain spaces) → Mailpit's `sendmail -t -S
  127.0.0.1:11025` shim → SMTP sink; inbox UI reads the HTTP API on 18025 (`core/mail.rs`).
  - **The Database Browser's URL is the PLATFORM's, before any CSP matters.** macOS serves
    the `rexdb:` handler at `rexdb://localhost`, WebView2 at `http://rexdb.localhost/`; the UI
    takes the origin from `PlatformWords::db_browser_origin` rather than spelling one. Building
    the macOS shape everywhere left the Windows iframe pointing at nothing — no request reached
    the handler at all, which is why two correct CSP fixes changed nothing (#703).
  - **The Database Browser needs BOTH CSPs to name the same frame.** The app's own
    `frame-src` (tauri.conf.json) must list the proxy scheme as the WEBVIEW serves it —
    `rexdb:` on macOS, `http://rexdb.localhost` on Windows — and Adminer's replayed
    `frame-ancestors` must name the app's origin (#699). Fixing one and not the other still
    renders an empty panel, which is how 0.8.4 shipped (#702); `connect-src` already carried
    `http://ipc.localhost` for exactly the same reason.
  - **The self-update never closes the app on its own.** The apply swaps and RETURNS;
    the app then says "rexenv X is installed — it will now close and open again", and the
    OK button is what calls `app_update_restart`, which arms the relaunch and quits through
    the ONE quit gate. It used to `app.exit(0)` the moment the swap landed, so the window
    vanished under the user's hands with no warning (owner, 20 Sep 2026). Between the two
    the app runs on a bundle already replaced on disk — the state the relauncher was built
    for — and dismissing the dialog is allowed: the sentence says the new version starts at
    the next ordinary launch, which is what happens. Ledger #700.
  - **The HTML preview's isolation is in the DOCUMENT, not on the frame — because a
    sandbox costs the clicks.** The frame shipped with `sandbox=""`, which reads as maximum
    isolation and also switches off top-level navigation and popups: every link in every
    HTML mail did nothing when clicked, for the screen's whole life (19 Sep 2026).
    `sandbox="allow-same-origin"` plus a listener from the app is not the repair either —
    **measured in WebKit**: the parent can read the sandboxed document and never receives
    its clicks. So `safeEmailHtml` prepends a `Content-Security-Policy` meta with
    `script-src 'none'` as the FIRST thing in `<head>` (the engine enforces it, whatever a
    parser misses), sweeps `script`/`iframe`/`object`/`embed`/`base`/`form`/meta-refresh,
    strips every `on*` handler and every `javascript:` URL, and drops `target` from anchors;
    the frame then renders that with no sandbox attribute. Every click is prevented; only
    `http`/`https` reaches `openExternal`, anything else is named in a toast rather than
    handed to the OS opener (ledger #697, `scripts/wk-checks/maillink.js`).
  - **The catch-all is TWO mechanisms, because one framework cannot see the other's.**
    `sendmail_path` catches PHP's own `mail()` — WordPress, when nothing has replaced
    PHPMailer's transport. It is **invisible to Laravel**: `config/mail.php` ships
    `'path' => env('MAIL_SENDMAIL_PATH', '/usr/sbin/sendmail -bs -i')`, so Laravel's
    sendmail transport never consults php.ini. Measured 4 Sep 2026 on a real site —
    `MAIL_MAILER=sendmail` + `Mail::raw(...)` exited 0, reported success, and Mailpit
    received nothing. The Laravel half is therefore `env[MAIL_*]`
    (`mail::laravel_env`), which works because Laravel's `LoadEnvironmentVariables`
    builds an **immutable** Dotenv repository: a variable already in the process
    environment is never overwritten by `.env`, so the catch beats a real
    `MAIL_HOST=smtp.mailgun.org` in the developer's own file. Both halves travel as ONE
    value (`mail::Catch`) through one setter — a pool holding the shim but not the
    environment catches WordPress and delivers Laravel, which is the bug this exists to
    end.
  - **Nine keys, because there are nine ways out**, not nine spellings of one:
    `MAIL_DRIVER` for Laravel ≤ 6, `MAIL_URL` because one DSN overrides host + port +
    credentials together, `USERNAME`/`PASSWORD` as null so AUTH is not attempted
    against a server that offers none, `SCHEME`/`ENCRYPTION` because Mailpit's listener
    is plaintext and a TLS attempt fails closed — and a mail that fails is a mail the
    developer never sees.
  - **Every `env[]` value is QUOTED in the pool ini.** php-fpm parses the file with
    PHP's ini parser, which reads the bare words `null`, `none`, `off`, `no` and
    `false` as the empty string — and an `env[]` that parses empty is
    `ERROR: empty value`, which refuses the whole config and the pool never starts.
    `env[MAIL_URL] = null` did exactly that against a real php-fpm 8.2 (4 Sep 2026)
    before the quoting landed. Verified the same day over FastCGI that a quoted
    `env[]` reaches `getenv()`, `$_SERVER` and `$_ENV` alike — the three places
    Dotenv's adapters look.
  - **The WordPress half needs its own mechanism too, for the mirror-image
    reason.** `sendmail_path` holds only while nothing has replaced PHPMailer's
    transport, and an SMTP plugin (WP Mail SMTP, FluentSMTP, Post SMTP …) hooks
    `phpmailer_init` and calls `isSMTP()`, after which PHPMailer opens its own
    socket. Measured 4 Sep 2026 on a real site through rexenv's own CLI shim:
    plain `wp_mail()` → Mailpit; the same call with the site calling `isSMTP()` →
    nothing in Mailpit, and against a REACHABLE provider it is genuinely
    delivered, from a laptop, to whoever the imported database happens to name.
    `core/wp_mail_catch.rs` is an auto-managed mu-plugin (`rexenv-mail.php`,
    installed like the loopback-DNS one) hooking `phpmailer_init` at
    `PHP_INT_MAX` and calling `isMail()` to put the transport back on PHP's
    `mail()`. **This is the DELIBERATE inverse of `wp_mailtag`'s rule**: the
    stamp is allowed to lose to a site's own filter because its failure
    direction is "the agent misses its own mail"; here the failure direction of
    losing is "a customer receives mail from a laptop", so the catch takes the
    last word. `isMail()` rather than clearing `Host` alone — a hook that runs
    last but leaves `Mailer = 'smtp'` makes PHPMailer FAIL the send, and a mail
    that fails is a mail the developer never sees. **Not caught, stated because
    the promise reads absolute:** an HTTP-API transport (Mailgun's API, SES via
    the SDK, Postmark's REST endpoint) posts with `wp_remote_post`, fires no
    `phpmailer_init`, and is invisible here. **Install and REMOVAL are one
    function** (`apply_for_site`): a file left behind when the switch went off
    would keep hijacking mail the user had asked to be delivered, with the
    setting looking broken and nothing to point at.
  - **Four surfaces, one switch.** The pool covers a page request. `php artisan` sees
    none of it — a queue worker, a scheduled command and a `tinker` one-liner are
    fresh processes with the app's own `.env` — so `laravel::mail_env` puts the same
    variables on the MCP artisan runner, the provisioning steps and rexenv's in-app
    terminal. Same split that bit wp-cli on 25 Aug 2026 (`wp_mail()` caught through the
    browser, dropped from the command line, `true` returned both times); same fix.
    The user's own iTerm is beyond reach and always will be.
  - **A FrankenPHP override site carries the catch itself** (#514, 5 Sep 2026). Both
    halves were properties of the php-fpm POOL, and a FrankenPHP site has none — for a
    day the audit found its embedded PHP on the default `sendmail_path` and its process
    on the site's own env alone, so a Laravel site on FrankenPHP with a real `MAIL_HOST`
    delivered for real. Now `ServiceManager::override_env` appends `laravel_env` to the
    backend's spawn env (LAST, so it beats a site variable of the same name — the
    switch's meaning) and `override_sendmail` hands the SAME shim string to
    `frankenphp::generate_config`, which renders it as `php_ini sendmail_path` in the
    global `frankenphp {}` block. **The shim is wrapped in an inner pair of double
    quotes, and that is measured, not tidy:** the Caddyfile lexer consumes the outer
    quotes, so what reaches PHP's ini parser is the pool's `'/App Support/mailpit'
    sendmail …` — whose bare single quotes the ini parser strips, exactly as in a pool
    ini, leaving a path `sh` splits at the space. With `\"…\"` inside, `ini_get` came
    back verbatim and a real `mail()` ran a fake sendmail at a path with a space with
    the right argv (`frankenphp_mail_catch_check`, live on the pinned 1.12.4). The
    toggle is the third carrier's respawn too: `set_mail_catch_all` reconciles the
    override backends after the pools, and a flipped catch is a changed config, which
    is what `reconcile_overrides` respawns on. Apache never needed any of this — its
    `.php` goes to the shared pool.
  - **`.env` is written too, and that is not redundancy** — it is the
    `php artisan config:cache` case. A cached config is baked from `env()` at cache
    time and `env()` is never read again, so a site that caches keeps whatever its file
    said. Provisioning therefore wires MAIL_* into `.env` as well, after keeping the
    original as `.env.rexenv-backup` (written once — a retry that overwrote the backup
    with the already-wired file would destroy the thing it exists to preserve).
  - **The switch does not merely record the flip** (`commands::mail::set_mail_catch_all`):
    it writes the setting, installs or REMOVES the mu-plugin per site, and
    restarts the RUNNING php-fpm pools so the rewritten configs are what the
    workers actually run. A toggle that only wrote the row would be honest about
    nothing until the next stack restart — a developer switching catching OFF to
    test a real provider would watch mail keep vanishing into Mailpit with the
    screen saying it should not. The restart is `restart_pools_for` over the live
    set, so a settings edit never starts a pool as a side effect.
  - **The switch** (`mail.catch_all`, `mail::catch_all_enabled`) reads **absent as ON**:
    a default that read a missing row as off would make "every site's mail is caught"
    mean "every site created after the user found the switch", and only the exact
    string `false` opts out. Off is a real task — deliberately proving a live SES or
    Postmark integration from a local box — and off means the site's own mail
    configuration is left ALONE, not re-pointed at a sink we guessed it wanted.
  - **Unread is a SERVER-side search, and read state never waits for the poll.** The
    inbox list refetches every 5s, which is the whole design constraint on this screen:
    anything that becomes true only on the next refetch happens somewhere between
    instantly and five seconds later. That is what the read flip did — fetching a
    message's detail is what marks it read in Mailpit (its documented side effect), and
    nothing told the list, so the row's unread dot cleared whenever the poll next came
    round. It reads as "clicking the subject works, clicking the sender doesn't"; it was
    the poll phase, not the click target. The preview now patches the cached list at the
    moment the fact becomes true and lets the poll reconcile. Same rule for **Mark all
    read** (`PUT /api/v1/messages`, no IDs — Mailpit's "all mailbox messages"), which
    deliberately does NOT invalidate afterwards: the write succeeded, so the patch is the
    truth, and an immediate refetch only opens a window for an in-flight list to answer
    with the state from before it. The **Unread filter** is `is:unread` composed into the
    Mailpit query (`mail::search_query`), never a filter over the fetched page — the page
    is what hides the unread mail you are looking for. Its counts stay mailbox-wide
    (Mailpit's own contract), and the message you are READING stays pinned in the list
    while its preview is open, or the filter would delete the row out from under you
    (CLAIM-LEDGER #313–#315).
- **"Log in as"** (`core/wp_login.rs`): one-time, single-use, local-only magic link via
  a mu-plugin. Three checks, and their relationship is NOT three independent layers —
  the module doc states what each one actually rests on, because a confident wrong
  account of it shipped twice. In short: the Cloudflare header set is what denies a
  tunnel replay; the Host check is a second expression of that same fact (it denies only
  because the tunnel mu-plugin restored the public host, gated on the same headers); the
  client-IP check reads the **last** `X-Forwarded-For` hop — never the first, which is
  whatever the caller sent, since Cloudflare appends rather than replaces (CLAIM-LEDGER
  #307, fixed 14 Aug 2026). That was reachable **through a tunnel and only through a
  tunnel**: the tunnel is the one path that skips the edge (cloudflared → nginx direct),
  and the edge REPLACES a caller-supplied `X-Forwarded-For` with its own peer, so on
  every other path the caller's entry never reaches PHP (measured, `wp_login_check` leg
  E — a Caddy default, so re-measured rather than assumed). The include-time/`init` ordering that makes the Host check
  work is WordPress's boot order, not filename sort (#308).
- **A site can reach ITSELF** (`core/wp_dns.rs`, 10 Aug 2026): the **static-php.dev**
  builds link libcurl against **c-ares**, which resolves from `/etc/resolv.conf` ALONE
  and never reads macOS split-DNS (`/etc/resolver/<tld>`) — where rexenv publishes every
  TLD it serves. So inside php-fpm `gethostbyname("x.rex")` answered `127.0.0.1` while
  `curl` to the same host died with errno 6, and **WP-Cron stopped on every hosted
  WordPress site with nothing logged** (it spawns itself with a fire-and-forget HTTP
  request and never checks the result). **Not every pinned build has it, measured 23 Aug
  2026 and not before: 7.4 uses the THREADED resolver** — it is the one build rexenv
  makes itself, against its own curl 8.21.0, and never got static-php.dev's
  `--enable-cares`. The seven had never been compared (`wp_dns_check` measures whichever
  PHP its fixture runs), so `docs/TODO.md` costed the real fix at "7 minors × cli/fpm ×
  2 arches" when one of the seven never had the bug. `core::wp_dns::resolver_for` records
  the measurement per minor; the L0 half refuses to let a pinned minor go unrecorded, and
  `wp_dns_check` FAILS on any disagreement between the record and the build in front of
  it — in either direction, because a minor that turned threaded means the mu-plugin is
  dead weight for it.
  **Since 7 Oct 2026 the real fix ships for 8.1–8.5** (`php-8x-8`): rexenv's own 8.x
  builds had linked c-ares too — not by asking for it, but because spc's curl builder
  turns it on whenever `libcares` is in the build, and **swoole** depends on it. The
  rebuild pre-builds libcurl without it (the system's `getaddrinfo`, so `/etc/resolver`
  is seen) and `rexenv/runtimes`' gate 9 fails any build whose curl links c-ares again.
  Measured on the published artifacts: raw `curl_init("https://amin.rex/")` was "Could not
  resolve host" on -5 and HTTP 200 on -8. **8.0.30 — static-php.dev's build — still has
  it**, so `resolver_for` and the mu-plugin stay; on every other pinned build the
  plugin's first guard returns at once.
  **What the mu-plugin does NOT cover, measured 31 Aug 2026 and now SAID rather than
  implied** (ledger #435): it patches the WordPress HTTP API, so a plugin calling
  `curl_init()` directly — and any non-WordPress PHP app rexenv hosts — still gets
  *"Could not resolve host"* on a c-ares build (only 8.0 since `php-8x-8`), while `gethostbyname` and PHP streams work
  on the same request. `rex doctor` prints that as a NOTE (which builds, how many sites,
  what is covered, and the two workarounds: the WP HTTP API, or `CURLOPT_RESOLVE`) —
  never as a ✗ and never counted in the exit code, because every normal install has it and
  a doctor that goes red for everyone is a doctor people stop reading. The same run
  measured that c-ares DOES read `/etc/hosts`, which prices the one non-build option and
  shows why it cannot serve a WILDCARD TLD: `/etc/hosts` has no wildcards, so subdomain
  multisite and any invented host would still fail. Site Health loopbacks, REST self-calls and
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
  per-site cloudflared quick tunnel, scoped to ONE site Host, outbound-only. **The
  origin is the backend that serves THAT site** (`tunnels::origin_port`, 15 Aug 2026):
  the shared nginx HTTP port for vhosted sites, the site's own RECORDED override port
  for Apache/FrankenPHP sites — read through the same accessor the config generator
  uses, so origin and reality cannot drift, and an override site's Host can never fall
  through to nginx's default server (#13's measured cross-site exposure; this replaced
  the old "can't be shared yet" refusal). A stopped override backend refuses at start
  (ownership+liveness from the ServiceManager, never a bare port-listen). Behind
  the edge `REMOTE_ADDR` is always `127.0.0.1`, so loopback-only enforcement keys off
  `CF-*` headers + last-hop `X-Forwarded-For` + `Host`, never the IP (the last hop,
  never the first — the first is whatever the caller sent; #307). On tunnel start
  an auto-managed mu-plugin bakes in the public origin (HOST/HTTPS overrides +
  siteurl/home filters + output-buffer rewrite for plain/JSON-escaped/%-encoded);
  removed on stop; local requests untouched. The siteurl/home filters swap the
  ORIGIN and keep the value's PATH: in a subdirectory multisite a sub-site's
  siteurl is `https://<local>/sub1`, and returning the bare origin dropped that
  path — the sub-site's front page 404'd and logging in at `/sub1/wp-login.php`
  landed on the MAIN site's dashboard, because `admin_url()` had lost the `/sub1`
  (found and fixed 26 Aug 2026, live through a quick tunnel). A **subdomain** network needs two more
  things, and neither can live in the mu-plugin. First, WP pins `COOKIE_DOMAIN` to
  `.<network domain>` in `ms_cookie_constants()`, inside wp-settings.php and therefore
  BEFORE mu-plugins load, so through a tunnel every auth cookie is cross-domain, the
  browser drops it, and wp-login answers "Cookies are blocked or not supported by your
  browser" — the share served pages and refused every login (measured 27 Aug 2026).
  Second, a tunnel issues ONE hostname and pins ONE Host, so the network's sub-sites are
  simply unreachable as subdomains — an outside browser gets `ERR_NAME_NOT_RESOLVED`.
  So while shared, a subdomain network is served as a **subdirectory** network, in three
  auto-managed pieces that are one feature and move together:
  - **`sunrise.php`** (tunnel lifetime, next to the mu-plugin) — the REQUEST half.
    `ms-settings.php` includes it BEFORE it resolves the blog, and `$wpdb` is already
    live, so `/s1/…` is looked up in `wp_blogs` and, if a sub-site owns that label, the
    request's `HTTP_HOST` becomes `s1.<network>`. Only the Host: `REQUEST_URI` keeps its
    prefix, because `WP::parse_request()` strips `home_url()`'s path itself and everything
    WP builds from `REQUEST_URI` needs the prefix still there — stripping it sent an
    unauthenticated `/s1/wp-admin/` to the login with `redirect_to` pointing at the MAIN
    site.
  - **the URL rewriter** (`rexenv-tunnel.php`) — the URL half: `<network>` → `<origin>`
    and `<label>.<network>` → `<origin>/<label>`, applied to siteurl/home, the `*_url`
    filters and the output buffer from ONE mapping.
  - **the wp-config block** — the cookie scope above, plus the `SUNRISE` declaration
    (`ms-settings.php` only looks for the drop-in when that constant exists, and by then
    wp-config has already run). Both guarded on a file that exists only while a share is
    live, so the block is inert otherwise.
  BOTH network modes' nginx vhosts carry WordPress's network rewrite rules. A subdomain
  network does not need them locally — every sub-site's `/wp-admin/` exists on disk, so
  the `!-e` guard never opens — but through a tunnel `/s1/wp-admin/` has no file behind
  it. `REQUEST_URI` stays `$request_uri`, the ORIGINAL, which is exactly what sunrise
  reads. Locally nothing changes: the network stays a real subdomain network, which is the
  point of developing on one. Real subdomain sharing needs a wildcard hostname a quick
  tunnel cannot issue — that is a named-tunnel-plus-own-domain feature, not this one.
  - **Tunnels DIE WITH THE APP** (ruled 28 Jul 2026 — the deliberate opposite of
    services-outlive-the-app: a public share must not outlive the thing supervising
    it). The v23 `tunnels` row is claimed atomically BEFORE spawn (the row IS the
    double-start guard; sentinel pid until the child exists), `RunEvent::Exit` kills
    from rows, and the launch sweep kills only on positive argv identity — plus a
    rowless backstop for cloudflared processes carrying our argv identity with no
    row (app-data reset class), which stops them with a loud WARN.
  - **And a share rexenv stops FOR you now reaches the screen** (30 Aug 2026, ledger #431).
    The launch sweeps run inside `setup()`, before any window exists, so an emitted event
    went to nobody: "rexenv stopped a public share you had no record of" lived only in the
    log. Notices are queued in the always-managed `StartupNotices` and DRAINED by the
    frontend on mount — once, so a reload cannot re-toast a share stopped hours ago. There
    is no warning toast kind and `error` would misname a correct action, so the notice
    takes the ACTION form ("Open Tunnels"), which also buys 10s of screen time instead of 4.
  - **A share dies with the app even when the app is KILLED** (30 Aug 2026, ledger #432).
    "Tunnels die with the app" had two legs and a hole between them: `RunEvent::Exit`
    covers a clean quit, the launch sweep covers a crash — *at the next launch*, which may
    be days away, and until then the site is public with nothing supervising it. macOS has
    no `PR_SET_PDEATHSIG`, so the third leg is a detached watcher per share: the app
    binary re-executed as `--tunnel-guard <parent> <child> <domain>`, blocking on kqueue
    `EVFILT_PROC`/`NOTE_EXIT` for BOTH pids. Parent dies → it re-reads the child's argv and
    signals only on the same positive identity the sweeps use (a recycled pid must never be
    killed); child dies first → the guard exits, so the ordinary stop leaves nothing behind.
    Best-effort at the call site: a share that could not get a guard still has the other two
    legs, and refusing to share over a missing watcher would trade worse than the window it
    closes.
  - **Every share leaves a trail in `rexenv.log`** (30 Aug 2026, ledger #430): a loud
    INFO naming the site, the public URL and the pid when one starts, and a closing line
    on every way it ends — user stop, the in-flight kill, quit, crash. Before this the
    app-wide log recorded only failures and sweeps, so a LIVE share was invisible there:
    a share for `mstest.rex` found running that nobody remembered starting (27 Aug 2026)
    had its whole record in `logs/tunnel-mstest.rex.log`, a per-domain file you can only
    think to open once you already know which domain to suspect — which is the question.
    The start line goes AFTER the registry insert, so the log never claims an exposure a
    failed start never created.
  - **Share health is its own tri-state** (Live / Unverified / Broken) probed via
    bounded HEADs of the public URL: any non-530 answer proves the path, 530×3 =
    Broken (sticky — only an HTTP answer clears it), transport errors are
    non-evidence. Phase A after start probes via 1.1.1.1 + pinned-address edge
    checks ONLY — never the system resolver, whose negative cache would poison the
    LAN for 30 minutes (trycloudflare SOA MINIMUM = 1800s, measured).
  - **The Tunnels list is searchable by name, domain OR public URL** (`routes/Tunnels.tsx`
  · #371). The URL is in the match set because this page is the only one that can answer
  "which of my sites is `https://odd-cat-42.trycloudflare.com`?", and pasting the link is
  how that question gets asked. **What the filter must not do is hide an exposure
  quietly:** hidden SHARED sites are counted and named in amber ("2 shared sites are
  hidden by this filter — still public until you stop sharing"), on the list and on the
  no-match screen alike, and the header keeps the machine-wide truth — the subtitle's
  shared count and Stop all sharing both ignore the filter, because they describe the
  machine rather than the view.
- **A share guards its site for the share's LIFETIME:** web-server switch,
    multisite convert, docroot move, db-import/rewrite and provision-retry all
    refuse while shared, naming the exposure (and tunnel start refuses while those
    run). rexenv never auto-stops a share on the user's behalf; quitting with live
    shares gets a native confirm naming the honest count.
- **Adminer** (`core/adminer.rs`): internal vhost `adminer.rexenv.rex` on the shared
  stack — never a tunnel origin. Per-site deep link via a generated `index.php` wrapper
  (`adminer_object()` hook): passwordless login for loopback servers only, auto-submits
  Adminer's own CSRF-tokened + CSP-nonced form on `?rexenv_auto`.
- **The console's palette follows the APP, not the OS** (`adminer::set_theme` +
  the wrapper's `css()` · `adminer_set_theme` · 20 Aug 2026). Adminer decides its
  scheme from what `css()` returns — values naming only `dark` make it load
  `dark.css` WITHOUT the `prefers-color-scheme` media query and emit
  `<meta name="color-scheme" content="dark">`; only `light` drops `dark.css`
  entirely; returning nothing leaves both, media-gated. That default is what left a
  light-themed rexenv framing a dark console. rexenv now writes the RESOLVED palette
  (`dark`/`light` — "system" is resolved app-side, so the two can never disagree about
  what the OS meant) into `.rexenv-theme` in the console's docroot, and the wrapper
  reads it per request. **A file, not a query parameter:** Adminer's own links carry no
  parameter of ours, so one click inside the console would have dropped it and snapped
  the page back. A dotfile, so the existing `NGINX_DOTFILE_DENY` keeps it unservable;
  the empty `rexenv-theme.css` beside it is deliberately NOT one, because Adminer emits
  a `<link>` for whatever key `css()` returns and a 404 in the console's own head is a
  defect report waiting to happen. The frame writes the file BEFORE it loads and is
  `key`ed on the palette, since the console is a separate document that read the file at
  request time and re-reads nothing.
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
- **The plugin list's icon column shows art for plugins wp.org has never heard
  of** (`core/wporg.rs::plugin_icons` · `wp_org_plugin_icons`): one cached
  `plugin_information` GET per installed slug, and a letter tile when there is no
  answer. Premium plugins have no answer — `betterdocs-pro`, `elementor-pro`,
  `wp-security-audit-log-premium` are not in the directory — yet wp-admin's update
  screen shows the vendor's logo for them, because it reads
  `$plugin_data->update->icons` out of the `update_plugins` transient the vendor's
  own updater fills in. **rexenv cannot read that transient**, and the measurement
  is why: on a real 47-plugin site the STORED transient carried no premium row at
  all, and a `wp eval` with every plugin loaded produced ONE of the nine installed
  premium plugins — the other eight inject their update data on an `is_admin()`
  request, which no wp-cli run is. So the icon is DERIVED: a slug carrying a
  premium marker (`-pro`/`-premium`, separator optional — `fluentformpro` is a real
  directory name) falls back to its free counterpart's wp.org icon, which is the
  same artwork the vendor points wp-admin at. Kept narrow on purpose: only those
  two markers, only for a slug wp.org itself had nothing for, and a counterpart the
  directory does not know leaves the letter tile alone rather than hanging a
  stranger's logo on someone's private plugin (`essential-addons-elementor`, whose
  free half is `…-for-elementor-lite`, is the honest miss). A counterpart installed
  alongside its add-on — the usual case — was already fetched, so the common shape
  costs no extra request.
- **A PREMIUM plugin's update is invisible to wp-cli until rexenv grants the
  capabilities its updater asks for** (`core/wordpress.rs`: `update_context_arg`,
  `checked_list` · #370). wp-admin listed BetterDocs Pro 3.9.0 → 4.1.0 while rexenv's
  list showed nothing, and the cause is not parsing: the vendors' updaters never
  REGISTER. Every premium updater measured on a real site adds its
  `pre_set_site_transient_update_plugins` filter behind
  `current_user_can( 'manage_options' )` (or `is_admin()`), and a wp-cli run has **no
  user at all**, so WordPress builds its update data with every paid plugin missing.
  The fix is a second `--require` file beside the phar — the EOO file's mechanism, a
  different job: it defines `WP_ADMIN` and grants three named capabilities
  (`manage_options`, `update_plugins`, `update_themes`) to that ONE process. **Nobody is
  impersonated** — no user is logged in, no session or cookie exists, and a later
  wp-cli run has no capabilities of its own. Measured on a 47-plugin site: 2 of 10 paid
  plugins reported an update before, 5 after, and the two grants each earned their
  place (`WP_ADMIN` alone moved nothing; the capabilities moved four; `WP_ADMIN` on top
  moved the fifth). **The UPDATE path carries it too, and that is not symmetry for its
  own sake:** the premium package URL comes out of the same filter, so
  `wp plugin update <paid-slug>` without the context answers "No plugin updates
  available" — a badge whose button cannot work. **Where it does NOT ride** is the
  claim worth guarding: not the fast list, not install/activate/deactivate/delete, not
  the terminal, and above all not the MCP raw runner an agent drives — read out of the
  source by `the_premium_update_context_rides_only_the_update_paths`, because a list of
  call sites in a comment is exactly what drifted in #228. And the checked pass **falls
  back**: if the context run fails for any reason other than the clock, the plain list
  runs, so vendor code dying on an unseen site costs the premium rows and never all
  the badges.
- **A row is labelled with the thing's own NAME, and keeps its slug** (`WpPlugin::title`,
  `WpTheme::title` — the theme half added 19 Aug 2026). wp-admin says "Twenty
  Twenty-Five"; rexenv said `twentytwentyfive`, because `title` is not in wp-cli's
  DEFAULT field set for themes and the argv never asked for it. Both lists now show the
  header name with the slug beside it rather than instead of it: the slug is the folder
  name, the argument every `theme`/`plugin` command takes, and what a person greps for.
  An empty title (a theme whose header has none) falls back to the slug — never a blank
  label. This is also what makes the plugin FILTER honest, since it matches the label
  the row displays as well as the slug (#372).
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
- **Re-uploading a zip of something already installed offers the REPLACE wp-admin
  offers** (`wp_install_job`'s `force` → `wp <kind> install --force` ·
  `blockedByExisting` · 19 Aug 2026). wp-cli refuses to unpack over an existing folder
  and reports it as an ordinary failure whose summary says only `Error: No plugins
  installed.` — true, unactionable, and exactly what a user sees after picking the zip
  they meant. The card now reads the reason out of the LOG (`Warning: Destination folder
  already exists. "…/plugins/<dir>/"`, wp-cli's own line — the folder name is taken from
  there rather than from the zip's filename, which is routinely `plugin.1.2.3.zip` for a
  folder called `plugin`), names that folder, and offers "Replace with the uploaded
  zip", which re-runs the SAME job with `--force`. **The fact rides the job STATE
  (`blockedBy`), parsed once on the stream** — not re-read from the log by each
  consumer: the log is a rotating 300-line tail and the TOAST never receives it at all.
  That toast is why it matters. It said *"Install of thinkrank.1.31.0.zip failed: Error:
  No plugins installed."* — wp-cli's sentence, and a lie about what happened, next to a
  card offering a one-click way forward. It now says the plugin is already installed and
  points at Replace, and the card's glyph and colour follow the same fact: an amber `!`
  rather than a red `✕`, because nothing broke and nothing was touched. wp-cli's own
  summary line stays verbatim under it — never paraphrased, only re-toned. **`force` is never defaulted on**, and
  the retry keeps the job's own source — a zip re-runs as a zip, a wp.org job as wp.org,
  because sending a slug down the zip gate would fail `ensure_zip_paths` and read as a
  second unrelated error. **One guard wp-admin has no need for:** rexenv knows which of
  those directories are git checkouts (it put some of them there), and `--force` unpacks
  over the working tree — uncommitted work, the branch, `.git` itself. A tracked or
  git-looking target gets a danger confirm naming exactly that; everything else replaces
  on the click, which is the confirmation wp-admin's own button is.
- **The install card outlives the install only when it has something left to say**
  (`INSTALL_CARD_LINGER_MS`, `useWpInstall`'s `dismiss`/`hold` · 19 Aug 2026): a
  SUCCESSFUL job clears its card 3s after settling — the toast already announced it and
  the list underneath now shows the plugin, so the card is a panel the user would have
  to tidy after every install. Every other outcome (failed, partial, cancelled, timed
  out) STAYS, because there the card holds the only copy of the reason, and gains an ×
  to put it away. Two rules keep the auto-hide from becoming the defect it replaced:
  **opening the log HOLDS the timer** (3s is exactly long enough to click "Show log" and
  watch it vanish), and a RUNNING job never offers dismiss — hiding work still happening
  is what this card exists to prevent. Both lists get this — plugins and themes run the
  same hook and the same card, and `wpinstallcard.js` drives both rather than trusting
  that they do.
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
- **A Blank-PHP site starts as a PAGE, not a `phpinfo()` dump** (`core/starter.rs` +
  `src-tauri/templates/starter/`): the generated `index.php` says what is running (PHP
  version, web server, scheme), where the files are, and — when the site asked for a
  starter database — renders four seeded rows out of `starter_items` through the
  `db.php` beside it. phpinfo answered a question nobody asked on their first page load
  and none of the ones they did.
  - **Two files, generated once, never overwritten.** `write_files` skips a file that
    already exists, per file. Retry re-enters the same code with the developer's edited
    page on disk, and "provisioning finished" must never mean "provisioning reverted my
    work". A user who deleted `db.php` and kept their page gets neither back.
  - **The page is written at `prepare`; `db.php` at `configure`.** The database name is
    allocated with the row (`unique_db_name` needs the connection) and the database
    itself does not exist until the job's `configure` phase — so a connection file
    written any earlier would name a database that is not there for the length of a
    provision. One template, no substitution: `index.php` decides what to render by
    looking for `db.php` beside it, so the with-database and without-database pages
    cannot drift apart.
  - **Seeding is idempotent** — `CREATE TABLE IF NOT EXISTS` plus a seed guarded by
    `WHERE NOT EXISTS`. DROP-and-recreate would delete whatever the developer had put in
    the table by the time they pressed Retry.
  - **The dialog's Database field became a CHOICE for Blank PHP** (`NewSiteDialog`,
    `NewSite.starter_db`): MySQL / MariaDB / **None**, defaulting to MySQL, and offered
    only for a folder rexenv creates. It was a dead "None" — a field the user could see
    and not use. None is the option that skips the ~600 MB engine download, which is why
    this is a choice rather than always-on: a scratch PHP file should not cost a database
    engine. The note under the field says which of the two the click will do.
  - **Two places stopped saying "Blank PHP sites have no database"** — the site row's
    database button and the Database tab's placeholder. Half of them now do, and both
    read the recorded `starterDb`, so a seeded site opens Adminer on its own database
    like every other type.
- **A SITE from Git** (v33 · `core/sites.rs::{validate_git_source, clone_into_docroot}` +
  the `clone`/`deps`/`finalize` phases in `commands/site_provision.rs` ·
  `docs/archive/PLAN-git-site-clone.md`): a repository is the **third source for a docroot**,
  after "rexenv makes it empty" and "the user points at theirs" — not a new site type.
  `NewSite.git_url`/`git_ref`; Laravel and Blank PHP only.
  - **One validator, asked twice.** `validate_git_source` runs in `provision_with`
    early enough that a refusal leaves no docroot or certificate behind, and again in
    `create_recording_ownership` for its value. It refuses `git_url` beside `path`
    (linking promises rexenv never writes into that folder; cloning fills one it just
    made — ranking them picks which promise to break), refuses `Ownership::Agent` (a
    clone downloads code a MODEL chose and `composer install` then runs that project's
    own scripts, with no click between), and refuses WordPress (a checkout without its
    database is not a site — that is the DB-import story).
  - **The clone cannot eat a docroot.** `clone_repo` refuses an existing `dest`, so the
    clone lands in a staging SIBLING (`.rexenv-clone-<domain>-<token>` — same
    filesystem, because the Sites folder is user-configurable and may be on another
    volume) and moves in via `remove_dir` + `rename`. `remove_dir`, never
    `remove_dir_all`: the guarantee is the kernel's refusal to remove a non-empty
    directory, not a check of ours. The Blank-PHP probe page is skipped at create and at
    Retry for a cloning site, so prepare cannot block its own clone phase.
  - **Type chosen, then verified.** The type fixes the phase list and the binary plan
    (including whether ~600 MB of database engine is fetched) and `ls-remote` cannot see
    files, so `detect_project` verifies afterwards and NAMES a mismatch instead of
    re-typing the site under a card that already described the job. The detected
    `docroot_rel` is recorded as `docroot_subdir` — a repo's real entry point, not the
    type's usual one.
  - **`.env` before dependencies.** Composer's `post-autoload-dump` runs `artisan
    package:discover`, which BOOTS the app; installing first boots it against Laravel's
    defaults, SQLite included. `.env` is copied from the repo's `.env.example`
    (`create-project`'s post-root-package-install script never fires on a plain
    install), an existing one is kept and only `APP_URL`/`DB_*` rewritten, and a repo
    with neither gets a minimal local seed rather than the framework's production
    posture. `finalize` then runs `key:generate --force` + `migrate --force`.
  - **Front-end assets are a phase of the CLONE, not of Laravel** (v35
    `git_build_assets`, default ON in the dialog): any repository can carry a
    `package.json`, and a Vite app throws *"Unable to locate file in Vite manifest"*
    on page one until it is built, so the default that produces a WORKING site is the
    one that builds. The package manager is the repo's own answer (`packageManager`
    field beats lockfile — `repo::inspect_repo`), run from the developer's login-shell
    Node. **It is the one NON-FATAL phase**, and that is the point: the build runs
    somebody else's scripts with somebody else's toolchain, so its failure is not
    evidence that provisioning failed. The job settles `ok` with an `assets_warning`
    the card shows as a warning banner — failing it instead would park a created,
    wired, serving site behind a "setup incomplete" badge whose Retry re-runs the
    clone, the database and Composer to reach the one step that was never rexenv's to
    guarantee. (Same "succeeded, but" shape as `serving_blocked`.)
  - **A stopped stack is STARTED by the serve phase, not deferred** (18 Sep 2026). Until
    then a site created with the stack down settled `ok` as "serves on next stack start"
    (a decision recorded in `docs/archive/PLAN-site-provisioning-progress.md` §D, now
    reversed), and the clean-VM smoke test showed what that meant for the FIRST site
    anyone creates: WordPress installed, card green, Safari "can't connect", and a
    Restart button to go and find — the headline promise ("rexenv will serve it
    instantly") false on the headline flow. The serve phase now runs the Start-all
    sequence itself (`commands::services::start_stack`, the body the button calls):
    prefetch, `start_core` with the new site already in its list, `await_ready`, the
    privileged edge prompt off the runtime, the wire verified. A foreign `:443` or a port
    conflict therefore fails THIS job with Start all's own words, the row stays
    `provisioned = 0` ("setup incomplete", Retry re-runs the serve step), and nothing
    ever settles green over a site nobody can open.
  - **The row reads "Setting up" while its job runs and "setup incomplete" only after the job
    died** (30 Sep 2026, #763). The row is persisted at job START with `provisioned = 0` and
    only the ok-settle writes 1, so the flag alone cannot tell a job in flight from a dead one —
    and `Sites.tsx` read the flag alone: for the whole of a first WordPress install the row
    behind the New Site dialog said the setup had failed while the card in front was still
    installing. The row now asks the backend's job registry (`siteProvisionActive(domain)`, a
    2 s poll per not-yet-provisioned row) — not the route's own `useSiteProvision`, which knows
    only the jobs this window started, while `rex site create` and an agent insert the same row
    — and renders the amber `Setting up` pill with no Retry for a running job, neither state
    until the first answer, and the badge + Retry only once the registry has no running job
    for it. (The dialog still hands its started job to the route through `onStarted`, for the
    route's own card behind the dialog and after "Close (keeps running)".)
  - **WordPress from a repository** (Stage 4) runs the SAME four phases a created
    WordPress site does, because each was already skip-aware — `core_download` when
    core is present, `configure` when `wp-config.php` is, `core_install` when
    WordPress is. What the user gets is their CODE and a **fresh, empty database**,
    said in the dialog before Create rather than discovered afterwards: "clone my
    site" and "clone my site's code" are different promises, and only the second is
    on offer (a dump import from the Database tab is the other half).
    Roots' **Bedrock/Radicle** are the one real fork: Composer owns core and `.env`
    owns the configuration, so `deps` runs FIRST and one answer
    (`sites::wordpress_core_from_composer`) turns off both `core_download` (a
    download would put a second WordPress beside `web/wp`, and the site would keep
    working from the wrong copy) and `wp config create` (it would overwrite the
    repository's own stub). `wordpress::wire_bedrock_env` writes the database, URLs
    and any UNSET salts — never rotating one that exists, which would log every
    session out silently. ⚠ **UNVERIFIED against a real Bedrock project** (ledger
    #294): the key set is from Roots' documented example.
    The served root and the CONTENT dir are both re-read from the checkout after the
    clone — creation could only guess them from the site type, off a folder that was
    still empty, so a Bedrock site would otherwise have had every mu-plugin written
    into a `web/wp-content` it does not load.
  - **Any PHP repository works as a Blank-PHP site.** `detect_project` already
    classifies Symfony, Craft, Statamic, Magento and generic front controllers, and
    the clone phase records its `docroot_rel` as `docroot_subdir` — so the document
    root is DETECTED, not assumed, and one site type covers every framework rexenv
    does not special-case. A cloned Php site therefore also gets the `deps` phase:
    `vendor/` is gitignored in all of them, so the checkout on its own is a 500. It
    does NOT get a database engine — `needs_database` says a Php site has none, and
    ~600 MB of MySQL for a phase that never runs is the waste the linked-site
    carve-out already avoids.
  - **The site's own checkout gets the SAME panel the assets use** (Stage 3):
    `commands/repo.rs::job_target` resolves `kind = "site"` to `site.path` — the
    project root, one level above what a Laravel site serves and exactly where the
    clone put `.git` — so `repo_git_op`, `repo_branches`, `repo_pull_refs`,
    `repo_check`, `repo_scripts`, `repo_script_job`, `repo_watch_start` and
    `repo_asset_status` all work unchanged. A site target carries **no `dir_name`**:
    the one sent alongside is the domain, used for display and the log key, never
    turned into a path — making it the one kind with no user-supplied path segment at
    all. Writing a second panel would have meant a second answer to "is this checkout
    dirty", the question every destructive confirmation is built on.
    **Never an upward walk.** `repo_site_info` is a single `<path>/.git` existence
    test. A linked Laravel site stores `…/app/public`, so a parent search would find
    the project's repo one level up — and one level further, a `~/code` repo holding
    forty projects, where the panel's Checkout button is a catastrophe nobody asked
    for. The Repository tab appears when that exact folder is a checkout (a cloned
    site always; a linked one that happens to be a repository too) and says which
    folder it looked at when it is not.
    `wp dist-archive` stays asset-only — a project root is not a distributable — and
    a site checkout never writes `sites.git_ref`: that column records the branch
    PICKED at create, and git owns what is checked out now.
  - **`git_url` is recorded at INSERT, not when the checkout lands** — Retry is the
    recovery path and has nowhere else to learn what to clone after an app restart. The
    row states the SOURCE; `provisioned` states whether the code arrived. Same argument
    puts the migrate choice on the row (v34 `git_migrate`, read through
    `Site::runs_migrations` — NULL = ON, exact for every pre-v34 Laravel site): without
    it, unchecking migrations would hold for one run and then reverse itself on Retry.
    The whole phase plan is derived from the row in one place (`PhasePlan::of`), and the
    `finalize` label changes with the choice — a phase that announces "app key +
    migrations" and then only generates a key is the small lie that makes the rest of
    the card unreadable.
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
    every JS dev). How a tool is found on that PATH is the platform's
    (`platform/path_lookup.rs`, ledger #628): `:` and the exact name `PATH`
    on macOS; on Windows `;`, `Path` in any case, and each `PATHEXT`
    extension (`git` is `git.exe`, `npm` is `npm.cmd`). The core split on `:`
    once, so the Dell — Git for Windows and Node installed — was told both
    were missing (15 Sep 2026). Missing tool = honest `$`-fix error; `git_preflight`
    probes the CLT quietly so `/usr/bin/git`'s shim can never pop a GUI
    dialog from a background task. EXCEPTION: composer is ALWAYS our pinned
    phar run by the SITE's PHP version — platform checks (`php`, `ext-*`)
    then match reality, and system "composer" can be a non-phar wrapper.
  - **Repo scripts never run implicitly.** Clone+detect executes no repo
    code; composer/install/build are one explicit click each, with the
    disclosure line above the buttons. `GIT_TERMINAL_PROMPT=0` forced — a
    hidden credential prompt fails fast with a mapped error, never hangs.
  - **Two doors out of a dirty tree — and rexenv creates the dirt.** Every
    other op refuses on local changes ("commit or stash them first"), and the
    changes are usually the residue of a `composer install`/`npm run build`
    started from this very panel: rexenv made the wedge and then pointed at a
    terminal. So the ops row carries **Stash** (recoverable), **Restore** from
    the entry list, **Reset** (not recoverable) and **Status**. Rules that are
    the whole point of them: stash is `push -u` — untracked included, so the
    tree is really clean — and NEVER `-a`, which would sweep `vendor/` and
    `node_modules/` (minutes of installs) into an entry that then fights the
    next install; reset is `--hard HEAD` with no `git clean`, so files the user
    wrote and never added survive an action whose name sounds total, and the
    confirm says both halves with the real counts; Restore is `pop` (an applied
    entry left behind is a second copy of the same work) and a conflict KEEPS
    the entry, which the error says. A stash entry is a REVISION, not a ref
    name, so it is whitelisted to exactly `stash@{N}` (`validate_stash_ref`) —
    revision syntax is an expression language. The list is read live per open:
    git renumbers it on every pop, so a cached `stash@{1}` names a different
    entry than it shows (CLAIM-LEDGER #310–#312).
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
    **On Windows the link is a directory JUNCTION** (`WindowsShell::symlink_dir` over `platform/windows/
    junction.rs` + `junction_rules.rs`, W7 S6, ledger #625): a directory symbolic link needs Developer Mode
    or a privilege (refused with 1314 on the Dell — measured), a junction any user may make. The guard above
    still holds because `symlink_metadata(..).file_type().is_symlink()` is TRUE for a junction (measured), and
    `remove_symlink` removes one with `remove_dir` — which leaves the target's files, where `remove_file` is
    refused (measured). A junction points only at a local drive path; a network share is refused by name.
    The folder name the panel offers is the picked path's last part through `src/lib/path.ts` `baseName`
    (either separator, ledger #629) — a `split("/")` offered the whole `C:\…` path on the Dell, and the
    backend's name check refused it. The app's own delete was then measured there: the junction gone, the
    checkout's files byte-identical.
    Delete confirms are status-driven (`loss_warning`: exact changed/
    untracked/unpushed counts; linked = calm "removes only the link").
    Watchers (`RepoWatches`) are session processes: process-grouped, die
    with the app, Restart-never-auto.
- **Autostart** (`AutostartManager`, macOS): per-user LaunchAgent
  `~/Library/LaunchAgents/dev.rexenv.rexenv.plist`, `RunAtLoad` — launches the app at
  login (not headless services; the edge still needs its `:443` prompt). **The login
  launch is also what runs Start all** (`auto_start_services`, #175's guards: never download,
  never prompt), gated on the `--hidden` flag and on nothing else — so "Start rexenv at
  login" is ONE toggle and the whole of "my sites are back after a reboot", and a launch the
  user made never starts services behind their back (ledger #739). Until 29 Sep 2026 a second
  toggle, "Start services when rexenv opens", held that half in a setting
  (`start_services_on_launch`) that gated ANY launch: two switches for one wish, and either one
  alone did something the user did not mean. The setting is retired and a leftover row is swept
  at launch. **A boot-supervised edge that is merely slow is waited for** (#743): login-start never
  prompts, so a plan that needs the password used to be "skipped" at once — on the 15.8 VM the
  LaunchDaemon brought caddy up 70 s after login-start had already told the user to press Start
  all. When the supervisor is installed and enabled (macOS LaunchDaemon, Linux systemd unit) it
  now polls the admin socket for up to 120 s and adopts the edge; Windows' edge has no boot
  supervisor and does not wait. **A login that finds rexenv already running still starts the stack:** the
  `--hidden` launch hands off with `{"cmd":"app.open","args":{"login":true}}`
  (`cli_server::handoff_request`, the same line over the unix socket and Windows' pipe), and the
  running instance runs login-start instead of raising its window — once per process, held
  until setup is ready if the handoff lands first (`LoginStartGate`). Without it, the drafted
  0.8.10 lost every login where macOS's "Reopen windows when logging back in" (the restart
  dialog's default) relaunched rexenv first: the `--hidden` launch sent a bare `app.open`, exited,
  and the stack stayed down (15.8 VM, 29 Sep 2026). The plist
  passes **`--hidden`**: a login launch goes to the menu bar and opens NO window, which
  is what makes autostart tolerable — an app that throws a window at every login is one
  that has to be closed before work starts. The flag is a REQUEST: `first_window_decision`
  still shows the window when first-run setup is unfinished (no OS resolver file, or the
  CA untrusted for this user — the same two facts `FirstRunGate` routes on), because a
  silent tray on a machine that cannot resolve `.rex` hides the only screen that fixes
  it. An init failure shows the window too: an error screen nobody can see is a log line.
  The plist is REFRESHED on every launch while autostart is on (`AutostartManager::
  refresh`) — a plist written by an older build names an older binary and lacks the
  flag, and nothing else would ever repair it. Refresh is NOT enable: a byte-identical
  plist is left alone (no `launchctl load -w` churn, which also clears a `Disabled` the
  user set), and a launch from OUTSIDE an `.app` bundle keeps the recorded binary while
  it still exists — one run of a dev build used to re-point the login item at a
  `target/debug` path the next `cargo clean` deleted, with no symptom until the next
  reboot (review, 3 Sep 2026).
  **On Windows the login item is the per-user Run value** (`WindowsAutostart` over `platform/windows/
  autostart.rs` + `autostart_rules.rs`, W7 S4, ledger #623): `rexenv` = `"<exe>" --hidden` under
  `HKCU\…\CurrentVersion\Run`, written with no elevation — never a scheduled task, whose job refuses the
  breakaway rexenv's services need (plan §3 D1, measured). Task Manager's Startup tab disables an item
  without touching the Run key, in a `StartupApproved\Run` value whose first byte has its low bit set, so
  `is_enabled` is the Run value present AND not disabled there; `enable` — the user's choice in rexenv —
  clears that value; `disable` removes both. `refresh` keeps macOS's rules: rewrite only a changed value, and
  never move the item to a build in a cargo `target` folder while the recorded program still exists.
- **One rexenv per app-data dir.** `run()` CLAIMS the CLI socket before Tauri boots
  (`cli_server::claim_at_startup`): if something accepts a connect on it, a live
  instance owns this app data, so the launch sends `app.open` (best-effort) and EXITS;
  otherwise the socket is bound right there and `setup` adopts the listener. Claim,
  not probe: the probe-then-bind-later shape left a seconds-wide window in which a
  second launch booted fully and then unlinked the first one's socket. The socket is the lock precisely because a
  listener dies with its process — a stale socket file refuses connections, so `connect`
  succeeding is proof of life, unlike a pid file that outlives the process that wrote it.
  Same reason the check runs before Tauri: a process that must not exist should not first
  open a window, adopt services and bind sockets. `rex open` sends the same command.
  **On Windows the lock is a named pipe** (`cli_server::claim_pipe_at_startup` over
  `platform/windows/app_pipe.rs` + `app_pipe_rules.rs`, W7 S1, ledger #620; owner's ruling Q1): the name is
  a digest of the lower-cased config folder (Windows paths are case-insensitive), the first instance is
  created with `FILE_FLAG_FIRST_PIPE_INSTANCE` and the owner-only descriptor private files get (#597), and a
  create refused with access denied — what the Dell measured for a held name — means another instance, so
  the launch connects, sends `app.open` and exits; any other error starts the app without the lock. The
  pipe answered ONLY `app.open` in W7; since W8 S2 it is the CLI's transport too — every connection runs
  `cli_server::serve_connection`, the one exchange the unix socket runs (ledger #631), so a same-user
  process can send it any `rex` command, the trust the socket's `0600` gives on macOS — and the server makes
  each next instance before letting the current one go — the lock is "an instance of this name exists", so a moment
  with none is a moment a second launch could take it. Before this, the unix-only claim meant every
  double-click on Windows booted a second full app: a second SQLite writer, a second watchdog.
- **The window starts hidden** (`tauri.conf.json` `visible: false`) and somebody has to
  decide to show it. A launch the USER asked for shows it immediately, BEFORE the
  database opens and services are adopted: that work takes seconds, and a launch that
  paints nothing for three seconds reads as a launch that failed.

### Built-in terminal — a LOGIN shell (`core/terminal.rs`)

The site Terminal tab is a `portable-pty` session running the user's `$SHELL` in the
docroot, with the site's PHP dir and the generated `wp` wrapper prepended to `PATH`
twice: once in the child's env, and again as an injected `export` AFTER the rc files
run (macOS `path_helper` reorders PATH, which would otherwise shadow the bundled PHP).

It spawns the shell with `-l` — a **login** shell — and that flag is the whole
difference between a usable terminal and a decorative one. rexenv is launched by
launchd/Finder, so the app process inherits the bare `/usr/bin:/bin:/usr/sbin:/sbin`,
and a non-login zsh reads only `~/.zshrc`: never `/etc/zprofile` (path_helper →
`/etc/paths`, `/etc/paths.d`) and never `~/.zprofile` (`brew shellenv`). Without it,
`code`, `rex`, brew-installed git and every version-manager shim answered
`command not found` inside rexenv while working in Terminal.app — reported from the
app's own terminal, and invisible to any developer who tests by running the example
from a terminal that already had a full PATH. Terminal.app itself gets this for free
(`login -pf`). Shells rexenv does not recognise get no flag at all: a bad flag fails
the spawn, and a short PATH beats a terminal that will not open. The cost is startup
latency — a real `~/.zshrc` (prompt frameworks, nvm) takes seconds, which is why
`terminal_check`'s deadlines are seconds and not milliseconds.

The wrapper stays AMBIENT on purpose (#228): this is the user's own command line, so
`WP_CLI_PACKAGES_DIR` is theirs here and pinned everywhere else.

A terminal can also open **in one plugin's or theme's own folder** — the terminal
button on a WordPress row (`/sites/:id/terminal?plugin=<slug>`). Only the kind and the
slug cross IPC; `core::terminal::asset_cwd` resolves the directory through
`repo::asset_dest`, the same place the recorded content dir (`app` for Bedrock,
`content` for Radicle) and the folder-name validation already live, so the frontend
never names a path and a Bedrock site does not get a shell in a dead `wp-content/`.
An asset with no folder of its own — a single-file plugin, a must-use plugin, a
drop-in — is an ERROR rather than a quiet shell in the docroot that would read as the
plugin's directory; the rows that can only be folderless (must-use, drop-in) do not
show the button at all. It is a SEPARATE session from the site's own shell, not a `cd`
typed into it: that shell may be mid-`composer install`, and the keystrokes would have
gone to composer.

The same folder can also be opened in the user's OWN terminal app
(`terminal_open_external`), from three places: the site's quick tile, each
plugin/theme row, and the Terminal tab's own toolbar — where it is a plain
"Open in terminal" list rather than a split button, because there is no
OS-level default terminal for a plain click to stand behind (the way the
`https` handler backs "Open in browser"). It resolves the directory through exactly the
same `asset_cwd` path, so the two destinations can never disagree about which
folder "this plugin" means; what it does NOT carry is the bundled PHP and the
`wp` wrapper, because that shell is spawned by the terminal app, not by us.

**The session outlives its React component** (`SiteTerminal.tsx`) — the same rule the
services follow. A tab switch unmounts the component, and the first version killed the
PTY and disposed the xterm in its cleanup, so a `composer install` died on an alt-tab
and its output was gone on the way back. The shell, its scrollback and its xterm now
live in a module-level map keyed by site AND by the folder the shell opened in; the
component borrows them. The container DOM
node is MOVED between the tab and an offscreen parking bay — xterm renders into the
element it was opened with and cannot be `open()`ed twice, and a node removed from the
document measures 0×0, which is what the renderer sizes itself from. At most
`MAX_LIVE` (4) shells are kept: each is a real login shell with a full rc behind it, and
an evicted one simply reopens on the next visit — the behaviour every session had before
the store existed. **Restart** is the one control that disposes: it closes the PTY, drops
the entry and builds a fresh one.

## 10. Verification pattern

Layer model + gate tiers: `docs/TESTING.md`. Claim inventory (the test metric):
`docs/CLAIM-LEDGER.md`. The pre-commit bar is `scripts/verify.sh` (lib tests +
example builds + clippy at zero + tsc — green ONLY from its own final line);
`scripts/verify-full.sh` adds the sandbox live-check tier + the WebKit harness.

- `cargo test --lib` in `src-tauri/` — unit tests on pure functions. The count is
  deliberately not written here: it was "539 and growing" for long enough to be wrong
  by 350, and a number nobody can cheaply check is worse than no number. `verify.sh`
  prints it on every run; the counts this file DOES state are generated and enforced
  (`scripts/doc-counts.sh`).
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

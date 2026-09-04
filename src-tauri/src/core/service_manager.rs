//! core::service_manager — owns the shared-service lifecycle (task 10.5).
//!
//! The app holds one `ServiceManager` (in app state) that starts/stops the
//! shared stack — MySQL, the php-fpm pool, the shared Nginx, and the Caddy edge
//! router — and reloads Nginx + Caddy when sites change. Each start is gated on
//! `core::ports::ensure_free` so a conflict (e.g. another stack on :443) is a
//! clear error, not a crash. Caddy on a privileged port (:443) is started as
//! root via `PrivilegeManager` (one prompt) and driven afterward through its
//! admin API; on a high port it's a supervised child.

use crate::core::db::DbEngine;
use crate::core::{adminer, apache, binaries, frankenphp, macho, mail, php, ports, proxy, services, sites, ssl, stack_guard};
use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use crate::state::models::{Site, SiteServing, WebServer};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use crate::core::proc::Proc;
use std::process::Child;
use std::time::Duration;

/// Ports the managed services bind. Defaults are the canonical rexenv ports
/// (Caddy on :80/:443); override for dev/testing on high ports.
#[derive(Debug, Clone)]
pub struct Ports {
    pub http: u16,
    pub https: u16,
    pub nginx: u16,
}

impl Default for Ports {
    fn default() -> Self {
        Self {
            http: proxy::DEFAULT_HTTP_PORT,
            https: proxy::DEFAULT_HTTPS_PORT,
            nginx: services::NGINX_HTTP_PORT,
        }
    }
}

struct Bins {
    nginx: PathBuf,
    caddy: PathBuf,
}

/// How Caddy is being run (privileged-root vs supervised child vs not running).
#[derive(Default)]
enum CaddyHandle {
    #[default]
    Stopped,
    /// Root edge under the OS supervisor (macOS LaunchDaemon `KeepAlive`) — the
    /// default privileged edge. launchd keeps it alive; an explicit stop must
    /// `bootout` it (`proxy::stop_edge_daemon`), and the health watchdog does NOT
    /// mark it down on a transient socket blip (launchd is already relaunching).
    Daemon,
    /// Legacy root edge started directly via PrivilegeManager/osascript (driven via
    /// the admin API). Still adopted if found live, but no longer the start path.
    Privileged,
    /// Supervised child (high, non-privileged port).
    Child(Child),
}

/// One service's status for the UI / metrics. `name` is owned because php-fpm
/// pools are named per version (e.g. `PHP-FPM 8.1`).
#[derive(Debug, Clone)]
pub struct ServiceInfo {
    pub name: String,
    pub running: bool,
    pub pid: Option<u32>,
    pub port: u16,
    /// True for services Start-all does NOT manage (user-toggled DB engines
    /// like Postgres): the footer's "All running" counts them only while
    /// running, so a never-started optional engine can't degrade it forever.
    pub optional: bool,
}

/// A prepared Caddy-edge start the caller runs OUTSIDE the services lock (the
/// privileged start blocks on an admin-password prompt).
pub struct EdgePlan {
    pub privileged: bool,
    pub caddy_bin: PathBuf,
    pub caddyfile: PathBuf,
}

/// What an UNATTENDED start may do with a prepared edge plan — LOGIN-SAFETY
/// guard 2 (`commands::services::auto_start_inner`). A privileged plan would
/// block on an admin-password prompt at login, so it is never run: skipped
/// and surfaced instead. Pure so the decision is testable
/// (`login_edge_action_never_runs_a_privileged_plan`).
pub enum LoginEdgeAction {
    /// No plan — the edge is already serving (adopted/reloaded).
    AlreadyServing,
    /// The plan needs the admin prompt: skip it, tell the user.
    SkipNeedsPrompt,
    /// Unprivileged high-port edge (dev config): safe to start silently.
    StartUnprivileged,
}

pub fn login_edge_action(plan: &Option<EdgePlan>) -> LoginEdgeAction {
    match plan {
        None => LoginEdgeAction::AlreadyServing,
        Some(p) if p.privileged => LoginEdgeAction::SkipNeedsPrompt,
        Some(_) => LoginEdgeAction::StartUnprivileged,
    }
}

/// One database engine's status (for the Databases view).
#[derive(Debug, Clone)]
pub struct DbInfo {
    pub engine: DbEngine,
    /// The version `spawn_db` runs / would run (the selected one).
    pub version: String,
    pub running: bool,
    pub pid: Option<u32>,
}

/// Consecutive automatic restarts the health watchdog attempts per service
/// before giving up (a crash loop must surface, not restart-storm).
pub const MAX_RESTART_ATTEMPTS: u32 = 3;

/// Watchdog polls (10s apart) a dead `Daemon` edge gets before the watchdog stops
/// trusting launchd to relaunch it. KeepAlive's restart throttle is ~10s, so a real
/// supervisor restart lands within 1–2 polls; 3 misses (~30s) means it is NOT coming
/// back (booted out, disabled, uninstalled, or `:443` blocked) → declare edge-down.
pub const EDGE_SUPERVISOR_GRACE_POLLS: u32 = 3;

/// One health-watchdog observation: a managed service found dead and what was
/// done about it. Serialized to the frontend (`service-health` event) and
/// appended to `<log_dir>/health.log` as root-cause evidence.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthEvent {
    pub service: String,
    /// "restarted" | "restart-failed" | "gave-up" | "edge-down" | "adopted".
    pub action: &'static str,
    pub detail: String,
}

#[derive(Default)]
pub struct ServiceManager {
    bins: Option<Bins>,
    ports: Ports,
    dbs: HashMap<DbEngine, Proc>,
    pools: php::PhpFpmPools,
    /// Per-site override backends (FrankenPHP / Apache), keyed by domain (§4.1).
    overrides: HashMap<String, OverrideBackend>,
    /// FrankenPHP binary, resolved lazily on first override (avoids a download
    /// when no site uses it).
    frankenphp_bin: Option<PathBuf>,
    /// Apache httpd bundle dir, resolved lazily on first Apache override.
    httpd_dir: Option<PathBuf>,
    nginx: Option<Proc>,
    caddy: CaddyHandle,
    /// Mailpit mail-catcher (§2.1), resolved + started lazily.
    mailpit: Option<Proc>,
    mailpit_bin: Option<PathBuf>,
    /// Consecutive health-watchdog restart attempts per service (keyed by the
    /// status row name). Reset when the service is seen healthy again and on
    /// manual start/stop, so a crash loop can't restart-storm forever.
    restart_attempts: HashMap<String, u32>,
    /// Consecutive watchdog probe-MISSES for an ADOPTED service (keyed by label).
    /// An adopted handle has no start-grace and its `alive()` is a bare-pid check
    /// we must NOT trust (recycle trap), so we require several consecutive misses
    /// of the positive-ID probe (a marked listener on our port) before reaping —
    /// a single transient miss must not reap a live adopted service (B29). Reset
    /// on any positive probe.
    adopted_misses: HashMap<String, u32>,
    /// The edge PROCESS runs but a foreign proxy answers loopback `:443` in front
    /// of it (a specific `127.0.0.1:443` bind shadows our wildcard bind with no
    /// error anywhere — observed live with Herd). Set/cleared by the watchdog's
    /// wire probe (`proxy::edge_answers_as_ours`); while true, `status()` reports
    /// Caddy NOT running — a green edge that serves nothing is a lie.
    edge_blocked: bool,
    /// Consecutive watchdog polls that found a `Daemon` edge's admin socket dead.
    /// launchd's KeepAlive normally relaunches within its ~10s throttle, so a few
    /// dead polls are a restart in progress — but a daemon that was booted out /
    /// disabled / uninstalled outside the app never comes back, and after
    /// [`EDGE_SUPERVISOR_GRACE_POLLS`] the watchdog must stop reassuring and
    /// declare the edge down (with a diagnosis) instead of lying every 10s forever.
    edge_dead_polls: u32,
    /// Per-minor PHP ini settings (whitelisted keys, pre-validated values) from
    /// the SQLite `php_settings` table — loaded by the start command, updated by
    /// the settings command. Source for both the pool configs (`php_value` lines)
    /// and the per-site nginx `client_max_body_size`.
    php_settings: HashMap<String, Vec<(String, String)>>,
    /// Per-site env vars (validated pairs) from the SQLite `site_env` table,
    /// keyed by site id — loaded by the start command, updated by the env
    /// command (mirrors `php_settings`). Source for the per-site nginx
    /// `fastcgi_param` lines and FrankenPHP `env` lines.
    site_env: HashMap<String, Vec<(String, String)>>,
    /// Extra domains per site id (v42) — mirrored, same reason as `site_env`.
    site_aliases: HashMap<String, Vec<String>>,
    /// Selected version per DB engine (per-engine version switch) — a mirror
    /// of the `db_version_<engine>` settings, refreshed by commands like
    /// `site_env`. Absent entry = the engine's default pin.
    db_versions: HashMap<DbEngine, String>,
}

/// Which override server a site runs (`None` for shared-nginx sites).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OverrideKind {
    Frankenphp,
    Apache,
}

impl OverrideKind {
    fn of(server: WebServer) -> Option<OverrideKind> {
        match server {
            WebServer::Frankenphp => Some(OverrideKind::Frankenphp),
            WebServer::Apache => Some(OverrideKind::Apache),
            _ => None,
        }
    }
    fn port(&self, domain: &str) -> u16 {
        match self {
            OverrideKind::Frankenphp => frankenphp::site_port(domain),
            OverrideKind::Apache => apache::site_port(domain),
        }
    }
    fn label(&self) -> &'static str {
        match self {
            OverrideKind::Frankenphp => "FrankenPHP",
            OverrideKind::Apache => "Apache",
        }
    }
}

/// One tracked per-site override backend process.
struct OverrideBackend {
    kind: OverrideKind,
    port: u16,
    child: Proc,
}

/// Consecutive positive-ID misses before an ADOPTED service is declared dead.
/// The watchdog ticks every 10s (`lib.rs`), so 2 = ~20s of a marked listener
/// being absent — past any transient hiccup, still prompt for a genuinely-dead
/// service (B29).
const ADOPTED_MISS_LIMIT: u32 = 2;

/// Decide an adopted service's fate from the positive-ID probe result and its
/// running miss count. Returns `(new_miss_count, reap)`. Pure, so the
/// flap-vs-recycle contract is unit-testable in isolation (B29):
/// - `still_ours` (a marked listener holds our port) → reset to 0, never reap —
///   a live-and-ours service can never accumulate misses.
/// - a miss → increment; reap only once `limit` CONSECUTIVE misses accrue.
///
/// `still_ours` MUST come from `owned_master(port, app-data-marker)` (ownership
/// AND liveness), never `proc.alive()`/`kill -0` (a bare pid a recycled process
/// would satisfy — the trap). A recycled foreign pid carries no app-data marker,
/// so it reads as a miss and is correctly reaped.
/// `pub(crate)`: the php-fpm pool reap (`core::php::pool_fate`, B29b) shares
/// this exact contract rather than re-expressing it — one definition of
/// "misses accumulate, health resets, reap at the limit".
pub(crate) fn adopted_reap_decision(still_ours: bool, misses: u32, limit: u32) -> (u32, bool) {
    if still_ours {
        (0, false)
    } else {
        let n = misses + 1;
        (n, n >= limit)
    }
}

/// What restarting ONE site actually means — and it is not the same thing for
/// every site, which is the whole reason this is a type rather than a `()`.
///
/// rexenv's default topology has NO per-site process: browser → Caddy → ONE
/// shared nginx → one php-fpm pool per PHP VERSION. So "restart this site" has
/// an honest answer only for a site with an override backend (FrankenPHP or
/// Apache on its own loopback port). For everything else the truthful answer is
/// that its config was rewritten and the web tier reloaded, and that the thing
/// a user might mean — bouncing the pool — would hit every other site on that
/// minor, so it is opt-in and says how many.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SiteRestartOutcome {
    /// The site has a backend of its own; it was stopped and respawned.
    Backend { server: &'static str, port: u16 },
    /// Nothing site-specific runs: shared nginx + a shared pool.
    Shared { php_minor: String, pool_port: u16 },
    /// The site HAS a backend and the stack guard refused to stop it (an
    /// adopted backend, asked by a non-app process). Never a silent no-op —
    /// the caller reports it, because the user asked for a restart and did not
    /// get one.
    Refused { server: &'static str },
}

/// A web-tier service a single-service command may name.
///
/// The web tier deliberately had NO per-service IPC for a long time
/// (`docs/CLI-ROADMAP.md`), because its parts are not independent: every default
/// site is served by the ONE nginx, every site on a PHP minor by the ONE pool,
/// and everything by the ONE edge. The ruling that unblocked it is that
/// **restart is the only safe verb** — there is no useful "stopped" state for a
/// web-tier service (a stopped nginx is every default site 502-ing with nothing
/// on screen to say why; the way to stop the stack is to stop the stack), while
/// restart is what people actually want: pick up a change, clear a wedged
/// worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebTarget {
    /// The shared nginx.
    Nginx,
    /// The php-fpm pool for one PHP minor (its debug pool too, when running).
    Pool(String),
    /// The edge. Reloaded, never restarted — see [`WebRestartOutcome::Reloaded`].
    Edge,
}

impl WebTarget {
    /// Parse a CLI name: `nginx`, `edge`/`caddy`, `php-8.3` (or a bare `8.3`).
    /// Unknown names are refused rather than guessed — a typo that silently
    /// restarted the wrong tier would be the worst possible convenience.
    pub fn parse(name: &str) -> Option<WebTarget> {
        let n = name.trim().to_ascii_lowercase();
        match n.as_str() {
            "nginx" => Some(WebTarget::Nginx),
            "edge" | "caddy" => Some(WebTarget::Edge),
            _ => {
                // A PINNED minor, not merely a well-formed one: `fpm_port`
                // computes a port for any `x.y` (the offset is arithmetic), so
                // gating on it would accept `php-9.9` and answer "restarted" for
                // a pool that has never existed.
                let minor = n.strip_prefix("php-").unwrap_or(&n);
                php::all_minors()
                    .into_iter()
                    .find(|m| m == minor)
                    .map(WebTarget::Pool)
            }
        }
    }

    /// The name status/logs use for this target.
    pub fn label(&self) -> String {
        match self {
            WebTarget::Nginx => "Nginx".into(),
            WebTarget::Pool(minor) => format!("PHP-FPM {minor}"),
            WebTarget::Edge => "Caddy".into(),
        }
    }
}

/// What a web-tier restart did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebRestartOutcome {
    /// Stopped and started again, on a FRESHLY generated config.
    Restarted,
    /// The edge: config regenerated and reloaded, not restarted. The edge is a
    /// root KeepAlive daemon — stopping it is a privileged `disable` + `bootout`
    /// with every site offline at :443 in between, and what a reload gives is
    /// the thing anybody asking for a restart wanted (the new config, live).
    Reloaded,
    /// It is not running, so there is nothing to restart. Says so rather than
    /// starting it: `rex start` brings the stack up in the right ORDER, and a
    /// single service started out of order is a stack that half works.
    NotRunning,
    /// Adopted from another session, and the stack guard forbids this process
    /// stopping it (a live-check example against the user's real stack).
    Refused,
}

impl ServiceManager {
    pub fn with_ports(ports: Ports) -> Self {
        Self {
            bins: None,
            ports,
            dbs: HashMap::new(),
            pools: php::PhpFpmPools::default(),
            overrides: HashMap::new(),
            frankenphp_bin: None,
            httpd_dir: None,
            nginx: None,
            caddy: CaddyHandle::Stopped,
            mailpit: None,
            mailpit_bin: None,
            restart_attempts: HashMap::new(),
            adopted_misses: HashMap::new(),
            edge_blocked: false,
            edge_dead_polls: 0,
            php_settings: HashMap::new(),
            site_env: HashMap::new(),
            site_aliases: HashMap::new(),
            db_versions: HashMap::new(),
        }
    }

    /// Load the per-minor PHP ini settings (from SQLite) into the manager + the
    /// pool writer. Like the site list, the map is handed in so this stays
    /// DB-agnostic. Applies to pools (re)started afterward.
    pub fn set_php_settings(&mut self, settings: HashMap<String, Vec<(String, String)>>) {
        self.php_settings = settings;
        self.pools.set_settings(self.php_settings.clone());
    }

    /// Load the per-site env vars (from SQLite, keyed by site id) into the
    /// manager. Like `set_php_settings`, the map is handed in so this stays
    /// DB-agnostic. Takes effect at the next reload/start.
    /// Replace the selected-DB-version mirror (from settings, at start/adopt/
    /// switch time). The watchdog's respawn reads it, so a crashed engine
    /// comes back on the SELECTED version.
    /// Mirror the registry's per-minor EFFECTIVE patch (the user's Update choice,
    /// floored by the pin) into the pool manager. Same seam as
    /// [`Self::set_php_settings`] and [`Self::set_db_versions`], and set from the
    /// same snapshot — so `core::php` never touches the database.
    pub fn set_php_patches(&mut self, patches: std::collections::HashMap<String, String>) {
        self.pools.set_patches(patches);
    }

    pub fn set_db_versions(&mut self, versions: HashMap<DbEngine, String>) {
        self.db_versions = versions;
    }

    /// Update ONE engine's mirrored selection (start/switch commands).
    pub fn set_db_version(&mut self, engine: DbEngine, version: &str) {
        self.db_versions.insert(engine, version.to_string());
    }

    /// The version `spawn_db` will run for an engine: the mirrored selection,
    /// else the default pin.
    pub fn db_version(&self, engine: DbEngine) -> String {
        self.db_versions
            .get(&engine)
            .cloned()
            .unwrap_or_else(|| engine.default_version().to_string())
    }

    /// Mirror the alias table into the manager (v42), like `set_site_env`: a
    /// config regeneration must not need the database, and the watchdog's
    /// respawn path has no connection at all.
    pub fn set_site_aliases(&mut self, aliases: HashMap<String, Vec<String>>) {
        self.site_aliases = aliases;
    }

    pub fn set_site_env(&mut self, env: HashMap<String, Vec<(String, String)>>) {
        self.site_env = env;
    }

    /// Apply a changed per-site env map: swap it in and — when the stack runs —
    /// regenerate + reload (nginx picks up the new `fastcgi_param` lines; a
    /// FrankenPHP override whose config changed is restarted by the reconcile's
    /// config diff). Values were validated by the caller (`site_env::validate`).
    /// No-op beyond storing the map when stopped (the next start uses it).
    pub async fn apply_site_env(
        &mut self,
        platform: &dyn Platform,
        ca: &ssl::LocalCa,
        sites: &[Site],
        env: HashMap<String, Vec<(String, String)>>,
    ) -> Result<Vec<ReadyCheck>> {
        self.set_site_env(env);
        if !self.is_running() {
            return Ok(Vec::new());
        }
        self.reload(platform, ca, sites, false).await
    }

    /// The resolved binary set — a clean error (not a panic) if a caller runs a
    /// bins-dependent step before `ensure_bins`.
    fn bins(&self) -> Result<&Bins> {
        self.bins
            .as_ref()
            .ok_or_else(|| Error::Other("internal: binaries not resolved yet (ensure_bins must run first)".into()))
    }

    /// Spawn a database engine if we don't already manage it (port-gated) and
    /// return its readiness probe for the caller to [`await_ready`] AFTER
    /// dropping the services lock (M4) — `None` if it was already running.
    pub async fn spawn_db(
        &mut self,
        platform: &dyn Platform,
        engine: DbEngine,
    ) -> Result<Option<ReadyCheck>> {
        if self.dbs.contains_key(&engine) {
            return Ok(None);
        }
        ports::ensure_free(platform, engine.port(), ports::Proto::Tcp, engine.label())?;
        let child = engine.start(platform, &self.db_version(engine)).await?;
        self.dbs.insert(engine, child.into());
        Ok(Some(ReadyCheck {
            service: engine.label().to_string(),
            log: stdout_log(platform, engine.key())?,
            tries: 30,
            probe: Box::new(move || engine.running()),
            // PostgreSQL is the reason this field exists: its pinned builds
            // declare `minos 26.0` while rexenv's floor is macOS 15.
            bin: engine.server_binary(platform, &self.db_version(engine)),
        }))
    }

    /// Ensure a database engine is running: spawn + await readiness inline.
    /// Convenience for examples/tests that hold no lock — commands use
    /// [`Self::spawn_db`] and await with the services lock released.
    pub async fn ensure_db(&mut self, platform: &dyn Platform, engine: DbEngine) -> Result<()> {
        if let Some(check) = self.spawn_db(platform, engine).await? {
            await_ready(vec![check]).await?;
        }
        Ok(())
    }

    /// Stop a database engine we manage (no-op if not running).
    pub fn stop_db(&mut self, platform: &dyn Platform, engine: DbEngine) -> Result<()> {
        if let Some(mut child) = self.dbs.remove(&engine) {
            let _ = engine.stop(platform, child.id());
            child.wait();
        }
        Ok(())
    }

    /// Per-engine status for the Databases view (available engines only).
    pub fn db_status(&self) -> Vec<DbInfo> {
        DbEngine::ALL
            .into_iter()
            .filter(|e| e.available())
            .map(|engine| DbInfo {
                engine,
                version: self.db_version(engine),
                // "running" means an engine WE started this session is alive — not a
                // bare port-listen (a foreign/system DB on the port doesn't count),
                // so status is honest and Stop acts only on ours (task 2.2 / H2).
                running: self.dbs.contains_key(&engine) && engine.running(),
                pid: self.dbs.get(&engine).map(Proc::id),
            })
            .collect()
    }

    /// Resolve (download + cache) the service binaries once.
    pub async fn ensure_bins(&mut self, platform: &dyn Platform) -> Result<()> {
        if self.bins.is_some() {
            return Ok(());
        }
        // php-fpm is resolved per version by the pool manager; DB engines resolve
        // their own binaries via `DbEngine::start`.
        let nginx = binaries::resolve(platform, "nginx", binaries::NGINX_VERSION).await?;
        let caddy = binaries::resolve(platform, "caddy", binaries::CADDY_VERSION).await?;
        self.bins = Some(Bins { nginx, caddy });
        Ok(())
    }

    /// Start the whole stack (idempotent per service). Convenience wrapper used by
    /// examples/tests — it awaits readiness inline. The `start_services` command
    /// instead calls `start_core` + [`await_ready`] + `prepare_edge` in separate
    /// lock scopes so neither the readiness waits (M4) nor the privileged edge
    /// prompt holds the services lock.
    pub async fn start_all(
        &mut self,
        platform: &dyn Platform,
        ca: &ssl::LocalCa,
        sites: &[Site],
        php_minors: &[String],
        adminer_version: &str,
        catch_mail: bool,
    ) -> Result<()> {
        let (caddyfile, checks) =
            self.start_core(platform, ca, sites, php_minors, adminer_version, catch_mail).await?;
        await_ready(checks).await?;
        if let Some(plan) = self.prepare_edge(platform, caddyfile)? {
            if plan.privileged {
                proxy::start_edge_daemon(platform, &plan.caddy_bin, &plan.caddyfile)?;
                self.set_edge_daemon();
            } else {
                let child = proxy::start(platform, &plan.caddy_bin, &plan.caddyfile)?;
                self.set_edge_child(child);
            }
        }
        Ok(())
    }

    /// Start everything EXCEPT the Caddy edge (DBs, adminer, mailpit, php pools,
    /// overrides, nginx) and return the generated Caddyfile path plus the batch
    /// of readiness probes. Run under the services lock; the caller then (1)
    /// [`await_ready`]s the probes and (2) starts the (blocking, privileged)
    /// edge — both WITHOUT the lock, so neither a slow-starting service (M4)
    /// nor the admin-password prompt blocks status polls or other commands
    /// (see `prepare_edge`). Spawning is fast (spawn + insert handle); only the
    /// waiting is deferred.
    pub async fn start_core(
        &mut self,
        platform: &dyn Platform,
        ca: &ssl::LocalCa,
        sites: &[Site],
        php_minors: &[String],
        // Passed down rather than mirrored in a field: `adminer::ensure` is
        // called from exactly one place and there is no watchdog respawn path, so
        // a mirror would only add a way for the planner and the stager to
        // disagree — and any fallback for "the field was never set" reproduces
        // ledger #175 by downloading inside the services lock.
        adminer_version: &str,
        // The mail catch-all (`mail::catch_all_enabled`), read by the CALLER
        // from SQLite. Passed rather than mirrored for the same reason, plus
        // one this module cares about more: a field would be a snapshot of a
        // fact the user can change from the Settings screen, and a stale
        // snapshot here means a pool that keeps hijacking mail the user just
        // asked to be delivered for real.
        catch_mail: bool,
    ) -> Result<(PathBuf, Vec<ReadyCheck>)> {
        self.ensure_bins(platform).await?;
        // Manual intervention resets the watchdog's give-up counters.
        self.restart_attempts.clear();
        let mut checks = Vec::new();

        // MySQL — the site stack needs it (started via the DB engine manager).
        checks.extend(self.spawn_db(platform, DbEngine::Mysql).await?);

        // MariaDB — required exactly when some site's database lives there
        // (it stays a user-toggled optional engine otherwise).
        if sites
            .iter()
            .any(|s| matches!(s.db_engine, crate::state::models::SiteDbEngine::Mariadb))
        {
            checks.extend(self.spawn_db(platform, DbEngine::Mariadb).await?);
        }

        // Adminer docroot (§5.2): download + stage the interpreter's copy so the
        // internal vhost the configs reference is actually served.
        crate::core::adminer::ensure(platform, adminer_version).await?;

        // Mailpit BEFORE the pools so each pool's config can route PHP `mail()` to
        // it (§2.2): resolves the binary (sets `mailpit_bin`) and starts the sink.
        // The pools only need the BINARY path (sendmail shim), not a ready Mailpit.
        checks.extend(self.spawn_mailpit(platform).await?);
        self.pools.set_mail_catch(mail::catch_for(self.mailpit_bin.as_deref(), catch_mail));

        // PHP-FPM: one pool per installed PHP version (always at least the default,
        // so the single-site path keeps working). Pools own their deterministic ports.
        let mut minors: Vec<String> = php_minors.to_vec();
        let default_minor = php::minor_of(binaries::PHP_VERSION);
        if !minors.contains(&default_minor) {
            minors.push(default_minor);
        }
        self.pools.start(platform, &minors).await?;

        // DEBUG pools for the minors of Xdebug-toggled sites (§8.2) — the
        // generated configs route those sites at the debug ports, so the pools
        // must exist for exactly those minors, and only those.
        let mut debug_minors: Vec<String> = sites
            .iter()
            .filter(|s| s.xdebug)
            .map(|s| php::minor_of(&s.php_version))
            .filter(|m| binaries::xdebug_supported(m))
            .collect();
        debug_minors.sort_unstable();
        debug_minors.dedup();
        for minor in &debug_minors {
            self.pools.ensure_debug(platform, minor).await?;
        }

        // Per-site override backends (FrankenPHP) for the current site set.
        checks.extend(self.reconcile_overrides(platform, sites).await?);

        let bins = self.bins()?;

        // Configs derived from all sites.
        let cfg = sites::rebuild_configs_for(
            sites,
            platform,
            ca,
            self.ports.nginx,
            self.ports.http,
            self.ports.https,
            &php::nginx_body_limits(&self.php_settings),
            &self.site_env,
            &self.site_aliases,
        )?;

        // Shared Nginx.
        if self.nginx.is_none() {
            ports::ensure_free(platform, self.ports.nginx, ports::Proto::Tcp, "Nginx")?;
            self.nginx = Some(services::start_nginx(platform, &bins.nginx, &cfg.nginx_conf, &cfg.nginx_prefix)?.into());
        }

        Ok((cfg.caddyfile, checks))
    }

    /// Prepare the Caddy edge start. If the edge is stopped, clear any stale edge
    /// and gate the port, then return a plan the caller runs WITHOUT the services
    /// lock (the privileged start blocks on the admin-password prompt). `None` if
    /// the edge is already running — including an edge found LIVE on our admin
    /// socket while this manager thought it stopped (a prior session's survivor,
    /// or a stale edge-down mark): that edge is adopted and reloaded in place,
    /// never stopped — stopping it would drop every site mid-serve and force a
    /// fresh admin-password prompt for a stack that was already up.
    pub fn prepare_edge(
        &mut self,
        platform: &dyn Platform,
        caddyfile: PathBuf,
    ) -> Result<Option<EdgePlan>> {
        let alive = proxy::admin_alive(platform);
        if !matches!(self.caddy, CaddyHandle::Stopped) {
            if alive {
                return Ok(None);
            }
            // The handle says running but the socket is dead — a STALE handle
            // ("running" = ownership AND liveness, H2; never trust state alone).
            // Seen live: the watchdog re-adopted a Daemon edge in the window
            // between Stop-all clearing the handle and the privileged bootout
            // landing; the old unconditional early-return then made every later
            // Start-all silently skip the edge forever. Reap a dead child and
            // fall through to a fresh start instead.
            if let CaddyHandle::Child(mut c) = std::mem::take(&mut self.caddy) {
                let _ = c.kill();
                let _ = c.wait();
            }
            self.caddy = CaddyHandle::Stopped;
        }
        let bins = self.bins()?;
        // A live listener on OUR admin socket is rexenv's own edge (private path,
        // 0600) — adopt it and push the current config through its admin API. Only
        // if the reload is refused (wedged edge, or a port set it can't rebind) do
        // we fall through to the stop + fresh-start path.
        if alive && proxy::reload(platform, &bins.caddy, &caddyfile, false).is_ok() {
            // A live edge backed by the KeepAlive daemon is tracked as `Daemon` so an
            // explicit Stop-all boots it out (an admin `caddy stop` alone would just
            // be relaunched); a live edge with no daemon is the legacy osascript one.
            self.caddy = if platform.edge().is_installed() {
                CaddyHandle::Daemon
            } else {
                CaddyHandle::Privileged
            };
            return Ok(None);
        }
        // Clear a leftover REXENV edge (its admin socket + :443) so our start isn't
        // blocked (§7.3). Ownership-gated to our own edge — a foreign Caddy on the
        // default :2019 admin is never touched (task 2.4 / M1).
        proxy::recover_stale_edge(platform, &bins.caddy)?;
        // If OUR KeepAlive daemon is installed, it (or a wedged instance of it) is what
        // holds :443 — and its admin socket may be unreachable (e.g. root-owned again
        // after a reload) so the adopt-reload above couldn't take it. Do NOT treat :443
        // as a foreign conflict: return the plan so `start_edge_daemon` REINSTALLS —
        // its `bootout` stops that edge and frees :443 before the fresh bootstrap
        // rebinds (and deploys the current launcher, healing the socket handoff). Only
        // when NO daemon is installed is a :443 holder a genuine foreign conflict.
        if !platform.edge().is_installed() {
            ports::ensure_free(platform, self.ports.https, ports::Proto::Tcp, "Caddy (HTTPS)")?;
        }
        Ok(Some(EdgePlan {
            privileged: self.ports.https < 1024,
            caddy_bin: bins.caddy.clone(),
            caddyfile,
        }))
    }

    /// Whether the edge is currently tracked as the KeepAlive daemon — the stop
    /// command checks this (under the lock) to decide whether it must `bootout` the
    /// daemon (privileged, OUTSIDE the lock) after `stop_all`.
    pub fn edge_is_daemon(&self) -> bool {
        matches!(self.caddy, CaddyHandle::Daemon)
    }

    /// Test hook: wire fake binary paths so edge state-machine tests can run
    /// `prepare_edge` without resolving (= downloading) real binaries.
    #[cfg(test)]
    pub(crate) fn set_bins_for_tests(&mut self, caddy: PathBuf) {
        self.bins = Some(Bins { nginx: caddy.clone(), caddy });
    }

    /// Insert an ADOPTED db handle (pid only) so the B29 watchdog path can be
    /// exercised without a real DB process.
    #[cfg(test)]
    pub(crate) fn insert_adopted_db_for_test(&mut self, engine: DbEngine, pid: u32) {
        self.dbs.insert(engine, Proc::Adopted(pid));
    }
    #[cfg(test)]
    pub(crate) fn has_db_for_test(&self, engine: DbEngine) -> bool {
        self.dbs.contains_key(&engine)
    }
    #[cfg(test)]
    pub(crate) fn adopted_misses_for_test(&self, label: &str) -> u32 {
        self.adopted_misses.get(label).copied().unwrap_or(0)
    }
    /// Preset the restart counter to the cap so a reap's `should_restart` gate
    /// returns false → `spawn_db` is skipped (no real DB spawn in a unit test).
    #[cfg(test)]
    pub(crate) fn preset_restart_attempts_for_test(&mut self, label: &str, n: u32) {
        self.restart_attempts.insert(label.to_string(), n);
    }

    /// Record the edge as running under the OS supervisor (LaunchDaemon KeepAlive),
    /// the current privileged-start path (`proxy::start_edge_daemon`).
    pub fn set_edge_daemon(&mut self) {
        self.edge_dead_polls = 0;
        self.caddy = CaddyHandle::Daemon;
    }

    /// Record the edge as a legacy root-privileged Caddy (osascript). Kept for the
    /// adopt path and examples; the app start path uses [`Self::set_edge_daemon`].
    pub fn set_edge_privileged(&mut self) {
        self.edge_dead_polls = 0;
        self.caddy = CaddyHandle::Privileged;
    }

    /// Forget the edge WITHOUT stopping anything — reproduces a stale edge-down
    /// mark (manager says stopped, edge still serving). Live-check hook for
    /// `examples/edge_adopt_reload_check.rs`; the app itself only reaches this
    /// state through `reconcile_health`.
    pub fn mark_edge_stopped(&mut self) {
        self.edge_dead_polls = 0;
        self.caddy = CaddyHandle::Stopped;
    }

    /// Record the edge as a child Caddy we own (unprivileged high port).
    pub fn set_edge_child(&mut self, child: std::process::Child) {
        self.edge_dead_polls = 0;
        self.caddy = CaddyHandle::Child(child);
    }

    /// Whether the shared stack is currently started (so reloads / pool changes
    /// take effect). False before `start_all` / after `stop_all`.
    /// Whether something else is answering :443 in front of our edge (a running
    /// Herd shadow-binds 127.0.0.1:443). Maintained by the health watchdog, so
    /// reading it costs nothing — no probe, no wait.
    pub fn edge_blocked(&self) -> bool {
        self.edge_blocked
    }

    pub fn is_running(&self) -> bool {
        self.nginx.is_some()
    }

    /// Ensure a php-fpm pool for `minor` is running, starting it if needed. Used
    /// when a site switches to a PHP version whose pool isn't up yet (§1.4).
    pub async fn ensure_php_pool(&mut self, platform: &dyn Platform, minor: &str) -> Result<()> {
        self.pools.ensure(platform, minor).await
    }

    /// Ensure the DEBUG (Xdebug) pool for `minor` is running (§8.2). Errors for
    /// minors without a pinned Xdebug bottle (8.0) and when the .so fails its
    /// load probe — never a silently Xdebug-less pool.
    pub async fn ensure_php_debug_pool(
        &mut self,
        platform: &dyn Platform,
        minor: &str,
    ) -> Result<()> {
        self.pools.ensure_debug(platform, minor).await
    }

    /// Stop the DEBUG pool for `minor` if it is running (toggle-off tidy-up when
    /// no site on the minor keeps Xdebug enabled). Returns whether one ran.
    pub fn stop_php_debug_pool(&mut self, platform: &dyn Platform, minor: &str) -> bool {
        self.pools.stop_one(platform, minor, true)
    }

    /// Spawn Mailpit if not already managed (resolve its binary on first use),
    /// port-gated on its SMTP + HTTP ports. Returns its readiness probe for the
    /// caller to [`await_ready`] once the services lock is dropped (M4);
    /// `None` if it was already running. Owns start lifecycle (§2.1).
    pub async fn spawn_mailpit(&mut self, platform: &dyn Platform) -> Result<Option<ReadyCheck>> {
        if self.mailpit.is_some() {
            return Ok(None);
        }
        ports::ensure_free(platform, mail::MAILPIT_SMTP_PORT, ports::Proto::Tcp, "Mailpit (SMTP)")?;
        ports::ensure_free(platform, mail::MAILPIT_HTTP_PORT, ports::Proto::Tcp, "Mailpit (HTTP)")?;
        let bin = match &self.mailpit_bin {
            Some(p) => p.clone(),
            None => {
                let p = binaries::resolve(platform, "mailpit", binaries::MAILPIT_VERSION).await?;
                self.mailpit_bin = Some(p.clone());
                p
            }
        };
        self.mailpit = Some(mail::start(platform, &bin)?.into());
        Ok(Some(ReadyCheck {
            service: "Mailpit".to_string(),
            log: stdout_log(platform, "mailpit")?,
            tries: 20,
            probe: Box::new(mail::running),
            bin: None,
        }))
    }

    /// Stop Mailpit if we manage it (no-op otherwise).
    pub fn stop_mailpit(&mut self, platform: &dyn Platform) -> Result<()> {
        if let Some(mut child) = self.mailpit.take() {
            let _ = mail::stop(platform, child.id());
            child.wait();
        }
        Ok(())
    }

    /// FrankenPHP binary, resolved + cached on first use.
    async fn ensure_frankenphp_bin(&mut self, platform: &dyn Platform) -> Result<PathBuf> {
        if let Some(p) = &self.frankenphp_bin {
            return Ok(p.clone());
        }
        let p = binaries::resolve(platform, "frankenphp", binaries::FRANKENPHP_VERSION).await?;
        self.frankenphp_bin = Some(p.clone());
        Ok(p)
    }

    /// Bring the running per-site override backends (FrankenPHP / Apache) in
    /// line with the site set: start one for each override site that isn't up,
    /// stop any whose site was deleted or switched away. Each backend listens
    /// on its kind's deterministic override port. Returns one readiness probe
    /// per newly spawned backend — the caller [`await_ready`]s them
    /// (concurrently) after dropping the services lock (M4).
    async fn reconcile_overrides(
        &mut self,
        platform: &dyn Platform,
        sites: &[Site],
    ) -> Result<Vec<ReadyCheck>> {
        // Desired override backends: domain → the full spawn recipe. The
        // rewrite mode is the site's real one (M5); env vars (§1.6) come from
        // the manager's site-id-keyed map; Apache additionally needs the
        // site's php-fpm pool port (no embedded PHP).
        struct Desired {
            kind: OverrideKind,
            docroot: PathBuf,
            port: u16,
            fpm_port: u16,
            rewrite: services::RewriteMode,
            env: Vec<(String, String)>,
        }
        let desired: HashMap<String, Desired> = sites
            .iter()
            .filter_map(|s| {
                let kind = OverrideKind::of(s.web_server)?;
                // The RECORDED backend port (B20 §4), never re-derived — so the
                // spawned backend and the edge route always agree, even after a
                // domain change. None (only if a stale nginx row) skips the site.
                let port = sites::recorded_override_port(s)?;
                Some((
                    s.domain.clone(),
                    Desired {
                        kind,
                        docroot: s.served_root(),
                        port,
                        fpm_port: sites::pool_port_for_site(s),
                        rewrite: sites::rewrite_mode_for(s.multisite),
                        env: self.site_env.get(&s.id).cloned().unwrap_or_default(),
                    },
                ))
            })
            .collect();

        // Stop backends that are no longer wanted — including a domain whose
        // KIND changed (FrankenPHP ↔ Apache switch = different port + binary).
        let stale: Vec<String> = self
            .overrides
            .iter()
            .filter(|(d, b)| desired.get(*d).map(|w| w.kind) != Some(b.kind))
            .map(|(d, _)| d.clone())
            .collect();
        for domain in stale {
            if let Some(mut backend) = self.overrides.remove(&domain) {
                if !Self::stop_override_backend(platform, &domain, &mut backend) {
                    // Guard refused (adopted backend, non-app process) — keep
                    // tracking it; a forgotten live backend is an instant orphan.
                    self.overrides.insert(domain, backend);
                }
            }
        }

        // Start backends that are wanted but not yet running — and RESTART any
        // whose desired config no longer matches the one the running backend
        // loaded (docroot moved, multisite rewrite changed, PHP pool switched).
        // The on-disk config file is the record of what the backend was started
        // with; a running backend never re-reads it, so a mismatch means stop +
        // respawn (spawn_override rewrites the file).
        let mut checks = Vec::new();
        for (domain, want) in &desired {
            if self.overrides.contains_key(domain) {
                let wanted = self.desired_override_config(platform, domain, want.kind, &want.docroot, want.port, want.fpm_port, want.rewrite, &want.env);
                let current = self
                    .override_config_path(platform, domain, want.kind)
                    .and_then(|p| std::fs::read_to_string(p).ok());
                if wanted.is_some() && wanted == current {
                    continue;
                }
                if let Some(mut backend) = self.overrides.remove(domain) {
                    if !Self::stop_override_backend(platform, domain, &mut backend) {
                        // Guard refused — leave the user's serving backend
                        // alone instead of spawning into its port.
                        self.overrides.insert(domain.clone(), backend);
                        continue;
                    }
                }
            }
            checks.push(
                self.spawn_override(platform, want.kind, domain, &want.docroot, want.port, want.fpm_port, want.rewrite, &want.env)
                    .await?,
            );
        }
        Ok(checks)
    }

    /// Restart the backend of ONE site — the seam a single-site restart needs.
    ///
    /// For an override site this is a real stop + respawn on the site's RECORDED
    /// port, so the edge route it already has keeps pointing at it. For a
    /// default (nginx-served) site there is deliberately nothing to do here: the
    /// caller reloads the web tier and reports [`SiteRestartOutcome::Shared`],
    /// because killing the shared pool to satisfy one site's restart would take
    /// down every other site on that PHP minor without anybody asking.
    ///
    /// Spawn under the lock, `await_ready` after dropping it (M4) — the checks
    /// come back with the outcome rather than being awaited here.
    pub async fn restart_site_backend(
        &mut self,
        platform: &dyn Platform,
        site: &Site,
    ) -> Result<(SiteRestartOutcome, Vec<ReadyCheck>)> {
        let Some(kind) = OverrideKind::of(site.web_server) else {
            return Ok((
                SiteRestartOutcome::Shared {
                    php_minor: php::minor_of(&site.php_version),
                    pool_port: sites::pool_port_for_site(site),
                },
                Vec::new(),
            ));
        };
        let port = sites::recorded_override_port(site).ok_or_else(|| {
            Error::Other(format!(
                "\"{}\" is set to {} but has no recorded backend port — reload the stack first",
                site.domain,
                kind.label()
            ))
        })?;
        if let Some(mut backend) = self.overrides.remove(&site.domain) {
            if !Self::stop_override_backend(platform, &site.domain, &mut backend) {
                // The guard's refusal is the user's answer, not a shrug: put the
                // backend back so it stays tracked, and SAY that nothing was
                // restarted.
                self.overrides.insert(site.domain.clone(), backend);
                return Ok((SiteRestartOutcome::Refused { server: kind.label() }, Vec::new()));
            }
        }
        let check = self
            .spawn_override(
                platform,
                kind,
                &site.domain,
                &site.served_root(),
                port,
                sites::pool_port_for_site(site),
                sites::rewrite_mode_for(site.multisite),
                &self.site_env.get(&site.id).cloned().unwrap_or_default(),
            )
            .await?;
        Ok((SiteRestartOutcome::Backend { server: kind.label(), port }, vec![check]))
    }

    /// Restart ONE web-tier service — the seam behind `rex service restart`.
    ///
    /// **Always on a freshly generated config.** A restart that reused whatever
    /// is on disk would resurrect the service on the state of the world at the
    /// last reload, which is the failure people restart to escape. So nginx is
    /// respawned from `rebuild_configs_for`, and the edge's reload is a rebuild
    /// too.
    ///
    /// Ordering is not this function's job and must not become it: bringing a
    /// stopped service up on its own is `start_all`'s (pools → nginx → edge),
    /// and a service that is not running is reported as such.
    pub async fn restart_web_service(
        &mut self,
        platform: &dyn Platform,
        ca: &ssl::LocalCa,
        sites: &[Site],
        target: &WebTarget,
    ) -> Result<(WebRestartOutcome, Vec<ReadyCheck>)> {
        match target {
            WebTarget::Pool(minor) => {
                if !self.has_php_pool(minor) {
                    return Ok((WebRestartOutcome::NotRunning, Vec::new()));
                }
                let checks = self.restart_pools_for(platform, std::slice::from_ref(minor)).await?;
                Ok((WebRestartOutcome::Restarted, checks))
            }
            WebTarget::Nginx => {
                let Some(nginx) = self.nginx.as_ref() else {
                    return Ok((WebRestartOutcome::NotRunning, Vec::new()));
                };
                if nginx.is_adopted() && !stack_guard::may_control_real_stack() {
                    return Ok((WebRestartOutcome::Refused, Vec::new()));
                }
                let nginx_bin = self.bins()?.nginx.clone();
                let cfg = sites::rebuild_configs_for(
                    sites,
                    platform,
                    ca,
                    self.ports.nginx,
                    self.ports.http,
                    self.ports.https,
                    &php::nginx_body_limits(&self.php_settings),
                    &self.site_env,
                    &self.site_aliases,
                )?;
                if let Some(mut child) = self.nginx.take() {
                    child.kill();
                    child.wait();
                }
                // Workers outlive a killed master and keep the port (their title
                // carries no app-data marker) — the same reap the watchdog does,
                // or the respawn's port gate fails on our own leftovers.
                for pid in platform.supervisor().owned_listeners(self.ports.nginx, "nginx") {
                    let _ = platform.supervisor().stop(pid);
                }
                ports::ensure_free(platform, self.ports.nginx, ports::Proto::Tcp, "Nginx")?;
                self.nginx = Some(
                    services::start_nginx(platform, &nginx_bin, &cfg.nginx_conf, &cfg.nginx_prefix)?
                        .into(),
                );
                let port = self.ports.nginx;
                Ok((
                    WebRestartOutcome::Restarted,
                    vec![ReadyCheck {
                        service: "Nginx".into(),
                        log: platform.paths().log_dir()?.join("nginx-error.log"),
                        tries: 20,
                        probe: Box::new(move || services::nginx_running(port)),
                        bin: None,
                    }],
                ))
            }
            WebTarget::Edge => {
                // Ownership AND liveness: a handle whose process has died is
                // not a running edge, and `proxy::reload` on a corpse would
                // report the reload's failure as the outcome rather than the
                // truth ("not running").
                if matches!(self.caddy, CaddyHandle::Stopped) || !proxy::admin_alive(platform) {
                    return Ok((WebRestartOutcome::NotRunning, Vec::new()));
                }
                let bins = self.bins()?;
                let caddy_bin = bins.caddy.clone();
                let cfg = sites::rebuild_configs_for(
                    sites,
                    platform,
                    ca,
                    self.ports.nginx,
                    self.ports.http,
                    self.ports.https,
                    &php::nginx_body_limits(&self.php_settings),
                    &self.site_env,
                    &self.site_aliases,
                )?;
                proxy::reload(platform, &caddy_bin, &cfg.caddyfile, true)?;
                Ok((WebRestartOutcome::Reloaded, Vec::new()))
            }
        }
    }

    /// Stop an override backend FOR REAL, honoring the stack guard: a non-app
    /// process may not stop an ADOPTED backend (the user's serving stack) —
    /// returns false and the caller keeps/skips it. See
    /// [`Self::reap_override_backend`] for what a real stop means.
    fn stop_override_backend(
        platform: &dyn Platform,
        domain: &str,
        backend: &mut OverrideBackend,
    ) -> bool {
        if backend.child.is_adopted() && !stack_guard::may_control_real_stack() {
            log::warn!("rexenv: stack guard — leaving adopted {domain} backend running");
            return false;
        }
        Self::reap_override_backend(platform, domain, backend);
        true
    }

    /// Reap whatever actually serves an override port. The tracked pid can go
    /// stale (pre-master-fix sessions adopted churning WORKER pids; a crashed
    /// session leaves an untracked tree), so resolve the CURRENT master of
    /// the port and signal that — the tracked pid too when it differs — then
    /// wait, bounded, for the port to actually close: workers exit a beat
    /// after their master, and "stopped" must mean the port is FREE or the
    /// next spawn's port gate trips over our own dying tree. Split from the
    /// guard check so it's unit-testable without global guard state.
    fn reap_override_backend(
        platform: &dyn Platform,
        domain: &str,
        backend: &mut OverrideBackend,
    ) {
        let master = platform
            .paths()
            .app_data_dir()
            .ok()
            .map(|d| d.display().to_string())
            .filter(|m| !m.is_empty())
            .and_then(|m| platform.supervisor().owned_master(backend.port, &m));
        let tracked = backend.child.id();
        let target = master.unwrap_or(tracked);
        let _ = platform.supervisor().stop(target);
        if tracked != target {
            let _ = platform.supervisor().stop(tracked); // no-op if already gone
        }
        backend.child.wait();
        if !ports::wait_free(
            backend.port,
            ports::Proto::Tcp,
            20,
            std::time::Duration::from_millis(100),
        ) {
            log::warn!(
                "rexenv: {domain} backend did not release port {} after stop",
                backend.port
            );
        }
    }

    /// The config a backend SHOULD be running with (the reconcile diff input).
    /// Pure string render — nothing is resolved or written.
    #[allow(clippy::too_many_arguments)]
    fn desired_override_config(
        &self,
        platform: &dyn Platform,
        domain: &str,
        kind: OverrideKind,
        docroot: &Path,
        port: u16,
        fpm_port: u16,
        rewrite: services::RewriteMode,
        env: &[(String, String)],
    ) -> Option<String> {
        match kind {
            OverrideKind::Frankenphp => {
                Some(frankenphp::generate_config(docroot, port, rewrite, env))
            }
            OverrideKind::Apache => {
                // Deterministic bundle dir — the diff must not trigger a resolve.
                let basedir = platform
                    .paths()
                    .bin_dir()
                    .ok()?
                    .join(format!("httpd-{}", binaries::HTTPD_VERSION));
                apache::desired_config(
                    platform, &basedir, docroot, domain, port, fpm_port, rewrite, env,
                )
                .ok()
            }
        }
    }

    fn override_config_path(
        &self,
        platform: &dyn Platform,
        domain: &str,
        kind: OverrideKind,
    ) -> Option<PathBuf> {
        match kind {
            OverrideKind::Frankenphp => frankenphp::config_path(platform, domain).ok(),
            OverrideKind::Apache => apache::config_path(platform, domain).ok(),
        }
    }

    /// Resolve the Apache bundle dir once (mirrors `ensure_frankenphp_bin`).
    async fn ensure_httpd_dir(&mut self, platform: &dyn Platform) -> Result<PathBuf> {
        if let Some(p) = &self.httpd_dir {
            return Ok(p.clone());
        }
        let p = binaries::resolve_bundle(platform, "httpd", binaries::HTTPD_VERSION).await?;
        self.httpd_dir = Some(p.clone());
        Ok(p)
    }

    /// Spawn one per-site override backend (port-gated) and track its handle.
    /// Shared by [`Self::reconcile_overrides`] and the health watchdog's respawn.
    #[allow(clippy::too_many_arguments)]
    async fn spawn_override(
        &mut self,
        platform: &dyn Platform,
        kind: OverrideKind,
        domain: &str,
        docroot: &Path,
        port: u16,
        fpm_port: u16,
        rewrite: services::RewriteMode,
        env: &[(String, String)],
    ) -> Result<ReadyCheck> {
        // Never reap a port a DIFFERENT site's tracked backend already holds:
        // positive identification via our own state, not a bare port match. Two
        // override sites hashing to the same slot would otherwise let this spawn
        // KILL the live sibling — refuse loudly instead (B20). create /
        // set_web_server / set_domain normally stop a collision from ever being
        // persisted; this guard is the permanent runtime safety net.
        if let Some(other) = self
            .overrides
            .iter()
            .find(|(d, b)| d.as_str() != domain && b.port == port)
            .map(|(d, _)| d.clone())
        {
            return Err(Error::Other(format!(
                "can't start {} for \"{domain}\": its backend port ({port}) collides with \
                 \"{other}\" — rename one site or give it a different web server.",
                kind.label()
            )));
        }

        // Self-heal: OUR OWN leftover holding the port (a tree adoption missed
        // — crashed session, pre-master-fix worker adoption) is reaped here,
        // never surfaced to the user as a conflict. Foreign holders fall
        // through to the port gate's honest error. Guarded: an unmarked
        // process (live-check example) fails the gate instead of stopping the
        // user's real backend.
        if !ports::is_free(port, ports::Proto::Tcp) && stack_guard::may_control_real_stack() {
            let leftover = platform
                .paths()
                .app_data_dir()
                .ok()
                .map(|d| d.display().to_string())
                .filter(|m| !m.is_empty())
                .and_then(|m| platform.supervisor().owned_master(port, &m));
            if let Some(master) = leftover {
                log::warn!(
                    "rexenv: reaping our leftover {} (pid {master}) holding port {port}",
                    kind.label()
                );
                let _ = platform.supervisor().stop(master);
                if !ports::wait_free(
                    port,
                    ports::Proto::Tcp,
                    20,
                    std::time::Duration::from_millis(100),
                ) {
                    log::warn!("rexenv: leftover pid {master} did not release port {port}");
                }
            }
        }
        ports::ensure_free(platform, port, ports::Proto::Tcp, kind.label())?;
        let child = match kind {
            OverrideKind::Frankenphp => {
                let bin = self.ensure_frankenphp_bin(platform).await?;
                let conf = frankenphp::write_config(platform, domain, docroot, port, rewrite, env)?;
                frankenphp::start(platform, &bin, domain, &conf, env)?
            }
            OverrideKind::Apache => {
                let basedir = self.ensure_httpd_dir(platform).await?;
                let conf = apache::write_config(
                    platform, &basedir, domain, docroot, port, fpm_port, rewrite, env,
                )?;
                apache::start(platform, &basedir, domain, &conf)?
            }
        };
        self.overrides
            .insert(domain.to_string(), OverrideBackend { kind, port, child: child.into() });
        let log_key = match kind {
            OverrideKind::Frankenphp => format!("frankenphp-{domain}"),
            OverrideKind::Apache => format!("apache-{domain}"),
        };
        Ok(ReadyCheck {
            service: format!("{} ({domain})", kind.label()),
            log: stdout_log(platform, &log_key)?,
            tries: 20,
            probe: Box::new(move || frankenphp::running(port)),
            bin: None,
        })
    }

    /// Reload the edge from the current site set (after create / delete / server
    /// switch): reconcile per-site override backends, then regenerate + reload
    /// Nginx and Caddy. The edge routes each site to its backend (shared Nginx or
    /// its own override port) via `rebuild_configs`. Returns the readiness probes
    /// of any newly spawned backends — callers [`await_ready`] them after
    /// dropping the services lock (M4). Until a backend is ready the edge may
    /// briefly 502 that one site; the command still fails with the named-service
    /// error (M3) if it never comes up.
    ///
    /// `force` forces the Caddy config reload even when the Caddyfile is
    /// byte-identical — required after re-issuing certs (stable cert paths mean
    /// an unchanged config, which Caddy otherwise skips, leaving the OLD leaf
    /// served from its in-memory cache). See [`proxy::reload`].
    pub async fn reload(
        &mut self,
        platform: &dyn Platform,
        ca: &ssl::LocalCa,
        sites: &[Site],
        force: bool,
    ) -> Result<Vec<ReadyCheck>> {
        let checks = self.reconcile_overrides(platform, sites).await?;
        let bins = self
            .bins
            .as_ref()
            .ok_or_else(|| Error::Other("services not started".into()))?;
        let cfg = sites::rebuild_configs_for(
            sites,
            platform,
            ca,
            self.ports.nginx,
            self.ports.http,
            self.ports.https,
            &php::nginx_body_limits(&self.php_settings),
            &self.site_env,
            &self.site_aliases,
        )?;
        if services::reload_nginx(
            platform,
            &bins.nginx,
            &cfg.nginx_conf,
            &cfg.nginx_prefix,
            self.ports.nginx,
        )? == services::ReloadOutcome::NotRunning
        {
            // We believed nginx was up. Not fatal — the config is on disk and
            // the next start picks it up — but it means the manager's view and
            // reality disagree, which the watchdog should be reconciling.
            log::warn!("nginx: nothing to reload — no master was running");
        }
        proxy::reload(platform, &bins.caddy, &cfg.caddyfile, force)?;
        Ok(checks)
    }

    /// Apply a changed per-version PHP settings map: swap in the new map, restart
    /// the affected minor's pool (new config written by `ensure`), and reload the
    /// shared nginx so per-site `client_max_body_size` tracks the new upload/post
    /// sizes. The caller has already VALIDATED the values and `php-fpm -t`-gated a
    /// candidate config, so this can't brick the pool. No-op beyond storing the
    /// map when the stack is stopped (the next start uses it). Returns the
    /// restarted pools' readiness probes (normal + debug when both live) to
    /// [`await_ready`] after the lock drops.
    pub async fn apply_php_settings(
        &mut self,
        platform: &dyn Platform,
        ca: &ssl::LocalCa,
        sites: &[Site],
        settings: HashMap<String, Vec<(String, String)>>,
        minor: &str,
    ) -> Result<Vec<ReadyCheck>> {
        self.set_php_settings(settings);
        if !self.is_running() {
            return Ok(Vec::new());
        }
        // ADOPTED sessions never ran start_all, so nginx/caddy paths aren't
        // resolved yet and the reload below would fail AFTER the pool restart
        // (cache hit here — an adopted stack is running from cached binaries).
        self.ensure_bins(platform).await?;
        // Restart only pools that were actually running; never start a new pool
        // as a side effect of a settings edit.
        let checks = self.restart_php_pool(platform, minor).await?;
        // Nginx: regenerate with the new body limits + reload. Caddy routes are
        // untouched by ini settings — no edge reload needed.
        let bins = self.bins()?;
        let cfg = sites::rebuild_configs_for(
            sites,
            platform,
            ca,
            self.ports.nginx,
            self.ports.http,
            self.ports.https,
            &php::nginx_body_limits(&self.php_settings),
            &self.site_env,
            &self.site_aliases,
        )?;
        if services::reload_nginx(
            platform,
            &bins.nginx,
            &cfg.nginx_conf,
            &cfg.nginx_prefix,
            self.ports.nginx,
        )? == services::ReloadOutcome::NotRunning
        {
            // We believed nginx was up. Not fatal — the config is on disk and
            // the next start picks it up — but it means the manager's view and
            // reality disagree, which the watchdog should be reconciling.
            log::warn!("nginx: nothing to reload — no master was running");
        }
        Ok(checks)
    }

    /// Whether a php-fpm pool for `minor` (normal or debug) is currently
    /// managed (spawned or adopted). Lets the startup patch-bump task skip
    /// minors with no live pool (nothing to restart — the next start resolves
    /// the new pin anyway).
    pub fn has_php_pool(&self, minor: &str) -> bool {
        self.pools.has(minor, false) || self.pools.has(minor, true)
    }

    /// The patches the live php-fpm masters are executing, or `None` if any is
    /// unidentifiable. See [`php::PhpFpmPools::running_patches`] — the live fact
    /// behind the binary-cache sweep's keep-set.
    pub fn running_php_patches(&self, platform: &dyn Platform) -> Option<Vec<String>> {
        self.pools.running_patches(platform)
    }

    /// Restart a minor's pools IF currently managed — BOTH the normal and the
    /// debug pool, since per-version settings and patch bumps apply to each:
    /// stop (reaping orphaned workers so the port gate passes) and
    /// re-`ensure`/`ensure_debug` — which rewrites the config from the current
    /// settings map AND resolves the currently pinned patch, so this is both
    /// the settings-change and the patch-bump restart. Empty if no pool for
    /// `minor` was running.
    async fn restart_php_pool(
        &mut self,
        platform: &dyn Platform,
        minor: &str,
    ) -> Result<Vec<ReadyCheck>> {
        let mut checks = Vec::new();
        for debug in [false, true] {
            if !self.pools.stop_one(platform, minor, debug) {
                continue;
            }
            let (port, log_name) = if debug {
                self.pools.ensure_debug(platform, minor).await?;
                let port = php::debug_fpm_port(minor)
                    .ok_or_else(|| Error::Other(format!("no debug fpm port for {minor}")))?;
                (port, format!("php-fpm-{minor}-debug.log"))
            } else {
                self.pools.ensure(platform, minor).await?;
                let port = php::fpm_port(minor)
                    .ok_or_else(|| Error::Other(format!("no fpm port for {minor}")))?;
                (port, format!("php-fpm-{minor}.log"))
            };
            checks.push(ReadyCheck {
                service: pool_service_name(minor, debug),
                log: platform.paths().log_dir()?.join(log_name),
                tries: 20,
                probe: Box::new(move || services::fpm_running(port)),
                bin: None,
            });
        }
        Ok(checks)
    }

    /// Set the pools' mail catch-all from the CACHED Mailpit binary path.
    ///
    /// Sync, no download, no running Mailpit — both halves are a path, a fixed
    /// SMTP port and a constant env set, which is the same derivation
    /// `adopt_startup` uses and for the same reason: the caller (a settings
    /// toggle) must not stream a download while holding the services lock.
    pub fn set_mail_catch_from(&mut self, platform: &dyn Platform, enabled: bool) {
        if self.mailpit_bin.is_none() {
            self.mailpit_bin = binaries::cached_bin(platform, "mailpit", binaries::MAILPIT_VERSION);
        }
        self.pools.set_mail_catch(mail::catch_for(self.mailpit_bin.as_deref(), enabled));
    }

    /// Restart every listed minor's live pool (patch bump riding an app release
    /// — Option A). The caller prefetches the new binaries FIRST (download hub,
    /// no lock held) so the stop→ensure gap under the services lock is a cache
    /// hit, not a download. Returns readiness probes to await after unlocking.
    pub async fn restart_pools_for(
        &mut self,
        platform: &dyn Platform,
        minors: &[String],
    ) -> Result<Vec<ReadyCheck>> {
        let mut checks = Vec::new();
        for minor in minors {
            checks.extend(self.restart_php_pool(platform, minor).await?);
        }
        Ok(checks)
    }

    /// Stop the whole stack.
    pub fn stop_all(&mut self, platform: &dyn Platform) -> Result<()> {
        // Manual intervention resets the watchdog's give-up counters.
        self.restart_attempts.clear();
        self.edge_dead_polls = 0;
        self.edge_blocked = false;
        // A tracked unprivileged child is killed by pid. For a root/privileged edge
        // — or a stray Caddy still on the admin port that we never tracked (common
        // after crashes/restarts) — drive Caddy's admin API to stop it and confirm
        // the port frees. This makes "Stop all" reliably release :443/:80.
        let prior_was_daemon = matches!(self.caddy, CaddyHandle::Daemon);
        if let CaddyHandle::Child(mut c) = std::mem::take(&mut self.caddy) {
            let _ = proxy::stop(platform, c.id());
            let _ = c.wait();
        }
        // The KeepAlive daemon is booted out by the stop COMMAND (privileged, OUTSIDE
        // the lock) — an admin `caddy stop` here would just be relaunched, so skip it.
        // Legacy osascript / stray untracked edges still get the admin-stop + reap.
        if !prior_was_daemon {
            if let Some(bins) = &self.bins {
                if let Err(e) = proxy::stop_edge(platform, &bins.caddy) {
                    log::warn!("rexenv: stop_all could not stop the Caddy edge: {e}");
                }
            }
        }
        // Stack guard: a non-app process (live-check example) may stop only what
        // it SPAWNED — adopted survivors are the user's serving stack; skip them
        // (dropping a `Proc::Adopted` handle never signals the process).
        let may_foreign = stack_guard::may_control_real_stack();
        if may_foreign || !self.mailpit.as_ref().is_some_and(Proc::is_adopted) {
            self.stop_mailpit(platform)?;
        } else {
            log::warn!("rexenv: stack guard — leaving adopted Mailpit running");
            self.mailpit = None;
        }
        self.pools.stop_all(platform);
        for (domain, mut backend) in std::mem::take(&mut self.overrides) {
            Self::stop_override_backend(platform, &domain, &mut backend);
        }
        for (engine, mut child) in std::mem::take(&mut self.dbs) {
            if may_foreign || !child.is_adopted() {
                let _ = engine.stop(platform, child.id());
                child.wait();
            } else {
                log::warn!("rexenv: stack guard — leaving adopted {engine:?} running");
            }
        }
        if let Some(mut c) = self.nginx.take() {
            if may_foreign || !c.is_adopted() {
                let _ = services::stop(platform, c.id());
                c.wait();
            } else {
                log::warn!("rexenv: stack guard — leaving adopted nginx running");
            }
        }
        // Orphan sweep: kill any rexenv-owned process STILL on one of our managed
        // ports that the handle-based stops above missed — a survivor of an app
        // restart/crash (its handle didn't survive, but the OS process did). This
        // is what makes "Stop all" reliable regardless of who started the service.
        self.stop_stale_owned(platform);
        Ok(())
    }

    /// Fixed TCP ports of the services we manage — used to stop orphans whose
    /// handles didn't survive an app restart. Uses the standard per-version pool
    /// ports (pools may be untracked after a restart) + all available DB engines +
    /// Mailpit + any tracked per-site override backends. Caddy's :80/:443 are NOT
    /// included: it's root-owned and stopped via its admin API in `stop_all`.
    fn managed_ports(&self) -> Vec<u16> {
        let mut ports = vec![self.ports.nginx];
        // Every pinned PHP minor's pool port (derived from binaries::PHP_VERSIONS, so a
        // future 8.4 pool is swept too — not a hardcoded list that would miss it).
        for minor in php::all_minors() {
            if let Some(p) = php::fpm_port(&minor) {
                ports.push(p);
            }
        }
        for engine in DbEngine::ALL.into_iter().filter(|e| e.available()) {
            ports.push(engine.port());
        }
        ports.push(mail::MAILPIT_SMTP_PORT);
        ports.push(mail::MAILPIT_HTTP_PORT);
        for backend in self.overrides.values() {
            ports.push(backend.port);
        }
        ports
    }

    /// Stop any rexenv-owned process still holding one of our managed ports (an
    /// orphan we no longer track). Guarded to our own processes by the app-data
    /// marker, so an unrelated process on the same port is never touched.
    fn stop_stale_owned(&self, platform: &dyn Platform) {
        // Stack guard: an untracked listener is by definition not THIS process's
        // child — in a live-check example it's the user's serving stack.
        if !stack_guard::may_control_real_stack() {
            log::warn!(
                "rexenv: stack guard — not the rexenv app; skipping the orphan sweep \
                 (set {}=1 to override)",
                stack_guard::ALLOW_ENV
            );
            return;
        }
        let marker = match platform.paths().app_data_dir() {
            Ok(p) => p.display().to_string(),
            Err(_) => return,
        };
        if marker.is_empty() {
            return;
        }
        for port in self.managed_ports() {
            for pid in platform.supervisor().owned_listeners(port, &marker) {
                let _ = platform.supervisor().stop(pid);
            }
        }
        // Orphaned WORKERS: php-fpm and nginx workers rewrite their process
        // title (`php-fpm: pool www` / `nginx: worker process`) — the app-data
        // marker above never matches, so a master killed uncleanly leaks
        // workers that keep the port accepting forever (status reads running,
        // sites hang, and the next start fails its port gate). On OUR fixed
        // ports a listener with the service's title can only be ours — sweep
        // those too.
        for minor in php::all_minors() {
            for port in [php::fpm_port(&minor), php::debug_fpm_port(&minor)]
                .into_iter()
                .flatten()
            {
                for pid in platform.supervisor().owned_listeners(port, "php-fpm") {
                    let _ = platform.supervisor().stop(pid);
                }
            }
        }
        for pid in platform.supervisor().owned_listeners(self.ports.nginx, "nginx") {
            let _ = platform.supervisor().stop(pid);
        }
    }

    /// Adopt ONLY rexenv-owned database engines (the DB slice of
    /// [`Self::adopt_startup`], same ownership gate). Public for live-check
    /// examples/fixtures that need `spawn_db` to recognize an already-running
    /// engine WITHOUT adopting the edge/web tier — a fixture manager that
    /// adopted nginx would report `is_running()` and rebuild the REAL stack's
    /// vhosts from its throwaway database on the serve phase.
    pub fn adopt_dbs(&mut self, platform: &dyn Platform) -> u32 {
        let marker = match platform.paths().app_data_dir() {
            Ok(p) => p.display().to_string(),
            Err(_) => return 0,
        };
        if marker.is_empty() {
            return 0;
        }
        let owned = |port: u16| platform.supervisor().owned_master(port, &marker);
        let mut adopted = 0u32;
        for engine in DbEngine::ALL.into_iter().filter(|e| e.available()) {
            if let std::collections::hash_map::Entry::Vacant(slot) = self.dbs.entry(engine) {
                if let Some(pid) = owned(engine.port()) {
                    slot.insert(Proc::Adopted(pid));
                    adopted += 1;
                }
            }
        }
        adopted
    }

    /// On app launch, ADOPT rexenv-owned services surviving from a prior session
    /// instead of restarting or stopping them: closing the app is NOT a stop —
    /// the stack keeps serving until the user explicitly stops it, and the next
    /// launch picks the survivors back up (accurate status, Stop all works,
    /// Start all skips them). Ownership-gated exactly like the orphan sweep: a
    /// process must hold one of our fixed ports AND reference our app-data dir
    /// on its command line (which matches only nginx/php-fpm MASTERS — workers
    /// have rewritten titles — so stops stay graceful); the root edge, invisible
    /// to unprivileged `lsof`, is adopted iff OUR admin unix socket answers.
    /// Returns how many services were adopted.
    pub fn adopt_startup(&mut self, platform: &dyn Platform, sites: &[Site], catch_mail: bool) -> u32 {
        let marker = match platform.paths().app_data_dir() {
            Ok(p) => p.display().to_string(),
            Err(_) => return 0,
        };
        if marker.is_empty() {
            return 0;
        }
        // Adopt the MASTER of whatever holds the port — workers share the
        // listen socket, so the listener query returns the whole tree, and the
        // old lowest-pid pick broke under worker churn + pid recycling
        // (Apache: a recycled worker got adopted, Stop-all killed that worker,
        // and the surviving master blocked the next start's port gate).
        let owned = |port: u16| platform.supervisor().owned_master(port, &marker);
        let mut adopted = 0u32;

        // Mail routing must survive adoption (QA P0-3): pool restarts (a settings
        // edit, the startup patch bump, a watchdog respawn) rewrite that pool's
        // fpm config from THIS session's catch state — which only the full
        // start_all path used to set. In an adopted session it was still `None`,
        // so the restarted pool silently lost `php_admin_value[sendmail_path]`
        // and that minor's `mail()` bypassed Mailpit. Derive the catch from the
        // CACHED binary path (sync, no download, no Mailpit process needed —
        // both halves are a path, a fixed SMTP port and a constant env set).
        if self.mailpit_bin.is_none() {
            self.mailpit_bin =
                binaries::cached_bin(platform, "mailpit", binaries::MAILPIT_VERSION);
        }
        self.pools.set_mail_catch(mail::catch_for(self.mailpit_bin.as_deref(), catch_mail));

        if self.nginx.is_none() {
            if let Some(pid) = owned(self.ports.nginx) {
                self.nginx = Some(Proc::Adopted(pid));
                adopted += 1;
            }
        }
        for minor in php::all_minors() {
            if let Some(port) = php::fpm_port(&minor) {
                if let Some(pid) = owned(port) {
                    self.pools.adopt(&minor, port, pid, false);
                    adopted += 1;
                }
            }
            // A DEBUG pool surviving from a prior session (its own port; the
            // Xdebug args live in the running master, so plain adoption keeps
            // them).
            if let Some(port) = php::debug_fpm_port(&minor) {
                if let Some(pid) = owned(port) {
                    self.pools.adopt(&minor, port, pid, true);
                    adopted += 1;
                }
            }
        }
        adopted += self.adopt_dbs(platform);
        if self.mailpit.is_none() {
            if let Some(pid) = owned(mail::MAILPIT_SMTP_PORT) {
                self.mailpit = Some(Proc::Adopted(pid));
                adopted += 1;
            }
        }
        for (site, kind) in sites
            .iter()
            .filter_map(|s| OverrideKind::of(s.web_server).map(|k| (s, k)))
        {
            if !self.overrides.contains_key(&site.domain) {
                // Adopt on the RECORDED port (B20 §4) — what the backend spawned on.
                let port = sites::recorded_override_port(site).unwrap_or_else(|| kind.port(&site.domain));
                if let Some(pid) = owned(port) {
                    self.overrides.insert(
                        site.domain.clone(),
                        OverrideBackend { kind, port, child: Proc::Adopted(pid) },
                    );
                    adopted += 1;
                }
            }
        }
        // Root edge: adopted iff our admin unix socket (0600, under our config
        // dir) accepts a connection — the same channel reload/stop already use.
        // If the KeepAlive daemon is installed, adopt as `Daemon` (Stop-all boots it
        // out); otherwise it's a legacy osascript survivor.
        if matches!(self.caddy, CaddyHandle::Stopped) && proxy::admin_alive(platform) {
            self.caddy = if platform.edge().is_installed() {
                CaddyHandle::Daemon
            } else {
                CaddyHandle::Privileged
            };
            adopted += 1;
        }
        // Adopted services imply cached binaries — wire the resolved-once binary
        // fields strictly offline: existence-checked paths, never a download at
        // startup. This pre-populates the cache fields so the FIRST override
        // reconcile that must restart an adopted FrankenPHP/Apache backend finds
        // Some(p) and skips resolve*() entirely, instead of resolving under the
        // services lock (B28). A wrong/stale path is impossible — the member must
        // be on disk; if the cache is somehow absent the field stays None and the
        // on-demand ensure_*() fallback runs exactly as today (no behavior change,
        // just no pre-wiring win).
        if adopted > 0 {
            if let Ok(bin_dir) = platform.paths().bin_dir() {
                // stop_all's edge stop + `reload` need nginx/caddy.
                if self.bins.is_none() {
                    let nginx =
                        bin_dir.join(format!("nginx-{}", binaries::NGINX_VERSION)).join("nginx");
                    let caddy =
                        bin_dir.join(format!("caddy-{}", binaries::CADDY_VERSION)).join("caddy");
                    if nginx.exists() && caddy.exists() {
                        self.bins = Some(Bins { nginx, caddy });
                    }
                }
            }
            // Per-site override backends: an adopted process runs FROM these, so
            // the member is provably on disk → the warm-but-cold re-download path
            // of resolve*() can't apply here.
            if self.frankenphp_bin.is_none() {
                self.frankenphp_bin =
                    binaries::cached_bin(platform, "frankenphp", binaries::FRANKENPHP_VERSION);
            }
            if self.httpd_dir.is_none() {
                self.httpd_dir =
                    binaries::cached_bundle_dir(platform, "httpd", binaries::HTTPD_VERSION, "bin/httpd");
            }
        }
        adopted
    }

    /// Stop rexenv-owned service orphans left by a prior session — the explicit
    /// cleanup path (NO LONGER run at boot; launch ADOPTS survivors instead, see
    /// [`Self::adopt_startup`]). Best-effort and guarded to our own processes;
    /// with no orphans it's a cheap no-op. Runs on a fresh (empty) manager, so it
    /// only ever stops things this session didn't start.
    pub fn reconcile_startup(&self, platform: &dyn Platform) {
        self.stop_stale_owned(platform);
        // Also clear a leftover Caddy edge if its binary is already cached (no
        // download): admin-API stop + best-effort reap. Skipped on a fresh install
        // (nothing to stop before the first Start all downloads Caddy).
        if let Ok(bin_dir) = platform.paths().bin_dir() {
            let caddy = bin_dir
                .join(format!("caddy-{}", binaries::CADDY_VERSION))
                .join("caddy");
            if caddy.exists() {
                let _ = proxy::stop_edge(platform, &caddy);
            }
        }
    }

    /// Per-service status for the Services view + metrics (§6.1): every service the
    /// app supervises — the available DB engines (MySQL, PostgreSQL), one row per
    /// installed PHP minor (`PHP-FPM <version>`, listed even when its pool is
    /// stopped, same as the DB engines), one per per-site FrankenPHP backend
    /// (`FrankenPHP <domain>`), then Nginx and Caddy. `installed_php` comes from
    /// the registry (the manager has no DB access); the command layer enriches
    /// each row with live RAM/CPU from the monitor (by pid).
    /// Bump the consecutive-restart counter for `service` and decide whether an
    /// automatic restart is still allowed. Emits ONE "gave-up" event (on the
    /// first tick past the cap), then stays silent until the counter is reset
    /// by a healthy observation or a manual start/stop.
    fn should_restart(&mut self, service: &str, events: &mut Vec<HealthEvent>) -> bool {
        let n = self.restart_attempts.entry(service.to_string()).or_insert(0);
        *n += 1;
        if *n <= MAX_RESTART_ATTEMPTS {
            return true;
        }
        if *n == MAX_RESTART_ATTEMPTS + 1 {
            events.push(HealthEvent {
                service: service.to_string(),
                action: "gave-up",
                detail: format!(
                    "still dead after {MAX_RESTART_ATTEMPTS} automatic restarts — \
                     check its log, then Stop all / Start all"
                ),
            });
        }
        false
    }

    /// Health watchdog pass: probe every service THIS manager owns and respawn
    /// the dead ones (same spawn paths as `start_core`, so config/ports/logs are
    /// identical), with a consecutive-attempt cap per service. The privileged
    /// Caddy edge is the exception — restarting it needs the admin-password
    /// prompt, which must never appear unprompted — so a dead edge is marked
    /// stopped and reported instead ("edge-down"), making the UI tell the truth.
    ///
    /// Never STARTS anything the user didn't: only owned-but-dead services are
    /// respawned; a stopped stack is a no-op. M4 shape: spawns happen under the
    /// caller's services lock, the returned [`ReadyCheck`]s are awaited after
    /// dropping it.
    pub async fn reconcile_health(
        &mut self,
        platform: &dyn Platform,
        ca: &ssl::LocalCa,
        sites: &[Site],
    ) -> (Vec<HealthEvent>, Vec<ReadyCheck>) {
        let mut events = Vec::new();
        let mut checks = Vec::new();

        // Database engines. Port-closed alone is not death: a child spawned
        // by an in-flight Start-all (readiness awaited outside the lock) may
        // not have bound yet — leave it alone while its master is alive and
        // within the start grace (the watchdog/Start-all race; a slow first
        // boot, e.g. MySQL initializing, must not be respawn-looped into
        // `gave-up`). A dead master is reaped regardless.
        //
        // ADOPTED handles are handled separately (B29): they have no start-grace
        // and their `alive()` is a bare-pid check we must NOT trust (a recycled
        // pid would read alive → never reaped = the trap). The positive-ID probe
        // is `owned_master(port, app-data-marker)` — a listener on our port
        // carrying our marker (ownership AND liveness) — and a single transient
        // miss must not reap, so we require `ADOPTED_MISS_LIMIT` consecutive
        // misses. If the marker can't be resolved we skip the adopted reap this
        // tick (conservative — never reap on a marker we can't compute).
        let marker = platform.paths().app_data_dir().ok().map(|p| p.display().to_string());
        let mut dead_dbs: Vec<DbEngine> = Vec::new();
        for (engine, proc_) in self.dbs.iter_mut() {
            let label = engine.label();
            if proc_.is_adopted() {
                let Some(marker) = marker.as_deref().filter(|m| !m.is_empty()) else {
                    continue; // no marker → don't reap this tick
                };
                let still_ours =
                    platform.supervisor().owned_master(engine.port(), marker).is_some();
                let misses = self.adopted_misses.get(label).copied().unwrap_or(0);
                let (next, reap) = adopted_reap_decision(still_ours, misses, ADOPTED_MISS_LIMIT);
                if still_ours {
                    self.restart_attempts.remove(label);
                    self.adopted_misses.remove(label);
                } else if reap {
                    self.adopted_misses.remove(label);
                    dead_dbs.push(*engine);
                } else {
                    self.adopted_misses.insert(label.to_string(), next);
                }
            } else if engine.running() {
                self.restart_attempts.remove(label);
            } else if !(proc_.alive() && proc_.starting()) {
                dead_dbs.push(*engine);
            }
        }
        for engine in dead_dbs {
            let name = engine.label().to_string();
            if let Some(mut child) = self.dbs.remove(&engine) {
                child.kill();
                child.wait();
            }
            if !self.should_restart(&name, &mut events) {
                continue;
            }
            match self.spawn_db(platform, engine).await {
                Ok(check) => {
                    checks.extend(check);
                    events.push(HealthEvent {
                        service: name,
                        action: "restarted",
                        detail: "process was dead (port closed); respawned".into(),
                    });
                }
                Err(e) => events.push(HealthEvent {
                    service: name,
                    action: "restart-failed",
                    detail: e.to_string(),
                }),
            }
        }

        // php-fpm pools: drop dead masters, then ensure those minors again
        // (a debug pool respawns through ensure_debug so its Xdebug args and
        // load-probe gate apply on every respawn, not just the first start).
        for p in self.pools.status() {
            if p.running {
                self.restart_attempts.remove(&pool_service_name(&p.minor, p.debug));
            }
        }
        for (minor, debug) in self.pools.reap_dead(platform) {
            let name = pool_service_name(&minor, debug);
            if !self.should_restart(&name, &mut events) {
                continue;
            }
            let result = if debug {
                self.pools.ensure_debug(platform, &minor).await
            } else {
                self.pools.ensure(platform, &minor).await
            };
            match result {
                Ok(()) => events.push(HealthEvent {
                    service: name,
                    action: "restarted",
                    detail: "pool master was dead (port closed); respawned".into(),
                }),
                Err(e) => events.push(HealthEvent {
                    service: name,
                    action: "restart-failed",
                    detail: e.to_string(),
                }),
            }
        }

        // Per-site override backends (FrankenPHP / Apache). Respawn ONLY sites
        // still on that override server — a switched/deleted site's dead
        // handle is just dropped. Same start grace as above: port-closed with
        // a live, just-spawned backend is "starting", not dead.
        let dead_overrides: Vec<(String, OverrideKind)> = self
            .overrides
            .iter_mut()
            .filter_map(|(d, b)| {
                // Dead = port closed AND (master gone OR out of start grace).
                let dead = !frankenphp::running(b.port)
                    && (!b.child.alive() || !b.child.starting());
                dead.then(|| (d.clone(), b.kind))
            })
            .collect();
        for (domain, backend) in self.overrides.iter() {
            if !dead_overrides.iter().any(|(d, _)| d == domain) {
                self.restart_attempts
                    .remove(&format!("{} {domain}", backend.kind.label()));
            }
        }
        for (domain, kind) in dead_overrides {
            let name = format!("{} {domain}", kind.label());
            if let Some(mut backend) = self.overrides.remove(&domain) {
                backend.child.kill();
                backend.child.wait();
            }
            let Some(site) = sites
                .iter()
                .find(|s| s.domain == domain && OverrideKind::of(s.web_server) == Some(kind))
            else {
                continue;
            };
            if !self.should_restart(&name, &mut events) {
                continue;
            }
            let env = self.site_env.get(&site.id).cloned().unwrap_or_default();
            let spawned = self
                .spawn_override(
                    platform,
                    kind,
                    &domain,
                    &site.served_root(),
                    sites::recorded_override_port(site).unwrap_or_else(|| kind.port(&domain)),
                    sites::pool_port_for_site(site),
                    sites::rewrite_mode_for(site.multisite),
                    &env,
                )
                .await;
            match spawned {
                Ok(check) => {
                    checks.push(check);
                    events.push(HealthEvent {
                        service: name,
                        action: "restarted",
                        detail: "backend was dead (port closed); respawned".into(),
                    });
                }
                Err(e) => events.push(HealthEvent {
                    service: name,
                    action: "restart-failed",
                    detail: e.to_string(),
                }),
            }
        }

        // Mailpit. Same start grace: alive + just spawned + port closed =
        // still starting, not dead.
        if self.mailpit.is_some() {
            let starting = self
                .mailpit
                .as_mut()
                .is_some_and(|p| p.alive() && p.starting());
            if mail::running() {
                self.restart_attempts.remove("Mailpit");
            } else if !starting {
                if let Some(mut child) = self.mailpit.take() {
                    child.kill();
                    child.wait();
                }
                if self.should_restart("Mailpit", &mut events) {
                    match self.spawn_mailpit(platform).await {
                        Ok(check) => {
                            checks.extend(check);
                            events.push(HealthEvent {
                                service: "Mailpit".into(),
                                action: "restarted",
                                detail: "process was dead (port closed); respawned".into(),
                            });
                        }
                        Err(e) => events.push(HealthEvent {
                            service: "Mailpit".into(),
                            action: "restart-failed",
                            detail: e.to_string(),
                        }),
                    }
                }
            }
        }

        // Shared Nginx: regenerate configs from the current site set (identical
        // to a reload) and respawn. Port probe AND master-alive — like php-fpm,
        // a SIGKILLed nginx master leaves `nginx: worker process` children
        // holding the port (title has no app-data marker), reading as healthy
        // while frozen; on OUR fixed port an `nginx`-titled listener is ours.
        if self.nginx.is_some() {
            let master_alive = self.nginx.as_mut().is_some_and(Proc::alive);
            // Alive + just spawned + port closed = still starting (start
            // grace) — never reaped mid-start.
            let starting =
                master_alive && self.nginx.as_ref().is_some_and(Proc::starting);
            if services::nginx_running(self.ports.nginx) && master_alive {
                self.restart_attempts.remove("Nginx");
            } else if !starting {
                if let Some(mut child) = self.nginx.take() {
                    child.kill();
                    child.wait();
                }
                for pid in platform.supervisor().owned_listeners(self.ports.nginx, "nginx") {
                    let _ = platform.supervisor().stop(pid);
                }
                if self.should_restart("Nginx", &mut events) {
                    let mut respawn = || -> Result<()> {
                        let nginx_bin = self.bins()?.nginx.clone();
                        let cfg = sites::rebuild_configs_for(
                            sites,
                            platform,
                            ca,
                            self.ports.nginx,
                            self.ports.http,
                            self.ports.https,
                            &php::nginx_body_limits(&self.php_settings),
                            &self.site_env,
                            &self.site_aliases,
                        )?;
                        ports::ensure_free(platform, self.ports.nginx, ports::Proto::Tcp, "Nginx")?;
                        self.nginx = Some(
                            services::start_nginx(platform, &nginx_bin, &cfg.nginx_conf, &cfg.nginx_prefix)?
                                .into(),
                        );
                        Ok(())
                    };
                    match respawn() {
                        Ok(()) => events.push(HealthEvent {
                            service: "Nginx".into(),
                            action: "restarted",
                            detail: "process was dead (port closed); respawned".into(),
                        }),
                        Err(e) => events.push(HealthEvent {
                            service: "Nginx".into(),
                            action: "restart-failed",
                            detail: e.to_string(),
                        }),
                    }
                }
            }
        }

        // Caddy edge: detect via our admin unix socket; NEVER auto-restart (the
        // privileged start blocks on an admin-password prompt). Mark stopped so
        // status/UI tell the truth; the user's next Start all brings it back.
        // The reverse transition heals too: an edge answering on OUR socket while
        // this manager says stopped (transient probe failure, adoption missed at
        // launch) is re-adopted — no start, just truth — so a stale edge-down
        // never leaves the UI lying or invites a Start-all that would kill a
        // healthy edge.
        let alive = proxy::admin_alive(platform);
        let installed = platform.edge().is_installed();
        let is_daemon = matches!(self.caddy, CaddyHandle::Daemon);
        let is_stopped = matches!(self.caddy, CaddyHandle::Stopped);
        if alive {
            self.edge_dead_polls = 0;
        }
        if is_daemon && !alive {
            // The edge is under launchd KeepAlive, which normally relaunches it
            // within its ~10s throttle — so briefly this is a restart in progress,
            // not a "down until Start all" state. Say so ONCE (no 10s toast spam).
            // But a daemon that was booted out / disabled / uninstalled outside the
            // app, or that can't rebind :443, is NOT coming back: after the grace
            // window stop reassuring — diagnose why, flip the handle to Stopped
            // (truth), and raise a real edge-down. Seen live: an external bootout
            // left the old unbounded branch claiming "restarting" every 10s forever.
            self.edge_dead_polls += 1;
            if self.edge_dead_polls == 1 {
                events.push(HealthEvent {
                    service: "Caddy".into(),
                    action: "edge-restarting",
                    detail: "edge went down — its system supervisor should relaunch it \
                             within seconds (sites may blink)"
                        .into(),
                });
            } else if self.edge_dead_polls >= EDGE_SUPERVISOR_GRACE_POLLS {
                self.edge_dead_polls = 0;
                self.caddy = CaddyHandle::Stopped;
                let why = if !installed {
                    "its KeepAlive daemon is no longer installed (removed outside the app)"
                        .to_string()
                } else if !platform.edge().is_enabled() {
                    "its KeepAlive daemon is disabled — it was explicitly stopped \
                     outside the app"
                        .to_string()
                } else if let Some(holder) = platform
                    .supervisor()
                    .port_conflict_help(self.ports.https, false)
                    .holder
                {
                    // The launchd relaunches keep losing the bind race: another
                    // local proxy (e.g. Herd) holds :443. Name it — Herd users are
                    // the target audience and a nameless failure reads as OUR bug.
                    format!(
                        "port {} is held by {holder} — quit that app (or stop its \
                         proxy), then Start all",
                        self.ports.https
                    )
                } else {
                    "its KeepAlive daemon is not bringing it back — port 443 may be \
                     blocked, or it is crash-looping (check logs/caddy-start.log)"
                        .to_string()
                };
                events.push(HealthEvent {
                    service: "Caddy".into(),
                    action: "edge-down",
                    detail: format!(
                        "edge stopped and {why}; every site is unreachable until it is \
                         started again (Start all)"
                    ),
                });
            }
            // Polls between first and grace: silent — already announced, still waiting.
        } else if !is_stopped && !alive {
            // A legacy osascript / child edge died — no OS supervisor to restart it,
            // so mark it down and alarm (the pre-daemon behavior).
            if let CaddyHandle::Child(mut c) = std::mem::take(&mut self.caddy) {
                let _ = c.kill();
                let _ = c.wait();
            }
            self.caddy = CaddyHandle::Stopped;
            events.push(HealthEvent {
                service: "Caddy".into(),
                action: "edge-down",
                detail: "edge stopped answering on its admin socket — every site is \
                         unreachable until it is started again (Start all)"
                    .into(),
            });
        } else if is_stopped && alive {
            // Edge answering while we thought it stopped: re-adopt (as the KeepAlive
            // daemon if installed, else a legacy osascript survivor) — truth, no start.
            self.caddy = if installed {
                CaddyHandle::Daemon
            } else {
                CaddyHandle::Privileged
            };
            events.push(HealthEvent {
                service: "Caddy".into(),
                action: "adopted",
                detail: "edge is answering on its admin socket again — re-adopted \
                         (sites were being served the whole time)"
                    .into(),
            });
        } else if !is_stopped && alive {
            // Our edge PROCESS is healthy — now verify the WIRE is ours. A foreign
            // proxy binding 127.0.0.1:443 specifically (Herd) shadows our wildcard
            // listener with no bind error anywhere: process checks all pass while
            // every site is answered by someone else. Probe through loopback :443
            // itself (positive identity via the config's marker header) and flip
            // `edge_blocked`, which `status()` folds into Caddy's running state.
            // Events fire on TRANSITIONS only — no per-poll spam.
            let wire = proxy::edge_wire(adminer::ADMINER_HOST, self.ports.https).await;
            let ours = wire == proxy::EdgeWire::Ours;
            if !ours && !self.edge_blocked {
                self.edge_blocked = true;
                // NOTHING listening while our edge process is ALIVE is a
                // different fault from a foreign proxy: the master is up and is
                // not serving. Saying "another local proxy answers port 443" —
                // which the old message did whenever the holder lookup found
                // nobody — describes a program that is not there and hides the
                // one that is.
                let detail = if wire == proxy::EdgeWire::NoAnswer {
                    format!(
                        "the edge process is running but nothing is answering port {} — no \
                         other app is holding it, so this is rexenv's own edge failing to \
                         serve. Its log has the reason; Stop all then Start all rebuilds it.",
                        self.ports.https
                    )
                } else {
                    let help = platform.supervisor().port_conflict_help(self.ports.https, false);
                    let holder = help.holder.unwrap_or_else(|| "another local proxy".into());
                    // Name the APP to quit when identifiable ("quit Herd"), never a
                    // bare process title the user can't act on. The trailing "\n$ <cmd>"
                    // becomes a copyable command block in the toast (supervisor-aware:
                    // quits the managing app, never kills a respawning worker).
                    let quit = help.app.unwrap_or_else(|| "that app".into());
                    let fix = help.free_command.map(|c| format!("\n$ {c}")).unwrap_or_default();
                    format!(
                        "the edge is running, but {holder} answers port {} in front \
                         of it — no site will load until you quit {quit}{fix}",
                        self.ports.https
                    )
                };
                events.push(HealthEvent {
                    service: "Caddy".into(),
                    action: "edge-blocked",
                    detail,
                });
            } else if ours && self.edge_blocked {
                self.edge_blocked = false;
                events.push(HealthEvent {
                    service: "Caddy".into(),
                    action: "edge-unblocked",
                    detail: format!(
                        "port {} is answered by rexenv again — sites are reachable",
                        self.ports.https
                    ),
                });
            }
        }

        (events, checks)
    }

    /// Domain → pid of every live per-site FrankenPHP override backend — the
    /// only sites with a process of their own, so the only ones that get REAL
    /// per-site CPU/RAM on the Sites page (shared-pool sites get activity
    /// metrics instead — see `core::site_metrics`).
    pub fn override_pids(&self) -> Vec<(String, u32)> {
        self.overrides.iter().map(|(d, b)| (d.clone(), b.child.id())).collect()
    }

    pub fn status(&self, platform: &dyn Platform, installed_php: &[String]) -> Vec<ServiceInfo> {
        let mut infos = Vec::new();

        // Database engines (available ones) — always listed, running-state per port.
        for engine in DbEngine::ALL.into_iter().filter(|e| e.available()) {
            infos.push(ServiceInfo {
                name: engine.label().to_string(),
                // Owned + alive, not a bare port-listen (task 2.2 / H2).
                running: self.dbs.contains_key(&engine) && engine.running(),
                pid: self.dbs.get(&engine).map(Proc::id),
                port: engine.port(),
                optional: !engine.required(),
            });
        }

        // One row per installed PHP minor, so the list doesn't grow/shrink with
        // Start/Stop all. A running pool for an uninstalled minor is still shown.
        let pools = self.pools.status();
        let mut minors: Vec<&str> = installed_php.iter().map(String::as_str).collect();
        minors.extend(pools.iter().map(|p| p.minor.as_str()));
        minors.sort_unstable();
        minors.dedup();
        for minor in minors {
            let pool = pools.iter().find(|p| p.minor == minor && !p.debug);
            infos.push(ServiceInfo {
                name: pool_service_name(minor, false),
                running: pool.is_some_and(|p| p.running),
                pid: pool.map(|p| p.pid),
                port: pool.map(|p| p.port).or_else(|| php::fpm_port(minor)).unwrap_or(0),
                optional: false,
            });
            // Debug pools appear only while managed (they exist on demand —
            // a permanent row would imply Xdebug is a start-all service).
            if let Some(p) = pools.iter().find(|p| p.minor == minor && p.debug) {
                infos.push(ServiceInfo {
                    name: pool_service_name(minor, true),
                    running: p.running,
                    pid: Some(p.pid),
                    port: p.port,
                    optional: true,
                });
            }
        }

        // One row per per-site override backend (sorted for stable display).
        let mut overrides: Vec<(&String, &OverrideBackend)> = self.overrides.iter().collect();
        overrides.sort_by(|a, b| a.0.cmp(b.0));
        for (domain, backend) in overrides {
            infos.push(ServiceInfo {
                name: format!("{} {domain}", backend.kind.label()),
                running: frankenphp::running(backend.port),
                pid: Some(backend.child.id()),
                port: backend.port,
                optional: false,
            });
        }

        infos.push(ServiceInfo {
            name: "Nginx".to_string(),
            // Owned (we hold the child) + alive, not a bare port-listen (task 2.2 / H2).
            running: self.nginx.is_some() && services::nginx_running(self.ports.nginx),
            pid: self.nginx.as_ref().map(Proc::id),
            port: self.ports.nginx,
            optional: false,
        });
        infos.push(ServiceInfo {
            name: "Caddy".to_string(),
            // Ownership + liveness: true only when WE started the edge (handle !=
            // Stopped, set only after a confirmed start, cleared on stop) AND its
            // private admin unix socket still answers. A foreign listener on :443
            // (e.g. another local server) must NOT read as rexenv's edge being up
            // (task 2.2 / H2) — the socket path is ours, so the probe stays
            // ownership-scoped. Without the probe, a crashed root edge kept showing
            // "running" forever while every site was unreachable.
            // Process liveness AND wire ownership: a foreign 127.0.0.1:443
            // bind (Herd) shadows our wildcard listener while our process runs
            // happily — a green row then lies (H2 applied to the wire).
            running: !matches!(self.caddy, CaddyHandle::Stopped)
                && proxy::admin_alive(platform)
                && !self.edge_blocked,
            pid: match &self.caddy {
                CaddyHandle::Child(c) => Some(c.id()),
                _ => None,
            },
            port: self.ports.https,
            optional: false,
        });
        infos.push(ServiceInfo {
            name: "Mailpit".to_string(),
            running: self.mailpit.is_some() && mail::running(),
            pid: self.mailpit.as_ref().map(Proc::id),
            port: mail::MAILPIT_HTTP_PORT,
            optional: false,
        });
        infos
    }
}

impl Drop for ServiceManager {
    fn drop(&mut self) {
        // Best-effort: stop our child processes so nothing is orphaned if
        // stop_all wasn't called. SIGTERM-first (Proc::terminate), NOT a bare
        // SIGKILL: a SIGKILLed nginx master leaks `nginx: worker process`
        // children that keep :18088 accepting forever (same for php-fpm — its
        // pools clean themselves up via PhpFpmPools::drop). A privileged-root
        // Caddy can't be signalled from here.
        for child in self.dbs.values_mut() {
            child.terminate();
        }
        for backend in self.overrides.values_mut() {
            backend.child.terminate();
        }
        if let Some(c) = &mut self.mailpit {
            c.terminate();
        }
        if let Some(c) = &mut self.nginx {
            c.terminate();
        }
        if let CaddyHandle::Child(c) = &mut self.caddy {
            let _ = c.kill();
        }
    }
}

/// Poll `cond` up to `tries` times (500ms apart). Returns whether it became true.
/// Async so the waits use `tokio::time::sleep` (which yields the worker) rather than
/// `std::thread::sleep` — the latter parks a tokio worker for up to `tries`×500ms
/// while the AppState lock is held, starving unrelated tasks (M4). The `cond` probes
/// are quick TCP checks and stay synchronous.
/// A deferred readiness probe for a just-spawned service: everything
/// [`await_ready`] needs to wait for it and to produce the M3 "named service +
/// its log" timeout error. Produced by the spawn phase (run under the services
/// lock), awaited by the caller AFTER dropping the lock — so a slow-starting
/// service never parks the whole service manager (M4). Probes are pure port /
/// socket checks and hold no reference to the manager.
pub struct ReadyCheck {
    service: String,
    log: PathBuf,
    tries: u32,
    probe: Box<dyn Fn() -> bool + Send + Sync>,
    /// The binary this service runs, when the caller has it cheaply to hand.
    ///
    /// Used for ONE thing, on the failure path only: if the service never came
    /// up and its binary declares a macOS newer than this machine's, say so.
    /// `None` everywhere that would have to resolve or download something to
    /// answer — an unexplained timeout is bad, but not bad enough to fetch an
    /// artifact in order to explain it.
    bin: Option<PathBuf>,
}

/// Append health-watchdog events to `<log_dir>/health.log` (timestamped) — the
/// durable evidence trail for "why were my sites down at 3pm". Best-effort:
/// logging must never take the watchdog down.
pub fn log_health_events(platform: &dyn Platform, events: &[HealthEvent]) {
    if events.is_empty() {
        return;
    }
    let Ok(dir) = platform.paths().log_dir() else { return };
    let _ = std::fs::create_dir_all(&dir);
    let ts = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("health.log"))
    {
        use std::io::Write;
        for e in events {
            let _ = writeln!(f, "{ts} [{}] {}: {}", e.action, e.service, e.detail);
        }
    }
}

/// Await a batch of [`ReadyCheck`]s CONCURRENTLY (worst case = the slowest
/// single probe, not the sum) with no lock held. All probes run to completion
/// so every failed service is named, then failures are joined into one error.
/// An empty batch is `Ok` immediately.
pub async fn await_ready(checks: Vec<ReadyCheck>) -> Result<()> {
    if checks.is_empty() {
        return Ok(());
    }
    let mut set = tokio::task::JoinSet::new();
    for check in checks {
        set.spawn(async move {
            let ReadyCheck { service, log, tries, probe, bin } = check;
            wait_until_ready(&service, &log, tries, probe, bin.as_deref()).await
        });
    }
    let mut failures = Vec::new();
    while let Some(res) = set.join_next().await {
        match res {
            Ok(Ok(())) => {}
            Ok(Err(e)) => failures.push(e.to_string()),
            Err(e) => failures.push(format!("readiness task failed: {e}")),
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(Error::Other(failures.join("; ")))
    }
}

async fn wait_until(mut cond: impl FnMut() -> bool, tries: u32) -> bool {
    for _ in 0..tries {
        if cond() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    cond()
}

/// Like [`wait_until`] but treats a timeout as a hard error naming the `service` and
/// its `log` — so a service that never comes up fails HERE with a clear, actionable
/// message instead of silently returning `Ok` and surfacing confusingly later (M3).
async fn wait_until_ready(
    service: &str,
    log: &Path,
    tries: u32,
    cond: impl FnMut() -> bool,
    bin: Option<&Path>,
) -> Result<()> {
    if wait_until(cond, tries).await {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "{service} did not start within {}s{} — see {}",
            tries / 2, // 500ms per try
            bin.and_then(macos_floor_note).unwrap_or_default(),
            log.display()
        )))
    }
}

/// The macOS-version half of a start failure, when there is one.
///
/// **Diagnosis, never a gate.** rexenv must not refuse to spawn on `minos`:
/// measured 15 Aug 2026, dyld on macOS 26 enforces it for neither executables
/// nor dylibs, so refusing would block builds that may run perfectly on the
/// strength of a prediction this project cannot test from a current machine.
/// Enforcement is a property of the OLDER host's dyld, and only a macOS 14/15 VM
/// can settle it (`docs/TODO.md`, `docs/PORTS.md`).
///
/// This runs only after the service has ALREADY failed, so it costs nothing when
/// the prediction is wrong: the sentence simply never appears. What it buys when
/// the prediction is right is the difference between "PostgreSQL did not start
/// within 15s — see postgres-stdout.log", which sends the reader at Postgres,
/// and a line naming the one fact that explains it. PostgreSQL's pinned builds
/// declare macOS 26 while rexenv's own floor is 15, so this is not an edge: it
/// is every supported user below 26.
fn macos_floor_note(bin: &Path) -> Option<String> {
    let need = macho::min_macos(bin)?;
    let host = macho::host_macos()?;
    if !macho::newer_than(need, host) {
        return None;
    }
    Some(format!(
        " — this build needs macOS {}.{}, and this Mac runs {}.{}, which can stop it \
         loading at all",
        need.0, need.1, host.0, host.1
    ))
}

/// The `spawn_logged` stdout log path for a service `key` (`<key>-stdout.log` under
/// `log_dir`, e.g. `mysql`, `mailpit`, `frankenphp-my.rex`). Matches `core::logs`.
fn stdout_log(platform: &dyn Platform, key: &str) -> Result<PathBuf> {
    Ok(platform.paths().log_dir()?.join(format!("{key}-stdout.log")))
}

/// Display/keying name for a minor's pool: `PHP-FPM 8.4` / `PHP-FPM 8.4 (Xdebug)`.
/// Shared by status rows, watchdog events, and restart-attempt counters so a
/// debug pool never aliases its minor's normal pool.
fn pool_service_name(minor: &str, debug: bool) -> String {
    if debug {
        format!("PHP-FPM {minor} (Xdebug)")
    } else {
        format!("PHP-FPM {minor}")
    }
}

/// Per-site serving status (H1 follow-up), derived from a live [`ServiceInfo`]
/// snapshot (the single non-blocking source shared with `services_status`). A site
/// is *serving* only when the edge is up AND its own upstream is up — so a partial
/// stack (e.g. one FrankenPHP backend down while nginx serves) no longer reports
/// every site as running.
///
/// Upstream, per server:
/// - **FrankenPHP override** — its per-site backend port must be up (nginx is bypassed).
/// - **nginx (default)** — the shared nginx AND the php-fpm pool the site actually
///   routes to (via [`sites::pool_port_for_site`], mirroring `nginx_site_for` —
///   an Xdebug-toggled site is checked against its DEBUG pool).
///
/// The edge and nginx are the fixed singletons, matched by their stable names; the
/// per-site upstreams are matched by port (the same ports the config generator emits,
/// so the check can't drift from what's actually wired). "Present in the generated
/// config" is approximated by the site being persisted — configs are regenerated from
/// the site list on every change.
pub fn site_serving(sites: &[Site], infos: &[ServiceInfo]) -> Vec<SiteServing> {
    let name_up = |name: &str| infos.iter().any(|i| i.name == name && i.running);
    let port_up = |port: u16| infos.iter().any(|i| i.port == port && i.running);
    let edge_up = name_up("Caddy");
    let nginx_up = name_up("Nginx");
    sites
        .iter()
        .map(|s| {
            // Read the RECORDED override port (B20 §4), never re-derive.
            let upstream_up = match s.web_server {
                WebServer::Frankenphp => {
                    sites::recorded_override_port(s).is_some_and(port_up)
                }
                // Apache serves through the shared pool too — both must be up.
                WebServer::Apache => {
                    sites::recorded_override_port(s).is_some_and(port_up)
                        && port_up(sites::pool_port_for_site(s))
                }
                _ => nginx_up && port_up(sites::pool_port_for_site(s)),
            };
            SiteServing {
                domain: s.domain.clone(),
                serving: edge_up && upstream_up,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::traits::*;

    /// #80 — **startup adoption is strictly OFFLINE.** `adopt_startup` runs on
    /// every launch, before the window is usable, and its job is to recognise
    /// services a previous session left running. A download in that path would
    /// put the network between the user and their own already-running stack:
    /// a slow or offline machine would sit there while a binary it does not
    /// need is fetched, and a launch would fail for want of connectivity that
    /// adoption never required.
    ///
    /// **The load-bearing half is the SIGNATURE**, which is why it is asserted
    /// first: every download in this tree is `async` (`binaries::resolve*`,
    /// `http_get`, `downloads::prefetch`), and a synchronous fn cannot await
    /// one. So the guard is: stay synchronous, never reach for a runtime to
    /// get round that, and call none of the resolving/fetching entry points —
    /// only the `cached_*` readers, which are existence checks on disk.
    #[test]
    fn startup_adoption_never_reaches_the_network() {
        let src = crate::core::copy_scan::production_source(include_str!("service_manager.rs"));
        assert!(
            src.contains("pub fn adopt_startup(&mut self, platform: &dyn Platform"),
            "`adopt_startup` is no longer a plain synchronous fn. Its sync signature is what \
             makes a download structurally impossible — an `async` one can await `resolve()` \
             and a launch then waits on the network to recognise a process it can already see"
        );
        let body = src
            .split("pub fn adopt_startup(&mut self, platform: &dyn Platform")
            .nth(1)
            .and_then(|b| b.split("\n    /// ").next())
            .expect("adopt_startup body");
        // `cached_bin` / `cached_path` / `is_cached` are the allowed shape: a
        // path that must already be on disk, or None.
        for fetch in [
            "binaries::resolve",
            "binaries::http_get",
            "downloads::prefetch",
            "block_on",
            "Handle::current",
            ".await",
        ] {
            assert!(
                !body.contains(fetch),
                "`adopt_startup` contains `{fetch}` — adoption must not touch the network, and \
                 must not reach for a runtime to get round its own synchronous signature. \
                 Existence checks (`cached_bin`, `cached_path`) are the allowed shape"
            );
        }
        assert!(
            body.contains("binaries::cached_bin("),
            "the offline binary wiring is gone from adoption — if it moved, move this guard \
             with it; a check that matches nothing passes for the wrong reason"
        );
    }

    /// LOGIN-SAFETY guard 2: an unattended start may adopt or start an
    /// unprivileged edge, but a plan that would prompt is always skipped.
    #[test]
    fn login_edge_action_never_runs_a_privileged_plan() {
        assert!(matches!(login_edge_action(&None), LoginEdgeAction::AlreadyServing));
        let privileged =
            EdgePlan { privileged: true, caddy_bin: "/x".into(), caddyfile: "/y".into() };
        assert!(matches!(
            login_edge_action(&Some(privileged)),
            LoginEdgeAction::SkipNeedsPrompt
        ));
        let high_port =
            EdgePlan { privileged: false, caddy_bin: "/x".into(), caddyfile: "/y".into() };
        assert!(matches!(
            login_edge_action(&Some(high_port)),
            LoginEdgeAction::StartUnprivileged
        ));
    }

    /// Minimal platform for edge state-machine tests: real paths (a tempdir, so the
    /// admin socket never exists → `admin_alive()` = false) + a configurable edge
    /// supervisor. Everything else panics — these tests must never touch it.
    struct EdgeTestPlatform {
        paths: TestPaths,
        edge: TestEdge,
    }
    struct TestPaths(PathBuf);
    impl Paths for TestPaths {
        fn app_data_dir(&self) -> Result<PathBuf> {
            Ok(self.0.clone())
        }
        fn config_dir(&self) -> Result<PathBuf> {
            Ok(self.0.join("config"))
        }
        fn log_dir(&self) -> Result<PathBuf> {
            Ok(self.0.join("logs"))
        }
        fn bin_dir(&self) -> Result<PathBuf> {
            Ok(self.0.join("bin"))
        }
        fn hosts_file(&self) -> PathBuf {
            self.0.join("hosts")
        }
    }
    struct TestEdge {
        installed: bool,
        enabled: bool,
    }
    impl EdgeSupervisor for TestEdge {
        fn is_installed(&self) -> bool {
            self.installed
        }
        fn is_enabled(&self) -> bool {
            self.enabled
        }
        fn plist_path(&self) -> PathBuf {
            unimplemented!()
        }
        fn wrapper_path(&self) -> PathBuf {
            unimplemented!()
        }
        fn daemon_binary_path(&self) -> PathBuf {
            unimplemented!()
        }
        fn plist_contents(&self, _: &Path, _: &Path) -> String {
            unimplemented!()
        }
        fn wrapper_contents(&self, _: &Path, _: &Path, _: &Path, _: &Path) -> String {
            unimplemented!()
        }
        fn install_command(&self, _: &Path, _: &Path, _: &Path) -> String {
            unimplemented!()
        }
        fn start_command(&self) -> String {
            unimplemented!()
        }
        fn stop_command(&self) -> String {
            unimplemented!()
        }
        fn uninstall_command(&self) -> String {
            unimplemented!()
        }
    }
    impl Platform for EdgeTestPlatform {
        fn paths(&self) -> &dyn Paths {
            &self.paths
        }
        fn edge(&self) -> &dyn EdgeSupervisor {
            &self.edge
        }
        fn dns(&self) -> &dyn DnsManager {
            unimplemented!()
        }
        fn cert_trust(&self) -> &dyn CertTrustManager {
            unimplemented!()
        }
        fn privileges(&self) -> &dyn PrivilegeManager {
            unimplemented!()
        }
        fn supervisor(&self) -> &dyn ProcessSupervisor {
            unimplemented!()
        }
        fn autostart(&self) -> &dyn AutostartManager {
            unimplemented!()
        }
        fn permissions(&self) -> &dyn PermissionManager {
            unimplemented!()
        }
        fn shell(&self) -> &dyn ShellRunner {
            unimplemented!()
        }
        fn binaries(&self) -> &dyn BinaryProvider {
            unimplemented!()
        }
        fn dns_agent(&self) -> &dyn DnsAgentManager {
            unimplemented!()
        }
    }

    /// Recording supervisor for override-stop tests: configurable
    /// `owned_master`, every `stop` call recorded.
    struct TestSupervisor {
        master: Option<u32>,
        stopped: std::sync::Mutex<Vec<u32>>,
    }
    impl ProcessSupervisor for TestSupervisor {
        fn spawn(&self, _: &Path, _: &[String]) -> Result<std::process::Child> {
            unimplemented!()
        }
        fn spawn_logged(&self, _: &Path, _: &[String], _: &Path) -> Result<std::process::Child> {
            unimplemented!()
        }
        fn stop(&self, pid: u32) -> Result<()> {
            self.stopped.lock().unwrap().push(pid);
            Ok(())
        }
        fn owned_master(&self, _port: u16, _marker: &str) -> Option<u32> {
            self.master
        }
    }
    struct OverrideTestPlatform {
        paths: TestPaths,
        sup: TestSupervisor,
    }
    impl Platform for OverrideTestPlatform {
        fn paths(&self) -> &dyn Paths {
            &self.paths
        }
        fn supervisor(&self) -> &dyn ProcessSupervisor {
            &self.sup
        }
        fn dns(&self) -> &dyn DnsManager {
            unimplemented!()
        }
        fn cert_trust(&self) -> &dyn CertTrustManager {
            unimplemented!()
        }
        fn privileges(&self) -> &dyn PrivilegeManager {
            unimplemented!()
        }
        fn autostart(&self) -> &dyn AutostartManager {
            unimplemented!()
        }
        fn permissions(&self) -> &dyn PermissionManager {
            unimplemented!()
        }
        fn shell(&self) -> &dyn ShellRunner {
            unimplemented!()
        }
        fn binaries(&self) -> &dyn BinaryProvider {
            unimplemented!()
        }
        fn edge(&self) -> &dyn EdgeSupervisor {
            unimplemented!()
        }
        fn dns_agent(&self) -> &dyn DnsAgentManager {
            unimplemented!()
        }
    }
    fn override_test_platform(name: &str, master: Option<u32>) -> OverrideTestPlatform {
        let dir = std::env::temp_dir().join(format!("rexenv-ovr-{name}"));
        let _ = std::fs::create_dir_all(&dir);
        OverrideTestPlatform {
            paths: TestPaths(dir),
            sup: TestSupervisor { master, stopped: std::sync::Mutex::new(Vec::new()) },
        }
    }
    /// A port that is actually free right now (bind :0, take the number).
    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
    }

    // --- B29: adopted-service watchdog test rig -----------------------------

    /// Supervisor whose positive-ID probe (`owned_master` via `owned_listeners`)
    /// is switched per tick through a shared flag — `true` = a marked listener
    /// holds our port (still ours), `false` = a miss. `spawn` must never be
    /// reached (the reap's `should_restart` gate is preset off in the test).
    struct ProbeSupervisor {
        ours: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }
    impl ProcessSupervisor for ProbeSupervisor {
        fn spawn(&self, _: &Path, _: &[String]) -> Result<std::process::Child> {
            panic!("spawn must not be reached — should_restart gates it in this test")
        }
        fn spawn_logged(&self, _: &Path, _: &[String], _: &Path) -> Result<std::process::Child> {
            panic!("spawn_logged must not be reached in this test")
        }
        fn stop(&self, _pid: u32) -> Result<()> {
            Ok(())
        }
        fn owned_listeners(&self, _port: u16, _marker: &str) -> Vec<u32> {
            if self.ours.load(std::sync::atomic::Ordering::SeqCst) {
                vec![9999] // a marked listener on our port → owned_master = Some
            } else {
                vec![] // no marked listener → owned_master = None (a miss)
            }
        }
    }
    struct AdoptedTestPlatform {
        paths: TestPaths,
        sup: ProbeSupervisor,
        edge: TestEdge,
    }
    impl Platform for AdoptedTestPlatform {
        fn paths(&self) -> &dyn Paths {
            &self.paths
        }
        fn supervisor(&self) -> &dyn ProcessSupervisor {
            &self.sup
        }
        fn edge(&self) -> &dyn EdgeSupervisor {
            &self.edge
        }
        fn dns(&self) -> &dyn DnsManager {
            unimplemented!()
        }
        fn cert_trust(&self) -> &dyn CertTrustManager {
            unimplemented!()
        }
        fn privileges(&self) -> &dyn PrivilegeManager {
            unimplemented!()
        }
        fn autostart(&self) -> &dyn AutostartManager {
            unimplemented!()
        }
        fn permissions(&self) -> &dyn PermissionManager {
            unimplemented!()
        }
        fn shell(&self) -> &dyn ShellRunner {
            unimplemented!()
        }
        fn binaries(&self) -> &dyn BinaryProvider {
            unimplemented!()
        }
        fn dns_agent(&self) -> &dyn DnsAgentManager {
            unimplemented!()
        }
    }
    fn adopted_test_platform(
        name: &str,
        ours: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> AdoptedTestPlatform {
        let dir = std::env::temp_dir().join(format!("rexenv-b29-{name}"));
        let _ = std::fs::create_dir_all(&dir);
        AdoptedTestPlatform {
            paths: TestPaths(dir),
            sup: ProbeSupervisor { ours },
            edge: TestEdge { installed: false, enabled: false },
        }
    }

    #[test]
    fn adopted_reap_decision_resets_on_ours_and_reaps_at_the_limit() {
        // still_ours → reset to 0, never reap (a live-and-ours service can never
        // accumulate misses — this is the flap fix).
        assert_eq!(adopted_reap_decision(true, 5, ADOPTED_MISS_LIMIT), (0, false));
        // A single miss → wait, not reaped.
        assert_eq!(adopted_reap_decision(false, 0, 2), (1, false));
        // A second CONSECUTIVE miss → reap.
        assert_eq!(adopted_reap_decision(false, 1, 2), (2, true));
    }

    #[tokio::test]
    async fn watchdog_reaps_an_adopted_db_only_after_consecutive_positive_id_misses() {
        use std::sync::atomic::Ordering::SeqCst;
        let ours = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let platform = adopted_test_platform("reap", ours.clone());
        let dir = std::env::temp_dir().join("rexenv-b29-ca");
        let ca = ssl::load_or_create_at(&dir.join("ca.pem"), &dir.join("ca.key"), None).unwrap();
        let mut mgr = ServiceManager::with_ports(Ports::default());

        let engine = DbEngine::Mysql;
        let label = engine.label().to_string();
        mgr.insert_adopted_db_for_test(engine, 9999);
        let engine_events =
            |evts: &[HealthEvent]| evts.iter().filter(|e| e.service == label).count();

        // Tick 1 — positive-ID present (a marked listener holds our port): still
        // ours → NO reap, miss count reset, handle retained. This is the probe
        // wiring proof: owned_master (not alive()) is what says "still ours".
        ours.store(true, SeqCst);
        let (e1, _) = mgr.reconcile_health(&platform, &ca, &[]).await;
        assert_eq!(engine_events(&e1), 0, "still-ours must not reap: {e1:?}");
        assert_eq!(mgr.adopted_misses_for_test(&label), 0, "reset on a positive probe");
        assert!(mgr.has_db_for_test(engine), "handle retained");

        // Tick 2 — first miss: a single transient miss must NOT reap (the flap
        // the current code gets wrong).
        ours.store(false, SeqCst);
        let (e2, _) = mgr.reconcile_health(&platform, &ca, &[]).await;
        assert_eq!(engine_events(&e2), 0, "one miss must not reap: {e2:?}");
        assert_eq!(mgr.adopted_misses_for_test(&label), 1, "miss counted");
        assert!(mgr.has_db_for_test(engine), "still retained after one miss");

        // Gate spawn_db off so the reap doesn't try to start a real DB.
        mgr.preset_restart_attempts_for_test(&label, MAX_RESTART_ATTEMPTS);

        // Tick 3 — second CONSECUTIVE miss: reaped exactly once.
        ours.store(false, SeqCst);
        let (e3, _) = mgr.reconcile_health(&platform, &ca, &[]).await;
        assert_eq!(engine_events(&e3), 1, "second miss reaps exactly once: {e3:?}");
        assert!(!mgr.has_db_for_test(engine), "reaped (removed) after two consecutive misses");
        assert_eq!(mgr.adopted_misses_for_test(&label), 0, "miss count cleared on reap");
    }

    /// The live incident's stop half: the tracked pid was a stale adopted
    /// WORKER (71063) while the real master (95274) kept serving the port —
    /// the reap must signal the resolved MASTER (first), and the stale
    /// tracked pid too, then see the port free.
    #[test]
    fn override_reap_targets_the_resolved_master_not_the_stale_tracked_pid() {
        let platform = override_test_platform("reap-master", Some(95274));
        let mut backend = OverrideBackend {
            kind: OverrideKind::Apache,
            port: free_port(),
            child: Proc::Adopted(71063),
        };
        ServiceManager::reap_override_backend(&platform, "tr4.rex", &mut backend);
        let stopped = platform.sup.stopped.lock().unwrap().clone();
        assert_eq!(stopped.first(), Some(&95274), "signal the resolved master first");
        assert!(stopped.contains(&71063), "reap the stale tracked pid too");
    }

    // The guard-refusal side of `stop_override_backend` is NOT unit-testable
    // here: `may_control_real_stack()` is always true under cfg(test) by
    // design (see stack_guard). It shares the exact check the other guarded
    // chokepoints use, covered by stack_guard's sequenced flag test and the
    // live `examples/stack_guard_check`.

    fn edge_test_platform(name: &str, installed: bool, enabled: bool) -> EdgeTestPlatform {
        let dir = std::env::temp_dir().join(format!("rexenv-edge-sm-{name}"));
        let _ = std::fs::create_dir_all(&dir);
        EdgeTestPlatform { paths: TestPaths(dir), edge: TestEdge { installed, enabled } }
    }

    /// The live incident: a bootout raced the watchdog's re-adopt, leaving a
    /// `Daemon` handle whose socket is dead — the old unconditional early-return
    /// then made every Start-all silently skip the edge forever. A stale handle
    /// must be treated as stopped (H2: running = ownership AND liveness) and
    /// produce a fresh start plan.
    #[test]
    fn prepare_edge_restarts_over_a_stale_daemon_handle() {
        let platform = edge_test_platform("stale-handle", true, true);
        let mut mgr = ServiceManager::with_ports(Ports::default());
        mgr.set_bins_for_tests(PathBuf::from("/nonexistent/caddy"));
        mgr.set_edge_daemon(); // handle says running; no socket exists → dead
        let plan = mgr
            .prepare_edge(&platform, PathBuf::from("/nonexistent/Caddyfile"))
            .expect("prepare_edge")
            .expect("a stale daemon handle must yield a fresh start plan, not None");
        assert!(plan.privileged, ":443 default is a privileged start");
        assert!(!mgr.edge_is_daemon(), "stale handle must be reset to Stopped");
    }

    /// A dead supervised edge is announced ONCE as edge-restarting (launchd's
    /// KeepAlive throttle is ~10s), stays silent while waiting, and after the
    /// grace window escalates to a DIAGNOSED edge-down (here: label disabled =
    /// stopped outside the app) with the handle flipped to Stopped — never an
    /// unbounded "restarting" reassurance for an edge that is not coming back.
    #[tokio::test]
    async fn watchdog_bounds_edge_restarting_and_diagnoses_the_giveup() {
        let platform = edge_test_platform("watchdog-giveup", true, false);
        let dir = std::env::temp_dir().join("rexenv-edge-sm-watchdog-giveup");
        let ca = ssl::load_or_create_at(&dir.join("ca.pem"), &dir.join("ca.key"), None).unwrap();
        let mut mgr = ServiceManager::with_ports(Ports::default());
        mgr.set_edge_daemon(); // supervised edge; socket never exists → dead

        let caddy_events = |evts: &[HealthEvent]| {
            evts.iter().filter(|e| e.service == "Caddy").cloned().collect::<Vec<_>>()
        };

        let (e1, _) = mgr.reconcile_health(&platform, &ca, &[]).await;
        let e1 = caddy_events(&e1);
        assert_eq!(e1.len(), 1, "first dead poll announces once: {e1:?}");
        assert_eq!(e1[0].action, "edge-restarting");

        let (e2, _) = mgr.reconcile_health(&platform, &ca, &[]).await;
        assert!(caddy_events(&e2).is_empty(), "waiting polls stay silent: {e2:?}");

        let (e3, _) = mgr.reconcile_health(&platform, &ca, &[]).await;
        let e3 = caddy_events(&e3);
        assert_eq!(e3.len(), 1, "grace poll escalates: {e3:?}");
        assert_eq!(e3[0].action, "edge-down");
        assert!(e3[0].detail.contains("disabled"), "diagnosis names the cause: {}", e3[0].detail);
        assert!(!mgr.edge_is_daemon(), "given-up edge must read Stopped");

        // Once stopped (and still dead), the watchdog has nothing more to say.
        let (e4, _) = mgr.reconcile_health(&platform, &ca, &[]).await;
        assert!(caddy_events(&e4).is_empty(), "no spam after give-up: {e4:?}");
    }

    #[test]
    fn default_ports_are_canonical() {
        let p = Ports::default();
        assert_eq!(p.https, 443);
        assert_eq!(p.http, 80);
        assert_eq!(p.nginx, services::NGINX_HTTP_PORT);
    }

    #[test]
    fn managed_ports_cover_every_pinned_php_minor() {
        // The orphan sweep must include each pinned minor's pool port; deriving from
        // php::all_minors() means a future 8.4 pool isn't silently missed (L1).
        let ports = ServiceManager::default().managed_ports();
        for minor in php::all_minors() {
            if let Some(p) = php::fpm_port(&minor) {
                assert!(ports.contains(&p), "managed_ports missing pool port for {minor}");
            }
        }
    }

    /// The web tier's target names, and the refusal that matters most: an
    /// unknown one. A typo that fell through to "restart something plausible"
    /// would bounce a service serving OTHER sites than the one the user meant,
    /// so the parse has no fuzzy arm — `None` and an error naming the accepted
    /// forms.
    #[test]
    fn a_web_target_is_named_exactly_or_refused() {
        assert_eq!(WebTarget::parse("nginx"), Some(WebTarget::Nginx));
        assert_eq!(WebTarget::parse("NGINX"), Some(WebTarget::Nginx));
        // Both names for the edge: `caddy` is what the process is, `edge` is
        // what the docs and status call it, and a user should not have to know
        // which vocabulary this command speaks.
        assert_eq!(WebTarget::parse("edge"), Some(WebTarget::Edge));
        assert_eq!(WebTarget::parse("caddy"), Some(WebTarget::Edge));
        // A pool, with or without the prefix status prints.
        let minors = php::all_minors();
        let minor = minors.first().expect("a pinned minor");
        assert_eq!(WebTarget::parse(minor), Some(WebTarget::Pool(minor.clone())));
        assert_eq!(WebTarget::parse(&format!("php-{minor}")), Some(WebTarget::Pool(minor.clone())));
        // Only PINNED minors: a version we ship no pool for has no port, and
        // "restarted php-9.9" would be a sentence about nothing.
        assert_eq!(WebTarget::parse("php-9.9"), None);
        for junk in ["", "  ", "mysql", "mailpit", "ngin", "php-", "edge2"] {
            assert_eq!(WebTarget::parse(junk), None, "`{junk}` parsed as a web target");
        }
        assert_eq!(WebTarget::Pool("8.3".into()).label(), "PHP-FPM 8.3");
    }

    /// A service that is NOT running is reported, never started.
    ///
    /// Starting it here would be the friendly-looking bug: `start_all` brings
    /// the tier up in ORDER (pools → nginx → edge) because nginx's config names
    /// pool ports and the edge routes to nginx, so a lone service started out of
    /// order is a stack that half works and a user who thinks it is up.
    #[tokio::test]
    async fn restarting_a_stopped_web_service_reports_it_instead_of_starting_it() {
        let platform = override_test_platform("web-restart-stopped", None);
        // The CA is never touched on this path (nothing is generated for a
        // service that is not running), so a literal is honest here and keeps
        // the test off the filesystem.
        let ca = ssl::LocalCa {
            cert_pem: String::new(),
            key_pem: String::new(),
            cert_path: std::path::PathBuf::from("/dev/null"),
            key_path: std::path::PathBuf::from("/dev/null"),
        };
        let mut mgr = ServiceManager::default();
        for target in [WebTarget::Nginx, WebTarget::Edge] {
            let (outcome, checks) = mgr
                .restart_web_service(&platform, &ca, &[], &target)
                .await
                .expect("a stopped service is an outcome, not an error");
            assert_eq!(
                outcome,
                WebRestartOutcome::NotRunning,
                "{} was not running and something other than NotRunning came back",
                target.label()
            );
            assert!(checks.is_empty(), "nothing was spawned, so nothing may be awaited");
        }
        assert!(mgr.nginx.is_none(), "a restart of a stopped nginx started one anyway");
        assert!(
            matches!(mgr.caddy, CaddyHandle::Stopped),
            "a restart of a stopped edge started one anyway"
        );
    }

    /// A single-site restart must tell the truth about what it can restart.
    ///
    /// The default topology gives a site NO process of its own — shared nginx,
    /// and one php-fpm pool per PHP MINOR — so for a default site the only
    /// honest outcome is `Shared`, naming the pool it sits on. Returning
    /// "restarted" there would be a lie in the direction that matters: the user
    /// would believe a bounce happened, see the same stale behaviour, and go
    /// looking for a bug in their code. The alternative reading — bounce the
    /// pool so SOMETHING was restarted — stops every other site on that minor to
    /// satisfy one, which is why the pool is opt-in one layer up.
    #[tokio::test]
    async fn restarting_a_default_site_is_reported_as_shared_not_as_a_restart() {
        use crate::state::models::{MultisiteMode, ServiceStatus, SiteOrigin, SiteType};
        let site = |domain: &str, ws: WebServer| Site {
            id: domain.into(),
            name: domain.into(),
            domain: domain.into(),
            site_type: SiteType::Php,
            status: ServiceStatus::Stopped,
            php_version: "8.3".into(),
            web_server: ws,
            ssl: true,
            path: format!("/tmp/{domain}"),
            created_at: "now".into(),
            multisite: MultisiteMode::None,
            db_name: String::new(),
            db_engine: crate::state::models::SiteDbEngine::Mysql,
            xdebug: false,
            override_port: None,
            provisioned: true,
            docroot_managed: Some(true),
            db_created: None,
            content_dir: None,
            mu_dir_created: None,
            origin: SiteOrigin::User,
            agent_client: None,
            expires_at: None,
            docroot_subdir: String::new(),
            git_url: None,
            git_ref: None,
            git_migrate: None,
            git_build_assets: None,
            starter_db: None,
            enabled: true,
        };
        let platform = override_test_platform("restart-shared", None);
        let mut mgr = ServiceManager::default();
        let (outcome, checks) = mgr
            .restart_site_backend(&platform, &site("n.test", WebServer::Nginx))
            .await
            .expect("a default site restart never errors");
        assert_eq!(
            outcome,
            SiteRestartOutcome::Shared {
                php_minor: "8.3".into(),
                pool_port: php::fpm_port("8.3").unwrap(),
            },
            "a default site must report the SHARED pool it sits on, not a restart it did not do"
        );
        assert!(
            checks.is_empty(),
            "nothing was spawned, so there is nothing to wait for — a readiness check here \
             would make the caller wait on a process that does not exist"
        );
        // …and nothing was touched: no backend was tracked before or after.
        assert!(mgr.override_pids().is_empty(), "a shared-site restart started a backend");
    }

    #[test]
    fn site_serving_reflects_each_sites_own_upstream() {
        use crate::state::models::{MultisiteMode, ServiceStatus, SiteOrigin, SiteType};
        use std::collections::HashMap;

        let si = |name: &str, port: u16, running: bool| ServiceInfo {
            name: name.to_string(),
            running,
            pid: None,
            port,
            optional: false,
        };
        let site = |domain: &str, ver: &str, ws: WebServer| Site {
            id: domain.into(),
            name: domain.into(),
            domain: domain.into(),
            site_type: SiteType::Php,
            status: ServiceStatus::Stopped,
            php_version: ver.into(),
            web_server: ws,
            ssl: true,
            path: format!("/tmp/{domain}"),
            created_at: "now".into(),
            multisite: MultisiteMode::None,
            db_name: crate::core::wordpress::db_name_for(SiteType::Wordpress, domain),
            db_engine: crate::state::models::SiteDbEngine::Mysql,
            xdebug: false,
            override_port: None,
            provisioned: true,
            docroot_managed: Some(true),
            db_created: None,
            content_dir: None,
            mu_dir_created: None,
            origin: SiteOrigin::User,
            agent_client: None,
            expires_at: None,
            docroot_subdir: String::new(),
            git_url: None,
            git_ref: None,
            git_migrate: None,
            git_build_assets: None,
            starter_db: None,
            enabled: true,
        };

        let sites = vec![
            site("n.test", "8.3", WebServer::Nginx),
            site("f.test", "8.3", WebServer::Frankenphp),
        ];
        let pool = php::fpm_port("8.3").unwrap();
        let fpport = frankenphp::site_port("f.test");
        let caddy = |up| si("Caddy", 443, up);
        let nginx = |up| si("Nginx", services::NGINX_HTTP_PORT, up);
        let map = |infos: &[ServiceInfo]| -> HashMap<String, bool> {
            site_serving(&sites, infos)
                .into_iter()
                .map(|s| (s.domain, s.serving))
                .collect()
        };

        // Full stack up → both sites serve.
        let m = map(&[caddy(true), nginx(true), si("PHP-FPM 8.3", pool, true), si("FrankenPHP f.test", fpport, true)]);
        assert!(m["n.test"] && m["f.test"], "all up → both serve");

        // Edge down → nothing serves, even with every upstream up.
        let m = map(&[caddy(false), nginx(true), si("PHP-FPM 8.3", pool, true), si("FrankenPHP f.test", fpport, true)]);
        assert!(!m["n.test"] && !m["f.test"], "edge down → nothing serves");

        // Partial: FrankenPHP backend down → only its site is down; the nginx site still serves.
        let m = map(&[caddy(true), nginx(true), si("PHP-FPM 8.3", pool, true), si("FrankenPHP f.test", fpport, false)]);
        assert!(m["n.test"] && !m["f.test"], "fp backend down → only fp site down");

        // Partial: the nginx site's pool down → only it is down; the FrankenPHP site still serves.
        let m = map(&[caddy(true), nginx(true), si("PHP-FPM 8.3", pool, false), si("FrankenPHP f.test", fpport, true)]);
        assert!(!m["n.test"] && m["f.test"], "pool down → only nginx site down");

        // Nginx down → the nginx site is down; FrankenPHP bypasses nginx, so it's unaffected.
        let m = map(&[caddy(true), nginx(false), si("PHP-FPM 8.3", pool, true), si("FrankenPHP f.test", fpport, true)]);
        assert!(!m["n.test"] && m["f.test"], "nginx down → nginx site down, fp unaffected");
    }

    #[tokio::test]
    async fn wait_until_ready_errors_naming_service_and_log() {
        // A cond that's already true returns Ok without waiting.
        assert!(wait_until_ready("X", Path::new("/tmp/x-stdout.log"), 1, || true, None)
            .await
            .is_ok());
        // A cond that never becomes true errors, naming the service + its log path so
        // the failure is actionable (M3) rather than a silent Ok.
        let err = wait_until_ready("MySQL", Path::new("/var/log/mysql-stdout.log"), 1, || false, None)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("MySQL"), "error names the service: {err}");
        assert!(err.contains("mysql-stdout.log"), "error names the log: {err}");
    }

    /// **A start failure names the macOS mismatch, and nothing else does.**
    ///
    /// PostgreSQL's pinned builds declare `minos 26.0` while rexenv's floor is
    /// macOS 15, so every supported user below 26 is in the suspect band. If
    /// dyld refuses them, the only thing they see is "PostgreSQL did not start
    /// within 15s — see postgres-stdout.log", which sends them at Postgres.
    ///
    /// The note is DIAGNOSIS, never a gate: dyld on macOS 26 enforces minos for
    /// nothing, so refusing to spawn would block builds that may run perfectly
    /// on a prediction this project cannot test from a current machine.
    #[test]
    fn a_binary_that_needs_a_newer_macos_says_so_and_only_then() {
        // Equal, older, and per-component ordering — "9" > "26" as text.
        assert!(!macho::newer_than((15, 4, 0), (15, 4, 0)));
        assert!(!macho::newer_than((9, 0, 0), (26, 0, 0)));
        assert!(macho::newer_than((26, 0, 0), (15, 4, 0)));

        // A path with no Mach-O behind it must produce NO note rather than a
        // guess — the failure message stays exactly as it was.
        assert_eq!(macos_floor_note(Path::new("/nonexistent/postgres")), None);

        // And the real one, when the cache has it: postgres is the binary this
        // whole mechanism exists for.
        if let Some(home) = std::env::var_os("HOME") {
            let dir = Path::new(&home).join("Library/Application Support/dev.rexenv.rexenv/bin");
            let pg = std::fs::read_dir(&dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path().join("bin/postgres"))
                .find(|p| p.is_file());
            if let Some(pg) = pg {
                let need = macho::min_macos(&pg).expect("a cached postgres is a Mach-O");
                let host = macho::host_macos().expect("this test runs on macOS");
                let note = macos_floor_note(&pg);
                assert_eq!(
                    note.is_some(),
                    macho::newer_than(need, host),
                    "the note must appear exactly when the build outranks the host \
                     (needs {need:?}, host {host:?})"
                );
                if let Some(note) = note {
                    assert!(note.contains(&need.0.to_string()), "{note}");
                    assert!(note.contains(&host.0.to_string()), "{note}");
                }
            }
        }
    }

    #[tokio::test]
    async fn await_ready_is_concurrent_and_names_every_failure() {
        let check = |service: &str, ok: bool| ReadyCheck {
            service: service.into(),
            log: PathBuf::from(format!("/var/log/{}-stdout.log", service.to_lowercase())),
            tries: 1,
            probe: Box::new(move || ok),
            bin: None,
        };

        // Empty batch and an all-ready batch are Ok.
        assert!(await_ready(Vec::new()).await.is_ok());
        assert!(await_ready(vec![check("A", true), check("B", true)]).await.is_ok());

        // Two never-ready probes: BOTH are named (all checks run to completion,
        // no early abort), and the M3 log-path hint survives the join.
        let err = await_ready(vec![check("MySQL", false), check("Mailpit", false), check("OK", true)])
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("MySQL"), "first failure named: {err}");
        assert!(err.contains("Mailpit"), "second failure named: {err}");
        assert!(err.contains("mysql-stdout.log"), "log hint kept: {err}");
        assert!(!err.contains("OK "), "ready service not reported as failed: {err}");

        // Concurrency: N failing probes (500ms each) finish in ~one probe's time,
        // not N× — the whole point of deferring the waits (M4).
        let start = std::time::Instant::now();
        let _ = await_ready((0..4).map(|i| check(&format!("S{i}"), false)).collect()).await;
        assert!(
            start.elapsed() < Duration::from_millis(1600),
            "4×500ms probes ran concurrently, took {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn db_status_lists_available_engines_when_stopped() {
        let m = ServiceManager::default();
        let dbs: Vec<_> = m.db_status().iter().map(|d| d.engine).collect();
        // Every engine ships on macOS now.
        assert_eq!(
            dbs,
            vec![DbEngine::Mysql, DbEngine::Mariadb, DbEngine::Postgres, DbEngine::Redis]
        );
        assert!(m.db_status().iter().all(|d| d.pid.is_none()));
        // H2: nothing we started ⇒ nothing running, regardless of a foreign DB.
        assert!(m.db_status().iter().all(|d| !d.running));
    }

    #[test]
    fn watchdog_attempts_cap_then_one_gave_up_then_reset() {
        let mut m = ServiceManager::default();
        let mut events = Vec::new();
        for _ in 0..MAX_RESTART_ATTEMPTS {
            assert!(m.should_restart("Nginx", &mut events));
        }
        assert!(events.is_empty(), "no give-up while under the cap");
        // Past the cap: restart denied + exactly ONE give-up event…
        assert!(!m.should_restart("Nginx", &mut events));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, "gave-up");
        // …then silence (no event spam every tick).
        assert!(!m.should_restart("Nginx", &mut events));
        assert_eq!(events.len(), 1);
        // A healthy observation (or manual start/stop) resets the counter.
        m.restart_attempts.remove("Nginx");
        assert!(m.should_restart("Nginx", &mut events));
        // Counters are per service.
        assert!(m.should_restart("MySQL", &mut events));
    }

    #[test]
    fn status_lists_core_services_when_stopped() {
        let m = ServiceManager::default();
        let s = m.status(&*crate::platform::current(), &["8.1".to_string(), "8.3".to_string()]);
        let names: Vec<_> = s.iter().map(|i| i.name.as_str()).collect();
        // When stopped: the available DB engines + every INSTALLED php minor (idle
        // rows — the list must not grow/shrink with Start/Stop all) + Nginx + Caddy +
        // Mailpit. FrankenPHP overrides appear only when running.
        assert_eq!(
            names,
            vec![
                "MySQL",
                "MariaDB",
                "PostgreSQL",
                "Redis",
                "PHP-FPM 8.1",
                "PHP-FPM 8.3",
                "Nginx",
                "Caddy",
                "Mailpit"
            ]
        );
        assert!(s.iter().all(|i| i.pid.is_none()));
        // An idle pool row still shows its (deterministic) pool port.
        assert_eq!(s.iter().find(|i| i.name == "PHP-FPM 8.3").unwrap().port, 9783);
        // H2: a stopped manager owns nothing, so NOTHING reads as running — even if
        // some foreign process happens to hold one of these ports (e.g. another local
        // server on :443). Ownership is gated on our handles, not a bare port-listen.
        assert!(s.iter().all(|i| !i.running));
        assert!(m.pools.is_empty());
    }
}

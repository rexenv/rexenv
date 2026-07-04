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
use crate::core::{binaries, frankenphp, mail, php, ports, proxy, services, sites, ssl};
use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use crate::state::models::{Site, SiteServing, WebServer};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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
    /// Started as root via PrivilegeManager (driven via the admin API).
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
}

/// A prepared Caddy-edge start the caller runs OUTSIDE the services lock (the
/// privileged start blocks on an admin-password prompt).
pub struct EdgePlan {
    pub privileged: bool,
    pub caddy_bin: PathBuf,
    pub caddyfile: PathBuf,
}

/// One database engine's status (for the Databases view).
#[derive(Debug, Clone)]
pub struct DbInfo {
    pub engine: DbEngine,
    pub running: bool,
    pub pid: Option<u32>,
}

#[derive(Default)]
pub struct ServiceManager {
    bins: Option<Bins>,
    ports: Ports,
    dbs: HashMap<DbEngine, Child>,
    pools: php::PhpFpmPools,
    /// Per-site override backends (FrankenPHP), keyed by domain (§4.1).
    overrides: HashMap<String, Child>,
    /// FrankenPHP binary, resolved lazily on first override (avoids a download
    /// when no site uses it).
    frankenphp_bin: Option<PathBuf>,
    nginx: Option<Child>,
    caddy: CaddyHandle,
    /// Mailpit mail-catcher (§2.1), resolved + started lazily.
    mailpit: Option<Child>,
    mailpit_bin: Option<PathBuf>,
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
            nginx: None,
            caddy: CaddyHandle::Stopped,
            mailpit: None,
            mailpit_bin: None,
        }
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
        ports::ensure_free(engine.port(), ports::Proto::Tcp, engine.label())?;
        let child = engine.start(platform).await?;
        self.dbs.insert(engine, child);
        Ok(Some(ReadyCheck {
            service: engine.label().to_string(),
            log: stdout_log(platform, engine.key())?,
            tries: 30,
            probe: Box::new(move || engine.running()),
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
            let _ = child.wait();
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
                // "running" means an engine WE started this session is alive — not a
                // bare port-listen (a foreign/system DB on the port doesn't count),
                // so status is honest and Stop acts only on ours (task 2.2 / H2).
                running: self.dbs.contains_key(&engine) && engine.running(),
                pid: self.dbs.get(&engine).map(Child::id),
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
    ) -> Result<()> {
        let (caddyfile, checks) = self.start_core(platform, ca, sites, php_minors).await?;
        await_ready(checks).await?;
        if let Some(plan) = self.prepare_edge(platform, caddyfile)? {
            if plan.privileged {
                proxy::start_privileged(platform, &plan.caddy_bin, &plan.caddyfile)?;
                self.set_edge_privileged();
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
    ) -> Result<(PathBuf, Vec<ReadyCheck>)> {
        self.ensure_bins(platform).await?;
        let mut checks = Vec::new();

        // MySQL — the site stack needs it (started via the DB engine manager).
        checks.extend(self.spawn_db(platform, DbEngine::Mysql).await?);

        // Adminer docroot (§5.2): download + stage `adminer.php` so the internal
        // vhost the configs reference is actually served.
        crate::core::adminer::ensure(platform).await?;

        // Mailpit BEFORE the pools so each pool's config can route PHP `mail()` to
        // it (§2.2): resolves the binary (sets `mailpit_bin`) and starts the sink.
        // The pools only need the BINARY path (sendmail shim), not a ready Mailpit.
        checks.extend(self.spawn_mailpit(platform).await?);
        let sendmail = self.mailpit_bin.as_ref().map(|b| mail::sendmail_path(b));
        self.pools.set_sendmail_path(sendmail);

        // PHP-FPM: one pool per installed PHP version (always at least the default,
        // so the single-site path keeps working). Pools own their deterministic ports.
        let mut minors: Vec<String> = php_minors.to_vec();
        let default_minor = php::minor_of(binaries::PHP_VERSION);
        if !minors.contains(&default_minor) {
            minors.push(default_minor);
        }
        self.pools.start(platform, &minors).await?;

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
        )?;

        // Shared Nginx.
        if self.nginx.is_none() {
            ports::ensure_free(self.ports.nginx, ports::Proto::Tcp, "Nginx")?;
            self.nginx = Some(services::start_nginx(platform, &bins.nginx, &cfg.nginx_conf, &cfg.nginx_prefix)?);
        }

        Ok((cfg.caddyfile, checks))
    }

    /// Prepare the Caddy edge start. If the edge is stopped, clear any stale edge
    /// and gate the port, then return a plan the caller runs WITHOUT the services
    /// lock (the privileged start blocks on the admin-password prompt). `None` if
    /// the edge is already running.
    pub fn prepare_edge(
        &mut self,
        platform: &dyn Platform,
        caddyfile: PathBuf,
    ) -> Result<Option<EdgePlan>> {
        if !matches!(self.caddy, CaddyHandle::Stopped) {
            return Ok(None);
        }
        let bins = self.bins()?;
        // Clear a leftover REXENV edge (its admin socket + :443) so our start isn't
        // blocked (§7.3). Ownership-gated to our own edge — a foreign Caddy on the
        // default :2019 admin is never touched (task 2.4 / M1).
        proxy::recover_stale_edge(platform, &bins.caddy)?;
        ports::ensure_free(self.ports.https, ports::Proto::Tcp, "Caddy (HTTPS)")?;
        Ok(Some(EdgePlan {
            privileged: self.ports.https < 1024,
            caddy_bin: bins.caddy.clone(),
            caddyfile,
        }))
    }

    /// Record the edge as a root-privileged Caddy (started via osascript).
    pub fn set_edge_privileged(&mut self) {
        self.caddy = CaddyHandle::Privileged;
    }

    /// Record the edge as a child Caddy we own (unprivileged high port).
    pub fn set_edge_child(&mut self, child: std::process::Child) {
        self.caddy = CaddyHandle::Child(child);
    }

    /// Whether the shared stack is currently started (so reloads / pool changes
    /// take effect). False before `start_all` / after `stop_all`.
    pub fn is_running(&self) -> bool {
        self.nginx.is_some()
    }

    /// Ensure a php-fpm pool for `minor` is running, starting it if needed. Used
    /// when a site switches to a PHP version whose pool isn't up yet (§1.4).
    pub async fn ensure_php_pool(&mut self, platform: &dyn Platform, minor: &str) -> Result<()> {
        self.pools.ensure(platform, minor).await
    }

    /// Spawn Mailpit if not already managed (resolve its binary on first use),
    /// port-gated on its SMTP + HTTP ports. Returns its readiness probe for the
    /// caller to [`await_ready`] once the services lock is dropped (M4);
    /// `None` if it was already running. Owns start lifecycle (§2.1).
    pub async fn spawn_mailpit(&mut self, platform: &dyn Platform) -> Result<Option<ReadyCheck>> {
        if self.mailpit.is_some() {
            return Ok(None);
        }
        ports::ensure_free(mail::MAILPIT_SMTP_PORT, ports::Proto::Tcp, "Mailpit (SMTP)")?;
        ports::ensure_free(mail::MAILPIT_HTTP_PORT, ports::Proto::Tcp, "Mailpit (HTTP)")?;
        let bin = match &self.mailpit_bin {
            Some(p) => p.clone(),
            None => {
                let p = binaries::resolve(platform, "mailpit", binaries::MAILPIT_VERSION).await?;
                self.mailpit_bin = Some(p.clone());
                p
            }
        };
        self.mailpit = Some(mail::start(platform, &bin)?);
        Ok(Some(ReadyCheck {
            service: "Mailpit".to_string(),
            log: stdout_log(platform, "mailpit")?,
            tries: 20,
            probe: Box::new(mail::running),
        }))
    }

    /// Stop Mailpit if we manage it (no-op otherwise).
    pub fn stop_mailpit(&mut self, platform: &dyn Platform) -> Result<()> {
        if let Some(mut child) = self.mailpit.take() {
            let _ = mail::stop(platform, child.id());
            let _ = child.wait();
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

    /// Bring the running per-site override backends (FrankenPHP) in line with the
    /// site set: start one for each FrankenPHP site that isn't up, stop any whose
    /// site was deleted or switched away. Each backend listens on the site's
    /// deterministic override port (`frankenphp::site_port`). Returns one
    /// readiness probe per newly spawned backend — the caller [`await_ready`]s
    /// them (concurrently) after dropping the services lock (M4).
    async fn reconcile_overrides(
        &mut self,
        platform: &dyn Platform,
        sites: &[Site],
    ) -> Result<Vec<ReadyCheck>> {
        // Desired FrankenPHP backends: domain → (docroot, port, rewrite mode). The
        // rewrite mode is the site's real one (M5): a subdirectory-multisite override
        // needs WordPress's network rewrites, not the single-site default.
        let desired: HashMap<String, (PathBuf, u16, services::RewriteMode)> = sites
            .iter()
            .filter(|s| matches!(s.web_server, WebServer::Frankenphp))
            .map(|s| {
                (
                    s.domain.clone(),
                    (
                        PathBuf::from(&s.path),
                        frankenphp::site_port(&s.domain),
                        sites::rewrite_mode_for(s.multisite),
                    ),
                )
            })
            .collect();

        // Stop backends that are no longer wanted.
        let stale: Vec<String> = self
            .overrides
            .keys()
            .filter(|d| !desired.contains_key(*d))
            .cloned()
            .collect();
        for domain in stale {
            if let Some(mut child) = self.overrides.remove(&domain) {
                let _ = frankenphp::stop(platform, child.id());
                let _ = child.wait();
            }
        }

        // Start backends that are wanted but not yet running.
        let mut checks = Vec::new();
        for (domain, (docroot, port, rewrite)) in &desired {
            if self.overrides.contains_key(domain) {
                continue;
            }
            ports::ensure_free(*port, ports::Proto::Tcp, "FrankenPHP")?;
            let bin = self.ensure_frankenphp_bin(platform).await?;
            let conf = frankenphp::write_config(platform, domain, docroot, *port, *rewrite)?;
            let child = frankenphp::start(platform, &bin, domain, &conf)?;
            self.overrides.insert(domain.clone(), child);
            let port = *port;
            checks.push(ReadyCheck {
                service: format!("FrankenPHP ({domain})"),
                log: stdout_log(platform, &format!("frankenphp-{domain}"))?,
                tries: 20,
                probe: Box::new(move || frankenphp::running(port)),
            });
        }
        Ok(checks)
    }

    /// Reload the edge from the current site set (after create / delete / server
    /// switch): reconcile per-site override backends, then regenerate + reload
    /// Nginx and Caddy. The edge routes each site to its backend (shared Nginx or
    /// its own override port) via `rebuild_configs`. Returns the readiness probes
    /// of any newly spawned backends — callers [`await_ready`] them after
    /// dropping the services lock (M4). Until a backend is ready the edge may
    /// briefly 502 that one site; the command still fails with the named-service
    /// error (M3) if it never comes up.
    pub async fn reload(
        &mut self,
        platform: &dyn Platform,
        ca: &ssl::LocalCa,
        sites: &[Site],
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
        )?;
        services::reload_nginx(platform, &bins.nginx, &cfg.nginx_conf, &cfg.nginx_prefix)?;
        proxy::reload(platform, &bins.caddy, &cfg.caddyfile)?;
        Ok(checks)
    }

    /// Stop the whole stack.
    pub fn stop_all(&mut self, platform: &dyn Platform) -> Result<()> {
        // A tracked unprivileged child is killed by pid. For a root/privileged edge
        // — or a stray Caddy still on the admin port that we never tracked (common
        // after crashes/restarts) — drive Caddy's admin API to stop it and confirm
        // the port frees. This makes "Stop all" reliably release :443/:80.
        if let CaddyHandle::Child(mut c) = std::mem::take(&mut self.caddy) {
            let _ = proxy::stop(platform, c.id());
            let _ = c.wait();
        }
        if let Some(bins) = &self.bins {
            if let Err(e) = proxy::stop_edge(platform, &bins.caddy) {
                log::warn!("rexenv: stop_all could not stop the Caddy edge: {e}");
            }
        }
        self.stop_mailpit(platform)?;
        self.pools.stop_all(platform);
        for (_domain, mut child) in std::mem::take(&mut self.overrides) {
            let _ = frankenphp::stop(platform, child.id());
            let _ = child.wait();
        }
        for (engine, mut child) in std::mem::take(&mut self.dbs) {
            let _ = engine.stop(platform, child.id());
            let _ = child.wait();
        }
        if let Some(mut c) = self.nginx.take() {
            let _ = services::stop(platform, c.id());
            let _ = c.wait();
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
        for domain in self.overrides.keys() {
            ports.push(frankenphp::site_port(domain));
        }
        ports
    }

    /// Stop any rexenv-owned process still holding one of our managed ports (an
    /// orphan we no longer track). Guarded to our own processes by the app-data
    /// marker, so an unrelated process on the same port is never touched.
    fn stop_stale_owned(&self, platform: &dyn Platform) {
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
    }

    /// On app launch, clear rexenv-owned service orphans left by a PRIOR session
    /// (a crash, or quitting the app while services ran detached) so we start from
    /// a known-clean baseline: status is accurate and a later Start all won't hit
    /// `port in use`. Best-effort and guarded to our own processes; a clean launch
    /// with no orphans is a cheap no-op. Runs on a fresh (empty) manager, so it only
    /// ever stops things we didn't start this session.
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
    pub fn status(&self, installed_php: &[String]) -> Vec<ServiceInfo> {
        let mut infos = Vec::new();

        // Database engines (available ones) — always listed, running-state per port.
        for engine in DbEngine::ALL.into_iter().filter(|e| e.available()) {
            infos.push(ServiceInfo {
                name: engine.label().to_string(),
                // Owned + alive, not a bare port-listen (task 2.2 / H2).
                running: self.dbs.contains_key(&engine) && engine.running(),
                pid: self.dbs.get(&engine).map(Child::id),
                port: engine.port(),
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
            let pool = pools.iter().find(|p| p.minor == minor);
            infos.push(ServiceInfo {
                name: format!("PHP-FPM {minor}"),
                running: pool.is_some_and(|p| p.running),
                pid: pool.map(|p| p.pid),
                port: pool.map(|p| p.port).or_else(|| php::fpm_port(minor)).unwrap_or(0),
            });
        }

        // One row per per-site FrankenPHP override backend (sorted for stable display).
        let mut overrides: Vec<(&String, &Child)> = self.overrides.iter().collect();
        overrides.sort_by(|a, b| a.0.cmp(b.0));
        for (domain, child) in overrides {
            let port = frankenphp::site_port(domain);
            infos.push(ServiceInfo {
                name: format!("FrankenPHP {domain}"),
                running: frankenphp::running(port),
                pid: Some(child.id()),
                port,
            });
        }

        infos.push(ServiceInfo {
            name: "Nginx".to_string(),
            // Owned (we hold the child) + alive, not a bare port-listen (task 2.2 / H2).
            running: self.nginx.is_some() && services::nginx_running(self.ports.nginx),
            pid: self.nginx.as_ref().map(Child::id),
            port: self.ports.nginx,
        });
        infos.push(ServiceInfo {
            name: "Caddy".to_string(),
            // Ownership + start: true only when WE started the edge (handle != Stopped,
            // set only after a confirmed start, cleared on stop). A foreign listener on
            // :443 (e.g. another local server) must NOT read as rexenv's edge being up
            // (task 2.2 / H2). The admin channel is now a unix socket (task 2.3 / H5),
            // so there is no port to probe here; a rare post-start crash self-heals on
            // the next reconcile/stop.
            running: !matches!(self.caddy, CaddyHandle::Stopped),
            pid: match &self.caddy {
                CaddyHandle::Child(c) => Some(c.id()),
                _ => None,
            },
            port: self.ports.https,
        });
        infos.push(ServiceInfo {
            name: "Mailpit".to_string(),
            running: self.mailpit.is_some() && mail::running(),
            pid: self.mailpit.as_ref().map(Child::id),
            port: mail::MAILPIT_HTTP_PORT,
        });
        infos
    }
}

impl Drop for ServiceManager {
    fn drop(&mut self) {
        // Best-effort: SIGKILL our child processes so nothing is orphaned if
        // stop_all wasn't called. (A privileged-root Caddy can't be killed here;
        // the php-fpm pools clean themselves up via PhpFpmPools::drop.)
        for child in self.dbs.values_mut() {
            let _ = child.kill();
        }
        for child in self.overrides.values_mut() {
            let _ = child.kill();
        }
        if let Some(c) = &mut self.mailpit {
            let _ = c.kill();
        }
        if let Some(c) = &mut self.nginx {
            let _ = c.kill();
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
            let ReadyCheck { service, log, tries, probe } = check;
            wait_until_ready(&service, &log, tries, probe).await
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
) -> Result<()> {
    if wait_until(cond, tries).await {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "{service} did not start within {}s — see {}",
            tries / 2, // 500ms per try
            log.display()
        )))
    }
}

/// The `spawn_logged` stdout log path for a service `key` (`<key>-stdout.log` under
/// `log_dir`, e.g. `mysql`, `mailpit`, `frankenphp-my.test`). Matches `core::logs`.
fn stdout_log(platform: &dyn Platform, key: &str) -> Result<PathBuf> {
    Ok(platform.paths().log_dir()?.join(format!("{key}-stdout.log")))
}

/// Per-site serving status (H1 follow-up), derived from a live [`ServiceInfo`]
/// snapshot (the single non-blocking source shared with `services_status`). A site
/// is *serving* only when the edge is up AND its own upstream is up — so a partial
/// stack (e.g. one FrankenPHP backend down while nginx serves) no longer reports
/// every site as running.
///
/// Upstream, per server:
/// - **FrankenPHP override** — its per-site backend port must be up (nginx is bypassed).
/// - **nginx (default)** — the shared nginx AND the php-fpm pool the site's version
///   routes to (via [`sites::pool_port_for`], mirroring `nginx_site_for`).
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
            let upstream_up = match s.web_server {
                WebServer::Frankenphp => port_up(frankenphp::site_port(&s.domain)),
                _ => nginx_up && port_up(sites::pool_port_for(&s.php_version)),
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

    #[test]
    fn site_serving_reflects_each_sites_own_upstream() {
        use crate::state::models::{MultisiteMode, ServiceStatus, SiteType};
        use std::collections::HashMap;

        let si = |name: &str, port: u16, running: bool| ServiceInfo {
            name: name.to_string(),
            running,
            pid: None,
            port,
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
        assert!(wait_until_ready("X", Path::new("/tmp/x-stdout.log"), 1, || true)
            .await
            .is_ok());
        // A cond that never becomes true errors, naming the service + its log path so
        // the failure is actionable (M3) rather than a silent Ok.
        let err = wait_until_ready("MySQL", Path::new("/var/log/mysql-stdout.log"), 1, || false)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("MySQL"), "error names the service: {err}");
        assert!(err.contains("mysql-stdout.log"), "error names the log: {err}");
    }

    #[tokio::test]
    async fn await_ready_is_concurrent_and_names_every_failure() {
        let check = |service: &str, ok: bool| ReadyCheck {
            service: service.into(),
            log: PathBuf::from(format!("/var/log/{}-stdout.log", service.to_lowercase())),
            tries: 1,
            probe: Box::new(move || ok),
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
        // Only the shipped engines are listed (MariaDB/Redis deferred on macOS).
        assert_eq!(dbs, vec![DbEngine::Mysql, DbEngine::Postgres]);
        assert!(m.db_status().iter().all(|d| d.pid.is_none()));
        // H2: nothing we started ⇒ nothing running, regardless of a foreign DB.
        assert!(m.db_status().iter().all(|d| !d.running));
    }

    #[test]
    fn status_lists_core_services_when_stopped() {
        let m = ServiceManager::default();
        let s = m.status(&["8.1".to_string(), "8.3".to_string()]);
        let names: Vec<_> = s.iter().map(|i| i.name.as_str()).collect();
        // When stopped: the available DB engines + every INSTALLED php minor (idle
        // rows — the list must not grow/shrink with Start/Stop all) + Nginx + Caddy +
        // Mailpit. FrankenPHP overrides appear only when running.
        assert_eq!(
            names,
            vec!["MySQL", "PostgreSQL", "PHP-FPM 8.1", "PHP-FPM 8.3", "Nginx", "Caddy", "Mailpit"]
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

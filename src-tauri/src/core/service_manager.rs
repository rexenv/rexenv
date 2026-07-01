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
use crate::state::models::{Site, WebServer};
use std::collections::HashMap;
use std::path::PathBuf;
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

    /// Ensure a database engine is running (start it if we don't already manage
    /// it), port-gated. Used by `start_all` (MySQL) and the Databases UI.
    pub async fn ensure_db(&mut self, platform: &dyn Platform, engine: DbEngine) -> Result<()> {
        if self.dbs.contains_key(&engine) {
            return Ok(());
        }
        ports::ensure_free(engine.port(), ports::Proto::Tcp, engine.label())?;
        let child = engine.start(platform).await?;
        self.dbs.insert(engine, child);
        wait_until(|| engine.running(), 30);
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
                running: engine.running(),
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
    /// examples/tests. The `start_services` command instead calls `start_core` +
    /// `prepare_edge` so the privileged edge prompt doesn't hold the services lock.
    pub async fn start_all(
        &mut self,
        platform: &dyn Platform,
        ca: &ssl::LocalCa,
        sites: &[Site],
        php_minors: &[String],
    ) -> Result<()> {
        let caddyfile = self.start_core(platform, ca, sites, php_minors).await?;
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
    /// overrides, nginx) and return the generated Caddyfile path. Run under the
    /// services lock; the caller then starts the (blocking, privileged) edge
    /// WITHOUT the lock so status polls never block on it (see `prepare_edge`).
    pub async fn start_core(
        &mut self,
        platform: &dyn Platform,
        ca: &ssl::LocalCa,
        sites: &[Site],
        php_minors: &[String],
    ) -> Result<PathBuf> {
        self.ensure_bins(platform).await?;

        // MySQL — the site stack needs it (started via the DB engine manager).
        self.ensure_db(platform, DbEngine::Mysql).await?;

        // Adminer docroot (§5.2): download + stage `adminer.php` so the internal
        // vhost the configs reference is actually served.
        crate::core::adminer::ensure(platform).await?;

        // Mailpit BEFORE the pools so each pool's config can route PHP `mail()` to
        // it (§2.2): resolves the binary (sets `mailpit_bin`) and starts the sink.
        self.ensure_mailpit(platform).await?;
        let sendmail = self.mailpit_bin.as_ref().map(|b| mail::sendmail_path(b));
        self.pools.set_sendmail_path(sendmail);

        // PHP-FPM: one pool per installed PHP version (always at least the default,
        // so the single-site path keeps working). Pools own their deterministic ports.
        let mut minors: Vec<String> = php_minors.to_vec();
        let default_minor = php::minor_of(binaries::PHP_VERSION);
        if !minors.iter().any(|m| *m == default_minor) {
            minors.push(default_minor);
        }
        self.pools.start(platform, &minors).await?;

        // Per-site override backends (FrankenPHP) for the current site set.
        self.reconcile_overrides(platform, sites).await?;

        let bins = self.bins.as_ref().expect("bins resolved");

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

        Ok(cfg.caddyfile)
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
        let bins = self.bins.as_ref().expect("bins resolved");
        // Clear a leftover edge holding the admin port (§7.3) so our start isn't
        // blocked by `bind: address already in use` on :2019.
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

    /// Ensure Mailpit is running (resolve its binary + spawn on first use),
    /// port-gated on its SMTP + HTTP ports. Idempotent. Owns start lifecycle (§2.1).
    pub async fn ensure_mailpit(&mut self, platform: &dyn Platform) -> Result<()> {
        if self.mailpit.is_some() {
            return Ok(());
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
        wait_until(mail::running, 20);
        Ok(())
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
    /// deterministic override port (`frankenphp::site_port`).
    async fn reconcile_overrides(&mut self, platform: &dyn Platform, sites: &[Site]) -> Result<()> {
        // Desired FrankenPHP backends: domain → (docroot, port).
        let desired: HashMap<String, (PathBuf, u16)> = sites
            .iter()
            .filter(|s| matches!(s.web_server, WebServer::Frankenphp))
            .map(|s| {
                (
                    s.domain.clone(),
                    (PathBuf::from(&s.path), frankenphp::site_port(&s.domain)),
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
        for (domain, (docroot, port)) in &desired {
            if self.overrides.contains_key(domain) {
                continue;
            }
            ports::ensure_free(*port, ports::Proto::Tcp, "FrankenPHP")?;
            let bin = self.ensure_frankenphp_bin(platform).await?;
            let conf =
                frankenphp::write_config(platform, domain, docroot, *port, services::RewriteMode::Single)?;
            let child = frankenphp::start(platform, &bin, domain, &conf)?;
            self.overrides.insert(domain.clone(), child);
            wait_until(|| frankenphp::running(*port), 20);
        }
        Ok(())
    }

    /// Reload the edge from the current site set (after create / delete / server
    /// switch): reconcile per-site override backends, then regenerate + reload
    /// Nginx and Caddy. The edge routes each site to its backend (shared Nginx or
    /// its own override port) via `rebuild_configs`.
    pub async fn reload(
        &mut self,
        platform: &dyn Platform,
        ca: &ssl::LocalCa,
        sites: &[Site],
    ) -> Result<()> {
        self.reconcile_overrides(platform, sites).await?;
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
        Ok(())
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
        for minor in ["8.1", "8.2", "8.3"] {
            if let Some(p) = php::fpm_port(minor) {
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

    /// Per-service status for the Services view + metrics (§6.1): every service the
    /// app supervises — the available DB engines (MySQL, PostgreSQL), one row per
    /// running php-fpm pool (`PHP-FPM <version>`), one per per-site FrankenPHP
    /// backend (`FrankenPHP <domain>`), then Nginx and Caddy. The command layer
    /// enriches each row with live RAM/CPU from the monitor (by pid).
    pub fn status(&self) -> Vec<ServiceInfo> {
        let mut infos = Vec::new();

        // Database engines (available ones) — always listed, running-state per port.
        for engine in DbEngine::ALL.into_iter().filter(|e| e.available()) {
            infos.push(ServiceInfo {
                name: engine.label().to_string(),
                running: engine.running(),
                pid: self.dbs.get(&engine).map(Child::id),
                port: engine.port(),
            });
        }

        // One row per running php-fpm pool.
        for p in self.pools.status() {
            infos.push(ServiceInfo {
                name: format!("PHP-FPM {}", p.minor),
                running: p.running,
                pid: Some(p.pid),
                port: p.port,
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
            running: services::nginx_running(self.ports.nginx),
            pid: self.nginx.as_ref().map(Child::id),
            port: self.ports.nginx,
        });
        infos.push(ServiceInfo {
            name: "Caddy".to_string(),
            running: ports::is_listening(self.ports.https),
            pid: match &self.caddy {
                CaddyHandle::Child(c) => Some(c.id()),
                _ => None,
            },
            port: self.ports.https,
        });
        infos.push(ServiceInfo {
            name: "Mailpit".to_string(),
            running: mail::running(),
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
fn wait_until(mut cond: impl FnMut() -> bool, tries: u32) -> bool {
    for _ in 0..tries {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    cond()
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
    fn db_status_lists_available_engines_when_stopped() {
        let m = ServiceManager::default();
        let dbs: Vec<_> = m.db_status().iter().map(|d| d.engine).collect();
        // Only the shipped engines are listed (MariaDB/Redis deferred on macOS).
        assert_eq!(dbs, vec![DbEngine::Mysql, DbEngine::Postgres]);
        assert!(m.db_status().iter().all(|d| d.pid.is_none()));
    }

    #[test]
    fn status_lists_core_services_when_stopped() {
        let m = ServiceManager::default();
        let s = m.status();
        let names: Vec<_> = s.iter().map(|i| i.name.as_str()).collect();
        // When stopped: the available DB engines (always listed) + Nginx + Caddy +
        // Mailpit. No php-fpm pools or FrankenPHP overrides (those appear only when running).
        assert_eq!(names, vec!["MySQL", "PostgreSQL", "Nginx", "Caddy", "Mailpit"]);
        assert!(s.iter().all(|i| i.pid.is_none()));
        assert!(m.pools.is_empty());
    }
}

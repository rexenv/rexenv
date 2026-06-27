//! core::service_manager — owns the shared-service lifecycle (task 10.5).
//!
//! The app holds one `ServiceManager` (in app state) that starts/stops the
//! shared stack — MySQL, the php-fpm pool, the shared Nginx, and the Caddy edge
//! router — and reloads Nginx + Caddy when sites change. Each start is gated on
//! `core::ports::ensure_free` so a conflict (e.g. another stack on :443) is a
//! clear error, not a crash. Caddy on a privileged port (:443) is started as
//! root via `PrivilegeManager` (one prompt) and driven afterward through its
//! admin API; on a high port it's a supervised child.

use crate::core::{binaries, database, ports, proxy, services, sites, ssl};
use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use rusqlite::Connection;
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
    pub php_fpm: u16,
    pub mysql: u16,
}

impl Default for Ports {
    fn default() -> Self {
        Self {
            http: proxy::DEFAULT_HTTP_PORT,
            https: proxy::DEFAULT_HTTPS_PORT,
            nginx: services::NGINX_HTTP_PORT,
            php_fpm: services::PHP_FPM_PORT,
            mysql: database::MYSQL_PORT,
        }
    }
}

struct Bins {
    php_fpm: PathBuf,
    nginx: PathBuf,
    caddy: PathBuf,
    mysql_base: PathBuf,
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

/// One service's status for the UI / metrics.
#[derive(Debug, Clone)]
pub struct ServiceInfo {
    pub name: &'static str,
    pub running: bool,
    pub pid: Option<u32>,
    pub port: u16,
}

#[derive(Default)]
pub struct ServiceManager {
    bins: Option<Bins>,
    ports: Ports,
    mysql: Option<Child>,
    php_fpm: Option<Child>,
    nginx: Option<Child>,
    caddy: CaddyHandle,
}

impl ServiceManager {
    pub fn with_ports(ports: Ports) -> Self {
        Self {
            bins: None,
            ports,
            mysql: None,
            php_fpm: None,
            nginx: None,
            caddy: CaddyHandle::Stopped,
        }
    }

    /// Resolve (download + cache) the service binaries once.
    pub async fn ensure_bins(&mut self, platform: &dyn Platform) -> Result<()> {
        if self.bins.is_some() {
            return Ok(());
        }
        let php_fpm = binaries::resolve(platform, "php-fpm", binaries::PHP_VERSION).await?;
        let nginx = binaries::resolve(platform, "nginx", binaries::NGINX_VERSION).await?;
        let caddy = binaries::resolve(platform, "caddy", binaries::CADDY_VERSION).await?;
        let mysql_base = binaries::resolve_dir(platform, "mysql", binaries::MYSQL_VERSION).await?;
        self.bins = Some(Bins {
            php_fpm,
            nginx,
            caddy,
            mysql_base,
        });
        Ok(())
    }

    /// Start the whole stack (idempotent per service). Returns a clear error if a
    /// required port is already taken by something else.
    pub async fn start_all(
        &mut self,
        platform: &dyn Platform,
        ca: &ssl::LocalCa,
        conn: &Connection,
    ) -> Result<()> {
        self.ensure_bins(platform).await?;
        let bins = self.bins.as_ref().expect("bins resolved");

        // MySQL.
        if self.mysql.is_none() {
            ports::ensure_free(self.ports.mysql, ports::Proto::Tcp, "MySQL")?;
            let datadir = database::data_dir(platform)?;
            let socket = database::socket_path(platform)?;
            if let Some(parent) = socket.parent() {
                std::fs::create_dir_all(parent)?;
            }
            database::initialize(platform, &bins.mysql_base, &datadir)?;
            let child = database::start(platform, &bins.mysql_base, &datadir, self.ports.mysql, &socket)?;
            self.mysql = Some(child);
            wait_until(|| database::mysql_running(self.ports.mysql), 30);
        }

        // PHP-FPM.
        if self.php_fpm.is_none() {
            ports::ensure_free(self.ports.php_fpm, ports::Proto::Tcp, "PHP-FPM")?;
            let conf = services::write_fpm_config(platform, "8.3", self.ports.php_fpm)?;
            self.php_fpm = Some(services::start_fpm(platform, &bins.php_fpm, &conf)?);
        }

        // Configs derived from all sites.
        let cfg =
            sites::rebuild_configs(conn, platform, ca, self.ports.nginx, self.ports.http, self.ports.https)?;

        // Shared Nginx.
        if self.nginx.is_none() {
            ports::ensure_free(self.ports.nginx, ports::Proto::Tcp, "Nginx")?;
            self.nginx = Some(services::start_nginx(platform, &bins.nginx, &cfg.nginx_conf, &cfg.nginx_prefix)?);
        }

        // Caddy edge router.
        if matches!(self.caddy, CaddyHandle::Stopped) {
            ports::ensure_free(self.ports.https, ports::Proto::Tcp, "Caddy (HTTPS)")?;
            if self.ports.https < 1024 {
                // Privileged port → start as root (one auth prompt), drive via admin API.
                proxy::start_privileged(platform, &bins.caddy, &cfg.caddyfile)?;
                self.caddy = CaddyHandle::Privileged;
            } else {
                self.caddy = CaddyHandle::Child(proxy::start(platform, &bins.caddy, &cfg.caddyfile)?);
            }
        }
        Ok(())
    }

    /// Reload Nginx + Caddy from the current site set (after create/delete).
    pub fn reload(
        &self,
        platform: &dyn Platform,
        ca: &ssl::LocalCa,
        conn: &Connection,
    ) -> Result<()> {
        let bins = self
            .bins
            .as_ref()
            .ok_or_else(|| Error::Other("services not started".into()))?;
        let cfg =
            sites::rebuild_configs(conn, platform, ca, self.ports.nginx, self.ports.http, self.ports.https)?;
        services::reload_nginx(platform, &bins.nginx, &cfg.nginx_conf, &cfg.nginx_prefix)?;
        proxy::reload(platform, &bins.caddy, &cfg.caddyfile)?;
        Ok(())
    }

    /// Stop the whole stack.
    pub fn stop_all(&mut self, platform: &dyn Platform) -> Result<()> {
        match std::mem::take(&mut self.caddy) {
            CaddyHandle::Privileged => {
                if let Some(bins) = &self.bins {
                    let _ = proxy::stop_admin(platform, &bins.caddy);
                }
            }
            CaddyHandle::Child(mut c) => {
                let _ = proxy::stop(platform, c.id());
                let _ = c.wait();
            }
            CaddyHandle::Stopped => {}
        }
        for child in [&mut self.nginx, &mut self.php_fpm, &mut self.mysql] {
            if let Some(mut c) = child.take() {
                let _ = services::stop(platform, c.id());
                let _ = c.wait();
            }
        }
        Ok(())
    }

    /// Per-service status (for the Services view + metrics).
    pub fn status(&self) -> Vec<ServiceInfo> {
        vec![
            ServiceInfo {
                name: "MySQL",
                running: database::mysql_running(self.ports.mysql),
                pid: self.mysql.as_ref().map(Child::id),
                port: self.ports.mysql,
            },
            ServiceInfo {
                name: "PHP-FPM",
                running: services::fpm_running(self.ports.php_fpm),
                pid: self.php_fpm.as_ref().map(Child::id),
                port: self.ports.php_fpm,
            },
            ServiceInfo {
                name: "Nginx",
                running: services::nginx_running(self.ports.nginx),
                pid: self.nginx.as_ref().map(Child::id),
                port: self.ports.nginx,
            },
            ServiceInfo {
                name: "Caddy",
                running: ports::is_listening(self.ports.https),
                pid: match &self.caddy {
                    CaddyHandle::Child(c) => Some(c.id()),
                    _ => None,
                },
                port: self.ports.https,
            },
        ]
    }
}

impl Drop for ServiceManager {
    fn drop(&mut self) {
        // Best-effort: SIGKILL our child processes so nothing is orphaned if
        // stop_all wasn't called. (A privileged-root Caddy can't be killed here.)
        for child in [&mut self.nginx, &mut self.php_fpm, &mut self.mysql] {
            if let Some(c) = child {
                let _ = c.kill();
            }
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
        assert_eq!(p.mysql, database::MYSQL_PORT);
    }

    #[test]
    fn status_lists_all_services_when_stopped() {
        let m = ServiceManager::default();
        let s = m.status();
        let names: Vec<_> = s.iter().map(|i| i.name).collect();
        assert_eq!(names, vec!["MySQL", "PHP-FPM", "Nginx", "Caddy"]);
        assert!(s.iter().all(|i| i.pid.is_none()));
    }
}

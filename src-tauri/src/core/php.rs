//! core::php — multiple PHP versions: per-version FPM pools + the installed
//! registry (Phase 2 task 1.2).
//!
//! One php-fpm master per PHP **minor** series (8.1, 8.2, 8.3) — never per site;
//! all sites on a version share its pool. Each pool listens on a deterministic
//! loopback port ([`fpm_port`]), is started via `ProcessSupervisor::spawn_logged`
//! (through [`crate::core::services`]), and is gated on `core::ports::ensure_free`.
//! Which versions are enabled is recorded in SQLite (the `php_versions` table, via
//! `state::store`); the manager is told which minors to start so it stays
//! DB-agnostic (mirroring how `ServiceManager` is handed the site list).

use crate::core::{binaries, ports, services};
use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use crate::state::{models::PhpVersion, store};
use rusqlite::Connection;
use std::process::Child;

/// Base for per-version FPM ports: `9700 + major*10 + minor`, so 8.1 → 9781,
/// 8.2 → 9782, 8.3 → 9783 (keeps the Phase-1 port for 8.3).
const FPM_PORT_BASE: u16 = 9700;

/// The minor series of a (patch) version string: `"8.3.31"` → `"8.3"`.
pub fn minor_of(version: &str) -> String {
    let mut parts = version.split('.');
    match (parts.next(), parts.next()) {
        (Some(major), Some(minor)) => format!("{major}.{minor}"),
        _ => version.to_string(),
    }
}

/// All PHP minor series that have a pinned build (derived from
/// [`binaries::PHP_VERSIONS`]), in the same order (newest last).
pub fn all_minors() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for patch in binaries::PHP_VERSIONS {
        let m = minor_of(patch);
        if !out.contains(&m) {
            out.push(m);
        }
    }
    out
}

/// The pinned patch build for a minor series (`"8.3"` → `"8.3.31"`), or `None`.
pub fn patch_for_minor(minor: &str) -> Option<&'static str> {
    binaries::PHP_VERSIONS
        .iter()
        .copied()
        .find(|p| minor_of(p) == minor)
}

/// Deterministic loopback FastCGI port for a minor series (`"8.3"` → `9783`), or
/// `None` if `minor` isn't exactly `major.minor` numeric.
pub fn fpm_port(minor: &str) -> Option<u16> {
    let mut parts = minor.split('.');
    let major: u16 = parts.next()?.parse().ok()?;
    let min: u16 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None; // exactly major.minor, not a patch string
    }
    Some(FPM_PORT_BASE + major * 10 + min)
}

/// Seed/refresh the `php_versions` registry from the pinned build set. Idempotent:
/// inserts unknown versions (the default minor enabled, others available), and on
/// re-run updates `patch`/`fpm_port`/`is_default` while **preserving** the user's
/// `installed` choices. Safe to call on every app start.
pub fn seed_registry(conn: &Connection) -> Result<()> {
    let default_minor = minor_of(binaries::PHP_VERSION);
    for minor in all_minors() {
        let patch = patch_for_minor(&minor)
            .ok_or_else(|| Error::Other(format!("no pinned PHP build for {minor}")))?;
        let port =
            fpm_port(&minor).ok_or_else(|| Error::Other(format!("no fpm port for {minor}")))?;
        let is_default = minor == default_minor;
        store::upsert_php_version(
            conn,
            &PhpVersion {
                minor: minor.clone(),
                patch: patch.to_string(),
                fpm_port: port,
                // On first insert the default is enabled; others are available but
                // off until the user installs them (Phase 2 §1.5). Preserved on update.
                installed: is_default,
                is_default,
            },
        )?;
    }
    Ok(())
}

/// All registered PHP versions (installed + available), for the UI.
pub fn list_versions(conn: &Connection) -> Result<Vec<PhpVersion>> {
    store::list_php_versions(conn)
}

/// Enable (install) or disable (remove) a PHP version. Guards on removal: the
/// default version can't be removed, nor can one a site currently uses (it would
/// silently fall back to the default pool). The version must be in the registry.
pub fn set_installed(conn: &Connection, minor: &str, installed: bool) -> Result<()> {
    if !installed {
        let versions = store::list_php_versions(conn)?;
        if versions.iter().any(|v| v.minor == minor && v.is_default) {
            return Err(Error::Other(format!(
                "cannot remove the default PHP version ({minor})"
            )));
        }
        let in_use = crate::core::sites::list(conn)?
            .iter()
            .any(|s| minor_of(&s.php_version) == minor);
        if in_use {
            return Err(Error::Other(format!(
                "PHP {minor} is in use by a site — switch those sites first"
            )));
        }
    }
    if !store::set_php_installed(conn, minor, installed)? {
        return Err(Error::Other(format!("unknown PHP version: {minor}")));
    }
    Ok(())
}

/// The minor series the app should start pools for: every registry row marked
/// `installed`. Falls back to the default minor if none are (so there is always a
/// working pool).
pub fn installed_minors(conn: &Connection) -> Result<Vec<String>> {
    let mut minors: Vec<String> = store::list_php_versions(conn)?
        .into_iter()
        .filter(|v| v.installed)
        .map(|v| v.minor)
        .collect();
    if minors.is_empty() {
        minors.push(minor_of(binaries::PHP_VERSION));
    }
    Ok(minors)
}

/// A running php-fpm pool's status (for the Services view / metrics).
#[derive(Debug, Clone)]
pub struct PoolStatus {
    pub minor: String,
    pub port: u16,
    pub pid: u32,
    pub running: bool,
}

struct Pool {
    minor: String,
    port: u16,
    child: Child,
}

/// Owns one php-fpm master per PHP version. Held by the `ServiceManager`.
#[derive(Default)]
pub struct PhpFpmPools {
    pools: Vec<Pool>,
    /// `php_admin_value[sendmail_path]` baked into every pool's config so site PHP
    /// `mail()` is routed to Mailpit (§2.2). Set by `ServiceManager` once Mailpit's
    /// binary is resolved; `None` ⇒ pools use PHP's default sendmail.
    sendmail_path: Option<String>,
}

impl PhpFpmPools {
    /// Set the mail-routing shim used when (re)writing pool configs. Applies to
    /// pools started afterward (a running pool keeps its config until restarted).
    pub fn set_sendmail_path(&mut self, sendmail_path: Option<String>) {
        self.sendmail_path = sendmail_path;
    }

    /// Start a pool for `minor` if one isn't already running. Idempotent: resolves
    /// (downloads on first use) the version's `php-fpm`, gates on a free port, then
    /// writes the pool config and spawns the foreground master.
    pub async fn ensure(&mut self, platform: &dyn Platform, minor: &str) -> Result<()> {
        if self.pools.iter().any(|p| p.minor == minor) {
            return Ok(());
        }
        let patch = patch_for_minor(minor)
            .ok_or_else(|| Error::Other(format!("no pinned PHP build for {minor}")))?;
        let port =
            fpm_port(minor).ok_or_else(|| Error::Other(format!("no fpm port for {minor}")))?;
        ports::ensure_free(port, ports::Proto::Tcp, "PHP-FPM")?;
        let bin = binaries::resolve(platform, "php-fpm", patch).await?;
        let conf = services::write_fpm_config(platform, minor, port, self.sendmail_path.as_deref())?;
        let child = services::start_fpm(platform, &bin, &conf)?;
        self.pools.push(Pool {
            minor: minor.to_string(),
            port,
            child,
        });
        Ok(())
    }

    /// Ensure a pool is running for each minor in `minors`.
    pub async fn start(&mut self, platform: &dyn Platform, minors: &[String]) -> Result<()> {
        for m in minors {
            self.ensure(platform, m).await?;
        }
        Ok(())
    }

    /// Stop and clear every pool.
    pub fn stop_all(&mut self, platform: &dyn Platform) {
        for mut p in std::mem::take(&mut self.pools) {
            let _ = services::stop(platform, p.child.id());
            let _ = p.child.wait();
        }
    }

    /// Whether no pools are currently managed.
    pub fn is_empty(&self) -> bool {
        self.pools.is_empty()
    }

    /// Per-pool status, ordered by minor series.
    pub fn status(&self) -> Vec<PoolStatus> {
        let mut out: Vec<PoolStatus> = self
            .pools
            .iter()
            .map(|p| PoolStatus {
                minor: p.minor.clone(),
                port: p.port,
                pid: p.child.id(),
                running: services::fpm_running(p.port),
            })
            .collect();
        out.sort_by(|a, b| a.minor.cmp(&b.minor));
        out
    }
}

impl Drop for PhpFpmPools {
    fn drop(&mut self) {
        // Best-effort SIGKILL so no pool is orphaned if stop_all wasn't called.
        for p in &mut self.pools {
            let _ = p.child.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::db;

    #[test]
    fn minor_of_strips_patch() {
        assert_eq!(minor_of("8.3.31"), "8.3");
        assert_eq!(minor_of("8.1.34"), "8.1");
        assert_eq!(minor_of("8.3"), "8.3");
    }

    #[test]
    fn fpm_port_is_deterministic_and_keeps_phase1_port() {
        assert_eq!(fpm_port("8.1"), Some(9781));
        assert_eq!(fpm_port("8.2"), Some(9782));
        // 8.3 must equal the Phase-1 single-pool port.
        assert_eq!(fpm_port("8.3"), Some(9783));
        assert_eq!(fpm_port("8.3"), Some(services::PHP_FPM_PORT));
        // Distinct per version.
        assert_ne!(fpm_port("8.1"), fpm_port("8.2"));
        // Rejects a patch string or junk.
        assert_eq!(fpm_port("8.3.31"), None);
        assert_eq!(fpm_port("x.y"), None);
    }

    #[test]
    fn minors_and_patches_track_pinned_builds() {
        let minors = all_minors();
        assert!(minors.contains(&"8.3".to_string()));
        assert!(minors.len() >= 2);
        for m in &minors {
            // Every minor maps back to a pinned patch in the same series.
            let patch = patch_for_minor(m).unwrap();
            assert_eq!(&minor_of(patch), m);
            assert!(fpm_port(m).is_some());
        }
        assert!(patch_for_minor("8.0").is_none());
    }

    #[test]
    fn seed_registry_marks_default_installed_and_is_idempotent() {
        let conn = db::open_in_memory().unwrap();
        seed_registry(&conn).unwrap();

        let rows = store::list_php_versions(&conn).unwrap();
        assert_eq!(rows.len(), all_minors().len());
        let default_minor = minor_of(binaries::PHP_VERSION);
        let def = rows.iter().find(|v| v.minor == default_minor).unwrap();
        assert!(def.is_default && def.installed);
        assert!(def.fpm_port == fpm_port(&default_minor).unwrap());
        // Non-default versions exist but aren't installed by default.
        assert!(rows.iter().any(|v| !v.is_default && !v.installed));

        // installed_minors reflects the default.
        assert_eq!(installed_minors(&conn).unwrap(), vec![default_minor.clone()]);

        // A user enables 8.1; re-seeding must NOT clobber that choice.
        store::set_php_installed(&conn, "8.1", true).unwrap();
        seed_registry(&conn).unwrap();
        let mut got = installed_minors(&conn).unwrap();
        got.sort();
        assert!(got.contains(&"8.1".to_string()));
        assert!(got.contains(&default_minor));
    }

    #[test]
    fn set_installed_guards_default_and_in_use() {
        use crate::core::sites;
        use crate::state::models::{NewSite, SiteType, WebServer};

        let conn = db::open_in_memory().unwrap();
        seed_registry(&conn).unwrap();
        let default_minor = minor_of(binaries::PHP_VERSION); // "8.3"

        // The default version can't be removed.
        assert!(set_installed(&conn, &default_minor, false).is_err());

        // Install 8.1, then put a site on it → it can't be removed.
        set_installed(&conn, "8.1", true).unwrap();
        sites::create(
            &conn,
            NewSite {
                name: "S".into(),
                domain: "s.test".into(),
                site_type: SiteType::Php,
                php_version: "8.1".into(),
                web_server: WebServer::Nginx,
                path: "~/Sites/s".into(),
            },
        )
        .unwrap();
        assert!(set_installed(&conn, "8.1", false).is_err());

        // An unknown version is rejected.
        assert!(set_installed(&conn, "9.9", true).is_err());
    }

    #[test]
    fn installed_minors_falls_back_to_default() {
        let conn = db::open_in_memory().unwrap();
        seed_registry(&conn).unwrap();
        // Disable everything → still returns the default minor.
        for v in store::list_php_versions(&conn).unwrap() {
            store::set_php_installed(&conn, &v.minor, false).unwrap();
        }
        assert_eq!(
            installed_minors(&conn).unwrap(),
            vec![minor_of(binaries::PHP_VERSION)]
        );
    }

    #[test]
    fn pools_start_empty() {
        let pools = PhpFpmPools::default();
        assert!(pools.is_empty());
        assert!(pools.status().is_empty());
    }
}

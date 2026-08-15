//! core::db — unified database-engine lifecycle (Phase 2 §5).
//!
//! One `DbEngine` enum models every built-in database service behind a single
//! shape: a known loopback port, binary resolution + init-if-needed, start/stop
//! via `ProcessSupervisor::spawn_logged`, and a TCP running-probe. MySQL (the
//! Phase-1 service) is expressed through it by delegating to `core::database`, so
//! its behavior is unchanged; MariaDB / PostgreSQL / Redis delegate to their own
//! modules the same way.

use crate::core::{binaries, database, mariadb, ports, postgres, redis};
use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use crate::state::models::SiteDbEngine;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::process::Child;

/// The SQL CLIENT BINARY (`bin/mysql` / `bin/mariadb`), as a type instead of a
/// bare `&Path`.
///
/// Constructible ONLY by [`DbEngine::sql_client_bins`] and
/// [`DbEngine::cached_sql_client`] — the places that know an engine's on-disk
/// layout. Exists because `&Path` meant two things on this surface: `b5861c4`
/// renamed `mysql_basedir` → `db_client` and changed the parameter's MEANING
/// (extracted tree → client binary) without changing its type, so nine
/// examples kept passing the tree, nothing failed to compile, and they sat
/// red from 15 Jul to 14 Aug 2026 — exec'ing a directory is EACCES before any
/// DB contact, so no leftover state could ever make them pass. With the
/// client as its own type, that call does not compile, which is the guard a
/// review comment provably was not.
#[derive(Debug, Clone)]
pub struct SqlClient(PathBuf);

impl SqlClient {
    /// The binary's path — for spawning and for error messages.
    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Test-only constructor (fixture clients at nonexistent paths, refusal
    /// tests). Everything that runs for real goes through
    /// [`DbEngine::sql_client_bins`] / [`DbEngine::cached_sql_client`].
    #[cfg(test)]
    pub(crate) fn test_at(path: impl Into<PathBuf>) -> SqlClient {
        SqlClient(path.into())
    }
}

/// Loopback ports for the database services. Each is non-default so it doesn't
/// clash with a system install (MySQL 3306 / Postgres 5432 / Redis 6379).
pub const MARIADB_PORT: u16 = 13307;
pub const POSTGRES_PORT: u16 = 15432;
pub const REDIS_PORT: u16 = 16379;

/// When an engine's datadir initialization fails, remove the half-written
/// datadir so a marker the init tool creates EARLY (e.g. MySQL/MariaDB's
/// `mysql/` system-schema dir, written before init completes) can't satisfy
/// `is_initialized` on the next run and start the server on a corrupt,
/// unrecoverable datadir. Returns the result unchanged. Every engine's
/// `initialize` funnels its work through this so cleanup covers ALL failure
/// paths — not just a nonzero exit (findings B22/B23).
pub(crate) fn clean_datadir_on_init_failure(
    datadir: &std::path::Path,
    result: Result<()>,
) -> Result<()> {
    if result.is_err() {
        let _ = std::fs::remove_dir_all(datadir);
    }
    result
}

/// A built-in database engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DbEngine {
    Mysql,
    Mariadb,
    Postgres,
    Redis,
}

impl DbEngine {
    /// All engines, in display order.
    pub const ALL: [DbEngine; 4] = [
        DbEngine::Mysql,
        DbEngine::Mariadb,
        DbEngine::Postgres,
        DbEngine::Redis,
    ];

    /// Stable lowercase key (wire/DB/log identifier).
    pub fn key(&self) -> &'static str {
        match self {
            DbEngine::Mysql => "mysql",
            DbEngine::Mariadb => "mariadb",
            DbEngine::Postgres => "postgres",
            DbEngine::Redis => "redis",
        }
    }

    /// Human label for the UI.
    pub fn label(&self) -> &'static str {
        match self {
            DbEngine::Mysql => "MySQL",
            DbEngine::Mariadb => "MariaDB",
            DbEngine::Postgres => "PostgreSQL",
            DbEngine::Redis => "Redis",
        }
    }

    /// The engine's known loopback port.
    pub fn port(&self) -> u16 {
        match self {
            DbEngine::Mysql => database::MYSQL_PORT,
            DbEngine::Mariadb => MARIADB_PORT,
            DbEngine::Postgres => POSTGRES_PORT,
            DbEngine::Redis => REDIS_PORT,
        }
    }

    /// The DEFAULT pinned version of the engine (what a fresh install runs).
    pub fn default_version(&self) -> &'static str {
        match self {
            DbEngine::Mysql => binaries::MYSQL_VERSION,
            DbEngine::Mariadb => binaries::MARIADB_VERSION,
            DbEngine::Postgres => binaries::POSTGRES_VERSION,
            DbEngine::Redis => binaries::REDIS_VERSION,
        }
    }

    /// All versions the engine can run (per-engine version switch), default
    /// first. A one-entry set means the UI hides the picker.
    pub fn versions(&self) -> &'static [&'static str] {
        match self {
            DbEngine::Mysql => binaries::MYSQL_VERSIONS,
            DbEngine::Mariadb => binaries::MARIADB_VERSIONS,
            DbEngine::Postgres => binaries::POSTGRES_VERSIONS,
            DbEngine::Redis => binaries::REDIS_VERSIONS,
        }
    }

    /// The settings key persisting the engine's selected version.
    fn version_setting_key(&self) -> String {
        format!("db_version_{}", self.key())
    }

    /// The engine's EFFECTIVE version: the stored selection when it's still an
    /// offered pin, else the default (a selection orphaned by a pin bump falls
    /// back safely — its per-series datadir stays on disk, never deleted).
    pub fn effective_version(&self, conn: &Connection) -> String {
        crate::state::store::get_setting(conn, &self.version_setting_key())
            .ok()
            .flatten()
            .filter(|v| self.versions().contains(&v.as_str()))
            .unwrap_or_else(|| self.default_version().to_string())
    }

    /// Persist the engine's selected version (validated against the offered set).
    pub fn set_version(&self, conn: &Connection, version: &str) -> Result<()> {
        if !self.versions().contains(&version) {
            return Err(Error::Other(format!(
                "{} {version} is not an offered version",
                self.label()
            )));
        }
        crate::state::store::set_setting(conn, &self.version_setting_key(), version)
    }

    /// The datadir SERIES a version belongs to — each series keeps its OWN
    /// datadir (never an in-place upgrade/downgrade: PG major datadirs are
    /// mutually incompatible, MySQL/MariaDB downgrades unsupported). MySQL/
    /// MariaDB/Redis key on major.minor; PostgreSQL on the major.
    pub fn series_of(&self, version: &str) -> String {
        let mut it = version.split('.');
        let major = it.next().unwrap_or_default();
        match self {
            DbEngine::Postgres => major.to_string(),
            _ => format!("{major}.{}", it.next().unwrap_or_default()),
        }
    }

    /// The datadir for a VERSION: the default pin's series keeps the legacy
    /// path (`<engine>/data` — existing installs are never moved), other
    /// series live under `<engine>/<series>/data`.
    pub fn data_dir(&self, platform: &dyn Platform, version: &str) -> Result<PathBuf> {
        let legacy = match self {
            DbEngine::Mysql => database::data_dir(platform)?,
            DbEngine::Mariadb => mariadb::data_dir(platform)?,
            DbEngine::Postgres => postgres::data_dir(platform)?,
            DbEngine::Redis => redis::data_dir(platform)?,
        };
        let series = self.series_of(version);
        if series == self.series_of(self.default_version()) {
            Ok(legacy)
        } else {
            Ok(platform
                .paths()
                .app_data_dir()?
                .join(self.key())
                .join(series)
                .join("data"))
        }
    }

    /// Whether this engine has a working macOS binary + lifecycle (so it can be
    /// listed/started). All four ship on macOS now; the gate stays for the
    /// windows/linux stubs era.
    pub fn available(&self) -> bool {
        matches!(
            self,
            DbEngine::Mysql | DbEngine::Mariadb | DbEngine::Postgres | DbEngine::Redis
        )
    }

    /// Whether the site stack REQUIRES this engine — required engines are
    /// started by Start-all and count toward the footer's "All running";
    /// optional ones (Postgres, …) are user-toggled on the Databases page and
    /// only count while running, so an engine the user never opted into can't
    /// pin the footer at "Partial" forever.
    pub fn required(&self) -> bool {
        matches!(self, DbEngine::Mysql)
    }

    /// Look up an engine by its `key`, or `None`.
    pub fn from_key(key: &str) -> Option<DbEngine> {
        DbEngine::ALL.into_iter().find(|e| e.key() == key)
    }

    /// The engine backing a site's database (`sites.db_engine`).
    pub fn from_site(engine: SiteDbEngine) -> DbEngine {
        match engine {
            SiteDbEngine::Mysql => DbEngine::Mysql,
            SiteDbEngine::Mariadb => DbEngine::Mariadb,
        }
    }

    /// Resolve the bundled SQL `(client, dump)` binaries for site DB
    /// operations (create/drop/import/export/sizes — the bundled-client rule:
    /// never a PATH client), from the engine's SELECTED version's tree so the
    /// client always matches the running server. Site-capable engines only.
    pub async fn sql_client_bins(
        &self,
        platform: &dyn Platform,
        version: &str,
    ) -> Result<(SqlClient, PathBuf)> {
        match self {
            DbEngine::Mysql => {
                let base = binaries::resolve_dir(platform, "mysql", version).await?;
                Ok((SqlClient(database::mysql_client_bin(&base)), base.join("bin/mysqldump")))
            }
            DbEngine::Mariadb => {
                let base = binaries::resolve_bundle(platform, "mariadb", version).await?;
                Ok((SqlClient(mariadb::mariadb_client_bin(&base)), mariadb::mariadb_dump_bin(&base)))
            }
            other => Err(Error::Other(format!(
                "{} does not host site databases",
                other.label()
            ))),
        }
    }

    /// The SQL client from an ALREADY-published cache — strictly offline, for
    /// status-poll paths (the per-site DB-size query) where triggering a
    /// download is wrong. `None` when uncached or not a site engine.
    pub fn cached_sql_client(&self, platform: &dyn Platform, version: &str) -> Option<SqlClient> {
        let bin_dir = platform.paths().bin_dir().ok()?;
        let client = match self {
            DbEngine::Mysql => bin_dir.join(format!("mysql-{version}")).join("bin/mysql"),
            DbEngine::Mariadb => {
                bin_dir.join(format!("mariadb-{version}")).join("bin/mariadb")
            }
            _ => return None,
        };
        client.is_file().then_some(SqlClient(client))
    }

    /// Whether the engine's datadir FOR A VERSION was ever initialized — the
    /// delete-site guard ("no datadir ⇒ no database can exist ⇒ nothing to
    /// drop").
    pub fn datadir_initialized(&self, platform: &dyn Platform, version: &str) -> bool {
        let Ok(datadir) = self.data_dir(platform, version) else {
            return false;
        };
        match self {
            DbEngine::Mysql => database::is_initialized(&datadir),
            DbEngine::Mariadb => mariadb::is_initialized(&datadir),
            _ => false,
        }
    }

    /// Resolve the binary for `version`, initialize that version-series'
    /// datadir if needed, and start the server (foreground, supervised) on the
    /// engine's port. Returns the child handle.
    pub async fn start(&self, platform: &dyn Platform, version: &str) -> Result<Child> {
        let datadir = self.data_dir(platform, version)?;
        match self {
            DbEngine::Mysql => {
                let basedir = binaries::resolve_dir(platform, "mysql", version).await?;
                let socket = database::socket_path(platform)?;
                if let Some(parent) = socket.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                database::initialize(platform, &basedir, &datadir)?;
                database::start(platform, &basedir, &datadir, self.port(), &socket)
            }
            DbEngine::Postgres => {
                let basedir = binaries::resolve_dir(platform, "postgres", version).await?;
                postgres::initialize(platform, &basedir, &datadir)?;
                postgres::start(platform, &basedir, &datadir, self.port())
            }
            DbEngine::Mariadb => {
                let basedir = binaries::resolve_bundle(platform, "mariadb", version).await?;
                let socket = mariadb::socket_path(platform)?;
                mariadb::initialize(platform, &basedir, &datadir)?;
                mariadb::start(platform, &basedir, &datadir, self.port(), &socket)
            }
            DbEngine::Redis => {
                let basedir = binaries::resolve_bundle(platform, "redis", version).await?;
                redis::start(platform, &basedir, &datadir, self.port())
            }
        }
    }

    /// Stop a running engine by pid.
    pub fn stop(&self, platform: &dyn Platform, pid: u32) -> Result<()> {
        platform.supervisor().stop(pid)
    }

    /// True if the engine is accepting connections on its loopback port.
    pub fn running(&self) -> bool {
        ports::is_listening(self.port())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_datadir_on_init_failure_removes_only_a_failed_datadir() {
        let base =
            std::env::temp_dir().join(format!("rexenv-initclean-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);

        // Failure: a datadir carrying the early "mysql/" marker must be removed,
        // so the lying marker can't survive to the next start (B22/B23).
        let failed = base.join("failed");
        std::fs::create_dir_all(failed.join("mysql")).unwrap();
        let out = clean_datadir_on_init_failure(&failed, Err(Error::Other("boom".into())));
        assert!(out.is_err(), "the error is passed through unchanged");
        assert!(!failed.exists(), "a failed init must remove the half-written datadir");

        // Success: the datadir is kept intact.
        let ok = base.join("ok");
        std::fs::create_dir_all(ok.join("mysql")).unwrap();
        let out = clean_datadir_on_init_failure(&ok, Ok(()));
        assert!(out.is_ok());
        assert!(ok.exists(), "a successful init keeps the datadir");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn engines_have_distinct_ports_and_keys() {
        let ports: Vec<u16> = DbEngine::ALL.iter().map(|e| e.port()).collect();
        let mut sorted = ports.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), ports.len(), "engine ports must be distinct");

        // MySQL keeps its Phase-1 port; the others avoid system defaults.
        assert_eq!(DbEngine::Mysql.port(), database::MYSQL_PORT);
        assert_ne!(DbEngine::Postgres.port(), 5432);
        assert_ne!(DbEngine::Redis.port(), 6379);

        // Keys/labels are unique.
        let keys: Vec<&str> = DbEngine::ALL.iter().map(|e| e.key()).collect();
        assert_eq!(keys, vec!["mysql", "mariadb", "postgres", "redis"]);
    }

    #[test]
    fn version_series_and_datadirs_are_per_series() {
        // Series keys: PG by major (its datadirs are major-incompatible),
        // the MySQL-protocol engines by major.minor.
        assert_eq!(DbEngine::Postgres.series_of("18.4.0"), "18");
        assert_eq!(DbEngine::Postgres.series_of("17.10.0"), "17");
        assert_eq!(DbEngine::Mysql.series_of("8.0.44"), "8.0");
        assert_eq!(DbEngine::Mysql.series_of("8.4.6"), "8.4");
        assert_eq!(DbEngine::Mariadb.series_of("11.4.12"), "11.4");

        let plat = crate::platform::current();
        // The default series keeps the LEGACY path — existing data never moves.
        let default_dir = DbEngine::Mysql
            .data_dir(&*plat, DbEngine::Mysql.default_version())
            .unwrap();
        assert!(default_dir.ends_with("mysql/data"), "{}", default_dir.display());
        // A non-default series gets its own dir under <engine>/<series>/data.
        let lts = DbEngine::Mysql.data_dir(&*plat, "8.0.44").unwrap();
        assert!(lts.ends_with("mysql/8.0/data"), "{}", lts.display());
        assert_ne!(default_dir, lts);
        let pg17 = DbEngine::Postgres.data_dir(&*plat, "17.10.0").unwrap();
        assert!(pg17.ends_with("postgres/17/data"), "{}", pg17.display());
    }

    #[test]
    fn effective_version_persists_validates_and_falls_back() {
        let conn = crate::state::db::open_in_memory().unwrap();
        // Unset → the default pin.
        assert_eq!(
            DbEngine::Postgres.effective_version(&conn),
            DbEngine::Postgres.default_version()
        );
        // Stored + offered → the selection.
        DbEngine::Postgres.set_version(&conn, "17.10.0").unwrap();
        assert_eq!(DbEngine::Postgres.effective_version(&conn), "17.10.0");
        // Not offered → refused at write.
        assert!(DbEngine::Postgres.set_version(&conn, "15.0.0").is_err());
        // A selection orphaned by a future pin bump falls back to the default
        // (simulated by writing the raw setting directly).
        crate::state::store::set_setting(&conn, "db_version_postgres", "9.9.9").unwrap();
        assert_eq!(
            DbEngine::Postgres.effective_version(&conn),
            DbEngine::Postgres.default_version()
        );
    }

    #[test]
    fn running_false_on_closed_port() {
        // Nothing should be listening on a DB port during a unit test run.
        assert!(!DbEngine::Postgres.running());
    }

    #[test]
    fn availability_versions_and_key_lookup() {
        // Implemented engines are available + carry a pinned version.
        for e in DbEngine::ALL {
            assert!(e.available() && !e.default_version().is_empty());
            // The default is offered, first in the set.
            assert_eq!(e.versions().first(), Some(&e.default_version()));
        }
        // Key round-trips.
        assert_eq!(DbEngine::from_key("postgres"), Some(DbEngine::Postgres));
        assert_eq!(DbEngine::from_key("nope"), None);
    }
}

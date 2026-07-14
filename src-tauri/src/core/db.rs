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
use std::path::PathBuf;
use std::process::Child;

/// Loopback ports for the database services. Each is non-default so it doesn't
/// clash with a system install (MySQL 3306 / Postgres 5432 / Redis 6379).
pub const MARIADB_PORT: u16 = 13307;
pub const POSTGRES_PORT: u16 = 15432;
pub const REDIS_PORT: u16 = 16379;

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

    /// The pinned version of the engine.
    pub fn version(&self) -> &'static str {
        match self {
            DbEngine::Mysql => binaries::MYSQL_VERSION,
            DbEngine::Mariadb => binaries::MARIADB_VERSION,
            DbEngine::Postgres => binaries::POSTGRES_VERSION,
            DbEngine::Redis => binaries::REDIS_VERSION,
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
    /// never a PATH client). Only the site-capable engines have them.
    pub async fn sql_client_bins(&self, platform: &dyn Platform) -> Result<(PathBuf, PathBuf)> {
        match self {
            DbEngine::Mysql => {
                let base =
                    binaries::resolve_dir(platform, "mysql", binaries::MYSQL_VERSION).await?;
                Ok((base.join("bin/mysql"), base.join("bin/mysqldump")))
            }
            DbEngine::Mariadb => {
                let base =
                    binaries::resolve_bundle(platform, "mariadb", binaries::MARIADB_VERSION)
                        .await?;
                Ok((mariadb::mariadb_client_bin(&base), mariadb::mariadb_dump_bin(&base)))
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
    pub fn cached_sql_client(&self, platform: &dyn Platform) -> Option<PathBuf> {
        let bin_dir = platform.paths().bin_dir().ok()?;
        let client = match self {
            DbEngine::Mysql => bin_dir
                .join(format!("mysql-{}", binaries::MYSQL_VERSION))
                .join("bin/mysql"),
            DbEngine::Mariadb => bin_dir
                .join(format!("mariadb-{}", binaries::MARIADB_VERSION))
                .join("bin/mariadb"),
            _ => return None,
        };
        client.is_file().then_some(client)
    }

    /// Whether this engine's datadir was ever initialized — the delete-site
    /// guard ("no datadir ⇒ no database can exist ⇒ nothing to drop").
    pub fn datadir_initialized(&self, platform: &dyn Platform) -> bool {
        match self {
            DbEngine::Mysql => database::data_dir(platform)
                .map(|d| database::is_initialized(&d))
                .unwrap_or(false),
            DbEngine::Mariadb => mariadb::data_dir(platform)
                .map(|d| mariadb::is_initialized(&d))
                .unwrap_or(false),
            _ => false,
        }
    }

    /// Resolve the binary, initialize its data dir if needed, and start the
    /// server (foreground, supervised) on its port. Returns the child handle.
    /// MySQL delegates to `core::database`; the others land in §5.2–§5.4.
    pub async fn start(&self, platform: &dyn Platform) -> Result<Child> {
        match self {
            DbEngine::Mysql => {
                let basedir =
                    binaries::resolve_dir(platform, "mysql", binaries::MYSQL_VERSION).await?;
                let datadir = database::data_dir(platform)?;
                let socket = database::socket_path(platform)?;
                if let Some(parent) = socket.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                database::initialize(platform, &basedir, &datadir)?;
                database::start(platform, &basedir, &datadir, self.port(), &socket)
            }
            DbEngine::Postgres => {
                let basedir =
                    binaries::resolve_dir(platform, "postgres", binaries::POSTGRES_VERSION).await?;
                let datadir = postgres::data_dir(platform)?;
                postgres::initialize(platform, &basedir, &datadir)?;
                postgres::start(platform, &basedir, &datadir, self.port())
            }
            DbEngine::Mariadb => {
                let basedir =
                    binaries::resolve_bundle(platform, "mariadb", binaries::MARIADB_VERSION)
                        .await?;
                let datadir = mariadb::data_dir(platform)?;
                let socket = mariadb::socket_path(platform)?;
                mariadb::initialize(platform, &basedir, &datadir)?;
                mariadb::start(platform, &basedir, &datadir, self.port(), &socket)
            }
            DbEngine::Redis => {
                let basedir =
                    binaries::resolve_bundle(platform, "redis", binaries::REDIS_VERSION).await?;
                let datadir = redis::data_dir(platform)?;
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
    fn running_false_on_closed_port() {
        // Nothing should be listening on a DB port during a unit test run.
        assert!(!DbEngine::Postgres.running());
    }

    #[test]
    fn availability_versions_and_key_lookup() {
        // Implemented engines are available + carry a pinned version.
        assert!(DbEngine::Mysql.available() && !DbEngine::Mysql.version().is_empty());
        assert!(DbEngine::Postgres.available() && !DbEngine::Postgres.version().is_empty());
        assert!(DbEngine::Redis.available() && !DbEngine::Redis.version().is_empty());
        assert!(DbEngine::Mariadb.available() && !DbEngine::Mariadb.version().is_empty());
        // Key round-trips.
        assert_eq!(DbEngine::from_key("postgres"), Some(DbEngine::Postgres));
        assert_eq!(DbEngine::from_key("nope"), None);
    }
}

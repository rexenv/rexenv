//! core::db — unified database-engine lifecycle (Phase 2 §5).
//!
//! One `DbEngine` enum models every built-in database service behind a single
//! shape: a known loopback port, binary resolution + init-if-needed, start/stop
//! via `ProcessSupervisor::spawn_logged`, and a TCP running-probe. MySQL (the
//! Phase-1 service) is expressed through it by delegating to `core::database`, so
//! its behavior is unchanged; MariaDB / PostgreSQL / Redis delegate to their own
//! modules the same way.

use crate::core::proc::Proc;
use crate::core::{binaries, database, mariadb, ports, postgres, redis};
use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use crate::state::models::SiteDbEngine;
use rusqlite::Connection;
use std::path::{Path, PathBuf};

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
pub struct SqlClient {
    path: PathBuf,
    engine: DbEngine,
}

impl SqlClient {
    /// The binary's path — for spawning and for error messages.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The engine this client speaks to. Every site-DB operation dispatches on
    /// THIS, not on the caller's engine argument: since PostgreSQL joined the
    /// site engines the path alone no longer says which SQL dialect and which
    /// flags the binary accepts, and `psql` handed to `mysql_exec` would fail
    /// somewhere inside a subprocess with a message about neither. One field,
    /// one place to catch the mismatch, instead of eight call sites each
    /// trusting an argument.
    pub fn engine(&self) -> DbEngine {
        self.engine
    }

    /// Test-only constructor (fixture clients at nonexistent paths, refusal
    /// tests). Everything that runs for real goes through
    /// [`DbEngine::sql_client_bins`] / [`DbEngine::cached_sql_client`].
    #[cfg(test)]
    pub(crate) fn test_at(path: impl Into<PathBuf>) -> SqlClient {
        SqlClient::test_at_engine(path, DbEngine::Mysql)
    }

    /// Test-only constructor naming the engine (the dispatch tests).
    #[cfg(test)]
    pub(crate) fn test_at_engine(path: impl Into<PathBuf>, engine: DbEngine) -> SqlClient {
        SqlClient { path: path.into(), engine }
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
            DbEngine::Mysql => binaries::pins().mysql,
            DbEngine::Mariadb => binaries::pins().mariadb,
            DbEngine::Postgres => binaries::pins().postgres,
            DbEngine::Redis => binaries::pins().redis,
        }
    }

    /// All versions the engine can run (per-engine version switch), default
    /// first. A one-entry set means the UI hides the picker.
    pub fn versions(&self) -> &'static [&'static str] {
        match self {
            DbEngine::Mysql => binaries::pins().mysql_versions,
            DbEngine::Mariadb => binaries::pins().mariadb_versions,
            DbEngine::Postgres => binaries::pins().postgres_versions,
            DbEngine::Redis => binaries::pins().redis_versions,
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
    /// Whether this engine ships on THIS OS — **the one filter every surface already goes
    /// through**: the Databases page, Services' rows, the reserved-port list, adoption, the
    /// download plan, the MCP read context and `start_database`'s refusal. An engine that is
    /// not available here is therefore not offered, not adopted, not prefetched and not
    /// started, from one answer.
    ///
    /// The answer is the PINS, not a list kept here (`binaries::ships_on`): D4 leaves Redis
    /// out of Windows v1 because there is no official build, and MariaDB because there is no
    /// Windows pin and MySQL 8.4/8.0 both ship there. Both facts live in the manifest tables
    /// already, and a second copy of them here is exactly the drift this project has paid for
    /// before — so the day a Redis pin lands, the engine appears without anyone editing this
    /// function (W10, ledger #642).
    pub fn available(&self) -> bool {
        self.available_on(std::env::consts::OS)
    }

    /// [`Self::available`] for a NAMED os.
    ///
    /// The os is a parameter for the same reason `binaries::manifest` and `shape_of_on` take
    /// one: otherwise this gate's Windows answer could only be measured ON Windows, and the
    /// bar runs `cargo check` there, not `cargo test` (W12 is when a Windows runner arrives).
    /// A guard whose interesting half cannot fail on the machine that runs it is not a guard —
    /// so both answers are asked of the pins from either host (W10, ledger #642).
    pub fn available_on(&self, os: &str) -> bool {
        self.versions().iter().any(|v| binaries::ships_on(self.key(), v, os))
    }

    /// Why this engine is not offered on THIS host when the reason is the host's
    /// macOS — the tier has no build that loads here but a newer macOS does
    /// (`docs/PLAN-macos-13-floor.md` §6.3). `None` when offered, or when the
    /// absence is the platform's (Windows v1 has no Redis pin — a different
    /// sentence, [`Self::ensure_available_on`] owns both).
    pub fn unavailable_reason(&self) -> Option<String> {
        binaries::engine_needs_macos(self.key()).map(binaries::needs_macos_sentence)
    }

    /// Refuse an engine this build cannot run, with the RIGHT sentence: the host's
    /// macOS when that is the reason (the tier), else the platform's. ONE door for
    /// `start_database`, the version switch, `spawn_db` and site creation — the
    /// first macOS 13 run (T7, 23 Sep 2026) got "PostgreSQL is not available on
    /// this platform yet" from `rex db versions --set`, the platform sentence on
    /// a tier refusal, because that command had its own copy of the gate.
    pub fn ensure_available_on(&self, os: &str) -> Result<()> {
        if self.available_on(os) {
            Ok(())
        } else if let Some(reason) = self.unavailable_reason() {
            Err(Error::Other(format!("{}: {reason}", self.label())))
        } else {
            Err(Error::Other(format!("{} is not available on this platform yet", self.label())))
        }
    }

    /// [`Self::ensure_available_on`] for the host.
    pub fn ensure_available(&self) -> Result<()> {
        self.ensure_available_on(std::env::consts::OS)
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
            SiteDbEngine::Postgres => DbEngine::Postgres,
        }
    }

    /// Whether this engine speaks the MySQL wire protocol — i.e. takes the
    /// `client_base_args` flags, MySQL's `CREATE USER … IDENTIFIED BY` / `GRANT`
    /// syntax, and the mysqldump family.
    ///
    /// Exists because "site engine" and "MySQL-protocol engine" stopped being
    /// the same set when PostgreSQL arrived, and several paths mean the second
    /// while saying the first. Deleting a PostgreSQL site ran the agent-principal
    /// cleanup against its own client and got
    /// `psql: unrecognized option '--no-defaults'` — the MySQL flag array handed
    /// to psql, in a path whose SQL PostgreSQL could not have run either.
    pub fn is_mysql_family(&self) -> bool {
        matches!(self, DbEngine::Mysql | DbEngine::Mariadb)
    }

    /// Whether this engine can back a SITE (as opposed to running standalone on
    /// the Databases page). Redis cannot: it is not a SQL store and there is no
    /// `db_name` in it to create, dump or drop.
    pub fn hosts_site_databases(&self) -> bool {
        matches!(self, DbEngine::Mysql | DbEngine::Mariadb | DbEngine::Postgres)
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
                Ok((
                    SqlClient { path: database::mysql_client_bin(&base), engine: *self },
                    base.join("bin/mysqldump"),
                ))
            }
            DbEngine::Mariadb => {
                let base = binaries::resolve_bundle(platform, "mariadb", version).await?;
                Ok((
                    SqlClient { path: mariadb::mariadb_client_bin(&base), engine: *self },
                    mariadb::mariadb_dump_bin(&base),
                ))
            }
            DbEngine::Postgres => {
                let base = binaries::resolve_dir(platform, "postgres", version).await?;
                Ok((
                    SqlClient { path: postgres::psql_bin(&base), engine: *self },
                    postgres::pg_dump_bin(&base),
                ))
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
            DbEngine::Postgres => {
                postgres::psql_bin(&bin_dir.join(format!("postgres-{version}")))
            }
            DbEngine::Redis => return None,
        };
        client
            .is_file()
            .then_some(SqlClient { path: client, engine: *self })
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
            DbEngine::Postgres => postgres::is_initialized(&datadir),
            DbEngine::Redis => false,
        }
    }

    /// Resolve the binary for `version`, initialize that version-series'
    /// datadir if needed, and start the server (foreground, supervised) on the
    /// engine's port. Returns the child handle.
    pub async fn start(&self, platform: &dyn Platform, version: &str) -> Result<Proc> {
        let datadir = self.data_dir(platform, version)?;
        match self {
            DbEngine::Mysql => {
                let basedir = binaries::resolve_dir(platform, "mysql", version).await?;
                let socket = database::socket_path(platform)?;
                if let Some(parent) = socket.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                database::initialize(platform, &basedir, &datadir)?;
                database::start(platform, &basedir, &datadir, self.port(), &socket).map(Proc::from)
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
                mariadb::start(platform, &basedir, &datadir, self.port(), &socket).map(Proc::from)
            }
            DbEngine::Redis => {
                let basedir = binaries::resolve_bundle(platform, "redis", version).await?;
                redis::start(platform, &basedir, &datadir, self.port()).map(Proc::from)
            }
        }
    }

    /// The SERVER binary this engine runs, if its version is already cached.
    ///
    /// Offline and non-resolving on purpose — [`cached_path`](binaries::cached_path)
    /// never downloads — because the one caller runs on a failure path, where
    /// fetching an artifact to explain why a spawn failed would be absurd.
    ///
    /// It exists so a readiness timeout can name a macOS-version mismatch:
    /// PostgreSQL's pinned builds declare `minos 26.0` while rexenv's own floor
    /// is macOS 15, and "PostgreSQL did not start within 15s" sends the reader at
    /// Postgres rather than at their OS. See `core::macho`.
    pub fn server_binary(&self, platform: &dyn Platform, version: &str) -> Option<PathBuf> {
        let dir = binaries::cached_path(platform, self.binary_name(), version)?;
        Some(match self {
            DbEngine::Postgres => postgres::postgres_bin(&dir),
            // The others' cache entries already point at the member that runs.
            _ => dir,
        })
    }

    /// The `binaries` catalog name for this engine's server artifact.
    fn binary_name(&self) -> &'static str {
        match self {
            DbEngine::Mysql => "mysql",
            DbEngine::Postgres => "postgres",
            DbEngine::Mariadb => "mariadb",
            DbEngine::Redis => "redis",
        }
    }

    /// Stop a running engine by pid.
    /// Stop the engine. `version` is what resolves PostgreSQL's `pg_ctl`, which
    /// is how a cluster started through it must be shut down (`postgres::stop`,
    /// ledger #698); every other engine ignores it. `None` is for a caller that
    /// does not know the selected version — an example's cleanup guard, which
    /// stops by pid what it started — and falls back to the supervisor's plain
    /// stop.
    pub fn stop(&self, platform: &dyn Platform, pid: u32, version: Option<&str>) -> Result<()> {
        if matches!(self, DbEngine::Postgres) {
            let basedir = version.and_then(|v| {
                platform
                    .paths()
                    .bin_dir()
                    .ok()
                    .map(|d| d.join(format!("postgres-{v}")))
                    .filter(|d| d.is_dir())
            });
            return postgres::stop(platform, basedir.as_deref(), pid);
        }
        platform.supervisor().stop(pid)
    }

    /// True if the engine is accepting connections on its loopback port.
    pub fn running(&self) -> bool {
        ports::is_listening(self.port())
    }

    // --- site-database operations ------------------------------------------
    //
    // The five things rexenv does to a SITE's database. They were free
    // functions in `core::database` called by name from eight places, which was
    // honest while every site engine spoke the MySQL protocol and MariaDB was a
    // different path to the same client. PostgreSQL is the first site engine
    // with its own client, dump tool and DDL (docs/archive/PLAN-postgres-sites.md §2),
    // so the choice moves here — beside `start`, `data_dir` and `server_binary`,
    // which already dispatch — and the callers stop naming an engine's module.
    //
    // The PORT stays a parameter, deliberately. `self.port()` is the engine's
    // production port, and the live examples run their own fixture server on a
    // private one (13396-13399) precisely so a check can never touch the
    // owner's real databases — folding the port in here would have quietly
    // pointed every one of them at the real MySQL.

    /// The client must speak for THIS engine. Not reachable through the public
    /// constructors (both stamp the engine they resolved for) — it is the
    /// backstop that makes "dispatch on the client's engine" true rather than
    /// merely intended.
    fn expect_client(&self, client: &SqlClient) -> Result<()> {
        if client.engine() == *self {
            return Ok(());
        }
        Err(Error::Other(format!(
            "{} client used for a {} operation",
            client.engine().label(),
            self.label()
        )))
    }

    /// Create the site's database if it doesn't exist.
    pub fn create_database(&self, client: &SqlClient, port: u16, name: &str) -> Result<()> {
        self.expect_client(client)?;
        match self {
            DbEngine::Mysql | DbEngine::Mariadb => database::create_database(client, port, name),
            DbEngine::Postgres => postgres::create_database(client, port, name),
            DbEngine::Redis => Err(self.not_a_site_engine()),
        }
    }

    /// Drop the site's database if it exists (site teardown).
    pub fn drop_database(&self, client: &SqlClient, port: u16, name: &str) -> Result<()> {
        self.expect_client(client)?;
        match self {
            DbEngine::Mysql | DbEngine::Mariadb => database::drop_database(client, port, name),
            DbEngine::Postgres => postgres::drop_database(client, port, name),
            DbEngine::Redis => Err(self.not_a_site_engine()),
        }
    }

    /// Import a `.sql` dump into database `name`. DESTRUCTIVE — the caller owns
    /// the confirm/backup UX.
    pub fn import_from_file(
        &self,
        client: &SqlClient,
        port: u16,
        name: &str,
        file: &Path,
    ) -> Result<()> {
        self.expect_client(client)?;
        match self {
            DbEngine::Mysql | DbEngine::Mariadb => {
                database::import_from_file(client, port, name, file)
            }
            DbEngine::Postgres => postgres::import_from_file(client, port, name, file),
            DbEngine::Redis => Err(self.not_a_site_engine()),
        }
    }

    /// Export database `name` into the user's Downloads folder. `dump` is the
    /// dump BINARY from [`sql_client_bins`](Self::sql_client_bins) — a separate
    /// argument because it is a different executable from the client, in both
    /// engine families.
    pub fn export_to_downloads(
        &self,
        dump: &Path,
        port: u16,
        domain: &str,
        name: &str,
    ) -> Result<PathBuf> {
        match self {
            DbEngine::Mysql | DbEngine::Mariadb => {
                database::export_to_downloads(dump, port, domain, name)
            }
            DbEngine::Postgres => postgres::export_to_downloads(dump, port, domain, name),
            DbEngine::Redis => Err(self.not_a_site_engine()),
        }
    }

    /// Run one SQL script INSIDE `database` — the starter seed's one need, and
    /// the one place the two engines differ in a way a caller should not know
    /// about: MySQL selects the database with a `USE` statement prepended to the
    /// script, PostgreSQL by connecting to it. The script itself is the caller's
    /// (and is dialect-specific — see `core::starter`).
    pub fn exec_in_database(
        &self,
        client: &SqlClient,
        port: u16,
        database: &str,
        sql: &str,
        what: &str,
    ) -> Result<()> {
        self.expect_client(client)?;
        database::validate_db_name(database)?;
        match self {
            DbEngine::Mysql | DbEngine::Mariadb => {
                database::mysql_exec(client, port, &format!("USE `{database}`; {sql}"), what)
            }
            DbEngine::Postgres => postgres::psql_exec(client, port, database, sql, what),
            DbEngine::Redis => Err(self.not_a_site_engine()),
        }
    }

    /// Disk size of every database in this engine, in bytes.
    pub fn db_sizes(&self, client: &SqlClient, port: u16) -> Result<Vec<(String, u64)>> {
        self.expect_client(client)?;
        match self {
            DbEngine::Mysql | DbEngine::Mariadb => database::db_sizes(client, port),
            DbEngine::Postgres => postgres::db_sizes(client, port),
            DbEngine::Redis => Err(self.not_a_site_engine()),
        }
    }

    /// The refusal every site-DB op gives for an engine that hosts none — the
    /// same sentence `sql_client_bins` has always answered with, so a caller
    /// that reaches an op some other way reads the same explanation.
    fn not_a_site_engine(&self) -> Error {
        Error::Other(format!("{} does not host site databases", self.label()))
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
    fn a_site_op_refuses_a_client_from_another_engine() {
        // The dispatch rests on the client carrying its engine, so the mismatch
        // has to fail HERE, by name, rather than inside a subprocess that was
        // handed flags from the wrong vendor. Not reachable through the public
        // constructors — that is the point: this is the backstop that makes the
        // claim true rather than merely intended.
        let psql = SqlClient::test_at_engine("/nonexistent/psql", DbEngine::Postgres);
        let err = DbEngine::Mysql
            .create_database(&psql, 13306, "shop")
            .expect_err("a psql client must not run a MySQL create");
        let msg = err.to_string();
        assert!(msg.contains("PostgreSQL") && msg.contains("MySQL"), "{msg}");

        let my = SqlClient::test_at_engine("/nonexistent/mysql", DbEngine::Mysql);
        assert!(DbEngine::Postgres.drop_database(&my, 15432, "shop").is_err());
        assert!(DbEngine::Postgres.db_sizes(&my, 15432).is_err());
    }

    #[test]
    fn redis_hosts_no_site_database() {
        // Every site-DB op answers with the one sentence sql_client_bins has
        // always given, so a caller reaching an op some other way reads the same
        // explanation rather than a subprocess failure.
        let client = SqlClient::test_at_engine("/nonexistent", DbEngine::Redis);
        let err = DbEngine::Redis.create_database(&client, 16379, "shop").unwrap_err();
        assert!(err.to_string().contains("does not host site databases"), "{err}");
        assert!(!DbEngine::Redis.hosts_site_databases());
        for e in [DbEngine::Mysql, DbEngine::Mariadb, DbEngine::Postgres] {
            assert!(e.hosts_site_databases(), "{} must host site databases", e.key());
        }
    }

    #[test]
    fn a_sites_engine_maps_to_its_service() {
        use crate::state::models::SiteDbEngine;
        assert_eq!(DbEngine::from_site(SiteDbEngine::Mysql), DbEngine::Mysql);
        assert_eq!(DbEngine::from_site(SiteDbEngine::Mariadb), DbEngine::Mariadb);
        assert_eq!(DbEngine::from_site(SiteDbEngine::Postgres), DbEngine::Postgres);
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
        assert_eq!(DbEngine::Postgres.series_of("18.6.0"), "18");
        assert_eq!(DbEngine::Postgres.series_of("17.11.0"), "17");
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
        let pg17 = DbEngine::Postgres.data_dir(&*plat, "17.11.0").unwrap();
        assert!(pg17.ends_with("postgres/17/data"), "{}", pg17.display());
    }

    /// Ledger #642 — **the engine gate reads the pins, per OS.** MySQL and PostgreSQL ship
    /// on both; Redis (no official Windows build) and MariaDB (no Windows pin — MySQL 8.4
    /// and 8.0 both ship there, so D4 left it out of v1) ship only on macOS. Asked for BOTH
    /// os values from either host: the bar only `cargo check`s for Windows, so a gate read
    /// from `std::env::consts::OS` alone would have a half nobody could fail.
    #[test]
    fn an_engine_is_available_where_its_pins_are() {
        for e in DbEngine::ALL {
            let by_pins = e.versions().iter().any(|v| binaries::ships_on(e.key(), v, "macos"));
            assert!(by_pins, "{} has no macOS pin", e.key());
            assert!(e.available_on("macos"), "{} must be available on macOS", e.key());
        }
        for e in [DbEngine::Mysql, DbEngine::Postgres] {
            assert!(e.available_on("windows"), "{} ships on Windows", e.key());
        }
        for e in [DbEngine::Mariadb, DbEngine::Redis] {
            assert!(!e.available_on("windows"), "{} is not in Windows v1 (D4)", e.key());
        }
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
        DbEngine::Postgres.set_version(&conn, "17.11.0").unwrap();
        assert_eq!(DbEngine::Postgres.effective_version(&conn), "17.11.0");
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
    fn running_reads_a_port_rather_than_a_flag() {
        // This asserted `!DbEngine::Postgres.running()` — "nothing should be
        // listening on a DB port during a unit test run" — which was true only
        // while PostgreSQL was an engine nobody actually ran. A PostgreSQL-backed
        // site keeps it up, so the test failed on a correct machine (10 Sep
        // 2026): it was asserting the developer's environment, not the code.
        //
        // What it can honestly prove is the probe's direction, on a port that
        // cannot be in use. Everything else about `running()` — that it reads a
        // live port instead of a stored flag — is settled by its one line.
        assert!(!ports::is_listening(9), "the discard port is not a listener");
    }

    #[test]
    fn availability_versions_and_key_lookup() {
        // Implemented engines are available + carry a pinned version — asked OF macOS,
        // which is where all four ship. Windows has MySQL and PostgreSQL only (D4), so the
        // host-reading form made this a claim about the machine running it (W12).
        for e in DbEngine::ALL {
            assert!(e.available_on("macos") && !e.default_version().is_empty());
            // The default is offered, first in the set.
            assert_eq!(e.versions().first(), Some(&e.default_version()));
        }
        // Key round-trips.
        assert_eq!(DbEngine::from_key("postgres"), Some(DbEngine::Postgres));
        assert_eq!(DbEngine::from_key("nope"), None);
    }
}

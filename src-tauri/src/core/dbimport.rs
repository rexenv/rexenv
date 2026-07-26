//! core::dbimport — reading a site's OWN database connection settings, and the
//! vocabulary for what we found (Stage 2, `docs/PLAN-valet-herd-db-import.md`).
//!
//! The source environment is strictly read-only: this module opens their config
//! files as text and nothing else. Parsing lives in [`crate::core::phpconf`] —
//! one reader, shared with the Logs tab.
//!
//! **Credentials never leave memory.** [`DbConnection`] carries the password for
//! the length of a job and is deliberately hard to leak: its `Debug` redacts,
//! it is not `Serialize`, and the IPC-facing [`DbConnectionInfo`] simply has no
//! password field to forget to strip. What reaches the UI can't contain a
//! secret, because the type can't hold one.

use crate::core::phpconf::{self, Unreadable};
use std::path::{Path, PathBuf};

/// Which server family a site's config points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Driver {
    /// MySQL or MariaDB — the same wire protocol; which one it really is comes
    /// from the server's own handshake, never from the site's config.
    MysqlFamily,
    Postgres,
    Sqlite,
}

impl Driver {
    /// Laravel's `DB_CONNECTION` values.
    fn from_dotenv(v: &str) -> Option<Driver> {
        match v.trim().to_ascii_lowercase().as_str() {
            "mysql" | "mariadb" => Some(Driver::MysqlFamily),
            "pgsql" | "postgres" | "postgresql" => Some(Driver::Postgres),
            "sqlite" => Some(Driver::Sqlite),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Driver::MysqlFamily => "MySQL/MariaDB",
            Driver::Postgres => "PostgreSQL",
            Driver::Sqlite => "SQLite",
        }
    }
}

/// Where the settings came from — named in the UI so the user can check us.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConfigSource {
    WpConfig { path: String },
    DotEnv { path: String },
}

impl ConfigSource {
    pub fn path(&self) -> &str {
        match self {
            ConfigSource::WpConfig { path } | ConfigSource::DotEnv { path } => path,
        }
    }
}

/// A site's database connection, as its own config states it.
///
/// Not `Serialize`, and `Debug` is written by hand: the password must not reach
/// a log, an event payload, or an error string by accident. See
/// [`DbConnectionInfo`] for the shape the UI gets.
#[derive(Clone, PartialEq, Eq)]
pub struct DbConnection {
    pub driver: Driver,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password: String,
    /// WordPress only.
    pub table_prefix: Option<String>,
    pub source: ConfigSource,
}

impl std::fmt::Debug for DbConnection {
    /// Redacts the password. A derived `Debug` would put it in every `{:?}`,
    /// and `{:?}` is exactly what ends up in an error message someone logs.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DbConnection")
            .field("driver", &self.driver)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("database", &self.database)
            .field("user", &self.user)
            .field("password", &if self.password.is_empty() { "<none>" } else { "<redacted>" })
            .field("table_prefix", &self.table_prefix)
            .field("source", &self.source)
            .finish()
    }
}

/// The UI's view of a connection: everything except the secret, which this type
/// cannot express.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbConnectionInfo {
    pub driver: Driver,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    /// Whether a password is set — never the password.
    pub has_password: bool,
    pub table_prefix: Option<String>,
    pub source: ConfigSource,
}

impl DbConnection {
    pub fn info(&self) -> DbConnectionInfo {
        DbConnectionInfo {
            driver: self.driver,
            host: self.host.clone(),
            port: self.port,
            database: self.database.clone(),
            user: self.user.clone(),
            has_password: !self.password.is_empty(),
            table_prefix: self.table_prefix.clone(),
            source: self.source.clone(),
        }
    }
}

/// What we know about a site's database, as one closed vocabulary.
///
/// Each variant is separately actionable, and the ones that look alike from a
/// distance are deliberately NOT collapsed:
///
/// - [`Unreadable`](DbSiteStatus::NeedsAttention) — we couldn't read the config
///   confidently. Nothing was contacted.
/// - [`ServerUnreachable`](DbSiteStatus::ServerUnreachable) — the config is
///   clear, nothing is listening there.
/// - [`CredentialsRejected`](DbSiteStatus::CredentialsRejected) — something is
///   listening and it said no.
/// - [`DatabaseMissing`](DbSiteStatus::DatabaseMissing) — we connected fine and
///   the database the site names is not on that server. Usually a database they
///   dropped, or a site pointed at a different engine than the one running.
/// - [`Ready`](DbSiteStatus::Ready) — there is something to dump.
///
/// The parse-side variants are produced here; the live ones are filled in by the
/// preflight (step 3/5) using this same vocabulary, so there is one set of
/// states and one set of words for them.
#[derive(Debug, Clone)]
pub enum DbSiteStatus {
    /// The site has no database configuration at all — a plain PHP project.
    NoDatabase,
    /// We declined to guess. Carries the shape we saw.
    NeedsAttention { reason: Unreadable, source: Option<ConfigSource> },
    /// A driver we can read but can't import (yet).
    UnsupportedDriver { driver: Driver, conn: Box<DbConnectionInfo> },
    ServerUnreachable { conn: Box<DbConnectionInfo> },
    CredentialsRejected { conn: Box<DbConnectionInfo>, detail: String },
    DatabaseMissing {
        conn: Box<DbConnectionInfo>,
        /// What the server DOES have, so the message can be useful rather than
        /// just negative (empty when we couldn't list).
        available: Vec<String>,
        /// The server's own version string from the handshake, e.g. "8.0.27".
        server: Option<String>,
    },
    Ready { conn: Box<DbConnectionInfo>, server: Option<String>, size_bytes: Option<u64> },
}

impl DbSiteStatus {
    /// A short label for the row.
    pub fn label(&self) -> &'static str {
        match self {
            DbSiteStatus::NoDatabase => "no database",
            DbSiteStatus::NeedsAttention { .. } => "needs attention",
            DbSiteStatus::UnsupportedDriver { .. } => "not supported yet",
            DbSiteStatus::ServerUnreachable { .. } => "database not reachable",
            DbSiteStatus::CredentialsRejected { .. } => "sign-in refused",
            DbSiteStatus::DatabaseMissing { .. } => "database not found",
            DbSiteStatus::Ready { .. } => "ready to import",
        }
    }

    /// The sentence under the row. States what we saw and what to do about it —
    /// never blames the project, and never implies a failure on our side that
    /// the user can't act on.
    pub fn message(&self) -> String {
        match self {
            DbSiteStatus::NoDatabase => {
                "This site doesn't have database settings rexenv can read, so there's \
                 nothing to import — the site itself imports normally."
                    .into()
            }
            DbSiteStatus::NeedsAttention { reason, source } => match source {
                Some(s) => format!("{} (read from {})", reason.message(), s.path()),
                None => reason.message(),
            },
            DbSiteStatus::UnsupportedDriver { driver, conn } => format!(
                "{} detected at {}:{} (`{}`). rexenv can't import {} databases yet — \
                 MySQL and MariaDB only for now. Everything else about this site imports \
                 normally.",
                driver.label(),
                conn.host,
                conn.port,
                conn.database,
                driver.label()
            ),
            DbSiteStatus::ServerUnreachable { conn } => format!(
                "`{}` is on a database server at {}:{}, and nothing is listening there. \
                 Start it (DBngin, Herd, or however you run it), then re-scan — rexenv \
                 never starts or stops your database server.",
                conn.database, conn.host, conn.port
            ),
            DbSiteStatus::CredentialsRejected { conn, detail } => format!(
                "The server at {}:{} refused the sign-in this site's config uses \
                 (user `{}`): {detail}",
                conn.host, conn.port, conn.user
            ),
            DbSiteStatus::DatabaseMissing { conn, available, server } => {
                let which = server
                    .as_deref()
                    .map(|v| format!("The {v} server"))
                    .unwrap_or_else(|| "The server".into());
                let mut m = format!(
                    "{} at {}:{} is running and accepted the sign-in, but has no database \
                     called `{}`.",
                    which, conn.host, conn.port, conn.database
                );
                if available.is_empty() {
                    m.push_str(" It has no user databases at all — this is usually a site \
                                pointed at a different server than the one that's running.");
                } else {
                    let shown: Vec<&str> =
                        available.iter().take(6).map(|s| s.as_str()).collect();
                    m.push_str(&format!(
                        " It does have: {}{}. Either the database was deleted, or this site \
                         points at a different server than the one running here.",
                        shown.join(", "),
                        if available.len() > shown.len() {
                            format!(" (+{} more)", available.len() - shown.len())
                        } else {
                            String::new()
                        }
                    ));
                }
                m
            }
            DbSiteStatus::Ready { conn, server, size_bytes } => {
                let mut m = format!("`{}` on ", conn.database);
                m.push_str(&match server {
                    Some(v) => format!("{v} at {}:{}", conn.host, conn.port),
                    None => format!("{}:{}", conn.host, conn.port),
                });
                if let Some(b) = size_bytes {
                    m.push_str(&format!(" ({})", human_bytes(*b)));
                }
                m.push('.');
                m
            }
        }
    }

    /// The connection behind this status, where there is one.
    pub fn connection(&self) -> Option<&DbConnectionInfo> {
        match self {
            DbSiteStatus::NoDatabase | DbSiteStatus::NeedsAttention { .. } => None,
            DbSiteStatus::UnsupportedDriver { conn, .. }
            | DbSiteStatus::ServerUnreachable { conn }
            | DbSiteStatus::CredentialsRejected { conn, .. }
            | DbSiteStatus::DatabaseMissing { conn, .. }
            | DbSiteStatus::Ready { conn, .. } => Some(conn),
        }
    }
}

fn human_bytes(b: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = b as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{b} {}", UNITS[0])
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

/// Read a site's connection settings from the project at `docroot`.
///
/// Order: `wp-config.php` first (it is unambiguous when present), then `.env`.
/// A WordPress config whose defines are computed (Bedrock) falls through to the
/// `.env` beside it rather than becoming a refusal, because that is where the
/// answer actually lives.
pub fn read_connection(docroot: &Path) -> Result<DbConnection, (Unreadable, Option<ConfigSource>)> {
    let wp = phpconf::wp_config_path(docroot);
    let env = dotenv_path(docroot);

    if let Some(path) = &wp {
        let text = std::fs::read_to_string(path)
            .map_err(|_| (Unreadable::NoConfigFile, None))?;
        let source = ConfigSource::WpConfig { path: path.display().to_string() };
        match read_wp(&text, source.clone()) {
            Ok(c) => return Ok(c),
            // Bedrock and friends: the defines exist but are computed from the
            // environment, so the .env beside them is the real source.
            Err(e @ Unreadable::NonLiteral { .. }) | Err(e @ Unreadable::MissingKey { .. })
                if env.is_some() =>
            {
                let _ = e;
            }
            Err(e) => return Err((e, Some(source))),
        }
    }

    let Some(path) = env else {
        return Err((Unreadable::NoConfigFile, None));
    };
    let text = std::fs::read_to_string(&path).map_err(|_| (Unreadable::NoConfigFile, None))?;
    let source = ConfigSource::DotEnv { path: path.display().to_string() };
    read_dotenv(&text, source.clone()).map_err(|e| (e, Some(source)))
}

/// A Laravel/Bedrock `.env` sits at the project root, which for a framework is
/// the PARENT of the docroot we serve (`public/`, `web/`).
fn dotenv_path(docroot: &Path) -> Option<PathBuf> {
    let here = docroot.join(".env");
    if here.is_file() {
        return Some(here);
    }
    let above = docroot.parent()?.join(".env");
    above.is_file().then_some(above)
}

/// WordPress: the four `DB_*` defines plus `$table_prefix`.
pub fn read_wp(text: &str, source: ConfigSource) -> Result<DbConnection, Unreadable> {
    let database = phpconf::wp_define_str(text, "DB_NAME")?;
    let user = phpconf::wp_define_str(text, "DB_USER")?;
    // An empty password is a real configuration, not a missing one — but a
    // MISSING define is ambiguous, so it still refuses.
    let password = phpconf::wp_define_str(text, "DB_PASSWORD")?;
    let host_raw = phpconf::wp_define_str(text, "DB_HOST")?;
    let (host, port) = split_host_port(&host_raw, 3306)?;
    Ok(DbConnection {
        driver: Driver::MysqlFamily,
        host,
        port,
        database,
        user,
        password,
        table_prefix: phpconf::wp_table_prefix(text),
        source,
    })
}

/// Laravel-style `.env`.
pub fn read_dotenv(text: &str, source: ConfigSource) -> Result<DbConnection, Unreadable> {
    // An absent DB_CONNECTION means Laravel's default, which is mysql.
    let driver = match phpconf::dotenv_value(text, "DB_CONNECTION") {
        Ok(v) => Driver::from_dotenv(&v)
            .ok_or(Unreadable::NonLiteral { key: "DB_CONNECTION".into(), saw: v })?,
        Err(Unreadable::MissingKey { .. }) => Driver::MysqlFamily,
        Err(e) => return Err(e),
    };
    let database = phpconf::dotenv_value(text, "DB_DATABASE")?;
    if matches!(driver, Driver::Sqlite) {
        // SQLite's "database" is a file path; there is no server to talk to.
        return Ok(DbConnection {
            driver,
            host: String::new(),
            port: 0,
            database,
            user: String::new(),
            password: String::new(),
            table_prefix: None,
            source,
        });
    }
    let host = phpconf::dotenv_value(text, "DB_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let default_port = if matches!(driver, Driver::Postgres) { 5432 } else { 3306 };
    let port = match phpconf::dotenv_value(text, "DB_PORT") {
        Ok(p) => p
            .trim()
            .parse::<u16>()
            .map_err(|_| Unreadable::NonLiteral { key: "DB_PORT".into(), saw: p })?,
        Err(Unreadable::MissingKey { .. }) => default_port,
        Err(e) => return Err(e),
    };
    let user = phpconf::dotenv_value(text, "DB_USERNAME")?;
    let password = match phpconf::dotenv_value(text, "DB_PASSWORD") {
        Ok(p) => p,
        // Laravel ships `DB_PASSWORD=` empty; an absent key means the same
        // thing here, unlike wp-config where the define is always written.
        Err(Unreadable::MissingKey { .. }) => String::new(),
        Err(e) => return Err(e),
    };
    // `DB_HOST` may still carry a port (host:port) even with DB_PORT set.
    let (host, port) = split_host_port(&host, port)?;
    Ok(DbConnection { driver, host, port, database, user, password, table_prefix: None, source })
}

/// `127.0.0.1`, `127.0.0.1:3307`, `localhost`, or a socket path. WordPress
/// packs the port into `DB_HOST`, which is why this exists.
///
/// A `:` followed by something that isn't a port (WordPress also accepts
/// `host:/path/to/socket`) is not guessed at.
fn split_host_port(raw: &str, default_port: u16) -> Result<(String, u16), Unreadable> {
    let raw = raw.trim();
    let Some((host, tail)) = raw.rsplit_once(':') else {
        return Ok((raw.to_string(), default_port));
    };
    match tail.parse::<u16>() {
        Ok(p) => Ok((host.to_string(), p)),
        Err(_) => Err(Unreadable::NonLiteral { key: "DB_HOST".into(), saw: raw.to_string() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wp_src() -> ConfigSource {
        ConfigSource::WpConfig { path: "/x/wp-config.php".into() }
    }
    fn env_src() -> ConfigSource {
        ConfigSource::DotEnv { path: "/x/.env".into() }
    }

    #[test]
    fn reads_the_live_sample_shape() {
        // The real `ea` site on this machine: root with a password, TCP host.
        let text = r#"<?php
define( 'DB_NAME', 'ea' );
define( 'DB_USER', 'root' );
define( 'DB_PASSWORD', 'hunter2' );
define( 'DB_HOST', '127.0.0.1' );
$table_prefix = 'wp_';
"#;
        let c = read_wp(text, wp_src()).unwrap();
        assert_eq!(c.database, "ea");
        assert_eq!(c.user, "root");
        assert_eq!((c.host.as_str(), c.port), ("127.0.0.1", 3306));
        assert_eq!(c.table_prefix.as_deref(), Some("wp_"));
        assert_eq!(c.driver, Driver::MysqlFamily);
    }

    #[test]
    fn a_port_packed_into_db_host_is_read() {
        let text = "<?php define('DB_NAME','x');define('DB_USER','u');\
                    define('DB_PASSWORD','p');define('DB_HOST','127.0.0.1:3307');";
        let c = read_wp(text, wp_src()).unwrap();
        assert_eq!((c.host.as_str(), c.port), ("127.0.0.1", 3307));
    }

    #[test]
    fn a_socket_style_db_host_is_refused_not_guessed() {
        let text = "<?php define('DB_NAME','x');define('DB_USER','u');\
                    define('DB_PASSWORD','p');define('DB_HOST','localhost:/tmp/mysql.sock');";
        assert!(matches!(read_wp(text, wp_src()), Err(Unreadable::NonLiteral { .. })));
    }

    #[test]
    fn the_password_never_appears_in_debug_output() {
        // Not a style preference: `{:?}` is what ends up in an error someone
        // logs, and this type is built to make that safe.
        let text = "<?php define('DB_NAME','x');define('DB_USER','u');\
                    define('DB_PASSWORD','sup3r-s3cret');define('DB_HOST','127.0.0.1');";
        let c = read_wp(text, wp_src()).unwrap();
        let printed = format!("{c:?}");
        assert!(!printed.contains("sup3r-s3cret"), "{printed}");
        assert!(printed.contains("<redacted>"));
        // And the UI shape simply cannot carry it.
        let info = c.info();
        assert!(info.has_password);
        assert!(!serde_json::to_string(&info).unwrap().contains("sup3r-s3cret"));
    }

    #[test]
    fn an_empty_password_is_a_real_setting_not_a_missing_one() {
        let text = "<?php define('DB_NAME','x');define('DB_USER','root');\
                    define('DB_PASSWORD','');define('DB_HOST','127.0.0.1');";
        let c = read_wp(text, wp_src()).unwrap();
        assert_eq!(c.password, "");
        assert!(!c.info().has_password);
    }

    #[test]
    fn laravel_defaults_are_applied_but_names_are_never_invented() {
        let env = "DB_CONNECTION=mysql\nDB_DATABASE=shop\nDB_USERNAME=shop_user\n";
        let c = read_dotenv(env, env_src()).unwrap();
        assert_eq!((c.host.as_str(), c.port), ("127.0.0.1", 3306));
        assert_eq!(c.password, "");
        // …but a missing DB_DATABASE is never defaulted to anything.
        assert!(matches!(
            read_dotenv("DB_CONNECTION=mysql\nDB_USERNAME=u\n", env_src()),
            Err(Unreadable::MissingKey { .. })
        ));
    }

    #[test]
    fn postgres_and_sqlite_are_read_then_declared_unsupported() {
        let pg = read_dotenv("DB_CONNECTION=pgsql\nDB_DATABASE=app\nDB_USERNAME=app\n", env_src())
            .unwrap();
        assert_eq!((pg.driver, pg.port), (Driver::Postgres, 5432));
        let status = DbSiteStatus::UnsupportedDriver {
            driver: pg.driver,
            conn: Box::new(pg.info()),
        };
        let m = status.message();
        assert!(m.contains("can't import PostgreSQL databases yet"), "{m}");
        assert!(m.contains("Everything else about this site imports normally"), "{m}");
        // Nothing implying their setup is broken.
        for blame in ["invalid", "unsupported configuration", "error"] {
            assert!(!m.to_lowercase().contains(blame), "{m}");
        }

        let lite =
            read_dotenv("DB_CONNECTION=sqlite\nDB_DATABASE=/x/database.sqlite\n", env_src())
                .unwrap();
        assert_eq!(lite.driver, Driver::Sqlite);
    }

    #[test]
    fn a_missing_database_reads_as_its_own_state_not_a_generic_failure() {
        // The three states that look alike from a distance must not collapse:
        // nothing listening, listening-but-refused, and connected-but-absent.
        let conn = Box::new(
            read_wp(
                "<?php define('DB_NAME','ea');define('DB_USER','root');\
                 define('DB_PASSWORD','p');define('DB_HOST','127.0.0.1');",
                wp_src(),
            )
            .unwrap()
            .info(),
        );
        let missing = DbSiteStatus::DatabaseMissing {
            conn: conn.clone(),
            available: vec!["ea_old".into(), "wordpress".into()],
            server: Some("8.0.27".into()),
        };
        let m = missing.message();
        assert!(m.contains("accepted the sign-in"), "{m}");
        assert!(m.contains("no database called `ea`"), "{m}");
        // Useful, not just negative: it names what IS there and both causes.
        assert!(m.contains("ea_old"), "{m}");
        assert!(m.contains("deleted") && m.contains("different server"), "{m}");
        assert_eq!(missing.label(), "database not found");

        let unreachable = DbSiteStatus::ServerUnreachable { conn: conn.clone() };
        assert_eq!(unreachable.label(), "database not reachable");
        assert!(unreachable.message().contains("never starts or stops"));

        let refused = DbSiteStatus::CredentialsRejected {
            conn,
            detail: "Access denied for user 'root'@'localhost'".into(),
        };
        assert_eq!(refused.label(), "sign-in refused");
        // Three distinct labels for three distinct situations.
        assert_ne!(missing.label(), unreachable.label());
        assert_ne!(refused.label(), unreachable.label());
    }

    #[test]
    fn an_empty_server_reads_differently_from_one_with_other_databases() {
        let conn = Box::new(DbConnectionInfo {
            driver: Driver::MysqlFamily,
            host: "127.0.0.1".into(),
            port: 3306,
            database: "ea".into(),
            user: "root".into(),
            has_password: true,
            table_prefix: None,
            source: wp_src(),
        });
        let empty = DbSiteStatus::DatabaseMissing {
            conn,
            available: vec![],
            server: None,
        };
        assert!(empty.message().contains("no user databases at all"));
    }

    #[test]
    fn bedrock_falls_through_to_the_env_beside_it() {
        let dir = std::env::temp_dir().join("rexenv-dbimport-bedrock");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("web")).unwrap();
        // Bedrock: computed defines in wp-config, real values in .env.
        std::fs::write(
            dir.join("web/wp-config.php"),
            "<?php define('DB_NAME', env('DB_NAME')); define('DB_USER', env('DB_USER'));",
        )
        .unwrap();
        std::fs::write(
            dir.join(".env"),
            "DB_DATABASE=bedrock\nDB_USERNAME=bud\nDB_PASSWORD=x\nDB_HOST=127.0.0.1\n",
        )
        .unwrap();
        let c = read_connection(&dir.join("web")).unwrap();
        assert_eq!(c.database, "bedrock");
        assert!(matches!(c.source, ConfigSource::DotEnv { .. }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_project_with_no_config_is_no_database_not_an_error() {
        let dir = std::env::temp_dir().join("rexenv-dbimport-plain");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.php"), "<?php echo 'hi';").unwrap();
        let err = read_connection(&dir).unwrap_err();
        assert_eq!(err.0, Unreadable::NoConfigFile);
        let status = DbSiteStatus::NoDatabase;
        assert!(status.message().contains("the site itself imports normally"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_ambiguous_wp_config_refuses_and_names_the_file() {
        let dir = std::env::temp_dir().join("rexenv-dbimport-ambiguous");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("wp-config.php"),
            "<?php\nif (X) { define('DB_NAME','a'); } else { define('DB_NAME','b'); }\n",
        )
        .unwrap();
        let (reason, source) = read_connection(&dir).unwrap_err();
        assert!(matches!(reason, Unreadable::DuplicateKey { .. }));
        let status = DbSiteStatus::NeedsAttention { reason, source };
        let m = status.message();
        assert!(m.contains("wp-config.php"), "{m}");
        assert!(m.contains("can't tell which one"), "{m}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

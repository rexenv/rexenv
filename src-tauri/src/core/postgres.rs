//! core::postgres — PostgreSQL service (Phase 2 §5.3).
//!
//! One shared PostgreSQL server on a loopback port. The datadir is initialized
//! once with `initdb` (trust auth, passwordless `postgres` superuser for local
//! dev), then `postgres` is supervised like the other services. `basedir` is the
//! extracted tree from `core::binaries::resolve_dir("postgres", …)`. We run
//! **TCP-only** (`unix_socket_directories=` empty) — our model connects over
//! `127.0.0.1:<port>`, and it sidesteps macOS's ~104-char Unix-socket path limit.

use crate::core::database::{dump_dest, validate_db_name};
use crate::core::db::SqlClient;
use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

/// `postgres` server executable inside the extracted tree.
pub fn postgres_bin(basedir: &Path) -> PathBuf {
    basedir.join("bin/postgres")
}

/// `initdb` executable inside the extracted tree.
pub fn initdb_bin(basedir: &Path) -> PathBuf {
    basedir.join("bin/initdb")
}

/// `psql` client inside the extracted tree.
pub fn psql_bin(basedir: &Path) -> PathBuf {
    basedir.join("bin/psql")
}

/// `pg_dump` executable inside the extracted tree.
pub fn pg_dump_bin(basedir: &Path) -> PathBuf {
    basedir.join("bin/pg_dump")
}

/// PostgreSQL data directory under app-data.
pub fn data_dir(platform: &dyn Platform) -> Result<PathBuf> {
    Ok(platform.paths().app_data_dir()?.join("postgres").join("data"))
}

/// A datadir is initialized once `initdb` has written its `PG_VERSION` marker.
pub fn is_initialized(datadir: &Path) -> bool {
    datadir.join("PG_VERSION").is_file()
}

/// Initialize the datadir if needed (`initdb … -A trust`): a fresh cluster with a
/// passwordless `postgres` superuser. Idempotent.
pub fn initialize(platform: &dyn Platform, basedir: &Path, datadir: &Path) -> Result<()> {
    if is_initialized(datadir) {
        return Ok(());
    }
    if let Some(parent) = datadir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // initdb writes the PG_VERSION marker late and usually self-cleans on
    // failure, but guard anyway so a partial cluster can't satisfy
    // is_initialized and start Postgres on a corrupt datadir (B22,
    // defense-in-depth).
    let result = (|| -> Result<()> {
        let args = vec![
            "-D".to_string(),
            datadir.display().to_string(),
            "-U".to_string(),
            "postgres".to_string(),
            "-A".to_string(),
            "trust".to_string(),
            "--no-instructions".to_string(),
        ];
        let mut child = platform.supervisor().spawn(&initdb_bin(basedir), &args)?;
        let status = child.wait()?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::Other(format!("initdb failed (exit {:?})", status.code())))
        }
    })();
    crate::core::db::clean_datadir_on_init_failure(datadir, result)
}

/// Start the shared PostgreSQL server (foreground, TCP-only) via `ProcessSupervisor`.
pub fn start(platform: &dyn Platform, basedir: &Path, datadir: &Path, port: u16) -> Result<Child> {
    let args = vec![
        "-D".to_string(),
        datadir.display().to_string(),
        "-p".to_string(),
        port.to_string(),
        "-c".to_string(),
        "listen_addresses=127.0.0.1".to_string(),
        // Disable Unix sockets: TCP-only, and avoids the macOS socket-path limit.
        "-c".to_string(),
        "unix_socket_directories=".to_string(),
    ];
    let log = platform.paths().log_dir()?.join("postgres-stdout.log");
    platform.supervisor().spawn_logged(&postgres_bin(basedir), &args, &log)
}

/// Stop a running PostgreSQL by pid.
pub fn stop(platform: &dyn Platform, pid: u32) -> Result<()> {
    platform.supervisor().stop(pid)
}

// ---------------------------------------------------------------------------
// Site databases (PLAN-postgres-sites §3). Every function here is reached only
// through `DbEngine`'s methods, which dispatch on the CLIENT's engine — a
// `psql` can never arrive at `database::mysql_exec`, nor a `mysql` here.
// ---------------------------------------------------------------------------

/// The maintenance database every cluster has: where CREATE/DROP DATABASE and
/// the cluster-wide size query run, because PostgreSQL cannot drop the database
/// the session is connected to.
const MAINTENANCE_DB: &str = "postgres";

/// The connect bound for every libpq tool, as an ENV var rather than a flag.
///
/// This is the one place PostgreSQL is easier than MySQL: `--connect-timeout`
/// had to be dropped from `mysqldump`'s argv (it hard-errors — B25, proven by
/// `examples/db_dump_flags_check`), leaving exports with an unbounded connect.
/// libpq reads `PGCONNECT_TIMEOUT` in `psql` AND `pg_dump`, so both are bounded
/// by the same line. Connect phase only — a big import still runs unbounded.
fn connect_timeout_env() -> (&'static str, &'static str) {
    ("PGCONNECT_TIMEOUT", "10")
}

/// The shared flag prefix for the bundled `psql`: loopback TCP, the `postgres`
/// superuser (trust auth — the local-dev setup written by `initialize`), no
/// psqlrc, and **`ON_ERROR_STOP=1`**.
///
/// That last flag is not a nicety. `psql` exits **0** after a script whose every
/// statement failed, so without it a restore that imported nothing would report
/// success — the one failure in this feature that produces a wrong answer
/// instead of an error. It is pinned by a test for that reason.
pub(crate) fn psql_base_args(port: u16, dbname: &str) -> [String; 9] {
    [
        "--no-psqlrc".into(),
        "--set".into(),
        "ON_ERROR_STOP=1".into(),
        "--host=127.0.0.1".into(),
        format!("--port={port}"),
        "--username=postgres".into(),
        "--no-password".into(),
        "--dbname".into(),
        dbname.into(),
    ]
}

/// Run `psql` with `args` appended to the base prefix, returning stdout.
fn psql_run(
    client: &SqlClient,
    port: u16,
    dbname: &str,
    args: &[&str],
    what: &str,
) -> Result<String> {
    let (k, v) = connect_timeout_env();
    let out = Command::new(client.path())
        .args(psql_base_args(port, dbname))
        .args(args)
        .env(k, v)
        .output()?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(Error::Other(format!(
            "{what} failed (exit {:?}): {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

/// Run one SQL statement via the bundled `psql`.
pub(crate) fn psql_exec(
    client: &SqlClient,
    port: u16,
    dbname: &str,
    sql: &str,
    what: &str,
) -> Result<()> {
    psql_run(client, port, dbname, &["--command", sql], what).map(|_| ())
}

/// Whether database `name` exists in the cluster.
fn database_exists(client: &SqlClient, port: u16, name: &str) -> Result<bool> {
    let sql = format!("SELECT 1 FROM pg_database WHERE datname = '{name}'");
    let out = psql_run(
        client,
        port,
        MAINTENANCE_DB,
        &["-tA", "--command", &sql],
        &format!("looking up database `{name}`"),
    )?;
    Ok(out.trim() == "1")
}

/// Create database `name` if it doesn't exist, via the bundled `psql`.
///
/// Two steps because **PostgreSQL has no `CREATE DATABASE IF NOT EXISTS`**. The
/// alternative — create and swallow SQLSTATE 42P04 — would also swallow every
/// other create failure (out of disk, bad encoding, cluster in recovery), which
/// is exactly the class of "it silently did nothing" this project keeps paying
/// for. The name is `validate_db_name`-checked (alnum + `_`) before it reaches
/// either statement.
pub fn create_database(client: &SqlClient, port: u16, name: &str) -> Result<()> {
    validate_db_name(name)?;
    if database_exists(client, port, name)? {
        return Ok(());
    }
    psql_exec(
        client,
        port,
        MAINTENANCE_DB,
        &format!("CREATE DATABASE \"{name}\""),
        &format!("creating database `{name}`"),
    )
}

/// Drop a site's database if it exists (site teardown).
///
/// `WITH (FORCE)` (PG 13+; all three pinned majors have it) terminates other
/// sessions first — without it one open Adminer tab, or a php-fpm worker that
/// still holds a connection, makes deleting a site fail with "database is being
/// accessed by other users", which the user cannot act on.
pub fn drop_database(client: &SqlClient, port: u16, name: &str) -> Result<()> {
    validate_db_name(name)?;
    psql_exec(
        client,
        port,
        MAINTENANCE_DB,
        &format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)"),
        &format!("dropping database `{name}`"),
    )
}

/// Import a `.sql` dump into database `name` via the bundled `psql`, with
/// `--file` (no shell, no quoting problems — same rationale as `--result-file`
/// on the MySQL side). DESTRUCTIVE: the dump executes as-is.
pub fn import_from_file(client: &SqlClient, port: u16, name: &str, file: &Path) -> Result<()> {
    validate_db_name(name)?;
    let meta = std::fs::metadata(file)
        .map_err(|e| Error::Other(format!("open {}: {e}", file.display())))?;
    if meta.len() == 0 {
        return Err(Error::Other(format!(
            "{} is empty — not importing",
            file.display()
        )));
    }
    psql_run(
        client,
        port,
        name,
        &["--file", &file.display().to_string()],
        &format!("importing into `{name}`"),
    )
    .map(|_| ())
}

/// Export database `name` into the user's Downloads folder as `<domain>-db.sql`
/// (numbered on collision — the shared [`dump_dest`] convention), via the
/// bundled `pg_dump`. `--no-owner --no-privileges` so the dump restores into any
/// cluster rather than only one that has this cluster's roles. Requires the
/// server to be running. `dump` is the dump BINARY.
pub fn export_to_downloads(dump: &Path, port: u16, domain: &str, name: &str) -> Result<PathBuf> {
    validate_db_name(name)?;
    let dest = dump_dest(domain)?;
    let (k, v) = connect_timeout_env();
    let out = Command::new(dump)
        .args([
            "--host=127.0.0.1".to_string(),
            format!("--port={port}"),
            "--username=postgres".to_string(),
            "--no-password".to_string(),
            "--no-owner".to_string(),
            "--no-privileges".to_string(),
            format!("--file={}", dest.display()),
            name.to_string(),
        ])
        .env(k, v)
        .output()?;
    if !out.status.success() {
        // A failed dump can leave a partial file — never leave it for the user
        // to mistake for a good backup.
        let _ = std::fs::remove_file(&dest);
        return Err(Error::Other(format!(
            "exporting database `{name}` failed (exit {:?}): {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(dest)
}

/// Disk size of every database in the cluster, in bytes — the same
/// `(name, bytes)` shape MySQL's `information_schema` query returns, so the
/// Sites page learns a third engine rather than a second shape. Template
/// databases are excluded: they are not sites.
pub fn db_sizes(client: &SqlClient, port: u16) -> Result<Vec<(String, u64)>> {
    let out = psql_run(
        client,
        port,
        MAINTENANCE_DB,
        &[
            "-tA",
            "-F",
            "\t",
            "--command",
            "SELECT datname, pg_database_size(datname) FROM pg_database \
             WHERE NOT datistemplate",
        ],
        "listing database sizes",
    )?;
    Ok(out
        .lines()
        .filter_map(|l| {
            let (name, size) = l.split_once('\t')?;
            Some((name.to_string(), size.trim().parse().ok()?))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn psql_args_stop_on_the_first_error() {
        // ON_ERROR_STOP is the difference between "the import failed" and "the
        // import reported success having executed nothing": psql exits 0 after a
        // script whose every statement errored unless it is set. Pin the whole
        // array — a reorder or a dropped flag here changes five subprocess
        // invocations at once.
        assert_eq!(
            psql_base_args(15432, "postgres"),
            [
                "--no-psqlrc",
                "--set",
                "ON_ERROR_STOP=1",
                "--host=127.0.0.1",
                "--port=15432",
                "--username=postgres",
                "--no-password",
                "--dbname",
                "postgres",
            ]
        );
    }

    #[test]
    fn the_connect_bound_is_an_env_var_both_libpq_tools_read() {
        // Unlike MySQL, where mysqldump rejects --connect-timeout outright (B25)
        // and exports had to stay unbounded, psql AND pg_dump both read this.
        assert_eq!(connect_timeout_env(), ("PGCONNECT_TIMEOUT", "10"));
    }

    #[test]
    fn create_and_drop_reject_unsafe_names() {
        let client = SqlClient::test_at_engine("/nonexistent", crate::core::db::DbEngine::Postgres);
        for bad in ["", "wp;drop", "a\"b", "a b", "a-b", "*", "wp_x.y"] {
            assert!(create_database(&client, 15432, bad).is_err(), "create accepted {bad:?}");
            assert!(drop_database(&client, 15432, bad).is_err(), "drop accepted {bad:?}");
        }
    }

    #[test]
    fn drop_forces_other_sessions_off() {
        // Without WITH (FORCE) one open Adminer tab, or a php-fpm worker still
        // holding a connection, makes deleting a site fail with "database is
        // being accessed by other users" — which the user cannot act on from
        // inside rexenv. A source guard because the alternative is a live
        // second connection, and the flag is a one-word revert.
        assert!(
            include_str!("postgres.rs").contains("WITH (FORCE)"),
            "the drop statement must keep WITH (FORCE)"
        );
    }

    #[test]
    fn bin_paths_under_basedir() {
        let base = Path::new("/opt/pg");
        assert_eq!(postgres_bin(base), Path::new("/opt/pg/bin/postgres"));
        assert_eq!(initdb_bin(base), Path::new("/opt/pg/bin/initdb"));
        assert_eq!(psql_bin(base), Path::new("/opt/pg/bin/psql"));
    }

    #[test]
    fn is_initialized_checks_pg_version_marker() {
        let dir = std::env::temp_dir().join("rexenv-pg-init-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!is_initialized(&dir));
        std::fs::write(dir.join("PG_VERSION"), "18\n").unwrap();
        assert!(is_initialized(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

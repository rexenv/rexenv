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
use crate::core::proc::Proc;

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
            Err(Error::Other(format!("initdb failed (exit {})", crate::core::proc::exit_text(status.code()))))
        }
    })();
    crate::core::db::clean_datadir_on_init_failure(datadir, result)
}

/// `pg_ctl` — the wrapper PostgreSQL ships to start and stop a cluster.
pub fn pg_ctl_bin(basedir: &Path) -> PathBuf {
    basedir.join("bin/pg_ctl")
}

/// The server options every start passes, whichever way the server is launched:
/// TCP-only on loopback (`unix_socket_directories=` empty also sidesteps macOS's
/// ~104-char socket-path limit).
fn server_args(datadir: &Path, port: u16) -> Vec<String> {
    vec![
        "-D".to_string(),
        datadir.display().to_string(),
        "-p".to_string(),
        port.to_string(),
        "-c".to_string(),
        "listen_addresses=127.0.0.1".to_string(),
        "-c".to_string(),
        "unix_socket_directories=".to_string(),
    ]
}

/// The same options as ONE `-o` string for `pg_ctl`, which passes its `-o`
/// value to the server. `-D` is `pg_ctl`'s own flag, so it is not repeated here.
fn pg_ctl_server_opts(port: u16) -> String {
    format!("-p {port} -c listen_addresses=127.0.0.1 -c unix_socket_directories=")
}

/// The pid of the running postmaster, read from the file it writes into its own
/// datadir. First line, by PostgreSQL's documented format — the same file
/// `pg_ctl` itself reads.
fn postmaster_pid(datadir: &Path) -> Result<u32> {
    let path = datadir.join("postmaster.pid");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| Error::Other(format!("could not read {}: {e}", path.display())))?;
    text.lines()
        .next()
        .and_then(|l| l.trim().parse::<u32>().ok())
        .ok_or_else(|| Error::Other(format!("{} does not start with a pid", path.display())))
}

/// Start the shared PostgreSQL server (TCP-only).
///
/// **Two launch paths, and the second one is not a preference.** Where the OS
/// can hand a plain spawn an administrator token
/// ([`ProcessSupervisor::may_spawn_with_admin_token`] — Windows, whenever UAC is
/// off or the app was started with "Run as administrator"), `postgres.exe`
/// REFUSES to run at all:
///
/// > Execution of PostgreSQL by a user with administrative permissions is not
/// > permitted.
///
/// Measured 20 Sep 2026 on a clean Windows 11 VM with `EnableLUA=0`, where every
/// process carries that token: the server died on every start and the health
/// watchdog respawn-looped it to `gave-up` while the log said only "did not start
/// within 15s". `initdb` survives the same machine because it re-executes ITSELF
/// under a restricted token; `postgres.exe` has no such code — `pg_ctl` is where
/// PostgreSQL keeps it. So rexenv starts the server the way PostgreSQL's own
/// tooling does, rather than reimplementing `CreateRestrictedToken`.
///
/// `pg_ctl` detaches, so the result is [`Proc::Adopted`]: a pid, not a child
/// handle. That is the honest shape — the postmaster is this session's
/// grandchild — and it is the shape the next launch would have adopted anyway.
pub fn start(platform: &dyn Platform, basedir: &Path, datadir: &Path, port: u16) -> Result<Proc> {
    let log = platform.paths().log_dir()?.join("postgres-stdout.log");
    if let Some(parent) = log.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // `postmaster.pid` from a postmaster that died unclean: PostgreSQL only asks whether
    // the pid EXISTS, and after a reboot it usually does — as some other program. Then
    // every start fails "lock file already exists" (`core::stale_lock`, measured on a VM).
    if let Some(why) = crate::core::stale_lock::clear_if_stale(platform, &datadir.join("postmaster.pid"), &datadir.display().to_string(), &[])? {
        log::warn!("postgres: removed a stale pid file before starting — {why}");
    }
    if !platform.supervisor().may_spawn_with_admin_token() {
        let child = platform.supervisor().spawn_logged(
            &postgres_bin(basedir),
            &server_args(datadir, port),
            &log,
        )?;
        return Ok(Proc::from(child));
    }

    // `-w` waits until the server is ACCEPTING CONNECTIONS, so the pid file
    // below exists and the port is open by the time this returns.
    //
    // Two rules here, both measured on the VM, both silent failures otherwise:
    //
    // 1. The wait is `status()`, never `output()`. `output()` waits for the
    //    pipes to reach EOF, and the server pg_ctl detaches INHERITS them — so
    //    the call would not return until PostgreSQL itself exited (the check
    //    hung for three minutes with a healthy server listening).
    // 2. pg_ctl's OWN output goes to its own file, never the server's log.
    //    `pg_ctl -l` hands the server's stdout to a `cmd` redirect, which opens
    //    that file with sharing that a second writer breaks: pointing both at
    //    one file made every start fail with "The process cannot access the
    //    file because it is being used by another process" — and the server
    //    never ran at all.
    let own_log = log.with_file_name("pg_ctl.log");
    let sink = std::fs::File::create(&own_log)?;
    let status = crate::platform::command(pg_ctl_bin(basedir))
        // The child runs under a RESTRICTED token, which the working directory
        // must be reachable from: inherit the caller's and the server dies with
        // "The current directory is invalid" before it reads a single setting
        // (measured on the VM, where the app's own cwd is not one that token can
        // use). The datadir is the one directory it must be able to reach
        // anyway, so it is the honest choice.
        .current_dir(datadir)
        .arg("-D")
        .arg(datadir)
        .arg("-l")
        .arg(&log)
        .arg("-w")
        .arg("-t")
        .arg("30")
        .arg("-o")
        .arg(pg_ctl_server_opts(port))
        .arg("start")
        .stdin(std::process::Stdio::null())
        .stdout(sink.try_clone()?)
        .stderr(sink)
        .status()
        .map_err(|e| Error::Other(format!("could not run pg_ctl: {e}")))?;
    if !status.success() {
        let why = std::fs::read_to_string(&own_log).unwrap_or_default();
        let why = why.trim().lines().last().unwrap_or("").trim().to_string();
        return Err(Error::Other(format!(
            "PostgreSQL did not start (pg_ctl exit {}){}{} — see {}",
            crate::core::proc::exit_text(status.code()),
            if why.is_empty() { "" } else { ": " },
            why,
            log.display()
        )));
    }
    // `Detached`, not `Adopted`: this session started it, and the watchdog's
    // start grace must apply to it (see `Proc::Detached`).
    Ok(Proc::Detached(postmaster_pid(datadir)?, std::time::Instant::now()))
}

/// Stop a running PostgreSQL.
///
/// Where the server was started through `pg_ctl` (see [`start`]), it is stopped
/// the same way: `-m fast` rolls back open transactions and shuts the cluster
/// down cleanly. Terminating the postmaster instead would be a hard kill on that
/// OS — its backend and auxiliary processes are SEPARATE processes that would
/// survive, keep the datadir locked and the port held, and the next start would
/// fail on a cluster that never checkpointed. `basedir` is `None` when the
/// caller cannot resolve it (an uncached version), and then the supervisor's
/// plain stop is all there is.
pub fn stop(platform: &dyn Platform, basedir: Option<&Path>, pid: u32) -> Result<()> {
    if platform.supervisor().may_spawn_with_admin_token() {
        if let Some(basedir) = basedir {
            let datadir = data_dir(platform)?;
            let out = crate::platform::command(pg_ctl_bin(basedir))
                .current_dir(&datadir)
                .arg("-D")
                .arg(&datadir)
                .arg("-m")
                .arg("fast")
                .arg("-w")
                .arg("-t")
                .arg("30")
                .arg("stop")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            match out {
                Ok(s) if s.success() => return Ok(()),
                Ok(s) => log::warn!(
                    "pg_ctl stop failed (exit {}) — falling back to stopping pid {pid}",
                    crate::core::proc::exit_text(s.code())
                ),
                Err(e) => log::warn!("could not run pg_ctl stop: {e} — stopping pid {pid}"),
            }
        }
    }
    platform.supervisor().stop(pid)
}

// ---------------------------------------------------------------------------
// Site databases (docs/archive/PLAN-postgres-sites.md §3). Every function here is reached only
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
///
/// `pub` rather than `pub(crate)` so a live check can query the fixture server
/// on the production argv instead of hand-rolling one — the same reason
/// `database::client_base_args` is public (a check that builds its own flags is
/// checking something the app does not do).
pub fn psql_base_args(port: u16, dbname: &str) -> [String; 9] {
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
    let out = crate::platform::command(client.path())
        .args(psql_base_args(port, dbname))
        .args(args)
        .env(k, v)
        .output()?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(Error::Other(format!(
            "{what} failed (exit {}): {}",
            crate::core::proc::exit_text(out.status.code()),
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
pub(crate) fn database_exists(client: &SqlClient, port: u16, name: &str) -> Result<bool> {
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
    let dest = dump_dest(domain)?;
    dump_to_file(dump, port, name, &dest)?;
    Ok(dest)
}

/// Dump database `name` into `dest` with the export's flags (`core::dbclone`
/// writes to its private scratch file). A failed dump removes what it wrote.
pub(crate) fn dump_to_file(dump: &Path, port: u16, name: &str, dest: &Path) -> Result<()> {
    validate_db_name(name)?;
    let (k, v) = connect_timeout_env();
    let out = crate::platform::command(dump)
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
        let _ = std::fs::remove_file(dest);
        return Err(Error::Other(format!(
            "exporting database `{name}` failed (exit {}): {}",
            crate::core::proc::exit_text(out.status.code()),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}

/// Every table in database `name`'s `public` schema, sorted.
pub(crate) fn list_tables(client: &SqlClient, port: u16, name: &str) -> Result<Vec<String>> {
    validate_db_name(name)?;
    let out = psql_run(
        client,
        port,
        name,
        &["-tA", "--command", "SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY 1"],
        &format!("listing the tables of `{name}`"),
    )?;
    Ok(out.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
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

    /// **The two launch paths pass the SAME server settings.**
    ///
    /// One is `postgres`'s argv, the other one `-o` string handed to `pg_ctl`;
    /// they are written separately, so nothing but this stops them drifting —
    /// and a cluster that listens on a different address, or opens a Unix socket
    /// on one OS only, is exactly the kind of difference nobody notices until a
    /// connection fails on one platform.
    #[test]
    fn both_launch_paths_carry_the_same_server_settings() {
        let args = server_args(Path::new("/tmp/pgdata"), 15432).join(" ");
        let opts = pg_ctl_server_opts(15432);
        for setting in ["-p 15432", "-c listen_addresses=127.0.0.1", "-c unix_socket_directories="] {
            assert!(args.contains(setting), "postgres argv lost {setting:?}: {args}");
            assert!(opts.contains(setting), "pg_ctl -o lost {setting:?}: {opts}");
        }
        // `-D` is pg_ctl's OWN flag; repeating it inside `-o` makes pg_ctl pass a
        // second datadir to the server, which then refuses to start.
        assert!(!opts.contains("-D"), "pg_ctl -o must not carry -D: {opts}");
        assert!(args.contains("/tmp/pgdata"), "{args}");
    }

    /// The pid comes from the file PostgreSQL itself writes — the same one
    /// `pg_ctl` reads — and a file that is not that shape is an error, never a
    /// guess: a wrong pid here is a pid rexenv would later STOP.
    #[test]
    fn the_postmaster_pid_is_the_first_line_or_an_error() {
        let dir = std::env::temp_dir().join(format!("rexenv-pgpid-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("fixture dir");
        let pid_file = dir.join("postmaster.pid");
        std::fs::write(&pid_file, "4242\n/tmp/pgdata\n1758000000\n15432\n").expect("write");
        assert_eq!(postmaster_pid(&dir).expect("pid"), 4242);
        std::fs::write(&pid_file, "not-a-pid\nrest\n").expect("write");
        assert!(postmaster_pid(&dir).is_err(), "a non-numeric first line was accepted");
        std::fs::remove_file(&pid_file).expect("rm");
        assert!(postmaster_pid(&dir).is_err(), "a missing pid file was accepted");
        let _ = std::fs::remove_dir_all(&dir);
    }
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

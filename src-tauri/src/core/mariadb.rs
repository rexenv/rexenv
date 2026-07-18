//! core::mariadb — MariaDB service (TODO "Deferred services").
//!
//! One shared MariaDB server on a loopback port, from a bottle bundle
//! (`core::binaries::resolve_bundle("mariadb", …)` — server/client/dump +
//! relinked openssl@3 + pcre2 dylibs). MariaDB has no `--initialize-insecure`
//! (that's MySQL) and its `mariadb-install-db` is a shell script full of baked
//! Homebrew paths, so [`initialize`] drives `mariadbd --bootstrap` DIRECTLY:
//! the same SQL files the script feeds, over stdin, with
//! `@auth_root_socket=NULL` so root gets passwordless normal auth for
//! localhost + 127.0.0.1 — the same local-dev model as our MySQL.
//!
//! Share data (errmsg.sys, charsets) is resolved with EXPLICIT
//! `--lc-messages-dir`/`--character-sets-dir` args: the compiled-in defaults
//! are the build prefix (a `@@HOMEBREW_PREFIX@@` placeholder in the bottle) and
//! must never be trusted.

use crate::core::db::MARIADB_PORT;
use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Child;

/// `mariadbd` server executable inside the bundle tree.
pub fn mariadbd_bin(basedir: &Path) -> PathBuf {
    basedir.join("bin/mariadbd")
}

/// `mariadb` client inside the bundle tree.
pub fn mariadb_client_bin(basedir: &Path) -> PathBuf {
    basedir.join("bin/mariadb")
}

/// `mariadb-dump` inside the bundle tree.
pub fn mariadb_dump_bin(basedir: &Path) -> PathBuf {
    basedir.join("bin/mariadb-dump")
}

/// MariaDB data directory under app-data.
pub fn data_dir(platform: &dyn Platform) -> Result<PathBuf> {
    Ok(platform.paths().app_data_dir()?.join("mariadb").join("data"))
}

/// Unix socket path (loopback TCP is the real interface, but mariadbd always
/// creates a socket too — the compiled-in default is a placeholder path, so it
/// must be pinned under app-data like MySQL's).
pub fn socket_path(platform: &dyn Platform) -> Result<PathBuf> {
    Ok(platform.paths().app_data_dir()?.join("run").join("mariadb.sock"))
}

/// A datadir is initialized once its system `mysql` schema dir exists (same
/// marker as MySQL — created by the bootstrap).
pub fn is_initialized(datadir: &Path) -> bool {
    datadir.join("mysql").is_dir()
}

/// The bootstrap SQL files `mariadb-install-db` feeds, in ITS order — system
/// tables, performance tables, then the data pass that creates the root users.
/// (help/GIS/sys-schema fills are optional content and deliberately skipped;
/// their tables are created empty by the system-tables script.)
const BOOTSTRAP_SQL: &[&str] = &[
    "share/mysql/mariadb_system_tables.sql",
    "share/mysql/mariadb_performance_tables.sql",
    "share/mysql/mariadb_system_tables_data.sql",
];

fn share_args(basedir: &Path) -> Vec<String> {
    vec![
        format!("--lc-messages-dir={}", basedir.join("share/mysql/english").display()),
        format!("--character-sets-dir={}", basedir.join("share/mysql/charsets").display()),
    ]
}

/// Initialize the datadir if needed via `mariadbd --bootstrap` (passwordless
/// root@localhost + root@127.0.0.1, the local-dev model). Idempotent.
/// (`_platform` keeps signature parity with the other engines' initializers —
/// the bootstrap is a run-to-completion `std::process::Command` with piped
/// stdin, which `ProcessSupervisor::spawn` doesn't provide.)
pub fn initialize(_platform: &dyn Platform, basedir: &Path, datadir: &Path) -> Result<()> {
    if is_initialized(datadir) {
        return Ok(());
    }
    std::fs::create_dir_all(datadir)?;
    // Any failure below removes the half-written datadir so its early `mysql/`
    // marker can't lie on the next run (B22/B23) — covers the SQL-read `?`, a
    // stdin-write EPIPE, and a nonzero bootstrap exit alike.
    let result = bootstrap(basedir, datadir);
    crate::core::db::clean_datadir_on_init_failure(datadir, result)
}

/// Drive `mariadbd --bootstrap` to completion, feeding the system-schema SQL over
/// stdin. Separated from [`initialize`] so the datadir cleanup wraps EVERY exit.
fn bootstrap(basedir: &Path, datadir: &Path) -> Result<()> {
    // Same preamble mariadb-install-db's cat_sql() emits for "normal" root
    // auth, then the SQL files in its order.
    let mut sql = String::from(
        "create database if not exists mysql;\nuse mysql;\nSET @auth_root_socket=NULL;\n",
    );
    for rel in BOOTSTRAP_SQL {
        let path = basedir.join(rel);
        sql.push_str(&std::fs::read_to_string(&path).map_err(|e| {
            Error::Other(format!("read bootstrap SQL {}: {e}", path.display()))
        })?);
        sql.push('\n');
    }

    let mut args = vec![
        "--no-defaults".to_string(),
        "--bootstrap".to_string(),
        format!("--basedir={}", basedir.display()),
        format!("--datadir={}", datadir.display()),
        "--log-warnings=0".to_string(),
        "--max_allowed_packet=8M".to_string(),
        "--net_buffer_length=16K".to_string(),
    ];
    args.extend(share_args(basedir));

    // Run-to-completion helper with piped stdin (same pattern as the bundled
    // mysql client in core::database::import_from_file — no shell involved).
    let mut child = std::process::Command::new(mariadbd_bin(basedir))
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| Error::Other(format!("spawn mariadbd --bootstrap: {e}")))?;
    // Feed the bootstrap SQL. If mariadbd died early the write hits EPIPE — reap
    // the child before propagating so a crashed bootstrap can't leave a zombie
    // (Child::drop neither kills nor waits).
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| Error::Other("mariadbd --bootstrap: no stdin".into()))?;
    if let Err(e) = stdin.write_all(sql.as_bytes()) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(Error::from(e));
    }
    drop(stdin); // EOF — bootstrap runs the fed SQL and exits.
    let out = child.wait_with_output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "mariadbd --bootstrap failed (exit {:?}): {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

/// Start the shared MariaDB server (foreground, loopback-only) via
/// `ProcessSupervisor`. The `--datadir` app-data path doubles as the adoption
/// ownership marker on the cmdline.
pub fn start(
    platform: &dyn Platform,
    basedir: &Path,
    datadir: &Path,
    port: u16,
    socket: &Path,
) -> Result<Child> {
    if let Some(parent) = socket.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let log = platform.paths().log_dir()?.join("mariadb-error.log");
    let mut args = vec![
        "--no-defaults".to_string(),
        format!("--basedir={}", basedir.display()),
        format!("--datadir={}", datadir.display()),
        format!("--port={port}"),
        format!("--socket={}", socket.display()),
        "--bind-address=127.0.0.1".to_string(),
        format!("--log-error={}", log.display()),
    ];
    args.extend(share_args(basedir));
    let stdout_log = platform.paths().log_dir()?.join("mariadb-stdout.log");
    platform
        .supervisor()
        .spawn_logged(&mariadbd_bin(basedir), &args, &stdout_log)
}

/// Stop a running MariaDB by pid (SIGTERM → graceful shutdown).
pub fn stop(platform: &dyn Platform, pid: u32) -> Result<()> {
    platform.supervisor().stop(pid)
}

/// The default loopback port (re-exported from the `DbEngine` registry).
pub fn port() -> u16 {
    MARIADB_PORT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bin_paths_under_basedir() {
        let base = Path::new("/opt/mariadb");
        assert_eq!(mariadbd_bin(base), Path::new("/opt/mariadb/bin/mariadbd"));
        assert_eq!(mariadb_client_bin(base), Path::new("/opt/mariadb/bin/mariadb"));
        assert_eq!(mariadb_dump_bin(base), Path::new("/opt/mariadb/bin/mariadb-dump"));
    }

    #[test]
    fn is_initialized_checks_system_schema() {
        let dir = std::env::temp_dir().join("rexenv-mariadb-init-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!is_initialized(&dir));
        std::fs::create_dir_all(dir.join("mysql")).unwrap();
        assert!(is_initialized(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bootstrap_sql_set_matches_the_bundle_include_list() {
        // Every SQL file initialize feeds must be part of the pinned bundle —
        // a drifted include list would fail at first init, not at pin time.
        use crate::core::binaries;
        use crate::platform::traits::Arch;
        let bundle =
            binaries::bundle_manifest("mariadb", binaries::MARIADB_VERSION, "macos", Arch::Arm64)
                .expect("mariadb bundle pinned");
        let includes = bundle.parts[0].include;
        for sql in BOOTSTRAP_SQL {
            assert!(includes.contains(sql), "{sql} missing from the mariadb bundle include list");
        }
    }
}

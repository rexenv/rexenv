//! core::postgres — PostgreSQL service (Phase 2 §5.3).
//!
//! One shared PostgreSQL server on a loopback port. The datadir is initialized
//! once with `initdb` (trust auth, passwordless `postgres` superuser for local
//! dev), then `postgres` is supervised like the other services. `basedir` is the
//! extracted tree from `core::binaries::resolve_dir("postgres", …)`. We run
//! **TCP-only** (`unix_socket_directories=` empty) — our model connects over
//! `127.0.0.1:<port>`, and it sidesteps macOS's ~104-char Unix-socket path limit.

use crate::core::db::POSTGRES_PORT;
use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use std::path::{Path, PathBuf};
use std::process::Child;

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
        Err(Error::Other(format!(
            "initdb failed (exit {:?})",
            status.code()
        )))
    }
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

/// The default loopback port (re-exported from the `DbEngine` registry).
pub fn port() -> u16 {
    POSTGRES_PORT
}

#[cfg(test)]
mod tests {
    use super::*;

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

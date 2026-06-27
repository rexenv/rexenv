//! core::database — database services (Phase 1: MySQL).
//!
//! One shared MySQL server on a loopback port. The datadir is initialized once
//! with `--initialize-insecure` (passwordless root for local dev), then the
//! `mysqld` master is supervised like the other services. `basedir` is the
//! extracted MySQL tree from `core::binaries::resolve_dir("mysql", …)`.

use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

/// Loopback port for the shared MySQL server (non-default to avoid clashing with
/// a system MySQL on 3306).
pub const MYSQL_PORT: u16 = 13306;

/// `mysqld` executable inside the extracted MySQL tree.
pub fn mysqld_bin(basedir: &Path) -> PathBuf {
    basedir.join("bin/mysqld")
}

/// `mysql` client inside the extracted MySQL tree.
pub fn mysql_client_bin(basedir: &Path) -> PathBuf {
    basedir.join("bin/mysql")
}

/// MySQL data directory under app-data.
pub fn data_dir(platform: &dyn Platform) -> Result<PathBuf> {
    Ok(platform.paths().app_data_dir()?.join("mysql").join("data"))
}

/// Unix socket path for the server.
pub fn socket_path(platform: &dyn Platform) -> Result<PathBuf> {
    Ok(platform.paths().app_data_dir()?.join("run").join("mysql.sock"))
}

/// A datadir is initialized once its system `mysql` schema dir exists.
pub fn is_initialized(datadir: &Path) -> bool {
    datadir.join("mysql").is_dir()
}

/// Initialize the datadir if needed (`mysqld --initialize-insecure`): creates a
/// fresh database with a passwordless `root@localhost`. Idempotent.
pub fn initialize(platform: &dyn Platform, basedir: &Path, datadir: &Path) -> Result<()> {
    if is_initialized(datadir) {
        return Ok(());
    }
    std::fs::create_dir_all(datadir)?;
    let log = platform.paths().log_dir()?.join("mysql-init.log");
    std::fs::create_dir_all(platform.paths().log_dir()?)?;

    let args = vec![
        "--no-defaults".to_string(),
        "--initialize-insecure".to_string(),
        format!("--basedir={}", basedir.display()),
        format!("--datadir={}", datadir.display()),
        format!("--log-error={}", log.display()),
    ];
    let mut child = platform.supervisor().spawn(&mysqld_bin(basedir), &args)?;
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "mysqld --initialize-insecure failed (exit {:?}); see {}",
            status.code(),
            log.display()
        )))
    }
}

/// Start the shared MySQL server (foreground) via `ProcessSupervisor`.
pub fn start(
    platform: &dyn Platform,
    basedir: &Path,
    datadir: &Path,
    port: u16,
    socket: &Path,
) -> Result<Child> {
    let log = platform.paths().log_dir()?.join("mysql-error.log");
    let args = vec![
        "--no-defaults".to_string(),
        format!("--basedir={}", basedir.display()),
        format!("--datadir={}", datadir.display()),
        format!("--port={port}"),
        format!("--socket={}", socket.display()),
        "--bind-address=127.0.0.1".to_string(),
        "--mysqlx=OFF".to_string(), // skip the X protocol (avoids :33060)
        format!("--log-error={}", log.display()),
    ];
    platform.supervisor().spawn(&mysqld_bin(basedir), &args)
}

/// Stop a running MySQL by pid (SIGTERM → graceful shutdown).
pub fn stop(platform: &dyn Platform, pid: u32) -> Result<()> {
    platform.supervisor().stop(pid)
}

/// True if MySQL is accepting connections on its loopback port.
pub fn mysql_running(port: u16) -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_millis(300),
    )
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bin_paths_under_basedir() {
        let base = Path::new("/opt/mysql");
        assert_eq!(mysqld_bin(base), Path::new("/opt/mysql/bin/mysqld"));
        assert_eq!(mysql_client_bin(base), Path::new("/opt/mysql/bin/mysql"));
    }

    #[test]
    fn is_initialized_checks_system_schema() {
        let dir = std::env::temp_dir().join("rexenv-mysql-init-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!is_initialized(&dir));
        std::fs::create_dir_all(dir.join("mysql")).unwrap();
        assert!(is_initialized(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mysql_running_false_on_closed_port() {
        assert!(!mysql_running(9));
    }
}

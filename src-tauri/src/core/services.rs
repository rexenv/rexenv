//! core::services — service supervision (Phase 1: PHP-FPM pools).
//!
//! One PHP-FPM master per PHP version (not per site) — all sites on a version
//! share one pool, listening on a loopback TCP port for FastCGI. Nginx (6.2)
//! connects to this port. We run php-fpm as the current user (no root), so the
//! pool needs no `user`/`group` directive.

use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::Duration;

/// Loopback FastCGI port for the PHP 8.3 pool (one pool per PHP version).
pub const PHP_FPM_PORT: u16 = 9783;

/// Render a php-fpm config: one foreground `[global]` master + one `[www]` pool
/// on `127.0.0.1:<port>`. The pool generator is per-version so more versions can
/// be added later, each on its own port.
pub fn generate_fpm_config(port: u16, pid_file: &Path, log_file: &Path) -> String {
    format!(
        "[global]\n\
         pid = {pid}\n\
         error_log = {log}\n\
         daemonize = no\n\
         \n\
         [www]\n\
         listen = 127.0.0.1:{port}\n\
         pm = dynamic\n\
         pm.max_children = 5\n\
         pm.start_servers = 2\n\
         pm.min_spare_servers = 1\n\
         pm.max_spare_servers = 3\n\
         catch_workers_output = yes\n",
        pid = pid_file.display(),
        log = log_file.display(),
    )
}

/// Write the php-fpm config for `version` (on `port`) under the config dir,
/// creating the run/log dirs. Returns the config path.
pub fn write_fpm_config(platform: &dyn Platform, version: &str, port: u16) -> Result<PathBuf> {
    let config_dir = platform.paths().config_dir()?;
    let log_dir = platform.paths().log_dir()?;
    let run_dir = platform.paths().app_data_dir()?.join("run");
    std::fs::create_dir_all(&config_dir)?;
    std::fs::create_dir_all(&log_dir)?;
    std::fs::create_dir_all(&run_dir)?;

    let conf = config_dir.join(format!("php-fpm-{version}.conf"));
    let pid = run_dir.join(format!("php-fpm-{version}.pid"));
    let log = log_dir.join(format!("php-fpm-{version}.log"));
    std::fs::write(&conf, generate_fpm_config(port, &pid, &log))?;
    Ok(conf)
}

/// Start the php-fpm master in the foreground (`-F`) with the given config, via
/// `ProcessSupervisor`. Returns the child handle (track its pid to `stop`).
pub fn start_fpm(platform: &dyn Platform, php_fpm_bin: &Path, conf: &Path) -> Result<Child> {
    let args = vec![
        "-F".to_string(),
        "-y".to_string(),
        conf.display().to_string(),
    ];
    platform.supervisor().spawn(php_fpm_bin, &args)
}

/// Validate a php-fpm config without starting it (`php-fpm -t -y <conf>`).
pub fn test_fpm_config(platform: &dyn Platform, php_fpm_bin: &Path, conf: &Path) -> Result<()> {
    let args = vec![
        "-t".to_string(),
        "-y".to_string(),
        conf.display().to_string(),
    ];
    let mut child = platform.supervisor().spawn(php_fpm_bin, &args)?;
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "php-fpm config test failed (exit {:?})",
            status.code()
        )))
    }
}

/// Stop a running php-fpm by pid.
pub fn stop(platform: &dyn Platform, pid: u32) -> Result<()> {
    platform.supervisor().stop(pid)
}

/// Service status the services/UI layer reads: a pool is `running` iff its
/// loopback FastCGI port accepts a connection.
pub fn fpm_running(port: u16) -> bool {
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
    fn fpm_config_has_global_and_pool() {
        let cfg = generate_fpm_config(
            9783,
            Path::new("/run/php-fpm-8.3.pid"),
            Path::new("/logs/php-fpm-8.3.log"),
        );
        assert!(cfg.contains("[global]"));
        assert!(cfg.contains("daemonize = no"));
        assert!(cfg.contains("pid = /run/php-fpm-8.3.pid"));
        assert!(cfg.contains("error_log = /logs/php-fpm-8.3.log"));
        assert!(cfg.contains("[www]"));
        assert!(cfg.contains("listen = 127.0.0.1:9783"));
        assert!(cfg.contains("pm = dynamic"));
        // No user/group: we run as the current user, not root.
        assert!(!cfg.contains("\nuser ="));
        assert!(!cfg.contains("\ngroup ="));
    }

    #[test]
    fn fpm_running_false_on_closed_port() {
        // An unlikely-to-be-open high port: status should read stopped.
        assert!(!fpm_running(8))
    }
}

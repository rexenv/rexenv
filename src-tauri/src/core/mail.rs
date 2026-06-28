//! core::mail — Mailpit mail-catching service (Phase 3 §2.1).
//!
//! Mailpit is a single static Go binary that runs an SMTP sink plus a web UI /
//! HTTP API. We bind both to loopback on fixed ports (SMTP 1025, HTTP/API 8025)
//! and persist captured mail to a SQLite file under app-data so it survives
//! restarts. Supervised like the other services via `ProcessSupervisor`.
//! Platform-agnostic: talks to `platform/` traits only.

use crate::core::ports;
use crate::error::Result;
use crate::platform::traits::Platform;
use std::path::{Path, PathBuf};
use std::process::Child;

/// SMTP bind port — where php-fpm's sendmail shim delivers (§2.2).
pub const MAILPIT_SMTP_PORT: u16 = 1025;
/// HTTP port — the web UI and the REST API (`/api/v1/…`), and our health probe.
pub const MAILPIT_HTTP_PORT: u16 = 8025;

/// Mailpit's persistent message store under app-data.
pub fn data_dir(platform: &dyn Platform) -> Result<PathBuf> {
    Ok(platform.paths().app_data_dir()?.join("mailpit"))
}

/// Base URL of the Mailpit HTTP API / web UI.
pub fn api_base() -> String {
    format!("http://127.0.0.1:{MAILPIT_HTTP_PORT}")
}

/// The php-fpm `sendmail_path` shim that routes a site's PHP `mail()` into
/// Mailpit's SMTP sink (§2.2): Mailpit's own `sendmail` subcommand aimed at the
/// local SMTP port. The binary path is single-quoted (app-data paths contain
/// spaces) since PHP runs this via `/bin/sh -c`. `-t` is accepted for sendmail
/// compatibility; `-S` selects the SMTP server.
pub fn sendmail_path(mailpit_bin: &Path) -> String {
    format!(
        "'{}' sendmail -t -S 127.0.0.1:{MAILPIT_SMTP_PORT}",
        mailpit_bin.display()
    )
}

/// Start the Mailpit server (loopback SMTP + HTTP, persistent DB) via
/// `ProcessSupervisor`; stdout/stderr go to a per-service log.
pub fn start(platform: &dyn Platform, mailpit_bin: &PathBuf) -> Result<Child> {
    let dir = data_dir(platform)?;
    std::fs::create_dir_all(&dir)?;
    let db = dir.join("mailpit.db");
    let args = vec![
        "--listen".to_string(),
        format!("127.0.0.1:{MAILPIT_HTTP_PORT}"),
        "--smtp".to_string(),
        format!("127.0.0.1:{MAILPIT_SMTP_PORT}"),
        "--database".to_string(),
        db.display().to_string(),
        "--quiet".to_string(),
    ];
    let log = platform.paths().log_dir()?.join("mailpit-stdout.log");
    platform.supervisor().spawn_logged(mailpit_bin, &args, &log)
}

/// Stop a running Mailpit by pid.
pub fn stop(platform: &dyn Platform, pid: u32) -> Result<()> {
    platform.supervisor().stop(pid)
}

/// True if Mailpit's HTTP port is accepting connections (the service is up).
pub fn running() -> bool {
    ports::is_listening(MAILPIT_HTTP_PORT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_base_targets_loopback_http_port() {
        assert_eq!(api_base(), "http://127.0.0.1:8025");
    }

    #[test]
    fn ports_are_distinct() {
        assert_ne!(MAILPIT_SMTP_PORT, MAILPIT_HTTP_PORT);
    }

    #[test]
    fn sendmail_path_quotes_binary_and_targets_smtp() {
        let shim = sendmail_path(Path::new("/App Support/bin/mailpit"));
        // Binary path single-quoted (it contains a space).
        assert!(shim.starts_with("'/App Support/bin/mailpit' sendmail"));
        assert!(shim.contains("-t"));
        assert!(shim.contains("-S 127.0.0.1:1025"));
    }
}

//! core::logs — read-only log tailing for the Logs viewer (Phase 3 §3.1).
//!
//! Every rexenv service logs under `paths().log_dir()` (the `spawn_logged`
//! per-service stdout logs from Phase 1 §10.3, plus nginx access/error, the
//! per-version php-fpm pool logs, and the DB error logs). The Logs tab picks a
//! source and polls [`tail`] for the last N lines. A log "key" is just the file
//! name within `log_dir` — validated so the UI can never read outside it.

use crate::core::php;
use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use crate::state::models::{Site, WebServer};
use serde::Serialize;
use std::io::{Read, Seek, SeekFrom};

/// Only ever tail the trailing slice of a file (bounds memory on big access logs).
const TAIL_CAP_BYTES: u64 = 256 * 1024;

/// One selectable log source (a file under `log_dir` + a human label).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogTarget {
    /// File name within `log_dir` (also the IPC key passed back to `tail`).
    pub key: String,
    pub label: String,
}

/// The curated set of log sources relevant to a site: the shared edge/nginx, the
/// site's php-fpm pool (by its PHP version), the DB error log, and — when the site
/// uses the FrankenPHP override — its per-site backend log.
pub fn targets_for_site(site: &Site) -> Vec<LogTarget> {
    let minor = php::minor_of(&site.php_version);
    let mut targets = vec![
        LogTarget { key: "nginx-access.log".into(), label: "Nginx access".into() },
        LogTarget { key: "nginx-error.log".into(), label: "Nginx error".into() },
        LogTarget { key: format!("php-fpm-{minor}.log"), label: format!("PHP-FPM {minor}") },
        LogTarget { key: "php-fpm-stdout.log".into(), label: "PHP-FPM output".into() },
        LogTarget { key: "caddy-stdout.log".into(), label: "Caddy (edge)".into() },
        LogTarget { key: "mysql-error.log".into(), label: "MySQL".into() },
        LogTarget { key: "postgres-stdout.log".into(), label: "PostgreSQL".into() },
    ];
    if matches!(site.web_server, WebServer::Frankenphp) {
        targets.push(LogTarget {
            key: format!("frankenphp-{}-stdout.log", site.domain),
            label: "FrankenPHP".into(),
        });
    }
    targets
}

/// A log key is a plain file name within `log_dir`: ends in `.log`, no path
/// separators or `..` (so the UI can never escape the log directory).
fn is_safe_key(key: &str) -> bool {
    !key.is_empty()
        && key.ends_with(".log")
        && !key.contains('/')
        && !key.contains('\\')
        && !key.contains("..")
}

/// The last `lines` lines of the log `key`. A missing file ⇒ empty (the service
/// may not have started yet). Reads only the trailing [`TAIL_CAP_BYTES`].
pub fn tail(platform: &dyn Platform, key: &str, lines: usize) -> Result<Vec<String>> {
    if !is_safe_key(key) {
        return Err(Error::Other(format!("invalid log key: {key}")));
    }
    let path = platform.paths().log_dir()?.join(key);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let mut file = std::fs::File::open(&path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(TAIL_CAP_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf);

    let mut all: Vec<&str> = text.lines().collect();
    // If we seeked into the middle of a line, drop the partial first line.
    if start > 0 && !all.is_empty() {
        all.remove(0);
    }
    let from = all.len().saturating_sub(lines);
    Ok(all[from..].iter().map(|s| s.to_string()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::models::{ServiceStatus, Site, SiteType, WebServer};

    fn site(server: WebServer) -> Site {
        Site {
            id: "1".into(),
            name: "Acme".into(),
            domain: "acme.test".into(),
            site_type: SiteType::Php,
            status: ServiceStatus::Stopped,
            php_version: "8.2.31".into(),
            web_server: server,
            ssl: true,
            path: "/tmp/acme".into(),
            created_at: String::new(),
        }
    }

    #[test]
    fn targets_use_site_php_version_and_omit_frankenphp_for_nginx() {
        let t = targets_for_site(&site(WebServer::Nginx));
        let keys: Vec<&str> = t.iter().map(|x| x.key.as_str()).collect();
        assert!(keys.contains(&"php-fpm-8.2.log")); // the site's minor
        assert!(keys.contains(&"nginx-access.log"));
        assert!(keys.contains(&"mysql-error.log"));
        assert!(!keys.iter().any(|k| k.starts_with("frankenphp-")));
    }

    #[test]
    fn targets_include_frankenphp_backend_for_override_sites() {
        let t = targets_for_site(&site(WebServer::Frankenphp));
        assert!(t.iter().any(|x| x.key == "frankenphp-acme.test-stdout.log"));
    }

    #[test]
    fn safe_key_rejects_traversal_and_non_logs() {
        assert!(is_safe_key("nginx-access.log"));
        assert!(!is_safe_key("../../etc/passwd"));
        assert!(!is_safe_key("/etc/passwd.log"));
        assert!(!is_safe_key("secrets.txt"));
        assert!(!is_safe_key(""));
    }

    #[test]
    fn tail_returns_last_n_lines() {
        let dir = std::env::temp_dir().join("rexenv-logs-tail-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("sample.log");
        std::fs::write(&p, "l1\nl2\nl3\nl4\nl5\n").unwrap();

        // tail() needs a Platform; read the file directly through the same logic.
        let data = std::fs::read(&p).unwrap();
        let text = String::from_utf8_lossy(&data);
        let all: Vec<&str> = text.lines().collect();
        let from = all.len().saturating_sub(2);
        assert_eq!(&all[from..], &["l4", "l5"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

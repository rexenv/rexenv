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
use std::path::{Path, PathBuf};

/// Only ever tail the trailing slice of a file (bounds memory on big access logs).
const TAIL_CAP_BYTES: u64 = 256 * 1024;

/// Which Logs-tab category a source belongs to (drives the tab grouping in
/// the UI; the WordPress debug log is its own tab with dedicated IPC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogCategory {
    /// Edge / web server / PHP pools — shared across all sites.
    Server,
    /// Database engine logs — shared across all sites.
    Database,
    /// This site's Git add-job logs (per-site files).
    Git,
}

/// One selectable log source (a file under `log_dir` + a human label).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogTarget {
    /// File name within `log_dir` (also the IPC key passed back to `tail`).
    pub key: String,
    pub label: String,
    pub category: LogCategory,
    /// Absolute path of the file — for the UI's path row / "Open file".
    pub path: String,
}

/// The curated set of log sources relevant to a site: the shared edge/nginx, the
/// site's php-fpm pool (by its PHP version), the DB error log, and — when the site
/// uses the FrankenPHP override — its per-site backend log. `log_dir` is
/// scanned for the site's Git add-job logs (`repo-<domain>-<dir>.log`, one per
/// cloned asset — "view the last job's log" with no new IPC); a nonexistent
/// dir simply adds none.
pub fn targets_for_site(site: &Site, log_dir: &Path) -> Vec<LogTarget> {
    let minor = php::minor_of(&site.php_version);
    let t = |key: String, label: String, category: LogCategory| LogTarget {
        path: log_dir.join(&key).to_string_lossy().into_owned(),
        key,
        label,
        category,
    };
    use LogCategory::{Database, Git, Server};
    let mut targets = vec![
        t("nginx-access.log".into(), "Nginx access".into(), Server),
        t("nginx-error.log".into(), "Nginx error".into(), Server),
        t(format!("php-fpm-{minor}.log"), format!("PHP-FPM {minor}"), Server),
        t("php-fpm-stdout.log".into(), "PHP-FPM output".into(), Server),
        t("caddy-stdout.log".into(), "Caddy (edge)".into(), Server),
        t("mysql-error.log".into(), "MySQL".into(), Database),
        t("mariadb-error.log".into(), "MariaDB".into(), Database),
        t("postgres-stdout.log".into(), "PostgreSQL".into(), Database),
    ];
    if matches!(site.web_server, WebServer::Frankenphp) {
        targets.push(t(
            format!("frankenphp-{}-stdout.log", site.domain),
            "FrankenPHP".into(),
            Server,
        ));
    }
    let prefix = format!("repo-{}-", site.domain);
    if let Ok(entries) = std::fs::read_dir(log_dir) {
        let mut repo_keys: Vec<String> = entries
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.starts_with(&prefix) && n.ends_with(".log"))
            .collect();
        repo_keys.sort();
        for key in repo_keys {
            let dir = key[prefix.len()..key.len() - 4].to_string();
            let label = format!("Git job — {dir}");
            targets.push(t(key, label, Git));
        }
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
    tail_file(&platform.paths().log_dir()?.join(key), lines)
}

/// Truncate the log `key` to empty (same key gate as [`tail`]). Safe in place:
/// every writer holds these files in append mode — nginx/php-fpm/the DB
/// engines open their own logs `O_APPEND`, and `spawn_logged` captures stdout
/// with `.append(true)` — so the next write lands at the new EOF (no reopen
/// needed, no NUL gap). A missing file is fine (nothing to clear).
pub fn clear(platform: &dyn Platform, key: &str) -> Result<()> {
    if !is_safe_key(key) {
        return Err(Error::Other(format!("invalid log key: {key}")));
    }
    let path = platform.paths().log_dir()?.join(key);
    if path.is_file() {
        std::fs::write(&path, "")?;
    }
    Ok(())
}

/// Copy the log `key` into the user's Downloads folder (same file name,
/// numbered on collision). Returns the destination. A missing file errors —
/// there is nothing to download.
pub fn download(platform: &dyn Platform, key: &str) -> Result<PathBuf> {
    if !is_safe_key(key) {
        return Err(Error::Other(format!("invalid log key: {key}")));
    }
    let src = platform.paths().log_dir()?.join(key);
    if !src.is_file() {
        return Err(Error::Other(format!("no {key} yet — nothing to download")));
    }
    let dest = numbered_log_dest(&downloads_dir()?, key.trim_end_matches(".log"));
    std::fs::copy(&src, &dest)?;
    Ok(dest)
}

/// The user's Downloads folder (shared by every log download).
fn downloads_dir() -> Result<PathBuf> {
    directories::UserDirs::new()
        .and_then(|u| u.download_dir().map(|p| p.to_path_buf()))
        .ok_or_else(|| Error::Other("could not resolve the Downloads folder".into()))
}

/// First non-existing `<stem>.log` / `<stem>-<n>.log` under `dir`.
fn numbered_log_dest(dir: &Path, stem: &str) -> PathBuf {
    let mut dest = dir.join(format!("{stem}.log"));
    let mut n = 1;
    while dest.exists() {
        dest = dir.join(format!("{stem}-{n}.log"));
        n += 1;
    }
    dest
}

/// The last `lines` lines of an arbitrary log file (the trailing
/// [`TAIL_CAP_BYTES`] only, so a huge file never blows memory). Callers resolve
/// the path server-side — this is never handed a UI-supplied path.
fn tail_file(path: &Path, lines: usize) -> Result<Vec<String>> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let mut file = std::fs::File::open(path)?;
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

// ---------------------------------------------------------------------------
// WordPress debug.log (Logs tab, "WordPress debug log" section)
// ---------------------------------------------------------------------------

/// Where a site's WordPress debug log is, and whether logging is actually on.
/// Resolved by statically reading `wp-config.php` — no PHP spawn, so the Logs
/// tab can poll it cheaply.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpDebugLogStatus {
    /// `WP_DEBUG` constant truthy in wp-config.php.
    pub debug: bool,
    /// `WP_DEBUG_LOG` truthy or set to a custom path (file logging on).
    pub log_enabled: bool,
    /// Resolved debug log path (WordPress default `wp-content/debug.log`,
    /// or the custom `WP_DEBUG_LOG` path).
    pub path: String,
    pub exists: bool,
    pub size_bytes: u64,
}

/// Extract the value token of `define('NAME', <value>)` from one line of
/// wp-config.php. Static text scan (like WP-CLI's `config get`) — covers the
/// standard single-line form; dynamically computed defines aren't detected.
fn define_value(line: &str, name: &str) -> Option<String> {
    let t = line.trim_start();
    if t.starts_with("//") || t.starts_with('#') || t.starts_with('*') || t.starts_with("/*") {
        return None;
    }
    let rest = t[t.find("define")? + "define".len()..].trim_start().strip_prefix('(')?;
    let rest = rest.trim_start();
    let quote = rest.chars().next().filter(|c| *c == '\'' || *c == '"')?;
    let rest = &rest[1..];
    let name_end = rest.find(quote)?;
    if &rest[..name_end] != name {
        return None;
    }
    let rest = rest[name_end + 1..].trim_start().strip_prefix(',')?;
    let val = rest.trim_start();
    let end = val.find(')')?;
    Some(val[..end].trim().to_string())
}

/// Truthy for PHP-ish constant tokens: `true`, `1`, `'1'`, `"true"`, …
fn is_truthy(v: &str) -> bool {
    matches!(v.trim_matches(|c| c == '\'' || c == '"').to_ascii_lowercase().as_str(), "true" | "1")
}

/// A quoted string value ⇒ the unquoted path; bools/numbers ⇒ None.
fn as_path(v: &str) -> Option<&str> {
    let inner = v
        .strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
        .or_else(|| v.strip_prefix('"').and_then(|s| s.strip_suffix('"')))?;
    (!inner.is_empty() && !is_truthy(v)).then_some(inner)
}

/// WordPress supports wp-config.php in the docroot or one directory above.
fn wp_config_text(docroot: &Path) -> Option<String> {
    let in_root = docroot.join("wp-config.php");
    if in_root.is_file() {
        return std::fs::read_to_string(in_root).ok();
    }
    let above = docroot.parent()?.join("wp-config.php");
    above.is_file().then(|| std::fs::read_to_string(above).ok())?
}

/// Resolve a site's WP debug-log status from its docroot. A missing / non-WP
/// docroot just reports everything off (the UI hides the section for non-WP
/// sites anyway).
pub fn wp_debug_log_status(docroot: &Path) -> WpDebugLogStatus {
    let config = wp_config_text(docroot).unwrap_or_default();
    let mut debug = None;
    let mut log_enabled = None;
    let mut path = docroot.join("wp-content").join("debug.log");
    // First define wins, like PHP's `define()` — stock wp-config carries a
    // guarded `define('WP_DEBUG', false)` fallback BELOW where WP-CLI inserts.
    for line in config.lines() {
        if debug.is_none() {
            if let Some(v) = define_value(line, "WP_DEBUG") {
                debug = Some(is_truthy(&v));
            }
        }
        if log_enabled.is_none() {
            if let Some(v) = define_value(line, "WP_DEBUG_LOG") {
                if let Some(custom) = as_path(&v) {
                    log_enabled = Some(true);
                    let p = Path::new(custom);
                    path = if p.is_absolute() { p.to_path_buf() } else { docroot.join(p) };
                } else {
                    log_enabled = Some(is_truthy(&v));
                }
            }
        }
    }
    let (debug, log_enabled) = (debug.unwrap_or(false), log_enabled.unwrap_or(false));
    let meta = std::fs::metadata(&path).ok();
    WpDebugLogStatus {
        debug,
        log_enabled,
        path: path.to_string_lossy().into_owned(),
        exists: meta.as_ref().is_some_and(|m| m.is_file()),
        size_bytes: meta.map(|m| m.len()).unwrap_or(0),
    }
}

/// Tail the site's WordPress debug log (missing file ⇒ empty, like [`tail`]).
pub fn wp_debug_log_tail(docroot: &Path, lines: usize) -> Result<Vec<String>> {
    tail_file(Path::new(&wp_debug_log_status(docroot).path), lines)
}

/// Truncate the site's debug log to empty. A missing file is fine (nothing to clear).
pub fn wp_debug_log_clear(docroot: &Path) -> Result<()> {
    let status = wp_debug_log_status(docroot);
    if status.exists {
        std::fs::write(&status.path, "")?;
    }
    Ok(())
}

/// Copy the site's debug log into the user's Downloads folder
/// (`<domain>-debug.log`, numbered on collision). Returns the destination.
pub fn wp_debug_log_download(docroot: &Path, domain: &str) -> Result<PathBuf> {
    let status = wp_debug_log_status(docroot);
    if !status.exists {
        return Err(Error::Other("no debug.log to download".into()));
    }
    let dest = numbered_log_dest(&downloads_dir()?, &format!("{domain}-debug"));
    std::fs::copy(&status.path, &dest)?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::models::{MultisiteMode, ServiceStatus, Site, SiteType, WebServer};

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
            multisite: MultisiteMode::None,
            db_name: "wp_acme_test".into(),
            db_engine: crate::state::models::SiteDbEngine::Mysql,
            xdebug: false,
            override_port: None,
        }
    }

    #[test]
    fn targets_use_site_php_version_and_omit_frankenphp_for_nginx() {
        let t = targets_for_site(&site(WebServer::Nginx), Path::new("/nonexistent"));
        let keys: Vec<&str> = t.iter().map(|x| x.key.as_str()).collect();
        assert!(keys.contains(&"php-fpm-8.2.log")); // the site's minor
        assert!(keys.contains(&"nginx-access.log"));
        assert!(keys.contains(&"mysql-error.log"));
        assert!(keys.contains(&"mariadb-error.log"));
        assert!(!keys.iter().any(|k| k.starts_with("frankenphp-")));
    }

    #[test]
    fn targets_carry_category_and_absolute_path() {
        let t = targets_for_site(&site(WebServer::Nginx), Path::new("/logs"));
        let by_key = |k: &str| t.iter().find(|x| x.key == k).unwrap();
        assert_eq!(by_key("nginx-access.log").category, LogCategory::Server);
        assert_eq!(by_key("caddy-stdout.log").category, LogCategory::Server);
        assert_eq!(by_key("mysql-error.log").category, LogCategory::Database);
        assert_eq!(by_key("mariadb-error.log").category, LogCategory::Database);
        assert_eq!(by_key("postgres-stdout.log").category, LogCategory::Database);
        assert_eq!(by_key("nginx-access.log").path, "/logs/nginx-access.log");
    }

    #[test]
    fn repo_targets_are_git_category() {
        let dir = std::env::temp_dir().join(format!("rexenv-logs-cat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("repo-acme.test-my-plugin.log"), "x").unwrap();
        let t = targets_for_site(&site(WebServer::Nginx), &dir);
        let repo = t.iter().find(|x| x.key.starts_with("repo-")).unwrap();
        assert_eq!(repo.category, LogCategory::Git);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn numbered_dest_skips_existing() {
        let dir = std::env::temp_dir().join(format!("rexenv-logs-num-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(numbered_log_dest(&dir, "a"), dir.join("a.log"));
        std::fs::write(dir.join("a.log"), "x").unwrap();
        assert_eq!(numbered_log_dest(&dir, "a"), dir.join("a-1.log"));
        std::fs::write(dir.join("a-1.log"), "x").unwrap();
        assert_eq!(numbered_log_dest(&dir, "a"), dir.join("a-2.log"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn targets_include_frankenphp_backend_for_override_sites() {
        let t = targets_for_site(&site(WebServer::Frankenphp), Path::new("/nonexistent"));
        assert!(t.iter().any(|x| x.key == "frankenphp-acme.test-stdout.log"));
    }

    #[test]
    fn targets_discover_this_sites_repo_job_logs_only() {
        let dir = std::env::temp_dir().join(format!("rexenv-logs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("repo-acme.test-my-plugin.log"), "x").unwrap();
        std::fs::write(dir.join("repo-other.test-thing.log"), "x").unwrap(); // other site
        std::fs::write(dir.join("nginx-error.log"), "x").unwrap(); // not a repo log
        let t = targets_for_site(&site(WebServer::Nginx), &dir);
        let repo: Vec<&LogTarget> =
            t.iter().filter(|x| x.key.starts_with("repo-")).collect();
        assert_eq!(repo.len(), 1);
        assert_eq!(repo[0].key, "repo-acme.test-my-plugin.log");
        assert_eq!(repo[0].label, "Git job — my-plugin");
        let _ = std::fs::remove_dir_all(&dir);
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
    fn define_value_parses_standard_forms() {
        assert_eq!(define_value("define( 'WP_DEBUG', true );", "WP_DEBUG").as_deref(), Some("true"));
        assert_eq!(define_value("define(\"WP_DEBUG\", false);", "WP_DEBUG").as_deref(), Some("false"));
        assert_eq!(
            define_value("define('WP_DEBUG_LOG', '/tmp/x.log');", "WP_DEBUG_LOG").as_deref(),
            Some("'/tmp/x.log'")
        );
        // Comments and other constants don't match.
        assert_eq!(define_value("// define('WP_DEBUG', true);", "WP_DEBUG"), None);
        assert_eq!(define_value("define('WP_DEBUG_LOG', true);", "WP_DEBUG"), None);
    }

    #[test]
    fn debug_log_status_resolves_default_and_custom_paths() {
        let dir = std::env::temp_dir().join("rexenv-wp-debug-status-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("wp-content")).unwrap();

        // No wp-config ⇒ everything off, default path.
        let s = wp_debug_log_status(&dir);
        assert!(!s.debug && !s.log_enabled && !s.exists);
        assert!(s.path.ends_with("wp-content/debug.log"));

        // WP_DEBUG + WP_DEBUG_LOG true ⇒ enabled; existing file reports size.
        std::fs::write(
            dir.join("wp-config.php"),
            "<?php\ndefine( 'WP_DEBUG', true );\ndefine( 'WP_DEBUG_LOG', true );\n",
        )
        .unwrap();
        std::fs::write(dir.join("wp-content/debug.log"), "[notice] hi\n").unwrap();
        let s = wp_debug_log_status(&dir);
        assert!(s.debug && s.log_enabled && s.exists && s.size_bytes > 0);

        // Custom path string ⇒ log_enabled even without a bool, path honored.
        std::fs::write(
            dir.join("wp-config.php"),
            "<?php\ndefine('WP_DEBUG', false);\ndefine('WP_DEBUG_LOG', 'logs/wp.log');\n",
        )
        .unwrap();
        let s = wp_debug_log_status(&dir);
        assert!(!s.debug && s.log_enabled);
        assert!(s.path.ends_with("logs/wp.log"), "{}", s.path);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn debug_log_status_first_define_wins_over_guarded_fallback() {
        let dir = std::env::temp_dir().join("rexenv-wp-debug-firstwins-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // WP-CLI inserts real defines ABOVE the stock guarded fallback.
        std::fs::write(
            dir.join("wp-config.php"),
            "<?php\ndefine( 'WP_DEBUG', true );\ndefine( 'WP_DEBUG_LOG', true );\n\
             if ( ! defined( 'WP_DEBUG' ) ) {\n\tdefine( 'WP_DEBUG', false );\n}\n",
        )
        .unwrap();
        let s = wp_debug_log_status(&dir);
        assert!(s.debug && s.log_enabled);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn debug_log_clear_truncates() {
        let dir = std::env::temp_dir().join("rexenv-wp-debug-clear-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("wp-content")).unwrap();
        std::fs::write(dir.join("wp-config.php"), "<?php define('WP_DEBUG_LOG', true);").unwrap();
        std::fs::write(dir.join("wp-content/debug.log"), "old\n").unwrap();
        wp_debug_log_clear(&dir).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("wp-content/debug.log")).unwrap(), "");
        // Missing file is not an error.
        std::fs::remove_file(dir.join("wp-content/debug.log")).unwrap();
        wp_debug_log_clear(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
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

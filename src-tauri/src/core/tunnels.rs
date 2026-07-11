//! core::tunnels — per-site public sharing via cloudflared quick tunnels (§9.1).
//!
//! A "quick tunnel" exposes ONE site publicly: cloudflared dials out to Cloudflare
//! and reverse-proxies a `https://<random>.trycloudflare.com` URL to a local
//! origin. The origin is the **shared nginx HTTP port** with the site's `Host`
//! (`--http-host-header`) — a plain-HTTP origin, so there's no local-CA
//! origin-trust problem and no `--no-tls-verify`; cloudflared provides the
//! external TLS. The tunnel is scoped to that single Host: it never points at the
//! edge wildcard or an internal tooling vhost (`adminer.rexenv.rex`, Mailpit…),
//! so sharing one site can't expose another site or a tool. Platform-agnostic:
//! spawns via `ProcessSupervisor` only.

use crate::error::Result;
use crate::platform::traits::Platform;
use std::path::PathBuf;
use std::process::Child;

/// Per-site tunnel log (cloudflared's stdout+stderr; the public URL is parsed
/// from here).
pub fn log_path(platform: &dyn Platform, domain: &str) -> Result<PathBuf> {
    Ok(platform.paths().log_dir()?.join(format!("tunnel-{domain}.log")))
}

/// Start a quick tunnel for `domain` against the shared nginx port (with the site
/// Host). Returns the supervised child; poll [`read_url`] for the public URL.
pub fn start(
    platform: &dyn Platform,
    cloudflared_bin: &std::path::Path,
    domain: &str,
    nginx_http_port: u16,
) -> Result<Child> {
    let log = log_path(platform, domain)?;
    if let Some(parent) = log.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Truncate any stale log so we parse THIS run's URL.
    let _ = std::fs::write(&log, b"");
    let args = vec![
        "tunnel".to_string(),
        "--no-autoupdate".to_string(),
        "--url".to_string(),
        format!("http://127.0.0.1:{nginx_http_port}"),
        "--http-host-header".to_string(),
        domain.to_string(),
    ];
    platform.supervisor().spawn_logged(cloudflared_bin, &args, &log)
}

/// Stop a running tunnel by pid.
pub fn stop(platform: &dyn Platform, pid: u32) -> Result<()> {
    platform.supervisor().stop(pid)
}

/// One-shot: read the tunnel log and return the public URL if cloudflared has
/// printed it yet. Callers poll this until `Some`.
pub fn read_url(platform: &dyn Platform, domain: &str) -> Option<String> {
    let log = log_path(platform, domain).ok()?;
    let text = std::fs::read_to_string(&log).ok()?;
    extract_url(&text)
}

/// Pull the first `https://<sub>.trycloudflare.com` out of cloudflared output
/// (it's printed inside a `|`-bordered banner).
pub fn extract_url(text: &str) -> Option<String> {
    text.split(|c: char| c.is_whitespace() || c == '|')
        .map(str::trim)
        .find(|t| t.starts_with("https://") && t.ends_with(".trycloudflare.com"))
        .map(|t| t.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_url_from_banner() {
        let banner = "\
2026-06-29T10:00:00Z INF +--------------------------------------------------+
2026-06-29T10:00:00Z INF |  Your quick Tunnel has been created! Visit it at  |
2026-06-29T10:00:00Z INF |  https://blue-cat-runs-fast.trycloudflare.com     |
2026-06-29T10:00:00Z INF +--------------------------------------------------+";
        assert_eq!(
            extract_url(banner).as_deref(),
            Some("https://blue-cat-runs-fast.trycloudflare.com")
        );
    }

    #[test]
    fn extract_url_none_before_ready() {
        assert!(extract_url("INF Starting tunnel...\nINF Requesting new quick tunnel...").is_none());
    }
}

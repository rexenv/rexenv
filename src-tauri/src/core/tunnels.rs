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

/// What we can honestly say about a live tunnel's public URL (§ status
/// honesty, step 2). One fact per state — the UI badge reads this and nothing
/// else:
/// - `Unverified` — the process runs, but the URL hasn't answered a check
///   (yet, or the last check couldn't reach Cloudflare at all).
/// - `Reachable` — the URL answered a probe THROUGH the tunnel path.
/// - `Broken` — Cloudflare's edge has repeatedly said the tunnel is gone
///   (HTTP 530 / error 1033) while the process still runs. Positive evidence,
///   distinct from "can't verify".
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TunnelHealth {
    Unverified,
    Reachable,
    Broken,
}

/// Positive identification of a recorded tunnel pid (lifecycle ruling
/// 28 Jul 2026): the live process's command line must reference our binary
/// name AND the app-data dir (the cloudflared binary lives under it — the
/// standard ownership marker) AND carry the exact `--http-host-header
/// <domain>` argument pair for THIS row's domain. The pair is matched on
/// whitespace-split tokens, not substrings, so `a.rex` never matches a
/// tunnel for `a.rexx`. Anything less than all three is NOT ours — most
/// importantly a recycled pid now naming some unrelated process, which must
/// only ever get file/row cleanup, never a signal.
pub fn is_our_tunnel(command: &str, app_data_marker: &str, domain: &str) -> bool {
    if app_data_marker.is_empty()
        || !command.contains("cloudflared")
        || !command.contains(app_data_marker)
    {
        return false;
    }
    let toks: Vec<&str> = command.split_whitespace().collect();
    toks.windows(2).any(|w| w[0] == "--http-host-header" && w[1] == domain)
}

/// Launch-time sweep: settle every tunnel row a crashed session left behind
/// (tunnels DIE WITH THE APP — a clean exit clears the table, so any row here
/// is a crash survivor). Per row: kill the pid only on [`is_our_tunnel`]
/// identification; in EVERY branch — identified, dead, or recycled pid —
/// remove the row's mu-plugin and delete the record. Returns how many live
/// tunnels were killed (callers log it).
pub fn sweep_startup(conn: &rusqlite::Connection, platform: &dyn Platform) -> u32 {
    let rows = match crate::state::store::list_tunnels(conn) {
        Ok(rows) if !rows.is_empty() => rows,
        _ => return 0,
    };
    let marker = platform
        .paths()
        .app_data_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let mut killed = 0u32;
    for row in rows {
        let ours = platform
            .supervisor()
            .pid_command(row.pid)
            .map(|cmd| is_our_tunnel(&cmd, &marker, &row.domain))
            .unwrap_or(false);
        if ours {
            log::warn!(
                "tunnels: killing the orphaned tunnel for {} (pid {}) — a prior session \
                 crashed while sharing; tunnels die with the app",
                row.domain,
                row.pid
            );
            let _ = platform.supervisor().stop(row.pid);
            killed += 1;
        }
        if let Err(e) = crate::core::wp_tunnel::disable(std::path::Path::new(&row.docroot)) {
            log::warn!("tunnels: could not remove the mu-plugin for {}: {e}", row.domain);
        }
        let _ = crate::state::store::delete_tunnel(conn, &row.domain);
    }
    killed
}

#[cfg(test)]
mod tests {
    use super::*;

    const MARKER: &str = "/Users/dev/Library/Application Support/dev.rexenv.rexenv";

    #[test]
    fn is_our_tunnel_accepts_only_the_full_identity() {
        let ours = format!(
            "{MARKER}/bin/cloudflared-2026.6.1/cloudflared tunnel --no-autoupdate \
             --url http://127.0.0.1:18088 --http-host-header acme.rex"
        );
        assert!(is_our_tunnel(&ours, MARKER, "acme.rex"));
        // Same process is NOT the identity for a different row's domain.
        assert!(!is_our_tunnel(&ours, MARKER, "other.rex"));
        // Exact-token match: a lookalike domain must not pass on prefix.
        let lookalike = ours.replace("acme.rex", "acme.rexx");
        assert!(!is_our_tunnel(&lookalike, MARKER, "acme.rex"));
    }

    #[test]
    fn is_our_tunnel_rejects_recycled_and_foreign_processes() {
        // The branch that would be silently wrong: a recycled pid now naming
        // an unrelated process must never identify as ours.
        for foreign in [
            "vim notes.txt",
            "",
            // The USER'S own cloudflared (homebrew) sharing the same domain —
            // no app-data marker, so not ours to kill.
            "/opt/homebrew/bin/cloudflared tunnel --url http://127.0.0.1:3000 \
             --http-host-header acme.rex",
        ] {
            assert!(!is_our_tunnel(foreign, MARKER, "acme.rex"), "identified: {foreign}");
        }
        // An empty marker must never wildcard-match.
        assert!(!is_our_tunnel("cloudflared --http-host-header acme.rex", "", "acme.rex"));
    }

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

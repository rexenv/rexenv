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

/// Refuse a tunnel for any site the shared nginx does not serve (step 3,
/// 28 Jul 2026). The tunnel's origin is nginx `:18088` routed by Host header;
/// an Apache/FrankenPHP override site has NO nginx vhost, so its Host would
/// fall through to nginx's DEFAULT server — we would publish a DIFFERENT
/// site's content on the public URL. Cross-site exposure, so starting must be
/// impossible, not discouraged: this runs in core, ahead of every IPC and CLI
/// path, and reads the SAME predicate the nginx config generator uses
/// (`sites::is_nginx_served`) so eligibility can never drift from reality.
/// Fixable later by originating from the site's own recorded backend port —
/// the refusal says "yet" truthfully.
pub fn ensure_tunnelable(site: &crate::state::models::Site) -> Result<()> {
    if crate::core::sites::is_nginx_served(site) {
        return Ok(());
    }
    Err(crate::error::Error::Other(format!(
        "{domain} can't be shared yet: it runs on {server}, and tunnels currently \
         originate from the shared nginx — starting one would publish whatever \
         nginx's default site answers with, which is a DIFFERENT site. Switch the \
         site's web server to nginx to share it.",
        domain = site.domain,
        server = site.web_server.as_db(),
    )))
}

/// A single health probe's raw result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// The URL answered with this HTTP status.
    Status(u16),
    /// No HTTP response at all (DNS/connect/timeout) — proves nothing about
    /// the tunnel: OUR connectivity may be the problem.
    TransportError,
}

/// Consecutive tunnel-gone (530) responses before the verdict is `Broken`.
/// One 530 is normal while cloudflared reconnects to the edge; three checks
/// (~90s at the 30s cadence) past that means the registration isn't coming
/// back for this quick tunnel.
pub const BROKEN_AFTER: u32 = 3;

/// Fold a probe outcome into the next `(health, strikes)`.
///
/// Any HTTP response EXCEPT 530 proves the tunnel path: the edge accepted the
/// hostname and something behind the tunnel answered — a WP 404/500, or
/// cloudflared's own 502 for a stopped local origin, all rode the tunnel to
/// get here. Origin health is the Services page's fact; this badge reads ONE
/// fact, the tunnel's. HTTP 530 is Cloudflare's tunnel-level failure (error
/// 1033, "could not resolve the tunnel") — positive evidence AGAINST, counted
/// as a strike. Transport errors change nothing: not evidence for, not
/// evidence against (strikes carry through, so a flapping edge can't dodge
/// `Broken` by timing out between 530s).
pub fn fold_probe(prev_strikes: u32, outcome: ProbeOutcome) -> (TunnelHealth, u32) {
    match outcome {
        ProbeOutcome::Status(530) => {
            let strikes = prev_strikes.saturating_add(1);
            if strikes >= BROKEN_AFTER {
                (TunnelHealth::Broken, strikes)
            } else {
                (TunnelHealth::Unverified, strikes)
            }
        }
        ProbeOutcome::Status(_) => (TunnelHealth::Reachable, 0),
        ProbeOutcome::TransportError => (TunnelHealth::Unverified, prev_strikes),
    }
}

/// One bounded HEAD against the public URL. The caller supplies a client with
/// connect/total timeouts baked in and runs this OUTSIDE any registry lock —
/// status snapshots must stay instant whatever the network does.
pub async fn probe_url(client: &reqwest::Client, url: &str) -> ProbeOutcome {
    match client.head(url).send().await {
        Ok(resp) => ProbeOutcome::Status(resp.status().as_u16()),
        Err(_) => ProbeOutcome::TransportError,
    }
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
    fn ensure_tunnelable_walls_off_override_sites() {
        use crate::state::models::*;
        let site = |ws: WebServer| Site {
            id: "t1".into(),
            name: "Acme".into(),
            domain: "acme.rex".into(),
            site_type: SiteType::Wordpress,
            status: ServiceStatus::Stopped,
            php_version: "8.3".into(),
            web_server: ws,
            ssl: true,
            path: "/sites/acme".into(),
            created_at: "now".into(),
            multisite: MultisiteMode::None,
            db_name: "wp_acme_rex".into(),
            db_engine: SiteDbEngine::Mysql,
            xdebug: false,
            override_port: None,
            provisioned: true,
            docroot_managed: Some(true),
            db_created: None,
        };
        assert!(ensure_tunnelable(&site(WebServer::Nginx)).is_ok());
        for ws in [WebServer::Apache, WebServer::Frankenphp] {
            let msg = ensure_tunnelable(&site(ws)).unwrap_err().to_string();
            // The reason must be specific: the site's own name, its server,
            // and WHY (the default-vhost exposure), not a generic unsupported.
            for needle in ["acme.rex", ws.as_db(), "DIFFERENT site"] {
                assert!(msg.contains(needle), "{ws:?} message missing {needle:?}: {msg}");
            }
        }
    }

    #[test]
    fn fold_probe_reads_one_fact_honestly() {
        use ProbeOutcome::{Status, TransportError};
        use TunnelHealth::{Broken, Reachable, Unverified};
        // Any HTTP answer except 530 proves the path — including origin-side
        // errors that rode the tunnel to reach us.
        for code in [200u16, 301, 404, 405, 500, 502] {
            assert_eq!(fold_probe(2, Status(code)), (Reachable, 0), "status {code}");
        }
        // 530s escalate: reconnect-tolerant, then Broken on positive evidence.
        assert_eq!(fold_probe(0, Status(530)), (Unverified, 1));
        assert_eq!(fold_probe(1, Status(530)), (Unverified, 2));
        assert_eq!(fold_probe(2, Status(530)), (Broken, 3));
        assert_eq!(fold_probe(3, Status(530)), (Broken, 4)); // stays broken
        // Transport errors are non-evidence: verdict Unverified, strikes kept
        // (a timeout between 530s must not reset the count).
        assert_eq!(fold_probe(0, TransportError), (Unverified, 0));
        assert_eq!(fold_probe(2, TransportError), (Unverified, 2));
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

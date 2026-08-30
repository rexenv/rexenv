//! core::tunnels — per-site public sharing via cloudflared quick tunnels (§9.1).
//!
//! A "quick tunnel" exposes ONE site publicly: cloudflared dials out to Cloudflare
//! and reverse-proxies a `https://<random>.trycloudflare.com` URL to a local
//! origin. The origin is the **backend that serves THAT site** ([`origin_port`]:
//! the shared nginx HTTP port for vhosted sites, the site's own recorded
//! override port for Apache/FrankenPHP sites) with the site's `Host`
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
use std::time::Duration;

/// Per-site tunnel log (cloudflared's stdout+stderr; the public URL is parsed
/// from here).
pub fn log_path(platform: &dyn Platform, domain: &str) -> Result<PathBuf> {
    Ok(platform.paths().log_dir()?.join(format!("tunnel-{domain}.log")))
}

/// Start a quick tunnel for `domain` against the site's own origin (with the
/// site Host). Returns the supervised child; poll [`read_url`] for the public
/// URL.
pub fn start(
    platform: &dyn Platform,
    cloudflared_bin: &std::path::Path,
    domain: &str,
    // The site's own serving backend — [`origin_port`], never a raw port a
    // caller picked (nginx for vhosted sites, the recorded override port for
    // Apache/FrankenPHP sites).
    origin_port: u16,
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
        format!("http://127.0.0.1:{origin_port}"),
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
/// (it's printed inside a `|`-bordered banner). `api.trycloudflare.com` is
/// cloudflared's REGISTRATION endpoint, not a tunnel URL — an error line
/// printing it as a bare host must never be recorded as the public URL
/// (audit A7).
pub fn extract_url(text: &str) -> Option<String> {
    text.split(|c: char| c.is_whitespace() || c == '|')
        .map(str::trim)
        .find(|t| {
            t.starts_with("https://")
                && t.ends_with(".trycloudflare.com")
                && !t.starts_with("https://api.")
        })
        .map(|t| t.to_string())
}

/// What we can honestly say about a live tunnel's public URL (§ status
/// honesty, step 2; semantics sharpened by the 28 Jul 2026 audit). One fact
/// per state — the UI badge reads this and nothing else:
/// - `Unverified` — the URL didn't answer the last check: still coming up,
///   OUR network is down, or the tunnel has dropped. This is the REALISTIC
///   TERMINAL state for a dropped tunnel: `trycloudflare.com` has no
///   wildcard DNS (verified live, 28 Jul 2026 — an unregistered subdomain
///   doesn't resolve), so once a dead tunnel's DNS record is gone, probes
///   fail at DNS and no HTTP verdict is possible.
/// - `Reachable` — the URL answered a probe THROUGH the tunnel path.
/// - `Broken` — Cloudflare's edge answered HTTP 530 (error 1033, tunnel
///   gone) repeatedly while the process runs. Positive evidence, distinct
///   from "can't verify" — reachable only in the deregistration window while
///   DNS still resolves, so most dead tunnels read Unverified, not Broken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TunnelHealth {
    Unverified,
    Reachable,
    Broken,
}

/// Row pid before the child exists: a claim is taken BEFORE binary resolve
/// and spawn (step 4 — the claim also gates the shared log truncation), so a
/// row can briefly name no process. `u32::MAX` is structurally inert — it can
/// never be a real pid, `kill` on it fails harmlessly if a guard is ever
/// missed, and `pid_command` finds nothing so the sweep treats it as dead.
pub const PID_PENDING: u32 = u32::MAX;

/// The loopback ORIGIN a tunnel for `site` publishes (step 3, 28 Jul 2026;
/// per-backend origins built 15 Aug 2026, replacing the "can't be shared yet"
/// refusal this used to be).
///
/// An nginx-served site's origin is the shared nginx HTTP port, routed by the
/// Host header `--http-host-header` pins. An Apache/FrankenPHP override site
/// has NO nginx vhost — a request carrying its Host falls through to nginx's
/// DEFAULT server, which is a DIFFERENT site's content (#13 measured the
/// fallthrough live) — so its origin is its OWN backend port, read through
/// `sites::recorded_override_port`, the SAME accessor the config generator
/// serves from. That sameness is the safety property: whatever port actually
/// serves this site is the only port a tunnel for it can publish, and the
/// cross-site case is unrepresentable rather than refused. A site whose
/// override port cannot be determined refuses rather than guessing.
///
/// Origin selection says nothing about the backend being UP — that is a
/// liveness fact the ServiceManager owns, checked (as a courtesy snapshot,
/// not a wall) at the one start path in `commands::tunnels`.
pub fn origin_port(site: &crate::state::models::Site) -> Result<u16> {
    if crate::core::sites::is_nginx_served(site) {
        return Ok(crate::core::services::NGINX_HTTP_PORT);
    }
    crate::core::sites::recorded_override_port(site).ok_or_else(|| {
        crate::error::Error::Other(format!(
            "{domain} runs on {server} but has no recorded backend port to share — \
             re-save the site's web server (Site → Settings), then share it.",
            domain = site.domain,
            server = site.web_server.as_db(),
        ))
    })
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
/// Any HTTP response EXCEPT 530 proves the tunnel path — on the honest
/// assumption that nothing between this machine and Cloudflare forges HTTP
/// (a captive portal would; ledger #15 marks that premise unprovable, and a
/// forged answer here can only OVER-claim Live, never mask Broken): the edge
/// accepted the hostname and something behind the tunnel answered — a WP
/// 404/500, a 3xx
/// (redirects are NOT followed — the first hop's status is the fact), or
/// cloudflared's own 502 for a stopped local origin, all rode the tunnel to
/// get here. Origin health is the Services page's fact; this badge reads ONE
/// fact, the tunnel's. HTTP 530 is Cloudflare's tunnel-level failure (error
/// 1033) — positive evidence AGAINST, counted as a strike.
///
/// Transport errors are NON-EVIDENCE and may never downgrade a verdict
/// (audit A4): strikes carry through, and a confirmed `Broken` stays Broken
/// — post-drop DNS expiry turns probes into transport errors, and that must
/// not soften the verdict the 530s already proved. `Reachable` DOES decay to
/// `Unverified` on a transport error: it is a freshness claim ("answered a
/// probe"), and holding it green through an outage would over-claim.
pub fn fold_probe(
    prev_health: TunnelHealth,
    prev_strikes: u32,
    outcome: ProbeOutcome,
) -> (TunnelHealth, u32) {
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
        ProbeOutcome::TransportError => {
            let health = if prev_health == TunnelHealth::Broken {
                TunnelHealth::Broken
            } else {
                TunnelHealth::Unverified
            };
            (health, prev_strikes)
        }
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

/// Why the primary probe couldn't reach the URL — the failure-gated
/// diagnosis (ruled 28 Jul 2026). One fact for the LINE under the badge; the
/// badge itself stays anchored to what a click from THIS machine experiences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TunnelDiagnosis {
    /// Cloudflare's edge answers for the hostname (DNS bypassed) — the
    /// tunnel works; this machine's own resolution/connection is what's
    /// failing (fresh-hostname DNS lag is the usual cause).
    LocalDnsBehind,
    /// The edge answers but 1.1.1.1 doesn't have the name yet — registration
    /// is live, public DNS is still propagating.
    DnsPropagating,
    /// The edge itself answered 530: the registration is gone. Feeds the
    /// strike counter — dead at the edge is dead everywhere.
    EdgeGone,
    /// Neither 1.1.1.1 nor the edge reachable: this machine looks offline;
    /// nothing about the tunnel can be honestly claimed.
    Offline,
}

/// Bounded A-record lookup DIRECTLY at 1.1.1.1 — and only 1.1.1.1, by
/// ruling: the hostname is Cloudflare-issued and this machine already holds
/// a QUIC connection to Cloudflare for the tunnel itself, so the query
/// discloses nothing to a party that doesn't already know it; a second
/// resolver would add a NEW third party (on a 30s schedule, while failing)
/// for zero diagnostic gain on a binary question. Returns `None` when
/// 1.1.1.1 didn't answer at all (offline-shaped), `Some(vec)` — possibly
/// empty — when it did.
pub async fn resolve_at_1111(host: &str) -> Option<Vec<std::net::Ipv4Addr>> {
    use hickory_proto::op::{Message, MessageType, OpCode, Query};
    use hickory_proto::rr::{Name, RData, RecordType};
    use hickory_proto::serialize::binary::BinDecodable;

    let name = Name::from_utf8(host).ok()?;
    let mut msg = Message::new();
    // Query id from the clock's low bits — std-only; anti-spoofing rigor is
    // not the threat model for a diagnostic asking a fixed resolver about an
    // already-public name.
    let id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.subsec_nanos() & 0xFFFF) as u16)
        .unwrap_or(0x5150);
    msg.set_id(id)
        .set_message_type(MessageType::Query)
        .set_op_code(OpCode::Query)
        .set_recursion_desired(true)
        .add_query(Query::query(name, RecordType::A));
    let bytes = msg.to_vec().ok()?;

    let sock = tokio::net::UdpSocket::bind("0.0.0.0:0").await.ok()?;
    sock.send_to(&bytes, "1.1.1.1:53").await.ok()?;
    let mut buf = [0u8; 512];
    let n = tokio::time::timeout(Duration::from_secs(3), sock.recv(&mut buf))
        .await
        .ok()?
        .ok()?;
    let reply = Message::from_bytes(&buf[..n]).ok()?;
    if reply.id() != id {
        return None; // not our answer — treat as no answer
    }
    Some(
        reply
            .answers()
            .iter()
            .filter_map(|r| match r.data() {
                Some(RData::A(a)) => Some(a.0),
                _ => None,
            })
            .collect(),
    )
}

/// The two failure-gated checks, run ONLY after the primary probe transport-
/// failed (a healthy tunnel generates zero extra traffic, forever — ruled).
/// Returns (does 1.1.1.1 have the name — None if 1.1.1.1 unreachable,
/// edge HTTP status — None if no connection).
///
/// The edge IP comes from the LIVE 1.1.1.1 answer, falling back to a live
/// apex (`trycloudflare.com`) lookup — NEVER a constant: a hardcoded edge IP
/// rots silently and measures nothing (ruled).
///
/// HONEST LIMIT (anycast): a passing edge check proves "the Cloudflare POP
/// nearest THIS machine routes the registration and the origin answers" —
/// NOT that every POP on earth does, and not that any specific visitor's
/// ISP, resolver, or network path works. Wording built on this must stay
/// scoped; the second-device check remains the only true end-to-end test.
pub async fn diagnose_unreachable(host: &str) -> (Option<bool>, Option<u16>) {
    let answer = resolve_at_1111(host).await;
    let public_resolves = answer.as_ref().map(|ips| !ips.is_empty());
    let edge_ip = match answer.as_ref().and_then(|ips| ips.first().copied()) {
        Some(ip) => Some(ip),
        None => resolve_at_1111("trycloudflare.com")
            .await
            .and_then(|ips| ips.first().copied()),
    };
    let Some(ip) = edge_ip else {
        return (public_resolves, None);
    };
    // TLS verifies the REAL hostname against Cloudflare's *.trycloudflare.com
    // wildcard cert — nothing is loosened; only the address lookup is pinned.
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .resolve(host, std::net::SocketAddr::new(ip.into(), 443))
        .build();
    let Ok(client) = client else {
        return (public_resolves, None);
    };
    let status = client
        .head(format!("https://{host}/"))
        .send()
        .await
        .ok()
        .map(|r| r.status().as_u16());
    (public_resolves, status)
}

/// Fold the failure-gated checks into the one diagnosis the UI line shows.
pub fn fold_diagnosis(public_resolves: Option<bool>, edge_status: Option<u16>) -> TunnelDiagnosis {
    match (public_resolves, edge_status) {
        (_, Some(530)) => TunnelDiagnosis::EdgeGone,
        (Some(false), Some(_)) => TunnelDiagnosis::DnsPropagating,
        (_, Some(_)) => TunnelDiagnosis::LocalDnsBehind,
        (_, None) => TunnelDiagnosis::Offline,
    }
}

/// How long a share may stay in Phase A (edge-only probing) before the
/// system-resolver gate opens regardless. This is NOT a propagation timeout —
/// propagation gaps last seconds, not minutes. It is the escape hatch for the
/// corner where 1.1.1.1 is unreachable (egress-blocked network) while system
/// DNS works fine: without it, that machine would sit in Phase A forever and
/// never regain `Reachable`. By the time it fires, the record has existed for
/// minutes, so a COMPLIANT resolver (RFC 2308 bounds negative TTL) can no
/// longer be holding an early-query negative-cache; a noncompliant router can
/// still lie longer — that residue is exactly what the second-device check
/// exists for (ledger #18).
pub const SYSTEM_PROBE_GATE_CAP: Duration = Duration::from_secs(300);

/// May the prober start asking the SYSTEM resolver about this hostname?
/// Yes once 1.1.1.1 provably has the record (a compliant resolver chain then
/// gets a positive answer — there is nothing left to negative-cache), or once
/// [`SYSTEM_PROBE_GATE_CAP`] passes (see its reasoning). Until then, one
/// system query from us would be the earliest query on the network by
/// construction, landing inside the propagation window Cloudflare's own
/// banner warns about — and `trycloudflare.com`'s SOA MINIMUM of 1800s means
/// that single query poisons the LAN's resolver for up to THIRTY MINUTES
/// (measured 28 Jul 2026; it is why a phone worked only on cellular).
pub fn gate_opens(public_resolves: Option<bool>, elapsed: Duration) -> bool {
    public_resolves == Some(true) || elapsed >= SYSTEM_PROBE_GATE_CAP
}

/// What one prober tick is ALLOWED to do — decided purely, so the property
/// "Phase A never produces a system-DNS query" is pinned at the decision
/// layer by its own test rather than implied by code shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbePlan {
    /// Phase A: 1.1.1.1 + pinned-address edge checks ONLY. A system-resolver
    /// query in this window is the bug this enum exists to prevent.
    EdgeOnly,
    /// Phase B: the normal system-path probe (plus the failure-gated
    /// diagnosis, as before).
    System,
}

/// The plan for a tick, from the gate flag the registry carries. The flag
/// only ever flips open (via [`gate_opens`]) — a share never re-enters
/// Phase A.
pub fn probe_plan(gate_open: bool) -> ProbePlan {
    if gate_open {
        ProbePlan::System
    } else {
        ProbePlan::EdgeOnly
    }
}

/// What the health fold sees after a diagnosis ran: an edge answer of 530
/// upgrades a transport error into the positive evidence it is — dead at the
/// edge is dead EVERYWHERE, which is what makes `Broken` reachable for the
/// common drop shape (DNS dies first; probes stop producing HTTP verdicts —
/// the audit's biggest honesty gap, partially closed here). DELIBERATELY
/// asymmetric: a positive edge status never upgrades toward `Reachable`,
/// because the badge anchors to what a click from THIS machine experiences,
/// and this machine's click still fails.
pub fn effective_outcome(primary: ProbeOutcome, edge_status: Option<u16>) -> ProbeOutcome {
    match (primary, edge_status) {
        (ProbeOutcome::TransportError, Some(530)) => ProbeOutcome::Status(530),
        (p, _) => p,
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
        // A crash between claim and spawn leaves the sentinel — no process
        // ever existed for it; cleanup only (the argv probe would agree, but
        // the sentinel must not even be looked up).
        let ours = row.pid != PID_PENDING
            && platform
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

/// The `--http-host-header` value out of a ps command line — token-pair
/// adjacency, same rule as [`is_our_tunnel`]'s match. `None` when the pair is
/// absent (we never spawn cloudflared without it, so no-pair = not a tunnel
/// we started).
pub fn host_header_domain(command: &str) -> Option<String> {
    let toks: Vec<&str> = command.split_whitespace().collect();
    toks.windows(2).find(|w| w[0] == "--http-host-header").map(|w| w[1].to_string())
}

/// Rowless-orphan backstop (ruled 28 Jul 2026, after the live diagnosis found
/// four pre-v23 fossils publicly serving for 9–15 days): kill any process
/// that is PROVABLY ours but has NO v23 row. The class is "the DB and the
/// process table disagree" — pre-v23 builds, an app-data reset, a restore
/// without rows, or any future record loss — and each instance is a public
/// share nothing else will ever reap.
///
/// Identity bar is EXACTLY the sweep's ([`is_our_tunnel`]: our app-data
/// binary path in argv + the exact `--http-host-header` token pair, with the
/// domain read from the argv itself since no record exists). A rowless
/// process that is not provably ours — a lookalike cloudflared from
/// elsewhere — is never touched. Runs AFTER [`sweep_startup`], so recorded
/// rows are already settled; `rows` re-checked defensively anyway. Each kill
/// logs at WARN with its weight: a public share the user didn't know about.
pub fn sweep_rowless(conn: &rusqlite::Connection, platform: &dyn Platform) -> u32 {
    let marker = platform
        .paths()
        .app_data_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    if marker.is_empty() {
        return 0;
    }
    let recorded: Vec<u32> = crate::state::store::list_tunnels(conn)
        .map(|rows| rows.iter().map(|r| r.pid).collect())
        .unwrap_or_default();
    let sites = crate::core::sites::list(conn).unwrap_or_default();
    let mut killed = 0u32;
    for pid in platform.supervisor().pids_named("cloudflared") {
        if recorded.contains(&pid) {
            continue; // the row sweep owns recorded pids
        }
        let Some(cmd) = platform.supervisor().pid_command(pid) else { continue };
        let Some(domain) = host_header_domain(&cmd) else { continue };
        if !is_our_tunnel(&cmd, &marker, &domain) {
            continue; // not provably ours — never touched
        }
        log::warn!(
            "tunnels: STOPPED A PUBLIC SHARE THIS APP HAD NO RECORD OF — {domain} (pid {pid}) \
             was serving publicly without rexenv's knowledge (pre-v23 build, app-data reset, \
             or a lost record). Share again from the Tunnels page if intended."
        );
        let _ = platform.supervisor().stop(pid);
        killed += 1;
        // The site may still exist (only the RECORD was lost) — its docroot
        // then holds a live-origin mu-plugin worth removing. No site row =
        // no known docroot = nothing reachable to clean.
        if let Some(site) = sites.iter().find(|s| s.domain == domain) {
            if let Err(e) = crate::core::wp_tunnel::disable(std::path::Path::new(&site.path)) {
                log::warn!("tunnels: could not remove the mu-plugin for {domain}: {e}");
            }
        }
    }
    killed
}

/// The `rexenv.log` line a share leaves BEHIND IT when it starts (ledger #430).
///
/// A public share is the only thing rexenv does that is visible from outside
/// this machine, and until 30 Aug 2026 starting one wrote NOTHING to the
/// app-wide log — only failures, crashes and sweeps did. So when a share for
/// `mstest.rex` was found running that nobody remembered starting (27 Aug
/// 2026), `rexenv.log` had no line for it: the evidence was in
/// `logs/tunnel-mstest.rex.log`, a file you only think to open once you
/// already know which domain to suspect, which is the thing you are trying to
/// find out. The line therefore carries the three facts that IDENTIFY an
/// exposure — the site, the public URL, the pid — plus the origin it points
/// at, so `grep tunnels: rexenv.log` answers "what was public, when, and
/// which process" from nothing.
pub fn share_started_line(domain: &str, url: &str, pid: u32, origin_port: u16) -> String {
    format!(
        "tunnels: SHARING {domain} PUBLICLY at {url} (pid {pid}, origin 127.0.0.1:{origin_port}) \
         — public until stopped"
    )
}

/// The closing half of [`share_started_line`]: a start line with no stop line
/// after it means the share was still up when the log ends. That only reads
/// as evidence if EVERY stop writes one, which is why the exit hook and the
/// in-flight branch log too.
pub fn share_stopped_line(domain: &str, pid: u32, ran_for: Duration) -> String {
    format!(
        "tunnels: stopped sharing {domain} (pid {pid}, was public for {}) — no longer reachable \
         from outside this machine",
        humanize(ran_for)
    )
}

/// Coarse, human-readable duration for the log ("47s", "3m 12s", "2h 05m").
/// Sub-second precision would be noise in a line about how long something was
/// exposed to the internet.
pub fn humanize(d: Duration) -> String {
    let secs = d.as_secs();
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m {:02}s", secs / 60, secs % 60),
        _ => format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MARKER: &str = "/Users/dev/Library/Application Support/dev.rexenv.rexenv";

    /// #430 — a share's log line identifies the exposure on its own.
    ///
    /// The test is about CONTENT, not wording: the reason the `mstest.rex`
    /// share could not be traced is that no line named it, so what has to hold
    /// is that the line names the site, the public URL and the pid — the three
    /// facts you need to go from "something is public" to "this process, this
    /// site, this link". A prettier sentence missing any of them is the bug.
    #[test]
    fn a_share_line_names_the_site_the_url_and_the_pid() {
        let line = share_started_line("mstest.rex", "https://odd-cat-42.trycloudflare.com", 78716, 18088);
        for fact in ["mstest.rex", "https://odd-cat-42.trycloudflare.com", "78716", "18088"] {
            assert!(
                line.contains(fact),
                "the start line drops {fact}, so a share found running cannot be traced from \
                 rexenv.log alone — the exact gap that left the 27 Aug 2026 mstest.rex share \
                 with no trail: {line}"
            );
        }
        // Loud, because a public exposure is not a routine INFO event.
        assert!(line.contains("PUBLICLY"), "the line no longer reads as an exposure: {line}");

        let stop = share_stopped_line("mstest.rex", 78716, Duration::from_secs(192));
        for fact in ["mstest.rex", "78716", "3m 12s"] {
            assert!(
                stop.contains(fact),
                "the stop line drops {fact}; a start with no matching stop is how the log says \
                 \"still public\", and that reading needs both halves to be identifiable: {stop}"
            );
        }
    }

    #[test]
    fn humanize_is_coarse_and_never_sub_second() {
        assert_eq!(humanize(Duration::from_millis(900)), "0s");
        assert_eq!(humanize(Duration::from_secs(59)), "59s");
        assert_eq!(humanize(Duration::from_secs(60)), "1m 00s");
        assert_eq!(humanize(Duration::from_secs(3599)), "59m 59s");
        assert_eq!(humanize(Duration::from_secs(3600)), "1h 00m");
        assert_eq!(humanize(Duration::from_secs(9000)), "2h 30m");
    }

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
    fn host_header_domain_needs_the_exact_pair() {
        assert_eq!(
            host_header_domain("/x/cloudflared tunnel --url http://127.0.0.1:18088 --http-host-header a.rex"),
            Some("a.rex".into())
        );
        assert_eq!(host_header_domain("/x/cloudflared tunnel --url http://127.0.0.1:18088"), None);
        assert_eq!(host_header_domain("vim --http-host-header"), None); // no value token
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

    /// A tunnel's origin is the backend that serves THAT site — never nginx's
    /// port for a site nginx has no vhost for, because a request carrying its
    /// Host falls through to nginx's DEFAULT server and a different site's
    /// content goes public (#13 measured the fallthrough live). The recorded
    /// override port wins over any derivation, and a site whose port cannot
    /// be determined refuses with the site's own name rather than guessing.
    #[test]
    fn origin_port_is_the_sites_own_backend_never_nginxs_default() {
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
            content_dir: None,
            mu_dir_created: None,
            origin: SiteOrigin::User,
            agent_client: None,
            expires_at: None,
            docroot_subdir: String::new(),
            git_url: None,
            git_ref: None,
            git_migrate: None,
            git_build_assets: None,
            starter_db: None,
        };
        // nginx-served → the shared nginx HTTP port, routed by Host.
        assert_eq!(
            origin_port(&site(WebServer::Nginx)).unwrap(),
            crate::core::services::NGINX_HTTP_PORT
        );
        for ws in [WebServer::Apache, WebServer::Frankenphp] {
            // The RECORDED port wins — it is what the config generator serves
            // from, so origin and reality cannot drift.
            let mut s = site(ws);
            s.override_port = Some(41234);
            assert_eq!(origin_port(&s).unwrap(), 41234, "{ws:?} recorded port");
            assert_ne!(
                origin_port(&s).unwrap(),
                crate::core::services::NGINX_HTTP_PORT,
                "an override site's origin must NEVER be nginx — that is the \
                 default-vhost cross-site exposure"
            );
            // No record → the same derived port the generator's accessor
            // yields, or a refusal naming the site — never nginx's port.
            let s = site(ws);
            match origin_port(&s) {
                Ok(p) => {
                    assert_eq!(
                        Some(p),
                        crate::core::sites::recorded_override_port(&s),
                        "{ws:?}: origin must be the generator's own answer"
                    );
                    assert_ne!(p, crate::core::services::NGINX_HTTP_PORT);
                }
                Err(e) => {
                    let msg = e.to_string();
                    for needle in ["acme.rex", ws.as_db()] {
                        assert!(msg.contains(needle), "{ws:?} refusal missing {needle:?}: {msg}");
                    }
                }
            }
        }
    }

    #[test]
    fn fold_probe_reads_one_fact_honestly() {
        use ProbeOutcome::{Status, TransportError};
        use TunnelHealth::{Broken, Reachable, Unverified};
        // Any HTTP answer except 530 proves the path — including origin-side
        // errors and unfollowed redirects that rode the tunnel to reach us.
        for code in [200u16, 301, 404, 405, 500, 502] {
            assert_eq!(fold_probe(Unverified, 2, Status(code)), (Reachable, 0), "status {code}");
        }
        // 530s escalate: reconnect-tolerant, then Broken on positive evidence.
        assert_eq!(fold_probe(Unverified, 0, Status(530)), (Unverified, 1));
        assert_eq!(fold_probe(Unverified, 1, Status(530)), (Unverified, 2));
        assert_eq!(fold_probe(Unverified, 2, Status(530)), (Broken, 3));
        assert_eq!(fold_probe(Broken, 3, Status(530)), (Broken, 4)); // stays broken
        // Transport errors are non-evidence and never DOWNGRADE a verdict
        // (A4): strikes carry, Broken is sticky — post-drop DNS expiry turns
        // probes into transport errors and must not soften what the 530s
        // proved. Reachable DECAYS (it's a freshness claim; green through an
        // outage would over-claim).
        assert_eq!(fold_probe(Unverified, 2, TransportError), (Unverified, 2));
        assert_eq!(fold_probe(Broken, 3, TransportError), (Broken, 3));
        assert_eq!(fold_probe(Reachable, 0, TransportError), (Unverified, 0));
        // Only positive evidence clears Broken.
        assert_eq!(fold_probe(Broken, 3, Status(200)), (Reachable, 0));
    }

    #[test]
    fn phase_a_never_plans_a_system_dns_query() {
        // THE property (ruled 28 Jul): our immediate post-start probe was the
        // earliest DNS query on the network by construction, inside the
        // propagation window, and trycloudflare's SOA MINIMUM (1800s) turned
        // that one query into a 30-minute LAN-wide dead link. Everything else
        // can look green while a reintroduced early system query brings the
        // bug back silently — so the decision layer is pinned here, loudly.
        //
        // Gate closed ⇒ EdgeOnly, unconditionally. There is no age, health,
        // or diagnosis input that may produce a system probe before the gate
        // opens — the plan takes ONLY the gate flag, by design.
        assert_eq!(probe_plan(false), ProbePlan::EdgeOnly);
        assert_eq!(probe_plan(true), ProbePlan::System);

        // And the gate itself opens ONLY on proof-of-record or the cap:
        use std::time::Duration as D;
        // fresh + not in public DNS (the poisoning window): stays closed.
        assert!(!gate_opens(Some(false), D::from_secs(1)));
        assert!(!gate_opens(None, D::from_secs(1))); // 1.1.1.1 unreachable
        assert!(!gate_opens(Some(false), SYSTEM_PROBE_GATE_CAP - D::from_secs(1)));
        // 1.1.1.1 has the record: a system query can no longer teach anyone
        // an NXDOMAIN — open.
        assert!(gate_opens(Some(true), D::from_secs(1)));
        // The escape-hatch cap (1.1.1.1 egress-blocked, system DNS fine):
        // open regardless, the record has existed for minutes.
        assert!(gate_opens(Some(false), SYSTEM_PROBE_GATE_CAP));
        assert!(gate_opens(None, SYSTEM_PROBE_GATE_CAP));
    }

    #[test]
    fn edge_530_makes_broken_reachable_after_dns_death() {
        // THE ruled assertion (28 Jul): the audit found Broken nearly
        // unreachable — a dead tunnel's DNS dies first, every probe becomes a
        // transport error, and strikes never accumulate. The DNS-bypassing
        // edge check restores the 530 verdict. This test IS that claim:
        // identical inputs, with and without the edge answer.
        use ProbeOutcome::TransportError;
        use TunnelHealth::{Broken, Unverified};

        // Without the diagnosis (today's shape): three DNS-dead ticks stay
        // Unverified forever.
        let (mut h, mut s) = (Unverified, 0);
        for _ in 0..3 {
            let (nh, ns) = fold_probe(h, s, effective_outcome(TransportError, None));
            h = nh;
            s = ns;
        }
        assert_eq!((h, s), (Unverified, 0), "DNS-dead without diagnosis can never strike");

        // With the edge answering 530 through the DNS-bypassing check: the
        // same three ticks reach Broken.
        let (mut h, mut s) = (Unverified, 0);
        for _ in 0..3 {
            let (nh, ns) = fold_probe(h, s, effective_outcome(TransportError, Some(530)));
            h = nh;
            s = ns;
        }
        assert_eq!((h, s), (Broken, 3), "edge 530s must accumulate to Broken");

        // The asymmetry is deliberate: a positive edge status never upgrades
        // toward Reachable — this machine's click still fails.
        assert_eq!(effective_outcome(TransportError, Some(200)), TransportError);
        assert_eq!(
            effective_outcome(ProbeOutcome::Status(200), Some(530)),
            ProbeOutcome::Status(200),
            "a real primary answer is never overridden"
        );
    }

    #[test]
    fn fold_diagnosis_reads_the_vector_honestly() {
        use TunnelDiagnosis::*;
        assert_eq!(fold_diagnosis(Some(true), Some(200)), LocalDnsBehind);
        assert_eq!(fold_diagnosis(None, Some(200)), LocalDnsBehind); // 1.1.1.1 mute, edge fine
        assert_eq!(fold_diagnosis(Some(false), Some(404)), DnsPropagating);
        assert_eq!(fold_diagnosis(Some(true), Some(530)), EdgeGone);
        assert_eq!(fold_diagnosis(Some(false), Some(530)), EdgeGone);
        assert_eq!(fold_diagnosis(Some(true), None), Offline);
        assert_eq!(fold_diagnosis(None, None), Offline);
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

    #[test]
    fn extract_url_never_takes_the_registration_endpoint() {
        // An error line naming the api host must not be recorded as the
        // public URL; the real banner later still wins.
        let log = "ERR request to https://api.trycloudflare.com failed, retrying\n\
                   INF |  https://blue-cat-runs-fast.trycloudflare.com  |";
        assert_eq!(extract_url(log).as_deref(), Some("https://blue-cat-runs-fast.trycloudflare.com"));
        assert!(extract_url("ERR https://api.trycloudflare.com unreachable").is_none());
    }
}

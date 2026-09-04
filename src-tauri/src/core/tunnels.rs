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
    platform.supervisor().spawn_logged(cloudflared_bin, &spawn_args(domain, origin_port), &log)
}

/// The ONE place a cloudflared command line is built — and the reason it is a
/// function rather than a `vec!` inside [`start`].
///
/// `--http-host-header <domain>` is not only routing. It is the IDENTITY every
/// later decision reads: [`is_our_tunnel`] refuses to signal a pid whose argv
/// does not carry this exact token pair, the rowless backstop finds shares by
/// it, and the parent-death guard re-reads it before signalling. A spawn that
/// omitted it would produce a live public share that NO sweep can recognise —
/// unkillable by us, and invisible to the ownership rules — which is the worst
/// direction for a process that publishes a developer's machine to the
/// internet. Building the argv here, once, is what lets the guard below assert
/// the round trip (`spawn_args` → `is_our_tunnel`) instead of asserting that a
/// literal appears somewhere in a function body.
pub fn spawn_args(domain: &str, origin_port: u16) -> Vec<String> {
    vec![
        "tunnel".to_string(),
        "--no-autoupdate".to_string(),
        "--url".to_string(),
        format!("http://127.0.0.1:{origin_port}"),
        "--http-host-header".to_string(),
        domain.to_string(),
    ]
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

/// A share's phase gate: closed (Phase A) until [`gate_opens`] says the record
/// is provably in public DNS, then open FOREVER.
///
/// **A type with no way back, rather than a `bool` and a rule.** Re-entering
/// Phase A is not a cosmetic regression: the point of Phase A is that we make
/// no system-resolver query for a name that may not exist yet, because ONE
/// early query negative-caches the whole LAN for up to thirty minutes
/// (`trycloudflare.com`'s SOA MINIMUM is 1800s — measured 28 Jul 2026, and it
/// is why a phone could only reach a share on cellular). A gate that reopens
/// would put a WORKING share back into a window whose only purpose is to
/// protect a share that does not resolve yet. So there is no `close`, and
/// `open` is idempotent: the only reachable transition is closed → open.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PhaseGate(bool);

impl PhaseGate {
    /// Open it. Idempotent, and the ONLY mutation this type has.
    pub fn open(&mut self) {
        self.0 = true;
    }

    /// Has it opened?
    pub fn is_open(self) -> bool {
        self.0
    }
}

/// The plan for a tick, from the gate the registry carries. The gate only ever
/// flips open ([`PhaseGate`] has no other transition) — a share never re-enters
/// Phase A.
pub fn probe_plan(gate: PhaseGate) -> ProbePlan {
    if gate.is_open() {
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

/// **The only predicate that may authorise a signal at a recorded pid.** Two
/// refusals, in order, because they fail differently:
///
/// - [`PID_PENDING`] is refused BEFORE the process table is consulted at all.
///   A crash between the row claim and the spawn leaves the sentinel and no
///   process ever existed for it, so there is nothing to look up; the sentinel
///   is inert by construction (`u32::MAX` can never be a real pid), and the
///   point of checking first is that the inertness must not be what saves us.
/// - Everything else must pass [`is_our_tunnel`] on the LIVE command line —
///   a recycled pid now naming an unrelated process gets file and row cleanup
///   only, never a signal.
///
/// A predicate rather than an `&&` chain at the call site because the sweeps
/// are where a missed guard costs someone else's process: one decision, one
/// place, testable without a process table.
pub fn may_signal(pid: u32, command: Option<&str>, app_data_marker: &str, domain: &str) -> bool {
    if pid == PID_PENDING {
        return false;
    }
    command.map(|cmd| is_our_tunnel(cmd, app_data_marker, domain)).unwrap_or(false)
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
        let ours = may_signal(
            row.pid,
            platform.supervisor().pid_command(row.pid).as_deref(),
            &marker,
            &row.domain,
        );
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
        if !may_signal(pid, Some(cmd.as_str()), &marker, &domain) {
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

/// The argv a tunnel guard is spawned with, and the parse of it — one pair, so
/// the two can never drift into a guard that starts and watches the wrong pid.
///
/// **Why a guard process exists at all.** Tunnels die with the app (ruled 28 Jul
/// 2026): a public share must not outlive the thing supervising it. A CLEAN quit
/// kills them in `RunEvent::Exit`, and a crash is caught by the launch sweep —
/// but only at the NEXT launch, which may be days away, and until then a site is
/// public with nothing watching it. macOS has no `PR_SET_PDEATHSIG`, so closing
/// that window needs a separate process watching the parent die.
#[derive(Debug, PartialEq, Eq)]
pub struct GuardArgs {
    /// The rexenv process whose death ends the share.
    pub parent: u32,
    /// The cloudflared child to stop when it does.
    pub child: u32,
    /// The site host the child must still be serving — the guard kills on argv
    /// IDENTITY, never on a bare pid (the pid may have been recycled between
    /// the parent's death and ours noticing).
    pub domain: String,
    /// The PARENT's identity beyond its pid: its start time as the OS prints
    /// it. Between rexenv reading its own pid and the guard registering on it,
    /// rexenv can die and the kernel recycle the number — registration then
    /// succeeds against a STRANGER and the guard sleeps until that process
    /// exits, the share public the whole time. A pid whose start time is not
    /// this one is not our parent, whatever number it wears.
    pub parent_start: String,
}

/// The flag the guard mode is dispatched on (`main.rs`, before Tauri boots).
pub const GUARD_FLAG: &str = "--tunnel-guard";

/// Build the guard's argv. Kept beside the parser so a change to one fails the
/// round-trip test rather than producing a guard that silently never fires.
pub fn guard_argv(parent: u32, child: u32, domain: &str, parent_start: &str) -> Vec<String> {
    vec![
        GUARD_FLAG.to_string(),
        parent.to_string(),
        child.to_string(),
        domain.to_string(),
        parent_start.to_string(),
    ]
}

/// Parse a guard invocation. `None` = not a guard invocation, or a malformed
/// one — and malformed must NEVER degrade into a guard with a default pid: pid
/// 0 or 1 would make the watcher wait on init and the killer aim at it.
pub fn parse_guard_args(args: &[String]) -> Option<GuardArgs> {
    let i = args.iter().position(|a| a == GUARD_FLAG)?;
    let parent: u32 = args.get(i + 1)?.parse().ok()?;
    let child: u32 = args.get(i + 2)?.parse().ok()?;
    let domain = args.get(i + 3)?.trim().to_string();
    let parent_start = args.get(i + 4)?.trim().to_string();
    if parent <= 1 || child <= 1 || domain.is_empty() || parent_start.is_empty() {
        return None;
    }
    Some(GuardArgs { parent, child, domain, parent_start })
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

    /// #432 — the guard's argv is a CONTRACT between two processes, and the
    /// only failure mode that matters is a guard that starts and watches the
    /// wrong thing. Round-trip, then every malformed shape that must refuse
    /// rather than default: a defaulted pid here is a watcher waiting on init
    /// and a killer aiming at it.
    #[test]
    fn a_guard_is_started_with_exactly_what_it_parses_back() {
        let argv = guard_argv(4242, 78716, "mstest.rex", "Wed Sep  3 12:00:01 2026");
        assert_eq!(argv[0], GUARD_FLAG);
        let parsed = parse_guard_args(&argv).expect("its own argv must parse");
        assert_eq!(
            parsed,
            GuardArgs {
                parent: 4242,
                child: 78716,
                domain: "mstest.rex".into(),
                parent_start: "Wed Sep  3 12:00:01 2026".into()
            }
        );

        // The app's OWN launch must never be read as a guard invocation.
        let normal: Vec<String> = ["/Applications/rexenv.app/Contents/MacOS/rexenv".to_string()].into();
        assert!(parse_guard_args(&normal).is_none());
        assert!(parse_guard_args(&["--dns-agent".to_string()]).is_none());

        // Malformed: refuse, never default.
        for bad in [
            vec![GUARD_FLAG.into()],                                     // no pids at all
            vec![GUARD_FLAG.into(), "4242".into()],                      // no child
            vec![GUARD_FLAG.into(), "4242".into(), "78716".into()],      // no domain
            vec![GUARD_FLAG.into(), "4242".into(), "78716".into(), "a.rex".into()], // no parent start (the pre-3-Sep shape)
            vec![GUARD_FLAG.into(), "x".into(), "78716".into(), "a.rex".into(), "t".into()], // parent NaN
            vec![GUARD_FLAG.into(), "4242".into(), "y".into(), "a.rex".into(), "t".into()],  // child NaN
            vec![GUARD_FLAG.into(), "1".into(), "78716".into(), "a.rex".into(), "t".into()], // parent = init
            vec![GUARD_FLAG.into(), "4242".into(), "0".into(), "a.rex".into(), "t".into()],  // child = 0
            vec![GUARD_FLAG.into(), "4242".into(), "78716".into(), "  ".into(), "t".into()], // blank domain
            vec![GUARD_FLAG.into(), "4242".into(), "78716".into(), "a.rex".into(), " ".into()], // blank start
        ] {
            assert!(
                parse_guard_args(&bad).is_none(),
                "a malformed guard invocation parsed anyway: {bad:?} — a guard with a defaulted \
                 pid watches init and signals init"
            );
        }
    }

    /// **The spawn and the sweeps must agree, or a share becomes unkillable.**
    /// `--http-host-header <domain>` is what every ownership decision reads, so
    /// this asserts the ROUND TRIP rather than the presence of a literal: the
    /// argv `start` actually spawns is fed to the identity functions the sweeps
    /// use, and they must recognise it. Dropping the pair from `spawn_args`
    /// (or renaming the flag on one side only) leaves a live public tunnel that
    /// `sweep_startup`, the rowless backstop and the parent-death guard all
    /// decline to touch — the exact failure this pair exists to prevent.
    /// **The gate has no way back, and that is the claim** — not that today's
    /// code happens never to write `false`. Re-entering Phase A would put a
    /// WORKING share back into a window whose only purpose is protecting a
    /// share that does not resolve yet, and the cost of the window is real:
    /// one early system query negative-caches the LAN for up to thirty minutes.
    /// Held two ways, because a behavioural test alone would pass on a type
    /// that grew a `close()` nobody had called yet.
    #[test]
    fn the_phase_gate_only_ever_opens() {
        let mut gate = PhaseGate::default();
        assert!(!gate.is_open(), "a share must START in Phase A — the default is the closed one");
        gate.open();
        assert!(gate.is_open());
        // Idempotent, and no sequence of the operations this type HAS can
        // close it again.
        for _ in 0..3 {
            gate.open();
            assert!(gate.is_open(), "the gate came back closed — the share re-entered Phase A");
            assert_eq!(probe_plan(gate), ProbePlan::System);
        }

        // The type surface itself: exactly one mutation, and the only write to
        // the inner flag is the one that opens it. A `close`/`set(false)` added
        // later fails here rather than the first time a share loses its verdict.
        let src = crate::core::copy_scan::production_source(include_str!("tunnels.rs"));
        let imp = src
            .split("impl PhaseGate {")
            .nth(1)
            .and_then(|b| b.split("\n}").next())
            .expect("impl PhaseGate");
        assert_eq!(
            imp.matches("self.0 =").count(),
            1,
            "`PhaseGate` has more than one write to its flag — the second one is the way back \
             into Phase A that this type exists to make unreachable:\n{imp}"
        );
        assert!(
            !imp.contains("self.0 = false"),
            "`PhaseGate` can be closed again — a share whose gate reopens starts making the \
             early system queries that poison the LAN's resolver for 30 minutes"
        );
    }

    /// The edge check's address must come from a LIVE lookup, never a literal.
    /// A pinned Cloudflare IP rots silently: the check keeps passing against an
    /// address that stopped being an edge, and "the edge answered" then measures
    /// nothing while reading as positive evidence in the UI.
    #[test]
    fn no_edge_ip_is_ever_hardcoded() {
        let src = crate::core::copy_scan::production_source(include_str!("tunnels.rs"));
        // Deliberate, non-edge addresses: the loopback origin a tunnel points
        // at, the wildcard bind for our own ephemeral UDP socket, and the
        // resolver we query ON PURPOSE (asking 1.1.1.1 is the measurement —
        // it is not an answer we pinned).
        let allowed = ["127.0.0.1", "0.0.0.0", "1.1.1.1"];
        for line in src.lines() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            let mut rest = code;
            while let Some(pos) = rest.find(|c: char| c.is_ascii_digit()) {
                let tail = &rest[pos..];
                let end = tail
                    .find(|c: char| !(c.is_ascii_digit() || c == '.'))
                    .unwrap_or(tail.len());
                let tok = tail[..end].trim_end_matches('.');
                let quads: Vec<&str> = tok.split('.').collect();
                if quads.len() == 4 && quads.iter().all(|q| !q.is_empty() && q.parse::<u8>().is_ok())
                {
                    assert!(
                        allowed.contains(&tok),
                        "`{tok}` is a hardcoded IP in tunnels.rs ({code}). The edge address must \
                         come from the live 1.1.1.1 answer (or the live apex lookup) — a pinned \
                         edge IP rots silently and the check that uses it measures nothing"
                    );
                }
                rest = &tail[end..];
            }
        }
    }

    #[test]
    fn what_we_spawn_is_what_the_sweeps_can_identify() {
        let args = spawn_args("acme.rex", 18088);
        // The command line as a process table would show it: our binary path
        // (under app-data, the ownership marker) plus the spawned argv.
        let cmd = format!("{MARKER}/bin/cloudflared-2026.6.1/cloudflared {}", args.join(" "));
        assert!(
            is_our_tunnel(&cmd, MARKER, "acme.rex"),
            "the sweep cannot identify the process we just spawned: {cmd}"
        );
        assert_eq!(
            host_header_domain(&cmd).as_deref(),
            Some("acme.rex"),
            "the rowless backstop reads the domain OUT of the argv — with no pair it has no \
             domain, and a share nobody recorded stays public"
        );
        assert!(
            may_signal(4242, Some(&cmd), MARKER, "acme.rex"),
            "we could not authorise stopping our own tunnel"
        );

        // The origin is loopback and the port is the one asked for — a tunnel
        // is a public door onto ONE local port, and `--url` is the door.
        assert!(args.contains(&"--url".to_string()));
        assert!(
            args.contains(&"http://127.0.0.1:18088".to_string()),
            "the origin must be loopback on the given port: {args:?}"
        );
        assert_ne!(
            spawn_args("acme.rex", 18088),
            spawn_args("acme.rex", 8080),
            "the origin port must reach the argv, or every share serves the same backend"
        );
        // Self-update is off: a cloudflared that replaces its own binary mid-
        // share leaves argv we pinned an ownership decision to.
        assert!(args.contains(&"--no-autoupdate".to_string()));

        // …and the identity is the ROW's domain, not any tunnel's: the same
        // live process must not authorise a signal for a different row.
        assert!(!may_signal(4242, Some(&cmd), MARKER, "other.rex"));
    }

    /// The sentinel is refused BEFORE the process table is consulted, and that
    /// ordering is the claim: `PID_PENDING` is inert (`u32::MAX` can never be a
    /// real pid), but inertness is a property of today's platform, not a rule.
    /// A row still carrying it crashed between claiming the row and spawning —
    /// no process ever existed for it — so any argv presented for that pid is
    /// somebody else's, and a `kill` there is a kill on a stranger.
    #[test]
    fn the_pending_sentinel_can_never_authorise_a_signal() {
        let ours = format!(
            "{MARKER}/bin/cloudflared/cloudflared tunnel --no-autoupdate \
             --url http://127.0.0.1:18088 --http-host-header acme.rex"
        );
        // Even a command line that satisfies the full identity — which is what
        // a recycled `u32::MAX` would have to look like — must not pass.
        assert!(
            !may_signal(PID_PENDING, Some(&ours), MARKER, "acme.rex"),
            "the pending sentinel authorised a signal — the sweep would kill whatever the \
             process table happens to answer for it"
        );
        assert!(!may_signal(PID_PENDING, None, MARKER, "acme.rex"));
        // A real pid with no live process (already gone) is cleanup-only too.
        assert!(!may_signal(4242, None, MARKER, "acme.rex"));
        // The sentinel is what the doc says it is: not a pid anything can hold.
        assert_eq!(PID_PENDING, u32::MAX);
    }

    /// Every signal in this module goes through [`may_signal`] — a drift guard,
    /// because the cost of a missed one is asymmetric: it is not our process.
    /// `stop` (the user pressing Stop on a share they can see, with the live
    /// `Child`'s own pid) is the one deliberate exception and is named here.
    #[test]
    fn every_sweep_signal_is_authorised_by_the_predicate() {
        let src = crate::core::copy_scan::production_source(include_str!("tunnels.rs"));
        for (sweep, end) in
            [("pub fn sweep_startup(", "\n/// "), ("pub fn sweep_rowless(", "\n/// ")]
        {
            let body = src
                .split(sweep)
                .nth(1)
                .and_then(|b| b.split(end).next())
                .unwrap_or_else(|| panic!("{sweep} not found"));
            assert!(
                body.contains("may_signal("),
                "{sweep} signals without going through `may_signal` — the sentinel check and \
                 the identity check are then two things a reader must remember"
            );
        }
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
            enabled: true,
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
        let mut gate = PhaseGate::default();
        assert_eq!(probe_plan(gate), ProbePlan::EdgeOnly, "a fresh share starts in Phase A");
        gate.open();
        assert_eq!(probe_plan(gate), ProbePlan::System);

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

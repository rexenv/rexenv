//! commands::tunnels — Tauri IPC for per-site cloudflared quick tunnels (§9.1).
//!
//! Owns the live tunnels in a Tauri-managed registry keyed by site domain. A
//! tunnel can only be started for a real site (looked up by id) — internal
//! tooling vhosts aren't sites, so they can never be shared.

use crate::core::{binaries, tunnels, wp_tunnel};
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::models::{Site, SiteType};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::process::Child;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::State;

/// Look up the site a tunnel command targets (brief DB lock).
fn tunnel_site(state: &State<'_, AppState>, id: &str) -> Result<Site> {
    let conn = state
        .db
        .lock()
        .map_err(|_| Error::Other("database lock poisoned".into()))?;
    crate::core::sites::get(&conn, id)?.ok_or_else(|| Error::Other(format!("no site {id}")))
}

struct TunnelEntry {
    child: Child,
    url: String,
    /// Docroot at start time — dead-child cleanup removes the mu-plugin here
    /// without a site lookup (mirrors the v23 row).
    docroot: String,
    health: crate::core::tunnels::TunnelHealth,
    /// Consecutive tunnel-gone (530) probe responses — see `fold_probe`.
    strikes: u32,
    /// Failure-gated diagnosis (why the primary probe couldn't reach the
    /// URL); cleared whenever the primary probe gets an HTTP answer.
    diagnosis: Option<crate::core::tunnels::TunnelDiagnosis>,
    /// Phase gate (ruled 28 Jul): closed = Phase A, the prober may not touch
    /// the system resolver for this hostname yet — our own too-early query
    /// was negative-caching the LAN for 30 minutes. Flips open once (never
    /// back) via `gate_opens`, and `PhaseGate` is the type that makes "never
    /// back" a fact rather than a comment: it has no closing transition.
    system_probing: crate::core::tunnels::PhaseGate,
    /// Share age, for the gate's escape-hatch cap.
    started: Instant,
}

/// Tauri-managed registry of live tunnels, keyed by site domain.
#[derive(Default)]
pub struct Tunnels(Mutex<HashMap<String, TunnelEntry>>);

impl Tunnels {
    /// Remove and kill a site's live tunnel; `true` if one was running. No-op
    /// when the site isn't shared. Used by `stop_tunnel`, site deletion, and
    /// domain rename (a deleted/renamed site must not stay publicly reachable).
    pub fn stop_for_domain(&self, state: &AppState, domain: &str) -> bool {
        let entry = self.0.lock().ok().and_then(|mut m| m.remove(domain));
        let stopped = match entry {
            Some(mut e) => {
                let pid = e.child.id();
                let _ = tunnels::stop(state.platform.as_ref(), pid);
                let _ = e.child.wait();
                // The closing half of the start line (ledger #430): a share
                // that started and never stopped is only readable as such if
                // every stop writes one.
                log::info!("{}", tunnels::share_stopped_line(domain, pid, e.started.elapsed()));
                true
            }
            None => false,
        };
        if !stopped {
            // A start may be IN FLIGHT: claim taken, child possibly spawned,
            // registry entry not yet inserted. Kill its recorded pid so stop
            // wins that race — but only on the sweep's positive argv
            // identification: the start's own failure path may have reaped
            // this pid between our read and the signal, and a reaped pid is
            // reusable. A sentinel (pre-spawn) claim has nothing to kill; the
            // row delete below revokes it, and `set_tunnel_pid` returning
            // false makes the in-flight start cancel itself.
            let row = state
                .db
                .lock()
                .ok()
                .and_then(|c| crate::state::store::get_tunnel(&c, domain).ok().flatten());
            if let Some(row) = row {
                if row.pid != tunnels::PID_PENDING {
                    let marker = state
                        .platform
                        .paths()
                        .app_data_dir()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    let ours = state
                        .platform
                        .supervisor()
                        .pid_command(row.pid)
                        .map(|cmd| tunnels::is_our_tunnel(&cmd, &marker, domain))
                        .unwrap_or(false);
                    if ours {
                        let _ = tunnels::stop(state.platform.as_ref(), row.pid);
                        log::info!(
                            "tunnels: stopped an IN-FLIGHT share of {domain} (pid {}) — it was \
                             starting when the stop arrived, so it may never have been public",
                            row.pid
                        );
                    }
                }
            }
        }
        // Row cleanup happens even with no registry entry: an in-flight
        // start's claim must not outlive a stop/delete/rename — deleting it
        // here is what revokes the claim.
        delete_tunnel_row(state, domain);
        stopped
    }

    /// Whether `domain` has a live OR starting share (step-7 guards): the v23
    /// row exists from claim to clean stop, so row-existence is the ONE fact
    /// both guard directions read — an in-flight start blocks a job exactly
    /// like a live share. Dead children are settled first: a crashed
    /// cloudflared must never block the user's work.
    pub(crate) fn sharing_domain(&self, state: &AppState, domain: &str) -> bool {
        settle_dead(state, self.take_dead());
        state
            .db
            .lock()
            .ok()
            .and_then(|c| crate::state::store::get_tunnel(&c, domain).ok().flatten())
            .is_some()
    }

    /// Remove every entry whose child has EXITED (`try_wait` — non-blocking,
    /// and `Some` means the zombie is already reaped). Returns the removed
    /// pairs for [`settle_dead`] to clean up outside the lock. `Err` from
    /// `try_wait` reads as alive — status may only claim death on positive
    /// evidence, the same rule the sweep applies to kills.
    fn take_dead(&self) -> Vec<(String, TunnelEntry)> {
        let Ok(mut map) = self.0.lock() else { return Vec::new() };
        let dead: Vec<String> = map
            .iter_mut()
            .filter_map(|(domain, e)| matches!(e.child.try_wait(), Ok(Some(_))).then(|| domain.clone()))
            .collect();
        dead.into_iter().filter_map(|d| map.remove(&d).map(|e| (d, e))).collect()
    }
}

/// Settle tunnels whose process died on its own (cloudflared crash): the
/// child is already reaped, so only the file and the row remain. After this
/// the site honestly reads "not sharing" — a dead tunnel must never sit in
/// the registry showing Live with a dead URL.
fn settle_dead(state: &AppState, dead: Vec<(String, TunnelEntry)>) {
    for (domain, entry) in dead {
        log::warn!(
            "tunnels: the tunnel for {domain} exited on its own — clearing its mu-plugin and record"
        );
        if let Err(e) = wp_tunnel::disable(Path::new(&entry.docroot)) {
            log::warn!("rexenv: could not remove the tunnel mu-plugin for {domain}: {e}");
        }
        delete_tunnel_row(state, &domain);
    }
}

/// Step-7 exposure guard for mutating jobs: refuse while `domain` is shared
/// (or a share is starting). A tunnel doesn't mutate, it EXPOSES — so the
/// refusal must name what a VISITOR would experience (`would`), never a bare
/// "busy". rexenv never stops a share on the user's behalf (same ruling as
/// Broken-tunnel auto-stop): the message says where to stop it and that a
/// re-share gets a fresh link, and the user decides.
pub(crate) fn refuse_if_shared(
    tunnels: &Tunnels,
    state: &AppState,
    domain: &str,
    would: &str,
) -> Result<()> {
    if tunnels.sharing_domain(state, domain) {
        return Err(Error::Other(format!(
            "{domain} is publicly shared right now — {would}. Stop sharing it (Tunnels page) \
             and retry; sharing again afterwards gets a NEW link."
        )));
    }
    Ok(())
}

/// Record that a mu-plugin writer CREATED the site's mu-plugins dir (v25) —
/// best-effort: a missed record just means the dir outlives the site, which
/// is the pre-v25 status quo, never a wrong deletion.
pub(crate) fn record_mu_dir_created(state: &AppState, site_id: &str) {
    match state.db.lock() {
        Ok(conn) => {
            if let Err(e) = crate::state::store::set_site_mu_dir_created(&conn, site_id) {
                log::warn!("rexenv: could not record mu-plugins dir ownership for {site_id}: {e}");
            }
        }
        Err(_) => log::warn!("rexenv: database lock poisoned; mu-plugins dir ownership unrecorded"),
    }
}

/// Best-effort row delete (v23). Failure is logged, not fatal: the exit hook
/// clears the table and the launch sweep settles survivors, so a missed
/// delete degrades to "settled later", never to a wrong kill (the sweep
/// signals only on positive argv identification).
fn delete_tunnel_row(state: &AppState, domain: &str) {
    match state.db.lock() {
        Ok(conn) => {
            if let Err(e) = crate::state::store::delete_tunnel(&conn, domain) {
                log::warn!("rexenv: could not delete the tunnel record for {domain}: {e}");
            }
        }
        Err(_) => log::warn!("rexenv: database lock poisoned; tunnel record for {domain} left for the sweep"),
    }
}

/// Probe cadence for live tunnels. External HTTPS HEAD per tunnel per tick —
/// negligible volume, and verdicts age at most this long.
const PROBE_INTERVAL: Duration = Duration::from_secs(30);

/// Probe one tunnel's public URL and fold the verdict into its entry. Network
/// waits happen OUTSIDE the registry lock (two brief locks around one bounded
/// HEAD); the URL is re-checked on write-back so a restart mid-probe can't
/// stamp the old run's verdict onto the new tunnel.
async fn probe_and_record<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    client: &reqwest::Client,
    domain: &str,
) {
    use tauri::Manager;
    let Some(registry) = app.try_state::<Tunnels>() else { return };
    let Some((url, prev_health, strikes, gate_open, elapsed)) =
        registry.0.lock().ok().and_then(|m| {
            m.get(domain).map(|e| {
                (e.url.clone(), e.health, e.strikes, e.system_probing, e.started.elapsed())
            })
        })
    else {
        return;
    };
    let (effective, diagnosis, open_gate) = match crate::core::tunnels::probe_plan(gate_open) {
        // Phase A — INVARIANT (pinned by `phase_a_never_plans_a_system_dns_query`
        // and by `diagnose_unreachable`'s construction: raw UDP to 1.1.1.1 +
        // a reqwest client with the hostname's address PINNED, which never
        // calls getaddrinfo for it): this arm must not resolve `host`
        // through the system resolver. One early query negative-caches the
        // LAN for up to 30 minutes (trycloudflare SOA MINIMUM = 1800s).
        crate::core::tunnels::ProbePlan::EdgeOnly => {
            let host = url.trim_start_matches("https://");
            let (public_resolves, edge_status) =
                crate::core::tunnels::diagnose_unreachable(host).await;
            let opens = crate::core::tunnels::gate_opens(public_resolves, elapsed);
            if opens {
                // The record is provably in public DNS (or the cap fired) —
                // a system query is now harmless. Run the real probe in the
                // SAME tick rather than leaving the user an extra 30s of
                // Unverified: the step-2 fast-first-verdict goal, kept,
                // without the poison that goal originally caused.
                let (eff, diag) = system_probe_cycle(client, &url).await;
                (eff, diag, true)
            } else {
                (
                    crate::core::tunnels::effective_outcome(
                        crate::core::tunnels::ProbeOutcome::TransportError,
                        edge_status,
                    ),
                    Some(crate::core::tunnels::fold_diagnosis(public_resolves, edge_status)),
                    false,
                )
            }
        }
        crate::core::tunnels::ProbePlan::System => {
            let (eff, diag) = system_probe_cycle(client, &url).await;
            (eff, diag, false)
        }
    };
    let (health, strikes) = crate::core::tunnels::fold_probe(prev_health, strikes, effective);
    if let Ok(mut m) = registry.0.lock() {
        if let Some(e) = m.get_mut(domain) {
            if e.url == url {
                e.health = health;
                e.strikes = strikes;
                e.diagnosis = diagnosis;
                if open_gate {
                    e.system_probing.open();
                }
            }
        }
    };
}

/// Phase B's whole cycle: the system-path probe, plus the failure-gated
/// diagnosis when it transport-fails (a healthy tunnel generates zero extra
/// traffic, forever — an edge 530 upgrades the outcome into strike evidence).
async fn system_probe_cycle(
    client: &reqwest::Client,
    url: &str,
) -> (crate::core::tunnels::ProbeOutcome, Option<crate::core::tunnels::TunnelDiagnosis>) {
    let outcome = crate::core::tunnels::probe_url(client, url).await;
    if outcome == crate::core::tunnels::ProbeOutcome::TransportError {
        let host = url.trim_start_matches("https://");
        let (public_resolves, edge_status) =
            crate::core::tunnels::diagnose_unreachable(host).await;
        (
            crate::core::tunnels::effective_outcome(outcome, edge_status),
            Some(crate::core::tunnels::fold_diagnosis(public_resolves, edge_status)),
        )
    } else {
        (outcome, None)
    }
}

/// A `reqwest` client bounded for health probes: a wedged edge costs at most
/// the total timeout, never an open-ended wait (B25 discipline).
fn probe_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(8))
        // The FIRST hop's status is the fact (audit A5): a 3xx already rode
        // the tunnel, and following it can leave the tunnel entirely — on
        // this machine a redirect to the site's local .test URL resolves and
        // would let the probe complete a chain real visitors can't.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_default()
}

/// Background health prober, spawned once at setup. Every tick: settle dead
/// children (so a crash is noticed within the interval even with no UI open,
/// and its mu-plugin/row are cleaned promptly), then probe each live tunnel's
/// public URL. Idles cheaply when nothing is shared.
pub fn spawn_health_prober<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    use tauri::Manager;
    tauri::async_runtime::spawn(async move {
        let client = probe_client();
        loop {
            tokio::time::sleep(PROBE_INTERVAL).await;
            let (Some(registry), Some(state)) =
                (app.try_state::<Tunnels>(), app.try_state::<crate::state::app::AppState>())
            else {
                continue;
            };
            settle_dead(&state, registry.take_dead());
            let domains: Vec<String> = registry
                .0
                .lock()
                .map(|m| m.keys().cloned().collect())
                .unwrap_or_default();
            // CONCURRENT: a failing probe costs up to ~19s (primary, 1.1.1.1,
            // edge), and running them in sequence stretched the "checked every
            // 30 s" the Tunnels page promises to minutes with a few broken
            // shares — every badge as stale as the sum of the others' waits.
            let probes: Vec<_> = domains
                .into_iter()
                .map(|domain| {
                    let (app, client) = (app.clone(), client.clone());
                    tokio::spawn(async move { probe_and_record(&app, &client, &domain).await })
                })
                .collect();
            for probe in probes {
                let _ = probe.await;
            }
        }
    });
}

/// Live share count for the quit warning: dead children are settled FIRST, so
/// the number names what is actually running — never registry entries that
/// liveness has already invalidated.
pub fn live_share_count<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> usize {
    use tauri::Manager;
    let (Some(registry), Some(state)) =
        (app.try_state::<Tunnels>(), app.try_state::<AppState>())
    else {
        return 0;
    };
    settle_dead(&state, registry.take_dead());
    registry.0.lock().map(|m| m.len()).unwrap_or(0)
}

/// Quit flows once the user has confirmed (or nothing was shared).
static QUIT_CONFIRMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// One dialog at a time — a second Cmd+Q while it's up must not stack another.
static QUIT_DIALOG_OPEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Quit warning (28 Jul 2026): inform, don't obstruct. `true` = let the
/// quit/close proceed. With no live shares (or after a confirm) quitting is
/// untouched. With live shares the caller prevents the exit and this shows a
/// native confirm naming the count — OFF the main thread, where
/// `blocking_show` would deadlock the event loop — then re-exits on "Quit"
/// with the flag set. "Keep sharing" simply drops the quit request.
pub fn confirm_quit_or_prompt<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    use std::sync::atomic::Ordering;
    if QUIT_CONFIRMED.load(Ordering::SeqCst) {
        return true;
    }
    let n = live_share_count(app);
    if n == 0 {
        return true;
    }
    if QUIT_DIALOG_OPEN.swap(true, Ordering::SeqCst) {
        return false; // dialog already up — keep holding the quit
    }
    // Come to the front before asking. rexenv has no dock icon, so a quit
    // triggered from the menu bar can open its confirm behind the browser the
    // developer is reading — and a prompt nobody sees is worse than no prompt:
    // the quit appears to have hung. Done HERE, on the caller's thread, because
    // AppKit activation is main-thread-only and the dialog runs on its own.
    #[cfg(target_os = "macos")]
    crate::platform::activate_app();
    let app = app.clone();
    std::thread::spawn(move || {
        use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
        // Drop guard, not a trailing store (audit A6): a panic anywhere in
        // the dialog path would otherwise leave the flag stuck true and
        // every later quit silently prevented — an unquittable app.
        struct DialogOpenReset;
        impl Drop for DialogOpenReset {
            fn drop(&mut self) {
                QUIT_DIALOG_OPEN.store(false, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let _reset = DialogOpenReset;
        let message = if n == 1 {
            "Quitting stops 1 public share — its link goes dead immediately.".to_string()
        } else {
            format!("Quitting stops {n} public shares — their links go dead immediately.")
        };
        let confirmed = app
            .dialog()
            .message(message)
            .title("Stop sharing?")
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Quit".to_string(),
                "Keep sharing".to_string(),
            ))
            .blocking_show();
        if confirmed {
            QUIT_CONFIRMED.store(true, Ordering::SeqCst);
            app.exit(0);
        }
    });
    false
}

/// App-exit hook: tunnels DIE WITH THE APP (lifecycle ruling 28 Jul 2026 —
/// `docs/archive/PLAN-tunnel-lifecycle.md`). A service outliving the app serves the
/// developer; a tunnel outliving it serves the PUBLIC, unattended — so this is
/// the deliberate opposite of services-outlive-the-app, same as repo jobs.
///
/// Kills from the RECORDED rows, not just the registry: a start still polling
/// for its URL has a row but no registry entry yet. Registry children are
/// reaped first (stop + wait) and their pids skipped in the row pass — after a
/// `wait()` a pid is free for reuse, and a bare-number signal to it would
/// violate the never-kill-on-a-bare-pid rule. Each row's mu-plugin is removed
/// (the file must not outlive its tunnel), then the table is cleared.
pub fn kill_all_on_exit<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    let (Some(registry), Some(state)) =
        (app.try_state::<Tunnels>(), app.try_state::<AppState>())
    else {
        return;
    };
    let mut reaped: std::collections::HashSet<u32> = std::collections::HashSet::new();
    if let Ok(mut map) = registry.0.lock() {
        for (domain, mut e) in map.drain() {
            let pid = e.child.id();
            let _ = tunnels::stop(state.platform.as_ref(), pid);
            let _ = e.child.wait();
            // Quitting is the commonest way a share ends, so it is the
            // commonest missing stop line if this one is skipped (#430).
            log::info!(
                "{} (rexenv quit)",
                tunnels::share_stopped_line(&domain, pid, e.started.elapsed())
            );
            reaped.insert(pid);
        }
    }
    let rows = state
        .db
        .lock()
        .ok()
        .map(|conn| crate::state::store::list_tunnels(&conn).unwrap_or_default())
        .unwrap_or_default();
    for row in rows {
        if should_signal_row(row.pid, &reaped) {
            let _ = tunnels::stop(state.platform.as_ref(), row.pid);
        }
        if let Err(e) = wp_tunnel::disable(Path::new(&row.docroot)) {
            log::warn!("rexenv: could not remove the tunnel mu-plugin for {}: {e}", row.domain);
        }
    }
    if let Ok(conn) = state.db.lock() {
        let _ = crate::state::store::clear_tunnels(&conn);
    };
}

/// May the exit hook signal this recorded pid? (ledger #30)
///
/// Extracted so the rule is a value rather than a condition buried in a loop
/// that needs a Tauri app to reach. TWO reasons to say no, and both are the
/// never-kill-on-a-bare-pid rule:
///
/// - **Already reaped.** The registry pass did `stop` + `wait` on it. After a
///   `wait()` the pid is free for the OS to reuse, so signalling it again is a
///   signal to whatever now owns that number.
/// - **A sentinel.** `PID_PENDING` means the claim was taken and no child
///   spawned yet; there is no process behind it. Its row and mu-plugin are still
///   cleaned by the caller — only the SIGNAL is skipped.
///
/// Anything else is our own child from THIS session (launch already swept older
/// rows), unreaped, so the number cannot have been recycled.
fn should_signal_row(row_pid: u32, reaped: &std::collections::HashSet<u32>) -> bool {
    row_pid != tunnels::PID_PENDING && !reaped.contains(&row_pid)
}

/// A tunnel's public status for the UI. `running` = a live process exists;
/// `health` = what we can say about the public URL (one fact, see
/// [`crate::core::tunnels::TunnelHealth`]). Absent from the list = not sharing.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelInfo {
    pub domain: String,
    pub url: String,
    pub running: bool,
    pub health: crate::core::tunnels::TunnelHealth,
    /// Why the URL is unreachable from THIS machine, when it is (the line
    /// under the badge — the badge itself never changes meaning for this).
    pub diagnosis: Option<crate::core::tunnels::TunnelDiagnosis>,
    /// What this share currently publishes, when that is not the site (v44):
    /// today, that the site is STOPPED and the link shows the stop page.
    ///
    /// Recomputed on every report rather than recorded at start — a site can be
    /// stopped after its tunnel exists, and a warning captured once would keep
    /// saying whatever was true at spawn time.
    pub warning: Option<String>,
}

/// The warning a share of this site carries right now, or `None`.
///
/// One function so the toast, the Tunnels row, the CLI line and an agent's
/// reply cannot drift; the sentence itself lives in `core::tunnels`, beside the
/// rule it describes.
fn share_warning(site: &Site) -> Option<String> {
    (!site.enabled).then(|| tunnels::stopped_share_warning(&site.domain))
}

/// Max time to wait for cloudflared to print the public URL.
const URL_TIMEOUT: Duration = Duration::from_secs(30);

/// Start (or return the existing) public quick tunnel for a site. Returns the
/// public `trycloudflare.com` URL once cloudflared reports it.
#[tauri::command]
pub async fn start_tunnel<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    tunnels: State<'_, Tunnels>,
    provision: State<'_, crate::commands::site_provision::ProvisionJobs>,
    id: String,
) -> Result<TunnelInfo> {
    let site = tunnel_site(&state, &id)?;
    // The origin is the backend that serves THIS site (per-backend origins,
    // 15 Aug 2026): nginx for vhosted sites, the site's own recorded override
    // port for Apache/FrankenPHP — never nginx's default server for a site it
    // has no vhost for (#13's cross-site fallthrough). Resolved before
    // anything spawns, on every path (UI and CLI both land here).
    let origin_port = tunnels::origin_port(&site)?;
    // For an override site, refuse when its backend isn't running. This is a
    // COURTESY SNAPSHOT, not the safety wall: ownership+liveness from the
    // ServiceManager (never a bare port-listen — a squatter on the recorded
    // port must not be published as the site), and the backend stopping after
    // this check merely 502s the site's OWN origin, it cannot serve anyone
    // else's content — the safety property is origin selection above.
    if !crate::core::sites::is_nginx_served(&site) {
        let served = state
            .services
            .lock()
            .await
            .override_pids()
            .iter()
            .any(|(d, _)| d == &site.domain);
        if !served {
            return Err(Error::Other(format!(
                "{domain} isn't being served right now — its {server} server is stopped. \
                 Start the site's server (Start all), then share it.",
                domain = site.domain,
                server = site.web_server.as_db(),
            )));
        }
    }
    let domain = site.domain.clone();

    // A dead child must never read "already sharing": sweep exited children
    // first, so an existing entry is a LIVE process and its URL is this run's.
    // A crashed tunnel therefore falls through to a fresh start instead of
    // handing back its stale URL as success.
    settle_dead(&state, tunnels.take_dead());
    {
        let map = tunnels.0.lock().map_err(|_| Error::Other("tunnel registry poisoned".into()))?;
        if let Some(e) = map.get(&domain) {
            return Ok(TunnelInfo {
                domain,
                url: e.url.clone(),
                running: true,
                health: e.health,
                diagnosis: e.diagnosis,
                warning: share_warning(&site),
            });
        }
    }

    // CLAIM the domain's row BEFORE resolving or spawning anything (step 4).
    // The insert is atomic (`ON CONFLICT DO NOTHING`), so of two concurrent
    // starts exactly one proceeds — the loser errors here, before it can
    // resolve a binary, spawn a second cloudflared, or truncate the shared
    // log the winner is polling. The claim doubles as the v23 lifecycle
    // record: a quit or crash from this point on finds the row (exit hook /
    // launch sweep), never an untracked child.
    {
        let claimed = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))
            .and_then(|conn| {
                crate::state::store::try_claim_tunnel(
                    &conn,
                    &domain,
                    tunnels::PID_PENDING,
                    &site.path,
                )
            })?;
        if !claimed {
            return Err(Error::Other(format!(
                "a share for {domain} is already starting or running — stop it first if it \
                 looks stuck"
            )));
        }
    }

    // Step-7 exposure guards, deliberately AFTER the claim: a job that races
    // this start either sees our row (its guard refuses) or set its marker
    // before we look here — the claim is what makes this side's ordering
    // sound. A tunnel publishes the site, so sharing mid-mutation would hand
    // visitors a half-built/half-restored/mid-change site; refusal releases
    // the claim, and rexenv never cancels the user's job to make room.
    let mid_mutation = if provision.busy_for(&domain) {
        Some("it's still being set up — the link would publish a half-built site. Share it when setup finishes")
    } else if state
        .db_import_active
        .lock()
        .ok()
        .is_some_and(|a| a.as_deref() == Some(domain.as_str()))
    {
        Some("its database is being imported right now — the link would publish a half-restored site. Share it again when the import finishes")
    } else if state
        .rewrite_active
        .lock()
        .ok()
        .is_some_and(|a| a.as_deref() == Some(domain.as_str()))
    {
        Some("its connection settings are being rewritten right now — the link would publish a site mid-change. Share it again when that finishes")
    } else {
        None
    };
    if let Some(why) = mid_mutation {
        delete_tunnel_row(&state, &domain);
        return Err(Error::Other(format!("can't share {domain}: {why}")));
    }

    let platform = state.platform.as_ref();
    let bin = match binaries::resolve(platform, "cloudflared", binaries::CLOUDFLARED_VERSION).await
    {
        Ok(bin) => bin,
        Err(e) => {
            delete_tunnel_row(&state, &domain); // release the claim
            return Err(e);
        }
    };
    let mut child = match tunnels::start(platform, &bin, &domain, origin_port) {
        Ok(child) => child,
        Err(e) => {
            delete_tunnel_row(&state, &domain);
            return Err(e);
        }
    };
    // The child's real pid lands on the claim; if it can't be written the
    // crash story is broken for this tunnel — fail the start rather than run
    // a public tunnel the exit hook couldn't kill.
    let pid_for_guard = child.id();
    {
        let recorded = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))
            .and_then(|conn| crate::state::store::set_tunnel_pid(&conn, &domain, child.id()));
        match recorded {
            Ok(true) => {}
            // The claim vanished: a stop or site delete raced this start and
            // revoked it. Cancel — killing our own child — rather than run a
            // tunnel nothing tracks.
            Ok(false) => {
                let _ = tunnels::stop(platform, child.id());
                let _ = child.wait();
                return Err(Error::Other(format!(
                    "sharing {domain} was stopped while it was still starting"
                )));
            }
            Err(e) => {
                let _ = tunnels::stop(platform, child.id());
                let _ = child.wait();
                delete_tunnel_row(&state, &domain);
                return Err(Error::Other(format!(
                    "tunnel start aborted: its lifecycle record could not be written: {e}"
                )));
            }
        }
    }

    // Third leg of "tunnels die with the app" (ledger #432): a detached guard
    // that ends THIS share when rexenv dies without running any of its own
    // shutdown code. Spawned as soon as the pid is recorded — before the URL
    // exists, because a crash during the 30s URL poll leaves exactly the same
    // orphan. Best-effort by design: a share that could not get a guard is
    // still covered by the exit hook and the launch sweep, and refusing to
    // share over a missing watcher would be a worse trade than the window it
    // closes.
    if let Err(e) = platform.supervisor().guard_child_against_our_death(pid_for_guard, &domain) {
        log::warn!(
            "tunnels: {domain} is sharing WITHOUT a parent-death guard ({e}) — a crash will \
             leave it public until the next launch"
        );
    }

    // Poll the log for the public URL (async sleeps — don't block the executor).
    let deadline = Instant::now() + URL_TIMEOUT;
    let url = loop {
        if let Some(u) = tunnels::read_url(platform, &domain) {
            break u;
        }
        // A child that died can never print a URL — fail NOW with the honest
        // reason instead of burning the rest of the 30s. Also how a stop that
        // raced this start resolves: stop kills the recorded pid, this poll
        // notices within an interval and releases everything.
        if matches!(child.try_wait(), Ok(Some(_))) {
            delete_tunnel_row(&state, &domain);
            return Err(Error::Other(format!(
                "cloudflared exited before reporting a public URL — see logs/tunnel-{domain}.log"
            )));
        }
        if Instant::now() >= deadline {
            let _ = tunnels::stop(platform, child.id());
            let _ = child.wait();
            // A failed start settles its own row — never left for the sweep.
            delete_tunnel_row(&state, &domain);
            return Err(Error::Other("cloudflared did not report a public URL in time".into()));
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    };

    // WordPress: bake the public origin into the URL-rewrite mu-plugin so the
    // whole site (admin, menus, permalinks, media, previews) is navigable from
    // any device through the tunnel — not just on this machine (§9.2). Sharing
    // without it is broken enough that a write failure fails the start.
    if site.site_type == SiteType::Wordpress {
        // A SUBDOMAIN network needs one more thing, and it cannot live in the
        // mu-plugin: COOKIE_DOMAIN is pinned before mu-plugins load, so without
        // this the share serves pages and refuses every login. No-op for every
        // other site, and written once (see `ensure_subdomain_cookie_scope`).
        if let Err(e) = wp_tunnel::ensure_subdomain_cookie_scope(
            Path::new(&site.path),
            site.content_dir_rel(),
        ) {
            let _ = tunnels::stop(state.platform.as_ref(), child.id());
            let mut c = child;
            let _ = c.wait();
            delete_tunnel_row(&state, &domain);
            return Err(Error::Other(format!(
                "tunnel started but this subdomain network could not be made loginable: {e}"
            )));
        }
        match wp_tunnel::enable(Path::new(&site.path), site.content_dir_rel(), &url) {
            Ok(created_dir) => {
                if created_dir {
                    record_mu_dir_created(&state, &site.id);
                }
            }
            Err(e) => {
                let _ = tunnels::stop(state.platform.as_ref(), child.id());
                let mut c = child;
                let _ = c.wait();
                delete_tunnel_row(&state, &domain);
                return Err(Error::Other(format!(
                    "tunnel started but the URL-rewrite mu-plugin could not be written: {e}"
                )));
            }
        }
    }

    // The pid, read before the child moves into the registry: it is one of the
    // three facts that identify an exposure in the log (ledger #430).
    let pid = child.id();
    tunnels.0.lock().map_err(|_| Error::Other("tunnel registry poisoned".into()))?.insert(
        domain.clone(),
        TunnelEntry {
            child,
            url: url.clone(),
            docroot: site.path.clone(),
            health: crate::core::tunnels::TunnelHealth::Unverified,
            strikes: 0,
            diagnosis: None,
            // Phase A: the system resolver stays untouched until the record
            // is provably in public DNS (gate_opens) — our own immediate
            // probe was the poisoning query.
            system_probing: crate::core::tunnels::PhaseGate::default(),
            started: Instant::now(),
        },
    );
    // The app-wide audit line for a public exposure, written AFTER the share is
    // live and registered so the log never claims one that failed to start
    // (every failure path above returns before here). Until 30 Aug 2026 the
    // success path logged nothing at all — see `share_started_line`.
    log::info!("{}", tunnels::share_started_line(&domain, &url, pid, origin_port));
    // First verdict promptly instead of waiting out a full prober tick —
    // "Unverified" right after a successful start should be seconds, not 30.
    let probe_domain = domain.clone();
    tauri::async_runtime::spawn(async move {
        probe_and_record(&app, &probe_client(), &probe_domain).await;
    });
    Ok(TunnelInfo {
        domain,
        url,
        running: true,
        health: crate::core::tunnels::TunnelHealth::Unverified,
        diagnosis: None,
        // A stopped site is SHARED, not refused (the owner's call) — but never
        // silently: this is the sentence the caller shows.
        warning: share_warning(&site),
    })
}

/// Stop a site's tunnel (no-op if not sharing).
#[tauri::command]
pub async fn stop_tunnel(
    state: State<'_, AppState>,
    tunnels: State<'_, Tunnels>,
    id: String,
) -> Result<()> {
    let site = tunnel_site(&state, &id)?;
    tunnels.stop_for_domain(&state, &site.domain);
    // Best-effort: the tunnel is already down, so a leftover mu-plugin is inert
    // (its dead URL receives no requests) — don't fail the stop over it.
    if site.site_type == SiteType::Wordpress {
        if let Err(e) = wp_tunnel::disable(Path::new(&site.path)) {
            log::warn!("rexenv: could not remove the tunnel mu-plugin for {}: {e}", site.domain);
        }
    }
    Ok(())
}

/// All active tunnels (domain → public URL + health). Dead children are
/// settled BEFORE the snapshot, so a crashed cloudflared drops out of the
/// list on the very poll that discovers it — never rendered as Live.
#[tauri::command]
pub async fn tunnels_status(
    state: State<'_, AppState>,
    tunnels: State<'_, Tunnels>,
) -> Result<Vec<TunnelInfo>> {
    settle_dead(&state, tunnels.take_dead());
    // The stopped-site warning is derived HERE, on every poll, from the site
    // rows — so stopping a site that is already shared starts warning about it,
    // and starting it again stops. One read for the whole list.
    let stopped: std::collections::HashSet<String> = {
        let conn = state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))?;
        crate::core::sites::list(&conn)?
            .into_iter()
            .filter(|s| !s.enabled)
            .map(|s| s.domain)
            .collect()
    };
    let map = tunnels.0.lock().map_err(|_| Error::Other("tunnel registry poisoned".into()))?;
    let mut out: Vec<TunnelInfo> = map
        .iter()
        .map(|(domain, e)| TunnelInfo {
            domain: domain.clone(),
            url: e.url.clone(),
            running: true,
            health: e.health,
            diagnosis: e.diagnosis,
            warning: stopped
                .contains(domain)
                .then(|| tunnels::stopped_share_warning(domain)),
        })
        .collect();
    out.sort_by(|a, b| a.domain.cmp(&b.domain));
    Ok(out)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn entry(child: Child) -> TunnelEntry {
        TunnelEntry {
            child,
            url: "https://x.trycloudflare.com".into(),
            docroot: "/nonexistent-fixture".into(),
            health: crate::core::tunnels::TunnelHealth::Unverified,
            strikes: 0,
            diagnosis: None,
            system_probing: crate::core::tunnels::PhaseGate::default(),
            started: Instant::now(),
        }
    }

    /// #32 — **a dead tunnel is settled BEFORE the snapshot, so it never
    /// renders as a live share on the poll that discovers it.**
    ///
    /// The order is the whole claim. Snapshot first and the reply contains a
    /// row whose `running` field is a hardcoded `true` — the map's entries are
    /// live by construction — so the UI paints a public link that stopped
    /// working, and the user hands it to someone. Settle first and the entry is
    /// gone before anything reads the map.
    ///
    /// A SOURCE-ORDER guard because there is no way to observe it otherwise:
    /// both orders return the same type, both compile, and the wrong one is
    /// only wrong for the length of one poll.
    #[test]
    fn a_dead_tunnel_is_settled_before_the_status_snapshot() {
        let src = crate::core::copy_scan::production_source(include_str!("tunnels.rs"));
        let body = src
            .split("pub async fn tunnels_status(")
            .nth(1)
            .and_then(|b| b.split("\n#[").next())
            .expect("tunnels_status");
        let settle = body
            .find("settle_dead(")
            .expect("`tunnels_status` no longer settles dead children — a crashed cloudflared \
                     then stays in the list, rendered as a live public link");
        let snapshot = body
            .find("tunnels.0.lock()")
            .expect("`tunnels_status` no longer reads the registry — if it moved, move this guard");
        assert!(
            settle < snapshot,
            "the registry is snapshotted BEFORE dead children are settled: the reply then \
             carries a share whose process is gone, with `running: true` — and the user hands \
             that link to somebody"
        );
        // …and the row it builds says running unconditionally, which is only
        // honest BECAUSE of the order above. If this ever becomes a computed
        // field, the order stops being the thing that makes it true and this
        // guard is measuring the wrong fact.
        assert!(
            body.contains("running: true"),
            "`running` is no longer a constant in the snapshot — the settle-first ordering was \
             what made it honest, so re-read what makes it true now"
        );
    }

    #[test]
    fn take_dead_removes_only_exited_children() {
        // Fixture-owned children: /usr/bin/true exits immediately (the
        // crashed-cloudflared stand-in), sleep stays alive and is killed +
        // reaped by this test before it returns.
        let reg = Tunnels::default();
        let dead = std::process::Command::new("/usr/bin/true").spawn().expect("spawn true");
        let live = std::process::Command::new("/bin/sleep").arg("30").spawn().expect("spawn sleep");
        reg.0.lock().unwrap().insert("dead.rex".into(), entry(dead));
        reg.0.lock().unwrap().insert("live.rex".into(), entry(live));

        let mut settled = Vec::new();
        for _ in 0..100 {
            settled = reg.take_dead();
            if !settled.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(settled.len(), 1, "exactly the exited child is taken");
        assert_eq!(settled[0].0, "dead.rex");
        // The live tunnel keeps its entry — liveness is per-child, not a purge.
        assert!(reg.0.lock().unwrap().contains_key("live.rex"));

        let mut e = reg.0.lock().unwrap().remove("live.rex").expect("live entry");
        let _ = e.child.kill();
        let _ = e.child.wait();
    }
}

/// #37 — internal tooling can never become a tunnel origin.
///
/// Adminer is a passwordless database browser served on an internal vhost. It
/// has no `Site` row, and the ONLY way to start a tunnel is a site id resolved
/// through `tunnel_site` — so "Adminer can never be shared publicly" is a
/// consequence of the lookup, not a check anyone has to remember. This pins
/// that shape: a second target source (a domain parameter, a host string) would
/// make the claim depend on a validator instead of on there being nothing to
/// validate.
#[cfg(test)]
mod adminer_can_never_be_shared {
    #[test]
    fn a_tunnel_target_can_only_ever_be_a_site_row() {
        let src = crate::core::copy_scan::production_source(include_str!("tunnels.rs"));
        let start = src
            .split("pub async fn start_tunnel<R: tauri::Runtime>(")
            .nth(1)
            .and_then(|b| b.split("\n#[tauri::command]").next())
            .expect("start_tunnel");

        assert!(
            start.contains("tunnel_site(&state, &id)"),
            "`start_tunnel` no longer resolves its target through a Sites lookup. The reason \
             Adminer — a PASSWORDLESS database browser — can never be published is that it has \
             no site row and there is no other way in (#37). A target that came from anywhere \
             else would make that a validation someone has to get right."
        );
        assert!(
            !start.contains("ADMINER_HOST"),
            "`start_tunnel` mentions the Adminer host, which it has no business naming"
        );
        // The lookup itself: a row, or an error. Never a fallback.
        let lookup = src
            .split("fn tunnel_site(")
            .nth(1)
            .and_then(|b| b.split("\n}").next())
            .expect("tunnel_site");
        assert!(
            lookup.contains("ok_or_else"),
            "`tunnel_site` no longer ERRORS on a missing site — a fallback here is how a \
             non-site becomes a tunnel origin (#37)"
        );
    }
}

/// #430 — a public share is never invisible in `rexenv.log`.
///
/// The claim is about CALL SITES, which no value can carry: the formatters are
/// unit-tested in `core/tunnels.rs`, and what regresses is somebody adding a
/// fourth way for a share to start or stop without a line. So each path is
/// asserted where it lives. The one path deliberately absent from this list is
/// `settle_dead` — a cloudflared that died on its own already logs its own
/// WARN, and it is checked here too so a "tidy-up" cannot silence it.
#[cfg(test)]
mod every_share_leaves_a_trail {
    #[test]
    fn every_start_and_every_stop_writes_a_line() {
        let src = crate::core::copy_scan::production_source(include_str!("tunnels.rs"));

        let start = src
            .split("pub async fn start_tunnel<R: tauri::Runtime>(")
            .nth(1)
            .and_then(|b| b.split("\n/// Stop a site's tunnel").next())
            .expect("start_tunnel");
        assert!(
            start.contains("tunnels::share_started_line(&domain, &url, pid, origin_port)"),
            "a share can now start without a line in rexenv.log. That is exactly how the \
             27 Aug 2026 mstest.rex share left no trail: its only record was \
             logs/tunnel-mstest.rex.log, a file you can only open once you already know which \
             domain to suspect (#430)"
        );
        // Position matters: the line must follow the registry insert, or a
        // start that fails after logging would claim an exposure that never
        // existed — the opposite lie, and worse (it sends someone hunting a
        // process that was never public).
        // The LAST registry lock in the function is the insert; the first is
        // the "already sharing" read at the top.
        let after_insert = start.rsplit("tunnels.0.lock()").next().expect("registry insert");
        assert!(
            after_insert.contains("share_started_line"),
            "the start line moved BEFORE the registry insert, so a start that fails afterwards \
             logs a public share that never happened (#430)"
        );

        let stop = src
            .split("pub fn stop_for_domain(")
            .nth(1)
            .and_then(|b| b.split("\n    /// Whether").next())
            .expect("stop_for_domain");
        assert!(
            stop.contains("tunnels::share_stopped_line(domain, pid, e.started.elapsed())"),
            "stopping a share no longer logs. A start line with no stop after it is how the log \
             says \"this was still public\" — a reading that only works if EVERY stop writes \
             one (#430)"
        );
        assert!(
            stop.contains("IN-FLIGHT"),
            "the in-flight kill (claim taken, child spawned, registry entry not yet inserted) \
             stopped logging — the one stop path whose share may never have been public, which \
             is worth saying rather than omitting (#430)"
        );

        let exit = src
            .split("pub fn kill_all_on_exit<R: tauri::Runtime>(")
            .nth(1)
            .and_then(|b| b.split("\n/// May the exit hook").next())
            .expect("kill_all_on_exit");
        assert!(
            exit.contains("share_stopped_line") && exit.contains("rexenv quit"),
            "quitting the app is the commonest way a share ends, and it stopped writing a stop \
             line — so the log would show starts that never close (#430)"
        );

        let settle = src.split("fn settle_dead(").nth(1).expect("settle_dead");
        assert!(
            settle.contains("exited on its own"),
            "a crashed cloudflared no longer logs; the share's start line would never close \
             (#430)"
        );
    }
}

/// The Tier-1 lifecycle guards (#26, #29, #30, #31).
///
/// All four are "a share never outlives / never blocks / never gets stopped for
/// you" claims, and all four were 🔨 because the obvious proof needs a real
/// cloudflared and a Tauri app. What is provable here is the part that actually
/// regresses: the DECISION each one turns on. The live legs stay in the ledger,
/// pointed at the one tunnel example that will carry them together.
#[cfg(test)]
mod lifecycle_guards {
    use super::*;
    use crate::state::{db, store};

    /// #26 — a start's CLAIM never outlives the stop that revoked it.
    ///
    /// The claim is a row, so this is testable for real against SQLite rather
    /// than asserted about code. Two starts race, one wins; the loser must not
    /// get a second claim, and the stop must revoke it whether or not a
    /// registry entry ever existed — an in-flight start has a row and no entry,
    /// which is the case the claim exists for.
    #[test]
    fn a_starts_claim_never_outlives_the_stop_that_revoked_it() {
        let conn = db::open_in_memory().unwrap();

        // Exactly one of two concurrent starts proceeds.
        assert!(store::try_claim_tunnel(&conn, "a.rex", tunnels::PID_PENDING, "/d").unwrap());
        assert!(
            !store::try_claim_tunnel(&conn, "a.rex", tunnels::PID_PENDING, "/d").unwrap(),
            "a second start claimed the same domain — two cloudflareds would share one log"
        );

        // The stop path revokes by DELETING the row, which is what a start
        // still polling for its URL reads to learn it was cancelled.
        assert!(store::delete_tunnel(&conn, "a.rex").unwrap());
        assert!(
            store::get_tunnel(&conn, "a.rex").unwrap().is_none(),
            "the claim outlived the stop — the domain reads as shared with nothing running"
        );
        // …and the domain is claimable again, or a stop would strand it.
        assert!(store::try_claim_tunnel(&conn, "a.rex", tunnels::PID_PENDING, "/d").unwrap());

        // `set_tunnel_pid` returning false is how an in-flight start learns its
        // claim was revoked mid-spawn and cancels itself.
        store::delete_tunnel(&conn, "a.rex").unwrap();
        assert!(
            !store::set_tunnel_pid(&conn, "a.rex", 4242).unwrap(),
            "a revoked start could still record its pid, and the child would outlive the stop"
        );
    }

    /// #30 — a pid the registry pass already reaped is never signalled again.
    #[test]
    fn the_exit_hook_never_signals_a_reaped_or_sentinel_pid() {
        let mut reaped = std::collections::HashSet::new();
        reaped.insert(4242u32);

        assert!(
            !should_signal_row(4242, &reaped),
            "a reaped pid was signalled again — after wait() the number is free for reuse, so \
             this is a signal to whatever now owns it (the never-kill-on-a-bare-pid rule)"
        );
        assert!(
            !should_signal_row(tunnels::PID_PENDING, &reaped),
            "a sentinel row has no process behind it; signalling PID_PENDING is signalling a \
             number that never was a pid"
        );
        assert!(
            should_signal_row(777, &reaped),
            "an unreaped child from this session must still be killed — tunnels die with the app"
        );
    }

    /// #31 and #29 — ordering and call-site facts, which no value can carry.
    ///
    /// #31: a crashed cloudflared must never answer "already sharing". Both
    /// readers settle dead children BEFORE reading, so the answer can't come
    /// from a corpse.
    ///
    /// **A share of a stopped site warns — and keeps warning, because the
    /// warning is DERIVED on every report.**
    ///
    /// The owner's call (4 Sep 2026) is a warning rather than a refusal: the
    /// link works, and what it publishes is rexenv's "site stopped" page, which
    /// there are real reasons to want standing. What it must never be is
    /// silent. The trap is the obvious implementation — decide at `start` and
    /// remember — because the ordinary sequence is share first, stop the site
    /// later, and a warning captured at spawn time would then say the opposite
    /// of what the link shows. Same shape as this repo's two lifetime-guard
    /// defects: a one-time check on a mutable fact is a snapshot.
    #[test]
    fn the_stopped_share_warning_is_recomputed_on_every_report_not_captured_at_start() {
        let src = crate::core::copy_scan::production_source(include_str!("tunnels.rs"));
        let body = src
            .split("pub async fn tunnels_status(")
            .nth(1)
            .and_then(|b| b.split("\n#[").next())
            .expect("tunnels_status");
        assert!(
            body.contains("core::sites::list(") && body.contains("s.enabled"),
            "`tunnels_status` no longer reads the site rows, so a site stopped AFTER it was \
             shared reports no warning — and the link goes on showing the stop page while \
             the app says nothing"
        );
        assert!(
            body.contains("stopped_share_warning("),
            "the warning text is no longer the one in `core::tunnels` — the app, the CLI and \
             an agent must all say the same sentence"
        );
        // And it is a WARNING, not a refusal: nothing on this path returns an
        // error for a stopped site.
        let start = src
            .split("pub async fn start_tunnel<R: tauri::Runtime>(")
            .nth(1)
            .and_then(|b| b.split("\n/// ").next())
            .expect("start_tunnel");
        assert!(
            !start.contains("is stopped in rexenv"),
            "sharing a stopped site now REFUSES; the ruling is that it shares and warns"
        );
        assert!(start.contains("share_warning("), "the start reply must carry the warning");
    }

    /// #29: rexenv never stops a share on the USER'S behalf. The reaper is
    /// where that now bites — it deletes sites unattended, and
    /// `delete_site_owned`'s first act is to stop the site's tunnel. It must
    /// SKIP a shared site instead, and `commands/scratch.rs` must therefore
    /// never reach `stop_for_domain`.
    #[test]
    fn a_dead_child_never_reads_as_sharing_and_the_reaper_never_stops_a_share() {
        let src = crate::core::copy_scan::production_source(include_str!("tunnels.rs"));

        // #31 — every reader settles the dead first.
        for (what, head) in [
            ("sharing_domain", "pub(crate) fn sharing_domain("),
            ("start_tunnel", "pub async fn start_tunnel<R: tauri::Runtime>("),
        ] {
            let body = src.split(head).nth(1).unwrap_or_else(|| panic!("{what} moved"));
            let before_read = body.split("self.0.lock()").next().unwrap_or(body);
            let before_read = before_read.split("tunnels.0.lock()").next().unwrap_or(before_read);
            assert!(
                before_read.contains("take_dead()"),
                "{what} reads the registry (or the row) before settling dead children, so a \
                 crashed cloudflared answers \"already sharing\" and the user cannot re-share \
                 or run a job on that site (#31)"
            );
        }

        // #29 — the reaper skips, and cannot stop.
        let reaper = crate::core::copy_scan::production_source(include_str!("scratch.rs"));
        assert!(
            reaper.contains("sharing_domain("),
            "the reaper no longer checks whether a scratch site is publicly shared before \
             deleting it (#29/#215)"
        );
        assert!(
            !reaper.contains("stop_for_domain("),
            "the reaper stops a share on the user's behalf. rexenv never does that — the ruling \
             is SKIP, not stop-then-delete, and unattended is the worst place to break it (#29)"
        );
    }
}

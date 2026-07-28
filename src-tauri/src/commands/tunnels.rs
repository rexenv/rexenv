//! commands::tunnels — Tauri IPC for per-site cloudflared quick tunnels (§9.1).
//!
//! Owns the live tunnels in a Tauri-managed registry keyed by site domain. A
//! tunnel can only be started for a real site (looked up by id) — internal
//! tooling vhosts aren't sites, so they can never be shared.

use crate::core::{binaries, services, tunnels, wp_tunnel};
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
                let _ = tunnels::stop(state.platform.as_ref(), e.child.id());
                let _ = e.child.wait();
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
    let Some((url, prev_health, strikes)) = registry
        .0
        .lock()
        .ok()
        .and_then(|m| m.get(domain).map(|e| (e.url.clone(), e.health, e.strikes)))
    else {
        return;
    };
    let outcome = crate::core::tunnels::probe_url(client, &url).await;
    let (health, strikes) = crate::core::tunnels::fold_probe(prev_health, strikes, outcome);
    if let Ok(mut m) = registry.0.lock() {
        if let Some(e) = m.get_mut(domain) {
            if e.url == url {
                e.health = health;
                e.strikes = strikes;
            }
        }
    };
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
            for domain in domains {
                probe_and_record(&app, &client, &domain).await;
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
    let app = app.clone();
    std::thread::spawn(move || {
        use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
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
        QUIT_DIALOG_OPEN.store(false, Ordering::SeqCst);
        if confirmed {
            QUIT_CONFIRMED.store(true, Ordering::SeqCst);
            app.exit(0);
        }
    });
    false
}

/// App-exit hook: tunnels DIE WITH THE APP (lifecycle ruling 28 Jul 2026 —
/// `docs/PLAN-tunnel-lifecycle.md`). A service outliving the app serves the
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
        for (_, mut e) in map.drain() {
            let pid = e.child.id();
            let _ = tunnels::stop(state.platform.as_ref(), pid);
            let _ = e.child.wait();
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
        if row.pid != tunnels::PID_PENDING && !reaped.contains(&row.pid) {
            // Our own child from THIS session (launch already swept older
            // rows), unreaped, so the pid can't have been recycled. A
            // sentinel row (claim taken, child not yet spawned) has no
            // process to signal — its file/row still get cleaned below.
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
    // Cross-site exposure wall (step 3): an override site has no nginx vhost,
    // and the tunnel's origin IS nginx — refused before anything spawns, on
    // every path (UI and CLI both land here).
    tunnels::ensure_tunnelable(&site)?;
    let domain = site.domain.clone();

    // A dead child must never read "already sharing": sweep exited children
    // first, so an existing entry is a LIVE process and its URL is this run's.
    // A crashed tunnel therefore falls through to a fresh start instead of
    // handing back its stale URL as success.
    settle_dead(&state, tunnels.take_dead());
    {
        let map = tunnels.0.lock().map_err(|_| Error::Other("tunnel registry poisoned".into()))?;
        if let Some(e) = map.get(&domain) {
            return Ok(TunnelInfo { domain, url: e.url.clone(), running: true, health: e.health });
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
    let mut child = match tunnels::start(platform, &bin, &domain, services::NGINX_HTTP_PORT) {
        Ok(child) => child,
        Err(e) => {
            delete_tunnel_row(&state, &domain);
            return Err(e);
        }
    };
    // The child's real pid lands on the claim; if it can't be written the
    // crash story is broken for this tunnel — fail the start rather than run
    // a public tunnel the exit hook couldn't kill.
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

    tunnels.0.lock().map_err(|_| Error::Other("tunnel registry poisoned".into()))?.insert(
        domain.clone(),
        TunnelEntry {
            child,
            url: url.clone(),
            docroot: site.path.clone(),
            health: crate::core::tunnels::TunnelHealth::Unverified,
            strikes: 0,
        },
    );
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
    let map = tunnels.0.lock().map_err(|_| Error::Other("tunnel registry poisoned".into()))?;
    let mut out: Vec<TunnelInfo> = map
        .iter()
        .map(|(domain, e)| TunnelInfo {
            domain: domain.clone(),
            url: e.url.clone(),
            running: true,
            health: e.health,
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
        }
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

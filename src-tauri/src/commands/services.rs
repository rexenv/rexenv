//! commands::services — IPC for the shared-service lifecycle (task 10.5).

use crate::core;
use crate::error::{Error, Result};
use crate::state::app::AppState;
use serde::Serialize;
use tauri::State;

/// One service's status + live metrics (mirrors the frontend `ServiceStatus`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    pub name: String,
    pub running: bool,
    pub pid: Option<u32>,
    pub port: u16,
    pub cpu_percent: f32,
    pub ram_mb: u64,
    /// Services-page grouping ("php" | "database" | "mail" | "web") — derived
    /// HERE so the UI never name-sniffs.
    pub kind: &'static str,
    /// PHP minor for pool rows (e.g. "8.3"); `None` elsewhere.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// `Some(bool)` ONLY for shared PHP pool rows: is this minor the default for
    /// new sites? The Set-default control keys off `Some(false)` — FrankenPHP
    /// per-site rows get `None` so they can never grow that control.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_default: Option<bool>,
    /// The served site for per-site FrankenPHP override rows — the UI renders
    /// it as the row's sub-line (NOT crammed into the version badge).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    /// True for services Start-all does NOT manage (user-toggled engines like
    /// Postgres) — the footer counts them only while running.
    pub optional: bool,
    /// Set ONLY for independently-toggleable services (DB engines → their
    /// `start_database` key, Mailpit → "mailpit"): the Services row renders a
    /// per-row Start/Stop toggle for these. The serving core (edge, nginx,
    /// pools, FrankenPHP) is one organism — half-states are broken by design —
    /// so its rows get `None` and the group-managed hint instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_key: Option<String>,
    /// Set ONLY for a running shared pool whose every worker is holding a request, sustained
    /// (`core::pool_busy`, ledger #608): the row's sub-line, e.g. "all 10 workers busy — requests
    /// are queuing".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub busy_note: Option<String>,
    /// What to SHOW instead of `name`, when the two differ: a Windows pool is a php-cgi group,
    /// not a php-fpm pool, so its row reads `PHP-CGI 8.3` while `name` stays `PHP-FPM 8.3` —
    /// the key `kind_of`, `version`, `is_default`, the busy tracker and the restart counters
    /// all still work from (ledger #651). Absent when they are the same.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Group a service row by its canonical name (the manager names them).
fn kind_of(name: &str) -> &'static str {
    let n = name.to_ascii_lowercase();
    if n.starts_with("php-fpm") || n.starts_with("frankenphp") {
        "php"
    } else if n.contains("mysql") || n.contains("postgres") || n.contains("maria") || n.contains("redis") {
        "database"
    } else if n.contains("mailpit") {
        "mail"
    } else {
        "web"
    }
}

/// Per-minor PHP ini settings, as loaded from the `php_settings` table.
type PhpSettingsMap = std::collections::HashMap<String, Vec<(String, String)>>;

/// Snapshot the site list + installed PHP minors + per-version ini settings +
/// per-site env vars + the mail catch-all (locking the DB briefly, never across
/// `.await`).
#[allow(clippy::type_complexity)] // one snapshot tuple, unpacked immediately
fn start_inputs(
    state: &AppState,
) -> Result<(
    Vec<crate::state::models::Site>,
    Vec<String>,
    PhpSettingsMap,
    PhpSettingsMap,
    std::collections::HashMap<String, Vec<String>>,
    std::collections::HashMap<crate::core::db::DbEngine, String>,
    std::collections::HashMap<String, String>,
    String,
    bool,
)> {
    let conn = state
        .db
        .lock()
        .map_err(|_| Error::Other("database lock poisoned".into()))?;
    let sites = core::sites::list(&conn)?;
    let minors = core::php::installed_minors(&conn)?;
    let php_settings = crate::state::store::all_php_settings(&conn)?;
    let site_env = crate::state::store::all_site_env(&conn)?;
    let site_aliases = crate::state::store::all_site_aliases(&conn)?;
    let db_versions = crate::core::db::DbEngine::ALL
        .into_iter()
        .filter(|e| e.available())
        .map(|e| (e, e.effective_version(&conn)))
        .collect::<std::collections::HashMap<_, _>>();
    // The user's Update choices, already floored by the pin. Snapshotted with
    // everything else so the pool manager is handed one consistent view and never
    // reads the database itself.
    let php_patches = core::php::effective_patches(&conn)?;
    // Same snapshot rule for Adminer: read ONCE here, then passed down. The
    // planner and the stager must agree, or login-start's offline guard clears a
    // start against a plan for bytes nobody stages (ledger #175).
    let adminer_version = core::adminer::effective_version(state.platform.as_ref(), &conn);
    // Read HERE with everything else rather than deeper down, for the snapshot
    // rule above and because the pool manager must never touch SQLite: whether
    // a site's mail is caught is a user setting, and the pools that are about to
    // be written have to agree with the one the user last chose.
    let catch_mail = core::mail::catch_all_enabled(&conn);
    Ok((sites, minors, php_settings, site_env, site_aliases, db_versions, php_patches, adminer_version, catch_mail))
}

/// Start the shared stack (MySQL + a php-fpm pool per installed PHP version +
/// Nginx + Caddy). Downloads binaries on first run; gated on free ports.
#[tauri::command]
pub async fn start_services(state: State<'_, AppState>) -> Result<()> {
    start_stack(state.inner()).await
}

/// The Start-all sequence, callable from anywhere that holds the app state —
/// the button, and a site provision whose serve phase finds the stack stopped
/// (18 Sep 2026: a first site used to settle as "serves on next stack start",
/// which the clean-VM smoke test read as the headline promise — "rexenv will
/// serve it instantly" — being false for the very first site anyone creates).
///
/// Five phases, and which lock each holds is the point (M3/M4): downloads and
/// readiness waits and the privileged edge prompt all run with the services
/// lock FREE, so status polls stay live throughout.
pub async fn start_stack(state: &AppState) -> Result<()> {
    let (sites, php_minors, php_settings, site_env, site_aliases, db_versions, php_patches, adminer_version, catch_mail) =
        start_inputs(state)?;
    // Phase 0 (UNLOCKED): plan the full binary set, then prefetch every missing
    // one through the download hub — real progress events for the UI, EVERY
    // failure surfaced (not just the first), and no download ever streams while
    // the services lock is held (status polls stay live on a cold first run).
    // After this, the resolves inside start_core are cache hits.
    let plan = core::downloads::plan_for_start_with(
        state.platform.as_ref(),
        &sites,
        &php_minors,
        &db_versions,
        &php_patches,
        &adminer_version,
    );
    core::downloads::prefetch(state.platform.as_ref(), "Start all", &plan).await?;
    // Phase 1 (locked): spawn everything except the edge; collect the readiness
    // probes + Caddyfile. Spawning is fast — no waiting happens under the lock.
    let (caddyfile, checks) = {
        let mut mgr = state.services.lock().await;
        // Per-version ini settings feed the pool configs written by start_core;
        // per-site env vars feed the nginx/FrankenPHP configs (§1.6).
        mgr.set_php_settings(php_settings);
        mgr.set_php_patches(php_patches);
        mgr.set_site_env(site_env);
        mgr.set_site_aliases(site_aliases);
        mgr.set_db_versions(db_versions);
        mgr.start_core(state.platform.as_ref(), &state.ca, &sites, &php_minors, &adminer_version, catch_mail)
            .await?
    };
    // Phase 2 (UNLOCKED): await readiness concurrently — a slow MySQL/Mailpit/
    // FrankenPHP no longer parks the service manager (M4); a service that never
    // comes up still fails HERE naming itself + its log (M3).
    core::service_manager::await_ready(checks).await?;
    // Phase 3 (locked briefly): gate the edge start now that backends are ready.
    let plan = {
        let mut mgr = state.services.lock().await;
        mgr.prepare_edge(state.platform.as_ref(), caddyfile)?
    };
    // Phase 4 (UNLOCKED): the privileged edge start blocks on the admin-password
    // prompt — with the services lock free, status polls keep working meanwhile.
    if let Some(plan) = plan {
        if plan.privileged {
            // Start the root edge under launchd KeepAlive so it stays up across any
            // death/sleep/reboot. First run installs the daemon (one admin prompt);
            // later starts just enable + kickstart. `caddy_bin` is the install SOURCE.
            core::prompt::while_prompting(|| {
                core::proxy::start_edge_daemon(state.platform.as_ref(), &plan.caddy_bin, &plan.caddyfile)
            })?;
            state.services.lock().await.set_edge_daemon();
        } else {
            let child = core::proxy::start(state.platform.as_ref(), &plan.caddy_bin, &plan.caddyfile)?;
            state.services.lock().await.set_edge_child(child);
        }
    }
    // Phase 5 (UNLOCKED): positive WIRE identity. The edge process being up is not
    // enough — a foreign proxy that binds 127.0.0.1:443 specifically (Herd) shadows
    // our wildcard listener with no bind error anywhere, and every green check above
    // passes while all traffic lands on the other tool. Fail Start-all honestly,
    // naming the interceptor, instead of reporting a stack that can't serve.
    verify_edge_wire(state).await
}

/// Positive wire-identity gate shared by Start-all and login auto-start: OUR edge
/// must be what answers loopback `:443` (marker-header probe), else error naming
/// the intercepting process. The watchdog keeps re-checking afterwards and flips
/// the status truthfully (`edge-blocked` / `edge-unblocked` events).
async fn verify_edge_wire(state: &AppState) -> Result<()> {
    let wire = core::proxy::edge_wire(
        core::adminer::ADMINER_HOST,
        core::proxy::DEFAULT_HTTPS_PORT,
    )
    .await;
    if wire == core::proxy::EdgeWire::Ours {
        return Ok(());
    }
    // NOTHING is listening, and we just started the stack — so this is not a
    // foreign proxy, it is OUR OWN start not taking. Naming a holder here (the
    // old message did, from a lookup that finds nobody) sends someone hunting
    // for a program that is not running, in the one situation where the thing
    // that failed is ours. Different problem, different fix.
    if wire == core::proxy::EdgeWire::NoAnswer {
        return Err(Error::Other(
            "services started, but nothing is answering port 443 — rexenv's edge did not come \
             up. Nothing else is holding the port, so this is ours to fix: check the edge log \
             (Services → Caddy → Logs), then Start all again."
                .into(),
        ));
    }
    let help = state
        .platform
        .supervisor()
        .port_conflict_help(core::proxy::DEFAULT_HTTPS_PORT, false);
    let holder = help.holder.unwrap_or_else(|| "another local proxy".into());
    // "quit Herd" beats "quit that app" — name the application when we know it.
    let quit = help.app.map_or("that app".to_string(), |a| a.to_string());
    // Trailing "\n$ <cmd>" renders as a copyable command block in the UI toast
    // (lib/toast.ts toastBackendError) — supervisor-aware, so it QUITS a managed
    // app instead of killing a worker its supervisor would respawn.
    let fix = help.free_command.map(|c| format!("\n$ {c}")).unwrap_or_default();
    Err(Error::Other(format!(
        "services are running, but {holder} answers port 443 in front of rexenv — \
         sites cannot load until you quit {quit} (then Start all again, or just \
         wait: rexenv re-checks automatically).{fix}"
    )))
}

/// Stop the shared stack.
#[tauri::command]
pub async fn stop_services(state: State<'_, AppState>) -> Result<()> {
    // Phase 1 (locked, brief): is the edge ours-under-launchd?
    let need_bootout = state.services.lock().await.edge_is_daemon();
    // Phase 2 (UNLOCKED): boot the daemon out FIRST, before ANY manager state is
    // touched. The privileged prompt can sit open for a long time; when stop_all
    // ran first (clearing the handle to Stopped while the edge was still serving),
    // the watchdog re-adopted the doomed edge during the prompt — the bootout then
    // landed on a `Daemon`-marked edge, leaving a stale handle that spammed
    // edge-restarting and made prepare_edge skip every later start. Bootout-first
    // leaves the handle truthful (Daemon && alive) for the whole prompt, and a
    // cancelled prompt errors out here with nothing stopped (all-or-nothing).
    // Runs with the services lock free (M4), like the privileged start.
    if need_bootout {
        core::prompt::while_prompting(|| core::proxy::stop_edge_daemon(state.platform.as_ref()))?;
    }
    // Phase 3 (locked): stop the rest. stop_all sees the Daemon handle and skips
    // the admin-API edge stop (the daemon edge is already down).
    state.services.lock().await.stop_all(state.platform.as_ref())
}

/// Setting key for the opt-in "start services when rexenv opens" behavior
/// (Settings toggle; combined with "Open rexenv at login" it brings the whole
/// stack back after a reboot without a click).
pub const AUTO_START_SETTING: &str = "start_services_on_launch";

/// Opt-in auto-start, run once from app setup when [`AUTO_START_SETTING`] is on.
/// Same flow as [`start_services`] with two LOGIN-SAFETY guards, because this
/// runs unattended at login:
///
/// 1. **Never download** — a cold binary cache aborts with an honest event
///    instead of streaming downloads nobody asked for at login.
/// 2. **Never prompt** — if the edge isn't adoptable (needs the privileged
///    daemon (re)install, e.g. after an explicit Stop-all), the edge is SKIPPED
///    and surfaced, not prompted for. The normal post-reboot path is silent:
///    the boot LaunchDaemon already has the edge up, so `prepare_edge` adopts
///    it over the admin socket — no prompt, whole stack up in seconds.
///
/// Failures surface as `service-health` events (the same toast pipeline the
/// watchdog uses) + the health log, so a broken login-start is never silent.
pub async fn auto_start_services(app: tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    let state = app.state::<AppState>();
    let event = match auto_start_inner(&state).await {
        Ok(Some(detail)) => core::service_manager::HealthEvent {
            service: "Auto-start".into(),
            action: "restarted",
            detail,
        },
        Ok(None) => return, // fully up, silently
        Err(e) => core::service_manager::HealthEvent {
            service: "Auto-start".into(),
            action: "restart-failed",
            detail: e.to_string(),
        },
    };
    core::service_manager::log_health_events(state.platform.as_ref(), std::slice::from_ref(&event));
    log::warn!("auto-start: [{}] {}", event.action, event.detail);
    let _ = app.emit("service-health", vec![event]);
}

/// `Ok(None)` = everything started; `Ok(Some(note))` = started with a caveat
/// (edge skipped); `Err` = aborted (nothing/partial started, reason inside).
async fn auto_start_inner(state: &State<'_, AppState>) -> Result<Option<String>> {
    let (sites, php_minors, php_settings, site_env, site_aliases, db_versions, php_patches, adminer_version, catch_mail) =
        start_inputs(state)?;
    // Guard 1: strictly offline. Every needed binary must already be cached
    // (the decision fn lives in core::downloads with its own test).
    // The EFFECTIVE patches, or login-start checks the pin's cache and then starts
    // pools that need the selection — downloading on the one path whose whole
    // contract is that it never does (ledger #175).
    let plan = core::downloads::plan_for_start_with(
        state.platform.as_ref(),
        &sites,
        &php_minors,
        &db_versions,
        &php_patches,
        &adminer_version,
    );
    let missing = core::downloads::uncached_names(&plan);
    if !missing.is_empty() {
        // "not ready" rather than "not downloaded": since ledger #336 a cache can
        // be present and still not resolvable — the bytes are there and the
        // licence texts beside them are not. Opening the app repairs that
        // (`lib.rs`'s launch task) as well as downloading anything genuinely
        // absent, so one sentence covers both without claiming which it was.
        return Err(Error::Other(format!(
            "binaries not ready yet ({}) — open rexenv once and press Start all",
            missing.join(", ")
        )));
    }
    let (caddyfile, checks) = {
        let mut mgr = state.services.lock().await;
        mgr.set_php_settings(php_settings);
        mgr.set_php_patches(php_patches);
        mgr.set_site_env(site_env);
        mgr.set_site_aliases(site_aliases);
        mgr.set_db_versions(db_versions);
        mgr.start_core(state.platform.as_ref(), &state.ca, &sites, &php_minors, &adminer_version, catch_mail).await?
    };
    core::service_manager::await_ready(checks).await?;
    let plan = {
        let mut mgr = state.services.lock().await;
        mgr.prepare_edge(state.platform.as_ref(), caddyfile)?
    };
    // Guard 2 is the pure `login_edge_action` decision (tested in core): a
    // privileged plan would show an auth prompt at login — skipped, surfaced.
    // (Normally unreachable post-reboot: RunAtLoad has the edge up before
    // login.)
    match core::service_manager::login_edge_action(&plan) {
        // Edge adopted (the boot daemon already serves it) or reloaded — but adopt
        // proves the PROCESS, not the wire: verify nothing (Herd) intercepts :443.
        core::service_manager::LoginEdgeAction::AlreadyServing => {
            verify_edge_wire(state).await.map(|()| None)
        }
        core::service_manager::LoginEdgeAction::SkipNeedsPrompt => Ok(Some(
            "services are up, but the HTTPS edge needs Start all (one admin prompt)".into(),
        )),
        // Unprivileged high-port edge (dev config) — no prompt, just start it.
        core::service_manager::LoginEdgeAction::StartUnprivileged => {
            let plan = plan.expect("StartUnprivileged implies a plan");
            let child =
                core::proxy::start(state.platform.as_ref(), &plan.caddy_bin, &plan.caddyfile)?;
            state.services.lock().await.set_edge_child(child);
            Ok(None)
        }
    }
}

/// The SINGLE monitor source of truth for resource numbers — both the Services
/// rows and the sidebar footer's app-total (which just sums these) read here,
/// so the two can never diverge. Each row is its FULL process tree (master +
/// workers: php-fpm/nginx keep most real memory in workers — a masters-only
/// read undercounted by ~100MB on a small stack). The root-privileged Caddy
/// edge has no child handle (pid unknown) and is invisible to sysinfo's
/// same-user read — discover its pid by our binary path in the cmdline
/// (marker-gated, never a foreign caddy) and fall back to the world-readable
/// `ps` accounting. CPU is per-core percent (Activity-Monitor style).
/// The embedded DNS resolver is deliberately NOT a row here: it's app-lifetime
/// (in-process task, never controlled by start/stop-all), so listing it would
/// make the footer's running/total + "Stop all" lie about what they control.
/// Its health is surfaced separately via `dns_status` (Services indicator +
/// Settings).
pub fn enriched_status(state: &AppState) -> Result<Vec<ServiceStatus>> {
    let infos = state.service_infos();
    // Default PHP minor, for the pool rows' Set-default control (brief DB lock,
    // released before the monitor lock).
    let default_minor: Option<String> = state
        .db
        .lock()
        .ok()
        .and_then(|conn| core::php::default_minor(&conn).ok())
        .flatten();
    let mut monitor = state
        .monitor
        .lock()
        .map_err(|_| Error::Other("monitor lock poisoned".into()))?;
    monitor.refresh_processes(); // one sweep per poll, then read each pid (M6)
    let sup = state.platform.supervisor();
    // OUR edge caddy's binary path — a cmdline marker only rexenv's edge has.
    let caddy_marker = state.platform.paths().bin_dir().ok().map(|b| {
        b.join(format!("caddy-{}", core::binaries::CADDY_VERSION))
            .join(core::binaries::exe_name("caddy", std::env::consts::OS))
            .display()
            .to_string()
    });

    let out: Vec<ServiceStatus> = infos
        .into_iter()
        .map(|i| {
            let pid = i.pid.or_else(|| {
                // Privileged edge: running but pid-less — marker-gated lookup.
                (i.running && i.name == "Caddy")
                    .then(|| {
                        caddy_marker
                            .as_deref()
                            .and_then(|m| sup.owned_pids(m).into_iter().next())
                    })
                    .flatten()
            });
            let tree = pid.and_then(|p| monitor.tree(p));
            let (cpu_percent, ram_mb) = match tree {
                // ram 0 for a live service ⇒ sysinfo couldn't actually read it
                // (cross-user) — use the ps fallback instead.
                Some(t) if t.ram_mb > 0 => (t.cpu_percent, t.ram_mb),
                _ => pid.and_then(|p| sup.resource_usage(p)).unwrap_or((0.0, 0)),
            };
            let kind = kind_of(&i.name);
            // The screen's name for a pool, when this OS runs something else (#651). Derived
            // from the SAME minor the key carries, so the two can never describe different pools.
            let label = i.name.strip_prefix("PHP-FPM ").and_then(|minor| {
                let shown = state.platform.supervisor().php_pool_model().display_name(minor);
                (shown != i.name).then_some(shown)
            });
            // "PHP-FPM 8.3" → version "8.3" (the shared pools);
            // "FrankenPHP my.rex" → the pinned FrankenPHP release + the domain
            // (it embeds its OWN PHP — not one of the pools).
            let version = i.name.strip_prefix("PHP-FPM ").map(str::to_string);
            let is_default = version
                .as_deref()
                .map(|minor| Some(minor) == default_minor.as_deref());
            let domain = i.name.strip_prefix("FrankenPHP ").map(str::to_string);
            let optional = i.optional;
            let service_key = core::db::DbEngine::ALL
                .into_iter()
                .find(|e| e.label() == i.name)
                .map(|e| e.key().to_string())
                .or_else(|| (i.name == "Mailpit").then(|| "mailpit".to_string()));
            let version = version.or_else(|| {
                domain
                    .is_some()
                    .then(|| core::binaries::FRANKENPHP_VERSION.to_string())
            });
            ServiceStatus {
                label,
                name: i.name,
                running: i.running,
                pid,
                port: i.port,
                cpu_percent,
                ram_mb,
                kind,
                version,
                is_default,
                domain,
                optional,
                service_key,
                busy_note: None,
            }
        })
        .collect();
    let mut out = out;
    note_busy_pools(state, &mut out);
    Ok(out)
}

/// The busy-workers note (plan §3 D1(b), ledger #608): sample the connections held on every running
/// shared pool's port, feed the tracker, set each busy row's note, and write a health-log line when a
/// pool turns busy or free. It samples only while a view polls — nothing runs when no window asks.
fn note_busy_pools(state: &AppState, rows: &mut [ServiceStatus]) {
    let platform = state.platform.as_ref();
    let sup = platform.supervisor();
    let workers = core::php::pool_workers(sup.php_pool_model());
    let Ok(mut tracker) = state.pool_busy.lock() else {
        return;
    };
    // Shared pools only: FrankenPHP rows carry a domain and are not pools.
    let running: Vec<String> = rows
        .iter()
        .filter(|r| r.running && r.name.starts_with("PHP-FPM ") && r.domain.is_none())
        .map(|r| r.name.clone())
        .collect();
    tracker.retain_running(&running);
    let mut events = Vec::new();
    for row in rows.iter_mut().filter(|r| running.contains(&r.name)) {
        match tracker.observe(&row.name, sup.established_on(row.port), workers) {
            core::pool_busy::Change::Busy { held } => {
                let recent = platform
                    .paths()
                    .log_dir()
                    .ok()
                    .map(|dir| {
                        let tail = core::php_cgi::read_tail(&dir.join("nginx-access.log"), 16 * 1024);
                        core::pool_busy::recent_hosts(&tail, 3)
                    })
                    .unwrap_or_default();
                events.push(core::service_manager::HealthEvent {
                    service: row.name.clone(),
                    action: "workers-busy",
                    detail: core::pool_busy::busy_detail(workers, held, &recent),
                });
            }
            core::pool_busy::Change::Free => events.push(core::service_manager::HealthEvent {
                service: row.name.clone(),
                action: "workers-free",
                detail: "a worker is free again".into(),
            }),
            core::pool_busy::Change::None => {}
        }
        if tracker.is_busy(&row.name) {
            row.busy_note = Some(core::pool_busy::note(workers));
        }
    }
    drop(tracker);
    if !events.is_empty() {
        core::service_manager::log_health_events(platform, &events);
    }
}

/// Per-service status + live RAM/CPU for the Services view.
#[tauri::command]
pub async fn services_status(state: State<'_, AppState>) -> Result<Vec<ServiceStatus>> {
    enriched_status(&state)
}

/// What `restart_web_service` did — the CLI prints this verbatim-ish.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebRestartReport {
    /// The service as status names it ("Nginx", "PHP-FPM 8.3", "Caddy").
    pub service: String,
    /// "restarted" | "reloaded" | "notRunning" | "refused".
    pub outcome: &'static str,
}

/// Restart ONE web-tier service: the shared nginx, a PHP minor's pool, or the
/// edge (which is RELOADED — see `core::service_manager::WebRestartOutcome`).
///
/// # Why there is no `stop`
///
/// The web tier has no useful stopped state. A stopped nginx is every default
/// site 502-ing with nothing on screen to explain it, and the honest way to stop
/// serving is to stop the stack (`stop_services`). Restart is the operation
/// people actually want, and it always runs on a freshly generated config —
/// resurrecting a service on the config it already had is the state a restart is
/// usually trying to escape.
/// Download `minor`'s pinned php-fpm BEFORE a pool restart takes the services
/// lock — the contract `restart_pools_for` states and the patch-update path
/// honours (`lib.rs`), and which both restart verbs skipped: `stop_one` runs
/// first and `ensure` then resolves the binary, which on a cold cache is a
/// download held under the lock — every site on that minor 502s for its
/// length while the Services screen (a `try_lock` snapshot) still shows the
/// pool running, and offline the pool is simply left stopped.
pub(crate) async fn prefetch_pool_binaries(state: &State<'_, AppState>, minor: &str) -> Result<()> {
    let patches = {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        core::php::effective_patches(&conn)?
    };
    let plan = core::downloads::plan_for_php_with(state.platform.as_ref(), minor, &patches);
    core::downloads::prefetch(state.platform.as_ref(), &format!("Restart PHP {minor}"), &plan).await
}

#[tauri::command]
pub async fn restart_web_service(
    state: State<'_, AppState>,
    target: String,
) -> Result<WebRestartReport> {
    let parsed = core::service_manager::WebTarget::parse(&target).ok_or_else(|| {
        Error::Other(format!(
            "unknown web service \"{target}\" — expected nginx, edge, or php-<minor> \
             (e.g. php-8.3)"
        ))
    })?;
    let sites = {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        core::sites::list(&conn)?
    };
    if let core::service_manager::WebTarget::Pool(minor) = &parsed {
        prefetch_pool_binaries(&state, minor).await?;
    }
    let (outcome, checks) = {
        let mut mgr = state.services.lock().await;
        mgr.restart_web_service(state.platform.as_ref(), &state.ca, &sites, &parsed).await?
    };
    core::service_manager::await_ready(checks).await?;
    Ok(WebRestartReport {
        service: parsed.label(),
        outcome: match outcome {
            core::service_manager::WebRestartOutcome::Restarted => "restarted",
            core::service_manager::WebRestartOutcome::Reloaded => "reloaded",
            core::service_manager::WebRestartOutcome::NotRunning => "notRunning",
            core::service_manager::WebRestartOutcome::Refused => "refused",
        },
    })
}

#[cfg(test)]
mod label_tests {
    /// Ledger #651 — **the row's screen name may differ from its key, and the KEY is what
    /// everything else reads.** `name` stays `PHP-FPM <minor>` on every OS because five things
    /// derive from it here and in core: the Services grouping (`kind_of`), `version`
    /// (`strip_prefix("PHP-FPM ")`), `is_default` from that version, `core::pool_busy`'s streaks
    /// and `restart_attempts`' counters. Renaming the key to match a Windows screen would move
    /// the row out of the PHP group and take the Set-default control with it.
    ///
    /// Read from this module's own production source, because building a row needs a platform:
    /// what is provable here is that the label is DERIVED from the pool model and that `name`
    /// is still passed through untouched.
    #[test]
    fn the_pool_row_shows_a_label_but_keys_off_its_name() {
        let src = crate::core::copy_scan::production_source(include_str!("services.rs"));
        assert!(src.contains("fn kind_of"), "the stripper ate the source");
        assert!(
            src.contains("php_pool_model().display_name(minor)"),
            "the row's label is not asked of the pool model"
        );
        assert!(
            src.contains("let kind = kind_of(&i.name);"),
            "the grouping no longer keys off the row's name"
        );
        assert!(
            src.contains("i.name.strip_prefix(\"PHP-FPM \")"),
            "the version is no longer parsed from the name key"
        );
        assert!(src.contains("name: i.name,"), "the row stopped carrying its key");
    }
}

#[cfg(test)]
mod tests {
    /// #175's order guard — and a statement of what it is NOT.
    ///
    /// **This proves TEXT ORDER, not behaviour.** It reads `auto_start_inner`'s
    /// source and asserts the download check comes before `start_core` and the
    /// edge decision before the edge could start. It cannot see whether a
    /// download or an escalation is ever ATTEMPTED — that is the wiring half of
    /// #175, which rides SMOKE-TEST (a reboot on a cold cache), because nothing
    /// outside `lib.rs` setup can construct the `AppState` the function takes.
    /// The dotfile guards went eight months string-proven without meeting a
    /// server; this guard is labelled to prevent the same misreading.
    ///
    /// Why it exists anyway (#308's reason): a tidy refactor that moves the
    /// uncached-cache check after `start_core` breaks "never download at login"
    /// SILENTLY — services come up, then the abort fires late or not at all —
    /// and a comment does not fail.
    #[test]
    fn auto_start_checks_the_cache_before_starting_and_decides_the_edge_before_running_it() {
        // Comment-stripped, and the stripping is LOAD-BEARING here: the guard-2
        // comment inside auto_start_inner names `login_edge_action` in prose, so
        // an unstripped scan could pass on the comment alone (the four-times
        // failure recorded in core::copy_scan).
        // BOTH strippers composed: production_source cuts the test module (this
        // one), strip_ts_comments drops `//` prose (same syntax as Rust). The
        // canary below caught this test's own first version using only the
        // first — the comment naming login_edge_action survived and the guard
        // was reading it.
        let src = crate::core::copy_scan::strip_ts_comments(
            &crate::core::copy_scan::production_source(include_str!("services.rs")),
        );
        let start = src
            .find("async fn auto_start_inner")
            .expect("auto_start_inner exists (renamed? update this guard AND ledger #175)");
        let body = &src[start..];
        let end = body[1..].find("\nasync fn ").map(|i| i + 1).unwrap_or(body.len());
        let body = &body[..end];

        let pos = |needle: &str| {
            body.find(needle).unwrap_or_else(|| {
                panic!(
                    "auto_start_inner no longer contains `{needle}` — a login-safety guard \
                     call is gone, or moved out of the function. TEXT-ORDER guard only: \
                     re-check the behaviour by hand (SMOKE-TEST: cold-cache login) and \
                     update ledger #175."
                )
            })
        };
        assert!(
            pos("uncached_names(") < pos("start_core("),
            "TEXT ORDER violated: the cold-cache check now sits AFTER start_core in \
             auto_start_inner's source. \"Never download at login\" aborts late or not at \
             all. This guard cannot see behaviour — if the reorder is intentional, the \
             SMOKE-TEST cold-cache login item is where the real answer lives."
        );
        assert!(
            pos("login_edge_action(") < pos("proxy::start("),
            "TEXT ORDER violated: the edge decision now sits AFTER the edge start in \
             auto_start_inner's source — a privileged plan could run (= an auth prompt \
             at login) before the skip decision is consulted."
        );
        // Canary both ways: the stripper stripped (the guard-2 prose names
        // login_edge_action and must be gone), and the file's test module is cut
        // (this very string would otherwise be found in itself).
        assert!(
            !body.contains("Guard 2 is the pure"),
            "production_source stripped nothing — the assertions above may be reading \
             the comments that explain them"
        );
        assert!(!body.contains("TEXT ORDER violated"), "test module not cut from the scan");
    }

    /// #61 — **the services lock is never held across an `.await`.**
    ///
    /// `AppState.services` is the one async Mutex in the app (everything else is
    /// field-level), and the rule CLAUDE.md states is: spawn under the lock,
    /// return `ReadyCheck`s, `await_ready` AFTER dropping it. The reason is what
    /// the user sees. A start holds the lock while a binary downloads or a port
    /// settles; a status poll then blocks on the same Mutex; the Services screen
    /// stops repainting and the app reads as hung during exactly the operation
    /// the user is watching. Nothing in the type system stops it — `tokio`'s
    /// Mutex is designed to be held across awaits — so the rule was a reading
    /// discipline, which is what this row has said since it was written.
    ///
    /// The check walks the SOURCE rather than a list of call sites: every file
    /// under `src/` that takes the lock is scanned at runtime, so a new command
    /// in a new file is covered without anyone remembering to add it here.
    #[test]
    fn the_services_lock_is_never_held_across_an_await() {
        use std::path::Path;

        /// The receiver chain an `.await` completes: everything before it back
        /// to the first character that is not part of `ident(...)?.ident(...)`.
        fn awaited_chain(before: &str) -> &str {
            let b = before.as_bytes();
            let mut i = b.len();
            loop {
                // `?` sits between a call and the next `.`: `mgr.x()?.y()`.
                while i > 0 && b[i - 1] == b'?' {
                    i -= 1;
                }
                // A call's argument list, balanced.
                if i > 0 && b[i - 1] == b')' {
                    let mut depth = 0i32;
                    let mut j = i;
                    while j > 0 {
                        j -= 1;
                        match b[j] {
                            b')' => depth += 1,
                            b'(' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    i = j;
                }
                // The identifier (or `?`) before it, then the `.` joining it on.
                let mut j = i;
                while j > 0 && (b[j - 1].is_ascii_alphanumeric() || b[j - 1] == b'_' || b[j - 1] == b'?' || b[j - 1] == b':') {
                    j -= 1;
                }
                i = j;
                if i > 0 && b[i - 1] == b'.' {
                    i -= 1;
                    continue;
                }
                break;
            }
            &before[i..]
        }

        // The walker itself, on the shapes that matter: the prescribed shape is
        // exempt, an await buried in the guard's argument list is not.
        assert_eq!(awaited_chain("let c = mgr.reload(platform, &ca, sites, false)"), "mgr.reload(platform, &ca, sites, false)");
        assert_eq!(awaited_chain("match mgr.restart_pools_for(p, std::slice::from_ref(m))"), "mgr.restart_pools_for(p, std::slice::from_ref(m))");
        assert_eq!(awaited_chain("mgr.record(download()"), "download()");
        assert_eq!(awaited_chain("Ok(mgr.x()?.y()"), "mgr.x()?.y()");
        assert_eq!(awaited_chain("let x = core::downloads::prefetch(a, b)"), "core::downloads::prefetch(a, b)");

        fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    rust_files(&p, out);
                } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
                    out.push(p);
                }
            }
        }
        let mut files = Vec::new();
        rust_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
        files.sort();

        let mut holders_seen = 0usize;
        for path in &files {
            let Ok(text) = std::fs::read_to_string(path) else { continue };
            if !text.contains("services.lock()") {
                continue;
            }
            let src = crate::core::copy_scan::production_source(&text);
            // (brace depth at the binding, line, variable) for each live guard.
            let mut holders: Vec<(i32, usize, String)> = Vec::new();
            let mut depth = 0i32;
            let mut stmt = String::new();
            let mut stmt_holds = false; // this statement already yielded a holder
            for (n, line) in src.lines().enumerate() {
                // A line comment is `//` at the start or after whitespace —
                // NOT the `//` inside `http://…`: splitting on the bare pair
                // truncated any line carrying a URL, so an await after it was
                // invisible.
                let code = if line.trim_start().starts_with("//") {
                    ""
                } else {
                    line.split(" //").next().unwrap_or("")
                };
                // Whitespace-stripped concatenation, so a call split across lines
                // (`match mgr` / `.restart_pools_for(…)` / `.await`) reads as the
                // one expression it is — with a space it does not, and the guard
                // then calls the prescribed shape a violation.
                stmt.push_str(code.trim());
                // The lock is taken on the line whose `.await` completes
                // `services.lock()` — which may be five lines below the
                // `.services` (the multi-line form the first version could not
                // see, so its holder was never pushed and every await under it
                // was unchecked).
                let takes_lock = !stmt_holds
                    && code.contains(".await")
                    && stmt.contains("services.lock().await");
                let awaits = code.matches(".await").count() - usize::from(takes_lock);
                if let Some((_, at, var)) = holders.last() {
                    // Awaiting the MANAGER'S OWN async methods under the lock is
                    // the prescribed shape, not a violation: `spawn_*` starts a
                    // process and returns a `ReadyCheck`, which is precisely the
                    // work that must happen while we hold it. What must never
                    // happen is awaiting anything ELSE — a download, a readiness
                    // wait, another service's command — because that is the wait
                    // a status poll then queues behind.
                    // Statement-scoped, not line-scoped: `mgr.start_core(…)` is
                    // written across several lines with `.await?` alone on the
                    // last one, and a line-only check calls that a violation.
                    // Per AWAIT, not per statement: for each `.await` on this
                    // line, walk back over the method chain it completes
                    // (balanced parens, `.ident`, `?`) and require that chain
                    // to START with the guard. A statement that merely mentions
                    // the guard — `mgr.record(download().await)` — used to be
                    // exempt as a whole; the chain under THAT await starts at
                    // `download()`, and is a violation.
                    let line_start = stmt.len() - code.trim().len();
                    let foreign_awaits = stmt
                        .match_indices(".await")
                        .filter(|(at, _)| *at >= line_start)
                        .filter(|(at, _)| !awaited_chain(&stmt[..*at]).starts_with(&format!("{var}.")))
                        .count();
                    let on_the_manager = foreign_awaits == 0;
                    assert_eq!(
                        awaits * usize::from(!on_the_manager),
                        0,
                        "{}:{} awaits while the services lock taken at line {at} (`{var}`) is \
                         still held:\n    {}\nSpawn under the lock, return `ReadyCheck`s, and \
                         `await_ready` after dropping it — a status poll blocking on this Mutex \
                         is the Services screen freezing during the very operation the user is \
                         watching",
                        path.file_name().unwrap_or_default().to_string_lossy(),
                        n + 1,
                        code.trim()
                    );
                }
                if takes_lock {
                    stmt_holds = true;
                    // The binding is in the STATEMENT, not necessarily on this
                    // line (`let pids = state` / `.services` / `.lock()` /
                    // `.await`). The LAST `let` in the statement is the guard's:
                    // `let checks = { let mut mgr = state.services.lock().await;`
                    // binds `mgr`, not `checks`.
                    let binding = stmt
                        .rsplit_once("let ")
                        .map(|(_, rest)| rest.trim_start_matches("mut "))
                        .and_then(|rest| rest.split([' ', '=', ':', ';']).next())
                        .map(str::to_string)
                        .filter(|name| {
                            !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_')
                        });
                    if let Some(var) = binding {
                        holders.push((depth, n + 1, var));
                        holders_seen += 1;
                    }
                }
                depth += code.matches('{').count() as i32 - code.matches('}').count() as i32;
                if code.contains(';') {
                    stmt.clear();
                    stmt_holds = false;
                }
                // The guard lives until its scope CLOSES (depth back below the
                // depth it was bound at) or it is explicitly dropped. Comparing
                // with `>` instead of `>=` retires every holder on the line that
                // binds it, which is a lint that inspects nothing — caught by the
                // plant that should have gone red and did not.
                holders.retain(|(d, _, var)| !code.contains(&format!("drop({var})")) && depth >= *d);
            }
        }
        // The scan must actually have found lock-holding scopes, or it is a
        // green light for a rule it never looked at.
        assert!(
            holders_seen >= 5,
            "the lint found only {holders_seen} bound services-lock guards across the tree — it \
             used to find more than five. Either the lock moved, or the scan stopped seeing \
             bodies, and a lint that inspects nothing passes for the wrong reason"
        );
    }
}

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
/// per-site env vars (locking the DB briefly, never across `.await`).
#[allow(clippy::type_complexity)] // one snapshot tuple, unpacked immediately
fn start_inputs(
    state: &State<'_, AppState>,
) -> Result<(
    Vec<crate::state::models::Site>,
    Vec<String>,
    PhpSettingsMap,
    PhpSettingsMap,
    std::collections::HashMap<crate::core::db::DbEngine, String>,
)> {
    let conn = state
        .db
        .lock()
        .map_err(|_| Error::Other("database lock poisoned".into()))?;
    let sites = core::sites::list(&conn)?;
    let minors = core::php::installed_minors(&conn)?;
    let php_settings = crate::state::store::all_php_settings(&conn)?;
    let site_env = crate::state::store::all_site_env(&conn)?;
    let db_versions = crate::core::db::DbEngine::ALL
        .into_iter()
        .filter(|e| e.available())
        .map(|e| (e, e.effective_version(&conn)))
        .collect::<std::collections::HashMap<_, _>>();
    Ok((sites, minors, php_settings, site_env, db_versions))
}

/// Start the shared stack (MySQL + a php-fpm pool per installed PHP version +
/// Nginx + Caddy). Downloads binaries on first run; gated on free ports.
#[tauri::command]
pub async fn start_services(state: State<'_, AppState>) -> Result<()> {
    let (sites, php_minors, php_settings, site_env, db_versions) = start_inputs(&state)?;
    // Phase 0 (UNLOCKED): plan the full binary set, then prefetch every missing
    // one through the download hub — real progress events for the UI, EVERY
    // failure surfaced (not just the first), and no download ever streams while
    // the services lock is held (status polls stay live on a cold first run).
    // After this, the resolves inside start_core are cache hits.
    let plan =
        core::downloads::plan_for_start(state.platform.as_ref(), &sites, &php_minors, &db_versions);
    core::downloads::prefetch(state.platform.as_ref(), "Start all", &plan).await?;
    // Phase 1 (locked): spawn everything except the edge; collect the readiness
    // probes + Caddyfile. Spawning is fast — no waiting happens under the lock.
    let (caddyfile, checks) = {
        let mut mgr = state.services.lock().await;
        // Per-version ini settings feed the pool configs written by start_core;
        // per-site env vars feed the nginx/FrankenPHP configs (§1.6).
        mgr.set_php_settings(php_settings);
        mgr.set_site_env(site_env);
        mgr.set_db_versions(db_versions);
        mgr.start_core(state.platform.as_ref(), &state.ca, &sites, &php_minors)
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
            core::proxy::start_edge_daemon(state.platform.as_ref(), &plan.caddy_bin, &plan.caddyfile)?;
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
    verify_edge_wire(&state).await
}

/// Positive wire-identity gate shared by Start-all and login auto-start: OUR edge
/// must be what answers loopback `:443` (marker-header probe), else error naming
/// the intercepting process. The watchdog keeps re-checking afterwards and flips
/// the status truthfully (`edge-blocked` / `edge-unblocked` events).
async fn verify_edge_wire(state: &State<'_, AppState>) -> Result<()> {
    if core::proxy::edge_answers_as_ours(
        core::adminer::ADMINER_HOST,
        core::proxy::DEFAULT_HTTPS_PORT,
    )
    .await
    {
        return Ok(());
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
        core::proxy::stop_edge_daemon(state.platform.as_ref())?;
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
    let (sites, php_minors, php_settings, site_env, db_versions) = start_inputs(state)?;
    // Guard 1: strictly offline. Every needed binary must already be cached
    // (the decision fn lives in core::downloads with its own test).
    let plan =
        core::downloads::plan_for_start(state.platform.as_ref(), &sites, &php_minors, &db_versions);
    let missing = core::downloads::uncached_names(&plan);
    if !missing.is_empty() {
        return Err(Error::Other(format!(
            "binaries not downloaded yet ({}) — open rexenv and press Start all once",
            missing.join(", ")
        )));
    }
    let (caddyfile, checks) = {
        let mut mgr = state.services.lock().await;
        mgr.set_php_settings(php_settings);
        mgr.set_site_env(site_env);
        mgr.set_db_versions(db_versions);
        mgr.start_core(state.platform.as_ref(), &state.ca, &sites, &php_minors).await?
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
        .and_then(|conn| core::php::list_versions(&conn).ok())
        .and_then(|v| v.into_iter().find(|v| v.is_default).map(|v| v.minor));
    let mut monitor = state
        .monitor
        .lock()
        .map_err(|_| Error::Other("monitor lock poisoned".into()))?;
    monitor.refresh_processes(); // one sweep per poll, then read each pid (M6)
    let sup = state.platform.supervisor();
    // OUR edge caddy's binary path — a cmdline marker only rexenv's edge has.
    let caddy_marker = state.platform.paths().bin_dir().ok().map(|b| {
        b.join(format!("caddy-{}", core::binaries::CADDY_VERSION))
            .join("caddy")
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
            }
        })
        .collect();
    Ok(out)
}

/// Per-service status + live RAM/CPU for the Services view.
#[tauri::command]
pub async fn services_status(state: State<'_, AppState>) -> Result<Vec<ServiceStatus>> {
    enriched_status(&state)
}

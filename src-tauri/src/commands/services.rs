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
}

/// Per-minor PHP ini settings, as loaded from the `php_settings` table.
type PhpSettingsMap = std::collections::HashMap<String, Vec<(String, String)>>;

/// Snapshot the site list + installed PHP minors + per-version ini settings +
/// per-site env vars (locking the DB briefly, never across `.await`).
fn start_inputs(
    state: &State<'_, AppState>,
) -> Result<(Vec<crate::state::models::Site>, Vec<String>, PhpSettingsMap, PhpSettingsMap)> {
    let conn = state
        .db
        .lock()
        .map_err(|_| Error::Other("database lock poisoned".into()))?;
    let sites = core::sites::list(&conn)?;
    let minors = core::php::installed_minors(&conn)?;
    let php_settings = crate::state::store::all_php_settings(&conn)?;
    let site_env = crate::state::store::all_site_env(&conn)?;
    Ok((sites, minors, php_settings, site_env))
}

/// Start the shared stack (MySQL + a php-fpm pool per installed PHP version +
/// Nginx + Caddy). Downloads binaries on first run; gated on free ports.
#[tauri::command]
pub async fn start_services(state: State<'_, AppState>) -> Result<()> {
    let (sites, php_minors, php_settings, site_env) = start_inputs(&state)?;
    // Phase 0 (UNLOCKED): plan the full binary set, then prefetch every missing
    // one through the download hub — real progress events for the UI, EVERY
    // failure surfaced (not just the first), and no download ever streams while
    // the services lock is held (status polls stay live on a cold first run).
    // After this, the resolves inside start_core are cache hits.
    let plan = core::downloads::plan_for_start(state.platform.as_ref(), &sites, &php_minors);
    core::downloads::prefetch(state.platform.as_ref(), "Start all", &plan).await?;
    // Phase 1 (locked): spawn everything except the edge; collect the readiness
    // probes + Caddyfile. Spawning is fast — no waiting happens under the lock.
    let (caddyfile, checks) = {
        let mut mgr = state.services.lock().await;
        // Per-version ini settings feed the pool configs written by start_core;
        // per-site env vars feed the nginx/FrankenPHP configs (§1.6).
        mgr.set_php_settings(php_settings);
        mgr.set_site_env(site_env);
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
            core::proxy::start_privileged(state.platform.as_ref(), &plan.caddy_bin, &plan.caddyfile)?;
            state.services.lock().await.set_edge_privileged();
        } else {
            let child = core::proxy::start(state.platform.as_ref(), &plan.caddy_bin, &plan.caddyfile)?;
            state.services.lock().await.set_edge_child(child);
        }
    }
    Ok(())
}

/// Stop the shared stack.
#[tauri::command]
pub async fn stop_services(state: State<'_, AppState>) -> Result<()> {
    let mut mgr = state.services.lock().await;
    mgr.stop_all(state.platform.as_ref())
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
            ServiceStatus { name: i.name, running: i.running, pid, port: i.port, cpu_percent, ram_mb }
        })
        .collect();
    Ok(out)
}

/// Per-service status + live RAM/CPU for the Services view.
#[tauri::command]
pub async fn services_status(state: State<'_, AppState>) -> Result<Vec<ServiceStatus>> {
    enriched_status(&state)
}

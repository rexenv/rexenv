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

/// Snapshot the site list + installed PHP minors (locking the DB briefly, never
/// across `.await`).
fn start_inputs(
    state: &State<'_, AppState>,
) -> Result<(Vec<crate::state::models::Site>, Vec<String>)> {
    let conn = state
        .db
        .lock()
        .map_err(|_| Error::Other("database lock poisoned".into()))?;
    let sites = core::sites::list(&conn)?;
    let minors = core::php::installed_minors(&conn)?;
    Ok((sites, minors))
}

/// Start the shared stack (MySQL + a php-fpm pool per installed PHP version +
/// Nginx + Caddy). Downloads binaries on first run; gated on free ports.
#[tauri::command]
pub async fn start_services(state: State<'_, AppState>) -> Result<()> {
    let (sites, php_minors) = start_inputs(&state)?;
    // Phase 1 (locked): spawn everything except the edge; collect the readiness
    // probes + Caddyfile. Spawning is fast — no waiting happens under the lock.
    let (caddyfile, checks) = {
        let mut mgr = state.services.lock().await;
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

/// Per-service status + live RAM/CPU for the Services view.
#[tauri::command]
pub async fn services_status(
    state: State<'_, AppState>,
    dns: State<'_, crate::state::app::DnsState>,
) -> Result<Vec<ServiceStatus>> {
    // Single source of truth (shared with `global_status`); non-blocking.
    let infos = state.service_infos();
    let mut monitor = state
        .monitor
        .lock()
        .map_err(|_| Error::Other("monitor lock poisoned".into()))?;
    monitor.refresh_processes(); // one sweep per poll, then read each pid (M6)
    let mut out: Vec<ServiceStatus> = infos
        .into_iter()
        .map(|i| {
            let m = i.pid.and_then(|p| monitor.process(p));
            ServiceStatus {
                name: i.name,
                running: i.running,
                pid: i.pid,
                port: i.port,
                cpu_percent: m.map(|m| m.cpu_percent).unwrap_or(0.0),
                ram_mb: m.map(|m| m.ram_mb).unwrap_or(0),
            }
        })
        .collect();
    // The embedded DNS resolver — in-process (no pid/metrics of its own), but a
    // dead resolver makes EVERY `.test` site unreachable, so it must be visible
    // here, not only in Settings.
    out.push(ServiceStatus {
        name: "DNS".to_string(),
        running: dns.running(),
        pid: None,
        port: core::dns::DEFAULT_DNS_PORT,
        cpu_percent: 0.0,
        ram_mb: 0,
    });
    Ok(out)
}

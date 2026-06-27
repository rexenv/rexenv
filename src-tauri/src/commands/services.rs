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

/// Snapshot the current site list (locking the DB briefly, never across `.await`).
fn site_list(state: &State<'_, AppState>) -> Result<Vec<crate::state::models::Site>> {
    let conn = state
        .db
        .lock()
        .map_err(|_| Error::Other("database lock poisoned".into()))?;
    core::sites::list(&conn)
}

/// Start the shared stack (MySQL + php-fpm + Nginx + Caddy). Downloads binaries
/// on first run; gated on free ports.
#[tauri::command]
pub async fn start_services(state: State<'_, AppState>) -> Result<()> {
    let sites = site_list(&state)?;
    let mut mgr = state.services.lock().await;
    mgr.start_all(state.platform.as_ref(), &state.ca, &sites).await
}

/// Stop the shared stack.
#[tauri::command]
pub async fn stop_services(state: State<'_, AppState>) -> Result<()> {
    let mut mgr = state.services.lock().await;
    mgr.stop_all(state.platform.as_ref())
}

/// Per-service status + live RAM/CPU for the Services view.
#[tauri::command]
pub async fn services_status(state: State<'_, AppState>) -> Result<Vec<ServiceStatus>> {
    let infos = {
        let mgr = state.services.lock().await;
        mgr.status()
    };
    let mut monitor = state
        .monitor
        .lock()
        .map_err(|_| Error::Other("monitor lock poisoned".into()))?;
    Ok(infos
        .into_iter()
        .map(|i| {
            let m = i.pid.and_then(|p| monitor.process(p));
            ServiceStatus {
                name: i.name.to_string(),
                running: i.running,
                pid: i.pid,
                port: i.port,
                cpu_percent: m.map(|m| m.cpu_percent).unwrap_or(0.0),
                ram_mb: m.map(|m| m.ram_mb).unwrap_or(0),
            }
        })
        .collect())
}

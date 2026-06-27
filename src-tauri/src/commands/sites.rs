//! commands::sites — thin Tauri IPC handlers for sites. Call `core/` only.

use crate::core;
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::models::{ServiceStatus, Site};
use tauri::State;

fn lock<'a>(
    state: &'a State<'a, AppState>,
) -> Result<std::sync::MutexGuard<'a, rusqlite::Connection>> {
    state
        .db
        .lock()
        .map_err(|_| Error::Other("database lock poisoned".into()))
}

/// List all sites (newest first).
#[tauri::command]
pub fn list_sites(state: State<'_, AppState>) -> Result<Vec<Site>> {
    let conn = lock(&state)?;
    core::sites::list(&conn)
}

/// Mark a site running; returns the updated site.
#[tauri::command]
pub fn start_site(state: State<'_, AppState>, id: String) -> Result<Option<Site>> {
    let conn = lock(&state)?;
    core::sites::set_status(&conn, &id, ServiceStatus::Running)
}

/// Mark a site stopped; returns the updated site.
#[tauri::command]
pub fn stop_site(state: State<'_, AppState>, id: String) -> Result<Option<Site>> {
    let conn = lock(&state)?;
    core::sites::set_status(&conn, &id, ServiceStatus::Stopped)
}

/// Delete a site: remove its DB row, cert, and docroot, then reload the running
/// stack so it stops being served. Returns whether it existed.
#[tauri::command]
pub async fn delete_site(state: State<'_, AppState>, id: String) -> Result<bool> {
    let (removed, sites) = {
        let conn = lock(&state)?;
        let removed = core::sites::teardown(&conn, state.platform.as_ref(), &id)?;
        (removed, core::sites::list(&conn)?)
    };
    // Best-effort reload (no-op if services aren't running).
    let mgr = state.services.lock().await;
    let _ = mgr.reload(state.platform.as_ref(), &state.ca, &sites);
    Ok(removed)
}

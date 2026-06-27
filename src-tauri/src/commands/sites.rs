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

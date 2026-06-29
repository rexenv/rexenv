//! commands::blueprints — thin IPC for site-blueprint CRUD (Phase 3 §11.3). The
//! apply-on-create step lives in `commands::sites::create_site`; this file is just
//! the list/save/delete of the reusable presets.

use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::models::Blueprint;
use crate::state::store;
use tauri::State;

fn lock<'a>(
    state: &'a State<'a, AppState>,
) -> Result<std::sync::MutexGuard<'a, rusqlite::Connection>> {
    state
        .db
        .lock()
        .map_err(|_| Error::Other("database lock poisoned".into()))
}

/// All blueprints (newest first).
#[tauri::command]
pub fn list_blueprints(state: State<'_, AppState>) -> Result<Vec<Blueprint>> {
    let conn = lock(&state)?;
    store::list_blueprints(&conn)
}

/// Insert or update a blueprint (upsert by id).
#[tauri::command]
pub fn save_blueprint(state: State<'_, AppState>, blueprint: Blueprint) -> Result<()> {
    let conn = lock(&state)?;
    store::upsert_blueprint(&conn, &blueprint)
}

/// Delete a blueprint by id; returns whether it existed.
#[tauri::command]
pub fn delete_blueprint(state: State<'_, AppState>, id: String) -> Result<bool> {
    let conn = lock(&state)?;
    store::delete_blueprint(&conn, &id)
}

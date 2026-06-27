//! commands::settings — IPC for app settings (key/value) + derived values.

use crate::core;
use crate::error::{Error, Result};
use crate::state::app::AppState;
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

/// Read a setting value by key (or `null`).
#[tauri::command]
pub fn get_setting(state: State<'_, AppState>, key: String) -> Result<Option<String>> {
    let conn = lock(&state)?;
    store::get_setting(&conn, &key)
}

/// Insert or update a setting.
#[tauri::command]
pub fn set_setting(state: State<'_, AppState>, key: String, value: String) -> Result<()> {
    let conn = lock(&state)?;
    store::set_setting(&conn, &key, &value)
}

/// The resolved sites folder (the `sites_dir` setting, else the app-data default).
#[tauri::command]
pub fn sites_folder(state: State<'_, AppState>) -> Result<String> {
    let conn = lock(&state)?;
    Ok(core::sites::sites_dir(&conn, state.platform.as_ref())?
        .display()
        .to_string())
}

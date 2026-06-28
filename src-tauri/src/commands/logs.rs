//! commands::logs — thin Tauri IPC for the Logs viewer (§3.1). Call `core/` only.

use crate::core;
use crate::core::logs::LogTarget;
use crate::error::{Error, Result};
use crate::state::app::AppState;
use tauri::State;

/// The log sources selectable for a site (shared edge/nginx, the site's php-fpm
/// pool, DB error log, and its FrankenPHP backend when overridden).
#[tauri::command]
pub fn log_targets(state: State<'_, AppState>, site_id: String) -> Result<Vec<LogTarget>> {
    let conn = state
        .db
        .lock()
        .map_err(|_| Error::Other("database lock poisoned".into()))?;
    let site = core::sites::get(&conn, &site_id)?
        .ok_or_else(|| Error::Other(format!("no site {site_id}")))?;
    Ok(core::logs::targets_for_site(&site))
}

/// The last `lines` lines of the log identified by `key` (a file name within the
/// log dir). Polled by the Logs tab to follow a source in near-real-time.
#[tauri::command]
pub fn tail_log(state: State<'_, AppState>, key: String, lines: usize) -> Result<Vec<String>> {
    core::logs::tail(state.platform.as_ref(), &key, lines)
}

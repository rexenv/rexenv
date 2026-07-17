//! commands::logs — thin Tauri IPC for the Logs viewer (§3.1). Call `core/` only.

use crate::core;
use crate::core::logs::{LogTarget, WpDebugLogStatus};
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::models::Site;
use std::path::PathBuf;
use tauri::State;

/// Look a site up by id (shared by every per-site log command).
fn get_site(state: &State<'_, AppState>, site_id: &str) -> Result<Site> {
    let conn = state
        .db
        .lock()
        .map_err(|_| Error::Other("database lock poisoned".into()))?;
    core::sites::get(&conn, site_id)?.ok_or_else(|| Error::Other(format!("no site {site_id}")))
}

/// The log sources selectable for a site (shared edge/nginx, the site's php-fpm
/// pool, DB error log, and its FrankenPHP backend when overridden).
#[tauri::command]
pub fn log_targets(state: State<'_, AppState>, site_id: String) -> Result<Vec<LogTarget>> {
    Ok(core::logs::targets_for_site(&get_site(&state, &site_id)?, &state.platform.paths().log_dir()?))
}

/// The last `lines` lines of the log identified by `key` (a file name within the
/// log dir). Polled by the Logs tab to follow a source in near-real-time.
#[tauri::command]
pub fn tail_log(state: State<'_, AppState>, key: String, lines: usize) -> Result<Vec<String>> {
    core::logs::tail(state.platform.as_ref(), &key, lines)
}

/// WordPress debug-log status for a site (WP_DEBUG / WP_DEBUG_LOG, resolved
/// path, file presence + size). Cheap static wp-config.php read — pollable.
#[tauri::command]
pub fn wp_debug_log_status(state: State<'_, AppState>, site_id: String) -> Result<WpDebugLogStatus> {
    let site = get_site(&state, &site_id)?;
    Ok(core::logs::wp_debug_log_status(&PathBuf::from(site.path)))
}

/// The last `lines` lines of the site's WordPress debug.log (missing ⇒ empty).
#[tauri::command]
pub fn wp_debug_log_tail(state: State<'_, AppState>, site_id: String, lines: usize) -> Result<Vec<String>> {
    let site = get_site(&state, &site_id)?;
    core::logs::wp_debug_log_tail(&PathBuf::from(site.path), lines)
}

/// Truncate the site's WordPress debug.log to empty.
#[tauri::command]
pub fn wp_debug_log_clear(state: State<'_, AppState>, site_id: String) -> Result<()> {
    let site = get_site(&state, &site_id)?;
    core::logs::wp_debug_log_clear(&PathBuf::from(site.path))
}

/// Copy the site's debug.log to the Downloads folder; returns the saved path.
#[tauri::command]
pub fn wp_debug_log_download(state: State<'_, AppState>, site_id: String) -> Result<String> {
    let site = get_site(&state, &site_id)?;
    core::logs::wp_debug_log_download(&PathBuf::from(&site.path), &site.domain)
        .map(|p| p.to_string_lossy().into_owned())
}

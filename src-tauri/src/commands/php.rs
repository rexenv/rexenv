//! commands::php — IPC for the PHP version registry (Phase 2 §1.5). Thin: all
//! logic (guards, registry reads/writes) lives in `core::php`.

use crate::core;
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::models::PhpVersion;
use tauri::State;

fn lock<'a>(
    state: &'a State<'a, AppState>,
) -> Result<std::sync::MutexGuard<'a, rusqlite::Connection>> {
    state
        .db
        .lock()
        .map_err(|_| Error::Other("database lock poisoned".into()))
}

/// All registered PHP versions (installed + available) for the Settings UI.
#[tauri::command]
pub fn list_php_versions(state: State<'_, AppState>) -> Result<Vec<PhpVersion>> {
    let conn = lock(&state)?;
    core::php::list_versions(&conn)
}

/// Enable (install) or disable (remove) a PHP version. Guarded in `core::php`
/// (can't remove the default or a version a site is using). Installing
/// prefetches the version's FPM + CLI builds right away (hub batch with live
/// progress) instead of silently deferring the download to the next
/// `start_services`; the pool itself still reconciles on the next start.
#[tauri::command]
pub async fn set_php_version_installed(
    state: State<'_, AppState>,
    minor: String,
    installed: bool,
) -> Result<()> {
    {
        // Registry update under a brief DB lock, dropped before any await.
        let conn = lock(&state)?;
        core::php::set_installed(&conn, &minor, installed)?;
    }
    if installed {
        let plan = core::downloads::plan_for_php(state.platform.as_ref(), &minor);
        core::downloads::prefetch(
            state.platform.as_ref(),
            &format!("Install PHP {minor}"),
            &plan,
        )
        .await?;
    }
    Ok(())
}

/// Make a PHP version the default for new sites (§4.4). Guarded in `core::php`
/// (must be installed). The New Site dialog reads this default.
#[tauri::command]
pub fn set_default_php_version(state: State<'_, AppState>, minor: String) -> Result<()> {
    let conn = lock(&state)?;
    core::php::set_default(&conn, &minor)
}

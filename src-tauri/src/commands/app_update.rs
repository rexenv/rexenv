//! commands::app_update — IPC for the app's own update check. Thin: every rule
//! lives in `core::app_update`, and this file only translates calls.
//!
//! There is no `app_update_apply` here yet — the swap and the relaunch are T3/T4
//! of `docs/PLAN-self-update.md`. When it lands it stays a GUI-only command:
//! self-update replaces the process that enforces the agent dial and
//! `settings_access`, and the relaunch kills the caller's socket mid-call, so an
//! agent could never observe the result of the thing it asked for.

use crate::core;
use crate::error::{Error, Result};
use crate::state::app::AppState;
use tauri::State;

fn lock<'a>(
    state: &'a State<'a, AppState>,
) -> Result<std::sync::MutexGuard<'a, rusqlite::Connection>> {
    state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))
}

/// What the About card renders. Pure reads — no network, so opening Settings
/// never waits on GitHub.
#[tauri::command]
pub fn app_update_state(state: State<'_, AppState>) -> Result<core::app_update::AppUpdateState> {
    let conn = lock(&state)?;
    Ok(core::app_update::state(&conn))
}

/// Check now: fetch the signed descriptor, accept it under one brief lock, and
/// answer with the state that follows.
///
/// **Fetch UNLOCKED, persist under the lock.** `core::app_update::fetch` takes no
/// `Connection` precisely so this cannot be written the other way round — the
/// house rule about never holding the database lock across a wait, made
/// structural.
///
/// The check timestamp is written ONLY on the success path, so a failed check
/// can never age into "checked just now" over yesterday's answer.
#[tauri::command]
pub async fn app_update_check(
    state: State<'_, AppState>,
) -> Result<core::app_update::AppUpdateState> {
    let (doc, sig) = core::app_update::fetch().await?;
    let conn = lock(&state)?;
    // A serial we already have is not an error — it is the ordinary answer on
    // every check after the first, and `accept` says so by writing nothing.
    core::app_update::accept(&conn, &doc, &sig)?;
    let st = core::app_update::state(&conn);
    let check = core::app_update::store_check(&conn, st.offered.clone())?;
    Ok(core::app_update::AppUpdateState { checked_at: Some(check.checked_at), ..st })
}

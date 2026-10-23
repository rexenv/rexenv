//! commands::update_hub — IPC for the ONE update inventory (`core::update_hub`,
//! `docs/PLAN-unified-updates.md` T1). Thin: this file gathers the facts the four
//! families already compute for their own screens and hands them to the hub; the
//! judging, the order and every sentence live in `core`.
//!
//! Nothing here installs. Each row still installs through its family's own command.

use crate::core;
use crate::error::{Error, Result};
use crate::state::app::AppState;
use tauri::State;

fn lock<'a>(
    state: &'a State<'a, AppState>,
) -> Result<std::sync::MutexGuard<'a, rusqlite::Connection>> {
    state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))
}

fn document() -> String {
    core::app_update::this_document(crate::platform::current().app_bundle().descriptor_variant().as_deref())
}

/// Everything newer than what runs here, in one list (the app last).
#[tauri::command]
pub fn updates_inventory(state: State<'_, AppState>) -> Result<core::update_hub::UpdateInventory> {
    // The running pools are the services lock's to say; read them BEFORE the db lock, the
    // way `list_php_versions` does, and never block on either.
    let running = state
        .services
        .try_lock()
        .ok()
        .and_then(|mgr| mgr.running_php_patches(state.platform.as_ref()))
        .unwrap_or_default();
    let platform = state.platform.as_ref();
    // The Homebrew fact decides the app's consent sentence; a bundle whose facts cannot be
    // read is treated as a plain install, which is the sentence that asks for nothing extra.
    let homebrew = std::env::current_exe()
        .ok()
        .and_then(|exe| platform.app_bundle().facts(&exe).ok())
        .map(|f| f.homebrew)
        .unwrap_or(false);
    let conn = lock(&state)?;
    let catalog = core::updates::cached(&conn);
    let arch = core::updates::catalog_arch(platform.binaries().arch());
    let app = core::app_update::state(&conn, &document());
    let php = core::php::list_versions(&conn, &running, &catalog, arch)?;
    let adminer_have = core::adminer::effective_version(platform, &conn);
    let adminer_new = catalog.newer_than(core::updates::Family::Adminer, &adminer_have, arch);
    let pma_have = core::phpmyadmin::effective_version(platform, &conn);
    let pma_new = catalog.newer_than(core::updates::Family::PhpMyAdmin, &pma_have, arch);
    Ok(core::update_hub::inventory(
        &conn,
        &core::update_hub::Facts {
            app: &app,
            homebrew,
            php: &php,
            adminer: (&adminer_have, adminer_new.as_deref()),
            phpmyadmin: (&pma_have, pma_new.as_deref()),
        },
    ))
}

/// Set one kind's offered version aside (`version: Some`) or take the skip back (`None`),
/// and answer with the inventory that follows.
#[tauri::command]
pub fn updates_skip(
    state: State<'_, AppState>,
    kind: core::update_hub::UpdateKind,
    version: Option<String>,
) -> Result<core::update_hub::UpdateInventory> {
    {
        let conn = lock(&state)?;
        core::update_hub::set_skipped(&conn, &kind, version.as_deref())?;
    }
    updates_inventory(state)
}

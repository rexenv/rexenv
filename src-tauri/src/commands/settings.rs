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

/// Insert or update a setting. Keys with a validating setter are routed to it,
/// so the generic KV command can't smuggle a value past the backend — a blocked
/// TLD, or a sites folder that would end a quoted path in a generated config.
///
/// **The routing is a REGISTRY lookup, not a per-key `if`.** It used to be two
/// hardcoded branches, and the guard that watched them named the same two keys,
/// so a third gated key would have been settable straight past its rule with
/// nothing failing. `core::settings_access::GATED_SETTERS` declares the gating
/// beside the setters; adding a row there gates the key everywhere at once —
/// here, in `cli_access`, and in the guard. Do not re-introduce a branch: the
/// guard fails the build on one, because a branch is a second place to remember.
#[tauri::command]
pub fn set_setting(state: State<'_, AppState>, key: String, value: String) -> Result<()> {
    let conn = lock(&state)?;
    if let Some(setter) = core::settings_access::gated_setter(&key) {
        setter(&conn, &value)?;
        return Ok(());
    }
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

/// The default TLD new sites are created under (the `default_tld` setting if
/// it holds an allowed TLD, else `test`).
#[tauri::command]
pub fn default_tld(state: State<'_, AppState>) -> Result<String> {
    let conn = lock(&state)?;
    core::sites::default_tld(&conn)
}

/// Set the default TLD for new sites. Backend policy gate: a blocked TLD
/// (.local, .dev, 2-letter, popular gTLDs) is refused with the reason, even
/// via direct invoke. Returns the stored (normalized) value.
#[tauri::command]
pub fn set_default_tld(state: State<'_, AppState>, tld: String) -> Result<String> {
    let conn = lock(&state)?;
    core::sites::set_default_tld(&conn, &tld)
}

/// Classify a TLD for the UI: `{ allowed, warn, reason }` — blocked TLDs carry
/// the refusal reason; allowed-but-unsafe ones set `warn` ("may shadow a real
/// internet TLD"). Display metadata only — enforcement stays in the backend
/// validators either way.
#[tauri::command]
pub fn tld_policy(tld: String) -> core::tld::TldPolicy {
    core::tld::classify(tld.trim().trim_start_matches('.'))
}

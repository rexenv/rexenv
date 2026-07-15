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

/// One editable ini setting for the Settings UI: the stored value (None = unset)
/// plus PHP's compiled default (what actually applies when unset — our static
/// builds load no php.ini).
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhpSettingView {
    pub key: &'static str,
    pub value: Option<String>,
    pub default: &'static str,
}

/// A submitted ini setting (key must be in `core::php::SETTINGS`).
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhpSettingInput {
    pub key: String,
    pub value: String,
}

/// The whitelisted ini settings for one PHP minor: every editable key, with the
/// stored value if any. Keys the user never set report `value: None` and apply
/// the shown default.
#[tauri::command]
pub fn get_php_settings(state: State<'_, AppState>, minor: String) -> Result<Vec<PhpSettingView>> {
    let conn = lock(&state)?;
    let stored = crate::state::store::get_php_settings(&conn, &minor)?;
    Ok(core::php::SETTINGS
        .iter()
        .map(|s| PhpSettingView {
            key: s.key,
            value: stored.iter().find(|(k, _)| k == s.key).map(|(_, v)| v.clone()),
            default: s.default,
        })
        .collect())
}

/// Replace the ini settings for one PHP minor (submitted keys are stored; omitted
/// keys revert to PHP defaults), then make them live: validate → `php-fpm -t` a
/// candidate config → persist → restart that minor's pool → reload nginx (per-site
/// `client_max_body_size` tracks upload/post sizes). Ordered so a bad value can
/// never brick a pool: typed validation rejects it first, and the `-t` gate runs
/// against a candidate file the live pool never reads. FrankenPHP-override sites
/// are unaffected (own embedded PHP).
#[tauri::command]
pub async fn apply_php_settings(
    state: State<'_, AppState>,
    minor: String,
    settings: Vec<PhpSettingInput>,
) -> Result<()> {
    let pairs: Vec<(String, String)> =
        settings.into_iter().map(|s| (s.key, s.value)).collect();
    let pairs = core::php::validate_settings(&pairs)?;
    let patch = core::php::patch_for_minor(&minor)
        .ok_or_else(|| Error::Other(format!("unknown PHP version: {minor}")))?;
    let port = core::php::fpm_port(&minor)
        .ok_or_else(|| Error::Other(format!("no fpm port for {minor}")))?;
    let platform = state.platform.as_ref();

    // `php-fpm -t` gate on a CANDIDATE config (the live file is never touched) —
    // only when the binary is already cached: a settings edit must not trigger a
    // PHP download for an uninstalled version (typed validation already ran, and
    // no pool is running for it anyway).
    if core::binaries::is_cached(platform, "php-fpm", patch) {
        let bin = core::binaries::resolve(platform, "php-fpm", patch).await?; // cache hit
        let candidate =
            core::services::write_fpm_config_candidate(platform, &minor, port, &pairs)?;
        let test = core::services::test_fpm_config(platform, &bin, &candidate);
        let _ = std::fs::remove_file(&candidate);
        test?;
    }

    // Persist + snapshot under ONE brief DB lock, dropped before any await.
    let (sites, all) = {
        let conn = lock(&state)?;
        crate::state::store::replace_php_settings(&conn, &minor, &pairs)?;
        (core::sites::list(&conn)?, crate::state::store::all_php_settings(&conn)?)
    };

    // Swap the map in + restart the affected pools (normal + debug when both
    // live) + reload nginx, then await readiness OUTSIDE the services lock
    // (locking rule).
    let checks = {
        let mut mgr = state.services.lock().await;
        mgr.apply_php_settings(platform, &state.ca, &sites, all, &minor)
            .await?
    };
    if !checks.is_empty() {
        core::service_manager::await_ready(checks).await?;
    }
    Ok(())
}

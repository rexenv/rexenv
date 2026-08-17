//! commands::php — IPC for the PHP version registry (Phase 2 §1.5). Thin: all
//! logic (guards, registry reads/writes) lives in `core::php`.

use crate::core;
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::models::PhpVersionView;
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
///
/// `try_lock` on the services, deliberately: this is a UI read, and a long
/// start/stop holding that lock must not block the Settings screen — the same
/// rule the status polls follow. A missed lock costs the `serving` line (the row
/// then says only what is pinned), never the list.
#[tauri::command]
pub fn list_php_versions(state: State<'_, AppState>) -> Result<Vec<PhpVersionView>> {
    let running = state
        .services
        .try_lock()
        .ok()
        .and_then(|mgr| mgr.running_php_patches(state.platform.as_ref()))
        .unwrap_or_default();
    let conn = lock(&state)?;
    let catalog = core::updates::cached(&conn);
    core::php::list_versions(&conn, &running, &catalog)
}

/// The PHP minor FrankenPHP actually serves — its embedded build, never the
/// site's pool (ruled 15 Aug 2026: the SiteDetail picker goes read-only with
/// this annotation for FrankenPHP sites, instead of a `php_version` promise
/// the server cannot keep). Served from the ONE pin (`FRANKENPHP_EMBEDDED_PHP`)
/// so the UI can never carry a second copy that drifts.
#[tauri::command]
pub fn frankenphp_embedded_php() -> String {
    core::php::minor_of(core::binaries::FRANKENPHP_EMBEDDED_PHP)
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
        let patches = {
            let conn = lock(&state)?;
            core::php::effective_patches(&conn).unwrap_or_default()
        };
        let plan = core::downloads::plan_for_php_with(state.platform.as_ref(), &minor, &patches);
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
    // The patch the pool WILL run after the restart below — not the pin. Gating a
    // candidate config with the pin's binary while the pool restarts onto the
    // selection tests a config against an interpreter nobody runs, and an ini key
    // the new patch rejects would then brick the pool the gate exists to protect.
    let patch = {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        core::php::patch_to_run(&conn, &minor)?
    };
    let patch = patch.as_str();
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

/// Refresh the signed update manifest and report what each installed minor could
/// move to.
///
/// Best-effort by contract: a failure returns an error the row renders as
/// "couldn't check", never a blocked screen. Verification, the serial rule and
/// the structural limits all live in `core::updates`; this command only moves
/// bytes and hands the result to the resolve path.
#[tauri::command]
pub async fn php_update_check(state: State<'_, AppState>) -> Result<Vec<PhpVersionView>> {
    // Fetch UNLOCKED, verify + persist under one brief lock. `fetch` takes no
    // Connection precisely so this cannot be written the other way round.
    let (doc, sig) = core::updates::fetch().await?;
    {
        let conn = lock(&state)?;
        let cat = core::updates::accept(&conn, &doc, &sig)?;
        core::binaries::install_catalog(cat);
    }
    list_php_versions(state)
}

/// Move `minor` onto `patch`, or fail leaving it exactly where it was.
///
/// # The shape, and why each step is where it is
///
/// 1. **Refuse anything the catalog does not vouch for.** The patch must resolve
///    through `binaries::manifest`, which consults the compiled-in pins first and
///    only then the VERIFIED catalog — so a patch nobody signed cannot be named
///    here even by a caller that bypasses the UI.
/// 2. **Download BEFORE persisting.** The prefetch runs with no lock held and no
///    row written, so a failed or cancelled download leaves the minor untouched.
/// 3. **Persist, then restart.** Only after the bytes are cached does the choice
///    become the registry's.
/// 4. **Revert on a pool that does not come back.** The selection returns to what
///    it was and the pool is restarted onto it, so a bad patch costs a restart
///    rather than a broken stack. **The downloaded tree is NOT deleted** — it is
///    valid, verified, and deleting it makes the retry re-fetch ~100MB.
#[tauri::command]
pub async fn php_update_apply(
    state: State<'_, AppState>,
    minor: String,
    patch: String,
) -> Result<()> {
    let platform = state.platform.as_ref();
    if core::php::minor_of(&patch) != minor {
        return Err(Error::Other(format!("{patch} is not a patch of PHP {minor}")));
    }
    // (1) Only a version something vouches for. `manifest` returning a spec IS
    // the check: it means either a compiled-in pin or a signed catalog entry.
    if core::binaries::manifest(
        "php-fpm",
        &patch,
        std::env::consts::OS,
        platform.binaries().arch(),
    )
    .is_none()
    {
        return Err(Error::Other(format!(
            "PHP {patch} is not in this build's pins or in a verified update manifest"
        )));
    }

    // (2) Download first, with nothing written and no lock held.
    let plan = core::downloads::plan_for_php_patch(platform, &minor, &patch);
    core::downloads::prefetch(platform, &format!("Update PHP {minor}"), &plan).await?;

    // (3) Persist the choice, then hand the pool manager the new snapshot.
    let previous = {
        let conn = lock(&state)?;
        let prev = crate::state::store::list_php_versions(&conn)?
            .into_iter()
            .find(|v| v.minor == minor)
            .and_then(|v| v.selected_patch);
        crate::state::store::set_php_selected_patch(&conn, &minor, Some(&patch))?;
        prev
    };
    let checks = {
        let conn_patches = {
            let conn = lock(&state)?;
            core::php::effective_patches(&conn)?
        };
        let mut mgr = state.services.lock().await;
        mgr.set_php_patches(conn_patches);
        mgr.restart_pools_for(platform, std::slice::from_ref(&minor)).await
    };

    // (4) Revert if the restart failed to spawn OR failed to come back ready.
    let outcome = match checks {
        Err(e) => Err(e),
        Ok(c) => core::service_manager::await_ready(c).await,
    };
    if let Err(e) = outcome {
        let restore = {
            let conn = lock(&state)?;
            crate::state::store::set_php_selected_patch(&conn, &minor, previous.as_deref())?;
            core::php::effective_patches(&conn)?
        };
        let back = {
            let mut mgr = state.services.lock().await;
            mgr.set_php_patches(restore);
            mgr.restart_pools_for(platform, std::slice::from_ref(&minor)).await
        };
        if let Ok(c) = back {
            let _ = core::service_manager::await_ready(c).await;
        }
        return Err(Error::Other(format!(
            "PHP {minor} did not come back on {patch}, so it was put back on the previous \
             build: {e}"
        )));
    }
    Ok(())
}

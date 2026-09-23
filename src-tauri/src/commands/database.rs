//! commands::database — IPC for the database engines (Phase 2 §5.5). Thin: the
//! lifecycle lives in `ServiceManager` + `core::db`.

use crate::core::db::DbEngine;
use crate::error::{Error, Result};
use crate::state::app::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

/// One database engine's status + live metrics (mirrors the frontend `DbStatus`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbStatus {
    pub key: String,
    pub label: String,
    pub port: u16,
    pub version: String,
    pub running: bool,
    pub pid: Option<u32>,
    pub cpu_percent: f32,
    pub ram_mb: u64,
}

/// Per-engine status + live RAM/CPU for the Databases view (available engines).
#[tauri::command]
pub async fn databases_status(state: State<'_, AppState>) -> Result<Vec<DbStatus>> {
    // Non-blocking: serve the last snapshot if a long start/stop holds the lock.
    let infos = match state.services.try_lock() {
        Ok(mgr) => {
            let infos = mgr.db_status();
            if let Ok(mut cache) = state.db_status_cache.lock() {
                *cache = infos.clone();
            }
            infos
        }
        Err(_) => state
            .db_status_cache
            .lock()
            .map(|c| c.clone())
            .unwrap_or_default(),
    };
    let mut monitor = state
        .monitor
        .lock()
        .map_err(|_| Error::Other("monitor lock poisoned".into()))?;
    monitor.refresh_processes(); // one sweep per poll, then read each pid (M6)
    Ok(infos
        .into_iter()
        .map(|i| {
            // Full process tree: postgres runs a worker family under its
            // postmaster (same lesson as php-fpm/nginx workers).
            let m = i.pid.and_then(|p| monitor.tree(p));
            DbStatus {
                key: i.engine.key().to_string(),
                label: i.engine.label().to_string(),
                port: i.engine.port(),
                version: i.version.clone(),
                running: i.running,
                pid: i.pid,
                cpu_percent: m.map(|m| m.cpu_percent).unwrap_or(0.0),
                ram_mb: m.map(|m| m.ram_mb).unwrap_or(0),
            }
        })
        .collect())
}

/// The engine's selected version from settings (default pin when unset) —
/// what every command that resolves engine binaries must use.
pub(crate) fn effective_db_version(state: &AppState, engine: DbEngine) -> Result<String> {
    let conn = state
        .db
        .lock()
        .map_err(|_| Error::Other("database lock poisoned".into()))?;
    Ok(engine.effective_version(&conn))
}

fn engine_from_key(key: &str) -> Result<DbEngine> {
    let engine =
        DbEngine::from_key(key).ok_or_else(|| Error::Other(format!("unknown DB engine: {key}")))?;
    if !engine.available() {
        return Err(Error::Other(format!(
            "{} is not available on this platform yet",
            engine.label()
        )));
    }
    Ok(engine)
}

/// Start a database engine (downloads its binary on first run; gated on a free port).
#[tauri::command]
pub async fn start_database(state: State<'_, AppState>, key: String) -> Result<()> {
    let engine = engine_from_key(&key)?;
    let version = effective_db_version(&state, engine)?;
    // Prefetch the engine's binary tree BEFORE taking the services lock — the
    // (potentially 600MB) first-run download streams with hub progress while
    // status polls stay live; the spawn below then hits cache.
    let plan = crate::core::downloads::plan_for_engine(state.platform.as_ref(), engine, &version);
    crate::core::downloads::prefetch(
        state.platform.as_ref(),
        &format!("Start {}", engine.label()),
        &plan,
    )
    .await?;
    // Spawn under the lock, await readiness with it released (M4) — a slow DB
    // start doesn't block other service commands or the manager.
    let check = {
        let mut mgr = state.services.lock().await;
        mgr.set_db_version(engine, &version);
        mgr.spawn_db(state.platform.as_ref(), engine).await?
    };
    crate::core::service_manager::await_ready(check.into_iter().collect()).await
}

/// The offered versions per engine (default first) for the Databases picker.
#[tauri::command]
pub fn db_engine_versions() -> Result<std::collections::HashMap<String, Vec<String>>> {
    Ok(DbEngine::ALL
        .into_iter()
        .filter(|e| e.available())
        .map(|e| {
            (
                e.key().to_string(),
                e.versions().iter().map(|v| v.to_string()).collect(),
            )
        })
        .collect())
}

/// The engines this host's macOS cannot run, with the sentence each shows —
/// `key → reason`. The Databases page lists these as disabled rows and New Site
/// as a disabled option, so an engine present on another Mac is never simply
/// missing here (§6.3 of the macOS-13 plan). Empty on every standard host.
#[tauri::command]
pub fn db_engine_refusals() -> Result<std::collections::HashMap<String, String>> {
    Ok(DbEngine::ALL
        .into_iter()
        .filter_map(|e| e.unavailable_reason().map(|r| (e.key().to_string(), r)))
        .collect())
}

/// Switch an engine to another offered version. Each version SERIES keeps its
/// own datadir (never an in-place upgrade/downgrade — PG major datadirs are
/// incompatible, MySQL/MariaDB downgrades unsupported), so databases created
/// on one version are not visible on another; the UI's confirm states this. A
/// RUNNING engine is stopped and restarted on the new version (prefetched
/// before the lock); a stopped one just records the choice.
#[tauri::command]
pub async fn set_db_engine_version(
    state: State<'_, AppState>,
    key: String,
    version: String,
) -> Result<()> {
    let engine = engine_from_key(&key)?;
    // Validate + persist FIRST (set_version enforces the offered set in core).
    {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        engine.set_version(&conn, &version)?;
    }
    // Prefetch the target version before any locked scope.
    let plan = crate::core::downloads::plan_for_engine(state.platform.as_ref(), engine, &version);
    crate::core::downloads::prefetch(
        state.platform.as_ref(),
        &format!("Switch {} to {version}", engine.label()),
        &plan,
    )
    .await?;
    let check = {
        let mut mgr = state.services.lock().await;
        let was_running = mgr.db_status().iter().any(|d| d.engine == engine && d.running);
        mgr.set_db_version(engine, &version);
        if was_running {
            mgr.stop_db(state.platform.as_ref(), engine)?;
            mgr.spawn_db(state.platform.as_ref(), engine).await?
        } else {
            None
        }
    };
    crate::core::service_manager::await_ready(check.into_iter().collect()).await
}

/// Stop a running database engine.
#[tauri::command]
pub async fn stop_database(state: State<'_, AppState>, key: String) -> Result<()> {
    let engine = engine_from_key(&key)?;
    let mut mgr = state.services.lock().await;
    mgr.stop_db(state.platform.as_ref(), engine)
}

// ── Adminer: the version row, its check, and its apply ─────────────────────────

/// What the Databases screen shows for Adminer (mirrors the frontend
/// `AdminerStatus`).
///
/// There is deliberately **no `upstream` field**. PHP carries two facts because
/// static-php.dev lags php.net, so "8.4.24 exists" and "8.4.23 is installable"
/// are genuinely different. rexenv downloads Adminer's OWN release asset, so a
/// version that exists and one rexenv can install are the same thing — a second
/// field would be one fact rendered twice, and the copy explaining the gap would
/// be a straight falsehood.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminerStatus {
    /// What the docroot is actually serving right now, if it has been staged.
    /// `None` before the first start — a state, not a zero value.
    pub staged: Option<String>,
    /// What it WILL serve: the choice, floored by the pin, vouched by the
    /// catalog (`adminer::effective_version`).
    pub effective: String,
    /// A newer version a VERIFIED manifest offers, or `None`.
    pub updatable: Option<String>,
}

/// Point the console at the app's palette. `dark` or `light` — the frontend
/// resolves "system" first, so the console matches what rexenv is RENDERING
/// rather than re-asking the OS and disagreeing with it (which is exactly the
/// bug: the app in light, the console in dark).
///
/// Written before the frame loads, and re-written whenever the theme changes;
/// the wrapper reads it per request, so the console's own links keep the
/// scheme too.
#[tauri::command]
pub fn adminer_set_theme(state: State<'_, AppState>, theme: String) -> Result<()> {
    crate::core::adminer::set_theme(state.platform.as_ref(), &theme)
}

/// Read the Adminer row. Cheap: a settings read, a marker read, a catalog look.
#[tauri::command]
pub fn adminer_status(state: State<'_, AppState>) -> Result<AdminerStatus> {
    let conn = state
        .db
        .lock()
        .map_err(|_| Error::Other("database lock poisoned".into()))?;
    Ok(adminer_row(state.platform.as_ref(), &conn))
}

/// The row, built from one consistent read. Shared by all three commands so the
/// apply cannot report a different shape from the poll that follows it.
fn adminer_row(
    platform: &dyn crate::platform::traits::Platform,
    conn: &rusqlite::Connection,
) -> AdminerStatus {
    let effective = crate::core::adminer::effective_version(platform, conn);
    AdminerStatus {
        staged: crate::core::adminer::staged_version(platform),
        // The machine's arch, even though the family maps it to `ANY_ARCH`:
        // passing a placeholder here would work today and be a lie the day a
        // family's `row_arch` stops ignoring it.
        updatable: crate::core::updates::cached(conn).newer_than(
            crate::core::updates::Family::Adminer,
            &effective,
            crate::core::updates::catalog_arch(platform.binaries().arch()),
        ),
        effective,
    }
}

/// Refresh the signed manifest, then report the row.
///
/// Best-effort by contract, exactly as `php_update_check` is: a failure returns
/// an error the row renders as "couldn't check", never a blocked screen.
#[tauri::command]
pub async fn adminer_update_check(state: State<'_, AppState>) -> Result<AdminerStatus> {
    // Fetch UNLOCKED, verify + persist under one brief lock — `fetch` takes no
    // `Connection` precisely so this cannot be written the other way round.
    let (doc, sig) = crate::core::updates::fetch().await?;
    {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        let cat = crate::core::updates::accept(&conn, &doc, &sig)?;
        crate::core::binaries::install_catalog(cat);
    }
    adminer_status(state)
}

/// Move Adminer onto `version`, or fail leaving it exactly where it was.
///
/// # The order, and why each step is where it is
///
/// 1. **Refuse anything the catalog does not vouch for** — the version must
///    resolve through `binaries::manifest`, which consults the compiled-in pin
///    first and only then the VERIFIED catalog, so a version nobody signed
///    cannot be named here even by a caller that bypasses the UI.
/// 2. **Download BEFORE persisting**, with no lock held and no row written, so a
///    failed or cancelled download leaves the console untouched.
/// 3. **PROBE the downloaded file** (`adminer::verify_pair`) against the bundled
///    PHP, in a throwaway directory. This is the step PHP has no equivalent of:
///    rexenv's login gate and frame protections for this console live inside
///    Adminer's own plugin API, so a build that no longer binds turns them off
///    silently. A refusal here has changed nothing — the selection is not
///    written and the docroot is not touched.
/// 4. **Persist, then restage.** Only after the bytes are cached AND proven to
///    bind does the choice become the registry's.
///
/// **No revert leg, and that is a ruling rather than an omission.**
/// `php_update_apply` earns one because a pool can fail to come back. Here the
/// only reachable failure after a successful download is the probe (which by
/// construction has touched nothing) or a filesystem error during restage —
/// where re-staging is as likely to fail as anything else. The floor plus the
/// kept cache tree make "set the selection back" always resolvable offline, so
/// the recovery is a second press, not a control with no measurement behind it.
#[tauri::command]
pub async fn adminer_update_apply(
    state: State<'_, AppState>,
    version: String,
) -> Result<AdminerStatus> {
    // One apply at a time — the same claim set PHP uses, because the property is
    // "one apply per thing" and two sets would be two answers to it.
    let _flight = crate::commands::php::InFlight::claim("adminer")?;
    let platform = state.platform.as_ref();

    // (1) Only a version something vouches for.
    if crate::core::binaries::manifest(
        "adminer",
        &version,
        std::env::consts::OS,
        platform.binaries().arch(),
    )
    .is_none()
    {
        return Err(Error::Other(format!(
            "Adminer {version} is not this build's pin and is not in a verified update manifest"
        )));
    }

    // (2) Download first, nothing written, no lock held.
    let plan = crate::core::downloads::plan_for_adminer(platform, &version);
    crate::core::downloads::prefetch(platform, &format!("Update Adminer {version}"), &plan).await?;
    let file = crate::core::binaries::resolve_file(platform, "adminer", &version).await?;

    // (3) Does it still bind? Refusing here has changed nothing.
    let php = crate::core::binaries::resolve_program(
        platform,
        "php",
        crate::core::binaries::pins().php,
    )
    .await?;
    crate::core::adminer::verify_pair(&php, &file)?;

    // (4) Persist, then restage the docroot onto it.
    {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        crate::core::adminer::set_selected_version(&conn, Some(&version))?;
    }
    crate::core::adminer::ensure(platform, &version).await?;
    adminer_status(state)
}


#[cfg(test)]
mod tests {
    /// **One apply at a time covers Adminer too, and does not block PHP.**
    ///
    /// The claim set is keyed and shared, because the property is "one apply per
    /// thing" — two sets would be two answers to it, and the second one is the
    /// one nobody remembers to check.
    #[test]
    fn an_adminer_apply_is_serialised_and_is_not_a_php_minor() {
        use crate::commands::php::InFlight;
        let a = InFlight::claim("adminer").expect("the first claim");
        assert!(InFlight::claim("adminer").is_err(), "a second Adminer apply must be refused");
        // …and it is its own key: updating Adminer must not block PHP 8.3.
        let php = InFlight::claim("8.3").expect("a PHP minor is unrelated work");
        drop(php);
        drop(a);
        assert!(InFlight::claim("adminer").is_ok(), "the Adminer claim did not come back");
    }
}

//! commands::database — IPC for the database engines (Phase 2 §5.5). Thin: the
//! lifecycle lives in `ServiceManager` + `core::db`.

use crate::core::db::DbEngine;
use crate::error::{Error, Result};
use crate::state::app::AppState;
use serde::Serialize;
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
pub(crate) fn effective_db_version(state: &State<'_, AppState>, engine: DbEngine) -> Result<String> {
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

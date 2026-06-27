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
    let infos = {
        let mgr = state.services.lock().await;
        mgr.db_status()
    };
    let mut monitor = state
        .monitor
        .lock()
        .map_err(|_| Error::Other("monitor lock poisoned".into()))?;
    Ok(infos
        .into_iter()
        .map(|i| {
            let m = i.pid.and_then(|p| monitor.process(p));
            DbStatus {
                key: i.engine.key().to_string(),
                label: i.engine.label().to_string(),
                port: i.engine.port(),
                version: i.engine.version().to_string(),
                running: i.running,
                pid: i.pid,
                cpu_percent: m.map(|m| m.cpu_percent).unwrap_or(0.0),
                ram_mb: m.map(|m| m.ram_mb).unwrap_or(0),
            }
        })
        .collect())
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
    let mut mgr = state.services.lock().await;
    mgr.ensure_db(state.platform.as_ref(), engine).await
}

/// Stop a running database engine.
#[tauri::command]
pub async fn stop_database(state: State<'_, AppState>, key: String) -> Result<()> {
    let engine = engine_from_key(&key)?;
    let mut mgr = state.services.lock().await;
    mgr.stop_db(state.platform.as_ref(), engine)
}

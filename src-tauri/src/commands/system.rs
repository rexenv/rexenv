//! commands::system — app-level IPC. App info + the global resource/status block.

use crate::core;
use crate::error::{Error, Result};
use crate::state::app::AppState;
use serde::Serialize;
use tauri::State;

/// Mirrors the frontend `AppInfo` type in `src/types/index.ts`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub tauri_version: String,
}

/// Round-trip smoke test for the typed IPC bridge (Phase 1 task 0.5).
#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        name: "rexenv".into(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        tauri_version: tauri::VERSION.to_string(),
    }
}

/// The sidebar status footer's global block. Mirrors the frontend `GlobalStatus`
/// type. CPU/RAM are real system totals (`sysinfo`); running/total are sites.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalStatus {
    /// "all" | "partial" | "stopped".
    pub summary: &'static str,
    pub running: u32,
    pub total: u32,
    pub cpu_percent: f32,
    pub ram_mb: u64,
    pub ram_total_mb: u64,
}

/// Live global status for the sidebar footer (task 7.4): real system CPU/RAM via
/// `sysinfo` + a running/total derived from the sites in the DB.
#[tauri::command]
pub fn global_status(state: State<'_, AppState>) -> Result<GlobalStatus> {
    let metrics = {
        let mut monitor = state
            .monitor
            .lock()
            .map_err(|_| Error::Other("monitor lock poisoned".into()))?;
        monitor.sample()
    };

    let conn = state
        .db
        .lock()
        .map_err(|_| Error::Other("database lock poisoned".into()))?;
    let sites = core::sites::list(&conn)?;
    let total = sites.len() as u32;
    let running = sites
        .iter()
        .filter(|s| matches!(s.status, crate::state::models::ServiceStatus::Running))
        .count() as u32;
    let summary = if total == 0 || running == 0 {
        "stopped"
    } else if running == total {
        "all"
    } else {
        "partial"
    };

    Ok(GlobalStatus {
        summary,
        running,
        total,
        cpu_percent: metrics.cpu_percent,
        ram_mb: metrics.ram_used_mb,
        ram_total_mb: metrics.ram_total_mb,
    })
}

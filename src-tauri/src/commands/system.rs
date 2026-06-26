//! commands::system — app-level IPC. Hosts the IPC smoke test (`app_info`).

use serde::Serialize;

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

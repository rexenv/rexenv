//! commands::downloads — IPC for the download manager (thin).

use crate::core::downloads::{self, Snapshot};
use crate::error::Result;
use crate::state::app::AppState;
use tauri::State;

/// Current download-manager state. The UI seeds from this on mount, then stays
/// live via the `download-progress` event (same snapshot shape).
#[tauri::command]
pub fn downloads_state() -> Snapshot {
    downloads::hub().snapshot()
}

/// Retry ONE failed download (the per-item retry button in the download
/// panel). Idempotent: a binary that meanwhile resolved returns instantly.
#[tauri::command]
pub async fn retry_download(
    state: State<'_, AppState>,
    name: String,
    version: String,
) -> Result<()> {
    downloads::resolve_any(state.platform.as_ref(), &name, &version).await
}

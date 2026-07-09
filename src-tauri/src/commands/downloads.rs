//! commands::downloads — IPC for the download manager (thin).

use crate::core::downloads::{self, PlannedBinary, Snapshot};
use crate::error::{Error, Result};
use crate::state::app::AppState;
use serde::Serialize;
use tauri::State;

/// One planned core binary for the onboarding Install step — rows render from
/// this immediately (cached ones as ✓) while live progress arrives separately
/// via `download-progress` events, keyed by `id`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub label: String,
    pub cached: bool,
}

impl From<&PlannedBinary> for PlannedInfo {
    fn from(p: &PlannedBinary) -> Self {
        PlannedInfo {
            id: downloads::item_id(&p.name, &p.version),
            label: downloads::label_for(&p.name, &p.version),
            name: p.name.clone(),
            version: p.version.clone(),
            cached: p.cached,
        }
    }
}

/// The Start-all binary plan (brief DB lock for sites + installed PHP minors,
/// never held across an await).
fn core_plan(state: &State<'_, AppState>) -> Result<Vec<PlannedBinary>> {
    let (sites, minors) = {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        (
            crate::core::sites::list(&conn)?,
            crate::core::php::installed_minors(&conn)?,
        )
    };
    Ok(downloads::plan_for_start(state.platform.as_ref(), &sites, &minors))
}

/// The core binary set (the Start-all plan) with cached flags — the onboarding
/// Install step's row source. Fast: filesystem marker checks only.
#[tauri::command]
pub fn core_binaries_plan(state: State<'_, AppState>) -> Result<Vec<PlannedInfo>> {
    Ok(core_plan(&state)?.iter().map(PlannedInfo::from).collect())
}

/// Kick off the core-set prefetch (onboarding auto-download). Resolves when the
/// batch finishes; the UI fires it without awaiting and follows progress via
/// events, so leaving onboarding never cancels the downloads.
#[tauri::command]
pub async fn prefetch_core_binaries(state: State<'_, AppState>) -> Result<()> {
    let plan = core_plan(&state)?;
    downloads::prefetch(state.platform.as_ref(), "First-run setup", &plan).await
}

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

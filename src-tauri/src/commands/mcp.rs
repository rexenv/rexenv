//! commands::mcp — IPC for Settings → "AI agents (MCP)".
//!
//! Three surfaces, all thin over `mcp_server`:
//! - the opt-in **toggle**, which really binds/unbinds the endpoint's socket
//!   (`mcp_set_enabled`) — a control over the socket, not a label on an
//!   always-on one; enabling binds, disabling drops live sessions and unlinks;
//! - the derived **status** the card's header reads (`mcp_status`);
//! - the **activity feed** read + clear (`agent_activity`, `agent_activity_clear`).
//!
//! Unix-only, mirroring the unix-socket `mcp_server` module.

use crate::error::{Error, Result};
use crate::mcp_server::{self, feed, MCP_ENABLED_KEY};
use crate::state::app::AppState;
use crate::state::store;
use serde::Serialize;
use tauri::{AppHandle, State};

/// The window the status line reads: recent enough to mean "now", and
/// self-recovering — an error state ages out on its own as its rows leave it.
const WINDOW_MINS: i64 = 15;

/// Feed rows the card lists.
const CARD_LIMIT: usize = 50;

/// The copy-paste connect line the card shows (`rex mcp` is the dumb pipe).
const CONNECT_COMMAND: &str = "claude mcp add rexenv -- rex mcp";

fn db(state: &AppState) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>> {
    state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))
}

/// The card's whole state in ONE read, so the header status and the feed rows it
/// shows come from the same snapshot and can never disagree (the plan's
/// "connected vs working" honesty: never green while the feed shows errors).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatus {
    /// Whether the endpoint is actually serving (socket bound) — the toggle state.
    pub enabled: bool,
    /// The copy-paste connect command for the card.
    pub connect_command: &'static str,
    /// The derived status the header line renders.
    pub activity: ActivityStatus,
    /// Recent feed rows, newest first, the card lists.
    pub recent: Vec<feed::AgentAction>,
}

/// The header line's state — derived from recent call OUTCOMES, never the socket
/// handshake alone: an `initialize` that then errors every call is exactly the
/// connected-but-broken split rexenv refuses to paint green elsewhere (a share
/// showing Live while 530-ing). So this speaks to what calls DID, not liveness.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum ActivityStatus {
    /// The endpoint is off.
    Off,
    /// On, but no agent call within the window.
    Idle,
    /// The most recent call in the window succeeded.
    Working { last_tool: String, minutes_ago: i64 },
    /// The most recent call(s) in the window errored — named with an N, not green.
    Erroring { errored: i64, minutes_ago: i64 },
}

fn status_snapshot(state: &AppState, limit: usize) -> Result<McpStatus> {
    let enabled = state
        .mcp
        .lock()
        .map_err(|_| Error::Other("mcp control lock poisoned".into()))?
        .is_running();
    let conn = db(state)?;
    let recent = feed::recent(&conn, limit)?;
    let activity = if !enabled {
        ActivityStatus::Off
    } else {
        match feed::recent_head(&conn, WINDOW_MINS)? {
            None => ActivityStatus::Idle,
            Some(h) if h.last_ok => {
                ActivityStatus::Working { last_tool: h.last_tool, minutes_ago: h.minutes_ago }
            }
            Some(h) => {
                ActivityStatus::Erroring { errored: h.trailing_errors, minutes_ago: h.minutes_ago }
            }
        }
    };
    Ok(McpStatus { enabled, connect_command: CONNECT_COMMAND, activity, recent })
}

/// The card's single read: toggle state + derived status + recent feed.
#[tauri::command]
pub fn mcp_status(state: State<'_, AppState>) -> Result<McpStatus> {
    status_snapshot(&state, CARD_LIMIT)
}

/// Flip the opt-in toggle. Enabling BINDS the socket, and persists "true" only
/// after the bind succeeds — the toggle never reads on while nothing listens.
/// Disabling drops every live session, unlinks the socket, and persists "false".
#[tauri::command]
pub fn mcp_set_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    enable: bool,
) -> Result<McpStatus> {
    {
        let mut ctl = state
            .mcp
            .lock()
            .map_err(|_| Error::Other("mcp control lock poisoned".into()))?;
        if enable {
            if !ctl.is_running() {
                // Bind first; a failure here surfaces to the caller and the
                // setting is NOT flipped on, so on/bound never diverge.
                let tx = mcp_server::start(app.clone())?;
                ctl.store_handle(tx);
            }
        } else {
            ctl.stop();
        }
    }
    {
        let conn = db(&state)?;
        store::set_setting(&conn, MCP_ENABLED_KEY, if enable { "true" } else { "false" })?;
    }
    status_snapshot(&state, CARD_LIMIT)
}

/// The activity feed — all recent rows, or just those naming one site (the
/// per-site SiteDetail section). Newest first; `limit` capped defensively.
#[tauri::command]
pub fn agent_activity(
    state: State<'_, AppState>,
    site_id: Option<String>,
    limit: usize,
) -> Result<Vec<feed::AgentAction>> {
    let conn = db(&state)?;
    let limit = limit.min(500);
    match site_id {
        Some(id) => feed::recent_for_site(&conn, &id, limit),
        None => feed::recent(&conn, limit),
    }
}

/// Clear the feed — the user's own record of their machine, theirs to wipe.
/// Returns the number of rows removed.
#[tauri::command]
pub fn agent_activity_clear(state: State<'_, AppState>) -> Result<usize> {
    let conn = db(&state)?;
    feed::clear(&conn)
}

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
use crate::mcp_server::{self, feed, MCP_ENABLED_KEY, MCP_MAIL_ENABLED_KEY};
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

/// Fill each row's `target_label` with the named site's CURRENT domain. The feed
/// stores the stable site id (`arguments.site_id`, a UUID); a human reading the
/// card needs the domain. A deleted site resolves to `None` and the UI falls back
/// to the raw id. rexenv-derived here (our own sites table), never agent content,
/// so the feed's typed-shape discipline is untouched — this is a READ-time view
/// join, not a stored field.
fn resolve_target_labels(conn: &rusqlite::Connection, rows: &mut [feed::AgentAction]) -> Result<()> {
    if rows.iter().all(|r| r.target_site.is_none()) {
        return Ok(());
    }
    let mut by_id = std::collections::HashMap::new();
    let mut stmt = conn.prepare("SELECT id, domain FROM sites")?;
    let mapped = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    for row in mapped {
        let (id, domain) = row?;
        by_id.insert(id, domain);
    }
    for r in rows.iter_mut() {
        if let Some(id) = &r.target_site {
            r.target_label = by_id.get(id).cloned();
        }
    }
    Ok(())
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
    /// The MAIL sub-toggle (M2b) — off by default, and independent of
    /// `enabled`: turning the endpoint on does not turn mail on.
    pub mail_enabled: bool,
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
    let mut recent = feed::recent(&conn, limit)?;
    resolve_target_labels(&conn, &mut recent)?;
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
    let mail_enabled = mcp_server::mail_enabled(&conn);
    Ok(McpStatus { enabled, connect_command: CONNECT_COMMAND, activity, recent, mail_enabled })
}

/// The card's single read: toggle state + derived status + recent feed.
#[tauri::command]
pub fn mcp_status(state: State<'_, AppState>) -> Result<McpStatus> {
    status_snapshot(&state, CARD_LIMIT)
}

/// Flip the opt-in toggle. Enabling BINDS the socket, and persists "true" only
/// after the bind succeeds — the toggle never reads on while nothing listens.
/// Disabling drops every live session, unlinks the socket, and persists "false".
/// Turn the MAIL sub-toggle on or off — **and put the scratch sites in step with
/// it**, which is the part that is not a flag flip.
///
/// The stamp that makes an agent's own mail findable (`core::wp_mailtag`) is a
/// file inside each scratch site, so "mail is on" has to mean "every scratch
/// site carries the stamp". Where that write happens was the real decision:
///
/// - **Not lazily, at the first `mail_list`.** The stamp must exist BEFORE the
///   mail is sent. Installing it when the agent READS is after the site already
///   sent, so the canonical loop — trigger a password reset, then read it —
///   would still miss on the first attempt, silently, looking exactly like a
///   site that overrode `From`. That turns a permanent confusion into a one-shot
///   one rather than fixing it.
/// - **Not at every launch.** That writes a `From`-forcing mu-plugin into a
///   user's sites even when the feature is off — a behaviour change nobody
///   asked for.
/// - **Here, at the toggle**, because this is the consent moment. The write is
///   tied to the decision that authorises it, the stamp is in place before any
///   agent connects, and disabling removes it. That yields one statable
///   invariant — *the stamp exists on every scratch site exactly while this is
///   on* — which **eliminates "this site predates the feature" as a category**
///   instead of leaving `mail_list` to report it.
///
/// Best-effort per site, and deliberately so: a site whose docroot is gone or
/// never provisioned is SKIPPED with a log, not an error that blocks the
/// toggle. Whether any individual site is really stamped is answered at READ
/// time by a stat (`wp_mailtag::is_installed`), which is live — a count
/// returned from here would be a snapshot that goes stale the moment a site is
/// created or deleted.
#[tauri::command]
pub fn mcp_set_mail_enabled(state: State<'_, AppState>, enable: bool) -> Result<McpStatus> {
    {
        let conn = db(&state)?;
        store::set_setting(&conn, MCP_MAIL_ENABLED_KEY, if enable { "true" } else { "false" })?;
        let sites = crate::core::sites::list(&conn)?;
        let (mut stamped, mut skipped) = (0usize, 0usize);
        for site in sites.iter().filter(|s| s.is_scratch()) {
            let docroot = std::path::Path::new(&site.path);
            if !docroot.is_dir() {
                // Not provisioned, or its folder is gone. Nothing to stamp, and
                // nothing wrong — `mail_list` reports this state per site.
                skipped += 1;
                continue;
            }
            let outcome = if enable {
                crate::core::wp_mailtag::enable(docroot, site.content_dir_rel(), &site.domain)
                    .map(|created_dir| {
                        // v25: record ownership of a dir WE made, so teardown
                        // removes it — never inferred later from emptiness.
                        if created_dir {
                            let _ = store::set_site_mu_dir_created(&conn, &site.id);
                        }
                    })
            } else {
                crate::core::wp_mailtag::disable(docroot)
            };
            match outcome {
                Ok(()) => stamped += 1,
                Err(e) => {
                    skipped += 1;
                    log::warn!("mcp: mail stamp for {} could not be updated: {e}", site.domain);
                }
            }
        }
        log::info!(
            "mcp: mail {} — {stamped} scratch site(s) updated, {skipped} skipped",
            if enable { "enabled" } else { "disabled" }
        );
    }
    status_snapshot(&state, CARD_LIMIT)
}

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
    let mut rows = match site_id {
        Some(id) => feed::recent_for_site(&conn, &id, limit)?,
        None => feed::recent(&conn, limit)?,
    };
    resolve_target_labels(&conn, &mut rows)?;
    Ok(rows)
}

/// Clear the feed — the user's own record of their machine, theirs to wipe.
/// Returns the number of rows removed.
#[tauri::command]
pub fn agent_activity_clear(state: State<'_, AppState>) -> Result<usize> {
    let conn = db(&state)?;
    feed::clear(&conn)
}

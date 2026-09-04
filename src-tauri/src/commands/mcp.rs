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

/// The copy-paste connect lines the card shows (`rex mcp` is the dumb pipe).
///
/// TWO, because Claude Code's default is not the one most people want: `claude
/// mcp add` writes a **local** entry, which is that ONE project directory, and
/// a developer who set rexenv up in one repo then finds it missing in the next.
/// `--scope user` writes it once for every project on the machine. Both are
/// offered rather than one chosen for them: a per-project entry is the right
/// answer for a shared repo, where a user-scope server nobody else has is a
/// config other people cannot reproduce.
const CONNECT_COMMAND: &str = "claude mcp add rexenv -- rex mcp";
const CONNECT_COMMAND_USER: &str = "claude mcp add --scope user rexenv -- rex mcp";

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
    /// The copy-paste connect command for the card — Claude Code's DEFAULT
    /// scope, which is this project only.
    pub connect_command: &'static str,
    /// The same, written once for every project on the machine (`--scope
    /// user`). Both are shown: neither is right for everyone.
    pub connect_command_user: &'static str,
    /// The derived status the header line renders.
    pub activity: ActivityStatus,
    /// Recent feed rows, newest first, the card lists.
    pub recent: Vec<feed::AgentAction>,
    /// The Agent access dial (D15): level, duration, expiry, and the copy for
    /// what the level hands over — served from Rust so the card, the refusal
    /// and the plan cannot drift (#404).
    pub access: crate::core::agent_access::AgentAccess,
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
    feed::resolve_target_labels(&conn, &mut recent)?;
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
    let access = crate::core::agent_access::current(&conn)?;
    Ok(McpStatus {
        enabled,
        connect_command: CONNECT_COMMAND,
        connect_command_user: CONNECT_COMMAND_USER,
        activity,
        recent,
        access,
    })
}

/// The card's single read: toggle state + derived status + recent feed.
#[tauri::command]
pub fn mcp_status(state: State<'_, AppState>) -> Result<McpStatus> {
    status_snapshot(&state, CARD_LIMIT)
}

/// Keep every scratch site's From stamp in step with the MCP endpoint (D16).
///
/// The stamp (`core::wp_mailtag`) is what lets `mail_list`/`mail_get` tell a
/// scratch site's mail from the user's; before D16 it rode a separate mail
/// sub-toggle, and a machine that turned MCP on and never touched that toggle
/// had agents refused for mail they were entitled to. Now the stamp exists
/// exactly while the endpoint is on: written for every scratch site when the
/// endpoint is enabled (and at launch when it already is — the backfill for a
/// machine upgraded with MCP on), removed when it is disabled. A user-owned
/// site is never touched — the filter is the RECORDED origin, never the
/// domain. A site whose folder is gone is skipped with a log; whether one is
/// really stamped is answered at READ time by a stat.
///
/// Over `&Connection` rather than `State`, so the sandbox examples can call it
/// without binding the real socket. Returns (updated, skipped).
pub fn sync_scratch_mail_stamps(conn: &rusqlite::Connection, on: bool) -> (usize, usize) {
    let sites = match crate::core::sites::list(conn) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("mcp: could not list sites to update mail stamps: {e}");
            return (0, 0);
        }
    };
    let (mut updated, mut skipped) = (0usize, 0usize);
    for site in sites.iter().filter(|s| s.is_scratch()) {
        let docroot = std::path::Path::new(&site.path);
        if !docroot.is_dir() {
            skipped += 1;
            continue;
        }
        let outcome = if on {
            crate::core::wp_mailtag::enable(docroot, site.content_dir_rel(), &site.domain).map(|created_dir| {
                // v25: record ownership of a dir WE made, so teardown removes
                // it — never inferred later from emptiness.
                if created_dir {
                    let _ = store::set_site_mu_dir_created(conn, &site.id);
                }
            })
        } else {
            crate::core::wp_mailtag::disable(docroot)
        };
        match outcome {
            Ok(()) => updated += 1,
            Err(e) => {
                skipped += 1;
                log::warn!("mcp: mail stamp for {} could not be updated: {e}", site.domain);
            }
        }
    }
    log::info!("mcp: scratch mail stamps {} — {updated} site(s) updated, {skipped} skipped", if on { "on" } else { "off" });
    (updated, skipped)
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
        // The stamp rides the endpoint (D16): after a SUCCESSFUL bind, never
        // before — a failed bind stamps nothing, so on/bound/stamped never
        // diverge.
        sync_scratch_mail_stamps(&conn, enable);
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
    feed::resolve_target_labels(&conn, &mut rows)?;
    Ok(rows)
}

/// Clear the feed — the user's own record of their machine, theirs to wipe.
/// Returns the number of rows removed.
#[tauri::command]
pub fn agent_activity_clear(state: State<'_, AppState>) -> Result<usize> {
    let conn = db(&state)?;
    feed::clear(&conn)
}

// ── Agent access — the ONE dial (`docs/PLAN-mcp-parity.md` §7, D15–D17) ───────
//
// USER-driven: no IPC an agent can reach, and the three settings keys are
// Denied to the `settings` tool and the CLI. D17 folded the last per-call
// consent (publishing a site) into the dial's Full level, so these two
// commands and the launch sweep are the whole surface.

/// Read the Agent access dial (D15).
#[tauri::command]
pub fn agent_access_get(state: State<'_, AppState>) -> Result<crate::core::agent_access::AgentAccess> {
    let conn = db(&state)?;
    crate::core::agent_access::current(&conn)
}

/// Set the Agent access dial: a level, and for a level above Read a duration.
/// A user's click in the card and nothing else reaches this — the `settings`
/// tool and the CLI refuse the three keys (`settings_access`), because the
/// card carries the sentence that says what each level hands over.
#[tauri::command]
pub fn agent_access_set(
    state: State<'_, AppState>,
    level: crate::core::agent_access::AccessLevel,
    mode: Option<crate::core::agent_access::Mode>,
) -> Result<crate::core::agent_access::AgentAccess> {
    let conn = db(&state)?;
    let a = crate::core::agent_access::set(&conn, level, mode)?;
    log::info!(
        "mcp: agent access set to {} ({})",
        a.level.as_db(),
        a.mode.map_or("no duration", |m| m.as_db())
    );
    Ok(a)
}

/// End a "for this session" Agent access level — run once at launch
/// (`lib.rs`), BEFORE the socket binds, which is what "this session" MEANS:
/// the level does not outlive the process it was set in. Best-effort and
/// logged; a failure here must not stop the app.
pub fn end_session_grants_at_launch(state: &AppState) {
    // The dial's "this session" — the only session-scoped consent left (D17).
    match db(state).and_then(|conn| crate::core::agent_access::end_session_at_launch(&conn)) {
        Ok(true) => log::info!("mcp: agent access was set for the previous session — back at Read"),
        Ok(false) => {}
        Err(e) => log::warn!("mcp: could not end the previous session's agent access: {e}"),
    }
}

#[cfg(test)]
mod site_access_tests {
    /// **The launch ends the previous session's grants, and the wiring is in
    /// `lib.rs`.** A "for this session" grant that survived a relaunch would be
    /// a week-long grant wearing a shorter label — so the sweep is not optional
    /// plumbing, and this pins that the app's setup calls it.
    #[test]
    fn the_launch_ends_the_previous_sessions_site_grants() {
        let lib = include_str!("../lib.rs");
        assert!(
            lib.contains("commands::mcp::end_session_grants_at_launch("),
            "lib.rs no longer ends session grants at launch — \"for this session\" would then \
             mean \"until the ceiling\", which is not what the button said"
        );
        // …and the settings key is not writable from the CLI: the toggle's copy
        // is the consent, and a shell write would skip it (the mcp_enabled rule).
        let access = include_str!("../core/settings_access.rs");
        assert!(access.contains("\"agent_access_level\""), "the dial keys must be ruled on in settings_access (Denied)");
    }
}


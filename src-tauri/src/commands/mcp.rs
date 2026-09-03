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
    /// The SITES sub-toggle (MCP parity) — "Let agents manage my own sites".
    /// Off by default, independent of both above; on its own grants nothing.
    pub sites_enabled: bool,
    /// The toggle's label, from the ONE constant the refusal text also uses —
    /// the card renders this rather than typing it (#404).
    pub sites_toggle_label: &'static str,
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
    let mail_enabled = mcp_server::mail_enabled(&conn);
    let sites_enabled = mcp_server::sites_enabled(&conn);
    Ok(McpStatus {
        enabled,
        connect_command: CONNECT_COMMAND,
        activity,
        recent,
        mail_enabled,
        sites_enabled,
        sites_toggle_label: mcp_server::SITES_TOGGLE_LABEL,
    })
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

// ── Agent database grants (M3 stage 4) ───────────────────────────────────────
//
// The consent surface. `db_query`'s refusal records the ask; these four
// commands are how a human answers it and how they see, later, what they
// answered. Everything here is USER-driven — there is no IPC an agent can
// reach, and no command that grants without a site id and a client name the
// user was actually shown.

/// The asks an agent has made and nobody has answered yet.
#[tauri::command]
pub fn agent_db_requests(
    state: State<'_, AppState>,
) -> Result<Vec<crate::core::agent_db::GrantRequest>> {
    let reqs = state
        .agent_db_requests
        .lock()
        .map_err(|_| crate::error::Error::Other("the agent request list is poisoned".into()))?;
    Ok(reqs.list().to_vec())
}

/// Every grant, live and dead, newest first — the answer to "what could that
/// agent see, and until when".
#[tauri::command]
pub fn agent_db_grants(
    state: State<'_, AppState>,
) -> Result<Vec<crate::state::store::AgentDbGrant>> {
    let conn = db(&state)?;
    crate::state::store::list_agent_db_grants(&conn)
}

/// Approve one ask: create the read-only principal on the engine and record the
/// grant with its expiry.
///
/// **The principal is created HERE, not on first use.** A grant row whose
/// database account does not exist would be a UI saying access is live while
/// every query fails, and the user would have no way to tell which half is
/// wrong. Provisioning first also means the failure the user sees is "the
/// database engine is not running" at the moment they clicked, which is the
/// moment they can do something about it.
#[tauri::command]
pub async fn agent_db_grant(
    state: State<'_, AppState>,
    site_id: String,
    client: String,
) -> Result<crate::state::store::AgentDbGrant> {
    use crate::core::agent_db::{self, Principal, GRANT_DAYS};

    let site = {
        let conn = db(&state)?;
        crate::state::store::get_site(&conn, &site_id)?
            .ok_or_else(|| crate::error::Error::Other(format!("no site with id {site_id:?}")))?
    };
    let engine = crate::core::db::DbEngine::from_site(site.db_engine);
    let version = super::database::effective_db_version(&state, engine)?;
    let client_bin = engine
        .cached_sql_client(state.platform.as_ref(), &version)
        .ok_or_else(|| {
            crate::error::Error::Other(format!(
                "{}'s client is not installed, so the read-only account cannot be created",
                engine.label()
            ))
        })?;
    let user = agent_db::principal_name(Principal::ReadOnly, &site.domain);
    agent_db::provision(&client_bin, engine.port(), Principal::ReadOnly, &site.db_name, &user)?;

    let conn = db(&state)?;
    let grant = crate::state::store::grant_agent_db(
        &conn,
        &uuid::Uuid::new_v4().to_string(),
        &site_id,
        &client,
        &user,
        GRANT_DAYS,
        false, // a human clicked Allow — this is the command the button calls
    )?;
    drop(conn);
    if let Ok(mut reqs) = state.agent_db_requests.lock() {
        reqs.answer(&site_id, &client);
    }
    Ok(grant)
}

/// Deny one ask without granting anything. Separate from `agent_db_grant`
/// because a denial is an answer the user gave, and leaving the prompt up until
/// it happens to be granted would make "no" the one response the UI cannot
/// express.
#[tauri::command]
pub fn agent_db_deny(state: State<'_, AppState>, site_id: String, client: String) -> Result<()> {
    let mut reqs = state
        .agent_db_requests
        .lock()
        .map_err(|_| crate::error::Error::Other("the agent request list is poisoned".into()))?;
    reqs.answer(&site_id, &client);
    Ok(())
}

/// Revoke a live grant: stop the access, then record when it stopped.
///
/// **In that order, and it matters.** Dropping the account first means a
/// revocation that fails halfway leaves access already closed and a row that
/// still says live — visibly wrong, and safe. Recording first would leave a row
/// saying "revoked" over an account that can still read, which is the same
/// wrongness pointed the other way: a user told they are safe when they are
/// not.
#[tauri::command]
pub async fn agent_db_revoke(state: State<'_, AppState>, id: String) -> Result<()> {
    let grant = {
        let conn = db(&state)?;
        crate::state::store::get_agent_db_grant(&conn, &id)?
            .ok_or_else(|| crate::error::Error::Other(format!("no grant with id {id:?}")))?
    };
    let site = {
        let conn = db(&state)?;
        crate::state::store::get_site(&conn, &grant.site_id)?
    };
    // A grant whose site is already gone has nothing to drop — the site delete
    // took the database and its accounts with it. Recording the revocation is
    // still right: the row is evidence, and evidence should say it ended.
    if let Some(site) = site {
        let engine = crate::core::db::DbEngine::from_site(site.db_engine);
        let version = super::database::effective_db_version(&state, engine)?;
        if let Some(client_bin) = engine.cached_sql_client(state.platform.as_ref(), &version) {
            crate::core::agent_db::deprovision(&client_bin, engine.port(), &grant.db_user)?;
        }
    }
    let conn = db(&state)?;
    crate::state::store::revoke_agent_db_grant(&conn, &id)?;
    Ok(())
}

/// Is auto-allow on for this session? (`core::agent_db::AutoAllow`.)
///
/// Session state, so the UI must ASK rather than remember: a fresh launch is
/// always off, and a toggle left visually on across a restart would be the
/// worst possible lie for this particular switch.
#[tauri::command]
pub fn agent_db_auto_allow(state: State<'_, AppState>) -> Result<bool> {
    Ok(state
        .agent_db_auto_allow
        .lock()
        .map(|a| a.is_on())
        .unwrap_or(false))
}

/// Turn auto-allow on or off for this session.
///
/// Switching it OFF does not revoke what it already granted — those are real
/// grants with real expiries, listed and revocable individually, exactly like
/// ones a person clicked. Silently revoking them here would make this switch
/// mean two things at once, and the user would have no way to tell which
/// access ended because of the toggle and which they ended themselves.
#[tauri::command]
pub fn agent_db_set_auto_allow(state: State<'_, AppState>, on: bool) -> Result<bool> {
    let mut a = state
        .agent_db_auto_allow
        .lock()
        .map_err(|_| crate::error::Error::Other("the auto-allow lock is poisoned".into()))?;
    a.set(on);
    log::info!("mcp: database auto-allow {} for this session", if on { "ON" } else { "off" });
    Ok(a.is_on())
}

// ── Site access (MCP parity, `docs/PLAN-mcp-parity.md` §3) ─────────────────────
//
// The scope-grant consent surface: the same shape as the database one above,
// generalised. USER-driven throughout — no IPC an agent can reach, and no
// command that grants without a client name and a scope the user was shown.

/// Turn the SITES sub-toggle on or off. A flag flip and nothing else — unlike
/// mail there is no filesystem state to keep in step — but it is the switch
/// every parity tool checks BEFORE the grant gate, so off means every such tool
/// refuses by name at once. Grants are left as they are: they are the user's
/// decisions, listed and revocable individually, and they resume when the
/// switch does. Silently revoking them here would make one switch mean two
/// things (the mail toggle's reasoning, the auto-allow toggle's reasoning).
#[tauri::command]
pub fn mcp_set_sites_enabled(state: State<'_, AppState>, enable: bool) -> Result<McpStatus> {
    {
        let conn = db(&state)?;
        store::set_setting(&conn, mcp_server::MCP_SITES_ENABLED_KEY, if enable { "true" } else { "false" })?;
        log::info!("mcp: acting on the user's own sites {}", if enable { "enabled" } else { "disabled" });
    }
    status_snapshot(&state, CARD_LIMIT)
}

/// End every "for this session" grant — run once at launch (`lib.rs`), which is
/// what "this session" MEANS: the grant does not outlive the process it was
/// given in. Best-effort and logged; a failure here must not stop the app.
pub fn end_session_grants_at_launch(state: &AppState) {
    match db(state).and_then(|conn| store::revoke_session_agent_site_grants(&conn)) {
        Ok(0) => {}
        Ok(n) => log::info!("mcp: ended {n} site-access grant(s) that were for the previous session"),
        Err(e) => log::warn!("mcp: could not end the previous session's site-access grants: {e}"),
    }
}

/// One ask as the card renders it: the request, plus the ONE sentence saying
/// what the scope allows — `Scope::what_it_allows`, served from Rust so the
/// prompt's most load-bearing line has one source and the copy guard pins it
/// beside the rule it describes rather than a TS copy of it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSiteAsk {
    #[serde(flatten)]
    pub request: crate::core::agent_grants::GrantRequest,
    pub allows: &'static str,
}

/// The scope asks an agent has made that nobody has answered yet.
#[tauri::command]
pub fn agent_site_requests(state: State<'_, AppState>) -> Result<Vec<AgentSiteAsk>> {
    let reqs = state
        .agent_site_requests
        .lock()
        .map_err(|_| crate::error::Error::Other("the agent request list is poisoned".into()))?;
    Ok(reqs
        .list()
        .iter()
        .cloned()
        .map(|request| AgentSiteAsk { allows: request.scope.what_it_allows(), request })
        .collect())
}

/// One grant as the list renders it: the row, plus the site's CURRENT domain
/// resolved at read time (`None` for a stack grant or a site since deleted —
/// the UI says "(deleted site)", never a bare id, the feed's own rule).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSiteGrantRow {
    #[serde(flatten)]
    pub grant: crate::state::store::AgentSiteGrant,
    pub site_label: Option<String>,
}

/// Every scope grant, live and dead, newest first.
#[tauri::command]
pub fn agent_site_grants(state: State<'_, AppState>) -> Result<Vec<AgentSiteGrantRow>> {
    let conn = db(&state)?;
    let rows = crate::state::store::list_agent_site_grants(&conn)?;
    let mut out = Vec::with_capacity(rows.len());
    for grant in rows {
        let site_label = match grant.site_id.as_deref() {
            Some(id) => crate::state::store::get_site(&conn, id)?.map(|s| s.domain),
            None => None,
        };
        out.push(AgentSiteGrantRow { grant, site_label });
    }
    Ok(out)
}

/// Approve one ask: record a grant for `scope` on `site_id` (or the stack, for
/// `None`) to `client`, for 7 days or for this session.
///
/// Refuses, rather than records, the one shape no dialog should have offered:
/// a scratch site (the agent's own — no grant applies, and a row for one would
/// be a permission nothing reads). Checked HERE and not only in the UI, because
/// this is the command the button calls and the UI is not the boundary.
#[tauri::command]
pub fn agent_site_grant(
    state: State<'_, AppState>,
    site_id: Option<String>,
    client: String,
    scope: crate::core::agent_grants::Scope,
    session: bool,
) -> Result<crate::state::store::AgentSiteGrant> {
    use crate::core::agent_grants::{GRANT_DAYS, SESSION_CEILING_DAYS};
    let conn = db(&state)?;
    // A stack-level grant (`None`) passes: every scope has a meaning there — the
    // inbox is a stack-level `read`, clearing it a stack-level `destroy`.
    if let Some(id) = site_id.as_deref() {
        let site = crate::state::store::get_site(&conn, id)?
            .ok_or_else(|| crate::error::Error::Other(format!("no site with id {id:?}")))?;
        if site.is_scratch() {
            return Err(crate::error::Error::Other(format!(
                "`{}` is a scratch site the agent created — it needs no permission, and rexenv \
                 will not record one for it",
                site.domain
            )));
        }
    }
    let days = if session { SESSION_CEILING_DAYS } else { GRANT_DAYS };
    let grant = crate::state::store::grant_agent_site(
        &conn,
        &uuid::Uuid::new_v4().to_string(),
        site_id.as_deref(),
        &client,
        scope.as_db(),
        days,
        false, // a human clicked — this is the command the buttons call
        session,
    )?;
    drop(conn);
    if let Ok(mut reqs) = state.agent_site_requests.lock() {
        reqs.answer(site_id.as_deref(), &client, scope);
    }
    Ok(grant)
}

/// Deny one scope ask without granting anything — "no" is an answer.
#[tauri::command]
pub fn agent_site_deny(
    state: State<'_, AppState>,
    site_id: Option<String>,
    client: String,
    scope: crate::core::agent_grants::Scope,
) -> Result<()> {
    let mut reqs = state
        .agent_site_requests
        .lock()
        .map_err(|_| crate::error::Error::Other("the agent request list is poisoned".into()))?;
    reqs.answer(site_id.as_deref(), &client, scope);
    Ok(())
}

/// Revoke a scope grant: a timestamp, kept as evidence. Nothing to drop on an
/// engine — a scope grant provisions no account (that is the DB grant's own
/// lifecycle), so recording the end IS the whole revocation, and a session
/// already inside a tool call re-asserts through `still_granted` before its
/// destructive step.
#[tauri::command]
pub fn agent_site_revoke(state: State<'_, AppState>, id: String) -> Result<()> {
    let conn = db(&state)?;
    if crate::state::store::get_agent_site_grant(&conn, &id)?.is_none() {
        return Err(crate::error::Error::Other(format!("no grant with id {id:?}")));
    }
    crate::state::store::revoke_agent_site_grant(&conn, &id)?;
    Ok(())
}

/// Which auto-allowable scopes are on for THIS session. Session state, so the
/// UI asks rather than remembers — a fresh launch is always none.
#[tauri::command]
pub fn agent_site_auto_allow(
    state: State<'_, AppState>,
) -> Result<Vec<crate::core::agent_grants::AutoAllowable>> {
    Ok(state.agent_site_auto_allow.lock().map(|a| a.on()).unwrap_or_default())
}

/// Turn auto-allow on or off for one scope, this session. The argument is
/// `AutoAllowable`, not `Scope`: a request to auto-allow `destroy` or `system`
/// does not deserialise, which is the type doing the refusing (#470). Switching
/// off does not revoke what it granted — those are ordinary grants.
#[tauri::command]
pub fn agent_site_set_auto_allow(
    state: State<'_, AppState>,
    scope: crate::core::agent_grants::AutoAllowable,
    on: bool,
) -> Result<Vec<crate::core::agent_grants::AutoAllowable>> {
    let mut a = state
        .agent_site_auto_allow
        .lock()
        .map_err(|_| crate::error::Error::Other("the auto-allow lock is poisoned".into()))?;
    a.set(scope, on);
    log::info!("mcp: site-access auto-allow for `{}` {} for this session", scope.scope(), if on { "ON" } else { "off" });
    Ok(a.on())
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
        assert!(access.contains("\"mcp_sites_enabled\""), "mcp_sites_enabled must be ruled on in settings_access (Denied)");
    }
}


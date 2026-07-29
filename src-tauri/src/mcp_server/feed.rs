//! The MCP agent activity feed — the accountability record of what an AI agent
//! did through the MCP server, so nothing an agent does is silent.
//!
//! Two disciplines, both structural:
//!
//! - **Complete by construction.** The session records EVERY tools/call outcome
//!   — success, a handler error, an unknown tool, a malformed request — not just
//!   the happy path. The non-happy-path rows are the ones an investigator most
//!   wants and the easiest to forget, so they are logged at the same one place
//!   (`mcp_server`'s session loop), not per-handler.
//!
//! - **A typed shape, not a growing string.** A "what did it pass" summary is
//!   exactly the field that creeps into "the whole request", and tool args can
//!   carry something sensitive tomorrow. So the record is TYPED: the only
//!   argument stored is `target_site` (the site a call named); there is no
//!   free-form arg column, so a later tool's args cannot smuggle content into
//!   the feed. `detail` is rexenv's OWN bounded reason for a non-ok outcome,
//!   never agent-supplied content. A per-tool value that genuinely belongs in
//!   the record (e.g. a future db_query's SQL) gets its OWN typed column, added
//!   deliberately when that tool lands — never a catch-all blob.
//!
//! The table only grows, and one agent session can make hundreds of calls, so it
//! is bounded by a row cap on every write and is user-clearable.

use crate::error::Result;
use rusqlite::{params, Connection};
use serde::Serialize;

/// Hard bound on the table — pruned to the newest this-many rows on every write,
/// so a runaway session can't grow the SQLite file without limit.
const ROW_CAP: i64 = 2000;

/// Bound on `detail` (rexenv's own reason text) — a message, never a document.
const DETAIL_MAX: usize = 500;

/// Bound on the AGENT-controlled fields (client name, tool name, target site id).
/// These come straight off the wire (`clientInfo.name`, `params.name`,
/// `arguments.site_id`) bounded only by the 4 MB line cap, so without this an
/// agent could drive gigabytes into the feed table — the row cap bounds row
/// COUNT, not row SIZE. The discipline: cap the UNTRUSTED fields hardest.
const FIELD_MAX: usize = 200;

/// How one agent action turned out. Non-`Ok` outcomes are the "something's off"
/// signal — an agent erroring, calling a tool that doesn't exist, sending
/// garbage, or (from M2) being denied — which the card surfaces more prominently
/// than a normal call (`is_concerning`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    Ok,
    Error,
    /// A tiered call refused for want of consent (M2 — no T0/M1 call is denied).
    Denied,
    UnknownTool,
    BadRequest,
}

impl Outcome {
    fn as_db(self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::Error => "error",
            Outcome::Denied => "denied",
            Outcome::UnknownTool => "unknown-tool",
            Outcome::BadRequest => "bad-request",
        }
    }

    fn from_db(s: &str) -> Outcome {
        match s {
            "ok" => Outcome::Ok,
            "denied" => Outcome::Denied,
            "unknown-tool" => Outcome::UnknownTool,
            "bad-request" => Outcome::BadRequest,
            _ => Outcome::Error,
        }
    }

    /// The card surfaces these more prominently — the signal that something is
    /// wrong with the agent or with what it read.
    pub fn is_concerning(self) -> bool {
        !matches!(self, Outcome::Ok)
    }
}

/// What the session knows about one action — everything but the client name
/// (session state, supplied at record time).
#[derive(Debug)]
pub struct PendingLog {
    pub tool: String,
    pub target_site: Option<String>,
    pub outcome: Outcome,
    pub detail: Option<String>,
}

/// One recorded action, as the card reads it.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentAction {
    pub id: i64,
    pub at: String,
    pub client: String,
    pub tool: String,
    pub target_site: Option<String>,
    pub outcome: Outcome,
    pub detail: Option<String>,
    /// Whether the card should surface this row prominently.
    pub concerning: bool,
}

/// Record one action, then prune to the row cap. `client` is the session's
/// self-reported client name; SQLite stamps `at`.
pub fn record(conn: &Connection, client: &str, log: &PendingLog) -> Result<()> {
    // Cap the agent-controlled fields (client/tool/target_site) hardest — they
    // arrive off the wire; only rexenv's own `detail` was previously bounded.
    let client = truncate(client, FIELD_MAX);
    let tool = truncate(&log.tool, FIELD_MAX);
    let target = log.target_site.as_deref().map(|s| truncate(s, FIELD_MAX));
    let detail = log.detail.as_deref().map(|d| truncate(d, DETAIL_MAX));
    conn.execute(
        "INSERT INTO agent_actions (at, client, tool, target_site, outcome, detail) \
         VALUES (datetime('now'), ?1, ?2, ?3, ?4, ?5)",
        params![client, tool, target, log.outcome.as_db(), detail],
    )?;
    conn.execute(
        "DELETE FROM agent_actions WHERE id NOT IN \
         (SELECT id FROM agent_actions ORDER BY id DESC LIMIT ?1)",
        params![ROW_CAP],
    )?;
    Ok(())
}

/// The most recent actions, newest first (for the card).
pub fn recent(conn: &Connection, limit: usize) -> Result<Vec<AgentAction>> {
    let mut stmt = conn.prepare(
        "SELECT id, at, client, tool, target_site, outcome, detail \
         FROM agent_actions ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit as i64], |r| {
            let outcome = Outcome::from_db(&r.get::<_, String>(5)?);
            Ok(AgentAction {
                id: r.get(0)?,
                at: r.get(1)?,
                client: r.get(2)?,
                tool: r.get(3)?,
                target_site: r.get(4)?,
                outcome,
                detail: r.get(6)?,
                concerning: outcome.is_concerning(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Clear the feed — the user's record of their own machine, theirs to wipe.
pub fn clear(conn: &Connection) -> Result<usize> {
    Ok(conn.execute("DELETE FROM agent_actions", [])?)
}

/// Truncate to `max` CHARACTERS (not bytes — never split a UTF-8 boundary).
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect::<String>() + "…"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        crate::state::db::open_in_memory().expect("in-memory db with migrations")
    }

    fn log(tool: &str, target: Option<&str>, outcome: Outcome, detail: Option<&str>) -> PendingLog {
        PendingLog {
            tool: tool.into(),
            target_site: target.map(String::from),
            outcome,
            detail: detail.map(String::from),
        }
    }

    #[test]
    fn records_and_reads_back_newest_first_with_the_concerning_flag() {
        let conn = mem();
        record(&conn, "Claude Code", &log("list_sites", None, Outcome::Ok, None)).unwrap();
        record(&conn, "Claude Code", &log("site_status", Some("s1"), Outcome::Error, Some("boom")))
            .unwrap();
        let rows = recent(&conn, 10).unwrap();
        assert_eq!(rows.len(), 2);
        // Newest first.
        assert_eq!(rows[0].tool, "site_status");
        assert_eq!(rows[0].target_site.as_deref(), Some("s1"));
        assert_eq!(rows[0].outcome, Outcome::Error);
        assert!(rows[0].concerning, "a non-ok outcome is concerning");
        assert!(!rows[1].concerning, "an ok outcome is not");
        assert!(rows[0].at.len() >= 10, "SQLite stamped a timestamp: {}", rows[0].at);
    }

    #[test]
    fn only_the_typed_fields_are_stored_args_cannot_smuggle_content() {
        // The session builds `target_site` from the site_id arg ONLY; there is
        // no column for anything else, so a smuggled arg has nowhere to land.
        let conn = mem();
        record(&conn, "c", &log("tail_log", Some("site-7"), Outcome::Ok, None)).unwrap();
        let row = &recent(&conn, 1).unwrap()[0];
        assert_eq!(row.target_site.as_deref(), Some("site-7"));
        // The record's whole serialized form carries no free-form arg field.
        let json = serde_json::to_value(row).unwrap();
        let keys: Vec<&str> = json.as_object().unwrap().keys().map(String::as_str).collect();
        for k in &keys {
            assert!(
                ["id", "at", "client", "tool", "targetSite", "outcome", "detail", "concerning"]
                    .contains(k),
                "unexpected feed field `{k}` — the record shape grew"
            );
        }
    }

    #[test]
    fn detail_is_bounded_so_a_reason_can_never_be_a_document() {
        let conn = mem();
        let huge = "x".repeat(5000);
        record(&conn, "c", &log("site_status", None, Outcome::Error, Some(&huge))).unwrap();
        let d = recent(&conn, 1).unwrap()[0].detail.clone().unwrap();
        assert!(d.chars().count() <= DETAIL_MAX + 1, "detail not bounded: {}", d.chars().count());
    }

    #[test]
    fn the_agent_controlled_fields_are_bounded_not_just_detail() {
        // client/tool/target_site come off the wire (≤4 MB each) — an
        // assembly-review finding: only `detail` was capped, so an agent could
        // amplify writes far past the row cap. All three must be bounded.
        let conn = mem();
        let huge = "z".repeat(5000);
        record(&conn, &huge, &log(&huge, Some(&huge), Outcome::UnknownTool, None)).unwrap();
        let row = &recent(&conn, 1).unwrap()[0];
        assert!(row.client.chars().count() <= FIELD_MAX + 1, "client unbounded");
        assert!(row.tool.chars().count() <= FIELD_MAX + 1, "tool unbounded");
        assert!(row.target_site.as_ref().unwrap().chars().count() <= FIELD_MAX + 1, "target unbounded");
    }

    #[test]
    fn the_row_cap_bounds_the_table_on_every_write() {
        let conn = mem();
        for i in 0..(ROW_CAP + 50) {
            record(&conn, "c", &log("list_sites", Some(&format!("s{i}")), Outcome::Ok, None)).unwrap();
        }
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM agent_actions", [], |r| r.get(0)).unwrap();
        assert_eq!(count, ROW_CAP, "the feed must be capped");
    }

    #[test]
    fn clear_empties_the_feed() {
        let conn = mem();
        record(&conn, "c", &log("list_sites", None, Outcome::Ok, None)).unwrap();
        assert_eq!(clear(&conn).unwrap(), 1);
        assert!(recent(&conn, 10).unwrap().is_empty());
    }

    #[test]
    fn the_feed_survives_a_close_and_reopen_it_is_a_table_not_in_memory_state() {
        let path = std::env::temp_dir().join(format!("rexenv-feed-persist-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let conn = crate::state::db::open(&path).unwrap();
            record(&conn, "Claude Code", &log("site_status", Some("s1"), Outcome::Denied, None))
                .unwrap();
        } // conn dropped — the app "closed"
        let conn = crate::state::db::open(&path).unwrap(); // reopened
        let rows = recent(&conn, 10).unwrap();
        assert_eq!(rows.len(), 1, "the feed did not survive reopen");
        assert_eq!(rows[0].outcome, Outcome::Denied);
        let _ = std::fs::remove_file(&path);
    }
}

//! The MCP agent activity feed — the accountability record of what happened to
//! agent-owned things, so nothing an agent does is silent.
//!
//! Three disciplines, all structural:
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
//!   `target_site` gained a SECOND provenance in M2a and it did not loosen this:
//!   a handler can name the site rexenv ACTED on ([`ActedTarget`]) — necessary,
//!   because a create has no `site_id` argument to record — but only by handing
//!   over a `&Site`, a row from our own sites table. Reading the id out of a
//!   tool's RESULT would have been the easy version and the wrong one: results
//!   are the channel a future tool could echo an argument through.
//!
//! - **Attribution is typed, because two true claims had to coexist** (v28).
//!   "An AI agent did this" was true while the session loop was the only
//!   writer. M2a's reaper breaks it: deleting an expired scratch site is the
//!   most consequential event in that lifecycle, so it cannot be invisible —
//!   and it is rexenv's own doing, so an unlabelled row would make every
//!   neighbour's attribution a lie by juxtaposition. `actor` keeps both: the
//!   row is listed, and it says who. It LABELS everywhere and FILTERS in
//!   exactly one place (`recent_head`, the "an agent is working" status line).
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

/// WHO performed the action a row records (v28).
///
/// The feed is an accountability record, and its whole value is that a row's
/// attribution is true. So the two directions are NOT symmetric:
///
/// - the migration's `DEFAULT 'agent'` records a KNOWN fact — `record` is
///   called from exactly one place (the MCP session loop), so every pre-v28 row
///   is an agent's tool call;
/// - but an unrecognised stored value reads as [`FeedActor::Rexenv`], because
///   labelling rexenv's own action as an agent's is the damaging error (a false
///   accusation in the record), while the reverse merely under-attributes. It
///   is also the correct forward-compatible read: a future actor this build
///   doesn't know is, by definition, not the agent.
///
/// **Actor LABELS a row; it never hides one.** `recent`/`recent_for_site`
/// return every row whatever its actor — the UI says who did each. The one
/// place it filters is [`recent_head`], which drives the card's "an agent is
/// working" line: a claim about the AGENT's session must not be fed by rexenv's
/// own housekeeping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FeedActor {
    /// A connected MCP client, through the session loop.
    Agent,
    /// rexenv itself — the scratch reaper (M2a) and anything like it.
    Rexenv,
}

impl FeedActor {
    pub fn as_db(self) -> &'static str {
        match self {
            FeedActor::Agent => "agent",
            FeedActor::Rexenv => "rexenv",
        }
    }

    /// Read a stored value. Never fails, and never fails TOWARD the agent — see
    /// the type doc for why that asymmetry is the point.
    pub fn from_db(s: &str) -> FeedActor {
        match s {
            "agent" => FeedActor::Agent,
            _ => FeedActor::Rexenv,
        }
    }
}

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

/// The site REXENV ACTED ON during a call — the second, narrower way a feed row
/// gets its target, and the one that carries no agent content.
///
/// Until M2a the feed recorded what the agent ASKED for (`arguments.site_id`, a
/// string off the wire). A create has no such argument — it makes the site — so
/// the row that matters most would name nothing. This records what rexenv DID.
///
/// **Two properties, both structural rather than remembered:**
///
/// 1. **It cannot carry agent content.** [`ActedTarget::set`] takes a `&Site` —
///    a row from rexenv's OWN sites table — and reads its `id`. There is no
///    constructor from a string, so no handler can route an argument, a tool
///    result, or anything else off the wire into the feed through here. Same
///    discipline as `AgentAction::target_label`: rexenv-derived by construction,
///    and nothing branches on it.
/// 2. **It survives `?`.** It is an out-parameter, not a return value,
///    deliberately: a handler records the site the instant the row exists and
///    then keeps going, so an error thrown LATER (provisioning failing after the
///    insert) still leaves the target recorded. A `Result<(Value, Target)>`
///    would drop it on exactly the path where naming the site matters most —
///    the half-created site the user can now see, retry, or delete.
///
/// The consequence is the rule worth stating plainly: **a feed row names a site
/// if and only if a row for it exists**, and neither half is a guess. Nothing
/// created ⇒ nothing named; created-then-failed ⇒ named.
#[derive(Debug, Default)]
pub struct ActedTarget(std::sync::Mutex<Option<String>>);

impl ActedTarget {
    /// Record the site rexenv acted on. Takes the ROW, never an id string, so
    /// the value provably came from our own sites table.
    pub fn set(&self, site: &crate::state::models::Site) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = Some(site.id.clone());
        }
    }

    /// The recorded site id, if a handler reached the point of having a row.
    pub fn take(&self) -> Option<String> {
        self.0.lock().ok().and_then(|mut s| s.take())
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
    /// What the call was ABOUT, when the tool's name and target don't say (v30).
    /// Produced by the TOOL's own `summarise`, never by parsing agent JSON here,
    /// and clamped by [`clamp_summary`] at the write. `None` means "the name and
    /// target already describe this" — true of 8 of the 11 registered tools, the
    /// three exceptions being `scratch_add_package`, `wp_run` and
    /// `set_php_version`. (This comment said "seven of the eight" until 21 Aug
    /// 2026: the arithmetic was left behind when the second registry grew, while
    /// the RULE it states stayed correct. Count the registries, not the memory.)
    pub args_summary: Option<String>,
}

/// One recorded action, as the card reads it.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentAction {
    pub id: i64,
    pub at: String,
    /// WHO did this (v28). `Rexenv` rows are rexenv's own housekeeping (the
    /// scratch reaper) — listed like any other, labelled as ours, and excluded
    /// from the card's agent-activity status line.
    pub actor: FeedActor,
    /// For an agent row, the client's self-reported name. For a `Rexenv` row it
    /// is rexenv itself, not an agent-asserted string.
    pub client: String,
    pub tool: String,
    /// The STABLE site id this call CONCERNED — keyed on by `recent_for_site`,
    /// survives a domain rename. Not human-readable (a UUID).
    ///
    /// Two provenances, and the narrower one wins ([`ActedTarget`]): the site
    /// rexenv ACTED on when a handler reported one (rexenv-derived, from a row
    /// in our sites table), otherwise the site the agent NAMED
    /// (`arguments.site_id` — agent content, length-capped like every other
    /// wire-supplied field). Both are ids of the same kind, so the column stays
    /// one typed fact; what differs is which one can be trusted, and only the
    /// rexenv-derived one may appear for a call that took no `site_id`.
    pub target_site: Option<String>,
    /// The named site's CURRENT domain, resolved at READ time (never stored — the
    /// feed keeps only the stable id). `None` when there is no target, or the site
    /// was deleted; the UI then falls back to the raw id. rexenv-derived, never
    /// agent content, so the typed-shape discipline holds. Filled by the command
    /// layer (`commands::mcp`), which alone can reach the sites table.
    pub target_label: Option<String>,
    pub outcome: Outcome,
    pub detail: Option<String>,
    /// Whether the card should surface this row prominently.
    /// What the call was ABOUT, when the tool's name and target don't say (v30).
    /// `None` for most tools and that is an ANSWER: their name and target
    /// already describe them. Clamped at the write to `[a-z][a-z0-9-]{0,19}`
    /// tokens, so it can never impersonate a client name or rexenv's own rows.
    pub args_summary: Option<String>,
    pub concerning: bool,
}

/// Record one action, then prune to the row cap. `client` is the session's
/// self-reported client name; SQLite stamps `at`.
pub fn record(conn: &Connection, client: &str, log: &PendingLog) -> Result<()> {
    write(conn, FeedActor::Agent, client, log)
}

/// Record something REXENV did to an agent-owned site — the scratch reaper
/// (M2a), and anything later of the same kind.
///
/// It lands in the same table because deleting a site is the most consequential
/// event in the scratch lifecycle and must not be invisible; it carries
/// `actor = Rexenv` because the record's value is that attribution is true.
/// `client` is rexenv itself, so — unlike an agent row — this string is ours,
/// never off the wire.
pub fn record_system(conn: &Connection, log: &PendingLog) -> Result<()> {
    write(conn, FeedActor::Rexenv, "rexenv", log)
}

/// The one writer. Both entry points funnel here so the bounds and the row cap
/// can't be applied to one kind of row and forgotten on the other.
fn write(conn: &Connection, actor: FeedActor, client: &str, log: &PendingLog) -> Result<()> {
    // Cap the agent-controlled fields (client/tool/target_site) hardest — they
    // arrive off the wire; only rexenv's own `detail` was previously bounded.
    let client = truncate(client, FIELD_MAX);
    let tool = truncate(&log.tool, FIELD_MAX);
    let target = log.target_site.as_deref().map(|s| truncate(s, FIELD_MAX));
    let detail = log.detail.as_deref().map(|d| truncate(d, DETAIL_MAX));
    // Clamped HERE, at the one writer, not at the summariser — a tool that got
    // its own validation wrong still cannot put arbitrary text in the feed.
    let summary = log.args_summary.as_deref().and_then(clamp_summary);
    conn.execute(
        "INSERT INTO agent_actions (at, actor, client, tool, target_site, outcome, detail, args_summary) \
         VALUES (datetime('now'), ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![actor.as_db(), client, tool, target, log.outcome.as_db(), detail, summary],
    )?;
    conn.execute(
        "DELETE FROM agent_actions WHERE id NOT IN \
         (SELECT id FROM agent_actions ORDER BY id DESC LIMIT ?1)",
        params![ROW_CAP],
    )?;
    Ok(())
}

/// Tokens allowed in an argument summary, and the reason the rule is this tight.
///
/// A summary token must match `[a-z][a-z0-9-]{0,19}` — lowercase, no spaces, no
/// punctuation but `-`. That is not tidiness. The feed is where a user goes to
/// find out what an agent did, so text an agent CHOSE, rendered in that list,
/// could otherwise impersonate the client-name slot, rexenv's own
/// `rexenv · automatic` rows, or the `·` separators between them. An audit
/// surface that can be made to lie is worse than one that shows less. The
/// charset excludes every character such a forgery needs.
///
/// A whole summary is up to [`SUMMARY_TOKENS`] such tokens joined by a single
/// space. Anything that doesn't fit becomes `?` — a token rendered, deliberately,
/// rather than dropped: "the agent ran something whose name we won't repeat" is
/// information, and silently omitting it would make an odd call look like an
/// ordinary one.
const SUMMARY_TOKEN_MAX: usize = 20;
const SUMMARY_TOKENS: usize = 2;

/// Does one token fit the summary charset?
///
/// THE definition, exported so a summariser can decide where to STOP and the
/// writer can decide what to REPLACE — two different uses of one rule. A second
/// copy of this predicate is how the two drift into disagreeing about what is
/// safe.
pub fn is_summary_token(t: &str) -> bool {
    !t.is_empty()
        && t.len() <= SUMMARY_TOKEN_MAX
        && t.starts_with(|c: char| c.is_ascii_lowercase())
        && t.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Clamp a tool-produced summary to the shape above, or `None` if it is empty.
pub fn clamp_summary(raw: &str) -> Option<String> {
    let ok = is_summary_token;
    let out: Vec<&str> = raw
        .split_whitespace()
        .take(SUMMARY_TOKENS)
        .map(|t| if ok(t) { t } else { "?" })
        .collect();
    (!out.is_empty()).then(|| out.join(" "))
}

/// Columns selected for an `AgentAction`, in struct order — shared so `recent`
/// and `recent_for_site` read the same shape through `row_to_action`.
const ACTION_COLUMNS: &str =
    "id, at, actor, client, tool, target_site, outcome, detail, args_summary";

/// Map a row (selecting `ACTION_COLUMNS`) into an `AgentAction`.
fn row_to_action(r: &rusqlite::Row) -> rusqlite::Result<AgentAction> {
    let outcome = Outcome::from_db(&r.get::<_, String>(6)?);
    Ok(AgentAction {
        id: r.get(0)?,
        at: r.get(1)?,
        actor: FeedActor::from_db(&r.get::<_, String>(2)?),
        client: r.get(3)?,
        tool: r.get(4)?,
        target_site: r.get(5)?,
        target_label: None, // resolved by the command layer, never stored
        outcome,
        detail: r.get(7)?,
        // Stored clamped (see `clamp_summary`), so nothing here needs to
        // re-validate — but nothing downstream may relax it either.
        args_summary: r.get(8)?,
        concerning: outcome.is_concerning(),
    })
}

/// The most recent actions, newest first (for the card).
pub fn recent(conn: &Connection, limit: usize) -> Result<Vec<AgentAction>> {
    let sql = format!("SELECT {ACTION_COLUMNS} FROM agent_actions ORDER BY id DESC LIMIT ?1");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params![limit as i64], row_to_action)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The most recent actions that named `site` (the per-site SiteDetail section),
/// newest first — a WHERE on the stored `target_site`, so an older row for this
/// site is not lost behind a burst of activity on others.
pub fn recent_for_site(conn: &Connection, site: &str, limit: usize) -> Result<Vec<AgentAction>> {
    let sql = format!(
        "SELECT {ACTION_COLUMNS} FROM agent_actions WHERE target_site = ?1 ORDER BY id DESC LIMIT ?2"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params![site, limit as i64], row_to_action)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The head of AGENT activity within `window_mins`: the newest agent call, how
/// long ago, and (if it errored) how many consecutive concerning calls precede
/// the first success. Drives the card's status line — and it self-recovers,
/// because a row ages OUT of the window on its own, so an old error state clears
/// without any write. `None` = no agent activity in the window.
///
/// **This is the ONE place `actor` filters instead of labelling** (v28): the
/// status line is a claim about the agent's session ("Working — tail_log, 2
/// minutes ago"), so a rexenv row — a reaper sweep — must not make the card say
/// an agent is working when nothing is even connected. The listed feed still
/// shows those rows; it just says who did them.
#[derive(Debug, PartialEq, Eq)]
pub struct RecentHead {
    pub last_tool: String,
    pub minutes_ago: i64,
    pub last_ok: bool,
    pub trailing_errors: i64,
}

pub fn recent_head(conn: &Connection, window_mins: i64) -> Result<Option<RecentHead>> {
    // SQLite does the time math in UTC (matching `datetime('now')` at write), so
    // there is one clock; `minutes_ago` is whole minutes since the row's stamp.
    let mut stmt = conn.prepare(
        "SELECT tool, CAST((julianday('now') - julianday(at)) * 1440 AS INTEGER), outcome \
         FROM agent_actions WHERE at >= datetime('now', ?1) AND actor = 'agent' ORDER BY id DESC",
    )?;
    let window = format!("-{window_mins} minutes");
    let rows = stmt
        .query_map(params![window], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                Outcome::from_db(&r.get::<_, String>(2)?),
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let Some((tool, mins, out)) = rows.first().cloned() else {
        return Ok(None);
    };
    let last_ok = !out.is_concerning();
    // Count concerning rows from the newest, stopping at the first success — "the
    // last N calls errored", the honest N the status line names.
    let trailing_errors =
        if last_ok { 0 } else { rows.iter().take_while(|(_, _, o)| o.is_concerning()).count() as i64 };
    Ok(Some(RecentHead { last_tool: tool, minutes_ago: mins.max(0), last_ok, trailing_errors }))
}

/// The newest `scratch_reap` row for `site`, as `(outcome, detail)` — the reaper
/// reads it to avoid saying the same thing every launch.
///
/// **Why this exists.** A reap that fails is retried at most once per launch, so
/// a site that CANNOT be deleted (a database drop that keeps failing, say) would
/// otherwise write one identical row per launch, forever: a slow flood that
/// buries the feed's real content and makes a persistent problem look like many
/// events. The reaper records the FIRST occurrence and then stays quiet until the
/// outcome or the reason changes — so the feed carries one row per distinct
/// problem, while the site's own visible state (expired, still present) is what
/// says the problem is ongoing.
pub fn last_reap(conn: &Connection, site: &str) -> Result<Option<(Outcome, Option<String>)>> {
    let mut stmt = conn.prepare(
        "SELECT outcome, detail FROM agent_actions \
         WHERE tool = 'scratch_reap' AND target_site = ?1 ORDER BY id DESC LIMIT 1",
    )?;
    let mut rows = stmt.query_map(params![site], |r| {
        Ok((Outcome::from_db(&r.get::<_, String>(0)?), r.get::<_, Option<String>>(1)?))
    })?;
    match rows.next() {
        Some(v) => Ok(Some(v?)),
        None => Ok(None),
    }
}

/// The reaper's own record: what rexenv did to an agent-owned site, recorded
/// ONLY when it is news (see [`last_reap`]). Returns whether a row was written.
pub fn record_reap(
    conn: &Connection,
    site_id: &str,
    outcome: Outcome,
    detail: Option<String>,
) -> Result<bool> {
    if outcome != Outcome::Ok {
        if let Some((prev, prev_detail)) = last_reap(conn, site_id)? {
            if prev == outcome && prev_detail == detail {
                return Ok(false); // same problem, already said once
            }
        }
    }
    record_system(
        conn,
        &PendingLog {
            tool: "scratch_reap".into(),
            target_site: Some(site_id.to_string()),
            outcome,
            detail,
                    // A reap is rexenv's own row: `tool` + the site it names
            // describe it completely, and its reason travels in `detail`.
            args_summary: None,
},
    )?;
    Ok(true)
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
            args_summary: None,
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
                // `actor` (v28) and `argsSummary` (v30) are admitted
                // DELIBERATELY, which is the point of this guard: a field joins
                // the record by someone editing this list, never by a struct
                // quietly growing. `actor` is a closed enum rexenv sets.
                //
                // `argsSummary` is the harder admission and the reasoning is
                // recorded because it is the exact shape this guard exists to
                // refuse — an "argument summary" is the field that creeps into
                // "the whole request". It is admitted on three conditions, all
                // structural: it is produced by the TOOL's own `summarise` (so
                // the feed layer never parses agent JSON generically), it is
                // clamped at the single writer to at most two
                // `[a-z][a-z0-9-]{0,19}` tokens (so it cannot forge a row), and
                // it carries VERBS only — never argument values. Widen any one
                // of those and this admission stops being justified.
                [
                    "id", "at", "actor", "client", "tool", "targetSite", "targetLabel", "outcome",
                    "detail", "argsSummary", "concerning"
                ]
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
    fn a_summary_cannot_forge_a_feed_row() {
        // The clamp is a SECURITY property, not tidiness. The feed is where a
        // user goes to find out what an agent did, so agent-chosen text rendered
        // in that list must not be able to impersonate its neighbours: the
        // client-name slot, rexenv's own "rexenv · automatic" rows, or the `·`
        // separators between them. Every forgery below needs a character the
        // charset excludes.
        for forgery in [
            "rexenv · automatic",
            "plugin\nactivate",
            "Claude Code",
            "eval; DROP",
            "·",
            "PLUGIN",
            "../../etc",
            "<script>x</script>",
        ] {
            let got = clamp_summary(forgery);
            let text = got.clone().unwrap_or_default();
            assert!(
                text.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == ' ' || c == '?'),
                "`{forgery}` survived as `{text}` — a character that can forge a row got through"
            );
            assert!(!text.contains('·'), "`{forgery}` kept a separator: {text}");
        }
        // A refused token renders as `?` rather than vanishing: "the agent ran
        // something whose name we won't repeat" is information, and dropping it
        // silently would make an odd call look like an ordinary one.
        assert_eq!(clamp_summary("PLUGIN activate").as_deref(), Some("? activate"));
        // The ordinary case is untouched, and stops at two tokens — the values
        // after the subcommand are the content, and they are not recorded.
        assert_eq!(clamp_summary("plugin activate acme --force").as_deref(), Some("plugin activate"));
        assert_eq!(clamp_summary("eval").as_deref(), Some("eval"));
        // Length is bounded per token, so a long word cannot pad the row out.
        let long = "a".repeat(40);
        assert_eq!(clamp_summary(&long).as_deref(), Some("?"));
        assert_eq!(clamp_summary("   ").as_deref(), None, "whitespace summarises to nothing");
    }

    #[test]
    fn a_summary_is_clamped_at_the_WRITE_not_only_at_the_summariser() {
        // Defence placement, asserted: a tool whose own `summarise` was wrong —
        // or a future one that skips validation entirely — still cannot put
        // arbitrary text in the feed, because the single writer clamps.
        let conn = crate::state::db::open_in_memory().unwrap();
        record(
            &conn,
            "Claude Code",
            &PendingLog {
                tool: "wp_run".into(),
                target_site: None,
                outcome: Outcome::Ok,
                detail: None,
                args_summary: Some("rexenv · automatic".into()),
            },
        )
        .unwrap();
        let row = &recent(&conn, 1).unwrap()[0];
        let got = row.args_summary.clone().unwrap_or_default();
        // Assert the PROPERTY, not the exact string — the first draft of this
        // asserted `"? ?"` and was wrong, which is the useful part: `rexenv`
        // splits off as its own token and is all-lowercase-ascii, so it SURVIVES
        // as a word. That is fine and worth stating rather than hardening away
        // with a reserved-word denylist (the shape this codebase rejects): what
        // the clamp guarantees is that no SEPARATOR-bearing, row-shaped string
        // can be constructed. The word `rexenv` sitting in a dimmed verb slot is
        // not an impersonation — attribution lives in the typed `actor` (#205)
        // and its own rendering slot, neither of which this field can reach.
        assert!(!got.contains('·'), "the writer let a separator through: {got}");
        assert!(
            got.split(' ').all(|t| t == "?" || t.chars().all(|c| c.is_ascii_lowercase())),
            "a token survived that the charset should have refused: {got}"
        );
        assert!(got.contains('?'), "the `·` token must be refused, leaving a visible `?`: {got}");
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
    fn recent_for_site_returns_only_that_sites_rows_newest_first() {
        let conn = mem();
        record(&conn, "c", &log("site_status", Some("s1"), Outcome::Ok, None)).unwrap();
        record(&conn, "c", &log("tail_log", Some("s2"), Outcome::Ok, None)).unwrap();
        record(&conn, "c", &log("site_status", Some("s1"), Outcome::Error, Some("x"))).unwrap();
        let s1 = recent_for_site(&conn, "s1", 10).unwrap();
        assert_eq!(s1.len(), 2, "only s1's rows");
        assert_eq!(s1[0].outcome, Outcome::Error, "newest first");
        assert!(s1.iter().all(|r| r.target_site.as_deref() == Some("s1")));
        assert_eq!(recent_for_site(&conn, "s2", 10).unwrap().len(), 1);
        assert!(recent_for_site(&conn, "nope", 10).unwrap().is_empty());
    }

    #[test]
    fn recent_head_reads_the_head_and_counts_trailing_errors() {
        let conn = mem();
        record(&conn, "c", &log("list_sites", None, Outcome::Ok, None)).unwrap();
        record(&conn, "c", &log("site_status", Some("s1"), Outcome::Error, Some("x"))).unwrap();
        record(&conn, "c", &log("tail_log", Some("s1"), Outcome::UnknownTool, None)).unwrap();
        let h = recent_head(&conn, 15).unwrap().expect("head present");
        assert_eq!(h.last_tool, "tail_log");
        assert!(!h.last_ok, "newest call errored");
        assert_eq!(h.trailing_errors, 2, "two trailing concerning rows before the ok");
        assert!(h.minutes_ago >= 0);
        // A fresh success flips it back to working — self-recovery within the window.
        record(&conn, "c", &log("list_sites", None, Outcome::Ok, None)).unwrap();
        let h2 = recent_head(&conn, 15).unwrap().unwrap();
        assert!(h2.last_ok);
        assert_eq!(h2.trailing_errors, 0);
    }

    #[test]
    fn recent_head_ages_activity_out_of_the_window() {
        let conn = mem();
        // A row stamped 30 minutes ago is outside a 15-minute window (self-recovery
        // needs no write — the state clears as the row leaves the window).
        conn.execute(
            "INSERT INTO agent_actions (at, client, tool, target_site, outcome, detail) \
             VALUES (datetime('now','-30 minutes'), 'c', 'list_sites', NULL, 'ok', NULL)",
            [],
        )
        .unwrap();
        assert!(recent_head(&conn, 15).unwrap().is_none(), "30-min-old row is outside a 15-min window");
        assert!(recent_head(&conn, 60).unwrap().is_some(), "but inside a 60-min window");
    }

    #[test]
    fn recent_head_is_none_when_the_feed_is_empty() {
        assert!(recent_head(&mem(), 15).unwrap().is_none());
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

    #[test]
    fn a_rexenv_row_is_listed_and_labelled_but_never_says_an_agent_is_working() {
        // The v28 split, both halves in one test because they are one decision:
        // `actor` LABELS a row everywhere the user reads the feed, and FILTERS in
        // exactly one place — the status line, which is a claim about the agent's
        // session. A reaper sweep must never render as "Working — scratch_reap".
        let conn = mem();
        record_system(&conn, &log("scratch_reap", Some("site-7"), Outcome::Ok, Some("expired")))
            .unwrap();

        // Listed, with its actor, and attributed to rexenv rather than a client.
        let rows = recent(&conn, 10).unwrap();
        assert_eq!(rows.len(), 1, "a rexenv row is never hidden from the feed");
        assert_eq!(rows[0].actor, FeedActor::Rexenv);
        assert_eq!(rows[0].client, "rexenv");
        assert_eq!(rows[0].tool, "scratch_reap");
        // And in the per-site section too — this is the site's own history.
        assert_eq!(recent_for_site(&conn, "site-7", 10).unwrap().len(), 1);

        // But the status line sees nothing: no agent has done anything.
        assert_eq!(recent_head(&conn, 15).unwrap(), None, "rexenv's own work is not agent activity");

        // With a real agent call present, the head is that call — not the newer
        // rexenv row that would otherwise be "the most recent thing".
        record(&conn, "Claude Code", &log("list_sites", None, Outcome::Ok, None)).unwrap();
        record_system(&conn, &log("scratch_reap", Some("site-9"), Outcome::Ok, None)).unwrap();
        let head = recent_head(&conn, 15).unwrap().expect("the agent call is the head");
        assert_eq!(head.last_tool, "list_sites");
    }

    #[test]
    fn a_failed_reap_is_recorded_as_concerning_and_still_ours() {
        // The reaper (M2a task 9) records its failures here rather than in a new
        // sites column. A failed reap must read as concerning — but as REXENV's
        // problem, never as an agent erroring, which would otherwise show up as
        // "the last N calls errored" in a line about the agent's session.
        let conn = mem();
        record_system(
            &conn,
            &log("scratch_reap", Some("site-7"), Outcome::Error, Some("database drop failed")),
        )
        .unwrap();
        let row = &recent(&conn, 1).unwrap()[0];
        assert!(row.concerning);
        assert_eq!(row.actor, FeedActor::Rexenv);
        assert_eq!(row.detail.as_deref(), Some("database drop failed"));
        assert_eq!(recent_head(&conn, 15).unwrap(), None, "not the agent's error to wear");
    }

    #[test]
    fn an_unrecognised_actor_reads_as_rexenv_never_as_the_agent() {
        // Direction matters more than the value: a corrupt cell, or a row
        // written by a FUTURE rexenv with an actor this build doesn't know,
        // must not be attributed to an AI agent. Under-attributing our own work
        // is survivable; a false accusation in an accountability record is not.
        let conn = mem();
        for odd in ["", "AGENT", " agent", "user", "reaper"] {
            conn.execute("DELETE FROM agent_actions", []).unwrap();
            conn.execute(
                "INSERT INTO agent_actions (at, actor, client, tool, outcome) \
                 VALUES (datetime('now'), ?1, 'x', 'list_sites', 'ok')",
                params![odd],
            )
            .unwrap();
            assert_eq!(recent(&conn, 1).unwrap()[0].actor, FeedActor::Rexenv, "{odd:?}");
            // ...and it cannot drive the agent status line either.
            assert_eq!(recent_head(&conn, 15).unwrap(), None, "{odd:?}");
        }
        // Only the exact value is the agent.
        assert_eq!(FeedActor::from_db("agent"), FeedActor::Agent);
    }

}

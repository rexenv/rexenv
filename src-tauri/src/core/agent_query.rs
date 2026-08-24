//! The agent read path: a native MySQL connection, never the bundled client
//! (`docs/PLAN-mcp-server.md` §3.6, M3 stage 2b).
//!
//! ## Why this is a deliberate departure from the bundled-client rule
//!
//! Everywhere else in rexenv, MySQL work goes through the bundled `mysql`/
//! `mariadb` binary, so client and server versions match on dump and restore.
//! That rule is right for dump/restore and wrong here, for one reason: **the
//! bundled client interprets commands before the server ever sees them.**
//! `mysql -e "system id"` runs a shell. `\!`, `source <file>` and `tee <path>`
//! execute and write as the user regardless of any GRANT. Every existing exec
//! helper feeds the client via `-e` or stdin, so handing a `GRANT SELECT`
//! principal to that path would give an agent shell-exec and file-write on a
//! REAL site, from the one tool the consent dialog authorized for reading.
//!
//! A native driver speaks the stable MySQL wire protocol to the same loopback
//! port and has no such vocabulary. Its refusal to interpret `system` is not a
//! side benefit — it is the entire reason this module exists.
//!
//! ## What bounds an agent query
//!
//! Four independent things, three of them structural:
//!
//! 1. **The GRANT** (`core::agent_db`) — a real site's principal holds `SELECT`
//!    on one escaped database. Writes, `DROP`, and `FILE` (so `INTO OUTFILE`)
//!    are refused by the server, not by anything here.
//! 2. **No `CLIENT_MULTI_STATEMENTS`** — one statement per call. A second
//!    statement smuggled past a `;` is a protocol error, not a second query.
//! 3. **No `LOCAL INFILE` handler** — `LOAD DATA LOCAL INFILE` has nothing to
//!    read the file with, so it cannot exfiltrate one.
//! 4. **A row and time bound**, because "SELECT-only" says nothing about
//!    `SELECT * FROM wp_posts` on a real site: an unbounded result is a way to
//!    exhaust memory, and an unbounded query is a way to hold a connection.
//!
//! Note what is NOT on that list: parsing the SQL to check it starts with
//! `SELECT`. A parser is a thing that can be wrong, and it would sit in front of
//! a server that already enforces the answer correctly. The privilege IS the
//! check.

use crate::error::{Error, Result};
use mysql_async::prelude::*;
use mysql_async::{Opts, OptsBuilder, Row, Value};
use std::time::Duration;

/// The most rows one agent call returns. Beyond this the result is TRUNCATED
/// and says so, rather than silently returning a prefix — an agent that cannot
/// tell a complete answer from a clipped one will report the clipped one as
/// complete.
pub const MAX_ROWS: usize = 500;

/// The wall-clock bound on one query, connect included. A read-only principal
/// can still write a join that never finishes; this is what stops that from
/// being a stuck MCP session rather than an error.
pub const QUERY_TIMEOUT: Duration = Duration::from_secs(15);

/// One query's result, shaped for the wire.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResult {
    pub columns: Vec<String>,
    /// Every value rendered as a string or null. Deliberately lossy: an agent
    /// reads these, and a schema-faithful JSON encoding of MySQL's type zoo
    /// (`DECIMAL`, `BIT`, binary blobs, zero dates) is a large surface to be
    /// subtly wrong in for no gain at the point of use.
    pub rows: Vec<Vec<Option<String>>>,
    /// True when the result hit [`MAX_ROWS`]. Named `truncated` rather than
    /// inferred from `rows.len() == MAX_ROWS`, because a query that returns
    /// exactly 500 rows is complete and must not be reported as clipped.
    pub truncated: bool,
}

/// Connection options for one agent principal.
///
/// Loopback TCP, no password, one database — matching what `agent_db`
/// provisioned. Split out from [`run_query`] so the flags that make this path
/// safe are readable and assertable in one place rather than buried in a call.
fn agent_opts(port: u16, user: &str, db: &str) -> Opts {
    OptsBuilder::default()
        .ip_or_hostname("127.0.0.1")
        .tcp_port(port)
        .user(Some(user))
        .pass(None::<String>)
        .db_name(Some(db))
        // No handler = `LOAD DATA LOCAL INFILE` has nothing to read a file
        // with. It is the driver's default and is stated anyway, because a
        // default that carries a security guarantee should be visible at the
        // place that depends on it rather than found in a changelog.
        // The turbofish names a concrete handler type ONLY because `None`
        // alone is ambiguous; nothing here installs one. `WhiteListFsHandler`
        // is the driver's own type and it is not constructed.
        .local_infile_handler(None::<mysql_async::WhiteListFsHandler>)
        // The statement cache is off: an agent's SQL is ad-hoc and different
        // every time, so a cache keyed by statement text only retains other
        // people's data in this process for no hit rate.
        .stmt_cache_size(0)
        .into()
}

/// Run ONE statement as one agent principal and return at most [`MAX_ROWS`]
/// rows.
///
/// This function is the entire agent query path. It takes a principal name
/// rather than deriving one, so the caller — which is the thing that checked
/// the grant — decides who this runs as, and there is no arm here that could
/// pick root.
pub async fn run_query(port: u16, user: &str, db: &str, sql: &str) -> Result<QueryResult> {
    let fut = async {
        let pool = mysql_async::Pool::new(agent_opts(port, user, db));
        let mut conn = pool.get_conn().await.map_err(|e| {
            Error::Other(format!("the agent principal {user:?} could not connect: {e}"))
        })?;
        // `query_iter` sends ONE statement. With CLIENT_MULTI_STATEMENTS
        // unset (never enabled above), a `;`-separated second statement is a
        // protocol error rather than a second query.
        let mut result = conn.query_iter(sql).await.map_err(|e| Error::Other(e.to_string()))?;
        let columns: Vec<String> = result
            .columns()
            .map(|c| c.iter().map(|c| c.name_str().to_string()).collect())
            .unwrap_or_default();

        let mut rows: Vec<Vec<Option<String>>> = Vec::new();
        let mut truncated = false;
        // One more row than the cap is fetched on purpose: it is the only way
        // to distinguish "exactly MAX_ROWS rows exist" from "there are more",
        // and reporting the first as truncated would be a lie in the direction
        // that makes an agent doubt a complete answer.
        while let Some(row) = result.next().await.map_err(|e| Error::Other(e.to_string()))? {
            if rows.len() == MAX_ROWS {
                truncated = true;
                break;
            }
            rows.push(render_row(&row));
        }
        drop(result);
        drop(conn);
        pool.disconnect().await.ok();
        Ok::<_, Error>(QueryResult { columns, rows, truncated })
    };

    match tokio::time::timeout(QUERY_TIMEOUT, fut).await {
        Ok(r) => r,
        Err(_) => Err(Error::Other(format!(
            "the query exceeded the {}s agent bound and was abandoned",
            QUERY_TIMEOUT.as_secs()
        ))),
    }
}

/// Render one row as strings/nulls. `Bytes` that are not UTF-8 become a length
/// note rather than lossy text: an agent shown mojibake would reason about it
/// as if it were the data.
fn render_row(row: &Row) -> Vec<Option<String>> {
    (0..row.len())
        .map(|i| match row.as_ref(i) {
            None | Some(Value::NULL) => None,
            Some(Value::Bytes(b)) => Some(match std::str::from_utf8(b) {
                Ok(s) => s.to_string(),
                Err(_) => format!("<{} bytes of binary>", b.len()),
            }),
            Some(v) => Some(v.as_sql(true).trim_matches('\'').to_string()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The agent query path never reaches the bundled client.**
    ///
    /// This is §3.6's hole 1 as a test rather than a comment. The danger is not
    /// that someone writes `mysql -e "system id"` on purpose — it is that the
    /// obvious way to add a feature here is to reuse `database::mysql_exec`,
    /// which already exists, already works, and would hand a read-only
    /// principal shell-exec and file-write on a real site.
    ///
    /// A source guard, because the property is "this code does not call that
    /// code" and no runtime test can observe a call that was never made. Comment
    /// lines are stripped first, so this file's own prose about
    /// `client_base_args` — which is the reason the module exists — does not
    /// make the guard fire on itself. That trap has caught this repo before.
    #[test]
    fn the_agent_query_path_never_calls_the_bundled_client() {
        let src = crate::core::copy_scan::production_source(include_str!("agent_query.rs"));
        for forbidden in ["client_base_args", "mysql_exec", "SqlClient", "Command::new"] {
            assert!(
                !src.contains(forbidden),
                "the agent query path reached {forbidden} — §3.6 hole 1: the bundled client \
                 interprets `system`/`\\!`/`source`/`tee` before the server sees a statement, \
                 so a GRANT SELECT principal fed through it has shell-exec and file-write"
            );
        }
    }

    /// The three connection flags this path's safety rests on, asserted where a
    /// change to them has to walk past an explanation.
    #[test]
    fn agent_connections_are_loopback_passwordless_and_cannot_read_local_files() {
        let opts = agent_opts(13306, "rex_ro_shop_rex", "wp_shop");
        assert_eq!(opts.ip_or_hostname(), "127.0.0.1", "an agent connection left loopback");
        assert_eq!(opts.tcp_port(), 13306);
        assert_eq!(opts.user(), Some("rex_ro_shop_rex"));
        assert_eq!(opts.pass(), None, "a password here is a secret with nowhere to live");
        assert_eq!(opts.db_name(), Some("wp_shop"));
        assert!(
            opts.local_infile_handler().is_none(),
            "a LOCAL INFILE handler makes `LOAD DATA LOCAL INFILE` an exfiltration path"
        );
        assert_eq!(opts.stmt_cache_size(), 0);
    }

    /// `truncated` must mean "there is more", not "the result is exactly the
    /// cap" — the two differ by one row and an agent acts differently on each.
    #[test]
    fn the_row_cap_distinguishes_a_full_result_from_a_clipped_one() {
        // The distinction lives in the loop's ordering: the cap is checked
        // BEFORE pushing, so the flag is only set once a row beyond the cap has
        // actually arrived from the server.
        let src = crate::core::copy_scan::production_source(include_str!("agent_query.rs"));
        let check = src.find("if rows.len() == MAX_ROWS").expect("the cap check");
        let push = src.find("rows.push(render_row").expect("the push");
        assert!(check < push, "the cap is checked after the push — a full result reads as clipped");
        assert!(
            !src.contains("rows.len() >= MAX_ROWS && truncated"),
            "truncation must not be inferred from the row count"
        );
    }
}

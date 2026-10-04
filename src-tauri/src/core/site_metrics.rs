//! core::site_metrics — honest per-site resource attribution (Sites page).
//!
//! Sites do NOT map 1:1 to processes here: default sites share one nginx and
//! one php-fpm pool per PHP version, so a real per-site CPU/RAM number only
//! exists for FrankenPHP-override sites (their own backend process). For
//! shared sites we report ACTIVITY — requests + bytes per host from the shared
//! nginx access log (`log_format rexenv '$host $time_iso8601 $bytes'`) — and
//! the site's MySQL database disk size. No fabricated per-site CPU/RAM.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// One site's numbers for the Sites page, explicit about what each IS:
/// - FrankenPHP-override sites: REAL CPU/RAM from their own backend's process
///   tree (same `Monitor::tree` source as the Services rows / footer).
/// - Every site with a database: its REAL disk size, from its own engine.
/// - Shared sites: ACTIVITY (requests + bytes over the last 60s window, from
///   the shared nginx access log) — never a fabricated per-site CPU/RAM.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteResources {
    pub id: String,
    pub domain: String,
    /// True for a FrankenPHP-override site (own process → real CPU/RAM);
    /// false ⇒ shared nginx + pool (activity metrics only).
    pub dedicated: bool,
    pub cpu_percent: Option<f32>,
    pub ram_mb: Option<u64>,
    /// Requests / sent bytes in the last 60s (shared nginx sites only —
    /// override sites bypass nginx).
    pub requests_per_min: Option<u64>,
    pub bytes_per_min: Option<u64>,
    /// Database size in bytes (`None`: no DB, its engine not running, or not asked).
    pub db_size_bytes: Option<u64>,
}

/// Every site's resources — the ONE computation behind the Sites page
/// (`commands::sites::sites_resources`) and the MCP `site_info` read.
///
/// `include_db_sizes` is the one difference, and it is a promise rather than a
/// preference: the sizes come from running a database CLIENT per engine, and the
/// MCP read tools run nothing. The agent's read passes `false` and gets every
/// other number; `db_query` is where it asks a database a question.
///
/// In core rather than `commands::sites` (moved 11 Sep 2026) because the MCP
/// read bridge may reach `core::` and never `commands::`.
pub fn resources_of(
    state: &crate::state::app::AppState,
    include_db_sizes: bool,
) -> crate::error::Result<Vec<SiteResources>> {
    use crate::core::db::DbEngine;
    use crate::error::Error;
    use crate::state::models::WebServer;

    let sites = {
        let conn = state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))?;
        crate::core::sites::list(&conn)?
    };

    // FrankenPHP override backends (domain → pid). try_lock: during a long
    // start/stop just omit the dedicated numbers for one poll.
    let override_pids: HashMap<String, u32> = state
        .services
        .try_lock()
        .map(|mgr| mgr.override_pids().into_iter().collect())
        .unwrap_or_default();

    // Activity per host from the shared nginx access log (last 60s).
    let access_log = state.platform.paths().log_dir()?.join("nginx-access.log");
    let activity = activity_by_host(&access_log, OffsetDateTime::now_utc());

    // DB sizes: one query per RUNNING site engine — resolved strictly from the
    // already-published cache (never a download from a status poll). Kept as
    // one map per engine: the same db name could exist in more than one, and a
    // site must read its own engine's number.
    //
    // The engine list is ASKED for (`hosts_site_databases`) rather than spelled.
    // It was a literal `[Mysql, Mariadb]`, so when PostgreSQL became a site
    // engine every PG-backed site's size silently read as "—" on the Sites page:
    // no error, no log line, just a number that is never there.
    let db_sizes: HashMap<&'static str, HashMap<String, u64>> = if !include_db_sizes {
        HashMap::new()
    } else {
        DbEngine::ALL
            .into_iter()
            .filter(|e| e.hosts_site_databases())
            .filter(|e| e.running())
            .filter_map(|e| {
                let version = {
                    let conn = state.db.lock().ok()?;
                    e.effective_version(&conn)
                };
                let client = e.cached_sql_client(state.platform.as_ref(), &version)?;
                let sizes = e.db_sizes(&client, e.port()).ok()?;
                Some((e.key(), sizes.into_iter().collect()))
            })
            .collect()
    };

    let mut monitor = state
        .monitor
        .lock()
        .map_err(|_| Error::Other("monitor lock poisoned".into()))?;
    monitor.refresh_processes();

    Ok(sites
        .into_iter()
        .map(|s| {
            let tree = override_pids.get(&s.domain).and_then(|pid| monitor.tree(*pid));
            let act = activity.get(&s.domain);
            SiteResources {
                dedicated: s.web_server != WebServer::Nginx,
                cpu_percent: tree.map(|t| t.cpu_percent),
                ram_mb: tree.map(|t| t.ram_mb),
                requests_per_min: act.map(|a| a.requests),
                bytes_per_min: act.map(|a| a.bytes),
                db_size_bytes: db_sizes
                    .get(DbEngine::from_site(s.db_engine).key())
                    .and_then(|m| m.get(&s.db_name))
                    .copied(),
                id: s.id,
                domain: s.domain,
            }
        })
        .collect())
}

/// Attribution window for "per minute" activity numbers.
pub const ACTIVITY_WINDOW_SECS: u64 = 60;

/// How much of the access-log tail is scanned per poll. 512KB ≈ tens of
/// thousands of the minimal rexenv-format lines — far more than a minute of
/// local-dev traffic; bounded so a giant log can't stall a poll.
const TAIL_BYTES: u64 = 512 * 1024;

/// One host's activity inside the window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SiteActivity {
    pub requests: u64,
    pub bytes: u64,
}

/// Parse `line` in the rexenv access format (`host time_iso8601 bytes`);
/// non-matching lines (old combined-format history) are skipped.
fn parse_line(line: &str) -> Option<(&str, OffsetDateTime, u64)> {
    let mut it = line.split_whitespace();
    let host = it.next()?;
    let ts = OffsetDateTime::parse(it.next()?, &Rfc3339).ok()?;
    let bytes = it.next()?.parse().ok()?;
    Some((host, ts, bytes))
}

/// Per-host activity within the last [`ACTIVITY_WINDOW_SECS`] before `now`,
/// from the TAIL of the shared nginx access log. Missing log ⇒ empty (stack
/// not started yet) — never an error.
pub fn activity_by_host(access_log: &Path, now: OffsetDateTime) -> HashMap<String, SiteActivity> {
    let mut out = HashMap::new();
    let Ok(mut f) = std::fs::File::open(access_log) else {
        return out;
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(TAIL_BYTES);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return out;
    }
    let mut buf = String::new();
    if f.read_to_string(&mut buf).is_err() {
        return out; // non-UTF8 chunk — skip this poll rather than fail it
    }
    let cutoff = now - time::Duration::seconds(ACTIVITY_WINDOW_SECS as i64);
    // Skip the first (possibly truncated) line when we started mid-file.
    let lines = buf.lines().skip(if start > 0 { 1 } else { 0 });
    for line in lines {
        if let Some((host, ts, bytes)) = parse_line(line) {
            if ts >= cutoff && ts <= now + time::Duration::seconds(5) {
                let e = out.entry(host.to_string()).or_insert(SiteActivity::default());
                e.requests += 1;
                e.bytes += bytes;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn parses_rexenv_lines_and_skips_combined_history() {
        let now = datetime!(2026-07-07 20:21:00 +6);
        let dir = std::env::temp_dir().join("rexenv-sitemetrics-test");
        let _ = std::fs::create_dir_all(&dir);
        let log = dir.join("access.log");
        std::fs::write(
            &log,
            "127.0.0.1 - - [07/Jul/2026:20:20:40 +0600] \"POST /x HTTP/1.1\" 200 58 \"-\" \"-\"\n\
             blog.test 2026-07-07T20:20:40+06:00 1000\n\
             blog.test 2026-07-07T20:20:50+06:00 500\n\
             shakib.test 2026-07-07T20:20:55+06:00 250\n\
             blog.test 2026-07-07T20:19:00+06:00 9999\n", // outside the 60s window
        )
        .unwrap();
        let act = activity_by_host(&log, now);
        assert_eq!(act["blog.test"], SiteActivity { requests: 2, bytes: 1500 });
        assert_eq!(act["shakib.test"], SiteActivity { requests: 1, bytes: 250 });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_log_is_empty_not_an_error() {
        let act = activity_by_host(Path::new("/nonexistent/access.log"), OffsetDateTime::now_utc());
        assert!(act.is_empty());
    }
}

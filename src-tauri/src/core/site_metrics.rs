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
             tr.test 2026-07-07T20:20:40+06:00 1000\n\
             tr.test 2026-07-07T20:20:50+06:00 500\n\
             shakib.test 2026-07-07T20:20:55+06:00 250\n\
             tr.test 2026-07-07T20:19:00+06:00 9999\n", // outside the 60s window
        )
        .unwrap();
        let act = activity_by_host(&log, now);
        assert_eq!(act["tr.test"], SiteActivity { requests: 2, bytes: 1500 });
        assert_eq!(act["shakib.test"], SiteActivity { requests: 1, bytes: 250 });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_log_is_empty_not_an_error() {
        let act = activity_by_host(Path::new("/nonexistent/access.log"), OffsetDateTime::now_utc());
        assert!(act.is_empty());
    }
}

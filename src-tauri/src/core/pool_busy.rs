//! core::pool_busy — "PHP 8.3: all 10 workers busy — requests are queuing" (plan §3 D1(b),
//! ledger #608).
//!
//! A pool whose every worker holds a request makes the next one WAIT in the listen backlog: the
//! user sees a slow page, then nginx's 504, which reads as "rexenv is slow". php-fpm writes
//! "server reached pm.max_children" to a log nobody reads and php-cgi writes nothing, so the
//! Services row says it instead, and the health log records it.
//!
//! The count is the ESTABLISHED connections on the pool's port
//! (`ProcessSupervisor::established_on`) against the pool's worker count
//! (`php::pool_workers`). Measured 14 Sep 2026 (`examples/pool_get_values_probe.rs`, 12 requests
//! on 10 workers): the Dell's table counted 12 (the queued two included), macOS `lsof` 10 once
//! php-fpm had spawned every worker (accepted connections only) — so "at least the worker count"
//! means every worker is holding one on both. One sample is a moment: it takes
//! [`SAMPLES_TO_CHANGE`] in a row to turn the note on, and as many to turn it off, so a burst does
//! not flicker the row.

use std::collections::HashMap;

/// Consecutive samples that agree before the note turns on or off.
pub const SAMPLES_TO_CHANGE: u32 = 2;

/// The Services row's words for a busy pool.
pub fn note(workers: u32) -> String {
    format!("all {workers} workers busy — requests are queuing")
}

/// What one observation changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    None,
    /// Busy for [`SAMPLES_TO_CHANGE`] samples in a row; `held` is the count that made it so.
    Busy { held: usize },
    /// Below the worker count for [`SAMPLES_TO_CHANGE`] samples in a row.
    Free,
}

#[derive(Debug, Default)]
struct Row {
    busy: bool,
    streak: u32,
}

/// Per-pool busy state across status polls, keyed by the pool's service name.
#[derive(Debug, Default)]
pub struct BusyTracker {
    rows: HashMap<String, Row>,
}

impl BusyTracker {
    /// Feed one sample for `pool`: `held` connections against `workers`. An unreadable count
    /// (`None`) changes nothing — it is neither evidence of busy nor of free.
    pub fn observe(&mut self, pool: &str, held: Option<usize>, workers: u32) -> Change {
        let Some(held) = held else {
            return Change::None;
        };
        let row = self.rows.entry(pool.to_string()).or_default();
        let sample = held >= workers as usize;
        if sample == row.busy {
            row.streak = 0;
            return Change::None;
        }
        row.streak += 1;
        if row.streak < SAMPLES_TO_CHANGE {
            return Change::None;
        }
        row.busy = sample;
        row.streak = 0;
        if sample {
            Change::Busy { held }
        } else {
            Change::Free
        }
    }

    /// Whether `pool` currently reads busy.
    pub fn is_busy(&self, pool: &str) -> bool {
        self.rows.get(pool).is_some_and(|r| r.busy)
    }

    /// Forget every pool not in `running`: a stopped pool is not busy, and its next start must not
    /// inherit a note or half a streak.
    pub fn retain_running(&mut self, running: &[String]) {
        self.rows.retain(|pool, _| running.iter().any(|r| r == pool));
    }
}

/// The hosts of the most recent requests in the tail of nginx's access log
/// (`$host $time_iso8601 $body_bytes_sent`, `services::generate_nginx_config`), newest first, each
/// once, at most `max`. These are COMPLETED requests — nginx writes the line when a request ends,
/// so the ones holding the workers right now are not in it yet — which is why the health log calls
/// them "recently served", never the cause. The tail's first line may be cut, so it is not read.
pub fn recent_hosts(tail: &str, max: usize) -> Vec<String> {
    let mut hosts: Vec<String> = Vec::new();
    for line in tail.lines().skip(1).collect::<Vec<_>>().into_iter().rev() {
        let Some(host) = line.split_whitespace().next() else {
            continue;
        };
        if !hosts.iter().any(|h| h == host) {
            hosts.push(host.to_string());
        }
        if hosts.len() == max {
            break;
        }
    }
    hosts
}

/// The health log's line for a pool that turned busy.
pub fn busy_detail(workers: u32, held: usize, recent: &[String]) -> String {
    let mut line = format!("all {workers} workers busy ({held} connections held) — requests are queuing");
    if !recent.is_empty() {
        line.push_str(&format!("; recently served: {}", recent.join(", ")));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    const POOL: &str = "PHP-FPM 8.3";

    /// The measured sequences (14 Sep 2026): the Dell's group counted 12 of 10 from the first
    /// sample; the Mac's php-fpm counted 4 while spawning, then 10. Two agreeing samples turn the
    /// note on, two turn it off, one disagreeing sample in between resets the streak.
    #[test]
    fn a_pool_turns_busy_after_two_samples_at_the_worker_count_and_free_after_two_below() {
        let mut t = BusyTracker::default();
        assert_eq!(t.observe(POOL, Some(12), 10), Change::None, "one sample is a moment");
        assert_eq!(t.observe(POOL, Some(12), 10), Change::Busy { held: 12 });
        assert!(t.is_busy(POOL));
        assert_eq!(t.observe(POOL, Some(12), 10), Change::None, "already busy");

        assert_eq!(t.observe(POOL, Some(2), 10), Change::None);
        assert_eq!(t.observe(POOL, Some(0), 10), Change::Free);
        assert!(!t.is_busy(POOL));

        // The Mac's ramp: 4 is not busy; 10 twice is.
        let mut mac = BusyTracker::default();
        assert_eq!(mac.observe(POOL, Some(4), 10), Change::None);
        assert_eq!(mac.observe(POOL, Some(10), 10), Change::None);
        assert_eq!(mac.observe(POOL, Some(10), 10), Change::Busy { held: 10 }, "exactly the worker count is every worker");

        // A flicker never completes a streak.
        let mut f = BusyTracker::default();
        for held in [10, 5, 10, 5, 10] {
            assert_eq!(f.observe(POOL, Some(held), 10), Change::None, "held {held}");
        }
        assert!(!f.is_busy(POOL));
    }

    #[test]
    fn an_unreadable_count_changes_nothing_and_advances_no_streak() {
        let mut t = BusyTracker::default();
        assert_eq!(t.observe(POOL, Some(10), 10), Change::None);
        assert_eq!(t.observe(POOL, None, 10), Change::None);
        assert_eq!(t.observe(POOL, Some(10), 10), Change::Busy { held: 10 }, "None neither broke nor counted");
        assert_eq!(t.observe(POOL, None, 10), Change::None);
        assert!(t.is_busy(POOL), "an unreadable sample is not evidence of free");
    }

    #[test]
    fn a_stopped_pool_is_forgotten_and_its_next_start_begins_clean() {
        let mut t = BusyTracker::default();
        t.observe(POOL, Some(10), 10);
        t.observe(POOL, Some(10), 10);
        t.observe("PHP-FPM 8.4", Some(10), 10);
        t.retain_running(&["PHP-FPM 8.4".to_string()]);
        assert!(!t.is_busy(POOL));
        assert_eq!(t.observe(POOL, Some(10), 10), Change::None, "no inherited state");
        assert_eq!(t.observe("PHP-FPM 8.4", Some(10), 10), Change::Busy { held: 10 }, "the running pool kept its streak");
    }

    #[test]
    fn recent_hosts_are_newest_first_unique_capped_and_skip_the_cut_first_line() {
        let tail = "e.rex 2026-09-14T10:00:00+06:00 5\n\
                    a.rex 2026-09-14T10:00:01+06:00 10\n\
                    b.rex 2026-09-14T10:00:02+06:00 10\n\
                    a.rex 2026-09-14T10:00:03+06:00 10\n\
                    c.rex 2026-09-14T10:00:04+06:00 10\n";
        assert_eq!(recent_hosts(tail, 2), vec!["c.rex", "a.rex"]);
        assert_eq!(recent_hosts(tail, 10), vec!["c.rex", "a.rex", "b.rex"], "the first, possibly cut, line is not read");
        assert!(recent_hosts("", 3).is_empty());
    }

    #[test]
    fn the_words_say_how_many_and_what_is_recent_not_the_cause() {
        assert_eq!(note(10), "all 10 workers busy — requests are queuing");
        let line = busy_detail(10, 12, &["c.rex".into(), "a.rex".into()]);
        assert!(line.starts_with("all 10 workers busy (12 connections held)"), "{line}");
        assert!(line.ends_with("; recently served: c.rex, a.rex"), "{line}");
        assert!(!busy_detail(10, 10, &[]).contains("recently"));
    }
}

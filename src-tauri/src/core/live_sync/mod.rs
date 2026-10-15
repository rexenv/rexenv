//! WordPress live ↔ local sync (`docs/PLAN-wp-live-sync.md`) — the client half
//! of the rexsync1 protocol (`docs/rexsync-protocol.md`).
//!
//! `sign` (the key and request signing, checked against the vectors the plugin's
//! tests read too), `client` (the HTTP side), `pull`/`push` (the two jobs' cores),
//! `base` (what both sides looked like at the last sync, and the history),
//! `stamps` (the local copy's table stamps), `status` (the verdict: up to date,
//! behind, ahead, diverged), `secrets` (the owner-only pairing file), `plugin_zip`
//! (the plugin itself, for "Download plugin"), `anonymise` (the pull's scrub rules),
//! `uploads_on_demand` (the mu-plugin that fetches a missing upload from live),
//! `progress` (what an interrupted pull or push already moved, for the next one to
//! continue from). The IPC edge is `commands::live_sync`.

pub mod sign;

pub mod client;
pub mod pull;
pub mod secrets;
pub mod base;
pub mod push;
pub mod plugin_zip;
pub mod stamps;
pub mod status;
pub mod anonymise;
pub mod uploads_on_demand;
pub mod progress;

/// `"1 row"`, `"2 rows"` — the job log's counts. The Ubuntu run of the SMOKE rows
/// (11 Oct 2026) read "wpcs_terms — 1 rows" and "1 files in the quarantine" in
/// the log the toast had just been fixed for. Pure.
pub fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

#[cfg(test)]
mod tests {
    use super::count;

    #[test]
    fn a_count_of_one_is_singular() {
        assert_eq!(count(1, "row", "rows"), "1 row");
        assert_eq!(count(0, "row", "rows"), "0 rows");
        assert_eq!(count(2, "file", "files"), "2 files");
    }
}

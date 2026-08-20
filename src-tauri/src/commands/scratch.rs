//! The scratch reaper — rexenv collecting its own disposable sites (MCP M2a).
//!
//! Runs UNATTENDED, which is what shapes every decision here.
//!
//! **Blast radius, stated honestly.** The witness `due_for_reap` hands back is
//! minted from the same predicate, so it is a type, not a second opinion. What
//! is genuinely independent is the teardown: `delete_site_owned` removes a
//! docroot only where `docroot_managed == Some(true)` and drops a database only
//! under the recorded provenance rules, both checked where the deletion happens.
//! So: **nothing outside what rexenv created is deleted, and that holds
//! independently of the predicate** — not "nothing is deleted". A site rexenv
//! made and the user later adopted is the real loss case, which is what the
//! per-sweep ceiling below is for.
//!
//! **Skip, never stop.** A scratch site that is publicly shared is SKIPPED, not
//! stopped-then-deleted. `delete_site_owned`'s first act is to stop the site's
//! tunnel, and ledger #29 says rexenv never stops a share on the user's behalf —
//! a reaper that called the normal delete path would break that invariant
//! unattended, which is exactly what the tunnel work exists to prevent. A failed
//! stop is a skip too, by construction: nothing is deleted while anything is
//! still serving it publicly.

use crate::commands::tunnels::Tunnels;
use crate::mcp_server::feed::{self, Outcome};
use crate::state::app::AppState;

/// Most sites one sweep may delete. A wrong predicate then costs N sites and a
/// loud log rather than everything it selected — the difference between a bug
/// and an incident. Deliberately the same as the scratch cap: a correct sweep
/// can never need more.
pub(crate) const MAX_PER_SWEEP: usize = crate::core::scratch::MAX_SCRATCH_SITES;

/// What one sweep did — the caller turns this into the user-visible summary.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct SweepOutcome {
    /// Domains actually removed.
    pub deleted: Vec<String>,
    /// Domains left alone because they are publicly shared right now.
    pub skipped_shared: Vec<String>,
    /// Sites whose deletion failed (recorded once per distinct reason).
    pub failed: Vec<String>,
    /// True when more sites were due than the ceiling allows.
    pub hit_ceiling: bool,
}

impl SweepOutcome {
    /// Whether this sweep is worth telling the user about — so a user with no
    /// scratch sites never learns the reaper exists.
    pub fn did_anything(&self) -> bool {
        !self.deleted.is_empty() || !self.skipped_shared.is_empty() || !self.failed.is_empty()
    }
}

/// Collect every scratch site whose clock has run out.
///
pub(crate) async fn reap_expired(state: &AppState, tunnels: &Tunnels) -> SweepOutcome {
    let mut out = SweepOutcome::default();
    let (now, due) = {
        let Ok(conn) = state.db.lock() else { return out };
        let Ok(now) = crate::state::store::db_now(&conn) else { return out };
        match crate::core::scratch::due_for_reap(&conn, &now) {
            Ok(due) => (now, due),
            Err(e) => {
                log::warn!("scratch reaper: could not read expired sites: {e}");
                return out;
            }
        }
    };
    let _ = now;
    if due.len() > MAX_PER_SWEEP {
        // Loud, because reaching here means either a user with many expired
        // scratch sites or a predicate selecting things it should not.
        log::warn!(
            "scratch reaper: {} sites are due but this sweep will remove at most {MAX_PER_SWEEP} \
             — refusing to delete more in one pass. If this repeats, check the reap predicate \
             before anything else.",
            due.len()
        );
        out.hit_ceiling = true;
    }

    for scratch in due.into_iter().take(MAX_PER_SWEEP) {
        let domain = scratch.domain().to_string();
        // SKIP, never stop — see the module doc (#29).
        if tunnels.sharing_domain(state, &domain) {
            out.skipped_shared.push(domain.clone());
            record(state, scratch.id(), Outcome::Error, "expired, but still shared publicly — not deleted");
            continue;
        }
        match crate::commands::sites::delete_site_owned(state, tunnels, scratch.id().to_string())
            .await
        {
            Ok(_) => {
                record(state, scratch.id(), Outcome::Ok, &format!("{domain} expired and was removed"));
                out.deleted.push(domain);
            }
            Err(e) => {
                record(state, scratch.id(), Outcome::Error, &e.to_string());
                out.failed.push(domain);
            }
        }
    }
    out
}

/// Record what the reaper did — deduped, so a site that cannot be deleted says
/// so ONCE rather than once per launch (`feed::record_reap`).
fn record(state: &AppState, id: &str, outcome: Outcome, detail: &str) {
    let Ok(conn) = state.db.lock() else { return };
    if let Err(e) = feed::record_reap(&conn, id, outcome, Some(detail.to_string())) {
        log::warn!("scratch reaper: could not record the outcome for {id}: {e}");
    }
}

// The production items below this module stay where they are. `summary` reads
// as the continuation of the sweep it formats, and hoisting ~200 lines above the
// tests to satisfy a lint would produce a diff nobody can review against a file
// whose ordering is deliberate.
#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sweep_ceiling_is_the_scratch_cap_so_a_correct_sweep_never_needs_more() {
        // A correct sweep can never have more due than the cap allows to exist,
        // so exceeding it is evidence about the PREDICATE, not about the user.
        assert_eq!(MAX_PER_SWEEP, crate::core::scratch::MAX_SCRATCH_SITES);
    }

    #[test]
    fn the_summary_names_the_sites_and_a_quiet_launch_says_nothing() {
        // Naming beats counting: a user recognises a domain they cared about and
        // can act on it, where "3 sites" only says something is gone.
        assert_eq!(summary(&SweepOutcome::default()), None, "a quiet launch is silent");
        let out = SweepOutcome {
            deleted: vec!["probe.scratch.rex".into(), "plugin-test.scratch.rex".into()],
            ..Default::default()
        };
        let text = summary(&out).unwrap();
        assert!(text.contains("`probe.scratch.rex` and `plugin-test.scratch.rex`"), "{text}");
        assert!(text.contains("removed 2 expired scratch sites"), "{text}");
        assert!(text.contains("Nothing of yours was touched"), "{text}");
        assert!(text.contains("press Keep"), "names the way to prevent it: {text}");

        // The shared case reads as left-alone, never as failed.
        let shared = SweepOutcome {
            skipped_shared: vec!["demo.scratch.rex".into()],
            ..Default::default()
        };
        let text = summary(&shared).unwrap();
        assert!(text.contains("still shared publicly") && text.contains("left it alone"), "{text}");
        assert!(!text.contains("removed"), "nothing was removed: {text}");
    }

    #[test]
    fn a_sweep_that_did_nothing_says_nothing() {
        // The launch summary must not appear on every launch — a user with no
        // scratch sites should never learn the reaper exists.
        assert!(!SweepOutcome::default().did_anything());
        let mut swept = SweepOutcome::default();
        swept.skipped_shared.push("probe.scratch.rex".into());
        assert!(swept.did_anything(), "a skip IS worth surfacing — it is a site that outlived its clock");
    }
}

/// The user-visible summary, in the approved words. `None` when the sweep did
/// nothing — a quiet launch says nothing at all.
///
/// It NAMES the domains rather than only counting them: a user recognises a name
/// they cared about and can act on it, where "3 sites" only tells them something
/// is gone. And it says what was NOT touched, which is a claim the code backs —
/// the teardown checks `docroot_managed` and the recorded database provenance
/// where the deletion happens, independently of the reap predicate.
pub(crate) fn summary(out: &SweepOutcome) -> Option<String> {
    if !out.did_anything() {
        return None;
    }
    let mut parts = Vec::new();
    if !out.deleted.is_empty() {
        parts.push(format!(
            "rexenv removed {} expired scratch site{}\n{} {} created by an AI agent and hadn't \
             been used for a while, so rexenv cleaned {} up. Nothing of yours was touched.\n\
             To stop this happening to one you want, open it and press Keep — that makes it yours.",
            out.deleted.len(),
            if out.deleted.len() == 1 { "" } else { "s" },
            list(&out.deleted),
            if out.deleted.len() == 1 { "was" } else { "were" },
            if out.deleted.len() == 1 { "it" } else { "them" },
        ));
    }
    if !out.skipped_shared.is_empty() {
        parts.push(format!(
            "{} expired scratch site{} still shared publicly\n{} ran out {} clock, but you're \
             sharing {}, so rexenv left {} alone. Stop sharing and {} be cleaned up next time — \
             or press Keep to have {} for good.",
            out.skipped_shared.len(),
            if out.skipped_shared.len() == 1 { " is" } else { "s are" },
            list(&out.skipped_shared),
            if out.skipped_shared.len() == 1 { "its" } else { "their" },
            if out.skipped_shared.len() == 1 { "it" } else { "them" },
            if out.skipped_shared.len() == 1 { "it" } else { "them" },
            if out.skipped_shared.len() == 1 { "it'll" } else { "they'll" },
            if out.skipped_shared.len() == 1 { "it" } else { "them" },
        ));
    }
    Some(parts.join("\n\n"))
}

/// `a`, `b` and `c` — the domains, read the way a person would say them.
fn list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => format!("`{one}`"),
        [rest @ .., last] => format!(
            "{} and `{last}`",
            rest.iter().map(|d| format!("`{d}`")).collect::<Vec<_>>().join(", ")
        ),
    }
}

/// Sweep now, then every hour, for as long as the app runs.
///
/// The launch sweep is what collects sites that expired while rexenv was closed
/// — the week-away case — so its summary is the one that matters most. Every
/// deletion is ALSO a feed row (`actor='rexenv'`, #205), so the record survives
/// a dismissed banner.
pub(crate) fn spawn(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        use tauri::{Emitter, Manager};
        loop {
            let (Some(state), Some(tunnels)) =
                (app.try_state::<AppState>(), app.try_state::<Tunnels>())
            else {
                return; // shutting down
            };
            let out = reap_expired(state.inner(), tunnels.inner()).await;
            if let Some(text) = summary(&out) {
                log::info!("scratch reaper: {text}");
                // The Sites/agents UI renders this as a dismissible banner; the
                // feed rows are the durable record either way.
                let _ = app.emit("scratch-reaped", &text);
            }
            tokio::time::sleep(std::time::Duration::from_secs(60 * 60)).await;
        }
    });
}

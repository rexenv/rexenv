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
    /// Whether this sweep is worth telling the user about. Used by the launch
    /// summary, which lands with its approved copy (see `reap_expired`).
    #[allow(dead_code)]
    pub fn did_anything(&self) -> bool {
        !self.deleted.is_empty() || !self.skipped_shared.is_empty() || !self.failed.is_empty()
    }
}

/// Collect every scratch site whose clock has run out.
///
/// NOT WIRED YET, deliberately: turning the sweep on at launch without the
/// user-visible summary is the silent-bulk-delete experience — a user back from
/// a week away would find several sites simply gone, with feed rows as the only
/// record. The summary copy is with the owner for approval; the launch + hourly
/// wiring lands with it, in the same commit. Remove this allow then.
#[allow(dead_code)]
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
#[allow(dead_code)]
fn record(state: &AppState, id: &str, outcome: Outcome, detail: &str) {
    let Ok(conn) = state.db.lock() else { return };
    if let Err(e) = feed::record_reap(&conn, id, outcome, Some(detail.to_string())) {
        log::warn!("scratch reaper: could not record the outcome for {id}: {e}");
    }
}

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
    fn a_sweep_that_did_nothing_says_nothing() {
        // The launch summary must not appear on every launch — a user with no
        // scratch sites should never learn the reaper exists.
        assert!(!SweepOutcome::default().did_anything());
        let mut swept = SweepOutcome::default();
        swept.skipped_shared.push("probe.scratch.rex".into());
        assert!(swept.did_anything(), "a skip IS worth surfacing — it is a site that outlived its clock");
    }
}

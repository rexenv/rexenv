//! Scratch sites — the agent-owned, disposable ones (MCP M2a).
//!
//! **This module is the ONE door between "a site" and "a site an agent may
//! touch."** M1's boundary was about the VERB — a read-only tool had no mutating
//! method to call ([`crate::mcp_server`]'s `ReadCtx`). M2 mutates by design, so
//! its structural half is about the OBJECT instead: every scratch mutator takes
//! a [`ScratchSite`], and the only way to obtain one is [`claim`], which reads
//! the row and checks `origin`. A code path that mutates the user's site does
//! not compile.
//!
//! The gate lives in `core`, not in the tool layer, so the CLI and the UI
//! inherit the same refusal — the tool layer is never the only thing standing
//! between an agent and the user's work (the M7 rule).
//!
//! **What the witness is NOT: a lock.** It proves the path was gated at the
//! moment it was claimed; it does not freeze `origin`, and the user can press
//! Keep a millisecond later. So the pairing this codebase uses elsewhere applies
//! here too — the witness is the compile-time proof that a path checked, and the
//! destructive WRITE re-asserts the recorded fact in its own `WHERE`
//! (`origin = 'agent'`, the shape [`crate::state::store::touch_site_expiry`]
//! already uses). Belt on the same fact from the other side; a one-time check on
//! a mutable fact is a snapshot, and this project has been bitten by treating
//! one as a guarantee.

use crate::error::{Error, Result};
use crate::state::models::{Site, SiteOrigin};
use crate::state::store;
use rusqlite::Connection;

/// Proof that a site is **agent-owned scratch**, recorded — not a name test, not
/// a path test. The private field IS the guarantee: there is no constructor, no
/// `From<Site>`, and the field is private to this module rather than
/// `pub(crate)`, so nothing anywhere in the crate can mint one from a `Site` it
/// happens to hold. [`claim`] is the only door, and it reads the row itself, so
/// a caller cannot supply a `Site` value it built or edited in memory.
#[derive(Debug, Clone)]
pub struct ScratchSite(
    // KEEP THIS FIELD PRIVATE. Attempting to build one elsewhere is a compile
    // error — and rustc helpfully suggests `pub Site` to make it go away, which
    // would delete the guarantee this type exists for and leave every scratch
    // mutator accepting the user's sites. If you arrived here from that
    // suggestion, the fix is `claim()`, not `pub`. Proven by plant-and-capture:
    // `ScratchSite(site)` → E0423, `ScratchSite { 0: site }` → E0451,
    // `site.into()` → E0277 (ledger #208).
    Site,
);

impl ScratchSite {
    /// The underlying row — read-only. Handing out `&Site` cannot launder the
    /// proof back into one: a `Site` grants nothing on its own, and going the
    /// other way still requires [`claim`].
    pub fn site(&self) -> &Site {
        &self.0
    }

    /// The stable site id, the value every scratch operation keys on.
    pub fn id(&self) -> &str {
        &self.0.id
    }

    /// The domain, for refusal copy and feed labels.
    pub fn domain(&self) -> &str {
        &self.0.domain
    }
}

/// The label every agent-created domain sits under: `<name>.scratch.<tld>`.
///
/// This is **UX, never policy** — it lets a human scanning the Sites list tell
/// at a glance which sites are disposable. Every decision reads
/// [`crate::state::models::SiteOrigin`]; a site the user hand-creates at
/// `mine.scratch.rex` is a normal site of theirs (`claim` refuses it, tested).
pub const SCRATCH_LABEL: &str = "scratch";

/// How many scratch sites may exist at once (PLAN §4.2). The answer to "an agent
/// creates twenty": it can't.
pub const MAX_SCRATCH_SITES: usize = 5;

/// Build the domain for a scratch site called `name` under `tld`.
///
/// `name` must be a SINGLE label: no dots, so an agent can neither nest
/// namespaces nor squat `scratch.<tld>` itself, and no leading/trailing dash.
/// The full domain is validated downstream by `sites::validate_domain` as well —
/// this is the shape rule, not the character rule.
pub fn scratch_domain(name: &str, tld: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Other(
            "A scratch site needs a name — a single word, like `plugin-test`.".into(),
        ));
    }
    if name.contains('.') {
        return Err(Error::Other(format!(
            "`{name}` can't be a scratch site name: use a single word with no dots (rexenv adds \
             `.{SCRATCH_LABEL}.{tld}` itself, so `plugin-test` becomes `plugin-test.{SCRATCH_LABEL}.{tld}`)."
        )));
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Err(Error::Other(format!(
            "`{name}` can't be a scratch site name: it must not start or end with a dash."
        )));
    }
    Ok(format!("{name}.{SCRATCH_LABEL}.{tld}"))
}

/// Refuse when the scratch pool is full, naming a WAY FORWARD.
///
/// A refusal that states the rule and stops is where a model starts improvising
/// — it will try a different name, then another, then reach for something else
/// entirely. So this lists the sites it can delete, by domain, and says the two
/// things it can actually do: delete one of its own, or ask the user (who can
/// Keep or remove them in rexenv). The count is the recorded fact, not an
/// estimate.
pub fn ensure_capacity(conn: &Connection) -> Result<()> {
    let mine: Vec<String> = store::list_sites(conn)?
        .into_iter()
        .filter(|s| s.is_scratch())
        .map(|s| s.domain)
        .collect();
    if mine.len() < MAX_SCRATCH_SITES {
        return Ok(());
    }
    Err(Error::Other(format!(
        "There are already {} scratch sites, which is the limit ({MAX_SCRATCH_SITES}): {}.\n\
         Delete one you no longer need with scratch_delete_site, or ask the person you're working \
         with — they can keep or remove scratch sites in rexenv under Settings → AI agents. \
         Scratch sites also expire on their own once nothing has used them for a while.",
        mine.len(),
        mine.join(", ")
    )))
}

/// The ONE conversion: read the row by id and prove it is the agent's, or refuse
/// with a sentence the agent can act on.
///
/// Both failures are deliberately **policy statements, not type errors leaking
/// into a tool result**. An agent reads these; "expected ScratchSite, found
/// Site" would tell it nothing about what to do next, and a bare "refused" would
/// invite it to retry the same call. So: name the site, name the rule, name the
/// way forward.
pub fn claim(conn: &Connection, id: &str) -> Result<ScratchSite> {
    let Some(site) = store::get_site(conn, id)? else {
        return Err(Error::Other(format!(
            "There is no site with id `{id}`. Use list_sites to see the sites that exist."
        )));
    };
    if site.origin != SiteOrigin::Agent {
        return Err(Error::Other(format!(
            "`{}` is one of your own sites, so agent tools cannot change or delete it. \
             They only work on scratch sites the agent created itself — make one with \
             scratch_create_site.",
            site.domain
        )));
    }
    Ok(ScratchSite(site))
}

/// Every scratch site currently DUE for reaping at `now`, as witnesses.
///
/// Goes through the same [`ScratchSite`] door as everything else, so the reaper
/// cannot end up holding a "site to delete" that no path proved was the agent's.
/// The predicate is [`Site::reap_due`] — the single expression of it — so this
/// enumerates rather than re-states the rule (a second copy of "and NULL means
/// never" is exactly how the two drift apart).
pub fn due_for_reap(conn: &Connection, now: &str) -> Result<Vec<ScratchSite>> {
    Ok(store::list_sites(conn)?
        .into_iter()
        .filter(|s| s.reap_due(now))
        .map(ScratchSite)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::models::test_site;

    /// A db holding one scratch site, one of the user's own, and one scratch
    /// site that has expired — all in production shape.
    fn db() -> Connection {
        let conn = crate::state::db::open_in_memory().unwrap();
        let mut scratch =
            test_site("c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24", "probe.scratch.rex", SiteOrigin::Agent);
        scratch.expires_at = Some("2099-01-01 00:00:00".into());
        store::insert_site(&conn, &scratch).unwrap();
        let real = test_site("7f3a1c02-9d51-4d2e-8b77-2c9a4e6f1b30", "myblog.rex", SiteOrigin::User);
        store::insert_site(&conn, &real).unwrap();
        let mut expired =
            test_site("d17c9b30-5f2e-4a68-b1d4-9c3e7a2f5011", "old.scratch.rex", SiteOrigin::Agent);
        expired.expires_at = Some("2020-01-01 00:00:00".into());
        store::insert_site(&conn, &expired).unwrap();
        conn
    }

    #[test]
    fn claim_proves_an_agent_owned_site() {
        let conn = db();
        let s = claim(&conn, "c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24").expect("scratch claims");
        assert_eq!(s.domain(), "probe.scratch.rex");
        assert_eq!(s.id(), "c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24");
        assert!(s.site().is_scratch());
    }

    #[test]
    fn claiming_the_users_own_site_refuses_with_a_policy_statement() {
        // What an AGENT reads. It has to say which site, which rule, and what to
        // do instead — a type error or a bare "refused" would leave the agent to
        // guess, and guessing means retrying the same call on the same site.
        let conn = db();
        let err = claim(&conn, "7f3a1c02-9d51-4d2e-8b77-2c9a4e6f1b30").unwrap_err().to_string();
        assert!(err.contains("myblog.rex"), "names the site: {err}");
        assert!(err.contains("your own sites"), "names the rule: {err}");
        assert!(err.contains("scratch_create_site"), "names the way forward: {err}");
        // And it never leaks an internal — the docroot is on the row we just read.
        assert!(!err.contains("/Users/"), "no path in a refusal: {err}");
        assert!(!err.contains("wp_"), "no database name in a refusal: {err}");
    }

    #[test]
    fn claiming_a_site_that_does_not_exist_says_so_distinctly() {
        // Distinct from the refusal above: "there is no such site" and "that site
        // is yours" send an agent to different next steps.
        let conn = db();
        let err = claim(&conn, "00000000-0000-4000-8000-000000000000").unwrap_err().to_string();
        assert!(err.contains("no site with id"), "{err}");
        assert!(err.contains("list_sites"), "names the way forward: {err}");
        assert!(!err.contains("your own sites"), "not the ownership refusal: {err}");
    }

    #[test]
    fn a_site_the_user_hand_named_scratch_is_still_the_users() {
        // The suffix is UX for humans scanning the Sites list. Policy reads
        // `origin`, never the name — so a user who creates `mine.scratch.rex`
        // themselves owns a normal site no agent tool can touch.
        let conn = db();
        let impostor = test_site("a1b2c3d4-1111-4222-8333-444455556666", "mine.scratch.rex", SiteOrigin::User);
        store::insert_site(&conn, &impostor).unwrap();
        assert!(claim(&conn, &impostor.id).is_err(), "the NAME must not grant scratch status");
    }

    #[test]
    fn due_for_reap_yields_witnesses_only_for_rows_the_predicate_admits() {
        let conn = db();
        let due = due_for_reap(&conn, "2026-08-01 12:00:00").unwrap();
        assert_eq!(due.len(), 1, "only the expired scratch site");
        assert_eq!(due[0].domain(), "old.scratch.rex");
        // Everything it hands back is, by type, something a path proved is the
        // agent's — the reaper can never hold a "site to delete" that isn't.
        assert!(due.iter().all(|s| s.site().is_scratch()));
    }
}

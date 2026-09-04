//! Scopes — the vocabulary the Agent access dial ranks, and the typed witness a
//! parity handler must hold to reach one of the USER's own sites.
//!
//! **What this module is after D17 (4 Sep 2026).** It began as a grants table:
//! a row per (site, client, scope), a prompt on the refusal path, expiry and
//! revocation. D15 replaced that with one global dial (`core::agent_access`)
//! for everything except publishing a site, and D16 folded mail and databases
//! into it; D17 folded publishing in too, on the owner's ruling that a local
//! dev tool should not ask twice for a thing its own level already describes.
//! So the grant row, the ask list, `authorize` and the implication rule are
//! gone (ledger #468–#474, retired), and what survives is the part that never
//! depended on them:
//!
//! - **[`Scope`]** — the closed set of things a tool can need, which is what
//!   `AccessLevel::needed_for` ranks into Read / Changes / Full.
//! - **[`Granted<S>`]** — the witness. Private fields, no `From`, no `new`: the
//!   only way to hold one is [`claim_by_level`], which asks the dial. A handler
//!   that takes `Granted<scope::Destroy>` cannot be reached without the dial
//!   having answered for exactly that scope.
//! - **[`still_granted`]** — the re-read before a destructive step, because the
//!   witness is a snapshot: the user can turn the dial down a moment later.
//!
//! **What a level is NOT (#197, unchanged):** containment. It bounds WHICH
//! verbs, not what code does once it runs; code an agent runs inside a site
//! runs as the user, and the card says so.

use crate::error::{Error, Result};
use crate::state::models::Site;
use rusqlite::Connection;
use std::marker::PhantomData;

/// The closed set of things a tool can need, ranked by blast radius.
///
/// Interpreted ONLY here and in `core::agent_access::AccessLevel::needed_for`,
/// which is the one place a scope becomes a level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// wp-cli reads, every log source, the user's inbox, their databases
    /// read-only — boots the site's code and reads real content. Not M1's read
    /// tools, which run nothing.
    Read,
    /// Non-destructive mutation: PHP/server/Xdebug, env, domains, restarts,
    /// installs, user creation, option writes, engine start/stop.
    Manage,
    /// Loses data or work: delete a site, import over a database, reset,
    /// delete a plugin/theme/user, switch core versions.
    Destroy,
    /// Executes code the agent chose, or exposes the machine: raw `wp_run` on a
    /// real site, artisan, repo scripts, git-sourced and linked-folder site
    /// creation, and publishing a site to the internet.
    Run,
    /// Machine-wide: the stack, the resolver, settings, default versions. The
    /// ones that reach `run_privileged` ALSO prompt macOS — that dialog is a
    /// second consent, and no level replaces it.
    System,
}

impl Scope {
    /// Every scope, in blast-radius order — the order the UI lists them in.
    pub const ALL: [Scope; 5] = [Scope::Read, Scope::Manage, Scope::Destroy, Scope::Run, Scope::System];

    /// The canonical text, for JSON and for a refusal.
    pub fn as_db(self) -> &'static str {
        match self {
            Scope::Read => "read",
            Scope::Manage => "manage",
            Scope::Destroy => "destroy",
            Scope::Run => "run",
            Scope::System => "system",
        }
    }

    /// Parse the text. `None` for anything else — an unknown scope reads as NO
    /// permission, never as "some permission".
    pub fn parse(s: &str) -> Option<Scope> {
        Scope::ALL.iter().copied().find(|sc| sc.as_db() == s)
    }
}

impl std::fmt::Display for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_db())
    }
}

/// The scope markers — one unit type per [`Scope`], so a handler's signature
/// SAYS which permission it needs (`Granted<scope::Destroy>`) and the compiler
/// refuses a call that only claimed a narrower one.
pub mod scope {
    use super::Scope;

    mod sealed {
        pub trait Sealed {}
    }

    /// A type that stands for exactly one [`Scope`]. Sealed: the five below are
    /// the whole set, matching `Scope::ALL`.
    pub trait Marker: sealed::Sealed + Send + Sync + 'static {
        const SCOPE: Scope;
    }

    macro_rules! marker {
        ($name:ident, $scope:expr) => {
            #[derive(Debug, Clone, Copy)]
            pub struct $name;
            impl sealed::Sealed for $name {}
            impl Marker for $name {
                const SCOPE: Scope = $scope;
            }
        };
    }
    marker!(Read, Scope::Read);
    marker!(Manage, Scope::Manage);
    marker!(Destroy, Scope::Destroy);
    marker!(Run, Scope::Run);
    marker!(System, Scope::System);
}

/// Proof that the dial answered for `S` on this target.
///
/// The private fields ARE the guarantee, the `ScratchSite` shape (#208): no
/// `From`, no `new`, nothing outside this module can mint one from a `Site` it
/// happens to hold. A parity handler that takes `Granted<S>` cannot be reached
/// without the gate having run for exactly `S`.
///
/// **Not a lock.** It proves the dial said yes when it was claimed. The user
/// can turn the dial down a moment later, so a handler's destructive step
/// re-asserts with [`still_granted`] immediately before it.
#[derive(Debug)]
pub struct Granted<S: scope::Marker> {
    site: Option<Site>,
    _scope: PhantomData<S>,
}

impl<S: scope::Marker> Granted<S> {
    /// The site row this witness is about — `None` for a stack-level claim.
    /// Handing out `&Site` cannot launder the proof: a `Site` grants nothing.
    pub fn site(&self) -> Option<&Site> {
        self.site.as_ref()
    }

    /// The scope this witness proves, from the type.
    pub fn scope() -> Scope {
        S::SCOPE
    }
}

/// The ONE door: resolve the site (a scratch site refused — it is the agent's
/// own and the scratch tools apply; "no such site" kept distinct, because the
/// two send an agent to different next steps), then ask the DIAL for `S`.
///
/// No client, no row, no expiry in the decision: the dial is global by ruling
/// (D15), and the refusal names it, the level needed, the level standing and
/// where a person turns it.
pub fn claim_by_level<S: scope::Marker>(conn: &Connection, site_id: Option<&str>) -> Result<Granted<S>> {
    let site = target_site(conn, site_id)?;
    let now = crate::core::agent_access::current(conn)?;
    if now.level < crate::core::agent_access::AccessLevel::needed_for(S::SCOPE) {
        let what = match &site {
            Some(s) => format!("acting on `{}`, one of the user's own sites, this way", s.domain),
            None => "this, on rexenv itself,".to_string(),
        };
        return Err(Error::Other(crate::core::agent_access::refusal(&what, S::SCOPE, &now)));
    }
    Ok(Granted { site, _scope: PhantomData })
}

/// Read the site row (or none, for the stack), refusing a scratch site.
fn target_site(conn: &Connection, site_id: Option<&str>) -> Result<Option<Site>> {
    Ok(match site_id {
        None => None,
        Some(id) => {
            let Some(site) = crate::state::store::get_site(conn, id)? else {
                return Err(Error::Other(format!(
                    "There is no site with id `{id}`. Use list_sites to see the sites that exist."
                )));
            };
            if site.is_scratch() {
                return Err(Error::Other(format!(
                    "`{}` is a scratch site the agent created, so no permission is needed for it — \
                     use the scratch tools on it (scratch_delete_site, scratch_add_package, wp_run, \
                     set_php_version, db_query) rather than the ones for the user's own sites.",
                    site.domain
                )));
            }
            Some(site)
        }
    })
}

/// Is the dial STILL at or above what this witness proved? The re-read before a
/// destructive step, for the case the witness deliberately does not cover — the
/// user turning it down in between.
pub fn still_granted<S: scope::Marker>(conn: &Connection, _granted: &Granted<S>) -> Result<bool> {
    crate::core::agent_access::allows(conn, S::SCOPE)
}

/// A claim's result. Kept as a struct rather than a bare `Granted` because
/// every handler destructures it, and a field added here (a note the reply
/// should carry, say) must not be a signature change at fifty call sites.
pub struct Claimed<S: scope::Marker> {
    pub granted: Granted<S>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::db;

    fn conn_with_site() -> Connection {
        let conn = db::open_in_memory().unwrap();
        let site = crate::state::models::test_site("s1", "shop.rex", crate::state::models::SiteOrigin::User);
        crate::state::store::insert_site(&conn, &site).unwrap();
        conn
    }

    /// **The scope vocabulary is closed and round-trips; unknown text is no
    /// scope, never some scope.**
    #[test]
    fn a_scope_round_trips_and_unknown_text_is_no_scope() {
        for s in Scope::ALL {
            assert_eq!(Scope::parse(s.as_db()), Some(s));
            assert_eq!(s.to_string(), s.as_db());
        }
        assert_eq!(Scope::parse("owner"), None, "an unknown scope is no scope");
        assert_eq!(Scope::parse("Read"), None, "the text is the canonical lower-case form");
        assert_eq!(Scope::ALL.len(), 5, "a sixth scope must be ranked in AccessLevel::needed_for first");
    }

    /// **`claim_by_level` — the one door — resolves the site (scratch refused,
    /// "no such site" distinct), then asks the DIAL and nothing else: no
    /// client, no row; `still_granted` re-reads the dial; the refusal names the
    /// scope, the level needed, the level standing and the dial.**
    #[test]
    fn claim_by_level_asks_the_dial_and_nothing_else() {
        use crate::core::agent_access::{self, AccessLevel, Mode};
        use crate::state::models::{test_site, SiteOrigin};
        use crate::state::store;
        let conn = conn_with_site();
        let theirs = test_site("c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24", "probe.scratch.rex", SiteOrigin::Agent);
        store::insert_site(&conn, &theirs).unwrap();

        // Read is free; the rest waits on the dial.
        let r = claim_by_level::<scope::Read>(&conn, Some("s1")).unwrap();
        assert_eq!(r.site().map(|s| s.domain.as_str()), Some("shop.rex"));
        let err = claim_by_level::<scope::Manage>(&conn, Some("s1")).unwrap_err().to_string();
        for must in ["`manage`", "`Agent access`", "Changes", "it is at Read", "shop.rex"] {
            assert!(err.contains(must), "missing {must:?}: {err}");
        }
        let err = claim_by_level::<scope::Destroy>(&conn, None).unwrap_err().to_string();
        assert!(err.contains("rexenv itself") && err.contains("Full"), "{err}");

        // Scratch and missing sites are refused BEFORE the dial is consulted.
        agent_access::set(&conn, AccessLevel::Full, Some(Mode::Always)).unwrap();
        let err = claim_by_level::<scope::Manage>(&conn, Some(&theirs.id)).unwrap_err().to_string();
        assert!(err.contains("scratch site the agent created"), "{err}");
        let err = claim_by_level::<scope::Manage>(&conn, Some("nope")).unwrap_err().to_string();
        assert!(err.contains("no site with id"), "{err}");

        // Changes covers manage and system; Full covers destroy and run.
        agent_access::set(&conn, AccessLevel::Changes, Some(Mode::Always)).unwrap();
        assert!(claim_by_level::<scope::Manage>(&conn, Some("s1")).is_ok());
        assert!(claim_by_level::<scope::System>(&conn, None).is_ok());
        assert!(claim_by_level::<scope::Destroy>(&conn, Some("s1")).is_err());
        assert!(claim_by_level::<scope::Run>(&conn, Some("s1")).is_err());
        agent_access::set(&conn, AccessLevel::Full, Some(Mode::Always)).unwrap();
        let d = claim_by_level::<scope::Destroy>(&conn, Some("s1")).unwrap();
        assert!(claim_by_level::<scope::Run>(&conn, Some("s1")).is_ok());

        // The witness is a snapshot: turning the dial down after the claim is
        // what `still_granted` exists to see.
        assert!(still_granted(&conn, &d).unwrap());
        agent_access::set(&conn, AccessLevel::Read, None).unwrap();
        assert!(!still_granted(&conn, &d).unwrap());
    }

    /// **Nothing in production reaches the retired grant machinery.** D17 took
    /// the last caller (publishing) into the dial; a row written by anything
    /// but a migration would be a second consent model growing back beside the
    /// one the card describes.
    #[test]
    fn no_production_code_writes_or_reads_a_site_grant_row() {
        for (name, src) in [
            ("agent_grants", include_str!("agent_grants.rs")),
            ("user_sites", include_str!("../mcp_server/user_sites.rs")),
            ("commands/mcp", include_str!("../commands/mcp.rs")),
            ("mcp_server", include_str!("../mcp_server.rs")),
        ] {
            let prod = &src[..src.find("#[cfg(test)]").unwrap_or(src.len())];
            for banned in ["grant_agent_site(", "active_agent_site_grant(", "list_agent_site_grants(", "revoke_agent_site_grant(", "claim_or_ask"] {
                assert!(!prod.contains(banned), "{name} still reaches `{banned}` — the grant model is retired (D17)");
            }
        }
    }
}

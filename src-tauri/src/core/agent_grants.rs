//! Scope grants — what an agent may do to the USER's own sites and to the
//! stack, decided from recorded facts (MCP parity, `docs/PLAN-mcp-parity.md` §3).
//!
//! `core::agent_db` is the one-resource version of this (a database, read-only,
//! one grant shape). This module is the same idea for a closed set of SCOPES,
//! and it keeps the same discipline that made M3 work in front of a real client:
//!
//! - **The gate is one pure function** ([`authorize`]) over the grants table and
//!   nothing else. Everything it needs is passed in; the only way to widen it is
//!   to edit it. It knows nothing about auto-allow (a guard asserts that) — the
//!   bypass lives at the call site, so the decision stays readable as one rule.
//! - **The ask exists only on the refusal path.** There is no "request access"
//!   tool; a refused call records what it wanted, and the user sees that in the
//!   app. So nothing can ask without having been told no first.
//! - **Auto-allow skips a prompt, never a boundary.** Here that is a TYPE fact:
//!   [`AutoAllowable`] has variants for `read`, `manage` and `run` and none for
//!   `destroy` or `system`. A standing "yes" to deleting a site is not a
//!   convenience anyone asked for, and the way to be sure nobody adds one in a
//!   hurry is for the enum to have nowhere to put it.
//!
//! **What a grant is NOT (#197, unchanged):** containment. A grant bounds WHICH
//! site and WHICH verb; code the agent runs inside a granted site runs as the
//! user. The dialog says so.

use crate::error::{Error, Result};
use crate::state::models::Site;
use crate::state::store::{self, AgentSiteGrant};
use rusqlite::Connection;
use std::marker::PhantomData;

/// The closed set of things a user can allow, ranked by blast radius.
///
/// Stored as its `as_db` text; interpreted ONLY here. The store matches by
/// exact text and the IMPLICATION between scopes ([`Scope::satisfied_by`]) is
/// this module's rule, so there is exactly one place that decides whether a
/// `destroy` grant covers a `manage` action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// wp-cli reads, every log source, the user's inbox — boots the site's code
    /// and reads real content. Not M1's read tools, which run nothing.
    Read,
    /// Non-destructive mutation: PHP/server/Xdebug, env, domains, restarts,
    /// installs, user creation, option writes, engine start/stop.
    Manage,
    /// Loses data or work: delete a site, import over a database, reset,
    /// delete a plugin/theme/user, switch core versions.
    Destroy,
    /// Executes code the agent chose: raw `wp_run` on a real site, artisan,
    /// repo scripts, git-sourced and linked-folder site creation, shares.
    Run,
    /// Machine-wide: the stack, the resolver, settings, default versions. The
    /// ones that reach `run_privileged` ALSO prompt macOS — that dialog is a
    /// second consent, not replaced by this one.
    System,
}

impl Scope {
    /// Every scope, in blast-radius order — the order the UI lists them in.
    pub const ALL: [Scope; 5] = [Scope::Read, Scope::Manage, Scope::Destroy, Scope::Run, Scope::System];

    /// The canonical text, for the column and for JSON.
    pub fn as_db(self) -> &'static str {
        match self {
            Scope::Read => "read",
            Scope::Manage => "manage",
            Scope::Destroy => "destroy",
            Scope::Run => "run",
            Scope::System => "system",
        }
    }

    /// Parse the column text. `None` for anything else — an unknown scope in a
    /// row reads as NO permission, never as "some permission".
    pub fn parse(s: &str) -> Option<Scope> {
        Scope::ALL.iter().copied().find(|sc| sc.as_db() == s)
    }

    /// The grants that SATISFY a demand for `self`.
    ///
    /// The one implication rule, stated once: `destroy` covers `manage` covers
    /// `read` — a user who allowed deleting a site has allowed editing it, and
    /// one who allowed editing has allowed looking. `run` and `system` stand
    /// alone: running the agent's chosen code and changing the machine are not
    /// wider versions of anything, and nothing wider than them exists. Order is
    /// narrowest first, so the first satisfying grant found is the least the
    /// user gave.
    pub fn satisfied_by(self) -> &'static [Scope] {
        match self {
            Scope::Read => &[Scope::Read, Scope::Manage, Scope::Destroy],
            Scope::Manage => &[Scope::Manage, Scope::Destroy],
            Scope::Destroy => &[Scope::Destroy],
            Scope::Run => &[Scope::Run],
            Scope::System => &[Scope::System],
        }
    }

    /// One sentence for the dialog and the list: what allowing this lets an
    /// agent do to the named site (or the stack). Verbatim in the consent copy,
    /// so it lives beside the rule it describes.
    pub fn what_it_allows(self) -> &'static str {
        match self {
            Scope::Read => {
                "read its content and settings, its logs, and the mail it sent — everything in it, \
                 including user emails and anything a plugin stored"
            }
            Scope::Manage => {
                "change how it is served and what is installed in it — PHP version, web server, \
                 domains, plugins, themes, users, options — but not delete it or its data"
            }
            Scope::Destroy => {
                "delete it, reset it, or replace its database — things that lose work and cannot be \
                 undone"
            }
            Scope::Run => {
                "run commands and code of the agent's choosing in it, as you, with your files and \
                 your permissions"
            }
            Scope::System => {
                "change rexenv itself — start or stop the stack, install PHP versions, change \
                 settings. Anything that needs an administrator password still asks you"
            }
        }
    }

}

impl std::fmt::Display for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_db())
    }
}


/// One outstanding ASK: an agent wanted `scope` on a site (or the stack) and was
/// refused. Session-scoped, in memory — a prompt whose context is gone is not
/// consent (the `agent_db::GrantRequest` rule, unchanged).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantRequest {
    /// `None` = about the stack.
    pub site_id: Option<String>,
    /// The site's domain for the prompt; `None` for the stack.
    pub domain: Option<String>,
    pub client: String,
    pub scope: Scope,
    /// What the agent was trying to do, in the tool's own words — the prompt
    /// shows it so the user answers a concrete question ("switch its PHP to
    /// 8.4") and not an abstract one ("manage?"). Bounded at the writer.
    pub wanted: String,
}

/// The most recent outstanding asks, deduplicated by (site, client, scope).
#[derive(Debug, Default)]
pub struct GrantRequests(Vec<GrantRequest>);

/// The most asks kept — a prompt list a human reads, not a log.
pub const MAX_REQUESTS: usize = 20;
/// The longest `wanted` text kept. Agent text, so it is clamped where it is
/// written rather than trusted to be short.
pub const MAX_WANTED_CHARS: usize = 120;

impl GrantRequests {
    /// Record an ask; `true` if it was NEW. A retry loop is one question, and
    /// the newest `wanted` wins so the prompt describes the latest attempt.
    pub fn ask(&mut self, mut req: GrantRequest) -> bool {
        if req.wanted.chars().count() > MAX_WANTED_CHARS {
            req.wanted = req.wanted.chars().take(MAX_WANTED_CHARS).collect::<String>() + "…";
        }
        let same = |r: &GrantRequest| {
            r.site_id == req.site_id && r.client == req.client && r.scope == req.scope
        };
        let existed = self.0.iter().any(same);
        self.0.retain(|r| !same(r));
        self.0.insert(0, req);
        self.0.truncate(MAX_REQUESTS);
        !existed
    }

    pub fn list(&self) -> &[GrantRequest] {
        &self.0
    }

    /// Clear one ask — what answering it (either way) does.
    pub fn answer(&mut self, site_id: Option<&str>, client: &str, scope: Scope) {
        self.0
            .retain(|r| !(r.site_id.as_deref() == site_id && r.client == client && r.scope == scope));
    }
}

/// How long "Allow for 7 days" lasts — the number in the dialog's own words.
pub const GRANT_DAYS: u32 = 7;
/// The ceiling on a "for this session" grant, so it dies even if the launch
/// sweep never runs (a crash, a database restored elsewhere).
pub const SESSION_CEILING_DAYS: u32 = 1;

/// What the gate needs to know about the target — a site by its RECORDED row,
/// or the stack.
#[derive(Debug, Clone, Copy)]
pub enum Target<'a> {
    Site { id: &'a str, domain: &'a str },
    Stack,
}

impl Target<'_> {
    fn site_id(&self) -> Option<&str> {
        match self {
            Target::Site { id, .. } => Some(id),
            Target::Stack => None,
        }
    }
}

/// May `client` do a `scope` thing to `target` RIGHT NOW? The grant that says
/// so, or a refusal that names what is missing and where it is given.
///
/// **Pure over recorded facts** — the grants table, read through the store's
/// exact-match lookup once per scope that would satisfy the demand, narrowest
/// first. It does not know about auto-allow (guarded), does not know about
/// ownership (a scratch site never reaches here — the tool layer refuses those
/// before asking, because a scratch site is the agent's and needs no grant),
/// and does not record the ask (that is the refusal path's job at the call
/// site, so this stays a question and not a side effect).
///
/// The refusal is the ONLY thing an agent sees, so it says what is missing,
/// who has to give it, and where — and that the agent cannot give it itself.
pub fn authorize(conn: &Connection, target: Target<'_>, scope: Scope, client: &str) -> Result<AgentSiteGrant> {
    // Every scope has a stack-level meaning (settled with the inbox, P3.4):
    // `read` on rexenv itself is the user's whole Mailpit inbox and every log
    // source, `destroy` on it is clearing that inbox, `manage` is creating a
    // site or bouncing a service. An earlier version refused stack-level
    // read/destroy as a shape; the inbox is exactly the stack-level read that
    // must NOT be free, so the refusal was the wrong rule.
    for satisfying in scope.satisfied_by() {
        if let Some(g) = store::active_agent_site_grant(conn, target.site_id(), client, satisfying.as_db())? {
            return Ok(g);
        }
    }
    Err(Error::Other(refusal(target, scope)))
}

/// The refusal text — one shape for both targets, so the pins that hold it to
/// the UI's labels (#404's lesson) have one string to hold.
fn refusal(target: Target<'_>, scope: Scope) -> String {
    let what = match target {
        Target::Site { domain, .. } => format!("`{domain}` is one of the user's own sites, and this"),
        Target::Stack => "this acts on rexenv itself, and it".to_string(),
    };
    format!(
        "{what} needs the user's `{scope}` permission, which has not been given (or has expired). \
         rexenv is asking for it now, in the app: Settings → \"AI agents (MCP)\" → \"Site access\", \
         where it can be allowed for {GRANT_DAYS} days, for this session only, or refused. That \
         section also lists every permission and when it expires. This is not something the agent \
         can grant itself; if the person you're working with wants it, they will allow it there."
    )
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

/// Proof that `client` holds a live `S` grant on the target — the row for the
/// site (or none, for the stack) and the grant that was found, read TOGETHER by
/// [`claim`], which is the only constructor.
///
/// The private fields ARE the guarantee, the `ScratchSite` shape (#208): no
/// `From`, no `new`, nothing outside this module can mint one from a `Site` and
/// a grant it happens to hold. A parity handler that takes `Granted<S>` cannot
/// be reached without the gate having run for exactly `S`.
///
/// **Not a lock**, as its sibling is not: it proves the gate passed when it was
/// claimed. The user can press Revoke a moment later, so a handler's
/// destructive step re-asserts with [`still_granted`] immediately before it.
#[derive(Debug)]
pub struct Granted<S: scope::Marker> {
    site: Option<Site>,
    proof: Proof,
    _scope: PhantomData<S>,
}

/// What satisfied the claim: the dial (D15 — everything but publishing), or a
/// grant row a person clicked (share only).
#[derive(Debug)]
enum Proof {
    Level,
    Grant(AgentSiteGrant),
}

impl<S: scope::Marker> Granted<S> {
    /// The site row this grant is about — `None` for a stack-level grant.
    /// Handing out `&Site` cannot launder the proof: a `Site` grants nothing.
    pub fn site(&self) -> Option<&Site> {
        self.site.as_ref()
    }

    /// The grant row that satisfied the claim — only for a share, the one
    /// tool still behind a person's click; `None` when the dial answered.
    pub fn grant(&self) -> Option<&AgentSiteGrant> {
        match &self.proof {
            Proof::Grant(g) => Some(g),
            Proof::Level => None,
        }
    }

    /// The scope this witness proves, from the type.
    pub fn scope() -> Scope {
        S::SCOPE
    }

    fn target(&self) -> Target<'_> {
        match &self.site {
            Some(s) => Target::Site { id: &s.id, domain: &s.domain },
            None => Target::Stack,
        }
    }
}

/// The ONE conversion: read the site row (or none, for the stack), refuse a
/// scratch site (it is the agent's — the scratch tools apply and no grant is
/// needed), run the gate for `S`, and hand back the witness.
///
/// Reads the row ITSELF from `site_id`, never a `Site` the caller holds — the
/// caller cannot pass a value it built or edited. Every failure is a policy
/// statement an agent can act on, and "no such site" is kept distinct from
/// "that one is yours, not the user's" because they send an agent to different
/// next steps.
pub fn claim<S: scope::Marker>(conn: &Connection, site_id: Option<&str>, client: &str) -> Result<Granted<S>> {
    let site = target_site(conn, site_id)?;
    let target = match &site {
        Some(s) => Target::Site { id: &s.id, domain: &s.domain },
        None => Target::Stack,
    };
    let grant = authorize(conn, target, S::SCOPE, client)?;
    Ok(Granted { site, proof: Proof::Grant(grant), _scope: PhantomData })
}

/// The D15 door: the same site resolution (a scratch site refused, "no such
/// site" kept distinct), then the DIAL for `S` — no client, no site, no row.
/// Everything but `share` comes through here.
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
    Ok(Granted { site, proof: Proof::Level, _scope: PhantomData })
}

/// Read the site row (or none, for the stack), refusing a scratch site.
fn target_site(conn: &Connection, site_id: Option<&str>) -> Result<Option<Site>> {
    Ok(match site_id {
        None => None,
        Some(id) => {
            let Some(site) = store::get_site(conn, id)? else {
                return Err(Error::Other(format!(
                    "There is no site with id `{id}`. Use list_sites to see the sites that exist."
                )));
            };
            if site.is_scratch() {
                return Err(Error::Other(format!(
                    "`{}` is a scratch site the agent created, so no permission is needed for it — \\
                     use the scratch tools on it (scratch_delete_site, scratch_add_package, wp_run, \\
                     set_php_version, db_query) rather than the ones for the user's own sites.",
                    site.domain
                )));
            }
            Some(site)
        }
    })
}

/// Is this witness's grant STILL live, right now? The re-read before a
/// destructive step, for the case the witness deliberately does not cover —
/// the user pressing Revoke in between. Re-runs the gate rather than checking
/// the one grant id, so a revoke of the found grant with another satisfying
/// grant still standing reads as what it is: still allowed.
pub fn still_granted<S: scope::Marker>(conn: &Connection, granted: &Granted<S>) -> Result<bool> {
    match &granted.proof {
        Proof::Grant(g) => Ok(authorize(conn, granted.target(), S::SCOPE, &g.client).is_ok()),
        Proof::Level => crate::core::agent_access::allows(conn, S::SCOPE),
    }
}

/// The result of [`claim_or_ask`]: the witness, and whether auto-allow (not a
/// person) produced the grant that satisfied it — so the reply can say so.
pub struct Claimed<S: scope::Marker> {
    pub granted: Granted<S>,
}

/// The call-site shape every parity handler uses: claim, and on refusal either
/// answer with auto-allow (when the scope CAN be and IS auto-allowed) or record
/// the ask and return the refusal.
///
/// **This is the one place auto-allow is consulted** (#408's one-call-site
/// rule, kept as one function instead of one arm). What it does on the auto
/// path is exactly what the button does — writes a grant row for `S::SCOPE`,
/// same expiry, listed and revocable, flagged `auto_granted` — and then claims
/// again through the same gate, so an auto-granted call is not a second code
/// path that could widen anything. What it never does: touch a scratch site
/// (refused before the gate, and auto-allow has no say — the tier boundary is
/// a rule, not a prompt), or auto-allow `destroy`/`system` (no variant).
///
/// `wanted` is the tool's own one-line description of what the agent was
/// trying to do, shown in the prompt; agent text, clamped at the writer.
pub fn claim_or_ask<S: scope::Marker>(
    conn: &Connection,
    requests: &mut GrantRequests,
    site_id: Option<&str>,
    client: &str,
    wanted: &str,
) -> Result<Claimed<S>> {
    let refusal = match claim::<S>(conn, site_id, client) {
        Ok(granted) => {
            // A grant that now satisfies the claim makes any outstanding ask for
            // it stale — a prompt for something already allowed is noise, and
            // the button's own path clears it the same way.
            requests.answer(site_id, client, S::SCOPE);
            return Ok(Claimed { granted });
        }
        Err(e) => e,
    };
    // Only a REAL, EXISTING site (or the stack) can be asked about: a missing
    // site or a scratch site failed before the gate, and recording an ask for
    // those would prompt the user about a permission that cannot apply.
    let site = match site_id {
        None => None,
        Some(id) => match store::get_site(conn, id)? {
            Some(s) if !s.is_scratch() => Some(s),
            _ => return Err(refusal),
        },
    };
    requests.ask(GrantRequest {
        site_id: site.as_ref().map(|s| s.id.clone()),
        domain: site.as_ref().map(|s| s.domain.clone()),
        client: client.to_string(),
        scope: S::SCOPE,
        wanted: wanted.to_string(),
    });
    Err(refusal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::db;

    fn conn_with_site() -> Connection {
        let conn = db::open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO sites (id, name, domain, type, status, php_version, web_server, ssl,
                                path, db_name, db_engine)
             VALUES ('s1','S','shop.rex','wordpress','stopped','8.3','nginx',1,'/tmp/s','wp_shop','mysql')",
            [],
        )
        .unwrap();
        conn
    }

    const SITE: Target<'static> = Target::Site { id: "s1", domain: "shop.rex" };

    /// **A demand is satisfied by its own scope and by the wider ones above it,
    /// and by nothing else.** The implication rule stated once, tested on every
    /// pair, so adding a scope has to say where it sits.
    #[test]
    fn a_demand_is_satisfied_by_itself_and_the_wider_scopes_only() {
        for demand in Scope::ALL {
            for granted in Scope::ALL {
                let covers = demand.satisfied_by().contains(&granted);
                let expected = match (demand, granted) {
                    (a, b) if a == b => true,
                    (Scope::Read, Scope::Manage | Scope::Destroy) => true,
                    (Scope::Manage, Scope::Destroy) => true,
                    _ => false,
                };
                assert_eq!(covers, expected, "does a `{granted}` grant cover a `{demand}` demand?");
            }
            // Narrowest first, so the grant found is the least the user gave.
            assert_eq!(demand.satisfied_by()[0], demand);
            // The text round-trips, and nothing else parses.
            assert_eq!(Scope::parse(demand.as_db()), Some(demand));
        }
        assert_eq!(Scope::parse("owner"), None, "an unknown scope is no scope");
        assert_eq!(Scope::parse("Read"), None, "the text is the canonical lower-case form");
    }

    /// **The gate reads recorded grants, honours the implication rule, and its
    /// refusal names the place consent is given.**
    #[test]
    fn the_gate_reads_recorded_grants_and_its_refusal_names_where_consent_lives() {
        use crate::state::store;
        let conn = conn_with_site();

        // Nothing granted: refused, and the refusal is followable — card,
        // section, both durations, and who cannot give it.
        let err = authorize(&conn, SITE, Scope::Manage, "claude-code").unwrap_err().to_string();
        for must in ["shop.rex", "`manage`", "AI agents (MCP)", "Site access", "7 days", "this session", "not something the agent can grant itself"] {
            assert!(err.contains(must), "refusal must say {must:?}: {err}");
        }

        // A `manage` grant satisfies `manage` AND `read` (implication, live)
        // and NOT `destroy`, `run` or `system`.
        store::grant_agent_site(&conn, "g1", Some("s1"), "claude-code", "manage", 7, false, false).unwrap();
        assert_eq!(authorize(&conn, SITE, Scope::Manage, "claude-code").unwrap().id, "g1");
        assert_eq!(authorize(&conn, SITE, Scope::Read, "claude-code").unwrap().id, "g1");
        for wider in [Scope::Destroy, Scope::Run, Scope::System] {
            assert!(authorize(&conn, SITE, wider, "claude-code").is_err(), "{wider} leaked from a manage grant");
        }
        // The narrowest satisfying grant wins when several exist.
        store::grant_agent_site(&conn, "g2", Some("s1"), "claude-code", "destroy", 7, false, false).unwrap();
        assert_eq!(authorize(&conn, SITE, Scope::Read, "claude-code").unwrap().id, "g1");
        assert_eq!(authorize(&conn, SITE, Scope::Destroy, "claude-code").unwrap().id, "g2");

        // One client, one site. A stack grant is not a site grant and vice versa.
        assert!(authorize(&conn, SITE, Scope::Read, "cursor").is_err());
        assert!(authorize(&conn, Target::Stack, Scope::Manage, "claude-code").is_err());
        store::grant_agent_site(&conn, "g3", None, "claude-code", "system", 7, false, false).unwrap();
        assert_eq!(authorize(&conn, Target::Stack, Scope::System, "claude-code").unwrap().id, "g3");
        assert!(authorize(&conn, SITE, Scope::System, "claude-code").is_err(), "a stack grant opened a site");
        let stack_err = authorize(&conn, Target::Stack, Scope::Run, "claude-code").unwrap_err().to_string();
        assert!(stack_err.contains("rexenv itself") && stack_err.contains("Site access"), "{stack_err}");
        // `read`/`destroy` DO have a stack meaning (the inbox): refused for
        // want of a grant, like any other, and satisfied by one.
        assert!(authorize(&conn, Target::Stack, Scope::Read, "claude-code").is_err());
        store::grant_agent_site(&conn, "g-inbox", None, "claude-code", "read", 7, false, false).unwrap();
        assert_eq!(authorize(&conn, Target::Stack, Scope::Read, "claude-code").unwrap().id, "g-inbox");

        // A row whose scope text is not one of ours satisfies nothing — it is
        // absent, not "some scope".
        store::grant_agent_site(&conn, "g4", Some("s1"), "cursor", "owner", 7, false, false).unwrap();
        for s in Scope::ALL {
            assert!(authorize(&conn, SITE, s, "cursor").is_err(), "an unknown scope text granted {s}");
        }

        // Revoking and expiring both close the gate, through the same column.
        store::revoke_agent_site_grant(&conn, "g1").unwrap();
        store::revoke_agent_site_grant(&conn, "g2").unwrap();
        assert!(authorize(&conn, SITE, Scope::Read, "claude-code").is_err());
        store::grant_agent_site(&conn, "g5", Some("s1"), "claude-code", "read", 7, false, false).unwrap();
        assert!(authorize(&conn, SITE, Scope::Read, "claude-code").is_ok());
        conn.execute("UPDATE agent_site_grants SET expires_at = datetime('now','-1 hour') WHERE id='g5'", []).unwrap();
        assert!(authorize(&conn, SITE, Scope::Read, "claude-code").is_err(), "an expired grant still opened the site");
    }

    /// **Repeated asks are one prompt per (site, client, scope), the newest
    /// wording wins, and answering clears exactly that one.**
    #[test]
    fn repeated_asks_are_one_prompt_per_scope_and_answering_clears_it() {
        let mut reqs = GrantRequests::default();
        let ask = |site: Option<&str>, scope: Scope, wanted: &str| GrantRequest {
            site_id: site.map(String::from),
            domain: site.map(|_| "shop.rex".into()),
            client: "claude-code".into(),
            scope,
            wanted: wanted.into(),
        };
        assert!(reqs.ask(ask(Some("s1"), Scope::Manage, "switch PHP to 8.4")));
        assert!(!reqs.ask(ask(Some("s1"), Scope::Manage, "switch web server to frankenphp")), "same key = one prompt");
        assert_eq!(reqs.list().len(), 1);
        assert_eq!(reqs.list()[0].wanted, "switch web server to frankenphp", "the newest wording is shown");
        assert!(reqs.ask(ask(Some("s1"), Scope::Destroy, "delete the site")), "a different scope is a different question");
        assert!(reqs.ask(ask(None, Scope::System, "start the stack")), "the stack is its own target");
        assert_eq!(reqs.list().len(), 3);

        reqs.answer(Some("s1"), "claude-code", Scope::Manage);
        assert_eq!(reqs.list().len(), 2);
        assert!(reqs.list().iter().all(|r| r.scope != Scope::Manage));
        reqs.answer(None, "claude-code", Scope::System);
        assert_eq!(reqs.list().len(), 1);

        // Agent text is clamped where it is written.
        let long = "x".repeat(500);
        reqs.ask(ask(Some("s1"), Scope::Run, &long));
        let kept = &reqs.list()[0].wanted;
        assert!(kept.chars().count() <= MAX_WANTED_CHARS + 1 && kept.ends_with('…'), "{}", kept.len());

        // And the list is bounded however many sites an agent invents.
        for i in 0..100 {
            reqs.ask(ask(Some(&format!("fake-{i}")), Scope::Read, "look"));
        }
        assert_eq!(reqs.list().len(), MAX_REQUESTS);
    }

    /// **A parity handler reaches a user's site only through `claim`, which
    /// reads the row itself, refuses the agent's own scratch sites, and proves
    /// exactly the scope in its type.**
    #[test]
    fn a_witness_is_minted_only_by_claim_and_proves_exactly_its_scope() {
        use crate::state::models::{test_site, SiteOrigin};
        use crate::state::store;
        let conn = conn_with_site();
        let theirs = test_site("c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24", "probe.scratch.rex", SiteOrigin::Agent);
        store::insert_site(&conn, &theirs).unwrap();

        // No grant: refused by the gate.
        assert!(claim::<scope::Manage>(&conn, Some("s1"), "claude-code").is_err());
        store::grant_agent_site(&conn, "g1", Some("s1"), "claude-code", "manage", 7, false, false).unwrap();

        // The witness carries the row and the grant, and its scope is the TYPE.
        let w = claim::<scope::Manage>(&conn, Some("s1"), "claude-code").unwrap();
        assert_eq!(w.site().map(|s| s.domain.as_str()), Some("shop.rex"));
        assert_eq!(w.grant().map(|g| g.id.as_str()), Some("g1"));
        assert_eq!(Granted::<scope::Manage>::scope(), Scope::Manage);
        // Implication through the witness: a `manage` grant mints a Read
        // witness, never a Destroy one.
        assert!(claim::<scope::Read>(&conn, Some("s1"), "claude-code").is_ok());
        assert!(claim::<scope::Destroy>(&conn, Some("s1"), "claude-code").is_err());

        // A scratch site is refused BEFORE the gate, with the scratch tools
        // named — and no grant would change that (the tier boundary is not a
        // prompt).
        store::grant_agent_site(&conn, "g2", Some(&theirs.id), "claude-code", "destroy", 7, false, false).unwrap();
        let err = claim::<scope::Read>(&conn, Some(&theirs.id), "claude-code").unwrap_err().to_string();
        assert!(err.contains("scratch site the agent created") && err.contains("wp_run"), "{err}");

        // "No such site" is a different sentence from "not the user's".
        let missing = claim::<scope::Read>(&conn, Some("nope"), "claude-code").unwrap_err().to_string();
        assert!(missing.contains("no site with id"), "{missing}");

        // The stack: no row, a NULL-site grant.
        assert!(claim::<scope::System>(&conn, None, "claude-code").is_err());
        store::grant_agent_site(&conn, "g3", None, "claude-code", "system", 7, false, false).unwrap();
        let st = claim::<scope::System>(&conn, None, "claude-code").unwrap();
        assert!(st.site().is_none());

        // Re-assert: revoking closes it; a still-standing wider grant keeps it.
        assert!(still_granted(&conn, &w).unwrap());
        store::grant_agent_site(&conn, "g4", Some("s1"), "claude-code", "destroy", 7, false, false).unwrap();
        store::revoke_agent_site_grant(&conn, "g1").unwrap();
        assert!(still_granted(&conn, &w).unwrap(), "another satisfying grant still stands");
        store::revoke_agent_site_grant(&conn, "g4").unwrap();
        assert!(!still_granted(&conn, &w).unwrap(), "revoked between claim and act");

        // The private fields are the guarantee. Plant-and-capture, 3 Sep 2026,
        // from `core/scratch.rs` (a sibling module, the nearest tempting place):
        //   `Granted { site: None, grant, _scope: PhantomData }`
        //     → E0451: fields `site`, `grant` and `_scope` of struct
        //       `agent_grants::Granted` are private — ONE error naming all three
        //   there is no tuple constructor and no `From`, so E0423/E0277 have
        //   nothing to name — the shape offers the one door and nothing else.
        let me = include_str!("agent_grants.rs");
        let prod = &me[..me.find("#[cfg(test)]").unwrap()];
        let start = prod.find("pub struct Granted<").unwrap();
        let body = &prod[start..start + prod[start..].find('}').unwrap()];
        assert!(!body.contains("pub site") && !body.contains("pub grant"), "{body}");
        let from_impl = ["impl<S: scope::Marker> ", "From<"].concat();
        assert!(!prod.contains(&from_impl), "a From would be a second door");
    }

    /// **`claim_or_ask` (share's door) records the ask on refusal — for a real
    /// site only — and answers from a grant row a person made.**
    #[test]
    fn claim_or_ask_records_the_ask_on_refusal_for_a_real_site_only() {
        use crate::state::models::{test_site, SiteOrigin};
        use crate::state::store;
        let conn = conn_with_site();
        let theirs = test_site("c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24", "probe.scratch.rex", SiteOrigin::Agent);
        store::insert_site(&conn, &theirs).unwrap();
        let mut reqs = GrantRequests::default();

        let err = claim_or_ask::<scope::Run>(&conn, &mut reqs, Some("s1"), "claude-code", "publish it for 5 minutes")
            .err()
            .expect("refused")
            .to_string();
        assert!(err.contains("Site access"), "{err}");
        assert_eq!(reqs.list().len(), 1);
        assert_eq!(reqs.list()[0].scope, Scope::Run);
        assert_eq!(reqs.list()[0].domain.as_deref(), Some("shop.rex"));
        assert_eq!(reqs.list()[0].wanted, "publish it for 5 minutes");

        // A scratch site or a missing site records NOTHING — there is no
        // permission that could apply, so a prompt would be a lie.
        assert!(claim_or_ask::<scope::Run>(&conn, &mut reqs, Some(&theirs.id), "claude-code", "x").is_err());
        assert!(claim_or_ask::<scope::Run>(&conn, &mut reqs, Some("nope"), "claude-code", "x").is_err());
        assert_eq!(reqs.list().len(), 1, "no ask for a scratch or missing site");

        // A person's row answers it, and the ask is cleared.
        store::grant_agent_site(&conn, "g1", Some("s1"), "claude-code", "run", 1, false, true).unwrap();
        let c = claim_or_ask::<scope::Run>(&conn, &mut reqs, Some("s1"), "claude-code", "publish").unwrap();
        assert_eq!(c.granted.grant().map(|g| g.id.as_str()), Some("g1"));
        assert!(reqs.list().is_empty(), "a satisfied ask is cleared");
    }

    /// **`claim_by_level` — the door everything but share uses — resolves the
    /// site the same way (scratch refused, "no such site" distinct), then
    /// asks the DIAL and nothing else: no client, no row; the witness carries
    /// no grant; `still_granted` re-reads the dial; the refusal names the
    /// scope, the level and the dial.**
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
        assert!(r.grant().is_none(), "the dial leaves no row");
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

        // No grant row was written by any of this.
        assert!(store::list_agent_site_grants(&conn).unwrap().is_empty());
    }
}

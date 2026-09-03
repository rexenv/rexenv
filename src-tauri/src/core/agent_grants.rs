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
use crate::state::store::{self, AgentSiteGrant};
use rusqlite::Connection;

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

    /// The scopes a stack-level grant (no site) can be given in. `read` of the
    /// stack is free (status tools are unattended); `destroy` of the stack has
    /// no meaning; so a stack grant is `manage`, `run` or `system`.
    pub fn allowed_at_stack_level(self) -> bool {
        matches!(self, Scope::Manage | Scope::Run | Scope::System)
    }
}

impl std::fmt::Display for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_db())
    }
}

/// The scopes auto-allow is ABLE to answer for. `destroy` and `system` have no
/// variant: the absence is the rule (a standing yes to losing work or changing
/// the machine is not a convenience), and a `match` on this type cannot grow
/// an arm for them without the type growing first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutoAllowable {
    Read,
    Manage,
    Run,
}

impl AutoAllowable {
    pub const ALL: [AutoAllowable; 3] = [AutoAllowable::Read, AutoAllowable::Manage, AutoAllowable::Run];

    pub fn scope(self) -> Scope {
        match self {
            AutoAllowable::Read => Scope::Read,
            AutoAllowable::Manage => Scope::Manage,
            AutoAllowable::Run => Scope::Run,
        }
    }
}

impl TryFrom<Scope> for AutoAllowable {
    type Error = Scope;
    /// `Err(scope)` for the two that cannot be auto-allowed — the caller gets
    /// the scope back to name in its refusal.
    fn try_from(s: Scope) -> std::result::Result<Self, Scope> {
        match s {
            Scope::Read => Ok(AutoAllowable::Read),
            Scope::Manage => Ok(AutoAllowable::Manage),
            Scope::Run => Ok(AutoAllowable::Run),
            Scope::Destroy | Scope::System => Err(s),
        }
    }
}

/// Which auto-allowable scopes are switched on, this session.
///
/// In memory and session-scoped, exactly as `core::agent_db::AutoAllow` and for
/// the same reason: a standing yes that survives a restart is one somebody
/// switches on for an afternoon and still has on a month later. A poisoned
/// lock reads as OFF at every call site — the safe direction for a bypass.
#[derive(Debug, Default)]
pub struct AutoAllowScopes(Vec<AutoAllowable>);

impl AutoAllowScopes {
    pub fn is_on(&self, s: AutoAllowable) -> bool {
        self.0.contains(&s)
    }

    pub fn set(&mut self, s: AutoAllowable, on: bool) {
        self.0.retain(|x| *x != s);
        if on {
            self.0.push(s);
        }
    }

    /// The ones currently on, for the UI.
    pub fn on(&self) -> Vec<AutoAllowable> {
        AutoAllowable::ALL.iter().copied().filter(|s| self.is_on(*s)).collect()
    }
}

/// The sentence appended to a reply when auto-allow produced the grant, so the
/// agent reports the access as what it was.
pub const AUTO_GRANTED_NOTE: &str =
    "This permission was granted automatically because the person you're working with has \
     auto-allow switched on for it in rexenv \u{2014} they were not asked. It is recorded and \
     expires like any other grant, and they can revoke it.";

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
    if matches!(target, Target::Stack) && !scope.allowed_at_stack_level() {
        return Err(Error::Other(format!(
            "`{scope}` is not something that can be granted for rexenv as a whole — it is a \
             per-site permission. Name a site."
        )));
    }
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
        Target::Stack => "this changes rexenv itself, and it".to_string(),
    };
    format!(
        "{what} needs the user's `{scope}` permission, which has not been given (or has expired). \
         rexenv is asking for it now, in the app: Settings → \"AI agents (MCP)\" → \"Site access\", \
         where it can be allowed for {GRANT_DAYS} days, for this session only, or refused. That \
         section also lists every permission and when it expires. This is not something the agent \
         can grant itself; if the person you're working with wants it, they will allow it there."
    )
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

    /// **`destroy` and `system` cannot be auto-allowed because the type has no
    /// place for them** — and auto-allow lives in memory, not in settings.
    #[test]
    fn destroy_and_system_cannot_be_auto_allowed_because_no_variant_exists() {
        assert_eq!(AutoAllowable::try_from(Scope::Destroy), Err(Scope::Destroy));
        assert_eq!(AutoAllowable::try_from(Scope::System), Err(Scope::System));
        for a in AutoAllowable::ALL {
            assert_eq!(AutoAllowable::try_from(a.scope()), Ok(a));
        }
        // The source has exactly three variants — a fourth would have to be
        // added by name, in the enum, past this line.
        let me = include_str!("agent_grants.rs");
        let start = me.find("pub enum AutoAllowable {").unwrap();
        let body = &me[start..start + me[start..].find('}').unwrap()];
        assert!(!body.contains("Destroy") && !body.contains("System"), "{body}");

        let mut on = AutoAllowScopes::default();
        assert!(on.on().is_empty(), "auto-allow defaults to nothing");
        on.set(AutoAllowable::Manage, true);
        assert!(on.is_on(AutoAllowable::Manage) && !on.is_on(AutoAllowable::Read));
        on.set(AutoAllowable::Manage, true);
        assert_eq!(on.on(), vec![AutoAllowable::Manage], "setting twice is once");
        on.set(AutoAllowable::Manage, false);
        assert!(on.on().is_empty());

        // In AppState, never in the settings table (a bypass must not survive
        // a restart — the `agent_db::AutoAllow` rule, re-asserted for this type).
        let app = include_str!("../state/app.rs");
        assert!(app.contains("agent_site_auto_allow: Mutex<crate::core::agent_grants::AutoAllowScopes>"));
        assert!(!include_str!("settings_access.rs").contains("auto_allow"));

        // The gate is auto-allow-unaware: the bypass is at the call site.
        let gate_start = me.find("pub fn authorize(").expect("the gate");
        let rest = &me[gate_start + 10..];
        let gate_end = rest.find("\nfn ").map(|i| gate_start + 10 + i).unwrap_or(me.len());
        let gate = &me[gate_start..gate_end];
        assert!(gate.len() > 200, "the gate body was not located");
        assert!(!gate.contains("AutoAllow") && !gate.contains("auto_allow"), "the gate became auto-allow-aware");

        assert!(AUTO_GRANTED_NOTE.contains("not asked") && AUTO_GRANTED_NOTE.contains("revoke"));
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
        // `read`/`destroy` have no stack meaning and are refused as a shape, not
        // by looking for a grant.
        let shape = authorize(&conn, Target::Stack, Scope::Destroy, "claude-code").unwrap_err().to_string();
        assert!(shape.contains("per-site"), "{shape}");

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
}

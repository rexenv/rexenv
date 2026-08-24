//! The database principals an MCP agent connects AS — names, and the SQL that
//! provisions them (`docs/PLAN-mcp-server.md` §3.6, M3 stage 2).
//!
//! ## Why this is not `dbmirror`
//!
//! `dbmirror` creates a user that stands in for a DEVELOPER's own account: it
//! holds their password and `ALL PRIVILEGES`, because their app has to keep
//! working. An agent principal is the opposite request — the least privilege
//! that answers a question — so it gets its own module rather than a flag on
//! that one. What IS shared is the part that was a live bug: `grant_db_object`,
//! which escapes `_` and `%` so a grant names exactly one database instead of a
//! wildcard pattern that also matches its siblings (ledger #196). Sharing the
//! escape rather than copying it is deliberate: a second copy is a second place
//! for that bug to come back, and it came back once already before the escape
//! existed.
//!
//! ## What this module does NOT do
//!
//! It builds strings and nothing else. The statements here are provisioning —
//! they run ONCE, as root, through the ordinary admin path, because only root
//! can `CREATE USER`. The agent's own queries never come near this module or
//! `client_base_args`; they go through the native driver, which is the point of
//! §3.6's hole 1 (`mysql -e "system id"` runs a shell before the server sees a
//! statement, so a `GRANT SELECT` principal handed to the bundled client is
//! shell-exec and file-write on a real site).
//!
//! Splitting the SQL out this way is also what makes the loopback-only and
//! least-privilege guarantees testable without an engine — the same split
//! `dbmirror::mirror_sql` uses, for the same reason.

use crate::core::dbmirror::{grant_db_object, sql_str, HOSTS, RESERVED_USERS, USER_NAME_MAX};
use crate::error::{Error, Result};

/// Which principal, and therefore how much it may do.
///
/// The two arms are not a preference the caller expresses — they follow from
/// what the database IS. A scratch site's schema is disposable and the agent
/// created it, so `ALL` on that one schema costs nothing. A real site's schema
/// holds the developer's data, so the answer is `SELECT` and there is no
/// second option: the T1 dialog promises "it cannot modify or delete
/// anything", and a privilege level chosen per call is a promise the user was
/// not shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Principal {
    /// A scratch site the agent itself created. `ALL` on its own escaped
    /// schema — it may drop that schema, which is harmless because disposable
    /// is what scratch means.
    Scratch,
    /// A real site. `SELECT` only, forever. `DROP DATABASE` and every write are
    /// unavailable by this fact rather than by a check somewhere that could be
    /// forgotten.
    ReadOnly,
}

impl Principal {
    /// The user-name prefix. Distinct per arm so that reading a `SHOW GRANTS`
    /// or a `mysql.user` dump tells you which kind of principal you are looking
    /// at without cross-referencing anything.
    fn prefix(self) -> &'static str {
        match self {
            Principal::Scratch => "rex_agent_",
            Principal::ReadOnly => "rex_ro_",
        }
    }

    /// The privilege list for a GRANT. `SELECT` is not "the default for now" —
    /// see the type docs.
    fn privileges(self) -> &'static str {
        match self {
            Principal::Scratch => "ALL PRIVILEGES",
            Principal::ReadOnly => "SELECT",
        }
    }
}

/// The principal name for one site, GENERATED — never caller-supplied.
///
/// Derived the same way `dbmirror::dedicated_user_name` derives its own, and
/// for the same two reasons: MySQL caps a user name at 32 bytes, and a name
/// that overflows must still be UNIQUE per site rather than truncating two
/// sites onto one principal. Overflow keeps a readable head and appends the
/// FNV-1a hash of the FULL domain, so `verylongsite…` and `verylongsite…2`
/// cannot collapse into the same account.
///
/// The `rex_` prefix means the result can never land in [`RESERVED_USERS`],
/// which is why [`provision_sql`] treats that check firing as a bug rather than
/// a user-facing outcome.
pub fn principal_name(kind: Principal, domain: &str) -> String {
    let slug: String = domain
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect();
    let clean = format!("{}{slug}", kind.prefix());
    if clean.len() <= USER_NAME_MAX {
        return clean;
    }
    let suffix = format!("{:08x}", crate::core::wordpress::fnv1a(domain.as_bytes()));
    let keep = USER_NAME_MAX - 1 - suffix.len();
    let head: String = clean.chars().take(keep).collect();
    format!("{head}_{suffix}")
}

/// The statements that create one agent principal and grant it its single
/// database.
///
/// **Passwordless, and that is not a shortcut.** The account exists only on
/// `localhost`/`127.0.0.1` — [`HOSTS`], the same two scopes and the same
/// absence of `'%'` that mirroring uses. Any process that could reach it can
/// already reach the engine's root-on-loopback socket, so a password here would
/// protect nothing while adding a secret to store, rotate and leak. What bounds
/// this principal is the grant, not a credential.
///
/// **Idempotent** (`CREATE USER IF NOT EXISTS`) because a re-grant after an
/// expiry must be able to run over an account that may still exist.
pub fn provision_sql(kind: Principal, db: &str, user: &str) -> Result<String> {
    crate::core::database::validate_db_name(db)?;
    if user.len() > USER_NAME_MAX {
        return Err(Error::Other(format!(
            "the agent principal {user:?} is longer than MySQL allows ({USER_NAME_MAX})"
        )));
    }
    if RESERVED_USERS.iter().any(|r| r.eq_ignore_ascii_case(user)) {
        // Unreachable via `principal_name` (every result is `rex_`-prefixed).
        // Reported as the bug it would be rather than mapped to an outcome.
        return Err(Error::Other(format!(
            "refusing to provision the reserved account {user:?} as an agent principal"
        )));
    }
    let u = sql_str(user);
    let db_obj = grant_db_object(db);
    let privileges = kind.privileges();
    let mut sql = String::new();
    for host in HOSTS {
        sql.push_str(&format!("CREATE USER IF NOT EXISTS '{u}'@'{host}';\n"));
        // The privileges are re-stated rather than assumed: a principal that
        // existed from an earlier grant must not keep whatever it had then.
        sql.push_str(&format!("REVOKE ALL PRIVILEGES, GRANT OPTION FROM '{u}'@'{host}';\n"));
        sql.push_str(&format!("GRANT {privileges} ON {db_obj}.* TO '{u}'@'{host}';\n"));
    }
    sql.push_str("FLUSH PRIVILEGES;\n");
    Ok(sql)
}

/// Drop one agent principal — exactly the two [`HOSTS`] scopes
/// [`provision_sql`] created, nothing broader.
///
/// This is what revocation and site-deletion run. It is separate from the
/// ledger row on purpose: `store::revoke_agent_db_grant` records WHEN access
/// stopped and this makes it stop. A revoked row with a live account would be a
/// UI that lies, and a dropped account with a live row would be an agent
/// getting connection errors instead of a refusal.
pub fn drop_sql(user: &str) -> String {
    let u = sql_str(user);
    let mut sql = String::new();
    for host in HOSTS {
        sql.push_str(&format!("DROP USER IF EXISTS '{u}'@'{host}';\n"));
    }
    sql
}

/// One agent's outstanding ASK to read a real site's database.
///
/// **Session-scoped and in memory on purpose.** A request is about a
/// conversation happening now: an agent asked, the user sees it, the user
/// answers. Persisting it would mean a request from last Tuesday could be
/// approved today, granting something nobody remembers being asked — a consent
/// prompt whose context is gone is not consent. Losing these on quit is the
/// correct behaviour, not a limitation: the agent asks again, and the user is
/// asked again while they can still see why.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantRequest {
    pub site_id: String,
    pub domain: String,
    pub client: String,
}

/// The outstanding asks, newest first, deduplicated by (site, client).
///
/// A retry loop must not become a list of a hundred identical prompts — the
/// same agent asking for the same site twice is one question, and answering it
/// answers both.
#[derive(Debug, Default)]
pub struct GrantRequests(Vec<GrantRequest>);

impl GrantRequests {
    /// Record an ask. Returns whether it was NEW, so a caller can decide
    /// whether anything needs surfacing.
    pub fn ask(&mut self, req: GrantRequest) -> bool {
        if self.0.iter().any(|r| r.site_id == req.site_id && r.client == req.client) {
            return false;
        }
        self.0.insert(0, req);
        // A cap, because the list is driven by whatever an agent sends. Without
        // one, an agent asking about invented site ids is a way to grow this
        // without bound in the app's memory, and to bury a real ask under noise.
        self.0.truncate(MAX_REQUESTS);
        true
    }

    pub fn list(&self) -> &[GrantRequest] {
        &self.0
    }

    /// Clear one ask — what answering it (either way) does. Denying and
    /// granting both remove it, because both are answers.
    pub fn answer(&mut self, site_id: &str, client: &str) {
        self.0.retain(|r| !(r.site_id == site_id && r.client == client));
    }
}

/// The most outstanding asks kept. Small on purpose: this is a prompt list a
/// human reads, not a log.
pub const MAX_REQUESTS: usize = 20;

/// How long a granted read lasts. The number in the consent dialog's own words
/// — "This access expires in 7 days" — so it lives beside nothing else that
/// could disagree with it.
pub const GRANT_DAYS: u32 = 7;

/// Who an agent may connect AS for one `db_query` call, decided from recorded
/// facts alone.
///
/// **This is the gate, and it is a pure function on purpose.** The decision
/// "may this agent read this database" is the whole security value of M3, and a
/// decision spread across a handler that also resolves paths, opens
/// connections and formats rows is a decision nobody can read in one sitting.
/// Everything it needs is passed in; the only way to widen it is to edit it.
///
/// `is_scratch` is the RECORDED ownership fact (`core::scratch::claim`), never
/// a domain-suffix guess — a user's own site called `foo.scratch.rex` must not
/// become writable because its name reads like one.
pub fn authorize(
    conn: &rusqlite::Connection,
    site_id: &str,
    domain: &str,
    is_scratch: bool,
    client: &str,
) -> Result<(Principal, String)> {
    if is_scratch {
        // The agent created it, it is disposable, and it holds nothing the user
        // put there. No consent to ask for — there is no one to ask about.
        return Ok((Principal::Scratch, principal_name(Principal::Scratch, domain)));
    }
    match crate::state::store::active_agent_db_grant(conn, site_id, client)? {
        Some(_) => Ok((Principal::ReadOnly, principal_name(Principal::ReadOnly, domain))),
        // The message is the ONLY thing an agent sees, so it says what is
        // missing, who has to do it, and where — a bare "denied" teaches an
        // agent to retry, which is the worst possible response to a consent
        // boundary. It deliberately does not say "ask the user to approve",
        // because an agent that relays that becomes the thing doing the asking.
        None => Err(Error::Other(format!(
            "reading the database of `{domain}` needs the user's approval, which has not been \
             given (or has expired). rexenv is asking for it now, in the app: \
             Settings → \"AI agents (MCP)\" → \"Database access\", where it can be allowed for 7 \
             days or refused. That section also lists every grant and when it expires. This is \
             not something the agent can grant itself."
        ))),
    }
}

/// Create (or re-grant) one agent principal on a running engine.
///
/// Runs as root through the bundled client, because only root can `CREATE
/// USER` — that is an ADMIN operation and it is meant to use the admin path.
/// The agent's own queries do not come through here; they go through
/// `core::agent_query`, which is source-guarded against ever reaching this
/// client. Keeping the two apart in different modules is the point: the
/// privileged path and the agent path should not be one function with a flag.
pub fn provision(
    client: &crate::core::db::SqlClient,
    port: u16,
    kind: Principal,
    db: &str,
    user: &str,
) -> Result<()> {
    let sql = provision_sql(kind, db, user)?;
    crate::core::dbmirror::run_sql(client, port, &sql, "agent principal provisioning")
}

/// Drop one agent principal — what revocation and site deletion run.
pub fn deprovision(client: &crate::core::db::SqlClient, port: u16, user: &str) -> Result<()> {
    if RESERVED_USERS.iter().any(|r| r.eq_ignore_ascii_case(user)) {
        // A drop must be unable to take root out even from a corrupt record —
        // the same rule `dbmirror::drop_mirrored` states, for the same reason.
        return Err(Error::Other(format!(
            "refusing to drop reserved database account {user:?}"
        )));
    }
    crate::core::dbmirror::run_sql(client, port, &drop_sql(user), "agent principal cleanup")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The delete path finds a SCRATCH principal, which no grant row names.**
    ///
    /// The first version of the delete cleanup collected accounts from
    /// `agent_db_grants` alone. A scratch site needs no consent and therefore
    /// has no grant row, so its `rex_agent_<slug>` was never found and stayed on
    /// the engine after the site and its database were gone — **measured
    /// 25 Aug 2026 against the packaged app**, running the §M3 gate: the
    /// database was dropped, the account was not.
    ///
    /// The fix is to derive BOTH arms' names from the site's domain and union
    /// them with whatever the grants recorded. Neither source is redundant: the
    /// derived names are the only way to reach a scratch principal, and the
    /// recorded ones are the only way to reach an account created before a
    /// site was RENAMED, since it carries the old domain's slug.
    #[test]
    fn the_delete_path_covers_the_scratch_arm_that_no_grant_row_can_name() {
        let src = include_str!("../commands/sites.rs");
        let block_start = src.find("let (agent_users, had_grant)").expect("the collection");
        let block = &src[block_start..block_start + 1800];
        for arm in ["Principal::Scratch", "Principal::ReadOnly"] {
            assert!(
                block.contains(arm),
                "the delete path no longer derives {arm}'s account name. If this went back to \
                 reading grant rows only, a scratch principal has no row to be found by and \
                 survives its own site — measured, not theorised."
            );
        }
        assert!(
            block.contains("list_agent_db_grants"),
            "the recorded names are gone — an account created before the site was RENAMED \
             carries the OLD domain's slug and cannot be derived from today's"
        );
    }

    /// **A deleted site's agent accounts are read while the site still exists.**
    ///
    /// `agent_db_grants` cascades on the sites row, so a delete path that reads
    /// the accounts AFTER removing the site gets an empty list and silently
    /// leaves them on the engine — and the name is derived from the domain, so
    /// a future site at that domain inherits an account still holding SELECT on
    /// a database name that collides by construction. A grant the user gave
    /// once, to a site that no longer exists, would come back attached to a
    /// different one. That is the same lifetime class the mirrored-user drop
    /// already carries a comment about.
    ///
    /// Asserted as an ORDERING in the delete path's source, because the failure
    /// is invisible at runtime: the wrong order returns an empty list, drops
    /// nothing, and reports success.
    ///
    /// **Honest about which half this actually carries.** A plant that moves the
    /// collection below the drop does not COMPILE — `agent_users` goes out of
    /// scope — so the compiler already owns that half, and the assertion on it
    /// is belt-and-braces rather than the guard. What is genuinely unproven by
    /// the compiler is the second assertion: a future edit that moves the whole
    /// drop block below the row removal would compile fine and silently drop
    /// nothing. No plant was constructed for that one — it needs relocating two
    /// separate blocks — so it is a source assertion taken on its reading, not a
    /// proof, and it is recorded that way rather than counted as one.
    #[test]
    fn a_sites_agent_accounts_are_collected_before_the_row_that_cascades_them() {
        let src = include_str!("../commands/sites.rs");
        let collect = src.find("let (agent_users, had_grant)").expect("the collection");
        let drop_users = src.find("core::agent_db::deprovision(").expect("the drop");
        assert!(collect < drop_users, "the accounts are dropped before they are known");
        // The row deletion is what cascades the grants away. Located by the
        // delete path's own step-3 marker rather than by a bare `delete_site`,
        // which appears in several places.
        let row_gone = src.find("// 3) Row + cert").expect("the row-removal step");
        assert!(
            collect < row_gone,
            "the grants are read AFTER the site row is removed — the cascade has already \
             emptied them, so this drops nothing and reports success"
        );
    }

    /// **An agent's retry loop is one prompt, not a hundred — and both answers
    /// clear it.**
    ///
    /// The ask is recorded on the REFUSAL path, so an agent that keeps trying
    /// keeps hitting the same question. If each attempt appended, a user would
    /// come back to a wall of identical rows and the real ask underneath
    /// somebody else's noise; and since the list is driven entirely by what an
    /// agent sends, an unbounded one is also a way to grow the app's memory
    /// from outside.
    #[test]
    fn repeated_asks_are_one_prompt_and_answering_either_way_clears_it() {
        let mut reqs = GrantRequests::default();
        let ask = |site: &str, client: &str| GrantRequest {
            site_id: site.into(),
            domain: format!("{site}.rex"),
            client: client.into(),
        };

        assert!(reqs.ask(ask("s1", "Claude Code")), "the first ask is new");
        assert!(!reqs.ask(ask("s1", "Claude Code")), "a retry is the same question");
        assert_eq!(reqs.list().len(), 1);

        // A different client asking about the same site IS a different
        // question — that is the re-consent rule, seen from the prompt side.
        assert!(reqs.ask(ask("s1", "Another Agent")));
        // …and so is the same client asking about a different site.
        assert!(reqs.ask(ask("s2", "Claude Code")));
        assert_eq!(reqs.list().len(), 3);
        assert_eq!(reqs.list()[0].site_id, "s2", "newest first");

        // Denying clears it, exactly as granting does: a denial is an answer,
        // and a prompt that only disappears on approval makes "no" the one
        // response the UI cannot express.
        reqs.answer("s1", "Another Agent");
        assert_eq!(reqs.list().len(), 2);
        assert!(!reqs.list().iter().any(|r| r.client == "Another Agent"));

        // The cap holds against an agent inventing site ids.
        for i in 0..100 {
            reqs.ask(ask(&format!("bulk{i}"), "Noisy Agent"));
        }
        assert_eq!(reqs.list().len(), MAX_REQUESTS, "the prompt list grew without bound");
    }

    /// **A real site is unreadable without a live grant, and a scratch site
    /// never needs one — decided from the RECORDED ownership fact.**
    ///
    /// The gate is one function precisely so this test is the whole story. Note
    /// what it does not test: nothing about the domain. A user's own site named
    /// `looks-like.scratch.rex` is a real site here, because `is_scratch` comes
    /// from `core::scratch::claim` reading the ownership row — the same
    /// distinction `scratch_delete_site` refuses on, for the same reason.
    #[test]
    fn a_real_site_needs_a_live_grant_and_a_scratch_site_never_does() {
        use crate::state::{db, store};
        let conn = db::open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO sites (id, name, domain, type, status, php_version, web_server, ssl,
                                path, db_name, db_engine)
             VALUES ('s1','S','shop.rex','wordpress','stopped','8.3','nginx',1,'/tmp/s','wp_shop','mysql')",
            [],
        )
        .unwrap();

        // No grant: refused, and the refusal NAMES the site and where approval
        // lives. An agent told only "denied" retries; an agent told what is
        // missing reports it.
        let err = authorize(&conn, "s1", "shop.rex", false, "Claude Code").unwrap_err().to_string();
        assert!(err.contains("shop.rex"), "{err}");
        // The FULL path, not just "Settings". A pointer that names the app's
        // settings and not the section is what a person actually fails on:
        // running §M3 on 25 Aug 2026 the first question back was "where do I
        // click Allow?", against a message that said "Settings → MCP" while the
        // card is titled "AI agents (MCP)" and the section "Database access" —
        // three names for one place. A refusal a human cannot follow is a
        // broken consent path, not a wording nit.
        assert!(err.contains("AI agents (MCP)"), "the refusal must name the CARD: {err}");
        assert!(err.contains("Database access"), "…and the SECTION in it: {err}");
        assert!(err.contains("7 days"), "…and what allowing actually grants: {err}");
        assert!(err.contains("not something the agent can grant itself"), "{err}");

        // A scratch site is readable and WRITABLE with no grant at all — there
        // is no user data in it and nobody to ask.
        let (p, user) = authorize(&conn, "s2", "tmp.scratch.rex", true, "Claude Code").unwrap();
        assert_eq!(p, Principal::Scratch);
        assert_eq!(user, "rex_agent_tmp_scratch_rex");

        // With a live grant the real site opens — READ-ONLY, never Scratch.
        store::grant_agent_db(&conn, "g1", "s1", "Claude Code", "rex_ro_shop_rex", 7).unwrap();
        let (p, user) = authorize(&conn, "s1", "shop.rex", false, "Claude Code").unwrap();
        assert_eq!(p, Principal::ReadOnly, "a granted real site must never get a writing principal");
        assert_eq!(user, "rex_ro_shop_rex");

        // A DIFFERENT client is not covered by it. This is the re-consent rule.
        assert!(authorize(&conn, "s1", "shop.rex", false, "Another Agent").is_err());

        // Revoking closes it again, without deleting the evidence.
        store::revoke_agent_db_grant(&conn, "g1").unwrap();
        assert!(authorize(&conn, "s1", "shop.rex", false, "Claude Code").is_err());
        assert_eq!(store::list_agent_db_grants(&conn).unwrap().len(), 1);

        // …and so does expiry, which is the same gate reading the same column.
        store::grant_agent_db(&conn, "g2", "s1", "Claude Code", "rex_ro_shop_rex", 7).unwrap();
        assert!(authorize(&conn, "s1", "shop.rex", false, "Claude Code").is_ok());
        conn.execute(
            "UPDATE agent_db_grants SET expires_at = datetime('now','-1 hour') WHERE id='g2'",
            [],
        )
        .unwrap();
        assert!(authorize(&conn, "s1", "shop.rex", false, "Claude Code").is_err(), "an expired grant still opened the database");
    }

    /// **A real site's agent principal can read and can do nothing else, and it
    /// can reach exactly one database.**
    ///
    /// Both halves are what the T1 dialog promises in words — "it will be able
    /// to read everything in it… it cannot modify or delete anything" — so this
    /// test is the sentence, in SQL.
    #[test]
    fn a_read_only_principal_gets_select_on_exactly_one_escaped_database() {
        let user = principal_name(Principal::ReadOnly, "shop.rex");
        assert_eq!(user, "rex_ro_shop_rex");

        let sql = provision_sql(Principal::ReadOnly, "wp_shop", &user).unwrap();

        // SELECT, and no write verb anywhere. Checked as an absence because the
        // failure mode is an EXTRA privilege sneaking in beside the right one.
        assert!(sql.contains("GRANT SELECT ON"), "{sql}");
        for forbidden in ["INSERT", "UPDATE", "DELETE", "DROP", "ALL PRIVILEGES ON", "FILE"] {
            assert!(!sql.contains(forbidden), "a read-only principal was granted {forbidden}:\n{sql}");
        }

        // The escape: `wp_shop` unescaped ALSO grants on `wpashop`/`wpXshop`,
        // because MySQL reads `_` as a wildcard in this position even inside
        // backticks. That was a live cross-site over-grant once (#196).
        assert!(sql.contains(r"GRANT SELECT ON `wp\_shop`.* TO"), "{sql}");

        // Loopback only — the two HOSTS scopes and no `'%'`.
        assert!(sql.contains("'rex_ro_shop_rex'@'localhost'"));
        assert!(sql.contains("'rex_ro_shop_rex'@'127.0.0.1'"));
        assert!(!sql.contains("@'%'"), "an agent principal was reachable from anywhere:\n{sql}");

        // Passwordless by design (see provision_sql): no credential to leak,
        // and nothing gained by one on loopback. A password appearing here
        // would mean someone added a secret this design has nowhere to store.
        assert!(!sql.contains("IDENTIFIED BY"), "{sql}");

        // Re-provisioning must not let an older, wider grant survive.
        assert_eq!(sql.matches("REVOKE ALL PRIVILEGES").count(), 2, "{sql}");
        assert!(
            sql.find("REVOKE ALL").unwrap() < sql.find("GRANT SELECT").unwrap(),
            "the revoke must precede the grant, or it removes what it just gave:\n{sql}"
        );

        // Idempotent: a re-grant after an expiry runs over a live account.
        assert_eq!(sql.matches("CREATE USER IF NOT EXISTS").count(), 2, "{sql}");

        // Dropping removes the two scopes provisioning created, nothing wider.
        let d = drop_sql(&user);
        assert_eq!(d.matches("DROP USER").count(), 2, "{d}");
        assert!(!d.contains("@'%'"), "{d}");
    }

    /// Scratch is the only arm that may write, and only on its own disposable
    /// schema — still ONE escaped database, not a pattern.
    #[test]
    fn a_scratch_principal_may_write_but_still_only_its_own_schema() {
        let user = principal_name(Principal::Scratch, "tmp.rex");
        assert_eq!(user, "rex_agent_tmp_rex");
        let sql = provision_sql(Principal::Scratch, "wp_tmp", &user).unwrap();
        assert!(sql.contains(r"GRANT ALL PRIVILEGES ON `wp\_tmp`.* TO"), "{sql}");
        assert!(!sql.contains("@'%'"), "{sql}");
        // `ON *.*` would be `ALL PRIVILEGES` on the whole ENGINE — every other
        // site's data — from a grant that reads almost identically.
        assert!(!sql.contains("ON *.*"), "{sql}");
    }

    /// The two prefixes must not be confusable, because the name is what a
    /// human reads in `mysql.user` when deciding whether a principal should
    /// exist at all.
    #[test]
    fn the_two_principal_kinds_never_produce_the_same_name() {
        let a = principal_name(Principal::Scratch, "shop.rex");
        let b = principal_name(Principal::ReadOnly, "shop.rex");
        assert_ne!(a, b);
        // …including at the length cap, where truncation is what could collide.
        let long = "a-very-long-development-domain-name-indeed.rex";
        let la = principal_name(Principal::Scratch, long);
        let lb = principal_name(Principal::ReadOnly, long);
        assert_ne!(la, lb, "truncation collapsed a writer and a reader into one account");
        assert!(la.len() <= USER_NAME_MAX && lb.len() <= USER_NAME_MAX, "{la} / {lb}");
    }

    /// Two long domains that share a prefix must not share a principal — the
    /// account is the security boundary, so a truncation collision hands one
    /// site's agent another site's grant.
    #[test]
    fn long_domains_sharing_a_prefix_get_different_principals() {
        let a = principal_name(Principal::ReadOnly, "a-very-long-development-domain-one.rex");
        let b = principal_name(Principal::ReadOnly, "a-very-long-development-domain-two.rex");
        assert!(a.len() <= USER_NAME_MAX, "{a}");
        assert_ne!(a, b, "two sites collapsed onto one principal");
    }

    /// A name that could reach a reserved account is refused, and a database
    /// name that never passed validation cannot reach the SQL builder at all —
    /// the escape is only sufficient BECAUSE the name is `[A-Za-z0-9_]`.
    #[test]
    fn reserved_users_and_unvalidated_database_names_are_refused() {
        assert!(provision_sql(Principal::ReadOnly, "wp_x", "root").is_err());
        assert!(provision_sql(Principal::ReadOnly, "wp_x", "ROOT").is_err(), "case-insensitive");
        assert!(provision_sql(Principal::ReadOnly, "wp`x", "rex_ro_x").is_err(), "backtick");
        assert!(provision_sql(Principal::ReadOnly, "wp x", "rex_ro_x").is_err(), "space");
        assert!(provision_sql(Principal::ReadOnly, "wp_x", &"z".repeat(33)).is_err(), "too long");
    }
}

//! The database principals an MCP agent connects AS — names, and the SQL that
//! provisions them (`docs/archive/PLAN-mcp-server.md` §3.6, M3 stage 2).
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

/// How long the RECORD of a provisioned read-only principal is kept (D16).
/// Not a consent expiry — consent is the Agent access dial — but the row that
/// remembers which account was made for which site under which domain, so a
/// rename cannot orphan it (#403). Ten years is "for the life of the site".
pub const PROVISION_RECORD_DAYS: u32 = 3650;

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
pub fn authorize(conn: &rusqlite::Connection, is_scratch: bool, domain: &str) -> Result<(Principal, String)> {
    if is_scratch {
        // The agent created it, it is disposable, and it holds nothing the user
        // put there. No consent to ask for — there is no one to ask about.
        return Ok((Principal::Scratch, principal_name(Principal::Scratch, domain)));
    }
    // D16: the user's own site is answered by the Agent access dial at Read —
    // the level every read on their sites needs, and the dial's floor while
    // the endpoint is on. Still SELECT-only on that one database (#399); the
    // refusal, should a level below Read ever exist, names the dial.
    let now = crate::core::agent_access::current(conn)?;
    if now.level < crate::core::agent_access::AccessLevel::Read {
        return Err(Error::Other(crate::core::agent_access::refusal(
            &format!("reading the database of `{domain}`"),
            crate::core::agent_grants::Scope::Read,
            &now,
        )));
    }
    Ok((Principal::ReadOnly, principal_name(Principal::ReadOnly, domain)))
}

/// Remember that `user` was provisioned for `site_id` on behalf of `client` —
/// the D16 meaning of an `agent_db_grants` row: not consent (the dial is), but
/// the record the site's delete path reads to drop an account made under a
/// domain the site no longer has (#403). Keyed by (site, PRINCIPAL): a rename
/// makes a new principal and a new row, the same principal twice makes none,
/// and an agent varying its `clientInfo` name cannot grow the table (the
/// review's find — the first version was keyed by client and frozen at the
/// first name). Call it AFTER the provision succeeded, so a row never names an
/// account that was never made. Returns whether a row was written.
pub fn record_principal(conn: &rusqlite::Connection, site_id: &str, client: &str, user: &str) -> Result<bool> {
    let known = crate::state::store::list_agent_db_grants(conn)?
        .into_iter()
        .any(|g| g.site_id == site_id && g.db_user == user);
    if known {
        return Ok(false);
    }
    crate::state::store::grant_agent_db(conn, &uuid::Uuid::new_v4().to_string(), site_id, client, user, PROVISION_RECORD_DAYS, false)?;
    Ok(true)
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


    /// **No user-facing message in this module contains a run of spaces.**
    ///
    /// Rust's `\` line-continuation strips the newline and the following
    /// indentation — but only when the backslash is actually there. Write the
    /// same paragraph as adjacent string literals and forget it, and every line
    /// break becomes a run of spaces in the text the user (or the agent) reads.
    /// It happened to the consent refusal, was fixed by hand, and then happened
    /// again to `AUTO_GRANTED_NOTE` — twice is a guard.
    #[test]
    fn the_user_facing_messages_carry_no_accidental_run_of_spaces() {
        // The refusal an agent would read below Read (D16 — through the dial's
        // own text) comes from string continuations, where a stray double
        // space is the usual slip.
        let conn = crate::state::db::open_in_memory().unwrap();
        let r = crate::core::agent_access::refusal("reading the database of `shop.rex`", crate::core::agent_grants::Scope::Read, &crate::core::agent_access::current(&conn).unwrap());
        assert!(!r.contains("  "), "double space in the refusal: {r}");
    }

    /// **The gate is one pure function over recorded facts: a scratch site is
    /// its own principal with no consent to ask for; the user's own site is
    /// the SELECT-only principal at the dial's Read level (D16) — the floor
    /// while the endpoint is on, so there is no prompt, no row and no client
    /// in the decision; and the refusal that would fire below Read names the
    /// dial.**
    #[test]
    fn a_users_site_reads_at_the_dials_read_and_a_scratch_site_writes_its_own() {
        let conn = crate::state::db::open_in_memory().unwrap();
        let (p, u) = authorize(&conn, true, "shop.scratch.rex").unwrap();
        assert_eq!((p, u.as_str()), (Principal::Scratch, principal_name(Principal::Scratch, "shop.scratch.rex").as_str()));
        let (p, u) = authorize(&conn, false, "shop.rex").unwrap();
        assert_eq!((p, u.as_str()), (Principal::ReadOnly, principal_name(Principal::ReadOnly, "shop.rex").as_str()));
        // Every level of the dial answers a read — Read is the floor.
        for lvl in crate::core::agent_access::LEVELS {
            let mode = if lvl == crate::core::agent_access::AccessLevel::Read { None } else { Some(crate::core::agent_access::Mode::Always) };
            crate::core::agent_access::set(&conn, lvl, mode).unwrap();
            assert_eq!(authorize(&conn, false, "shop.rex").unwrap().0, Principal::ReadOnly, "{lvl:?}");
        }
        // The refusal shape, for the day a level below Read exists: the
        // dial, both levels, where to turn it — never "ask the user".
        let r = crate::core::agent_access::refusal("reading the database of `shop.rex`", crate::core::agent_grants::Scope::Read, &crate::core::agent_access::current(&conn).unwrap());
        assert!(r.contains("`Agent access`") && r.contains("AI agents (MCP)") && !r.contains("ask the user"), "{r}");
        // Nothing here wrote a grant row: consent is not a row any more.
        assert!(crate::state::store::list_agent_db_grants(&conn).unwrap().is_empty());
    }

    /// **The provisioning record is keyed by (site, principal): written once
    /// per principal, again after a rename made a new one, never for a second
    /// client name, so the delete path can find every account and an agent
    /// cannot grow the table by renaming itself.**
    #[test]
    fn the_provisioning_record_is_one_row_per_principal_and_a_rename_adds_one() {
        use crate::state::models::{test_site, SiteOrigin};
        let conn = crate::state::db::open_in_memory().unwrap();
        let site = test_site("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", "shop.rex", SiteOrigin::User);
        crate::state::store::insert_site(&conn, &site).unwrap();
        let first = principal_name(Principal::ReadOnly, "shop.rex");
        assert!(record_principal(&conn, &site.id, "claude-code", &first).unwrap());
        assert!(!record_principal(&conn, &site.id, "claude-code", &first).unwrap(), "the same principal twice is one row");
        assert!(!record_principal(&conn, &site.id, "cursor", &first).unwrap(), "a second client name is not a second row");
        let renamed = principal_name(Principal::ReadOnly, "store.rex");
        assert!(record_principal(&conn, &site.id, "claude-code", &renamed).unwrap(), "a rename's new principal is recorded");
        let rows = crate::state::store::list_agent_db_grants(&conn).unwrap();
        let users: Vec<&str> = rows.iter().filter(|g| g.site_id == site.id).map(|g| g.db_user.as_str()).collect();
        assert_eq!(rows.len(), 2);
        assert!(users.contains(&first.as_str()) && users.contains(&renamed.as_str()), "{users:?}");
        // The record is written AFTER the provision in db_query — a row must
        // never name an account that was never made. Source order, pinned.
        let scratch = include_str!("../mcp_server/scratch.rs");
        let handler = &scratch[scratch.find("fn db_query<'a>(").unwrap()..];
        let provision_at = handler.find("agent_db::provision(").expect("the provision call");
        let record_at = handler.find("agent_db::record_principal(").expect("the record call");
        assert!(provision_at < record_at, "the provisioning record must follow the provision");
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

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

#[cfg(test)]
mod tests {
    use super::*;

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

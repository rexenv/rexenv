//! core::dbmirror — mirroring a site's OWN database credentials into our
//! engine (Stage 2 step 7), so their existing config keeps working and Stage
//! 3's rewrite shrinks to host/port.
//!
//! Three rules, all structural or SQL-shaped rather than remembered:
//!
//! - **Never our root** (or any reserved account). Every rexenv database
//!   operation authenticates as passwordless root through one pinned flag
//!   array (`client_base_args`); touching root's password breaks all of them
//!   at once. [`mirror`] refuses reserved names as an OUTCOME, not an error —
//!   the caller reports "your config connects as root; the interim change is
//!   three keys" rather than a failure.
//! - **Loopback-scoped, never `'%'`.** The mirrored user is created for
//!   `localhost` and `127.0.0.1` only — the two spellings a local site can
//!   reach us by. A `'%'` grant would turn a local-dev convenience into a
//!   network-reachable account with the user's real password on it.
//! - **Idempotent, because Retry reruns it.** `CREATE USER IF NOT EXISTS` +
//!   `ALTER USER` + `GRANT`: the second run converges (and heals a changed
//!   password) instead of erroring on the first statement.
//!
//! The password travels over the client's stdin inside the SQL text — never on
//! argv, never in an env var, never logged (the SQL is not echoed anywhere).

use crate::core::database::{client_base_args, validate_db_name};
use crate::error::{Error, Result};
use std::io::Write;
use std::path::Path;

/// Accounts we never create, alter, or grant as — whatever a site's config
/// says. Checked case-insensitively.
pub const RESERVED_USERS: &[&str] =
    &["root", "mysql.sys", "mysql.session", "mysql.infoschema", "mariadb.sys", "postgres", ""];

/// The two host scopes a mirrored user gets — and the only two. `'%'` is not in
/// this list on purpose, and [`mirror`] builds its SQL exclusively from it.
const HOSTS: [&str; 2] = ["localhost", "127.0.0.1"];

/// What mirroring did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirrorOutcome {
    /// The user exists on our engine with their password, granted this one
    /// database, reachable from loopback only.
    Mirrored { user: String },
    /// Their config connects as a reserved account (almost always root).
    /// Mirroring is impossible by rule; the caller's interim message grows the
    /// user/password lines. NOT an error — the import itself succeeded.
    RefusedReserved { user: String },
}

impl MirrorOutcome {
    pub fn message(&self, db: &str) -> String {
        match self {
            MirrorOutcome::Mirrored { user } => format!(
                "The site's database user `{user}` now works on rexenv's engine too \
                 (local connections only), with access to `{db}` — so its existing \
                 password keeps working and only the host and port differ."
            ),
            MirrorOutcome::RefusedReserved { user } => format!(
                "This site connects as `{user}`, which rexenv never creates or alters on \
                 its own engine — changing it would break every database operation rexenv \
                 performs. When you point the site at rexenv's database, also set its \
                 user to `root` with an empty password (rexenv's local-dev default)."
            ),
        }
    }
}

/// MySQL/MariaDB user-name limit (32 since 5.7.8).
pub const USER_NAME_MAX: usize = 32;

/// Mirror `user`/`password` onto our engine with access to `db`, loopback only.
///
/// The SQL is fed over stdin; nothing secret reaches argv. Statement shape per
/// host in [`HOSTS`]:
///
/// ```sql
/// CREATE USER IF NOT EXISTS 'u'@'h' IDENTIFIED BY 'p';
/// ALTER USER 'u'@'h' IDENTIFIED BY 'p';   -- converge a changed password
/// GRANT ALL PRIVILEGES ON `db`.* TO 'u'@'h';
/// ```
pub fn mirror(
    client: &Path,
    port: u16,
    db: &str,
    user: &str,
    password: &str,
) -> Result<MirrorOutcome> {
    if RESERVED_USERS.iter().any(|r| r.eq_ignore_ascii_case(user)) {
        return Ok(MirrorOutcome::RefusedReserved { user: user.to_string() });
    }
    validate_db_name(db)?;
    if user.len() > USER_NAME_MAX {
        return Err(Error::Other(format!(
            "the database user name {user:?} is longer than MySQL allows ({USER_NAME_MAX})"
        )));
    }

    let sql = mirror_sql(db, user, password);
    run_sql(client, port, &sql, "user mirroring")?;
    Ok(MirrorOutcome::Mirrored { user: user.to_string() })
}

/// The dedicated per-SITE user for a root-owned config (Stage 3 D1):
/// `rex_<domain-slug>`, capped at [`USER_NAME_MAX`] with the same FNV
/// disambiguation as `wordpress::db_name_disambiguated` — two long domains
/// that truncate to the same head still get distinct names, because the
/// suffix hashes the FULL domain (the B21 reasoning, reused).
///
/// Per-SITE, never per-database: two sites sharing one database under a
/// per-database name would reset each other's password through the
/// idempotent `ALTER USER` converge (plan §1 — the one real trap).
///
/// CREATION-TIME ONLY, the `db_name_for` rule: the result is recorded in
/// `db_imports.mirrored_user`, and every later operation — above all the
/// drop at site delete — reads the RECORD, never re-derives from the domain.
/// A domain change between import and delete would otherwise compute a user
/// we never created and leave the real one behind.
pub fn dedicated_user_name(domain: &str) -> String {
    let slug: String = domain
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect();
    let clean = format!("rex_{slug}");
    if clean.len() <= USER_NAME_MAX {
        return clean;
    }
    let suffix = format!("{:08x}", super::wordpress::fnv1a(domain.as_bytes()));
    let keep = USER_NAME_MAX - 1 - suffix.len(); // reserve "_" + the 8-hex suffix
    let head: String = clean.chars().take(keep).collect();
    format!("{head}_{suffix}")
}

/// Create the dedicated user (D1) holding THEIR password, granted this one
/// database, loopback only — the root-case answer that keeps the password
/// line out of every rewrite. Returns the name for recording.
///
/// The name is GENERATED, never caller-supplied, and `rex_`-prefixed, so it
/// cannot land in [`RESERVED_USERS`]; [`mirror`]'s refusal branch firing here
/// is a bug, and reported as one rather than mapped to a user-facing outcome.
pub fn mirror_dedicated(
    client: &Path,
    port: u16,
    db: &str,
    domain: &str,
    password: &str,
) -> Result<String> {
    let user = dedicated_user_name(domain);
    match mirror(client, port, db, &user, password)? {
        MirrorOutcome::Mirrored { user } => Ok(user),
        MirrorOutcome::RefusedReserved { user } => Err(Error::Other(format!(
            "generated dedicated user {user:?} landed in the reserved set — a rexenv bug"
        ))),
    }
}

/// Drop a RECORDED mirrored user at site delete (D3) — both loopback hosts,
/// `IF EXISTS` so a half-cleaned engine converges on rerun.
///
/// The name must come from `db_imports.mirrored_user`, never re-derived. And
/// the reserved refusal is a hard error here (unlike [`mirror`]'s outcome):
/// no record should ever hold a reserved name — mirror never records one —
/// but a drop must be UNABLE to take root out even on a corrupt record.
pub fn drop_mirrored(client: &Path, port: u16, user: &str) -> Result<()> {
    if RESERVED_USERS.iter().any(|r| r.eq_ignore_ascii_case(user)) {
        return Err(Error::Other(format!(
            "refusing to drop reserved database account {user:?}"
        )));
    }
    if user.len() > USER_NAME_MAX {
        return Err(Error::Other(format!(
            "recorded mirrored user {user:?} is longer than MySQL allows ({USER_NAME_MAX})"
        )));
    }
    run_sql(client, port, &drop_sql(user), "mirrored-user cleanup")
}

/// Feed SQL to the bundled client over stdin (never argv), as passwordless
/// root via the pinned [`client_base_args`]. Shared by mirror and drop so the
/// no-argv rule has one implementation.
fn run_sql(client: &Path, port: u16, sql: &str, what: &str) -> Result<()> {
    let mut child = std::process::Command::new(client)
        .args(client_base_args(port))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| Error::Other(format!("starting the client for {what}: {e}")))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| Error::Other(format!("{what} client has no stdin")))?;
    if let Err(e) = stdin.write_all(sql.as_bytes()) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(Error::from(e));
    }
    drop(stdin);
    let out = child.wait_with_output()?;
    if !out.status.success() {
        return Err(Error::Other(format!(
            "{what} failed (exit {:?}): {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}

/// The statements [`mirror`] feeds — separated so the loopback-only and
/// idempotency guarantees are unit-testable without an engine.
fn mirror_sql(db: &str, user: &str, password: &str) -> String {
    let u = sql_str(user);
    let p = sql_str(password);
    let mut sql = String::new();
    for host in HOSTS {
        sql.push_str(&format!("CREATE USER IF NOT EXISTS '{u}'@'{host}' IDENTIFIED BY '{p}';\n"));
        sql.push_str(&format!("ALTER USER '{u}'@'{host}' IDENTIFIED BY '{p}';\n"));
        sql.push_str(&format!("GRANT ALL PRIVILEGES ON `{db}`.* TO '{u}'@'{host}';\n"));
    }
    sql.push_str("FLUSH PRIVILEGES;\n");
    sql
}

/// The statements [`drop_mirrored`] feeds — same testability split. Exactly
/// the two [`HOSTS`] scopes mirroring created, nothing broader.
fn drop_sql(user: &str) -> String {
    let u = sql_str(user);
    let mut sql = String::new();
    for host in HOSTS {
        sql.push_str(&format!("DROP USER IF EXISTS '{u}'@'{host}';\n"));
    }
    sql
}

/// Escape for a single-quoted MySQL string literal.
fn sql_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_users_are_an_outcome_not_an_error_and_never_reach_a_client() {
        // A nonexistent client path proves refusal happens before any spawn.
        for user in ["root", "ROOT", "Root", "mysql.sys", "mariadb.sys", "postgres", ""] {
            match mirror(Path::new("/nonexistent/mysql"), 1, "ea", user, "pw") {
                Ok(MirrorOutcome::RefusedReserved { user: u }) => assert_eq!(u, user),
                other => panic!("{user}: {other:?}"),
            }
        }
        let msg = MirrorOutcome::RefusedReserved { user: "root".into() }.message("ea");
        assert!(msg.contains("never creates or alters"), "{msg}");
        assert!(msg.contains("empty password"), "{msg}");
    }

    #[test]
    fn the_mirrored_user_is_loopback_only_by_construction() {
        let sql = mirror_sql("ea", "ea_user", "pw");
        assert!(sql.contains("'ea_user'@'localhost'"));
        assert!(sql.contains("'ea_user'@'127.0.0.1'"));
        // The dangerous scope does not appear, in any statement.
        assert!(!sql.contains("'%'"), "{sql}");
        // And access is to the one database, not *.*.
        assert!(sql.contains("ON `ea`.*"));
        assert!(!sql.contains("ON *.*"), "{sql}");
    }

    #[test]
    fn the_statements_converge_on_rerun_by_shape() {
        // IF NOT EXISTS + ALTER + GRANT: every statement tolerates its own
        // prior success, which is what makes Retry safe to rerun.
        let sql = mirror_sql("ea", "u", "pw");
        for create in sql.lines().filter(|l| l.starts_with("CREATE USER")) {
            assert!(create.contains("IF NOT EXISTS"), "{create}");
        }
        assert_eq!(sql.matches("ALTER USER").count(), 2, "one password converge per host");
    }

    #[test]
    fn passwords_with_quotes_and_backslashes_survive_into_the_sql_whole() {
        let sql = mirror_sql("ea", "u", r#"p'a\s"s"#);
        assert!(sql.contains(r#"IDENTIFIED BY 'p\'a\\s"s'"#), "{sql}");
        // A user name that would break out of its quotes is neutralised too.
        let sql = mirror_sql("ea", "o'brien", "pw");
        assert!(sql.contains(r"'o\'brien'@'localhost'"), "{sql}");
    }

    #[test]
    fn oversized_user_names_error_before_any_client_runs() {
        let long = "u".repeat(33);
        assert!(mirror(Path::new("/nonexistent/mysql"), 1, "ea", &long, "pw").is_err());
        assert!(drop_mirrored(Path::new("/nonexistent/mysql"), 1, &long).is_err());
    }

    #[test]
    fn dedicated_names_are_per_site_deterministic_and_capped() {
        // The common case reads cleanly.
        assert_eq!(dedicated_user_name("myblog.test"), "rex_myblog_test");
        // Deterministic: same domain, same name, every time.
        assert_eq!(dedicated_user_name("myblog.test"), dedicated_user_name("myblog.test"));
        // Every name fits MySQL's 32-char user limit and its charset is tame.
        for domain in ["myblog.test", "a.b", &format!("{}.test", "x".repeat(80))] {
            let name = dedicated_user_name(domain);
            assert!(name.len() <= USER_NAME_MAX, "{name}");
            assert!(name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'));
        }
    }

    #[test]
    fn two_domains_sharing_a_truncation_get_distinct_names() {
        // The trap the cap invites: long domains identical through the kept
        // head. The suffix hashes the FULL domain (unique_db_name's B21
        // reasoning), so the names differ — otherwise two sites would share
        // one account and the ALTER converge would reset each other's
        // password, which is exactly what per-SITE naming exists to prevent.
        let shared = "very-long-project-domain-name";
        let a = format!("{shared}-alpha.test");
        let b = format!("{shared}-beta.test");
        let (ua, ub) = (dedicated_user_name(&a), dedicated_user_name(&b));
        assert_eq!(&ua[..23], &ub[..23], "precondition: same truncated head");
        assert_ne!(ua, ub, "hash suffix must disambiguate");
        assert!(ua.len() == USER_NAME_MAX && ub.len() == USER_NAME_MAX);
    }

    #[test]
    fn dedicated_names_can_never_be_reserved() {
        // The rex_ prefix keeps the generated name out of the reserved set
        // structurally — mirror_dedicated treats that branch as a bug.
        for domain in ["myblog.test", "root", "mysql.sys", "postgres", ""] {
            let name = dedicated_user_name(domain);
            assert!(name.starts_with("rex_"), "{name}");
            assert!(
                !RESERVED_USERS.iter().any(|r| r.eq_ignore_ascii_case(&name)),
                "{name} is reserved"
            );
        }
    }

    #[test]
    fn dropping_is_loopback_scoped_if_exists_and_refuses_reserved() {
        let sql = drop_sql("rex_myblog_test");
        assert!(sql.contains("DROP USER IF EXISTS 'rex_myblog_test'@'localhost';"));
        assert!(sql.contains("DROP USER IF EXISTS 'rex_myblog_test'@'127.0.0.1';"));
        assert!(!sql.contains("'%'"), "{sql}");
        assert_eq!(sql.matches("DROP USER").count(), 2, "exactly the two mirrored scopes");

        // Reserved names refuse BEFORE any client could run (nonexistent
        // path proves it), and as a hard error — a drop must be unable to
        // take root out even on a corrupt record.
        for user in ["root", "ROOT", "mysql.sys", "postgres", ""] {
            let err = drop_mirrored(Path::new("/nonexistent/mysql"), 1, user);
            assert!(err.is_err(), "{user} was accepted");
        }
        // A quote in a recorded name cannot break out of the SQL string.
        assert!(drop_sql("o'brien").contains(r"'o\'brien'@'localhost'"));
    }
}

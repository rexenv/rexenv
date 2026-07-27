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
    if user.len() > 32 {
        return Err(Error::Other(format!(
            "the database user name {user:?} is longer than MySQL allows (32)"
        )));
    }

    let sql = mirror_sql(db, user, password);
    let mut child = std::process::Command::new(client)
        .args(client_base_args(port))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| Error::Other(format!("starting the client for user mirroring: {e}")))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| Error::Other("mirroring client has no stdin".into()))?;
    if let Err(e) = stdin.write_all(sql.as_bytes()) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(Error::from(e));
    }
    drop(stdin);
    let out = child.wait_with_output()?;
    if !out.status.success() {
        return Err(Error::Other(format!(
            "mirroring the database user failed (exit {:?}): {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(MirrorOutcome::Mirrored { user: user.to_string() })
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
    }
}

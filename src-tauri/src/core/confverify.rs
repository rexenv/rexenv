//! core::confverify — what actually proves "connected" (Stage 3 plan §6).
//!
//! The `connected` state's whole discipline is that it comes from a
//! verification that RAN, never from "the write succeeded". This module is
//! where that verification lives, and [`Verified`] is its proof:
//!
//! - **Private field, no production constructor.** The only non-test code
//!   that can build a `Verified` is [`verify_signin`]'s success path — after
//!   it has RE-READ the rewritten file from disk, checked the file names the
//!   expected rexenv target, and actually signed in with the file's own
//!   credentials (`USE <db>`). `store::ConnectedVerified::from_verification`
//!   is the only production mint of the witness, and it demands this proof —
//!   so the chain is: connected fact ⇐ witness ⇐ proof ⇐ a real sign-in with
//!   the file as re-read. Not the plan we meant to write, not the write's
//!   exit status.
//! - **The HTTP probe can only upgrade (D4).** [`Verified::with_http_confirmed`]
//!   consumes an existing proof; there is no path from an HTTP response to a
//!   `Verified` — a probe can never gate, never substitute, never un-set.
//!
//! Secrets: the password comes from the re-read file, travels through the
//! 0600 [`DefaultsFile`] (deleted on Drop), and never touches argv or logs —
//! Stage 2's rules, same implementation.

use crate::core::db::SqlClient;
use crate::core::dbdump::{self, DefaultsFile};
use crate::core::dbimport;
use crate::core::phpconf::Unreadable;
use crate::error::Result;
use crate::platform::traits::Platform;
use std::path::Path;

/// Proof of a completed sign-in verification. See the module doc — the
/// private field IS the guarantee.
#[derive(Debug)]
pub struct Verified {
    http_confirmed: bool,
}

impl Verified {
    pub fn http_confirmed(&self) -> bool {
        self.http_confirmed
    }

    /// Record that the supplementary HTTP probe ALSO passed. Consumes an
    /// existing proof — an upgrade, structurally never a substitute.
    pub fn with_http_confirmed(self) -> Verified {
        Verified { http_confirmed: true }
    }

    #[cfg(test)]
    pub(crate) fn test_signin() -> Verified {
        Verified { http_confirmed: false }
    }
}

/// Why verification produced no proof — first-class outcomes, not errors:
/// each downgrades to an honest report, and none can be mistaken for
/// "connected". Same no-blame tone rules as every refusal vocabulary.
#[derive(Debug, Clone)]
pub enum VerifyFail {
    /// The rewritten file didn't read back confidently.
    Unreadable(Unreadable),
    /// The re-read file doesn't point where the rewrite aimed — someone
    /// changed it between the write and the check, or the write missed.
    WrongTarget { host: String, port: u16, expected_port: u16 },
    /// The re-read file names a different database than the rexenv copy.
    WrongDatabase { file_db: String, expected_db: String },
    /// Nothing answered at the file's host:port.
    Unreachable(String),
    /// The engine answered and rejected the file's credentials.
    SigninRefused(String),
    /// Signed in fine; the database isn't there.
    DatabaseMissing { db: String },
}

impl VerifyFail {
    pub fn message(&self) -> String {
        match self {
            VerifyFail::Unreadable(u) => u.message(),
            VerifyFail::WrongTarget { host, port, expected_port } => format!(
                "after the change, the file points at {host}:{port} rather than rexenv's \
                 127.0.0.1:{expected_port} — it may have been edited while the check ran, \
                 so rexenv didn't mark the site connected."
            ),
            VerifyFail::WrongDatabase { file_db, expected_db } => format!(
                "the file names the database `{file_db}`, but this site's rexenv copy is \
                 `{expected_db}` — rexenv didn't mark the site connected."
            ),
            VerifyFail::Unreachable(detail) => format!(
                "nothing answered at the address the file now points to ({detail}) — the \
                 site isn't marked connected until a sign-in succeeds."
            ),
            VerifyFail::SigninRefused(detail) => format!(
                "the server refused the file's own credentials ({detail}) — the site isn't \
                 marked connected until a sign-in succeeds."
            ),
            VerifyFail::DatabaseMissing { db } => format!(
                "the sign-in worked, but the server has no database called `{db}` — the \
                 site isn't marked connected."
            ),
        }
    }
}

/// The sign-in verification: re-read the site's config FROM DISK, confirm it
/// points at rexenv's engine (`expected_port`, loopback) and at the imported
/// copy (`expected_db`), then authenticate with the file's own user/password
/// and `USE` the database. Success is the ONLY production source of
/// [`Verified`].
///
/// `scratch_dir` hosts the 0600 credentials file for the client (its own
/// directory, so it can never clash with a running import's).
pub fn verify_signin(
    platform: &dyn Platform,
    client: &SqlClient,
    docroot: &Path,
    scratch_dir: &Path,
    expected_port: u16,
    expected_db: &str,
) -> Result<std::result::Result<Verified, VerifyFail>> {
    // 1. The file as it is NOW — the thing being verified is the file, so
    //    nothing here reuses the plan or the content the writer produced.
    let conn = match dbimport::read_connection(docroot) {
        Ok(c) => c,
        Err((unreadable, _)) => return Ok(Err(VerifyFail::Unreadable(unreadable))),
    };

    // 2. Does the re-read file aim at the rewrite's target?
    let loopback = matches!(conn.host.as_str(), "127.0.0.1" | "localhost");
    if !loopback || conn.port != expected_port {
        return Ok(Err(VerifyFail::WrongTarget {
            host: conn.host.clone(),
            port: conn.port,
            expected_port,
        }));
    }
    if conn.database != expected_db {
        return Ok(Err(VerifyFail::WrongDatabase {
            file_db: conn.database.clone(),
            expected_db: expected_db.to_string(),
        }));
    }

    // 3. Sign in AS the file's credentials and enter the database. The
    //    backtick-quoted name came from our own store (validated at import);
    //    the credentials go through the 0600 defaults file, never argv.
    let defaults = DefaultsFile::create(platform, scratch_dir, &conn)?;
    match dbdump::client_query(client, &defaults, &format!("USE `{expected_db}`; SELECT 1;")) {
        Ok(_) => Ok(Ok(Verified { http_confirmed: false })),
        Err(stderr) => {
            let s = stderr.to_lowercase();
            if s.contains("unknown database") {
                Ok(Err(VerifyFail::DatabaseMissing { db: expected_db.to_string() }))
            } else if s.contains("access denied") {
                Ok(Err(VerifyFail::SigninRefused(stderr)))
            } else if s.contains("can't connect") || s.contains("connection refused") {
                Ok(Err(VerifyFail::Unreachable(stderr)))
            } else {
                Ok(Err(VerifyFail::SigninRefused(stderr)))
            }
        }
    }
}

/// The WordPress database-error page's stable marker. Localised WP versions
/// emit translated text, which is exactly why the probe is supplementary:
/// missing the marker upgrades nothing to a failure.
const WP_DB_ERROR_MARKER: &str = "Error establishing a database connection";

/// The supplementary HTTP check (D4): one loopback request for the site
/// through our own edge. `true` = the site answered 2xx without the
/// WordPress database-error page. `false` means COULDN'T CONFIRM — never
/// "not connected" — and the caller's only use for it is
/// [`Verified::with_http_confirmed`], which needs a sign-in proof first.
pub async fn probe_http(domain: &str) -> bool {
    let url = format!("https://{domain}/");
    let Ok(client) = reqwest::Client::builder()
        // Our local-CA leaf won't chain for reqwest's store; this is a
        // loopback request to our own edge, pinned by .resolve().
        .danger_accept_invalid_certs(true)
        .resolve(domain, std::net::SocketAddr::from(([127, 0, 0, 1], 443)))
        .timeout(std::time::Duration::from_secs(5))
        .build()
    else {
        return false;
    };
    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => match resp.text().await {
            Ok(body) => !body.contains(WP_DB_ERROR_MARKER),
            Err(_) => false,
        },
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_fail_reads_as_a_sentence_without_blame_and_never_says_connected() {
        // Each case pairs the failure with the datum a user would need in
        // order to act — actionable means NAMING the thing, which a length
        // check (the old `len() > 30`) cannot see and padding satisfies.
        let cases = [
            (VerifyFail::Unreadable(Unreadable::MissingKey { key: "DB_HOST".into() }), "DB_HOST"),
            (
                VerifyFail::WrongTarget {
                    host: "10.0.0.5".into(),
                    port: 3306,
                    expected_port: 13306,
                },
                "10.0.0.5:3306",
            ),
            (
                VerifyFail::WrongDatabase { file_db: "ea_old".into(), expected_db: "ea".into() },
                "`ea_old`",
            ),
            (VerifyFail::Unreachable("connection refused".into()), "connection refused"),
            (VerifyFail::SigninRefused("access denied".into()), "access denied"),
            (VerifyFail::DatabaseMissing { db: "ea".into() }, "`ea`"),
        ];
        for (c, datum) in cases {
            let m = c.message();
            assert!(m.contains(datum), "{m:?} never names the thing to act on ({datum})");
            assert!(m.ends_with('.'), "{m:?} should read as a sentence");
            for blame in ["invalid", "malformed", "wrong,", "bad "] {
                assert!(!m.to_lowercase().contains(blame), "{m:?} reads as blame");
            }
            // No fail message may claim the positive state.
            assert!(!m.contains("is connected"), "{m:?} overclaims");
        }
    }

    #[test]
    fn the_proof_cannot_be_minted_without_a_signin() {
        // Compile-level: `Verified`'s field is private and no production
        // constructor exists outside verify_signin's success path. What CAN
        // be checked at runtime: the upgrade path consumes a proof and only
        // sets the flag — it cannot create one.
        let v = Verified::test_signin();
        assert!(!v.http_confirmed());
        let v = v.with_http_confirmed();
        assert!(v.http_confirmed());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_missing_config_fails_closed_not_open() {
        // A docroot with no config at all must produce a VerifyFail, never a
        // proof — using a real temp dir and a nonexistent client so nothing
        // can actually connect.
        let dir = std::env::temp_dir().join(format!("rexenv-confverify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let platform = crate::platform::current();
        let out = verify_signin(
            &*platform,
            &SqlClient::test_at("/nonexistent/mysql"),
            &dir,
            &dir.join("scratch"),
            13306,
            "ea",
        )
        .unwrap();
        assert!(matches!(out, Err(VerifyFail::Unreadable(_))), "{out:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_file_aimed_elsewhere_fails_before_any_connection() {
        // wp-config pointing at their OLD server: WrongTarget, and the
        // nonexistent client proves no connection was attempted.
        let dir = std::env::temp_dir()
            .join(format!("rexenv-confverify-tgt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("wp-config.php"),
            "<?php\ndefine('DB_NAME','ea');\ndefine('DB_USER','root');\n\
             define('DB_PASSWORD','x');\ndefine('DB_HOST','127.0.0.1:3306');\n",
        )
        .unwrap();
        let platform = crate::platform::current();
        let out = verify_signin(
            &*platform,
            &SqlClient::test_at("/nonexistent/mysql"),
            &dir,
            &dir.join("scratch"),
            13306,
            "ea",
        )
        .unwrap();
        assert!(
            matches!(out, Err(VerifyFail::WrongTarget { port: 3306, expected_port: 13306, .. })),
            "{out:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

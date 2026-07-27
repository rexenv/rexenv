//! core::dbrestore — restoring an artifact into OUR engine (Stage 2 step 6).
//!
//! **Provenance before creation, and the ordering can't invert.** The only way
//! to create or reset the target database is through functions that require the
//! [`Recorded`] witness, and the only mint of [`Recorded`] is
//! [`record_provenance`] — which writes `sites.db_created` FIRST. A crash
//! between the write and the create leaves a recorded claim on a database that
//! doesn't exist yet, which `DROP DATABASE IF EXISTS` shrugs off; the reverse
//! order — the one that can strand a database we'd then refuse to clean up —
//! has no code path.
//!
//! **Recorded truth outranks re-derivation, including on Retry.** When
//! `db_created` is already recorded, [`record_provenance`] KEEPS it and ignores
//! the live existence check: a retry after a crash-between-create-and-feed
//! would otherwise see "the database exists", call it pre-existing, and demote
//! our own half-made database into one no path may drop.
//!
//! **Retry is drop-and-refeed, never resume.** A SQL dump is a statement
//! stream, not a journal: its INSERTs aren't idempotent and "where it stopped"
//! isn't knowable from the wreckage, so resuming would guess. [`prepare_target`]
//! IS the retry path — for a database we created it drops and recreates (a
//! fresh slate, same code first run and every retry); for a pre-existing one it
//! NEVER drops the database — the dump's own per-table `DROP TABLE IF EXISTS`
//! does the overwriting the user's typed confirmation agreed to, and any table
//! of theirs the dump doesn't mention survives.
//!
//! **"Some tables exist" can never read as success.** A restore settles ok only
//! through [`verify_complete`], which checks MEMBERSHIP — every table the
//! manifest says the copy CREATEs must exist in the restored database (never
//! count equality, which a pre-existing target's own extra tables would break
//! in both directions) — and is the only mint of the [`Verified`] witness that
//! [`finish`], the function pointing the site row at the database, requires. A
//! partial restore fails naming the missing tables, and nothing downstream can
//! call it done.

use crate::core::database::client_base_args;
use crate::core::dbdump::Manifest;
use crate::error::{Error, Result};
use rusqlite::Connection;
use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Witness that provenance is on disk. Private constructor; carries whether the
/// database is OURS (we created it) — read from the record, so callers can't
/// second-guess it.
#[derive(Debug)]
pub struct Recorded {
    ours: bool,
}

impl Recorded {
    pub fn ours(&self) -> bool {
        self.ours
    }
}

/// Write `sites.db_created` BEFORE anything touches the engine, and mint the
/// witness everything else requires.
///
/// `exists_now` is the live `SHOW DATABASES` answer — consulted ONLY when
/// nothing is recorded yet. Once recorded, the record wins (see module doc for
/// the retry inversion this prevents). The `false → true` refusal lives in the
/// SQL itself (`set_site_db_created`), so even a confused caller cannot
/// re-claim a pre-existing database.
pub fn record_provenance(conn: &Connection, site_id: &str, exists_now: bool) -> Result<Recorded> {
    let site = crate::state::store::get_site(conn, site_id)?
        .ok_or_else(|| Error::Other(format!("no site with id {site_id}")))?;
    match site.db_created {
        Some(ours) => Ok(Recorded { ours }),
        None => {
            let ours = !exists_now;
            if !crate::state::store::set_site_db_created(conn, site_id, ours)? {
                return Err(Error::Other(
                    "recording database ownership failed — not proceeding without it".into(),
                ));
            }
            Ok(Recorded { ours })
        }
    }
}

/// Bring the target database to the state a restore may feed into. This is the
/// retry path too — one code path, idempotent by construction:
///
/// - OURS: `DROP DATABASE IF EXISTS` + `CREATE DATABASE` — a fresh slate, which
///   is what makes a retried restore converge instead of layering on wreckage.
/// - PRE-EXISTING: `CREATE DATABASE IF NOT EXISTS` (a no-op) — the database
///   itself is never dropped by any path; the dump's per-table
///   `DROP TABLE IF EXISTS` performs exactly the overwrite that was confirmed.
pub fn prepare_target(
    recorded: &Recorded,
    client: &Path,
    port: u16,
    name: &str,
) -> Result<()> {
    if recorded.ours {
        crate::core::database::drop_database(client, port, name)?;
    }
    crate::core::database::create_database(client, port, name)
}

/// Drop a failed restore's database — only if the record says we made it. The
/// check lives HERE, not at call sites: there is no drop-on-cleanup path that
/// doesn't consult the witness.
pub fn cleanup_failed(recorded: &Recorded, client: &Path, port: u16, name: &str) -> Result<bool> {
    if !recorded.ours {
        return Ok(false);
    }
    crate::core::database::drop_database(client, port, name)?;
    Ok(true)
}

/// How a feed ended.
#[derive(Debug)]
pub enum FeedOutcome {
    /// The client consumed the whole artifact and exited 0. NOT success yet —
    /// [`verify_complete`] decides that.
    Fed { bytes: u64 },
    Cancelled,
}

/// Feed the artifact into the target database over the client's stdin — no
/// shell, no argv, and the bytes WE write are the progress signal (real
/// measurement of our own writing; the client is still executing SQL after the
/// last byte, which is why `Fed` is not success).
///
/// Skips line 1 when the manifest flagged the May-2024 MariaDB sandbox form —
/// the one line MySQL hard-fails on.
// Eight arguments, deliberately: two are witnesses (Recorded) and signals
// (cancel/progress) that must stay separate to keep their guarantees visible.
#[allow(clippy::too_many_arguments)]
pub fn feed(
    _recorded: &Recorded,
    client: &Path,
    port: u16,
    name: &str,
    artifact: &Path,
    manifest: &Manifest,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64),
) -> Result<FeedOutcome> {
    let file = std::fs::File::open(artifact)
        .map_err(|e| Error::Other(format!("open {}: {e}", artifact.display())))?;
    let mut reader = std::io::BufReader::new(file);

    let mut child = std::process::Command::new(client)
        .args(client_base_args(port))
        .arg(name)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| Error::Other(format!("starting the restore client: {e}")))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| Error::Other("restore client has no stdin".into()))?;
    let mut stderr_pipe = child.stderr.take();
    let stderr_thread = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(p) = stderr_pipe.as_mut() {
            use std::io::Read;
            let _ = p.read_to_string(&mut buf);
        }
        buf
    });

    let kill = |child: &mut std::process::Child| {
        let pid = child.id().to_string();
        let _ = std::process::Command::new("kill").arg(&pid).status();
        for _ in 0..20 {
            if matches!(child.try_wait(), Ok(Some(_))) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = child.kill();
        let _ = child.wait();
    };

    // Skip the flagged sandbox line, if any.
    if manifest.findings.skip_sandbox_line {
        let mut first = String::new();
        let _ = reader.read_line(&mut first);
    }

    let mut fed: u64 = 0;
    let mut buf = vec![0u8; 256 * 1024];
    let outcome = loop {
        if cancel.load(Ordering::SeqCst) {
            drop(stdin);
            kill(&mut child);
            let _ = stderr_thread.join();
            return Ok(FeedOutcome::Cancelled);
        }
        use std::io::Read;
        let n = match reader.read(&mut buf) {
            Ok(0) => break None,
            Ok(n) => n,
            Err(e) => break Some(format!("reading the copy: {e}")),
        };
        if let Err(e) = stdin.write_all(&buf[..n]) {
            // EPIPE: the client died mid-feed — its stderr has the real reason.
            let _ = e;
            break None;
        }
        fed += n as u64;
        progress(fed);
    };
    drop(stdin); // EOF — the client finishes executing what it read.
    if let Some(read_err) = outcome {
        kill(&mut child);
        let _ = stderr_thread.join();
        return Err(Error::Other(read_err));
    }
    let status = child.wait()?;
    let stderr = stderr_thread.join().unwrap_or_default();
    if !status.success() {
        return Err(Error::Other(format!(
            "the restore stopped part-way (exit {:?}): {}",
            status.code(),
            last_lines(&stderr, 3)
        )));
    }
    Ok(FeedOutcome::Fed { bytes: fed })
}

/// Witness that the restored database is WHOLE. Private field; the only mint is
/// [`verify_complete`], and [`finish`] requires it — so nothing can point a
/// site at a partially restored database.
#[derive(Debug)]
// Not #[non_exhaustive] (clippy's suggestion): that only stops construction
// across CRATES, and the whole point is that no other module in THIS crate can
// mint one. The private unit field is the guarantee.
#[allow(clippy::manual_non_exhaustive)]
pub struct Verified {
    pub tables: u64,
    _proof: (),
}

/// Verify by MEMBERSHIP: every table the manifest says the copy CREATEs must
/// exist in the restored database. Not count equality — a pre-existing target
/// legitimately holds tables the dump never mentioned, and those must neither
/// fail the check nor count toward it.
pub fn verify_complete(
    client: &Path,
    port: u16,
    name: &str,
    manifest: &Manifest,
) -> Result<Verified> {
    let present = list_tables(client, port, name)?;
    let missing: Vec<&str> = manifest
        .tables
        .iter()
        .filter(|t| !present.contains(*t))
        .map(|t| t.as_str())
        .collect();
    if !missing.is_empty() {
        return Err(Error::Other(partial_message(name, &missing, manifest.tables.len())));
    }
    Ok(Verified { tables: manifest.tables.len() as u64, _proof: () })
}

/// The partial-restore sentence: the numbers, what's missing, the guarantee
/// that nothing calls this imported, and what Retry will do about it.
fn partial_message(name: &str, missing: &[&str], want: usize) -> String {
    let shown: Vec<&str> = missing.iter().take(4).copied().collect();
    let more = if missing.len() > shown.len() {
        format!(" (+{} more)", missing.len() - shown.len())
    } else {
        String::new()
    };
    format!(
        "the restore did not finish: `{name}` has {} of the {want} tables the copy \
         contains — missing {}{more}. Nothing will treat this as imported — Retry \
         drops this copy and restores it again from the start.",
        want - missing.len(),
        shown.join(", "),
    )
}

/// Point the site row at the database it now actually uses — the LAST step, and
/// it will not compile without proof the restore verified whole.
pub fn finish(conn: &Connection, site_id: &str, db_name: &str, _proof: &Verified) -> Result<()> {
    if !crate::state::store::set_site_db_name(conn, site_id, db_name)? {
        return Err(Error::Other(format!("no site with id {site_id}")));
    }
    Ok(())
}

/// The tables a database holds, via `information_schema` on the bundled client.
pub fn list_tables(client: &Path, port: u16, name: &str) -> Result<Vec<String>> {
    let sql = format!(
        "SELECT table_name FROM information_schema.tables WHERE table_schema = '{}'",
        name.replace('\\', "\\\\").replace('\'', "''")
    );
    let out = std::process::Command::new(client)
        .args(client_base_args(port))
        .args(["-N", "-B", "-e", &sql])
        .output()?;
    if !out.status.success() {
        return Err(Error::Other(format!(
            "listing tables in `{name}`: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

/// Does a database exist on our engine right now? The live half of the
/// provenance question — consulted only when nothing is recorded.
pub fn database_exists(client: &Path, port: u16, name: &str) -> Result<bool> {
    let out = std::process::Command::new(client)
        .args(client_base_args(port))
        .args(["-N", "-B", "-e", "SHOW DATABASES"])
        .output()?;
    if !out.status.success() {
        return Err(Error::Other(format!(
            "listing databases: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).lines().any(|l| l.trim() == name))
}

fn last_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.trim().lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Connection {
        let conn = crate::state::db::open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO sites (id, name, domain, type, php_version, path, db_name)
             VALUES ('s1','A','a.test','wordpress','8.3','/x/a','wp_a_test')",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn provenance_is_written_before_anything_and_the_record_outranks_rederivation() {
        let conn = test_conn();

        // Nothing recorded + the name doesn't exist -> ours, and it's on disk
        // BEFORE the caller can reach prepare_target (which needs the witness).
        let rec = record_provenance(&conn, "s1", false).unwrap();
        assert!(rec.ours());
        assert_eq!(
            crate::state::store::get_site(&conn, "s1").unwrap().unwrap().db_created,
            Some(true)
        );

        // The crash-then-retry inversion: our created-but-unfed database now
        // EXISTS, so a re-derivation would call it pre-existing and strand it.
        // The record wins instead.
        let rec = record_provenance(&conn, "s1", true).unwrap();
        assert!(rec.ours(), "recorded truth outranks the live existence check");

        assert!(record_provenance(&conn, "ghost", false).is_err());
    }

    #[test]
    fn a_pre_existing_record_stays_pre_existing_forever() {
        let conn = test_conn();
        let rec = record_provenance(&conn, "s1", true).unwrap();
        assert!(!rec.ours());
        // Even a caller insisting "it doesn't exist now" cannot re-claim it:
        // the record wins here, and the SQL guard refuses false -> true anyway.
        let rec = record_provenance(&conn, "s1", false).unwrap();
        assert!(!rec.ours());
        assert!(!crate::state::store::set_site_db_created(&conn, "s1", true).unwrap());
    }

    #[test]
    fn cleanup_refuses_to_drop_what_we_did_not_create_without_touching_an_engine() {
        let conn = test_conn();
        let theirs = record_provenance(&conn, "s1", true).unwrap();
        // A nonexistent client binary proves no engine call happens on the
        // refusal path: reaching one would error, refusing returns Ok(false).
        let out = cleanup_failed(&theirs, Path::new("/nonexistent/mysql"), 1, "keep_me").unwrap();
        assert!(!out, "a pre-existing database is never dropped, so no client runs");
    }

    #[test]
    fn a_partial_restore_names_the_numbers_the_missing_tables_and_the_way_out() {
        let missing = ["wp_options", "wp_users", "wp_usermeta", "wp_terms", "wp_postmeta"];
        let msg = partial_message("ea", &missing, 48);
        assert!(msg.contains("`ea` has 43 of the 48 tables"), "{msg}");
        assert!(msg.contains("missing wp_options, wp_users"), "{msg}");
        assert!(msg.contains("(+1 more)"), "{msg}");
        assert!(msg.contains("Nothing will treat this as imported"), "{msg}");
        assert!(msg.contains("Retry drops this copy"), "{msg}");
        for blame in ["invalid", "corrupt", "error", "bad "] {
            assert!(!msg.to_lowercase().contains(blame), "{msg}");
        }
    }
}

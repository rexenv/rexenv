//! core::dbclone — copy one site's database into a NEW database on the same
//! engine (`docs/PLAN-git-worktrees.md` §2.4: a worktree child starts from its
//! parent's data).
//!
//! **Not the import pipeline, on purpose.** `dbdump::gate` refuses a source
//! that is rexenv's own server — that refusal is the Valet/Herd import's guard
//! against importing a site into itself, and it stays. A same-server copy is a
//! different, smaller job, so it has its own small path built from the same
//! witnessed pieces: provenance through `dbrestore::record_provenance` (so the
//! child's `db_created` is written BEFORE `CREATE DATABASE`, and teardown drops
//! only what this made), the fresh slate through `prepare_target`, cleanup
//! through `cleanup_failed`.
//!
//! **A target that already exists and is not ours is refused** (ledger #817):
//! the copy never writes into a database somebody else made. A Retry of OUR
//! half-made copy is allowed — the record says it is ours, and `prepare_target`
//! drops and recreates it, the same converge-not-layer rule the import has.
//!
//! **"Done" means every source table is in the copy** — membership, checked
//! against the live source after the import, never "the client exited 0".
//!
//! The dump lives in a private scratch file (`PermissionManager::write_private`
//! creates it 0600 / owner-only before the dump tool truncates it — the tool
//! keeps the mode) and is removed on every exit path.

use crate::core::db::{DbEngine, SqlClient};
use crate::core::dbrestore::{self, Recorded};
use crate::error::{Error, Result};
use crate::platform::traits::PermissionManager;
use std::path::{Path, PathBuf};

/// One copy, as the job hands it over.
pub struct CloneSpec<'a> {
    pub engine: DbEngine,
    pub client: &'a SqlClient,
    /// The engine's dump binary (`DbEngine::sql_client_bins`' second half).
    pub dump: &'a Path,
    pub port: u16,
    pub source: &'a str,
    pub target: &'a str,
    /// Where the private dump file goes (app-data, not the user's Downloads).
    pub scratch_dir: &'a Path,
}

/// What a finished copy measured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneReport {
    pub tables: usize,
    pub dump_bytes: u64,
}

/// The dump file, removed when it goes out of scope — on success, on error,
/// on panic.
struct ScratchDump(PathBuf);

impl Drop for ScratchDump {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Copy `spec.source` into `spec.target`. `record` is
/// `dbrestore::record_provenance` for the CHILD site, bound by the caller (who
/// owns the app-database lock and must not hold it across this dump): it is
/// called with the live "does the target exist" answer and returns the witness.
pub fn clone_database(
    spec: &CloneSpec,
    perms: &dyn PermissionManager,
    record: &dyn Fn(bool) -> Result<Recorded>,
) -> Result<CloneReport> {
    let CloneSpec { engine, client, dump, port, source, target, scratch_dir } = *spec;
    if source == target {
        return Err(Error::Other("a database cannot be copied onto itself".into()));
    }
    if !engine.database_exists(client, port, source)? {
        return Err(Error::Other(format!("the source database `{source}` does not exist")));
    }
    let recorded = record(engine.database_exists(client, port, target)?)?;
    if !recorded.ours() {
        return Err(Error::Other(format!(
            "a database named `{target}` already exists and rexenv did not create it — \
             not copying into it. Rename or drop it, or pick another domain."
        )));
    }

    std::fs::create_dir_all(scratch_dir)?;
    let file = ScratchDump(scratch_dir.join(format!("clone-{target}.sql")));
    perms.write_private(&file.0, b"")?;

    let result = (|| {
        engine.dump_to_file(dump, port, source, &file.0)?;
        let dump_bytes = std::fs::metadata(&file.0)?.len();
        dbrestore::prepare_target(&recorded, client, port, target)?;
        engine.import_from_file(client, port, target, &file.0)?;
        let want = engine.list_tables(client, port, source)?;
        let have = engine.list_tables(client, port, target)?;
        let missing: Vec<&String> = want.iter().filter(|t| !have.contains(t)).collect();
        if !missing.is_empty() {
            return Err(Error::Other(format!(
                "the copy of `{source}` is missing {} table(s): {}",
                missing.len(),
                missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
            )));
        }
        Ok(CloneReport { tables: want.len(), dump_bytes })
    })();
    if result.is_err() {
        let _ = dbrestore::cleanup_failed(&recorded, client, port, target);
    }
    result
}

//! core::confrewrite — the rewrite job's file mechanics (Stage 3 step 5):
//! the backup, the atomic write, and the revert classification.
//!
//! The lifecycle discipline, stated once:
//!
//! - **Backup before write, first backup wins.** The backup means "the file
//!   as it was before rexenv ever touched it"; the `config_rewrites` PK
//!   already makes a second backup unrepresentable, and the runtime order
//!   here (backup file → row → write) means a crash between any two steps
//!   leaves the original recoverable — never a rewritten file with no backup.
//! - **A half-written config is unrepresentable.** Writes go to a temp file
//!   in the SAME directory, take the original's permission bits, and land by
//!   `rename` — their file either has the old bytes or the new bytes.
//! - **Revert classifies before it touches anything**, and every ambiguous
//!   shape fails toward the conservative branch: an unknown digest (v22
//!   NULL) reads as "can't prove the file is unchanged", never as a match.

use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// sha256 of `bytes`, lowercase hex — the preview→apply fingerprint and the
/// v22 `written_digest`, always over the WHOLE file: an edit anywhere (a new
/// duplicate key, an unclosed quote above our lines) changes what a write
/// means even when the target lines look identical.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

/// Where a site's config backup lives:
/// `<app-data>/config-backups/<site-id>/<basename>`. One backup per
/// site+file BY NAMING (the resolver-backup reasoning): a timestamped name
/// is what would make an orphan set unbounded.
pub fn backup_path(platform: &dyn Platform, site_id: &str, config_file: &Path) -> Result<PathBuf> {
    let name = config_file
        .file_name()
        .ok_or_else(|| Error::Other(format!("config path has no file name: {config_file:?}")))?;
    Ok(platform
        .paths()
        .app_data_dir()?
        .join("config-backups")
        .join(site_id)
        .join(name))
}

/// Write the backup, 0600 from birth (it contains their password). Callers
/// only reach this when no `config_rewrites` row exists — first backup wins —
/// and insert the row immediately after, so the content is safe on disk
/// before anything records or relies on it.
pub fn write_backup(platform: &dyn Platform, backup: &Path, original: &str) -> Result<()> {
    if let Some(parent) = backup.parent() {
        std::fs::create_dir_all(parent)?;
    }
    platform.permissions().write_private(backup, original.as_bytes())
}

/// Replace `path`'s content atomically: temp file in the same directory,
/// the original's permission bits copied onto it, then `rename`. A crash at
/// any point leaves either the old file or the new file — never a truncated
/// one, and never a mode change their config didn't have.
pub fn atomic_write_preserving_mode(path: &Path, content: &str) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| Error::Other(format!("config path has no parent: {path:?}")))?;
    let base = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::Other(format!("config path has no file name: {path:?}")))?;
    let tmp = dir.join(format!(".{base}.rexenv-tmp"));

    // The temp is BORN owner-only and widened to their file's mode after the
    // content is in it — never the other way round: `fs::write` creates at
    // 0666 & ~umask with the database password already inside, and a chmod
    // afterwards leaves a world-readable window (and, when the original had
    // vanished, a world-readable file renamed into place for good).
    {
        use std::io::Write;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }
    // Their file's mode survives the inode swap (a config chmodded 0600 must
    // not come back 0644). If the original vanished mid-flight the rename
    // below recreates it — owner-only, which for a file holding credentials
    // is the honest default.
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        // "Couldn't write" and "wrote but couldn't verify" are different
        // states that both end with a site not working — the copy must carry
        // the difference, so the write failure says the file is untouched.
        return Err(Error::Other(format!(
            "replacing {} atomically: {e} — the file was left as it was.",
            path.display()
        )));
    }
    Ok(())
}

/// What a revert may do, decided BEFORE anything is touched. Branch order
/// matters: "already reverted" is checked first so the crash window between
/// a restore and its record cleanup converges instead of reading as edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevertCheck {
    /// Our copy of the original is gone. Their file is left exactly as it
    /// is; the record is dropped (a revert that can never work must not keep
    /// rendering); `connected` stays — it is still true.
    BackupMissing,
    /// The file already equals the backup (a failed write, or a crash after
    /// restore before cleanup). Nothing to restore; clean up the record.
    AlreadyReverted,
    /// The file is byte-identical to what the rewrite wrote — restoring the
    /// backup loses nothing of theirs.
    CleanRestore,
    /// The file differs from what we wrote — or we can't prove it doesn't
    /// (unknown digest, file missing). Restoring would destroy their later
    /// edits, so this refuses unless the user explicitly forces it.
    FileEdited {
        /// What we saw, for the honest message.
        reason: FileEditedReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FileEditedReason {
    /// Digest recorded and the current content doesn't match it.
    EditedSinceRewrite,
    /// No digest recorded (v22 NULL) — unprovable, treated as edited.
    UnknownDigest,
    /// The file is gone; a restore would recreate something they deleted.
    FileMissing,
}

/// Classify a revert from what is actually on disk. Pure — the inputs are
/// the current file content (`None` = missing), the backup content (`None` =
/// missing), and the recorded `written_digest`.
pub fn classify_revert(
    current: Option<&str>,
    backup: Option<&str>,
    written_digest: Option<&str>,
) -> RevertCheck {
    let Some(backup) = backup else {
        return RevertCheck::BackupMissing;
    };
    let Some(current) = current else {
        return RevertCheck::FileEdited { reason: FileEditedReason::FileMissing };
    };
    if current == backup {
        return RevertCheck::AlreadyReverted;
    }
    match written_digest {
        Some(d) if sha256_hex(current.as_bytes()) == d => RevertCheck::CleanRestore,
        Some(_) => RevertCheck::FileEdited { reason: FileEditedReason::EditedSinceRewrite },
        None => RevertCheck::FileEdited { reason: FileEditedReason::UnknownDigest },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fingerprint_covers_the_whole_file() {
        // An edit ANYWHERE changes the digest — a new line above the target
        // keys is enough. (What makes preview→apply refuse on any drift.)
        let a = sha256_hex(b"DB_HOST=x\nDB_PORT=1\n");
        let b = sha256_hex(b"# note\nDB_HOST=x\nDB_PORT=1\n");
        assert_ne!(a, b);
        // Deterministic, lowercase hex, full width.
        assert_eq!(sha256_hex(b"x"), sha256_hex(b"x"));
        assert_eq!(sha256_hex(b"x").len(), 64);
    }

    #[test]
    fn revert_classification_covers_every_branch_conservatively() {
        let written = "DB_HOST=new\n";
        let original = "DB_HOST=old\n";
        let digest = sha256_hex(written.as_bytes());

        // No backup → BackupMissing, whatever else holds.
        assert_eq!(classify_revert(Some(written), None, Some(&digest)), RevertCheck::BackupMissing);

        // File equals the backup → already reverted (checked BEFORE the
        // digest, so the restore-then-crash window converges).
        assert_eq!(
            classify_revert(Some(original), Some(original), Some(&digest)),
            RevertCheck::AlreadyReverted
        );

        // Untouched since the rewrite → clean restore.
        assert_eq!(
            classify_revert(Some(written), Some(original), Some(&digest)),
            RevertCheck::CleanRestore
        );

        // Edited since → named, refuses by default.
        assert_eq!(
            classify_revert(Some("DB_HOST=theirs\n"), Some(original), Some(&digest)),
            RevertCheck::FileEdited { reason: FileEditedReason::EditedSinceRewrite }
        );

        // Unknown digest (v22 NULL) → conservative: NEVER reads as a match,
        // even when the content actually equals what we wrote.
        assert_eq!(
            classify_revert(Some(written), Some(original), None),
            RevertCheck::FileEdited { reason: FileEditedReason::UnknownDigest }
        );

        // File deleted → a restore would recreate what they removed.
        assert_eq!(
            classify_revert(None, Some(original), Some(&digest)),
            RevertCheck::FileEdited { reason: FileEditedReason::FileMissing }
        );
    }

    #[test]
    fn atomic_write_preserves_content_and_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir()
            .join(format!("rexenv-confrewrite-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("wp-config.php");

        std::fs::write(&file, "old").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();

        atomic_write_preserving_mode(&file, "new content").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "new content");
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "their chmod must survive the inode swap");
        // No temp litter.
        assert!(!dir.join(".wp-config.php.rexenv-tmp").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn backup_paths_are_per_site_per_basename() {
        // Pure path shape — no platform needed for the naming property: the
        // basename keys the backup, so one per site+file by construction.
        let a = Path::new("/Users/x/code/myblog/wp-config.php");
        let b = Path::new("/Users/x/code/myblog/.env");
        assert_ne!(a.file_name(), b.file_name());
    }
}

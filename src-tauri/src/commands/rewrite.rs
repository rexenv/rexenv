//! commands::rewrite — the opt-in connection rewrite (Stage 3 step 5): thin
//! over `core::{confedit, confrewrite, confverify, dbmirror}`.
//!
//! Three commands, one contract:
//!
//! - `rewrite_preview` is READ-ONLY: it builds the plan, produces the diff
//!   FROM the bytes that would be written, and fingerprints the whole file
//!   (sha256) — an edit anywhere changes what a write means, so the
//!   fingerprint covers everything, not just the target lines.
//! - `rewrite_apply` refuses on a fingerprint mismatch, then runs the settled
//!   order: engine check → mirror (record-first, converge always — a second
//!   apply must never skip account creation because a record exists) →
//!   backup (first backup wins) → temp+rename write → digest → sign-in
//!   verification. `connected` is set only from the verification's proof.
//! - `rewrite_revert` classifies BEFORE touching anything, and every ugly
//!   case is a named outcome: file edited since the rewrite (digest), backup
//!   missing, already reverted. Clearing `connected` happens BEFORE the
//!   restore, so a crash lands in the under-claim direction.
//!
//! Cross-guards both ways, Stage 2's shape: apply/revert refuse while a
//! provision or database import runs for the site, and those jobs refuse
//! while `AppState::rewrite_active` names the domain.

use crate::core::confedit::{self, DiffLine, RewritePlan};
use crate::core::confrewrite::{self, FileEditedReason, RevertCheck};
use crate::core::confverify;
use crate::core::db::DbEngine;
use crate::core::dbimport::{self, ConfigSource};
use crate::core::dbmirror::{self, RESERVED_USERS};
use crate::core::{self};
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::core::phpconf;
use crate::state::models::{MultisiteMode, Site};
use crate::state::store::{self, ConnectedVerified, DbImportRecord};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::State;

/// Clears `rewrite_active` on every exit path, including `?` and panic.
struct Slot<'a>(&'a std::sync::Mutex<Option<String>>);
impl Drop for Slot<'_> {
    fn drop(&mut self) {
        if let Ok(mut g) = self.0.lock() {
            *g = None;
        }
    }
}

fn lock<'a>(
    state: &'a State<'a, AppState>,
) -> Result<std::sync::MutexGuard<'a, rusqlite::Connection>> {
    state.db.lock().map_err(|_| Error::Other("db lock poisoned".into()))
}

// ---------------------------------------------------------------------------
// Preview
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status")]
pub enum RewritePreview {
    /// The diff IS the write: `rewrite_apply` writes exactly the bytes this
    /// diff was derived from, or refuses.
    #[serde(rename = "ready", rename_all = "camelCase")]
    Ready {
        /// The file the rewrite touches (theirs, absolute).
        file: String,
        diff: Vec<DiffLine>,
        /// sha256 of the WHOLE file at preview time — apply refuses on drift.
        fingerprint: String,
        /// `Some` = the root case: this dedicated account (holding the
        /// config's existing password) will be created on apply.
        creates_user: Option<String>,
        /// A backup already exists (an earlier rewrite): it is kept — first
        /// backup wins — and still holds the original file.
        backup_exists: bool,
        /// `.env` shape with `bootstrap/cache/config.php` present: the §5
        /// warning leads the panel, because a cached config imitates a
        /// failed rewrite.
        laravel_cache_warning: bool,
        /// Where the site will connect: "127.0.0.1:13306".
        target: String,
        /// The config still names Local's own server, which rexenv can't
        /// reach — so until this change is applied the site reads NO database
        /// (it does not "keep reading the old one"). Ledger #575.
        old_database_unreachable: bool,
        /// A multisite network whose `DOMAIN_CURRENT_SITE` this change moves to
        /// the site's rexenv name — so while connected, the network no longer
        /// loads under its old name in the source tool (the folder is shared).
        moves_network_domain: bool,
    },
    /// Downgraded to tell-only, with the reason. Never a guess.
    #[serde(rename = "refused", rename_all = "camelCase")]
    Refused { reason: String, file: Option<String> },
}

/// Everything preview and apply share, resolved fresh each time from the
/// site's CURRENT config — nothing is cached between the two calls except
/// the user-approved fingerprint.
struct Resolved {
    site: Site,
    record: DbImportRecord,
    conn: dbimport::DbConnection,
    file: PathBuf,
    original: String,
    rewrite: confedit::Rewrite,
    /// The dedicated account the root case creates (D1), else None.
    creates_user: Option<String>,
    engine: DbEngine,
    /// The plan moves a network's DOMAIN_CURRENT_SITE.
    moves_network_domain: bool,
}

enum Resolution {
    Ok(Box<Resolved>),
    Refused { reason: String, file: Option<String> },
}

fn resolve(state: &State<'_, AppState>, site_id: &str) -> Result<Resolution> {
    let (site, record) = {
        let conn = lock(state)?;
        let site = core::sites::get(&conn, site_id)?
            .ok_or_else(|| Error::Other("site not found".into()))?;
        let record = store::get_db_import(&conn, site_id)?.ok_or_else(|| {
            Error::Other(format!(
                "no imported database is recorded for {} — import it first.",
                site.domain
            ))
        })?;
        (site, record)
    };

    let conn = match dbimport::read_connection(Path::new(&site.path)) {
        Ok(c) => c,
        Err((unreadable, source)) => {
            return Ok(Resolution::Refused {
                reason: unreadable.message(),
                file: source.map(|s| s.path().to_string()),
            })
        }
    };
    let file = PathBuf::from(conn.source.path());

    // A renamed import (their `local` restored as `local_<domain>` — EVERY Local
    // import since 12 Sep 2026, where every site's database is called `local`): the
    // plan moves the config's database NAME to the copy as well. Until 12 Sep
    // 2026 this refused to tell-only, a ruling called permanent on the evidence
    // of zero collisions across a dozen Valet sites; the second Local import on
    // the owner's machine hit it at once, and the owner reversed it (ledger
    // #574). A name is not a secret, so the diff stays secret-free, and the
    // sign-in verification below already expects the copy's name.
    let rename = (conn.database != record.db_name).then(|| record.db_name.clone());

    let engine = DbEngine::from_site(site.db_engine);

    // The config's DRIVER must be the one this site's database actually speaks.
    // `RewriteKey` is a closed set — Host, Port, User, Name — so a driver
    // change is unrepresentable
    // here, and rewriting host+port WITHOUT it produces the worst kind of
    // success. A `.env` still saying `DB_CONNECTION=mysql`, pointed at 15432,
    // sends a MySQL client at PostgreSQL: the handshake is unreadable to both
    // sides and the error names neither, exactly the shape ledger #546 exists
    // to prevent one layer down. Tell-only, naming both halves.
    if conn.driver != dbimport::Driver::for_engine(engine) {
        return Ok(Resolution::Refused {
            reason: format!(
                "this site's database lives in {}, but the config says {} — the one-click \
                 change covers the host, port, user and database name, not the driver. Set \
                 `DB_CONNECTION` yourself (the Database tab has the connection lines), \
                 then run this again.",
                engine.label(),
                conn.driver.label()
            ),
            file: Some(file.display().to_string()),
        });
    }

    let port = engine.port();
    let reserved = RESERVED_USERS.iter().any(|r| r.eq_ignore_ascii_case(&conn.user));
    let creates_user = reserved.then(|| dbmirror::dedicated_user_name(&site.domain));

    let original = std::fs::read_to_string(&file)
        .map_err(|e| Error::Other(format!("reading {}: {e}", file.display())))?;

    let plan = match &conn.source {
        ConfigSource::WpConfig { .. } => {
            RewritePlan::wp(&format!("127.0.0.1:{port}"), creates_user.as_deref())?
        }
        ConfigSource::DotEnv { .. } => {
            RewritePlan::env("127.0.0.1", port, creates_user.as_deref())?
        }
    };
    let plan = match &rename {
        Some(name) => plan.with_database(name)?,
        None => plan,
    };

    // A multisite NETWORK moves its domain too (docs/PLAN-local-multisite.md T3).
    // WordPress finds the network — and scopes its login cookies — by
    // DOMAIN_CURRENT_SITE, which an imported network's wp-config still gives as
    // the source's host (`multi.local`); served as `multi.rex`, logins fail. The
    // site row says it is a network (ledger #580); the FILE says whether the
    // constant exists and differs. Absent = nothing to move (WordPress then uses
    // the request's host). Any other unreadable shape is staged anyway, so the
    // editor refuses it by name instead of connecting a network that can't log in.
    let moves_network_domain = matches!(conn.source, ConfigSource::WpConfig { .. })
        && !matches!(site.multisite, MultisiteMode::None)
        && match phpconf::wp_define_str(&original, "DOMAIN_CURRENT_SITE") {
            Ok(v) => v != site.domain,
            Err(phpconf::Unreadable::MissingKey { .. }) => false,
            Err(_) => true,
        };
    let plan = if moves_network_domain { plan.with_network_domain(&site.domain)? } else { plan };

    let rewrite = match confedit::rewrite(&original, &plan) {
        Ok(r) => r,
        Err(refusal) => {
            return Ok(Resolution::Refused {
                reason: refusal.message(),
                file: Some(file.display().to_string()),
            })
        }
    };

    Ok(Resolution::Ok(Box::new(Resolved {
        site,
        record,
        conn,
        file,
        original,
        rewrite,
        creates_user,
        engine,
        moves_network_domain,
    })))
}

/// `.env` shape + a config cache file = the §5 warning must lead.
fn laravel_cache_present(source: &ConfigSource) -> bool {
    match source {
        ConfigSource::DotEnv { path } => Path::new(path)
            .parent()
            .map(|root| root.join("bootstrap/cache/config.php").is_file())
            .unwrap_or(false),
        ConfigSource::WpConfig { .. } => false,
    }
}

#[tauri::command]
pub async fn rewrite_preview(
    state: State<'_, AppState>,
    site_id: String,
) -> Result<RewritePreview> {
    match resolve(&state, &site_id)? {
        Resolution::Refused { reason, file } => Ok(RewritePreview::Refused { reason, file }),
        Resolution::Ok(r) => {
            let backup_exists = {
                let conn = lock(&state)?;
                store::get_config_rewrite(&conn, &site_id, &r.file.display().to_string())?
                    .is_some()
            };
            Ok(RewritePreview::Ready {
                file: r.file.display().to_string(),
                diff: r.rewrite.diff.clone(),
                fingerprint: confrewrite::sha256_hex(r.original.as_bytes()),
                creates_user: r.creates_user.clone(),
                backup_exists,
                laravel_cache_warning: laravel_cache_present(&r.conn.source),
                target: format!("127.0.0.1:{}", r.engine.port()),
                old_database_unreachable: directories::BaseDirs::new().is_some_and(|b| {
                    core::localwp::config_reaches_only_local(
                        b.home_dir(),
                        &r.conn.host,
                        r.conn.port,
                        Path::new(&r.site.path),
                    )
                }),
                moves_network_domain: r.moves_network_domain,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Apply
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status")]
pub enum RewriteApplied {
    /// Written, verified, and marked connected — the record carries HOW.
    #[serde(rename = "applied", rename_all = "camelCase")]
    Applied { record: DbImportRecord, message: String },
    /// The file no longer matches the approved fingerprint — nothing was
    /// touched; re-open the preview.
    #[serde(rename = "fileChanged", rename_all = "camelCase")]
    FileChanged { message: String },
    /// rexenv's own engine isn't running — nothing was touched (§7's message
    /// gap, closed: this is OUR port, so the fix is OUR Databases page).
    #[serde(rename = "engineStopped", rename_all = "camelCase")]
    EngineStopped { message: String },
    /// The change was applied and backed up, but the sign-in check did not
    /// pass: the site is NOT marked connected, and revert is available.
    #[serde(rename = "verifyFailed", rename_all = "camelCase")]
    VerifyFailed { reason: String, message: String },
    /// Downgraded to tell-only at apply time (the file changed shape).
    #[serde(rename = "refused", rename_all = "camelCase")]
    Refused { reason: String, file: Option<String> },
}

#[tauri::command]
pub async fn rewrite_apply(
    state: State<'_, AppState>,
    provision: State<'_, crate::commands::site_provision::ProvisionJobs>,
    tunnels: State<'_, crate::commands::tunnels::Tunnels>,
    db_jobs: State<'_, crate::commands::db_import::DbImportJobs>,
    site_id: String,
    fingerprint: String,
) -> Result<RewriteApplied> {
    let r = match resolve(&state, &site_id)? {
        Resolution::Refused { reason, file } => {
            return Ok(RewriteApplied::Refused { reason, file })
        }
        Resolution::Ok(r) => r,
    };

    // Cross-guards, then the single-slot claim (cleared by Drop).
    if provision.busy_for(&r.site.domain) {
        return Err(Error::Other(format!(
            "a provision job is running for {} — wait for it to finish first.",
            r.site.domain
        )));
    }
    if let Ok(active) = state.db_import_active.lock() {
        if active.as_deref() == Some(r.site.domain.as_str()) {
            return Err(Error::Other(format!(
                "a database import is running for {} — wait for it (or cancel it) first.",
                r.site.domain
            )));
        }
    }
    {
        let mut g = state
            .rewrite_active
            .lock()
            .map_err(|_| Error::Other("rewrite flag poisoned".into()))?;
        if let Some(d) = g.as_deref() {
            return Err(Error::Other(format!(
                "a connection rewrite is already running (for {d}) — one at a time."
            )));
        }
        *g = Some(r.site.domain.clone());
    }
    let _slot = Slot(&state.rewrite_active);
    // Step 7: a tunnel EXPOSES rather than mutates, so this guard names what
    // a visitor would see. Placed AFTER the slot claim (set-then-check) so it
    // pairs with the tunnel start's claim-then-check — the two directions
    // can't cross; the Slot's Drop releases on refusal.
    crate::commands::tunnels::refuse_if_shared(
        &tunnels,
        &state,
        &r.site.domain,
        "rewriting its connection settings under the live link can break the site for anyone visiting",
    )?;

    // The approved fingerprint covers the WHOLE file. Any drift — even
    // outside the target lines — changes what a write means: refuse, and
    // touch nothing.
    if confrewrite::sha256_hex(r.original.as_bytes()) != fingerprint {
        // No path inside the prose — the UI shows the file in mono beside
        // the message (the mono rule can't reach into a flat string).
        return Ok(RewriteApplied::FileChanged {
            message: "The file changed since the diff was shown — nothing was written. \
                      The refreshed preview shows the change against the file as it is now."
                .into(),
        });
    }

    // Our engine must be up before anything mutates: the mirror needs it and
    // the verification needs it. This is rexenv's own port, so the honest
    // fix names OUR page, not DBngin.
    let engine = r.engine;
    if !engine.running() {
        return Ok(RewriteApplied::EngineStopped {
            message: format!(
                "this change points {} at rexenv's own {} (127.0.0.1:{}), which isn't \
                 running — start it from the Databases page, then apply again.",
                r.site.domain,
                engine.label(),
                engine.port()
            ),
        });
    }
    let engine_version = super::database::effective_db_version(&state, engine)?;
    let (db_client, _) =
        engine.sql_client_bins(state.platform.as_ref(), &engine_version).await?;

    // Mirror before the file points anywhere new — record-first (provenance,
    // db_created's shape), and ALWAYS converge: a second apply must never
    // skip account creation because the record already names the user.
    if let Some(dedicated) = &r.creates_user {
        {
            let conn = lock(&state)?;
            store::set_db_import_mirrored_user(&conn, &site_id, dedicated)?;
        }
        dbmirror::mirror_dedicated(
            &db_client,
            engine.port(),
            &r.record.db_name,
            &r.site.domain,
            &r.conn.password,
        )?;
    } else {
        // Non-root config: converge the Stage 2 mirror so a password the
        // user changed since import heals instead of failing verification.
        {
            let conn = lock(&state)?;
            store::set_db_import_mirrored_user(&conn, &site_id, &r.conn.user)?;
        }
        dbmirror::mirror(
            &db_client,
            engine.port(),
            &r.record.db_name,
            &r.conn.user,
            &r.conn.password,
        )?;
    }

    // Backup, first backup wins: written only when no record exists, and the
    // runtime order is backup file → row → write, so a crash between any two
    // steps leaves the original recoverable.
    let file_key = r.file.display().to_string();
    let backup_missing = {
        let conn = lock(&state)?;
        store::get_config_rewrite(&conn, &site_id, &file_key)?.is_none()
    };
    if backup_missing {
        let backup = confrewrite::backup_path(state.platform.as_ref(), &site_id, &r.file)?;
        confrewrite::write_backup(state.platform.as_ref(), &backup, &r.original)?;
        let conn = lock(&state)?;
        store::insert_config_rewrite(&conn, &site_id, &file_key, &backup.display().to_string())?;
    }

    // The write: exactly the previewed bytes, atomically, their mode kept.
    // An all-no-op plan (config already points at us) skips the write and
    // goes straight to verification — re-verifying is the point then.
    if r.rewrite.new_content != r.original {
        confrewrite::atomic_write_preserving_mode(state.platform.as_ref(), &r.file, &r.rewrite.new_content)?;
    }
    {
        let conn = lock(&state)?;
        store::set_config_rewrite_digest(
            &conn,
            &site_id,
            &file_key,
            &confrewrite::sha256_hex(r.rewrite.new_content.as_bytes()),
        )?;
    }

    // Verification — the ONLY source of `connected`. Re-reads the file from
    // disk and signs in with its own credentials.
    let scratch = state.platform.paths().app_data_dir()?.join("config-rewrites");
    let proof = match confverify::verify_signin(
        state.platform.as_ref(),
        &db_client,
        Path::new(&r.site.path),
        &scratch,
        engine.port(),
        &r.record.db_name,
    )? {
        Ok(proof) => proof,
        Err(fail) => {
            return Ok(RewriteApplied::VerifyFailed {
                reason: fail.message(),
                message: format!(
                    "the change was applied and backed up, but the sign-in check didn't \
                     pass — {} is not marked connected. You can revert the change from \
                     this card.",
                    r.site.domain
                ),
            })
        }
    };

    // D4: the supplementary probe can only upgrade the proof.
    let proof =
        if confverify::probe_http(&r.site.domain).await { proof.with_http_confirmed() } else { proof };

    let record = {
        let conn = lock(&state)?;
        store::set_db_import_connected(&conn, &site_id, ConnectedVerified::from_verification(&proof))?;
        store::get_db_import(&conn, &site_id)?
            .ok_or_else(|| Error::Other("db_imports row vanished mid-apply".into()))?
    };
    // The record just changed: an import job from before it describes a state that is gone (#812).
    db_jobs.forget_settled(&site_id);
    Ok(RewriteApplied::Applied {
        record,
        message: format!(
            "verified: the rewritten settings sign in to the rexenv copy of `{}`.",
            r.record.db_name
        ),
    })
}

// ---------------------------------------------------------------------------
// Revert
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status")]
pub enum RevertOutcome {
    /// The original is back, byte for byte; the backup and record are gone;
    /// the state returned to `imported`.
    #[serde(rename = "reverted", rename_all = "camelCase")]
    Reverted { file: String, message: String },
    /// The file was edited after the rewrite (or can't be proven unchanged):
    /// refused without `force` — restoring would lose those edits.
    #[serde(rename = "refusedEdited", rename_all = "camelCase")]
    RefusedEdited { file: String, reason: FileEditedReason, message: String },
    /// Our copy of the original is gone. THEIR file is left exactly as it
    /// is; `connected` stays (it is still true); the record is dropped.
    #[serde(rename = "backupMissing", rename_all = "camelCase")]
    BackupMissing { file: String, message: String },
    /// Nothing to revert.
    #[serde(rename = "noRewrite", rename_all = "camelCase")]
    NoRewrite { message: String },
}

#[tauri::command]
pub async fn rewrite_revert(
    state: State<'_, AppState>,
    provision: State<'_, crate::commands::site_provision::ProvisionJobs>,
    tunnels: State<'_, crate::commands::tunnels::Tunnels>,
    db_jobs: State<'_, crate::commands::db_import::DbImportJobs>,
    site_id: String,
    force: bool,
) -> Result<RevertOutcome> {
    let site = {
        let conn = lock(&state)?;
        core::sites::get(&conn, &site_id)?
            .ok_or_else(|| Error::Other("site not found".into()))?
    };
    if provision.busy_for(&site.domain) {
        return Err(Error::Other(format!(
            "a provision job is running for {} — wait for it to finish first.",
            site.domain
        )));
    }
    if let Ok(active) = state.db_import_active.lock() {
        if active.as_deref() == Some(site.domain.as_str()) {
            return Err(Error::Other(format!(
                "a database import is running for {} — wait for it (or cancel it) first.",
                site.domain
            )));
        }
    }
    {
        let mut g = state
            .rewrite_active
            .lock()
            .map_err(|_| Error::Other("rewrite flag poisoned".into()))?;
        if let Some(d) = g.as_deref() {
            return Err(Error::Other(format!(
                "a connection rewrite is already running (for {d}) — one at a time."
            )));
        }
        *g = Some(site.domain.clone());
    }
    let _slot = Slot(&state.rewrite_active);
    // Step 7 (same shape as apply): reverting connection settings under a
    // live link can break the site for anyone visiting.
    crate::commands::tunnels::refuse_if_shared(
        &tunnels,
        &state,
        &site.domain,
        "reverting its connection settings under the live link can break the site for anyone visiting",
    )?;

    let rows = {
        let conn = lock(&state)?;
        store::config_rewrites_for_site(&conn, &site_id)?
    };
    let Some(row) = rows.into_iter().next() else {
        return Ok(RevertOutcome::NoRewrite {
            message: format!("no connection rewrite is recorded for {}.", site.domain),
        });
    };

    let current = std::fs::read_to_string(&row.file).ok();
    let backup = std::fs::read_to_string(&row.backup_path).ok();
    let check =
        confrewrite::classify_revert(current.as_deref(), backup.as_deref(), row.written_digest.as_deref());

    let outcome = match check {
        RevertCheck::BackupMissing => {
            // D-b: their file is untouched; `connected` stays — the config
            // still signs into our copy, and clearing it would be the
            // over-claim's mirror image. The row goes: a revert that can
            // never work must not keep rendering.
            let conn = lock(&state)?;
            store::delete_config_rewrite(&conn, &site_id, &row.file)?;
            Ok(RevertOutcome::BackupMissing {
                file: row.file.clone(),
                message: "rexenv's copy of the original is gone; your file was left \
                          exactly as it is. To go back to the old database, edit the \
                          file yourself."
                    .into(),
            })
        }
        RevertCheck::FileEdited { reason } if !force => Ok(RevertOutcome::RefusedEdited {
            file: row.file.clone(),
            reason,
            // Pathless prose: the UI renders `file` in mono beside it.
            message: match reason {
                FileEditedReason::EditedSinceRewrite => {
                    "This file was edited after the rewrite — restoring the backup would \
                     replace those edits. Choose \"restore anyway\" to proceed."
                }
                FileEditedReason::UnknownDigest => {
                    "rexenv can't prove this file is unchanged since the rewrite — \
                     restoring the backup could replace later edits. Choose \
                     \"restore anyway\" to proceed."
                }
                FileEditedReason::FileMissing => {
                    "This file no longer exists — restoring would recreate a file that \
                     was removed. Choose \"restore anyway\" to proceed."
                }
            }
            .into(),
        }),
        RevertCheck::AlreadyReverted => {
            // A failed write, or the crash window after a restore: converge.
            let conn = lock(&state)?;
            store::clear_db_import_connected(&conn, &site_id)?;
            let _ = std::fs::remove_file(&row.backup_path);
            store::delete_config_rewrite(&conn, &site_id, &row.file)?;
            Ok(RevertOutcome::Reverted {
                file: row.file.clone(),
                message: format!(
                    "{} already matches the original — the backup and the connection \
                     fact were cleaned up.",
                    row.file
                ),
            })
        }
        RevertCheck::CleanRestore | RevertCheck::FileEdited { .. } => {
            // D-c order: clear `connected` FIRST, so a crash mid-revert lands
            // in the under-claim direction (badge pessimistic, site working)
            // rather than showing "connected" over a reverted file.
            {
                let conn = lock(&state)?;
                store::clear_db_import_connected(&conn, &site_id)?;
            }
            let backup_text = backup.ok_or_else(|| {
                Error::Other("backup vanished between classification and restore".into())
            })?;
            confrewrite::atomic_write_preserving_mode(state.platform.as_ref(), Path::new(&row.file), &backup_text)?;
            {
                let conn = lock(&state)?;
                let _ = std::fs::remove_file(&row.backup_path);
                store::delete_config_rewrite(&conn, &site_id, &row.file)?;
            }
            Ok(RevertOutcome::Reverted {
                file: row.file.clone(),
                message: format!(
                    "{} was restored to the original, byte for byte; the site is back on \
                     its previous connection settings.",
                    row.file
                ),
            })
        }
    };
    // The record just changed: an import job from before it describes a state that is gone (#812).
    if matches!(outcome, Ok(RevertOutcome::Reverted { .. })) {
        db_jobs.forget_settled(&site_id);
    }
    outcome
}


/// #182 — the two orderings that decide what a CRASH leaves behind.
#[cfg(test)]
mod crash_ordering {
    fn body_of(fn_sig: &str) -> String {
        let src = crate::core::copy_scan::production_source(include_str!("rewrite.rs"));
        src.split(fn_sig)
            .nth(1)
            .unwrap_or_else(|| panic!("`{fn_sig}` is gone — if it moved, move this guard"))
            .split("\npub ")
            .next()
            .unwrap_or_default()
            .to_string()
    }

    fn at(body: &str, needle: &str) -> usize {
        body.find(needle)
            .unwrap_or_else(|| panic!("`{needle}` is no longer in this function — the step it \
                                       represents moved, so re-read what the ordering protects"))
    }

    /// **A collision-renamed copy is connected by renaming the config, not
    /// refused** (ledger #574). The refusal this replaces was ruled permanent
    /// on 28 Jul 2026 and reversed on 12 Sep 2026, when every second Local
    /// import met it. The guard pins BOTH halves: the refusal is gone, and the
    /// rename reaches the plan — a `rename` computed and never applied would
    /// connect the site to a database that isn't its copy, and the sign-in
    /// check would then fail the apply with no explanation of why.
    #[test]
    fn a_renamed_copy_reaches_the_plan_instead_of_a_refusal() {
        let body = body_of("fn resolve(");
        assert!(
            !body.contains("doesn't cover a database rename"),
            "the tell-only rename refusal is back in `resolve`"
        );
        let computed = at(&body, "conn.database != record.db_name");
        let applied = at(&body, "plan.with_database(name)");
        assert!(computed < applied, "the rename must be computed before it is applied to the plan");
        assert!(
            at(&body, "confedit::rewrite(&original, &plan)") > applied,
            "the diff must be produced from the plan AFTER the rename is added, or the preview omits it"
        );
    }

    /// A network's DOMAIN_CURRENT_SITE reaches the plan before the diff is
    /// produced — decided from the site row (is it a network?) AND the file
    /// (does the constant differ from the rexenv name?). A decision computed and
    /// never staged connects a network whose logins fail; one staged after the
    /// diff writes a change the preview never showed.
    #[test]
    fn a_networks_domain_reaches_the_plan_before_the_diff() {
        let body = body_of("fn resolve(");
        let decided = at(&body, "let moves_network_domain");
        let staged = at(&body, "plan.with_network_domain(&site.domain)");
        assert!(decided < staged, "the network decision must precede staging it");
        assert!(
            at(&body, "confedit::rewrite(&original, &plan)") > staged,
            "the diff must be produced from the plan AFTER the network domain is added"
        );
        let rule = &body[decided..staged];
        for needed in ["site.multisite", "\"DOMAIN_CURRENT_SITE\"", "v != site.domain"] {
            assert!(rule.contains(needed), "the network rule lost `{needed}`: {rule}");
        }
    }

    /// **Apply is ordered so every crash window leaves the ORIGINAL
    /// recoverable, and `connected` is never claimed before it is true.**
    ///
    /// Four windows, each with a different wrong answer if the order slips:
    /// mirroring after the file points at us leaves a config aimed at an
    /// account that does not exist yet; recording the mirrored user after
    /// mirroring leaves an account nothing knows we created; writing before the
    /// backup file and its row leaves the user's original nowhere; and setting
    /// `connected` before the sign-in verification is the badge claiming a
    /// thing nobody checked — the one direction this product refuses to fail
    /// in.
    #[test]
    fn apply_never_leaves_a_crash_in_the_over_claiming_direction() {
        let b = body_of("pub async fn rewrite_apply(");
        let record = at(&b, "set_db_import_mirrored_user(");
        let mirror = at(&b, "dbmirror::mirror");
        let backup_file = at(&b, "write_backup(");
        let backup_row = at(&b, "insert_config_rewrite(");
        let write = at(&b, "atomic_write_preserving_mode(");
        let verify = at(&b, "confverify::verify_signin");
        let connected = at(&b, "set_db_import_connected(");

        assert!(
            record < mirror,
            "the mirrored user is recorded AFTER the account is created — a crash between them \
             leaves an account nothing knows we made, so nothing will ever clean it up"
        );
        assert!(
            mirror < write,
            "the file is pointed at the new account BEFORE that account exists — a crash \
             between them leaves the site connecting as a user that was never created"
        );
        assert!(
            backup_file < backup_row && backup_row < write,
            "the backup order (file → row → write) is broken: a crash then leaves the user's \
             ORIGINAL config nowhere — the one thing revert exists to hand back"
        );
        assert!(
            verify < connected,
            "`connected` is set before the sign-in verification — the badge would claim a \
             connection nobody proved, which is the over-claim this whole module is shaped to \
             avoid"
        );
    }

    /// **Revert clears `connected` BEFORE it restores the file**, so a crash
    /// mid-revert lands in the under-claim direction.
    ///
    /// The two failures are not symmetric. Cleared-then-crashed shows a
    /// pessimistic badge over a site that still works — annoying, and the next
    /// revert fixes it. Restored-then-crashed shows "connected" over a config
    /// that no longer points at us: the user believes a thing that is false and
    /// has no reason to look.
    #[test]
    fn revert_clears_the_claim_before_it_changes_the_file() {
        // The RESTORING arm only. `rewrite_revert` classifies first and has
        // several outcomes, two of which also clear the flag and drop the row —
        // scanning the whole function finds the first occurrence in whichever
        // arm comes first in the file and compares steps that never run
        // together. (It did: the guard's first version failed on an
        // already-reverted arm's row delete.)
        let whole = body_of("pub async fn rewrite_revert(");
        let b = whole
            .split("RevertCheck::CleanRestore | RevertCheck::FileEdited { .. } => {")
            .nth(1)
            .expect("the restoring arm — if the match changed shape, re-read what it now does")
            .to_string();
        let clear = at(&b, "clear_db_import_connected(");
        let restore = at(&b, "atomic_write_preserving_mode(");
        let drop_row = at(&b, "delete_config_rewrite(");
        assert!(
            clear < restore,
            "the file is restored BEFORE `connected` is cleared — a crash in between leaves the \
             badge claiming a connection over a config that no longer points at us, and the \
             user has no reason to doubt it"
        );
        assert!(
            restore < drop_row,
            "the rewrite row is dropped before the file is restored — the backup it names is \
             then unreachable exactly when the restore needs retrying"
        );
    }
}

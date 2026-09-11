//! commands::db_import — the streamed database-import job (Stage 2 steps 8–9).
//! Thin over `core::{dbimport, dbsource, dbcompat, dbdump, dbrestore, dbmirror}`;
//! the sequencing here IS the preflight order, and the core signatures enforce
//! it (`Cleared` → `Preflight` → `Verified` witnesses).
//!
//! One import at a time, and never alongside a provision job for the same
//! site: `AppState::db_import_active` is the flag `site_provision::start`
//! checks, and this command checks `ProvisionJobs::busy_for` — both directions
//! covered.
//!
//! Honest-progress contract as everywhere: monotonic, ≤99 until settle, frozen
//! on failure/cancel; the only intra-phase signals are REAL bytes (artifact
//! growth during dump, bytes we fed during restore — the latter tops out below
//! the phase ceiling because the client is still executing after the last
//! byte).

use crate::core::db::DbEngine;
use crate::core::dbcompat::{self, Verdict};
use crate::core::dbimport::{DbSiteStatus, Driver};
use crate::core::dbrestore::FeedOutcome;
use crate::core::dbsource::{Identity, Vendor};
use crate::core::{self, dbdump, dbimport, dbmirror, dbrestore, dbsource};
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::store::DbImportRecord;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

fn state_event(id: &str) -> String {
    format!("db-import://state/{id}")
}

#[derive(Default)]
pub struct DbImportJobs {
    jobs: Mutex<HashMap<String, Arc<Entry>>>,
}

struct Entry {
    id: String,
    cancel: AtomicBool,
    running: AtomicBool,
    log_path: PathBuf,
    state: Mutex<DbImportJobState>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobPhase {
    pub key: String,
    pub label: String,
    /// "pending" | "running" | "ok" | "failed" | "cancelled" | "skipped".
    pub status: String,
}

/// The card's whole truth for one running/settled import job.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbImportJobState {
    pub id: String,
    pub site_id: String,
    pub domain: String,
    pub phases: Vec<JobPhase>,
    pub phase_cursor: usize,
    pub pct: u8,
    /// "running" | "ok" | "failed" | "cancelled".
    pub status: String,
    pub error: Option<String>,
    pub log_key: String,
    /// On failure: the kept artifact, so the summary can name where the copy
    /// (their data!) sits and Settings can offer to remove it.
    pub kept_artifact: Option<String>,
    /// On success: the settled record the UI renders §9 from.
    pub result: Option<DbImportRecord>,
}

const PHASES: [(&str, &str, u8); 5] = [
    ("check", "checking the source database", 10),
    ("dump", "copying the database out", 40),
    ("engine", "starting rexenv's database", 10),
    ("restore", "restoring the copy", 35),
    ("settle", "finishing up", 5),
];

fn snapshot(entry: &Entry) -> DbImportJobState {
    entry.state.lock().expect("db import state lock").clone()
}

fn emit<R: tauri::Runtime>(app: &AppHandle<R>, entry: &Entry) {
    let _ = app.emit(&state_event(&entry.id), snapshot(entry));
}

fn log_line<R: tauri::Runtime>(app: &AppHandle<R>, entry: &Entry, line: &str) {
    if let Ok(mut f) =
        std::fs::OpenOptions::new().create(true).append(true).open(&entry.log_path)
    {
        let _ = writeln!(f, "{line}");
    }
    let _ = app.emit(&format!("db-import://output/{}", entry.id), line.to_string());
}

/// Advance to phase `idx` (marks everything before it ok) and floor pct at the
/// weight boundary. Monotonic by construction.
fn enter_phase(entry: &Entry, idx: usize) {
    let mut st = entry.state.lock().expect("db import state lock");
    for (i, p) in st.phases.iter_mut().enumerate() {
        if i < idx && (p.status == "running" || p.status == "pending") {
            p.status = "ok".into();
        }
    }
    if let Some(p) = st.phases.get_mut(idx) {
        p.status = "running".into();
    }
    st.phase_cursor = idx;
    let floor: u8 = PHASES.iter().take(idx).map(|(_, _, w)| w).sum();
    st.pct = st.pct.max(floor);
}

/// Intra-phase progress: `frac` of the phase's weight, capped so a phase never
/// claims its own ceiling (the settle flips 100).
fn phase_progress(entry: &Entry, idx: usize, frac: f64) {
    let mut st = entry.state.lock().expect("db import state lock");
    let floor: u8 = PHASES.iter().take(idx).map(|(_, _, w)| w).sum();
    let weight = PHASES[idx].2 as f64;
    let add = (weight * frac.clamp(0.0, 0.95)) as u8;
    st.pct = st.pct.max(floor + add).min(99);
}

fn settle(entry: &Entry, status: &str, error: Option<String>, kept: Option<String>, result: Option<DbImportRecord>) {
    let mut st = entry.state.lock().expect("db import state lock");
    let cursor = st.phase_cursor;
    for (i, p) in st.phases.iter_mut().enumerate() {
        match status {
            "ok" => p.status = "ok".into(),
            _ if i == cursor => p.status = status.to_string(),
            _ if p.status == "pending" => p.status = "skipped".into(),
            _ => {}
        }
    }
    if status == "ok" {
        st.pct = 100;
    } // else frozen where it was
    st.status = status.into();
    st.error = error;
    st.kept_artifact = kept;
    st.result = result;
}

/// Start importing `site_id`'s database. One at a time; refuses while a
/// provision job runs for the same domain.
#[tauri::command]
pub async fn db_import_start<R: tauri::Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    jobs: State<'_, DbImportJobs>,
    provision: State<'_, crate::commands::site_provision::ProvisionJobs>,
    tunnels: State<'_, crate::commands::tunnels::Tunnels>,
    site_id: String,
    confirm_overwrite: Option<String>,
) -> Result<DbImportJobState> {
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
    // The rewrite holds this site's config file and imported-state row; an
    // import alongside it would race both (same guard the rewrite honours in
    // the other direction).
    if let Ok(active) = state.rewrite_active.lock() {
        if active.as_deref() == Some(site.domain.as_str()) {
            return Err(Error::Other(format!(
                "a connection rewrite is running for {} — wait for it to finish first.",
                site.domain
            )));
        }
    }
    {
        let mut active = state
            .db_import_active
            .lock()
            .map_err(|_| Error::Other("db import flag poisoned".into()))?;
        if let Some(d) = active.as_deref() {
            return Err(Error::Other(format!(
                "a database import is already running (for {d}) — one at a time."
            )));
        }
        *active = Some(site.domain.clone());
    }
    // Step 7: a tunnel EXPOSES rather than mutates — the refusal names what a
    // visitor would see. Placed AFTER the marker-set (set-then-check) so it
    // pairs with the tunnel start's claim-then-check and the two directions
    // can't cross; refusal clears the marker it just took.
    if let Err(e) = crate::commands::tunnels::refuse_if_shared(
        &tunnels,
        &state,
        &site.domain,
        "importing its database would drop and rebuild it under the live link, showing \
         visitors errors or half-restored content",
    ) {
        if let Ok(mut active) = state.db_import_active.lock() {
            *active = None;
        }
        return Err(e);
    }

    let id = uuid::Uuid::new_v4().to_string();
    let log_dir = state.platform.paths().log_dir()?;
    prune_logs(&log_dir, &format!("db-import-{}-", site.domain));
    let log_key = format!("db-import-{}-{}.log", site.domain, &id[..8]);
    let entry = Arc::new(Entry {
        id: id.clone(),
        cancel: AtomicBool::new(false),
        running: AtomicBool::new(true),
        log_path: log_dir.join(&log_key),
        state: Mutex::new(DbImportJobState {
            id: id.clone(),
            site_id: site.id.clone(),
            domain: site.domain.clone(),
            phases: PHASES
                .iter()
                .map(|(k, l, _)| JobPhase {
                    key: (*k).into(),
                    label: (*l).into(),
                    status: "pending".into(),
                })
                .collect(),
            phase_cursor: 0,
            pct: 0,
            status: "running".into(),
            error: None,
            log_key,
            kept_artifact: None,
            result: None,
        }),
    });
    {
        // One settled job per site: the map was never pruned, so a retry's
        // `db_import_state` lookup (a `find` over `values()`) could bind the
        // card to the OLD settled job — its old error, a `kept_artifact` the
        // new run already deleted — and, being settled, never subscribe.
        let mut map = jobs.jobs.lock().expect("db import jobs lock");
        map.retain(|_, e| {
            e.running.load(Ordering::SeqCst)
                || e.state.lock().map(|s| s.site_id != site.id).unwrap_or(true)
        });
        map.insert(id, entry.clone());
    }
    let _ = std::fs::write(&entry.log_path, "");

    let worker_app = app.clone();
    let worker_entry = entry.clone();
    let app_handle = app.app_handle().clone();
    tauri::async_runtime::spawn(async move {
        let state: State<'_, AppState> = app_handle.state();
        let outcome = run(&worker_app, &state, &worker_entry, site, confirm_overwrite).await;
        match outcome {
            Ok(()) => {}
            Err(e) => {
                log_line(&worker_app, &worker_entry, &format!("FAILED: {e}"));
                let kept = {
                    let st = worker_entry.state.lock().expect("state lock");
                    st.kept_artifact.clone()
                };
                settle(&worker_entry, "failed", Some(e.to_string()), kept, None);
            }
        }
        worker_entry.running.store(false, Ordering::SeqCst);
        if let Ok(mut active) = state.db_import_active.lock() {
            *active = None;
        }
        emit(&worker_app, &worker_entry);
    });
    Ok(snapshot(&entry))
}

/// The job body. Any `Err` becomes a frozen `failed` state in the caller.
async fn run<R: tauri::Runtime>(
    app: &AppHandle<R>,
    state: &State<'_, AppState>,
    entry: &Entry,
    site: crate::state::models::Site,
    confirm_overwrite: Option<String>,
) -> Result<()> {
    let platform = state.platform.as_ref();
    let dest_dir = platform.paths().app_data_dir()?.join("db-imports");

    // ── check: config → probe → is-this-us → verdict → live → disk ─────────
    enter_phase(entry, 0);
    emit(app, entry);
    let mut conn_info = dbimport::read_connection(Path::new(&site.path)).map_err(|(reason, source)| {
        Error::Other(DbSiteStatus::NeedsAttention { reason, source }.message())
    })?;
    // A Local site's wp-config says `localhost` — Local's socket for THIS site's
    // own mysqld — so its data is wherever Local's registry puts that server,
    // not on `localhost:3306` (which is nothing, or somebody's Homebrew MySQL
    // with its own `local` database). Only a bare `localhost` is replaced; an
    // explicit host is a choice and wins. Re-read from their registry on every
    // run, like the credentials are from wp-config (ledger #572).
    let local = directories::BaseDirs::new()
        .filter(|_| core::localwp::overrides_config(&conn_info.host, conn_info.port))
        .and_then(|b| core::localwp::db_source_for(b.home_dir(), Path::new(&site.path)));
    if let Some(l) = &local {
        log_line(
            app,
            entry,
            &format!(
                "{} is a Local site: its database is Local's own server for it (127.0.0.1:{}, \
                 signed in over its socket), not the `localhost` wp-config names",
                l.site_name, l.port
            ),
        );
        conn_info.host = "127.0.0.1".into();
        conn_info.port = l.port;
    }
    log_line(app, entry, &format!("source: {:?}", conn_info.info()));
    // The target name is decided (and validated) HERE, before the dump: the
    // first version validated it at prepare_target, after minutes and
    // gigabytes of dump, and `DB_NAME=my-site` — routine under Valet and Herd,
    // legal MySQL when backticked — burned the whole dump to die on
    // `invalid database name`.
    crate::core::database::validate_db_name(&target_db_name(&conn_info.database, None))?;
    match conn_info.driver {
        Driver::MysqlFamily => {}
        other => {
            return Err(Error::Other(
                DbSiteStatus::UnsupportedDriver {
                    driver: other,
                    conn: Box::new(conn_info.info()),
                }
                .message(),
            ))
        }
    }

    let identity = match dbsource::probe(&conn_info.host, conn_info.port) {
        dbsource::Probe::Listening(id) => id,
        // Named for Local, because the generic "start it (DBngin, Herd…)" points
        // at the wrong app — and Local runs a site's database only while that
        // one site is started, which nobody guesses from a port number.
        dbsource::Probe::NotListening(_) if local.is_some() => {
            let l = local.as_ref().expect("guarded");
            return Err(Error::Other(format!(
                "`{}` is the database of the \"{}\" site in Local, and its server isn't \
                 running — Local runs a site's database only while that site is started. \
                 Start \"{}\" in Local, then retry. rexenv never starts or stops Local's servers.",
                conn_info.database, l.site_name, l.site_name
            )));
        }
        dbsource::Probe::NotListening(_) => {
            return Err(Error::Other(
                DbSiteStatus::ServerUnreachable { conn: Box::new(conn_info.info()) }.message(),
            ))
        }
    };
    log_line(app, entry, &format!("source server: {identity:?}"));

    // Snapshot OUR engines for the is-this-us question.
    let ours: Vec<dbdump::OurEngine> = {
        let mgr = state.services.lock().await;
        mgr.db_status()
            .iter()
            .filter(|d| d.running)
            .map(|d| dbdump::OurEngine { port: d.engine.port(), version: d.version.clone() })
            .collect()
    };
    let self_import = if dbdump::server_is_ours(&conn_info.host, conn_info.port, &identity, &ours)
    {
        let conn = lock(state)?;
        Some((dbdump::classify_self_import(&conn, &site.id, &conn_info.database)?, conn_info.database.clone()))
    } else {
        None
    };

    let target_engine = DbEngine::from_site(site.db_engine);
    let target_version = super::database::effective_db_version(state, target_engine)?;
    let (src_vendor, src_version) = match &identity {
        Identity::Handshake { vendor, version } => (Some(*vendor), Some(version.clone())),
        _ => (None, None),
    };
    let verdict = dbcompat::compat(
        &dbcompat::Source {
            vendor: src_vendor,
            version: src_version.as_deref().and_then(dbcompat::Version::parse),
        },
        &dbcompat::Target {
            vendor: match target_engine {
                DbEngine::Mariadb => Vendor::Mariadb,
                _ => Vendor::Mysql,
            },
            version: dbcompat::Version::parse(&target_version)
                .ok_or_else(|| Error::Other("unparseable target version".into()))?,
        },
    );
    if let v @ Verdict::NeedsOverride { .. } = &verdict {
        // The per-site override UI arrives with the batch screen wiring; the
        // honest default is the verdict's own explanation.
        return Err(Error::Other(v.explain()));
    }
    let cleared = dbdump::gate(self_import, &verdict, false)
        .map_err(|r| Error::Other(r.message()))?;

    // Tools for the SOURCE vendor (client pairing is a hard invariant).
    let source_vendor = src_vendor.expect("gate passed implies identified");
    let (src_engine, src_engine_version) = match source_vendor {
        Vendor::Mysql => {
            let v = super::database::effective_db_version(state, DbEngine::Mysql)?;
            (DbEngine::Mysql, v)
        }
        Vendor::Mariadb => {
            let v = super::database::effective_db_version(state, DbEngine::Mariadb)?;
            (DbEngine::Mariadb, v)
        }
    };
    let plan = core::downloads::plan_for_engine(platform, src_engine, &src_engine_version);
    core::downloads::prefetch(platform, "Database import (tools)", &plan).await?;
    let (src_client, src_dump) = src_engine.sql_client_bins(platform, &src_engine_version).await?;

    let defaults = match &local {
        // The TCP port answered the probe above; sign-in goes the way the site
        // itself signs in. No socket file (an unusual Local build) → TCP.
        Some(l) if l.socket.exists() => {
            dbdump::DefaultsFile::create_via_socket(platform, &dest_dir, &conn_info, &l.socket)?
        }
        _ => dbdump::DefaultsFile::create(platform, &dest_dir, &conn_info)?,
    };
    let (size, skip_tables) = match dbdump::preflight_live(
        &cleared,
        &src_client,
        &defaults,
        &conn_info.database,
    )? {
        dbdump::LiveCheck::Ready { size, unreadable } => (size, unreadable),
        dbdump::LiveCheck::SigninRefused(detail) => {
            return Err(Error::Other(
                DbSiteStatus::CredentialsRejected {
                    conn: Box::new(conn_info.info()),
                    detail,
                }
                .message(),
            ))
        }
        dbdump::LiveCheck::Unreachable(_) => {
            return Err(Error::Other(
                DbSiteStatus::ServerUnreachable { conn: Box::new(conn_info.info()) }.message(),
            ))
        }
        dbdump::LiveCheck::DatabaseMissing { available } => {
            return Err(Error::Other(
                DbSiteStatus::DatabaseMissing {
                    conn: Box::new(conn_info.info()),
                    available,
                    server: src_version.clone(),
                }
                .message(),
            ))
        }
    };
    let preflight =
        dbdump::check_disk(size, &dest_dir).map_err(|s| Error::Other(s.message()))?;
    log_line(
        app,
        entry,
        &format!("{} tables, ~{} bytes; disk ok", size.table_count, size.total_bytes),
    );
    // Said BEFORE the copy starts and named one per line: a skipped table is
    // data the user does not get, so it is never a footnote on a success.
    if !skip_tables.is_empty() {
        log_line(
            app,
            entry,
            &format!(
                "{} table(s) can't be read on the source and will be SKIPPED — the copy \
                 will be missing them:",
                skip_tables.len()
            ),
        );
        for t in &skip_tables {
            log_line(app, entry, &format!("  skipped: {t}"));
        }
    }

    // ── dump ────────────────────────────────────────────────────────────────
    enter_phase(entry, 1);
    emit(app, entry);
    let src_version_str = src_version.clone().unwrap_or_default();
    let req = dbdump::DumpRequest {
        tool: &src_dump,
        tool_vendor: source_vendor,
        db: &conn_info.database,
        domain: &site.domain,
        source_host: &conn_info.host,
        source_port: conn_info.port,
        source_vendor,
        source_version: &src_version_str,
        target_engine: target_engine.key(),
        target_version: &target_version,
        dump_tool_label: &format!("{} {}", if source_vendor == Vendor::Mariadb { "mariadb-dump" } else { "mysqldump" }, src_engine_version),
        dest_dir: &dest_dir,
        skip_tables: &skip_tables,
    };
    let estimated = preflight.estimated_dump_bytes.max(1);
    let app2 = app.clone();
    let entry_id = entry.id.clone();
    let _ = (&app2, &entry_id);
    let mut last_emit = std::time::Instant::now();
    let outcome = {
        let mut on_bytes = |b: u64| {
            phase_progress(entry, 1, b as f64 / estimated as f64);
            if last_emit.elapsed().as_millis() > 300 {
                emit(app, entry);
                last_emit = std::time::Instant::now();
            }
        };
        dbdump::dump(&cleared, platform, &preflight, &req, &defaults, &entry.cancel, &mut on_bytes)?
    };
    let (artifact, manifest) = match outcome {
        dbdump::DumpOutcome::Done { artifact, manifest } => (artifact, manifest),
        dbdump::DumpOutcome::Cancelled => {
            settle(entry, "cancelled", None, None, None);
            return Ok(());
        }
    };
    log_line(app, entry, &format!("dumped {} bytes, {} tables", manifest.artifact_bytes, manifest.tables.len()));
    // From here a failure keeps the artifact for diagnosis — and the state
    // names it, because it contains their data.
    {
        let mut st = entry.state.lock().expect("state lock");
        st.kept_artifact = Some(artifact.display().to_string());
    }

    // ── our engine up ───────────────────────────────────────────────────────
    enter_phase(entry, 2);
    emit(app, entry);
    let plan = core::downloads::plan_for_engine(platform, target_engine, &target_version);
    core::downloads::prefetch(platform, "Database import", &plan).await?;
    let check = {
        let mut mgr = state.services.lock().await;
        mgr.set_db_version(target_engine, &target_version);
        mgr.spawn_db(platform, target_engine).await?
    };
    core::service_manager::await_ready(check.into_iter().collect()).await?;
    let (tgt_client, _) = target_engine.sql_client_bins(platform, &target_version).await?;

    // ── restore ─────────────────────────────────────────────────────────────
    enter_phase(entry, 3);
    emit(app, entry);
    // D1: keep their name when free; disambiguate when another SITE owns it;
    // typed confirmation when an unclaimed database of that name exists.
    let mut name = target_db_name(&conn_info.database, None);
    if name != conn_info.database {
        log_line(app, entry, &format!(
            "`{}` is not a name rexenv's engine can hold as-is — importing as `{name}`",
            conn_info.database
        ));
    }
    {
        let conn = lock(state)?;
        if let Some(other) = crate::state::store::site_with_db_name(&conn, &name)? {
            if other.id != site.id {
                name = target_db_name(&conn_info.database, Some(&site.domain));
                log_line(app, entry, &format!(
                    "`{}` belongs to {} — importing as `{name}` instead",
                    conn_info.database, other.domain
                ));
            }
        }
    }
    let exists_now = dbrestore::database_exists(&tgt_client, target_engine.port(), &name)?;
    let recorded = {
        let conn = lock(state)?;
        let site_now = core::sites::get(&conn, &site.id)?
            .ok_or_else(|| Error::Other("site vanished mid-import".into()))?;
        if exists_now && site_now.db_created.is_none() {
            // An unclaimed database of this name is on our engine. Never
            // overwritten silently: the user must type the name to confirm.
            if confirm_overwrite.as_deref() != Some(name.as_str()) {
                return Err(Error::Other(format!(
                    "a database called `{name}` already exists on rexenv's {} — and no \
                     rexenv site owns it, so rexenv won't overwrite it silently. To \
                     replace it with this import, type the database name to confirm.",
                    target_engine.label()
                )));
            }
        }
        dbrestore::record_provenance(&conn, &site.id, exists_now)?
    };
    dbrestore::prepare_target(&recorded, &tgt_client, target_engine.port(), &name)?;
    let total = manifest.artifact_bytes.max(1);
    let mut last_emit = std::time::Instant::now();
    let fed = {
        let mut on_bytes = |b: u64| {
            phase_progress(entry, 3, b as f64 / total as f64);
            if last_emit.elapsed().as_millis() > 300 {
                emit(app, entry);
                last_emit = std::time::Instant::now();
            }
        };
        dbrestore::feed(
            &recorded,
            &tgt_client,
            target_engine.port(),
            &name,
            &artifact,
            &manifest,
            &entry.cancel,
            &mut on_bytes,
        )
    };
    let fed = match fed {
        Ok(FeedOutcome::Fed { bytes }) => bytes,
        Ok(FeedOutcome::Cancelled) => {
            // Our partial copy is dropped (only if ours — the function checks);
            // the artifact stays for a quick retry-by-hand or deletion.
            let dropped =
                dbrestore::cleanup_failed(&recorded, &tgt_client, target_engine.port(), &name)?;
            log_line(app, entry, &format!("cancelled; partial copy dropped: {dropped}"));
            settle(entry, "cancelled", None, Some(artifact.display().to_string()), None);
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    log_line(app, entry, &format!("fed {fed} bytes"));
    let verified = dbrestore::verify_complete(&tgt_client, target_engine.port(), &name, &manifest)?;

    // ── settle: finish, RECORD, then mirror, drop the artifact ─────────────
    //
    // Record-first, like the Stage 3 rewrite (`rewrite.rs`) and for the same
    // reason: the mirror creates an account holding the site's real password,
    // and a failure between creating it and recording it (a poisoned lock, a
    // sqlite error, a site deleted mid-import) left a credentialed account
    // nothing named — site delete drops users strictly from the record, so it
    // was never cleaned up and needed a manual `DROP USER`. The record now
    // names the user it is ABOUT to create; a mirror that then fails leaves a
    // record over-claiming a user that does not exist, which the delete's
    // `DROP USER IF EXISTS` tolerates — the safe direction.
    enter_phase(entry, 4);
    emit(app, entry);
    let intended_user = (!dbmirror::is_reserved(&conn_info.user)).then(|| conn_info.user.clone());
    let record = {
        let conn = lock(state)?;
        dbrestore::finish(&conn, &site.id, &name, &verified)?;
        // The write shape has no state field: an import can only land
        // 'imported' — 'connected' is minted solely by the Stage 3 rewrite
        // job's verification. The upsert returns the stored row.
        let new = crate::state::store::NewDbImport {
            site_id: site.id.clone(),
            db_name: name.clone(),
            table_count: verified.tables,
            size_bytes: preflight.size.total_bytes,
            source_label: format!(
                "{}{}",
                local.as_ref().map(|l| format!("Local's \"{}\" site — ", l.site_name)).unwrap_or_default(),
                match &src_version {
                    Some(v) => format!("{} {} at {}:{}", source_vendor.label(), v, conn_info.host, conn_info.port),
                    None => format!("{}:{}", conn_info.host, conn_info.port),
                }
            ),
            mirrored_user: intended_user,
            // From the MANIFEST, not the local variable: the manifest is what
            // the artifact actually was, and a re-import from an existing
            // artifact settles this record without re-running the probe.
            skipped_tables: manifest.skipped_tables.clone(),
        };
        crate::state::store::upsert_db_import(&conn, &new)?
    };
    let mirror_outcome = dbmirror::mirror(
        &tgt_client,
        target_engine.port(),
        &name,
        &conn_info.user,
        &conn_info.password,
    )?;
    log_line(app, entry, &mirror_outcome.message(&name));
    // D5: the artifact is deleted when the job settles ok.
    let _ = std::fs::remove_file(&artifact);
    let _ = std::fs::remove_file(dbdump::manifest_path(&dest_dir, &site.domain));
    settle(entry, "ok", None, None, Some(record));
    log_line(app, entry, "imported — the site still reads its old database (see the summary)");
    Ok(())
}

/// The name the imported database gets on rexenv's engine: theirs, with every
/// character the engine cannot hold unquoted mapped to `_`; and when another
/// site already owns that name, suffixed with the site's domain slug — CAPPED
/// at the identifier budget with the FNV disambiguation `dedicated_user_name`
/// uses, because `<name>_<full domain>` ran past 64 characters and died as
/// MySQL error 1059 after the dump.
pub(crate) fn target_db_name(source: &str, disambiguate_with: Option<&str>) -> String {
    let clean = |s: &str| -> String {
        s.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect()
    };
    let base = clean(source);
    let base = if base.is_empty() { "imported".to_string() } else { base };
    let Some(domain) = disambiguate_with else { return base };
    let full = format!("{base}_{}", clean(domain));
    let max = crate::core::wordpress::DB_NAME_MAX;
    if full.len() <= max {
        return full;
    }
    let suffix = format!("{:08x}", crate::core::wordpress::fnv1a(full.as_bytes()));
    let keep = max - 1 - suffix.len();
    let head: String = full.chars().take(keep).collect();
    format!("{head}_{suffix}")
}

/// Ledger #572 — the Local registry replaces the config's host ONLY behind the
/// bare-`localhost` rule, and only in the one job that reads a source.
#[cfg(test)]
mod local_source_wiring {
    /// `overrides_config` is the rule; this pins that `run` actually asks it
    /// BEFORE consulting Local's registry, and asks the registry exactly once.
    /// A registry lookup outside that gate would let `sites.json` silently
    /// redirect a site whose config names an explicit server — a Local site
    /// someone pointed at DBngin would be dumped from the wrong database.
    #[test]
    fn the_registry_is_consulted_only_behind_the_localhost_rule() {
        let src = crate::core::copy_scan::production_source(include_str!("db_import.rs"));
        assert_eq!(src.matches("db_source_for(").count(), 1, "one registry lookup, in `run`");
        let gate = src.find("overrides_config(").expect("the rule is no longer asked");
        let lookup = src.find("db_source_for(").expect("lookup");
        assert!(gate < lookup, "the registry is read before the rule decides it may be");
        let head = &src[gate.saturating_sub(80)..gate];
        let between = &src[gate..lookup];
        assert!(
            head.contains(".filter(") && !between.contains(';'),
            "the rule must GATE the lookup in one expression, not merely precede it: {head}{between}"
        );
        // A stopped Local site is named as Local's, not DBngin's.
        assert!(src.contains("Start \\\"{}\\\" in Local"), "the Local-named unreachable message is gone");
    }
}

#[cfg(test)]
mod target_name_tests {
    use super::*;

    #[test]
    fn the_target_name_is_engine_safe_and_capped() {
        assert_eq!(target_db_name("my-site", None), "my_site");
        assert_eq!(target_db_name("acme.local", None), "acme_local");
        assert_eq!(target_db_name("wp", Some("acme.test")), "wp_acme_test");
        let long = target_db_name(&"d".repeat(40), Some(&format!("{}.test", "x".repeat(40))));
        assert!(long.len() <= crate::core::wordpress::DB_NAME_MAX, "{long}");
        assert!(crate::core::database::validate_db_name(&long).is_ok());
        // Two long names that truncate to the same head stay distinct.
        let a = target_db_name(&"d".repeat(40), Some(&format!("{}a.test", "x".repeat(40))));
        let b = target_db_name(&"d".repeat(40), Some(&format!("{}b.test", "x".repeat(40))));
        assert_ne!(a, b);
    }
}

/// Latest job state for a site (the SiteDetail card re-attaches on mount).
#[tauri::command]
pub fn db_import_state(
    jobs: State<'_, DbImportJobs>,
    site_id: String,
) -> Result<Option<DbImportJobState>> {
    let map = jobs.jobs.lock().expect("db import jobs lock");
    Ok(map.values().map(|e| snapshot(e)).find(|s| s.site_id == site_id))
}

/// Cancel the running import (checked between phases and inside dump/feed).
#[tauri::command]
pub fn db_import_cancel(jobs: State<'_, DbImportJobs>, id: String) -> Result<()> {
    if let Some(e) = jobs.jobs.lock().expect("db import jobs lock").get(&id) {
        e.cancel.store(true, Ordering::SeqCst);
    }
    Ok(())
}

/// The settled §9 record for a site — the ONE fact the badge and summary read.
#[tauri::command]
pub fn db_import_record(
    state: State<'_, AppState>,
    site_id: String,
) -> Result<Option<DbImportRecord>> {
    let conn = lock(&state)?;
    crate::state::store::get_db_import(&conn, &site_id)
}

/// All settled records (the Sites page badges, one query).
#[tauri::command]
pub fn db_import_records(state: State<'_, AppState>) -> Result<Vec<DbImportRecord>> {
    let conn = lock(&state)?;
    crate::state::store::list_db_imports(&conn)
}

/// A leftover dump on disk (kept by a failed import). Their data — visible and
/// removable, never a file someone finds later.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftoverDump {
    pub file: String,
    pub path: String,
    pub size_bytes: u64,
}

/// Enumerate kept artifacts (`<app-data>/db-imports/*.sql`).
#[tauri::command]
pub fn db_import_leftovers(state: State<'_, AppState>) -> Result<Vec<LeftoverDump>> {
    let dir = state.platform.paths().app_data_dir()?.join("db-imports");
    let Ok(entries) = std::fs::read_dir(&dir) else { return Ok(vec![]) };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        if !(name.ends_with(".sql") || name.ends_with(".sql.partial")) {
            continue;
        }
        let size = e.metadata().map(|m| m.len()).unwrap_or(0);
        out.push(LeftoverDump { file: name, path: p.display().to_string(), size_bytes: size });
    }
    out.sort_by(|a, b| a.file.cmp(&b.file));
    Ok(out)
}

/// Delete one leftover dump (and its manifest). Only inside our db-imports dir.
#[tauri::command]
pub fn db_import_delete_leftover(state: State<'_, AppState>, file: String) -> Result<()> {
    if file.contains('/') || file.contains("..") {
        return Err(Error::Other("not a dump file name".into()));
    }
    let dir = state.platform.paths().app_data_dir()?.join("db-imports");
    let path = dir.join(&file);
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    let manifest = dir.join(format!("{file}.manifest.json"));
    let _ = std::fs::remove_file(manifest);
    Ok(())
}

fn lock<'a>(state: &'a State<'a, AppState>) -> Result<std::sync::MutexGuard<'a, rusqlite::Connection>> {
    state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))
}

fn prune_logs(log_dir: &Path, prefix: &str) {
    let Ok(entries) = std::fs::read_dir(log_dir) else { return };
    let mut logs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(prefix) && n.ends_with(".log"))
        })
        .collect();
    logs.sort_by_key(|p| p.metadata().and_then(|m| m.modified()).ok());
    while logs.len() > 4 {
        let _ = std::fs::remove_file(logs.remove(0));
    }
}


/// #181 — the mutual exclusion between the three long per-site jobs.
#[cfg(test)]
mod one_long_job_per_site {
    /// The three jobs that own a site's database or config file while they run,
    /// each with its entry point(s) and the marker OTHERS check it by.
    ///
    /// They are mutually exclusive because they fight over the same things: a
    /// provision creates the database, an import DROPS and rebuilds it, a
    /// rewrite holds the site's config file and the imported-state row. Any two
    /// at once is a race whose loser leaves a half-built site, and the user sees
    /// a failure in whichever screen happened to be open.
    const JOBS: &[(&str, &[&str], &str)] = &[
        (
            "provision",
            &["pub(crate) fn start<R: tauri::Runtime>("],
            "busy_for(",
        ),
        ("db import", &["pub async fn db_import_start"], "db_import_active"),
        (
            "rewrite",
            &["pub async fn rewrite_apply(", "pub async fn rewrite_revert("],
            "rewrite_active",
        ),
    ];

    fn source(file: &str) -> String {
        crate::core::copy_scan::production_source(match file {
            "provision" => include_str!("site_provision.rs"),
            "db import" => include_str!("db_import.rs"),
            _ => include_str!("rewrite.rs"),
        })
    }

    /// **Every job refuses while EITHER of the other two is running for that
    /// site** — the whole 3×2 matrix, not the pairs somebody remembered.
    ///
    /// This is the guard-covers-claimed-surface family: the doc says "both
    /// directions covered", and the directions are hand-written in three files.
    /// Adding a fourth long job, or a second entry point to an existing one, is
    /// where a pair goes missing — and the missing pair is invisible until two
    /// users' worth of timing produces it.
    #[test]
    fn each_long_job_refuses_while_either_other_one_runs() {
        for (job, entries, _) in JOBS {
            let src = source(job);
            for entry in *entries {
                let body = src
                    .split(entry)
                    .nth(1)
                    .unwrap_or_else(|| panic!("`{job}`'s entry point `{entry}` is gone — if it \
                                               moved, move this guard with it"))
                    .split("\npub ")
                    .next()
                    .unwrap_or_default();
                for (other, _, marker) in JOBS {
                    if other == job {
                        continue;
                    }
                    assert!(
                        body.contains(marker),
                        "`{job}` ({entry}) never checks `{marker}` — it can start while a \
                         {other} job owns this site's database or config file, and the loser \
                         of that race leaves a half-built site"
                    );
                }
            }
        }
    }

    /// A job must also MARK itself, or the other two are checking a flag nobody
    /// sets — three guards that all pass and exclude nothing.
    #[test]
    fn a_job_that_checks_the_others_also_announces_itself() {
        for (job, entries, marker) in JOBS {
            let src = source(job);
            // The provision job's marker is a registry entry rather than a flag
            // (`busy_for` reads the jobs map), so its "announcement" is the
            // insert every start does.
            // The provision job's marker is a registry ENTRY rather than a
            // flag (`busy_for` reads the jobs map), so its announcement is the
            // insert every start does — `map.insert(`, on the locked map.
            let needle = if *job == "provision" { "map.insert(" } else { marker };
            assert!(
                src.contains(needle),
                "`{job}` never sets the marker the other jobs check (`{needle}`) — their \
                 guards then read a flag nobody writes, which passes and excludes nothing"
            );
            assert!(!entries.is_empty());
        }
    }
}

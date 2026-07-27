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
use crate::core::dbmirror::MirrorOutcome;
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
    jobs.jobs.lock().expect("db import jobs lock").insert(id, entry.clone());
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
    let conn_info = dbimport::read_connection(Path::new(&site.path)).map_err(|(reason, source)| {
        Error::Other(DbSiteStatus::NeedsAttention { reason, source }.message())
    })?;
    log_line(app, entry, &format!("source: {:?}", conn_info.info()));
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

    let defaults = dbdump::DefaultsFile::create(platform, &dest_dir, &conn_info)?;
    let size = match dbdump::preflight_live(&cleared, &src_client, &defaults, &conn_info.database)?
    {
        dbdump::LiveCheck::Ready(s) => s,
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
    let mut name = conn_info.database.clone();
    {
        let conn = lock(state)?;
        if let Some(other) = crate::state::store::site_with_db_name(&conn, &name)? {
            if other.id != site.id {
                let slug: String = site
                    .domain
                    .chars()
                    .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                    .collect();
                name = format!("{name}_{slug}");
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

    // ── settle: finish, mirror, record the ONE fact, drop the artifact ──────
    enter_phase(entry, 4);
    emit(app, entry);
    let mirror_outcome = dbmirror::mirror(
        &tgt_client,
        target_engine.port(),
        &name,
        &conn_info.user,
        &conn_info.password,
    )?;
    log_line(app, entry, &mirror_outcome.message(&name));
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
            source_label: match &src_version {
                Some(v) => format!("{} {} at {}:{}", source_vendor.label(), v, conn_info.host, conn_info.port),
                None => format!("{}:{}", conn_info.host, conn_info.port),
            },
            mirrored_user: match &mirror_outcome {
                MirrorOutcome::Mirrored { user } => Some(user.clone()),
                MirrorOutcome::RefusedReserved { .. } => None,
            },
        };
        crate::state::store::upsert_db_import(&conn, &new)?
    };
    // D5: the artifact is deleted when the job settles ok.
    let _ = std::fs::remove_file(&artifact);
    let _ = std::fs::remove_file(dbdump::manifest_path(&dest_dir, &site.domain));
    settle(entry, "ok", None, None, Some(record));
    log_line(app, entry, "imported — the site still reads its old database (see the summary)");
    Ok(())
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

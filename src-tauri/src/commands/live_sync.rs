//! Live ↔ local sync — the IPC edge of `docs/PLAN-wp-live-sync.md` (L2's store,
//! L6's pull job). Thin: a pairing is parsed and stored, a pull is one streamed
//! job around `core::live_sync::pull::pull_into`.
//!
//! The secret never crosses to the webview: `live_sync_pair` takes the pasted key
//! and answers with [`Pairing`] (no secret); every later call names the site, and
//! the key is read from the owner-only file (`core::live_sync::secrets`).

use crate::commands::wordpress::wp_tools;
use crate::core::db::DbEngine;
use crate::core::live_sync::{client::Client, pull, secrets, sign};
use crate::core::{self, service_manager, sites};
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::models::{Site, SiteType};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

pub use secrets::Pairing;

fn state_event(id: &str) -> String {
    format!("live-sync://state/{id}")
}

/// The running and settled pull jobs.
#[derive(Default)]
pub struct LiveSyncJobs {
    jobs: Mutex<HashMap<String, Arc<Entry>>>,
}

struct Entry {
    id: String,
    running: AtomicBool,
    state: Mutex<LiveSyncJobState>,
}

/// One pull, as the Live tab sees it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveSyncJobState {
    pub id: String,
    pub site_id: String,
    pub domain: String,
    /// "running" | "ok" | "failed".
    pub status: String,
    pub lines: Vec<String>,
    pub error: Option<String>,
    pub tables: usize,
    pub rows: u64,
    pub files: usize,
    pub refused_files: Vec<String>,
    pub backup_db: Option<String>,
}

fn snapshot(e: &Entry) -> LiveSyncJobState {
    e.state.lock().expect("live sync state lock").clone()
}

fn emit<R: tauri::Runtime>(app: &AppHandle<R>, e: &Entry) {
    let _ = app.emit(&state_event(&e.id), snapshot(e));
}

fn lock_db(state: &AppState) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>> {
    state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))
}

fn wordpress_site(state: &AppState, site_id: &str) -> Result<Site> {
    let conn = lock_db(state)?;
    let site = sites::get(&conn, site_id)?.ok_or_else(|| Error::Other(format!("no site {site_id}")))?;
    if site.site_type != SiteType::Wordpress {
        return Err(Error::Other(format!("{} is not a WordPress site — only a WordPress site can mirror a live one.", site.domain)));
    }
    Ok(site)
}

/// What `live_sync_pair` answers: the pairing, and what the site said about itself.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Paired {
    pub pairing: Pairing,
    pub wp: String,
    pub php: String,
    pub tables: usize,
    pub multisite: bool,
}

/// Build the client for `site_id` from its stored pairing.
fn client_for(state: &AppState, site_id: &str) -> Result<Client> {
    let (key, auth) = secrets::get(state.platform.as_ref(), site_id)?
        .ok_or_else(|| Error::Other("this site is not connected to a live site — paste its key first.".into()))?;
    let mut c = Client::new(key)?;
    if let Some((u, p)) = auth {
        c = c.with_basic_auth(&u, &p);
    }
    Ok(c)
}

/// `live_sync_pair` — paste a key: parse it, prove it against the site's
/// `/manifest`, THEN store it. A key that does not reach its site is not stored.
#[tauri::command]
pub async fn live_sync_pair(
    state: State<'_, AppState>,
    site_id: String,
    key: String,
    basic_auth_user: Option<String>,
    basic_auth_password: Option<String>,
) -> Result<Paired> {
    let site = wordpress_site(&state, &site_id)?;
    let key = sign::parse_key(&key)?;
    let auth = match (basic_auth_user.filter(|u| !u.trim().is_empty()), basic_auth_password) {
        (Some(u), p) => Some((u, p.unwrap_or_default())),
        _ => None,
    };
    let mut client = Client::new(key.clone())?;
    if let Some((u, p)) = &auth {
        client = client.with_basic_auth(u, p);
    }
    let m = client.manifest().await?;
    secrets::put(state.platform.as_ref(), &site.id, &key, auth)?;
    let pairing = secrets::pairing(state.platform.as_ref(), &site.id)?.expect("just stored");
    Ok(Paired { pairing, wp: m.wp, php: m.php, tables: m.tables.len(), multisite: m.multisite })
}

/// `live_sync_pairing` — the site's pairing, without its secret, or null.
#[tauri::command]
pub async fn live_sync_pairing(state: State<'_, AppState>, site_id: String) -> Result<Option<Pairing>> {
    secrets::pairing(state.platform.as_ref(), &site_id)
}

/// `live_sync_unpair` — forget the site's pairing (the live site's own key stays
/// valid until Disconnect there).
#[tauri::command]
pub async fn live_sync_unpair(state: State<'_, AppState>, site_id: String) -> Result<bool> {
    secrets::delete(state.platform.as_ref(), &site_id)
}

/// `live_sync_job` — one job's state.
#[tauri::command]
pub async fn live_sync_job(jobs: State<'_, LiveSyncJobs>, id: String) -> Result<LiveSyncJobState> {
    let map = jobs.jobs.lock().expect("live sync jobs lock");
    map.get(&id).map(|e| snapshot(e)).ok_or_else(|| Error::Other(format!("no live-sync job {id}")))
}

/// `live_sync_active` — the newest job for a site, running or settled.
#[tauri::command]
pub async fn live_sync_active(jobs: State<'_, LiveSyncJobs>, site_id: String) -> Result<Option<LiveSyncJobState>> {
    let map = jobs.jobs.lock().expect("live sync jobs lock");
    Ok(map.values().filter(|e| snapshot(e).site_id == site_id).map(|e| snapshot(e)).max_by_key(|s| s.id.clone()))
}

/// `live_sync_pull` — start a pull (live → this site). Returns the first snapshot;
/// progress streams on `live-sync://state/<id>`. One pull per site at a time.
#[tauri::command]
pub async fn live_sync_pull<R: tauri::Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    jobs: State<'_, LiveSyncJobs>,
    site_id: String,
    uploads_since: Option<i64>,
) -> Result<LiveSyncJobState> {
    let site = wordpress_site(&state, &site_id)?;
    if !site.provisioned {
        return Err(Error::Other(format!("{} has not finished setting up — Retry it first.", site.domain)));
    }
    let client = client_for(&state, &site.id)?;
    // A pull replaces the site's database: not beside an import or a provision of it.
    if let Ok(active) = state.db_import_active.lock() {
        if active.as_deref() == Some(site.domain.as_str()) {
            return Err(Error::Other(format!("a database import is running for {} — wait for it first.", site.domain)));
        }
    }
    let id = format!("{}-{}", chrono_like_now(), &uuid::Uuid::new_v4().simple().to_string()[..6]);
    let entry = Arc::new(Entry {
        id: id.clone(),
        running: AtomicBool::new(true),
        state: Mutex::new(LiveSyncJobState {
            id: id.clone(),
            site_id: site.id.clone(),
            domain: site.domain.clone(),
            status: "running".into(),
            lines: Vec::new(),
            error: None,
            tables: 0,
            rows: 0,
            files: 0,
            refused_files: Vec::new(),
            backup_db: None,
        }),
    });
    {
        let mut map = jobs.jobs.lock().expect("live sync jobs lock");
        if map.values().any(|e| e.running.load(Ordering::SeqCst) && snapshot(e).site_id == site.id) {
            return Err(Error::Other(format!("a pull is already running for {}.", site.domain)));
        }
        map.insert(id, entry.clone());
    }
    let worker = app.clone();
    tauri::async_runtime::spawn(async move {
        let outcome = run_pull(&worker, &entry, &site, client, uploads_since).await;
        {
            let mut st = entry.state.lock().expect("live sync state lock");
            match outcome {
                Ok(r) => {
                    st.status = "ok".into();
                    st.tables = r.tables;
                    st.rows = r.rows;
                    st.files = r.files;
                    st.refused_files = r.refused_files;
                    st.backup_db = Some(r.backup_db);
                }
                Err(e) => {
                    st.status = "failed".into();
                    st.error = Some(e.to_string());
                }
            }
        }
        entry.running.store(false, Ordering::SeqCst);
        emit(&worker, &entry);
    });
    let map = jobs.jobs.lock().expect("live sync jobs lock");
    Ok(map.values().find(|e| snapshot(e).site_id == site_id && e.running.load(Ordering::SeqCst)).map(|e| snapshot(e)).expect("just inserted"))
}

/// A sortable job id prefix (seconds since the epoch) — no chrono dependency.
fn chrono_like_now() -> String {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0).to_string()
}

async fn run_pull<R: tauri::Runtime>(
    app: &AppHandle<R>,
    entry: &Arc<Entry>,
    site: &Site,
    client: Client,
    uploads_since: Option<i64>,
) -> Result<pull::PullReport> {
    let state = app.state::<AppState>();
    let engine = DbEngine::from_site(site.db_engine);
    let minor = core::php::minor_of(&site.php_version);
    let (php, wp_phar) = wp_tools(&state, &minor).await?;
    let engine_version = crate::commands::database::effective_db_version(&state, engine)?;
    let check = {
        let mut mgr = state.services.lock().await;
        mgr.spawn_db(state.platform.as_ref(), engine).await?
    };
    service_manager::await_ready(check.into_iter().collect()).await?;
    let (db_client, _) = engine.sql_client_bins(state.platform.as_ref(), &engine_version).await?;
    let scratch = state.platform.paths().app_data_dir()?.join("live-sync").join("scratch");
    let docroot = site.served_root();
    let (a2, e2) = (app.clone(), entry.clone());
    let mut on_line = move |l: &str| {
        e2.state.lock().expect("live sync state lock").lines.push(l.to_string());
        emit(&a2, &e2);
    };
    let local = pull::LocalSite {
        platform: state.platform.as_ref(),
        engine,
        client: &db_client,
        port: engine.port(),
        db_name: &site.db_name,
        domain: &site.domain,
        docroot: &docroot,
        php: &php,
        wp_phar: &wp_phar,
        scratch: &scratch,
    };
    let options = pull::PullOptions { excludes: Vec::new(), uploads_since };
    pull::pull_into(&client, &local, &options, &mut on_line).await
}

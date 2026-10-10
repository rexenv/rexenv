//! Live ↔ local sync — the IPC edge of `docs/PLAN-wp-live-sync.md` (L2's store,
//! L6's pull job, L10's push job). Thin: a pairing is parsed and stored, a pull
//! is one streamed job around `core::live_sync::pull::pull_into`, a push one
//! around `core::live_sync::push::push_from`.
//!
//! A push is the only thing in rexenv that can change something OUTSIDE this
//! machine, so its two gates live HERE, in Rust, not in the button (ledger #834):
//! the caller types the live host and `live_sync_push` compares it; and a site
//! that was never pulled cannot push (no base → the plugin has nothing to judge
//! conflicts against). Neither push nor rollback is reachable from the MCP
//! server or the CLI without that typed host (plan §2.9, owner 9 Oct 2026).
//!
//! The secret never crosses to the webview: `live_sync_pair` takes the pasted key
//! and answers with [`Pairing`] (no secret); every later call names the site, and
//! the key is read from the owner-only file (`core::live_sync::secrets`).

use crate::commands::wordpress::wp_tools;
use crate::core::db::DbEngine;
use crate::core::live_sync::{base, client::Client, pull, push, secrets, sign};
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

/// The running and settled pull and push jobs.
#[derive(Default)]
pub struct LiveSyncJobs {
    jobs: Mutex<HashMap<String, Arc<Entry>>>,
}

struct Entry {
    id: String,
    running: AtomicBool,
    state: Mutex<LiveSyncJobState>,
}

/// One pull or push, as the Live tab sees it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveSyncJobState {
    pub id: String,
    pub site_id: String,
    pub domain: String,
    /// "pull" | "push".
    pub kind: String,
    /// "running" | "ok" | "failed" | "conflicts" (a push that sent nothing: live
    /// changed since the base — `conflicts` names what).
    pub status: String,
    pub lines: Vec<String>,
    pub error: Option<String>,
    pub tables: usize,
    pub rows: u64,
    pub files: usize,
    pub refused_files: Vec<String>,
    pub backup_db: Option<String>,
    /// A push's backup id on LIVE — what Roll back names.
    pub backup_id: Option<String>,
    pub conflicts: Vec<String>,
    /// What a push was asked to send — so "Push anyway" after a conflict repeats
    /// exactly that with overrides, even after the app was reopened.
    pub asked_tables: Option<Vec<String>>,
    pub asked_files: bool,
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
    let job_site = site.clone();
    start_job(&app, &jobs, &site, "pull", move |worker, entry| async move {
        let outcome = run_pull(&worker, &entry, &job_site, client, uploads_since).await;
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
    })
}

/// Register a job for `site`, spawn `work` on it, answer the first snapshot.
/// One job per site at a time, pull or push.
fn start_job<R, F, Fut>(app: &AppHandle<R>, jobs: &LiveSyncJobs, site: &Site, kind: &str, work: F) -> Result<LiveSyncJobState>
where
    R: tauri::Runtime,
    F: FnOnce(AppHandle<R>, Arc<Entry>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let id = format!("{}-{}", chrono_like_now(), &uuid::Uuid::new_v4().simple().to_string()[..6]);
    let entry = Arc::new(Entry {
        id: id.clone(),
        running: AtomicBool::new(true),
        state: Mutex::new(LiveSyncJobState {
            id: id.clone(),
            site_id: site.id.clone(),
            domain: site.domain.clone(),
            kind: kind.into(),
            status: "running".into(),
            lines: Vec::new(),
            error: None,
            tables: 0,
            rows: 0,
            files: 0,
            refused_files: Vec::new(),
            backup_db: None,
            backup_id: None,
            conflicts: Vec::new(),
            asked_tables: None,
            asked_files: false,
        }),
    });
    {
        let mut map = jobs.jobs.lock().expect("live sync jobs lock");
        if let Some(e) = map.values().find(|e| e.running.load(Ordering::SeqCst) && snapshot(e).site_id == site.id) {
            return Err(Error::Other(format!("a {} is already running for {}.", snapshot(e).kind, site.domain)));
        }
        map.insert(id, entry.clone());
    }
    let worker = app.clone();
    let first = snapshot(&entry);
    tauri::async_runtime::spawn(async move {
        work(worker.clone(), entry.clone()).await;
        entry.running.store(false, Ordering::SeqCst);
        emit(&worker, &entry);
    });
    Ok(first)
}

/// A sortable job id prefix (seconds since the epoch) — no chrono dependency.
fn chrono_like_now() -> String {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0).to_string()
}

/// Everything a pull or push needs from this machine: the site's PHP + wp-cli,
/// its database engine up, the SQL client binaries. Owned, so a `LocalSite`
/// can borrow from it.
struct LocalTools {
    engine: DbEngine,
    php: std::path::PathBuf,
    wp_phar: std::path::PathBuf,
    db_client: crate::core::db::SqlClient,
    dump: std::path::PathBuf,
    scratch: std::path::PathBuf,
    docroot: std::path::PathBuf,
}

impl LocalTools {
    async fn for_site(state: &State<'_, AppState>, site: &Site) -> Result<Self> {
        let engine = DbEngine::from_site(site.db_engine);
        let minor = core::php::minor_of(&site.php_version);
        let (php, wp_phar) = wp_tools(state, &minor).await?;
        let engine_version = crate::commands::database::effective_db_version(state, engine)?;
        let check = {
            let mut mgr = state.services.lock().await;
            mgr.spawn_db(state.platform.as_ref(), engine).await?
        };
        service_manager::await_ready(check.into_iter().collect()).await?;
        let (db_client, dump) = engine.sql_client_bins(state.platform.as_ref(), &engine_version).await?;
        let scratch = state.platform.paths().app_data_dir()?.join("live-sync").join("scratch");
        Ok(Self { engine, php, wp_phar, db_client, dump, scratch, docroot: site.served_root() })
    }

    fn local<'a>(&'a self, state: &'a AppState, site: &'a Site) -> pull::LocalSite<'a> {
        pull::LocalSite {
            platform: state.platform.as_ref(),
            engine: self.engine,
            client: &self.db_client,
            dump: self.dump.clone(),
            port: self.engine.port(),
            db_name: &site.db_name,
            domain: &site.domain,
            docroot: &self.docroot,
            php: &self.php,
            wp_phar: &self.wp_phar,
            scratch: &self.scratch,
        }
    }
}

fn line_sink<R: tauri::Runtime>(app: &AppHandle<R>, entry: &Arc<Entry>) -> impl FnMut(&str) + Send {
    let (a2, e2) = (app.clone(), entry.clone());
    move |l: &str| {
        e2.state.lock().expect("live sync state lock").lines.push(l.to_string());
        emit(&a2, &e2);
    }
}

async fn run_pull<R: tauri::Runtime>(
    app: &AppHandle<R>,
    entry: &Arc<Entry>,
    site: &Site,
    client: Client,
    uploads_since: Option<i64>,
) -> Result<pull::PullReport> {
    let state = app.state::<AppState>();
    let tools = LocalTools::for_site(&state, site).await?;
    let local = tools.local(&state, site);
    let mut on_line = line_sink(app, entry);
    let options = pull::PullOptions { excludes: Vec::new(), uploads_since };
    let report = pull::pull_into(&client, &local, &options, &mut on_line).await?;
    // The base for the next push (§2.6).
    base::put(state.platform.as_ref(), &site.id, &report.base)?;
    Ok(report)
}

// ── push ─────────────────────────────────────────────────────────────────

/// One table the push picker offers.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushTable {
    pub name: String,
    /// Live writes here (users, comments, orders, form entries): unticked by default.
    pub live_owned: bool,
}

/// What `live_sync_push_plan` answers: the picker's rows and the gates' facts.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushPlan {
    /// The host the person must type to push.
    pub live_host: String,
    pub tables: Vec<PushTable>,
    /// Unix seconds of the last pull/push, or null when the site was never synced.
    pub base_at: Option<i64>,
}

/// The rule, in one place: what the person typed must be the live host, exactly
/// (case-folded, trimmed — a scheme or a path is not a host).
pub fn typed_host_matches(typed: &str, live_host: &str) -> bool {
    let t = typed.trim();
    !t.is_empty() && t.eq_ignore_ascii_case(live_host)
}

/// `live_sync_push_plan` — the LOCAL site's tables with the live-owned ones
/// marked, the live host, and whether a base exists. Reaches the live site
/// (its manifest gives the prefix), so a dead pairing fails here, before a picker.
#[tauri::command]
pub async fn live_sync_push_plan(state: State<'_, AppState>, site_id: String) -> Result<PushPlan> {
    let site = wordpress_site(&state, &site_id)?;
    let client = client_for(&state, &site.id)?;
    let m = client.manifest().await?;
    let tools = LocalTools::for_site(&state, &site).await?;
    let names = tools.engine.list_tables(&tools.db_client, tools.engine.port(), &site.db_name)?;
    let tables = names.into_iter().map(|name| PushTable { live_owned: push::live_owned(&m.prefix, &name), name }).collect();
    Ok(PushPlan { live_host: pull::host_of(&m.site_url), tables, base_at: base::get(state.platform.as_ref(), &site.id)?.map(|b| b.at) })
}

/// `live_sync_push` — push this site TO live. `confirm_host` must be the live
/// host, typed; `tables` = the picked full names (null = every table but the
/// live-owned); `override_items` = conflicts the person chose to overwrite. A
/// conflict stop settles the job as `conflicts` with nothing sent.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn live_sync_push<R: tauri::Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    jobs: State<'_, LiveSyncJobs>,
    site_id: String,
    confirm_host: String,
    tables: Option<Vec<String>>,
    files: bool,
    override_items: Vec<String>,
) -> Result<LiveSyncJobState> {
    let site = wordpress_site(&state, &site_id)?;
    if !site.provisioned {
        return Err(Error::Other(format!("{} has not finished setting up — Retry it first.", site.domain)));
    }
    let pairing = secrets::pairing(state.platform.as_ref(), &site.id)?
        .ok_or_else(|| Error::Other("this site is not connected to a live site — paste its key first.".into()))?;
    let live_host = pull::host_of(&pairing.site_url);
    if !typed_host_matches(&confirm_host, &live_host) {
        return Err(Error::Other(format!("To push, type the live site's host exactly: {live_host}")));
    }
    let prior = base::get(state.platform.as_ref(), &site.id)?
        .ok_or_else(|| Error::Other(format!("{} has never been pulled from {live_host} — pull once first, so rexenv knows what live looked like and can tell what changed there since.", site.domain)))?;
    let client = client_for(&state, &site.id)?;
    if tables.as_ref().is_some_and(|t| t.is_empty()) && !files {
        return Err(Error::Other("nothing picked — choose at least one table, or the changed files.".into()));
    }
    let options = push::PushOptions { tables, files, override_items };
    let job_site = site.clone();
    let asked = (options.tables.clone(), options.files);
    start_job(&app, &jobs, &site, "push", move |worker, entry| async move {
        {
            let mut st = entry.state.lock().expect("live sync state lock");
            st.asked_tables = asked.0;
            st.asked_files = asked.1;
        }
        let outcome = run_push(&worker, &entry, &job_site, client, &prior, &options).await;
        let mut st = entry.state.lock().expect("live sync state lock");
        match outcome {
            Ok(r) => {
                st.status = "ok".into();
                st.tables = r.tables.len();
                st.files = r.files;
                st.backup_id = Some(r.backup_id);
            }
            Err(push::PushStop::Conflicts(list)) => {
                st.status = "conflicts".into();
                st.conflicts = list;
            }
            Err(push::PushStop::Failed(e)) => {
                st.status = "failed".into();
                st.error = Some(e.to_string());
            }
        }
    })
}

async fn run_push<R: tauri::Runtime>(
    app: &AppHandle<R>,
    entry: &Arc<Entry>,
    site: &Site,
    client: Client,
    prior: &base::SyncBase,
    options: &push::PushOptions,
) -> std::result::Result<push::PushReport, push::PushStop> {
    let state = app.state::<AppState>();
    let tools = LocalTools::for_site(&state, site).await?;
    let local = tools.local(&state, site);
    let mut on_line = line_sink(app, entry);
    let report = push::push_from(&client, &local, Some(prior), options, &mut on_line).await?;
    base::put(state.platform.as_ref(), &site.id, &report.base)?;
    Ok(report)
}

/// `live_sync_rollback` — put the live site back to `backup_id` (the tables and
/// files of the push that made it). The base is re-read from live afterwards,
/// so the next push judges conflicts against what live now holds.
#[tauri::command]
pub async fn live_sync_rollback(state: State<'_, AppState>, site_id: String, backup_id: String) -> Result<()> {
    let site = wordpress_site(&state, &site_id)?;
    let client = client_for(&state, &site.id)?;
    client.push_rollback(&backup_id).await?;
    if let Some(mut b) = base::get(state.platform.as_ref(), &site.id)? {
        let m = client.manifest().await?;
        b.tables = m.tables.iter().map(|t| (t.name.clone(), t.checksum.clone())).collect();
        b.files.clear();
        for f in client.list_files(&pull::DEFAULT_EXCLUDES, None).await? {
            b.files.insert(f.path, format!("{}:{}", f.size, f.mtime));
        }
        b.at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        base::put(state.platform.as_ref(), &site.id, &b)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::typed_host_matches;

    /// **The typed host must be the live host, exactly** (ledger #834): a scheme,
    /// a path, a different host or nothing never pass; case and whitespace do.
    #[test]
    fn typed_host_is_the_gate() {
        assert!(typed_host_matches("example.com", "example.com"));
        assert!(typed_host_matches("  Example.COM ", "example.com"));
        for bad in ["", "https://example.com", "example.com/", "example.co", "www.example.com", "example.com.rex"] {
            assert!(!typed_host_matches(bad, "example.com"), "{bad:?}");
        }
    }
}

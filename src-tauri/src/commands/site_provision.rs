//! Streamed site provisioning — the "New Site" card's job backend. Mirrors
//! `commands::wp_install` (registry + `site-provision://state|output/<id>`
//! events + per-job log + CancelToken), driving the REAL create sequence with
//! phase-weighted honest progress (`core::sites::ProvisionProgress`).
//!
//! Honesty contract:
//! - phases are OUR step boundaries (deterministic Rust code) — zero
//!   subprocess-output parsing; wp-cli's verbatim lines stream to the log and
//!   the card's sub-detail only,
//! - the only intra-phase progress is the download Hub's REAL byte fraction
//!   during `fetch` (genuine bytes, never an estimate),
//! - pct is monotonic, ≤99 until the job settles ok, frozen on
//!   failure/cancel/timeout,
//! - CANCEL never touches anything shared: the binary prefetch runs in a
//!   DETACHED task (a Hub download another consumer may be waiting on is
//!   never aborted — cancelling a create just stops WAITING; the fetch
//!   completes into the cache), the pgid kill only ever hits wp-cli children
//!   this job spawned, and the DB-spawn/pool/reload phases (shared services)
//!   are cancel-checked at boundaries only,
//! - a job that dies mid-provision leaves the site row with `provisioned=0`
//!   (the honest "setup incomplete" badge); `site_provision_retry` re-enters
//!   the idempotent steps and re-ENSURES prepare's artifacts (docroot, cert)
//!   rather than assuming they exist.

use crate::commands::repo::{shell_env, EnvSnapshot, RepoJobs};
use crate::commands::wordpress::{composer_tools, wp_tools};
use crate::core::db::DbEngine;
use crate::core::{self, blueprints, database, downloads, repo, service_manager, sites, wordpress};
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::models::{Blueprint, MultisiteMode, NewSite, Site, SiteType, WebServer};
use serde::Serialize;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

pub fn state_event(id: &str) -> String {
    format!("site-provision://state/{id}")
}
pub fn output_event(id: &str) -> String {
    format!("site-provision://output/{id}")
}

#[derive(Default)]
pub struct ProvisionJobs {
    jobs: Mutex<HashMap<String, Arc<ProvisionEntry>>>,
    /// Creation order — "the most recent job" must mean MOST RECENT.
    next_seq: AtomicU64,
}

impl ProvisionJobs {
    /// Is a provision job currently RUNNING for `domain`? The database import
    /// consults this before starting: the two jobs would otherwise race on the
    /// same site's database and edge reload.
    pub(crate) fn busy_for(&self, domain: &str) -> bool {
        self.jobs
            .lock()
            .expect("provision jobs lock")
            .values()
            .any(|e| e.domain == domain && e.running.load(Ordering::SeqCst))
    }
}

struct ProvisionEntry {
    id: String,
    seq: u64,
    domain: String,
    /// Install options are held HERE, never in the serialized state — they
    /// carry the admin password.
    wp_opts: wordpress::InstallOptions,
    blueprint: Option<Blueprint>,
    cancel: repo::CancelToken,
    running: AtomicBool,
    /// Set by a step's outer-cap timer BEFORE it cancels (the token can't
    /// tell user-cancel from timeout apart — the reason is pre-recorded).
    timed_out: AtomicBool,
    log_path: PathBuf,
    state: Mutex<SiteProvisionState>,
}

/// One phase of the job as the card sees it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseState {
    pub key: String,
    pub label: String,
    /// "pending" | "running" | "ok" | "skipped" | "failed" | "cancelled".
    pub status: String,
}

/// The card's whole truth. `pct` follows the `ProvisionProgress` contract
/// (monotonic, ≤99 until settle-ok, frozen on any early end).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteProvisionState {
    pub id: String,
    pub domain: String,
    pub site_id: Option<String>,
    pub phases: Vec<PhaseState>,
    pub phase_cursor: usize,
    pub pct: u8,
    /// "running" | "ok" | "failed" | "cancelled" | "timed_out".
    pub status: String,
    pub summary: Option<String>,
    pub error: Option<String>,
    pub log_key: String,
    /// Uncached Hub item ids this job's fetch phase waits on — the card
    /// filters the app-wide `download-progress` snapshot to these.
    pub download_ids: Vec<String>,
    /// The site was created, but something else is answering :443 in front of
    /// our edge (a running Herd shadow-binds 127.0.0.1:443), so it will not
    /// actually load yet.
    ///
    /// A FIELD rather than a `status` value on purpose: `status` is a closed
    /// set the card reads as ok-or-failure, so a new variant would render the
    /// failure glyph and freeze the bar on a job that genuinely succeeded.
    #[serde(default)]
    pub serving_blocked: bool,
}

fn snapshot(entry: &ProvisionEntry) -> SiteProvisionState {
    entry.state.lock().expect("provision state lock").clone()
}

fn emit_state<R: tauri::Runtime>(app: &AppHandle<R>, entry: &ProvisionEntry) {
    let _ = app.emit(&state_event(&entry.id), snapshot(entry));
}

/// Append one line to the job log + stream it to the card.
fn append_line<R: tauri::Runtime>(app: &AppHandle<R>, entry: &ProvisionEntry, line: &str) {
    if let Ok(mut f) =
        std::fs::OpenOptions::new().create(true).append(true).open(&entry.log_path)
    {
        let _ = writeln!(f, "{line}");
    }
    let _ = app.emit(&output_event(&entry.id), line.to_string());
}


/// May a blueprint be applied to the site about to be created?
///
/// A blueprint is a WORDPRESS preset — plugins, themes, multisite mode,
/// WP_DEBUG, language — and [`phase_defs`] gives the blueprint phase only to a
/// MANAGED WordPress site. Every other combination used to be ACCEPTED and then
/// silently do nothing, which is the dishonest half: the caller believed the
/// preset applied. The dialog no longer offers the field outside that case, and
/// this is the same rule for callers that never see a dialog (CLI, MCP).
///
/// `path` is the linked-folder path from `NewSite`: non-empty means "serve this
/// folder in place", which rexenv adopts as-is and installs nothing into.
fn ensure_blueprint_applies(
    site_type: SiteType,
    path: &str,
    blueprint_id: Option<&str>,
) -> Result<()> {
    if !blueprint_id.is_some_and(|id| !id.is_empty()) {
        return Ok(());
    }
    if !matches!(site_type, SiteType::Wordpress) {
        return Err(Error::Other(format!(
            "blueprints apply to WordPress sites only — this is a {} site",
            site_type.as_db()
        )));
    }
    if !path.trim().is_empty() {
        return Err(Error::Other(
            "this site serves an existing folder, which rexenv adopts as-is — a blueprint would \
             install into someone else's project"
                .into(),
        ));
    }
    Ok(())
}

/// The job's applicable phases, in execution order. Weights renormalize over
/// exactly this set (`ProvisionProgress`) — no reserved slice can fail to fill.
///
/// A LINKED site gets prepare/fetch/serve whatever its type: we adopt the
/// folder exactly as it is and install nothing into it. That is not merely
/// polite — `configure` is only half idempotent (it skips `wp config create`
/// when wp-config.php exists, but creates the database unconditionally), so
/// running the WordPress phases over someone's existing install would leave a
/// stray empty database beside their real one.
fn phase_defs(
    site_type: SiteType,
    has_blueprint: bool,
    linked: bool,
) -> Vec<(&'static str, &'static str)> {
    let mut v = vec![("prepare", "preparing site (domain, certificate)"), ("fetch", "downloading binaries")];
    if matches!(site_type, SiteType::Wordpress) && !linked {
        v.push(("db", "starting database"));
        v.push(("core_download", "downloading WordPress core"));
        v.push(("configure", "writing wp-config + creating database"));
        v.push(("core_install", "installing WordPress"));
        if has_blueprint {
            v.push(("blueprint", "applying blueprint"));
        }
    }
    // Laravel: the same shape one type over. The app comes from Composer rather
    // than a core zip, and `configure` both creates the database and points the
    // skeleton's `.env` at it — a fresh Laravel defaults to SQLite, so without
    // that step the site would run on a file the Databases screen never shows.
    if matches!(site_type, SiteType::Laravel) && !linked {
        v.push(("db", "starting database"));
        // Labels stay in the same size class as the WordPress ones: they render
        // on ONE row beside the domain and the buttons, so a long one is a
        // layout problem, not just wordy. The full commands stream to the log.
        v.push(("app_install", "installing Laravel"));
        v.push(("configure", "creating database + .env"));
    }
    v.push(("serve", "starting to serve"));
    v
}

/// Build the create plan exactly the way `create_site` always has (the split
/// mirrors `commands/sites.rs` — pool vs override server, engine + wp tooling
/// only for WordPress).
fn build_plan(
    state: &AppState,
    site: &Site,
    minor: &str,
    engine: DbEngine,
    engine_version: &str,
) -> Vec<downloads::PlannedBinary> {
    let mut plan = if matches!(site.web_server, WebServer::Frankenphp) {
        downloads::plan_for_override(state.platform.as_ref(), site.web_server)
    } else {
        downloads::plan_for_pool(state.platform.as_ref(), minor)
    };
    if matches!(site.web_server, WebServer::Apache) {
        plan.extend(downloads::plan_for_override(state.platform.as_ref(), site.web_server));
    }
    // A LINKED WordPress site is adopted, never installed into, so `phase_defs`
    // omits its db/configure/core_install phases entirely — fetching the engine
    // and wp-cli for them would download ~600 MB of MySQL that this job will
    // never touch. On a dozen imported Valet sites that is the difference
    // between a fast import and a long first-run download.
    let linked = site.docroot_managed == Some(false);
    if matches!(site.site_type, SiteType::Wordpress) && !linked {
        plan.extend(downloads::plan_for_engine(state.platform.as_ref(), engine, engine_version));
        plan.extend(downloads::plan_for_wp_tooling(state.platform.as_ref(), minor));
    }
    // Laravel needs the same database engine, and Composer + the PHP CLI to run
    // it — the phar is executed by the SITE's PHP so `create-project`'s platform
    // checks are made against the PHP the app will actually run on.
    if matches!(site.site_type, SiteType::Laravel) && !linked {
        plan.extend(downloads::plan_for_engine(state.platform.as_ref(), engine, engine_version));
        plan.extend(downloads::plan_for_laravel_tooling(state.platform.as_ref(), minor));
    }
    plan
}

fn lock_db(state: &AppState) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>> {
    state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))
}

/// Keep only the newest `keep` logs with `prefix` (per-job files so a failed
/// provision's log survives later attempts).
fn prune_logs(log_dir: &Path, prefix: &str, keep: usize) {
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
    while logs.len() > keep {
        let _ = std::fs::remove_file(logs.remove(0));
    }
}

/// Shared start: register the entry + spawn the worker. `site` must already
/// exist (row inserted by `start_job`, or the retried half-site).
#[allow(clippy::too_many_arguments)]
fn spawn_job<R: tauri::Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    jobs: &ProvisionJobs,
    site: Site,
    wp_opts: wordpress::InstallOptions,
    blueprint: Option<Blueprint>,
    plan: Vec<downloads::PlannedBinary>,
) -> Result<SiteProvisionState> {
    let id = uuid::Uuid::new_v4().to_string();
    let log_dir = state.platform.paths().log_dir()?;
    let log_prefix = format!("site-provision-{}-", site.domain);
    prune_logs(&log_dir, &log_prefix, 4);
    let log_key = format!("site-provision-{}-{}.log", site.domain, &id[..8]);
    let log_path = log_dir.join(&log_key);

    let defs =
        phase_defs(site.site_type, blueprint.is_some(), site.docroot_managed == Some(false));
    let mut phases: Vec<PhaseState> = defs
        .iter()
        .map(|(k, l)| PhaseState { key: (*k).into(), label: (*l).into(), status: "pending".into() })
        .collect();
    // Prepare ran inline (row/docroot/cert exist before any job is visible).
    phases[0].status = "ok".into();
    let download_ids: Vec<String> = plan
        .iter()
        .filter(|p| !p.cached)
        .map(|p| downloads::item_id(&p.name, &p.version))
        .collect();

    let entry = Arc::new(ProvisionEntry {
        id: id.clone(),
        seq: jobs.next_seq.fetch_add(1, Ordering::SeqCst),
        domain: site.domain.clone(),
        wp_opts,
        blueprint,
        cancel: repo::CancelToken::new(),
        running: AtomicBool::new(true),
        timed_out: AtomicBool::new(false),
        log_path,
        state: Mutex::new(SiteProvisionState {
            id: id.clone(),
            domain: site.domain.clone(),
            site_id: Some(site.id.clone()),
            phases,
            phase_cursor: 1, // prepare done, fetch is next
            pct: 0,
            status: "running".into(),
            summary: None,
            error: None,
            log_key,
            download_ids,
            serving_blocked: false,
        }),
    });
    {
        let mut map = jobs.jobs.lock().expect("provision jobs lock");
        let busy = map
            .values()
            .any(|e| e.domain == entry.domain && e.running.load(Ordering::SeqCst));
        if busy {
            return Err(Error::Other(format!(
                "a provision job is already running for {} — wait for it (or cancel it) first.",
                entry.domain
            )));
        }
        map.insert(id, entry.clone());
    }
    let _ = std::fs::write(&entry.log_path, "");
    append_line(app, &entry, &format!("── preparing site ({}) — done", entry.domain));

    let worker_app = app.clone();
    let worker_entry = entry.clone();
    tauri::async_runtime::spawn(async move {
        run_provision_job(worker_app, worker_entry, site, plan).await;
    });
    Ok(snapshot(&entry))
}

/// Shared start (the command + the `create_site` compat wrapper): runs the
/// PREPARE phase INLINE — resolver, docroot, cert, row insert — so validation
/// errors (bad domain, duplicate, declined privileged prompt) surface as
/// command errors with nothing half created, exactly like the old
/// `create_site` (the resolver runs BEFORE provisioning for the same reason).
pub(crate) fn start<R: tauri::Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    jobs: &ProvisionJobs,
    site: NewSite,
    wp: Option<wordpress::InstallOptions>,
    blueprint_id: Option<String>,
    ownership: core::sites::Ownership,
) -> Result<SiteProvisionState> {
    // The mirror of ProvisionJobs::busy_for: a database import mid-run for this
    // domain owns the site's database (it may be mid-DROP on a retry) — a
    // provision alongside it would race that.
    if let Ok(active) = state.db_import_active.lock() {
        if active.as_deref() == Some(site.domain.as_str()) {
            return Err(Error::Other(format!(
                "a database import is running for {} — wait for it (or cancel it) first.",
                site.domain
            )));
        }
    }
    // Same shape for a connection rewrite mid-apply/revert: it holds the
    // site's config file and the imported-state row.
    if let Ok(active) = state.rewrite_active.lock() {
        if active.as_deref() == Some(site.domain.as_str()) {
            return Err(Error::Other(format!(
                "a connection rewrite is running for {} — wait for it to finish first.",
                site.domain
            )));
        }
    }
    // Step 7: a tunnel EXPOSES rather than mutates — provisioning under a
    // live link publishes a half-built site to whoever holds it. RETRY is the
    // live case (the site exists and can be shared); on a fresh create this
    // is a belt (no site row yet ⇒ no tunnel).
    if let Some(tunnels) = app.try_state::<crate::commands::tunnels::Tunnels>() {
        crate::commands::tunnels::refuse_if_shared(
            &tunnels,
            state,
            &site.domain,
            "provisioning would rebuild it under the live link, publishing a half-built site",
        )?;
    }
    let site_tld = sites::domain_tld(&site.domain)?;
    // The prompt policy is DERIVED from the same ownership value that gets
    // recorded on the row — never passed separately. An agent-created site
    // therefore cannot raise a macOS authorization dialog, and that stays true
    // for the WHOLE operation rather than at an entry check: this is the only
    // prompting call the create path makes, `ensure_resolver`'s policy argument
    // is REQUIRED so a future one cannot be silent about it, and the job phases
    // downstream reach no privileged op (proven by
    // `an_agent_create_never_reaches_a_privileged_prompt`).
    core::dns::ensure_resolver(
        state.platform.as_ref(),
        &site_tld,
        core::dns::DEFAULT_DNS_PORT,
        ownership.resolver_prompt(),
    )?;

    // Refused HERE, before the row exists, so the caller gets a prepare-phase
    // error with nothing created rather than a half-site to clean up.
    ensure_blueprint_applies(site.site_type, &site.path, blueprint_id.as_deref())?;

    let (created, blueprint) = {
        let conn = lock_db(state)?;
        let created =
            sites::provision_with(&conn, state.platform.as_ref(), &state.ca, site, ownership)?;
        // The job owns this row's lifecycle from here: 0 until settle-ok. A
        // crash between insert and this write leaves 1 — yesterday's
        // semantics, never a false alarm.
        crate::state::store::set_site_provisioned(&conn, &created.id, false)?;
        let blueprint = match &blueprint_id {
            Some(id) if !id.is_empty() => crate::state::store::get_blueprint(&conn, id)?,
            _ => None,
        };
        (created, blueprint)
    };
    let minor = core::php::minor_of(&created.php_version);
    let engine = DbEngine::from_site(created.db_engine);
    let engine_version = super::database::effective_db_version(state, engine)?;
    let plan = build_plan(state, &created, &minor, engine, &engine_version);
    spawn_job(app, state, jobs, created, wp.unwrap_or_default(), blueprint, plan)
}

/// Start a streamed create job. Returns the initial snapshot; progress
/// arrives via `site-provision://state|output/<id>` events.
#[tauri::command]
pub async fn site_provision_job<R: tauri::Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    jobs: State<'_, ProvisionJobs>,
    site: NewSite,
    wp: Option<wordpress::InstallOptions>,
    blueprint_id: Option<String>,
) -> Result<SiteProvisionState> {
    // The IPC create command IS the user's own action.
    start(&app, &state, &jobs, site, wp, blueprint_id, core::sites::Ownership::User)
}

/// Re-enter provisioning for a `provisioned = 0` half-site (the "setup
/// incomplete" badge's Retry). Prepare's artifacts are re-ENSURED, not
/// assumed: docroot `create_dir_all`, the Blank-PHP `index.php` only if
/// missing (never clobbers user files), `ensure_site_cert` (early-returns
/// when both files exist) — a half-site missing any of them (failure landed
/// mid-prepare, user deleted files) gets them back before the idempotent
/// install steps run again.
#[tauri::command]
pub async fn site_provision_retry<R: tauri::Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    jobs: State<'_, ProvisionJobs>,
    site_id: String,
) -> Result<SiteProvisionState> {
    let site = {
        let conn = lock_db(&state)?;
        sites::get(&conn, &site_id)?.ok_or_else(|| Error::Other(format!("no site {site_id}")))?
    };
    if site.provisioned {
        return Err(Error::Other(format!(
            "{} is already fully provisioned — nothing to retry.",
            site.domain
        )));
    }
    // The mirror of ProvisionJobs::busy_for: a database import mid-run for this
    // domain owns the site's database (it may be mid-DROP on a retry) — a
    // provision alongside it would race that.
    if let Ok(active) = state.db_import_active.lock() {
        if active.as_deref() == Some(site.domain.as_str()) {
            return Err(Error::Other(format!(
                "a database import is running for {} — wait for it (or cancel it) first.",
                site.domain
            )));
        }
    }
    // Same shape for a connection rewrite mid-apply/revert: it holds the
    // site's config file and the imported-state row.
    if let Ok(active) = state.rewrite_active.lock() {
        if active.as_deref() == Some(site.domain.as_str()) {
            return Err(Error::Other(format!(
                "a connection rewrite is running for {} — wait for it to finish first.",
                site.domain
            )));
        }
    }
    // Step 7: a tunnel EXPOSES rather than mutates — provisioning under a
    // live link publishes a half-built site to whoever holds it. RETRY is the
    // live case (the site exists and can be shared); on a fresh create this
    // is a belt (no site row yet ⇒ no tunnel).
    if let Some(tunnels) = app.try_state::<crate::commands::tunnels::Tunnels>() {
        crate::commands::tunnels::refuse_if_shared(
            &tunnels,
            &state,
            &site.domain,
            "provisioning would rebuild it under the live link, publishing a half-built site",
        )?;
    }
    let site_tld = sites::domain_tld(&site.domain)?;
    // Retry is a USER action — the Retry button in the app, or the CLI. (No
    // agent tool retries: a scratch create that failed leaves a site the user
    // can retry or delete, and the agent is told exactly that.)
    core::dns::ensure_resolver(
        state.platform.as_ref(),
        &site_tld,
        core::dns::DEFAULT_DNS_PORT,
        core::dns::ResolverPrompt::Allow,
    )?;
    let docroot = PathBuf::from(&site.path);
    // Re-ensure prepare's artifacts — but ONLY for a docroot we own. A linked
    // site's folder is the user's: creating it, or dropping our phpinfo probe
    // into an empty one, would write into their project on a retry. The cert
    // below is ours either way.
    if site.docroot_managed != Some(false) {
        std::fs::create_dir_all(&docroot)?;
        if matches!(site.site_type, SiteType::Php) && !docroot.join("index.php").exists() {
            std::fs::write(docroot.join("index.php"), "<?php phpinfo();\n")?;
        }
    } else if !docroot.is_dir() {
        return Err(Error::Other(format!(
            "{} is linked to {}, which no longer exists — rexenv won't recreate a folder it \
             doesn't own. Restore it (or delete the site) and try again.",
            site.domain,
            docroot.display()
        )));
    }
    core::ssl::ensure_site_cert(
        state.platform.paths(),
        state.platform.permissions(),
        &state.ca,
        &site.domain,
    )?;

    let minor = core::php::minor_of(&site.php_version);
    let engine = DbEngine::from_site(site.db_engine);
    let engine_version = super::database::effective_db_version(&state, engine)?;
    let plan = build_plan(&state, &site, &minor, engine, &engine_version);
    // NOTE: the original blueprint id isn't persisted on the site — a retry
    // re-runs the core install path; blueprint items that already installed
    // persist (real installs), missing ones need a manual pass.
    spawn_job(&app, &state, &jobs, site, wordpress::InstallOptions::default(), None, plan)
}

/// Cancel a running provision. Only ever kills wp-cli children THIS job
/// spawned (registered pgid); between phases it just stops the job at the
/// next boundary. The detached binary prefetch is untouched — see module doc.
#[tauri::command]
pub async fn site_provision_cancel(
    state: State<'_, AppState>,
    jobs: State<'_, ProvisionJobs>,
    id: String,
) -> Result<()> {
    let entry = jobs
        .jobs
        .lock()
        .expect("provision jobs lock")
        .get(&id)
        .cloned()
        .ok_or_else(|| Error::Other(format!("no provision job {id}")))?;
    entry.cancel.cancel(state.platform.supervisor());
    Ok(())
}

/// One job's current snapshot by id (the CLI's / wrapper's settle-poll).
pub fn state_of(jobs: &ProvisionJobs, id: &str) -> Result<SiteProvisionState> {
    jobs.jobs
        .lock()
        .expect("provision jobs lock")
        .get(id)
        .map(|e| snapshot(e))
        .ok_or_else(|| Error::Other(format!("no provision job {id}")))
}

/// The most recent provision job (optionally for one domain), running or
/// settled — lets the Sites route / reopened dialog re-adopt the card.
#[tauri::command]
pub async fn site_provision_active(
    jobs: State<'_, ProvisionJobs>,
    domain: Option<String>,
) -> Result<Option<SiteProvisionState>> {
    let map = jobs.jobs.lock().expect("provision jobs lock");
    Ok(map
        .values()
        .filter(|e| domain.as_deref().map_or(true, |d| e.domain == d))
        .max_by_key(|e| e.seq)
        .map(|e| snapshot(e)))
}

// ---------------------------------------------------------------------------
// Worker
// ---------------------------------------------------------------------------

/// How the drive ended. Cancelled/timed-out attribution comes from the
/// pre-recorded flag, not the token.
enum JobEnd {
    Ok(String),
    Failed(String),
    Cancelled,
}

async fn run_provision_job<R: tauri::Runtime>(
    app: AppHandle<R>,
    entry: Arc<ProvisionEntry>,
    site: Site,
    plan: Vec<downloads::PlannedBinary>,
) {
    let keys: Vec<String> =
        snapshot(&entry).phases.iter().map(|p| p.key.clone()).collect();
    let key_refs: Vec<&str> = keys.iter().map(String::as_str).collect();
    let mut progress = sites::ProvisionProgress::new(&key_refs);
    progress.advance(1, 0.0); // prepare done

    let end = drive(&app, &entry, &site, plan, &mut progress).await;
    {
        let mut st = entry.state.lock().expect("provision state lock");
        match end {
            JobEnd::Ok(summary) => {
                st.status = "ok".into();
                st.pct = progress.finish();
                st.summary = Some(summary);
            }
            JobEnd::Cancelled => {
                st.status = if entry.timed_out.load(Ordering::SeqCst) {
                    "timed_out".into()
                } else {
                    "cancelled".into()
                };
                let cur = st.phase_cursor;
                if let Some(p) = st.phases.get_mut(cur) {
                    if p.status == "running" {
                        p.status = "cancelled".into();
                    }
                }
            }
            JobEnd::Failed(err) => {
                st.status = "failed".into();
                let cur = st.phase_cursor;
                if let Some(p) = st.phases.get_mut(cur) {
                    if p.status == "running" {
                        p.status = "failed".into();
                    }
                }
                st.error = Some(err);
            }
        }
    }
    // Settle-ok is the ONLY provisioned=1 writer — the flag means "the job
    // finished everything it could", independent of whether the stack was
    // running to serve it (that truth lives in the summary).
    if snapshot(&entry).status == "ok" {
        let state = app.state::<AppState>();
        if let Ok(conn) = lock_db(&state) {
            let _ = crate::state::store::set_site_provisioned(&conn, &site.id, true);
        };
    }
    emit_state(&app, &entry);
    entry.running.store(false, Ordering::SeqCst);
}

/// Enter phase `idx`: mark running + log the boundary marker.
fn enter_phase<R: tauri::Runtime>(app: &AppHandle<R>, entry: &ProvisionEntry, idx: usize) {
    let label = {
        let mut st = entry.state.lock().expect("provision state lock");
        st.phase_cursor = idx;
        st.phases[idx].status = "running".into();
        st.phases[idx].label.clone()
    };
    append_line(app, entry, &format!("── {label}"));
    emit_state(app, entry);
}

/// Finish phase `idx` (ok/skipped) and fold the weight into the bar.
fn finish_phase<R: tauri::Runtime>(
    app: &AppHandle<R>,
    entry: &ProvisionEntry,
    progress: &mut sites::ProvisionProgress,
    idx: usize,
    status: &str,
    note: Option<&str>,
) {
    let pct = progress.advance(idx + 1, 0.0);
    {
        let mut st = entry.state.lock().expect("provision state lock");
        st.phases[idx].status = status.into();
        st.pct = pct;
    }
    if let Some(n) = note {
        append_line(app, entry, n);
    }
    emit_state(app, entry);
}

fn phase_index(entry: &ProvisionEntry, key: &str) -> usize {
    snapshot(entry).phases.iter().position(|p| p.key == key).unwrap_or(0)
}

async fn drive<R: tauri::Runtime>(
    app: &AppHandle<R>,
    entry: &Arc<ProvisionEntry>,
    site: &Site,
    plan: Vec<downloads::PlannedBinary>,
    progress: &mut sites::ProvisionProgress,
) -> JobEnd {
    let cancelled = || entry.cancel.is_cancelled();
    macro_rules! bail_if_cancelled {
        () => {
            if cancelled() {
                return JobEnd::Cancelled;
            }
        };
    }

    // ── fetch ────────────────────────────────────────────────────────────
    let fx = phase_index(entry, "fetch");
    enter_phase(app, entry, fx);
    let download_ids = snapshot(entry).download_ids.clone();
    if download_ids.is_empty() {
        finish_phase(app, entry, progress, fx, "skipped", Some("binaries already cached"));
    } else {
        // DETACHED on purpose: cancel stops WAITING, never the download —
        // a Hub item is shared state other consumers (another create, a PHP
        // switch, Start all) may be waiting on, and an aborted stream would
        // hand the next caller a truncated-restart. A cancelled create still
        // warms the cache for its own retry.
        let plan2 = plan.clone();
        let mut fetch = tauri::async_runtime::spawn(async move {
            let plat = crate::platform::current();
            downloads::prefetch(plat.as_ref(), "Create site", &plan2).await
        });
        let fetched = loop {
            tokio::select! {
                r = &mut fetch => break Some(r),
                _ = tokio::time::sleep(Duration::from_millis(500)) => {
                    if cancelled() {
                        break None; // detach — the task keeps downloading
                    }
                    // Fold REAL bytes into the fetch slice. totalBytes can be
                    // None (no Content-Length) → count that item done-by-phase
                    // instead of by bytes.
                    let snap = downloads::hub().snapshot();
                    let mine: Vec<_> = snap
                        .items
                        .iter()
                        .filter(|i| download_ids.contains(&i.id))
                        .collect();
                    let frac = if mine.iter().all(|i| i.total_bytes.is_some()) && !mine.is_empty() {
                        let got: u64 = mine.iter().map(|i| i.downloaded_bytes).sum();
                        let total: u64 = mine.iter().filter_map(|i| i.total_bytes).sum();
                        if total > 0 { got as f64 / total as f64 } else { 0.0 }
                    } else {
                        let done = mine
                            .iter()
                            .filter(|i| matches!(i.phase, downloads::Phase::Done | downloads::Phase::Cached))
                            .count();
                        done as f64 / download_ids.len() as f64
                    };
                    let pct = progress.advance(fx, frac);
                    let changed = {
                        let mut st = entry.state.lock().expect("provision state lock");
                        let moved = st.pct != pct;
                        st.pct = pct;
                        moved
                    };
                    if changed {
                        emit_state(app, entry);
                    }
                }
            }
        };
        match fetched {
            None => return JobEnd::Cancelled,
            Some(Err(join)) => return JobEnd::Failed(format!("binary fetch worker died: {join}")),
            Some(Ok(Err(e))) => return JobEnd::Failed(format!("binary download failed: {e}")),
            Some(Ok(Ok(()))) => finish_phase(app, entry, progress, fx, "ok", None),
        }
    }
    bail_if_cancelled!();

    let state = app.state::<AppState>();
    // A linked site is ADOPTED, never installed into — see `phase_defs`. The
    // phase list already omits the WordPress phases; this keeps the driver in
    // step so it can't run a step that has no phase to report into.
    let linked = site.docroot_managed == Some(false);
    let is_wp = matches!(site.site_type, SiteType::Wordpress) && !linked;
    let is_laravel = matches!(site.site_type, SiteType::Laravel) && !linked;
    let minor = core::php::minor_of(&site.php_version);

    if is_wp {
        // ── db ───────────────────────────────────────────────────────────
        let ix = phase_index(entry, "db");
        enter_phase(app, entry, ix);
        let engine = DbEngine::from_site(site.db_engine);
        let check = {
            let mut mgr = state.services.lock().await;
            match mgr.spawn_db(state.platform.as_ref(), engine).await {
                Ok(c) => c,
                Err(e) => return JobEnd::Failed(format!("database start failed: {e}")),
            }
        };
        if let Err(e) = service_manager::await_ready(check.into_iter().collect()).await {
            return JobEnd::Failed(format!("database not ready: {e}"));
        }
        finish_phase(app, entry, progress, ix, "ok", None);
        bail_if_cancelled!();

        // Tools + env for the streamed wp-cli steps.
        let (php_bin, wp_phar) = match wp_tools(&state, &minor).await {
            Ok(t) => t,
            Err(e) => return JobEnd::Failed(e.to_string()),
        };
        let env = {
            let a = app.clone();
            match tauri::async_runtime::spawn_blocking(move || {
                let st = a.state::<AppState>();
                shell_env(&st, &a.state::<RepoJobs>(), false)
            })
            .await
            {
                Ok(Ok(env)) => env,
                Ok(Err(e)) => return JobEnd::Failed(e.to_string()),
                Err(e) => return JobEnd::Failed(format!("env worker died: {e}")),
            }
        };
        let docroot = PathBuf::from(&site.path);
        let engine_version = match super::database::effective_db_version(&state, engine) {
            Ok(v) => v,
            Err(e) => return JobEnd::Failed(e.to_string()),
        };
        let (db_client, _) =
            match engine.sql_client_bins(state.platform.as_ref(), &engine_version).await {
                Ok(c) => c,
                Err(e) => return JobEnd::Failed(e.to_string()),
            };
        let db_host = format!("127.0.0.1:{}", engine.port());
        let resolved =
            wordpress::resolve_install_options(&site.domain, &site.name, &entry.wp_opts);

        // ── core_download ────────────────────────────────────────────────
        let ix = phase_index(entry, "core_download");
        enter_phase(app, entry, ix);
        if docroot.join("wp-load.php").exists() {
            finish_phase(app, entry, progress, ix, "skipped", Some("WordPress core already present"));
        } else {
            let mut args: Vec<String> = vec!["core".into(), "download".into()];
            if !resolved.locale.is_empty() {
                args.push(format!("--locale={}", resolved.locale));
            }
            match streamed_step(app, entry, &env, &php_bin, &wp_phar, &docroot, args).await {
                StepEnd::Ok => finish_phase(app, entry, progress, ix, "ok", None),
                StepEnd::Cancelled => return JobEnd::Cancelled,
                StepEnd::Failed(e) => return JobEnd::Failed(format!("wp core download failed: {e}")),
            }
        }
        bail_if_cancelled!();

        // ── configure ────────────────────────────────────────────────────
        let ix = phase_index(entry, "configure");
        enter_phase(app, entry, ix);
        if !docroot.join("wp-config.php").exists() {
            let args: Vec<String> = vec![
                "config".into(),
                "create".into(),
                format!("--dbname={}", site.db_name),
                "--dbuser=root".into(),
                "--dbpass=".into(),
                format!("--dbhost={db_host}"),
                "--skip-check".into(),
            ];
            match streamed_step(app, entry, &env, &php_bin, &wp_phar, &docroot, args).await {
                StepEnd::Ok => {}
                StepEnd::Cancelled => return JobEnd::Cancelled,
                StepEnd::Failed(e) => return JobEnd::Failed(format!("wp config create failed: {e}")),
            }
        }
        // Bundled client, never PATH (`wp db create` shells out to a PATH
        // mysql that a Finder-launched app doesn't have).
        let port = engine.port();
        let (dbc, dbn) = (db_client.clone(), site.db_name.clone());
        match tauri::async_runtime::spawn_blocking(move || database::create_database(&dbc, port, &dbn))
            .await
        {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return JobEnd::Failed(format!("database create failed: {e}")),
            Err(e) => return JobEnd::Failed(format!("db worker died: {e}")),
        }
        finish_phase(app, entry, progress, ix, "ok", None);
        bail_if_cancelled!();

        // ── core_install ─────────────────────────────────────────────────
        let ix = phase_index(entry, "core_install");
        enter_phase(app, entry, ix);
        let (p2, w2, d2) = (php_bin.clone(), wp_phar.clone(), docroot.clone());
        let installed = tauri::async_runtime::spawn_blocking(move || {
            wordpress::core_is_installed(&p2, &w2, &d2)
        })
        .await
        .unwrap_or(false);
        if installed {
            finish_phase(app, entry, progress, ix, "skipped", Some("WordPress already installed"));
        } else {
            let args: Vec<String> = vec![
                "core".into(),
                "install".into(),
                format!("--url={}", resolved.url),
                format!("--title={}", resolved.title),
                format!("--admin_user={}", resolved.admin_user),
                format!("--admin_password={}", resolved.admin_password),
                format!("--admin_email={}", resolved.admin_email),
            ];
            match streamed_step(app, entry, &env, &php_bin, &wp_phar, &docroot, args).await {
                StepEnd::Ok => finish_phase(app, entry, progress, ix, "ok", None),
                StepEnd::Cancelled => return JobEnd::Cancelled,
                StepEnd::Failed(e) => return JobEnd::Failed(format!("wp core install failed: {e}")),
            }
        }
        bail_if_cancelled!();

        // ── blueprint ────────────────────────────────────────────────────
        if let Some(bp) = entry.blueprint.clone() {
            let ix = phase_index(entry, "blueprint");
            enter_phase(app, entry, ix);
            let (a2, e2) = (app.clone(), entry.clone());
            let (p2, w2, d2, env2) = (php_bin.clone(), wp_phar.clone(), docroot.clone(), env.clone());
            let spec = bp.spec.clone();
            let applied = tauri::async_runtime::spawn_blocking(move || {
                let st = a2.state::<AppState>();
                let stream = wordpress::WpStream {
                    sup: st.platform.supervisor(),
                    env: &env2,
                    cancel: &e2.cancel,
                };
                let mut on_line = |line: &str| append_line(&a2, &e2, line);
                blueprints::apply_wordpress(&p2, &w2, &d2, &spec, &stream, &mut on_line)
            })
            .await;
            match applied {
                Ok(Ok(blueprints::ApplyOutcome::Done(_))) => {}
                Ok(Ok(blueprints::ApplyOutcome::Cancelled(_))) => return JobEnd::Cancelled,
                Ok(Err(e)) => return JobEnd::Failed(e.to_string()),
                Err(e) => return JobEnd::Failed(format!("blueprint worker died: {e}")),
            }
            if !matches!(bp.spec.multisite, MultisiteMode::None) {
                let (a2, sid) = (app.clone(), site.id.clone());
                let (p2, w2, d2, mode) =
                    (php_bin.clone(), wp_phar.clone(), docroot.clone(), bp.spec.multisite);
                let converted = tauri::async_runtime::spawn_blocking(move || {
                    let st = a2.state::<AppState>();
                    let conn = lock_db(&st)?;
                    sites::convert_multisite(&conn, &p2, &w2, &d2, &sid, mode)
                })
                .await;
                match converted {
                    Ok(Ok(_)) => {}
                    Ok(Err(e)) => return JobEnd::Failed(format!("multisite convert failed: {e}")),
                    Err(e) => return JobEnd::Failed(format!("multisite worker died: {e}")),
                }
            }
            finish_phase(app, entry, progress, ix, "ok", None);
            bail_if_cancelled!();
        }
    }

    if is_laravel {
        // ── db ───────────────────────────────────────────────────────────
        let ix = phase_index(entry, "db");
        enter_phase(app, entry, ix);
        let engine = DbEngine::from_site(site.db_engine);
        let check = {
            let mut mgr = state.services.lock().await;
            match mgr.spawn_db(state.platform.as_ref(), engine).await {
                Ok(c) => c,
                Err(e) => return JobEnd::Failed(format!("database start failed: {e}")),
            }
        };
        if let Err(e) = service_manager::await_ready(check.into_iter().collect()).await {
            return JobEnd::Failed(format!("database not ready: {e}"));
        }
        finish_phase(app, entry, progress, ix, "ok", None);
        bail_if_cancelled!();

        let (php_bin, composer_phar) = match composer_tools(&state, &minor).await {
            Ok(t) => t,
            Err(e) => return JobEnd::Failed(e.to_string()),
        };
        let env = {
            let a = app.clone();
            match tauri::async_runtime::spawn_blocking(move || {
                let st = a.state::<AppState>();
                shell_env(&st, &a.state::<RepoJobs>(), false)
            })
            .await
            {
                Ok(Ok(env)) => env,
                Ok(Err(e)) => return JobEnd::Failed(e.to_string()),
                Err(e) => return JobEnd::Failed(format!("env worker died: {e}")),
            }
        };
        // The PROJECT root — `site.path`, not `served_root()`. Composer, the
        // `.env` and artisan all live one level above what nginx serves, which
        // is the entire point of `docroot_subdir` (v32).
        let project = PathBuf::from(&site.path);
        let engine_version = match super::database::effective_db_version(&state, engine) {
            Ok(v) => v,
            Err(e) => return JobEnd::Failed(e.to_string()),
        };
        let (db_client, _) =
            match engine.sql_client_bins(state.platform.as_ref(), &engine_version).await {
                Ok(c) => c,
                Err(e) => return JobEnd::Failed(e.to_string()),
            };

        // ── app_install ──────────────────────────────────────────────────
        let ix = phase_index(entry, "app_install");
        enter_phase(app, entry, ix);
        if core::laravel::is_installed(&project) {
            // A retry after a later phase failed: the app is already there and
            // `create-project` would refuse the non-empty directory anyway.
            finish_phase(app, entry, progress, ix, "skipped", Some("Laravel app already present"));
        } else {
            let (a2, e2, env2) = (app.clone(), entry.clone(), env.clone());
            let (p2, c2, d2) = (php_bin.clone(), composer_phar.clone(), project.clone());
            let created = tauri::async_runtime::spawn_blocking(move || {
                let st = a2.state::<AppState>();
                let mut on_line = |line: &str| append_line(&a2, &e2, line);
                core::laravel::create_project(
                    st.platform.supervisor(),
                    &p2,
                    &c2,
                    &d2,
                    &env2,
                    &e2.cancel,
                    &mut on_line,
                )
            })
            .await;
            match created {
                Ok(Ok(())) => finish_phase(app, entry, progress, ix, "ok", None),
                Ok(Err(e)) if e.to_string().contains("cancelled") => return JobEnd::Cancelled,
                Ok(Err(e)) => return JobEnd::Failed(format!("composer create-project failed: {e}")),
                Err(e) => return JobEnd::Failed(format!("composer worker died: {e}")),
            }
        }
        bail_if_cancelled!();

        // ── configure ────────────────────────────────────────────────────
        let ix = phase_index(entry, "configure");
        enter_phase(app, entry, ix);
        let port = engine.port();
        let (dbc, dbn) = (db_client.clone(), site.db_name.clone());
        match tauri::async_runtime::spawn_blocking(move || database::create_database(&dbc, port, &dbn))
            .await
        {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return JobEnd::Failed(format!("database create failed: {e}")),
            Err(e) => return JobEnd::Failed(format!("db worker died: {e}")),
        }
        append_line(app, entry, &format!("created database `{}`", site.db_name));
        // Record the PROVENANCE, not just the fact: `should_drop_database` only
        // drops a non-WordPress site's database on `db_created == Some(true)`,
        // because for every other type NULL used to mean "provisioning never
        // made one". Laravel provisioning now does, so without this line
        // deleting the site would leave its database behind forever.
        {
            let (a2, sid) = (app.clone(), site.id.clone());
            let recorded = tauri::async_runtime::spawn_blocking(move || {
                let st = a2.state::<AppState>();
                let conn = lock_db(&st)?;
                crate::state::store::set_site_db_created(&conn, &sid, true)
            })
            .await;
            match recorded {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => return JobEnd::Failed(format!("recording the database failed: {e}")),
                Err(e) => return JobEnd::Failed(format!("db-record worker died: {e}")),
            }
        }

        let env_file = core::laravel::env_path(&project);
        let db_settings = core::laravel::DbSettings {
            connection: "mysql".into(),
            host: "127.0.0.1".into(),
            port,
            database: site.db_name.clone(),
            username: "root".into(),
            password: String::new(),
        };
        let app_url = format!("https://{}", site.domain);
        match std::fs::read_to_string(&env_file) {
            Ok(original) => {
                let wired = core::laravel::wire_env(&original, &app_url, &db_settings);
                if let Err(e) = std::fs::write(&env_file, wired) {
                    return JobEnd::Failed(format!("writing {} failed: {e}", env_file.display()));
                }
                append_line(app, entry, ".env wired to this site's database and URL");
            }
            // Composer proved the app is installed, so a missing `.env` is a
            // real anomaly (the skeleton's post-create script writes it) — the
            // site would run on Laravel's SQLite default and nothing would say
            // so. Fail loudly rather than serve a site whose database is a
            // fiction.
            Err(e) => {
                return JobEnd::Failed(format!(
                    "the Laravel app installed but has no .env to wire ({}): {e}",
                    env_file.display()
                ))
            }
        }

        // Re-run the migrations against the database we just wired.
        //
        // Not optional, and not belt-and-braces: `composer create-project` runs
        // `artisan migrate --graceful` in its post-create script, and at that
        // moment the skeleton's `.env` still says SQLite — so the users/cache/
        // jobs tables were created inside `database/database.sqlite`, and the
        // MySQL database this site advertises is EMPTY. Skipping this would ship
        // a site whose Databases screen shows a database the app never filled.
        // The stray SQLite file is left alone (it is the project's own file, and
        // deleting what Composer wrote would be a surprise) — it is simply no
        // longer the connection `.env` names.
        {
            let (a2, e2, env2) = (app.clone(), entry.clone(), env.clone());
            let (p2, d2) = (php_bin.clone(), project.clone());
            let migrated = tauri::async_runtime::spawn_blocking(move || {
                let st = a2.state::<AppState>();
                let mut on_line = |line: &str| append_line(&a2, &e2, line);
                core::laravel::artisan(
                    st.platform.supervisor(),
                    &p2,
                    &d2,
                    &["migrate", "--force"],
                    &env2,
                    &e2.cancel,
                    &mut on_line,
                )
            })
            .await;
            match migrated {
                Ok(Ok(())) => append_line(app, entry, "migrations applied to the site's database"),
                Ok(Err(e)) if e.to_string().contains("cancelled") => return JobEnd::Cancelled,
                Ok(Err(e)) => return JobEnd::Failed(format!("php artisan migrate failed: {e}")),
                Err(e) => return JobEnd::Failed(format!("artisan worker died: {e}")),
            }
        }
        finish_phase(app, entry, progress, ix, "ok", None);
        bail_if_cancelled!();
    }

    // ── serve ────────────────────────────────────────────────────────────
    let ix = phase_index(entry, "serve");
    enter_phase(app, entry, ix);
    let fresh_sites = {
        match lock_db(&state) {
            Ok(conn) => match sites::list(&conn) {
                Ok(s) => s,
                Err(e) => return JobEnd::Failed(e.to_string()),
            },
            Err(e) => return JobEnd::Failed(e.to_string()),
        }
    };
    // Read the edge's blocked state while we already hold the lock: the
    // watchdog maintains it, so this costs nothing, where probing :443 per site
    // would add seconds to every import.
    let edge_blocked;
    let checks = {
        let mut mgr = state.services.lock().await;
        edge_blocked = mgr.edge_blocked();
        if mgr.is_running() {
            if !matches!(site.web_server, WebServer::Frankenphp) {
                if let Err(e) = mgr.ensure_php_pool(state.platform.as_ref(), &minor).await {
                    return JobEnd::Failed(format!("php pool failed: {e}"));
                }
            }
            match mgr.reload(state.platform.as_ref(), &state.ca, &fresh_sites, false).await {
                Ok(c) => Some(c),
                // The site EXISTS — row, folder, certificate — it just isn't
                // being served yet. Say that, say what to do, and carry the
                // underlying reason verbatim rather than an exit code. The row
                // stays `provisioned = 0`, so the list shows "setup incomplete"
                // with Retry, which re-runs exactly these steps.
                Err(e) => {
                    return JobEnd::Failed(format!(
                        "created, but not being served yet — the web server wouldn't reload. \
                         Fix the cause below, then Retry.\n{e}"
                    ))
                }
            }
        } else {
            None
        }
    };
    match checks {
        Some(checks) => {
            if let Err(e) = service_manager::await_ready(checks).await {
                return JobEnd::Failed(format!("site not answering after reload: {e}"));
            }
            finish_phase(app, entry, progress, ix, "ok", None);
            if edge_blocked {
                // The site really was created and the stack really did reload —
                // but something else owns :443, so claiming it is serving would
                // be a lie the user discovers by clicking the link.
                entry.state.lock().expect("provision state lock").serving_blocked = true;
                return JobEnd::Ok(format!(
                    "created — but another app is answering port 443, so {} won't load until you quit it",
                    site.domain
                ));
            }
            JobEnd::Ok(format!("created — serving at https://{}", site.domain))
        }
        None => {
            finish_phase(app, entry, progress, ix, "skipped", Some("stack is stopped — nothing to reload"));
            JobEnd::Ok(format!(
                "created — stack is stopped, {} serves on next stack start",
                site.domain
            ))
        }
    }
}

/// One streamed wp-cli step under the job's token + a B25 outer wall-clock
/// cap (`download_timeout(1)` — wp-cli's own 300s/download bound still fires
/// first on a dead link; the timer records the reason THEN cancels).
enum StepEnd {
    Ok,
    Cancelled,
    Failed(String),
}

async fn streamed_step<R: tauri::Runtime>(
    app: &AppHandle<R>,
    entry: &Arc<ProvisionEntry>,
    env: &EnvSnapshot,
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    args: Vec<String>,
) -> StepEnd {
    let done = Arc::new(AtomicBool::new(false));
    let (t_done, t_entry, t_app) = (done.clone(), entry.clone(), app.clone());
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(wordpress::download_timeout(1)).await;
        if !t_done.load(Ordering::SeqCst) && t_entry.running.load(Ordering::SeqCst) {
            t_entry.timed_out.store(true, Ordering::SeqCst);
            let st = t_app.state::<AppState>();
            t_entry.cancel.cancel(st.platform.supervisor());
        }
    });

    let (b_app, b_entry, b_env) = (app.clone(), entry.clone(), env.clone());
    let (php, phar, dr) = (php_bin.to_path_buf(), wp_phar.to_path_buf(), docroot.to_path_buf());
    let res = tauri::async_runtime::spawn_blocking(move || {
        let st = b_app.state::<AppState>();
        let stream = wordpress::WpStream {
            sup: st.platform.supervisor(),
            env: &b_env,
            cancel: &b_entry.cancel,
        };
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let mut on_line = |line: &str| append_line(&b_app, &b_entry, line);
        wordpress::wp_step_streamed(&stream, &php, &phar, &dr, &arg_refs, &mut on_line)
    })
    .await;
    done.store(true, Ordering::SeqCst);
    match res {
        Ok(Ok(sr)) if sr.ok => StepEnd::Ok,
        Ok(Ok(sr)) if sr.cancelled => StepEnd::Cancelled,
        Ok(Ok(sr)) => StepEnd::Failed(
            sr.tail.last().cloned().unwrap_or_else(|| format!("exit {:?}", sr.exit)),
        ),
        Ok(Err(e)) => StepEnd::Failed(e.to_string()),
        Err(e) => StepEnd::Failed(format!("step worker died: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::sites::Ownership;

    /// The blueprint rule, stated once. Accepting a blueprint for a site that
    /// has no blueprint phase is the dishonest failure — it succeeds and does
    /// nothing — so every non-WordPress combination must be an ERROR whose
    /// message says why, not a quiet no-op.
    #[test]
    fn a_blueprint_is_refused_wherever_no_blueprint_phase_would_run() {
        // The one case that works: a WordPress site whose folder we create.
        assert!(ensure_blueprint_applies(SiteType::Wordpress, "", Some("bp-1")).is_ok());

        for ty in [SiteType::Laravel, SiteType::Php] {
            let err = ensure_blueprint_applies(ty, "", Some("bp-1")).unwrap_err().to_string();
            assert!(err.contains("WordPress sites only"), "{ty:?}: {err}");
            assert!(err.contains(ty.as_db()), "the message must name the type: {err}");
        }

        // A LINKED WordPress folder is adopted as-is — `phase_defs` gives it no
        // blueprint phase either, and installing into someone's own project is
        // the worse half of the bug.
        let err = ensure_blueprint_applies(SiteType::Wordpress, "/Users/x/Sites/theirs", Some("bp-1"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("adopts as-is"), "{err}");

        // No blueprint asked for: every type passes, including the ones above.
        for ty in [SiteType::Wordpress, SiteType::Laravel, SiteType::Php] {
            assert!(ensure_blueprint_applies(ty, "", None).is_ok());
            assert!(ensure_blueprint_applies(ty, "", Some("")).is_ok(), "empty id means none");
            assert!(ensure_blueprint_applies(ty, "/Users/x/theirs", None).is_ok());
        }
    }

    /// The guard and the phase list must agree: exactly the shape that ACCEPTS
    /// a blueprint is the shape that gets a blueprint phase to run it in.
    #[test]
    fn the_guard_admits_exactly_the_shapes_phase_defs_gives_a_blueprint_phase() {
        for ty in [SiteType::Wordpress, SiteType::Laravel, SiteType::Php] {
            for linked in [false, true] {
                let path = if linked { "/Users/x/Sites/theirs" } else { "" };
                let admitted = ensure_blueprint_applies(ty, path, Some("bp-1")).is_ok();
                let has_phase =
                    phase_defs(ty, true, linked).iter().any(|(k, _)| *k == "blueprint");
                assert_eq!(admitted, has_phase, "{ty:?} linked={linked}");
            }
        }
    }

    use crate::platform::traits::{DnsManager, Paths, PrivilegeManager};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    /// Paths rooted in a throwaway temp dir — nothing here touches real app data.
    struct TmpPaths(PathBuf);
    impl Paths for TmpPaths {
        fn app_data_dir(&self) -> crate::error::Result<PathBuf> {
            Ok(self.0.clone())
        }
        fn config_dir(&self) -> crate::error::Result<PathBuf> {
            Ok(self.0.join("config"))
        }
        fn log_dir(&self) -> crate::error::Result<PathBuf> {
            Ok(self.0.join("logs"))
        }
        fn bin_dir(&self) -> crate::error::Result<PathBuf> {
            Ok(self.0.join("bin"))
        }
        fn hosts_file(&self) -> PathBuf {
            self.0.join("hosts")
        }
    }

    /// A resolver file that does NOT exist — `resolver_owner` reads that as
    /// `Absent`, the state that decides whether a prompt happens. This is the
    /// half-onboarded machine the never-prompt rule exists for.
    struct AbsentResolver(PathBuf);
    impl DnsManager for AbsentResolver {
        fn resolver_path(&self, tld: &str) -> PathBuf {
            self.0.join(format!("resolver-{tld}"))
        }
        fn resolver_contents(&self, port: u16) -> String {
            format!("nameserver 127.0.0.1\nport {port}\n")
        }
        fn install_command(&self, _tld: &str, _port: u16) -> String {
            "true".into()
        }
        fn uninstall_command(&self, _tlds: &[String]) -> String {
            "true".into()
        }
        fn restore_command(&self, _restores: &[(String, PathBuf)]) -> String {
            "true".into()
        }
    }

    /// Records every privileged escalation instead of performing one. An EMPTY
    /// log is the assertion: "nothing an agent can call ever asks macOS for an
    /// administrator password" is the guarantee paragraph's most brittle
    /// sentence (PLAN §4.2/§6.0), and this recorder is what catches it going
    /// false. Returning Ok keeps the path RUNNING past the escalation, so the
    /// test measures whether one was attempted rather than whether it worked.
    #[derive(Default)]
    struct RecordingPrivileges {
        calls: Mutex<Vec<String>>,
    }
    impl PrivilegeManager for RecordingPrivileges {
        fn run_privileged(&self, script: &str) -> crate::error::Result<String> {
            self.calls.lock().unwrap().push(script.to_string());
            Ok(String::new())
        }
    }

    /// Real file ops in the temp dir — `configure_resolver` writes its backup
    /// through this on the USER path, and stubbing it would stop the control
    /// test before it reaches the escalation it exists to observe.
    struct TmpPermissions;
    impl crate::platform::traits::PermissionManager for TmpPermissions {
        fn set_executable(&self, _path: &std::path::Path) -> crate::error::Result<()> {
            Ok(())
        }
        fn set_private(&self, _path: &std::path::Path) -> crate::error::Result<()> {
            Ok(())
        }
        fn write_private(
            &self,
            path: &std::path::Path,
            contents: &[u8],
        ) -> crate::error::Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(path, contents)?;
            Ok(())
        }
    }

    struct ProvisionTestPlatform {
        paths: TmpPaths,
        dns: AbsentResolver,
        privileges: Arc<RecordingPrivileges>,
    }

    impl crate::platform::traits::Platform for ProvisionTestPlatform {
        fn paths(&self) -> &dyn Paths {
            &self.paths
        }
        fn dns(&self) -> &dyn DnsManager {
            &self.dns
        }
        fn privileges(&self) -> &dyn PrivilegeManager {
            self.privileges.as_ref()
        }
        fn edge(&self) -> &dyn crate::platform::traits::EdgeSupervisor {
            unimplemented!("the create path must not reach the edge")
        }
        fn cert_trust(&self) -> &dyn crate::platform::traits::CertTrustManager {
            unimplemented!()
        }
        fn supervisor(&self) -> &dyn crate::platform::traits::ProcessSupervisor {
            unimplemented!()
        }
        fn autostart(&self) -> &dyn crate::platform::traits::AutostartManager {
            unimplemented!()
        }
        fn permissions(&self) -> &dyn crate::platform::traits::PermissionManager {
            &TmpPermissions
        }
        fn shell(&self) -> &dyn crate::platform::traits::ShellRunner {
            unimplemented!()
        }
        fn binaries(&self) -> &dyn crate::platform::traits::BinaryProvider {
            unimplemented!()
        }
        fn dns_agent(&self) -> &dyn crate::platform::traits::DnsAgentManager {
            unimplemented!()
        }
    }

    /// Drive the REAL `start()` — the whole prepare phase, not a precheck —
    /// against a machine whose resolver file is missing. Returns the outcome and
    /// every privileged escalation the path attempted.
    fn run_start(ownership: Ownership) -> (crate::error::Result<SiteProvisionState>, Vec<String>) {
        let dir = std::env::temp_dir()
            .join(format!("rexenv-provision-prompt-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let recorder = Arc::new(RecordingPrivileges::default());
        let platform = ProvisionTestPlatform {
            paths: TmpPaths(dir.clone()),
            dns: AbsentResolver(dir.clone()),
            privileges: Arc::clone(&recorder),
        };
        let conn = crate::state::db::open_in_memory().unwrap();
        let ca = crate::core::ssl::LocalCa {
            cert_pem: String::new(),
            key_pem: String::new(),
            cert_path: dir.join("ca.pem"),
            key_path: dir.join("ca.key"),
        };
        let app = tauri::test::mock_app();
        let state = AppState::new(conn, Box::new(platform), ca);
        let jobs = ProvisionJobs::default();
        let new = NewSite {
            name: "probe".into(),
            domain: "probe.scratch.rex".into(),
            site_type: SiteType::Wordpress,
            php_version: "8.3".into(),
            web_server: crate::state::models::WebServer::Nginx,
            path: String::new(),
            db_engine: crate::state::models::SiteDbEngine::Mysql,
        };
        let out = start(&app.handle().clone(), &state, &jobs, new, None, None, ownership);
        let calls = recorder.calls.lock().unwrap().clone();
        let _ = std::fs::remove_dir_all(&dir);
        (out, calls)
    }

    #[test]
    fn an_agent_create_never_reaches_a_privileged_prompt() {
        // THE guarantee paragraph's most brittle sentence, driven end to end
        // through the real provision path rather than asserted at an entry
        // check: resolver absent, agent ownership, and the recorder must stay
        // empty. If the policy stopped being derived from ownership — or a
        // future call site defaulted to prompting — `calls` fills and this
        // fails. (Proven load-bearing: with `Ownership::User` the same path
        // DOES escalate, see the sibling test.)
        let (out, calls) = run_start(Ownership::Agent { client: "Claude Code".into(), ttl_hours: 24 });
        assert!(calls.is_empty(), "an agent create attempted a privileged escalation: {calls:?}");
        let err = out.expect_err("a missing resolver must FAIL the agent path").to_string();
        assert!(err.contains("administrator password"), "says why it stopped: {err}");
        assert!(err.contains("finish its setup"), "names the USER's action: {err}");
        assert!(!err.contains("try a different"), "never suggests routing around it: {err}");
    }

    #[test]
    fn the_same_path_does_prompt_for_a_user_create_so_the_flag_is_what_stops_it() {
        // The control. Without it, `an_agent_create_never_reaches_a_privileged_prompt`
        // could pass because this fixture cannot escalate at all — which would
        // make it decoration rather than a guard. Same platform, same missing
        // resolver, ownership the only difference.
        let (_out, calls) = run_start(Ownership::User);
        assert!(
            !calls.is_empty(),
            "the user path must still install the resolver — otherwise the agent test proves nothing"
        );
        assert!(calls[0].contains("resolver") || !calls[0].is_empty(), "escalated to install it");
    }
}

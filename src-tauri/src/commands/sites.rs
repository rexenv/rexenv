//! commands::sites — thin Tauri IPC handlers for sites. Call `core/` only.

use crate::core;
use crate::core::db::DbEngine;
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::models::{NewSite, Site, SiteServing, SiteType, WebServer};
use tauri::State;

fn lock<'a>(
    state: &'a State<'a, AppState>,
) -> Result<std::sync::MutexGuard<'a, rusqlite::Connection>> {
    state
        .db
        .lock()
        .map_err(|_| Error::Other("database lock poisoned".into()))
}

/// List all sites (newest first).
#[tauri::command]
pub fn list_sites(state: State<'_, AppState>) -> Result<Vec<Site>> {
    let conn = lock(&state)?;
    core::sites::list(&conn)
}

/// What linking a folder would do, so the New Site dialog can show it BEFORE
/// anything is created.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedFolderInfo {
    /// The project folder, canonicalized.
    pub root: String,
    /// The folder we would actually serve — often a subfolder, since a
    /// framework's docroot is rarely its project root.
    pub serve_path: String,
    /// That subfolder relative to the root (`""` = the root itself).
    pub docroot_rel: String,
    pub site_type: SiteType,
    /// Framework name for display ("WordPress", "Laravel", …).
    pub label: String,
    /// The folder already holds an app — we adopt it, install nothing.
    pub existing_install: bool,
    /// A `LocalValetDriver.php` decides this project's docroot by running PHP,
    /// so our own detection may disagree with what Valet served.
    pub has_custom_valet_driver: bool,
}

/// Inspect a folder the user picked, WITHOUT creating anything: classify it,
/// resolve what we'd serve, and run the full link preflight so the dialog can
/// refuse early with the real reason. Pure filesystem — nothing in the folder
/// is executed.
#[tauri::command]
pub fn inspect_linked_folder(
    state: State<'_, AppState>,
    path: String,
) -> Result<LinkedFolderInfo> {
    let conn = lock(&state)?;
    let platform = state.platform.as_ref();
    // Validate the ROOT first so an unusable pick fails with the honest reason
    // (too broad, overlaps another site, …) rather than a confusing miss on a
    // subfolder we derived from it.
    let root = core::sites::validate_linked_docroot(&conn, platform, &path)?;
    let detected = core::sites::detect_project(&root);
    let serve = if detected.docroot_rel.is_empty() {
        root.clone()
    } else {
        root.join(&detected.docroot_rel)
    };
    // The served subfolder is what actually gets stored, so it must pass too.
    let serve_path = core::sites::validate_linked_docroot(
        &conn,
        platform,
        &serve.display().to_string(),
    )?;
    Ok(LinkedFolderInfo {
        root: root.display().to_string(),
        serve_path: serve_path.display().to_string(),
        docroot_rel: detected.docroot_rel,
        site_type: detected.site_type,
        label: detected.label.to_string(),
        existing_install: detected.existing_install,
        has_custom_valet_driver: core::sites::has_custom_valet_driver(&root),
    })
}

/// Live per-site serving status (H1 follow-up): whether each site is actually
/// reachable (edge up AND its own upstream up), not just whether the stack is up.
/// Derived from the non-blocking `service_infos()` snapshot, so it never blocks the
/// UI on a long start/stop. The frontend overlays it on the site rows by domain.
#[tauri::command]
pub fn sites_serving(state: State<'_, AppState>) -> Result<Vec<SiteServing>> {
    let sites = {
        let conn = lock(&state)?;
        core::sites::list(&conn)?
    };
    Ok(core::service_manager::site_serving(&sites, &state.service_infos()))
}

/// Honest per-site resource attribution (Sites page). A site is NOT a process
/// here — default sites share nginx + a per-version php-fpm pool — so the shape
/// is explicit about what each number IS:
/// - FrankenPHP-override sites: REAL CPU/RAM from their own backend's process
///   tree (same `Monitor::tree` source as the Services rows / footer).
/// - Every WP/Laravel site: REAL MySQL database disk size.
/// - Shared sites: ACTIVITY (requests + bytes over the last 60s window, from
///   the shared nginx access log) — never a fabricated per-site CPU/RAM.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteResources {
    pub id: String,
    pub domain: String,
    /// True for a FrankenPHP-override site (own process → real CPU/RAM);
    /// false ⇒ shared nginx + pool (activity metrics only).
    pub dedicated: bool,
    pub cpu_percent: Option<f32>,
    pub ram_mb: Option<u64>,
    /// Requests / sent bytes in the last 60s (shared nginx sites only —
    /// override sites bypass nginx).
    pub requests_per_min: Option<u64>,
    pub bytes_per_min: Option<u64>,
    /// MySQL database size in bytes (`None`: no DB / MySQL not running).
    pub db_size_bytes: Option<u64>,
}

/// Per-site resources for the Sites page — one monitor source of truth
/// (`Monitor::tree`) for the dedicated numbers, the shared nginx access log
/// for activity, one `information_schema` query for DB sizes.
#[tauri::command]
pub async fn sites_resources(state: State<'_, AppState>) -> Result<Vec<SiteResources>> {
    let sites = {
        let conn = lock(&state)?;
        core::sites::list(&conn)?
    };

    // FrankenPHP override backends (domain → pid). try_lock: during a long
    // start/stop just omit the dedicated numbers for one poll.
    let override_pids: std::collections::HashMap<String, u32> = state
        .services
        .try_lock()
        .map(|mgr| mgr.override_pids().into_iter().collect())
        .unwrap_or_default();

    // Activity per host from the shared nginx access log (last 60s).
    let access_log = state.platform.paths().log_dir()?.join("nginx-access.log");
    let activity =
        core::site_metrics::activity_by_host(&access_log, time::OffsetDateTime::now_utc());

    // DB sizes: one query per RUNNING site engine — resolved strictly from the
    // already-published cache (never a download from a status poll). Kept as
    // one map per engine: the same db name could exist in both engines, and a
    // site must read its own engine's number.
    let db_sizes: std::collections::HashMap<&'static str, std::collections::HashMap<String, u64>> =
        [DbEngine::Mysql, DbEngine::Mariadb]
            .into_iter()
            .filter(|e| e.running())
            .filter_map(|e| {
                let version = super::database::effective_db_version(&state, e).ok()?;
                let client = e.cached_sql_client(state.platform.as_ref(), &version)?;
                let sizes = core::database::db_sizes(&client, e.port()).ok()?;
                Some((e.key(), sizes.into_iter().collect()))
            })
            .collect();

    let mut monitor = state
        .monitor
        .lock()
        .map_err(|_| Error::Other("monitor lock poisoned".into()))?;
    monitor.refresh_processes();

    Ok(sites
        .into_iter()
        .map(|s| {
            let tree = override_pids.get(&s.domain).and_then(|pid| monitor.tree(*pid));
            let act = activity.get(&s.domain);
            SiteResources {
                dedicated: matches!(s.web_server, WebServer::Frankenphp | WebServer::Apache),
                cpu_percent: tree.map(|t| t.cpu_percent),
                ram_mb: tree.map(|t| t.ram_mb),
                requests_per_min: act.map(|a| a.requests),
                bytes_per_min: act.map(|a| a.bytes),
                db_size_bytes: db_sizes
                    .get(DbEngine::from_site(s.db_engine).key())
                    .and_then(|m| m.get(&s.db_name))
                    .copied(),
                id: s.id,
                domain: s.domain,
            }
        })
        .collect())
}

/// Rename a site's display name (domain/docroot/DB/certs unchanged); returns the
/// updated site.
#[tauri::command]
pub fn rename_site(state: State<'_, AppState>, id: String, name: String) -> Result<Option<Site>> {
    let conn = lock(&state)?;
    core::sites::rename(&conn, &id, &name)
}

/// Read-only info about a site's HTTPS leaf cert (Settings tab): validity dates,
/// days left, SANs, cert folder. `None` when no cert file exists yet.
#[tauri::command]
pub fn site_cert_info(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<core::ssl::SiteCertInfo>> {
    let site = {
        let conn = lock(&state)?;
        core::sites::get(&conn, &id)?
    }
    .ok_or_else(|| Error::Other(format!("site not found: {id}")))?;
    core::ssl::site_cert_info(state.platform.paths(), &site.domain)
}

/// Re-issue one site's HTTPS leaf cert (same local CA, SANs `domain` + `*.domain`)
/// and FORCE-reload the edge so Caddy serves it immediately. The re-issue is
/// atomic (temp-write + rename — a failure leaves the old cert intact and served);
/// a failed reload keeps the old, still-valid cert served and says so. Backs the
/// Settings-tab "Regenerate certificate" action; also the recovery path for a
/// deleted/corrupted cert or one nearing the 398-day Safari cap.
#[tauri::command]
pub async fn regenerate_site_cert(state: State<'_, AppState>, id: String) -> Result<()> {
    let (site, sites) = {
        let conn = lock(&state)?;
        let site = core::sites::get(&conn, &id)?
            .ok_or_else(|| Error::Other(format!("site not found: {id}")))?;
        (site, core::sites::list(&conn)?)
    };
    core::ssl::reissue_site_cert(
        state.platform.paths(),
        state.platform.permissions(),
        &state.ca,
        &site.domain,
    )?;
    super::system::reload_edge_for_new_certs(&state, &sites).await
}

/// A create that failed — **and whether a site row EXISTS anyway**.
///
/// The prepare phase is inline, so a validation failure creates nothing; but a
/// failure in provisioning (WordPress download, install) happens AFTER the row
/// is inserted, and leaves a real site the user can see, retry, or delete. A
/// bare `Error` cannot distinguish those two, and the difference is the whole
/// content of "what should I do now" — for a person and, in the MCP path, for an
/// agent that must not report "nothing happened" about a site sitting in the
/// user's list.
#[derive(Debug)]
pub(crate) struct CreateFailure {
    /// The id of the site that was created before the failure, if any.
    pub site_id: Option<String>,
    pub error: Error,
}

/// **The promotion choke point**: a user deliberately changing a scratch site
/// makes it theirs, so the reaper can never take a site they just adopted.
///
/// The rule is *"the user deliberately changed THIS site"*, not a list — which is
/// why it lives in one function called from every user-facing site-mutation
/// command rather than in four remembered places. §4.3 named four (rename, move,
/// env, share); applying the rule instead adds the PHP switch, the web-server
/// switch and the Xdebug toggle, and removes share — sharing a scratch site is
/// sharing a scratch site, not claiming it, and the reaper's skip-and-surface
/// already tells that story. It also excludes the agent's own tools (an agent
/// promoting its own sites would be a cap bypass with a plausible face) and
/// rexenv's housekeeping (a reconcile is not user intent).
///
/// One call, because Keep is ONE write: four call sites each setting `origin`
/// and clearing `expires_at` would be four places for two facts to disagree.
/// Best-effort — failing to promote must never fail the change the user asked
/// for; the reaper's own re-read is the backstop.
pub(crate) fn promote_if_scratch(state: &AppState, id: &str) {
    let Ok(conn) = state.db.lock() else { return };
    match crate::state::store::keep_site(&conn, id) {
        Ok(true) => log::info!(
            "sites: {id} was a scratch site and you changed it — it's yours now, and rexenv \
             will not clean it up"
        ),
        Ok(false) => {}
        Err(e) => log::warn!("sites: could not promote scratch site {id}: {e}"),
    }
}

/// **Keep** — the user adopting a scratch site on purpose (MCP M2a).
///
/// The same ONE write as [`promote_if_scratch`], through the same store fn, so
/// the deliberate door and the implied one cannot express "not disposable any
/// more" differently. Idempotent by its own `WHERE origin = 'agent'`: keeping a
/// site twice changes nothing, and keeping a site that was never the agent's is
/// a no-op rather than an error.
///
/// There is no un-keep, deliberately, and the confirm dialog says so — the way
/// back is deleting the site like any other. Returns whether a row changed, so
/// the UI can tell "adopted just now" from "already yours".
#[tauri::command]
pub async fn keep_site(state: State<'_, AppState>, id: String) -> Result<bool> {
    let conn = state
        .db
        .lock()
        .map_err(|_| Error::Other("database lock poisoned".into()))?;
    crate::state::store::keep_site(&conn, &id)
}

/// Add an extra domain to a site and make it SERVE on it: validate, record,
/// then rebuild the configs and reload the web tier.
///
/// The reload is not an optimisation — it is the difference between the feature
/// and a row in a table. An alias that is stored and not served is the worst of
/// both: the UI says the site answers on it and the browser says it does not.
#[tauri::command]
pub async fn add_site_domain(
    state: State<'_, AppState>,
    id: String,
    domain: String,
) -> Result<Vec<String>> {
    let (added, site, sites, aliases) = {
        let conn = lock(&state)?;
        let added = core::sites::add_alias(&conn, &id, &domain)?;
        let site = core::sites::get(&conn, &id)?
            .ok_or_else(|| Error::Other(format!("site not found: {id}")))?;
        let sites = core::sites::list(&conn)?;
        let aliases = crate::state::store::all_site_aliases(&conn)?;
        (added, site, sites, aliases)
    };
    let _ = added;
    reload_for_domains(&state, sites, aliases).await?;
    let conn = lock(&state)?;
    core::sites::all_domains(&conn, &site)
}

/// Remove an extra domain and stop serving on it.
#[tauri::command]
pub async fn remove_site_domain(
    state: State<'_, AppState>,
    id: String,
    domain: String,
) -> Result<Vec<String>> {
    let (removed, site, sites, aliases) = {
        let conn = lock(&state)?;
        let removed = core::sites::remove_alias(&conn, &id, &domain)?;
        let site = core::sites::get(&conn, &id)?
            .ok_or_else(|| Error::Other(format!("site not found: {id}")))?;
        let sites = core::sites::list(&conn)?;
        let aliases = crate::state::store::all_site_aliases(&conn)?;
        (removed, site, sites, aliases)
    };
    if !removed {
        return Err(Error::Other(format!(
            "\"{}\" doesn't serve {domain}",
            site.domain
        )));
    }
    reload_for_domains(&state, sites, aliases).await?;
    let conn = lock(&state)?;
    core::sites::all_domains(&conn, &site)
}

/// Every hostname a site answers on (primary first).
#[tauri::command]
pub async fn site_domains(state: State<'_, AppState>, id: String) -> Result<Vec<String>> {
    let conn = lock(&state)?;
    let site = core::sites::get(&conn, &id)?
        .ok_or_else(|| Error::Other(format!("site not found: {id}")))?;
    core::sites::all_domains(&conn, &site)
}

/// Mirror the new alias map into the manager and reload the web tier — the
/// shared tail of add/remove.
///
/// `set_site_aliases` BEFORE `reload`, in the same locked scope: the reload
/// regenerates configs from the manager's mirror, not the database, so setting
/// it afterwards would serve the previous name set until something else
/// happened to reload.
async fn reload_for_domains(
    state: &State<'_, AppState>,
    sites: Vec<Site>,
    aliases: std::collections::HashMap<String, Vec<String>>,
) -> Result<()> {
    let checks = {
        let mut mgr = state.services.lock().await;
        mgr.set_site_aliases(aliases);
        if !mgr.is_running() {
            // Nothing to reload; the configs are regenerated at the next start.
            return Ok(());
        }
        mgr.reload(state.platform.as_ref(), &state.ca, &sites, false).await?
    };
    core::service_manager::await_ready(checks).await
}

/// What `restart_site` did, in the words the caller reports.
///
/// The shape exists because "restart this site" has no single honest meaning in
/// rexenv's topology (`docs/ARCHITECTURE.md`): a default site has NO process of
/// its own — shared nginx, and a php-fpm pool shared with every other site on
/// its PHP minor — so the truthful answer is what was reloaded and what a pool
/// bounce would cost, not a cheerful "restarted".
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteRestartReport {
    /// "backend" | "shared" | "refused".
    pub kind: &'static str,
    /// The override server that was bounced, when there was one.
    pub server: Option<String>,
    /// Its loopback port.
    pub port: Option<u16>,
    /// The PHP minor whose pool serves this site.
    pub php_minor: String,
    /// That pool's port.
    pub pool_port: u16,
    /// How many sites (including this one) that pool serves — the number a
    /// `--pool` restart affects, and the reason it is opt-in.
    pub sites_on_pool: usize,
    /// Whether the pool was actually restarted (only on request).
    pub pool_restarted: bool,
}

/// Restart ONE site: its own backend if it has one, otherwise a config rebuild
/// and a web-tier reload — never a silent pool bounce.
///
/// # Why this refuses to do the obvious thing
///
/// The default topology gives a site no process to restart. The action a user
/// means by "restart my site" is usually "make it pick up my change", and for a
/// default site that IS the config rebuild + nginx/edge reload this does. The
/// other candidate — restarting the php-fpm pool — is shared by every site on
/// that PHP minor, so doing it implicitly would stop other people's sites to
/// satisfy this one. It is therefore `pool: true` only, and the report says how
/// many sites that number covers whether or not it was asked for.
#[tauri::command]
pub async fn restart_site(
    state: State<'_, AppState>,
    id: String,
    pool: bool,
) -> Result<SiteRestartReport> {
    let (site, sites) = {
        let conn = lock(&state)?;
        let site = core::sites::get(&conn, &id)?
            .ok_or_else(|| Error::Other(format!("site not found: {id}")))?;
        (site, core::sites::list(&conn)?)
    };
    let pool_port = core::sites::pool_port_for_site(&site);
    let sites_on_pool =
        sites.iter().filter(|s| core::sites::pool_port_for_site(s) == pool_port).count();

    // Spawn under the lock, await readiness after dropping it (M4) — a backend
    // that takes seconds to bind must not park status polls.
    let (outcome, checks) = {
        let mut mgr = state.services.lock().await;
        mgr.restart_site_backend(state.platform.as_ref(), &site).await?
    };
    core::service_manager::await_ready(checks).await?;

    let (kind, server, port) = match &outcome {
        core::service_manager::SiteRestartOutcome::Backend { server, port } => {
            ("backend", Some((*server).to_string()), Some(*port))
        }
        core::service_manager::SiteRestartOutcome::Refused { server } => {
            ("refused", Some((*server).to_string()), None)
        }
        core::service_manager::SiteRestartOutcome::Shared { .. } => {
            // No per-site process: rebuild the configs and reload the web tier,
            // which is what actually makes this site pick up a change.
            let checks = {
                let mut mgr = state.services.lock().await;
                mgr.reload(state.platform.as_ref(), &state.ca, &sites, false).await?
            };
            core::service_manager::await_ready(checks).await?;
            ("shared", None, None)
        }
    };

    let pool_restarted = if pool {
        let checks = {
            let mut mgr = state.services.lock().await;
            mgr.restart_pools_for(
                state.platform.as_ref(),
                std::slice::from_ref(&core::php::minor_of(&site.php_version)),
            )
            .await?
        };
        core::service_manager::await_ready(checks).await?;
        true
    } else {
        false
    };

    Ok(SiteRestartReport {
        kind,
        server,
        port,
        php_minor: core::php::minor_of(&site.php_version),
        pool_port,
        sites_on_pool,
        pool_restarted,
    })
}

/// A cloned package as the Sites page reads it — the recorded row plus ONE fact
/// the row cannot carry.
///
/// `source_missing` is stat-ed at read time because "the source moved" is a
/// different state from "nothing changed", and only the filesystem knows which.
/// Without it a moved source would render as the most reassuring reading of a
/// stale timestamp, which is the worst version of this field: the user would be
/// told the site is running code from a directory that is no longer there.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScratchPackageView {
    pub site_id: String,
    pub slug: String,
    pub kind: String,
    /// The user's OWN project directory — the path they chose and already know.
    /// Shown so "I changed my plugin and the site didn't see it" is answered on
    /// screen. (Not the site's docroot, which this view never carries.)
    pub source_path: String,
    pub synced_at: String,
    /// The recorded source is not a directory right now.
    pub source_missing: bool,
}

/// Every scratch package whose site still exists — one read for the whole page.
#[tauri::command]
pub async fn scratch_packages(state: State<'_, AppState>) -> Result<Vec<ScratchPackageView>> {
    let rows = {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        crate::state::store::all_scratch_packages(&conn)?
    };
    Ok(rows
        .into_iter()
        .map(|p| ScratchPackageView {
            source_missing: !std::path::Path::new(&p.source_path).is_dir(),
            site_id: p.site_id,
            slug: p.slug,
            kind: p.kind,
            source_path: p.source_path,
            synced_at: p.synced_at,
        })
        .collect())
}

/// Create a site (Phase 2 §1.6 + Phase 3 §1.2): provision it (docroot + cert + DB
/// row); for a **WordPress** site bring MySQL up and run the one-click installer
/// (`wp`) so the site is browsable; then — if the stack is running — ensure its
/// PHP version's pool is up and reload the edge so it serves immediately. `wp`
/// carries the dialog's WordPress fields (admin account, title, language) and is
/// ignored for non-WordPress sites. Returns the new site.
#[tauri::command]
pub async fn create_site<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    jobs: State<'_, super::site_provision::ProvisionJobs>,
    site: NewSite,
    wp: Option<core::wordpress::InstallOptions>,
    blueprint_id: Option<String>,
) -> Result<Site> {
    // Ownership is NOT a parameter of this command, and that is the point: the
    // IPC surface has no field that could set `origin='agent'`, so no caller —
    // UI, CLI, or anything that reaches the socket — can create a site that the
    // reaper is then allowed to delete unattended. The agent path calls
    // [`create_site_owned`] directly, inside the app.
    create_site_owned(app, &state, &jobs, site, wp, blueprint_id, core::sites::Ownership::User)
        .await
        .map_err(|f| f.error)
}

/// [`create_site`], with the ownership recorded on the new row (v27). Internal:
/// reachable from the MCP scratch tool and the IPC command above, never from IPC
/// input itself.
pub(crate) async fn create_site_owned<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: &AppState,
    jobs: &super::site_provision::ProvisionJobs,
    site: NewSite,
    wp: Option<core::wordpress::InstallOptions>,
    blueprint_id: Option<String>,
    ownership: core::sites::Ownership,
) -> std::result::Result<Site, CreateFailure> {
    // ONE execution path: this is a thin blocking wrapper over the streamed
    // provision job (`commands::site_provision`) that preserves the old
    // contract exactly — prepare-phase errors (invalid/duplicate domain,
    // declined resolver prompt) surface immediately with nothing created,
    // the await returns only when the site is fully provisioned (and served,
    // when the stack runs), and the fresh `Site` row comes back on success.
    // Nothing exists yet: a prepare-phase failure created no row (that is what
    // running prepare INLINE buys), so its `CreateFailure` carries no site id.
    let snap = super::site_provision::start(&app, state, jobs, site, wp, blueprint_id, ownership)
        .map_err(|error| CreateFailure { site_id: None, error })?;
    let settled = loop {
        let st = super::site_provision::state_of(jobs, &snap.id)
            .map_err(|error| CreateFailure { site_id: None, error })?;
        if st.status != "running" {
            break st;
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    };
    if settled.status == "ok" {
        let conn = state
            .db
            .lock()
            .map_err(|_| CreateFailure {
                site_id: settled.site_id.clone(),
                error: Error::Other("database lock poisoned".into()),
            })?;
        let id = settled.site_id.as_deref().unwrap_or_default();
        core::sites::get(&conn, id)
            .map_err(|error| CreateFailure { site_id: settled.site_id.clone(), error })?
            .ok_or_else(|| CreateFailure {
                site_id: settled.site_id.clone(),
                error: Error::Other(format!("created site vanished: {id}")),
            })
    } else {
        // Failure reply names the FAILING PHASE + points at the per-job log
        // (CLI callers can't stream the events — this line is their whole
        // diagnostic). The site row stays with provisioned=0: the reply says
        // so, since a CLI user has no badge in front of them.
        let phase = settled
            .phases
            .get(settled.phase_cursor)
            .map(|p| p.label.clone())
            .unwrap_or_else(|| "unknown step".into());
        let detail = settled.error.or(settled.summary).unwrap_or_else(|| settled.status.clone());
        let log = state
            .platform
            .paths()
            .log_dir()
            .map(|d| d.join(&settled.log_key).display().to_string())
            .unwrap_or_else(|_| settled.log_key.clone());
        // The row EXISTS from prepare onward, so this failure names it — the
        // caller (and the MCP tool's reply) must not read as "nothing happened"
        // about a site sitting in the user's list.
        Err(CreateFailure {
            site_id: settled.site_id.clone(),
            error: Error::Other(format!(
                "site create {} at \"{phase}\": {detail}\n  the site stays listed as \"setup incomplete\" — Retry it from the app, or delete it\n  full log: {log}",
                settled.status
            )),
        })
    }
}

/// Switch a site's web server (§4.1): update the DB row, then — if the stack is
/// running — bring the new backend up (and the old one down if unused), reload
/// the edge. No docroot/cert/DB rebuild. Returns the updated site.
#[tauri::command]
pub async fn set_site_web_server(
    state: State<'_, AppState>,
    tunnels: State<'_, crate::commands::tunnels::Tunnels>,
    id: String,
    server: WebServer,
) -> Result<Option<Site>> {
    // The user is changing this site's web server — that adopts it (promotion
    // choke point; a scratch site they touched is theirs).
    promote_if_scratch(&state, &id);
    // Lifetime guard (audit A1, 28 Jul 2026): step 3 made "safe to tunnel" ≡
    // "has an nginx vhost" — ONE fact — but a check at tunnel start is a
    // snapshot of a MUTABLE fact. Switching the server of a shared site
    // removes its nginx vhost while the live tunnel keeps pointing at nginx:
    // the public URL falls through to the DEFAULT vhost and publishes a
    // DIFFERENT site. The guard must hold for the tunnel's lifetime; never
    // auto-stop the share to make room.
    {
        let domain = {
            let conn = lock(&state)?;
            core::sites::get(&conn, &id)?.map(|s| s.domain)
        };
        if let Some(domain) = domain {
            crate::commands::tunnels::refuse_if_shared(
                &tunnels,
                &state,
                &domain,
                "switching its web server would remove it from the shared nginx the tunnel \
                 serves from, and the live link would start publishing whatever nginx's \
                 default site answers with — a DIFFERENT site",
            )?;
        }
    }
    let (site, sites, php_patches) = {
        let conn = lock(&state)?;
        let updated = core::sites::set_web_server(&conn, &id, server)?;
        (
            updated,
            core::sites::list(&conn)?,
            core::php::effective_patches(&conn)?,
        )
    };
    if let Some(ref s) = site {
        // The new backend's binary must be cached BEFORE the locked scope below
        // (pool ensure / override reconcile download otherwise). No-op when warm.
        let mut plan = if matches!(s.web_server, WebServer::Frankenphp) {
            core::downloads::plan_for_override(state.platform.as_ref(), s.web_server)
        } else {
            core::downloads::plan_for_pool_with(
                state.platform.as_ref(),
                &core::php::minor_of(&s.php_version),
                &php_patches,
            )
        };
        if matches!(s.web_server, WebServer::Apache) {
            plan.extend(core::downloads::plan_for_override(state.platform.as_ref(), s.web_server));
        }
        core::downloads::prefetch(state.platform.as_ref(), "Switch web server", &plan).await?;
        // Readiness of a newly spawned FrankenPHP backend is awaited with the
        // services lock released (M4).
        let checks = {
            let mut mgr = state.services.lock().await;
            if mgr.is_running() {
                // Switching to an nginx-served site needs its PHP version's pool up;
                // FrankenPHP uses its embedded PHP, so no pool is needed.
                if !matches!(s.web_server, WebServer::Frankenphp) {
                    let minor = core::php::minor_of(&s.php_version);
                    mgr.ensure_php_pool(state.platform.as_ref(), &minor).await?;
                }
                mgr.reload(state.platform.as_ref(), &state.ca, &sites, false).await?
            } else {
                Vec::new()
            }
        };
        core::service_manager::await_ready(checks).await?;
    }
    Ok(site)
}

/// Switch a site's PHP version (§1.4): update the DB row, then — if the stack is
/// running — ensure that version's php-fpm pool is up and reload nginx so the
/// switch takes effect. No docroot/cert/DB rebuild. Returns the updated site.
#[tauri::command]
pub async fn set_site_php_version(
    state: State<'_, AppState>,
    id: String,
    version: String,
) -> Result<Option<Site>> {
    // The user is changing this site's PHP version — that adopts it (promotion
    // choke point; a scratch site they touched is theirs). This is the ONE line
    // that differs from the agent's path, and it is policy, not mechanism —
    // see `switch_php_version`.
    promote_if_scratch(&state, &id);
    switch_php_version(state.inner(), &id, &version).await
}

/// Switch a site's PHP version — **the mechanism, with no ownership policy in
/// it**: row update, pinned-build prefetch, pool ensure (plus the debug pool if
/// the site uses Xdebug), reload, await.
///
/// **Why this is extracted rather than shared by calling the command.** The
/// obvious move — an agent tool calling `set_site_php_version` — is the
/// one-brain rule pointing at a trap. That command's FIRST act is
/// `promote_if_scratch`, because a USER changing PHP has adopted the site
/// (#214). Run it for an agent and switching PHP promotes the scratch site,
/// clears its expiry and frees a cap slot: switch, promote, create another,
/// repeat. A cap bypass with an entirely plausible face, arrived at by obeying
/// the reuse rule.
///
/// So the rule needs its sharper form: **reusing a command reuses its POLICY,
/// and a command's policy can be the wrong one for a different caller.** What
/// both callers must share is the mechanism; what they must not share is who
/// the site belongs to afterwards. The Tauri command wraps this with promotion
/// (user intent); the MCP tool wraps it with `claim` (#208) and re-asserts the
/// recorded origin, and #214's source guard still sees the command calling the
/// choke point.
pub(crate) async fn switch_php_version(
    state: &AppState,
    id: &str,
    version: &str,
) -> Result<Option<Site>> {
    let (site, sites, php_patches) = {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        let updated = core::sites::set_php_version(&conn, id, version)?;
        (
            updated,
            core::sites::list(&conn)?,
            core::php::effective_patches(&conn)?,
        )
    };
    if let Some(ref s) = site {
        let minor = core::php::minor_of(&s.php_version);
        // A toggled site also needs the NEW minor's debug pool (and its
        // xdebug.so) — pool_port_for_site routes it there after the reload.
        let needs_debug = s.xdebug && core::binaries::xdebug_supported(&minor);
        // Pool binary cached before the locked ensure below. No-op when warm.
        let plan = if needs_debug {
            core::downloads::plan_for_xdebug_with(state.platform.as_ref(), &minor, &php_patches)
        } else {
            core::downloads::plan_for_pool_with(state.platform.as_ref(), &minor, &php_patches)
        };
        core::downloads::prefetch(state.platform.as_ref(), "Switch PHP version", &plan).await?;
        let checks = {
            let mut mgr = state.services.lock().await;
            if mgr.is_running() {
                mgr.ensure_php_pool(state.platform.as_ref(), &minor).await?;
                if needs_debug {
                    mgr.ensure_php_debug_pool(state.platform.as_ref(), &minor).await?;
                }
                mgr.reload(state.platform.as_ref(), &state.ca, &sites, false).await?
            } else {
                Vec::new()
            }
        };
        core::service_manager::await_ready(checks).await?;
    }
    Ok(site)
}

/// Toggle a site's Xdebug (§8.2): core-validated flag flip (FrankenPHP and
/// PHP 8.0 refused with the real reason), then — if the stack is running —
/// ensure the minor's DEBUG pool (downloading the pinned xdebug.so on first
/// use, prefetched before the lock) and reload so nginx/Apache route the site
/// at the debug port. Toggling off routes back and stops the debug pool once
/// no site on that minor still uses it. Returns the updated site.
#[tauri::command]
pub async fn set_site_xdebug(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<Option<Site>> {
    // The user is changing this site's Xdebug — that adopts it (promotion
    // choke point; a scratch site they touched is theirs).
    promote_if_scratch(&state, &id);
    let (site, sites, php_patches) = {
        let conn = lock(&state)?;
        let updated = core::sites::set_xdebug(&conn, &id, enabled)?;
        (
            updated,
            core::sites::list(&conn)?,
            core::php::effective_patches(&conn)?,
        )
    };
    if let Some(ref s) = site {
        let minor = core::php::minor_of(&s.php_version);
        if enabled {
            // Pool binary + xdebug bundle cached BEFORE the locked ensure below.
            let plan =
                core::downloads::plan_for_xdebug_with(state.platform.as_ref(), &minor, &php_patches);
            core::downloads::prefetch(state.platform.as_ref(), "Enable Xdebug", &plan).await?;
        }
        let checks = {
            let mut mgr = state.services.lock().await;
            if mgr.is_running() {
                if enabled {
                    // Load-probe-gated: a bad artifact errors here, before any
                    // config routes the site at the debug port.
                    mgr.ensure_php_debug_pool(state.platform.as_ref(), &minor).await?;
                }
                let checks = mgr.reload(state.platform.as_ref(), &state.ca, &sites, false).await?;
                if !enabled
                    && !sites
                        .iter()
                        .any(|o| o.xdebug && core::php::minor_of(&o.php_version) == minor)
                {
                    // Tidy-up AFTER the reload: nothing routes there anymore.
                    mgr.stop_php_debug_pool(state.platform.as_ref(), &minor);
                }
                checks
            } else {
                Vec::new()
            }
        };
        core::service_manager::await_ready(checks).await?;
    }
    Ok(site)
}

/// Move a site's docroot into a user-picked PARENT directory (the folder keeps
/// its name). Ordered so the row NEVER points at a path that doesn't exist:
///   preflight (all rejections, no file touched) → files to the new location
///   (same-volume rename, else copy + verify with partial-copy cleanup) →
///   sites.path update → config regen + reload (nginx root; a FrankenPHP
///   override backend is restarted by the reconcile when its config changed;
///   Caddy holds no docroot) → delete the old tree LAST (copy case only,
///   best-effort). Not destructive: data is verified at the destination before
///   anything old is removed. Works with the stack down (configs load at the
///   next start). Returns the updated site.
#[tauri::command]
pub async fn move_site_docroot(
    state: State<'_, AppState>,
    tunnels: State<'_, crate::commands::tunnels::Tunnels>,
    id: String,
    dest_parent: String,
) -> Result<Site> {
    // The user is changing this site's docroot — that adopts it (promotion
    // choke point; a scratch site they touched is theirs).
    promote_if_scratch(&state, &id);
    let dest = std::path::PathBuf::from(&dest_parent);
    let (site, target) = {
        let conn = lock(&state)?;
        let site = core::sites::get(&conn, &id)?
            .ok_or_else(|| Error::Other(format!("site not found: {id}")))?;
        let target = core::sites::check_docroot_move(&site, &dest)?;
        (site, target)
    };
    // Lifetime guard (audit A2): a live tunnel depends on the docroot two
    // ways — nginx serves it, and the v23 row records it for mu-plugin
    // removal. Moving under a live share churns what the public link serves
    // mid-copy. (A share STARTED mid-move is handled by the row-docroot
    // update after the commit below — either half alone leaves a window.)
    crate::commands::tunnels::refuse_if_shared(
        &tunnels,
        &state,
        &site.domain,
        "moving its files would change what the live link serves mid-copy, and visitors \
         could hit a half-moved site",
    )?;
    let src = std::path::PathBuf::from(&site.path);

    // File work off the async runtime AND outside the DB lock — a cross-volume
    // copy of a big docroot must not stall status polls or the UI.
    let copied = {
        let (s, t) = (src.clone(), target.clone());
        super::wordpress::wp_blocking(move || core::sites::move_dir(&s, &t)).await?
    };

    // Files verifiably at the new location — only now flip the row.
    let (updated, sites) = {
        let conn = lock(&state)?;
        let updated = core::sites::set_path(&conn, state.platform.as_ref(), &id, &target)?
            .ok_or_else(|| Error::Other(format!("site not found: {id}")))?;
        // A share that STARTED during the copy (the refusal above ran before
        // the copy began) recorded the OLD docroot on its v23 row — re-point
        // it, or settle/exit/sweep remove the mu-plugin at a path that no
        // longer holds it and orphan the live-origin file at the new one.
        // Best-effort: a miss degrades to the pre-fix leftover, never worse.
        if let Err(e) =
            crate::state::store::set_tunnel_docroot(&conn, &updated.domain, &updated.path)
        {
            log::warn!("sites: could not re-point the tunnel record for {}: {e}", updated.domain);
        }
        (updated, core::sites::list(&conn)?)
    };

    // Regenerate + reload so nginx's root (and any FrankenPHP override) points
    // at the new path. Retriable: the row is already correct, so a later
    // reload/start also serves from the new location.
    let reload = async {
        let checks = {
            let mut mgr = state.services.lock().await;
            if mgr.is_running() {
                mgr.reload(state.platform.as_ref(), &state.ca, &sites, false).await?
            } else {
                Vec::new()
            }
        };
        core::service_manager::await_ready(checks).await
    };
    reload.await.map_err(|e| {
        Error::Other(format!(
            "Files moved to {}, but the config reload failed — the site may not serve \
             until services reload. Retry from Services → Restart. ({e})",
            target.display()
        ))
    })?;

    // Cross-volume copy: the old tree still exists — delete it LAST, after the
    // reload proved the new location serves. Best-effort: a failure leaves
    // duplicate files, never a broken site.
    if copied {
        let old = src.clone();
        let _ = super::wordpress::wp_blocking(move || {
            std::fs::remove_dir_all(&old).map_err(crate::error::Error::from)
        })
        .await;
    }

    Ok(updated)
}

/// Re-point a site at a docroot the USER moved themselves: `path` is the folder
/// itself, and rexenv touches no file — it records the new location, then
/// regenerates + reloads the config so the web server serves from there. This
/// is the path for a linked/imported folder (`docroot_managed = false`), which
/// `move_site_docroot` refuses to relocate because it isn't ours to copy and
/// delete. Retriable and non-destructive: the old folder (if it still exists)
/// is left exactly as it is. Returns the updated site.
#[tauri::command]
pub async fn relink_site_docroot(
    state: State<'_, AppState>,
    tunnels: State<'_, crate::commands::tunnels::Tunnels>,
    id: String,
    path: String,
) -> Result<Site> {
    // The user is changing this site's docroot — that adopts it (promotion
    // choke point; a scratch site they touched is theirs).
    promote_if_scratch(&state, &id);
    let dest = std::path::PathBuf::from(&path);
    let site = {
        let conn = lock(&state)?;
        let site = core::sites::get(&conn, &id)?
            .ok_or_else(|| Error::Other(format!("site not found: {id}")))?;
        core::sites::check_docroot_relink(&site, &dest)?;
        site
    };
    // Same lifetime guard as the move: a live tunnel serves THIS docroot and
    // its v23 row records it for mu-plugin removal, so swapping the folder
    // under a running share changes what the public link serves mid-flight.
    crate::commands::tunnels::refuse_if_shared(
        &tunnels,
        &state,
        &site.domain,
        "re-pointing it would change what the live link serves",
    )?;

    let (updated, sites) = {
        let conn = lock(&state)?;
        let updated = core::sites::set_path(&conn, state.platform.as_ref(), &id, &dest)?
            .ok_or_else(|| Error::Other(format!("site not found: {id}")))?;
        if let Err(e) =
            crate::state::store::set_tunnel_docroot(&conn, &updated.domain, &updated.path)
        {
            log::warn!("sites: could not re-point the tunnel record for {}: {e}", updated.domain);
        }
        (updated, core::sites::list(&conn)?)
    };

    // Regenerate + reload so nginx's root (and any FrankenPHP override) points
    // at the new path. Retriable: the row is already correct, so a later
    // reload/start also serves from the new location.
    let reload = async {
        let checks = {
            let mut mgr = state.services.lock().await;
            if mgr.is_running() {
                mgr.reload(state.platform.as_ref(), &state.ca, &sites, false).await?
            } else {
                Vec::new()
            }
        };
        core::service_manager::await_ready(checks).await
    };
    reload.await.map_err(|e| {
        Error::Other(format!(
            "The site now points at {path}, but the config reload failed — it may not serve \
             until services reload. Retry from Services → Restart. ({e})"
        ))
    })?;

    Ok(updated)
}

/// One env-var row as the UI sends it (name/value strings).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct EnvVarInput {
    pub name: String,
    pub value: String,
}

/// A site's per-request env vars, name-sorted (Phase 3 §1.6).
#[tauri::command]
pub fn list_site_env(state: State<'_, AppState>, id: String) -> Result<Vec<EnvVarInput>> {
    let conn = lock(&state)?;
    Ok(crate::state::store::get_site_env(&conn, &id)?
        .into_iter()
        .map(|(name, value)| EnvVarInput { name, value })
        .collect())
}

/// Replace a site's env vars (replace-all, like the PHP settings editor).
/// BACKEND validation is the enforcement (`core::site_env::validate_all` —
/// name shape, reserved names, unescapable characters); the UI mirror is only
/// for instant feedback. Then persist + swap the manager's map + regen/reload:
/// nginx picks up the new `fastcgi_param` lines; a FrankenPHP override whose
/// config changed is restarted by the reconcile diff. Non-destructive and
/// reversible (config text only); no-op beyond persisting when stopped.
#[tauri::command]
pub async fn set_site_env(
    state: State<'_, AppState>,
    id: String,
    vars: Vec<EnvVarInput>,
) -> Result<()> {
    // The user is changing this site's env vars — that adopts it (promotion
    // choke point; a scratch site they touched is theirs).
    promote_if_scratch(&state, &id);
    let pairs: Vec<(String, String)> =
        vars.into_iter().map(|v| (v.name.trim().to_string(), v.value)).collect();
    core::site_env::validate_all(&pairs)?;

    // Persist + snapshot under ONE brief DB lock, dropped before any await
    // (same shape as apply_php_settings).
    let (site_exists, sites, all) = {
        let conn = lock(&state)?;
        let exists = core::sites::get(&conn, &id)?.is_some();
        if exists {
            crate::state::store::replace_site_env(&conn, &id, &pairs)?;
        }
        (exists, core::sites::list(&conn)?, crate::state::store::all_site_env(&conn)?)
    };
    if !site_exists {
        return Err(Error::Other(format!("site not found: {id}")));
    }

    let checks = {
        let mut mgr = state.services.lock().await;
        mgr.apply_site_env(state.platform.as_ref(), &state.ca, &sites, all).await?
    };
    core::service_manager::await_ready(checks).await
}

/// Result of a domain change: the updated site, where the pre-change database
/// backup landed (WordPress sites only), and how many search-replace
/// substitutions ran (both passes; 0 for non-WordPress sites).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainChange {
    pub site: Site,
    pub backup_path: Option<String>,
    pub replacements: u64,
}

/// Change a site's domain (e.g. `myapp.test → myshop.test`). DESTRUCTIVE for
/// WordPress sites — the URL rewrite touches serialized data one-way, so the
/// database is ALWAYS exported to Downloads first and the whole flow aborts if
/// that export fails. Refused on multisite (see `core::sites::check_domain_change`).
///
/// Ordered so the one non-reversible step (search-replace) runs while everything
/// around it is still intact or trivially retriable:
///   preflight → backup → new cert (additive) → search-replace dry-run gate →
///   real search-replace (`https://old→https://new`, then bare `old→new`,
///   `--all-tables`) → SQLite domain flip (path + db_name untouched) →
///   config regen + forced edge reload → old-artifact cleanup (best-effort).
/// A failure before the SQLite flip leaves the site fully working on the old
/// domain (the DB restorable from the fresh backup); after the flip the only
/// remaining step that can fail is the reload, which is retriable.
///
/// DNS: the embedded resolver answers any name, so the only DNS step is
/// ensuring the new domain's TLD has its OS resolver file (first use of a TLD
/// = one privileged prompt; `.test` and previously used TLDs are no-ops). The
/// docroot folder and database name stay keyed to the old domain on purpose
/// (cosmetic; renaming either is risk for zero value). Bare-domain pass also
/// rewrites `…@old.test` email addresses — acceptable for local dev, stated in
/// the UI.
#[tauri::command]
pub async fn change_site_domain(
    state: State<'_, AppState>,
    tunnels: State<'_, crate::commands::tunnels::Tunnels>,
    id: String,
    domain: String,
) -> Result<DomainChange> {
    // The user is changing this site's domain — that adopts it (promotion
    // choke point; a scratch site they touched is theirs).
    promote_if_scratch(&state, &id);
    let domain = domain.trim().to_string();
    let site = {
        let conn = lock(&state)?;
        let site = core::sites::get(&conn, &id)?
            .ok_or_else(|| Error::Other(format!("site not found: {id}")))?;
        // Preflight BEFORE any destructive step; re-checked inside set_domain.
        core::sites::check_domain_change(&conn, &site, &domain)?;
        site
    };
    let old_domain = site.domain.clone();
    let is_wp = matches!(site.site_type, SiteType::Wordpress);

    // New TLD → its OS resolver file must exist, or the renamed site wouldn't
    // resolve. First use of a TLD = one privileged prompt; no-op otherwise.
    // BEFORE the backup/search-replace, so declining the prompt changes nothing.
    let new_tld = core::sites::domain_tld(&domain)?;
    core::dns::ensure_resolver(
        state.platform.as_ref(),
        &new_tld,
        core::dns::DEFAULT_DNS_PORT,
        // A user typing a new domain in the app: prompting is the point.
        core::dns::ResolverPrompt::Allow,
    )?;

    let mut backup_path = None;
    let mut replacements = 0u64;
    if is_wp {
        // Fail fast with an actionable message (same guard as export/reset).
        let engine = DbEngine::from_site(site.db_engine);
        if !engine.running() {
            return Err(Error::Other(format!(
                "{} isn't running — start it (Services → Start all, or the Databases page), then change the domain again.",
                engine.label()
            )));
        }
        let engine_version = super::database::effective_db_version(&state, engine)?;
        let (_, dump_bin) =
            engine.sql_client_bins(state.platform.as_ref(), &engine_version).await?;
        let (php, wp) = super::wordpress::wp_tools(&state, &site.php_version).await?;
        let docroot = std::path::PathBuf::from(&site.path);

        // 1) MANDATORY backup — the search-replace below isn't reversible.
        let dump = {
            let (old, db) = (old_domain.clone(), site.db_name.clone());
            super::wordpress::wp_blocking(move || {
                core::database::export_to_downloads(&dump_bin, engine.port(), &old, &db)
            })
            .await
            .map_err(|e| Error::Other(format!("domain unchanged — the safety backup failed: {e}")))?
        };
        backup_path = Some(dump.to_string_lossy().into_owned());

        // 2) Cert for the new domain — purely additive; the old cert keeps
        //    being served until the reload below.
        core::ssl::ensure_site_cert(
            state.platform.paths(),
            state.platform.permissions(),
            &state.ca,
            &domain,
        )?;

        // 3) URL migration. Dry-run first as an environment gate (wp-cli boots,
        //    DB reachable) — if it fails, nothing has been mutated. Then two
        //    real passes: full URL, then bare domain (srcset, protocol-relative
        //    and hardcoded refs). wp-cli handles serialized data; --all-tables
        //    covers non-prefix tables (each site owns its database).
        let (from_url, to_url) = (format!("https://{old_domain}"), format!("https://{domain}"));
        replacements = {
            let (old, new) = (old_domain.clone(), domain.clone());
            super::wordpress::wp_blocking(move || {
                core::wordpress::search_replace(&php, &wp, &docroot, &from_url, &to_url, true, true)?;
                let mut n =
                    core::wordpress::search_replace(&php, &wp, &docroot, &from_url, &to_url, false, true)?;
                n += core::wordpress::search_replace(&php, &wp, &docroot, &old, &new, false, true)?;
                Ok(n)
            })
            .await?
        };
    } else {
        // Non-WordPress sites store no URL — cert + configs are the whole change.
        core::ssl::ensure_site_cert(
            state.platform.paths(),
            state.platform.permissions(),
            &state.ca,
            &domain,
        )?;
    }

    // 4) Flip the row (domain only) — from here the configs regenerate to the
    //    new domain.
    let (updated, sites) = {
        let conn = lock(&state)?;
        let updated = core::sites::set_domain(&conn, &id, &domain)?
            .ok_or_else(|| Error::Other(format!("site not found: {id}")))?;
        (updated, core::sites::list(&conn)?)
    };

    // 5) Regenerate nginx/Caddy from the rows + FORCE-reload the edge (forced:
    //    if only cert bytes changed Caddy would skip a normal reload). No-op
    //    when the stack is down — the new configs load at the next start.
    //    Retriable: the row is already flipped, so a later reload also serves
    //    the new domain.
    let reload = async {
        let checks = {
            let mut mgr = state.services.lock().await;
            if mgr.is_running() {
                mgr.reload(state.platform.as_ref(), &state.ca, &sites, true).await?
            } else {
                Vec::new()
            }
        };
        core::service_manager::await_ready(checks).await
    };
    reload.await.map_err(|e| {
        Error::Other(format!(
            "Domain changed to {domain}, but the edge reload failed — the site may not \
             be reachable until services reload. Retry from Services → Restart. ({e})"
        ))
    })?;

    // 6) Old-domain artifacts (best-effort, non-fatal): a live tunnel proxies a
    //    vhost that no longer exists; cert dir / FrankenPHP config / logs are
    //    keyed by the old domain (mirrors teardown).
    tunnels.stop_for_domain(&state, &old_domain);
    // Both mu-plugins are stale after a rename (tunnel file: dead origin;
    // login file: OLD domain baked into its host allow-list) — remove them;
    // the next share / login recreates them with the new domain. The dir
    // stays (the site is still managed and will likely want it again).
    core::sites::cleanup_muplugin_artifacts(&updated, false);
    // ...except the loopback-DNS file, which the sweep just took: it bakes in
    // NOTHING per-site, and the site is still managed. Re-install now — without
    // this, a Change domain silently un-fixes WP-Cron until the next launch,
    // which is the exact silence this file exists to end.
    {
        let conn = state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))?;
        core::wp_dns::ensure_for_site(&conn, &updated);
    }
    if let Ok(dir) = core::ssl::site_cert_dir(state.platform.paths(), &old_domain) {
        let _ = std::fs::remove_dir_all(dir);
    }
    if let Ok(conf) = core::frankenphp::config_path(state.platform.as_ref(), &old_domain) {
        let _ = std::fs::remove_file(conf);
    }
    if let Ok(log) = core::frankenphp::log_path(state.platform.as_ref(), &old_domain) {
        let _ = std::fs::remove_file(log);
    }
    // Apache's pair was missing here for the same reason it was missing from
    // teardown: a hand-maintained list beside a name that promises all of them.
    if let Ok(conf) = core::apache::config_path(state.platform.as_ref(), &old_domain) {
        let _ = std::fs::remove_file(conf);
    }
    if let Ok(log) = core::apache::log_path(state.platform.as_ref(), &old_domain) {
        let _ = std::fs::remove_file(log);
    }
    if let Ok(log) = core::tunnels::log_path(state.platform.as_ref(), &old_domain) {
        let _ = std::fs::remove_file(log);
    }

    Ok(DomainChange { site: updated, backup_path, replacements })
}

/// May deleting `site` drop the database its row names? (v19.)
///
/// Two ways the answer is no, and the second one is why the column exists:
///
/// - `db_created == Some(false)` — the database import restored into a name that
///   ALREADY existed on our engine. That database is not ours, whatever the
///   site row says about it, and no path may drop it.
/// - a LINKED site that never imported one (`db_created` still NULL) — its
///   provision job skips the `configure` phase entirely, so the derived
///   `db_name` on the row names a database that was never created. Dropping it
///   is a no-op today, but reaching it means booting MySQL (and possibly
///   downloading it) to drop nothing.
///
/// Everything else keeps yesterday's behaviour: NULL on a rexenv-created site
/// means our own provisioning made it, and `Some(true)` means an import did.
fn may_drop_database(site: &crate::state::models::Site) -> bool {
    match site.db_created {
        Some(false) => false,
        Some(true) => true,
        None => site.docroot_managed != Some(false),
    }
}

/// Whether THIS site's delete drops `site.db_name` (settled 28 Jul 2026):
/// [`may_drop_database`]'s provenance rules, scoped by type. WordPress keeps
/// yesterday's behaviour (NULL = our own provisioning made it). Every other
/// type drops ONLY on the explicit provenance `Some(true)` — set by an import,
/// and (since the Laravel create flow shipped) by Laravel provisioning right
/// after it creates the database. NULL there still means "we never made one",
/// and the derived `db_name` could collide with a database the user made
/// themselves: dropping on NULL for those would be data loss wearing a
/// cleanup's clothes.
fn should_drop_database(site: &crate::state::models::Site) -> bool {
    match site.site_type {
        SiteType::Wordpress => may_drop_database(site),
        _ => site.db_created == Some(true),
    }
}

/// Delete a site — complete cleanup: stop its public tunnel, drop its MySQL
/// database, then tear down the DB row + cert + per-site configs/logs + docroot,
/// and reload the running stack so it stops being served. Returns whether it
/// existed. If the database drop fails the site is left intact (retryable) —
/// never a silently orphaned database.
#[tauri::command]
pub async fn delete_site(
    state: State<'_, AppState>,
    tunnels: State<'_, crate::commands::tunnels::Tunnels>,
    id: String,
) -> Result<bool> {
    delete_site_owned(&state, &tunnels, id).await
}

/// [`delete_site`] without the IPC wrappers, so in-app callers (the MCP scratch
/// tool, the reaper) run the SAME full teardown rather than a second one.
pub(crate) async fn delete_site_owned(
    state: &AppState,
    tunnels: &crate::commands::tunnels::Tunnels,
    id: String,
) -> Result<bool> {
    let site = {
        let conn = state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))?;
        core::sites::get(&conn, &id)?
    };
    let Some(site) = site else { return Ok(false) };

    // 1) A deleted site must not stay publicly shared: kill its live tunnel
    //    (registry keyed by domain). Its mu-plugins do NOT simply "go away
    //    with the docroot" — a linked docroot is preserved — so step 3 below
    //    removes them explicitly.
    tunnels.stop_for_domain(state, &site.domain);

    // 2) Drop the site's database, and any RECORDED mirrored user (D3). Only
    //    WordPress sites get a provisioned database (the stored
    //    `Site::db_name`, derived from the validated domain at creation —
    //    `drop_database` re-validates the name, so nothing else can be named).
    //    The mirrored user comes from `db_imports.mirrored_user` — the RECORD,
    //    never re-derived from the domain: a domain change between import and
    //    delete would compute a user we never created and leave the real one
    //    behind. Skipped entirely when the engine's datadir was never
    //    initialized (then neither the database nor the user can exist);
    //    otherwise the engine is brought up first, exactly like site creation.
    let mirrored_user = {
        let conn = state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))?;
        crate::state::store::get_db_import(&conn, &id)?.and_then(|r| r.mirrored_user)
    };
    // The agent principals this site's grants created (MCP M3). Read from the
    // RECORD, like the mirrored user above, and read BEFORE the row is deleted —
    // `agent_db_grants` cascades on the site, so after the delete there is
    // nothing left to tell us which accounts to drop.
    //
    // This matters for the same reason the mirrored drop does, stated in the
    // comment below: the name is DERIVED from the domain (`rex_ro_<slug>`), so a
    // future site created at that domain would inherit a stale account that
    // still holds SELECT on a database name that also collides by construction.
    // A grant the user gave once, to a site that no longer exists, would come
    // back attached to a different one.
    let (agent_users, had_grant): (Vec<String>, bool) = {
        let conn = state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))?;
        // BOTH sources, unioned, and neither is redundant:
        //
        //  - DERIVED from the site's current domain, both arms. This is the only
        //    way a SCRATCH principal is ever found: a scratch site needs no
        //    consent, so it has no grant row, and a delete that read only the
        //    grants left `rex_agent_<slug>` on the engine forever. Found by
        //    running the §M3 gate against the packaged app on 25 Aug 2026 — the
        //    site's database was dropped and its account was not.
        //  - RECORDED on the grants, because a site can be RENAMED: the account
        //    was created from the domain the site had at grant time, and
        //    deriving from today's domain would miss it. The recorded name is
        //    what actually exists on the engine.
        //
        // Dropping a name that does not exist is a no-op (`DROP USER IF
        // EXISTS`), so the union costs nothing and each source covers the other's
        // blind spot.
        let mut users: Vec<String> = vec![
            crate::core::agent_db::principal_name(
                crate::core::agent_db::Principal::Scratch,
                &site.domain,
            ),
            crate::core::agent_db::principal_name(
                crate::core::agent_db::Principal::ReadOnly,
                &site.domain,
            ),
        ];
        let recorded: Vec<String> = crate::state::store::list_agent_db_grants(&conn)?
            .into_iter()
            .filter(|g| g.site_id == id)
            .map(|g| g.db_user)
            .collect();
        let had_grant = !recorded.is_empty();
        users.extend(recorded);
        users.sort();
        users.dedup();
        (users, had_grant)
    };
    let engine = DbEngine::from_site(site.db_engine);
    let engine_version = super::database::effective_db_version(state, engine)?;
    let want_db_drop = should_drop_database(&site);
    // Booting a STOPPED engine just to drop one account is deliberate
    // (settled 28 Jul 2026), not the linked-site over-fetch mistake
    // repeating: the drop genuinely runs, and skipping it is not harmless —
    // the account holds GRANT ALL on a database NAME, and names collide by
    // construction (wp_<slug>), so a future site under that name would
    // inherit a stale account with an old password over it.
    // `datadir_initialized` already skips fresh installs, where neither the
    // database nor the user can exist.
    // `agent_users` is never empty now (the two derived names are always there),
    // so it cannot gate this branch — `had_grant` does, and it is a separate
    // flag rather than `!agent_users.is_empty()` for exactly that reason. It
    // matters for a site with a grant but no database of ours to drop (a linked
    // site): without it the branch would not run and the account would stay.
    if (want_db_drop || mirrored_user.is_some() || had_grant)
        && engine.datadir_initialized(state.platform.as_ref(), &engine_version)
    {
        // Engine binaries cached before the locked spawn below (an initialized
        // datadir with an evicted binary cache would otherwise download under
        // the lock).
        let plan =
            core::downloads::plan_for_engine(state.platform.as_ref(), engine, &engine_version);
        core::downloads::prefetch(state.platform.as_ref(), "Delete site", &plan).await?;
        let check = {
            let mut mgr = state.services.lock().await;
            mgr.set_db_version(engine, &engine_version);
            mgr.spawn_db(state.platform.as_ref(), engine).await?
        };
        core::service_manager::await_ready(check.into_iter().collect()).await?;
        let (db_client, _) =
            engine.sql_client_bins(state.platform.as_ref(), &engine_version).await?;
        if want_db_drop {
            core::database::drop_database(&db_client, engine.port(), &site.db_name)?;
        }
        if let Some(user) = &mirrored_user {
            core::dbmirror::drop_mirrored(&db_client, engine.port(), user)?;
        }
        for user in &agent_users {
            core::agent_db::deprovision(&db_client, engine.port(), user)?;
        }
    }

    // 3) Row + cert + per-site configs/logs + docroot (the last only if it's
    //    ours — a linked folder is never touched).
    let (outcome, sites) = {
        let conn = state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))?;
        let outcome = core::sites::teardown(&conn, state.platform.as_ref(), &id)?;
        (outcome, core::sites::list(&conn)?)
    };
    if outcome.existed && !outcome.docroot_removed {
        log::info!(
            "sites: deleted {} — its folder ({}) was left in place (not ours to remove)",
            site.domain,
            site.path
        );
        // The folder stays, our files inside it must not (linked-repo lens):
        // tunnel + login mu-plugins, and the mu-plugins dir itself when it is
        // RECORDED as ours and empty again.
        core::sites::cleanup_muplugin_artifacts(&site, true);
    }
    // Best-effort reload (no-op if services aren't running). A delete only
    // REMOVES backends, so there are no readiness probes to await.
    let mut mgr = state.services.lock().await;
    let _ = mgr.reload(state.platform.as_ref(), &state.ca, &sites, false).await;
    Ok(outcome.existed)
}

/// The pool bounce a single-site restart must never do on its own.
#[cfg(test)]
mod a_site_restart_never_bounces_a_shared_pool_uninvited {
    /// `restart_pools_for` stops the php-fpm pool for a PHP MINOR, which every
    /// site on that minor is served by. A `rex site restart one.rex` that took
    /// it would stop other people's sites — on a machine where the whole point
    /// of the shared pool is that it is shared — and the user who typed one
    /// domain would have no way to know. So the call must sit behind the opt-in
    /// flag, and the guard is on the SHAPE, because "we only call it when the
    /// flag is set" is exactly the kind of claim that stays true until someone
    /// hoists the call for tidiness.
    #[test]
    fn the_pool_restart_is_reachable_only_through_the_opt_in_flag() {
        let src = crate::core::copy_scan::production_source(include_str!("sites.rs"));
        let body = src
            .split("pub async fn restart_site(")
            .nth(1)
            .and_then(|b| b.split("\n/// ").next())
            .expect("restart_site");
        let call = body
            .find("restart_pools_for(")
            .expect("restart_site no longer bounces the pool at all — if the flag was dropped, \
                     drop this guard with it; if it moved, move the guard");
        let gate = body
            .find("if pool {")
            .expect("`restart_site` no longer gates the pool restart on the caller's flag");
        assert!(
            gate < call,
            "the pool restart is no longer inside the `if pool` branch: a single-site restart \
             would stop every other site on that PHP minor, and the person who typed one \
             domain would never know it happened"
        );
        // The count must be reported whether or not the flag was passed — the
        // number is what makes the flag an informed choice rather than a dare.
        assert!(
            body.contains("sites_on_pool"),
            "the report no longer carries how many sites share the pool, so `--pool` is a \
             flag with no stated cost"
        );
    }
}

/// #188 — **a share guard must hold for the tunnel's LIFETIME, not just at the
/// moment the share starts.**
///
/// "Safe to tunnel" is one fact (step 3: the site has an nginx vhost, at a
/// docroot, under a domain), and every fact in it is MUTABLE while the share is
/// live. Switching a shared site's web server removes its nginx vhost while
/// cloudflared keeps pointing at nginx — the public link then falls through to
/// nginx's DEFAULT vhost and publishes a DIFFERENT site (#13 measured that
/// fallthrough live). Moving or re-pointing the docroot changes what the link
/// serves mid-copy. Renaming or deleting the site leaves a public URL for
/// something that no longer exists at that name.
///
/// A check at start is a SNAPSHOT of a mutable fact — the bug class this repo
/// has paid for twice in a week — so each of those commands carries the guard
/// itself: refuse while shared (the user stops the share; we never auto-stop
/// one to make room), or, where the operation is the end of the site,
/// deliberately STOP the share as part of it.
///
/// This is a surface guard rather than a spot check, because the way it breaks
/// is a NEW command that mutates one of those facts and simply never learns
/// about tunnels — which no test of the five existing call sites can see.
#[cfg(test)]
mod share_guards_hold_for_the_tunnels_lifetime {
    /// Core mutations a live share depends on, and what a public visitor gets
    /// if one happens under it. Keyed by the core call, so the guard is about
    /// what a command DOES, not what it is called.
    const TUNNEL_INVALIDATING: &[(&str, &str)] = &[
        ("core::sites::set_web_server(", "the vhost the tunnel serves from disappears"),
        ("core::sites::check_docroot_move(", "the link serves a half-moved docroot"),
        ("core::sites::check_docroot_relink(", "the link starts serving a different folder"),
        ("core::sites::set_domain(", "the link points at a vhost name that no longer exists"),
        ("core::sites::teardown(", "the link outlives the site it published"),
    ];
    /// The two acceptable answers: refuse while shared, or end the share as
    /// part of the operation.
    const GUARDS: &[&str] = &["refuse_if_shared(", "stop_for_domain("];

    /// Every function in this module that performs one of those mutations must
    /// carry a guard — and each mutation must still be performed SOMEWHERE, so
    /// a renamed core function fails here instead of quietly emptying the list.
    #[test]
    fn every_command_that_invalidates_a_live_share_carries_a_guard() {
        let src = crate::core::copy_scan::production_source(include_str!("sites.rs"));
        // Split into function bodies: `pub (async) fn name(` … next top-level fn.
        let mut bodies: Vec<(String, String)> = Vec::new();
        for (i, chunk) in src.split("\npub ").enumerate().skip(1) {
            let Some(sig_end) = chunk.find('(') else { continue };
            let sig = &chunk[..sig_end];
            let Some(name) = sig.split_whitespace().last() else { continue };
            let _ = i;
            bodies.push((name.to_string(), chunk.to_string()));
        }
        assert!(bodies.len() > 10, "the function split found almost nothing — guard is vacuous");

        for (call, cost) in TUNNEL_INVALIDATING {
            let callers: Vec<&(String, String)> =
                bodies.iter().filter(|(_, b)| b.contains(call)).collect();
            assert!(
                !callers.is_empty(),
                "nothing in commands/sites.rs calls `{call}` any more. If it was renamed, rename \
                 it here too — an entry matching nothing turns this guard green while the \
                 mutation it watches goes unguarded"
            );
            for (name, body) in callers {
                assert!(
                    GUARDS.iter().any(|g| body.contains(g)),
                    "`{name}` calls `{call}` with no share guard. Under a live tunnel, {cost} — \
                     and a check at share START cannot help, because this is the mutation that \
                     happens afterwards. Add `refuse_if_shared` (never auto-stop someone's \
                     share to make room), or `stop_for_domain` if the operation ends the site"
                );
            }
        }
    }

    // **The wiring half is NOT tested here, and that is a measurement rather
    // than an omission.** A command that guards must take the registry, and the
    // planted removal of `tunnels: State<'_, …Tunnels>` from `relink_site_docroot`
    // does not compile (`cannot find value `tunnels``, plus the arity error at
    // the guard call). A test asserting it could never be the first thing to
    // fail, and a proof at a layer that cannot see its subject is a plan wearing
    // a test's clothes (#40/#166).
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::models::{
        MultisiteMode, ServiceStatus, Site, SiteDbEngine, SiteOrigin, SiteType, WebServer,
    };

    fn site(docroot_managed: Option<bool>, db_created: Option<bool>) -> Site {
        Site {
            id: "s1".into(),
            name: "S".into(),
            domain: "s.rex".into(),
            site_type: SiteType::Wordpress,
            status: ServiceStatus::Stopped,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            ssl: true,
            path: "/tmp/s".into(),
            created_at: "2026-07-26 00:00:00".into(),
            multisite: MultisiteMode::None,
            db_name: "wp_s_rex".into(),
            db_engine: SiteDbEngine::Mysql,
            xdebug: false,
            override_port: None,
            provisioned: true,
            docroot_managed,
            db_created,
            content_dir: None,
            mu_dir_created: None,
            origin: SiteOrigin::User,
            agent_client: None,
            expires_at: None,
            docroot_subdir: String::new(),
            git_url: None,
            git_ref: None,
            git_migrate: None,
            git_build_assets: None,
            starter_db: None,
        }
    }

    #[test]
    fn a_pre_existing_database_is_never_dropped() {
        // The whole reason v19 exists: the import restored into a name that was
        // already on our engine. No combination of the other flags may make it
        // droppable.
        for docroot in [None, Some(true), Some(false)] {
            assert!(!may_drop_database(&site(docroot, Some(false))));
        }
    }

    #[test]
    fn our_own_databases_stay_droppable() {
        // Legacy rows (NULL) on a site we created keep yesterday's behaviour,
        // and a database this import created is ours to drop.
        assert!(may_drop_database(&site(None, None)));
        assert!(may_drop_database(&site(Some(true), None)));
        assert!(may_drop_database(&site(Some(true), Some(true))));
        assert!(may_drop_database(&site(Some(false), Some(true))));
    }

    #[test]
    fn a_linked_site_with_no_import_has_no_database_to_drop() {
        // Its provision job skips `configure` entirely, so the derived db_name
        // on the row names a database that was never created — reaching it only
        // boots (or downloads) MySQL to drop nothing.
        assert!(!may_drop_database(&site(Some(false), None)));
    }

    #[test]
    fn non_wordpress_null_rows_never_drop_a_database_we_never_made() {
        // THE silent-regression branch (settled 28 Jul 2026): Laravel/PHP
        // provisioning never creates a database, so NULL means "we never made
        // one" — the derived db_name could be the USER'S OWN database, and a
        // NULL-row drop would be data loss, not cleanup. Explicit import
        // provenance (Some(true)) is the only thing that drops.
        let laravel = |managed, created| {
            let mut s = site(managed, created);
            s.site_type = SiteType::Laravel;
            s
        };
        for managed in [None, Some(true), Some(false)] {
            assert!(!should_drop_database(&laravel(managed, None)), "{managed:?}: NULL dropped");
            assert!(!should_drop_database(&laravel(managed, Some(false))));
            assert!(should_drop_database(&laravel(managed, Some(true))));
        }
        // WordPress keeps yesterday's behaviour, NULL included.
        assert!(should_drop_database(&site(Some(true), None)));
        assert!(should_drop_database(&site(Some(true), Some(true))));
        assert!(!should_drop_database(&site(Some(true), Some(false))));
    }

    /// Every user-facing site-mutation COMMAND must promote a scratch site it
    /// touches. A list of four was how §4.3 put it; a list is what gets stale,
    /// so this scans the source for the commands and asserts each one calls the
    /// choke point. A NEW mutation command added later fails here rather than
    /// silently leaving the reaper able to take a site the user just changed.
    #[test]
    fn the_extracted_mechanism_carries_no_ownership_policy() {
        // The other half of #214's guard, and the reason this extraction exists.
        //
        // `set_site_php_version` MUST promote (the guard above proves it does).
        // `switch_php_version` — the mechanism both callers share — must NOT,
        // because the agent's path runs through it. If promotion leaked down
        // here, an agent switching PHP would adopt the scratch site: expiry
        // cleared, a cap slot freed, and the loop switch → promote → create
        // another is a cap bypass with an entirely plausible face. It would
        // arrive by OBEYING the reuse rule, which is what makes it worth a
        // guard rather than a comment.
        //
        // Source-scanned, like the guard above, because the property is "this
        // function does not call that one" — there is no value to assert.
        const SRC: &str = include_str!("sites.rs");
        let body = SRC
            .split("pub(crate) async fn switch_php_version(")
            .nth(1)
            .expect("switch_php_version must exist — the agent's PHP switch shares it");
        // Its body ends at the next top-level item.
        let body = body.split("\n/// ").next().unwrap_or(body);
        assert!(
            !body.contains("promote_if_scratch("),
            "switch_php_version calls promote_if_scratch. It is the MECHANISM, shared with the \
             agent's tool — promotion is the USER-intent policy and belongs in the command that \
             wraps it. Leaving it here makes an agent's PHP switch adopt the scratch site, which \
             frees a cap slot and turns switch/create into an unbounded loop."
        );
        assert!(
            !body.contains("origin") && !body.contains("expires_at"),
            "switch_php_version writes ownership state directly — Keep is ONE write (#213)"
        );
    }

    #[test]
    fn every_user_facing_site_mutation_promotes_through_the_one_choke_point() {
        const SRC: &str = include_str!("sites.rs");
        // The commands a USER drives to change one site. Not `delete_site` (the
        // site is going anyway), not `create_site` (nothing to adopt), not the
        // agent's own tools (an agent promoting its sites would be a cap bypass).
        const MUST_PROMOTE: &[&str] = &[
            "pub async fn set_site_web_server(",
            "pub async fn set_site_php_version(",
            "pub async fn set_site_xdebug(",
            "pub async fn move_site_docroot(",
            "pub async fn relink_site_docroot(",
            "pub async fn set_site_env(",
            "pub async fn change_site_domain(",
        ];
        for head in MUST_PROMOTE {
            let at = SRC.find(head).unwrap_or_else(|| panic!("command not found: {head}"));
            let body = &SRC[at..(at + 900).min(SRC.len())];
            assert!(
                body.contains("promote_if_scratch("),
                "`{head}` changes a site on the user's behalf but does not call \
                 promote_if_scratch. A scratch site the user deliberately changed is THEIRS — \
                 without this the reaper can delete a site they just renamed, moved or \
                 reconfigured. Add the call at the top; do not write `origin`/`expires_at` \
                 yourself (Keep is one write, on purpose)."
            );
        }
    }

    #[test]
    fn re_pointing_a_site_records_the_new_path_and_touches_no_file() {
        const SRC: &str = include_str!("sites.rs");
        // The whole promise of the re-point path — the confirm dialog says "no
        // file is copied, moved or deleted", and it is the ONLY reason we let a
        // linked/imported folder (the USER's own project) be relocated at all
        // where `move_site_docroot` is refused. A future edit that reaches for
        // the filesystem here would break that silently: the user is not
        // consenting to a copy, only to a record change.
        let at = SRC.find("pub async fn relink_site_docroot(").expect("command not found");
        let end = SRC[at..].find("\n}\n").expect("unterminated fn") + at;
        let body = &SRC[at..end];
        for banned in
            ["std::fs::", "move_dir(", "remove_dir_all(", "copy_dir_recursive(", "fs::rename("]
        {
            assert!(
                !body.contains(banned),
                "relink_site_docroot calls `{banned}` — re-pointing must only RECORD where the \
                 user already moved their files. Anything that writes, copies or deletes belongs \
                 in move_site_docroot, behind its own consent + verify + delete-last ordering."
            );
        }
        // ...and it does record + serve from there: preflight, path write, reload.
        for required in ["check_docroot_relink(", "core::sites::set_path(", "mgr.reload("] {
            assert!(body.contains(required), "relink_site_docroot no longer calls `{required}`");
        }
    }

    /// What the choke point does to a real scratch row — the EFFECT half.
    ///
    /// Honest scope: this does NOT drive `change_site_domain` end to end (that
    /// needs a full platform + Tunnels fixture), so it cannot catch a dropped
    /// call on its own. The dropped-call half is
    /// `every_user_facing_site_mutation_promotes_through_the_one_choke_point`,
    /// which fails if any command stops calling it. Named for what it proves.
    #[test]
    fn promotion_turns_a_scratch_row_into_the_users_in_one_write() {
        use crate::state::models::{test_site, SiteOrigin};
        use crate::state::store;
        let conn = crate::state::db::open_in_memory().unwrap();
        let mut row = test_site("c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24", "probe.scratch.rex", SiteOrigin::Agent);
        row.expires_at = Some("2099-01-01 00:00:00".into());
        store::insert_site(&conn, &row).unwrap();
        assert!(store::get_site(&conn, &row.id).unwrap().unwrap().is_scratch());

        crate::state::store::keep_site(&conn, &row.id).unwrap();
        let after = store::get_site(&conn, &row.id).unwrap().unwrap();
        assert_eq!(after.origin, SiteOrigin::User, "the user changed it — it is theirs");
        assert_eq!(after.expires_at, None, "and the clock is gone, in the same write");
        assert!(!after.reap_due("2099-01-01 00:00:00"));
    }

}

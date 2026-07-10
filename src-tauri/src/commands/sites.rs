//! commands::sites — thin Tauri IPC handlers for sites. Call `core/` only.

use crate::core;
use crate::core::db::DbEngine;
use crate::core::{binaries, php};
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::models::{NewSite, Site, SiteServing, SiteType, WebServer};
use std::path::Path;
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

    // DB sizes: one query — only when MySQL is actually up, and only resolved
    // from the already-extracted tree (never a download from a status poll).
    let db_sizes: std::collections::HashMap<String, u64> = if DbEngine::Mysql.running() {
        state
            .platform
            .paths()
            .bin_dir()
            .ok()
            .map(|b| b.join(format!("mysql-{}", binaries::MYSQL_VERSION)))
            .filter(|base| base.join("bin").is_dir())
            .and_then(|base| core::database::db_sizes(&base, DbEngine::Mysql.port()).ok())
            .unwrap_or_default()
            .into_iter()
            .collect()
    } else {
        Default::default()
    };

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
                dedicated: matches!(s.web_server, WebServer::Frankenphp),
                cpu_percent: tree.map(|t| t.cpu_percent),
                ram_mb: tree.map(|t| t.ram_mb),
                requests_per_min: act.map(|a| a.requests),
                bytes_per_min: act.map(|a| a.bytes),
                db_size_bytes: db_sizes.get(&core::wordpress::db_name_for(&s.domain)).copied(),
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

/// Create a site (Phase 2 §1.6 + Phase 3 §1.2): provision it (docroot + cert + DB
/// row); for a **WordPress** site bring MySQL up and run the one-click installer
/// (`wp`) so the site is browsable; then — if the stack is running — ensure its
/// PHP version's pool is up and reload the edge so it serves immediately. `wp`
/// carries the dialog's WordPress fields (admin account, title, language) and is
/// ignored for non-WordPress sites. Returns the new site.
#[tauri::command]
pub async fn create_site(
    state: State<'_, AppState>,
    site: NewSite,
    wp: Option<core::wordpress::InstallOptions>,
    blueprint_id: Option<String>,
) -> Result<Site> {
    let (created, mut sites, blueprint) = {
        let conn = lock(&state)?;
        let created = core::sites::provision(&conn, state.platform.as_ref(), &state.ca, site)?;
        // Resolve the blueprint up front (so we don't hold the lock across awaits).
        let blueprint = match &blueprint_id {
            Some(id) if !id.is_empty() => crate::state::store::get_blueprint(&conn, id)?,
            _ => None,
        };
        (created, core::sites::list(&conn)?, blueprint)
    };
    let minor = php::minor_of(&created.php_version);

    // Prefetch everything this create could need BEFORE any services-lock scope
    // below — `spawn_db` / `ensure_php_pool` / the reload's override reconcile
    // all run under the lock and must hit cache, or a cold cache would stream
    // downloads while holding it. No-op (no batch) when everything's cached.
    let mut plan = if matches!(created.web_server, WebServer::Frankenphp) {
        core::downloads::plan_for_override(state.platform.as_ref())
    } else {
        core::downloads::plan_for_pool(state.platform.as_ref(), &minor)
    };
    if matches!(created.site_type, SiteType::Wordpress) {
        plan.extend(core::downloads::plan_for_engine(state.platform.as_ref(), DbEngine::Mysql));
        plan.extend(core::downloads::plan_for_wp_tooling(state.platform.as_ref(), &minor));
    }
    core::downloads::prefetch(state.platform.as_ref(), "Create site", &plan).await?;

    // WordPress needs a database + a one-click install before it's browsable.
    // The services lock is held only to SPAWN MySQL; the readiness wait and the
    // (long) installer run with it released (M4), so other commands and status
    // stay responsive during a site create.
    if matches!(created.site_type, SiteType::Wordpress) {
        let check = {
            let mut mgr = state.services.lock().await;
            mgr.spawn_db(state.platform.as_ref(), DbEngine::Mysql).await?
        };
        core::service_manager::await_ready(check.into_iter().collect()).await?;
        let patch = php::patch_for_minor(&minor)
            .ok_or_else(|| Error::Other(format!("no pinned PHP build for {minor}")))?;
        let php_bin = binaries::resolve(state.platform.as_ref(), "php", patch).await?;
        let wp_phar =
            binaries::resolve_file(state.platform.as_ref(), "wp-cli", binaries::WP_CLI_VERSION)
                .await?;
        let db_host = format!("127.0.0.1:{}", DbEngine::Mysql.port());
        let mysql_base =
            binaries::resolve_dir(state.platform.as_ref(), "mysql", binaries::MYSQL_VERSION)
                .await?;
        let docroot = Path::new(&created.path);
        core::wordpress::install_for_site(
            &php_bin,
            &wp_phar,
            docroot,
            &created.domain,
            &created.name,
            &db_host,
            &mysql_base,
            &wp.unwrap_or_default(),
        )?;

        // Apply a blueprint (§11.3): install/activate its plugins + themes, set
        // WP_DEBUG, and convert to multisite — the reusable "site setup" automation.
        if let Some(bp) = &blueprint {
            core::blueprints::apply_wordpress(&php_bin, &wp_phar, docroot, &bp.spec)?;
            if !matches!(bp.spec.multisite, crate::state::models::MultisiteMode::None) {
                let conn = lock(&state)?;
                core::sites::convert_multisite(
                    &conn, &php_bin, &wp_phar, docroot, &created.id, bp.spec.multisite,
                )?;
                // Refresh so the reload below serves the new multisite rewrite.
                sites = core::sites::list(&conn)?;
            }
        }
    }

    let checks = {
        let mut mgr = state.services.lock().await;
        if mgr.is_running() {
            if !matches!(created.web_server, WebServer::Frankenphp) {
                mgr.ensure_php_pool(state.platform.as_ref(), &minor).await?;
            }
            mgr.reload(state.platform.as_ref(), &state.ca, &sites, false).await?
        } else {
            Vec::new()
        }
    };
    core::service_manager::await_ready(checks).await?;
    Ok(created)
}

/// Switch a site's web server (§4.1): update the DB row, then — if the stack is
/// running — bring the new backend up (and the old one down if unused), reload
/// the edge. No docroot/cert/DB rebuild. Returns the updated site.
#[tauri::command]
pub async fn set_site_web_server(
    state: State<'_, AppState>,
    id: String,
    server: WebServer,
) -> Result<Option<Site>> {
    let (site, sites) = {
        let conn = lock(&state)?;
        let updated = core::sites::set_web_server(&conn, &id, server)?;
        (updated, core::sites::list(&conn)?)
    };
    if let Some(ref s) = site {
        // The new backend's binary must be cached BEFORE the locked scope below
        // (pool ensure / override reconcile download otherwise). No-op when warm.
        let plan = if matches!(s.web_server, WebServer::Frankenphp) {
            core::downloads::plan_for_override(state.platform.as_ref())
        } else {
            core::downloads::plan_for_pool(
                state.platform.as_ref(),
                &core::php::minor_of(&s.php_version),
            )
        };
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
    let (site, sites) = {
        let conn = lock(&state)?;
        let updated = core::sites::set_php_version(&conn, &id, &version)?;
        (updated, core::sites::list(&conn)?)
    };
    if let Some(ref s) = site {
        let minor = core::php::minor_of(&s.php_version);
        // Pool binary cached before the locked ensure below. No-op when warm.
        let plan = core::downloads::plan_for_pool(state.platform.as_ref(), &minor);
        core::downloads::prefetch(state.platform.as_ref(), "Switch PHP version", &plan).await?;
        let checks = {
            let mut mgr = state.services.lock().await;
            if mgr.is_running() {
                mgr.ensure_php_pool(state.platform.as_ref(), &minor).await?;
                mgr.reload(state.platform.as_ref(), &state.ca, &sites, false).await?
            } else {
                Vec::new()
            }
        };
        core::service_manager::await_ready(checks).await?;
    }
    Ok(site)
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
    let site = {
        let conn = lock(&state)?;
        core::sites::get(&conn, &id)?
    };
    let Some(site) = site else { return Ok(false) };

    // 1) A deleted site must not stay publicly shared: kill its live tunnel
    //    (registry keyed by domain; the mu-plugin goes away with the docroot).
    tunnels.stop_for_domain(state.platform.as_ref(), &site.domain);

    // 2) Drop the site's database. Only WordPress sites get one (`wp_<domain>`,
    //    derived from the validated stored domain — `drop_database` re-validates
    //    the name, so nothing else can be named). No per-site DB user exists to
    //    remove (local-dev connects as passwordless root). Skipped entirely when
    //    the MySQL datadir was never initialized (then no database can exist);
    //    otherwise MySQL is brought up first, exactly like site creation does.
    if matches!(site.site_type, SiteType::Wordpress)
        && core::database::is_initialized(&core::database::data_dir(state.platform.as_ref())?)
    {
        // MySQL tree cached before the locked spawn below (an initialized datadir
        // with an evicted binary cache would otherwise download under the lock).
        let plan = core::downloads::plan_for_engine(state.platform.as_ref(), DbEngine::Mysql);
        core::downloads::prefetch(state.platform.as_ref(), "Delete site", &plan).await?;
        let check = {
            let mut mgr = state.services.lock().await;
            mgr.spawn_db(state.platform.as_ref(), DbEngine::Mysql).await?
        };
        core::service_manager::await_ready(check.into_iter().collect()).await?;
        let mysql_base =
            binaries::resolve_dir(state.platform.as_ref(), "mysql", binaries::MYSQL_VERSION)
                .await?;
        core::database::drop_database(
            &mysql_base,
            DbEngine::Mysql.port(),
            &core::wordpress::db_name_for(&site.domain),
        )?;
    }

    // 3) Row + cert + per-site configs/logs + docroot.
    let (removed, sites) = {
        let conn = lock(&state)?;
        let removed = core::sites::teardown(&conn, state.platform.as_ref(), &id)?;
        (removed, core::sites::list(&conn)?)
    };
    // Best-effort reload (no-op if services aren't running). A delete only
    // REMOVES backends, so there are no readiness probes to await.
    let mut mgr = state.services.lock().await;
    let _ = mgr.reload(state.platform.as_ref(), &state.ca, &sites, false).await;
    Ok(removed)
}

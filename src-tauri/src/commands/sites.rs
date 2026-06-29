//! commands::sites — thin Tauri IPC handlers for sites. Call `core/` only.

use crate::core;
use crate::core::db::DbEngine;
use crate::core::{binaries, php};
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::models::{NewSite, ServiceStatus, Site, SiteType, WebServer};
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

/// Mark a site running; returns the updated site.
#[tauri::command]
pub fn start_site(state: State<'_, AppState>, id: String) -> Result<Option<Site>> {
    let conn = lock(&state)?;
    core::sites::set_status(&conn, &id, ServiceStatus::Running)
}

/// Mark a site stopped; returns the updated site.
#[tauri::command]
pub fn stop_site(state: State<'_, AppState>, id: String) -> Result<Option<Site>> {
    let conn = lock(&state)?;
    core::sites::set_status(&conn, &id, ServiceStatus::Stopped)
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
    let mut mgr = state.services.lock().await;

    // WordPress needs a database + a one-click install before it's browsable.
    if matches!(created.site_type, SiteType::Wordpress) {
        mgr.ensure_db(state.platform.as_ref(), DbEngine::Mysql).await?;
        let patch = php::patch_for_minor(&minor)
            .ok_or_else(|| Error::Other(format!("no pinned PHP build for {minor}")))?;
        let php_bin = binaries::resolve(state.platform.as_ref(), "php", patch).await?;
        let wp_phar =
            binaries::resolve_file(state.platform.as_ref(), "wp-cli", binaries::WP_CLI_VERSION)
                .await?;
        let db_host = format!("127.0.0.1:{}", DbEngine::Mysql.port());
        let docroot = Path::new(&created.path);
        core::wordpress::install_for_site(
            &php_bin,
            &wp_phar,
            docroot,
            &created.domain,
            &created.name,
            &db_host,
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

    if mgr.is_running() {
        if !matches!(created.web_server, WebServer::Frankenphp) {
            mgr.ensure_php_pool(state.platform.as_ref(), &minor).await?;
        }
        mgr.reload(state.platform.as_ref(), &state.ca, &sites).await?;
    }
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
        let mut mgr = state.services.lock().await;
        if mgr.is_running() {
            // Switching to an nginx-served site needs its PHP version's pool up;
            // FrankenPHP uses its embedded PHP, so no pool is needed.
            if !matches!(s.web_server, WebServer::Frankenphp) {
                let minor = core::php::minor_of(&s.php_version);
                mgr.ensure_php_pool(state.platform.as_ref(), &minor).await?;
            }
            mgr.reload(state.platform.as_ref(), &state.ca, &sites).await?;
        }
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
        let mut mgr = state.services.lock().await;
        if mgr.is_running() {
            mgr.ensure_php_pool(state.platform.as_ref(), &minor).await?;
            mgr.reload(state.platform.as_ref(), &state.ca, &sites).await?;
        }
    }
    Ok(site)
}

/// Delete a site: remove its DB row, cert, and docroot, then reload the running
/// stack so it stops being served. Returns whether it existed.
#[tauri::command]
pub async fn delete_site(state: State<'_, AppState>, id: String) -> Result<bool> {
    let (removed, sites) = {
        let conn = lock(&state)?;
        let removed = core::sites::teardown(&conn, state.platform.as_ref(), &id)?;
        (removed, core::sites::list(&conn)?)
    };
    // Best-effort reload (no-op if services aren't running).
    let mut mgr = state.services.lock().await;
    let _ = mgr.reload(state.platform.as_ref(), &state.ca, &sites).await;
    Ok(removed)
}

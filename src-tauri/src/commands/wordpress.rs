//! commands::wordpress — thin Tauri IPC for WordPress detection/management
//! (Phase 3 §1). Resolves the bundled PHP + wp-cli phar and delegates to
//! `core::wordpress`. No business logic here.

use crate::core::wordpress::{WpInfo, WpNetworkSite, WpPlugin, WpTheme, WpUser};
use crate::core::{self, binaries, php};
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::models::{MultisiteMode, Site};
use std::path::PathBuf;
use tauri::State;

/// Run a blocking WP-CLI call off the async runtime. Every call spawns PHP and
/// boots WordPress (hundreds of ms; installs/updates take seconds), and
/// `Command::output()` blocks — the WordPress tab fires several of these at
/// once, which used to tie up tokio worker threads and stall the whole app.
async fn wp_blocking<T, F>(f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| Error::Other(format!("wp-cli task failed: {e}")))?
}

/// Resolve (downloading on first use) the bundled PHP CLI for a site's PHP minor
/// version + the wp-cli `.phar`.
async fn wp_tools(state: &State<'_, AppState>, php_minor: &str) -> Result<(PathBuf, PathBuf)> {
    let patch = php::patch_for_minor(php_minor)
        .ok_or_else(|| Error::Other(format!("no pinned PHP build for {php_minor}")))?;
    let php_bin = binaries::resolve(state.platform.as_ref(), "php", patch).await?;
    // wp-cli is a .phar (not a Mach-O) → resolve_file (no chmod/codesign).
    let wp_phar =
        binaries::resolve_file(state.platform.as_ref(), "wp-cli", binaries::WP_CLI_VERSION).await?;
    Ok((php_bin, wp_phar))
}

/// Detect whether a site runs WordPress, plus its core version + multisite flag.
/// A non-WordPress docroot (e.g. a Blank-PHP site) returns `isWordpress: false`.
#[tauri::command]
pub async fn wp_info(state: State<'_, AppState>, id: String) -> Result<WpInfo> {
    let site = {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        core::sites::get(&conn, &id)?.ok_or_else(|| Error::Other(format!("no site {id}")))?
    };
    let (php_bin, wp_phar) = wp_tools(&state, &site.php_version).await?;
    let docroot = PathBuf::from(site.path);
    wp_blocking(move || core::wordpress::wp_info(&php_bin, &wp_phar, &docroot)).await
}

/// Resolve a site's docroot + its bundled PHP/WP-CLI tools (for the WP manager).
async fn site_tools(
    state: &State<'_, AppState>,
    id: &str,
) -> Result<(PathBuf, PathBuf, PathBuf)> {
    let site = {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        core::sites::get(&conn, id)?.ok_or_else(|| Error::Other(format!("no site {id}")))?
    };
    let (php_bin, wp_phar) = wp_tools(state, &site.php_version).await?;
    Ok((PathBuf::from(site.path), php_bin, wp_phar))
}

/// List the site's plugins (`wp plugin list`). `checkUpdates` opts into the
/// api.wordpress.org update check (slow / offline-hostile) — the UI lists fast
/// without it, then refreshes update badges in a background query.
#[tauri::command]
pub async fn wp_plugins(
    state: State<'_, AppState>,
    id: String,
    check_updates: Option<bool>,
) -> Result<Vec<WpPlugin>> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || {
        core::wordpress::plugin_list(&php, &wp, &docroot, check_updates.unwrap_or(false))
    })
    .await
}

/// Install a plugin by slug (optionally activating it).
#[tauri::command]
pub async fn wp_plugin_install(
    state: State<'_, AppState>,
    id: String,
    slug: String,
    activate: bool,
) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || {
        core::wordpress::plugin_install(&php, &wp, &docroot, &slug, activate).map(|_| ())
    })
    .await
}

/// Activate one or more plugins.
#[tauri::command]
pub async fn wp_plugin_activate(state: State<'_, AppState>, id: String, names: Vec<String>) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::plugin_activate(&php, &wp, &docroot, &names).map(|_| ())).await
}

/// Deactivate one or more plugins.
#[tauri::command]
pub async fn wp_plugin_deactivate(state: State<'_, AppState>, id: String, names: Vec<String>) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::plugin_deactivate(&php, &wp, &docroot, &names).map(|_| ())).await
}

/// Update one or more plugins.
#[tauri::command]
pub async fn wp_plugin_update(state: State<'_, AppState>, id: String, names: Vec<String>) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::plugin_update(&php, &wp, &docroot, &names).map(|_| ())).await
}

/// Delete one or more plugins.
#[tauri::command]
pub async fn wp_plugin_delete(state: State<'_, AppState>, id: String, names: Vec<String>) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::plugin_delete(&php, &wp, &docroot, &names).map(|_| ())).await
}

/// List the site's themes (`wp theme list`), each with its screenshot as a
/// `data:` URL. `checkUpdates` as in [`wp_plugins`].
#[tauri::command]
pub async fn wp_themes(
    state: State<'_, AppState>,
    id: String,
    check_updates: Option<bool>,
) -> Result<Vec<WpTheme>> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || {
        core::wordpress::theme_list(&php, &wp, &docroot, check_updates.unwrap_or(false))
    })
    .await
}

/// Install a theme by slug (optionally activating it).
#[tauri::command]
pub async fn wp_theme_install(
    state: State<'_, AppState>,
    id: String,
    slug: String,
    activate: bool,
) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || {
        core::wordpress::theme_install(&php, &wp, &docroot, &slug, activate).map(|_| ())
    })
    .await
}

/// Activate a theme (only one can be live).
#[tauri::command]
pub async fn wp_theme_activate(state: State<'_, AppState>, id: String, name: String) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::theme_activate(&php, &wp, &docroot, &name).map(|_| ())).await
}

/// Update one or more themes.
#[tauri::command]
pub async fn wp_theme_update(state: State<'_, AppState>, id: String, names: Vec<String>) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::theme_update(&php, &wp, &docroot, &names).map(|_| ())).await
}

/// Delete one or more themes (not the active one).
#[tauri::command]
pub async fn wp_theme_delete(state: State<'_, AppState>, id: String, names: Vec<String>) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::theme_delete(&php, &wp, &docroot, &names).map(|_| ())).await
}

/// List the site's WordPress users (`wp user list`).
#[tauri::command]
pub async fn wp_users(state: State<'_, AppState>, id: String) -> Result<Vec<WpUser>> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::user_list(&php, &wp, &docroot)).await
}

/// Create a WordPress user (`wp user create`); WP-CLI generates the password.
#[tauri::command]
pub async fn wp_user_create(
    state: State<'_, AppState>,
    id: String,
    login: String,
    email: String,
    role: String,
) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || {
        core::wordpress::user_create(&php, &wp, &docroot, &login, &email, &role).map(|_| ())
    })
    .await
}

/// The site's PRIMARY administrator id (lowest-ID admin). The UI locks this
/// user's role control — core::user_set_role refuses it regardless.
#[tauri::command]
pub async fn wp_primary_admin(state: State<'_, AppState>, id: String) -> Result<u64> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::primary_admin_id(&php, &wp, &docroot)).await
}

/// Change a user's role (whitelisted stock roles; the primary administrator is
/// refused in core).
#[tauri::command]
pub async fn wp_user_set_role(
    state: State<'_, AppState>,
    id: String,
    user_id: u64,
    role: String,
) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || {
        core::wordpress::user_set_role(&php, &wp, &docroot, user_id, &role).map(|_| ())
    })
    .await
}

/// Issue a one-time "Log in as" URL for `userId`: a single-use, short-TTL,
/// loopback-only magic link the UI opens in the browser (§7.1).
#[tauri::command]
pub async fn wp_user_login_url(state: State<'_, AppState>, id: String, user_id: u64) -> Result<String> {
    let site = {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        core::sites::get(&conn, &id)?.ok_or_else(|| Error::Other(format!("no site {id}")))?
    };
    let (php_bin, wp_phar) = wp_tools(&state, &site.php_version).await?;
    let docroot = PathBuf::from(&site.path);
    let token = wp_blocking(move || {
        core::wp_login::issue(&php_bin, &wp_phar, &docroot, user_id, core::wp_login::LOGIN_TTL_SECS)
    })
    .await?;
    Ok(format!(
        "https://{}/?rexenv_login={}&rexenv_user={}",
        site.domain, token, user_id
    ))
}

/// One-click "Open admin": issue a magic login URL for the site's PRIMARY
/// administrator (lowest-ID admin — the install's original account). Same
/// hardened single-use / short-TTL / loopback-only token as `wp_user_login_url`
/// (§7.1); the mu-plugin lands the browser on `/wp-admin/`. Scoped to managed
/// sites by construction: the site row must exist in OUR database, and the
/// token is planted via WP-CLI in that site's own docroot.
#[tauri::command]
pub async fn wp_admin_login_url(state: State<'_, AppState>, id: String) -> Result<String> {
    let site = {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        core::sites::get(&conn, &id)?.ok_or_else(|| Error::Other(format!("no site {id}")))?
    };
    let (php_bin, wp_phar) = wp_tools(&state, &site.php_version).await?;
    let docroot = PathBuf::from(&site.path);
    let (admin_id, token) = wp_blocking(move || {
        let admin_id = core::wordpress::primary_admin_id(&php_bin, &wp_phar, &docroot)?;
        let token = core::wp_login::issue(
            &php_bin,
            &wp_phar,
            &docroot,
            admin_id,
            core::wp_login::LOGIN_TTL_SECS,
        )?;
        Ok((admin_id, token))
    })
    .await?;
    Ok(format!(
        "https://{}/?rexenv_login={}&rexenv_user={}",
        site.domain, token, admin_id
    ))
}

/// Whether WP_DEBUG is on for the site.
#[tauri::command]
pub async fn wp_debug_get(state: State<'_, AppState>, id: String) -> Result<bool> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::wp_debug_get(&php, &wp, &docroot)).await
}

/// Toggle WP_DEBUG for the site.
#[tauri::command]
pub async fn wp_debug_set(state: State<'_, AppState>, id: String, on: bool) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::wp_debug_set(&php, &wp, &docroot, on).map(|_| ())).await
}

/// Read one whitelisted boolean wp-config debug constant
/// (WP_DEBUG_LOG / WP_DEBUG_DISPLAY / SCRIPT_DEBUG).
#[tauri::command]
pub async fn wp_debug_flag_get(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<bool> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::config_flag_get(&php, &wp, &docroot, &name)).await
}

/// Set one whitelisted boolean wp-config debug constant.
#[tauri::command]
pub async fn wp_debug_flag_set(
    state: State<'_, AppState>,
    id: String,
    name: String,
    on: bool,
) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || {
        core::wordpress::config_flag_set(&php, &wp, &docroot, &name, on).map(|_| ())
    })
    .await
}

/// Whether maintenance mode is active for the site.
#[tauri::command]
pub async fn wp_maintenance_get(state: State<'_, AppState>, id: String) -> Result<bool> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::maintenance_mode_get(&php, &wp, &docroot)).await
}

/// Toggle maintenance mode for the site (visitors see WordPress's
/// "briefly unavailable" page while it's on).
#[tauri::command]
pub async fn wp_maintenance_set(state: State<'_, AppState>, id: String, on: bool) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || {
        core::wordpress::maintenance_mode_set(&php, &wp, &docroot, on).map(|_| ())
    })
    .await
}

/// Search-replace across the DB; `dryRun` reports the count without changing data.
/// Returns the number of replacements.
#[tauri::command]
pub async fn wp_search_replace(
    state: State<'_, AppState>,
    id: String,
    from: String,
    to: String,
    dry_run: bool,
) -> Result<u64> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::search_replace(&php, &wp, &docroot, &from, &to, dry_run))
        .await
}

/// The site's current permalink structure (`""` = Plain).
#[tauri::command]
pub async fn wp_permalink_get(state: State<'_, AppState>, id: String) -> Result<String> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::permalink_structure_get(&php, &wp, &docroot)).await
}

/// Set the permalink structure (whitelisted presets only) + flush rewrites.
#[tauri::command]
pub async fn wp_permalink_set(
    state: State<'_, AppState>,
    id: String,
    structure: String,
) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || {
        core::wordpress::permalink_structure_set(&php, &wp, &docroot, &structure).map(|_| ())
    })
    .await
}

/// List the site's scheduled cron events (soonest first).
#[tauri::command]
pub async fn wp_cron_events(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<core::wordpress::WpCronEvent>> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::cron_event_list(&php, &wp, &docroot)).await
}

/// Run all currently-due cron events. Returns WP-CLI's summary message.
#[tauri::command]
pub async fn wp_cron_run_due(state: State<'_, AppState>, id: String) -> Result<String> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::cron_run_due(&php, &wp, &docroot)).await
}

/// Run one hook's scheduled event(s) immediately, due or not.
#[tauri::command]
pub async fn wp_cron_run_hook(
    state: State<'_, AppState>,
    id: String,
    hook: String,
) -> Result<String> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::cron_run_hook(&php, &wp, &docroot, &hook)).await
}

/// Verify core files against wordpress.org checksums. `ok: false` + per-file
/// warnings is a normal result, not an error.
#[tauri::command]
pub async fn wp_core_verify_checksums(
    state: State<'_, AppState>,
    id: String,
) -> Result<core::wordpress::WpChecksumReport> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::core_verify_checksums(&php, &wp, &docroot)).await
}

/// Flush the object cache. Returns WP-CLI's confirmation message.
#[tauri::command]
pub async fn wp_cache_flush(state: State<'_, AppState>, id: String) -> Result<String> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::cache_flush(&php, &wp, &docroot)).await
}

/// Delete all transients. Returns WP-CLI's "N transients deleted" message.
#[tauri::command]
pub async fn wp_transient_delete_all(state: State<'_, AppState>, id: String) -> Result<String> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::transient_delete_all(&php, &wp, &docroot)).await
}

/// Regenerate permalinks (`wp rewrite flush`).
#[tauri::command]
pub async fn wp_rewrite_flush(state: State<'_, AppState>, id: String) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::rewrite_flush(&php, &wp, &docroot).map(|_| ())).await
}

/// Update WordPress core to the latest release. Returns WP-CLI's output.
#[tauri::command]
pub async fn wp_core_update(state: State<'_, AppState>, id: String) -> Result<String> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::core_update(&php, &wp, &docroot)).await
}

/// Re-download core files of the current version. Returns WP-CLI's output.
#[tauri::command]
pub async fn wp_core_reinstall(state: State<'_, AppState>, id: String) -> Result<String> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::core_reinstall(&php, &wp, &docroot)).await
}

/// Export the site's database to the user's Downloads folder
/// (`<domain>-db.sql`, numbered on collision). Uses the bundled `mysqldump`
/// directly — WP-CLI's `wp db export` shells out to a PATH `mysqldump` a
/// Finder-launched app doesn't have (see `core::database`). Returns the
/// written path for the success toast.
#[tauri::command]
pub async fn wp_db_export(state: State<'_, AppState>, id: String) -> Result<String> {
    use crate::core::db::DbEngine;
    let site = {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        core::sites::get(&conn, &id)?.ok_or_else(|| Error::Other(format!("no site {id}")))?
    };
    // Fail fast with an actionable message — a stopped server would otherwise
    // surface as mysqldump's opaque "Can't connect" error.
    if !DbEngine::Mysql.running() {
        return Err(Error::Other(
            "MySQL isn't running — start it (Services → Start all, or the Databases page), then export again.".into(),
        ));
    }
    let mysql_base =
        binaries::resolve_dir(state.platform.as_ref(), "mysql", binaries::MYSQL_VERSION).await?;
    wp_blocking(move || {
        core::database::export_to_downloads(
            &mysql_base,
            DbEngine::Mysql.port(),
            &site.domain,
            &core::wordpress::db_name_for(&site.domain),
        )
        .map(|p| p.to_string_lossy().into_owned())
    })
    .await
}

/// Import a `.sql` dump into the site's database (DESTRUCTIVE — the dump's
/// tables overwrite existing ones; the UI gates this behind a typed confirm +
/// backup-first offer). Bundled `mysql` client over stdin, same PATH rationale
/// as export. Fails fast when MySQL is down or the file isn't a `.sql`.
#[tauri::command]
pub async fn wp_db_import(state: State<'_, AppState>, id: String, path: String) -> Result<()> {
    use crate::core::db::DbEngine;
    let site = {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        core::sites::get(&conn, &id)?.ok_or_else(|| Error::Other(format!("no site {id}")))?
    };
    let file = std::path::PathBuf::from(&path);
    if !file
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("sql"))
    {
        return Err(Error::Other(format!(
            "{path} is not a .sql file — pick a SQL dump (e.g. one made by Export database)."
        )));
    }
    if !DbEngine::Mysql.running() {
        return Err(Error::Other(
            "MySQL isn't running — start it (Services → Start all, or the Databases page), then import again.".into(),
        ));
    }
    let mysql_base =
        binaries::resolve_dir(state.platform.as_ref(), "mysql", binaries::MYSQL_VERSION).await?;
    wp_blocking(move || {
        core::database::import_from_file(
            &mysql_base,
            DbEngine::Mysql.port(),
            &core::wordpress::db_name_for(&site.domain),
            &file,
        )
    })
    .await
}

/// Reset a WordPress site to a clean **single-site** install: drop + recreate
/// its database and re-run the installer with the default local-dev
/// credentials (admin / admin) — files stay on disk. A multisite site is
/// flipped back to single-site — constants cleared by the core reset, row
/// updated here, configs regenerated/reloaded (its rewrite rules are
/// multisite-specific). Fails fast when MySQL is down.
#[tauri::command]
pub async fn wp_site_reset(state: State<'_, AppState>, id: String) -> Result<()> {
    use crate::core::db::DbEngine;
    let site = {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        core::sites::get(&conn, &id)?.ok_or_else(|| Error::Other(format!("no site {id}")))?
    };
    // Fail fast with an actionable message (same guard as the DB export).
    if !DbEngine::Mysql.running() {
        return Err(Error::Other(
            "MySQL isn't running — start it (Services → Start all, or the Databases page), then reset again.".into(),
        ));
    }
    let mysql_base =
        binaries::resolve_dir(state.platform.as_ref(), "mysql", binaries::MYSQL_VERSION).await?;
    let (php, wp) = wp_tools(&state, &site.php_version).await?;
    let was_multisite = !matches!(site.multisite, MultisiteMode::None);
    let (docroot, domain, name) =
        (PathBuf::from(&site.path), site.domain.clone(), site.name.clone());
    wp_blocking(move || {
        core::wordpress::reset_site(&php, &wp, &docroot, &domain, &name, &mysql_base)
    })
    .await?;
    if was_multisite {
        // Back to single-site: flip the row, then regenerate + reload configs
        // (multisite rewrite rules differ) — same flow as wp_multisite_convert.
        let sites = {
            let conn = state
                .db
                .lock()
                .map_err(|_| Error::Other("database lock poisoned".into()))?;
            core::sites::clear_multisite(&conn, &id)?;
            core::sites::list(&conn)?
        };
        let checks = {
            let mut mgr = state.services.lock().await;
            if mgr.is_running() {
                mgr.reload(state.platform.as_ref(), &state.ca, &sites).await?
            } else {
                Vec::new()
            }
        };
        core::service_manager::await_ready(checks).await?;
    }
    Ok(())
}

/// Whether the site still accepts the default admin / admin credentials —
/// backs the tunnel-share warning (public URL + default creds = open
/// wp-admin). Any failure (no admin user, broken/non-WP site) reads `false`.
#[tauri::command]
pub async fn wp_default_creds(state: State<'_, AppState>, id: String) -> Result<bool> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || Ok(core::wordpress::default_creds_active(&php, &wp, &docroot))).await
}

// ── Network / multisite management (§10.3) ───────────────────────────────────

/// List the network's sub-sites (`wp site list`). Multisite-only.
#[tauri::command]
pub async fn wp_network_sites(state: State<'_, AppState>, id: String) -> Result<Vec<WpNetworkSite>> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::network_site_list(&php, &wp, &docroot)).await
}

/// Create a sub-site by slug (`wp site create --slug=`).
#[tauri::command]
pub async fn wp_network_site_create(state: State<'_, AppState>, id: String, slug: String) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || {
        core::wordpress::network_site_create(&php, &wp, &docroot, &slug).map(|_| ())
    })
    .await
}

/// Delete a sub-site by `blogId` (`wp site delete`). The main site can't be deleted.
#[tauri::command]
pub async fn wp_network_site_delete(state: State<'_, AppState>, id: String, blog_id: String) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || {
        core::wordpress::network_site_delete(&php, &wp, &docroot, &blog_id).map(|_| ())
    })
    .await
}

/// Network-activate one or more plugins (`wp plugin activate … --network`).
#[tauri::command]
pub async fn wp_plugin_activate_network(state: State<'_, AppState>, id: String, names: Vec<String>) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || {
        core::wordpress::plugin_activate_network(&php, &wp, &docroot, &names).map(|_| ())
    })
    .await
}

/// Network-deactivate one or more plugins (`wp plugin deactivate … --network`).
#[tauri::command]
pub async fn wp_plugin_deactivate_network(state: State<'_, AppState>, id: String, names: Vec<String>) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || {
        core::wordpress::plugin_deactivate_network(&php, &wp, &docroot, &names).map(|_| ())
    })
    .await
}

/// Network-enable a theme (`wp theme enable <name> --network`).
#[tauri::command]
pub async fn wp_theme_enable_network(state: State<'_, AppState>, id: String, name: String) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || {
        core::wordpress::theme_enable_network(&php, &wp, &docroot, &name).map(|_| ())
    })
    .await
}

/// Network-disable a theme (`wp theme disable <name> --network`).
#[tauri::command]
pub async fn wp_theme_disable_network(state: State<'_, AppState>, id: String, name: String) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || {
        core::wordpress::theme_disable_network(&php, &wp, &docroot, &name).map(|_| ())
    })
    .await
}

/// List the network's super-admins (`wp super-admin list`).
#[tauri::command]
pub async fn wp_super_admins(state: State<'_, AppState>, id: String) -> Result<Vec<String>> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::super_admin_list(&php, &wp, &docroot)).await
}

/// Grant super-admin to a user (`wp super-admin add <user>`).
#[tauri::command]
pub async fn wp_super_admin_add(state: State<'_, AppState>, id: String, user: String) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::super_admin_add(&php, &wp, &docroot, &user).map(|_| ())).await
}

/// Convert a WordPress site to multisite (`subdomain` | `subdirectory`): writes
/// the network constants, persists the mode, and — if the stack is running —
/// reloads the edge so nginx serves with the matching rewrite template (§10.1).
/// (Not `wp_blocking`-wrapped: the WP-CLI convert runs under the SQLite lock in
/// `core::sites::convert_multisite`, which can't move onto a blocking thread.)
#[tauri::command]
pub async fn wp_multisite_convert(
    state: State<'_, AppState>,
    id: String,
    mode: String,
) -> Result<Option<Site>> {
    let mode = MultisiteMode::parse_db(&mode)?;
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    let (site, sites) = {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        let updated = core::sites::convert_multisite(&conn, &php, &wp, &docroot, &id, mode)?;
        (updated, core::sites::list(&conn)?)
    };
    if site.is_some() {
        // Await any backend readiness with the services lock released (M4).
        let checks = {
            let mut mgr = state.services.lock().await;
            if mgr.is_running() {
                mgr.reload(state.platform.as_ref(), &state.ca, &sites).await?
            } else {
                Vec::new()
            }
        };
        core::service_manager::await_ready(checks).await?;
    }
    Ok(site)
}

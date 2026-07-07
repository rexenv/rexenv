//! commands::wordpress — thin Tauri IPC for WordPress detection/management
//! (Phase 3 §1). Resolves the bundled PHP + wp-cli phar and delegates to
//! `core::wordpress`. No business logic here.

use crate::core::wordpress::{WpInfo, WpNetworkSite, WpPlugin, WpTheme, WpUser};
use crate::core::{self, binaries, php};
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::models::{MultisiteMode, Site};
use std::path::{Path, PathBuf};
use tauri::State;

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
    core::wordpress::wp_info(&php_bin, &wp_phar, Path::new(&site.path))
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

/// List the site's plugins (`wp plugin list`).
#[tauri::command]
pub async fn wp_plugins(state: State<'_, AppState>, id: String) -> Result<Vec<WpPlugin>> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    core::wordpress::plugin_list(&php, &wp, &docroot)
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
    core::wordpress::plugin_install(&php, &wp, &docroot, &slug, activate).map(|_| ())
}

/// Activate one or more plugins.
#[tauri::command]
pub async fn wp_plugin_activate(state: State<'_, AppState>, id: String, names: Vec<String>) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    core::wordpress::plugin_activate(&php, &wp, &docroot, &names).map(|_| ())
}

/// Deactivate one or more plugins.
#[tauri::command]
pub async fn wp_plugin_deactivate(state: State<'_, AppState>, id: String, names: Vec<String>) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    core::wordpress::plugin_deactivate(&php, &wp, &docroot, &names).map(|_| ())
}

/// Update one or more plugins.
#[tauri::command]
pub async fn wp_plugin_update(state: State<'_, AppState>, id: String, names: Vec<String>) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    core::wordpress::plugin_update(&php, &wp, &docroot, &names).map(|_| ())
}

/// Delete one or more plugins.
#[tauri::command]
pub async fn wp_plugin_delete(state: State<'_, AppState>, id: String, names: Vec<String>) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    core::wordpress::plugin_delete(&php, &wp, &docroot, &names).map(|_| ())
}

/// List the site's themes (`wp theme list`).
#[tauri::command]
pub async fn wp_themes(state: State<'_, AppState>, id: String) -> Result<Vec<WpTheme>> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    core::wordpress::theme_list(&php, &wp, &docroot)
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
    core::wordpress::theme_install(&php, &wp, &docroot, &slug, activate).map(|_| ())
}

/// Activate a theme (only one can be live).
#[tauri::command]
pub async fn wp_theme_activate(state: State<'_, AppState>, id: String, name: String) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    core::wordpress::theme_activate(&php, &wp, &docroot, &name).map(|_| ())
}

/// Update one or more themes.
#[tauri::command]
pub async fn wp_theme_update(state: State<'_, AppState>, id: String, names: Vec<String>) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    core::wordpress::theme_update(&php, &wp, &docroot, &names).map(|_| ())
}

/// Delete one or more themes (not the active one).
#[tauri::command]
pub async fn wp_theme_delete(state: State<'_, AppState>, id: String, names: Vec<String>) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    core::wordpress::theme_delete(&php, &wp, &docroot, &names).map(|_| ())
}

/// List the site's WordPress users (`wp user list`).
#[tauri::command]
pub async fn wp_users(state: State<'_, AppState>, id: String) -> Result<Vec<WpUser>> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    core::wordpress::user_list(&php, &wp, &docroot)
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
    core::wordpress::user_create(&php, &wp, &docroot, &login, &email, &role).map(|_| ())
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
    let token = core::wp_login::issue(
        &php_bin,
        &wp_phar,
        Path::new(&site.path),
        user_id,
        core::wp_login::LOGIN_TTL_SECS,
    )?;
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
    let docroot = Path::new(&site.path);
    let admin_id = core::wordpress::primary_admin_id(&php_bin, &wp_phar, docroot)?;
    let token = core::wp_login::issue(
        &php_bin,
        &wp_phar,
        docroot,
        admin_id,
        core::wp_login::LOGIN_TTL_SECS,
    )?;
    Ok(format!(
        "https://{}/?rexenv_login={}&rexenv_user={}",
        site.domain, token, admin_id
    ))
}

/// Whether WP_DEBUG is on for the site.
#[tauri::command]
pub async fn wp_debug_get(state: State<'_, AppState>, id: String) -> Result<bool> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    core::wordpress::wp_debug_get(&php, &wp, &docroot)
}

/// Toggle WP_DEBUG for the site.
#[tauri::command]
pub async fn wp_debug_set(state: State<'_, AppState>, id: String, on: bool) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    core::wordpress::wp_debug_set(&php, &wp, &docroot, on).map(|_| ())
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
    core::wordpress::search_replace(&php, &wp, &docroot, &from, &to, dry_run)
}

/// Regenerate permalinks (`wp rewrite flush`).
#[tauri::command]
pub async fn wp_rewrite_flush(state: State<'_, AppState>, id: String) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    core::wordpress::rewrite_flush(&php, &wp, &docroot).map(|_| ())
}

/// Update WordPress core to the latest release. Returns WP-CLI's output.
#[tauri::command]
pub async fn wp_core_update(state: State<'_, AppState>, id: String) -> Result<String> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    core::wordpress::core_update(&php, &wp, &docroot)
}

/// Re-download core files of the current version. Returns WP-CLI's output.
#[tauri::command]
pub async fn wp_core_reinstall(state: State<'_, AppState>, id: String) -> Result<String> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    core::wordpress::core_reinstall(&php, &wp, &docroot)
}

// ── Network / multisite management (§10.3) ───────────────────────────────────

/// List the network's sub-sites (`wp site list`). Multisite-only.
#[tauri::command]
pub async fn wp_network_sites(state: State<'_, AppState>, id: String) -> Result<Vec<WpNetworkSite>> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    core::wordpress::network_site_list(&php, &wp, &docroot)
}

/// Create a sub-site by slug (`wp site create --slug=`).
#[tauri::command]
pub async fn wp_network_site_create(state: State<'_, AppState>, id: String, slug: String) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    core::wordpress::network_site_create(&php, &wp, &docroot, &slug).map(|_| ())
}

/// Delete a sub-site by `blogId` (`wp site delete`). The main site can't be deleted.
#[tauri::command]
pub async fn wp_network_site_delete(state: State<'_, AppState>, id: String, blog_id: String) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    core::wordpress::network_site_delete(&php, &wp, &docroot, &blog_id).map(|_| ())
}

/// Network-activate one or more plugins (`wp plugin activate … --network`).
#[tauri::command]
pub async fn wp_plugin_activate_network(state: State<'_, AppState>, id: String, names: Vec<String>) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    core::wordpress::plugin_activate_network(&php, &wp, &docroot, &names).map(|_| ())
}

/// Network-deactivate one or more plugins (`wp plugin deactivate … --network`).
#[tauri::command]
pub async fn wp_plugin_deactivate_network(state: State<'_, AppState>, id: String, names: Vec<String>) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    core::wordpress::plugin_deactivate_network(&php, &wp, &docroot, &names).map(|_| ())
}

/// Network-enable a theme (`wp theme enable <name> --network`).
#[tauri::command]
pub async fn wp_theme_enable_network(state: State<'_, AppState>, id: String, name: String) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    core::wordpress::theme_enable_network(&php, &wp, &docroot, &name).map(|_| ())
}

/// Network-disable a theme (`wp theme disable <name> --network`).
#[tauri::command]
pub async fn wp_theme_disable_network(state: State<'_, AppState>, id: String, name: String) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    core::wordpress::theme_disable_network(&php, &wp, &docroot, &name).map(|_| ())
}

/// List the network's super-admins (`wp super-admin list`).
#[tauri::command]
pub async fn wp_super_admins(state: State<'_, AppState>, id: String) -> Result<Vec<String>> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    core::wordpress::super_admin_list(&php, &wp, &docroot)
}

/// Grant super-admin to a user (`wp super-admin add <user>`).
#[tauri::command]
pub async fn wp_super_admin_add(state: State<'_, AppState>, id: String, user: String) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    core::wordpress::super_admin_add(&php, &wp, &docroot, &user).map(|_| ())
}

/// Convert a WordPress site to multisite (`subdomain` | `subdirectory`): writes
/// the network constants, persists the mode, and — if the stack is running —
/// reloads the edge so nginx serves with the matching rewrite template (§10.1).
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

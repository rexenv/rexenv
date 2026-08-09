//! commands::wordpress — thin Tauri IPC for WordPress detection/management
//! (Phase 3 §1). Resolves the bundled PHP + wp-cli phar and delegates to
//! `core::wordpress`. No business logic here.

use crate::core::wordpress::{WpInfo, WpNetworkSite, WpPlugin, WpTheme, WpUser};
use crate::core::{self, binaries, php};
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::models::{MultisiteMode, Site};
use std::path::PathBuf;
use tauri::{Emitter, Manager, State};

/// Run a blocking WP-CLI call off the async runtime. Every call spawns PHP and
/// boots WordPress (hundreds of ms; installs/updates take seconds), and
/// `Command::output()` blocks — the WordPress tab fires several of these at
/// once, which used to tie up tokio worker threads and stall the whole app.
pub(crate) async fn wp_blocking<T, F>(f: F) -> Result<T>
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
pub(crate) async fn wp_tools(
    state: &State<'_, AppState>,
    php_minor: &str,
) -> Result<(PathBuf, PathBuf)> {
    let patch = php::patch_for_minor(php_minor)
        .ok_or_else(|| Error::Other(format!("no pinned PHP build for {php_minor}")))?;
    let php_bin = binaries::resolve(state.platform.as_ref(), "php", patch).await?;
    // wp-cli is a .phar (not a Mach-O) → resolve_file (no chmod/codesign).
    let wp_phar =
        binaries::resolve_file(state.platform.as_ref(), "wp-cli", binaries::WP_CLI_VERSION).await?;
    Ok((php_bin, wp_phar))
}

/// The same pair for Laravel: the site's PHP CLI build + the pinned Composer
/// phar. Composer is ALWAYS run through this PHP (never a system `composer`,
/// which can be a wrapper script rather than a phar — Herd ships one), so
/// `create-project`'s platform checks are made against the PHP the app runs on.
pub(crate) async fn composer_tools(
    state: &State<'_, AppState>,
    php_minor: &str,
) -> Result<(PathBuf, PathBuf)> {
    let patch = php::patch_for_minor(php_minor)
        .ok_or_else(|| Error::Other(format!("no pinned PHP build for {php_minor}")))?;
    let php_bin = binaries::resolve(state.platform.as_ref(), "php", patch).await?;
    // A .phar (not a Mach-O) → resolve_file: no chmod/codesign step.
    let composer_phar =
        binaries::resolve_file(state.platform.as_ref(), "composer", binaries::COMPOSER_VERSION)
            .await?;
    Ok((php_bin, composer_phar))
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

/// The site's recorded content dir (v24). The delete guards must stat the
/// SAME directory wp-cli will act on — a hardcoded `wp-content` on a Bedrock
/// site stats the wrong path, sees no symlink, and silently defeats the
/// unlink-only partition.
fn site_content_rel(state: &State<'_, AppState>, id: &str) -> Result<String> {
    let conn = state
        .db
        .lock()
        .map_err(|_| Error::Other("database lock poisoned".into()))?;
    let site = core::sites::get(&conn, id)?.ok_or_else(|| Error::Other(format!("no site {id}")))?;
    Ok(site.content_dir_rel().to_string())
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

/// Live search of the WordPress.org plugin directory (Add-plugin flow) —
/// stateless, hard 10s timeout, honest error offline.
#[tauri::command]
pub async fn wp_org_search_plugins(query: String) -> Result<Vec<core::wporg::WpOrgPlugin>> {
    core::wporg::search_plugins(&query).await
}

/// Live search of the WordPress.org theme directory (Add-theme flow).
#[tauri::command]
pub async fn wp_org_search_themes(query: String) -> Result<Vec<core::wporg::WpOrgTheme>> {
    core::wporg::search_themes(&query).await
}

/// Icon URLs for installed plugins (plugin-list display) — cached per app run;
/// unknown/non-wp.org slugs map to null. Never fails: icons are decoration.
#[tauri::command]
pub async fn wp_org_plugin_icons(
    slugs: Vec<String>,
) -> Result<std::collections::HashMap<String, Option<String>>> {
    Ok(core::wporg::plugin_icons(&slugs).await)
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

/// The per-site event an update streams its progress on, one channel per noun
/// (`wp-update://plugins/<id>`, `…/themes/<id>`, `…/core/<id>`). Per SITE, not
/// per run: two updates on the same site would fight over WordPress's
/// maintenance mode anyway, so there is nothing to disambiguate.
pub fn update_event(kind: core::wordpress::UpdateKind, site_id: &str) -> String {
    let channel = match kind {
        core::wordpress::UpdateKind::Plugin => "plugins",
        core::wordpress::UpdateKind::Theme => "themes",
        core::wordpress::UpdateKind::Core => "core",
    };
    format!("wp-update://{channel}/{site_id}")
}

/// Run `wp <noun> update` streamed, emitting a phase snapshot per meaningful
/// line and returning everything WP-CLI printed.
///
/// ONE code path per noun, not a UI-only variant: the CLI/MCP callers simply
/// have no listener, and an unheard emit costs nothing. A big plugin
/// (WooCommerce, Elementor) or a core release spends tens of seconds inside a
/// single wp-cli call, and the silent version read as a frozen app.
async fn run_update_streamed<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: &State<'_, AppState>,
    id: &str,
    kind: core::wordpress::UpdateKind,
    names: Vec<String>,
) -> Result<String> {
    let (docroot, php, wp) = site_tools(state, id).await?;
    let event = update_event(kind, id);
    wp_blocking(move || {
        // The supervisor is borrowed from AppState INSIDE the blocking task
        // (the repo-job pattern): `AppState.platform` is a `Box`, so it can't
        // cross the spawn — the `AppHandle` can.
        let state = app.state::<AppState>();
        let mut tracker = core::wordpress::UpdateTracker::new(kind, &names);
        let mut log = String::new();
        {
            let mut on_line = |line: &str| {
                log.push_str(line);
                log.push('\n');
                if tracker.feed(line) {
                    let _ = app.emit(&event, tracker.snapshot(line));
                }
            };
            core::wordpress::update_streamed(
                state.platform.supervisor(),
                &php,
                &wp,
                &docroot,
                kind,
                &names,
                &mut on_line,
            )?;
        }
        Ok(log)
    })
    .await
}

/// Update one or more plugins, streaming WP-CLI's own phases.
#[tauri::command]
pub async fn wp_plugin_update<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    id: String,
    names: Vec<String>,
) -> Result<()> {
    run_update_streamed(app, &state, &id, core::wordpress::UpdateKind::Plugin, names)
        .await
        .map(|_| ())
}

/// Delete one or more plugins.
#[tauri::command]
pub async fn wp_plugin_delete(state: State<'_, AppState>, id: String, names: Vec<String>) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    // SAFETY SPLIT on filesystem truth: a symlinked plugin dir must be
    // UNLINKED — `wp plugin delete` walks INTO the link and destroys the
    // user's real checkout elsewhere on disk. Covers manually-linked dirs
    // that were never adopted (provenance is metadata, the fs is the guard).
    let content = docroot.join(site_content_rel(&state, &id)?).join("plugins");
    let (linked, normal) = core::repo::partition_symlink_deletes(&content, &names);
    for name in &linked {
        // Best-effort deactivate so WP doesn't trip over a vanished active
        // plugin; the unlink below is the real operation.
        let (p2, w2, d2, n2) = (php.clone(), wp.clone(), docroot.clone(), name.clone());
        let _ = wp_blocking(move || {
            core::wordpress::plugin_deactivate(&p2, &w2, &d2, &[n2]).map(|_| ())
        })
        .await;
        let dir = content.join(core::repo::validate_dir_name(name)?);
        state.platform.shell().remove_symlink(&dir)?;
    }
    if !normal.is_empty() {
        let (p2, w2, d2, n2) = (php.clone(), wp.clone(), docroot.clone(), normal.clone());
        wp_blocking(move || core::wordpress::plugin_delete(&p2, &w2, &d2, &n2).map(|_| ())).await?;
    }
    // Provenance rows for anything that's gone.
    if let Ok(conn) = state.db.lock() {
        for name in linked.iter().chain(normal.iter()) {
            let _ = crate::state::store::delete_git_asset(&conn, &id, "plugin", name);
        }
    }
    Ok(())
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
    let content_rel = site_content_rel(&state, &id)?;
    wp_blocking(move || {
        core::wordpress::theme_list(&php, &wp, &docroot, &content_rel, check_updates.unwrap_or(false))
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
pub async fn wp_theme_update<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    id: String,
    names: Vec<String>,
) -> Result<()> {
    run_update_streamed(app, &state, &id, core::wordpress::UpdateKind::Theme, names)
        .await
        .map(|_| ())
}

/// Delete one or more themes (not the active one).
#[tauri::command]
pub async fn wp_theme_delete(state: State<'_, AppState>, id: String, names: Vec<String>) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    // Same unlink-only guard as plugins (fs truth). The ACTIVE theme's link
    // is refused — wp-cli refuses deleting the active theme on the normal
    // path, and removing its link would leave WP themeless.
    let content = docroot.join(site_content_rel(&state, &id)?).join("themes");
    let (linked, normal) = core::repo::partition_symlink_deletes(&content, &names);
    if !linked.is_empty() {
        let (p2, w2, d2) = (php.clone(), wp.clone(), docroot.clone());
        let active =
            wp_blocking(move || core::wordpress::active_stylesheet(&p2, &w2, &d2)).await?;
        if let Some(a) = linked.iter().find(|n| **n == active) {
            return Err(Error::Other(format!(
                "\"{a}\" is the ACTIVE theme — activate another theme first, \
                 then remove the link."
            )));
        }
        for name in &linked {
            let dir = content.join(core::repo::validate_dir_name(name)?);
            state.platform.shell().remove_symlink(&dir)?;
        }
    }
    if !normal.is_empty() {
        let (p2, w2, d2, n2) = (php.clone(), wp.clone(), docroot.clone(), normal.clone());
        wp_blocking(move || core::wordpress::theme_delete(&p2, &w2, &d2, &n2).map(|_| ())).await?;
    }
    if let Ok(conn) = state.db.lock() {
        for name in linked.iter().chain(normal.iter()) {
            let _ = crate::state::store::delete_git_asset(&conn, &id, "theme", name);
        }
    }
    Ok(())
}

/// List the site's WordPress users (`wp user list`).
#[tauri::command]
pub async fn wp_users(state: State<'_, AppState>, id: String) -> Result<Vec<WpUser>> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::user_list(&php, &wp, &docroot)).await
}

/// Create a WordPress user (`wp user create`) with an explicit password.
#[tauri::command]
pub async fn wp_user_create(
    state: State<'_, AppState>,
    id: String,
    login: String,
    email: String,
    role: String,
    password: String,
) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || {
        core::wordpress::user_create(&php, &wp, &docroot, &login, &email, &role, &password)
            .map(|_| ())
    })
    .await
}

/// Change an existing user's password (`wp user update --user_pass`).
#[tauri::command]
pub async fn wp_user_set_password(
    state: State<'_, AppState>,
    id: String,
    user_id: u64,
    password: String,
) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || {
        core::wordpress::user_set_password(&php, &wp, &docroot, user_id, &password).map(|_| ())
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
    let content_rel = site.content_dir_rel().to_string();
    let domain = site.domain.clone();
    let (token, created_dir) = wp_blocking(move || {
        core::wp_login::issue(
            &php_bin,
            &wp_phar,
            &docroot,
            &content_rel,
            &domain,
            user_id,
            core::wp_login::LOGIN_TTL_SECS,
        )
    })
    .await?;
    if created_dir {
        crate::commands::tunnels::record_mu_dir_created(&state, &site.id);
    }
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
    let content_rel = site.content_dir_rel().to_string();
    let domain = site.domain.clone();
    let (admin_id, token, created_dir) = wp_blocking(move || {
        let admin_id = core::wordpress::primary_admin_id(&php_bin, &wp_phar, &docroot)?;
        let (token, created_dir) = core::wp_login::issue(
            &php_bin,
            &wp_phar,
            &docroot,
            &content_rel,
            &domain,
            admin_id,
            core::wp_login::LOGIN_TTL_SECS,
        )?;
        Ok((admin_id, token, created_dir))
    })
    .await?;
    if created_dir {
        crate::commands::tunnels::record_mu_dir_created(&state, &site.id);
    }
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
    wp_blocking(move || {
        core::wordpress::search_replace(&php, &wp, &docroot, &from, &to, dry_run, false)
    })
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

/// Installable WordPress releases (newest first) from wordpress.org's
/// stable-check API — feeds the version picker. Needs network.
#[tauri::command]
pub async fn wp_core_versions() -> Result<Vec<core::wordpress::WpCoreVersion>> {
    core::wordpress::core_versions().await
}

/// Switch core to an exact version (downgrades use `--force`). The version
/// must be on a FRESH stable-check list (picker-only, re-fetched here);
/// success is gated on `wp core version` reporting the target, and the result
/// says explicitly whether wp-admin will ask for a database update.
#[tauri::command]
pub async fn wp_core_switch_version(
    state: State<'_, AppState>,
    id: String,
    version: String,
) -> Result<core::wordpress::WpCoreSwitch> {
    let allowed: Vec<String> =
        core::wordpress::core_versions().await?.into_iter().map(|v| v.version).collect();
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || {
        core::wordpress::core_switch_version(&php, &wp, &docroot, &version, &allowed)
    })
    .await
}

/// The whitelisted site-options form (values + timezone/role choice lists).
/// One `wp eval` + one `wp role list` — never a per-option wp-cli call.
#[tauri::command]
pub async fn wp_options(
    state: State<'_, AppState>,
    id: String,
) -> Result<core::wordpress::WpOptionsForm> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::options_get(&php, &wp, &docroot)).await
}

/// Update one whitelisted option. Core enforces the whitelist by name
/// ("not an editable option" for anything else — a bypassed UI can't write
/// `siteurl`), validates the value per kind, and refuses non-scalar targets.
#[tauri::command]
pub async fn wp_option_update(
    state: State<'_, AppState>,
    id: String,
    name: String,
    value: String,
) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::option_update(&php, &wp, &docroot, &name, &value)).await
}

/// Delete the checksum panel's benign macOS-noise files, then re-run the
/// checksum verify so the panel refreshes in one round-trip. The UI's paths
/// are suggestions only — core re-validates every file (noise basename,
/// relative/no-`..`, not a symlink, canonicalizes inside the docroot) and
/// skips (never aborts) on any guard/io failure.
#[tauri::command]
pub async fn wp_checksum_cleanup(
    state: State<'_, AppState>,
    id: String,
    paths: Vec<String>,
) -> Result<core::wordpress::ChecksumCleanup> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || {
        let cleaned = core::wordpress::cleanup_os_noise(&docroot, &paths)?;
        let report = core::wordpress::core_verify_checksums(&php, &wp, &docroot)?;
        Ok(core::wordpress::ChecksumCleanup {
            removed: cleaned.removed,
            skipped: cleaned.skipped,
            report,
        })
    })
    .await
}

/// Available + installed core languages (`wp language core list`) — feeds the
/// Language picker. Hits api.wordpress.org (~3s; needs network).
#[tauri::command]
pub async fn wp_languages(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<core::wordpress::WpLanguage>> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::language_list(&php, &wp, &docroot)).await
}

/// Switch the site language in one action: install the core pack if missing
/// (success gated on `is-installed`, never on install's lying exit code), then
/// activate via `wp site switch-language`. Locale comes from the picker and is
/// shape-validated in core.
#[tauri::command]
pub async fn wp_switch_language(
    state: State<'_, AppState>,
    id: String,
    locale: String,
) -> Result<()> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::switch_language(&php, &wp, &docroot, &locale)).await
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
pub async fn wp_core_update<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    id: String,
) -> Result<String> {
    run_update_streamed(app, &state, &id, core::wordpress::UpdateKind::Core, Vec::new()).await
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
    // surface as the dump tool's opaque "Can't connect" error.
    let engine = DbEngine::from_site(site.db_engine);
    if !engine.running() {
        return Err(Error::Other(format!(
            "{} isn't running — start it (Services → Start all, or the Databases page), then export again.",
            engine.label()
        )));
    }
    let version = super::database::effective_db_version(&state, engine)?;
    let (_, dump) = engine.sql_client_bins(state.platform.as_ref(), &version).await?;
    wp_blocking(move || {
        core::database::export_to_downloads(&dump, engine.port(), &site.domain, &site.db_name)
            .map(|p| p.to_string_lossy().into_owned())
    })
    .await
}

/// Export site content as WXR XML into the user's Downloads folder. Returns
/// the written file paths (wp-cli may split large exports).
#[tauri::command]
pub async fn wp_content_export(state: State<'_, AppState>, id: String) -> Result<Vec<String>> {
    let (docroot, php, wp) = site_tools(&state, &id).await?;
    wp_blocking(move || core::wordpress::content_export_to_downloads(&php, &wp, &docroot)).await
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
    let engine = DbEngine::from_site(site.db_engine);
    if !engine.running() {
        return Err(Error::Other(format!(
            "{} isn't running — start it (Services → Start all, or the Databases page), then import again.",
            engine.label()
        )));
    }
    let version = super::database::effective_db_version(&state, engine)?;
    let (client, _) = engine.sql_client_bins(state.platform.as_ref(), &version).await?;
    wp_blocking(move || {
        core::database::import_from_file(&client, engine.port(), &site.db_name, &file)
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
    let engine = DbEngine::from_site(site.db_engine);
    if !engine.running() {
        return Err(Error::Other(format!(
            "{} isn't running — start it (Services → Start all, or the Databases page), then reset again.",
            engine.label()
        )));
    }
    let version = super::database::effective_db_version(&state, engine)?;
    let (db_client, _) = engine.sql_client_bins(state.platform.as_ref(), &version).await?;
    let (php, wp) = wp_tools(&state, &site.php_version).await?;
    let was_multisite = !matches!(site.multisite, MultisiteMode::None);
    let (docroot, domain, name, db_name) = (
        PathBuf::from(&site.path),
        site.domain.clone(),
        site.name.clone(),
        site.db_name.clone(),
    );
    wp_blocking(move || {
        core::wordpress::reset_site(
            &php, &wp, &docroot, &domain, &name, &db_name, &db_client, engine.port(),
        )
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
                mgr.reload(state.platform.as_ref(), &state.ca, &sites, false).await?
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
    tunnels: State<'_, crate::commands::tunnels::Tunnels>,
    id: String,
    mode: String,
) -> Result<Option<Site>> {
    let mode = MultisiteMode::parse_db(&mode)?;
    // Mutation-under-share guard (audit A2 enumeration, 28 Jul 2026): the
    // conversion rewrites the database and wp-config under the live link —
    // same exposure class as the step-7 job guards. The vhost survives (this
    // is not the A1 vhost-loss case), but visitors would hit a site
    // mid-conversion.
    {
        let domain = {
            let conn = state
                .db
                .lock()
                .map_err(|_| Error::Other("database lock poisoned".into()))?;
            core::sites::get(&conn, &id)?.map(|s| s.domain)
        };
        if let Some(domain) = domain {
            crate::commands::tunnels::refuse_if_shared(
                &tunnels,
                &state,
                &domain,
                "converting it between single-site and multisite rewrites its database and \
                 config under the live link — visitors would hit a site mid-conversion",
            )?;
        }
    }
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
                mgr.reload(state.platform.as_ref(), &state.ca, &sites, false).await?
            } else {
                Vec::new()
            }
        };
        core::service_manager::await_ready(checks).await?;
    }
    Ok(site)
}

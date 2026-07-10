//! core::wordpress — WP-CLI wrapper (Phase 1 §9).
//!
//! WP-CLI is a `.phar` run through the bundled PHP: `php wp-cli.phar <args>`.
//! Invocation is identical on every OS (the per-OS bit is which `php` binary,
//! resolved via `BinaryProvider`), so this runs the command directly and
//! captures output (unlike `ProcessSupervisor`, which is for long-lived services).

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Run `php <wp_phar> <args>` (optionally in `cwd`) and return the raw `Output`.
pub fn wp_cli(
    php_bin: &Path,
    wp_phar: &Path,
    args: &[&str],
    cwd: Option<&Path>,
) -> Result<Output> {
    let mut cmd = Command::new(php_bin);
    // WP-CLI (esp. core extraction) needs more than the default 128M.
    cmd.arg("-d")
        .arg("memory_limit=512M")
        .arg(wp_phar)
        .args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    Ok(cmd.output()?)
}

/// Run WP-CLI and return stdout, erroring (with stderr) on a non-zero exit.
pub fn wp_cli_checked(
    php_bin: &Path,
    wp_phar: &Path,
    args: &[&str],
    cwd: Option<&Path>,
) -> Result<String> {
    let out = wp_cli(php_bin, wp_phar, args, cwd)?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(Error::Other(format!(
            "wp {} failed (exit {:?}): {}",
            args.first().copied().unwrap_or(""),
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

/// Run a WP-CLI command scoped to a docroot (`--path=<docroot>` is appended for
/// the caller) and return trimmed stdout, erroring (with stderr) on non-zero exit.
pub fn wp_run(php_bin: &Path, wp_phar: &Path, docroot: &Path, args: &[&str]) -> Result<String> {
    let path = format!("--path={}", docroot.display());
    let mut full: Vec<&str> = Vec::with_capacity(args.len() + 1);
    full.extend_from_slice(args);
    full.push(&path);
    Ok(wp_cli_checked(php_bin, wp_phar, &full, None)?.trim().to_string())
}

/// Typed JSON bridge: run a WP-CLI command scoped to a docroot with
/// `--format=json` and deserialize stdout into `T` (e.g. `Vec<PluginRow>`).
/// Non-zero exit / stderr surfaces as a clean `Error`, as does a parse failure.
pub fn wp_json<T: serde::de::DeserializeOwned>(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    args: &[&str],
) -> Result<T> {
    let path = format!("--path={}", docroot.display());
    let mut full: Vec<&str> = Vec::with_capacity(args.len() + 2);
    full.extend_from_slice(args);
    full.push(&path);
    full.push("--format=json");
    let out = wp_cli_checked(php_bin, wp_phar, &full, None)?;
    serde_json::from_str(out.trim())
        .map_err(|e| Error::Other(format!("wp {}: bad JSON: {e}", args.first().copied().unwrap_or(""))))
}

/// What `wp_info` reports about a docroot (mirrors the frontend `WpInfo`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpInfo {
    /// A present, installed WordPress lives at the docroot.
    pub is_wordpress: bool,
    /// Core version (`wp core version`) when WordPress; else `None`.
    pub version: Option<String>,
    /// Whether the install is a multisite network.
    pub multisite: bool,
}

/// Detect WordPress at a docroot via `core is-installed` (presence), `core version`,
/// and the `MULTISITE` constant. A non-WordPress docroot (e.g. a Blank-PHP site)
/// reports `is_wordpress: false` rather than erroring.
pub fn wp_info(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<WpInfo> {
    let path = format!("--path={}", docroot.display());

    // `core is-installed` exits 0 only for a present, installed WordPress.
    let is_wordpress = wp_cli(php_bin, wp_phar, &["core", "is-installed", &path], None)
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !is_wordpress {
        return Ok(WpInfo { is_wordpress: false, version: None, multisite: false });
    }

    let version = wp_run(php_bin, wp_phar, docroot, &["core", "version"]).ok();

    // `config get MULTISITE` prints the constant ("1") for a network; it errors
    // when the constant is unset — treat that as not-multisite.
    let multisite = wp_cli(php_bin, wp_phar, &["config", "get", "MULTISITE", &path], None)
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().eq_ignore_ascii_case("1"))
        .unwrap_or(false);

    Ok(WpInfo { is_wordpress, version, multisite })
}

/// WP-CLI's `update` field is a string ("none" | "available" | "version higher
/// than expected") for regular plugins/themes but a BOOLEAN for must-use +
/// drop-in rows (e.g. rexenv's own `rexenv-login` mu-plugin — present on every
/// rexenv site, so a strict `String` made the whole list fail to parse).
fn de_update<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Update {
        S(String),
        B(bool),
        Null,
    }
    Ok(match Update::deserialize(d)? {
        Update::S(s) => s,
        Update::B(true) => "available".into(),
        Update::B(false) => "none".into(),
        Update::Null => "none".into(),
    })
}

/// One plugin row from `wp plugin list --format=json` (§6.1). Field names match
/// WP-CLI's JSON (all lowercase) and the frontend DTO. `update == "available"`
/// drives the update-available badge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WpPlugin {
    pub name: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub version: String,
    #[serde(default, deserialize_with = "de_update")]
    pub update: String,
}

/// `wp plugin list` (name, status, version, update). `check_updates: false`
/// passes `--skip-update-check` — the default check hits api.wordpress.org on
/// EVERY list (seconds when slow, a hang when offline), so the UI lists fast
/// without it and refreshes update badges in a background pass.
pub fn plugin_list(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    check_updates: bool,
) -> Result<Vec<WpPlugin>> {
    let mut args = vec!["plugin", "list"];
    if !check_updates {
        args.push("--skip-update-check");
    }
    wp_json(php_bin, wp_phar, docroot, &args)
}

/// Run `wp <noun> <verb> <names…>` (bulk-capable: one call for many items).
/// A no-op (empty names) returns Ok without invoking WP-CLI.
fn item_verb(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    noun: &str,
    verb: &str,
    names: &[String],
) -> Result<String> {
    if names.is_empty() {
        return Ok(String::new());
    }
    let mut args: Vec<&str> = vec![noun, verb];
    args.extend(names.iter().map(String::as_str));
    wp_run(php_bin, wp_phar, docroot, &args)
}

fn plugin_verb(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    verb: &str,
    names: &[String],
) -> Result<String> {
    item_verb(php_bin, wp_phar, docroot, "plugin", verb, names)
}

/// Activate one or more plugins (`wp plugin activate …`).
pub fn plugin_activate(php_bin: &Path, wp_phar: &Path, docroot: &Path, names: &[String]) -> Result<String> {
    plugin_verb(php_bin, wp_phar, docroot, "activate", names)
}
/// Deactivate one or more plugins (`wp plugin deactivate …`).
pub fn plugin_deactivate(php_bin: &Path, wp_phar: &Path, docroot: &Path, names: &[String]) -> Result<String> {
    plugin_verb(php_bin, wp_phar, docroot, "deactivate", names)
}
/// Update one or more plugins (`wp plugin update …`).
pub fn plugin_update(php_bin: &Path, wp_phar: &Path, docroot: &Path, names: &[String]) -> Result<String> {
    plugin_verb(php_bin, wp_phar, docroot, "update", names)
}
/// Delete one or more plugins (`wp plugin delete …`).
pub fn plugin_delete(php_bin: &Path, wp_phar: &Path, docroot: &Path, names: &[String]) -> Result<String> {
    plugin_verb(php_bin, wp_phar, docroot, "delete", names)
}

/// Install a plugin by slug (`wp plugin install <slug> [--activate]`).
pub fn plugin_install(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    slug: &str,
    activate: bool,
) -> Result<String> {
    let mut args: Vec<&str> = vec!["plugin", "install", slug];
    if activate {
        args.push("--activate");
    }
    wp_run(php_bin, wp_phar, docroot, &args)
}

/// One theme row from `wp theme list --format=json` (§6.2). `status == "active"`
/// marks the live theme. `screenshot` is filled AFTER parsing (it's not a WP-CLI
/// field): the theme's `screenshot.*` file as a `data:` URL, `None` when absent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WpTheme {
    pub name: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub version: String,
    #[serde(default, deserialize_with = "de_update")]
    pub update: String,
    #[serde(default)]
    pub screenshot: Option<String>,
}

/// A theme's `screenshot.*` preview as a `data:` URL. WP-CLI's `name` field is
/// the stylesheet slug — the theme's directory under `wp-content/themes/`.
/// Missing file (screenshots are optional) ⇒ `None`, never an error.
fn theme_screenshot(docroot: &Path, slug: &str) -> Option<String> {
    use base64::Engine;
    let dir = docroot.join("wp-content").join("themes").join(slug);
    for (ext, mime) in [
        ("png", "image/png"),
        ("jpg", "image/jpeg"),
        ("jpeg", "image/jpeg"),
        ("webp", "image/webp"),
        ("gif", "image/gif"),
    ] {
        if let Ok(bytes) = std::fs::read(dir.join(format!("screenshot.{ext}"))) {
            let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
            return Some(format!("data:{mime};base64,{b64}"));
        }
    }
    None
}

/// `wp theme list` (name, status, version, update) + each theme's screenshot as
/// a `data:` URL. `check_updates: false` passes `--skip-update-check` (same
/// api.wordpress.org round-trip as [`plugin_list`]).
pub fn theme_list(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    check_updates: bool,
) -> Result<Vec<WpTheme>> {
    let mut args = vec!["theme", "list"];
    if !check_updates {
        args.push("--skip-update-check");
    }
    let mut themes: Vec<WpTheme> = wp_json(php_bin, wp_phar, docroot, &args)?;
    for t in &mut themes {
        t.screenshot = theme_screenshot(docroot, &t.name);
    }
    Ok(themes)
}

/// Activate a theme (`wp theme activate <name>`) — only one can be live.
pub fn theme_activate(php_bin: &Path, wp_phar: &Path, docroot: &Path, name: &str) -> Result<String> {
    wp_run(php_bin, wp_phar, docroot, &["theme", "activate", name])
}
/// Update one or more themes (`wp theme update …`).
pub fn theme_update(php_bin: &Path, wp_phar: &Path, docroot: &Path, names: &[String]) -> Result<String> {
    item_verb(php_bin, wp_phar, docroot, "theme", "update", names)
}
/// Delete one or more themes (`wp theme delete …`) — the active theme can't be deleted.
pub fn theme_delete(php_bin: &Path, wp_phar: &Path, docroot: &Path, names: &[String]) -> Result<String> {
    item_verb(php_bin, wp_phar, docroot, "theme", "delete", names)
}
/// Install a theme by slug (`wp theme install <slug> [--activate]`).
pub fn theme_install(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    slug: &str,
    activate: bool,
) -> Result<String> {
    let mut args: Vec<&str> = vec!["theme", "install", slug];
    if activate {
        args.push("--activate");
    }
    wp_run(php_bin, wp_phar, docroot, &args)
}

/// A WordPress user for the Users sub-tab (§7.1).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpUser {
    pub id: u64,
    pub login: String,
    pub email: String,
    /// Comma-separated role list (WP-CLI's `roles` field).
    pub roles: String,
    pub name: String,
}

// `wp user list --format=json` wire shape (WP-CLI's snake_case keys).
#[derive(Deserialize)]
struct WireUser {
    #[serde(rename = "ID", default)]
    id: u64,
    #[serde(rename = "user_login", default)]
    login: String,
    #[serde(rename = "user_email", default)]
    email: String,
    #[serde(rename = "roles", default)]
    roles: String,
    #[serde(rename = "display_name", default)]
    name: String,
}

/// `wp user list` (id, login, email, roles, display name).
pub fn user_list(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<Vec<WpUser>> {
    let wire: Vec<WireUser> = wp_json(
        php_bin,
        wp_phar,
        docroot,
        &["user", "list", "--fields=ID,user_login,user_email,roles,display_name"],
    )?;
    Ok(wire
        .into_iter()
        .map(|u| WpUser { id: u.id, login: u.login, email: u.email, roles: u.roles, name: u.name })
        .collect())
}

/// The site's PRIMARY administrator: the lowest-ID user holding the
/// `administrator` role — the install's original admin on a one-click site
/// (rexenv installs create it as user 1). Backs the "Open admin" one-click
/// login, which must never guess: no administrator ⇒ clean error, the caller
/// falls back to the plain login page.
pub fn primary_admin_id(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<u64> {
    let out = wp_run(
        php_bin,
        wp_phar,
        docroot,
        &["user", "list", "--role=administrator", "--field=ID", "--orderby=ID", "--order=ASC"],
    )?;
    out.lines()
        .next()
        .and_then(|l| l.trim().parse::<u64>().ok())
        .ok_or_else(|| Error::Other("no administrator user found on this site".into()))
}

/// Create a user (`wp user create <login> <email> --role=<role>`). WP-CLI
/// generates a random password; returns the new user's id (via `--porcelain`).
pub fn user_create(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    login: &str,
    email: &str,
    role: &str,
) -> Result<String> {
    let role_arg = format!("--role={role}");
    wp_run(php_bin, wp_phar, docroot, &["user", "create", login, email, &role_arg, "--porcelain"])
}

/// Convert a single-site WordPress install to a network (`wp core
/// multisite-convert [--subdomains]`), writing the network constants into
/// wp-config. `subdomains` chooses subdomain vs subdirectory install (§10.1).
pub fn multisite_convert(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    subdomains: bool,
) -> Result<String> {
    let mut args: Vec<&str> = vec!["core", "multisite-convert"];
    if subdomains {
        args.push("--subdomains");
    }
    wp_run(php_bin, wp_phar, docroot, &args)
}

// ── Network / multisite management (§10.3) ───────────────────────────────────

/// One sub-site in a network for the Network sub-tab (`wp site list`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpNetworkSite {
    /// `blog_id` (1 = the main site).
    pub id: String,
    /// Full sub-site URL (e.g. `http://a.mysite.test/`).
    pub url: String,
    pub registered: String,
    /// Soft-deleted (archived) — kept in the list but flagged.
    pub deleted: bool,
}

// `wp site list --format=json` wire shape (all values arrive as strings).
#[derive(Deserialize)]
struct WireSite {
    #[serde(rename = "blog_id", default)]
    blog_id: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    registered: String,
    #[serde(default)]
    deleted: String,
}

/// List the network's sub-sites (`wp site list`). Multisite-only — errors on a
/// single-site install (WP-CLI: "This is not a multisite installation").
pub fn network_site_list(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<Vec<WpNetworkSite>> {
    let wire: Vec<WireSite> = wp_json(
        php_bin,
        wp_phar,
        docroot,
        &["site", "list", "--fields=blog_id,url,registered,deleted"],
    )?;
    Ok(wire
        .into_iter()
        .map(|s| WpNetworkSite {
            id: s.blog_id,
            url: s.url,
            registered: s.registered,
            deleted: s.deleted == "1",
        })
        .collect())
}

/// Create a sub-site by slug (`wp site create --slug=<slug>`). The slug becomes a
/// subdomain (`<slug>.mysite.test`) or a path (`mysite.test/<slug>`) per the
/// network's install type. Returns the new `blog_id` (via `--porcelain`).
pub fn network_site_create(php_bin: &Path, wp_phar: &Path, docroot: &Path, slug: &str) -> Result<String> {
    let slug_arg = format!("--slug={slug}");
    wp_run(php_bin, wp_phar, docroot, &["site", "create", &slug_arg, "--porcelain"])
}

/// Delete a sub-site by `blog_id` (`wp site delete <id> --yes`). The main site
/// (id 1) can't be deleted — WP-CLI rejects it.
pub fn network_site_delete(php_bin: &Path, wp_phar: &Path, docroot: &Path, blog_id: &str) -> Result<String> {
    wp_run(php_bin, wp_phar, docroot, &["site", "delete", blog_id, "--yes"])
}

/// Network-activate one or more plugins (`wp plugin activate … --network`) — they
/// become `active-network` (the "Network active" badge) for every sub-site.
pub fn plugin_activate_network(php_bin: &Path, wp_phar: &Path, docroot: &Path, names: &[String]) -> Result<String> {
    if names.is_empty() {
        return Ok(String::new());
    }
    let mut args: Vec<&str> = vec!["plugin", "activate"];
    args.extend(names.iter().map(String::as_str));
    args.push("--network");
    wp_run(php_bin, wp_phar, docroot, &args)
}

/// Network-deactivate one or more plugins (`wp plugin deactivate … --network`).
pub fn plugin_deactivate_network(php_bin: &Path, wp_phar: &Path, docroot: &Path, names: &[String]) -> Result<String> {
    if names.is_empty() {
        return Ok(String::new());
    }
    let mut args: Vec<&str> = vec!["plugin", "deactivate"];
    args.extend(names.iter().map(String::as_str));
    args.push("--network");
    wp_run(php_bin, wp_phar, docroot, &args)
}

/// Network-enable a theme (`wp theme enable <name> --network`) — make it available
/// to every sub-site.
pub fn theme_enable_network(php_bin: &Path, wp_phar: &Path, docroot: &Path, name: &str) -> Result<String> {
    wp_run(php_bin, wp_phar, docroot, &["theme", "enable", name, "--network"])
}

/// Network-disable a theme (`wp theme disable <name> --network`).
pub fn theme_disable_network(php_bin: &Path, wp_phar: &Path, docroot: &Path, name: &str) -> Result<String> {
    wp_run(php_bin, wp_phar, docroot, &["theme", "disable", name, "--network"])
}

/// List the network's super-admins (`wp super-admin list` — one login per line).
pub fn super_admin_list(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<Vec<String>> {
    let out = wp_run(php_bin, wp_phar, docroot, &["super-admin", "list"])?;
    Ok(out.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
}

/// Grant super-admin to a user (`wp super-admin add <user>`).
pub fn super_admin_add(php_bin: &Path, wp_phar: &Path, docroot: &Path, user: &str) -> Result<String> {
    wp_run(php_bin, wp_phar, docroot, &["super-admin", "add", user])
}

// ── Tools (§7.2) ─────────────────────────────────────────────────────────────

/// Whether `WP_DEBUG` is enabled (`wp config get WP_DEBUG`). A missing constant
/// reads as off.
pub fn wp_debug_get(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<bool> {
    let v = wp_run(php_bin, wp_phar, docroot, &["config", "get", "WP_DEBUG"]).unwrap_or_default();
    Ok(matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true"))
}

/// Toggle full WordPress debugging (`wp config set … --raw` so values are
/// boolean constants, not the string `"true"`).
///
/// On ⇒ the recommended development trio: `WP_DEBUG` + `WP_DEBUG_LOG` (write to
/// `wp-content/debug.log` — PHP creates the file on the first entry) with
/// `WP_DEBUG_DISPLAY` false (log, don't print; core's `wp_debug_mode()` also
/// runs `ini_set('display_errors', 0)` for us when it's false).
/// Off ⇒ `WP_DEBUG`/`WP_DEBUG_LOG` false and `WP_DEBUG_DISPLAY` removed, i.e.
/// back to a stock wp-config.
pub fn wp_debug_set(php_bin: &Path, wp_phar: &Path, docroot: &Path, on: bool) -> Result<String> {
    let val = if on { "true" } else { "false" };
    let out = wp_run(php_bin, wp_phar, docroot, &["config", "set", "WP_DEBUG", val, "--raw"])?;
    wp_run(php_bin, wp_phar, docroot, &["config", "set", "WP_DEBUG_LOG", val, "--raw"])?;
    if on {
        wp_run(php_bin, wp_phar, docroot, &["config", "set", "WP_DEBUG_DISPLAY", "false", "--raw"])?;
    } else {
        // May not exist (e.g. debugging was enabled by hand) — not an error.
        let _ = wp_run(php_bin, wp_phar, docroot, &["config", "delete", "WP_DEBUG_DISPLAY"]);
    }
    Ok(out)
}

/// Permalink structures the UI picker offers (wp-admin's stock choices; `""` =
/// Plain). Whitelisted like [`DEBUG_FLAGS`]: the value lands in wp-cli argv.
pub const PERMALINK_STRUCTURES: [&str; 5] = [
    "",
    "/%year%/%monthnum%/%day%/%postname%/",
    "/%year%/%monthnum%/%postname%/",
    "/archives/%post_id%",
    "/%postname%/",
];

/// The site's current permalink structure (`""` = Plain).
pub fn permalink_structure_get(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<String> {
    Ok(
        wp_run(php_bin, wp_phar, docroot, &["option", "get", "permalink_structure"])
            .unwrap_or_default()
            .trim()
            .to_string(),
    )
}

/// Set the permalink structure to one of [`PERMALINK_STRUCTURES`] and flush the
/// rewrite rules. `option update` (not `rewrite structure`) so Plain's empty
/// string takes the same path as every preset.
pub fn permalink_structure_set(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    structure: &str,
) -> Result<String> {
    if !PERMALINK_STRUCTURES.contains(&structure) {
        return Err(Error::Other(format!(
            "not a supported permalink structure: {structure:?}"
        )));
    }
    wp_run(
        php_bin,
        wp_phar,
        docroot,
        &["option", "update", "permalink_structure", structure],
    )?;
    rewrite_flush(php_bin, wp_phar, docroot)
}

/// Boolean wp-config constants the UI may toggle individually (Tools →
/// Debugging). A whitelist: the name lands in wp-cli argv, so free-form input
/// would be config injection at the trust boundary.
pub const DEBUG_FLAGS: [&str; 3] = ["WP_DEBUG_LOG", "WP_DEBUG_DISPLAY", "SCRIPT_DEBUG"];

fn ensure_debug_flag(name: &str) -> Result<()> {
    if DEBUG_FLAGS.contains(&name) {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "not a toggleable debug constant: {name}"
        )))
    }
}

/// Read one whitelisted boolean wp-config constant (unset ⇒ false).
pub fn config_flag_get(php_bin: &Path, wp_phar: &Path, docroot: &Path, name: &str) -> Result<bool> {
    ensure_debug_flag(name)?;
    let v = wp_run(php_bin, wp_phar, docroot, &["config", "get", name]).unwrap_or_default();
    Ok(matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true"))
}

/// Set one whitelisted boolean wp-config constant (`--raw` ⇒ a real boolean,
/// not the string `"true"`). Explicit `false` rather than delete, so the state
/// the UI shows is the state wp-config declares.
pub fn config_flag_set(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    name: &str,
    on: bool,
) -> Result<String> {
    ensure_debug_flag(name)?;
    let val = if on { "true" } else { "false" };
    wp_run(php_bin, wp_phar, docroot, &["config", "set", name, val, "--raw"])
}

/// Whether WP-CLI maintenance mode is active for the site (the `.maintenance`
/// file WP-CLI manages in the docroot). `is-active` exits 0 when active,
/// non-zero when not — same status-as-answer shape as `core is-installed`.
pub fn maintenance_mode_get(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<bool> {
    let path = format!("--path={}", docroot.display());
    Ok(
        wp_cli(php_bin, wp_phar, &["maintenance-mode", "is-active", &path], None)
            .map(|o| o.status.success())
            .unwrap_or(false),
    )
}

/// Toggle maintenance mode. Idempotent: WP-CLI errors on activate-when-active /
/// deactivate-when-inactive, so an already-in-state site is a no-op success.
pub fn maintenance_mode_set(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    on: bool,
) -> Result<String> {
    if maintenance_mode_get(php_bin, wp_phar, docroot)? == on {
        return Ok(String::new());
    }
    let sub = if on { "activate" } else { "deactivate" };
    wp_run(php_bin, wp_phar, docroot, &["maintenance-mode", sub])
}

/// Run `wp search-replace <from> <to> [--dry-run] --format=count` and return the
/// number of replacements (a dry-run reports the count WITHOUT changing data).
pub fn search_replace(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    from: &str,
    to: &str,
    dry_run: bool,
) -> Result<u64> {
    let mut args: Vec<&str> = vec!["search-replace", from, to, "--format=count"];
    if dry_run {
        args.push("--dry-run");
    }
    let out = wp_run(php_bin, wp_phar, docroot, &args)?;
    out.trim()
        .lines()
        .last()
        .unwrap_or("0")
        .trim()
        .parse::<u64>()
        .map_err(|e| Error::Other(format!("search-replace count: {e} (output: {out:?})")))
}

/// Regenerate permalinks (`wp rewrite flush`).
pub fn rewrite_flush(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<String> {
    wp_run(php_bin, wp_phar, docroot, &["rewrite", "flush"])
}

/// Update WordPress core to the latest release (`wp core update`).
pub fn core_update(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<String> {
    wp_run(php_bin, wp_phar, docroot, &["core", "update"])
}

/// Re-download core files of the current version (`wp core download --force`) —
/// repairs a corrupt/modified core without touching the DB or wp-content.
pub fn core_reinstall(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<String> {
    wp_run(php_bin, wp_phar, docroot, &["core", "download", "--force", "--skip-content"])
}

/// A valid MySQL database name derived from a site domain
/// (`blog.test` → `wp_blog_test`).
pub fn db_name_for(domain: &str) -> String {
    let safe: String = domain
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("wp_{safe}")
}

/// Inputs for a one-click WordPress install. Phase 1 installs **single-site**.
pub struct WpInstall<'a> {
    pub docroot: &'a Path,
    pub db_name: &'a str,
    /// `host:port`, e.g. `127.0.0.1:13306`.
    pub db_host: &'a str,
    /// Extracted MySQL tree (for the bundled `mysql` client that creates the DB).
    pub mysql_basedir: &'a Path,
    /// Full site URL, e.g. `https://mysite.test`.
    pub url: &'a str,
    pub title: &'a str,
    pub admin_user: &'a str,
    pub admin_password: &'a str,
    pub admin_email: &'a str,
    /// WordPress locale for `core download` (e.g. `fr_FR`); empty = default en_US.
    pub locale: &'a str,
}

/// Per-site WordPress install options from the New Site dialog. Empty fields fall
/// back to sensible defaults derived from the site (mirrors the frontend DTO).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InstallOptions {
    pub title: String,
    pub admin_user: String,
    pub admin_email: String,
    pub admin_password: String,
    pub language: String,
}

/// One-click WordPress install via WP-CLI: download core → write wp-config →
/// create the DB → `wp core install` (single-site). Each step is skipped if
/// already done, so the flow is re-runnable.
pub fn install_wordpress(php_bin: &Path, wp_phar: &Path, opts: &WpInstall) -> Result<()> {
    let path = format!("--path={}", opts.docroot.display());

    // 1) WordPress core (optionally a localized build).
    if !opts.docroot.join("wp-load.php").exists() {
        let locale = format!("--locale={}", opts.locale);
        let mut args = vec!["core", "download", &path];
        if !opts.locale.trim().is_empty() {
            args.push(&locale);
        }
        wp_cli_checked(php_bin, wp_phar, &args, None)?;
    }

    // 2) wp-config.php (skip the live DB check — the DB is created next).
    if !opts.docroot.join("wp-config.php").exists() {
        let dbname = format!("--dbname={}", opts.db_name);
        let dbhost = format!("--dbhost={}", opts.db_host);
        wp_cli_checked(
            php_bin,
            wp_phar,
            &[
                "config",
                "create",
                &path,
                &dbname,
                "--dbuser=root",
                "--dbpass=",
                &dbhost,
                "--skip-check",
            ],
            None,
        )?;
    }

    // 3) Create the database (idempotent) with the BUNDLED mysql client.
    //    `wp db create` shells out to a `mysql` found on PATH — absent in a
    //    Finder-launched app — and its swallowed failure used to surface later
    //    as `wp core install`'s "Cannot select database".
    let port = opts
        .db_host
        .rsplit_once(':')
        .and_then(|(_, p)| p.parse().ok())
        .unwrap_or(super::database::MYSQL_PORT);
    super::database::create_database(opts.mysql_basedir, port, opts.db_name)?;

    // 4) Install (single-site) if not already installed.
    let installed = wp_cli(php_bin, wp_phar, &["core", "is-installed", &path], None)
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !installed {
        let url = format!("--url={}", opts.url);
        let title = format!("--title={}", opts.title);
        let au = format!("--admin_user={}", opts.admin_user);
        let ap = format!("--admin_password={}", opts.admin_password);
        let ae = format!("--admin_email={}", opts.admin_email);
        wp_cli_checked(
            php_bin,
            wp_phar,
            &[
                "core", "install", &path, &url, &title, &au, &ap, &ae,
            ],
            None,
        )?;
    }
    Ok(())
}

/// One-click install for a provisioned site (Phase 3 §1.2): fill the install
/// fields from `opts`, defaulting from `domain`/`name` where empty, and delegate
/// to [`install_wordpress`]. The canonical URL is `https://<domain>`; the DB name
/// is derived from the domain. `db_host` is `host:port` (e.g. `127.0.0.1:13306`);
/// `mysql_basedir` is the extracted MySQL tree (bundled client creates the DB).
#[allow(clippy::too_many_arguments)] // flat mirror of the New Site dialog inputs
pub fn install_for_site(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    domain: &str,
    name: &str,
    db_host: &str,
    mysql_basedir: &Path,
    opts: &InstallOptions,
) -> Result<()> {
    let nonempty = |s: &str| !s.trim().is_empty();
    let title = if nonempty(&opts.title) { opts.title.trim().to_string() } else { name.to_string() };
    let admin_user =
        if nonempty(&opts.admin_user) { opts.admin_user.trim().to_string() } else { "admin".into() };
    let admin_email = if nonempty(&opts.admin_email) {
        opts.admin_email.trim().to_string()
    } else {
        format!("admin@{domain}")
    };
    let admin_password = if nonempty(&opts.admin_password) {
        opts.admin_password.clone()
    } else {
        DEFAULT_ADMIN.into() // local-dev default, consistent with reset_site
    };
    let url = format!("https://{domain}");
    let db_name = db_name_for(domain);

    install_wordpress(
        php_bin,
        wp_phar,
        &WpInstall {
            docroot,
            db_name: &db_name,
            db_host,
            mysql_basedir,
            url: &url,
            title: &title,
            admin_user: &admin_user,
            admin_password: &admin_password,
            admin_email: &admin_email,
            locale: opts.language.trim(),
        },
    )
}

/// Multisite constants `wp core multisite-convert` writes into wp-config.php.
/// A reset must clear them: a wp-config that still defines `MULTISITE` over a
/// fresh single-site database is a fatal config error on every request.
const MULTISITE_CONSTANTS: &[&str] = &[
    "WP_ALLOW_MULTISITE",
    "MULTISITE",
    "SUBDOMAIN_INSTALL",
    "DOMAIN_CURRENT_SITE",
    "PATH_CURRENT_SITE",
    "SITE_ID_CURRENT_SITE",
    "BLOG_ID_CURRENT_SITE",
];

/// Default local-dev admin credentials (`admin` / `admin`) — used by the
/// site reset and the New-site fallback. A deliberate convenience for a
/// LOCAL-only site; the tunnels UI warns when a site still accepting these is
/// shared publicly (see [`default_creds_active`]).
pub const DEFAULT_ADMIN: &str = "admin";

/// Reset a WordPress site to a clean **single-site** install: DROP the
/// database, clear any multisite constants from wp-config.php, and re-run the
/// step-skipping installer with the default local-dev credentials
/// (admin / admin, `admin@<domain>`). Files stay on disk — core, wp-config
/// (same DB name/salts), plugins, themes, uploads; only the database is
/// recreated. Re-runnable: each step skips or tolerates already-done work, so
/// a failure partway is fixed by running it again.
pub fn reset_site(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    domain: &str,
    site_name: &str,
    mysql_basedir: &Path,
) -> Result<()> {
    // 1) Erase: drop the database with the bundled client (PATH-safe).
    let db_name = db_name_for(domain);
    super::database::drop_database(mysql_basedir, super::database::MYSQL_PORT, &db_name)?;

    // 2) Clear multisite constants — best effort per constant (`wp config
    //    delete` errors on one that isn't defined, which is the common case).
    let path = format!("--path={}", docroot.display());
    for constant in MULTISITE_CONSTANTS {
        let _ = wp_cli(php_bin, wp_phar, &["config", "delete", constant, &path], None);
    }

    // 3) Fresh install via the existing re-runnable flow: core download and
    //    wp-config creation skip (files kept), the DB is recreated, and
    //    `wp core install` runs because `is-installed` is now false.
    let db_host = format!("127.0.0.1:{}", super::database::MYSQL_PORT);
    let url = format!("https://{domain}");
    let admin_email = format!("admin@{domain}");
    install_wordpress(
        php_bin,
        wp_phar,
        &WpInstall {
            docroot,
            db_name: &db_name,
            db_host: &db_host,
            mysql_basedir,
            url: &url,
            title: site_name,
            admin_user: DEFAULT_ADMIN,
            admin_password: DEFAULT_ADMIN,
            admin_email: &admin_email,
            locale: "",
        },
    )
}

/// Whether the site still accepts the default local-dev credentials
/// (admin / admin). Backs the tunnel-share warning: locally the default is a
/// convenience, but on a public tunnel URL it's an open wp-admin. Any failure
/// (no `admin` user, broken site, non-WP docroot) reads as `false`.
pub fn default_creds_active(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> bool {
    let path = format!("--path={}", docroot.display());
    wp_cli(
        php_bin,
        wp_phar,
        &["user", "check-password", DEFAULT_ADMIN, DEFAULT_ADMIN, &path],
        None,
    )
    .map(|o| o.status.success())
    .unwrap_or(false)
}

/// The wp-config.php path for a docroot (used by callers/tests).
pub fn wp_config_path(docroot: &Path) -> PathBuf {
    docroot.join("wp-config.php")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_name_sanitizes_domain() {
        assert_eq!(db_name_for("blog.test"), "wp_blog_test");
        assert_eq!(db_name_for("my-site.test"), "wp_my_site_test");
        assert_eq!(db_name_for("a.b.c.test"), "wp_a_b_c_test");
    }

    #[test]
    fn permalink_whitelist_covers_presets_and_only_presets() {
        assert!(PERMALINK_STRUCTURES.contains(&""), "Plain must be offered");
        assert!(PERMALINK_STRUCTURES.contains(&"/%postname%/"));
        // Anything else is rejected before reaching wp-cli.
        for bad in ["/custom/%postname%/", "%postname%", "/index.php/%postname%/"] {
            assert!(!PERMALINK_STRUCTURES.contains(&bad), "{bad:?} should not pass");
        }
    }

    #[test]
    fn debug_flag_whitelist_blocks_arbitrary_constants() {
        for ok in DEBUG_FLAGS {
            assert!(ensure_debug_flag(ok).is_ok(), "{ok} should be allowed");
        }
        // Free-form names would reach wp-cli argv — config injection.
        for bad in ["WP_DEBUG", "DISALLOW_FILE_MODS", "X; rm -rf", "", "wp_debug_log"] {
            assert!(ensure_debug_flag(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn plugin_rows_parse_boolean_update() {
        // Verbatim `wp plugin list --format=json` from a live rexenv site: the
        // must-use rexenv-login row reports `update` as a BOOLEAN — a strict
        // String field made the entire plugin list fail to deserialize.
        let json = r#"[
            {"name":"akismet","status":"inactive","update":"none","version":"5.7"},
            {"name":"rexenv-login","status":"must-use","update":false,"version":""},
            {"name":"other-mu","status":"must-use","update":true,"version":""}
        ]"#;
        let rows: Vec<WpPlugin> = serde_json::from_str(json).unwrap();
        assert_eq!(rows[0].update, "none");
        assert_eq!(rows[1].update, "none");
        assert_eq!(rows[2].update, "available");
    }

    #[test]
    fn theme_screenshot_data_url_and_missing() {
        let docroot = std::env::temp_dir().join(format!("rexenv-shot-{}", std::process::id()));
        let dir = docroot.join("wp-content/themes/twentytwentyfive");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("screenshot.png"), [0x89, b'P', b'N', b'G']).unwrap();

        let url = theme_screenshot(&docroot, "twentytwentyfive").expect("screenshot found");
        assert!(url.starts_with("data:image/png;base64,"), "{url}");
        assert!(theme_screenshot(&docroot, "no-such-theme").is_none());

        std::fs::remove_dir_all(&docroot).unwrap();
    }
}

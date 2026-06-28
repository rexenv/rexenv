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
    #[serde(default)]
    pub update: String,
}

/// `wp plugin list` (name, status, version, update).
pub fn plugin_list(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<Vec<WpPlugin>> {
    wp_json(php_bin, wp_phar, docroot, &["plugin", "list"])
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
/// marks the live theme.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WpTheme {
    pub name: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub update: String,
}

/// `wp theme list` (name, status, version, update).
pub fn theme_list(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<Vec<WpTheme>> {
    wp_json(php_bin, wp_phar, docroot, &["theme", "list"])
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

    // 3) Create the database (ignore "already exists").
    let _ = wp_cli(php_bin, wp_phar, &["db", "create", &path], None);

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
/// is derived from the domain. `db_host` is `host:port` (e.g. `127.0.0.1:13306`).
pub fn install_for_site(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    domain: &str,
    name: &str,
    db_host: &str,
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
    let admin_password =
        if nonempty(&opts.admin_password) { opts.admin_password.clone() } else { "password".into() };
    let url = format!("https://{domain}");
    let db_name = db_name_for(domain);

    install_wordpress(
        php_bin,
        wp_phar,
        &WpInstall {
            docroot,
            db_name: &db_name,
            db_host,
            url: &url,
            title: &title,
            admin_user: &admin_user,
            admin_password: &admin_password,
            admin_email: &admin_email,
            locale: opts.language.trim(),
        },
    )
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
}

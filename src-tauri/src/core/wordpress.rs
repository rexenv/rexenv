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
use std::time::{Duration, Instant};

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

/// Run a command with a hard wall-clock cap: poll `try_wait`, SIGKILL on
/// expiry. For wp-cli subcommands that download from the network — WP's
/// `download_url` waits up to **300s per attempt**, which offline reads as a
/// frozen spinner. Output is drained on reader THREADS while the child runs
/// (the `repo::run_captured_with_cap` lesson): a wait-then-read would deadlock
/// once a chatty child fills the ~64KB pipe buffer and read as a FAKE timeout —
/// a big `plugin list --format=json` must never trip the guard by being long.
pub(crate) fn run_with_timeout(mut cmd: Command, timeout: Duration, what: &str) -> Result<Output> {
    use std::io::Read;
    use std::process::Stdio;
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let mut stdout = child.stdout.take().expect("piped");
    let mut stderr = child.stderr.take().expect("piped");
    let out_t = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = stdout.read_to_end(&mut b);
        b
    });
    let err_t = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = stderr.read_to_end(&mut b);
        b
    });
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait(); // reap — no zombie
            // Deliberately no join here: the readers exit when the killed
            // child's pipes close; blocking on them could hang the caller if
            // anything else held a pipe end (the B7 lesson).
            return Err(Error::Other(format!(
                "{what} timed out after {}s",
                timeout.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    Ok(Output {
        status,
        stdout: out_t.join().unwrap_or_default(),
        stderr: err_t.join().unwrap_or_default(),
    })
}

/// [`wp_cli`] with a wall-clock timeout (see [`run_with_timeout`]).
fn wp_cli_timed(
    php_bin: &Path,
    wp_phar: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<Output> {
    let mut cmd = Command::new(php_bin);
    cmd.arg("-d").arg("memory_limit=512M").arg(wp_phar).args(args);
    let what = format!("wp {}", args.first().copied().unwrap_or(""));
    run_with_timeout(cmd, timeout, &what)
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

/// The ACTIVE theme's directory name (`wp option get stylesheet`) — the
/// unlink-only delete path refuses to remove the active theme's link.
pub fn active_stylesheet(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<String> {
    wp_run(php_bin, wp_phar, docroot, &["option", "get", "stylesheet"])
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

/// Fixed allowance for a download-capable command's non-download work (api
/// lookups, unpack, install, DB writes).
const WP_DOWNLOAD_TIMEOUT_BASE: Duration = Duration::from_secs(120);
/// Per-download allowance. A GENEROUS WALL-CLOCK, deliberately not a stall
/// guard: wp-cli is opaque mid-download under pipe capture (plugin/theme
/// install print nothing between "Downloading…" and "Unpacking…"; core's
/// progress bar is TTY-only), so watching output would read every legit
/// download as a stall. The cap is safe because wp-cli is the TIGHTER bound in
/// the stack: WP's `download_url` caps each plugin/theme download at 300s
/// (verified: wp-admin/includes/file.php) and core's own request at ~600s — a
/// too-slow link fails INSIDE wp-cli with its own error long before 900s, so
/// this only ever catches a wedge outside wp-cli's bounds (hung DNS, a wedged
/// PHP, stuck disk I/O). NOT the B34 mistake: B34's cap was tighter than legit
/// transfers; this one is provably looser than anything wp-cli lets succeed.
const WP_DOWNLOAD_TIMEOUT_PER_ITEM: Duration = Duration::from_secs(900);
/// Bound for the update-checking list calls. WP's update-check request is
/// internally capped at 3s interactive / 30s cron (verified:
/// wp-includes/update.php `'timeout' => $doing_cron ? 30 : 3`), so 300s is
/// ~10× looser than the worst legit case — it only catches a wedged child.
const WP_LIST_TIMEOUT: Duration = Duration::from_secs(300);

/// Wall-clock cap for a command that downloads `items` archives sequentially:
/// scaled, because a multi-slug install legitimately runs N internally-capped
/// downloads back to back — a fixed cap would false-trip exactly the slow-link
/// user the bound must never hurt.
fn download_timeout(items: usize) -> Duration {
    WP_DOWNLOAD_TIMEOUT_BASE + WP_DOWNLOAD_TIMEOUT_PER_ITEM * items as u32
}

/// [`wp_run`] with a hard wall-clock cap (see [`run_with_timeout`]): same
/// `--path` scoping and non-zero-exit mapping, for the network-capable
/// commands a wedged child would otherwise hang forever (B25).
fn wp_run_timed(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<String> {
    let path = format!("--path={}", docroot.display());
    let mut full: Vec<&str> = Vec::with_capacity(args.len() + 1);
    full.extend_from_slice(args);
    full.push(&path);
    let out = wp_cli_timed(php_bin, wp_phar, &full, timeout)?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(Error::Other(format!(
            "wp {} failed (exit {:?}): {}",
            args.first().copied().unwrap_or(""),
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

/// [`wp_json`] with a hard wall-clock cap — for the update-checking list calls
/// (`plugin list` / `theme list`), whose api.wordpress.org refresh hangs the
/// whole listing when the child wedges (B25).
fn wp_json_timed<T: serde::de::DeserializeOwned>(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<T> {
    let mut full: Vec<&str> = Vec::with_capacity(args.len() + 1);
    full.extend_from_slice(args);
    full.push("--format=json");
    let out = wp_run_timed(php_bin, wp_phar, docroot, &full, timeout)?;
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
    /// Human title from the plugin header ("Title:" / "Plugin Name:"); may be
    /// empty for drop-ins whose file has no header — the UI falls back to the
    /// slug then.
    #[serde(default)]
    pub title: String,
}

/// `wp plugin list` (name, status, version, update, title). `check_updates:
/// false` passes `--skip-update-check` — the default check hits
/// api.wordpress.org on EVERY list (seconds when slow, a hang when offline),
/// so the UI lists fast without it and refreshes update badges in a
/// background pass.
pub fn plugin_list(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    check_updates: bool,
) -> Result<Vec<WpPlugin>> {
    let mut args = vec![
        "plugin",
        "list",
        "--fields=name,status,update,version,title",
    ];
    if !check_updates {
        args.push("--skip-update-check");
    }
    wp_json_timed(php_bin, wp_phar, docroot, &args, WP_LIST_TIMEOUT)
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
    let args = item_verb_argv(noun, verb, names);
    match item_verb_timeout(verb, names.len()) {
        // "update" downloads one archive per item from wp.org — the same wedge
        // class as install, same scaled cap (B25). The other verbs
        // (activate/deactivate/delete) are local ops, left untimed.
        Some(t) => wp_run_timed(php_bin, wp_phar, docroot, &args, t),
        None => wp_run(php_bin, wp_phar, docroot, &args),
    }
}

/// The wall-clock cap for an item verb: `update` gets the scaled DOWNLOAD
/// bound (it fetches an archive per item, exactly like install — NOT the flat
/// list bound, which is for metadata-only calls); everything else is a local
/// operation and stays unbounded.
fn item_verb_timeout(verb: &str, items: usize) -> Option<Duration> {
    (verb == "update").then(|| download_timeout(items))
}

/// Argv for a plugin/theme verb over one-or-more items: `[noun, verb, ...names]`.
/// NO `--` separator: WP-CLI does NOT honor the getopt end-of-flags convention —
/// a bare `--` is passed through as a LITERAL positional (a phantom slug), which
/// broke `plugin activate` ("The '--' plugin could not be found"). The names here
/// are real installed slugs from the app's own listings; the arbitrary-source
/// vector is closed separately by `ensure_slugs`/`valid_slug` on INSTALL.
fn item_verb_argv<'a>(noun: &'a str, verb: &'a str, names: &'a [String]) -> Vec<&'a str> {
    let mut args: Vec<&str> = vec![noun, verb];
    args.extend(names.iter().map(String::as_str));
    args
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

/// A wp.org plugin/theme slug: `^[a-z0-9][a-z0-9-]*$`. Mirrors [`valid_locale`] /
/// `parse_wp_version` — refuses argv smuggling (a leading `-` can't become a
/// wp-cli flag) AND install-from-source tricks (a slug is NOT a URL / path / zip:
/// no `:`, `/`, `.`, `_`). Real wp.org slugs always match.
fn valid_slug(slug: &str) -> bool {
    let mut bytes = slug.bytes();
    matches!(bytes.next(), Some(b) if b.is_ascii_lowercase() || b.is_ascii_digit())
        && slug.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Refuse anything that isn't a bare wp.org slug BEFORE it reaches `wp … install`
/// — where a URL/path/zip would install arbitrary code and a leading `-` a flag.
fn ensure_slugs(kind: &str, slugs: &[String]) -> Result<()> {
    for slug in slugs {
        if !valid_slug(slug) {
            return Err(Error::Other(format!(
                "invalid {kind} slug \"{slug}\": expected a wp.org slug (lowercase letters, \
                 digits, hyphens) — installing from a URL, path, or zip isn't supported here."
            )));
        }
    }
    Ok(())
}

/// Install plugins by slug (`wp plugin install <slugs…> [--activate]`) —
/// bulk-capable: one WP-CLI boot installs (and optionally activates) them all.
pub fn plugin_install(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    slugs: &[String],
    activate: bool,
) -> Result<String> {
    if slugs.is_empty() {
        return Ok(String::new());
    }
    ensure_slugs("plugin", slugs)?;
    let mut args: Vec<&str> = vec!["plugin", "install"];
    args.extend(slugs.iter().map(String::as_str));
    if activate {
        args.push("--activate");
    }
    // Scaled cap: one internally-bounded download per slug (B25).
    wp_run_timed(php_bin, wp_phar, docroot, &args, download_timeout(slugs.len()))
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
    let mut themes: Vec<WpTheme> =
        wp_json_timed(php_bin, wp_phar, docroot, &args, WP_LIST_TIMEOUT)?;
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
/// Install themes by slug (`wp theme install <slugs…> [--activate]`) —
/// bulk-capable like [`plugin_install`]; `--activate` applies the LAST slug.
pub fn theme_install(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    slugs: &[String],
    activate: bool,
) -> Result<String> {
    if slugs.is_empty() {
        return Ok(String::new());
    }
    ensure_slugs("theme", slugs)?;
    let mut args: Vec<&str> = vec!["theme", "install"];
    args.extend(slugs.iter().map(String::as_str));
    if activate {
        args.push("--activate");
    }
    // Scaled cap: one internally-bounded download per slug (B25).
    wp_run_timed(php_bin, wp_phar, docroot, &args, download_timeout(slugs.len()))
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
    password: &str,
) -> Result<String> {
    let role_arg = format!("--role={role}");
    // Explicit password (local dev default: a known throwaway) instead of
    // wp-cli's generated one that nobody ever sees. Passed as a single argv
    // element — no shell, no interpolation.
    let pass_arg = format!("--user_pass={password}");
    // Flags first, then `--`, then the positionals: a login/email starting with
    // `-` is a positional, never a wp-cli flag (e.g. can't smuggle --role=admin).
    wp_run(
        php_bin,
        wp_phar,
        docroot,
        &["user", "create", &role_arg, &pass_arg, "--porcelain", login, email],
    )
}

/// Set an existing user's password (`wp user update --user_pass`). wp-cli
/// does not email the user; sessions stay valid per WordPress semantics.
pub fn user_set_password(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    user_id: u64,
    password: &str,
) -> Result<String> {
    if password.is_empty() {
        return Err(Error::Other("password must not be empty".into()));
    }
    let pass_arg = format!("--user_pass={password}");
    wp_run(
        php_bin,
        wp_phar,
        docroot,
        &["user", "update", &user_id.to_string(), &pass_arg],
    )
}

/// Stock roles assignable from the UI. Whitelisted like [`DEBUG_FLAGS`]: the
/// role lands in wp-cli argv.
pub const USER_ROLES: [&str; 5] =
    ["administrator", "editor", "author", "contributor", "subscriber"];

/// Change a user's role (`wp user set-role` — replaces all current roles, same
/// as wp-admin's dropdown). Whitelisted roles only, and the PRIMARY
/// administrator ([`primary_admin_id`]) is protected: demoting it breaks
/// one-click admin login / site tools and can lock the install out of
/// wp-admin entirely.
pub fn user_set_role(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    user_id: u64,
    role: &str,
) -> Result<String> {
    if !USER_ROLES.contains(&role) {
        return Err(Error::Other(format!("not an assignable role: {role}")));
    }
    if user_id == primary_admin_id(php_bin, wp_phar, docroot)? {
        return Err(Error::Other(
            "the primary administrator's role is protected — one-click admin login and \
             rexenv's site tools depend on it. Create another administrator first."
                .into(),
        ));
    }
    wp_run(
        php_bin,
        wp_phar,
        docroot,
        &["user", "set-role", &user_id.to_string(), role],
    )
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
    wp_run(php_bin, wp_phar, docroot, &["site", "delete", "--yes", blog_id])
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

/// One scheduled cron event (Tools → Cron).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpCronEvent {
    pub hook: String,
    /// GMT timestamp, WP-CLI's `next_run_gmt` (e.g. `2026-07-11 12:00:00`).
    pub next_run: String,
    /// Human offset, WP-CLI's `next_run_relative` (e.g. `11 hours 4 minutes`).
    pub next_run_relative: String,
    /// `1 hour`, `1 day`, … or `Non-repeating`.
    pub recurrence: String,
}

// `wp cron event list --format=json` wire shape (WP-CLI's snake_case keys).
#[derive(Deserialize)]
struct WireCronEvent {
    #[serde(default)]
    hook: String,
    #[serde(default)]
    next_run_gmt: String,
    #[serde(default)]
    next_run_relative: String,
    #[serde(default)]
    recurrence: String,
}

/// List scheduled cron events, soonest first (WP-CLI's default order).
pub fn cron_event_list(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<Vec<WpCronEvent>> {
    let wire: Vec<WireCronEvent> = wp_json(
        php_bin,
        wp_phar,
        docroot,
        &["cron", "event", "list", "--fields=hook,next_run_gmt,next_run_relative,recurrence"],
    )?;
    Ok(wire
        .into_iter()
        .map(|e| WpCronEvent {
            hook: e.hook,
            next_run: e.next_run_gmt,
            next_run_relative: e.next_run_relative,
            recurrence: e.recurrence,
        })
        .collect())
}

/// Run every cron event that is currently due (`wp cron event run --due-now`).
/// Returns WP-CLI's "Executed a total of N cron events" message.
pub fn cron_run_due(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<String> {
    wp_run(php_bin, wp_phar, docroot, &["cron", "event", "run", "--due-now"])
}

/// Run ONE hook's scheduled event(s) immediately, due or not
/// (`wp cron event run <hook>`). WP-CLI addresses cron events by hook name —
/// there is no per-instance id — so a hook scheduled more than once runs every
/// instance. Hook names are site-defined (no whitelist possible); they pass as
/// a single argv element, never through a shell.
pub fn cron_run_hook(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    hook: &str,
) -> Result<String> {
    let hook = hook.trim();
    if hook.is_empty() {
        return Err(Error::Other("empty cron hook name".into()));
    }
    wp_run(php_bin, wp_phar, docroot, &cron_run_hook_argv(hook))
}

/// Argv for running one cron hook: `[cron, event, run, hook]`. NO `--` — WP-CLI
/// treats a bare `--` as a literal positional, so `["cron","event","run","--",h]`
/// made WP-CLI see `--` as the hook name ("Invalid cron event '--'"). The hook
/// passes as a single argv element, never through a shell.
fn cron_run_hook_argv(hook: &str) -> [&str; 4] {
    ["cron", "event", "run", hook]
}

/// Result of `wp core verify-checksums` (Tools → Maintenance). A failed
/// verification is a RESULT, not an `Err` — errors are reserved for wp-cli
/// itself failing to run. Findings are split so the UI can tell harmless OS
/// clutter from a genuinely modified install:
/// - `real`: modified core files, missing core files, and foreign files that
///   are NOT known OS noise (plus any warning we don't recognize — unknown
///   stays loud, never silently benign).
/// - `benign`: foreign files whose basename is known OS/Finder clutter
///   (`.DS_Store`, AppleDouble `._*`, `Thumbs.db`, …). ONLY "should not
///   exist" findings can be benign — a modified/missing core file never is.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpChecksumReport {
    /// Raw wp-cli exit verdict. CAVEAT (verified live): extra "should not
    /// exist" files do NOT fail the command — it exits 0 with a Success line
    /// despite those warnings; only modified/missing core files exit 1. So
    /// `ok` alone must never drive a pass decision; `real` is the signal.
    pub ok: bool,
    pub real: Vec<String>,
    pub benign: Vec<String>,
    /// Raw combined stdout+stderr (warnings arrive on stderr).
    pub output: String,
}

/// OS/editor clutter that Finder & co. drop into directories — matched on the
/// path's basename only, so `foo/.DS_Store-backdoor.php` stays a real finding.
fn is_os_noise(path: &str) -> bool {
    const NOISE: [&str; 7] = [
        ".DS_Store",
        "Thumbs.db",
        "desktop.ini",
        ".Spotlight-V100",
        ".fseventsd",
        ".Trashes",
        ".localized",
    ];
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    NOISE.contains(&name) || name.starts_with("._")
}

/// Split WP-CLI's verify-checksums warnings into real vs benign (see
/// [`WpChecksumReport`]). The trailing "Error: … doesn't verify" summary line
/// carries no per-file info and is skipped.
fn classify_checksum_output(text: &str) -> (Vec<String>, Vec<String>) {
    let mut real = Vec::new();
    let mut benign = Vec::new();
    for line in text.lines() {
        let Some(msg) = line.trim().strip_prefix("Warning: ") else {
            continue;
        };
        match msg.strip_prefix("File should not exist: ") {
            Some(path) if is_os_noise(path) => benign.push(path.to_string()),
            // Non-noise extras, modified ("doesn't verify against checksum"),
            // missing ("doesn't exist"), and anything unrecognized: loud.
            _ => real.push(msg.to_string()),
        }
    }
    (real, benign)
}

/// Verify core files against wordpress.org's checksums for the installed
/// version. Detects modified/missing core files and foreign files in core dirs.
pub fn core_verify_checksums(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
) -> Result<WpChecksumReport> {
    let path = format!("--path={}", docroot.display());
    let out = wp_cli(php_bin, wp_phar, &["core", "verify-checksums", &path], None)?;
    let mut text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !err.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&err);
    }
    let (real, benign) = classify_checksum_output(&text);
    Ok(WpChecksumReport { ok: out.status.success(), real, benign, output: text })
}

/// One file [`cleanup_os_noise`] did NOT delete, with the reason.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedNoiseFile {
    pub path: String,
    pub reason: String,
}

/// Result of [`cleanup_os_noise`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoiseCleanup {
    pub removed: u32,
    pub skipped: Vec<SkippedNoiseFile>,
}

/// Delete known-noise files (the checksum panel's `benign` list) from a
/// docroot. The paths come from the UI and are NEVER trusted — every file is
/// re-validated from scratch by [`noise_delete_one`]'s guards; anything that
/// fails a guard or errors is SKIPPED with a reason, never aborting the rest.
/// Deletion is a direct `remove_file` (not Trash): the entire reachable scope
/// is regenerable Finder metadata.
pub fn cleanup_os_noise(docroot: &Path, paths: &[String]) -> Result<NoiseCleanup> {
    let root = docroot
        .canonicalize()
        .map_err(|e| Error::Other(format!("site folder {}: {e}", docroot.display())))?;
    let mut removed = 0u32;
    let mut skipped = Vec::new();
    for rel in paths {
        match noise_delete_one(&root, rel) {
            Ok(()) => removed += 1,
            Err(reason) => skipped.push(SkippedNoiseFile { path: rel.clone(), reason }),
        }
    }
    Ok(NoiseCleanup { removed, skipped })
}

/// [`cleanup_os_noise`] outcome + the fresh post-cleanup verify report, so the
/// UI panel updates in the same round-trip (mirrors frontend `WpChecksumCleanup`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChecksumCleanup {
    pub removed: u32,
    pub skipped: Vec<SkippedNoiseFile>,
    pub report: WpChecksumReport,
}

/// The four delete guards, per file (`canon_root` is the canonicalized docroot):
/// 1. basename passes [`is_os_noise`] — the SAME list classification uses;
/// 2. lexical: relative with only normal components (no `..`, no absolute);
/// 3. the entry itself (lstat) is a regular file — this, not canonicalize,
///    is what stops symlinks: a symlink named `.DS_Store` canonicalizes to its
///    TARGET, and if that target is a regular file inside the docroot (say
///    `wp-config.php`) the canonical-path checks all pass — deleting the
///    target. lstat sees the link itself and refuses anything but a plain file;
/// 4. the canonicalized path stays under the canonical docroot — catches the
///    remaining escape: a symlinked intermediate DIRECTORY resolving outside.
fn noise_delete_one(canon_root: &Path, rel: &str) -> std::result::Result<(), String> {
    if !is_os_noise(rel) {
        return Err("not a known macOS system file".into());
    }
    let rel_path = Path::new(rel);
    if rel_path.is_absolute()
        || !rel_path
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
    {
        return Err("path escapes the site folder".into());
    }
    let joined = canon_root.join(rel_path);
    let lmeta = joined
        .symlink_metadata()
        .map_err(|e| format!("cannot stat: {e}"))?;
    if lmeta.file_type().is_symlink() {
        return Err("symbolic link — not followed".into());
    }
    if !lmeta.is_file() {
        return Err("not a regular file".into());
    }
    let canon = joined
        .canonicalize()
        .map_err(|e| format!("cannot resolve: {e}"))?;
    if !canon.starts_with(canon_root) {
        return Err("resolves outside the site folder".into());
    }
    std::fs::remove_file(&canon).map_err(|e| format!("cannot delete: {e}"))
}

/// Export site content (posts, pages, comments, menus, terms) as WXR XML into
/// the user's Downloads folder (`wp export --dir=…` — same destination
/// convention as the DB export). WP-CLI names the files itself
/// (`<site>.WordPress.<date>.xml`) and may split a large export into several;
/// the paths are parsed from its "Writing to file" lines.
pub fn content_export_to_downloads(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
) -> Result<Vec<String>> {
    let downloads = directories::UserDirs::new()
        .and_then(|u| u.download_dir().map(|p| p.to_path_buf()))
        .ok_or_else(|| Error::Other("could not resolve the Downloads folder".into()))?;
    let dir_arg = format!("--dir={}", downloads.display());
    let out = wp_run(php_bin, wp_phar, docroot, &["export", &dir_arg])?;
    let files: Vec<String> = out
        .lines()
        .filter_map(|l| l.trim().strip_prefix("Writing to file "))
        .map(|p| p.trim().to_string())
        .collect();
    if files.is_empty() {
        return Err(Error::Other(format!(
            "wp export reported no output file — output was: {out}"
        )));
    }
    Ok(files)
}

/// Flush the WordPress object cache (`wp cache flush`).
pub fn cache_flush(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<String> {
    wp_run(php_bin, wp_phar, docroot, &["cache", "flush"])
}

/// Delete ALL transients — expired or not (`wp transient delete --all`).
/// Returns WP-CLI's "N transients deleted" message for the UI toast.
pub fn transient_delete_all(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<String> {
    wp_run(php_bin, wp_phar, docroot, &["transient", "delete", "--all"])
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
/// wp-cli walks serialized PHP data correctly — this is the safe way to rewrite
/// URLs, never raw SQL. `all_tables` adds `--all-tables` (every table in the
/// site's database, not just the ones matching the WP prefix) — each site owns
/// its database, so this is safe and what a domain change needs.
pub fn search_replace(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    from: &str,
    to: &str,
    dry_run: bool,
    all_tables: bool,
) -> Result<u64> {
    let mut args: Vec<&str> = vec!["search-replace", "--format=count"];
    if all_tables {
        args.push("--all-tables");
    }
    if dry_run {
        args.push("--dry-run");
    }
    // NO `--` separator: WP-CLI doesn't honor getopt end-of-flags (a bare `--`
    // becomes a literal positional). from/to follow the flags directly; a term
    // that itself looks like a flag is a WP-CLI limitation, not something `--`
    // could fix — and it's the user's own dev DB (same-privilege footgun).
    args.push(from);
    args.push(to);
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
    wp_run_timed(php_bin, wp_phar, docroot, &["core", "update"], download_timeout(1))
}

/// Re-download core files of the current version (`wp core download --force`) —
/// repairs a corrupt/modified core without touching the DB or wp-content.
pub fn core_reinstall(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<String> {
    wp_run_timed(
        php_bin,
        wp_phar,
        docroot,
        &["core", "download", "--force", "--skip-content"],
        download_timeout(1),
    )
}

/// Input/validation kind of a whitelisted option (drives the UI input AND the
/// backend validation — both sides of the same rule).
#[derive(Debug, Clone, Copy, PartialEq)]
enum OptionKind {
    Text,
    Email,
    /// Integer within `[min, max]` inclusive.
    IntRange(i64, i64),
    /// `"0"` / `"1"` (WP stores booleans as those strings).
    Bool,
    /// Day-of-week int `0..=6` (0 = Sunday).
    Weekday,
    /// A PHP `timezone_identifiers_list()` entry, or empty (site uses a raw
    /// UTC offset via `gmt_offset` — a legitimate state, seen live).
    Timezone,
    /// A role slug from `wp role list` (covers custom roles).
    Role,
}

struct OptionField {
    name: &'static str,
    label: &'static str,
    kind: OptionKind,
}

/// The options editor's ENTIRE reachable surface — default-deny. Nothing
/// outside this list can be read for editing or written, so foot-guns
/// (`siteurl`, `home`, `active_plugins`, `template`, any serialized option…)
/// aren't "blocked", they're unreachable by construction. Every entry is a
/// scalar on a standard install (verified live); non-scalar values are refused
/// at read AND write time anyway. `WPLANG` is deliberately absent — the
/// Language card owns it (install/download flow).
const OPTION_FIELDS: &[OptionField] = &[
    OptionField { name: "blogname", label: "Site title", kind: OptionKind::Text },
    OptionField { name: "blogdescription", label: "Tagline", kind: OptionKind::Text },
    OptionField { name: "admin_email", label: "Admin email", kind: OptionKind::Email },
    OptionField { name: "timezone_string", label: "Timezone", kind: OptionKind::Timezone },
    OptionField { name: "date_format", label: "Date format", kind: OptionKind::Text },
    OptionField { name: "time_format", label: "Time format", kind: OptionKind::Text },
    OptionField { name: "start_of_week", label: "Week starts on", kind: OptionKind::Weekday },
    OptionField {
        name: "posts_per_page",
        label: "Posts per page",
        kind: OptionKind::IntRange(1, 1000),
    },
    OptionField { name: "default_role", label: "New user default role", kind: OptionKind::Role },
    OptionField {
        name: "users_can_register",
        label: "Anyone can register",
        kind: OptionKind::Bool,
    },
    OptionField {
        name: "blog_public",
        label: "Visible to search engines",
        kind: OptionKind::Bool,
    },
];

fn option_field(name: &str) -> Option<&'static OptionField> {
    OPTION_FIELDS.iter().find(|f| f.name == name)
}

/// Frontend tag for an [`OptionKind`].
fn option_kind_str(kind: OptionKind) -> &'static str {
    match kind {
        OptionKind::Text => "text",
        OptionKind::Email => "email",
        OptionKind::IntRange(..) => "int",
        OptionKind::Bool => "bool",
        OptionKind::Weekday => "weekday",
        OptionKind::Timezone => "timezone",
        OptionKind::Role => "role",
    }
}

/// A JSON scalar as its WP display/storage string; `None` for array/object/
/// null — the "never round-trip PHP serialization" guard.
fn scalar_display(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(if *b { "1" } else { "0" }.into()),
        _ => None,
    }
}

/// Minimal email shape check (`local@domain.tld`, no whitespace) — enough to
/// stop typos; WP itself does no validation on a direct option write.
fn valid_email(v: &str) -> bool {
    let Some((local, domain)) = v.split_once('@') else { return false };
    !local.is_empty()
        && !domain.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !v.contains(char::is_whitespace)
}

/// Backend per-kind validation — runs on every write regardless of what the
/// UI already checked. `timezones`/`roles` are only consulted for those kinds.
fn validate_option_value(
    kind: OptionKind,
    value: &str,
    timezones: &[String],
    roles: &[String],
) -> std::result::Result<(), String> {
    let int_in = |min: i64, max: i64| {
        value
            .parse::<i64>()
            .ok()
            .filter(|n| (min..=max).contains(n))
            .map(|_| ())
            .ok_or(format!("must be a whole number between {min} and {max}"))
    };
    match kind {
        OptionKind::Text => Ok(()),
        OptionKind::Email => {
            if valid_email(value) {
                Ok(())
            } else {
                Err("not a valid email address".into())
            }
        }
        OptionKind::IntRange(min, max) => int_in(min, max),
        OptionKind::Bool => {
            if matches!(value, "0" | "1") {
                Ok(())
            } else {
                Err("must be 0 or 1".into())
            }
        }
        OptionKind::Weekday => int_in(0, 6),
        OptionKind::Timezone => {
            if value.is_empty() || timezones.iter().any(|t| t == value) {
                Ok(())
            } else {
                Err("not a known timezone".into())
            }
        }
        OptionKind::Role => {
            if roles.iter().any(|r| r == value) {
                Ok(())
            } else {
                Err("not an existing role".into())
            }
        }
    }
}

/// One editable (or refused) option row (mirrors the frontend `WpOptionRow`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpOptionRow {
    pub name: String,
    pub label: String,
    pub kind: String,
    pub min: Option<i64>,
    pub max: Option<i64>,
    pub value: String,
    pub editable: bool,
    pub note: Option<String>,
}

/// A role (`wp role list` row).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WpRole {
    pub name: String,
    pub role: String,
}

/// The whole options form: rows + the choice lists the pickers need.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpOptionsForm {
    pub fields: Vec<WpOptionRow>,
    pub timezones: Vec<String>,
    pub roles: Vec<WpRole>,
}

/// Fixed `wp eval` script reading every whitelisted option in ONE wp-cli call
/// (11 separate `option get`s would cost ~10s of WP boots) plus the timezone
/// list. Built ONLY from `OPTION_FIELDS` consts — no user input reaches it.
fn options_eval_script() -> String {
    let names = OPTION_FIELDS
        .iter()
        .map(|f| format!("\"{}\"", f.name))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "$n=[{names}];$v=[];foreach($n as $x){{$v[$x]=get_option($x);}}\
         echo json_encode([\"values\"=>$v,\"timezones\"=>timezone_identifiers_list()]);"
    )
}

/// Read the options form. `get_option` unserializes, so a serialized value
/// arrives as a JSON array/object → refused per row (`editable: false`,
/// "not editable (non-scalar value)") rather than shown as corruptible text.
pub fn options_get(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<WpOptionsForm> {
    #[derive(Deserialize)]
    struct Eval {
        values: serde_json::Map<String, serde_json::Value>,
        timezones: Vec<String>,
    }
    let out = wp_run(php_bin, wp_phar, docroot, &["eval", &options_eval_script()])?;
    let ev: Eval = serde_json::from_str(out.trim())
        .map_err(|e| Error::Other(format!("options read: bad JSON: {e}")))?;
    let roles: Vec<WpRole> = wp_json(php_bin, wp_phar, docroot, &["role", "list"])?;

    let fields = OPTION_FIELDS
        .iter()
        .map(|f| {
            let (min, max) = match f.kind {
                OptionKind::IntRange(a, b) => (Some(a), Some(b)),
                OptionKind::Weekday => (Some(0), Some(6)),
                _ => (None, None),
            };
            let mut row = WpOptionRow {
                name: f.name.into(),
                label: f.label.into(),
                kind: option_kind_str(f.kind).into(),
                min,
                max,
                value: String::new(),
                editable: false,
                note: None,
            };
            match ev.values.get(f.name).map(scalar_display) {
                Some(Some(v)) => {
                    row.value = v;
                    row.editable = true;
                }
                Some(None) => row.note = Some("not editable (non-scalar value)".into()),
                None => row.note = Some("could not read".into()),
            }
            row
        })
        .collect();
    Ok(WpOptionsForm { fields, timezones: ev.timezones, roles })
}

/// Update ONE whitelisted option. Guards, in order:
/// 1. `name` must be in `OPTION_FIELDS` — anything else ("siteurl", "home",
///    "active_plugins", …) is "not an editable option", checked BEFORE any
///    wp-cli call, so a bypassed UI still can't write them;
/// 2. per-kind value validation (choice kinds fetch their live list);
/// 3. the CURRENT value must be a JSON scalar — a serialized option is never
///    overwritten even if its name were whitelisted.
pub fn option_update(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    name: &str,
    value: &str,
) -> Result<()> {
    let field =
        option_field(name).ok_or_else(|| Error::Other(format!("not an editable option: {name}")))?;

    let timezones: Vec<String> = if field.kind == OptionKind::Timezone {
        let out =
            wp_run(php_bin, wp_phar, docroot, &["eval", "echo json_encode(timezone_identifiers_list());"])?;
        serde_json::from_str(out.trim())
            .map_err(|e| Error::Other(format!("timezone list: bad JSON: {e}")))?
    } else {
        Vec::new()
    };
    let roles: Vec<String> = if field.kind == OptionKind::Role {
        let rows: Vec<WpRole> = wp_json(php_bin, wp_phar, docroot, &["role", "list"])?;
        rows.into_iter().map(|r| r.role).collect()
    } else {
        Vec::new()
    };
    validate_option_value(field.kind, value, &timezones, &roles)
        .map_err(|reason| Error::Other(format!("{}: {reason}", field.label)))?;

    let cur = wp_run(php_bin, wp_phar, docroot, &["option", "get", name, "--format=json"])?;
    let cur_v: serde_json::Value = serde_json::from_str(cur.trim())
        .map_err(|e| Error::Other(format!("option {name}: bad JSON: {e}")))?;
    if scalar_display(&cur_v).is_none() {
        return Err(Error::Other(format!("{name} is not editable (non-scalar value)")));
    }

    wp_run(php_bin, wp_phar, docroot, &["option", "update", name, value])?;
    Ok(())
}

/// One row of `wp language core list` (mirrors the frontend `WpLanguage`).
/// `status` is `active` | `installed` | `uninstalled`; `en_US` is always
/// present (the built-in default — activating it needs no files).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WpLanguage {
    pub language: String,
    #[serde(default, alias = "english_name")]
    pub english_name: String,
    #[serde(default, alias = "native_name")]
    pub native_name: String,
    #[serde(default)]
    pub status: String,
}

/// Available + installed core languages (`wp language core list`). Hits
/// api.wordpress.org for the available-translations list (~3s; needs network).
pub fn language_list(php_bin: &Path, wp_phar: &Path, docroot: &Path) -> Result<Vec<WpLanguage>> {
    wp_json(php_bin, wp_phar, docroot, &["language", "core", "list"])
}

/// Locale shape guard (`fr_FR`, `pt_BR`, `de_DE_formal`, `ceb`): 2–20 chars,
/// leading lowercase ASCII letter, then letters/digits/underscore only. The UI
/// is a picker fed by [`language_list`]; this is defense in depth so a locale
/// string can never look like a wp-cli flag or smuggle anything into argv.
pub fn valid_locale(locale: &str) -> bool {
    (2..=20).contains(&locale.len())
        && locale.starts_with(|c: char| c.is_ascii_lowercase())
        && locale.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Whether a core language pack is on disk (`wp language core is-installed`
/// exits 0/1). This is the ONLY trustworthy install signal — see
/// [`switch_language`].
fn language_is_installed(php_bin: &Path, wp_phar: &Path, docroot: &Path, locale: &str) -> bool {
    let path = format!("--path={}", docroot.display());
    wp_cli(php_bin, wp_phar, &["language", "core", "is-installed", locale, &path], None)
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Switch the site language in one step: install the core pack if missing, then
/// activate via `wp site switch-language` (`language core activate` is
/// deprecated in wp-cli 2.12). Reversible — switching to `en_US` restores the
/// default (empties `WPLANG`).
///
/// `language core install` LIES on failure: an unavailable locale or a failed/
/// offline download still exits **0** ("Installed 0 of 1 languages (1 skipped)")
/// with only a stderr warning. Success is therefore gated on `is-installed`
/// AFTER the install — never on install's exit code or output. On a failed
/// download nothing was activated, so the site language is unchanged.
/// Cap on the language-pack download. WP's own `download_url` waits up to 300s
/// per attempt, so offline the install "hangs" for minutes at a spinner. 60s is
/// generous for a ~4MB pack on a slow line and keeps the failure user-visible.
const LANG_INSTALL_TIMEOUT: Duration = Duration::from_secs(60);

pub fn switch_language(php_bin: &Path, wp_phar: &Path, docroot: &Path, locale: &str) -> Result<()> {
    if !valid_locale(locale) {
        return Err(Error::Other(format!("invalid locale: {locale:?}")));
    }
    if !language_is_installed(php_bin, wp_phar, docroot, locale) {
        let path = format!("--path={}", docroot.display());
        // Timed AND its result deliberately not trusted: install lies (exit 0
        // on a failed download) and stalls for minutes offline. Whatever it
        // claims — success, error, or timeout — `is-installed` below is the
        // only verdict; the outcome only feeds the error detail.
        let detail = match wp_cli_timed(
            php_bin,
            wp_phar,
            &["language", "core", "install", locale, &path],
            LANG_INSTALL_TIMEOUT,
        ) {
            Ok(out) => String::from_utf8_lossy(&out.stderr).trim().to_string(),
            Err(e) => e.to_string(),
        };
        if !language_is_installed(php_bin, wp_phar, docroot, locale) {
            return Err(Error::Other(format!(
                "Couldn't download the {locale} language pack — check your connection.{}",
                if detail.is_empty() { String::new() } else { format!(" ({detail})") }
            )));
        }
    }
    wp_run(php_bin, wp_phar, docroot, &["site", "switch-language", locale])?;
    Ok(())
}

/// One WordPress release from the stable-check API (mirrors the frontend
/// `WpCoreVersion`). `status` ∈ `latest` | `outdated` | `insecure`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpCoreVersion {
    pub version: String,
    pub status: String,
}

/// `"X.Y"` / `"X.Y.Z"` → sortable tuple; `None` for anything else. Doubles as
/// the argv shape guard: digits and dots only, so a version can never read as
/// a wp-cli flag.
fn parse_wp_version(v: &str) -> Option<(u64, u64, u64)> {
    if v.is_empty() || !v.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    let mut it = v.split('.');
    let a = it.next()?.parse().ok()?;
    let b = it.next()?.parse().ok()?;
    let c = match it.next() {
        Some(s) => s.parse().ok()?,
        None => 0,
    };
    if it.next().is_some() {
        return None;
    }
    Some((a, b, c))
}

const STABLE_CHECK_URL: &str = "https://api.wordpress.org/core/stable-check/1.0/";

/// Installable releases, newest first, from wordpress.org's stable-check API
/// (`version → status`; 800+ entries live). Filtered to ≥ 6.0 — older cores
/// predate the bundled PHP versions. Needs network; offline surfaces the
/// download error, and the picker simply doesn't load.
pub async fn core_versions() -> Result<Vec<WpCoreVersion>> {
    let body = super::binaries::http_get(STABLE_CHECK_URL).await?;
    parse_stable_check(&body)
}

fn parse_stable_check(body: &[u8]) -> Result<Vec<WpCoreVersion>> {
    let map: std::collections::BTreeMap<String, String> = serde_json::from_slice(body)
        .map_err(|e| Error::Other(format!("version list: bad JSON: {e}")))?;
    let mut rows: Vec<((u64, u64, u64), WpCoreVersion)> = map
        .into_iter()
        .filter_map(|(version, status)| {
            parse_wp_version(&version)
                .filter(|t| t.0 >= 6)
                .map(|t| (t, WpCoreVersion { version, status }))
        })
        .collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(rows.into_iter().map(|(_, v)| v).collect())
}

/// Result of a core version switch (mirrors the frontend `WpCoreSwitch`).
/// `db_update_required` tells the panel — explicitly, not guessed — whether
/// wp-admin will show the "Database Update Required" screen: WP redirects on
/// ANY `db_version` mismatch (`wp-admin/admin.php`), including DB-newer-than-
/// code after a downgrade; running it re-stamps the option (`upgrade.php`) —
/// the schema itself is never downgraded.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpCoreSwitch {
    pub version: String,
    pub db_update_required: bool,
}

/// Cap on the core zip download (~25 MB — needs more headroom than the 60s
/// language cap; still bounded so offline fails visibly, never a frozen
/// spinner).
const CORE_SWITCH_TIMEOUT: Duration = Duration::from_secs(300);

/// Switch core to an exact version: `wp core update --version=<v> --force`
/// (`--force` is the official downgrade path — "update even when installed WP
/// version is greater than the requested version"). `allowed` is a fresh
/// stable-check list; the version must be on it (picker-only, defense in
/// depth). Success is gated on `wp core version` reporting the target
/// afterward — never on the update command's claim (the language/checksum
/// exit-code lesson). wp-content and the DB are untouched.
pub fn core_switch_version(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    version: &str,
    allowed: &[String],
) -> Result<WpCoreSwitch> {
    if parse_wp_version(version).is_none() {
        return Err(Error::Other(format!("invalid version: {version:?}")));
    }
    if !allowed.iter().any(|v| v == version) {
        return Err(Error::Other(format!("{version} is not a known WordPress release")));
    }

    let path = format!("--path={}", docroot.display());
    let varg = format!("--version={version}");
    let update = wp_cli_timed(
        php_bin,
        wp_phar,
        &["core", "update", &varg, "--force", &path],
        CORE_SWITCH_TIMEOUT,
    );
    // The gate: what does core ACTUALLY report now?
    let now = wp_run(php_bin, wp_phar, docroot, &["core", "version"])?;
    if now.trim() != version {
        let detail = match update {
            Ok(out) => String::from_utf8_lossy(&out.stderr).trim().to_string(),
            Err(e) => e.to_string(),
        };
        return Err(Error::Other(format!(
            "Couldn't switch to WordPress {version} — core still reports {now}. Check your connection.{}",
            if detail.is_empty() { String::new() } else { format!(" ({detail})") }
        )));
    }

    // Explicit DB answer for the panel: code's $wp_db_version vs the stored option.
    #[derive(Deserialize)]
    struct DbProbe {
        code: i64,
        db: i64,
    }
    let probe = wp_run(
        php_bin,
        wp_phar,
        docroot,
        &[
            "eval",
            "global $wp_db_version; echo json_encode([\"code\"=>(int)$wp_db_version,\"db\"=>(int)get_option(\"db_version\")]);",
        ],
    )?;
    let db: DbProbe = serde_json::from_str(probe.trim())
        .map_err(|e| Error::Other(format!("db-version probe: bad JSON: {e}")))?;

    Ok(WpCoreSwitch { version: version.into(), db_update_required: db.code != db.db })
}

/// A valid MySQL database name derived from a site domain
/// (`blog.test` → `wp_blog_test`). CREATION-TIME ONLY: the result is stored on
/// the site row (`Site::db_name`, backfilled by migration v6 with identical
/// logic) and every runtime operation reads the stored value — deriving from
/// the domain at runtime would break sites whose domain has changed.
pub fn db_name_for(domain: &str) -> String {
    let safe: String = domain
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("wp_{safe}")
}

/// MySQL/MariaDB identifier limit — a database name may not exceed this.
pub const DB_NAME_MAX: usize = 64;

/// A hash-disambiguated database name for a domain: the [`db_name_for`] base
/// truncated to fit, plus a stable FNV-1a hash of the FULL domain. Used at
/// create ONLY when the clean base would collide with an existing site or exceed
/// the 64-char identifier limit ([`crate::core::sites`] decides). `db_name_for`
/// is NOT injective — it maps every non-alphanumeric char to `_`, so
/// `a-b.test` and `a.b.test` both reduce to `wp_a_b_test`; the domain hash makes
/// two such sites land in DISTINCT databases instead of silently sharing one.
pub fn db_name_disambiguated(domain: &str) -> String {
    let base = db_name_for(domain);
    let suffix = format!("{:08x}", fnv1a(domain.as_bytes()));
    let keep = DB_NAME_MAX - 1 - suffix.len(); // reserve "_" + the 8-hex suffix
    let head: String = base.chars().take(keep).collect();
    format!("{head}_{suffix}")
}

/// FNV-1a (32-bit) — a small, dependency-free stable hash (the same primitive
/// the per-site override-port allocator uses).
fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// Inputs for a one-click WordPress install. Phase 1 installs **single-site**.
pub struct WpInstall<'a> {
    pub docroot: &'a Path,
    pub db_name: &'a str,
    /// `host:port`, e.g. `127.0.0.1:13306`.
    pub db_host: &'a str,
    /// Bundled MySQL-protocol client BINARY that creates the DB (`bin/mysql`
    /// from the MySQL tree, or `bin/mariadb` from the mariadb bundle).
    pub db_client: &'a Path,
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
    super::database::create_database(opts.db_client, port, opts.db_name)?;

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
/// to [`install_wordpress`]. The canonical URL is `https://<domain>`; `db_name`
/// is the site's STORED database name (`Site::db_name` — derived once at
/// creation, never from the current domain). `db_host` is `host:port` (e.g.
/// `127.0.0.1:13306`); `db_client` is the site engine's bundled client binary
/// (creates the DB).
#[allow(clippy::too_many_arguments)] // flat mirror of the New Site dialog inputs
pub fn install_for_site(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    domain: &str,
    name: &str,
    db_name: &str,
    db_host: &str,
    db_client: &Path,
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

    install_wordpress(
        php_bin,
        wp_phar,
        &WpInstall {
            docroot,
            db_name,
            db_host,
            db_client,
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
/// (admin / admin, `admin@<domain>`). `db_name` is the site's STORED database
/// name (`Site::db_name`), never re-derived from the domain. Files stay on
/// disk — core, wp-config (same DB name/salts), plugins, themes, uploads; only
/// the database is recreated. Re-runnable: each step skips or tolerates
/// already-done work, so a failure partway is fixed by running it again.
#[allow(clippy::too_many_arguments)] // flat per-site tool set, mirrors install_for_site
pub fn reset_site(
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    domain: &str,
    site_name: &str,
    db_name: &str,
    db_client: &Path,
    db_port: u16,
) -> Result<()> {
    // 1) Erase: drop the database with the bundled client (PATH-safe).
    super::database::drop_database(db_client, db_port, db_name)?;

    // 2) Clear multisite constants — best effort per constant (`wp config
    //    delete` errors on one that isn't defined, which is the common case).
    let path = format!("--path={}", docroot.display());
    for constant in MULTISITE_CONSTANTS {
        let _ = wp_cli(php_bin, wp_phar, &["config", "delete", constant, &path], None);
    }

    // 3) Fresh install via the existing re-runnable flow: core download and
    //    wp-config creation skip (files kept), the DB is recreated, and
    //    `wp core install` runs because `is-installed` is now false.
    let db_host = format!("127.0.0.1:{db_port}");
    let url = format!("https://{domain}");
    let admin_email = format!("admin@{domain}");
    install_wordpress(
        php_bin,
        wp_phar,
        &WpInstall {
            docroot,
            db_name,
            db_host: &db_host,
            db_client,
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
    fn db_name_disambiguated_is_injective_and_bounded() {
        // The whole point: two domains that `db_name_for` reduces to the SAME
        // slug get DISTINCT disambiguated names (the domain hash differs).
        let a = db_name_disambiguated("my-shop.test");
        let b = db_name_disambiguated("my.shop.test");
        assert_ne!(a, b, "slug-colliding domains must not share a database");
        assert!(a.starts_with("wp_my_shop_test_"), "{a}");
        assert!(b.starts_with("wp_my_shop_test_"), "{b}");
        // Deterministic (stored once at create; must be stable within a build).
        assert_eq!(a, db_name_disambiguated("my-shop.test"));
        // Always within MySQL's 64-char identifier limit, even for a long domain.
        let long = format!("{}.test", "a".repeat(250));
        let d = db_name_disambiguated(&long);
        assert!(d.len() <= DB_NAME_MAX, "must fit the 64-char limit, got {}", d.len());
        // Only valid identifier characters ([a-z0-9_]).
        assert!(
            a.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'),
            "valid identifier chars only: {a}"
        );
    }

    #[test]
    fn valid_slug_accepts_real_slugs_and_rejects_argv_and_source_smuggling() {
        for ok in ["akismet", "wp-super-cache", "woocommerce", "jetpack", "classic-editor", "2fa"] {
            assert!(valid_slug(ok), "{ok}");
        }
        for bad in [
            "", "--all", "-akismet", "Akismet", "my_plugin", "a b",
            "https://evil.example/x.zip", "/tmp/x.zip", "../x", "evil.zip", "a.b",
        ] {
            assert!(!valid_slug(bad), "{bad:?}");
        }
    }

    #[test]
    fn plugin_install_refuses_a_url_or_flag_slug_before_any_wp_call() {
        // Nonexistent binaries: reaching wp-cli would give an io error, NOT this
        // message — proving the guard runs first (same shape as the locale test).
        let run = |slug: &str| {
            plugin_install(
                Path::new("/nonexistent/php"),
                Path::new("/nonexistent/wp.phar"),
                Path::new("/nonexistent/docroot"),
                &[slug.to_string()],
                false,
            )
            .unwrap_err()
            .to_string()
        };
        assert!(run("https://evil.example/x.zip").contains("invalid plugin slug"));
        assert!(run("--activate").contains("invalid plugin slug"));
        // A real slug passes validation and only THEN fails on the missing binary.
        assert!(
            !run("akismet").contains("invalid plugin slug"),
            "a real slug must pass validation"
        );
    }

    #[test]
    fn wp_positional_argv_has_no_stray_end_of_flags_separator() {
        // Regression: B24 inserted a `--` "end-of-flags" token, but WP-CLI does
        // NOT honor getopt `--` — it read the bare `--` as a literal positional,
        // so `plugin activate` saw a phantom "--" slug and `cron event run` saw a
        // "--" hook. The argv must carry the real positionals ONLY, no `--`.
        let one = ["akismet".to_string()];
        assert_eq!(
            item_verb_argv("plugin", "activate", &one),
            ["plugin", "activate", "akismet"],
            "no stray -- before the plugin slug"
        );
        let two = ["akismet".to_string(), "jetpack".to_string()];
        assert_eq!(
            item_verb_argv("plugin", "activate", &two),
            ["plugin", "activate", "akismet", "jetpack"],
            "multiple slugs, still no --"
        );
        assert_eq!(
            cron_run_hook_argv("my_cron_hook"),
            ["cron", "event", "run", "my_cron_hook"],
            "no stray -- as the cron hook name"
        );
        // Guard against the token creeping back into either builder.
        assert!(!item_verb_argv("plugin", "activate", &one).contains(&"--"));
        assert!(!cron_run_hook_argv("my_cron_hook").contains(&"--"));
    }

    #[test]
    fn checksum_findings_split_real_from_os_noise() {
        let out = "\
Warning: File doesn't verify against checksum: wp-includes/version.php
Warning: File should not exist: wp-admin/.DS_Store
Warning: File should not exist: wp-includes/._blocks
Warning: File should not exist: wp-admin/backdoor.php
Warning: File doesn't exist: wp-includes/functions.php
Warning: something new wp-cli might say: mystery.php
Error: WordPress installation doesn't verify against checksums.";
        let (real, benign) = classify_checksum_output(out);
        // Benign: basename-matched OS clutter among "should not exist" only.
        assert_eq!(benign, vec!["wp-admin/.DS_Store", "wp-includes/._blocks"]);
        // Real: modified, non-noise extra, missing, and the unknown warning.
        assert_eq!(real.len(), 4, "real: {real:?}");
        assert!(real.iter().any(|r| r.contains("version.php")));
        assert!(real.iter().any(|r| r.contains("backdoor.php")));
        assert!(real.iter().any(|r| r.contains("functions.php")));
        assert!(real.iter().any(|r| r.contains("mystery.php")));
        // A noise-looking name buried in a real filename stays real.
        let (real2, benign2) =
            classify_checksum_output("Warning: File should not exist: x/.DS_Store-backdoor.php");
        assert!(benign2.is_empty() && real2.len() == 1);
        // Modified/missing core files are NEVER benign, even with noise names.
        let (real3, benign3) = classify_checksum_output(
            "Warning: File doesn't verify against checksum: wp-admin/.DS_Store",
        );
        assert!(benign3.is_empty() && real3.len() == 1);
    }

    #[test]
    fn role_whitelist_is_the_stock_five() {
        for ok in ["administrator", "editor", "author", "contributor", "subscriber"] {
            assert!(USER_ROLES.contains(&ok), "{ok} should be assignable");
        }
        for bad in ["super-admin", "Administrator", "", "none", "custom_role"] {
            assert!(!USER_ROLES.contains(&bad), "{bad:?} should be rejected");
        }
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
    fn language_rows_parse_live_wp_cli_output() {
        // Verbatim rows from `wp language core list --format=json` (wp-cli 2.12,
        // WP 7.0): snake_case keys, en_US with empty `updated`.
        let json = r#"[
            {"language":"en_US","english_name":"English (United States)","native_name":"English (United States)","status":"active","update":"none","updated":""},
            {"language":"fr_FR","english_name":"French (France)","native_name":"Français","status":"uninstalled","update":"none","updated":"2026-06-17 09:57:00"}
        ]"#;
        let rows: Vec<WpLanguage> = serde_json::from_str(json).unwrap();
        assert_eq!(rows[0].language, "en_US");
        assert_eq!(rows[0].status, "active");
        assert_eq!(rows[1].english_name, "French (France)");
        assert_eq!(rows[1].native_name, "Français");
        // Serializes camelCase for the frontend.
        let out = serde_json::to_string(&rows[1]).unwrap();
        assert!(out.contains("\"englishName\""), "{out}");
    }

    #[test]
    fn valid_locale_accepts_real_locales_and_rejects_argv_smuggling() {
        for ok in ["fr_FR", "de_DE_formal", "pt_BR", "ceb", "zh_CN"] {
            assert!(valid_locale(ok), "{ok}");
        }
        for bad in ["", "e", "--skip-plugins", "-f", "fr_FR; rm -rf /", "fr FR", "FR_fr", "../x", "fr\u{2013}FR"] {
            assert!(!valid_locale(bad), "{bad:?}");
        }
    }

    #[test]
    fn wp_version_shape_accepts_releases_and_rejects_argv_smuggling() {
        for ok in ["7.0", "7.0.1", "6.8.5"] {
            assert!(parse_wp_version(ok).is_some(), "{ok}");
        }
        for bad in ["", "nightly", "6", "6.8.5.1", "+6.8", "-6.8", "--force", "6..8", "6.8 ", "6.8.5; rm -rf /"] {
            assert!(parse_wp_version(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn stable_check_parses_filters_and_sorts_newest_first() {
        // Shape verbatim from api.wordpress.org/core/stable-check/1.0/.
        let body = br#"{"1.0.2":"insecure","5.9.3":"insecure","6.5.2":"insecure","6.9.4":"outdated","7.0":"outdated","7.0.1":"latest"}"#;
        let rows = parse_stable_check(body).unwrap();
        let versions: Vec<&str> = rows.iter().map(|r| r.version.as_str()).collect();
        // < 6.0 filtered out; newest first ("7.0" sorts as 7.0.0 below 7.0.1).
        assert_eq!(versions, ["7.0.1", "7.0", "6.9.4", "6.5.2"]);
        assert_eq!(rows[0].status, "latest");
        assert_eq!(rows[3].status, "insecure");
    }

    #[test]
    fn core_switch_rejects_bad_versions_before_any_wp_call() {
        // Nonexistent binaries — reaching wp-cli would yield an io error, not
        // these messages, proving both guards fire first.
        let allowed = vec!["7.0.1".to_string(), "6.9.4".to_string()];
        let run = |v: &str| {
            core_switch_version(
                Path::new("/nonexistent/php"),
                Path::new("/nonexistent/wp.phar"),
                Path::new("/nonexistent/docroot"),
                v,
                &allowed,
            )
            .unwrap_err()
            .to_string()
        };
        assert!(run("--force").contains("invalid version"));
        assert!(run("6.8.5.1").contains("invalid version"));
        // Well-shaped but not a real release (not in the fresh stable-check list).
        assert!(run("6.9.9").contains("not a known WordPress release"));
    }

    #[test]
    fn option_whitelist_makes_dangerous_options_unreachable() {
        // Editable: on the list.
        assert!(option_field("blogname").is_some());
        assert!(option_field("posts_per_page").is_some());
        // The foot-guns are not "blocked" — they simply don't exist here.
        for dangerous in ["siteurl", "home", "active_plugins", "template", "stylesheet", "db_version", "WPLANG", ""] {
            assert!(option_field(dangerous).is_none(), "{dangerous} must not be editable");
        }
        // option_update refuses BEFORE any wp-cli call: nonexistent binaries
        // would yield an io error, not this message.
        let e = option_update(
            Path::new("/nonexistent/php"),
            Path::new("/nonexistent/wp.phar"),
            Path::new("/nonexistent/docroot"),
            "siteurl",
            "https://evil.example",
        )
        .unwrap_err();
        assert!(e.to_string().contains("not an editable option"), "{e}");
    }

    #[test]
    fn option_values_validate_per_kind_in_the_backend() {
        let tz = vec!["Europe/Paris".to_string()];
        let roles = vec!["subscriber".to_string(), "shop_manager".to_string()];
        let v = |kind, val: &str| validate_option_value(kind, val, &tz, &roles);

        // posts_per_page: 1..=1000.
        for bad in ["0", "-1", "1001", "abc", "", "10.5"] {
            assert!(v(OptionKind::IntRange(1, 1000), bad).is_err(), "{bad:?}");
        }
        assert!(v(OptionKind::IntRange(1, 1000), "1").is_ok());
        assert!(v(OptionKind::IntRange(1, 1000), "1000").is_ok());
        // Email shape.
        for bad in ["", "nope", "@x.com", "a@b", "a b@c.d", "a@.com", "a@com."] {
            assert!(v(OptionKind::Email, bad).is_err(), "{bad:?}");
        }
        assert!(v(OptionKind::Email, "admin@site.test").is_ok());
        // Toggles are the two WP strings only.
        assert!(v(OptionKind::Bool, "2").is_err());
        assert!(v(OptionKind::Bool, "true").is_err());
        assert!(v(OptionKind::Bool, "1").is_ok());
        // Weekday 0..=6.
        assert!(v(OptionKind::Weekday, "7").is_err());
        assert!(v(OptionKind::Weekday, "0").is_ok());
        // Timezone: from the list, or empty (gmt_offset mode — seen live).
        assert!(v(OptionKind::Timezone, "Mars/Olympus").is_err());
        assert!(v(OptionKind::Timezone, "Europe/Paris").is_ok());
        assert!(v(OptionKind::Timezone, "").is_ok());
        // Role: live list incl. custom roles.
        assert!(v(OptionKind::Role, "administrator2").is_err());
        assert!(v(OptionKind::Role, "shop_manager").is_ok());
    }

    #[test]
    fn scalar_display_refuses_everything_serialized() {
        use serde_json::json;
        assert_eq!(scalar_display(&json!("Think Rank")).as_deref(), Some("Think Rank"));
        assert_eq!(scalar_display(&json!(10)).as_deref(), Some("10"));
        assert_eq!(scalar_display(&json!(false)).as_deref(), Some("0"));
        // Arrays/objects (unserialized PHP data) and null: never editable.
        assert_eq!(scalar_display(&json!(["a.php", "b.php"])), None);
        assert_eq!(scalar_display(&json!({"k": "v"})), None);
        assert_eq!(scalar_display(&serde_json::Value::Null), None);
    }

    #[test]
    fn options_eval_script_contains_exactly_the_whitelist() {
        let s = options_eval_script();
        for f in OPTION_FIELDS {
            assert!(s.contains(&format!("\"{}\"", f.name)), "{} missing", f.name);
        }
        assert!(!s.contains("siteurl") && !s.contains("active_plugins"));
    }

    #[test]
    fn cleanup_os_noise_deletes_only_validated_noise_files() {
        let root = std::env::temp_dir().join(format!("rexenv-noise-{}", std::process::id()));
        let outside = std::env::temp_dir().join(format!("rexenv-noise-out-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(root.join("wp-admin/css")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();

        // Legit noise (nested) — deleted.
        std::fs::write(root.join(".DS_Store"), "x").unwrap();
        std::fs::write(root.join("wp-admin/css/.DS_Store"), "x").unwrap();
        std::fs::write(root.join("wp-admin/._resource"), "x").unwrap();
        // Non-noise basename — refused even though the UI sent it.
        std::fs::write(root.join("wp-admin/evil.php"), "x").unwrap();
        // Victims for the symlink cases.
        std::fs::write(outside.join(".DS_Store"), "outside-victim").unwrap();
        std::fs::write(root.join("wp-config.php"), "inside-victim").unwrap();
        // Symlink named like noise → OUTSIDE file: must not be followed.
        std::os::unix::fs::symlink(outside.join(".DS_Store"), root.join("wp-admin/.DS_Store"))
            .unwrap();
        // Symlink named like noise → INSIDE non-noise file: the canonical path
        // passes the prefix check — only the lstat guard saves wp-config.php.
        std::os::unix::fs::symlink(root.join("wp-config.php"), root.join("wp-admin/css/._cfg"))
            .unwrap();
        // Directory named like noise — files only.
        std::fs::create_dir(root.join(".Trashes")).unwrap();

        let paths: Vec<String> = [
            ".DS_Store",
            "wp-admin/css/.DS_Store",
            "wp-admin/._resource",
            "wp-admin/evil.php",             // guard 1: basename
            "../escape/.DS_Store",           // guard 2: `..`
            "/tmp/.DS_Store",                // guard 2: absolute
            "wp-admin/.DS_Store",            // guard 3: symlink → outside
            "wp-admin/css/._cfg",            // guard 3: symlink → inside victim
            ".Trashes",                      // guard 3: directory
            "wp-includes/.DS_Store",         // vanished: skip, not abort
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let res = cleanup_os_noise(&root, &paths).unwrap();

        assert_eq!(res.removed, 3, "skipped: {:?}", res.skipped);
        assert_eq!(res.skipped.len(), 7);
        // The real noise is gone…
        assert!(!root.join(".DS_Store").exists());
        assert!(!root.join("wp-admin/css/.DS_Store").exists());
        assert!(!root.join("wp-admin/._resource").exists());
        // …every victim/refusal survives.
        assert_eq!(std::fs::read_to_string(outside.join(".DS_Store")).unwrap(), "outside-victim");
        assert_eq!(std::fs::read_to_string(root.join("wp-config.php")).unwrap(), "inside-victim");
        assert!(root.join("wp-admin/evil.php").exists());
        assert!(root.join(".Trashes").is_dir());
        // Reasons name the guard, not a generic failure.
        let reason = |p: &str| {
            res.skipped.iter().find(|s| s.path == p).map(|s| s.reason.clone()).unwrap_or_default()
        };
        assert!(reason("wp-admin/evil.php").contains("not a known macOS system file"));
        assert!(reason("../escape/.DS_Store").contains("escapes"));
        assert!(reason("/tmp/.DS_Store").contains("escapes"));
        assert!(reason("wp-admin/.DS_Store").contains("symbolic link"));
        assert!(reason("wp-admin/css/._cfg").contains("symbolic link"));
        assert!(reason(".Trashes").contains("not a regular file"));

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn run_with_timeout_kills_a_stalled_child_fast() {
        // Simulates the offline language download: a child that would sit for
        // 30s (WP's download_url waits 300s) must be killed at the cap, not
        // waited out — the UI spinner rides on this returning.
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "sleep 30"]);
        let start = Instant::now();
        let e = run_with_timeout(cmd, Duration::from_millis(400), "sleep-test").unwrap_err();
        assert!(e.to_string().contains("timed out after 0s"), "{e}");
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "took {:?} — child not killed at the cap",
            start.elapsed()
        );
    }

    #[test]
    fn run_with_timeout_returns_output_of_a_fast_child() {
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "echo out; echo err 1>&2"]);
        let out = run_with_timeout(cmd, Duration::from_secs(10), "echo-test").unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "out");
        assert_eq!(String::from_utf8_lossy(&out.stderr).trim(), "err");
    }

    #[test]
    fn run_with_timeout_drains_a_chatty_child_without_a_fake_timeout() {
        // A child that writes far past the ~64KB pipe buffer before exiting.
        // The old wait-then-read shape deadlocked here (child blocked writing,
        // try_wait never Some) and reported a FAKE timeout — a long
        // `plugin list --format=json` must never trip the guard by being long.
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "head -c 300000 /dev/zero | tr '\\0' 'x'; echo done"]);
        let out = run_with_timeout(cmd, Duration::from_secs(10), "chatty-test").unwrap();
        assert!(out.status.success());
        assert!(out.stdout.len() > 300_000, "full output drained: {}", out.stdout.len());
    }

    #[test]
    fn download_timeout_scales_with_item_count_and_stays_loose() {
        // base 120s + 900s/item — 900 ≥ 1.5× WP's verified 300s per-download
        // internal bound (download_url, file.php) and dominates core's ~600s,
        // so a legit slow download always fails INSIDE wp-cli first; only a
        // wedge outside wp-cli's own bounds can reach this cap.
        assert_eq!(download_timeout(1), Duration::from_secs(1020));
        assert_eq!(download_timeout(5), Duration::from_secs(4620));
        // List calls: WP's update-check request is internally capped at
        // 3s/30s (update.php), so 300s is ~10× the worst legit case.
        assert_eq!(WP_LIST_TIMEOUT, Duration::from_secs(300));
    }

    #[test]
    fn item_verb_update_gets_the_scaled_download_bound_local_verbs_stay_unbounded() {
        // plugin/theme `update` downloads an archive per item (same wedge class
        // as install) → the SCALED download cap, not the flat list bound.
        assert_eq!(item_verb_timeout("update", 3), Some(download_timeout(3)));
        assert_eq!(item_verb_timeout("update", 1), Some(Duration::from_secs(1020)));
        // Local verbs never spawn a network wait — left untimed.
        for local in ["activate", "deactivate", "delete"] {
            assert_eq!(item_verb_timeout(local, 3), None, "{local}");
        }
    }

    #[test]
    fn switch_language_rejects_malformed_locale_before_any_wp_call() {
        // Nonexistent binaries: reaching wp-cli would error with an io message,
        // NOT the validation message — proving the guard runs first.
        let e = switch_language(
            Path::new("/nonexistent/php"),
            Path::new("/nonexistent/wp.phar"),
            Path::new("/nonexistent/docroot"),
            "--skip-plugins",
        )
        .unwrap_err();
        assert!(e.to_string().contains("invalid locale"), "{e}");
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

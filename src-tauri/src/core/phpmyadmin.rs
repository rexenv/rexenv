//! core::phpmyadmin — the SECOND database console, served as an internal vhost
//! (`docs/archive/PLAN-db-console-open-and-phpmyadmin.md`).
//!
//! phpMyAdmin is a whole PHP application tree (`binaries` pins the release zip,
//! `Shape::Dir`). Like Adminer it is served through the shared stack on a fixed
//! internal host behind the edge, rooted at an isolated docroot under app data,
//! and it is deliberately NOT a `Site` — so it can never be a tunnel origin (§9).
//!
//! Unlike Adminer it has no wrapper: everything rexenv decides about it is in the
//! `config.inc.php` this module GENERATES ([`config_inc_php`]) — the two loopback
//! servers it may reach, the auth mode, the framing policy, the temp dir — and in
//! the proxy that frames it in-app (`core::dbconsole_proxy`). MySQL and MariaDB
//! only: phpMyAdmin has no PostgreSQL driver, so a PostgreSQL row keeps Adminer
//! whatever the console setting says.

use crate::core::binaries;
use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use std::path::{Path, PathBuf};

/// Internal host phpMyAdmin is served on (resolved by the embedded `.rex` resolver).
/// NEVER a public tunnel origin (§9) — it isn't a site; `commands/tunnels.rs`
/// asserts neither console host can appear in a tunnel start.
pub const PHPMYADMIN_HOST: &str = "phpmyadmin.rexenv.rex";

/// The generated configuration, rewritten by every stage/regeneration — edits
/// are lost, and its header says so.
pub const CONFIG_FILE: &str = "config.inc.php";

/// Records which release the docroot IS (the Adminer marker's shape: a dotfile,
/// unservable through `NGINX_DOTFILE_DENY`, written LAST so it can never claim a
/// version whose files are not beside it).
const STAGED_VERSION: &str = ".phpmyadmin-version";

/// `$cfg['blowfish_secret']` — 32 bytes phpMyAdmin uses to encrypt its cookies.
/// Generated ONCE per machine into app data, OUTSIDE the docroot (nothing in a
/// served tree should be a secret, dotfile-denied or not), owner-only through the
/// platform (`write_private`: born 0600 / owner ACL), and read back on every
/// regeneration so sessions survive a rebuild.
const SECRET_FILE: &str = "phpmyadmin.secret";
const SECRET_LEN: usize = 32;

/// `$cfg['TempDir']` — phpMyAdmin's Twig cache. Without a writable one every page
/// recompiles its templates (slow) and the console warns on each load.
const TEMP_DIR: &str = "phpmyadmin-tmp";

/// The palette file — `dark` or `light`, written by rexenv whenever the app's own theme
/// resolves or changes (the frame writes it BEFORE it loads), read by the generated
/// `config.inc.php` on EVERY request. Adminer's mechanism and Adminer's file name
/// (`adminer::THEME_FILE`), in this console's docroot: a dotfile, so `NGINX_DOTFILE_DENY`
/// keeps it unservable. A file rather than a query parameter for Adminer's reason — the
/// console's own links carry nothing of ours, so one click would have snapped the page
/// back.
const THEME_FILE: &str = ".rexenv-theme";
/// The vendored dark theme's directory name under `themes/` — BooDark
/// (`binaries::PHPMYADMIN_THEME_VERSION`), staged by [`ensure`] beside the tree.
pub const DARK_THEME: &str = "boodark";
/// phpMyAdmin's own default — the light palette.
pub const LIGHT_THEME: &str = "pmahomme";

/// Record which palette the console must render in — `dark` or `light` only, the app
/// having resolved "system" first so the two can never disagree about what the OS meant.
/// A no-op before the console is staged: there is no docroot to write into, and creating
/// one would be an empty directory with a palette file and no tree.
pub fn set_theme(platform: &dyn Platform, theme: &str) -> Result<()> {
    if staged_version(platform).is_none() {
        return Ok(());
    }
    crate::core::adminer::write_theme(&docroot(platform)?, theme)
}

/// Where the user's console choice is stored — a settings ROW, `adminer` or
/// `phpmyadmin`. Absent means Adminer. Postgres rows ignore it (no driver).
pub const CONSOLE_KEY: &str = "db_console";

/// Where the user's chosen phpMyAdmin VERSION is stored — a settings row, the
/// Adminer shape (`adminer::VERSION_KEY`). Absent means "follow the pin", never
/// backfilled, so a user who never presses Update keeps receiving the tree their
/// app ships as releases move it.
pub const VERSION_KEY: &str = "phpmyadmin_version";

/// **The ONE answer to "which phpMyAdmin version is the console".** Everything that
/// stages, plans or displays it asks this — never `binaries::pins().phpmyadmin`.
///
/// The three steps are Adminer's (`adminer::effective_version`, ledger #363), for the
/// same reasons: the stored selection; FLOORED by the compiled-in pin
/// ([`crate::core::updates::phpmyadmin_floored`]) so a stale choice can never hold a
/// user below the tree their app ships; VOUCHED on read through `binaries::manifest`
/// (the pin first, then the verified catalog) so a selection whose entry has since
/// left the manifest reads as "we are on the pin" rather than as a version nothing
/// can produce. Read-time filtering also makes the generic `set_setting` door inert
/// for this key — the same property `effective_console` has for the console key.
pub fn effective_version(platform: &dyn Platform, conn: &rusqlite::Connection) -> String {
    effective_version_for(conn, platform.binaries().arch())
}

/// [`effective_version`] against an explicit arch — the seam the tests use.
fn effective_version_for(conn: &rusqlite::Connection, arch: crate::platform::traits::Arch) -> String {
    let selected = crate::state::store::get_setting(conn, VERSION_KEY).ok().flatten();
    let want = crate::core::updates::phpmyadmin_floored(selected.as_deref());
    if binaries::manifest("phpmyadmin", &want, std::env::consts::OS, arch).is_some() {
        return want;
    }
    binaries::pins().phpmyadmin.to_string()
}

/// Record the user's chosen phpMyAdmin version, or clear it back to the pin.
pub fn set_selected_version(conn: &rusqlite::Connection, version: Option<&str>) -> Result<()> {
    match version {
        Some(v) => crate::state::store::set_setting(conn, VERSION_KEY, v),
        None => crate::state::store::delete_setting(conn, VERSION_KEY),
    }
}

/// The console the UI should open for MySQL/MariaDB: `phpmyadmin` ONLY when it is
/// both chosen AND staged — read-time filtering, as `adminer::effective_version`
/// does, so the generic `set_setting` door is inert for this key: a value written
/// past `db_console_set` (which downloads, probes and stages before it persists)
/// reads as Adminer until a real stage exists, never as a console with no vhost.
pub fn effective_console(platform: &dyn Platform, conn: &rusqlite::Connection) -> &'static str {
    let chosen = crate::state::store::get_setting(conn, CONSOLE_KEY).ok().flatten();
    console_for(chosen.as_deref(), staged_version(platform).is_some())
}

/// The pure rule behind [`effective_console`].
pub fn console_for(chosen: Option<&str>, staged: bool) -> &'static str {
    match chosen {
        Some("phpmyadmin") if staged => "phpmyadmin",
        _ => "adminer",
    }
}

/// Web docroot for phpMyAdmin, isolated from the binary cache: a COPY of the
/// resolved tree plus the generated config. A copy rather than the cache dir
/// itself, because the cache tree is pin-marked and may be retired/replaced by a
/// resolve while nginx is serving it.
pub fn docroot(platform: &dyn Platform) -> Result<PathBuf> {
    Ok(platform.paths().app_data_dir()?.join("phpmyadmin"))
}

/// The Twig cache directory (created by [`ensure`]).
pub fn temp_dir(platform: &dyn Platform) -> Result<PathBuf> {
    Ok(platform.paths().app_data_dir()?.join(TEMP_DIR))
}

fn secret_path(platform: &dyn Platform) -> Result<PathBuf> {
    Ok(platform.paths().app_data_dir()?.join(SECRET_FILE))
}

/// Which release the docroot is ACTUALLY serving, or `None` before the first
/// stage. `None` is a state ("not installed"), not a zero value.
pub fn staged_version(platform: &dyn Platform) -> Option<String> {
    let dir = docroot(platform).ok()?;
    let v = std::fs::read_to_string(dir.join(STAGED_VERSION)).ok()?;
    let v = v.trim().to_string();
    (!v.is_empty() && dir.join("index.php").is_file()).then_some(v)
}

/// Whether the docroot has to be (re)built from the cached tree — a value, not a
/// condition inside an async fn, so the rule is testable without a `Platform`.
/// Both halves matter: the marker says WHICH version, and `index.php`'s existence
/// says the tree is really there (a marker without its tree is a docroot that 404s
/// while claiming to be up to date).
fn needs_restage(marker: Option<&str>, index_exists: bool, want: &str) -> bool {
    marker != Some(want) || !index_exists
}

/// A filesystem path as a single-quoted PHP string literal: forward slashes on
/// every OS (PHP accepts them on Windows; a backslash inside a quoted string is
/// an escape waiting to happen), `\` and `'` escaped. App-data paths contain
/// spaces on every OS and backslashes on Windows — both fixtures are tested.
pub fn php_path_literal(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// The one server block, from the fixed loopback port — `host` is a literal
/// `127.0.0.1` and the port a compile-time constant, so there is no path by which
/// anything but rexenv's own engines can be named here.
fn server_block(index: usize, label: &str, port: u16) -> String {
    format!(
        "$cfg['Servers'][{index}]['verbose'] = '{label}';\n\
         $cfg['Servers'][{index}]['host'] = '127.0.0.1';\n\
         $cfg['Servers'][{index}]['port'] = '{port}';\n\
         $cfg['Servers'][{index}]['connect_type'] = 'tcp';\n\
         $cfg['Servers'][{index}]['auth_type'] = 'config';\n\
         $cfg['Servers'][{index}]['user'] = 'root';\n\
         $cfg['Servers'][{index}]['password'] = '';\n\
         $cfg['Servers'][{index}]['AllowNoPassword'] = true;\n"
    )
}

/// `$cfg['Servers'][1]` is MySQL, `[2]` MariaDB — the deep links
/// (`src/lib/phpmyadmin.ts`) carry `server=1|2`, so the order is a contract.
pub const SERVER_MYSQL: usize = 1;
pub const SERVER_MARIADB: usize = 2;

/// The whole `config.inc.php`, generated — never shipped, never edited.
///
/// **This is the passwordless gate.** `auth_type = 'config'` means phpMyAdmin
/// renders no login form and takes no host from the request: the only servers it
/// can ever connect to are the two written here, both `127.0.0.1` on rexenv's own
/// fixed ports, with the empty root password those engines are provisioned with.
/// That is the promise Adminer's wrapper keeps in `login()` — loopback engines
/// only, never a remote host — kept here by construction instead of by code.
///
/// `AllowThirdPartyFraming` is `'sameorigin'`, and the value is MEASURED, not chosen
/// for its name (macOS VM, 15 Oct 2026 — the first in-app frame was EMPTY while the
/// proxy answered 200): phpMyAdmin's `Header.php` sends `X-Frame-Options: DENY` for
/// anything but `true` and `SAMEORIGIN` for `'sameorigin'` — the direct https vhost
/// stays unframeable by any OTHER origin, which is the clickjacking case — but it
/// also adds `cross_framing_protection.js` and `<style id="cfs-style">html{display:
/// none}</style>` ONLY when the value is exactly `false`: that script hides the whole
/// document and tries `top.location = self.location` whenever `self !== top`, which
/// inside rexenv's cross-origin iframe is a blank panel with no error anywhere.
/// `'sameorigin'` keeps the header and drops the script; the proxy then strips the
/// header and rewrites `frame-ancestors` on its own copy — an origin only rexenv's
/// scheme handler answers — see `core::dbconsole_proxy`. Never `true`: that is no
/// header at all on the direct vhost.
///
/// No `PmaAbsoluteUri`: both the https vhost and the proxy origin serve this same
/// tree, and every link must stay relative to whichever origin loaded it.
///
/// **The palette follows the APP** (15 Oct 2026): the file reads [`THEME_FILE`] per
/// request and names [`DARK_THEME`] only when it says `dark` AND the theme directory is
/// there (a docroot staged by an older app has no theme yet; naming a missing theme makes
/// phpMyAdmin print a warning box on every page). The theme picker is off
/// (`ThemeManager = false`) — the app decides, as for Adminer — and a `pma_theme` cookie
/// a user set while the picker was on (v1 shipped with it) is dropped before phpMyAdmin
/// reads it, or it would outrank the app forever.
pub fn config_inc_php(secret: &str, temp_dir: &Path) -> String {
    debug_assert_eq!(secret.len(), SECRET_LEN);
    let mut out = String::new();
    out.push_str(
        "<?php\n\
         // Managed by rexenv — regenerated on every stage and config rebuild; edits here are lost.\n\
         // The two servers below are rexenv's own loopback engines; phpMyAdmin has no login form\n\
         // (auth_type = config), so nothing else can be reached through this console.\n\
         declare(strict_types=1);\n\n",
    );
    out.push_str(&format!("$cfg['blowfish_secret'] = '{secret}';\n"));
    out.push_str(&format!("$cfg['TempDir'] = {};\n\n", php_path_literal(temp_dir)));
    out.push_str(&server_block(SERVER_MYSQL, "MySQL", crate::core::database::MYSQL_PORT));
    out.push('\n');
    out.push_str(&server_block(SERVER_MARIADB, "MariaDB", crate::core::db::MARIADB_PORT));
    out.push_str(
        "\n\
         // Framing: 'sameorigin' — the direct vhost sends X-Frame-Options: SAMEORIGIN (no other\n\
         // origin can frame it) WITHOUT phpMyAdmin's cross_framing_protection.js, which only\n\
         // `false` adds and which blanks the page inside rexenv's own frame; that frame reaches\n\
         // this tree through rexenv's proxy origin, which rewrites the framing headers itself.\n\
         $cfg['AllowThirdPartyFraming'] = 'sameorigin';\n\
         // No upstream error reports, no phpinfo page, and the file-permission nag is for a\n\
         // config somebody hand-edits — this one is rexenv's.\n\
         $cfg['SendErrorReports'] = 'never';\n\
         $cfg['ShowPhpInfo'] = false;\n\
         $cfg['CheckConfigurationPermissions'] = false;\n\
         $cfg['VersionCheck'] = false;\n",
    );
    out.push_str(&format!(
        "\n\
         // Palette: rexenv writes the app's RESOLVED scheme (dark/light) to {THEME_FILE} beside\n\
         // this file and reloads the console on change. The dark theme is {DARK_THEME} (vendored\n\
         // beside the tree); the picker is off because the app decides, and a cookie the picker\n\
         // once set must not outrank it.\n\
         unset($_COOKIE['pma_theme']);\n\
         $cfg['ThemeManager'] = false;\n\
         $cfg['ThemeDefault'] = (is_dir(__DIR__ . '/themes/{DARK_THEME}') && is_file(__DIR__ . '/{THEME_FILE}')\n\
         \x20   && trim((string) file_get_contents(__DIR__ . '/{THEME_FILE}')) === 'dark') ? '{DARK_THEME}' : '{LIGHT_THEME}';\n"
    ));
    out
}

/// Per-docroot ini for this vhost: the SAME import cap Adminer's vhost gets
/// (importing dumps is both consoles' job — one number, `adminer::import_cap`),
/// and deprecations kept off the page: a notice from a vendored Twig on a newer
/// PHP is not something the user should read inside a table list. Written for
/// every OS — `.user.ini` is what reaches a Windows php-cgi group (#788).
pub fn user_ini_contents(bytes: u64) -> String {
    format!(
        "; Managed by rexenv — phpMyAdmin's import cap (the vhost's client_max_body_size) and\n\
         ; error policy. Rewritten on every config regeneration; edits here are lost.\n\
         {}\n\
         error_reporting=E_ALL & ~E_DEPRECATED & ~E_STRICT\n\
         display_errors=Off\n",
        crate::core::adminer::import_php_value(bytes)
    )
}

/// Write [`user_ini_contents`] into `dir` unless it already holds exactly that.
pub fn write_user_ini(dir: &Path, bytes: u64) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(crate::core::adminer::USER_INI);
    let want = user_ini_contents(bytes);
    if std::fs::read_to_string(&path).is_ok_and(|have| have == want) {
        return Ok(());
    }
    std::fs::write(&path, want)?;
    Ok(())
}

/// The machine's blowfish secret: read, or generated once and born owner-only.
/// 32 hex characters from two v4 UUIDs — PHP wants exactly 32 bytes, and hex keeps
/// the literal free of quotes and backslashes.
fn blowfish_secret(platform: &dyn Platform) -> Result<String> {
    let path = secret_path(platform)?;
    if let Ok(have) = std::fs::read_to_string(&path) {
        let have = have.trim().to_string();
        if have.len() == SECRET_LEN && have.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(have);
        }
    }
    let fresh: String = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
    .chars()
    .take(SECRET_LEN)
    .collect();
    platform.permissions().write_private(&path, fresh.as_bytes())?;
    Ok(fresh)
}

/// Re-render the STAGED docroot's `config.inc.php` from this build's generator — called by
/// every config regeneration (`sites::rebuild`), beside the `.user.ini`, so an app update
/// that changes what rexenv writes reaches a tree staged by the previous version. Found on
/// the macOS VM (15 Oct 2026): the framing fix shipped, the app was swapped, and the
/// docroot kept the old `false` until the user chose phpMyAdmin AGAIN — nothing else
/// re-rendered it. No download, no probe, no rename: a text write into an existing tree.
pub fn refresh_config(platform: &dyn Platform) -> Result<()> {
    let dir = docroot(platform)?;
    if staged_version(platform).is_none() {
        return Ok(());
    }
    write_config(platform, &dir)
}

/// Render and write `config.inc.php` into `dir` (only when it differs).
fn write_config(platform: &dyn Platform, dir: &Path) -> Result<()> {
    let tmp = temp_dir(platform)?;
    std::fs::create_dir_all(&tmp)?;
    let rendered = config_inc_php(&blowfish_secret(platform)?, &tmp);
    let path = dir.join(CONFIG_FILE);
    if std::fs::read_to_string(&path).ok().as_deref() == Some(rendered.as_str()) {
        return Ok(());
    }
    write_durable(&path, rendered.as_bytes())?;
    Ok(())
}

/// The CLI probe: load the tree's own constants and autoloader, then build
/// `PhpMyAdmin\Config` from the generated file — the same path every request
/// takes before any output. A release whose vendored tree fatals on this PHP, or
/// whose config loader rejects what rexenv wrote, fails HERE, before the tree is
/// published. Verdicts go to stderr, as Adminer's probe does.
const PROBE_PHP: &str = r#"<?php
$root = $argv[1];
set_error_handler(function ($no, $str, $file, $line) {
    if (in_array($no, [E_DEPRECATED, E_USER_DEPRECATED, E_NOTICE, E_USER_NOTICE], true)) { return true; }
    fwrite(STDERR, "REXENV-PROBE-FAIL: $str at $file:$line\n");
    exit(1);
});
register_shutdown_function(function () {
    $e = error_get_last();
    if ($e !== null && in_array($e['type'], [E_ERROR, E_PARSE, E_CORE_ERROR, E_COMPILE_ERROR], true)) {
        fwrite(STDERR, "REXENV-PROBE-FAIL: {$e['message']} at {$e['file']}:{$e['line']}\n");
    }
});
chdir($root);
// What index.php defines before anything else; constants.php exits SILENTLY (code 0) without
// them — "no verdict" is a failure here, never a pass.
define('PHPMYADMIN', true);
define('ROOT_PATH', $root . DIRECTORY_SEPARATOR);
require ROOT_PATH . 'libraries/constants.php';
require ROOT_PATH . 'vendor/autoload.php';
$cfg = new \PhpMyAdmin\Config($root . '/config.inc.php');
$servers = $cfg->settings['Servers'] ?? [];
if (count($servers) < 2) { fwrite(STDERR, "REXENV-PROBE-FAIL: config loaded " . count($servers) . " servers\n"); exit(1); }
foreach ($servers as $s) {
    if (($s['host'] ?? '') !== '127.0.0.1' || ($s['auth_type'] ?? '') !== 'config') {
        fwrite(STDERR, "REXENV-PROBE-FAIL: a server is not a loopback config-auth server\n"); exit(1);
    }
}
// 'sameorigin' keeps X-Frame-Options on the direct vhost; `true` would drop it, `false`
// would add the frame-hiding script (plan §3.3, the macOS VM's blank panel).
if (($cfg->settings['AllowThirdPartyFraming'] ?? true) !== 'sameorigin') {
    fwrite(STDERR, "REXENV-PROBE-FAIL: AllowThirdPartyFraming is not 'sameorigin'\n"); exit(1);
}
// The palette is the app's: no picker, and the vendored dark theme (when staged) must
// declare support for THIS tree's series, or phpMyAdmin drops it with a warning box on
// every page — the gate that catches a tree bump outrunning the theme pin.
if (($cfg->settings['ThemeManager'] ?? true) !== false) {
    fwrite(STDERR, "REXENV-PROBE-FAIL: ThemeManager is not off\n"); exit(1);
}
$themeJson = $root . '/themes/boodark/theme.json';
if (is_dir($root . '/themes/boodark')) {
    $meta = json_decode((string) @file_get_contents($themeJson), true);
    $supports = is_array($meta) && isset($meta['supports']) && is_array($meta['supports']) ? $meta['supports'] : [];
    if (!in_array(\PhpMyAdmin\Version::SERIES, $supports, true)) {
        fwrite(STDERR, "REXENV-PROBE-FAIL: the dark theme does not support phpMyAdmin " . \PhpMyAdmin\Version::SERIES . " (supports: " . implode(',', $supports) . ")\n"); exit(1);
    }
}
fwrite(STDERR, "REXENV-PROBE-OK\n");
"#;
const PROBE_OK: &str = "REXENV-PROBE-OK";

/// Does this tree, with its generated config, load on `php_bin`? Run BEFORE a
/// staged tree becomes the docroot: a tree that fails never replaces a working
/// console. Cheap structural checks first (no PHP needed to know a tree without
/// `index.php` or `vendor/autoload.php` is not phpMyAdmin), then the CLI probe.
pub fn verify_tree(php_bin: &Path, tree: &Path) -> Result<()> {
    for must in ["index.php", "vendor/autoload.php", "libraries/constants.php", CONFIG_FILE] {
        if !tree.join(must).is_file() {
            return Err(Error::Other(format!(
                "this phpMyAdmin tree is incomplete: {must} is missing under {}",
                tree.display()
            )));
        }
    }
    let probe = tree.join(".rexenv-probe.php");
    std::fs::write(&probe, PROBE_PHP)?;
    let out = crate::platform::command(php_bin)
        .arg("-d")
        .arg("display_errors=0")
        .arg(&probe)
        .arg(tree)
        .current_dir(tree)
        .output();
    let _ = std::fs::remove_file(&probe);
    let out = out?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains(PROBE_OK) {
        return Ok(());
    }
    let why = stderr
        .lines()
        .find(|l| l.contains("REXENV-PROBE-FAIL"))
        .map(|l| l.trim().to_string())
        .unwrap_or_else(|| {
            format!(
                "the probe produced no verdict (exit {})",
                crate::core::proc::exit_text(out.status.code())
            )
        });
    Err(Error::Other(format!(
        "phpMyAdmin {} does not run on the bundled PHP, so it was not installed — {why}",
        tree.display()
    )))
}

/// Ensure the phpMyAdmin docroot serves `version`: resolve the pinned tree
/// (download + verify + extract on first use), COPY it beside a generated
/// `config.inc.php` into `phpmyadmin.new`, PROBE it on the pool's pinned PHP, and
/// only then publish by rename — the old docroot is moved aside first (a rename
/// over an existing directory fails on Windows) and removed after. The marker is
/// written LAST. Idempotent: an already-staged version only re-renders the config
/// when it differs.
///
/// Never called from stack start: the console is staged when the user CHOOSES it
/// (`db_console_set`), so a machine that never picks phpMyAdmin never downloads
/// it, and the login-start offline planner has nothing new to know (#175).
pub async fn ensure(platform: &dyn Platform, version: &str) -> Result<PathBuf> {
    let dir = docroot(platform)?;
    let marker = dir.join(STAGED_VERSION);
    let staged = std::fs::read_to_string(&marker).ok();
    if !needs_restage(staged.as_deref().map(str::trim), dir.join("index.php").is_file(), version) {
        ensure_theme(platform, &dir).await?;
        write_config(platform, &dir)?;
        return Ok(dir);
    }

    let src = binaries::resolve_dir(platform, "phpmyadmin", version).await?;
    let theme = binaries::resolve_dir(platform, "phpmyadmin-theme", binaries::pins().phpmyadmin_theme).await?;
    let php = binaries::resolve_program(platform, "php", binaries::pins().php).await?;
    ensure_from(platform, version, &src, Some(&theme), &php)
}

/// The dark theme into a docroot that lacks it — a tree staged before the theme existed
/// (the generated config names the theme only when its directory is there, so such a
/// docroot renders light until this runs). Called when the user CHOOSES the console
/// (`ensure`'s idempotent path), never by a start: it may download. Staged by rename.
async fn ensure_theme(platform: &dyn Platform, dir: &Path) -> Result<()> {
    if dir.join("themes").join(DARK_THEME).join("theme.json").is_file() {
        return Ok(());
    }
    let theme = binaries::resolve_dir(platform, "phpmyadmin-theme", binaries::pins().phpmyadmin_theme).await?;
    stage_theme_into(dir, &theme)
}

/// Copy the resolved theme tree to `themes/<DARK_THEME>` under `dir`, by rename.
fn stage_theme_into(dir: &Path, theme_src: &Path) -> Result<()> {
    let themes = dir.join("themes");
    std::fs::create_dir_all(&themes)?;
    let fresh = themes.join(format!(".{DARK_THEME}.new"));
    let _ = std::fs::remove_dir_all(&fresh);
    copy_dir_recursive(theme_src, &fresh)?;
    let target = themes.join(DARK_THEME);
    let _ = std::fs::remove_dir_all(&target);
    std::fs::rename(&fresh, &target)?;
    Ok(())
}

/// The stage itself, from an explicit source tree, an optional theme tree and PHP — the
/// seam the live example uses to PLANT a tree that fails the probe and assert the docroot
/// is untouched (ledger #866). `ensure` is this with the resolved cache trees and the
/// pinned PHP. The theme goes into the copy BEFORE the probe, which checks it supports
/// the tree's series.
pub fn ensure_from(platform: &dyn Platform, version: &str, src: &Path, theme: Option<&Path>, php: &Path) -> Result<PathBuf> {
    let dir = docroot(platform)?;
    let marker = dir.join(STAGED_VERSION);

    let parent = dir
        .parent()
        .ok_or_else(|| Error::Other("phpmyadmin docroot has no parent".into()))?
        .to_path_buf();
    let fresh = parent.join("phpmyadmin.new");
    let _ = std::fs::remove_dir_all(&fresh);
    let build: Result<()> = (|| {
        copy_dir_recursive(src, &fresh)?;
        if let Some(theme) = theme {
            stage_theme_into(&fresh, theme)?;
        }
        write_config(platform, &fresh)?;
        verify_tree(php, &fresh)
    })();
    if let Err(e) = build {
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(e);
    }

    let old = parent.join(".phpmyadmin.old");
    let _ = std::fs::remove_dir_all(&old);
    if dir.exists() {
        std::fs::rename(&dir, &old)?;
    }
    std::fs::rename(&fresh, &dir)?;
    let _ = std::fs::remove_dir_all(&old);
    // LAST: "the tree beside me is this version" — and flushed, so it can never
    // outlive the tree it vouches for (#877).
    write_durable(&marker, version.as_bytes())?;
    Ok(dir)
}

/// Recursive copy (files and directories; symlinks materialised — a release zip
/// carries none). Every file is [`copy_durable`]'d: see it for the incident.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &to)?;
        } else {
            copy_durable(&entry.path(), &to)?;
        }
    }
    Ok(())
}

/// Write `bytes` to `path` and FLUSH them to the disk before returning.
///
/// **A rename publish is atomic for the NAME, not for the bytes behind it** (ledger
/// #877). On the Ubuntu VM (15 Oct 2026) a stage completed, the marker read `5.2.3`,
/// and the VM was powered off ~20 s later: the next boot found `index.php`,
/// `config.inc.php` AND the marker all ZERO bytes — ext4's delayed allocation had the
/// directory entries but not the data. The console read "not installed" (safe: the
/// marker is read-filtered, so nothing was served and the next choice restaged), but
/// a laptop losing power right after an update would have lost the console it had
/// just reported installing. So the stage syncs each file it writes, and the marker —
/// written LAST — is synced too, so it can never outlive the bytes it vouches for.
/// Both consoles use these (`adminer::ensure` for its pair).
pub(crate) fn write_durable(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}

/// [`std::fs::copy`] followed by a flush of the copy — [`write_durable`]'s reason.
pub(crate) fn copy_durable(src: &Path, dst: &Path) -> Result<()> {
    std::fs::copy(src, dst)?;
    std::fs::OpenOptions::new().write(true).open(dst)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "0123456789abcdef0123456789abcdef";

    /// The passwordless gate, as a property of the TEXT: every server is
    /// `127.0.0.1` on a fixed rexenv port with config auth, and there are exactly
    /// the two. A third server, a hostname, or a prompt-based auth here would be a
    /// console that can reach something rexenv does not own.
    #[test]
    fn every_server_is_a_loopback_engine_with_config_auth() {
        let cfg = config_inc_php(SECRET, Path::new("/tmp/x"));
        let hosts: Vec<&str> = cfg.lines().filter(|l| l.contains("['host']")).collect();
        assert_eq!(hosts.len(), 2, "{hosts:?}");
        for h in &hosts {
            assert!(h.ends_with("= '127.0.0.1';"), "{h}");
        }
        let ports: Vec<&str> = cfg.lines().filter(|l| l.contains("['port']")).collect();
        assert_eq!(
            ports,
            [
                format!("$cfg['Servers'][1]['port'] = '{}';", crate::core::database::MYSQL_PORT),
                format!("$cfg['Servers'][2]['port'] = '{}';", crate::core::db::MARIADB_PORT),
            ]
        );
        let auths: Vec<&str> = cfg.lines().filter(|l| l.contains("['auth_type']")).collect();
        assert_eq!(auths.len(), 2);
        assert!(auths.iter().all(|a| a.ends_with("= 'config';")), "{auths:?}");
        assert!(cfg.contains("$cfg['Servers'][1]['verbose'] = 'MySQL';"));
        assert!(cfg.contains("$cfg['Servers'][2]['verbose'] = 'MariaDB';"));
        assert_eq!(SERVER_MYSQL, 1);
        assert_eq!(SERVER_MARIADB, 2);
    }

    /// The direct vhost is never frameable by ANOTHER origin (`'sameorigin'` →
    /// `X-Frame-Options: SAMEORIGIN`), and never `false` — that value adds phpMyAdmin's
    /// frame-hiding script, which blanked rexenv's own frame (macOS VM, 15 Oct 2026) —
    /// and never `true`, which sends no header at all. The proxy widens its own copy.
    /// And no absolute URI, so both origins keep relative links.
    #[test]
    fn framing_is_denied_at_the_source_and_no_absolute_uri_is_set() {
        let cfg = config_inc_php(SECRET, Path::new("/tmp/x"));
        assert!(cfg.contains("$cfg['AllowThirdPartyFraming'] = 'sameorigin';"), "{cfg}");
        assert!(!cfg.contains("AllowThirdPartyFraming'] = false") && !cfg.contains("AllowThirdPartyFraming'] = true"));
        assert!(!cfg.contains("PmaAbsoluteUri"));
        assert!(cfg.contains("$cfg['SendErrorReports'] = 'never';"));
        assert!(cfg.starts_with("<?php\n"));
        assert!(cfg.contains(&format!("$cfg['blowfish_secret'] = '{SECRET}';")));
    }

    /// App-data paths carry spaces on every OS and backslashes on Windows; the
    /// literal must survive both without an escape in sight.
    #[test]
    fn paths_are_forward_slashed_single_quoted_literals() {
        assert_eq!(
            php_path_literal(Path::new("/Users/x/Library/Application Support/dev.rexenv.rexenv/phpmyadmin-tmp")),
            "'/Users/x/Library/Application Support/dev.rexenv.rexenv/phpmyadmin-tmp'"
        );
        assert_eq!(
            php_path_literal(Path::new(r"C:\Users\dell\AppData\Local\rexenv\rexenv\phpmyadmin-tmp")),
            "'C:/Users/dell/AppData/Local/rexenv/rexenv/phpmyadmin-tmp'"
        );
        assert_eq!(php_path_literal(Path::new("/o'brien/x")), "'/o\\'brien/x'");
        let cfg = config_inc_php(SECRET, Path::new(r"C:\Users\d e\tmp"));
        assert!(cfg.contains("$cfg['TempDir'] = 'C:/Users/d e/tmp';"), "{cfg}");
    }

    /// **The palette is the app's, read per request from the dotfile, and the dark theme
    /// is named only when it is THERE.** Text properties: the picker is off, the stale
    /// cookie is dropped before phpMyAdmin reads it, the expression guards on the theme
    /// directory, and the two names are the vendored dark theme and phpMyAdmin's default.
    fn tail_has_at(cfg: &str) -> bool {
        cfg.lines().skip_while(|l| !l.contains("ThemeDefault")).any(|l| l.contains('@'))
    }

    #[test]
    fn the_config_follows_the_app_palette_through_the_dotfile_and_guards_on_the_theme_dir() {
        let cfg = config_inc_php(SECRET, Path::new("/tmp/x"));
        assert!(cfg.contains("$cfg['ThemeManager'] = false;"), "{cfg}");
        assert!(cfg.contains("unset($_COOKIE['pma_theme']);"), "{cfg}");
        let line = cfg.lines().find(|l| l.contains("$cfg['ThemeDefault'] =")).expect("ThemeDefault");
        assert!(line.contains(&format!("is_dir(__DIR__ . '/themes/{DARK_THEME}') && is_file(__DIR__ . '/{THEME_FILE}')")), "{line}");
        // No `@`: the probe's error handler fires on a suppressed warning too (PHP 8), and a
        // missing palette file is the ORDINARY state before the frame first loads.
        assert!(!line.contains('@') && !tail_has_at(&cfg), "{cfg}");
        let tail = cfg.lines().skip_while(|l| !l.contains("$cfg['ThemeDefault'] =")).collect::<Vec<_>>().join("\n");
        assert!(tail.contains(&format!("'/{THEME_FILE}')) === 'dark') ? '{DARK_THEME}' : '{LIGHT_THEME}';")), "{tail}");
        assert_eq!(THEME_FILE, ".rexenv-theme", "Adminer's file name — one mechanism, one name");
        assert!(THEME_FILE.starts_with('.'), "a dotfile, or nginx serves it");
    }

    /// The choice is honoured only over a staged tree; anything else — junk,
    /// absent, chosen-but-not-yet-staged — is Adminer.
    #[test]
    fn the_console_is_phpmyadmin_only_when_chosen_and_staged() {
        assert_eq!(console_for(Some("phpmyadmin"), true), "phpmyadmin");
        assert_eq!(console_for(Some("phpmyadmin"), false), "adminer");
        assert_eq!(console_for(Some("adminer"), true), "adminer");
        assert_eq!(console_for(Some("junk"), true), "adminer");
        assert_eq!(console_for(None, true), "adminer");
    }

    #[test]
    fn restage_when_the_marker_or_the_tree_is_missing_or_another_version() {
        assert!(needs_restage(None, false, "5.2.3"));
        assert!(needs_restage(None, true, "5.2.3"));
        assert!(needs_restage(Some("5.2.3"), false, "5.2.3"));
        assert!(needs_restage(Some("5.2.2"), true, "5.2.3"));
        assert!(!needs_restage(Some("5.2.3"), true, "5.2.3"));
    }

    /// The ini carries the SAME cap Adminer's does — one number for both consoles.
    #[test]
    fn user_ini_carries_the_shared_cap_and_hides_deprecations() {
        let ini = user_ini_contents(2048);
        assert!(ini.contains(&crate::core::adminer::import_php_value(2048)));
        assert!(ini.contains("error_reporting=E_ALL & ~E_DEPRECATED"));
        assert!(ini.contains("display_errors=Off"));
    }

    /// **The effective phpMyAdmin version is the choice, floored by the pin, and
    /// vouched** — Adminer's three legs (#363), each planted the same way: a choice
    /// nothing vouches for reads as the pin; a vouched newer one is honoured on either
    /// arch; a vouched OLDER one is still the pin (the floor, not the vouch, decides).
    #[test]
    fn the_effective_phpmyadmin_version_is_the_choice_floored_by_the_pin_and_vouched() {
        use crate::platform::traits::Arch;
        let _catalog = binaries::catalog_test_lock();
        let conn = crate::state::db::open_in_memory().unwrap();
        let pin = binaries::pins().phpmyadmin;
        let newer = {
            let mut parts: Vec<u32> = pin.split('.').map(|n| n.parse().unwrap()).collect();
            *parts.last_mut().unwrap() += 1;
            parts.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(".")
        };
        let row = |v: &str, sha: char| {
            (
                "phpmyadmin",
                v.to_string(),
                crate::core::updates::ANY_ARCH,
                format!("https://files.phpmyadmin.net/phpMyAdmin/{v}/phpMyAdmin-{v}-english.zip"),
                sha.to_string().repeat(64),
            )
        };
        let install = |rows: &[(&str, String, &str, String, String)]| {
            let borrowed: Vec<(&str, &str, &str, &str, &str)> =
                rows.iter().map(|(n, v, a, u, s)| (*n, v.as_str(), *a, u.as_str(), s.as_str())).collect();
            binaries::install_catalog(crate::core::updates::catalog_for_tests(&borrowed));
        };

        binaries::install_catalog(Default::default());
        assert_eq!(effective_version_for(&conn, Arch::Arm64), pin, "no choice → the pin");

        set_selected_version(&conn, Some(&newer)).unwrap();
        assert_eq!(effective_version_for(&conn, Arch::Arm64), pin, "an unvouched selection became effective");

        install(&[row(&newer, 'c')]);
        assert_eq!(effective_version_for(&conn, Arch::Arm64), newer);
        assert_eq!(effective_version_for(&conn, Arch::X86_64), newer, "arch-free");

        // OLDER than the pin, VOUCHED (or this tests the vouching step, not the floor).
        install(&[row(&newer, 'c'), row("4.9.11", 'd')]);
        assert!(binaries::manifest("phpmyadmin", "4.9.11", std::env::consts::OS, Arch::Arm64).is_some());
        set_selected_version(&conn, Some("4.9.11")).unwrap();
        assert_eq!(effective_version_for(&conn, Arch::Arm64), pin, "a stale choice held the machine below the pin");

        set_selected_version(&conn, None).unwrap();
        assert!(crate::state::store::get_setting(&conn, VERSION_KEY).unwrap().is_none());
        assert_eq!(effective_version_for(&conn, Arch::Arm64), pin);
        binaries::install_catalog(Default::default());
    }

    /// **Every byte the stage writes is flushed before the marker, and the marker is
    /// flushed too** (#877): the stage functions of BOTH consoles reach the disk only
    /// through `write_durable` / `copy_durable`. A bare `fs::write`/`fs::copy` in a
    /// stage is the Ubuntu VM's zero-byte docroot waiting for the next power cut.
    /// Text scan, because a power loss is not a thing a test can produce; plant: a
    /// `std::fs::copy(` back in `copy_dir_recursive` fails it.
    #[test]
    fn the_stages_write_only_through_the_durable_helpers() {
        let me = include_str!("phpmyadmin.rs");
        let prod = &me[..me.find("\n#[cfg(test)]\nmod tests {\n").expect("tests")];
        for (name, start, end) in [
            ("copy_dir_recursive", "fn copy_dir_recursive(", "\n}\n"),
            ("write_config", "fn write_config(", "\n}\n"),
            ("ensure_from", "pub fn ensure_from(", "\n}\n"),
        ] {
            let i = prod.find(start).unwrap_or_else(|| panic!("{name}"));
            let body = &prod[i..i + prod[i..].find(end).unwrap()];
            assert!(!body.contains("std::fs::copy(") && !body.contains("std::fs::write("), "{name} writes without a flush:\n{body}");
        }
        let adminer = include_str!("adminer.rs");
        let i = adminer.find("let tmp = dir.join(\".adminer.php.new\");").expect("adminer stage");
        let body = &adminer[i..i + 700];
        assert!(body.contains("copy_durable(") && body.contains("write_durable("), "adminer's stage pair is not flushed:\n{body}");
        assert!(!body.contains("std::fs::copy(") && !body.contains("std::fs::write("), "{body}");
    }

    /// A tree with no autoloader is refused by the structural check — no PHP is
    /// run, and the message names what is missing.
    #[test]
    fn an_incomplete_tree_is_refused_before_any_php_runs() {
        let dir = std::env::temp_dir().join(format!("rexenv-pma-probe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.php"), "<?php").unwrap();
        let err = verify_tree(Path::new("/definitely/not/php"), &dir).unwrap_err().to_string();
        assert!(err.contains("vendor/autoload.php is missing"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

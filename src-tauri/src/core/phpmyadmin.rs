//! core::phpmyadmin — the SECOND database console, served as an internal vhost
//! (`docs/PLAN-db-console-open-and-phpmyadmin.md`).
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
/// `AllowThirdPartyFraming` stays FALSE (the default, written explicitly so a
/// reader cannot wonder): the direct https vhost sends `X-Frame-Options: DENY`,
/// so no page in the user's browser can frame a passwordless console. The in-app
/// frame gets its headers from the `rexpma` proxy, which only rexenv's own scheme
/// handler answers — see `core::dbconsole_proxy`.
///
/// No `PmaAbsoluteUri`: both the https vhost and the proxy origin serve this same
/// tree, and every link must stay relative to whichever origin loaded it.
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
         // Framing: the direct vhost is never frameable by a browser (X-Frame-Options: DENY);\n\
         // rexenv's in-app frame reaches this tree through its own proxy origin instead.\n\
         $cfg['AllowThirdPartyFraming'] = false;\n\
         // No upstream error reports, no phpinfo page, and the file-permission nag is for a\n\
         // config somebody hand-edits — this one is rexenv's.\n\
         $cfg['SendErrorReports'] = 'never';\n\
         $cfg['ShowPhpInfo'] = false;\n\
         $cfg['CheckConfigurationPermissions'] = false;\n\
         $cfg['VersionCheck'] = false;\n",
    );
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

/// Render and write `config.inc.php` into `dir` (only when it differs).
fn write_config(platform: &dyn Platform, dir: &Path) -> Result<()> {
    let tmp = temp_dir(platform)?;
    std::fs::create_dir_all(&tmp)?;
    let rendered = config_inc_php(&blowfish_secret(platform)?, &tmp);
    let path = dir.join(CONFIG_FILE);
    if std::fs::read_to_string(&path).ok().as_deref() == Some(rendered.as_str()) {
        return Ok(());
    }
    std::fs::write(&path, rendered)?;
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
if (($cfg->settings['AllowThirdPartyFraming'] ?? true) !== false) {
    fwrite(STDERR, "REXENV-PROBE-FAIL: AllowThirdPartyFraming is not false\n"); exit(1);
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
        write_config(platform, &dir)?;
        return Ok(dir);
    }

    let src = binaries::resolve_dir(platform, "phpmyadmin", version).await?;
    let php = binaries::resolve_program(platform, "php", binaries::pins().php).await?;

    let parent = dir
        .parent()
        .ok_or_else(|| Error::Other("phpmyadmin docroot has no parent".into()))?
        .to_path_buf();
    let fresh = parent.join("phpmyadmin.new");
    let _ = std::fs::remove_dir_all(&fresh);
    let build: Result<()> = (|| {
        copy_dir_recursive(&src, &fresh)?;
        write_config(platform, &fresh)?;
        verify_tree(&php, &fresh)
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
    // LAST: "the tree beside me is this version".
    std::fs::write(&marker, version)?;
    Ok(dir)
}

/// Recursive copy (files and directories; symlinks materialised — a release zip
/// carries none).
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to)?;
        }
    }
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

    /// The direct vhost is never frameable by a browser; the proxy widens its own
    /// copy. And no absolute URI, so both origins keep relative links.
    #[test]
    fn framing_is_denied_at_the_source_and_no_absolute_uri_is_set() {
        let cfg = config_inc_php(SECRET, Path::new("/tmp/x"));
        assert!(cfg.contains("$cfg['AllowThirdPartyFraming'] = false;"));
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

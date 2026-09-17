//! core::laravel — creating a Laravel app in a site rexenv owns.
//!
//! The counterpart of [`crate::core::wordpress`] for `SiteType::Laravel`: this
//! module knows how a fresh Laravel project is made (`composer create-project`),
//! how its `.env` is pointed at the site's database, and what "installed" means
//! on disk. It runs Composer through the SITE's bundled PHP — never a system
//! composer, which may be a wrapper rather than a phar (Herd ships one).
//!
//! Two facts this module exists to keep straight:
//!
//! - **The project root is not the served root.** `composer create-project`
//!   writes `.env` — the database credentials — at the project root, and the
//!   front controller at `public/index.php`. Serving the project root would put
//!   `.env` on a public URL, so the site row records `docroot_subdir = public`
//!   (v32, [`Site::served_root`](crate::state::models::Site::served_root)) and
//!   this module never serves anything itself.
//! - **A fresh Laravel defaults to SQLite** (11.x and later). rexenv's card
//!   promises an app "wired to a database", which here means the MySQL/MariaDB
//!   the rest of the site uses, so the `.env` rewrite below is not cosmetic —
//!   without it the app silently runs on a file nobody can see from the
//!   Databases screen.

use std::path::{Path, PathBuf};

use crate::core::repo::{
    map_composer_error, run_step_streamed, CancelToken, StepResult, STEP_IDLE_LIMIT,
};
use crate::error::{Error, Result};
use crate::platform::traits::ProcessSupervisor;
use rusqlite::Connection;

/// The Composer package a new site is created from. Pinned to the meta-package
/// rather than a version: `laravel/laravel` IS the skeleton, and Composer
/// resolves the current stable release of it.
pub const SKELETON_PACKAGE: &str = "laravel/laravel";

/// Has a Laravel app actually been installed into `project` — the skeleton AND
/// its dependencies?
///
/// It used to ask only for the two markers linked-project detection uses
/// (`artisan`, `public/index.php`). Those are extracted BEFORE Composer resolves
/// anything, so a `create-project` that then fails leaves both behind with no
/// `vendor/` — measured 11 Sep 2026 on PHP 8.1, where Laravel 10's framework
/// releases are advisory-blocked. The retry then read the dead skeleton as
/// "already present", skipped the install, and failed later on every attempt,
/// whatever PHP the user had switched to. Detection of a LINKED folder is a
/// different question (is this a Laravel project at all?) and keeps its own
/// markers.
pub fn is_installed(project: &Path) -> bool {
    project.join("artisan").is_file()
        && project.join("public/index.php").is_file()
        && project.join("vendor/autoload.php").is_file()
}

/// Is `project` exactly what a FAILED `composer create-project laravel/laravel`
/// leaves: the skeleton, with no dependencies installed?
///
/// Deliberately narrow, because the answer licenses deleting the folder's
/// contents ([`clear_failed_skeleton`]): the skeleton's own `composer.json`
/// name, an `artisan`, and no `vendor/` at all. A folder a person had started
/// working in — their own package name, or an installed `vendor/` — is not one.
pub fn is_failed_skeleton(project: &Path) -> bool {
    if !project.join("artisan").is_file() || project.join("vendor").exists() {
        return false;
    }
    std::fs::read_to_string(project.join("composer.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(|n| n == SKELETON_PACKAGE))
        .unwrap_or(false)
}

/// Empty a folder [`is_failed_skeleton`] recognises, so `create-project` (which
/// refuses a non-empty target) can run again on the site's CURRENT PHP. The
/// folder itself stays: it is the site's recorded path. Refuses anything else.
/// Callers must also hold that rexenv created the folder (`docroot_managed`).
pub fn clear_failed_skeleton(project: &Path) -> Result<()> {
    if !is_failed_skeleton(project) {
        return Err(Error::Other(format!(
            "{} is not a failed Laravel install — leaving it untouched",
            project.display()
        )));
    }
    for entry in std::fs::read_dir(project)? {
        let entry = entry?;
        // `file_type` does not follow symlinks: a link is removed as a link,
        // never walked into. A LINK is checked first, because on Windows the kind
        // of link decides the call: a directory symlink reports `is_dir() == false`
        // here (it is a link, not a directory) and `remove_file` on it fails with
        // "Access is denied" — measured on the Dell 17 Sep 2026, where it stopped
        // this function dead. `remove_dir` removes that link and leaves its target
        // alone; a file symlink takes `remove_file`, as on unix, where either call
        // works and this branch simply picks the same one it always did.
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            // Neither call works on both, and which one is needed depends on the OS AND
            // on what the link points at — measured both ways 17 Sep 2026:
            //
            //   macOS    remove_file(dir link) ok        remove_dir(dir link) NotADirectory
            //   Windows  remove_file(dir link) denied    remove_dir(dir link) ok
            //
            // So: try the file form, fall back to the directory form. Deliberately not
            // `entry.path().is_dir()` — that FOLLOWS the link, which gets a dangling one
            // wrong, and is what made my first attempt at this fail on macOS.
            if std::fs::remove_file(entry.path()).is_err() {
                std::fs::remove_dir(entry.path())?;
            }
        } else if kind.is_dir() {
            std::fs::remove_dir_all(entry.path())?;
        } else {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

/// `composer create-project laravel/laravel <project>` — the app itself.
///
/// Runs INTO an existing empty directory (`.` inside it), which is what site
/// provisioning has already made; Composer refuses a non-empty target, and that
/// refusal is the guard we want — it means this can never overwrite a folder
/// that has something in it.
#[allow(clippy::too_many_arguments)]
pub fn create_project(
    supervisor: &dyn ProcessSupervisor,
    php: &Path,
    composer_phar: &Path,
    project: &Path,
    env: &[(String, String)],
    cancel: &CancelToken,
    on_line: &mut dyn FnMut(&str),
) -> Result<()> {
    let args: Vec<String> = vec![
        composer_phar.to_string_lossy().into_owned(),
        "create-project".into(),
        SKELETON_PACKAGE.into(),
        ".".into(),
        "--no-interaction".into(),
        // The skeleton's post-create scripts are what write `.env` and generate
        // APP_KEY, so they must run — but they also ask about starting a dev
        // server on some versions; `--no-interaction` above answers no.
        "--prefer-dist".into(),
    ];
    on_line(&format!("$ composer create-project {SKELETON_PACKAGE} ."));
    // Package downloads stream per package — total silence is a wedge (B25).
    let result = run_step_streamed(
        supervisor,
        php,
        &args,
        project,
        env,
        cancel,
        on_line,
        Some(STEP_IDLE_LIMIT),
    )?;
    verdict(result, "composer create-project")?;
    // Composer can exit 0 having written nothing useful if a script failed
    // softly; the site is about to be SERVED, so prove the front controller is
    // really there rather than trusting the exit code.
    if !is_installed(project) {
        return Err(Error::Other(format!(
            "Composer finished but the Laravel app is not there: {} has no artisan + public/index.php",
            project.display()
        )));
    }
    Ok(())
}

/// The MAIL_* environment a rexenv-spawned process gets so a Laravel app's mail
/// lands in Mailpit — empty when the user has turned the catch-all off.
///
/// # Why the CLI needs its own copy of a pool setting
///
/// The php-fpm pool carries `env[MAIL_*]`, which covers mail a PAGE REQUEST
/// sends. Nothing of the pool reaches `php artisan`: a queue worker, a
/// scheduled command, a `tinker` one-liner and the provisioning steps all run
/// as fresh processes with the app's own `.env` and nothing else. This is the
/// same split that bit wp-cli on 25 Aug 2026 — `wp_mail()` caught through the
/// browser and dropped from the command line, with `true` returned both times —
/// and it is the same fix: put it in the ONE argv/env builder each surface uses.
///
/// Gated on the setting HERE, where a `Connection` exists, rather than inside
/// `core::mail`: the toggle has to mean the same thing on both surfaces, and a
/// CLI that ignored it would keep hijacking the mail of a developer who had
/// just asked to send it for real.
pub fn mail_env(conn: &Connection) -> Vec<(String, String)> {
    if !crate::core::mail::catch_all_enabled(conn) {
        return Vec::new();
    }
    crate::core::mail::laravel_env()
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
}

/// `php artisan <args…>` in the project root.
pub fn artisan(
    supervisor: &dyn ProcessSupervisor,
    php: &Path,
    project: &Path,
    args: &[&str],
    env: &[(String, String)],
    cancel: &CancelToken,
    on_line: &mut dyn FnMut(&str),
) -> Result<()> {
    let mut argv: Vec<String> = vec![project.join("artisan").to_string_lossy().into_owned()];
    argv.extend(args.iter().map(|a| (*a).to_string()));
    on_line(&format!("$ php artisan {}", args.join(" ")));
    let result =
        run_step_streamed(supervisor, php, &argv, project, env, cancel, on_line, Some(STEP_IDLE_LIMIT))?;
    verdict(result, &format!("php artisan {}", args.join(" ")))
}

/// Path of the project's `.env`.
pub fn env_path(project: &Path) -> PathBuf {
    project.join(".env")
}

/// Laravel's `.env` seed, for a repository that ships no `.env.example`.
///
/// Deliberately short: it is the keys Laravel would otherwise default to its
/// PRODUCTION posture (`APP_ENV=production`, `APP_DEBUG=false`) — a local site
/// that hides its own errors is the least useful failure mode there is.
/// Everything else is the project's business, and inventing config it never
/// asked for is how a "helpful" default becomes a bug report about rexenv.
const ENV_SEED: &str = "APP_NAME=Laravel\nAPP_ENV=local\nAPP_DEBUG=true\n";

/// The `.env` a freshly cloned Laravel project needs — see
/// [`crate::core::dotenv::ensure_file`], which never overwrites one.
pub fn ensure_env_file(project: &Path) -> Result<crate::core::dotenv::EnvOrigin> {
    crate::core::dotenv::ensure_file(project, ENV_SEED)
}

/// The database connection a site's `.env` must describe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbSettings {
    /// Laravel's driver name: `mysql` for both MySQL-protocol engines (Laravel
    /// has no separate `mariadb` driver before 11.x), `pgsql` for PostgreSQL.
    pub connection: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: String,
}

impl DbSettings {
    /// The settings for a site on `engine` — derived from the engine rather
    /// than typed at the call site, because three values move together and
    /// getting one of them wrong is silent: a `pgsql` connection on port 13306
    /// reaches MySQL, which answers the handshake with something libpq cannot
    /// read, and the error a developer sees names neither engine.
    ///
    /// The superuser differs too (`root` vs `postgres`) — trust auth, no
    /// password, exactly what each engine's datadir was initialized with in
    /// `core::database::initialize` / `core::postgres::initialize`.
    pub fn for_engine(engine: crate::core::db::DbEngine, database: String) -> DbSettings {
        use crate::core::db::DbEngine;
        let (connection, username) = match engine {
            DbEngine::Postgres => ("pgsql", "postgres"),
            _ => ("mysql", "root"),
        };
        DbSettings {
            connection: connection.into(),
            host: "127.0.0.1".into(),
            port: engine.port(),
            database,
            username: username.into(),
            password: String::new(),
        }
    }
}

/// Point a freshly created `.env` at the site: its database and its URL.
///
/// Returns the rewritten text rather than writing it, so the whole edit is
/// unit-testable against real `.env` shapes without a filesystem.
///
/// Every key is SET, present or not: the skeleton's `.env` ships `DB_*` lines
/// commented out in some versions and live-but-SQLite in others, and "the app
/// talks to the site's database" must not depend on which shape shipped today.
/// A commented-out line is replaced in place (not left beside a new one), so the
/// file never ends up with two answers for one key.
///
/// # Why the MAIL_* keys are here as well as in the pool
///
/// The pool's `env[MAIL_*]` already beats this file at runtime, so writing the
/// same values looks redundant. It is not, and the case that needs it is
/// `php artisan config:cache`: a cached config is baked from `env()` AT CACHE
/// TIME and `env()` is never consulted again, so a site that caches its config
/// keeps whatever its `.env` said and mails straight past Mailpit. Writing the
/// file is what makes the catch survive that.
///
/// `catch_mail` is the user's setting, threaded in rather than read here — off
/// means the project's own mail configuration is left exactly as it is, which
/// is the whole point of the switch.
pub fn wire_env(original: &str, app_url: &str, db: &DbSettings, catch_mail: bool) -> String {
    let mut keys = vec![
        ("APP_URL", app_url.to_string()),
        ("DB_CONNECTION", db.connection.clone()),
        ("DB_HOST", db.host.clone()),
        ("DB_PORT", db.port.to_string()),
        ("DB_DATABASE", db.database.clone()),
        ("DB_USERNAME", db.username.clone()),
        ("DB_PASSWORD", db.password.clone()),
    ];
    if catch_mail {
        keys.extend(crate::core::mail::laravel_env());
    }
    crate::core::dotenv::set_keys(original, keys)
}

/// Raw `php artisan <args…>` for the MCP runner (`site_artisan`) — the
/// [`crate::core::wordpress::wp_run_raw`] shape, on purpose:
///
/// - **A non-zero exit is an answer, not an error.** `artisan migrate:status`
///   exits 1 to say "pending"; the caller reports the code and both streams.
/// - **A hard timeout, always**, and stdin is `/dev/null`: `artisan tinker`
///   with nothing to read exits instead of waiting for a keyboard that is not
///   there, and a wedged command comes back as a killed one.
/// - **`--no-interaction` is rexenv's and goes LAST.** Symfony Console takes
///   the option wherever it sits, so this is a belt rather than a race — a
///   confirm prompt (`migrate:fresh` in production, `db:wipe`) answers itself
///   "no" instead of hanging on the timeout.
///
/// - **`env` outranks the project's `.env`** — see [`mail_env`], the catch-all's
///   command-line half.
///
/// The project root comes from the site row — the caller has already decided
/// WHICH artisan runs; this only runs it.
pub fn artisan_raw(
    php: &Path,
    project: &Path,
    args: &[String],
    env: &[(String, String)],
    timeout: std::time::Duration,
) -> Result<std::process::Output> {
    let mut cmd = std::process::Command::new(php);
    cmd.arg(project.join("artisan"))
        .args(args)
        .arg("--no-interaction")
        .current_dir(project);
    // Laravel's Dotenv repository is IMMUTABLE, so these beat the project's own
    // `.env` — which is the whole point: `mail_env` is how a command run against
    // a site configured for a real SMTP provider still lands in Mailpit.
    cmd.envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    let what = format!("php artisan {}", args.first().map(String::as_str).unwrap_or(""));
    crate::core::wordpress::run_with_timeout(cmd, timeout, &what)
}

/// A Composer package on disk, about to be linked into a project as a `path`
/// repository (D14 in `PLAN-mcp-parity.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerLink {
    /// The `name` from the source's `composer.json` — `vendor/package`.
    pub name: String,
    /// The repository key in the project's `composer.json`: the name with `/`
    /// (and anything else Composer would not take in a key) folded to `-`.
    pub key: String,
    /// The source directory, already canonical and blast-radius checked.
    pub source: PathBuf,
}

/// Read the package name out of `source/composer.json`.
///
/// The name is a FACT read from the source, never a parameter the agent
/// asserts (the scratch clone reads the plugin header the same way): a name
/// that disagrees with the manifest would link nothing and `require` the
/// wrong package from Packagist — over the network, as the user.
pub fn read_composer_link(source: &Path) -> Result<ComposerLink> {
    let manifest = source.join("composer.json");
    let raw = std::fs::read_to_string(&manifest).map_err(|_| {
        Error::Other(format!(
            "`{}` has no composer.json — a Composer path repository needs one with a `name`.",
            source.file_name().and_then(|n| n.to_str()).unwrap_or("that folder")
        ))
    })?;
    let json: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| Error::Other(format!("the source's composer.json is not valid JSON: {e}")))?;
    let name = json
        .get("name")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|n| is_composer_package_name(n))
        .ok_or_else(|| {
            Error::Other(
                "the source's composer.json has no `name` of the form `vendor/package` — \
                 Composer cannot require a package without one."
                    .into(),
            )
        })?;
    let key: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    Ok(ComposerLink { name: name.to_string(), key, source: source.to_path_buf() })
}

/// Composer's own rule for a package name, without the regex: lowercase
/// `vendor/package`, each side non-empty and made of `[a-z0-9_.-]`.
fn is_composer_package_name(name: &str) -> bool {
    let Some((vendor, package)) = name.split_once('/') else { return false };
    let ok = |s: &str| {
        !s.is_empty()
            && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-'))
            && s.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
    };
    ok(vendor) && ok(package) && !package.contains('/')
}

/// The two Composer invocations a link IS, as argv (the phar first — Composer
/// runs through the site's PHP), so a test can hold them still.
///
/// **The repository is a SYMLINK, stated explicitly** (`"symlink": true` is
/// Composer's default, written out so the ruling is in the file, not in a
/// default someone reads later). This is the S1 question re-run for Composer
/// (D14): the scratch clone protected a checkout from an UNATTENDED raw runner
/// on the agent's own site; here the site is the user's, the `run` grant they
/// answered says code of the agent's choosing runs there as them, and a live
/// checkout is what a Composer path repository is FOR. The truth a symlink
/// carries — the site writes into `vendor/<name>` land in the checkout — is
/// said in the reply rather than engineered away.
///
/// `@dev` is the stability that lets Composer pick the path repository over
/// a Packagist release of the same name, and `--no-interaction` answers every
/// prompt "no" rather than waiting.
pub fn composer_link_argv(link: &ComposerLink, composer_phar: &Path) -> [Vec<String>; 2] {
    let phar = composer_phar.to_string_lossy().into_owned();
    let spec = serde_json::json!({
        "type": "path",
        "url": link.source.display().to_string(),
        "options": { "symlink": true },
    })
    .to_string();
    [
        vec![phar.clone(), "config".into(), format!("repositories.{}", link.key), spec, "--no-interaction".into()],
        vec![phar, "require".into(), format!("{}:@dev", link.name), "--no-interaction".into()],
    ]
}

/// Link a package checkout into a project: write the path repository, then
/// `composer require <name>:@dev`. Streams Composer's lines (the same idle
/// limit as an install — silence is a wedge, B25); the require's failure maps
/// through the same Composer error table as `composer install`.
#[allow(clippy::too_many_arguments)]
pub fn composer_link(
    supervisor: &dyn ProcessSupervisor,
    php: &Path,
    composer_phar: &Path,
    project: &Path,
    link: &ComposerLink,
    env: &[(String, String)],
    cancel: &CancelToken,
    on_line: &mut dyn FnMut(&str),
) -> Result<()> {
    let [config, require] = composer_link_argv(link, composer_phar);
    on_line(&format!("$ composer config repositories.{} path <source>", link.key));
    let result = run_step_streamed(supervisor, php, &config, project, env, cancel, on_line, Some(STEP_IDLE_LIMIT))?;
    verdict(result, "composer config")?;
    on_line(&format!("$ composer require {}:@dev --no-interaction", link.name));
    let result = run_step_streamed(supervisor, php, &require, project, env, cancel, on_line, Some(STEP_IDLE_LIMIT))?;
    verdict(result, &format!("composer require {}", link.name))
}

fn verdict(result: StepResult, what: &str) -> Result<()> {
    if result.ok {
        return Ok(());
    }
    if result.cancelled {
        return Err(Error::Other(format!("{what} cancelled")));
    }
    Err(map_composer_error(&result.tail))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::dotenv::EnvOrigin;

    /// **The link reads its name from the SOURCE's manifest, folds it to a key
    /// Composer accepts, writes the repository as an explicit symlink and
    /// requires it at `@dev`, non-interactively — and a manifest without a
    /// `vendor/package` name is refused before Composer is asked anything.**
    #[test]
    fn a_composer_link_is_read_from_the_manifest_and_pinned_as_a_symlink_at_dev() {
        let dir = std::env::temp_dir().join(format!("rexenv-laravel-{}-link", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let err = read_composer_link(&dir).unwrap_err().to_string();
        assert!(err.contains("no composer.json"), "{err}");
        std::fs::write(dir.join("composer.json"), r#"{"name": "Acme Widgets"}"#).unwrap();
        let err = read_composer_link(&dir).unwrap_err().to_string();
        assert!(err.contains("`vendor/package`"), "{err}");
        std::fs::write(dir.join("composer.json"), r#"{"name": "acme/widgets.v2", "type": "library"}"#).unwrap();

        let link = read_composer_link(&dir).unwrap();
        assert_eq!(link.name, "acme/widgets.v2");
        assert_eq!(link.key, "acme-widgets-v2", "the key is the name with everything Composer's key grammar refuses folded to `-`");
        let [config, require] = composer_link_argv(&link, Path::new("/bin/composer.phar"));
        assert_eq!(&config[..3], &["/bin/composer.phar", "config", "repositories.acme-widgets-v2"]);
        let spec: serde_json::Value = serde_json::from_str(&config[3]).unwrap();
        assert_eq!(spec["type"], "path");
        assert_eq!(spec["url"], dir.display().to_string());
        assert_eq!(spec["options"]["symlink"], true, "the ruling is written into the file, not left to Composer's default");
        assert_eq!(config.last().map(String::as_str), Some("--no-interaction"));
        assert_eq!(require, vec!["/bin/composer.phar", "require", "acme/widgets.v2:@dev", "--no-interaction"]);
        assert!(!is_composer_package_name("acme/"), "an empty package side");
        assert!(!is_composer_package_name("acme/one/two"));
        assert!(!is_composer_package_name("-acme/x"), "a leading separator");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// **`artisan_raw` runs the project's own `artisan` through the given PHP
    /// in the project directory, puts `--no-interaction` LAST, reads no stdin
    /// and reports a non-zero exit as output rather than an error.** Proven
    /// with a fake "php" that prints its argv and exits 3.
    /// **`config:cache` is the hole the file half exists to close.**
    ///
    /// The pool's `env[MAIL_*]` beats `.env` at runtime, so writing the same
    /// values into the file looks redundant — until the app caches its config,
    /// at which point `env()` is never read again and the BAKED value decides
    /// where mail goes. A wired `.env` is what makes the catch survive that.
    #[test]
    fn wiring_points_the_env_file_at_mailpit_so_a_cached_config_bakes_the_catch() {
        let original = "APP_NAME=Shop\nMAIL_MAILER=smtp\nMAIL_HOST=smtp.mailgun.org\n\
             MAIL_PORT=587\nMAIL_USERNAME=postmaster@shop.test\nMAIL_PASSWORD=hunter2\n";
        let out = wire_env(original, "https://shop.rex", &db(), true);
        assert!(out.contains("MAIL_HOST=127.0.0.1"));
        assert!(out.contains(&format!("MAIL_PORT={}", crate::core::mail::MAILPIT_SMTP_PORT)));
        // The real provider's credentials are REPLACED, not left beside the new
        // values: two answers for one key is how a file starts lying.
        assert!(!out.contains("smtp.mailgun.org"), "{out}");
        assert!(!out.contains("hunter2"), "{out}");
        assert_eq!(out.matches("MAIL_HOST=").count(), 1);
        // Every key the pool sets, the file sets — a subset here would mean the
        // cached config and the live config disagreed about where mail goes.
        for (k, v) in crate::core::mail::laravel_env() {
            assert!(out.contains(&format!("{k}={v}")), "missing {k} in:\n{out}");
        }
    }

    /// **Off means the project's own mail config is left alone — untouched, not
    /// re-pointed at the provider we guessed it wanted.**
    ///
    /// The switch exists for the developer deliberately proving a live SES or
    /// Postmark integration from a local box. A `wire_env` that "helpfully"
    /// normalised MAIL_* while off would break exactly that task.
    #[test]
    fn wiring_leaves_mail_alone_when_the_catch_all_is_off() {
        let original = "MAIL_MAILER=ses\nMAIL_HOST=email-smtp.eu-west-1.amazonaws.com\n";
        let out = wire_env(original, "https://shop.rex", &db(), false);
        assert!(out.contains("MAIL_MAILER=ses"));
        assert!(out.contains("email-smtp.eu-west-1.amazonaws.com"));
        assert!(!out.contains("127.0.0.1:"), "{out}");
        // The database half still happens: the switch is about mail only.
        assert!(out.contains("DB_DATABASE=lv_shop_rex"));
    }

    /// The CLI half answers to the SAME switch as the pool half. A command-line
    /// runner that always caught would hijack the mail of a developer who had
    /// just turned catching off, and it would do it on the surface they were
    /// most likely testing from.
    #[test]
    fn the_cli_mail_env_answers_to_the_same_switch_as_the_pool() {
        let conn = crate::state::db::open_in_memory().unwrap();
        let on = mail_env(&conn);
        assert_eq!(on.len(), crate::core::mail::laravel_env().len());
        assert!(on.iter().any(|(k, v)| k == "MAIL_PORT"
            && v == &crate::core::mail::MAILPIT_SMTP_PORT.to_string()));

        crate::state::store::set_setting(&conn, crate::core::mail::CATCH_ALL_KEY, "false").unwrap();
        assert!(mail_env(&conn).is_empty(), "off must add nothing, not add a different sink");
    }

    // The fixture IS a `#!/bin/sh` script standing in for `php`, which Windows cannot
    // execute at all (`os error 193`, measured on the Dell — W12). Same reason
    // `devtools`' probe_version test is unix-only, and stated the same way. What Windows
    // therefore loses is real and worth naming: nothing checks the artisan argv shape
    // (`--no-interaction` last, the project as cwd, stdin closed) on that OS.
    #[cfg(unix)] // runs a `#!/bin/sh` script as the fake `php`
    #[test]
    fn artisan_raw_runs_in_the_project_with_no_interaction_last_and_returns_a_nonzero_exit() {
        let dir = std::env::temp_dir().join(format!("rexenv-laravel-{}-artisan", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fake_php = dir.join("php");
        std::fs::write(&fake_php, "#!/bin/sh\npwd\nfor a in \"$@\"; do echo \"$a\"; done\nread x && echo \"read: $x\"\nexit 3\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake_php, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let out = artisan_raw(&fake_php, &dir, &["migrate:status".to_string(), "--pending".to_string()], &[], std::time::Duration::from_secs(10)).unwrap();
        assert_eq!(out.status.code(), Some(3), "a non-zero exit is an answer");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let lines: Vec<&str> = stdout.lines().collect();
        assert_eq!(std::fs::canonicalize(lines[0]).unwrap(), std::fs::canonicalize(&dir).unwrap(), "runs in the project root");
        assert_eq!(&lines[1..], &[dir.join("artisan").display().to_string().as_str(), "migrate:status", "--pending", "--no-interaction"]);
        assert!(!stdout.contains("read:"), "stdin is /dev/null — nothing was read");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_engine_decides_driver_port_and_superuser_together() {
        use crate::core::db::DbEngine;
        // Three values that must move as one: a `pgsql` connection on 13306
        // reaches MySQL, whose handshake libpq cannot read, and the error names
        // neither engine.
        let pg = DbSettings::for_engine(DbEngine::Postgres, "lv_shop_rex".into());
        assert_eq!((pg.connection.as_str(), pg.port, pg.username.as_str()), ("pgsql", 15432, "postgres"));

        // Laravel has no `mariadb` driver before 11.x, so both MySQL-protocol
        // engines are `mysql` — differing only in the port.
        for (engine, port) in [(DbEngine::Mysql, 13306), (DbEngine::Mariadb, 13307)] {
            let s = DbSettings::for_engine(engine, "lv_shop_rex".into());
            assert_eq!((s.connection.as_str(), s.port, s.username.as_str()), ("mysql", port, "root"));
        }

        // And the wired `.env` carries one answer per key — a leftover
        // `DB_CONNECTION=sqlite` from the skeleton would send the app at a file.
        let wired = wire_env("DB_CONNECTION=sqlite\nDB_HOST=x\n", "https://s.rex", &pg, false);
        assert_eq!(wired.matches("DB_CONNECTION=").count(), 1);
        assert!(wired.contains("DB_CONNECTION=pgsql") && !wired.contains("sqlite"));
        assert!(wired.contains("DB_PORT=15432") && wired.contains("DB_USERNAME=postgres"));
    }

    fn db() -> DbSettings {
        DbSettings {
            connection: "mysql".into(),
            host: "127.0.0.1".into(),
            port: 13306,
            database: "lv_shop_rex".into(),
            username: "root".into(),
            password: String::new(),
        }
    }

    /// The shape `composer create-project laravel/laravel` REALLY writes —
    /// copied verbatim from a live run (laravel/framework ^13.8, 9 Aug 2026):
    /// `DB_CONNECTION` live and SQLITE, the MySQL keys COMMENTED OUT. A tidier
    /// invented fixture would not have exercised the commented block at all,
    /// and that is the half that decides whether the app talks to the database
    /// this site advertises or to a SQLite file nobody can see.
    #[test]
    fn wire_env_replaces_the_sqlite_default_and_uncomments_the_db_block() {
        let original = "APP_NAME=Laravel\n\
             APP_URL=http://localhost\n\
             \n\
             LOG_LEVEL=debug\n\
             \n\
             DB_CONNECTION=sqlite\n\
             # DB_HOST=127.0.0.1\n\
             # DB_PORT=3306\n\
             # DB_DATABASE=laravel\n\
             # DB_USERNAME=root\n\
             # DB_PASSWORD=\n\
             \n\
             SESSION_DRIVER=database\n";
        let out = wire_env(original, "https://shop.rex", &db(), true);

        assert!(out.contains("DB_CONNECTION=mysql"));
        assert!(!out.contains("sqlite"), "the SQLite default must be gone, not merely overridden below");
        assert!(out.contains("DB_DATABASE=lv_shop_rex"));
        assert!(out.contains("DB_PORT=13306"));
        assert!(out.contains("APP_URL=https://shop.rex"));
        assert!(!out.contains("http://localhost"));
        // No commented-out survivors: one answer per key.
        assert!(!out.contains("# DB_"), "a commented twin is a second answer: {out}");
        assert!(out.ends_with('\n'), "the trailing newline is preserved");
        // Untouched keys stay — including the ones that merely sit near the DB
        // block, which a line-range edit would have eaten.
        assert!(out.contains("APP_NAME=Laravel"));
        assert!(out.contains("LOG_LEVEL=debug"));
        assert!(out.contains("SESSION_DRIVER=database"));
    }

    /// The older shape: live MySQL keys with the skeleton's placeholder values.
    #[test]
    fn wire_env_overwrites_live_placeholder_values_exactly_once() {
        let original = "DB_CONNECTION=mysql\n\
             DB_HOST=127.0.0.1\n\
             DB_PORT=3306\n\
             DB_DATABASE=laravel\n\
             DB_USERNAME=root\n\
             DB_PASSWORD=secret\n";
        let out = wire_env(original, "https://shop.rex", &db(), true);

        assert_eq!(out.matches("DB_DATABASE=").count(), 1);
        assert!(out.contains("DB_DATABASE=lv_shop_rex"));
        assert!(!out.contains("DB_DATABASE=laravel"));
        // The site's database has no password; an inherited `secret` would make
        // every query fail with an auth error that reads like a rexenv bug.
        assert!(out.contains("DB_PASSWORD="));
        assert!(!out.contains("DB_PASSWORD=secret"));
        // APP_URL was absent — appended, not silently dropped.
        assert!(out.contains("APP_URL=https://shop.rex"));
    }

    /// `export DB_HOST=…` is valid `.env` and the reader in `phpconf` tolerates
    /// it; the writer must not leave such a line beside its replacement.
    #[test]
    fn wire_env_replaces_an_exported_key_rather_than_appending_beside_it() {
        let out = wire_env("export DB_HOST=db.internal\n", "https://shop.rex", &db(), true);
        assert_eq!(out.matches("DB_HOST=").count(), 1);
        assert!(out.contains("DB_HOST=127.0.0.1"));
        assert!(!out.contains("db.internal"));
    }

    /// Fresh throwaway project dir. Named per test so a failure leaves exactly
    /// one identifiable directory behind, and removed only by path we built.
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("rexenv-laravel-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The three shapes a cloned repo actually arrives in, and the one thing
    /// that must never happen in any of them: an existing `.env` being lost.
    #[test]
    fn ensure_env_file_never_overwrites_and_names_where_the_file_came_from() {
        // Normal: `.env` gitignored, `.env.example` committed.
        let dir = scratch("env-example");
        std::fs::write(dir.join(".env.example"), "APP_NAME=Shop\nMAIL_MAILER=log\n").unwrap();
        assert_eq!(ensure_env_file(&dir).unwrap(), EnvOrigin::Example);
        let written = std::fs::read_to_string(env_path(&dir)).unwrap();
        assert!(written.contains("MAIL_MAILER=log"), "the example's own keys must survive");

        // Second run (a Retry after a later phase failed): reported, NOT redone.
        // Proven by content, not by the verdict alone — a copy that ran again
        // would return `Example` too and quietly discard the edit below.
        std::fs::write(env_path(&dir), "APP_NAME=Shop\nMAIL_MAILER=smtp\nAPP_KEY=base64:x\n")
            .unwrap();
        assert_eq!(ensure_env_file(&dir).unwrap(), EnvOrigin::Repo);
        let kept = std::fs::read_to_string(env_path(&dir)).unwrap();
        assert!(kept.contains("MAIL_MAILER=smtp"), "an existing .env is never re-copied over");
        assert!(kept.contains("APP_KEY=base64:x"), "a generated key must survive a retry");
        std::fs::remove_dir_all(&dir).unwrap();

        // Repo committed a `.env`: kept whole. `wire_env` rewrites APP_URL and
        // the DB block on top; dropping the file would take the app's mail,
        // queue and third-party keys with it.
        let dir = scratch("env-committed");
        std::fs::write(dir.join(".env"), "STRIPE_KEY=sk_live_xyz\n").unwrap();
        std::fs::write(dir.join(".env.example"), "STRIPE_KEY=\n").unwrap();
        assert_eq!(ensure_env_file(&dir).unwrap(), EnvOrigin::Repo);
        assert!(std::fs::read_to_string(env_path(&dir)).unwrap().contains("sk_live_xyz"));
        std::fs::remove_dir_all(&dir).unwrap();

        // Neither: a seed that keeps the site debuggable. Laravel's own
        // fallbacks are the production posture (`APP_ENV=production`,
        // `APP_DEBUG=false`) — a local site that hides its errors.
        let dir = scratch("env-neither");
        assert_eq!(ensure_env_file(&dir).unwrap(), EnvOrigin::Seeded);
        let seeded = std::fs::read_to_string(env_path(&dir)).unwrap();
        assert!(seeded.contains("APP_ENV=local"));
        assert!(seeded.contains("APP_DEBUG=true"));
        // The seed is the floor, not a config the user never asked for: the
        // DB block is `wire_env`'s job and must not be guessed at here.
        assert!(!seeded.contains("DB_"), "the seed invents no database settings: {seeded}");
        // And it composes: wiring the seed yields a complete local .env.
        let wired = wire_env(&seeded, "https://shop.rex", &db(), true);
        assert!(wired.contains("APP_ENV=local") && wired.contains("DB_DATABASE=lv_shop_rex"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// **"Installed" means the skeleton AND its dependencies; a skeleton without
    /// `vendor/` is what a failed `create-project` leaves, and only THAT shape
    /// may be cleared for a retry.** The failed shape is the one measured on
    /// 11 Sep 2026 (PHP 8.1, advisory-blocked Laravel 10): `artisan`,
    /// `public/index.php`, the skeleton's `composer.json`, no `vendor/`.
    #[test]
    fn a_skeleton_without_dependencies_is_not_installed_and_only_it_may_be_cleared() {
        let dir = std::env::temp_dir()
            .join(format!("rexenv-laravel-{}-installed", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        assert!(!is_installed(&dir));
        assert!(!is_failed_skeleton(&dir), "an empty folder is nothing to clear");
        std::fs::write(dir.join("artisan"), "#!/usr/bin/env php").unwrap();
        assert!(!is_installed(&dir), "artisan alone is a half-written project");
        std::fs::create_dir_all(dir.join("public")).unwrap();
        std::fs::write(dir.join("public/index.php"), "<?php").unwrap();
        assert!(!is_installed(&dir), "no vendor/ — the shape a failed create-project leaves");

        // Not the skeleton's own manifest → a person's project → never cleared.
        std::fs::write(dir.join("composer.json"), r#"{"name": "acme/shop"}"#).unwrap();
        assert!(!is_failed_skeleton(&dir));
        assert!(clear_failed_skeleton(&dir).is_err());
        assert!(dir.join("artisan").is_file(), "a refused clear touched nothing");

        // The skeleton's manifest, no vendor/ → the failed install.
        std::fs::write(dir.join("composer.json"), r#"{"name": "laravel/laravel"}"#).unwrap();
        assert!(is_failed_skeleton(&dir));

        // Any vendor/ at all → dependencies were (at least partly) installed →
        // not ours to clear, even though autoload.php is missing.
        std::fs::create_dir_all(dir.join("vendor")).unwrap();
        assert!(!is_failed_skeleton(&dir));
        assert!(!is_installed(&dir));
        std::fs::write(dir.join("vendor/autoload.php"), "<?php").unwrap();
        assert!(is_installed(&dir));
        std::fs::remove_dir_all(dir.join("vendor")).unwrap();

        // Cleared: contents gone, the folder (the site's recorded path) kept, and
        // a symlink inside removed as a link — its target survives.
        let outside = dir.with_extension("outside");
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("keep.txt"), "mine").unwrap();
        crate::test_support::symlink(&outside, dir.join("linked")).unwrap();
        clear_failed_skeleton(&dir).unwrap();
        assert!(dir.is_dir());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        assert!(outside.join("keep.txt").is_file(), "a link is never walked into");

        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&outside).unwrap();
    }
}

#[cfg(test)]
mod live_shape_tests {
    use super::*;

    /// Runs the writer over the `.env` from a REAL `composer create-project` run.
    ///
    /// **A MANUAL leg, and it has never run automatically** (recorded 21 Aug
    /// 2026, ledger #245). It was written as the thing that stops the
    /// hand-copied fixture above going stale "in silence" — but nothing sets
    /// `REXENV_LARAVEL_DOTENV`: no example runs `composer create-project` (the
    /// clone-based checks read the repo's `.env.example`, which is a different
    /// file), so this skipped on every run since the day it was added. A guard
    /// against silence that is itself silent is worth less than no guard, because
    /// the ledger row was counting it.
    ///
    /// To actually run it, stage a real generated file and point at it:
    ///
    /// ```text
    /// composer create-project laravel/laravel /tmp/lv   # or take the .env from a real site
    /// REXENV_LARAVEL_DOTENV=/tmp/lv/.env cargo test --lib wire_env_over_a_real
    /// ```
    ///
    /// The skip is now LOUD — it prints what it wanted and how to give it that,
    /// visible under `cargo test -- --nocapture`, rather than returning in
    /// silence.
    #[test]
    fn wire_env_over_a_real_generated_dotenv_when_one_is_staged() {
        let Ok(path) = std::env::var("REXENV_LARAVEL_DOTENV") else {
            println!(
                "SKIPPED: REXENV_LARAVEL_DOTENV is unset, so the writer was not run over a \
                 real generated .env. Stage one and re-run: \
                 REXENV_LARAVEL_DOTENV=<path>/.env cargo test --lib wire_env_over_a_real"
            );
            return;
        };
        let Ok(original) = std::fs::read_to_string(&path) else {
            println!("SKIPPED: REXENV_LARAVEL_DOTENV={path} could not be read");
            return;
        };
        let db = DbSettings {
            connection: "mysql".into(),
            host: "127.0.0.1".into(),
            port: 13306,
            database: "wp_lv_rex".into(),
            username: "root".into(),
            password: String::new(),
        };
        let out = wire_env(&original, "https://lv.rex", &db, true);
        assert!(out.contains("DB_CONNECTION=mysql"));
        assert!(out.contains("DB_DATABASE=wp_lv_rex"));
        assert!(out.contains("APP_URL=https://lv.rex"));
        assert!(!out.contains("DB_CONNECTION=sqlite"));
        assert!(!out.contains("# DB_DATABASE"));
        // Nothing else was disturbed: APP_KEY must survive verbatim.
        for line in original.lines().filter(|l| l.starts_with("APP_KEY=")) {
            assert!(out.contains(line), "APP_KEY must survive the rewrite");
        }
    }
}

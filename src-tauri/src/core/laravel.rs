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

/// The Composer package a new site is created from. Pinned to the meta-package
/// rather than a version: `laravel/laravel` IS the skeleton, and Composer
/// resolves the current stable release of it.
pub const SKELETON_PACKAGE: &str = "laravel/laravel";

/// Has a Laravel app actually been installed into `project`? The two markers
/// detection uses for a LINKED project (`core::sites::detect`), asked here of a
/// project we just wrote — so "installed" means the same thing whether the app
/// arrived from Composer or from the user's disk.
pub fn is_installed(project: &Path) -> bool {
    project.join("artisan").is_file() && project.join("public/index.php").is_file()
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

/// The database connection a site's `.env` must describe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbSettings {
    /// `mysql` — both engines rexenv ships speak the MySQL protocol, and
    /// Laravel has no separate `mariadb` driver before 11.x.
    pub connection: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: String,
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
pub fn wire_env(original: &str, app_url: &str, db: &DbSettings) -> String {
    let mut text = original.to_string();
    for (key, value) in [
        ("APP_URL", app_url.to_string()),
        ("DB_CONNECTION", db.connection.clone()),
        ("DB_HOST", db.host.clone()),
        ("DB_PORT", db.port.to_string()),
        ("DB_DATABASE", db.database.clone()),
        ("DB_USERNAME", db.username.clone()),
        ("DB_PASSWORD", db.password.clone()),
    ] {
        text = set_env_key(&text, key, &value);
    }
    text
}

/// Set one `KEY=value` in `.env` text: replaces the first live line for the key,
/// un-comments and replaces a `# KEY=…` line, or appends the entry.
///
/// Deliberately simple — this file is one WE just generated, not a user's
/// hand-edited config, which is why it does not carry
/// [`crate::core::confedit`]'s refuse-rather-than-guess machinery. It writes
/// values unquoted, so the caller's values must not need quoting: the only
/// dynamic ones are our own `wp_`/`rex_` identifiers, a port, and an https URL
/// built from an already-validated domain.
fn set_env_key(text: &str, key: &str, value: &str) -> String {
    let line = format!("{key}={value}");
    let mut out: Vec<String> = Vec::new();
    let mut written = false;
    for raw in text.lines() {
        let trimmed = raw.trim_start().trim_start_matches("export ").trim_start();
        let is_live = trimmed.starts_with(&format!("{key}="));
        let is_commented = trimmed
            .strip_prefix('#')
            .map(|rest| rest.trim_start().starts_with(&format!("{key}=")))
            .unwrap_or(false);
        if (is_live || is_commented) && !written {
            out.push(line.clone());
            written = true;
        } else if is_live || is_commented {
            // A duplicate for the same key: drop it rather than leave a second
            // answer below the one we just wrote.
            continue;
        } else {
            out.push(raw.to_string());
        }
    }
    if !written {
        out.push(line);
    }
    let mut joined = out.join("\n");
    if text.ends_with('\n') {
        joined.push('\n');
    }
    joined
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
        let out = wire_env(original, "https://shop.rex", &db());

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
        let out = wire_env(original, "https://shop.rex", &db());

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
        let out = wire_env("export DB_HOST=db.internal\n", "https://shop.rex", &db());
        assert_eq!(out.matches("DB_HOST=").count(), 1);
        assert!(out.contains("DB_HOST=127.0.0.1"));
        assert!(!out.contains("db.internal"));
    }

    #[test]
    fn is_installed_wants_both_markers_the_linked_project_detector_wants() {
        let dir = std::env::temp_dir()
            .join(format!("rexenv-laravel-{}-installed", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        assert!(!is_installed(&dir));
        std::fs::write(dir.join("artisan"), "#!/usr/bin/env php").unwrap();
        assert!(!is_installed(&dir), "artisan alone is a half-written project");
        std::fs::create_dir_all(dir.join("public")).unwrap();
        std::fs::write(dir.join("public/index.php"), "<?php").unwrap();
        assert!(is_installed(&dir));

        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[cfg(test)]
mod live_shape_tests {
    use super::*;

    /// Runs the writer over the .env from a REAL `composer create-project` run
    /// staged by the live check, when one is present. Skips silently otherwise
    /// so the unit suite stays hermetic — this exists to catch the day the
    /// skeleton changes its .env shape and the hand-copied fixture above goes
    /// stale without anything failing.
    #[test]
    fn wire_env_over_a_real_generated_dotenv_when_one_is_staged() {
        let Ok(path) = std::env::var("REXENV_LARAVEL_DOTENV") else { return };
        let Ok(original) = std::fs::read_to_string(&path) else { return };
        let db = DbSettings {
            connection: "mysql".into(),
            host: "127.0.0.1".into(),
            port: 13306,
            database: "wp_lv_rex".into(),
            username: "root".into(),
            password: String::new(),
        };
        let out = wire_env(&original, "https://lv.rex", &db);
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

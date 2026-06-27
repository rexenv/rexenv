//! core::wordpress — WP-CLI wrapper (Phase 1 §9).
//!
//! WP-CLI is a `.phar` run through the bundled PHP: `php wp-cli.phar <args>`.
//! Invocation is identical on every OS (the per-OS bit is which `php` binary,
//! resolved via `BinaryProvider`), so this runs the command directly and
//! captures output (unlike `ProcessSupervisor`, which is for long-lived services).

use crate::error::{Error, Result};
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
}

/// One-click WordPress install via WP-CLI: download core → write wp-config →
/// create the DB → `wp core install` (single-site). Each step is skipped if
/// already done, so the flow is re-runnable.
pub fn install_wordpress(php_bin: &Path, wp_phar: &Path, opts: &WpInstall) -> Result<()> {
    let path = format!("--path={}", opts.docroot.display());

    // 1) WordPress core.
    if !opts.docroot.join("wp-load.php").exists() {
        wp_cli_checked(php_bin, wp_phar, &["core", "download", &path], None)?;
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

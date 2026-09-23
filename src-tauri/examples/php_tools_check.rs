//! Live check: **every pinned PHP can run the tools rexenv drives it with.**
//! `cargo run --example php_tools_check`
//!
//! ## Why this exists
//!
//! rexenv does not run WP-CLI or Composer with "some PHP" — it runs them with
//! **the site's** PHP, deliberately, so a plugin's platform checks match the
//! interpreter it will actually execute on. That makes "this PHP can run our
//! tools" a claim about EVERY pinned minor, and nothing checked it: the WordPress
//! examples resolve `binaries::pins().php` and create sites on `"8.3"`, so they
//! prove the default and assume the rest.
//!
//! It cost a real failure. A self-built PHP 7.4 shipped without `phar`, and since
//! WP-CLI and Composer ARE phars, every WordPress action on a 7.4 site died with
//! `Class 'Phar' not found` — while `php -v`, `php -m` and the whole build gate
//! looked healthy, because a list of extension names is a proxy for "can this PHP
//! do the job" and this check is the job.
//!
//! Deliberately cheap: no MySQL, no site, no services — resolve each PHP and each
//! phar, run `--version`, done. A check that needs a whole stack to answer "can
//! this interpreter load an archive" would not get run.
//!
//! Fixture scope: resolves into the SHARED binary cache (the documented exception
//! in `examples/common/mod.rs`) and spawns nothing that outlives the run.

use rexenv_lib::core::{binaries, wordpress, wp_packages};
use rexenv_lib::platform;
use std::path::Path;
use std::process::{Command, ExitCode};

mod common;

/// Run a phar through `php` and return the line carrying its version.
///
/// `args` is the FULL argv rexenv itself would use, not a naive `php <phar>`:
/// for WP-CLI that means `wordpress::wp_argv_prefix`, which carries
/// `-d display_errors=stderr`. Without it this check fails PHP 8.5 for a reason
/// that is not a failure — the CLI SAPI writes diagnostics to STDOUT, so
/// WP-CLI's vendored `react/promise` deprecation arrives ahead of the version
/// string and the first line is a notice (ledger #316/#317, the same trap that
/// broke every WordPress read on 8.5). Borrowing rexenv's own prefix means this
/// check tests the invocation the app performs and cannot drift from it.
fn tool_version(php: &Path, args: &[String], wp_phar: &Path) -> Result<String, String> {
    // Pin the command set, like every other wp-cli spawn in the tree (#228).
    // rexenv's source scan caught this example running unpinned — the guard
    // exists because a bug that depends on `~/.wp-cli/packages` appears in no
    // log, diff or bug report, and a CHECK that inherits it is worse than a
    // feature that does: it would report a machine's own packages as our result.
    let (pk, pv) = wp_packages::pin_packages_env(wp_phar);
    let out = Command::new(php)
        .args(args)
        .arg("--version")
        .env(pk, pv)
        .output()
        .map_err(|e| format!("spawn: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        // The phar failure mode is a PHP fatal on stderr, not a bad exit alone —
        // report it verbatim so the reason is in the log, not just "failed".
        return Err(format!(
            "exit {}: {}",
            out.status,
            stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("(no output)")
        ));
    }
    // Take the line that IDENTIFIES the tool, not merely the first one: even
    // with diagnostics moved to stderr, a tool is free to print a banner.
    Ok(stdout
        .lines()
        .chain(stderr.lines())
        .map(str::trim)
        .find(|l| l.contains("WP-CLI") || l.contains("Composer"))
        .unwrap_or_else(|| {
            stdout.lines().chain(stderr.lines()).map(str::trim).find(|l| !l.is_empty()).unwrap_or("(no output)")
        })
        .to_string())
}

#[tokio::main]
async fn main() -> ExitCode {
    let plat = platform::current();
    let mut checks = common::Check::new("php_tools_check");

    let wp = binaries::resolve_file(&*plat, "wp-cli", binaries::pins().wp_cli)
        .await
        .expect("resolve wp-cli phar");
    let composer = binaries::resolve_file(&*plat, "composer", binaries::pins().composer)
        .await
        .expect("resolve composer phar");
    println!("wp-cli  {}\ncomposer {}\n", wp.display(), composer.display());

    for version in binaries::pins().php_versions {
        let php = match binaries::resolve(&*plat, "php", version).await {
            Ok(p) => p,
            Err(e) => {
                checks.is(&format!("PHP {version} resolves"), false, &e.to_string());
                continue;
            }
        };

        // `phar` is what makes both of these possible. Named separately so the
        // log says WHY when they fail, instead of only that they did.
        let mods = Command::new(&php).arg("-m").output().expect("php -m");
        let has_phar = String::from_utf8_lossy(&mods.stdout)
            .lines()
            .any(|l| l.trim().eq_ignore_ascii_case("phar"));
        checks.is(&format!("PHP {version} has phar"), has_phar, "absent — no phar, no wp-cli, no composer");

        match tool_version(&php, &wordpress::wp_argv_prefix(&wp), &wp) {
            Ok(v) => {
                println!("  PHP {version:<8} wp-cli   → {v}");
                checks.is(&format!("PHP {version} runs WP-CLI"), v.contains("WP-CLI"), &v);
            }
            Err(e) => {
                println!("  PHP {version:<8} wp-cli   → FAILED: {e}");
                checks.is(&format!("PHP {version} runs WP-CLI"), false, &e);
            }
        }

        match tool_version(&php, &[composer.display().to_string()], &wp) {
            Ok(v) => {
                println!("  PHP {version:<8} composer → {v}");
                checks.is(&format!("PHP {version} runs Composer"), v.contains("Composer"), &v);
            }
            Err(e) => {
                println!("  PHP {version:<8} composer → FAILED: {e}");
                checks.is(&format!("PHP {version} runs Composer"), false, &e);
            }
        }
    }

    checks.verdict()
}

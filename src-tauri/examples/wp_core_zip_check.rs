//! Ledger #604 against the REAL WordPress downloads, with rexenv's own PHP and WP-CLI: a
//! downloaded core is COMPLETE, and a reinstall restores a broken one without touching content.
//!
//! `cargo run --example wp_core_zip_check` — network tier (wordpress.org, api.wordpress.org).
//!
//! Why it exists: WP-CLI's default `core download` fetched the `.tar.gz`, which PHP's PharData
//! extracted with 40 names cut at 100 characters — sites were created missing core classes, on
//! the owner's own machine too. What this proves, through `core_zip_url` + `core_download_args`:
//! the default build and a localized one arrive with `WithRequestAuthenticationInterface.php`
//! whole, no dot-ended names, and `wp core verify-checksums` passing; `core_reinstall` puts a
//! deleted core file back while a user plugin and a theme edit survive, and checksums pass again.
//!
//! Fixture-owned: everything under a temp directory this check creates and removes. No database,
//! no services, no site rows — `verify-checksums` does not load WordPress.

#[path = "common/mod.rs"]
mod common;

use common::Check;
use rexenv_lib::core::{binaries, wordpress};
use std::path::Path;

const LONG: &str = "wp-includes/php-ai-client/src/Providers/Http/Contracts/WithRequestAuthenticationInterface.php";

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let mut check = Check::new("wp_core_zip_check");
    let plat = rexenv_lib::platform::current();
    let php = binaries::resolve(&*plat, "php", binaries::PHP_VERSION).await.expect("php");
    let wp = binaries::resolve_file(&*plat, "wp-cli", binaries::WP_CLI_VERSION).await.expect("wp-cli");
    let root = std::env::temp_dir().join(format!("rexenv-wp-core-zip-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    // The default build.
    let site = root.join("site");
    download(&mut check, &php, &wp, "", &site);
    complete(&mut check, "default build", &site);
    checksums(&mut check, "default build", &php, &wp, &site, None);

    // A localized build.
    let de = root.join("de");
    download(&mut check, &php, &wp, "de_DE", &de);
    complete(&mut check, "de_DE build", &de);
    let version_php = std::fs::read_to_string(de.join("wp-includes/version.php")).unwrap_or_default();
    check.is("the de_DE build is localized", version_php.contains("$wp_local_package = 'de_DE'"), "no wp_local_package");
    checksums(&mut check, "de_DE build", &php, &wp, &de, Some("de_DE"));

    // A reinstall over a damaged core, with content that must survive.
    std::fs::remove_file(site.join(LONG)).ok();
    std::fs::create_dir_all(site.join("wp-content/plugins/mine")).unwrap();
    std::fs::write(site.join("wp-content/plugins/mine/mine.php"), "<?php // mine").unwrap();
    let style = site.join("wp-content/themes/twentytwentyfive/style.css");
    let edited = format!("{}\n/* edited by the user */\n", std::fs::read_to_string(&style).unwrap_or_default());
    std::fs::write(&style, &edited).unwrap();
    let re = wordpress::core_reinstall(&php, &wp, &site);
    check.is("core_reinstall succeeds", re.is_ok(), &format!("{re:?}"));
    check.is("core_reinstall restored the deleted core file", site.join(LONG).is_file(), "still missing");
    check.is("a user plugin survived the reinstall", site.join("wp-content/plugins/mine/mine.php").is_file(), "gone");
    check.is("a theme edit survived the reinstall", std::fs::read_to_string(&style).unwrap_or_default() == edited, "overwritten");
    checksums(&mut check, "after the reinstall", &php, &wp, &site, None);

    let _ = std::fs::remove_dir_all(&root);
    check.verdict()
}

fn download(check: &mut Check, php: &Path, wp: &Path, locale: &str, dir: &Path) {
    let url = wordpress::core_zip_url(locale, None, false).expect("url");
    let mut args = wordpress::core_download_args(&url);
    args.push(format!("--path={}", dir.display()));
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = wordpress::wp_cli(php, wp, &refs, None);
    check.is(
        &format!("core download {url}"),
        out.as_ref().is_ok_and(|o| o.status.success()),
        &out.map(|o| String::from_utf8_lossy(&o.stderr).into_owned()).unwrap_or_else(|e| e.to_string()),
    );
}

fn complete(check: &mut Check, what: &str, dir: &Path) {
    check.is(&format!("{what}: the 103-character core file is whole"), dir.join(LONG).is_file(), "missing");
    let mut dotted = Vec::new();
    walk(dir, &mut |p| {
        if p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with('.')) {
            dotted.push(p.display().to_string());
        }
    });
    check.is(&format!("{what}: no file name was cut to a trailing dot"), dotted.is_empty(), &format!("{dotted:?}"));
}

fn checksums(check: &mut Check, what: &str, php: &Path, wp: &Path, dir: &Path, locale: Option<&str>) {
    let path = format!("--path={}", dir.display());
    let loc = locale.map(|l| format!("--locale={l}"));
    let mut args = vec!["core", "verify-checksums", path.as_str()];
    if let Some(l) = loc.as_deref() {
        args.push(l);
    }
    let out = wordpress::wp_cli(php, wp, &args, None);
    let text = out.as_ref().map(|o| format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))).unwrap_or_default();
    check.is(&format!("{what}: wp core verify-checksums passes"), out.as_ref().is_ok_and(|o| o.status.success()), text.trim());
}

fn walk(dir: &Path, f: &mut dyn FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, f);
        } else {
            f(&p);
        }
    }
}

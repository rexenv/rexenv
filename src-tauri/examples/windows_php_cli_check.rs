//! W4 on a real Windows machine (ledger #603): the PHP CLI — WP-CLI's interpreter — loads the
//! pool's extensions from the `php.ini` `core` writes beside `php.exe`, and WP-CLI runs through it.
//!
//! ```text
//! ssh dell@<host> .\windows_php_cli_check.exe
//! ```
//!
//! What it proves there: `binaries::resolve_program("php")` returns `php.exe` inside the official
//! tree AND leaves a `php.ini` beside it; `php.exe -m` run with NO flags (as WP-CLI, Composer and a
//! terminal run it) lists every extension the platform's model names; the file is rewritten when
//! it differs; WP-CLI runs through that PHP (`wp --info`) and `wp core download` reaches
//! wordpress.org over HTTPS with WP-CLI's own CA bundle — no CA configured in PHP. Then, for
//! EVERY pinned PHP version, a bare `php -m` and the group's preflight load exactly the
//! extensions that version ships — the 8.4 IMAP report (8 Oct 2026) is what one 8.3-only run
//! missed.
//!
//! Fixture-owned: a sandboxed platform under `%TEMP%\rexenv-php-cli-check` (the WordPress download
//! lands there; removed at the end), the real binary cache (the documented sandbox exception —
//! the ini it writes there is the product's own file). `demo` tier: needs Windows and the network.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_php_cli_check: skipped — a Windows live check (ledger #603)");
}

#[cfg(target_os = "windows")]
#[tokio::main]
async fn main() -> std::process::ExitCode {
    windows::main().await
}

#[cfg(target_os = "windows")]
mod windows {
    use super::common::{self, Check};
    use rexenv_lib::core::{binaries, php, php_cgi, wordpress};
    use rexenv_lib::platform::traits::PoolModel;
    use std::process::{Command, ExitCode};

    pub async fn main() -> ExitCode {
        let mut check = Check::new("windows_php_cli_check");
        let root = std::env::temp_dir().join("rexenv-php-cli-check");
        let _ = std::fs::remove_dir_all(&root);
        let plat = common::sandbox_platform_at(root.clone());
        let PoolModel::CgiGroup(group) = plat.supervisor().php_pool_model() else {
            check.is("this platform names the php-cgi group model", false, "Fpm");
            return check.verdict();
        };

        let php = binaries::resolve_program(&*plat, "php", binaries::pins().php).await;
        check.is("resolve_program finds php.exe inside the tree", php.as_ref().is_ok_and(|p| p.ends_with("php.exe")), &format!("{php:?}"));
        let Ok(php) = php else { return check.verdict() };
        let dir = php.parent().unwrap().to_path_buf();
        let ini = dir.join("php.ini");
        let want = php_cgi::render_cli_ini(&group, binaries::pins().php, &dir);
        check.is("the tree carries rexenv's php.ini", std::fs::read_to_string(&ini).ok().as_deref() == Some(want.as_str()), &ini.display().to_string());

        // Exactly how WP-CLI, Composer and a terminal run it: no -n, no -c, no -d.
        let out = Command::new(&php).arg("-m").output().expect("run php -m");
        let listed: Vec<String> = String::from_utf8_lossy(&out.stdout).lines().map(|l| l.trim().to_ascii_lowercase()).collect();
        let expected = group.modules(binaries::pins().php);
        let missing: Vec<&str> = expected.iter().copied().filter(|e| !listed.contains(&e.to_string()))
            .chain(group.zend_extensions.iter().copied().filter(|_| !listed.contains(&"zend opcache".to_string())))
            .collect();
        check.is(&format!("a bare `php -m` lists all {} extensions", expected.len() + group.zend_extensions.len()), out.status.success() && missing.is_empty(), &format!("missing {missing:?}; stderr {}", String::from_utf8_lossy(&out.stderr)));
        let loaded = Command::new(&php).args(["-r", "echo php_ini_loaded_file();"]).output().expect("run php -r");
        check.is("the ini PHP loaded is the one beside php.exe", String::from_utf8_lossy(&loaded.stdout).trim().eq_ignore_ascii_case(&ini.display().to_string()), &String::from_utf8_lossy(&loaded.stdout));

        // A stale file is corrected on the next resolve.
        std::fs::write(&ini, "; stale\n").unwrap();
        let again = binaries::resolve_program(&*plat, "php", binaries::pins().php).await;
        check.is("a php.ini that differs is rewritten on the next resolve", again.is_ok() && std::fs::read_to_string(&ini).ok().as_deref() == Some(want.as_str()), "not rewritten");

        // WP-CLI through that PHP, and a real HTTPS download with no CA configured in PHP.
        let wp = binaries::resolve_file(&*plat, "wp-cli", binaries::pins().wp_cli).await.expect("wp-cli phar");
        let info = wordpress::wp_cli(&php, &wp, &["--info"], None);
        let info_text = info.as_ref().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
        check.is("wp --info runs through the tree's PHP", info.as_ref().is_ok_and(|o| o.status.success()) && info_text.contains("PHP version"), &format!("{info:?}"));
        let docroot = root.join("wp");
        std::fs::create_dir_all(&docroot).unwrap();
        let path = format!("--path={}", docroot.display());
        let dl = wordpress::wp_cli(&php, &wp, &["core", "download", &path], None);
        let dl_err = dl.as_ref().map(|o| String::from_utf8_lossy(&o.stderr).into_owned()).unwrap_or_default();
        check.is("wp core download reaches wordpress.org over HTTPS", dl.as_ref().is_ok_and(|o| o.status.success()) && docroot.join("wp-load.php").exists(), &dl_err);

        // Every pinned version: the CLI's php.ini and the group's preflight, startup-warning free.
        for &version in binaries::pins().php_versions {
            let Ok(php) = binaries::resolve_program(&*plat, "php", version).await else {
                check.is(&format!("PHP {version} resolves"), false, "resolve failed");
                continue;
            };
            let dir = php.parent().unwrap().to_path_buf();
            let out = Command::new(&php).arg("-m").output().expect("run php -m");
            let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            let listed: Vec<String> = stdout.lines().map(|l| l.trim().to_ascii_lowercase()).collect();
            let expected = group.modules(version);
            let missing: Vec<&str> = expected.iter().copied().filter(|e| !listed.contains(&e.to_string())).collect();
            check.is(
                &format!("PHP {version}: a bare `php -m` loads its {} extensions with no startup warning", expected.len()),
                out.status.success() && missing.is_empty() && !stdout.contains("PHP Startup") && !stderr.contains("PHP Startup"),
                &format!("missing {missing:?}; stderr {stderr}"),
            );
            let minor = php::minor_of(version);
            let ini = php_cgi::write_ini(&*plat, &group, version, &dir, &format!("{minor}.check"), None, &[]);
            let pre = ini.and_then(|ini| php_cgi::preflight(&*plat, &group, version, &dir, &ini));
            check.is(&format!("PHP {version}: the group's preflight passes"), pre.is_ok(), &format!("{pre:?}"));
        }

        let _ = std::fs::remove_dir_all(&root);
        check.verdict()
    }
}

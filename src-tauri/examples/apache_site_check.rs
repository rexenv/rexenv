//! Manual check: Apache httpd as a per-site override backend (TODO "Deferred
//! services"). Run: `cargo run --example apache_site_check`
//!
//! Proves the whole chain on the REAL binary cache, with throwaway processes
//! only (the running stack is untouched):
//!   1. `resolve_bundle("httpd")` — 4 ghcr bottles merge, relink, re-sign;
//!      audit `bin/httpd`, the dylibs, a module, and the mime map.
//!   2. A throwaway php-fpm pool on :9799 (the bundled static build).
//!   3. `apache::start` on the site's deterministic override port serving a
//!      temp docroot, `.php` handed to the pool via mod_proxy_fcgi.
//!   4. HTTP checks: PHP executes; SetEnv env var arrives per-request;
//!      pretty-URL falls back to /index.php (front controller); static css
//!      gets a real Content-Type (bundled mime.types); an `.htaccess`
//!      RewriteRule fires (AllowOverride All — the point of Apache).

use rexenv_lib::core::services::RewriteMode;
use rexenv_lib::core::{apache, binaries};
use rexenv_lib::platform;
use std::process::Command;
use std::thread;
use std::time::Duration;

const DOMAIN: &str = "apcheck.rex";
const FPM_PORT: u16 = 9799;

fn http(url: &str, args: &[&str]) -> String {
    let out = Command::new("curl")
        .args(["-s", "-i", "--max-time", "5"])
        .args(args)
        .arg(url)
        .output()
        .expect("curl");
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn load_commands_clean(path: &std::path::Path) -> bool {
    let out = Command::new("otool").arg("-L").arg(path).output().expect("otool");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .skip(1)
        .filter_map(|l| l.split_whitespace().next())
        .all(|d| {
            d.starts_with("/usr/lib/") || d.starts_with("/System/") || d.starts_with("@loader_path/")
        })
}

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let mut ok = true;

    println!("=== resolve_bundle(httpd {}) ===", binaries::HTTPD_VERSION);
    let basedir = binaries::resolve_bundle(&*plat, "httpd", binaries::HTTPD_VERSION)
        .await
        .expect("resolve httpd bundle");
    println!("  published at {}", basedir.display());

    println!("\n=== relink + content audit ===");
    for rel in [
        "bin/httpd",
        "lib/libapr-1.0.dylib",
        "lib/libaprutil-1.0.dylib",
        "lib/libpcre2-8.0.dylib",
        "lib/httpd/modules/mod_proxy_fcgi.so",
        "lib/httpd/modules/mod_rewrite.so",
    ] {
        let path = basedir.join(rel);
        let exists = path.is_file();
        let clean = exists && load_commands_clean(&path);
        println!("  {rel:<40} exists={exists} loads-clean={clean}");
        ok &= exists && clean;
    }
    let mime = basedir.join(".bottle/etc/httpd/mime.types").is_file();
    println!("  .bottle/etc/httpd/mime.types             present={mime}");
    ok &= mime;

    // Temp docroot with a probe script, a static file, and an .htaccess rule.
    let docroot = std::env::temp_dir().join("rexenv-apache-site-check");
    let _ = std::fs::remove_dir_all(&docroot);
    std::fs::create_dir_all(&docroot).unwrap();
    std::fs::write(
        docroot.join("index.php"),
        "<?php echo 'APX|' . ($_SERVER['APCHECK_ENV'] ?? 'none') . '|' . $_SERVER['REQUEST_URI'];",
    )
    .unwrap();
    std::fs::write(docroot.join("style.css"), "body{}\n").unwrap();
    std::fs::write(
        docroot.join(".htaccess"),
        "RewriteEngine On\nRewriteRule ^ht-check$ /index.php [R=302,L]\n",
    )
    .unwrap();

    println!("\n=== throwaway php-fpm pool on :{FPM_PORT} ===");
    let fpm_bin = binaries::resolve(&*plat, "php-fpm", binaries::PHP_VERSION)
        .await
        .expect("php-fpm");
    let fpm_conf = docroot.join("fpm.ini");
    std::fs::write(
        &fpm_conf,
        format!(
            "[global]\ndaemonize = no\nerror_log = {log}\n[www]\nlisten = 127.0.0.1:{FPM_PORT}\npm = static\npm.max_children = 2\n",
            log = docroot.join("fpm.log").display()
        ),
    )
    .unwrap();
    let mut fpm = Command::new(&fpm_bin)
        .args(["-y", &fpm_conf.display().to_string(), "-F"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn php-fpm");

    println!("\n=== apache::start on the override port ===");
    let port = apache::site_port(DOMAIN);
    let env = vec![("APCHECK_ENV".to_string(), "from-apache".to_string())];
    let conf = apache::write_config(
        &*plat,
        &basedir,
        DOMAIN,
        &docroot,
        port,
        FPM_PORT,
        RewriteMode::Single,
        &env,
    )
    .expect("write conf");
    // Config sanity gate first — a broken conf should fail HERE, loudly.
    let t = Command::new(apache::httpd_bin(&basedir))
        .args(["-f", &conf.display().to_string(), "-t"])
        .output()
        .expect("httpd -t");
    println!(
        "  httpd -t → {} {}",
        t.status.success(),
        String::from_utf8_lossy(&t.stderr).trim()
    );
    ok &= t.status.success();

    let mut httpd = apache::start(&*plat, &basedir, DOMAIN, &conf).expect("start apache");
    for _ in 0..40 {
        if apache::running(port) {
            break;
        }
        thread::sleep(Duration::from_millis(250));
    }
    println!("  pid {} · listening on :{port} = {}", httpd.id(), apache::running(port));
    ok &= apache::running(port);

    if apache::running(port) {
        let base = format!("http://127.0.0.1:{port}");
        // PHP executes through mod_proxy_fcgi; SetEnv arrives per-request.
        let root = http(&base, &[]);
        let php_ok = root.contains("APX|from-apache|/");
        // Pretty URL → front controller (FallbackResource).
        let pretty = http(&format!("{base}/pretty/permalink"), &[]);
        let pretty_ok = pretty.contains("APX|from-apache|/pretty/permalink");
        // Static file gets a real mime type from the bundled map.
        let css = http(&format!("{base}/style.css"), &[]);
        let css_ok = css.to_lowercase().contains("content-type: text/css");
        // .htaccess rewrite fires (302), proving AllowOverride All.
        let ht = http(&format!("{base}/ht-check"), &[]);
        let ht_ok = ht.starts_with("HTTP/1.1 302");
        println!("  php-via-fpm={php_ok} · fallback-routing={pretty_ok} · css-mime={css_ok} · htaccess-302={ht_ok}");
        ok &= php_ok && pretty_ok && css_ok && ht_ok;
    }

    // Cleanup: children + docroot (the conf under app-data config dir stays —
    // same lifecycle as FrankenPHP override configs).
    let _ = apache::stop(&*plat, httpd.id());
    let _ = httpd.wait();
    let _ = fpm.kill();
    let _ = fpm.wait();
    let _ = std::fs::remove_dir_all(&docroot);

    if ok {
        println!("\nOK — httpd bundle serves PHP via the fpm pool, routes, mimes, and honors .htaccess.");
    } else {
        eprintln!("\nFAILED — see above.");
        std::process::exit(1);
    }
}

//! Manual check: Apache httpd as a per-site override backend (TODO "Deferred
//! services"). Run: `cargo run --example apache_site_check`
//!
//! Proves the whole chain on the REAL binary cache, with throwaway processes
//! only (the running stack is untouched):
//!   1. `resolve_bundle("httpd")` — 4 ghcr bottles merge, relink, re-sign;
//!      audit `bin/httpd`, the dylibs, a module, and the mime map.
//!   2. A throwaway php-fpm pool on :9799 (the bundled static build), WAITED
//!      FOR — see the readiness loop's comment: not waiting is what made this
//!      example's failures read as "Apache cannot execute PHP".
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

mod common;
use common::Reaped;

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

const DOT_SECRET: &str = "REXENV_APACHE_DOT_9f2c";

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
    // #103's Apache leg. The guard is in the generated conf
    // (`RewriteRule "(^|/)\\.(?!well-known(/|$))" - [R=404,L]`) and unit-tested as a
    // string; this is the only place it is exercised over the wire on this backend.
    // The secret is checked for ABSENCE in the body, not just a 404 code: a 404
    // page that happens to echo the request would still leak it.
    std::fs::create_dir_all(docroot.join(".git")).unwrap();
    std::fs::create_dir_all(docroot.join(".hidden")).unwrap();
    std::fs::create_dir_all(docroot.join(".well-known")).unwrap();
    std::fs::write(docroot.join(".env"), format!("APP_KEY={DOT_SECRET}\n")).unwrap();
    std::fs::write(docroot.join(".git/config"), "[core]\n").unwrap();
    std::fs::write(docroot.join(".hidden/x.php"), "<?php echo 'DOTPHP-RAN';").unwrap();
    std::fs::write(docroot.join(".well-known/probe"), "well-known-ok").unwrap();
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
    // Drop-guarded: the HTTP checks below assert, and a leaked pool's workers
    // keep :FPM_PORT (see examples/common).
    let mut fpm = Reaped::new(
        Command::new(&fpm_bin)
            .args(["-y", &fpm_conf.display().to_string(), "-F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn php-fpm"),
        FPM_PORT,
        "php-fpm",
    );
    // WAIT FOR THE POOL. Nothing did, and that is this example's documented
    // transient (docs/TODO.md "live-check transients"): Apache binds in
    // milliseconds and its own readiness loop is satisfied at once, while a
    // cold php-fpm under CPU contention has not bound :FPM_PORT yet. The first
    // PHP request then reaches mod_proxy_fcgi with nothing behind it, and the
    // example reported `php-via-fpm=false` — which reads as "Apache cannot
    // execute PHP" and sent two investigations at the wrong subject. The
    // captured 20 Aug 2026 failure says so exactly: php-via-fpm=false,
    // fallback-routing=false (both need PHP), css-mime=true, htaccess-302=true
    // (neither does). Apache was fine; there was no pool.
    let mut pool_up = false;
    for _ in 0..80 {
        if apache::running(FPM_PORT) {
            pool_up = true;
            break;
        }
        thread::sleep(Duration::from_millis(250));
    }
    // And say so HERE rather than letting it surface as a PHP failure below.
    // A check that misreports its own precondition is worse than one that
    // fails: it is a signpost pointing away from the problem.
    if !pool_up {
        eprintln!("\nphp-fpm never bound :{FPM_PORT} in 20s — the PHP legs below would blame Apache.");
        eprintln!("php-fpm's own log ({}):", docroot.join("fpm.log").display());
        eprintln!("{}", std::fs::read_to_string(docroot.join("fpm.log")).unwrap_or_default());
        fpm.reap();
        std::process::exit(1);
    }
    println!("  pool listening on :{FPM_PORT} = {pool_up}");

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

    let mut httpd = Reaped::new(
        apache::start(&*plat, &basedir, DOMAIN, &conf).expect("start apache"),
        port,
        "httpd",
    );
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

        // #103, Apache backend — the same four probes the nginx leg runs.
        let mut dot_ok = true;
        for (path, what) in [
            ("/.env", "the .env"),
            ("/.git/config", "the .git config"),
            ("/.hidden/x.php", "a dot-dir .php"),
        ] {
            let r = http(&format!("{base}{path}"), &[]);
            let blocked = r.starts_with("HTTP/1.1 404");
            let leaked = r.contains(DOT_SECRET) || r.contains("DOTPHP-RAN");
            if !blocked || leaked {
                println!("  ✗ {what} at {path}: 404={blocked} leaked={leaked}");
            }
            dot_ok &= blocked && !leaked;
        }
        // The exemption has to stay REAL, or "everything 404s" would pass the
        // three above while breaking ACME.
        let wk = http(&format!("{base}/.well-known/probe"), &[]);
        let wk_ok = wk.starts_with("HTTP/1.1 200") && wk.contains("well-known-ok");
        println!("  dotfiles-404={dot_ok} · well-known-still-200={wk_ok}");
        ok &= dot_ok && wk_ok;
    }

    // Cleanup: children + docroot (the conf under app-data config dir stays —
    // same lifecycle as FrankenPHP override configs).
    // `apache::stop` first (the production stop path is part of what this
    // example exercises), then reap — explicit because the `exit(1)` below
    // skips destructors. Both are idempotent; the Drop guards cover panics.
    let _ = apache::stop(&*plat, httpd.id());
    httpd.reap();
    fpm.reap();
    let _ = std::fs::remove_dir_all(&docroot);

    if ok {
        println!("\nOK — httpd bundle serves PHP via the fpm pool, routes, mimes, and honors .htaccess.");
    } else {
        eprintln!("\nFAILED — see above.");
        std::process::exit(1);
    }
}

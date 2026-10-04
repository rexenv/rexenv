//! Live check: OpenLiteSpeed as a per-site override backend, end to end, on the REAL binary
//! cache and a SANDBOXED app-data root. Run: `cargo run --example openlitespeed_site_check`
//!
//! Proves, over the wire (macOS and Linux — OpenLiteSpeed has no Windows build):
//!   1. `resolve_dir("openlitespeed")` — rexenv/runtimes' tarball verified, extracted flat
//!      (`openlitespeed`, `mime.properties`), its licence archive beside it.
//!   2. A throwaway php-fpm pool, WAITED for (`common::await_listening`).
//!   3. The config core generates (the same `generate_config` the ServiceManager renders)
//!      passes OpenLiteSpeed's own `-t` without an error.
//!   4. `openlitespeed::start`, then: PHP through the pool; a per-site env var with a quote,
//!      `%1` and a backslash arrives intact; `X-Forwarded-Proto: https` → `HTTPS=on`; a pretty
//!      URL reaches the front controller with its URI; CSS gets `text/css`; an `.htaccess`
//!      rule fires; dotfiles 404 while `/.well-known/` serves; LSCache answers miss then hit.
//!   5. OWNERSHIP: `owned_master(port, app-data)` finds the server although OpenLiteSpeed
//!      rewrites its argv[0] — the doubled marker argument is what makes that work on macOS.
//!   6. Runtime files land under the site's `run/` (LSWS_TMP_DIR), nothing new under
//!      `/tmp/lshttpd`, and no remote fetch is attempted.
//!   7. The production stop path frees the port.

use rexenv_lib::core::services::RewriteMode;
use rexenv_lib::core::{binaries, openlitespeed};
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime};

mod common;
use common::Reaped;

const DOMAIN: &str = "olscheck.rex";
const PORT: u16 = openlitespeed::OPENLITESPEED_BASE_PORT + 97;
const FPM_PORT: u16 = 9797;
const DOT_SECRET: &str = "REXENV_OLS_DOT_7a1e";
const ENV_VALUE: &str = "it's 50%1 C:\\path";

fn http(url: &str, args: &[&str]) -> String {
    let out = Command::new("curl")
        .args(["-s", "-i", "--max-time", "5"])
        .args(args)
        .arg(url)
        .output()
        .expect("curl");
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn listening(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(200)).is_ok()
}

fn header(resp: &str, name: &str) -> Option<String> {
    resp.lines()
        .take_while(|l| !l.trim().is_empty())
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_string())
        })
}

#[tokio::main]
async fn main() {
    common::require_ports_free(&[(PORT, "the check's OpenLiteSpeed"), (FPM_PORT, "the check's php-fpm")]);
    let (plat, _sandbox) = common::sandbox("openlitespeed_site_check");
    let mut ok = true;
    let mut check = |what: &str, pass: bool| {
        println!("  {} {what}", if pass { "✓" } else { "✗" });
        ok &= pass;
    };

    println!("=== resolve_dir(openlitespeed {}) ===", binaries::pins().openlitespeed);
    let basedir = binaries::resolve_dir(&*plat, "openlitespeed", binaries::pins().openlitespeed)
        .await
        .expect("resolve openlitespeed");
    println!("  {}", basedir.display());
    check("binary at the tree root", openlitespeed::server_bin(&basedir).is_file());
    check("mime.properties beside it", openlitespeed::mime_path(&basedir).is_file());
    check(
        "licence texts staged (GPL-3.0 travels with the bytes)",
        basedir.join("licenses").read_dir().map(|d| d.count() > 5).unwrap_or(false),
    );

    // Fixture docroot — a temp dir this check creates and removes.
    let docroot = std::env::temp_dir().join("rexenv-openlitespeed-site-check");
    let _ = std::fs::remove_dir_all(&docroot);
    std::fs::create_dir_all(docroot.join(".git")).unwrap();
    std::fs::create_dir_all(docroot.join(".well-known")).unwrap();
    std::fs::write(
        docroot.join("index.php"),
        "<?php header('X-LiteSpeed-Cache-Control: public,max-age=120');\n\
         echo 'OLS|', getenv('OLSCHECK_ENV') ?: 'none', '|', $_SERVER['HTTPS'] ?? 'off', '|', \
         $_SERVER['REQUEST_URI'], '|', microtime(true);",
    )
    .unwrap();
    std::fs::write(docroot.join("style.css"), "body{}\n").unwrap();
    std::fs::write(docroot.join(".env"), format!("APP_KEY={DOT_SECRET}\n")).unwrap();
    std::fs::write(docroot.join(".git/config"), "[core]\n").unwrap();
    std::fs::write(docroot.join(".well-known/probe"), "well-known-ok").unwrap();
    // WordPress's own block after a plugin-style redirect: with a root .htaccess the site
    // routes itself (the vhost's fallback steps aside), exactly as on a LiteSpeed host.
    std::fs::write(
        docroot.join(".htaccess"),
        "RewriteEngine On\nRewriteRule ^ht-check$ /index.php [R=302,L]\n\
         RewriteBase /\nRewriteRule ^index\\.php$ - [L]\n\
         RewriteCond %{REQUEST_FILENAME} !-f\nRewriteCond %{REQUEST_FILENAME} !-d\n\
         RewriteRule . /index.php [L]\n",
    )
    .unwrap();

    println!("\n=== throwaway php-fpm pool on :{FPM_PORT} ===");
    let fpm_bin = binaries::resolve(&*plat, "php-fpm", binaries::pins().php).await.expect("php-fpm");
    let fpm_conf = docroot.join("fpm.ini");
    std::fs::write(
        &fpm_conf,
        format!(
            "[global]\ndaemonize = no\nerror_log = {log}\n[www]\nlisten = 127.0.0.1:{FPM_PORT}\n\
             pm = static\npm.max_children = 2\nclear_env = no\n",
            log = docroot.join("fpm.log").display()
        ),
    )
    .unwrap();
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
    common::await_listening(FPM_PORT, "php-fpm", Some(&docroot.join("fpm.log")));

    println!("\n=== generated config + OpenLiteSpeed's own config test ===");
    let account = plat.supervisor().service_account().expect("a uid on macOS/Linux");
    let server_root = openlitespeed::server_root(&*plat, DOMAIN).unwrap();
    let log_dir = plat.paths().log_dir().unwrap();
    let env = vec![("OLSCHECK_ENV".to_string(), ENV_VALUE.to_string())];
    let content = openlitespeed::generate_config(&openlitespeed::ConfigInput {
        basedir: &basedir,
        server_root: &server_root,
        docroot: &docroot,
        log_dir: &log_dir,
        domain: DOMAIN,
        port: PORT,
        fpm_port: FPM_PORT,
        mode: RewriteMode::Single,
        env: &env,
        account: &account,
    });
    openlitespeed::write_config(&*plat, DOMAIN, &content).expect("write config");
    let t = Command::new(openlitespeed::server_bin(&basedir))
        .arg("-t")
        .env("LSWS_HOME", format!("{}/", server_root.display()))
        .env("LSWS_TMP_DIR", server_root.join("run"))
        .output()
        .expect("openlitespeed -t");
    let code = t.status.code().unwrap_or(-1);
    // 0 clean, 1 warnings (macOS's `staff` is gid 20, under OLS's 100), 2 errors.
    println!("  -t exit {code}");
    check("config test reports no ERROR", code == 0 || code == 1);

    println!("\n=== openlitespeed::start on :{PORT} ===");
    let started = SystemTime::now();
    let mut server = Reaped::new(openlitespeed::start(&*plat, &basedir, DOMAIN, &[]).expect("start"), PORT, "openlitespeed");
    common::await_listening(PORT, "openlitespeed", Some(&openlitespeed::error_log_path(&*plat, DOMAIN).unwrap()));
    // The cache manager comes up on the first timer tick; requests before it are never cached.
    thread::sleep(Duration::from_secs(3));

    let base = format!("http://127.0.0.1:{PORT}");
    let root = http(&format!("{base}/"), &["-H", "X-Forwarded-Proto: https"]);
    check(&format!("PHP via the pool, env intact ({ENV_VALUE}), HTTPS=on"), root.contains(&format!("OLS|{ENV_VALUE}|on|/|")));
    // A URL of its own: `/` is in LSCache by now, with the HTTPS=on answer.
    let plain = http(&format!("{base}/?plain"), &[]);
    check("no X-Forwarded-Proto → no HTTPS", plain.contains(&format!("OLS|{ENV_VALUE}|off|/|")) || plain.contains("|off|"));
    let pretty = http(&format!("{base}/pretty/permalink"), &[]);
    check("pretty URL → front controller with its URI", pretty.contains("|/pretty/permalink|"));
    let css = http(&format!("{base}/style.css"), &[]);
    check("css is text/css", header(&css, "content-type").is_some_and(|v| v.starts_with("text/css")));
    let ht = http(&format!("{base}/ht-check"), &[]);
    check(".htaccess redirect sees its own path (302)", ht.starts_with("HTTP/1.1 302"));
    for path in ["/.env", "/.git/config"] {
        let r = http(&format!("{base}{path}"), &[]);
        check(&format!("{path} → 404, nothing leaked"), r.starts_with("HTTP/1.1 404") && !r.contains(DOT_SECRET));
    }
    let wk = http(&format!("{base}/.well-known/probe"), &[]);
    check("/.well-known/ still serves", wk.starts_with("HTTP/1.1 200") && wk.contains("well-known-ok"));
    // No .htaccess at all: the vhost's own front controller takes over.
    std::fs::remove_file(docroot.join(".htaccess")).unwrap();
    thread::sleep(Duration::from_millis(1100));
    let bare = http(&format!("{base}/no-htaccess/route"), &[]);
    check("without .htaccess the vhost fallback routes", bare.contains("|/no-htaccess/route|"));
    let key = format!("{base}/?k={}", std::process::id());
    let first = header(&http(&key, &[]), "x-litespeed-cache");
    let second = header(&http(&key, &[]), "x-litespeed-cache");
    check(&format!("LSCache miss → hit ({first:?} → {second:?})"), first.as_deref() == Some("miss") && second.as_deref() == Some("hit"));

    println!("\n=== ownership, runtime files, phone-home ===");
    let marker = plat.paths().app_data_dir().unwrap().display().to_string();
    let owner = plat.supervisor().owned_master(PORT, &marker);
    check(&format!("owned_master finds pid {} through the rewritten title ({owner:?})", server.id()), owner == Some(server.id()));
    check("pid file under the site's run/ (LSWS_TMP_DIR)", server_root.join("run/lshttpd.pid").is_file());
    let tmp_untouched = std::fs::metadata("/tmp/lshttpd")
        .and_then(|m| m.modified())
        .map(|m| m < started)
        .unwrap_or(true);
    check("nothing new under /tmp/lshttpd", tmp_untouched);
    let errlog = std::fs::read_to_string(openlitespeed::error_log_path(&*plat, DOMAIN).unwrap()).unwrap_or_default();
    check("no remote fetch in the error log", !errlog.contains("HttpFetch") && !errlog.contains("quic.cloud"));
    check(
        "no update/quic download left behind",
        !server_root.join("autoupdate/release").exists() && !server_root.join("tmp/download-quic-cloud-ips").exists(),
    );

    println!("\n=== production stop ===");
    let _ = plat.supervisor().stop(server.id());
    for _ in 0..40 {
        if !listening(PORT) {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    check("port released", !listening(PORT));
    server.reap();
    fpm.reap();
    let _ = std::fs::remove_dir_all(&docroot);

    if ok {
        println!("\nOK — OpenLiteSpeed serves PHP via the pool, carries env/HTTPS, routes, caches, honours .htaccess, and is ours by cmdline.");
    } else {
        eprintln!("\nFAILED — see above.");
        std::process::exit(1);
    }
}

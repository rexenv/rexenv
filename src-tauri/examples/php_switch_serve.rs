//! End-to-end check for switching a site's PHP version (Phase 2 task 1.4).
//! Run: `cargo run --example php_switch_serve`
//!
//! Provisions one Blank-PHP site on 8.1, serves it (Caddy→nginx→fpm), curls it
//! (reports 8.1), then SWITCHES it to 8.3 the way the IPC command does:
//!   sites::set_php_version  →  ensure the 8.3 pool  →  rebuild configs  →  reload nginx
//! and curls again (now reports 8.3). Proves it's a config change + reload, NOT a
//! rebuild: the docroot (incl. a sentinel file dropped before the switch) and the
//! TLS cert are untouched (same cert mtime). Cleans everything up at the end.

use rexenv_lib::core::php::PhpFpmPools;
use rexenv_lib::core::{binaries, proxy, services, sites, ssl};
use rexenv_lib::platform;
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime};

const NGINX_PORT: u16 = services::NGINX_HTTP_PORT; // 18088
const CADDY_HTTP: u16 = 8080;
const CADDY_HTTPS: u16 = 8443;
const DOMAIN: &str = "switch.test";

fn fetch_version(ca_pem: &str) -> (String, String) {
    let code = Command::new("curl")
        .args([
            "-s", "--resolve", &format!("{DOMAIN}:{CADDY_HTTPS}:127.0.0.1"),
            "--cacert", ca_pem, "-o", "/dev/null", "-w", "%{http_code}",
            &format!("https://{DOMAIN}:{CADDY_HTTPS}/"),
        ])
        .output().expect("curl");
    let body = Command::new("curl")
        .args([
            "-s", "--resolve", &format!("{DOMAIN}:{CADDY_HTTPS}:127.0.0.1"),
            "--cacert", ca_pem, &format!("https://{DOMAIN}:{CADDY_HTTPS}/"),
        ])
        .output().expect("curl");
    let body = String::from_utf8_lossy(&body.stdout);
    // Extract the "PHP Version 8.x.y" phpinfo reports.
    let ver = body
        .split("PHP Version ")
        .nth(1)
        .and_then(|s| s.split('<').next())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    (String::from_utf8_lossy(&code.stdout).trim().to_string(), ver)
}

fn mtime(p: &Path) -> SystemTime {
    std::fs::metadata(p).and_then(|m| m.modified()).expect("mtime")
}

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let db_path = std::env::temp_dir().join("rexenv-1_4.db");
    let _ = std::fs::remove_file(&db_path);
    let conn = db::open(&db_path).expect("open db");
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).expect("ca");
    let ca_pem = ca.cert_path.display().to_string();

    // Provision on 8.1.
    let site = sites::provision(
        &conn, &*plat, &ca,
        NewSite {
            name: "Switch".into(), domain: DOMAIN.into(), site_type: SiteType::Php,
            php_version: "8.1".into(), web_server: WebServer::Nginx, path: String::new(), db_engine: rexenv_lib::state::models::SiteDbEngine::Mysql,
        },
    ).expect("provision");

    // Drop a sentinel into the docroot + record cert mtime — neither must change.
    let docroot = Path::new(&site.path);
    std::fs::write(docroot.join("SENTINEL"), b"keep me").unwrap();
    let cert = ssl::ensure_site_cert(plat.paths(), plat.permissions(), &ca, DOMAIN).unwrap();
    let cert_mtime_before = mtime(&cert.cert_path);

    // Serve: start the 8.1 pool + nginx + Caddy.
    let mut pools = PhpFpmPools::default();
    pools.start(&*plat, &["8.1".to_string()]).await.expect("pools");
    let cfg = sites::rebuild_configs(&conn, &*plat, &ca, NGINX_PORT, CADDY_HTTP, CADDY_HTTPS).unwrap();
    let nginx_bin = binaries::resolve(&*plat, "nginx", binaries::NGINX_VERSION).await.unwrap();
    let caddy_bin = binaries::resolve(&*plat, "caddy", binaries::CADDY_VERSION).await.unwrap();
    let mut nginx = services::start_nginx(&*plat, &nginx_bin, &cfg.nginx_conf, &cfg.nginx_prefix).unwrap();
    let mut caddy = proxy::start(&*plat, &caddy_bin, &cfg.caddyfile).unwrap();
    thread::sleep(Duration::from_millis(1500));

    let (code_before, ver_before) = fetch_version(&ca_pem);
    println!("before switch: http={code_before}  reports PHP {ver_before}");

    // ── SWITCH 8.1 → 8.3 (the same steps the IPC command runs) ──────────────
    sites::set_php_version(&conn, &site.id, "8.3").expect("set version");
    pools.ensure(&*plat, "8.3").await.expect("ensure 8.3 pool");
    let cfg = sites::rebuild_configs(&conn, &*plat, &ca, NGINX_PORT, CADDY_HTTP, CADDY_HTTPS).unwrap();
    thread::sleep(Duration::from_millis(800));
    services::reload_nginx(&*plat, &nginx_bin, &cfg.nginx_conf, &cfg.nginx_prefix).expect("reload");
    thread::sleep(Duration::from_millis(800));

    let (code_after, ver_after) = fetch_version(&ca_pem);
    println!("after  switch: http={code_after}  reports PHP {ver_after}");

    // No-rebuild proof: sentinel survived + cert untouched.
    let sentinel_kept = docroot.join("SENTINEL").exists();
    let cert_unchanged = mtime(&cert.cert_path) == cert_mtime_before;
    println!(
        "no-rebuild proof: docroot sentinel kept={sentinel_kept}  cert mtime unchanged={cert_unchanged}"
    );

    // Cleanup (explicit — process::exit skips Drop).
    let _ = proxy::stop(&*plat, caddy.id()); let _ = caddy.wait();
    let _ = services::stop(&*plat, nginx.id()); let _ = nginx.wait();
    pools.stop_all(&*plat);

    let ok = code_before == "200" && ver_before.starts_with("8.1")
        && code_after == "200" && ver_after.starts_with("8.3")
        && sentinel_kept && cert_unchanged;
    if ok {
        println!("\nOK — switched 8.1 → 8.3 via config reload, no docroot/cert rebuild.");
    } else {
        eprintln!("\nFAILED — see above.");
        std::process::exit(1);
    }
}

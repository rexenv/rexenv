//! End-to-end check for site → PHP-version mapping (Phase 2 task 1.3).
//! Run: `cargo run --example php_per_site_serve`
//!
//! Provisions two Blank-PHP sites pinned to DIFFERENT PHP versions (a.test → 8.1,
//! b.test → 8.3), starts a php-fpm pool per version, rebuilds the shared nginx +
//! Caddy configs (each server block's `fastcgi_pass` → its version's pool port),
//! starts nginx + Caddy on high ports, then curls each site through the full chain
//! (Caddy TLS → nginx by server_name → the right php-fpm pool) and asserts each
//! reports ITS pinned PHP version in phpinfo(). Cleans everything up at the end.

use rexenv_lib::core::php::{self, PhpFpmPools};
use rexenv_lib::core::{binaries, proxy, services, sites, ssl};
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::process::Command;
use std::thread;

mod common;
use std::time::Duration;

const NGINX_PORT: u16 = services::NGINX_HTTP_PORT; // 18088
const CADDY_HTTP: u16 = 8080;
const CADDY_HTTPS: u16 = 8443;

#[tokio::main]
async fn main() {
    // Sandboxed: every path the app derives (config dir, nginx PREFIX and
    // therefore nginx.pid, run/, certs) lands in a throwaway root, so this
    // example cannot touch the running stack. See examples/common.
    let (plat, _sandbox) = common::sandbox("php_per_site_serve");
    // (domain, minor) pairs — two different PHP versions.
    let sites_spec = [("a.test", "8.1"), ("b.test", "8.3")];
    let minors: Vec<String> = sites_spec.iter().map(|(_, v)| v.to_string()).collect();

    let db_path = std::env::temp_dir().join("rexenv-1_3.db");
    let _ = std::fs::remove_file(&db_path);
    let conn = db::open(&db_path).expect("open db");
    // Pin `sites_dir` into the sandbox: `sites::provision` reads the SETTING,
    // which falls back to the home directory, so without this the docroots land
    // in the user's real ~/rexenv/Sites (14 Aug 2026 sweep).
    common::pin_sites_dir(&conn, &*plat);
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).expect("ca");

    for (domain, minor) in sites_spec {
        sites::provision(
            &conn,
            &*plat,
            &ca,
            NewSite {
                name: domain.into(),
                domain: domain.into(),
                site_type: SiteType::Php, // drops a phpinfo() index.php
                php_version: minor.into(),
                web_server: WebServer::Nginx,
                path: String::new(),
            db_engine: rexenv_lib::state::models::SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
            git_migrate: true,
            git_build_assets: false,
            },
        )
        .expect("provision");
        println!("provisioned {domain} on PHP {minor}");
    }

    // One php-fpm pool per version.
    let mut pools = PhpFpmPools::default();
    pools.start(&*plat, &minors).await.expect("start pools");

    // Configs: each server block routes to its version's pool.
    let cfg = sites::rebuild_configs(&conn, &*plat, &ca, NGINX_PORT, CADDY_HTTP, CADDY_HTTPS)
        .expect("rebuild configs");

    let nginx_bin = binaries::resolve(&*plat, "nginx", binaries::NGINX_VERSION).await.unwrap();
    let caddy_bin = binaries::resolve(&*plat, "caddy", binaries::CADDY_VERSION).await.unwrap();
    services::test_nginx_config(&*plat, &nginx_bin, &cfg.nginx_conf, &cfg.nginx_prefix)
        .expect("nginx -t");
    let mut nginx =
        services::start_nginx(&*plat, &nginx_bin, &cfg.nginx_conf, &cfg.nginx_prefix).expect("nginx");
    let mut caddy = proxy::start(&*plat, &caddy_bin, &cfg.caddyfile).expect("caddy");
    thread::sleep(Duration::from_millis(1500));

    println!("\n=== fetch each site through Caddy→nginx→php-fpm ===");
    let mut all_ok = true;
    for (domain, minor) in sites_spec {
        let want = php::patch_for_minor(minor).unwrap(); // e.g. "8.1.34"
        let out = Command::new("curl")
            .args([
                "-s",
                "--resolve",
                &format!("{domain}:{CADDY_HTTPS}:127.0.0.1"),
                "--cacert",
                &ca.cert_path.display().to_string(),
                "-o",
                "/dev/null",
                "-w",
                "%{http_code}",
                &format!("https://{domain}:{CADDY_HTTPS}/"),
            ])
            .output()
            .expect("run curl");
        let code = String::from_utf8_lossy(&out.stdout).trim().to_string();

        // Re-fetch the body to read the reported PHP version from phpinfo().
        let body = Command::new("curl")
            .args([
                "-s",
                "--resolve",
                &format!("{domain}:{CADDY_HTTPS}:127.0.0.1"),
                "--cacert",
                &ca.cert_path.display().to_string(),
                &format!("https://{domain}:{CADDY_HTTPS}/"),
            ])
            .output()
            .expect("run curl");
        let body = String::from_utf8_lossy(&body.stdout);
        let reports_want = body.contains(want);
        // Cross-check: it must NOT report the OTHER version.
        let other = php::patch_for_minor(if minor == "8.1" { "8.3" } else { "8.1" }).unwrap();
        let reports_other = body.contains(other);

        let ok = code == "200" && reports_want && !reports_other;
        println!(
            "  {domain:<7} PHP {minor}  http={code}  reports {want}: {reports_want}  (leaks {other}: {reports_other})  {}",
            if ok { "OK" } else { "FAIL" }
        );
        if !ok {
            all_ok = false;
        }
    }

    // Cleanup (explicit — process::exit skips Drop).
    let _ = proxy::stop(&*plat, caddy.id());
    let _ = caddy.wait();
    let _ = services::stop(&*plat, nginx.id());
    let _ = nginx.wait();
    pools.stop_all(&*plat);

    if all_ok {
        println!("\nOK — each site served by its own PHP version's pool.");
    } else {
        eprintln!("\nFAILED — see above.");
        std::process::exit(1);
    }
}

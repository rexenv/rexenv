//! End-to-end check for routing a FrankenPHP override site at the edge (Phase 2 §2.3).
//! Run: `cargo run --example frankenphp_edge_serve`
//!
//! Provisions a default NGINX site (ng.test, PHP 8.3) and a FRANKENPHP override
//! site (fp.test). Starts the 8.3 pool + shared nginx (for ng.test) and a
//! FrankenPHP backend (for fp.test) on its own loopback port, rebuilds configs
//! (Caddy routes ng.test → nginx, fp.test → the FrankenPHP backend), starts the
//! edge Caddy, then curls BOTH over HTTPS through the edge and asserts:
//!   - both 200 with the served cert issued by OUR local CA (--cacert validates);
//!   - ng.test is served by the php-fpm POOL (PHP 8.3.x), fp.test by FrankenPHP's
//!     EMBEDDED PHP (8.5.x) — i.e. routed to different backends.
//!
//! Cleans everything up at the end.

use rexenv_lib::core::services::RewriteMode;
use rexenv_lib::core::php::PhpFpmPools;
use rexenv_lib::core::{binaries, frankenphp, proxy, services, sites, ssl};
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::path::Path;
use std::process::Command;
use std::thread;

mod common;
use std::time::Duration;

const NGINX_PORT: u16 = services::NGINX_HTTP_PORT;
const CADDY_HTTP: u16 = 8080;
const CADDY_HTTPS: u16 = 8443;

fn fetch(domain: &str, ca_pem: &str) -> (String, String) {
    let code = Command::new("curl")
        .args(["-s", "--resolve", &format!("{domain}:{CADDY_HTTPS}:127.0.0.1"),
               "--cacert", ca_pem, "-o", "/dev/null", "-w", "%{http_code}",
               &format!("https://{domain}:{CADDY_HTTPS}/")])
        .output().expect("curl").stdout;
    let body = Command::new("curl")
        .args(["-s", "--resolve", &format!("{domain}:{CADDY_HTTPS}:127.0.0.1"),
               "--cacert", ca_pem, &format!("https://{domain}:{CADDY_HTTPS}/")])
        .output().expect("curl").stdout;
    let body = String::from_utf8_lossy(&body);
    let ver = body.split("PHP Version ").nth(1)
        .and_then(|s| s.split('<').next()).map(|s| s.trim().to_string()).unwrap_or_default();
    (String::from_utf8_lossy(&code).trim().to_string(), ver)
}

#[tokio::main]
async fn main() {
    // Sandboxed: every path the app derives (config dir, nginx PREFIX and
    // therefore nginx.pid, run/, certs) lands in a throwaway root, so this
    // example cannot touch the running stack. See examples/common.
    let (plat, _sandbox) = common::sandbox("frankenphp_edge_serve");
    let db_path = std::env::temp_dir().join("rexenv-2_3.db");
    let _ = std::fs::remove_file(&db_path);
    let conn = db::open(&db_path).expect("db");
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).expect("ca");
    let ca_pem = ca.cert_path.display().to_string();

    let ng = sites::provision(&conn, &*plat, &ca, NewSite {
        name: "NG".into(), domain: "ng.test".into(), site_type: SiteType::Php,
        php_version: "8.3".into(), web_server: WebServer::Nginx, path: String::new(),
            db_engine: rexenv_lib::state::models::SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
    }).expect("provision ng");
    let fp = sites::provision(&conn, &*plat, &ca, NewSite {
        name: "FP".into(), domain: "fp.test".into(), site_type: SiteType::Php,
        php_version: "8.3".into(), web_server: WebServer::Frankenphp, path: String::new(),
            db_engine: rexenv_lib::state::models::SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
    }).expect("provision fp");
    let _ = &ng;

    // Backends: php-fpm pool (for ng.test) + a FrankenPHP backend (for fp.test).
    let mut pools = PhpFpmPools::default();
    pools.start(&*plat, &["8.3".to_string()]).await.expect("pool");

    let fp_bin = binaries::resolve(&*plat, "frankenphp", binaries::FRANKENPHP_VERSION).await.unwrap();
    let fp_port = frankenphp::site_port(&fp.domain);
    let fp_conf = frankenphp::write_config(&*plat, &fp.domain, Path::new(&fp.path), fp_port, RewriteMode::Single, &[]).unwrap();
    let mut fp_child = frankenphp::start(&*plat, &fp_bin, &fp.domain, &fp_conf, &[]).expect("frankenphp");

    // Shared nginx (ng.test) + edge Caddy (routes both).
    let cfg = sites::rebuild_configs(&conn, &*plat, &ca, NGINX_PORT, CADDY_HTTP, CADDY_HTTPS).unwrap();
    let nginx_bin = binaries::resolve(&*plat, "nginx", binaries::NGINX_VERSION).await.unwrap();
    let caddy_bin = binaries::resolve(&*plat, "caddy", binaries::CADDY_VERSION).await.unwrap();
    let mut nginx = services::start_nginx(&*plat, &nginx_bin, &cfg.nginx_conf, &cfg.nginx_prefix).unwrap();
    let mut caddy = proxy::start(&*plat, &caddy_bin, &cfg.caddyfile).unwrap();
    thread::sleep(Duration::from_millis(1800));

    println!("fp.test backend port = {fp_port} (override range)\n");
    let (ng_code, ng_ver) = fetch("ng.test", &ca_pem);
    let (fp_code, fp_ver) = fetch("fp.test", &ca_pem);
    println!("=== through the edge (Caddy :{CADDY_HTTPS}, local-CA TLS) ===");
    println!("  ng.test  (nginx→pool)        http={ng_code}  PHP {ng_ver}");
    println!("  fp.test  (frankenphp backend) http={fp_code}  PHP {fp_ver}");

    // Cleanup.
    let _ = proxy::stop(&*plat, caddy.id()); let _ = caddy.wait();
    let _ = services::stop(&*plat, nginx.id()); let _ = nginx.wait();
    let _ = frankenphp::stop(&*plat, fp_child.id()); let _ = fp_child.wait();
    pools.stop_all(&*plat);

    // ng.test served by the pool (8.3.x); fp.test by FrankenPHP embedded PHP (8.5.x).
    let ok = ng_code == "200" && ng_ver.starts_with("8.3")
        && fp_code == "200" && fp_ver.starts_with("8.5");
    if ok {
        println!("\nOK — edge routes nginx + FrankenPHP sites to different backends; both TLS via our CA.");
    } else {
        eprintln!("\nFAILED — see above.");
        std::process::exit(1);
    }
}

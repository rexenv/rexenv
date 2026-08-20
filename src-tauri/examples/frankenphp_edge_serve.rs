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

mod common;

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
    // Pin `sites_dir` into the sandbox: `sites::provision` reads the SETTING,
    // which falls back to the home directory, so without this the docroots land
    // in the user's real ~/rexenv/Sites (14 Aug 2026 sweep).
    common::pin_sites_dir(&conn, &*plat);
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).expect("ca");
    let ca_pem = ca.cert_path.display().to_string();

    let ng = sites::provision(&conn, &*plat, &ca, NewSite {
        name: "NG".into(), domain: "ng.test".into(), site_type: SiteType::Php,
        php_version: "8.3".into(), web_server: WebServer::Nginx, path: String::new(),
            db_engine: rexenv_lib::state::models::SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
            git_migrate: true,
            git_build_assets: false,
    }).expect("provision ng");
    let fp = sites::provision(&conn, &*plat, &ca, NewSite {
        name: "FP".into(), domain: "fp.test".into(), site_type: SiteType::Php,
        php_version: "8.3".into(), web_server: WebServer::Frankenphp, path: String::new(),
            db_engine: rexenv_lib::state::models::SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
            git_migrate: true,
            git_build_assets: false,
    }).expect("provision fp");
    let _ = &ng;

    // Backends: php-fpm pool (for ng.test) + a FrankenPHP backend (for fp.test).
    let mut pools = PhpFpmPools::default();
    pools.start(&*plat, &["8.3".to_string()]).await.expect("pool");

    let fp_bin = binaries::resolve(&*plat, "frankenphp", binaries::FRANKENPHP_VERSION).await.unwrap();
    // The RECORDED port, never the derived one — the same rule the product
    // states at `service_manager::reconcile_overrides`: "The RECORDED backend
    // port (B20 §4), never re-derived — so the spawned backend and the edge
    // route always agree, even after a domain change."
    //
    // This example re-derived it, and so spawned FrankenPHP on
    // `site_port("fp.test")` = 8243 while `rebuild_configs` wrote the edge a
    // route to the site's recorded 8200. Nothing was on 8200, so `fp.test`
    // came back 502 through a working edge, with a healthy backend listening
    // one port away — measured 20 Aug 2026:
    //
    //   [probe] derived site_port=8243  recorded_override_port=Some(8200)
    //   [caddyfile] reverse_proxy 127.0.0.1:8200
    //
    // It read as "FrankenPHP is broken" for as long as nobody diffed the two
    // numbers. The example was testing an arrangement the product never
    // produces.
    let fp_port = sites::recorded_override_port(&fp).expect("fp.test has a recorded override port");
    let fp_conf = frankenphp::write_config(&*plat, &fp.domain, Path::new(&fp.path), fp_port, RewriteMode::Single, &[]).unwrap();
    // Drop-GUARDED, all three. These were raw `Child`s, which Rust does not kill
    // on drop, so any early exit leaked them — and the readiness gates above
    // exit by PANICKING, which made the leak likelier than the flat sleep ever
    // did. Four failing runs on 20 Aug 2026 left four caddies alive, and they
    // then fought each other for :8443 and turned every later run into `000`.
    // A guard that reports a problem must not manufacture one.
    let mut fp_child = common::OwnedService::new(
        frankenphp::start(&*plat, &fp_bin, &fp.domain, &fp_conf, &[]).expect("frankenphp"),
        "frankenphp",
    );

    // Shared nginx (ng.test) + edge Caddy (routes both).
    let cfg = sites::rebuild_configs(&conn, &*plat, &ca, NGINX_PORT, CADDY_HTTP, CADDY_HTTPS).unwrap();
    let nginx_bin = binaries::resolve(&*plat, "nginx", binaries::NGINX_VERSION).await.unwrap();
    let caddy_bin = binaries::resolve(&*plat, "caddy", binaries::CADDY_VERSION).await.unwrap();
    let mut nginx = common::OwnedService::new(
        services::start_nginx(&*plat, &nginx_bin, &cfg.nginx_conf, &cfg.nginx_prefix).unwrap(),
        "nginx",
    );
    let mut caddy = common::OwnedService::new(
        proxy::start(&*plat, &caddy_bin, &cfg.caddyfile).unwrap(),
        "caddy",
    );
    // FOUR spawns shared one flat sleep. Each returns at fork, and the two
    // requests below traverse different backends — a pool for ng.test, the
    // FrankenPHP process for fp.test — so a miss on either reads as the edge
    // routing to the wrong place.
    common::await_listening(services::PHP_FPM_PORT, "php-fpm 8.3", None);
    common::await_listening(fp_port, "frankenphp", None);
    common::await_listening(NGINX_PORT, "nginx", None);
    common::await_listening(CADDY_HTTPS, "the caddy edge", None);
    // ...and then wait for it to ANSWER, which is a different fact.
    //
    // `await_listening` proves the socket accepts. Caddy binds its listener
    // before it has finished loading certificates and routes, so a request in
    // that window comes back `000` — curl's "no HTTP response at all" — and the
    // table below reads it as the SITE being broken. Measured 20 Aug 2026: the
    // edge logged `enabling HTTP/3 listener addr :8443` and the whole run was
    // over 245ms later with both sites at 000.
    //
    // The flat `sleep(1800ms)` this replaced hid that by being generous, which
    // is the honest reason a sleep sometimes "works": it is not a check, but it
    // is a long one. The fix is not to go back to sleeping — it is to poll the
    // thing the assertions actually depend on.
    //
    // NOT circular: this waits for ANY http status, then the assertions below
    // demand 200. A 502 ends the wait immediately and fails on its own merits.
    common::await_ready("the caddy edge answering HTTPS", None, || {
        fetch(&ng.domain, &ca_pem).0 != "000"
    });

    println!("fp.test backend port = {fp_port} (override range)\n");
    let (ng_code, ng_ver) = fetch("ng.test", &ca_pem);
    let (fp_code, fp_ver) = fetch("fp.test", &ca_pem);
    println!("=== through the edge (Caddy :{CADDY_HTTPS}, local-CA TLS) ===");
    println!("  ng.test  (nginx→pool)        http={ng_code}  PHP {ng_ver}");
    println!("  fp.test  (frankenphp backend) http={fp_code}  PHP {fp_ver}");

    // Cleanup. `stop()` is the guard's own idempotent shutdown — the same code
    // its Drop runs, so the happy path and the panic path tear down identically
    // instead of the happy path having its own hand-written version.
    caddy.stop();
    nginx.stop();
    fp_child.stop();
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

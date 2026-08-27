//! End-to-end check for per-site server switching (Phase 2 §4.1).
//! Run: `cargo run --example server_switch_serve`
//!
//! Brings the stack up (ServiceManager, high ports) for one site, then switches
//! its web server Nginx → FrankenPHP → Nginx the way the IPC command does
//! (`sites::set_web_server` + `ServiceManager::reload`, which reconciles per-site
//! override backends + reloads the edge). Curls the SAME URL after each switch:
//!   - Nginx step  → served by the shared php-fpm pool (PHP 8.3.x);
//!   - FrankenPHP  → served by its embedded PHP (8.5.x);
//!
//! all HTTP 200, no docroot/cert/DB rebuild. Cleans up at the end.

use rexenv_lib::core::service_manager::{self, Ports, ServiceManager};
use rexenv_lib::core::{binaries, services, sites, ssl};
use rexenv_lib::platform;
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::process::Command;
use std::time::Duration;

mod common;

const CADDY_HTTPS: u16 = 8443;
const DOMAIN: &str = "sw.test";

fn fetch(ca_pem: &str) -> (String, String) {
    let code = Command::new("curl")
        .args(["-s", "--resolve", &format!("{DOMAIN}:{CADDY_HTTPS}:127.0.0.1"),
               "--cacert", ca_pem, "-o", "/dev/null", "-w", "%{http_code}",
               &format!("https://{DOMAIN}:{CADDY_HTTPS}/")])
        .output().expect("curl").stdout;
    let body = Command::new("curl")
        .args(["-s", "--resolve", &format!("{DOMAIN}:{CADDY_HTTPS}:127.0.0.1"),
               "--cacert", ca_pem, &format!("https://{DOMAIN}:{CADDY_HTTPS}/")])
        .output().expect("curl").stdout;
    let body = String::from_utf8_lossy(&body);
    let ver = body.split("PHP Version ").nth(1)
        .and_then(|s| s.split('<').next()).map(|s| s.trim().to_string()).unwrap_or_default();
    (String::from_utf8_lossy(&code).trim().to_string(), ver)
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    // Refuse beside a live stack: these services would JOIN it, not collide
    // with it, and a connect-based readiness gate is satisfied by the user's
    // server. FIRST statement — after anything is spawned, exiting leaks it.
    common::require_stack_stopped();
    let plat = platform::current();
    let db_path = std::env::temp_dir().join("rexenv-4_1.db");
    let _ = std::fs::remove_file(&db_path);
    let conn = db::open(&db_path).expect("db");
    // Fixture-owned docroot. `sites::provision` reads the `sites_dir` SETTING,
    // which falls back to a path derived from $HOME — so without this the site
    // lands in the user's real ~/rexenv/Sites and SURVIVES into the next run.
    // That is not untidy, it is the bug: two checks failed on their own
    // leftovers on 21 Aug 2026 (`wp_tools_check`, `wp_themes_check`).
    let _sites_dir = common::pin_fixture_sites_dir(&conn, "swserve");
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).expect("ca");
    let ca_pem = ca.cert_path.display().to_string();

    let site = sites::provision(&conn, &*plat, &ca, NewSite {
        name: "Switch".into(), domain: DOMAIN.into(), site_type: SiteType::Php,
        php_version: "8.3".into(), web_server: WebServer::Nginx, path: String::new(),
            db_engine: rexenv_lib::state::models::SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
            git_migrate: true,
            git_build_assets: false,
            starter_db: false,
    }).expect("provision");

    let mut mgr = ServiceManager::with_ports(Ports { http: 8080, https: CADDY_HTTPS, nginx: services::NGINX_HTTP_PORT });
    let all = sites::list(&conn).unwrap();
    if let Err(e) = mgr.start_all(&*plat, &ca, &all, &["8.3".to_string()], binaries::ADMINER_VERSION).await {
        eprintln!("start_all failed: {e}");
        // `FAILURE` rather than `process::exit(1)`: exit runs no destructors, so it
        // skipped the ServiceManager's own Drop and left whatever `start_core` had
        // already spawned holding a production port (common/mod.rs, the verdict
        // contract).
        return std::process::ExitCode::FAILURE;
    }
    // `start_all` awaits ReadyChecks for the databases, mailpit and the
    // FrankenPHP overrides — not the edge, not nginx, not the pools. The flat
    // sleep that stood here was covering all three of those.
    common::await_listening(CADDY_HTTPS, "the caddy edge", None);
    common::await_listening(services::NGINX_HTTP_PORT, "nginx", None);
    common::await_listening(services::PHP_FPM_PORT, "php-fpm 8.3", None);
    // ACCEPTING is not ANSWERING. The 1200ms sleep the sweep deleted at this
    // exact spot was the only thing covering Caddy's certificate/route load
    // window, and the very next statement is the FIRST fetch — whose `000`
    // would be recorded as the nginx backend failing to serve.
    common::await_answering(
        DOMAIN,
        CADDY_HTTPS,
        std::path::Path::new(&ca_pem),
        "the caddy edge answering HTTPS",
    );

    let mut results = Vec::new();
    let (c0, v0) = fetch(&ca_pem);
    println!("nginx       → http={c0}  PHP {v0}");
    results.push((c0, v0, "8.3")); // pool

    // Switch → FrankenPHP.
    sites::set_web_server(&conn, &site.id, WebServer::Frankenphp).unwrap();
    let checks = mgr.reload(&*plat, &ca, &sites::list(&conn).unwrap(), false).await.expect("reload→fp");
    service_manager::await_ready(checks).await.expect("fp backend ready");
    std::thread::sleep(Duration::from_millis(1200));
    let (c1, v1) = fetch(&ca_pem);
    println!("frankenphp  → http={c1}  PHP {v1}");
    results.push((c1, v1, "8.5")); // embedded

    // Switch back → Nginx.
    sites::set_web_server(&conn, &site.id, WebServer::Nginx).unwrap();
    let checks = mgr.reload(&*plat, &ca, &sites::list(&conn).unwrap(), false).await.expect("reload→nginx");
    service_manager::await_ready(checks).await.expect("ready after switch back");
    std::thread::sleep(Duration::from_millis(1200));
    let (c2, v2) = fetch(&ca_pem);
    println!("nginx again → http={c2}  PHP {v2}  (same shared 8.3 pool — no per-server pool)");
    results.push((c2, v2, "8.3"));

    mgr.stop_all(&*plat).ok();

    let ok = results.iter().all(|(code, ver, want)| code == "200" && ver.starts_with(want));
    if ok {
        println!("\nOK — switched Nginx → FrankenPHP → Nginx live; each served 200 at the same URL, no rebuild.");
    } else {
        eprintln!("\nFAILED — see above.");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}

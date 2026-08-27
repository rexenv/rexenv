//! Manual check for the health watchdog (services stay healthy long-run).
//! Starts the stack on HIGH ports (no admin prompt), then SIGKILLs Nginx, a
//! php-fpm pool, and Mailpit behind the manager's back — the exact "UI says
//! running but nothing serves" failure. One `reconcile_health` pass must
//! detect + respawn each, report events, and leave the probes green. The
//! killed edge must be reported "edge-down" (never auto-restarted: privileged).

use rexenv_lib::core::service_manager::{await_ready, Ports, ServiceManager};
use rexenv_lib::core::{binaries, mail, services, sites, ssl};
use rexenv_lib::platform;
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::time::Duration;

mod common;

fn kill(pid: u32, name: &str) {
    let ok = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    println!("SIGKILL {name} (pid {pid}) -> {ok}");
}

#[tokio::main]
async fn main() {
    // Refuse beside a live stack: these services would JOIN it, not collide
    // with it, and a connect-based readiness gate is satisfied by the user's
    // server. FIRST statement — after anything is spawned, exiting leaks it.
    common::require_stack_stopped();
    let plat = platform::current();
    let ports = Ports { http: 8080, https: 8443, nginx: services::NGINX_HTTP_PORT };

    let conn = {
        let p = std::env::temp_dir().join("rexenv-health-check.db");
        let _ = std::fs::remove_file(&p);
        db::open(&p).unwrap()
    };
    // Fixture-owned docroot. `sites::provision` reads the `sites_dir` SETTING,
    // which falls back to a path derived from $HOME — so without this the site
    // lands in the user's real ~/rexenv/Sites and SURVIVES into the next run.
    // That is not untidy, it is the bug: two checks failed on their own
    // leftovers on 21 Aug 2026 (`wp_tools_check`, `wp_themes_check`).
    let _sites_dir = common::pin_fixture_sites_dir(&conn, "healthchk");
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();
    sites::provision(
        &conn,
        &*plat,
        &ca,
        NewSite {
            name: "Health Check".into(),
            domain: "healthchk.test".into(),
            site_type: SiteType::Php,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            path: String::new(),
            db_engine: rexenv_lib::state::models::SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
            git_migrate: true,
            git_build_assets: false,
            starter_db: false,
        },
    )
    .unwrap();
    let all = sites::list(&conn).unwrap();

    let mut mgr = ServiceManager::with_ports(ports);
    println!("starting stack (high ports)…");
    mgr.start_all(&*plat, &ca, &all, &["8.3".into()], binaries::ADMINER_VERSION).await.unwrap();

    // Baseline: everything green.
    let running = |mgr: &ServiceManager, name: &str| {
        mgr.status(&*plat, &[]).iter().any(|s| s.name == name && s.running)
    };
    // The child edge binds its admin socket asynchronously after spawn — give it
    // a moment (the app's status poll simply shows it a tick later).
    for _ in 0..20 {
        if running(&mgr, "Caddy") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert!(running(&mgr, "Nginx"), "baseline: nginx up");
    assert!(running(&mgr, "Mailpit"), "baseline: mailpit up");
    assert!(running(&mgr, "PHP-FPM 8.3"), "baseline: pool up");
    assert!(running(&mgr, "Caddy"), "baseline: edge up");
    println!("baseline OK — all services green");

    // Kill Nginx + the 8.3 pool + Mailpit behind the manager's back.
    let pid_of = |mgr: &ServiceManager, name: &str| {
        mgr.status(&*plat, &[]).iter().find(|s| s.name == name).and_then(|s| s.pid).unwrap()
    };
    kill(pid_of(&mgr, "Nginx"), "Nginx");
    kill(pid_of(&mgr, "PHP-FPM 8.3"), "PHP-FPM 8.3");
    kill(pid_of(&mgr, "Mailpit"), "Mailpit");
    tokio::time::sleep(Duration::from_millis(600)).await;

    // Mailpit is single-process, so the port probe sees the kill immediately.
    // Nginx/php-fpm masters leave ORPHANED WORKERS holding their ports (title
    // rewritten, no marker) — the port probe may still read green; that is the
    // exact deception the watchdog's master-alive check exists for.
    assert!(!running(&mgr, "Mailpit"), "status must see dead mailpit");
    println!("mailpit kill visible in status; master kills hidden by orphan workers (by design of the test)");

    // One watchdog pass respawns all three.
    let (events, checks) = mgr.reconcile_health(&*plat, &ca, &all).await;
    for e in &events {
        println!("event: [{}] {}: {}", e.action, e.service, e.detail);
    }
    await_ready(checks).await.unwrap();
    assert!(events.iter().any(|e| e.service == "Nginx" && e.action == "restarted"));
    assert!(events.iter().any(|e| e.service == "PHP-FPM 8.3" && e.action == "restarted"));
    assert!(events.iter().any(|e| e.service == "Mailpit" && e.action == "restarted"));
    // Nginx has no ReadyCheck (synchronous spawn) — give it a moment to bind,
    // exactly like the app's next status poll would.
    for _ in 0..20 {
        if running(&mgr, "Nginx") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert!(running(&mgr, "Nginx"), "nginx respawned");
    assert!(running(&mgr, "Mailpit"), "mailpit respawned");
    assert!(mail::running(), "mailpit answering");
    assert!(running(&mgr, "PHP-FPM 8.3"), "pool respawned");
    println!("watchdog respawned Nginx + pool + Mailpit ✓");

    // A healthy pass right after: no events, nothing touched.
    let (events, checks) = mgr.reconcile_health(&*plat, &ca, &all).await;
    assert!(events.is_empty(), "healthy pass must be silent, got {events:?}");
    assert!(checks.is_empty());
    println!("healthy pass is a no-op ✓");

    // Kill the (unprivileged child) edge: watchdog must mark it down, NOT restart.
    let edge_pid = pid_of(&mgr, "Caddy");
    kill(edge_pid, "Caddy edge");
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(!running(&mgr, "Caddy"), "status must see dead edge (admin socket probe)");
    let (events, _) = mgr.reconcile_health(&*plat, &ca, &all).await;
    for e in &events {
        println!("event: [{}] {}: {}", e.action, e.service, e.detail);
    }
    assert!(events.iter().any(|e| e.service == "Caddy" && e.action == "edge-down"));
    assert!(!running(&mgr, "Caddy"), "edge stays down (privileged restart is the user's call)");
    println!("edge death detected + surfaced (no silent privileged restart) ✓");

    // Health log written.
    let health_log = plat.paths().log_dir().unwrap().join("health.log");
    // (log_health_events is the caller's job in the app; do it here like lib.rs does)
    rexenv_lib::core::service_manager::log_health_events(&*plat, &events);
    assert!(health_log.exists(), "health.log written");
    println!("health.log at {}", health_log.display());

    mgr.stop_all(&*plat).unwrap();
    println!("\nALL CHECKS PASSED");
}

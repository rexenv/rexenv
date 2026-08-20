//! Manual check that the resource monitor covers ALL Phase-2 services (§6.1).
//! Run: `cargo run --example monitor_coverage_demo`
//!
//! Brings up a mixed stack (ServiceManager, high ports): an Nginx site (php-fpm
//! pool) + a FrankenPHP-override site, plus MySQL and PostgreSQL. Then prints
//! `ServiceManager::status()` enriched with live RAM/CPU per pid (the same data
//! the Services view shows) and asserts every supervised service appears with a
//! pid + RAM. Cleans up at the end.

use rexenv_lib::core::monitor::Monitor;
use rexenv_lib::core::db::DbEngine;
use rexenv_lib::core::service_manager::{Ports, ServiceManager};
use rexenv_lib::core::{binaries, services, sites, ssl};
use rexenv_lib::platform;

use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::time::Duration;


mod common;
#[tokio::main]
async fn main() {
    let plat = platform::current();
    let (conn, _dbf) = common::fixture_db("monitor_coverage_demo");
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).expect("ca");

    for (name, domain, server) in [
        ("NG", "ng6.test", WebServer::Nginx),
        ("FP", "fp6.test", WebServer::Frankenphp),
    ] {
        sites::provision(&conn, &*plat, &ca, NewSite {
            name: name.into(), domain: domain.into(), site_type: SiteType::Php,
            php_version: "8.3".into(), web_server: server, path: String::new(),
            db_engine: rexenv_lib::state::models::SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
            git_migrate: true,
            git_build_assets: false,
        }).expect("provision");
    }

    let mut mgr = ServiceManager::with_ports(Ports { http: 8080, https: 8443, nginx: services::NGINX_HTTP_PORT });
    let all = sites::list(&conn).unwrap();
    if let Err(e) = mgr.start_all(&*plat, &ca, &all, &["8.3".to_string()], binaries::ADMINER_VERSION).await {
        eprintln!("start_all failed: {e}");
        std::process::exit(1);
    }
    // Also start PostgreSQL (a standalone DB engine).
    if let Err(e) = mgr.ensure_db(&*plat, DbEngine::Postgres).await {
        eprintln!("postgres start failed: {e}");
    }
    // The edge and the pool are both spawned inside `start_all` and neither is
    // covered by its `await_ready`, so this flat sleep was the only thing
    // standing between them and the status table below — which REPORTS
    // per-service liveness, so a service still binding read as one that died.
    common::await_listening(services::PHP_FPM_PORT, "php-fpm 8.3", None);

    // Two refresh sweeps so CPU% is computed over an interval (`process()` is
    // read-only since M6 — `refresh_processes()` does the sampling).
    let mut mon = Monitor::new();
    mon.refresh_processes();
    std::thread::sleep(Duration::from_millis(500));
    mon.refresh_processes();

    println!("=== services_status coverage (name · running · pid · CPU · RAM) ===");
    let rows = mgr.status(&*plat, &[]);
    let expected = ["MySQL", "PostgreSQL", "PHP-FPM 8.3", "FrankenPHP fp6.test", "Nginx", "Caddy"];
    let mut all_ok = true;
    for i in &rows {
        let m = i.pid.and_then(|p| mon.tree(p));
        let (cpu, ram) = m.map(|m| (m.cpu_percent, m.ram_mb)).unwrap_or((0.0, 0));
        println!(
            "  {:<18} running={:<5} pid={:<7} cpu={:>5.1}%  ram={:>5} MB",
            i.name,
            i.running,
            i.pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
            cpu,
            ram
        );
        // Every service THIS EXAMPLE STARTED should be running with a pid and
        // measurable RAM. The row set from `status` covers every engine rexenv
        // knows, and this example starts MySQL and PostgreSQL only — so MariaDB
        // and Redis come back `running=false, pid=None, ram=0` by design, and
        // the loop convicted them. It passed anyway for as long as LEFTOVER
        // MariaDB/Redis processes from other examples happened to be up;
        // sweeping the machine clean before a run (20 Aug 2026) is what made it
        // fail, which is the same story as everything else found that day: the
        // check was reading the machine, not the subject. `expected` below
        // already listed the right six — the loop just was not consulting it.
        if expected.contains(&i.name.as_str()) && (!i.running || i.pid.is_none() || ram == 0) {
            all_ok = false;
        }
    }

    // The Phase-2 services must all be present.
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    let present = expected.iter().all(|e| names.iter().any(|n| n == e));

    mgr.stop_all(&*plat).ok();

    if all_ok && present {
        println!("\nOK — every supervised Phase-2 service reports live RAM/CPU.");
    } else {
        eprintln!("\nFAILED — missing service or zero metrics. present={present} names={names:?}");
        std::process::exit(1);
    }
}

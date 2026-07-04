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
use rexenv_lib::core::{sites, ssl};
use rexenv_lib::platform;
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::time::Duration;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let db_path = std::env::temp_dir().join("rexenv-6_1.db");
    let _ = std::fs::remove_file(&db_path);
    let conn = db::open(&db_path).expect("db");
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).expect("ca");

    for (name, domain, server) in [
        ("NG", "ng6.test", WebServer::Nginx),
        ("FP", "fp6.test", WebServer::Frankenphp),
    ] {
        sites::provision(&conn, &*plat, &ca, NewSite {
            name: name.into(), domain: domain.into(), site_type: SiteType::Php,
            php_version: "8.3".into(), web_server: server, path: String::new(),
        }).expect("provision");
    }

    let mut mgr = ServiceManager::with_ports(Ports { http: 8080, https: 8443, nginx: 8088 });
    let all = sites::list(&conn).unwrap();
    if let Err(e) = mgr.start_all(&*plat, &ca, &all, &["8.3".to_string()]).await {
        eprintln!("start_all failed: {e}");
        std::process::exit(1);
    }
    // Also start PostgreSQL (a standalone DB engine).
    if let Err(e) = mgr.ensure_db(&*plat, DbEngine::Postgres).await {
        eprintln!("postgres start failed: {e}");
    }
    std::thread::sleep(Duration::from_millis(1200));

    // Two refresh sweeps so CPU% is computed over an interval (`process()` is
    // read-only since M6 — `refresh_processes()` does the sampling).
    let mut mon = Monitor::new();
    mon.refresh_processes();
    std::thread::sleep(Duration::from_millis(500));
    mon.refresh_processes();

    println!("=== services_status coverage (name · running · pid · CPU · RAM) ===");
    let rows = mgr.status(&[]);
    let mut all_ok = true;
    for i in &rows {
        let m = i.pid.and_then(|p| mon.process(p));
        let (cpu, ram) = m.map(|m| (m.cpu_percent, m.ram_mb)).unwrap_or((0.0, 0));
        println!(
            "  {:<18} running={:<5} pid={:<7} cpu={:>5.1}%  ram={:>5} MB",
            i.name,
            i.running,
            i.pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
            cpu,
            ram
        );
        // Every supervised service should be running with a pid + measurable RAM.
        if !i.running || i.pid.is_none() || ram == 0 {
            all_ok = false;
        }
    }

    // The Phase-2 services must all be present.
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    let expected = ["MySQL", "PostgreSQL", "PHP-FPM 8.3", "FrankenPHP fp6.test", "Nginx", "Caddy"];
    let present = expected.iter().all(|e| names.iter().any(|n| n == e));

    mgr.stop_all(&*plat).ok();

    if all_ok && present {
        println!("\nOK — every supervised Phase-2 service reports live RAM/CPU.");
    } else {
        eprintln!("\nFAILED — missing service or zero metrics. present={present} names={names:?}");
        std::process::exit(1);
    }
}

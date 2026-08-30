//! Manual check: per-engine DB version switch (the shipped "Deferred services"
//! plan — docs/archive/SHIPPED-2026-07.md).
//! Run: `cargo run --example db_version_switch_check`
//!
//! Proves the switch semantics end to end on PostgreSQL (its port must be
//! free; the MySQL-protocol engines may be busy serving the real stack, so
//! their alternate versions are proven by resolve + `--version` instead):
//!   1. Manager mirror: `set_db_version(Postgres, 17)` → `spawn_db` runs 17
//!      into a FRESH `postgres/17/data` (per-series datadir, never the legacy
//!      18 dir) — `SELECT version()` says 17; a marker database is created.
//!   2. Switch to 16 → its own fresh datadir — the marker db is NOT visible
//!      (per-series isolation).
//!   3. Switch back to 17 — the marker db is STILL there (data survives
//!      switching away and back). Marker dropped, engine stopped.
//!   4. MariaDB 11.4.12 bundle + MySQL 8.0.44 tree resolve into the real
//!      cache and their servers run (`--version` — no port needed).
//!
//! The legacy default-series datadirs are never touched.

use rexenv_lib::core::db::DbEngine;
use rexenv_lib::core::service_manager::ServiceManager;
use rexenv_lib::core::{binaries, ports, postgres};
use rexenv_lib::platform;
use std::process::Command;

const MARKER_DB: &str = "rexenv_switch_check";

fn psql(basedir: &std::path::Path, port: u16, sql: &str) -> (bool, String) {
    let out = Command::new(postgres::psql_bin(basedir))
        .args([
            "-h",
            "127.0.0.1",
            "-p",
            &port.to_string(),
            "-U",
            "postgres",
            "-tAc",
            sql,
        ])
        .output()
        .expect("run psql");
    (
        out.status.success(),
        String::from_utf8_lossy(if out.status.success() { &out.stdout } else { &out.stderr })
            .trim()
            .to_string(),
    )
}

async fn up(mgr: &mut ServiceManager, plat: &dyn rexenv_lib::platform::traits::Platform, version: &str) {
    mgr.set_db_version(DbEngine::Postgres, version);
    mgr.ensure_db(plat, DbEngine::Postgres).await.expect("spawn postgres");
}

fn down(mgr: &mut ServiceManager, plat: &dyn rexenv_lib::platform::traits::Platform) {
    mgr.stop_db(plat, DbEngine::Postgres).expect("stop postgres");
    for _ in 0..20 {
        if !DbEngine::Postgres.running() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let engine = DbEngine::Postgres;
    let mut ok = true;

    if let Err(e) = ports::ensure_free(&*plat, engine.port(), ports::Proto::Tcp, "PostgreSQL") {
        eprintln!("PostgreSQL port busy — stop it first: {e}");
        std::process::exit(1);
    }
    let mut mgr = ServiceManager::default();

    println!("=== 17.11.0 into its own fresh datadir ===");
    let dir17 = engine.data_dir(&*plat, "17.11.0").unwrap();
    println!("  datadir {} (fresh={})", dir17.display(), !postgres::is_initialized(&dir17));
    up(&mut mgr, &*plat, "17.11.0").await;
    let base17 = binaries::resolve_dir(&*plat, "postgres", "17.11.0").await.unwrap();
    let (_, ver) = psql(&base17, engine.port(), "SELECT version()");
    println!("  version → {}", ver.split(" on ").next().unwrap_or(&ver));
    ok &= ver.starts_with("PostgreSQL 17.");
    let (cok, _) = psql(&base17, engine.port(), &format!("CREATE DATABASE {MARKER_DB}"));
    println!("  marker db created = {cok}");
    ok &= cok;
    down(&mut mgr, &*plat);

    println!("\n=== switch to 16.15.0 — per-series isolation ===");
    up(&mut mgr, &*plat, "16.15.0").await;
    let base16 = binaries::resolve_dir(&*plat, "postgres", "16.15.0").await.unwrap();
    let (_, ver) = psql(&base16, engine.port(), "SELECT version()");
    println!("  version → {}", ver.split(" on ").next().unwrap_or(&ver));
    ok &= ver.starts_with("PostgreSQL 16.");
    let (_, seen) = psql(
        &base16,
        engine.port(),
        &format!("SELECT count(*) FROM pg_database WHERE datname='{MARKER_DB}'"),
    );
    println!("  17's marker db visible on 16 = {} (must be 0)", seen);
    ok &= seen == "0";
    down(&mut mgr, &*plat);

    println!("\n=== back to 17.11.0 — data survives the round-trip ===");
    up(&mut mgr, &*plat, "17.11.0").await;
    let (_, seen) = psql(
        &base17,
        engine.port(),
        &format!("SELECT count(*) FROM pg_database WHERE datname='{MARKER_DB}'"),
    );
    println!("  marker db still present = {} (must be 1)", seen);
    ok &= seen == "1";
    let _ = psql(&base17, engine.port(), &format!("DROP DATABASE {MARKER_DB}"));
    down(&mut mgr, &*plat);

    println!("\n=== alternate MySQL-protocol versions resolve + run ===");
    let mdb = binaries::resolve_bundle(&*plat, "mariadb", "11.4.12").await.expect("mariadb 11.4");
    let out = Command::new(mdb.join("bin/mariadbd")).arg("--version").output().unwrap();
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    println!("  {v}");
    ok &= out.status.success() && v.contains("11.4.12");
    let my = binaries::resolve_dir(&*plat, "mysql", "8.0.44").await.expect("mysql 8.0");
    let out = Command::new(my.join("bin/mysqld")).arg("--version").output().unwrap();
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    println!("  {v}");
    ok &= out.status.success() && v.contains("8.0.44");

    if ok {
        println!("\nOK — per-series datadirs isolate versions, data survives switching back, alternate pins run.");
    } else {
        eprintln!("\nFAILED — see above.");
        std::process::exit(1);
    }
}

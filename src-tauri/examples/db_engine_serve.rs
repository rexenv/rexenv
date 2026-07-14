//! Manual check for the DbEngine abstraction + PostgreSQL (Phase 2 §5.1, §5.3).
//! Run: `cargo run --example db_engine_serve`
//!
//! Proves every built-in DB engine is modeled behind one shape (key · port ·
//! start · running · stop). MySQL and PostgreSQL are started THROUGH `DbEngine`
//! (delegating to `core::database` / `core::postgres`): start → port listening →
//! (Postgres) `psql 'SELECT version();'` → stop. MariaDB is deferred and returns
//! the same-shaped error; Redis has its own check (`redis_bundle_check`).

use rexenv_lib::core::db::DbEngine;
use rexenv_lib::core::{binaries, ports, postgres};
use rexenv_lib::platform;
use std::process::Command;
use std::thread;
use std::time::Duration;

/// Start an engine via DbEngine, wait for its port, return (pid, up).
async fn bring_up(
    plat: &dyn rexenv_lib::platform::traits::Platform,
    engine: DbEngine,
) -> Option<(u32, std::process::Child)> {
    if let Err(e) = ports::ensure_free(plat, engine.port(), ports::Proto::Tcp, engine.label()) {
        eprintln!("{} port busy: {e}", engine.label());
        return None;
    }
    let child = engine.start(plat).await.expect("start via DbEngine");
    let pid = child.id();
    for _ in 0..40 {
        if engine.running() {
            return Some((pid, child));
        }
        thread::sleep(Duration::from_millis(500));
    }
    Some((pid, child))
}

#[tokio::main]
async fn main() {
    let plat = platform::current();

    println!("=== registered DB engines ===");
    for e in DbEngine::ALL {
        println!("  {:<10} key={:<9} port={}", e.label(), e.key(), e.port());
    }

    let mut ok = true;

    // MySQL through the abstraction.
    println!("\n=== DbEngine::Mysql ===");
    if let Some((pid, mut child)) = bring_up(&*plat, DbEngine::Mysql).await {
        let up = DbEngine::Mysql.running();
        println!("  pid {pid} · listening on :{} = {up}", DbEngine::Mysql.port());
        ok &= up;
        let _ = DbEngine::Mysql.stop(&*plat, pid);
        let _ = child.wait();
        println!("  stopped");
    } else {
        ok = false;
    }

    // PostgreSQL through the abstraction + a real query via bundled psql.
    println!("\n=== DbEngine::Postgres ===");
    if let Some((pid, mut child)) = bring_up(&*plat, DbEngine::Postgres).await {
        let up = DbEngine::Postgres.running();
        println!("  pid {pid} · listening on :{} = {up}", DbEngine::Postgres.port());
        let basedir = binaries::resolve_dir(&*plat, "postgres", binaries::POSTGRES_VERSION).await.unwrap();
        let out = Command::new(postgres::psql_bin(&basedir))
            .args(["-h", "127.0.0.1", "-p", &DbEngine::Postgres.port().to_string(),
                   "-U", "postgres", "-tAc", "SELECT version();"])
            .output().expect("run psql");
        let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let queried = out.status.success() && ver.starts_with("PostgreSQL");
        println!("  psql SELECT version() → {ver}");
        ok &= up && queried;
        let _ = DbEngine::Postgres.stop(&*plat, pid);
        let _ = child.wait();
        println!("  stopped");
    } else {
        ok = false;
    }

    // Deferred engines share the shape; not implemented on macOS.
    println!("\n=== deferred engines (uniform shape) ===");
    for e in [DbEngine::Mariadb] {
        match e.start(&*plat).await {
            Ok(mut c) => { let _ = e.stop(&*plat, c.id()); let _ = c.wait(); println!("  {} started (unexpected)", e.label()); }
            Err(err) => println!("  {:<8} → {err}", e.label()),
        }
    }

    if ok {
        println!("\nOK — MySQL + PostgreSQL run through DbEngine; PostgreSQL answered a query.");
    } else {
        eprintln!("\nFAILED — see above.");
        std::process::exit(1);
    }
}

//! Manual check for the DbEngine abstraction (Phase 2 task 5.1).
//! Run: `cargo run --example db_engine_serve`
//!
//! Proves every built-in DB engine is modeled behind one shape (key · port ·
//! start · running · stop). MySQL is started through `DbEngine::Mysql` (delegating
//! to the Phase-1 `core::database`, behavior unchanged): start → port listening →
//! stop. The not-yet-implemented engines (§5.2–§5.4) return the same-shaped error.

use rexenv_lib::core::db::DbEngine;
use rexenv_lib::core::ports;
use rexenv_lib::platform;
use std::thread;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let plat = platform::current();

    println!("=== registered DB engines ===");
    for e in DbEngine::ALL {
        println!("  {:<10} key={:<9} port={}", e.label(), e.key(), e.port());
    }

    // MySQL through the abstraction.
    println!("\n=== DbEngine::Mysql lifecycle ===");
    if let Err(e) = ports::ensure_free(DbEngine::Mysql.port(), ports::Proto::Tcp, "MySQL") {
        eprintln!("MySQL port busy: {e}");
        std::process::exit(1);
    }
    let mut child = DbEngine::Mysql.start(&*plat).await.expect("start MySQL via DbEngine");
    let mut up = false;
    for _ in 0..30 {
        if DbEngine::Mysql.running() {
            up = true;
            break;
        }
        thread::sleep(Duration::from_millis(500));
    }
    println!("  started pid {} · listening on :{} = {up}", child.id(), DbEngine::Mysql.port());
    let _ = DbEngine::Mysql.stop(&*plat, child.id());
    let _ = child.wait();
    println!("  stopped");

    // The other engines share the shape; not implemented until §5.2–§5.4.
    println!("\n=== not-yet-implemented engines (uniform shape) ===");
    for e in [DbEngine::Mariadb, DbEngine::Postgres, DbEngine::Redis] {
        match e.start(&*plat).await {
            Ok(mut c) => {
                let _ = e.stop(&*plat, c.id());
                let _ = c.wait();
                println!("  {} started (unexpected)", e.label());
            }
            Err(err) => println!("  {:<10} → {err}", e.label()),
        }
    }

    if up {
        println!("\nOK — MySQL runs through DbEngine; the shape is ready for MariaDB/PostgreSQL/Redis.");
    } else {
        eprintln!("\nFAILED — MySQL did not come up via DbEngine.");
        std::process::exit(1);
    }
}

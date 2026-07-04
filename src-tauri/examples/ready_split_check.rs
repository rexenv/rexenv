//! Live check for the M4-residual fix: the spawn/await readiness split.
//! Run: `cargo run --example ready_split_check`
//!
//! Uses PostgreSQL (loopback :15432) so it can run alongside a serving rexenv
//! stack (which holds MySQL :13306 + the edge). Proves, against a real engine:
//!   - `spawn_db` returns a `ReadyCheck` after SPAWNING only (the phase a
//!     command runs under the services lock);
//!   - `await_ready` (the phase run with the lock RELEASED) drives the probe
//!     to readiness — `engine.running()` is true afterwards;
//!   - a second `spawn_db` is a no-op (`None`) while the engine is managed;
//!   - `stop_db` brings it back down. Cleans up at the end.

use rexenv_lib::core::db::DbEngine;
use rexenv_lib::core::service_manager::{self, ServiceManager};
use rexenv_lib::platform;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let mut mgr = ServiceManager::default();
    let engine = DbEngine::Postgres;

    // Phase 1 (in the app: under the services lock) — spawn, don't wait.
    let check = mgr
        .spawn_db(&*plat, engine)
        .await
        .expect("spawn postgres")
        .expect("engine was not already managed → a ReadyCheck");
    let tracked = mgr.db_status().iter().any(|d| d.engine == engine && d.pid.is_some());
    println!("spawned: handle tracked = {tracked}");

    // Idempotence: while managed, another spawn returns no new check.
    let again = mgr.spawn_db(&*plat, engine).await.expect("re-spawn");
    println!("re-spawn while managed → None = {}", again.is_none());

    // Phase 2 (in the app: services lock RELEASED) — await readiness.
    service_manager::await_ready(vec![check]).await.expect("postgres became ready");
    let up = engine.running();
    println!("after await_ready → engine.running() = {up}");

    mgr.stop_db(&*plat, engine).expect("stop");
    let down = !engine.running();
    println!("after stop_db → engine stopped = {down}");

    if tracked && again.is_none() && up && down {
        println!("\nOK — spawn/await split works live: spawn tracks the child, await_ready drives it to ready, stop cleans up.");
    } else {
        eprintln!("\nFAILED — tracked={tracked} up={up} down={down}");
        std::process::exit(1);
    }
}

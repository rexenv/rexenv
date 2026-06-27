//! Manual check for the per-version PHP-FPM pool manager (Phase 2 task 1.2).
//! Run: `cargo run --example php_pools_serve`
//!
//! Starts ONE php-fpm master per pinned PHP version (`php::all_minors()`), each on
//! its own deterministic loopback port (8.1→9781, 8.2→9782, 8.3→9783), via
//! `PhpFpmPools` (ProcessSupervisor::spawn_logged, port-gated). Confirms each pool
//! is listening, prints the same per-pool status the Services view reads, then
//! stops them all. No per-site pools — one shared master per version.

use rexenv_lib::core::php::{self, PhpFpmPools};
use rexenv_lib::platform;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let minors = php::all_minors();
    println!("starting a php-fpm pool per version: {minors:?}\n");

    let mut pools = PhpFpmPools::default();
    if let Err(e) = pools.start(&*plat, &minors).await {
        eprintln!("pool start failed: {e}");
        // process::exit skips Drop, so stop any partially-started pools explicitly.
        pools.stop_all(&*plat);
        std::process::exit(1);
    }

    // Give the masters a moment to bind their ports.
    std::thread::sleep(Duration::from_millis(800));

    println!("=== pool status (version · port · pid · listening) ===");
    let status = pools.status();
    let mut all_up = true;
    for s in &status {
        println!(
            "  PHP-FPM {:<5} 127.0.0.1:{}  pid {:<6} {}",
            s.minor,
            s.port,
            s.pid,
            if s.running { "LISTENING" } else { "DOWN" }
        );
        if !s.running {
            all_up = false;
        }
    }

    println!("\nstopping pools…");
    pools.stop_all(&*plat);

    if all_up && status.len() == minors.len() {
        println!("OK — {} pools, one per version, each on its own port.", status.len());
    } else {
        eprintln!("FAILED — not every pool came up (see above).");
        std::process::exit(1);
    }
}

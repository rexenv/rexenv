//! Live check: the per-version PHP-FPM pool manager — one master per pinned
//! minor, each on its version's port, all stopped clean. Run (stack STOPPED —
//! see below): `cargo run --example php_pools_serve`
//!
//! SANDBOXED for paths (previously wrote every pool config into the REAL
//! config dir — the incident-3 shape). The PORTS stay the production
//! per-version ones (9781/9782/9783): they are derived inside `PhpFpmPools`,
//! which is the thing under test, and forking a fixture-port variant of the
//! production manager would test the fork instead. `ensure_free`'s port gate
//! turns a running stack into a clean refusal here, never a corruption —
//! that refusal is this example's documented requirement, not a bug.

use rexenv_lib::core::php::{self, PhpFpmPools};
use rexenv_lib::core::services;
use std::process::ExitCode;
use std::time::Duration;

mod common;

#[tokio::main]
async fn main() -> ExitCode {
    let (plat, _sandbox) = common::sandbox("php_pools_serve");
    let mut checks = common::Check::new("php_pools_serve");
    let minors = php::all_minors();
    println!("starting a php-fpm pool per version: {minors:?}\n");

    let mut pools = PhpFpmPools::default();
    if let Err(e) = pools.start(&*plat, &minors).await {
        // process::exit would skip Drop guards; report through the verdict.
        pools.stop_all(&*plat);
        checks.is("pools started", false, &format!("{e} (is the stack running? stop it first)"));
        return checks.verdict();
    }

    // Give the masters a moment to bind their ports.
    std::thread::sleep(Duration::from_millis(800));

    println!("=== pool status (version · port · pid · listening) ===");
    let status = pools.status();
    for s in &status {
        println!(
            "  PHP-FPM {:<5} 127.0.0.1:{}  pid {:<6} {}",
            s.minor,
            s.port,
            s.pid,
            if s.running { "LISTENING" } else { "DOWN" }
        );
        checks.is(&format!("pool {} listening on {}", s.minor, s.port), s.running, "DOWN");
    }
    checks.is(
        "one pool per pinned version",
        status.len() == minors.len(),
        &format!("{} pools for {} versions", status.len(), minors.len()),
    );

    println!("\nstopping pools…");
    pools.stop_all(&*plat);
    for s in &status {
        checks.is(&format!("pool {} port freed", s.minor), !services::fpm_running(s.port), "held");
    }
    checks.verdict()
}

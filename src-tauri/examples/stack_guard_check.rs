//! Live check: `core::stack_guard` — a non-app process (this example) must NOT
//! be able to stop the USER'S running stack, even through the exact paths that
//! historically tore it down (docs/archive/SHIPPED-2026-07.md, "Isolate live-check examples").
//! Run WITH the stack running: `cargo run --example stack_guard_check`
//!
//! Deliberately does NOT call `allow_real_stack_control()` — exercising the
//! guarded paths against the real stack is the point. Worst-case failure
//! self-heals: the edge is a KeepAlive LaunchDaemon (launchd relaunches it),
//! and the example restarts any DB engine it manages to stop.

use rexenv_lib::core::db::DbEngine;
use rexenv_lib::core::service_manager::ServiceManager;
use rexenv_lib::core::{binaries, php, proxy, services};
use rexenv_lib::platform;

#[tokio::main]
async fn main() {
    let plat = platform::current();

    // Snapshot what is serving BEFORE any guarded call.
    let edge_before = proxy::admin_alive(&*plat);
    let dbs_before: Vec<DbEngine> = DbEngine::ALL
        .into_iter()
        .filter(|e| e.available() && e.running())
        .collect();
    let pools_before: Vec<(String, u16)> = php::all_minors()
        .into_iter()
        .filter_map(|m| php::fpm_port(&m).map(|p| (m, p)))
        .filter(|(_, p)| services::fpm_running(*p))
        .collect();
    println!(
        "before: edge={edge_before} dbs={dbs_before:?} pools={:?}",
        pools_before.iter().map(|(m, _)| m).collect::<Vec<_>>()
    );
    if !edge_before && dbs_before.is_empty() && pools_before.is_empty() {
        println!("SKIP — no running stack to guard; start the stack and re-run.");
        return;
    }

    // 1) recover_stale_edge must REFUSE (error naming the guard), edge survives.
    if edge_before {
        let caddy = binaries::resolve(&*plat, "caddy", binaries::CADDY_VERSION)
            .await
            .expect("resolve caddy");
        match proxy::recover_stale_edge(&*plat, &caddy) {
            Err(e) => {
                let msg = e.to_string();
                assert!(msg.contains("stack guard"), "wrong refusal: {msg}");
                println!("✓ recover_stale_edge refused: {msg}");
            }
            Ok(()) => panic!("recover_stale_edge should have refused with a live edge"),
        }
        assert!(proxy::admin_alive(&*plat), "edge died after recover_stale_edge");

        // 2) stop_edge must skip (Ok), edge survives.
        proxy::stop_edge(&*plat, &caddy).expect("stop_edge should skip, not fail");
        assert!(proxy::admin_alive(&*plat), "edge died after guarded stop_edge");
        println!("✓ stop_edge skipped — edge admin socket still live");
    }

    // 3) adopt the running services, then stop_all — adopted survivors + the
    //    orphan sweep must be skipped; everything keeps serving.
    let mut mgr = ServiceManager::default();
    let adopted = mgr.adopt_startup(&*plat, &[]);
    println!("adopted {adopted} service(s)");
    mgr.stop_all(&*plat).expect("stop_all");

    let mut failed = false;
    for engine in &dbs_before {
        if !engine.running() {
            eprintln!("✗ FAIL: guarded stop_all stopped adopted {engine:?} — restarting it");
            failed = true;
            let mut fixer = ServiceManager::default();
            fixer.ensure_db(&*plat, *engine).await.expect("restart stopped DB");
        }
    }
    for (minor, port) in &pools_before {
        if !services::fpm_running(*port) {
            eprintln!("✗ FAIL: guarded stop_all stopped adopted php-fpm {minor}");
            failed = true;
        }
    }
    if edge_before && !proxy::admin_alive(&*plat) {
        eprintln!("✗ FAIL: guarded stop_all stopped the edge (launchd should relaunch it)");
        failed = true;
    }
    assert!(!failed, "stack guard failed — see FAIL lines above");
    println!("✓ stop_all left every adopted service serving");
    println!("\nALL CHECKS PASSED — the guard protects the real stack.");
}

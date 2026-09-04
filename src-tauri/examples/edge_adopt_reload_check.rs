//! Manual check: a LIVE edge is adopted, never killed (root cause of the
//! "Caddy stops by itself" reports — see logs/health.log `edge-down` entries).
//!
//! Reproduces the two flows that used to murder a healthy root edge:
//!   1. Start-all with a stale manager state (`prepare_edge` used to
//!      `recover_stale_edge` — i.e. `caddy stop` — before starting fresh).
//!      Now it must ADOPT the live edge + reload it in place: same pid after.
//!   2. A stale edge-down mark left by the watchdog (`reconcile_health` was
//!      one-way running→stopped). Now a pass must RE-ADOPT a live edge and
//!      report it ("adopted"), leaving the UI truthful.
//!
//! Requires the real stack's edge to be RUNNING (Start all in the app first);
//! exits 0 with a skip note otherwise. Deliberately reloads with the CURRENT
//! on-disk Caddyfile, so the reload is a config no-op and the user's sites are
//! never disturbed.

use rexenv_lib::core::{binaries, proxy, service_manager::ServiceManager, ssl};
use rexenv_lib::platform;

/// Pids of a live edge, whichever of the TWO caddys is serving.
///
/// **There are two, and this only knew one** (found 24 Aug 2026 by running the
/// stack tier in the shipped configuration). The user-space edge runs the
/// versioned binary out of the per-user cache; the PRIVILEGED edge — the one
/// that holds :443 on a normal install — runs a root-owned copy at
/// `/Library/Application Support/dev.rexenv.rexenv/bin/caddy`, unversioned.
///
/// That is not an accident to paper over: a root daemon executing a
/// user-writable binary is a standing local privilege escalation, so the
/// daemon's binary MUST live in a root-owned tree
/// (`platform/macos/mod.rs`, `MacosEdgeDaemon`). The path difference is the
/// security property.
///
/// Matching only the user-cache marker meant this check could pass only when the
/// edge was the UNPRIVILEGED one — i.e. never on a normal install. It asserted
/// "the admin socket answers, so a caddy pid must match" and got "no caddy pid
/// found" against an edge that was running perfectly.
fn edge_pids(plat: &dyn rexenv_lib::platform::traits::Platform) -> Vec<u32> {
    let user_space = plat
        .paths()
        .bin_dir()
        .unwrap()
        .join(format!("caddy-{}", binaries::CADDY_VERSION))
        .join("caddy")
        .display()
        .to_string();
    let privileged = plat.edge().daemon_binary_path().display().to_string();
    let mut pids = plat.supervisor().owned_pids(&user_space);
    pids.extend(plat.supervisor().owned_pids(&privileged));
    pids.sort_unstable();
    pids.dedup();
    pids
}

#[tokio::main]
async fn main() {
    let plat = platform::current();

    if !proxy::admin_alive(&*plat) {
        println!("SKIP: no live edge on the admin socket — Start all in the app, then rerun");
        return;
    }
    let pids_before = edge_pids(&*plat);
    assert!(!pids_before.is_empty(), "admin socket answers but no caddy pid found");
    println!("live edge: pid(s) {pids_before:?}");

    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();

    // Fresh manager, like an app relaunch: adopt the survivors (wires `bins`).
    let mut mgr = ServiceManager::default();
    let adopted = mgr.adopt_startup(&*plat, &[], true);
    println!("adopted {adopted} service(s)");
    assert!(adopted > 0, "expected to adopt at least the edge");

    // ── Flow 1: Start-all with a stale "stopped" mark must NOT kill the edge.
    mgr.mark_edge_stopped();
    let caddyfile = plat.paths().config_dir().unwrap().join(proxy::CADDYFILE);
    let plan = mgr.prepare_edge(&*plat, caddyfile).expect("prepare_edge");
    assert!(plan.is_none(), "live edge must be adopted+reloaded, not scheduled for a fresh start");
    assert!(proxy::admin_alive(&*plat), "edge must still answer after adopt+reload");
    let pids_after = edge_pids(&*plat);
    assert_eq!(pids_before, pids_after, "edge pid changed — it was restarted, not adopted");
    println!("prepare_edge adopted the live edge in place (pid unchanged) ✓");

    // ── Flow 2: watchdog pass must heal a stale edge-down mark, not sit on it.
    mgr.mark_edge_stopped();
    let (events, checks) = mgr.reconcile_health(&*plat, &ca, &[]).await;
    for e in &events {
        println!("event: [{}] {}: {}", e.action, e.service, e.detail);
    }
    assert!(checks.is_empty(), "re-adopt must not spawn anything");
    assert!(
        events.iter().any(|e| e.service == "Caddy" && e.action == "adopted"),
        "watchdog must re-adopt a live edge"
    );
    assert!(
        !events.iter().any(|e| e.action == "edge-down"),
        "no edge-down for a live edge"
    );
    assert!(proxy::admin_alive(&*plat), "edge untouched by the watchdog pass");
    assert_eq!(edge_pids(&*plat), pids_before, "edge pid unchanged after watchdog re-adopt");
    println!("watchdog re-adopted the live edge (no restart, truthful status) ✓");

    println!("\nALL CHECKS PASSED — live edge survives Start-all and watchdog passes");
}

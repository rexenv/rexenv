//! Manual check for stale-edge recovery (Phase 2 §7.3).
//! Run: `cargo run --example caddy_recovery_demo`
//!
//! Reproduces the recurring blocker: a LEFTOVER Caddy holding rexenv's admin
//! socket (and the HTTP ports) makes a fresh `caddy run` die with
//! `bind: address already in use`. Starts a stale edge, then shows
//! `proxy::recover_stale_edge` stopping it via OUR admin unix socket (no
//! privilege, never TCP :2019), after which a fresh edge starts cleanly.

use rexenv_lib::core::proxy::{self, CaddyConfig};
use rexenv_lib::core::binaries;
use rexenv_lib::platform;
use std::thread;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let caddy = binaries::resolve(&*plat, "caddy", binaries::CADDY_VERSION).await.unwrap();
    let sock = proxy::admin_socket_path(&*plat).expect("admin socket path");

    // An edge config with no sites — enough to bind the admin socket.
    let caddyfile = proxy::write_caddyfile(&*plat, &CaddyConfig {
        http_port: 8080,
        https_port: 8443,
        routes: Vec::new(),
        admin_socket: Some(sock),
    }).unwrap();

    // Clean slate: clear any pre-existing stray first.
    proxy::recover_stale_edge(&*plat, &caddy).expect("pre-clean");

    // 1) Start a "stale" leftover edge (holds the admin socket).
    let mut stale = proxy::start(&*plat, &caddy, &caddyfile).expect("start stale edge");
    thread::sleep(Duration::from_millis(1000));
    let stale_up = proxy::admin_alive(&*plat);
    println!("stale edge holding the admin socket = {stale_up}");

    // 2) Recover: stop the stale edge via the admin socket (no privilege).
    proxy::recover_stale_edge(&*plat, &caddy).expect("recover");
    let _ = stale.wait(); // reap the stopped process
    let freed = !proxy::admin_alive(&*plat);
    println!("after recover_stale_edge → admin socket free = {freed}");

    // 3) A fresh edge now starts cleanly (previously: bind: address already in use).
    let mut fresh = proxy::start(&*plat, &caddy, &caddyfile).expect("start fresh edge");
    thread::sleep(Duration::from_millis(1000));
    let fresh_up = proxy::admin_alive(&*plat);
    println!("fresh edge started, admin socket alive = {fresh_up}");

    // Cleanup.
    let _ = proxy::stop(&*plat, fresh.id());
    let _ = fresh.wait();
    proxy::recover_stale_edge(&*plat, &caddy).ok();

    if stale_up && freed && fresh_up {
        println!("\nOK — a leftover edge is detected + stopped via our admin socket; a fresh edge starts cleanly.");
    } else {
        eprintln!("\nFAILED — stale_up={stale_up} freed={freed} fresh_up={fresh_up}");
        std::process::exit(1);
    }
}

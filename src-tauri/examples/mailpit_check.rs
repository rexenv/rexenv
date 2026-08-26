//! Phase-3 §2.1 check: Mailpit binary provider + lifecycle.
//! Resolves Mailpit via `BinaryProvider` (sign + de-quarantine), runs
//! `mailpit version`, starts it via `ProcessSupervisor::spawn_logged`, then
//! verifies the SMTP port (:11025) accepts a TCP connection AND the HTTP API
//! (:18025 `GET /api/v1/messages`) responds.
//!
//! Run: `cargo run --example mailpit_check`

#[path = "common/mod.rs"]
mod common;

use rexenv_lib::core::{binaries, mail};
use rexenv_lib::platform;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

#[tokio::main]
async fn main() {
    // Refuse beside a live stack: these services would JOIN it, not collide
    // with it, and a connect-based readiness gate is satisfied by the user's
    // server. FIRST statement — after anything is spawned, exiting leaks it.
    common::require_stack_stopped();
    let plat = platform::current();

    // Resolve (download + verify + sign) the Mailpit binary.
    let bin = binaries::resolve(&*plat, "mailpit", binaries::MAILPIT_VERSION)
        .await
        .expect("resolve mailpit");
    println!("✓ resolved {}", bin.display());

    // `mailpit version` runs (binary is executable + signed).
    let out = std::process::Command::new(&bin).arg("version").output().unwrap();
    assert!(out.status.success(), "mailpit version failed");
    println!("✓ {}", String::from_utf8_lossy(&out.stdout).trim());

    // Start it (loopback SMTP + HTTP, persistent DB).
    let mut child = common::OwnedService::new(mail::start(&*plat, &bin).expect("start mailpit"), "mailpit");
    let mut up = false;
    for _ in 0..40 {
        if mail::running() {
            up = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    assert!(up, "mailpit HTTP port never came up");
    println!("✓ mailpit running on http {}", mail::MAILPIT_HTTP_PORT);

    // SMTP port accepts a connection.
    let smtp = SocketAddr::from((Ipv4Addr::LOCALHOST, mail::MAILPIT_SMTP_PORT));
    TcpStream::connect_timeout(&smtp, Duration::from_millis(500))
        .expect("connect mailpit SMTP :11025");
    println!("✓ SMTP :{} accepts connections", mail::MAILPIT_SMTP_PORT);

    // HTTP API responds.
    let url = format!("{}/api/v1/messages", mail::api_base());
    let resp = reqwest::get(&url).await.expect("GET messages");
    assert!(resp.status().is_success(), "API status {}", resp.status());
    let body = resp.text().await.unwrap();
    println!("✓ GET /api/v1/messages → {} ({} bytes)", 200, body.len());

    child.stop();
    println!("\nALL GOOD — Mailpit resolves, runs, and serves SMTP + HTTP API.");
}

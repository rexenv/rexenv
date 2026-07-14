//! Manual check: Redis via the FIRST Homebrew-bottle bundle (TODO "Deferred
//! services"). Run: `cargo run --example redis_bundle_check`
//!
//! Proves the whole bundle pipeline end to end on the REAL binary cache:
//!   1. `resolve_bundle("redis")` — two ghcr bottles download + verify, merge,
//!      relink to `@loader_path`, re-sign, publish atomically.
//!   2. Every published Mach-O carries ONLY system / `@loader_path` load
//!      commands (no `@@HOMEBREW_*@@` placeholder survives) + a valid ad-hoc
//!      signature.
//!   3. `DbEngine::Redis.start()` serves on :16379 — PING and a SET/GET
//!      round-trip through the bundled `redis-cli` (itself relinked).
//! Cleans up its own child only — never touches the running stack.

use rexenv_lib::core::db::DbEngine;
use rexenv_lib::core::{binaries, ports, redis};
use rexenv_lib::platform;
use std::process::Command;
use std::thread;
use std::time::Duration;

fn load_commands_clean(path: &std::path::Path) -> bool {
    let out = Command::new("otool").arg("-L").arg(path).output().expect("otool");
    let listing = String::from_utf8_lossy(&out.stdout);
    let mut clean = true;
    for line in listing.lines().skip(1) {
        let Some(dep) = line.split_whitespace().next() else { continue };
        let ok = dep.starts_with("/usr/lib/")
            || dep.starts_with("/System/")
            || dep.starts_with("@loader_path/");
        if !ok {
            eprintln!("  UNRESOLVED load command in {}: {dep}", path.display());
            clean = false;
        }
    }
    clean
}

fn signature_valid(path: &std::path::Path) -> bool {
    Command::new("codesign")
        .args(["--verify", "--strict"])
        .arg(path)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn cli(basedir: &std::path::Path, port: u16, args: &[&str]) -> String {
    let out = Command::new(redis::redis_cli_bin(basedir))
        .args(["-h", "127.0.0.1", "-p", &port.to_string()])
        .args(args)
        .output()
        .expect("run redis-cli");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let mut ok = true;

    println!("=== resolve_bundle(redis {}) ===", binaries::REDIS_VERSION);
    let basedir = binaries::resolve_bundle(&*plat, "redis", binaries::REDIS_VERSION)
        .await
        .expect("resolve redis bundle");
    println!("  published at {}", basedir.display());

    println!("\n=== relink + signature audit ===");
    for rel in [
        "bin/redis-server",
        "bin/redis-cli",
        "lib/libssl.3.dylib",
        "lib/libcrypto.3.dylib",
    ] {
        let path = basedir.join(rel);
        let exists = path.is_file();
        let clean = exists && load_commands_clean(&path);
        let signed = exists && signature_valid(&path);
        println!("  {rel:<24} exists={exists} loads-clean={clean} signed={signed}");
        ok &= exists && clean && signed;
    }

    println!("\n=== DbEngine::Redis lifecycle ===");
    let port = DbEngine::Redis.port();
    if let Err(e) = ports::ensure_free(&*plat, port, ports::Proto::Tcp, "Redis") {
        eprintln!("  port {port} busy — {e}");
        std::process::exit(1);
    }
    let mut child = DbEngine::Redis
        .start(&*plat, DbEngine::Redis.default_version())
        .await
        .expect("start redis");
    let pid = child.id();
    let mut up = false;
    for _ in 0..40 {
        if DbEngine::Redis.running() {
            up = true;
            break;
        }
        thread::sleep(Duration::from_millis(250));
    }
    println!("  pid {pid} · listening on :{port} = {up}");
    ok &= up;

    if up {
        let pong = cli(&basedir, port, &["PING"]);
        let set = cli(&basedir, port, &["SET", "rexenv:check", "bundle-works"]);
        let got = cli(&basedir, port, &["GET", "rexenv:check"]);
        let _ = cli(&basedir, port, &["DEL", "rexenv:check"]);
        println!("  PING → {pong} · SET → {set} · GET → {got}");
        ok &= pong == "PONG" && set == "OK" && got == "bundle-works";
    }

    let _ = DbEngine::Redis.stop(&*plat, pid);
    let _ = child.wait();
    println!("  stopped");

    if ok {
        println!("\nOK — bottle bundle resolved, relinked, signed; Redis served and answered.");
    } else {
        eprintln!("\nFAILED — see above.");
        std::process::exit(1);
    }
}

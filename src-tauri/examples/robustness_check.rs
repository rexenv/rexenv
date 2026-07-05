//! Release §2 check: failure paths produce clear errors, never a crash/hang.
//!
//! Exercises the non-interactive failure modes live:
//!   - 2.1 a busy port → `ensure_free` errors naming the port + service;
//!   - 2.2 a 404 download → a clear PERMANENT error (no pointless retries), and the
//!     write-after-verify flow means nothing is cached;
//!   - 2.4 no route to host → a connectivity-aware error ("check your internet
//!     connection"), returned within a BOUNDED time (retries are capped, the
//!     connect timeout prevents a hang).
//!
//! (2.3 cancelled-privilege messaging is covered by the macOS unit test
//! `privileged_cancel_is_a_friendly_recoverable_message`.)
//!
//! Run: `cargo run --example robustness_check`

use rexenv_lib::core::{binaries, ports};
use rexenv_lib::platform;
use std::net::{Ipv4Addr, TcpListener};
use std::time::Instant;

#[tokio::main]
async fn main() {
    // 2.1 — busy port.
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let plat = platform::current();
    let err = ports::ensure_free(&*plat, port, ports::Proto::Tcp, "edge").unwrap_err().to_string();
    println!("2.1 busy port → {err}");
    assert!(err.contains(&port.to_string()) && err.contains("edge") && err.contains("in use"));
    drop(listener);
    println!("✓ 2.1 busy port: clear error naming the port + service\n");

    // 2.2 — 404 download (reachable host, missing path) → permanent, fast.
    let t = Instant::now();
    let r = binaries::http_get("https://github.com/rexenv/does-not-exist-zzz/raw/x").await;
    let took = t.elapsed();
    match r {
        Ok(_) => panic!("expected a 404 failure"),
        Err(e) => {
            let m = e.to_string();
            println!("2.2 404 → {m}  ({}ms)", took.as_millis());
            assert!(m.contains("failed"), "msg: {m}");
            // 4xx is permanent: it must NOT spend the full retry/backoff budget.
            assert!(took.as_secs() < 10, "404 took too long ({took:?}) — should not retry");
        }
    }
    println!("✓ 2.2 download 404: clear permanent error, no wasted retries (nothing cached — write happens only after checksum verify)\n");

    // 2.4 — unreachable host (simulates no internet) → connectivity hint, bounded.
    let t = Instant::now();
    let r = binaries::http_get("https://rexenv-offline.invalid/php.tar.gz").await;
    let took = t.elapsed();
    match r {
        Ok(_) => panic!("expected a connectivity failure"),
        Err(e) => {
            let m = e.to_string();
            println!("2.4 no route → {m}  ({}ms)", took.as_millis());
            assert!(
                m.contains("internet") || m.contains("reach"),
                "expected a connectivity hint, got: {m}"
            );
            assert!(m.contains("gave up after"), "expected the bounded-retry note, got: {m}");
            // Bounded: capped retries + connect timeout — must not hang for minutes.
            assert!(took.as_secs() < 90, "no-internet path took too long ({took:?})");
        }
    }
    println!("✓ 2.4 no internet: connectivity-aware error, bounded retries (no hang)\n");

    println!("ALL GOOD — busy port, failed download (404), and no-internet each fail clean + clear; no crash, no hang.");
}

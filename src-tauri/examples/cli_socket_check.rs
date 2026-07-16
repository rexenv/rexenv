//! Live check: the `rex` CLI socket server against the REAL app data.
//!
//!   cargo run --example cli_socket_check
//!
//! Boots the app's state the way `lib::run` does (real SQLite, real CA, real
//! platform, adopt running survivors) inside a MockRuntime app — no GUI — and
//! serves the real dispatch on the REAL socket path. Then self-tests over the
//! socket (status round-trip, garbage request, unknown cmd, 0600 perms) and
//! keeps serving for 30s so `rex status` can be run against it by hand.
//!
//! Read-only by construction: it only ever dispatches `status` (adoption
//! mutates nothing; adopted `Proc`s are Drop-safe). The stack guard stays
//! CLOSED — this example cannot stop anything it didn't spawn.

use rexenv_lib::cli_server;
use rexenv_lib::state::app::{AppState, DnsMode, DnsState};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use tauri::Manager;

fn ask(path: &std::path::Path, line: &str) -> Value {
    let mut s = UnixStream::connect(path).expect("connect");
    s.write_all(format!("{line}\n").as_bytes()).expect("write");
    let mut reply = String::new();
    BufReader::new(s).read_line(&mut reply).expect("read");
    serde_json::from_str(reply.trim()).expect("valid JSON envelope")
}

#[tokio::main]
async fn main() {
    let platform = rexenv_lib::platform::current();
    let conn = rexenv_lib::state::db::open_for_platform(platform.paths()).expect("open app db");
    let ca = rexenv_lib::core::ssl::load_or_create(platform.paths(), platform.permissions())
        .expect("load CA");
    let sites = rexenv_lib::core::sites::list(&conn).unwrap_or_default();

    let sock = platform.paths().config_dir().expect("config dir").join(cli_server::SOCKET_FILE);
    // Refuse to steal a LIVE socket (a new-build app is running): connect tells
    // the truth, a stale file from a crash refuses and is safe to replace.
    if UnixStream::connect(&sock).is_ok() {
        panic!("the app's CLI socket is live at {} — quit the app first", sock.display());
    }

    let app = tauri::test::mock_app();
    // Display-only mode label: liveness in dns_status is a real wire probe
    // either way; this example doesn't know agent vs in-process.
    app.manage(DnsState::new(None, DnsMode::Agent));
    let state = AppState::new(conn, platform, ca);
    let adopted = {
        let mut mgr = state.services.lock().await;
        mgr.adopt_startup(state.platform.as_ref(), &sites)
    };
    println!("adopted {adopted} running service(s) (guard closed — nothing stoppable)");
    app.manage(state);

    let listener = cli_server::bind(&sock).expect("bind CLI socket");
    let mode = std::fs::metadata(&sock).expect("sock meta").permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "socket must be 0600");
    let handle = app.handle().clone();
    tokio::spawn(cli_server::serve(listener, move |line| {
        let handle = handle.clone();
        async move { cli_server::handle_request(&handle, line).await }
    }));

    // 1) status round-trip — real dispatch, real ServiceManager snapshot.
    let v = ask(&sock, "{\"cmd\":\"status\"}");
    assert_eq!(v["ok"], true, "status must succeed: {v}");
    let services = v["data"]["services"].as_array().expect("services array");
    let running = services.iter().filter(|s| s["running"] == true).count();
    println!("status: {} services, {running} running", services.len());
    println!(
        "dns: answering={} mode={}",
        v["data"]["dns"]["running"], v["data"]["dns"]["mode"]
    );
    assert!(!services.is_empty(), "expected at least the core service rows");

    // 2) garbage + unknown cmd → error envelopes, never a hang or panic.
    let v = ask(&sock, "this is not json");
    assert_eq!(v["ok"], false);
    let v = ask(&sock, "{\"cmd\":\"nope\"}");
    assert_eq!(v["ok"], false);
    assert!(v["error"].as_str().unwrap().contains("unknown command"));

    println!("✓ self-test green — serving 30s for a manual `rex status` …");
    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    let _ = std::fs::remove_file(&sock);
    println!("✓ cli_socket_check done (socket removed)");
}

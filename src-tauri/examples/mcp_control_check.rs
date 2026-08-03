//! Live check: the MCP endpoint's OFF switch — the half SMOKE step 4 is a HOLD for.
//!
//!   cargo run --example mcp_control_check
//!
//! #203's open leg. The easy version of this check — enable, `stat` the socket,
//! disable, `connect` fails — would pass on an endpoint that is still serving
//! every agent already attached to it, and on one whose socket node is still
//! sitting on disk advertising a dead listener. Neither is what "turn it off"
//! has to mean for a local API surface that any process running as the user can
//! reach. So this asserts the two things that version skips:
//!
//! 1. **A session holding an OPEN connection is dropped.** The check connects,
//!    completes a handshake, makes a successful call, and *keeps the stream
//!    open* across the disable. Its next read must reach EOF. A server that only
//!    stopped accepting NEW connections leaves the already-attached agent — the
//!    one an injected prompt is already talking to — working exactly as before,
//!    which is the state the toggle claims to end.
//! 2. **The socket FILE is gone, not merely closed.** A closed listener with the
//!    node still on disk reads as an endpoint to anything that stats it, blocks
//!    a later clean rebind, and leaves `ls -l` telling the user something that
//!    isn't true. `stat` is the assertion; `connect` alone is not.
//!
//! Then it re-enables and proves the endpoint comes back on a fresh socket, so
//! "off" is a state you can leave rather than a one-way door.
//!
//! # Fixture ownership (`examples/common/mod.rs`)
//!
//! Sandboxed `Platform` + sandbox database, and the socket lives under the
//! sandbox root — never the app's own. That is safe *because* `serve` now
//! unlinks the path its listener actually bound rather than a re-derived one; a
//! re-derived path is how an example deletes the running app's socket, which is
//! the class this project has already been bitten by three times.
//!
//! This drives `mcp_server::bind_socket`/`serve` directly rather than the
//! `mcp_set_enabled` command, because that command needs a real `AppHandle` with
//! managed state. What it therefore does NOT cover: the command's own
//! bind-before-flip ordering (an L0 test) and the packaged toggle (SMOKE 2/4).

use rexenv_lib::state::app::AppState;
use rexenv_lib::{core, mcp_server};
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use tauri::Manager;

mod common;

fn send(stream: &mut UnixStream, msg: &str) {
    stream.write_all(msg.as_bytes()).expect("write");
    stream.write_all(b"\n").expect("newline");
    stream.flush().expect("flush");
}

fn read_reply(reader: &mut impl BufRead) -> Value {
    let mut line = String::new();
    reader.read_line(&mut line).expect("read reply");
    serde_json::from_str(line.trim()).expect("valid JSON-RPC")
}

/// Handshake + one successful call, leaving the connection OPEN.
fn open_session(sock: &Path, tag: &str) -> (UnixStream, BufReader<UnixStream>) {
    let mut stream = UnixStream::connect(sock).expect("connect to the endpoint");
    let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
    send(
        &mut stream,
        &format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-11-25","capabilities":{{}},"clientInfo":{{"name":"{tag}","version":"1"}}}}}}"#
        ),
    );
    assert_eq!(read_reply(&mut reader)["result"]["serverInfo"]["name"], "rexenv");
    send(&mut stream, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
    send(
        &mut stream,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_sites","arguments":{}}}"#,
    );
    let v = read_reply(&mut reader);
    assert!(v["result"]["isError"].is_null(), "the session must be working before we disable: {v}");
    (stream, reader)
}

#[tokio::main]
async fn main() {
    let (plat, _sandbox) = common::sandbox("mcp_control_check");
    let root = plat.paths().app_data_dir().expect("sandbox data dir");
    let conn = rexenv_lib::state::db::open_for_platform(plat.paths()).expect("open sandbox db");
    let ca = core::ssl::load_or_create(plat.paths(), plat.permissions()).expect("sandbox CA");
    let app = tauri::test::mock_app();
    app.manage(AppState::new(conn, plat, ca));

    let sock = root.join(mcp_server::SOCKET_FILE);
    assert!(!sock.exists(), "the sandbox starts with no socket — default off");

    // ---- ON -------------------------------------------------------------
    let listener = mcp_server::bind_socket(&sock).expect("bind");
    assert_eq!(
        std::fs::metadata(&sock).expect("socket metadata").permissions().mode() & 0o777,
        0o600,
        "the socket is 0600 (#198/#55) — filesystem perms ARE the auth"
    );
    let (shutdown, rx) = tokio::sync::watch::channel(true);
    let served = tokio::spawn(mcp_server::serve(listener, app.handle().clone(), rx));
    println!("✓ enable → socket bound 0600 at the sandbox path");

    // Two sessions: one we hold OPEN across the disable, one that proves the
    // endpoint is genuinely serving more than a single caller beforehand.
    let (mut held, mut held_reader) = open_session(&sock, "mcp_control_check/held");
    let (_second, _second_reader) = open_session(&sock, "mcp_control_check/second");
    println!("✓ two live sessions, each handshaken and answering calls");

    // ---- OFF ------------------------------------------------------------
    // The toggle's disable is exactly this: signal the watch channel that
    // `McpControl::stop` holds.
    shutdown.send(false).expect("signal shutdown");
    // Bounded wait for the accept loop to finish — never an unbounded await, so
    // a regression fails the run instead of hanging it.
    tokio::time::timeout(std::time::Duration::from_secs(5), served)
        .await
        .expect("the serve task must stop within 5s of disable")
        .expect("serve task panicked");

    // (1) THE HELD SESSION IS DROPPED. This is the assertion the naive version
    //     skips: the connection was open and working before the disable, so a
    //     server that merely stopped accepting would leave it answering. Read to
    //     EOF — zero bytes is the socket closed from the far end.
    // Bounded at the TASK, not with SO_RCVTIMEO (which macOS rejects on this
    // socket): if the session was NOT dropped, this read blocks forever and the
    // timeout turns a regression into a failure rather than a hung run.
    let (n, rest, mut held_reader) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::task::spawn_blocking(move || {
            let mut rest = Vec::new();
            let n = held_reader.get_mut().read_to_end(&mut rest).unwrap_or(0);
            (n, rest, held_reader)
        }),
    )
    .await
    .expect("the held session did not reach EOF within 5s — it was NOT dropped by disable")
    .expect("read task panicked");
    assert_eq!(n, 0, "the held session was not dropped — it read {n} more byte(s): {rest:?}");
    // …and it cannot be used again. Which way that shows up is platform detail:
    // on macOS the WRITE fails outright (EPIPE, observed) because the peer is
    // gone; elsewhere the write may buffer and the read returns EOF instead.
    // Both mean dropped, so assert the disjunction — picking one and treating
    // the other as a failure would make this check a portability trap rather
    // than a claim about rexenv.
    let wrote = held
        .write_all(br#"{"jsonrpc":"2.0","id":9,"method":"ping"}"#)
        .and_then(|_| held.write_all(b"\n"))
        .and_then(|_| held.flush());
    match wrote {
        Err(e) => println!("  (writing to the dropped session failed outright: {e})"),
        Ok(()) => {
            let mut line = String::new();
            assert_eq!(
                held_reader.read_line(&mut line).unwrap_or(0),
                0,
                "a dropped session still answered `ping`: {line}"
            );
        }
    }
    println!("✓ disable → the session that was ALREADY CONNECTED is dropped (EOF, and no reply to ping)");

    // (2) THE SOCKET FILE IS GONE — not merely closed to new callers. `connect`
    //     failing is the weaker fact and would also hold for a live-but-closed
    //     listener whose node is still on disk, which still reads as an endpoint
    //     to anything that stats it and blocks a clean rebind.
    assert!(
        !sock.exists(),
        "the socket FILE is still at {} after disable — a closed listener with the node on disk \
         still advertises an endpoint",
        sock.display()
    );
    assert!(UnixStream::connect(&sock).is_err(), "a new connection still succeeded after disable");
    println!("✓ disable → the socket FILE is unlinked, and a new connect fails");

    // ---- ON AGAIN -------------------------------------------------------
    // Off must be a state you can leave: the same path rebinds cleanly (which
    // the unlink above is what makes true) and serves a fresh session.
    let listener = mcp_server::bind_socket(&sock).expect("re-bind after disable");
    let (shutdown2, rx2) = tokio::sync::watch::channel(true);
    let served2 = tokio::spawn(mcp_server::serve(listener, app.handle().clone(), rx2));
    let (_again, _again_reader) = open_session(&sock, "mcp_control_check/again");
    println!("✓ re-enable → rebinds on the same path and serves a new session");

    shutdown2.send(false).expect("signal shutdown");
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), served2).await;
    assert!(!sock.exists(), "the socket outlived the second disable");

    println!(
        "✓ mcp_control_check green — disable drops a session that was ALREADY CONNECTED (not just \
         new ones) and UNLINKS the socket file (not just closes it), and the endpoint rebinds \
         cleanly afterwards. Not covered here: the `mcp_set_enabled` command's bind-before-flip \
         ordering (L0) and the packaged toggle (SMOKE steps 2 and 4)."
    );
}

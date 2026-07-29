//! Live check: the MCP server handshake over the REAL socket path.
//!
//!   cargo run --example mcp_socket_check
//!
//! Binds the app's MCP socket (guarding a live app the cli_socket_check way),
//! serves the real `mcp_server`, then drives a SPEC-LITERAL MCP handshake over
//! it — initialize, initialized, tools/list, ping — asserting a valid server
//! that exposes zero tools. The bytes sent are written to match the MCP spec's
//! own message shapes, NOT round-tripped through our own types, so this catches
//! the our-encoder-agrees-with-our-decoder trap the plan (§8) warns about. It
//! does NOT prove the real Claude Code client — that is the manual
//! `claude mcp add rexenv -- rex mcp` step.
//!
//! Read-only by construction: an M1 zero-tool server has nothing to execute and
//! touches no app state, so there is no fixture to own and nothing to reap.

use rexenv_lib::{cli_server, mcp_server};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;

fn send(stream: &mut UnixStream, msg: &str) {
    stream.write_all(msg.as_bytes()).expect("write message");
    stream.write_all(b"\n").expect("write newline");
    stream.flush().expect("flush");
}

fn read_reply(reader: &mut impl BufRead) -> Value {
    let mut line = String::new();
    reader.read_line(&mut line).expect("read reply line");
    serde_json::from_str(line.trim()).expect("reply is valid JSON-RPC")
}

#[tokio::main]
async fn main() {
    let platform = rexenv_lib::platform::current();
    let sock =
        platform.paths().config_dir().expect("config dir").join(mcp_server::SOCKET_FILE);

    // Refuse to steal a LIVE app's MCP socket — connect tells the truth; a stale
    // file from a crash refuses and is safe to replace (the cli_socket_check guard).
    if UnixStream::connect(&sock).is_ok() {
        panic!("the app's MCP socket is live at {} — quit the app first", sock.display());
    }

    // Bind with the SAME convention as the CLI socket (one convention, not two),
    // then serve the real M1 server.
    let listener = cli_server::bind(&sock).expect("bind MCP socket");
    let mode = std::fs::metadata(&sock).expect("socket metadata").permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "socket must be 0600");
    tokio::spawn(mcp_server::serve(listener));

    let mut stream = UnixStream::connect(&sock).expect("connect to MCP socket");
    let mut reader = BufReader::new(stream.try_clone().expect("clone stream for reads"));

    // 1) initialize — the shape a real MCP client sends first.
    send(
        &mut stream,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"mcp_socket_check","version":"1"}}}"#,
    );
    let v = read_reply(&mut reader);
    assert_eq!(v["jsonrpc"], "2.0", "reply: {v}");
    assert_eq!(v["id"], 1, "reply: {v}");
    assert_eq!(v["result"]["serverInfo"]["name"], "rexenv", "reply: {v}");
    assert!(v["result"]["serverInfo"]["version"].is_string(), "reply: {v}");
    assert!(v["result"]["capabilities"]["tools"].is_object(), "must advertise tools: {v}");
    assert_eq!(v["result"]["protocolVersion"], "2025-11-25", "reply: {v}");
    println!("✓ initialize → rexenv {}", v["result"]["serverInfo"]["version"]);

    // 2) initialized notification — no reply must come back.
    send(&mut stream, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);

    // 3) tools/list — empty in M1 (the contained, no-execution surface).
    send(&mut stream, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
    let v = read_reply(&mut reader);
    assert_eq!(v["id"], 2, "reply: {v}");
    assert_eq!(v["result"]["tools"], json!([]), "M1 exposes zero tools: {v}");
    println!("✓ tools/list → [] (zero tools, contained)");

    // 4) ping — an empty result.
    send(&mut stream, r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#);
    let v = read_reply(&mut reader);
    assert_eq!(v["result"], json!({}), "reply: {v}");
    println!("✓ ping → {{}}");

    let _ = std::fs::remove_file(&sock);
    println!(
        "✓ mcp_socket_check green — spec-literal handshake OK. \
         Real-client check: `claude mcp add rexenv -- rex mcp` (manual, §8)."
    );
}

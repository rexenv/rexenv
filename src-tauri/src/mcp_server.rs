//! App-side MCP (Model Context Protocol) server: a private unix-socket JSON-RPC
//! endpoint an AI agent's client drives through the dumb `rex mcp` pipe.
//!
//! Like `cli_server`, this is remote control of THIS process only — the `cli/`
//! shim never links the app lib; it copies bytes between the client's stdio and
//! our socket. Once tools land they execute through the same `commands::*` fns
//! the UI calls, so there is one brain (M1-T1 exposes ZERO tools — just the
//! handshake, which is contained because there is nothing to execute).
//!
//! **No SDK** (docs/PLAN-mcp-server.md §2.1): every `rmcp` is edition 2024
//! (rustc ≥ 1.85) and the repo pins 1.77.2, so a proposed feature would raise
//! the toolchain floor as a side effect — and rmcp's whole tree cuts against
//! this codebase's dependency-closure discipline. M1 needs only MCP's stable,
//! boring core, so it is hand-rolled on `serde_json` (already a dep). We
//! implement the **2025-11-25** core: `initialize`, `notifications/initialized`,
//! `tools/list`, `tools/call`, `ping`. Framing is newline-delimited JSON-RPC 2.0
//! — one message per line, no embedded newlines (the MCP stdio transport, piped
//! through the shim verbatim). Owning the protocol means owning its
//! compatibility, so the handshake is proven against a REAL client, not just our
//! own encoder (`examples/mcp_socket_check.rs` speaks a spec-literal handshake;
//! the manual `claude mcp add` check closes the last gap — §8).
//!
//! Socket: `<config>/rexenv-mcp.sock`, `0600`, bound with the SAME convention as
//! the CLI socket — `cli_server::bind`, deliberately reused so there is one
//! socket convention, not two. Never TCP.

use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

pub const SOCKET_FILE: &str = "rexenv-mcp.sock";

/// The MCP revision whose stable JSON-RPC core we implement (docs/PLAN §2.1).
/// Returned when the client requests a version we do not recognise.
const PROTOCOL_VERSION: &str = "2025-11-25";

/// Versions whose `initialize`/`tools` core is identical to ours — we echo the
/// client's requested version when it is one of these (spec: respond with the
/// same version if supported, else one we do support).
const SUPPORTED_PROTOCOLS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// Cap on one JSON-RPC message line. M1 messages are tiny; this only bounds a
/// same-user client that streams bytes without a newline (the `cli_server` B17
/// lesson) — an over-long line closes the session rather than buffering forever.
const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

/// Spawn the MCP listener at app startup. Failure is logged, never fatal — the
/// app works without its MCP endpoint, exactly like the CLI socket.
pub fn spawn() {
    let path = match crate::platform::current().paths().config_dir() {
        Ok(dir) => dir.join(SOCKET_FILE),
        Err(e) => {
            log::error!("mcp: no config dir for the socket: {e}");
            return;
        }
    };
    tauri::async_runtime::spawn(async move {
        // Reuse the CLI socket's bind: 0600, stale-file unlink, connect-to-detect
        // liveness — one socket convention across both sockets, not two.
        let listener = match crate::cli_server::bind(&path) {
            Ok(l) => l,
            Err(e) => {
                log::error!("mcp: could not bind {}: {e}", path.display());
                return;
            }
        };
        log::info!("mcp: listening on {}", path.display());
        serve(listener).await;
    });
}

/// Accept loop: one long-lived MCP session per connection (unlike the CLI
/// socket's one-request-per-connection). Each session runs on its own task.
/// Public so the `mcp_socket_check` example can serve it over the real socket.
pub async fn serve(listener: UnixListener) {
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue; // transient accept error; the socket stays bound
        };
        tokio::spawn(async move {
            let (read, write) = stream.into_split();
            session(read, write).await;
        });
    }
}

/// One MCP session: read newline-delimited JSON-RPC messages, dispatch each,
/// write a reply for every request (never for a notification), until EOF.
/// Generic over the byte streams so it is testable without a socket.
async fn session<R, W>(read: R, mut write: W)
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut reader = BufReader::new(read);
    let mut buf = Vec::new();
    while read_line_capped(&mut reader, &mut buf, MAX_LINE_BYTES).await.is_some() {
        let text = String::from_utf8_lossy(&buf);
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(response) = handle_message(trimmed) {
            if write.write_all(response.as_bytes()).await.is_err()
                || write.write_all(b"\n").await.is_err()
                || write.flush().await.is_err()
            {
                break; // client went away mid-reply
            }
        }
    }
}

/// Read one newline-delimited message into `buf` (newline stripped), bounded by
/// `cap`. `None` ends the session: EOF with nothing buffered, an over-long line,
/// or a read error. A final line with no trailing newline is still delivered.
async fn read_line_capped<R>(reader: &mut R, buf: &mut Vec<u8>, cap: usize) -> Option<()>
where
    R: AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;
    buf.clear();
    let mut byte = [0u8; 1];
    loop {
        match reader.read(&mut byte).await {
            Ok(0) => return if buf.is_empty() { None } else { Some(()) },
            Ok(_) => {
                if byte[0] == b'\n' {
                    return Some(());
                }
                if buf.len() >= cap {
                    return None; // refuse an unbounded line rather than buffer it
                }
                buf.push(byte[0]);
            }
            Err(_) => return None,
        }
    }
}

/// Dispatch one JSON-RPC message. `Some(json line)` for a request (a result or
/// an error, echoing the request `id`); `None` for a notification or an
/// unparseable notification-shaped message (nothing to reply to). A message we
/// cannot parse at all is answered with a parse error at `id: null`.
fn handle_message(text: &str) -> Option<String> {
    let msg: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        // JSON-RPC: a parse error is reported with a null id.
        Err(_) => return Some(error_response(Value::Null, -32700, "parse error")),
    };
    // Absence of `id` marks a notification — never answered.
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    match (method, id) {
        ("initialize", Some(id)) => Some(result_response(id, initialize_result(&msg))),
        ("ping", Some(id)) => Some(result_response(id, json!({}))),
        ("tools/list", Some(id)) => Some(result_response(id, json!({ "tools": [] }))),
        // M1 exposes no tools, so every tools/call names an unknown tool.
        ("tools/call", Some(id)) => {
            let name = msg.pointer("/params/name").and_then(Value::as_str).unwrap_or("");
            Some(error_response(id, -32602, &format!("unknown tool: {name}")))
        }
        // Notifications (no id): `initialized` and anything else — no reply.
        (_, None) => None,
        // Any other request method.
        (other, Some(id)) => {
            Some(error_response(id, -32601, &format!("method not found: {other}")))
        }
    }
}

/// The `initialize` result: advertise the tools capability (so clients call
/// `tools/list`), our name and version, and the negotiated protocol version —
/// echoing the client's when we recognise it, else our own.
fn initialize_result(msg: &Value) -> Value {
    let requested = msg.pointer("/params/protocolVersion").and_then(Value::as_str);
    let version = match requested {
        Some(v) if SUPPORTED_PROTOCOLS.contains(&v) => v,
        _ => PROTOCOL_VERSION,
    };
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "rexenv", "version": env!("CARGO_PKG_VERSION") }
    })
}

fn result_response(id: Value, result: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

fn error_response(id: Value, code: i64, message: &str) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    fn parse(line: &str) -> Value {
        serde_json::from_str(line).expect("handler emits valid JSON")
    }

    #[test]
    fn initialize_returns_server_info_and_the_tools_capability() {
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"probe","version":"0"}}}"#;
        let v = parse(&handle_message(req).expect("initialize is a request"));
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 1);
        assert_eq!(v["result"]["serverInfo"]["name"], "rexenv");
        assert!(v["result"]["serverInfo"]["version"].is_string());
        // Advertising the tools capability is what makes a client call tools/list.
        assert!(v["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn initialize_echoes_a_supported_version_and_falls_back_otherwise() {
        for (asked, want) in [
            ("2025-11-25", "2025-11-25"),
            ("2024-11-05", "2024-11-05"), // an older but core-compatible revision
            ("2099-01-01", PROTOCOL_VERSION), // unknown → our own version
        ] {
            let req = format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"{asked}"}}}}"#
            );
            let v = parse(&handle_message(&req).unwrap());
            assert_eq!(v["result"]["protocolVersion"], want, "asked {asked}");
        }
    }

    #[test]
    fn tools_list_is_empty_and_ping_is_an_empty_result_in_m1() {
        let v = parse(&handle_message(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#).unwrap());
        assert_eq!(v["result"]["tools"], json!([]));
        let v = parse(&handle_message(r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#).unwrap());
        assert_eq!(v["result"], json!({}));
    }

    #[test]
    fn notifications_are_never_answered() {
        assert!(handle_message(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
        assert!(handle_message(r#"{"jsonrpc":"2.0","method":"notifications/anything"}"#).is_none());
    }

    #[test]
    fn unknown_method_and_any_tool_call_error_by_id_never_panic() {
        let v = parse(&handle_message(r#"{"jsonrpc":"2.0","id":4,"method":"resources/list"}"#).unwrap());
        assert_eq!(v["id"], 4);
        assert_eq!(v["error"]["code"], -32601);
        // Zero tools in M1: any call is an unknown tool, never an execution.
        let call = r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"list_sites"}}"#;
        let v = parse(&handle_message(call).unwrap());
        assert_eq!(v["error"]["code"], -32602);
        assert!(v["error"]["message"].as_str().unwrap().contains("list_sites"));
    }

    #[test]
    fn garbage_is_a_parse_error_at_null_id_not_a_crash() {
        let v = parse(&handle_message("this is not json").unwrap());
        assert_eq!(v["id"], Value::Null);
        assert_eq!(v["error"]["code"], -32700);
    }

    #[tokio::test]
    async fn a_session_replies_to_requests_and_stays_silent_on_notifications() {
        // Two requests and one notification over one connection: exactly two
        // reply lines come back, in order, and the notification is silent.
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            "\n",
        );
        let (mut client, server) = tokio::io::duplex(64 * 1024);
        let (sr, sw) = tokio::io::split(server);
        let task = tokio::spawn(async move { session(sr, sw).await });
        client.write_all(input.as_bytes()).await.unwrap();
        client.shutdown().await.unwrap(); // EOF ends the session
        let mut out = String::new();
        client.read_to_string(&mut out).await.unwrap();
        task.await.unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2, "one reply per request, none for the notification: {out}");
        assert_eq!(parse(lines[0])["result"]["serverInfo"]["name"], "rexenv");
        assert_eq!(parse(lines[1])["result"]["tools"], json!([]));
    }
}

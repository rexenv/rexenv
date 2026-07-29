//! App-side MCP (Model Context Protocol) server: a private unix-socket JSON-RPC
//! endpoint an AI agent's client drives through the dumb `rex mcp` pipe.
//!
//! Like `cli_server`, this is remote control of THIS process only — the `cli/`
//! shim never links the app lib; it copies bytes between the client's stdio and
//! our socket. Tools execute through the same read paths the UI uses, via
//! `ReadCtx` (see `readctx`), so there is one brain.
//!
//! **No SDK** (docs/PLAN-mcp-server.md §2.1): every `rmcp` is edition 2024
//! (rustc ≥ 1.85) and the repo pins 1.77.2, so the server is hand-rolled on
//! `serde_json`. We implement the **2025-11-25** stable core: `initialize`,
//! `notifications/initialized`, `tools/list`, `tools/call`, `ping`. Framing is
//! newline-delimited JSON-RPC 2.0 — one message per line (the MCP stdio
//! transport, piped verbatim).
//!
//! **M1 is the read-only, contained tier**, and that is structural, not a
//! convention: a tool handler receives only a `ReadCtx` (no mutating method,
//! `readctx`) and the `tools` module imports no manager or command at all —
//! pinned by the read-only import guard in `tests`. When executing tools arrive
//! (M2) they go in a *different* module with its own capability; this boundary
//! does not erode.
//!
//! Socket: `<config>/rexenv-mcp.sock`, `0600`, bound with the SAME convention as
//! the CLI socket — `cli_server::bind`, deliberately reused so there is one
//! socket convention, not two. Never TCP.

mod readctx;
mod tools;
mod view;

use crate::state::app::AppState;
use readctx::ReadCtx;
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
pub fn spawn(app: tauri::AppHandle) {
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
        serve(listener, app).await;
    });
}

/// Accept loop: one long-lived MCP session per connection (unlike the CLI
/// socket's one-request-per-connection). Each session runs on its own task.
/// Public and runtime-generic so the `mcp_socket_check` example can serve it
/// with a `MockRuntime` app, exactly as `cli_server` tests do.
pub async fn serve<Rt: tauri::Runtime>(listener: UnixListener, app: tauri::AppHandle<Rt>) {
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue; // transient accept error; the socket stays bound
        };
        let app = app.clone();
        tokio::spawn(async move {
            let (read, write) = stream.into_split();
            session(read, write, app).await;
        });
    }
}

/// One MCP session: read newline-delimited JSON-RPC messages, dispatch each,
/// write a reply for every request (never for a notification), until EOF.
/// Generic over the byte streams so it is testable without a socket.
async fn session<R, W, Rt>(read: R, mut write: W, app: tauri::AppHandle<Rt>)
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    Rt: tauri::Runtime,
{
    let mut reader = BufReader::new(read);
    let mut buf = Vec::new();
    while read_line_capped(&mut reader, &mut buf, MAX_LINE_BYTES).await.is_some() {
        let text = String::from_utf8_lossy(&buf);
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        let reply = match dispatch(trimmed) {
            Dispatch::Silent => None,
            Dispatch::Reply(r) => Some(r),
            Dispatch::ToolCall { id, name, args } => {
                Some(fulfill_tool_call(&app, id, &name, &args))
            }
        };
        if let Some(reply) = reply {
            if write.write_all(reply.as_bytes()).await.is_err()
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

/// The outcome of dispatching one message, separated so the STATELESS protocol
/// (everything but running a tool) stays a pure, unit-testable function and only
/// `tools/call` reaches into app state (via `fulfill_tool_call`).
#[derive(Debug)]
enum Dispatch {
    /// A complete JSON-RPC reply line to send back.
    Reply(String),
    /// A notification — nothing to send.
    Silent,
    /// A call to a REGISTERED tool; needs app state to fulfil.
    ToolCall { id: Value, name: String, args: Value },
}

/// Pure protocol dispatch: initialize / ping / tools-list / notifications /
/// unknown-method / parse-error all resolve here without touching app state; a
/// call to a *known* tool becomes a `ToolCall` the session fulfils with state,
/// while a call to an *unknown* tool errors here.
fn dispatch(text: &str) -> Dispatch {
    let msg: Value = match serde_json::from_str(text) {
        // JSON-RPC: a parse error is reported with a null id.
        Err(_) => return Dispatch::Reply(error_response(Value::Null, -32700, "parse error")),
        Ok(v) => v,
    };
    // Absence of `id` marks a notification — never answered.
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    match (method, id) {
        ("initialize", Some(id)) => Dispatch::Reply(result_response(id, initialize_result(&msg))),
        ("ping", Some(id)) => Dispatch::Reply(result_response(id, json!({}))),
        ("tools/list", Some(id)) => Dispatch::Reply(result_response(id, tools::tools_list_result())),
        ("tools/call", Some(id)) => {
            let name = msg.pointer("/params/name").and_then(Value::as_str).unwrap_or("");
            if tools::find(name).is_none() {
                Dispatch::Reply(error_response(id, -32602, &format!("unknown tool: {name}")))
            } else {
                let args = msg.pointer("/params/arguments").cloned().unwrap_or_else(|| json!({}));
                Dispatch::ToolCall { id, name: name.to_string(), args }
            }
        }
        // Notifications (no id): `initialized` and anything else — no reply.
        (_, None) => Dispatch::Silent,
        // Any other request method.
        (other, Some(id)) => {
            Dispatch::Reply(error_response(id, -32601, &format!("method not found: {other}")))
        }
    }
}

/// Run a registered read-only tool and wrap its outcome as an MCP `tools/call`
/// result. A handler error is a TOOL error (`isError: true` content), not a
/// JSON-RPC protocol error — the agent sees a message, not a broken transport.
fn fulfill_tool_call<Rt: tauri::Runtime>(
    app: &tauri::AppHandle<Rt>,
    id: Value,
    name: &str,
    args: &Value,
) -> String {
    use tauri::Manager;
    let Some(tool) = tools::find(name) else {
        return error_response(id, -32602, &format!("unknown tool: {name}"));
    };
    let Some(state) = app.try_state::<AppState>() else {
        return result_response(
            id,
            tool_error_content("rexenv is still starting — try again in a moment"),
        );
    };
    let ctx = ReadCtx::new(state.inner());
    match (tool.handler)(&ctx, args) {
        Ok(v) => result_response(id, tool_success_content(&v)),
        Err(e) => result_response(id, tool_error_content(&e.to_string())),
    }
}

/// The `initialize` result: advertise the tools capability, our name and
/// version, and the negotiated protocol version — echoing the client's when we
/// recognise it, else our own.
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

/// A successful tool result: the tool's JSON rendered as a text content block.
fn tool_success_content(v: &Value) -> Value {
    let text = serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string());
    json!({ "content": [ { "type": "text", "text": text } ] })
}

/// A tool execution failure: an `isError` text block (not a JSON-RPC error).
fn tool_error_content(msg: &str) -> Value {
    json!({ "content": [ { "type": "text", "text": msg } ], "isError": true })
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

    fn reply(text: &str) -> Value {
        match dispatch(text) {
            Dispatch::Reply(r) => serde_json::from_str(&r).expect("reply is valid JSON"),
            other => panic!("expected a Reply, got {other:?}"),
        }
    }

    #[test]
    fn initialize_returns_server_info_and_the_tools_capability() {
        let v = reply(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"probe","version":"0"}}}"#,
        );
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 1);
        assert_eq!(v["result"]["serverInfo"]["name"], "rexenv");
        assert!(v["result"]["serverInfo"]["version"].is_string());
        assert!(v["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn initialize_echoes_a_supported_version_and_falls_back_otherwise() {
        for (asked, want) in [
            ("2025-11-25", "2025-11-25"),
            ("2024-11-05", "2024-11-05"),
            ("2099-01-01", PROTOCOL_VERSION),
        ] {
            let v = reply(&format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"{asked}"}}}}"#
            ));
            assert_eq!(v["result"]["protocolVersion"], want, "asked {asked}");
        }
    }

    #[test]
    fn tools_list_advertises_list_sites_with_a_schema() {
        let v = reply(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
        let tools = v["result"]["tools"].as_array().expect("tools array");
        let ls = tools.iter().find(|t| t["name"] == "list_sites").expect("list_sites present");
        assert!(ls["description"].as_str().unwrap().len() > 10);
        assert_eq!(ls["inputSchema"]["type"], "object");
    }

    #[test]
    fn ping_is_an_empty_result() {
        let v = reply(r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#);
        assert_eq!(v["result"], json!({}));
    }

    #[test]
    fn notifications_are_never_answered() {
        assert!(matches!(
            dispatch(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#),
            Dispatch::Silent
        ));
        assert!(matches!(
            dispatch(r#"{"jsonrpc":"2.0","method":"notifications/anything"}"#),
            Dispatch::Silent
        ));
    }

    #[test]
    fn a_known_tool_becomes_a_toolcall_an_unknown_one_errors() {
        // A registered tool defers to the stateful fulfil step.
        match dispatch(r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"list_sites"}}"#) {
            Dispatch::ToolCall { name, .. } => assert_eq!(name, "list_sites"),
            other => panic!("expected a ToolCall, got {other:?}"),
        }
        // An unregistered tool errors at dispatch, statelessly — never executes.
        let v = reply(
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"definitely_not_a_tool"}}"#,
        );
        assert_eq!(v["error"]["code"], -32602);
        assert!(v["error"]["message"].as_str().unwrap().contains("definitely_not_a_tool"));
    }

    #[test]
    fn unknown_method_errors_and_garbage_is_a_parse_error_at_null_id() {
        let v = reply(r#"{"jsonrpc":"2.0","id":6,"method":"resources/list"}"#);
        assert_eq!(v["error"]["code"], -32601);
        let v = reply("this is not json");
        assert_eq!(v["id"], Value::Null);
        assert_eq!(v["error"]["code"], -32700);
    }

    #[test]
    fn m1_tools_are_read_only_by_construction() {
        // The tools module may reach app state ONLY through ReadCtx. A direct
        // reference to a manager, a command, or raw AppState would become a hole
        // the moment M2 lands, so scan the source and fail LOUDLY + SPECIFICALLY.
        // Comments are stripped first: prose may name these to explain the rule.
        let src = include_str!("mcp_server/tools.rs");
        const FORBIDDEN: &[&str] = &[
            "core::",
            "commands::",
            "ServiceManager",
            "service_manager",
            "AppState",
            "PrivilegeManager",
            "run_privileged",
        ];
        for (i, line) in src.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            for tok in FORBIDDEN {
                assert!(
                    !code.contains(tok),
                    "M1 read-only boundary violated: mcp_server/tools.rs:{} reaches `{}`.\n\
                     M1 tools are read-only BY CONSTRUCTION — a handler may touch app state ONLY \
                     through ReadCtx (super::readctx), never a manager, command, or raw AppState. \
                     If you are adding a tool that must mutate or execute, it belongs in the M2 \
                     executing-tools module (a different capability), NOT here.",
                    i + 1,
                    tok
                );
            }
        }
    }
}

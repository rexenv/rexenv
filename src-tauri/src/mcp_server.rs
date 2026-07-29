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

// `pub` because the feed's read/clear API (`feed::recent`, `feed::clear`) is the
// surface the Settings card's IPC consumes; `record` is the server's own write.
pub mod feed;
mod readctx;
mod tools;
mod view;

use crate::state::app::AppState;
use readctx::ReadCtx;
use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::watch;

pub const SOCKET_FILE: &str = "rexenv-mcp.sock";

/// Settings key (KV `settings` table) for the opt-in toggle. Absent = OFF, the
/// default: a new API surface into rexenv is opt-in, not ambient (docs/PLAN §6).
/// The socket is bound ONLY while this is "true" AND `start` bound it — never on
/// the setting alone, so the toggle can never read on while nothing listens.
pub const MCP_ENABLED_KEY: &str = "mcp_enabled";

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

/// Live control of the opt-in MCP endpoint, held in `AppState`. A held sender =
/// serving; `None` = not. Sending `false` stops the accept loop AND drops every
/// live session (each `select!`s on this receiver), and `serve` then unlinks the
/// socket. The socket exists ONLY while a sender is held here — never ambiently,
/// so disabling the toggle genuinely removes the endpoint. Dropping the whole
/// `AppState` (app exit) drops the sender too, which `serve` reads as "stop".
#[derive(Default)]
pub struct McpControl {
    shutdown: Option<watch::Sender<bool>>,
}

impl McpControl {
    /// Whether the endpoint is serving right now (a socket is bound).
    pub fn is_running(&self) -> bool {
        self.shutdown.is_some()
    }

    /// Adopt the shutdown handle of a freshly-`start`ed server.
    pub fn store_handle(&mut self, tx: watch::Sender<bool>) {
        self.shutdown = Some(tx);
    }

    /// Stop serving: drop the accept loop and every live session (they `select!`
    /// on the receiver), after which `serve` unlinks the socket. Idempotent.
    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(false);
        }
    }
}

/// The MCP socket path — a sibling of the CLI socket in the config dir.
fn socket_path() -> crate::error::Result<std::path::PathBuf> {
    Ok(crate::platform::current().paths().config_dir()?.join(SOCKET_FILE))
}

/// Bind the MCP socket WITHOUT an ambient tokio runtime — a plain `std`
/// `UnixListener`, `0600`, stale-file unlinked, non-blocking so the serve task
/// can adopt it via `UnixListener::from_std`.
///
/// This is deliberately runtime-agnostic. `start` is called from app startup AND
/// from the SYNCHRONOUS `mcp_set_enabled` command, both of which run OFF the tokio
/// runtime — and `tokio::net::UnixListener::bind` panics there (`Handle::current`:
/// "there is no reactor running"), which is the packaged enable-crash (a SIGABRT
/// across wry's ObjC callback). Binding with `std` here and converting to tokio
/// INSIDE the runtime-resident serve task removes that hidden requirement while
/// keeping bind errors synchronous to the toggle. Guarded by
/// `binding_the_socket_needs_no_ambient_runtime`.
pub fn bind_socket(path: &std::path::Path) -> crate::error::Result<std::os::unix::net::UnixListener> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    let listener = std::os::unix::net::UnixListener::bind(path)?;
    // 0600 BEFORE anything can connect — and, unlike the crashing path, this line
    // always runs, so a bound socket is never left world-accessible.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?; // required for `from_std`
    Ok(listener)
}

/// Bind the socket and start serving. Returns the shutdown sender for `AppState`
/// to hold; `McpControl::stop` (or dropping it) tears the endpoint down. Binding
/// is synchronous (via the runtime-agnostic `bind_socket`) so a port/permission
/// failure surfaces to the caller (the Settings toggle) rather than vanishing into
/// a spawned task — the toggle must not read on if nothing bound.
pub fn start<Rt: tauri::Runtime>(
    app: tauri::AppHandle<Rt>,
) -> crate::error::Result<watch::Sender<bool>> {
    let path = socket_path()?;
    let listener = bind_socket(&path)?;
    let (tx, rx) = watch::channel(true);
    log::info!("mcp: listening on {} (opt-in enabled)", path.display());
    tauri::async_runtime::spawn(serve(listener, app, rx));
    Ok(tx)
}

/// At app startup, start the endpoint IF the user has enabled it — otherwise the
/// socket stays unbound (default off). The handle is stored in `AppState` so the
/// Settings toggle can stop/restart it live. Never fatal: the app works without
/// the endpoint, exactly like the CLI socket.
pub fn spawn_if_enabled(app: tauri::AppHandle) {
    use tauri::Manager;
    let enabled = match app.try_state::<AppState>() {
        Some(state) => match state.db.lock() {
            Ok(conn) => {
                matches!(crate::state::store::get_setting(&conn, MCP_ENABLED_KEY), Ok(Some(v)) if v == "true")
            }
            Err(_) => false,
        },
        None => false,
    };
    if !enabled {
        log::info!("mcp: endpoint off (default) — enable in Settings → AI agents (MCP)");
        return;
    }
    match start(app.clone()) {
        Ok(tx) => {
            if let Some(state) = app.try_state::<AppState>() {
                if let Ok(mut ctl) = state.mcp.lock() {
                    ctl.store_handle(tx);
                }
            }
        }
        Err(e) => log::error!("mcp: enabled but the socket did not bind at startup: {e}"),
    }
}

/// Accept loop: one long-lived MCP session per connection (unlike the CLI
/// socket's one-request-per-connection). Each session runs on its own task.
/// Public and runtime-generic so the `mcp_socket_check` example can serve it
/// with a `MockRuntime` app, exactly as `cli_server` tests do.
pub async fn serve<Rt: tauri::Runtime>(
    listener: std::os::unix::net::UnixListener,
    app: tauri::AppHandle<Rt>,
    mut shutdown: watch::Receiver<bool>,
) {
    // Adopt the std listener into tokio HERE — this task runs on the tokio
    // runtime, where `Handle::current` exists. `bind_socket` did NOT need it (it
    // may be called off the runtime, e.g. the sync enable command); `from_std`
    // does, and this is the first point we are guaranteed to be inside it.
    let listener = match UnixListener::from_std(listener) {
        Ok(l) => l,
        Err(e) => {
            log::error!("mcp: could not adopt the socket into the runtime: {e}");
            if let Ok(path) = socket_path() {
                let _ = std::fs::remove_file(path);
            }
            return;
        }
    };
    loop {
        tokio::select! {
            // Stop when the toggle sends `false`, OR when every sender is dropped
            // (app exit) — `changed()` errors then, and no one can turn it back on.
            res = shutdown.changed() => {
                if res.is_err() || !*shutdown.borrow() {
                    break;
                }
            }
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else {
                    continue; // transient accept error; the socket stays bound
                };
                let app = app.clone();
                let sd = shutdown.clone();
                tokio::spawn(async move {
                    let (read, write) = stream.into_split();
                    session(read, write, app, sd).await;
                });
            }
        }
    }
    // Accept loop stopped (disabled or app exit): unlink the socket so nothing
    // lingers advertising a dead endpoint and a later enable rebinds cleanly.
    if let Ok(path) = socket_path() {
        let _ = std::fs::remove_file(path);
    }
}

/// One MCP session: read newline-delimited JSON-RPC messages, dispatch each,
/// write a reply for every request (never for a notification), until EOF.
/// Generic over the byte streams so it is testable without a socket.
async fn session<R, W, Rt>(
    read: R,
    mut write: W,
    app: tauri::AppHandle<Rt>,
    mut shutdown: watch::Receiver<bool>,
) where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    Rt: tauri::Runtime,
{
    let mut reader = BufReader::new(read);
    let mut buf = Vec::new();
    // The client's self-reported name, for feed attribution — set at initialize.
    let mut client = String::from("unknown");
    loop {
        // Disabling the toggle drops this session mid-idle: `select!` wakes on the
        // shutdown signal instead of waiting for the client's next line.
        tokio::select! {
            res = shutdown.changed() => {
                if res.is_err() || !*shutdown.borrow() {
                    break; // disabled or app exit — drop the session
                }
                continue;
            }
            got = read_line_capped(&mut reader, &mut buf, MAX_LINE_BYTES) => {
                if got.is_none() {
                    break; // EOF / over-long line / read error
                }
            }
        }
        let text = String::from_utf8_lossy(&buf);
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(name) = client_name_if_initialize(trimmed) {
            client = name;
        }
        // Record the action BEFORE replying, so a reader that sees the reply
        // already sees the feed row. Every tools/call and every rejected/
        // malformed message is logged here — one place, no tool can forget.
        let reply = match dispatch(trimmed) {
            Dispatch::Silent => None,
            Dispatch::Reply(r) => Some(r),
            Dispatch::Rejected { reply, log } => {
                log_action(&app, &client, &log);
                Some(reply)
            }
            Dispatch::ToolCall { id, name, args } => {
                let target = args.get("site_id").and_then(Value::as_str).map(String::from);
                let (reply, outcome, detail) = fulfill_tool_call(&app, id, &name, &args).await;
                log_action(
                    &app,
                    &client,
                    &feed::PendingLog { tool: name, target_site: target, outcome, detail },
                );
                Some(reply)
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
    /// A protocol reply (initialize/ping/tools-list/unknown-method) — sent back,
    /// NOT an agent action, so never logged to the feed.
    Reply(String),
    /// A notification — nothing to send.
    Silent,
    /// A call to a REGISTERED tool; needs app state to fulfil, logged after.
    ToolCall { id: Value, name: String, args: Value },
    /// A tools/call refused before any handler (unknown tool) or a message we
    /// couldn't parse — carries BOTH the reply and the feed entry, so the
    /// non-happy-path is recorded by construction, not by anyone remembering.
    Rejected { reply: String, log: feed::PendingLog },
}

/// Pure protocol dispatch: initialize / ping / tools-list / notifications /
/// unknown-method / parse-error all resolve here without touching app state; a
/// call to a *known* tool becomes a `ToolCall` the session fulfils with state,
/// while a call to an *unknown* tool errors here.
fn dispatch(text: &str) -> Dispatch {
    let msg: Value = match serde_json::from_str(text) {
        // JSON-RPC: a parse error is reported with a null id — and it is an
        // attempt at SOMETHING, so it is a feed entry (bad-request).
        Err(_) => {
            return Dispatch::Rejected {
                reply: error_response(Value::Null, -32700, "parse error"),
                log: feed::PendingLog {
                    tool: "(unparseable)".into(),
                    target_site: None,
                    outcome: feed::Outcome::BadRequest,
                    detail: Some("could not parse the JSON-RPC message".into()),
                },
            }
        }
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
            let target = tool_target_site(&msg);
            if tools::find(name).is_none() {
                Dispatch::Rejected {
                    reply: error_response(id, -32602, &format!("unknown tool: {name}")),
                    log: feed::PendingLog {
                        tool: name.to_string(),
                        target_site: target,
                        outcome: feed::Outcome::UnknownTool,
                        detail: Some("no such tool".into()),
                    },
                }
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

/// The ONE argument the feed records — the site a tools/call named — read from
/// the request's `arguments.site_id`. No other argument is captured (§feed).
fn tool_target_site(msg: &Value) -> Option<String> {
    msg.pointer("/params/arguments/site_id").and_then(Value::as_str).map(String::from)
}

/// The client's self-reported name from `initialize`, so the feed can attribute
/// actions. Cheap: only initialize-shaped messages are parsed here.
fn client_name_if_initialize(text: &str) -> Option<String> {
    if !text.contains("\"initialize\"") {
        return None;
    }
    let msg: Value = serde_json::from_str(text).ok()?;
    if msg.get("method")?.as_str()? != "initialize" {
        return None;
    }
    msg.pointer("/params/clientInfo/name")?.as_str().map(String::from)
}

/// Run a registered read-only tool and wrap its outcome as an MCP `tools/call`
/// result. A handler error is a TOOL error (`isError: true` content), not a
/// JSON-RPC protocol error — the agent sees a message, not a broken transport.
/// Run a registered tool, returning the reply AND the feed outcome (+ a bounded
/// reason for a non-ok one), so the session records what happened.
async fn fulfill_tool_call<Rt: tauri::Runtime>(
    app: &tauri::AppHandle<Rt>,
    id: Value,
    name: &str,
    args: &Value,
) -> (String, feed::Outcome, Option<String>) {
    use tauri::Manager;
    let Some(tool) = tools::find(name) else {
        // Defensive: dispatch already rejected unknown tools before here.
        return (
            error_response(id, -32602, &format!("unknown tool: {name}")),
            feed::Outcome::UnknownTool,
            Some("no such tool".into()),
        );
    };
    let Some(state) = app.try_state::<AppState>() else {
        return (
            result_response(id, tool_error_content("rexenv is still starting — try again in a moment")),
            feed::Outcome::Error,
            Some("rexenv is still starting".into()),
        );
    };
    let ctx = ReadCtx::new(state.inner());
    match (tool.handler)(ctx, args).await {
        Ok(v) => (result_response(id, tool_success_content(&v)), feed::Outcome::Ok, None),
        Err(e) => (
            result_response(id, tool_error_content(&e.to_string())),
            feed::Outcome::Error,
            Some(e.to_string()),
        ),
    }
}

/// Run EVERY registered tool against `app`'s state with the fixture site id, and
/// return each tool's serialised output (or its error text — errors can leak
/// too). For the secret-leak sweep (`examples/mcp_secret_sweep`): it plants
/// secrets in the state and asserts none appear in any output here. Enumerates
/// the registry (`tools::sweep_plan`), so a new tool is swept by construction —
/// adding one WITHOUT the sweep covering it is not possible.
pub async fn sweep_tool_outputs<Rt: tauri::Runtime>(
    app: &tauri::AppHandle<Rt>,
    fixture_site_id: &str,
) -> Vec<(&'static str, String)> {
    use tauri::Manager;
    let Some(state) = app.try_state::<AppState>() else {
        return Vec::new();
    };
    let ctx = ReadCtx::new(state.inner());
    let mut outputs = Vec::new();
    for (tool, args) in tools::sweep_plan(fixture_site_id) {
        let text = match (tool.handler)(ctx, &args).await {
            Ok(v) => serde_json::to_string(&v).unwrap_or_default(),
            Err(e) => e.to_string(),
        };
        outputs.push((tool.name, text));
    }
    outputs
}

/// Record one agent action to the feed, best-effort — a logging failure must
/// never break the session (accountability is important, but not at the cost of
/// the connection). The feed is its own table; this is the server's write, not a
/// tool's, so the read-only tool boundary is untouched.
fn log_action<Rt: tauri::Runtime>(app: &tauri::AppHandle<Rt>, client: &str, log: &feed::PendingLog) {
    use tauri::Manager;
    let Some(state) = app.try_state::<AppState>() else { return };
    let conn = match state.db.lock() {
        Ok(conn) => conn,
        Err(_) => {
            log::warn!("mcp: skipped feed record — the db lock is poisoned");
            return;
        }
    };
    if let Err(e) = feed::record(&conn, client, log) {
        log::warn!("mcp: could not record agent action: {e}");
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

    /// The packaged enable-crash guard: `mcp_set_enabled` is a SYNC command, so it
    /// binds OFF the tokio runtime, where `tokio::net::UnixListener::bind` aborts on
    /// `Handle::current` ("there is no reactor running"). `bind_socket` must need no
    /// ambient runtime — proven by binding on a plain `std::thread` with NONE and
    /// requiring success + 0600. This FAILS on the pre-fix path (the throwaway proof
    /// against `cli_server::bind` panicked on this exact thread); it passes on the
    /// std bind. A harness (L2) can never catch this — it mocks the IPC command.
    #[test]
    fn binding_the_socket_needs_no_ambient_runtime() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("rexenv-mcp-bindguard-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("mcp.sock");
        let p = path.clone();
        // A plain std thread — deliberately NO tokio runtime in context.
        let joined = std::thread::spawn(move || bind_socket(&p).map(|_| ())).join();
        assert!(joined.is_ok(), "bind_socket panicked off the runtime (Handle::current) — the packaged crash");
        assert!(joined.unwrap().is_ok(), "bind_socket errored");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "the bound socket must be 0600, always");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn reply(text: &str) -> Value {
        let r = match dispatch(text) {
            Dispatch::Reply(r) => r,
            Dispatch::Rejected { reply, .. } => reply,
            other => panic!("expected a reply, got {other:?}"),
        };
        serde_json::from_str(&r).expect("reply is valid JSON")
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
    fn mcp_control_is_off_by_default_and_stop_signals_shutdown_idempotently() {
        // The opt-in state machine: off until a handle is held, serving while it
        // is, and `stop` both drops the handle AND signals every session (via the
        // watch value) to shut down. Idempotent so a double-disable can't panic.
        let mut ctl = McpControl::default();
        assert!(!ctl.is_running(), "off by default (opt-in, socket unbound)");
        let (tx, rx) = watch::channel(true);
        ctl.store_handle(tx);
        assert!(ctl.is_running(), "serving once a handle is held");
        ctl.stop();
        assert!(!ctl.is_running(), "stop drops the handle");
        assert!(!*rx.borrow(), "stop signalled false — sessions and accept loop drop");
        ctl.stop(); // idempotent
        assert!(!ctl.is_running());
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
    fn a_known_tool_becomes_a_toolcall() {
        // A registered tool defers to the stateful fulfil step.
        match dispatch(r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"list_sites"}}"#) {
            Dispatch::ToolCall { name, .. } => assert_eq!(name, "list_sites"),
            other => panic!("expected a ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn unknown_method_errors_without_a_feed_entry() {
        // A protocol method that isn't a tool call is answered but NOT logged —
        // the feed is agent actions, not handshake noise.
        match dispatch(r#"{"jsonrpc":"2.0","id":6,"method":"resources/list"}"#) {
            Dispatch::Reply(r) => {
                let v: Value = serde_json::from_str(&r).unwrap();
                assert_eq!(v["error"]["code"], -32601);
            }
            other => panic!("expected a plain Reply, got {other:?}"),
        }
    }

    #[test]
    fn the_non_happy_paths_are_loggable_by_construction() {
        // Malformed input → a bad-request feed entry with its reply.
        match dispatch("this is not json") {
            Dispatch::Rejected { reply, log } => {
                let v: Value = serde_json::from_str(&reply).unwrap();
                assert_eq!(v["error"]["code"], -32700);
                assert_eq!(v["id"], Value::Null);
                assert_eq!(log.outcome, feed::Outcome::BadRequest);
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
        // An unknown tool → an unknown-tool entry that STILL captures the typed
        // target (site_id), and never executes.
        match dispatch(
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"nope","arguments":{"site_id":"s9","smuggle":"SECRET"}}}"#,
        ) {
            Dispatch::Rejected { log, .. } => {
                assert_eq!(log.outcome, feed::Outcome::UnknownTool);
                assert_eq!(log.tool, "nope");
                assert_eq!(log.target_site.as_deref(), Some("s9"));
                // The extra arg is not captured anywhere in the entry.
                assert!(log.detail.as_deref() != Some("SECRET"));
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn every_tool_declares_valid_sweep_args_so_the_leak_sweep_can_exercise_it() {
        // The secret-leak sweep (examples/mcp_secret_sweep) enumerates the
        // registry and runs each tool with these args against a planted fixture.
        // Requiring the field (and this smoke test) means a tool cannot be
        // registered without being exercisable by the sweep.
        for t in tools::registry() {
            let args = (t.sweep_args)("fixture-site-id");
            assert!(args.is_object(), "{}: sweep_args must be a JSON object", t.name);
        }
    }

    #[test]
    fn m1_read_only_boundary_holds_across_tools_and_the_read_bridge() {
        // The read-only guarantee is enforced structurally by ReadCtx (no
        // mutator method; private state field) — but a handler is a plain fn and
        // ReadCtx reaches `core::` for its READS, so the assembly review taught us
        // to scan BOTH surfaces, not just tools.rs (the door isn't the only file
        // with reach):
        //   - tools.rs: a handler reaches state ONLY through ReadCtx — never a
        //     manager, command, raw AppState, OR a direct syscall (fs/process/net).
        //   - readctx.rs (the audited bridge): may reach `core::` READS, but never
        //     a MUTATOR or executor.
        // Comments are stripped (prose names these to explain the rule). A token
        // hidden in a string literal after `//` is a known minor gap — these
        // tokens don't appear in string literals in either file; the type-level
        // ReadCtx boundary is the real guarantee, this scan is the belt.
        fn scan(src: &str, file: &str, forbidden: &[&str]) {
            for (i, line) in src.lines().enumerate() {
                let code = line.split("//").next().unwrap_or("");
                for tok in forbidden {
                    assert!(
                        !code.contains(tok),
                        "M1 read-only boundary violated: {}:{} reaches `{}`.\n\
                         M1 is read-only: a tool handler touches state ONLY through ReadCtx, and \
                         ReadCtx itself may only READ. Anything that mutates or executes belongs \
                         in the M2 executing-tools module (a different capability), NOT here.",
                        file,
                        i + 1,
                        tok
                    );
                }
            }
        }
        scan(
            include_str!("mcp_server/tools.rs"),
            "mcp_server/tools.rs",
            &[
                "core::", "commands::", "ServiceManager", "service_manager", "AppState",
                "PrivilegeManager", "run_privileged", "std::fs", "std::process", "std::os",
                "Command", "reqwest",
            ],
        );
        scan(
            include_str!("mcp_server/readctx.rs"),
            "mcp_server/readctx.rs",
            &[
                "start_all", "stop_all", "spawn_db", "stop_db", "start_edge", "stop_edge",
                ".reload(", "start_privileged", "run_privileged", "PrivilegeManager",
                "commands::", "std::fs::write", "std::fs::remove", "std::fs::create",
                "std::process", "Command", ".execute(",
            ],
        );
    }
}

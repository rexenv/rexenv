//! App-side MCP (Model Context Protocol) server: a private unix-socket JSON-RPC
//! endpoint an AI agent's client drives through the dumb `rex mcp` pipe.
//!
//! Like `cli_server`, this is remote control of THIS process only — the `cli/`
//! shim never links the app lib; it copies bytes between the client's stdio and
//! our socket. Tools execute through the same read paths the UI uses, via
//! `ReadCtx` (see `readctx`), so there is one brain.
//!
//! **No SDK** (docs/archive/PLAN-mcp-server.md §2.1): every `rmcp` is edition 2024
//! (rustc ≥ 1.85) and the repo pins 1.77.2, so the server is hand-rolled on
//! `serde_json`. We implement the **2025-11-25** stable core: `initialize`,
//! `notifications/initialized`, `tools/list`, `tools/call`, `ping`. Framing is
//! newline-delimited JSON-RPC 2.0 — one message per line (the MCP stdio
//! transport, piped verbatim).
//!
//! **M1 is the read-only, contained tier**, and that is structural, not a
//! convention: a tool handler receives only a `ReadCtx` (no mutating method,
//! `readctx`) and the `tools` module imports no manager or command at all —
//! pinned by the read-only import guard in `tests`. Executing tools live in a
//! *different* module with its own capability (`scratch`); this boundary does
//! not erode.
//!
//! **What the SOCKET guarantees, as opposed to what M1's tools guarantee.**
//! These are two different statements and only one of them is #199. Dispatch
//! routes EVERY registry through one enumeration (`every_tool`), so the honest
//! description of the endpoint is:
//!
//! - a call reaches exactly one registry, and its capability is decided by which
//!   one it came from (`find_tool` → `Tool::Read` gets a `ReadCtx`,
//!   `Tool::Scratch` a `ScratchCtx`, `Tool::User` a `UserCtx`) — never by the
//!   tool's own say-so, never by its arguments, and never by lookup order,
//!   because the registries are disjoint by test;
//! - a READ tool cannot mutate anything (#199, unchanged by M2's arrival);
//! - an EXECUTING tool can only reach a site the agent OWNS — its context's only
//!   door to a site is the `origin`-checked witness (#208) — but within such a
//!   site it runs the user's code, which is user-level power over this machine
//!   (PLAN §3.1, ledger #197);
//! - a PARITY tool (`user_sites`, MCP parity) can reach the user's OWN site or
//!   the stack only through a scope witness minted from a grant the user gave in
//!   the app (#471), and only while the "manage my own sites" switch is on —
//!   and inside such a site it runs the user's code too. Wider surface, same
//!   residual, said in the same words.
//!
//! So "the MCP socket is read-only" is true of M1 alone and **false of the
//! endpoint** the moment a scratch tool lands. Nothing here, in the plan, or in
//! the Settings card may say the wider thing — and the card's enable-moment copy
//! is held to the registry by a test rather than by memory
//! (`the_enable_moment_copy_cannot_keep_claiming_read_only_once_a_tool_executes`).
//!
//! **What "read-only" scopes: the HANDLER, not the call.** A tool call always
//! writes rexenv's own records — the activity feed, and (M2a) the named scratch
//! site's TTL — both in the session layer, on rexenv's account, unreachable from
//! a handler (`log_action`). That is the point of keeping them out here: a
//! refresh inside `tools.rs` would either break the handler boundary or force
//! the guard to be weakened around it. Neither write touches the user's sites,
//! their files, or their databases, which is what the containment claim is
//! about — but "an M1 call writes nothing" would be the false wider reading, so
//! it is not said anywhere.
//!
//! Socket: `<config>/rexenv-mcp.sock`, `0600`, sibling of the CLI socket and
//! bound with the SAME convention — unlink a stale file, chmod 0600, never TCP.
//!
//! **The convention is shared; the BINDER is not, and the difference is why this
//! sentence was corrected on 21 Aug 2026.** It used to say the socket was bound
//! by `cli_server::bind`, "deliberately reused so there is one socket
//! convention, not two". That was the intent and it did not survive contact:
//! `cli_server::bind` returns a TOKIO listener and panics when constructed off a
//! runtime, which is exactly how it failed in the packaged build the first time
//! someone flipped the toggle. [`bind_socket`] below is a plain `std` binder for
//! that reason, pinned by `binding_the_socket_needs_no_ambient_runtime`. A
//! comment describing the design that was TRIED, beside code that does something
//! else, is the shape this project keeps paying for.

// `pub` because the feed's read/clear API (`feed::recent`, `feed::clear`) is the
// surface the Settings card's IPC consumes; `record` is the server's own write.
pub mod feed;
mod parity;
mod readctx;
mod scratch;
mod tools;
mod user_sites;
mod view;

use crate::state::app::AppState;
// `pub(crate)`: `rex site info` renders the SAME serving classification an agent
// gets (`ReadCtx::probe_serving` + `AgentSiteStatus`) — the CLI read the
// manager's belief until 29 Sep 2026 and said `serving` about a site the edge
// refused (the TODO row of 23 Sep). The modules stay private; the two types cross.
pub(crate) use readctx::ReadCtx;
pub(crate) use view::AgentSiteStatus;
use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
// The transport is the only OS-specific part of this module: on unix `socket_path`,
// `bind_socket`, `start` and `serve`; on Windows `start` and `serve_pipe` over the MCP
// pipe (W8 S3, ledger #632). Sessions, dispatch, tools and the feed compile everywhere.
#[cfg(unix)]
use tokio::net::UnixListener;
use tokio::sync::{mpsc, watch};

/// Where a long tool call reports progress while it runs (parity P6.3).
///
/// Built by the session ONLY when the client asked for it — a `progressToken`
/// in the call's `_meta`, the MCP spec's own opt-in — and handed to the ops
/// layer, whose poll loops (a provision, a repo job, a database import) are the
/// only places rexenv knows a long operation's phases. Each report becomes one
/// `notifications/progress` line on the session's socket, written BEFORE the
/// call's reply; a client that never sent a token gets exactly what it got
/// before. Reporting is best-effort: a dropped receiver is a client that went
/// away, not a reason to fail the operation.
#[derive(Clone)]
pub struct ProgressSink {
    token: Value,
    tx: mpsc::UnboundedSender<String>,
}

impl ProgressSink {
    pub fn report(&self, progress: f64, total: Option<f64>, message: &str) {
        let _ = self.tx.send(progress_notification(&self.token, progress, total, message));
    }
}

/// One `notifications/progress` line, the spec's shape: the client's token
/// echoed, `progress` monotonic, `total` when the operation knows its end.
fn progress_notification(token: &Value, progress: f64, total: Option<f64>, message: &str) -> String {
    let mut params = json!({ "progressToken": token, "progress": progress, "message": message });
    if let Some(t) = total {
        params["total"] = json!(t);
    }
    json!({ "jsonrpc": "2.0", "method": "notifications/progress", "params": params }).to_string()
}

/// Run a tool call while forwarding its progress notifications to the writer
/// as they arrive, then drain what is left so every notification lands BEFORE
/// the reply the caller writes next. Generic over the writer so the ordering
/// is testable without a socket.
async fn run_with_progress<W, F, T>(write: &mut W, rx: Option<mpsc::UnboundedReceiver<String>>, call: F) -> T
where
    W: AsyncWrite + Unpin,
    F: std::future::Future<Output = T>,
{
    let Some(mut rx) = rx else { return call.await };
    tokio::pin!(call);
    let result = loop {
        tokio::select! {
            r = &mut call => break r,
            Some(line) = rx.recv() => {
                if write.write_all(line.as_bytes()).await.is_err() || write.write_all(b"\n").await.is_err() {
                    // The client is gone; the call still finishes (it is the
                    // app's own job) and its reply write will fail the same way.
                }
                let _ = write.flush().await;
            }
        }
    };
    while let Ok(line) = rx.try_recv() {
        let _ = write.write_all(line.as_bytes()).await;
        let _ = write.write_all(b"\n").await;
    }
    let _ = write.flush().await;
    result
}

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
#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
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

/// Create the MCP pipe and start serving (W8 S3, plan §5 W8 ruling Q2, ledger #632). The pipe's first
/// instance is created HERE, synchronously and off any runtime — `mcp_set_enabled` runs off the tokio
/// runtime, the unix `bind_socket` lesson — so a failure reaches the toggle and it stays off (#203).
#[cfg(windows)]
pub fn start<Rt: tauri::Runtime>(
    app: tauri::AppHandle<Rt>,
) -> crate::error::Result<watch::Sender<bool>> {
    let dir = crate::platform::current().paths().config_dir()?;
    let held = crate::platform::create_mcp_pipe(&dir)?;
    let (tx, rx) = watch::channel(true);
    log::info!("mcp: listening on {} (opt-in enabled)", held.name());
    tauri::async_runtime::spawn(serve_pipe(held, app, rx));
    Ok(tx)
}

/// The pipe's accept loop — one MCP session per connection, the unix `serve`'s shape. The next instance is
/// made before a connected one is handed to its session. Turning the toggle off (or the app exiting) ends the
/// loop and drops the listening instance; every session ends on the same signal, so once they have, no
/// instance of the name exists and a connect finds nothing — the Windows form of the socket file being
/// removed (ledger #632).
#[cfg(windows)]
async fn serve_pipe<Rt: tauri::Runtime>(
    held: crate::platform::HeldAppPipe,
    app: tauri::AppHandle<Rt>,
    mut shutdown: watch::Receiver<bool>,
) {
    let name = held.name().to_string();
    let mut server = match held.into_server() {
        Ok(server) => server,
        Err(e) => {
            log::error!("mcp: could not serve {name}: {e}");
            return;
        }
    };
    loop {
        tokio::select! {
            res = shutdown.changed() => {
                if res.is_err() || !*shutdown.borrow() {
                    break;
                }
            }
            connected = server.connect() => {
                if connected.is_err() {
                    continue;
                }
                let next = match crate::platform::next_app_pipe_instance(&name) {
                    Ok(next) => next,
                    Err(e) => {
                        log::error!("mcp: could not make the next instance of {name}: {e} — serving this session, then stopping");
                        let (read, write) = tokio::io::split(server);
                        session(read, write, app.clone(), shutdown.clone()).await;
                        return;
                    }
                };
                let current = std::mem::replace(&mut server, next);
                let app = app.clone();
                let sd = shutdown.clone();
                tokio::spawn(async move {
                    let (read, write) = tokio::io::split(current);
                    session(read, write, app, sd).await;
                });
            }
        }
    }
    log::info!("mcp: stopped serving {name}");
}

/// No MCP transport on this OS. Refusing HERE keeps the bind-first rule intact (ledger #203):
/// `mcp_set_enabled` returns this error before it persists "true", nothing is stored
/// in `AppState.mcp`, and the toggle can never read on while nothing listens.
#[cfg(not(any(unix, windows)))]
pub fn start<Rt: tauri::Runtime>(
    _app: tauri::AppHandle<Rt>,
) -> crate::error::Result<watch::Sender<bool>> {
    Err(crate::error::Error::Other(
        "The AI agent (MCP) endpoint is not available on this operating system yet.".into(),
    ))
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
                // D16: the stamp rides the endpoint. A machine upgraded with
                // MCP already on never clicks the toggle, so the backfill for
                // its existing scratch sites happens here.
                if let Ok(conn) = state.db.lock() {
                    crate::commands::mcp::sync_scratch_mail_stamps(&conn, true);
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
#[cfg(unix)]
pub async fn serve<Rt: tauri::Runtime>(
    listener: std::os::unix::net::UnixListener,
    app: tauri::AppHandle<Rt>,
    mut shutdown: watch::Receiver<bool>,
) {
    // Adopt the std listener into tokio HERE — this task runs on the tokio
    // runtime, where `Handle::current` exists. `bind_socket` did NOT need it (it
    // may be called off the runtime, e.g. the sync enable command); `from_std`
    // does, and this is the first point we are guaranteed to be inside it.
    // Unlink the socket THIS listener bound, never a re-derived one.
    //
    // `serve` used to call `socket_path()` here, which recomputes the default
    // from the real config dir. In the app those always agree (`start` uses it
    // for both), so it was latent — but a caller that binds anywhere else (an
    // example with its own sandbox path) would have had its shutdown delete the
    // APP'S socket instead of its own. That is the examples-touch-real-state
    // class this codebase has already been bitten by three times, and the fix is
    // the one-fact rule: the listener knows where it is, so ask it. If it can't
    // say, unlink NOTHING — a guessed path is what the bug was.
    let bound_path = listener.local_addr().ok().and_then(|a| a.as_pathname().map(|p| p.to_path_buf()));
    let unlink = || {
        if let Some(p) = &bound_path {
            let _ = std::fs::remove_file(p);
        }
    };
    let listener = match UnixListener::from_std(listener) {
        Ok(l) => l,
        Err(e) => {
            log::error!("mcp: could not adopt the socket into the runtime: {e}");
            unlink();
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
    // The socket FILE going away is the security-relevant half — a closed
    // listener with the node still on disk reads as an endpoint to anything that
    // stats it, and `mcp_control_check` asserts the file is gone, not merely
    // that new connects fail.
    unlink();
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
            Dispatch::ToolCall { id, name, args, progress_token } => {
                let named = args.get("site_id").and_then(Value::as_str).map(String::from);
                // The handler may report the site rexenv ACTED on (a create has
                // no `site_id` to name). It is an out-parameter, so a handler
                // that records the row and THEN fails still names it.
                let acted = feed::ActedTarget::default();
                // Ask the TOOL what this call was about, before it runs. Per-tool
                // rather than a generic reader of `args`, so the feed layer never
                // parses agent JSON itself (#202); computed BEFORE the handler so
                // a call that fails is still described by what it tried to do.
                let args_summary = find_tool(&name).and_then(|t| match t {
                    Tool::Read(t) => (t.summarise)(&args),
                    Tool::Scratch(t) => (t.summarise)(&args),
                    Tool::User(t) => (t.summarise)(&args),
                });
                // Progress only when asked for (`_meta.progressToken`): the
                // notifications go out on THIS writer, before the reply.
                let (sink, rx) = match progress_token {
                    Some(token) => {
                        let (tx, rx) = mpsc::unbounded_channel();
                        (Some(ProgressSink { token, tx }), Some(rx))
                    }
                    None => (None, None),
                };
                let (reply, outcome, detail) = run_with_progress(
                    &mut write,
                    rx,
                    fulfill_tool_call(&app, id, &name, &args, &acted, &client, sink),
                )
                .await;
                log_action(
                    &app,
                    &client,
                    &feed::PendingLog {
                        tool: name,
                        target_site: target_for_record(acted.take(), named),
                        outcome,
                        detail,
                        args_summary,
                    },
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
    ToolCall { id: Value, name: String, args: Value, progress_token: Option<Value> },
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
                    // Nothing parsed, so there is no tool to ask and nothing to
                    // summarise. `None` here means the same as everywhere: the
                    // tool name and target already say what this row is.
                    args_summary: None,
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
        ("tools/list", Some(id)) => Dispatch::Reply(result_response(id, tools_list_result())),
        ("tools/call", Some(id)) => {
            let name = msg.pointer("/params/name").and_then(Value::as_str).unwrap_or("");
            let target = tool_target_site(&msg);
            if find_tool(name).is_none() {
                Dispatch::Rejected {
                    reply: error_response(id, -32602, &format!("unknown tool: {name}")),
                    log: feed::PendingLog {
                        tool: name.to_string(),
                        target_site: target,
                        outcome: feed::Outcome::UnknownTool,
                        detail: Some("no such tool".into()),
                        // No such tool, so no summariser exists to ask — and
                        // deliberately NOT a guess from the raw arguments,
                        // which is exactly the generic arg-dump the typed shape
                        // exists to prevent (#202).
                        args_summary: None,
                    },
                }
            } else {
                let args = msg.pointer("/params/arguments").cloned().unwrap_or_else(|| json!({}));
                // The spec's opt-in for progress: a token in the call's `_meta`.
                let progress_token = msg.pointer("/params/_meta/progressToken").cloned().filter(|t| !t.is_null());
                Dispatch::ToolCall { id, name: name.to_string(), args, progress_token }
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

/// One registered tool, from any registry — the socket's whole tool surface.
///
/// The variants ARE the capability split: a `Read` tool's handler gets a
/// `ReadCtx` (no mutating method), a `Scratch` tool's gets a `ScratchCtx` whose
/// only door to a site is the `origin`-checked witness (#208), a `User` tool's
/// gets a `UserCtx` whose only door is the scope witness (#471). Dispatch is the
/// one place that maps a name to a capability, so a tool cannot be routed to a
/// context its module never gave it.
#[derive(Clone, Copy)]
enum Tool {
    Read(&'static tools::ReadTool),
    Scratch(&'static scratch::ScratchTool),
    User(&'static user_sites::UserTool),
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Tool::Read(t) => t.name,
            Tool::Scratch(t) => t.name,
            Tool::User(t) => t.name,
        }
    }

    /// Which module this tool came from — for the disjointness guard's message.
    #[cfg(test)]
    fn module(self) -> &'static str {
        match self {
            Tool::Read(_) => "mcp_server/tools.rs",
            Tool::Scratch(_) => "mcp_server/scratch.rs",
            Tool::User(_) => "mcp_server/user_sites.rs",
        }
    }

    /// The MCP descriptor, from the registry's own descriptor list.
    /// The MCP tool annotations (`readOnlyHint`, `destructiveHint`), decided
    /// HERE from the registry a tool came from — the same fact dispatch uses
    /// for its capability — never from a field the tool sets about itself.
    /// The spec says clients must treat these as untrusted UX hints, and so
    /// does rexenv: the registry and the witness types are the boundary; this
    /// is that boundary said in the protocol's vocabulary (PLAN-mcp-parity L6).
    fn annotations(self) -> (bool, bool) {
        match self {
            Tool::Read(_) => (true, false),
            Tool::Scratch(t) => (false, t.name == "scratch_delete_site"),
            Tool::User(t) => (t.scope == crate::core::agent_grants::Scope::Read, t.scope == crate::core::agent_grants::Scope::Destroy),
        }
    }

    fn descriptor(self) -> Value {
        let list = match self {
            Tool::Read(_) => tools::tools_list_result()["tools"].clone(),
            Tool::Scratch(_) => scratch::tools_list_descriptors(),
            Tool::User(_) => user_sites::tools_list_descriptors(),
        };
        let mut d = list
            .as_array()
            .and_then(|a| a.iter().find(|d| d["name"] == self.name()).cloned())
            .unwrap_or_else(|| json!({ "name": self.name() }));
        let (read_only, destructive) = self.annotations();
        if let Some(obj) = d.as_object_mut() {
            obj.insert("annotations".into(), json!({ "readOnlyHint": read_only, "destructiveHint": destructive }));
        }
        d
    }
}

/// **The ONE enumeration of every registry**, in precedence order. `find_tool`,
/// `tools/list`, the leak sweep and the disjointness guard all walk THIS, so a
/// registry that exists but is not chained here is unreachable, unlisted AND
/// unswept at once — which is loud, where the old shape (each consumer naming
/// "both" registries by hand) would have let a third registry be listed but not
/// swept, or swept but not dispatched. A source guard fails the build on a
/// registry module missing from this chain.
fn every_tool() -> impl Iterator<Item = Tool> {
    tools::registry()
        .iter()
        .map(Tool::Read)
        .chain(scratch::registry().iter().map(Tool::Scratch))
        .chain(user_sites::registry().iter().map(Tool::User))
}

/// Look a tool up across every registry. Read side first — an M1 name can never
/// be shadowed by a later tool, and the disjointness guard means that precedence
/// never has to be exercised (see
/// `every_registry_is_disjoint_and_the_guard_says_which_side_a_tool_belongs_on`).
fn find_tool(name: &str) -> Option<Tool> {
    every_tool().find(|t| t.name() == name)
}

/// The `tools/list` result — the UNION of every registry, which is what the
/// socket actually offers. A client sees one flat list; the tier a tool belongs
/// to is a fact about what rexenv will let it do, not something the agent picks.
fn tools_list_result() -> Value {
    json!({ "tools": every_tool().map(Tool::descriptor).collect::<Vec<_>>() })
}

/// Which site a feed row names, given both provenances.
///
/// **What rexenv DID beats what the agent ASKED for**, for two reasons: it is
/// the more accurate fact when they differ (a tool that resolves elsewhere, or
/// creates), and it is the only one that exists for a call with no `site_id`.
/// `named` remains the answer for M1's read tools — including when they fail,
/// where "the agent asked about a site that isn't there" is the diagnostic worth
/// keeping. Deliberately pure so both directions are testable without a session.
fn target_for_record(acted: Option<String>, named: Option<String>) -> Option<String> {
    acted.or(named)
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
    acted: &feed::ActedTarget,
    client: &str,
    progress: Option<ProgressSink>,
) -> (String, feed::Outcome, Option<String>) {
    use tauri::Manager;
    let Some(tool) = find_tool(name) else {
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
    // The capability is decided HERE, by which registry the tool came from —
    // never by the tool's own say-so and never by its arguments.
    let outcome = match tool {
        Tool::Read(t) => (t.handler)(ReadCtx::new(state.inner()), args, acted).await,
        Tool::Scratch(t) => {
            let creator = AppSiteCreator { app: app.clone(), progress: progress.clone() };
            let ctx = scratch::ScratchCtx::new(state.inner(), &creator, &creator, client);
            (t.handler)(ctx, args, acted).await
        }
        Tool::User(t) => {
            let ops = AppSiteCreator { app: app.clone(), progress: progress.clone() };
            (t.handler)(user_sites::UserCtx::new(state.inner(), &ops, &ops, &ops, &ops, &ops, &ops, &ops, client), args, acted).await
        }
    };
    match outcome {
        Ok(v) => (result_response(id, tool_success_content(&v)), feed::Outcome::Ok, None),
        Err(e) => {
            let text = scrub_error(state.inner(), acted, &e.to_string());
            (result_response(id, tool_error_content(&text)), feed::Outcome::Error, Some(text))
        }
    }
}

/// Every handler's ERROR goes through the one scrubber before an agent or the
/// feed sees it — with the acted-on site's docroot when a row was reached,
/// rexenv's own paths and home always. Found by `mcp_secret_sweep` the moment
/// D15 made reads free: a vetted `wp plugin list` on a docroot that is not a
/// WordPress install failed with wp-cli's own "The used path is: /…" — a path
/// every SUCCESS reply had scrubbed and no error reply had, because until then
/// the gate refused the call before the runner could fail. Success replies are
/// scrubbed inside each handler (they know their shape); an error is a string,
/// and one place is enough for a string.
fn scrub_error(state: &AppState, acted: &feed::ActedTarget, raw: &str) -> String {
    let docroot = acted
        .peek()
        .and_then(|id| state.db.lock().ok().and_then(|c| crate::state::store::get_site(&c, &id).ok().flatten()))
        .map(|s| s.path);
    let paths = state.platform.paths();
    let known = match docroot.as_deref() {
        Some(d) if !d.is_empty() => view::KnownPaths::for_site(paths, d),
        _ => view::KnownPaths::for_app(paths),
    };
    raw.lines().map(|l| view::scrub_log_line(l, &known)).collect::<Vec<_>>().join("\n")
}

/// The `SiteCreator` the scratch tools run through: the app's OWN provision job,
/// reached via the app handle.
///
/// This exists to erase the `tauri::Runtime` generic — a `static` registry of fn
/// pointers cannot be generic, and the provision path is. Erasing it here rather
/// than re-implementing creation for agents is what keeps the one-brain rule: a
/// scratch site is built by exactly the code that builds a site the user asks
/// for, ownership being the only difference.
struct AppSiteCreator<Rt: tauri::Runtime> {
    app: tauri::AppHandle<Rt>,
    /// Where the poll loops report, when the client asked (P6.3).
    progress: Option<ProgressSink>,
}

impl<Rt: tauri::Runtime> AppSiteCreator<Rt> {
    /// The app's `State` handles, straight off the handle — so a parity op
    /// calls the COMMAND the dialog calls, with the same arguments, and no
    /// second implementation of a site operation exists for agents.
    fn state(&self) -> crate::error::Result<tauri::State<'_, AppState>> {
        use tauri::Manager;
        self.app
            .try_state::<AppState>()
            .ok_or_else(|| crate::error::Error::Other("rexenv is still starting — try again in a moment".into()))
    }
    fn tunnels(&self) -> crate::error::Result<tauri::State<'_, crate::commands::tunnels::Tunnels>> {
        use tauri::Manager;
        self.app.try_state::<crate::commands::tunnels::Tunnels>().ok_or_else(|| {
            crate::error::Error::Other(
                "rexenv cannot change sites right now — the person you're working with may need to restart it.".into(),
            )
        })
    }
    fn db_jobs(&self) -> crate::error::Result<tauri::State<'_, crate::commands::db_import::DbImportJobs>> {
        use tauri::Manager;
        self.app.try_state::<crate::commands::db_import::DbImportJobs>()
            .ok_or_else(|| crate::error::Error::Other("rexenv's import jobs are not ready".into()))
    }
    fn jobs(&self) -> crate::error::Result<tauri::State<'_, crate::commands::site_provision::ProvisionJobs>> {
        use tauri::Manager;
        self.app.try_state::<crate::commands::site_provision::ProvisionJobs>().ok_or_else(|| {
            crate::error::Error::Other(
                "rexenv cannot provision sites right now — its provisioning service is not running.".into(),
            )
        })
    }
}

impl<Rt: tauri::Runtime> scratch::SiteDeleter for AppSiteCreator<Rt> {
    fn delete<'a>(
        &'a self,
        id: String,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::error::Result<()>> + Send + 'a>>
    {
        use tauri::Manager;
        Box::pin(async move {
            let state = self
                .app
                .try_state::<AppState>()
                .ok_or_else(|| crate::error::Error::Other("rexenv is still starting".into()))?;
            let tunnels = self
                .app
                .try_state::<crate::commands::tunnels::Tunnels>()
                .ok_or_else(|| crate::error::Error::Other(
                    "rexenv cannot delete sites right now — the person you're working with may \
                     need to restart it."
                        .into(),
                ))?;
            // The app's OWN delete: stops the tunnel, drops the database by
            // provenance, tears down the docroot by `docroot_managed`, reloads.
            crate::commands::sites::delete_site_owned(state.inner(), tunnels.inner(), id).await?;
            Ok(())
        })
    }
}

impl<Rt: tauri::Runtime> scratch::SiteCreator for AppSiteCreator<Rt> {
    fn create<'a>(
        &'a self,
        new: crate::state::models::NewSite,
        ownership: crate::core::sites::Ownership,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<
                        crate::state::models::Site,
                        crate::commands::sites::CreateFailure,
                    >,
                > + Send
                + 'a,
        >,
    > {
        use tauri::Manager;
        Box::pin(async move {
            let state = self.app.try_state::<AppState>().ok_or_else(|| {
                crate::commands::sites::CreateFailure {
                    site_id: None,
                    error: crate::error::Error::Other("rexenv is still starting — try again in a moment".into()),
                }
            })?;
            let jobs = self
                .app
                .try_state::<crate::commands::site_provision::ProvisionJobs>()
                .ok_or_else(|| crate::commands::sites::CreateFailure {
                    site_id: None,
                    error: crate::error::Error::Other(
                        "rexenv cannot create sites right now — its provisioning service is not \
                         running. The person you're working with may need to restart rexenv."
                            .into(),
                    ),
                })?;
            let sink = self.progress.clone();
            let report = move |st: &crate::commands::site_provision::SiteProvisionState| {
                if let Some(s) = &sink {
                    s.report(f64::from(st.pct), Some(100.0), &provision_message(st));
                }
            };
            crate::commands::sites::create_site_owned_with(
                self.app.clone(),
                state.inner(),
                jobs.inner(),
                new,
                None,
                None,
                ownership,
                Some(&report),
            )
            .await
        })
    }
}

impl<Rt: tauri::Runtime> user_sites::SiteOps for AppSiteCreator<Rt> {
    fn create<'a>(
        &'a self,
        new: crate::state::models::NewSite,
        wp: Option<crate::core::wordpress::InstallOptions>,
        blueprint_id: Option<String>,
        ownership: crate::core::sites::Ownership,
    ) -> user_sites::OpFuture<'a, Result<crate::state::models::Site, crate::commands::sites::CreateFailure>> {
        use tauri::Manager;
        Box::pin(async move {
            let state = self.app.try_state::<AppState>().ok_or_else(|| crate::commands::sites::CreateFailure {
                site_id: None,
                error: crate::error::Error::Other("rexenv is still starting — try again in a moment".into()),
            })?;
            let jobs = self
                .app
                .try_state::<crate::commands::site_provision::ProvisionJobs>()
                .ok_or_else(|| crate::commands::sites::CreateFailure {
                    site_id: None,
                    error: crate::error::Error::Other(
                        "rexenv cannot create sites right now — its provisioning service is not \
                         running. The person you're working with may need to restart rexenv."
                            .into(),
                    ),
                })?;
            let sink = self.progress.clone();
            let report = move |st: &crate::commands::site_provision::SiteProvisionState| {
                if let Some(s) = &sink {
                    s.report(f64::from(st.pct), Some(100.0), &provision_message(st));
                }
            };
            crate::commands::sites::create_site_owned_with(self.app.clone(), state.inner(), jobs.inner(), new, wp, blueprint_id, ownership, Some(&report))
                .await
        })
    }

    fn delete<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        <Self as scratch::SiteDeleter>::delete(self, id)
    }

    fn share_start<'a>(&'a self, id: String, minutes: u64) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::tunnels::TunnelInfo>> {
        Box::pin(async move {
            let info = crate::commands::tunnels::start_tunnel(self.app.clone(), self.state()?, self.tunnels()?, self.jobs()?, id.clone()).await?;
            // The auto-stop: a bounded timer in the app, which also dies with the
            // app (every tunnel is swept at launch and stopped on quit — #29's
            // family), so an agent-started share can never become a fossil.
            //
            // **It ends the share it STARTED, and no other** (ledger #513). The
            // timer fires up to an hour later, and in that hour the ordinary
            // sequence is: the agent's share is stopped (by the agent, or it
            // died), and the PERSON shares the same site again from the Tunnels
            // page. A stop keyed on the site alone would then end the user's
            // share at the agent's deadline — rexenv stopping a share on the
            // user's behalf, which #29 forbids. The public URL is unique per
            // tunnel, so it is the identity: the timer re-reads the registry
            // and stops only if the site's live share still carries the URL
            // this call handed out.
            let started_url = info.url.clone();
            let domain = info.domain.clone();
            let app = self.app.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(minutes * 60)).await;
                use tauri::Manager;
                let (Some(state), Some(tunnels)) = (app.try_state::<AppState>(), app.try_state::<crate::commands::tunnels::Tunnels>()) else {
                    return;
                };
                let live = match crate::commands::tunnels::tunnels_status(state, tunnels).await {
                    Ok(list) => list,
                    Err(e) => {
                        log::warn!("mcp: the {minutes}-minute share of {id} could not be checked at its deadline: {e}");
                        return;
                    }
                };
                let live_url = live.iter().find(|t| t.domain == domain).map(|t| t.url.as_str());
                if !auto_stop_applies(&started_url, live_url) {
                    log::info!("mcp: the {minutes}-minute share of {id} is not the one running any more — leaving it alone");
                    return;
                }
                let (Some(state), Some(tunnels)) = (app.try_state::<AppState>(), app.try_state::<crate::commands::tunnels::Tunnels>()) else {
                    return;
                };
                if let Err(e) = crate::commands::tunnels::stop_tunnel(state, tunnels, id.clone()).await {
                    log::warn!("mcp: the {minutes}-minute share of {id} could not be stopped on time: {e}");
                } else {
                    log::info!("mcp: stopped the {minutes}-minute share of {id} on time");
                }
            });
            Ok(info)
        })
    }

    fn share_stop<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::tunnels::stop_tunnel(self.state()?, self.tunnels()?, id).await })
    }
    fn shares<'a>(&'a self) -> user_sites::OpFuture<'a, crate::error::Result<Vec<crate::commands::tunnels::TunnelInfo>>> {
        Box::pin(async move { crate::commands::tunnels::tunnels_status(self.state()?, self.tunnels()?).await })
    }
    fn save_blueprint<'a>(&'a self, bp: crate::state::models::Blueprint) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::blueprints::save_blueprint(self.state()?, bp) })
    }
    fn delete_blueprint<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<bool>> {
        Box::pin(async move { crate::commands::blueprints::delete_blueprint(self.state()?, id) })
    }

    fn rename<'a>(&'a self, id: String, name: String) -> user_sites::OpFuture<'a, crate::error::Result<Option<crate::state::models::Site>>> {
        Box::pin(async move { crate::commands::sites::rename_site(self.state()?, id, name) })
    }
    fn change_domain<'a>(&'a self, id: String, domain: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::sites::DomainChange>> {
        Box::pin(async move { crate::commands::sites::change_site_domain(self.state()?, self.tunnels()?, id, domain).await })
    }
    fn add_domain<'a>(&'a self, id: String, domain: String) -> user_sites::OpFuture<'a, crate::error::Result<Vec<String>>> {
        Box::pin(async move { crate::commands::sites::add_site_domain(self.state()?, id, domain).await })
    }
    fn remove_domain<'a>(&'a self, id: String, domain: String) -> user_sites::OpFuture<'a, crate::error::Result<Vec<String>>> {
        Box::pin(async move { crate::commands::sites::remove_site_domain(self.state()?, id, domain).await })
    }
    fn set_php<'a>(&'a self, id: String, version: String) -> user_sites::OpFuture<'a, crate::error::Result<Option<crate::state::models::Site>>> {
        Box::pin(async move { crate::commands::sites::set_site_php_version(self.state()?, id, version).await })
    }
    fn set_server<'a>(&'a self, id: String, server: crate::state::models::WebServer) -> user_sites::OpFuture<'a, crate::error::Result<Option<crate::state::models::Site>>> {
        Box::pin(async move { crate::commands::sites::set_site_web_server(self.state()?, self.tunnels()?, id, server).await })
    }
    fn set_xdebug<'a>(&'a self, id: String, enabled: bool) -> user_sites::OpFuture<'a, crate::error::Result<Option<crate::state::models::Site>>> {
        Box::pin(async move { crate::commands::sites::set_site_xdebug(self.state()?, id, enabled).await })
    }
    fn set_enabled<'a>(&'a self, id: String, enabled: bool) -> user_sites::OpFuture<'a, crate::error::Result<Option<crate::commands::sites::SiteEnabledReport>>> {
        // The mechanism, NOT `set_site_enabled`: the Tauri command promotes a
        // scratch site, so an agent could adopt its own disposable site by
        // stopping and starting it — a cap bypass with a very plausible face.
        Box::pin(async move { crate::commands::sites::set_enabled(self.state()?.inner(), &id, enabled).await })
    }
    fn list_env<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<Vec<crate::commands::sites::EnvVarInput>>> {
        Box::pin(async move { crate::commands::sites::list_site_env(self.state()?, id) })
    }
    fn set_env<'a>(&'a self, id: String, vars: Vec<crate::commands::sites::EnvVarInput>) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::sites::set_site_env(self.state()?, id, vars).await })
    }
    fn move_docroot<'a>(&'a self, id: String, dest_parent: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::state::models::Site>> {
        Box::pin(async move { crate::commands::sites::move_site_docroot(self.state()?, self.tunnels()?, id, dest_parent).await })
    }
    fn relink<'a>(&'a self, id: String, path: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::state::models::Site>> {
        Box::pin(async move { crate::commands::sites::relink_site_docroot(self.state()?, self.tunnels()?, id, path).await })
    }
    fn regenerate_cert<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::sites::regenerate_site_cert(self.state()?, id).await })
    }
    fn restart<'a>(&'a self, id: String, pool: bool) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::sites::SiteRestartReport>> {
        Box::pin(async move { crate::commands::sites::restart_site(self.state()?, id, pool).await })
    }
    fn worktree_children<'a>(&'a self, parent_id: String) -> user_sites::OpFuture<'a, crate::error::Result<Vec<crate::commands::worktree::WorktreeView>>> {
        Box::pin(async move { crate::commands::worktree::worktree_children(self.app.clone(), parent_id).await })
    }
    fn worktree_preview<'a>(&'a self, req: crate::commands::worktree::WorktreeRequest) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::worktree::WorktreePreview>> {
        Box::pin(async move { crate::commands::worktree::worktree_preview(self.state()?, req).await })
    }
    fn worktree_create<'a>(&'a self, req: crate::commands::worktree::WorktreeRequest) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::site_provision::SiteProvisionState>> {
        Box::pin(async move {
            let (state, jobs) = (self.state()?, self.jobs()?);
            let started = crate::commands::worktree::start(&self.app, state.inner(), jobs.inner(), req)?;
            // The OUTCOME, as `retry` answers (the first snapshot says `running`).
            crate::commands::site_provision::settle(&jobs, &started.id, None).await
        })
    }
    fn worktree_remove<'a>(&'a self, id: String, force: bool) -> user_sites::OpFuture<'a, crate::error::Result<bool>> {
        Box::pin(async move { crate::commands::worktree::worktree_remove(self.state()?, self.tunnels()?, id, force).await })
    }
    fn retry<'a>(&'a self, site_id: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::site_provision::SiteProvisionState>> {
        Box::pin(async move {
            let started =
                crate::commands::site_provision::site_provision_retry(self.app.clone(), self.state()?, self.jobs()?, site_id)
                    .await?;
            // The tool answers with the OUTCOME. `site_provision_retry` returns the
            // job's first snapshot — right for the card, which streams — and
            // handing that back told an agent `running` with every phase pending.
            let jobs = self.jobs()?;
            crate::commands::site_provision::settle(&jobs, &started.id, None).await
        })
    }

    fn multisite_convert<'a>(&'a self, id: String, mode: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        use tauri::Manager;
        Box::pin(async move {
            // The app's OWN command, with its share guard — `State` handles come
            // straight off the app handle, so this IS the dialog's post-create path.
            let state = self.app.try_state::<AppState>()
                .ok_or_else(|| crate::error::Error::Other("rexenv is still starting".into()))?;
            let tunnels = self.app.try_state::<crate::commands::tunnels::Tunnels>()
                .ok_or_else(|| crate::error::Error::Other("rexenv cannot convert sites right now".into()))?;
            crate::commands::wordpress::wp_multisite_convert(state, tunnels, id, mode).await.map(|_| ())
        })
    }
    fn log_clear<'a>(&'a self, key: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::logs::log_clear(self.state()?, key) })
    }
    fn wp_debug_log_clear<'a>(&'a self, site_id: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::logs::wp_debug_log_clear(self.state()?, site_id) })
    }
}

impl<Rt: tauri::Runtime> user_sites::WpOps for AppSiteCreator<Rt> {
    fn info<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::core::wordpress::WpInfo>> {
        Box::pin(async move { crate::commands::wordpress::wp_info(self.state()?, id).await })
    }
    fn plugins<'a>(&'a self, id: String, check_updates: bool) -> user_sites::OpFuture<'a, crate::error::Result<Vec<crate::core::wordpress::WpPlugin>>> {
        Box::pin(async move { crate::commands::wordpress::wp_plugins(self.state()?, id, Some(check_updates)).await })
    }
    fn plugin_activate<'a>(&'a self, id: String, names: Vec<String>) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_plugin_activate(self.state()?, id, names).await })
    }
    fn plugin_deactivate<'a>(&'a self, id: String, names: Vec<String>) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_plugin_deactivate(self.state()?, id, names).await })
    }
    fn plugin_update<'a>(&'a self, id: String, names: Vec<String>) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_plugin_update(self.app.clone(), self.state()?, id, names).await })
    }
    fn plugin_delete<'a>(&'a self, id: String, names: Vec<String>) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_plugin_delete(self.state()?, id, names).await })
    }
    fn plugin_activate_network<'a>(&'a self, id: String, names: Vec<String>) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_plugin_activate_network(self.state()?, id, names).await })
    }
    fn plugin_deactivate_network<'a>(&'a self, id: String, names: Vec<String>) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_plugin_deactivate_network(self.state()?, id, names).await })
    }
    fn themes<'a>(&'a self, id: String, check_updates: bool) -> user_sites::OpFuture<'a, crate::error::Result<Vec<crate::core::wordpress::WpTheme>>> {
        Box::pin(async move { crate::commands::wordpress::wp_themes(self.state()?, id, Some(check_updates)).await })
    }
    fn theme_activate<'a>(&'a self, id: String, name: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_theme_activate(self.state()?, id, name).await })
    }
    fn theme_update<'a>(&'a self, id: String, names: Vec<String>) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_theme_update(self.app.clone(), self.state()?, id, names).await })
    }
    fn theme_delete<'a>(&'a self, id: String, names: Vec<String>) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_theme_delete(self.state()?, id, names).await })
    }
    fn themes_network_enabled<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<Vec<String>>> {
        Box::pin(async move { crate::commands::wordpress::wp_themes_network_enabled(self.state()?, id).await })
    }
    fn theme_enable_network<'a>(&'a self, id: String, name: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_theme_enable_network(self.state()?, id, name).await })
    }
    fn theme_disable_network<'a>(&'a self, id: String, name: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_theme_disable_network(self.state()?, id, name).await })
    }
    fn options<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::core::wordpress::WpOptionsForm>> {
        Box::pin(async move { crate::commands::wordpress::wp_options(self.state()?, id).await })
    }
    fn debug_get<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<bool>> {
        Box::pin(async move { crate::commands::wordpress::wp_debug_get(self.state()?, id).await })
    }
    fn debug_flag_get<'a>(&'a self, id: String, name: String) -> user_sites::OpFuture<'a, crate::error::Result<bool>> {
        Box::pin(async move { crate::commands::wordpress::wp_debug_flag_get(self.state()?, id, name).await })
    }
    fn maintenance_get<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<bool>> {
        Box::pin(async move { crate::commands::wordpress::wp_maintenance_get(self.state()?, id).await })
    }
    fn permalink_get<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<String>> {
        Box::pin(async move { crate::commands::wordpress::wp_permalink_get(self.state()?, id).await })
    }
    fn languages<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<Vec<crate::core::wordpress::WpLanguage>>> {
        Box::pin(async move { crate::commands::wordpress::wp_languages(self.state()?, id).await })
    }
    fn cron_events<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<Vec<crate::core::wordpress::WpCronEvent>>> {
        Box::pin(async move { crate::commands::wordpress::wp_cron_events(self.state()?, id).await })
    }
    fn core_verify_checksums<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::core::wordpress::WpChecksumReport>> {
        Box::pin(async move { crate::commands::wordpress::wp_core_verify_checksums(self.state()?, id).await })
    }
    fn primary_admin<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<u64>> {
        Box::pin(async move { crate::commands::wordpress::wp_primary_admin(self.state()?, id).await })
    }
    fn core_versions<'a>(&'a self) -> user_sites::OpFuture<'a, crate::error::Result<Vec<crate::core::wordpress::WpCoreVersion>>> {
        Box::pin(async move { crate::commands::wordpress::wp_core_versions().await })
    }
    fn cli_packages<'a>(&'a self) -> user_sites::OpFuture<'a, crate::error::Result<Option<crate::commands::wordpress::WpCliPackagesView>>> {
        Box::pin(async move { crate::commands::wordpress::wp_cli_packages(self.state()?, self.repo_jobs()?).await })
    }
    fn users<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<Vec<crate::core::wordpress::WpUser>>> {
        Box::pin(async move { crate::commands::wordpress::wp_users(self.state()?, id).await })
    }
    fn user_create<'a>(&'a self, id: String, login: String, email: String, role: String, password: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_user_create(self.state()?, id, login, email, role, password).await })
    }
    fn user_set_password<'a>(&'a self, id: String, user_id: u64, password: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_user_set_password(self.state()?, id, user_id, password).await })
    }
    fn user_set_role<'a>(&'a self, id: String, user_id: u64, role: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_user_set_role(self.state()?, id, user_id, role).await })
    }
    fn user_delete<'a>(&'a self, id: String, user_id: u64, reassign: Option<u64>, delete_posts: bool) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_user_delete(self.state()?, id, user_id, reassign, delete_posts).await })
    }
    fn user_login_url<'a>(&'a self, id: String, user_id: u64) -> user_sites::OpFuture<'a, crate::error::Result<String>> {
        Box::pin(async move { crate::commands::wordpress::wp_user_login_url(self.state()?, id, user_id).await })
    }
    fn admin_login_url<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<String>> {
        Box::pin(async move { crate::commands::wordpress::wp_admin_login_url(self.state()?, id, None).await })
    }
    fn super_admins<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<Vec<String>>> {
        Box::pin(async move { crate::commands::wordpress::wp_super_admins(self.state()?, id).await })
    }
    fn super_admin_add<'a>(&'a self, id: String, user: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_super_admin_add(self.state()?, id, user).await })
    }
    fn option_update<'a>(&'a self, id: String, name: String, value: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_option_update(self.state()?, id, name, value).await })
    }
    fn debug_set<'a>(&'a self, id: String, on: bool) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_debug_set(self.state()?, id, on).await })
    }
    fn debug_flag_set<'a>(&'a self, id: String, name: String, on: bool) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_debug_flag_set(self.state()?, id, name, on).await })
    }
    fn maintenance_set<'a>(&'a self, id: String, on: bool) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_maintenance_set(self.state()?, id, on).await })
    }
    fn permalink_set<'a>(&'a self, id: String, structure: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_permalink_set(self.state()?, id, structure).await })
    }
    fn switch_language<'a>(&'a self, id: String, locale: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_switch_language(self.state()?, id, locale).await })
    }
    fn cache_flush<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<String>> {
        Box::pin(async move { crate::commands::wordpress::wp_cache_flush(self.state()?, id).await })
    }
    fn rewrite_flush<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_rewrite_flush(self.state()?, id).await })
    }
    fn transient_delete_all<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<String>> {
        Box::pin(async move { crate::commands::wordpress::wp_transient_delete_all(self.state()?, id).await })
    }
    fn cron_run_due<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<String>> {
        Box::pin(async move { crate::commands::wordpress::wp_cron_run_due(self.state()?, id).await })
    }
    fn cron_run_hook<'a>(&'a self, id: String, hook: String) -> user_sites::OpFuture<'a, crate::error::Result<String>> {
        Box::pin(async move { crate::commands::wordpress::wp_cron_run_hook(self.state()?, id, hook).await })
    }
    fn checksum_cleanup<'a>(&'a self, id: String, paths: Vec<String>) -> user_sites::OpFuture<'a, crate::error::Result<crate::core::wordpress::ChecksumCleanup>> {
        Box::pin(async move { crate::commands::wordpress::wp_checksum_cleanup(self.state()?, id, paths).await })
    }
    fn core_update<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<String>> {
        Box::pin(async move { crate::commands::wordpress::wp_core_update(self.app.clone(), self.state()?, id).await })
    }
    fn core_reinstall<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<String>> {
        Box::pin(async move { crate::commands::wordpress::wp_core_reinstall(self.state()?, id).await })
    }
    fn core_repair<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<String>> {
        Box::pin(async move { crate::commands::wordpress::wp_core_repair_cut_names(self.state()?, id).await })
    }
    fn core_switch_version<'a>(&'a self, id: String, version: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::core::wordpress::WpCoreSwitch>> {
        Box::pin(async move { crate::commands::wordpress::wp_core_switch_version(self.state()?, id, version).await })
    }
    fn db_export<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<String>> {
        Box::pin(async move { crate::commands::wordpress::wp_db_export(self.state()?, id).await })
    }
    fn content_export<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<Vec<String>>> {
        Box::pin(async move { crate::commands::wordpress::wp_content_export(self.state()?, id).await })
    }
    fn search_replace<'a>(&'a self, id: String, from: String, to: String, dry_run: bool) -> user_sites::OpFuture<'a, crate::error::Result<u64>> {
        Box::pin(async move { crate::commands::wordpress::wp_search_replace(self.state()?, id, from, to, dry_run).await })
    }
    fn db_import<'a>(&'a self, id: String, path: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_db_import(self.state()?, id, path).await })
    }
    fn site_reset<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_site_reset(self.state()?, id).await })
    }
    fn network_sites<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<Vec<crate::core::wordpress::WpNetworkSite>>> {
        Box::pin(async move { crate::commands::wordpress::wp_network_sites(self.state()?, id).await })
    }
    fn network_site_create<'a>(&'a self, id: String, slug: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_network_site_create(self.state()?, id, slug).await })
    }
    fn network_site_delete<'a>(&'a self, id: String, blog_id: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::wordpress::wp_network_site_delete(self.state()?, id, blog_id).await })
    }
}

impl<Rt: tauri::Runtime> user_sites::MailOps for AppSiteCreator<Rt> {
    fn list<'a>(&'a self, query: Option<String>, unread_only: bool) -> user_sites::OpFuture<'a, crate::error::Result<crate::core::mail::MailList>> {
        Box::pin(async move { crate::commands::mail::mailpit_messages(self.state()?, query, Some(unread_only)).await })
    }
    fn detail<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::core::mail::MailDetail>> {
        Box::pin(async move { crate::commands::mail::mailpit_message(self.state()?, id).await })
    }
    fn raw<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<String>> {
        Box::pin(async move { crate::commands::mail::mailpit_message_raw(self.state()?, id).await })
    }
    fn mark_all_read<'a>(&'a self) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::mail::mailpit_mark_all_read(self.state()?).await })
    }
    fn clear<'a>(&'a self) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::mail::mailpit_clear(self.state()?).await })
    }
    fn delete<'a>(&'a self, ids: Vec<String>) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::mail::mailpit_delete(self.state()?, ids).await })
    }
}

impl<Rt: tauri::Runtime> user_sites::StackOps for AppSiteCreator<Rt> {
    // The two calls below reach `run_privileged` (the edge daemon). They are
    // reachable from ONE tool arm, behind the `system` scope, and the macOS
    // dialog they raise is a consent the agent cannot give — stated in the
    // tool's description and in ledger #485.
    fn start_all<'a>(&'a self) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::services::start_services(self.state()?).await })
    }
    fn stop_all<'a>(&'a self) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::services::stop_services(self.state()?, self.jobs()?).await })
    }
    fn restart_web<'a>(&'a self, target: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::services::WebRestartReport>> {
        Box::pin(async move { crate::commands::services::restart_web_service(self.state()?, target).await })
    }
    fn start_database<'a>(&'a self, key: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::database::start_database(self.state()?, key).await })
    }
    fn stop_database<'a>(&'a self, key: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::database::stop_database(self.state()?, key).await })
    }
    fn start_mail<'a>(&'a self) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::mail::start_mail(self.state()?).await })
    }
    fn stop_mail<'a>(&'a self) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::mail::stop_mail(self.state()?).await })
    }
    fn set_all_sites_enabled<'a>(
        &'a self,
        enabled: bool,
    ) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::sites::BulkEnabledReport>> {
        Box::pin(async move {
            crate::commands::sites::set_all_enabled(self.state()?.inner(), enabled).await
        })
    }
    fn set_mail_catch_all<'a>(&'a self, enabled: bool) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::mail::set_mail_catch_all(self.state()?, enabled).await })
    }
    fn set_engine_version<'a>(&'a self, key: String, version: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::database::set_db_engine_version(self.state()?, key, version).await })
    }
    fn adminer_status<'a>(&'a self) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::database::AdminerStatus>> {
        Box::pin(async move { crate::commands::database::adminer_status(self.state()?) })
    }
    fn adminer_update_check<'a>(&'a self) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::database::AdminerStatus>> {
        Box::pin(async move { crate::commands::database::adminer_update_check(self.state()?).await })
    }
    fn adminer_update_apply<'a>(&'a self, version: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::database::AdminerStatus>> {
        Box::pin(async move { crate::commands::database::adminer_update_apply(self.state()?, version).await })
    }
    fn adminer_set_theme<'a>(&'a self, theme: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::database::adminer_set_theme(self.state()?, theme) })
    }
    fn downloads<'a>(&'a self) -> user_sites::OpFuture<'a, crate::error::Result<(Vec<crate::commands::downloads::PlannedInfo>, crate::core::downloads::Snapshot)>> {
        Box::pin(async move {
            let plan = crate::commands::downloads::core_binaries_plan(self.state()?)?;
            Ok((plan, crate::commands::downloads::downloads_state()))
        })
    }
    fn prefetch<'a>(&'a self) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::downloads::prefetch_core_binaries(self.state()?).await })
    }
    fn retry_download<'a>(&'a self, name: String, version: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::downloads::retry_download(self.state()?, name, version).await })
    }
    fn set_autostart<'a>(&'a self, enabled: bool) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::system::set_autostart(self.state()?, enabled) })
    }
    fn edge_conflict<'a>(&'a self) -> user_sites::OpFuture<'a, crate::error::Result<Option<crate::commands::system::SetupEdgeConflict>>> {
        Box::pin(async move { crate::commands::system::setup_edge_conflict(self.state()?).await })
    }
}

impl<Rt: tauri::Runtime> user_sites::SystemOps for AppSiteCreator<Rt> {
    fn set_setting<'a>(&'a self, key: String, value: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::settings::set_setting(self.state()?, key, value) })
    }
    // The three resolver writes reach `run_privileged` (an /etc/resolver file):
    // `system` scope + the macOS dialog, exactly as the stack's start/stop.
    fn set_default_tld<'a>(&'a self, tld: String) -> user_sites::OpFuture<'a, crate::error::Result<String>> {
        Box::pin(async move { crate::commands::settings::set_default_tld(self.state()?, tld) })
    }
    fn repair_resolver<'a>(&'a self, tld: String) -> user_sites::OpFuture<'a, crate::error::Result<String>> {
        Box::pin(async move { crate::commands::system::repair_resolver(self.state()?, tld).await.map(|o| o.action.as_str().to_string()) })
    }
    fn remove_resolver<'a>(&'a self, tld: String) -> user_sites::OpFuture<'a, crate::error::Result<bool>> {
        Box::pin(async move { crate::commands::system::remove_resolver(self.state()?, tld).await })
    }
    fn set_php_installed<'a>(&'a self, minor: String, installed: bool) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::php::set_php_version_installed(self.state()?, minor, installed).await })
    }
    fn set_default_php<'a>(&'a self, minor: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::php::set_default_php_version(self.state()?, minor) })
    }
    fn apply_php_settings<'a>(&'a self, minor: String, settings: Vec<crate::commands::php::PhpSettingInput>) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::php::apply_php_settings(self.state()?, minor, settings).await })
    }
    fn php_update_check<'a>(&'a self) -> user_sites::OpFuture<'a, crate::error::Result<Vec<crate::state::models::PhpVersionView>>> {
        Box::pin(async move { crate::commands::php::php_update_check(self.state()?).await })
    }
    fn php_update_apply<'a>(&'a self, minor: String, patch: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::php::PhpUpdateOutcome>> {
        Box::pin(async move { crate::commands::php::php_update_apply(self.state()?, minor, patch).await })
    }
    fn browsers<'a>(&'a self) -> user_sites::OpFuture<'a, Vec<crate::platform::traits::BrowserApp>> {
        Box::pin(async move { self.state().map(crate::commands::system::list_browsers).unwrap_or_default() })
    }
    fn open_in_browser<'a>(&'a self, browser_id: String, url: String, private: bool) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::system::open_in_browser(self.state()?, browser_id, url, private) })
    }
    fn editors<'a>(&'a self) -> user_sites::OpFuture<'a, Vec<crate::platform::traits::EditorApp>> {
        Box::pin(async move { self.state().map(crate::commands::system::list_editors).unwrap_or_default() })
    }
    fn open_in_editor<'a>(&'a self, editor_id: String, path: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::system::open_in_editor(self.state()?, editor_id, path) })
    }
    fn reveal_path<'a>(&'a self, path: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::system::reveal_path(self.state()?, path) })
    }
    fn resolver_tld_status<'a>(&'a self, tld: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::valet_import::ResolverTldStatus>> {
        Box::pin(async move { crate::commands::valet_import::resolver_tld_status(self.state()?, tld) })
    }
    fn terminals<'a>(&'a self) -> user_sites::OpFuture<'a, Vec<crate::platform::traits::TerminalApp>> {
        Box::pin(async move { self.state().map(crate::commands::terminal::list_terminals).unwrap_or_default() })
    }
    fn open_in_terminal<'a>(&'a self, site_id: String, terminal_id: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::terminal::terminal_open_external(self.state()?, site_id, terminal_id, None) })
    }
}

impl<Rt: tauri::Runtime> AppSiteCreator<Rt> {
    fn repo_jobs(&self) -> crate::error::Result<tauri::State<'_, crate::commands::repo::RepoJobs>> {
        use tauri::Manager;
        self.app.try_state::<crate::commands::repo::RepoJobs>().ok_or_else(|| crate::error::Error::Other("rexenv's repo jobs are not ready".into()))
    }
    fn repo_watches(&self) -> crate::error::Result<tauri::State<'_, crate::commands::repo::RepoWatches>> {
        use tauri::Manager;
        self.app.try_state::<crate::commands::repo::RepoWatches>().ok_or_else(|| crate::error::Error::Other("rexenv's watch registry is not ready".into()))
    }
    /// Poll a repo job until it settles — the CLI's own rule (`repo_job_settled`),
    /// with a ceiling so a wedged job cannot hold an MCP session forever.
    async fn settle(&self, job_id: &str, waiting_for: Option<&str>) -> crate::error::Result<crate::commands::repo::RepoJobState> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30 * 60);
        let mut last: Option<String> = None;
        loop {
            let st = crate::commands::repo::repo_job_state(self.repo_jobs()?, job_id.to_string()).await?;
            if let Some(sink) = &self.progress {
                // Steps done over steps total, the running step's label as the message.
                let total = st.steps.len();
                let done = st.steps.iter().filter(|s| !matches!(s.status.as_str(), "pending" | "running")).count();
                let running = st.steps.iter().find(|s| s.status == "running").map(|s| s.label.clone()).unwrap_or_else(|| st.op.clone());
                let key = format!("{done}/{total}:{running}");
                if last.as_deref() != Some(&key) {
                    last = Some(key);
                    sink.report(done as f64, Some(total as f64), &running);
                }
            }
            if crate::commands::repo::repo_job_settled(&st, waiting_for) {
                return Ok(st);
            }
            if std::time::Instant::now() > deadline {
                return Err(crate::error::Error::Other(format!("job {job_id} has not settled after 30 minutes — it keeps running in rexenv; read it later with repo `job`.")));
            }
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
    }
}

impl<Rt: tauri::Runtime> user_sites::RepoOps for AppSiteCreator<Rt> {
    fn composer_link<'a>(&'a self, site_id: String, link: crate::core::laravel::ComposerLink) -> user_sites::OpFuture<'a, crate::error::Result<user_sites::ComposerRun>> {
        Box::pin(async move {
            let state = self.state()?;
            let (project, minor) = {
                let conn = state.db.lock().map_err(|_| crate::error::Error::Other("database lock poisoned".into()))?;
                let site = crate::state::store::get_site(&conn, &site_id)?
                    .ok_or_else(|| crate::error::Error::Other(format!("no site {site_id}")))?;
                (std::path::PathBuf::from(&site.path), site.php_version)
            };
            // The site's PHP and the pinned phar — never a system `composer`.
            let (php, composer) = crate::commands::wordpress::composer_tools(&state, &minor).await?;
            let jobs = self.repo_jobs()?;
            let env = crate::commands::repo::shell_env(state.inner(), &jobs, false)?;
            let app = self.app.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let state = tauri::Manager::state::<AppState>(&app);
                let mut log = Vec::new();
                let cancel = crate::core::repo::CancelToken::new();
                let outcome = crate::core::laravel::composer_link(
                    state.platform.supervisor(), &php, &composer, &project, &link, &env, &cancel,
                    &mut |l| log.push(l.to_string()),
                );
                let ok = match outcome {
                    Ok(()) => true,
                    Err(e) => { log.push(e.to_string()); false }
                };
                Ok(user_sites::ComposerRun { ok, log })
            })
            .await
            .map_err(|e| crate::error::Error::Other(format!("the Composer thread ended early: {e}")))?
        })
    }
    fn assets<'a>(&'a self, site_id: String) -> user_sites::OpFuture<'a, crate::error::Result<Vec<crate::state::models::GitAsset>>> {
        Box::pin(async move { crate::commands::repo::repo_assets(self.state()?, site_id).await })
    }
    fn asset_status<'a>(&'a self, site_id: String, kind: String, dir: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::repo::AssetStatusResult>> {
        Box::pin(async move { crate::commands::repo::repo_asset_status(self.app.clone(), site_id, kind, dir).await })
    }
    fn branches<'a>(&'a self, site_id: String, kind: String, dir: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::repo::RepoBranches>> {
        Box::pin(async move { crate::commands::repo::repo_branches(self.app.clone(), site_id, kind, dir).await })
    }
    fn pull_refs<'a>(&'a self, site_id: String, kind: String, dir: String) -> user_sites::OpFuture<'a, crate::error::Result<Vec<crate::core::repo::PullRef>>> {
        Box::pin(async move { crate::commands::repo::repo_pull_refs(self.app.clone(), site_id, kind, dir).await })
    }
    fn stashes<'a>(&'a self, site_id: String, kind: String, dir: String) -> user_sites::OpFuture<'a, crate::error::Result<Vec<crate::core::repo::StashEntry>>> {
        Box::pin(async move { crate::commands::repo::repo_stashes(self.app.clone(), site_id, kind, dir).await })
    }
    fn scripts<'a>(&'a self, site_id: String, kind: String, dir: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::repo::RepoScriptsInfo>> {
        Box::pin(async move { crate::commands::repo::repo_scripts(self.state()?, site_id, kind, dir).await })
    }
    fn site_info<'a>(&'a self, site_id: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::repo::SiteRepoInfo>> {
        Box::pin(async move { crate::commands::repo::repo_site_info(self.state()?, site_id).await })
    }
    fn site_jobs<'a>(&'a self, site_id: String, kind: String) -> user_sites::OpFuture<'a, crate::error::Result<Vec<crate::commands::repo::RepoJobState>>> {
        Box::pin(async move { crate::commands::repo::repo_site_jobs(self.repo_jobs()?, site_id, kind).await })
    }
    fn job_state<'a>(&'a self, job_id: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::repo::RepoJobState>> {
        Box::pin(async move { crate::commands::repo::repo_job_state(self.repo_jobs()?, job_id).await })
    }
    fn watches<'a>(&'a self, site_id: String) -> user_sites::OpFuture<'a, crate::error::Result<Vec<crate::commands::repo::WatchState>>> {
        Box::pin(async move { crate::commands::repo::repo_watches(self.repo_watches()?, Some(site_id), None).await })
    }
    fn watch_log<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<Vec<String>>> {
        Box::pin(async move { crate::commands::repo::repo_watch_log(self.repo_watches()?, id).await })
    }
    fn unmanaged<'a>(&'a self, site_id: String, kind: String) -> user_sites::OpFuture<'a, crate::error::Result<Vec<crate::core::repo::UnmanagedRepo>>> {
        Box::pin(async move { crate::commands::repo::repo_unmanaged(self.state()?, site_id, kind).await })
    }
    fn check<'a>(&'a self, site_id: String, kind: String, dir: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::repo::RepoJobState>> {
        Box::pin(async move { crate::commands::repo::repo_check(self.app.clone(), self.state()?, self.repo_jobs()?, site_id, kind, dir).await })
    }
    fn tools<'a>(&'a self, refresh: bool) -> user_sites::OpFuture<'a, crate::error::Result<Vec<crate::commands::repo::ToolStatus>>> {
        Box::pin(async move { crate::commands::repo::repo_tools(self.app.clone(), refresh).await })
    }
    fn probe<'a>(&'a self, url: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::repo::RepoProbeResult>> {
        Box::pin(async move { crate::commands::repo::repo_probe(self.app.clone(), url).await })
    }
    fn add<'a>(&'a self, site_id: String, kind: String, url: String, git_ref: Option<String>, dir: Option<String>, install: bool) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::repo::RepoJobState>> {
        Box::pin(async move {
            let snap = crate::commands::repo::repo_add(self.app.clone(), self.state()?, self.repo_jobs()?, site_id, kind, url, git_ref, dir).await?;
            if install { crate::commands::repo::run_offered_steps(self.app.clone(), snap.id).await } else { self.settle(&snap.id, None).await }
        })
    }
    fn adopt<'a>(&'a self, site_id: String, kind: String, dir: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::repo::repo_adopt(self.app.clone(), site_id, kind, dir).await })
    }
    fn link<'a>(&'a self, site_id: String, kind: String, dir: Option<String>, target: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::repo::RepoLinkResult>> {
        Box::pin(async move { crate::commands::repo::repo_link(self.app.clone(), site_id, kind, dir, target).await })
    }
    fn git_op<'a>(&'a self, site_id: String, kind: String, dir: String, op: String, target_ref: Option<String>, install: bool) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::repo::RepoJobState>> {
        Box::pin(async move {
            let snap = crate::commands::repo::repo_git_op(self.app.clone(), self.state()?, self.repo_jobs()?, site_id, kind, dir, op, target_ref).await?;
            if install { crate::commands::repo::run_offered_steps(self.app.clone(), snap.id).await } else { self.settle(&snap.id, None).await }
        })
    }
    fn run_step<'a>(&'a self, job_id: String, step: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::repo::RepoJobState>> {
        Box::pin(async move {
            crate::commands::repo::repo_run_step(self.app.clone(), self.state()?, self.repo_jobs()?, job_id.clone(), step.clone()).await?;
            self.settle(&job_id, Some(&step)).await
        })
    }
    fn run_offered<'a>(&'a self, job_id: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::repo::RepoJobState>> {
        Box::pin(async move { crate::commands::repo::run_offered_steps(self.app.clone(), job_id).await })
    }
    fn script<'a>(&'a self, site_id: String, kind: String, dir: String, script: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::repo::RepoJobState>> {
        Box::pin(async move {
            let snap = crate::commands::repo::repo_script_job(self.app.clone(), self.state()?, self.repo_jobs()?, site_id, kind, dir, script).await?;
            self.settle(&snap.id, None).await
        })
    }
    fn dist_archive<'a>(&'a self, site_id: String, kind: String, dir: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::repo::RepoJobState>> {
        Box::pin(async move {
            let snap = crate::commands::repo::repo_dist_archive(self.app.clone(), self.state()?, self.repo_jobs()?, site_id, kind, dir).await?;
            self.settle(&snap.id, None).await
        })
    }
    fn watch_start<'a>(&'a self, site_id: String, kind: String, dir: String, script: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::repo::WatchState>> {
        Box::pin(async move { crate::commands::repo::repo_watch_start(self.app.clone(), self.state()?, self.repo_watches()?, site_id, kind, dir, script).await })
    }
    fn watch_stop<'a>(&'a self, id: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::repo::repo_watch_stop(self.app.clone(), self.state()?, self.repo_watches()?, id).await })
    }
    fn cancel<'a>(&'a self, job_id: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::repo::repo_cancel(self.state()?, self.repo_jobs()?, job_id).await })
    }
    fn job_log<'a>(&'a self, log_key: String) -> user_sites::OpFuture<'a, Vec<String>> {
        Box::pin(async move {
            match self.state() {
                Ok(state) => crate::core::logs::tail(state.platform.as_ref(), &log_key, 200).unwrap_or_default(),
                Err(_) => Vec::new(),
            }
        })
    }
}

impl<Rt: tauri::Runtime> user_sites::ImportOps for AppSiteCreator<Rt> {
    fn valet_scan<'a>(&'a self) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::valet_import::ImportScan>> {
        Box::pin(async move { crate::commands::valet_import::scan_valet_import(self.state()?) })
    }
    fn valet_drift<'a>(&'a self) -> user_sites::OpFuture<'a, crate::error::Result<Vec<String>>> {
        Box::pin(async move { crate::commands::valet_import::resolver_drift(self.state()?) })
    }
    fn valet_run<'a>(&'a self, request: crate::commands::valet_import::ImportRequest) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::valet_import::ImportResult>> {
        Box::pin(async move {
            use tauri::Manager;
            let import_jobs = self.app.try_state::<crate::commands::valet_import::ImportJobs>().ok_or_else(|| crate::error::Error::Other("rexenv's import jobs are not ready".into()))?;
            let db_jobs = self.app.try_state::<crate::commands::db_import::DbImportJobs>().ok_or_else(|| crate::error::Error::Other("rexenv's import jobs are not ready".into()))?;
            crate::commands::valet_import::valet_import_run(self.app.clone(), self.state()?, import_jobs, self.jobs()?, db_jobs, request).await
        })
    }
    fn valet_cancel<'a>(&'a self) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move {
            use tauri::Manager;
            let import_jobs = self.app.try_state::<crate::commands::valet_import::ImportJobs>().ok_or_else(|| crate::error::Error::Other("rexenv's import jobs are not ready".into()))?;
            crate::commands::valet_import::valet_import_cancel(import_jobs);
            Ok(())
        })
    }
    // Root: the resolver file. `system` + the dialog, the stack's rule.
    fn resolver_take_over<'a>(&'a self, tld: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::valet_import::resolver_take_over(self.state()?, tld).await })
    }
    fn resolver_hand_back<'a>(&'a self, tld: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::core::dns::ResolverPlan>> {
        Box::pin(async move { crate::commands::valet_import::resolver_hand_back(self.state()?, tld).await })
    }
    fn rewrite_preview<'a>(&'a self, site_id: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::rewrite::RewritePreview>> {
        Box::pin(async move { crate::commands::rewrite::rewrite_preview(self.state()?, site_id).await })
    }
    fn rewrite_apply<'a>(&'a self, site_id: String, fingerprint: String) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::rewrite::RewriteApplied>> {
        Box::pin(async move { crate::commands::rewrite::rewrite_apply(self.state()?, self.jobs()?, self.tunnels()?, self.db_jobs()?, site_id, fingerprint).await })
    }
    fn rewrite_revert<'a>(&'a self, site_id: String, force: bool) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::rewrite::RevertOutcome>> {
        Box::pin(async move { crate::commands::rewrite::rewrite_revert(self.state()?, self.jobs()?, self.tunnels()?, self.db_jobs()?, site_id, force).await })
    }
    fn db_import_start<'a>(&'a self, site_id: String, confirm_overwrite: Option<String>) -> user_sites::OpFuture<'a, crate::error::Result<crate::commands::db_import::DbImportJobState>> {
        Box::pin(async move {
            use tauri::Manager;
            let db_jobs = self.app.try_state::<crate::commands::db_import::DbImportJobs>().ok_or_else(|| crate::error::Error::Other("rexenv's import jobs are not ready".into()))?;
            let mut st = crate::commands::db_import::db_import_start(self.app.clone(), self.state()?, db_jobs, self.jobs()?, self.tunnels()?, site_id.clone(), confirm_overwrite).await?;
            // Block until it settles — a tool reply is one message.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30 * 60);
            let mut last = (usize::MAX, u8::MAX);
            while st.status == "running" {
                if let Some(sink) = &self.progress {
                    if (st.phase_cursor, st.pct) != last {
                        last = (st.phase_cursor, st.pct);
                        let phase = st.phases.get(st.phase_cursor).map(|p| p.label.clone()).unwrap_or_default();
                        sink.report(f64::from(st.pct), Some(100.0), &phase);
                    }
                }
                if std::time::Instant::now() > deadline {
                    return Err(crate::error::Error::Other("the database import has not settled after 30 minutes — it keeps running in rexenv; read it later with db_import `status`.".into()));
                }
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                let db_jobs = self.app.try_state::<crate::commands::db_import::DbImportJobs>().ok_or_else(|| crate::error::Error::Other("rexenv's import jobs are not ready".into()))?;
                match crate::commands::db_import::db_import_state(db_jobs, site_id.clone())? {
                    Some(next) => st = next,
                    None => break,
                }
            }
            Ok(st)
        })
    }
    fn db_import_state<'a>(&'a self, site_id: String) -> user_sites::OpFuture<'a, crate::error::Result<Option<crate::commands::db_import::DbImportJobState>>> {
        Box::pin(async move {
            use tauri::Manager;
            let db_jobs = self.app.try_state::<crate::commands::db_import::DbImportJobs>().ok_or_else(|| crate::error::Error::Other("rexenv's import jobs are not ready".into()))?;
            crate::commands::db_import::db_import_state(db_jobs, site_id)
        })
    }
    fn db_import_cancel<'a>(&'a self, job_id: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move {
            use tauri::Manager;
            let db_jobs = self.app.try_state::<crate::commands::db_import::DbImportJobs>().ok_or_else(|| crate::error::Error::Other("rexenv's import jobs are not ready".into()))?;
            crate::commands::db_import::db_import_cancel(db_jobs, job_id)
        })
    }
    fn db_import_record<'a>(&'a self, site_id: String) -> user_sites::OpFuture<'a, crate::error::Result<Option<crate::state::store::DbImportRecord>>> {
        Box::pin(async move { crate::commands::db_import::db_import_record(self.state()?, site_id) })
    }
    fn db_import_records<'a>(&'a self) -> user_sites::OpFuture<'a, crate::error::Result<Vec<crate::state::store::DbImportRecord>>> {
        Box::pin(async move { crate::commands::db_import::db_import_records(self.state()?) })
    }
    fn db_import_leftovers<'a>(&'a self) -> user_sites::OpFuture<'a, crate::error::Result<Vec<crate::commands::db_import::LeftoverDump>>> {
        Box::pin(async move { crate::commands::db_import::db_import_leftovers(self.state()?) })
    }
    fn db_import_delete_leftover<'a>(&'a self, file: String) -> user_sites::OpFuture<'a, crate::error::Result<()>> {
        Box::pin(async move { crate::commands::db_import::db_import_delete_leftover(self.state()?, file) })
    }
}

/// The provision job's current phase as a progress message — the card's own
/// label, never a path (a phase label is rexenv's text).
fn provision_message(st: &crate::commands::site_provision::SiteProvisionState) -> String {
    st.phases.get(st.phase_cursor).map(|p| p.label.clone()).unwrap_or_else(|| st.status.clone())
}

/// May the agent's auto-stop timer end the site's LIVE share? Only when it is
/// the share the timer was started for — same public URL. A different URL is a
/// share somebody else started since (the user, from the Tunnels page), and
/// `None` is no share at all; both are left alone (#29, #513).
///
/// A function rather than an inline `==` so the rule has a name a test can
/// hold, and so the timer body cannot quietly go back to stopping by site.
pub(crate) fn auto_stop_applies(started_url: &str, live_url: Option<&str>) -> bool {
    live_url == Some(started_url)
}

/// Run EVERY registered tool against `app`'s state with the fixture site id, and
/// return each tool's serialised output (or its error text — errors can leak
/// too). For the secret-leak sweep (`examples/mcp_secret_sweep`): it plants
/// secrets in the state and asserts none appear in any output here. Walks
/// `every_tool` — the same enumeration dispatch uses — so a tool that can be
/// CALLED is swept by construction, whichever registry it lives in. (This used
/// to name "both" registries by hand; the third one is exactly the moment that
/// shape would have narrowed "every registered tool's output is swept" to
/// "every tool in the two someone remembered".)
pub async fn sweep_tool_outputs<Rt: tauri::Runtime>(
    app: &tauri::AppHandle<Rt>,
    fixture_site_id: &str,
) -> Vec<(&'static str, String)> {
    use tauri::Manager;
    let Some(state) = app.try_state::<AppState>() else {
        return Vec::new();
    };
    let ctx = ReadCtx::new(state.inner());
    let creator = AppSiteCreator { app: app.clone(), progress: None };
    let sctx = scratch::ScratchCtx::new(state.inner(), &creator, &creator, "secret-sweep");
    let uctx = user_sites::UserCtx::new(state.inner(), &creator, &creator, &creator, &creator, &creator, &creator, &creator, "secret-sweep");
    let mut outputs = Vec::new();
    // The sweep exercises handlers for their OUTPUT; a target they record is
    // irrelevant here, so each gets a throwaway recorder.
    let acted = feed::ActedTarget::default();
    for tool in every_tool() {
        let (name, result) = match tool {
            Tool::Read(t) => (t.name, (t.handler)(ctx, &(t.sweep_args)(fixture_site_id), &acted).await),
            Tool::Scratch(t) => (t.name, (t.handler)(sctx, &(t.sweep_args)(fixture_site_id), &acted).await),
            Tool::User(t) => (t.name, (t.handler)(uctx, &(t.sweep_args)(fixture_site_id), &acted).await),
        };
        // The SAME error scrub the live dispatch applies — the sweep judges
        // what an agent would see, and an agent sees `scrub_error`'s text.
        let text = match result {
            Ok(v) => serde_json::to_string(&v).unwrap_or_default(),
            Err(e) => scrub_error(state.inner(), &acted, &e.to_string()),
        };
        acted.take();
        outputs.push((name, text));
    }
    outputs
}

/// Record one agent action to the feed AND push the named scratch site's TTL
/// out — the session's own two writes for a call, best-effort under one lock.
///
/// Both are REXENV's writes on its own account, not the tool's: a handler
/// touches state only through `ReadCtx`, which has no mutator (the M1 boundary,
/// #199, unchanged). They live here rather than in a handler for exactly that
/// reason — a refresh in `tools.rs` would either break that boundary or force
/// the guard to be weakened to accommodate it.
///
/// A failure in either must never break the session: accountability matters, but
/// not at the cost of the connection.
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
    refresh_scratch_ttl(&conn, log.target_site.as_deref());
}

/// "The agent is still using this site" — push a SCRATCH site's expiry out
/// (PLAN §4.3: idle scratch dies, active scratch lives).
///
/// Read tools count: naming a site to diagnose it is using it, and M1's tools
/// get this for free precisely because the write lives out here in the session
/// rather than in a handler.
///
/// It is a **no-op unless the named site is a live scratch row**, which
/// `store::touch_site_expiry` enforces in its `WHERE` rather than here — a real
/// site the agent named (a `tail_log` on the user's own site) is never touched,
/// a KEPT site is never re-armed, a site that no longer exists matches nothing,
/// and no row is ever GIVEN an expiry it didn't have. Best-effort: a failure to
/// extend a TTL is not worth failing a call the user asked for.
fn refresh_scratch_ttl(conn: &rusqlite::Connection, target: Option<&str>) {
    let Some(id) = target else { return };
    if let Err(e) =
        crate::state::store::touch_site_expiry(conn, id, crate::core::scratch::scratch_ttl_hours(conn))
    {
        log::warn!("mcp: could not refresh the scratch TTL for {id}: {e}");
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

    /// Ledger #632 — **the Windows endpoint exists only while the toggle is on, and the toggle cannot read on
    /// without it**: `start` creates the pipe before it spawns anything (so a failure reaches `mcp_set_enabled`,
    /// #203), `serve_pipe` hands each connection to the one `session` and leaves its loop on the shutdown
    /// signal the sessions share, and no OS but "not unix and not Windows" gets the refusal.
    #[test]
    fn the_windows_endpoint_is_made_before_the_toggle_reads_on_and_ends_with_it() {
        let src = include_str!("mcp_server.rs");
        let body = |head: &str| -> &str {
            let start = src.find(head).unwrap_or_else(|| panic!("`{head}` is gone"));
            let rest = &src[start..];
            &rest[..rest.find("\n}\n").expect("the function's end")]
        };
        let start = body("#[cfg(windows)]\npub fn start<");
        let created = start.find("create_mcp_pipe(").expect("start no longer creates the pipe itself");
        let spawned = start.find("spawn(serve_pipe(").expect("start no longer serves the pipe");
        assert!(created < spawned, "the pipe must exist before anything is spawned — a failure has to reach the toggle:\n{start}");
        let serve = body("async fn serve_pipe<");
        assert!(serve.contains("shutdown.changed()") && serve.contains("break;"), "the accept loop no longer ends on the toggle:\n{serve}");
        assert_eq!(serve.matches("session(").count(), 2, "both of the loop's serving paths run the one session:\n{serve}");
        assert!(src.contains("#[cfg(not(any(unix, windows)))]\npub fn start<"), "the refusal must not cover Windows any more");
    }

    /// The violation message when a name appears in MORE THAN ONE registry, or
    /// `None` when they are all disjoint. Takes `(module, names)` per registry.
    ///
    /// Phrased for the person who trips it — who is, by definition, mid-way through
    /// adding a tool: it names the tool, states the rule, and says which side it
    /// belongs on. A set difference would tell them what happened and not what to
    /// do. (The import guard's lesson: a guard that fires without teaching gets
    /// worked around.)
    fn registry_conflict(registries: &[(&str, &[&str])]) -> Option<String> {
        let mut clash: Vec<String> = Vec::new();
        for (i, (module, names)) in registries.iter().enumerate() {
            for name in names.iter() {
                for (other, theirs) in registries.iter().skip(i + 1) {
                    if theirs.contains(name) {
                        clash.push(format!("`{name}` (in `{module}` and `{other}`)"));
                    }
                }
            }
        }
        if clash.is_empty() {
            return None;
        }
        Some(format!(
            "MCP tool name(s) registered in more than one registry: {}.\n\
             One name, one capability — a tool belongs to exactly one side:\n\
             - `mcp_server/tools.rs` (M1) if it only READS: its handler gets a ReadCtx, which has no \
             mutating method, and the read-only guard scans that module.\n\
             - `mcp_server/scratch.rs` (M2) if it changes or runs anything in a site the AGENT made: \
             its handler gets a ScratchCtx, whose only door to a site is the origin-checked witness.\n\
             - `mcp_server/user_sites.rs` (parity) if it acts on the USER's own site or the stack: its \
             handler gets a UserCtx, whose only door is the scope witness minted from the user's grant.\n\
             Duplicating a name would let one side shadow the other (whichever `every_tool` chains \
             first), so the tool an agent called would not be the tool that ran. Delete the copy from \
             the side it does not belong on.",
            clash.join(", ")
        ))
    }


    /// The packaged enable-crash guard: `mcp_set_enabled` is a SYNC command, so it
    /// binds OFF the tokio runtime, where `tokio::net::UnixListener::bind` aborts on
    /// `Handle::current` ("there is no reactor running"). `bind_socket` must need no
    /// ambient runtime — proven by binding on a plain `std::thread` with NONE and
    /// requiring success + 0600. This FAILS on the pre-fix path (the throwaway proof
    /// against `cli_server::bind` panicked on this exact thread); it passes on the
    /// std bind. A harness (L2) can never catch this — it mocks the IPC command.
    #[test]
    #[cfg(unix)] // exercises `bind_socket`, which is the unix transport itself
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
        //
        // BOTH registries, deliberately: checking only M1's would narrow "every
        // registered tool's output is swept" to "every tool except the widest
        // ones" — the executing side is where a docroot actually re-enters the
        // output (`wp_run`), so it is the half that most needs to be in the
        // sweep (#209).
        let named: Vec<(&str, Value)> = every_tool()
            .map(|t| match t {
                Tool::Read(t) => (t.name, (t.sweep_args)("fixture-site-id")),
                Tool::Scratch(t) => (t.name, (t.sweep_args)("fixture-site-id")),
                Tool::User(t) => (t.name, (t.sweep_args)("fixture-site-id")),
            })
            .collect();
        assert_eq!(
            named.len(),
            tools::registry().len() + scratch::registry().len() + user_sites::registry().len(),
            "every tool in EVERY registry declares sweep_args"
        );
        for (name, args) in named {
            assert!(args.is_object(), "{name}: sweep_args must be a JSON object");
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

    #[test]
    fn what_rexenv_did_beats_what_the_agent_asked_for() {
        // A create has no `site_id` to name, so the recorded target can only come
        // from the row rexenv made; a read tool names what the agent asked for,
        // INCLUDING when it fails — "the agent asked about a site that isn't
        // there" is the diagnostic worth keeping.
        let acted = || Some("7f3a1c02-9d51-4d2e-8b77-2c9a4e6f1b30".to_string());
        let named = || Some("2b91d0f4-1a33-4e77-9a0c-8d2e4f5a6b1c".to_string());
        assert_eq!(target_for_record(acted(), None), acted(), "a create names what it made");
        assert_eq!(target_for_record(None, named()), named(), "a read names the ask");
        assert_eq!(target_for_record(acted(), named()), acted(), "what happened wins");
        assert_eq!(target_for_record(None, None), None, "list_sites names nothing");
    }

    #[tokio::test]
    async fn a_create_that_fails_after_the_row_exists_still_names_the_site() {
        // The case this mechanism exists for, and the one a return value would
        // get wrong. `ActedTarget` is an out-parameter, so a handler records the
        // site the instant the row exists and a LATER `?` cannot discard it —
        // which is exactly what a provisioning failure after a successful insert
        // looks like. The user can see, retry or delete that half-built site, so
        // the feed must name it.
        use crate::state::models::{test_site, SiteOrigin};

        // Shaped like the M2 create handler: insert, record, then fail.
        async fn create_then_fail(acted: &feed::ActedTarget) -> crate::error::Result<Value> {
            let row = test_site("c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24", "probe.scratch.rex", SiteOrigin::Agent);
            acted.set(&row); // the row exists from here on
            Err(crate::error::Error::Other("wp core download failed".into()))?;
            unreachable!()
        }

        let acted = feed::ActedTarget::default();
        assert!(create_then_fail(&acted).await.is_err());
        // No `site_id` argument existed — without the out-parameter this row
        // would name nothing at all.
        assert_eq!(
            target_for_record(acted.take(), None).as_deref(),
            Some("c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24"),
            "a site that EXISTS must be named even though the call failed"
        );
    }

    #[tokio::test]
    async fn a_create_that_fails_before_any_row_names_nothing_rather_than_guessing() {
        // The other half of the same rule, and neither half is a guess: refused
        // at the cap, or a bad name, or the resolver missing — nothing was
        // created, so nothing is named. A row naming a site that does not exist
        // would send the user looking for it.
        async fn refuse_early(_acted: &feed::ActedTarget) -> crate::error::Result<Value> {
            Err(crate::error::Error::Other("at the scratch-site cap (5)".into()))
        }
        let acted = feed::ActedTarget::default();
        assert!(refuse_early(&acted).await.is_err());
        assert_eq!(target_for_record(acted.take(), None), None);
    }

    #[test]
    fn a_recorded_target_can_only_be_a_site_row_never_agent_content() {
        // `ActedTarget::set` takes a `&Site` — a row from rexenv's OWN sites
        // table — and reads its id. There is deliberately no constructor from a
        // string, so an argument, a tool result, or anything else off the wire
        // cannot reach the feed through this channel (the typed-shape discipline
        // the feed exists to keep). This test pins the VALUE half of that: what
        // lands is the row's id, not anything the caller chose.
        use crate::state::models::{test_site, SiteOrigin};
        let row = test_site("b41d7c58-2e0a-49f6-9a13-7d5c8e2f4011", "shop.rex", SiteOrigin::User);
        let acted = feed::ActedTarget::default();
        acted.set(&row);
        assert_eq!(acted.take().as_deref(), Some(row.id.as_str()));
        // And it is taken ONCE — a second read cannot re-attribute a stale
        // target to the next call on the same session.
        assert_eq!(acted.take(), None);
    }


    /// An in-memory app db with one scratch row and one of the user's own, both
    /// in production shape. Returns the connection.
    #[cfg(test)]
    fn db_with_a_scratch_and_a_real_site() -> rusqlite::Connection {
        use crate::state::models::{test_site, SiteOrigin};
        use crate::state::store;
        let conn = crate::state::db::open_in_memory().unwrap();
        let mut scratch =
            test_site("c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24", "probe.scratch.rex", SiteOrigin::Agent);
        scratch.expires_at = Some("2026-08-02 09:00:00".into());
        scratch.agent_client = Some("Claude Code".into());
        store::insert_site(&conn, &scratch).unwrap();
        // The user's own site — and deliberately given a stale expiry, so the
        // refusal below rests on `origin`, not on the expiry happening to be NULL.
        let mut real = test_site("7f3a1c02-9d51-4d2e-8b77-2c9a4e6f1b30", "myblog.rex", SiteOrigin::User);
        real.expires_at = Some("2020-01-01 00:00:00".into());
        store::insert_site(&conn, &real).unwrap();
        conn
    }

    #[test]
    fn using_a_scratch_site_pushes_its_expiry_out() {
        use crate::state::store;
        let conn = db_with_a_scratch_and_a_real_site();
        let id = "c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24";
        refresh_scratch_ttl(&conn, Some(id));
        let after = store::get_site(&conn, id).unwrap().unwrap().expires_at.unwrap();
        assert!(after.as_str() > "2026-08-02 09:00:00", "the deadline moved out: {after}");
        // ...and it moved to roughly the TTL from now, not to some other clock.
        let expected: String = conn
            .query_row("SELECT datetime('now', '+24 hours')", [], |r| r.get(0))
            .unwrap();
        assert_eq!(after[..16], expected[..16], "expiry is now + SCRATCH_TTL_HOURS");
    }

    #[test]
    fn a_read_tool_naming_the_users_own_site_touches_nothing() {
        // `tail_log` against a real site is a perfectly ordinary call. It must
        // not write lifecycle state onto a site the agent does not own — and the
        // refusal rests on `origin`, which is why this fixture's real site
        // carries a stale expiry rather than a NULL one.
        use crate::state::store;
        let conn = db_with_a_scratch_and_a_real_site();
        let id = "7f3a1c02-9d51-4d2e-8b77-2c9a4e6f1b30";
        refresh_scratch_ttl(&conn, Some(id));
        assert_eq!(
            store::get_site(&conn, id).unwrap().unwrap().expires_at.as_deref(),
            Some("2020-01-01 00:00:00"),
            "a user's site is untouched, expiry and all"
        );
    }

    #[test]
    fn naming_a_site_that_no_longer_exists_creates_nothing() {
        // A feed row can name a site that has since been deleted (a reap, or a
        // stale id an agent kept). The touch must not resurrect it, and must not
        // invent a row.
        use crate::state::store;
        let conn = db_with_a_scratch_and_a_real_site();
        let before = store::list_sites(&conn).unwrap().len();
        refresh_scratch_ttl(&conn, Some("00000000-0000-4000-8000-000000000000"));
        refresh_scratch_ttl(&conn, None); // a call that named no site at all
        assert_eq!(store::list_sites(&conn).unwrap().len(), before, "no row appeared");
        assert!(store::get_site(&conn, "00000000-0000-4000-8000-000000000000").unwrap().is_none());
    }

    #[test]
    fn a_touch_extends_a_deadline_and_never_starts_a_clock() {
        // The direction that matters: an agent row with NO expiry (what a KEPT
        // site would look like if `origin` had not also been flipped) must not
        // be GIVEN one. Writing an expiry here would create deletion state that
        // did not exist — the one move this must never make.
        use crate::state::models::{test_site, SiteOrigin};
        use crate::state::store;
        let conn = crate::state::db::open_in_memory().unwrap();
        let kept = test_site("b41d7c58-2e0a-49f6-9a13-7d5c8e2f4011", "kept.scratch.rex", SiteOrigin::Agent);
        assert_eq!(kept.expires_at, None);
        store::insert_site(&conn, &kept).unwrap();
        refresh_scratch_ttl(&conn, Some(&kept.id));
        let after = store::get_site(&conn, &kept.id).unwrap().unwrap();
        assert_eq!(after.expires_at, None, "a touch must not start a clock");
        assert!(!after.reap_due("2099-01-01 00:00:00"), "and so it stays unreapable");
    }

    #[test]
    fn an_expired_but_uncollected_scratch_site_is_revived_by_use() {
        // The reaper hasn't got to it and the agent is demonstrably still using
        // it — which is the question the TTL asks. Refreshing is the answer;
        // letting it die under an active session would be the surprise.
        use crate::state::models::{test_site, SiteOrigin};
        use crate::state::store;
        let conn = crate::state::db::open_in_memory().unwrap();
        let mut stale = test_site("d17c9b30-5f2e-4a68-b1d4-9c3e7a2f5011", "old.scratch.rex", SiteOrigin::Agent);
        stale.expires_at = Some("2020-01-01 00:00:00".into());
        store::insert_site(&conn, &stale).unwrap();
        assert!(stale.reap_due("2026-08-01 12:00:00"), "it was due");
        refresh_scratch_ttl(&conn, Some(&stale.id));
        let after = store::get_site(&conn, &stale.id).unwrap().unwrap();
        assert!(!after.reap_due("2026-08-01 12:00:00"), "using it bought it another TTL");
    }


    #[test]
    fn every_registry_is_disjoint_and_the_guard_says_which_side_a_tool_belongs_on() {
        // The load-bearing half of the module split. One name, one capability:
        // a duplicate would mean the tool an agent CALLED is not the tool that
        // RAN (whichever registry dispatch consults first), which is a capability
        // decided by lookup order instead of by where the tool lives.
        //
        // Grouped from `every_tool` — the same enumeration dispatch uses — so
        // the registries this guard sees are the registries a call can reach.
        let mut groups: Vec<(&str, Vec<&str>)> = Vec::new();
        for t in every_tool() {
            match groups.iter_mut().find(|(m, _)| *m == t.module()) {
                Some((_, names)) => names.push(t.name()),
                None => groups.push((t.module(), vec![t.name()])),
            }
        }
        let regs: Vec<(&str, &[&str])> = groups.iter().map(|(m, n)| (*m, n.as_slice())).collect();
        // `panic!` with the message itself, NOT `assert_eq!(.., None)`: the
        // latter prints it Debug-escaped as one long line with literal \n, which
        // is the guidance made unreadable at the exact moment someone needs it.
        if let Some(msg) = registry_conflict(&regs) {
            panic!("{msg}");
        }

        // ...and the message a person actually gets. Whoever trips this is
        // mid-way through adding a tool, so it has to teach the rule, not report
        // a set difference. Three-way, because a clash can now be with either
        // other side.
        let msg = registry_conflict(&[
            ("mcp_server/tools.rs", &["list_sites", "tail_log"]),
            ("mcp_server/scratch.rs", &["scratch_create_site", "tail_log"]),
            ("mcp_server/user_sites.rs", &["site_create", "scratch_create_site"]),
        ])
        .expect("a shared name must be caught");
        assert!(msg.contains("`tail_log`") && msg.contains("`scratch_create_site`"), "names each offender: {msg}");
        assert!(!msg.contains("`list_sites`") && !msg.contains("`site_create`"), "names ONLY the offenders: {msg}");
        assert!(
            msg.contains("mcp_server/tools.rs") && msg.contains("mcp_server/scratch.rs") && msg.contains("mcp_server/user_sites.rs"),
            "says which sides exist: {msg}"
        );
        assert!(msg.contains("only READS") && msg.contains("AGENT made") && msg.contains("USER's own"),
                "states the rule that decides the side: {msg}");
        assert!(msg.contains("shadow"), "says what goes wrong, not just that it did: {msg}");
    }

    /// **A registry that exists but is not in `every_tool` is a build failure.**
    /// The chain is the ONE place a registry becomes reachable, listed and
    /// swept; this scans the module list for any registry module missing from it.
    #[test]
    fn every_registry_module_is_chained_into_the_one_enumeration() {
        let me = include_str!("mcp_server.rs");
        // The TEST MODULE's marker, not the first `#[cfg(test)]` — `Tool::module`
        // is test-only and sits above the chain.
        let prod = &me[..me.find("#[cfg(test)]\nmod tests").unwrap()];
        let chain_start = prod.find("fn every_tool()").expect("the enumeration");
        let chain = &prod[chain_start..chain_start + prod[chain_start..].find("\n}\n").unwrap()];
        let sources: &[(&str, &str)] = &[
            ("tools", include_str!("mcp_server/tools.rs")),
            ("scratch", include_str!("mcp_server/scratch.rs")),
            ("user_sites", include_str!("mcp_server/user_sites.rs")),
            ("readctx", include_str!("mcp_server/readctx.rs")),
            ("view", include_str!("mcp_server/view.rs")),
            ("feed", include_str!("mcp_server/feed.rs")),
            // Test-only: the command-by-command parity guard, not a registry.
            ("parity", include_str!("mcp_server/parity.rs")),
        ];
        let mut registries = 0;
        for (module, src) in sources {
            assert!(prod.contains(&format!("mod {module};")), "the source list here must name every mcp_server module — `{module}` is missing");
            if src.contains("pub fn registry()") {
                registries += 1;
                assert!(
                    chain.contains(&format!("{module}::registry()")),
                    "`mcp_server/{module}.rs` defines a registry that `every_tool` does not chain — its tools \
                     would be unreachable, unlisted AND unswept. Add it to the chain (and a `Tool` variant)."
                );
            }
        }
        assert_eq!(registries, 3, "three registries today — update this if a fourth is a real decision");
        // …and every module file is in the list above (a new `mod x;` must be
        // classified as registry-or-not here, by name).
        let declared = prod.matches("\nmod ").count() + prod.matches("\npub mod ").count();
        assert_eq!(declared, sources.len(), "a module was added to mcp_server.rs without being classified here");
    }

    /// **Every advertised tool carries `readOnlyHint`/`destructiveHint`, decided
    /// from its registry** — a read tool is read-only, `scratch_delete_site`
    /// and every `destroy`-scoped parity tool are destructive, nothing else is.
    #[test]
    fn every_advertised_tool_carries_honest_annotations() {
        let v = reply(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
        let listed = v["result"]["tools"].as_array().expect("tools array");
        assert!(!listed.is_empty());
        for d in listed {
            let name = d["name"].as_str().unwrap();
            let a = &d["annotations"];
            assert!(a["readOnlyHint"].is_boolean() && a["destructiveHint"].is_boolean(), "{name} lacks annotations: {d}");
            let tool = find_tool(name).expect("advertised tools are registered");
            let (ro, de) = tool.annotations();
            assert_eq!((a["readOnlyHint"].as_bool(), a["destructiveHint"].as_bool()), (Some(ro), Some(de)), "{name}");
            assert!(!(ro && de), "{name}: read-only and destructive at once");
            match tool {
                Tool::Read(_) => assert!(ro && !de, "{name}"),
                Tool::Scratch(_) => assert!(!ro && (de == (name == "scratch_delete_site")), "{name}"),
                Tool::User(t) => assert_eq!((ro, de), (t.scope == crate::core::agent_grants::Scope::Read, t.scope == crate::core::agent_grants::Scope::Destroy), "{name}"),
            }
        }
    }

    /// **Progress is opt-in by the client's token, its notifications are the
    /// spec's shape, and every one of them lands BEFORE the call's reply.**
    #[tokio::test]
    async fn progress_is_opt_in_and_arrives_before_the_reply() {
        // The token is read from `_meta`, and only from there.
        match dispatch(r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"list_sites","arguments":{},"_meta":{"progressToken":"p1"}}}"#) {
            Dispatch::ToolCall { progress_token, .. } => assert_eq!(progress_token, Some(json!("p1"))),
            other => panic!("expected a ToolCall, got {other:?}"),
        }
        match dispatch(r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"list_sites","arguments":{"progressToken":"nope"}}}"#) {
            Dispatch::ToolCall { progress_token, .. } => assert_eq!(progress_token, None, "an ARGUMENT is not the spec's opt-in"),
            other => panic!("expected a ToolCall, got {other:?}"),
        }
        // The notification's shape.
        let line = progress_notification(&json!("p1"), 40.0, Some(100.0), "downloading WordPress");
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["method"], "notifications/progress");
        assert_eq!(v["params"]["progressToken"], "p1");
        assert_eq!(v["params"]["progress"], 40.0);
        assert_eq!(v["params"]["total"], 100.0);
        assert_eq!(v["params"]["message"], "downloading WordPress");
        assert!(v.get("id").is_none(), "a notification has no id");
        // Ordering: two reports during the call, then the call's value — all
        // notifications on the writer before the caller writes the reply.
        let (tx, rx) = mpsc::unbounded_channel::<String>();
        let sink = ProgressSink { token: json!("p1"), tx };
        let mut out: Vec<u8> = Vec::new();
        let value = run_with_progress(&mut out, Some(rx), async move {
            sink.report(1.0, Some(3.0), "one");
            tokio::task::yield_now().await;
            sink.report(2.0, Some(3.0), "two");
            "reply"
        })
        .await;
        assert_eq!(value, "reply");
        let written = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = written.lines().collect();
        assert_eq!(lines.len(), 2, "both notifications, nothing else: {written:?}");
        assert!(lines[0].contains("\"one\"") && lines[1].contains("\"two\""), "{written}");
        // No token: no channel, no lines, the same value.
        let mut out2: Vec<u8> = Vec::new();
        let v2 = run_with_progress(&mut out2, None, async { 7 }).await;
        assert_eq!((v2, out2.len()), (7, 0));
    }

    #[test]
    fn tools_list_offers_the_union_of_every_registry() {
        // What the SOCKET advertises is every registry, flat — the tier is a
        // fact about what rexenv will let a tool do, never something the agent
        // selects. Pins the count relationship rather than a literal.
        let v = reply(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
        let listed = v["result"]["tools"].as_array().expect("tools array").len();
        assert_eq!(
            listed,
            tools::registry().len() + scratch::registry().len() + user_sites::registry().len(),
            "tools/list must advertise EVERY registry — a tool that exists but isn't listed is \
             a tool an agent will never call, and one listed twice is a name collision"
        );
        // …and each descriptor is the registry's own (name, description, schema).
        for d in v["result"]["tools"].as_array().unwrap() {
            assert!(d["description"].is_string() && d["inputSchema"].is_object(), "{d}");
        }
    }

    #[test]
    fn a_scratch_handler_can_only_reach_a_site_through_the_origin_checked_witness() {
        // The capability difference, stated as a property of the CONTEXT rather
        // than of the tools (there are none yet): `ScratchCtx` has no
        // `site_by_id`/`sites` — `claim` is the only door, and it refuses the
        // user's own sites. This is what makes the second registry a different
        // capability rather than the same one with a different name.
        use crate::state::models::{test_site, SiteOrigin};
        use crate::state::store;
        let conn = crate::state::db::open_in_memory().unwrap();
        let mine = test_site("7f3a1c02-9d51-4d2e-8b77-2c9a4e6f1b30", "myblog.rex", SiteOrigin::User);
        store::insert_site(&conn, &mine).unwrap();
        let theirs =
            test_site("c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24", "probe.scratch.rex", SiteOrigin::Agent);
        store::insert_site(&conn, &theirs).unwrap();
        // `ScratchCtx::claim` delegates to the same core door, so assert on it
        // directly (building an AppState here would need a mock app).
        assert!(crate::core::scratch::claim(&conn, &theirs.id).is_ok());
        assert!(crate::core::scratch::claim(&conn, &mine.id).is_err());
    }


    #[test]
    fn the_enable_moment_copy_says_what_an_agent_can_actually_do() {
        // WHAT THIS CATCHES NOW — because its first job is spent, and a guard
        // that can never fail again is the shape this project removes.
        //
        // It was written to stop task 8 landing with "Today those are read-only"
        // above the toggle once an executing tool existed. It did exactly that
        // (it failed the build, the copy was rewritten). That specific trip
        // CANNOT recur: the phrase is gone and the registry never empties again.
        //
        // What remains is drift on the paragraph, in two directions, and both
        // are live:
        //   - REGRESSION (the ban list): a revert, a paste from git history, or
        //     a "simplify this" edit that reaches for the old wording and puts a
        //     false sentence back where the user decides.
        //   - EROSION (the must-say list): the likelier one. The paragraph is
        //     long, it sits above a toggle, and the obvious edit is to trim it —
        //     dropping the residual ("as you", #197) or what an agent still
        //     cannot touch. Deleting the false sentence and saying nothing would
        //     pass a ban-only guard while leaving the user LESS informed, which
        //     is why both halves exist.
        // The registry-empty early return below is now unreachable in practice;
        // it is kept only so the guard is readable as a rule rather than as a
        // list of today's strings.
        const CARD: &str = include_str!("../../src/components/mcp/AgentsMcpCard.tsx");
        // Phrases that are only true while NOTHING an agent calls can execute.
        const ONLY_TRUE_WHEN_READ_ONLY: &[&str] =
            &["Today those are read-only", "not change or run anything"];
        // ...and what the copy must say once something can. Checked as well as
        // the ban, because deleting the false sentence and saying nothing would
        // pass a ban-only guard while leaving the user less informed, not more.
        const MUST_SAY_WHEN_EXECUTING: &[(&str, &str)] = &[
            ("create", "that an agent can create sites of its own"),
            ("run it", "that code in them RUNS"),
            ("as you", "that it runs with the user's own power — the residual, #197"),
            ("cannot", "what it still cannot touch: the user's own sites"),
            // Added 3 Aug 2026, from the §6.0 re-read. The plan's guarantee had
            // drifted into "your sites … are not reachable from any tool", which
            // is true of MUTATION and false of the read tools — and the reading
            // it invites ("an agent sees only what it creates") was never true of
            // any shipped version. This card never said that, because the copy
            // guard forced it to be rewritten honestly; the unguarded draft
            // rotted. So the visibility claim joins the must-say list: a trim
            // that removes it would leave the user believing the narrower thing,
            // which is the more damaging half — the guarantee is read by people
            // auditing, this paragraph is read by everyone who turns it on.
            ("look at your sites", "that an agent can SEE every site, not only its own (§3.1c)"),
            // D16 (4 Sep 2026): the mail sub-toggle and the database prompt are
            // gone — both are READ, and Read is on whenever the endpoint is. The
            // sentences the old toggle and the old prompt carried were the
            // deliberately unflattering ones (#226, #402); they move HERE, to the
            // one paragraph a user reads before turning the endpoint on, because
            // it is now the only moment they consent to either.
            ("every site's mail", "that the WHOLE inbox is readable — not only the agent's own sites' mail"),
            ("password-reset links", "the concrete thing in that inbox a trim drops first"),
            ("read-only", "the bound that makes a database read a read"),
            ("password hashes", "WHAT is actually in reach in a database, in the words that make it concrete"),
            ("API keys", "the second concrete thing — a trim usually keeps one and drops this"),
        ];
        if scratch::registry().is_empty() {
            return; // unreachable in practice — see the note above
        }
        for phrase in ONLY_TRUE_WHEN_READ_ONLY {
            assert!(
                !CARD.contains(phrase),
                "the executing registry has {} tool(s), but the enable-moment copy still says \
                 \"{phrase}\" — a sentence that is now FALSE, sitting where the user decides. \
                 Rewrite it to say what an agent can actually do.",
                scratch::registry().len()
            );
        }
        // The card heading the dial's refusal sends people to (#404's lesson,
        // kept after D16 retired the database section): COMMENTS STRIPPED, so a
        // doc comment cannot satisfy it, and a landmark so an empty scan cannot.
        let card = crate::core::copy_scan::strip_ts_comments(include_str!(
            "../../src/components/mcp/AgentsMcpCard.tsx"
        ));
        assert!(card.contains("StartStopToggle"), "the card scan came back empty");
        assert!(card.contains("AI agents (MCP)"), "the card heading the refusal sends people to was renamed");
        assert!(include_str!("core/agent_access.rs").contains("AI agents (MCP)"), "the refusal stopped naming the card");
        // Whitespace-insensitive (a JSX re-wrap is not a copy change) over the
        // COMMENT-STRIPPED source, so a doc comment cannot satisfy a must-say.
        let card_squashed = crate::core::copy_scan::strip_ts_comments(CARD).split_whitespace().collect::<Vec<_>>().join(" ");
        for (phrase, why) in MUST_SAY_WHEN_EXECUTING {
            assert!(
                card_squashed.contains(phrase),
                "the enable-moment copy no longer tells the user {why} (looked for \"{phrase}\"). \
                 The paragraph is what a security-minded user reads AT the moment of enabling; it \
                 has to describe the capability honestly, including that this is a paved road and \
                 not a sandbox."
            );
        }
    }

    /// **An agent's auto-stop timer ends the share it STARTED, and no other**
    /// (#513, the `#29` family).
    ///
    /// The timer fires up to an hour after `share {action: "start"}`, and the
    /// ordinary sequence inside that hour is: the agent's share ends (stopped,
    /// or cloudflared died), and the PERSON shares the same site again from the
    /// Tunnels page. The first version stopped by SITE at the deadline, which
    /// would have ended the user's share — rexenv stopping a share on the
    /// user's behalf, the thing #29 forbids. The public URL is unique per
    /// tunnel, so the predicate compares the URL the call handed out with the
    /// one the site's live share carries at the deadline, and a source guard
    /// holds the timer body to consulting the registry BEFORE it stops.
    #[test]
    fn the_auto_stop_timer_stops_only_the_share_it_started() {
        assert!(auto_stop_applies("https://abc.trycloudflare.com", Some("https://abc.trycloudflare.com")));
        assert!(
            !auto_stop_applies("https://abc.trycloudflare.com", Some("https://xyz.trycloudflare.com")),
            "a different URL is somebody else's share — the user's, started after the agent's ended"
        );
        assert!(!auto_stop_applies("https://abc.trycloudflare.com", None), "no share: nothing to stop");

        let src = crate::core::copy_scan::production_source(include_str!("mcp_server.rs"));
        let body = src
            .split("fn share_start<'a>(")
            .nth(1)
            .and_then(|b| b.split("\n    fn share_stop").next())
            .expect("share_start");
        let read = body.find("tunnels_status(").expect(
            "the timer no longer re-reads the registry at its deadline — it would stop whatever \
             share the site has, the user's included",
        );
        let gate = body.find("auto_stop_applies(").expect("the timer no longer asks the predicate");
        let stop = body.find("stop_tunnel(").expect("the timer no longer stops anything");
        assert!(read < gate && gate < stop, "the registry read and the URL check must come BEFORE the stop");
    }

    /// **An error reply is scrubbed by the one scrubber: the acted site's
    /// docroot, rexenv's own paths and home become labels, and a call that
    /// reached no site still loses rexenv's paths.** The leak `mcp_secret_sweep`
    /// found on the day reads became free (D15).
    #[test]
    fn a_handlers_error_is_scrubbed_before_an_agent_or_the_feed_sees_it() {
        let state = crate::mcp_server::user_sites::tests::app_state_for_scrub();
        let site = crate::state::models::test_site("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", "shop.rex", crate::state::models::SiteOrigin::User);
        {
            let conn = state.db.lock().unwrap();
            crate::state::store::insert_site(&conn, &site).unwrap();
        }
        let acted = feed::ActedTarget::default();
        let raw = format!("wp failed: The used path is: {}/wp-content\nsee {}/logs/x.log", site.path, state.platform.paths().app_data_dir().unwrap().display());
        let before = scrub_error(&state, &acted, &raw);
        assert!(!before.contains(&state.platform.paths().app_data_dir().unwrap().display().to_string()), "app-data path with no site: {before}");
        assert!(before.contains(&site.path), "with no acted site the docroot is not known yet: {before}");
        acted.set(&site);
        let after = scrub_error(&state, &acted, &raw);
        assert!(!after.contains(&site.path) && after.contains("<docroot>/wp-content"), "{after}");
        assert!(after.contains("<rexenv-data>") || after.contains("<rexenv-logs>"), "{after}");
        assert!(acted.peek().is_some(), "the scrub must not consume the feed's target");
    }

    /// **The Agent access copy says what each level hands over, that publishing
    /// is Full, that the password dialog still asks, and the residual; the
    /// refusal, the dial and the section share their names.** D15's consent
    /// surface — the ONE dial that replaced per-site prompts, so its sentences
    /// are the whole of what a user reads before turning it up.
    #[test]
    /// **The MCP retry op answers with the SETTLED job.** Pinned in source
    /// because the op needs a running app to call: `site_provision_retry` alone
    /// returns the first snapshot, and that is what the tool replied on
    /// 11 Sep 2026 — `running`, every phase pending — while its description
    /// promised it blocks until the job settles.
    fn the_retry_op_answers_with_the_settled_job_not_the_first_snapshot() {
        let src = include_str!("mcp_server.rs");
        let body = src
            .split("fn retry<'a>(&'a self, site_id: String)")
            .nth(1)
            .expect("the retry op")
            .split("fn multisite_convert")
            .next()
            .unwrap();
        assert!(body.contains("site_provision_retry("), "the op starts the app's own retry");
        assert!(
            body.contains("site_provision::settle("),
            "the op must wait for the job to settle before answering"
        );
    }

    #[test]
    fn the_agent_access_copy_says_what_a_level_hands_over() {
        const CARD: &str = include_str!("../../src/components/mcp/AgentsMcpCard.tsx");
        const DIAL: &str = include_str!("../../src/components/mcp/AgentAccessDial.tsx");
        const REFUSALS: &str = include_str!("core/agent_access.rs");

        // The enable-moment paragraph: the refusal is conditional on the dial.
        const CARD_MUST_SAY: &[(&str, &str)] = &[
            ("unless you turn Agent access up below", "that the refusal is conditional on the dial the user turns"),
            ("for this session, 7 days or always", "the three durations, so a user knows what they are choosing"),
            ("once you allow changes", "that the residual (#197) reaches the user's own sites once the dial is up"),
            ("at Full an agent can also publish a site to the internet", "that publishing is IN Full, not a separate ask (D17)"),
            ("stops any share it starts within the hour", "that a share ends on its own — the bound that makes publishing grantable"),
            ("never\n        asks for your administrator password", "that no level replaces the administrator-password dialog"),
        ];
        // The dial itself.
        const DIAL_MUST_SAY: &[(&str, &str)] = &[
            ("Read is on whenever the endpoint is", "that Read is not a choice — it is what MCP on means"),
            ("publish a site to the internet", "that Full includes publishing (D17)"),
            ("stops the share within the hour", "that a share ends on its own"),
            ("still asks you", "that the administrator-password dialog is untouched"),
            ("runs as\n        you", "the residual, on the dial itself"),
            ("switches itself off when you quit rexenv", "what 'this session' means"),
            ("expires on its own after 7 days", "what '7 days' means"),
            ("An agent can {l.allows}", "that each level's sentence is rendered from Rust, not retyped"),
            ("agents are back at Read", "what an expired 7-day setting means"),
        ];
        // Whitespace-insensitive (a JSX re-wrap is not a copy change) over the
        // COMMENT-STRIPPED source, so a doc comment cannot satisfy a must-say.
        let squash = |s: &str| crate::core::copy_scan::strip_ts_comments(s).split_whitespace().collect::<Vec<_>>().join(" ");
        let (card_s, dial_s) = (squash(CARD), squash(DIAL));
        for (phrase, why) in CARD_MUST_SAY {
            assert!(card_s.contains(&squash(phrase)), "the card no longer tells the user {why} (looked for \"{phrase}\")");
        }
        for (phrase, why) in DIAL_MUST_SAY {
            assert!(dial_s.contains(&squash(phrase)), "the dial no longer tells the user {why} (looked for \"{phrase}\")");
        }
        // No auto-allow switch survived D15 — absent, not hidden.
        let dial_ui = crate::core::copy_scan::strip_ts_comments(DIAL);

        assert!(!dial_ui.contains("without asking"), "an auto-allow switch is back");

        // The refusal an AGENT reads names the card, the dial and the section
        // by the strings the UI renders — #404's lesson, on comment-stripped
        // source so a doc comment cannot satisfy it.
        let card = crate::core::copy_scan::strip_ts_comments(CARD);
        assert!(dial_ui.contains("role=\"radiogroup\""), "a scan came back empty");
        assert!(REFUSALS.contains("AI agents (MCP)") && card.contains("AI agents (MCP)"), "the card heading");
        assert!(REFUSALS.contains("{DIAL_LABEL}") && REFUSALS.contains("pub const DIAL_LABEL: &str = \"Agent access\""), "the refusal names the dial by its constant");
        assert!(dial_ui.contains("a?.label") && dial_ui.contains("{label}"), "the dial renders its heading from the status, not a literal");
        // D17: there is no Site access section — publishing is the dial's Full.
        assert!(!card_s.contains("Site access") && !dial_s.contains("Site access"), "a retired consent section is back");
        // The level sentences reach the card from Rust (`what_it_allows`) —
        // so there is no TS copy to drift.
        assert!(!dial_ui.contains("delete or reset a site"), "a TS copy of a level sentence appeared");
    }
}

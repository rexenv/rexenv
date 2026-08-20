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
//! pinned by the read-only import guard in `tests`. Executing tools live in a
//! *different* module with its own capability (`scratch`); this boundary does
//! not erode.
//!
//! **What the SOCKET guarantees, as opposed to what M1's tools guarantee.**
//! These are two different statements and only one of them is #199. Dispatch
//! routes BOTH registries, so once the executing registry is non-empty the
//! honest description of the endpoint is:
//!
//! - a call reaches exactly one registry, and its capability is decided by which
//!   one it came from (`find_tool` → `Tool::Read` gets a `ReadCtx`,
//!   `Tool::Scratch` a `ScratchCtx`) — never by the tool's own say-so, never by
//!   its arguments, and never by lookup order, because the two registries are
//!   disjoint by test;
//! - a READ tool cannot mutate anything (#199, unchanged by M2's arrival);
//! - an EXECUTING tool can only reach a site the agent OWNS — its context's only
//!   door to a site is the `origin`-checked witness (#208) — but within such a
//!   site it runs the user's code, which is user-level power over this machine
//!   (PLAN §3.1, ledger #197).
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
mod readctx;
mod scratch;
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

/// The MAIL sub-toggle (M2b, D4) — a settings row, deliberately not a schema
/// change: it is user preference, not a fact about a site.
///
/// **What the value MEANS is a filesystem state, not just a flag.** While it is
/// on, every scratch site carries rexenv's `From` stamp; while it is off, none
/// does. `mcp_set_mail_enabled` is what keeps those two in step — see its doc
/// for why the backfill happens at the toggle rather than at provision or
/// lazily at read.
pub const MCP_MAIL_ENABLED_KEY: &str = "mcp_mail_enabled";

/// Is the mail sub-toggle on? Default OFF (D4): a global inbox carrying real
/// sites' password-reset links is not a safe ambient default, so mail is opt-in
/// even once the endpoint itself is enabled.
pub fn mail_enabled(conn: &rusqlite::Connection) -> bool {
    matches!(crate::state::store::get_setting(conn, MCP_MAIL_ENABLED_KEY), Ok(Some(v)) if v == "true")
}

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
            Dispatch::ToolCall { id, name, args } => {
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
                });
                let (reply, outcome, detail) =
                    fulfill_tool_call(&app, id, &name, &args, &acted, &client).await;
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

/// One registered tool, from either registry — the socket's whole tool surface.
///
/// The variants ARE the capability split: a `Read` tool's handler gets a
/// `ReadCtx` (no mutating method), a `Scratch` tool's gets a `ScratchCtx` whose
/// only door to a site is the `origin`-checked witness (#208). Dispatch is the
/// one place that maps a name to a capability, so a tool cannot be routed to a
/// context its module never gave it.
enum Tool {
    Read(&'static tools::ReadTool),
    Scratch(&'static scratch::ScratchTool),
}

/// Look a tool up across BOTH registries. Read side first — an M1 name can never
/// be shadowed by a later scratch tool, and the disjointness guard means that
/// precedence never has to be exercised (see
/// `the_two_registries_are_disjoint_and_say_which_side_a_tool_belongs_on`).
fn find_tool(name: &str) -> Option<Tool> {
    tools::find(name)
        .map(Tool::Read)
        .or_else(|| scratch::find(name).map(Tool::Scratch))
}

/// The `tools/list` result — the UNION of both registries, which is what the
/// socket actually offers. A client sees one flat list; the tier a tool belongs
/// to is a fact about what rexenv will let it do, not something the agent picks.
fn tools_list_result() -> Value {
    let mut list = tools::tools_list_result();
    if let (Some(all), Some(extra)) = (
        list.get_mut("tools").and_then(Value::as_array_mut),
        scratch::tools_list_descriptors().as_array(),
    ) {
        all.extend(extra.iter().cloned());
    }
    list
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
            let creator = AppSiteCreator { app: app.clone() };
            let ctx = scratch::ScratchCtx::new(state.inner(), &creator, &creator, client);
            (t.handler)(ctx, args, acted).await
        }
    };
    match outcome {
        Ok(v) => (result_response(id, tool_success_content(&v)), feed::Outcome::Ok, None),
        Err(e) => (
            result_response(id, tool_error_content(&e.to_string())),
            feed::Outcome::Error,
            Some(e.to_string()),
        ),
    }
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
            crate::commands::sites::create_site_owned(
                self.app.clone(),
                state.inner(),
                jobs.inner(),
                new,
                None,
                None,
                ownership,
            )
            .await
        })
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
    // The sweep exercises handlers for their OUTPUT; a target they record is
    // irrelevant here, so each gets a throwaway recorder.
    let acted = feed::ActedTarget::default();
    for (tool, args) in tools::sweep_plan(fixture_site_id) {
        let text = match (tool.handler)(ctx, &args, &acted).await {
            Ok(v) => serde_json::to_string(&v).unwrap_or_default(),
            Err(e) => e.to_string(),
        };
        outputs.push((tool.name, text));
    }
    // ...and the executing registry too. Walking only M1's would silently narrow
    // "every registered tool's output is swept" to "every READ tool's" the first
    // time a scratch tool lands — the surface-coverage defect family, which is
    // exactly what a second registry invites.
    let creator = AppSiteCreator { app: app.clone() };
    let sctx = scratch::ScratchCtx::new(state.inner(), &creator, &creator, "secret-sweep");
    for (tool, args) in scratch::sweep_plan(fixture_site_id) {
        let text = match (tool.handler)(sctx, &args, &acted).await {
            Ok(v) => serde_json::to_string(&v).unwrap_or_default(),
            Err(e) => e.to_string(),
        };
        outputs.push((tool.name, text));
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
        crate::state::store::touch_site_expiry(conn, id, crate::core::sites::SCRATCH_TTL_HOURS)
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

    /// The violation message when a name appears in BOTH registries, or `None` when
    /// they are disjoint.
    ///
    /// Phrased for the person who trips it — who is, by definition, mid-way through
    /// adding a tool: it names the tool, states the rule, and says which side it
    /// belongs on. A set difference would tell them what happened and not what to
    /// do. (The import guard's lesson: a guard that fires without teaching gets
    /// worked around.)
    fn registry_conflict(read: &[&str], scratch: &[&str]) -> Option<String> {
        let clash: Vec<&str> = read.iter().copied().filter(|n| scratch.contains(n)).collect();
        if clash.is_empty() {
            return None;
        }
        Some(format!(
            "MCP tool name(s) registered in BOTH registries: {}.\n\
             One name, one capability — a tool belongs to exactly one side:\n\
             - `mcp_server/tools.rs` (M1) if it only READS: its handler gets a ReadCtx, which has no \
             mutating method, and the read-only guard scans that module.\n\
             - `mcp_server/scratch.rs` (M2) if it changes or runs anything: its handler gets a \
             ScratchCtx, whose only door to a site is the origin-checked witness.\n\
             Duplicating a name would let the read side shadow the executing one (or the reverse after \
             any reordering), so the tool an agent called would not be the tool that ran. Delete the \
             copy from the side it does not belong on.",
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
        let named: Vec<(&str, Value)> = tools::registry()
            .iter()
            .map(|t| (t.name, (t.sweep_args)("fixture-site-id")))
            .chain(
                scratch::registry()
                    .iter()
                    .map(|t| (t.name, (t.sweep_args)("fixture-site-id"))),
            )
            .collect();
        assert_eq!(
            named.len(),
            tools::registry().len() + scratch::registry().len(),
            "every tool in BOTH registries declares sweep_args"
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
        assert!(after > "2026-08-02 09:00:00".to_string(), "the deadline moved out: {after}");
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
    fn the_two_registries_are_disjoint_and_say_which_side_a_tool_belongs_on() {
        // The load-bearing half of the module split. One name, one capability:
        // a duplicate would mean the tool an agent CALLED is not the tool that
        // RAN (whichever registry dispatch consults first), which is a capability
        // decided by lookup order instead of by where the tool lives.
        let read: Vec<&str> = tools::registry().iter().map(|t| t.name).collect();
        let scratch: Vec<&str> = scratch::registry().iter().map(|t| t.name).collect();
        // `panic!` with the message itself, NOT `assert_eq!(.., None)`: the
        // latter prints it Debug-escaped as one long line with literal \n, which
        // is the guidance made unreadable at the exact moment someone needs it.
        if let Some(msg) = registry_conflict(&read, &scratch) {
            panic!("{msg}");
        }

        // ...and the message a person actually gets. Whoever trips this is
        // mid-way through adding a tool, so it has to teach the rule, not report
        // a set difference.
        let msg = registry_conflict(&["list_sites", "tail_log"], &["scratch_create_site", "tail_log"])
            .expect("a shared name must be caught");
        assert!(msg.contains("tail_log"), "names the offending tool: {msg}");
        assert!(!msg.contains("list_sites"), "names ONLY the offender: {msg}");
        assert!(msg.contains("mcp_server/tools.rs") && msg.contains("mcp_server/scratch.rs"),
                "says which side to put it on: {msg}");
        assert!(msg.contains("only READS") && msg.contains("changes or runs"),
                "states the rule that decides the side: {msg}");
        assert!(msg.contains("shadow"), "says what goes wrong, not just that it did: {msg}");
    }

    #[test]
    fn tools_list_offers_the_union_of_both_registries() {
        // What the SOCKET advertises is both registries, flat — the tier is a
        // fact about what rexenv will let a tool do, never something the agent
        // selects. Today the scratch side is empty, so this pins the count
        // relationship rather than a literal.
        let v = reply(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
        let listed = v["result"]["tools"].as_array().expect("tools array").len();
        assert_eq!(
            listed,
            tools::registry().len() + scratch::registry().len(),
            "tools/list must advertise BOTH registries — a tool that exists but isn't listed is \
             a tool an agent will never call, and one listed twice is a name collision"
        );
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
            // The MAIL sub-toggle's own load-bearing pair (M2b, D4). This is the
            // SECOND place a user consents to something, and the first one only
            // stayed honest because a guard forced it — so both halves are
            // pinned here rather than trusted:
            //   - the scope claim, which is what the user is actually deciding;
            //   - the FAIL-CLOSED direction, which is what a trim cuts first as
            //     "detail" and is the sentence that makes the scope claim
            //     survivable when a site overrides the stamp.
            (
                "your own sites' mail is never returned",
                "what the mail sub-toggle does NOT expose — the claim being consented to",
            ),
            (
                "misses its own mail, never that it sees yours",
                "WHICH WAY mail fails when the stamp is overridden (fail-closed, in plain words)",
            ),
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
        for (phrase, why) in MUST_SAY_WHEN_EXECUTING {
            assert!(
                CARD.contains(phrase),
                "the enable-moment copy no longer tells the user {why} (looked for \"{phrase}\"). \
                 The paragraph is what a security-minded user reads AT the moment of enabling; it \
                 has to describe the capability honestly, including that this is a paved road and \
                 not a sandbox."
            );
        }
    }

}

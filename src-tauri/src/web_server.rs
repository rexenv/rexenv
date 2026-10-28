//! The web UI's server (docs/PLAN-headless-web.md Phase 1b, T6): the same frontend bundle the
//! window shows, served over HTTP on `127.0.0.1:9443` (D5) to a browser reached through
//! `ssh -L`, with the window's own IPC behind it. App-shell code like `cli_server.rs` — it holds
//! the `AppHandle` and runs nothing of its own.
//!
//! - `POST /ipc/<cmd>` → the app's invoke handler, through `Webview::on_message` on the main
//!   (hidden) webview — the same handler, ACL and `State` the window's `invoke` reaches, run on the
//!   main thread as the window's calls are. The command list it may name is the ONE list
//!   (`lib.rs` `for_each_command!`, D2); the rules are `web_rules.rs` (ledger #929).
//! - `GET /events` → server-sent events. A browser `listen(name)` is `POST /events/listen`, which
//!   takes a Tauri `listen_any` on that exact name for that stream; the stream's end takes them
//!   all off. The same names the window listens to, with no list of them to keep.
//! - `GET /…` → the embedded bundle (`AssetResolver`), `index.html` marked so the frontend picks
//!   the web transport; an unknown path is the app's own route, so it gets `index.html` too.
//!
//! Started only by `--web`. Loopback only: a public bind is Phase 2 (TLS, a login).
//!
//! **Every request passes `web_rules::gate` before anything is routed (T7, ledger #930):** the
//! Host is loopback, a token in the URL (`rex web url`; owner-only file) becomes an `HttpOnly;
//! SameSite=Strict` session cookie, and every POST carries our Origin and its session's CSRF
//! header, which the page learns from a `<meta>` in the `index.html` that session is served.

use crate::web_rules::{self as rules, Route};
use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::body::{Frame, Incoming};
use hyper::{Request, Response, StatusCode};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;
use tauri::{AppHandle, Listener, Manager};

type Body = BoxBody<Bytes, Infallible>;

/// One open `/events` stream: where its frames go, and the Tauri listeners feeding it.
struct Stream {
    tx: tokio::sync::mpsc::Sender<Bytes>,
    listeners: Vec<tauri::EventId>,
}

#[derive(Default)]
struct Streams(Mutex<HashMap<String, Stream>>);

/// The sign-in token and the live sessions (session id → its CSRF value). Sessions live in memory:
/// a restart signs every browser out, and the link `rex web url` prints signs it back in.
struct Auth {
    token: String,
    sessions: Mutex<HashMap<String, String>>,
}

/// The address `rex web url` prints, once the server is up.
static SIGN_IN_URL: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// `rex web url`'s answer: the sign-in link, or `None` when this launch serves no web UI.
pub fn sign_in_url() -> Option<String> {
    SIGN_IN_URL.get().cloned()
}

/// 32 random bytes as hex — a token, a session id, a CSRF value.
fn random_hex() -> String {
    use ring::rand::SecureRandom;
    let mut b = [0u8; 32];
    ring::rand::SystemRandom::new().fill(&mut b).expect("the OS random source");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The token from its owner-only file, made on first use. Kept across restarts, so a link the
/// developer saved keeps working; delete the file to change it.
fn load_token() -> crate::error::Result<String> {
    let platform = crate::platform::current();
    let path = platform.paths().config_dir()?.join(rules::TOKEN_FILE);
    if let Ok(t) = std::fs::read_to_string(&path) {
        let t = t.trim().to_string();
        if t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(t);
        }
    }
    let t = random_hex();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    platform.permissions().write_private(&path, t.as_bytes())?;
    Ok(t)
}

impl Streams {
    fn drop_stream(&self, app: &AppHandle, id: &str) {
        let gone = self.0.lock().ok().and_then(|mut m| m.remove(id));
        if let Some(s) = gone {
            for l in s.listeners {
                app.unlisten(l);
            }
        }
    }
}

/// A body fed by a channel: the SSE stream. Ends when every sender is gone.
struct ChannelBody(tokio::sync::mpsc::Receiver<Bytes>);

impl hyper::body::Body for ChannelBody {
    type Data = Bytes;
    type Error = Infallible;
    fn poll_frame(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        self.0.poll_recv(cx).map(|b| b.map(|b| Ok(Frame::data(b))))
    }
}

/// Whether this launch asked for the web UI.
pub fn requested(argv: &[String]) -> bool {
    argv.iter().any(|a| a == rules::WEB_FLAG)
}

/// Start the web UI if `--web` was passed. Port-gated: a holder of 9443 is named, never fought.
pub fn spawn_if_requested(app: AppHandle) {
    let argv: Vec<String> = std::env::args().collect();
    if !requested(&argv) {
        return;
    }
    let platform = crate::platform::current();
    if let Err(e) = crate::core::ports::ensure_free(platform.as_ref(), rules::WEB_PORT, crate::core::ports::Proto::Tcp, "rexenv web UI") {
        log::error!("web: not started — {e}");
        return;
    }
    let token = match load_token() {
        Ok(t) => t,
        Err(e) => {
            log::error!("web: not started — no sign-in token: {e}");
            return;
        }
    };
    let auth = Arc::new(Auth { token, sessions: Mutex::new(HashMap::new()) });
    // The Database Browser's consoles, one origin each (T8, ledger #933) — same sessions.
    for (console, port) in rules::console_ports() {
        if let Err(e) = crate::core::ports::ensure_free(platform.as_ref(), port, crate::core::ports::Proto::Tcp, "rexenv web UI's database console") {
            log::error!("web: the {} console is not served — {e}", console.label());
            continue;
        }
        let (app, auth) = (app.clone(), auth.clone());
        tauri::async_runtime::spawn(async move {
            let addr = std::net::SocketAddr::from((rules::WEB_BIND, port));
            let listener = match tokio::net::TcpListener::bind(addr).await {
                Ok(l) => l,
                Err(e) => {
                    log::error!("web: could not bind {addr} for the {} console: {e}", console.label());
                    return;
                }
            };
            log::info!("web: the {} console on http://{addr}", console.label());
            loop {
                let Ok((tcp, _)) = listener.accept().await else { continue };
                let (app, auth) = (app.clone(), auth.clone());
                tauri::async_runtime::spawn(async move {
                    let svc = hyper::service::service_fn(move |req| {
                        let (app, auth) = (app.clone(), auth.clone());
                        async move { Ok::<_, Infallible>(handle_console(&app, &auth, console, req).await) }
                    });
                    let io = hyper_util::rt::TokioIo::new(tcp);
                    let _ = hyper::server::conn::http1::Builder::new().serve_connection(io, svc).await;
                });
            }
        });
    }
    tauri::async_runtime::spawn(async move {
        let addr = std::net::SocketAddr::from((rules::WEB_BIND, rules::WEB_PORT));
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                log::error!("web: could not bind {addr}: {e}");
                return;
            }
        };
        log::info!("web: serving the UI on http://{addr} (loopback only) — `rex web url` prints its sign-in link");
        let _ = SIGN_IN_URL.set(format!("http://localhost:{}/?{}={}", rules::WEB_PORT, rules::TOKEN_PARAM, auth.token));
        let streams = Arc::new(Streams::default());
        loop {
            let (tcp, _) = match listener.accept().await {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("web: accept failed: {e}");
                    continue;
                }
            };
            let (app, streams, auth) = (app.clone(), streams.clone(), auth.clone());
            tauri::async_runtime::spawn(async move {
                let svc = hyper::service::service_fn(move |req| {
                    let (app, streams, auth) = (app.clone(), streams.clone(), auth.clone());
                    async move { Ok::<_, Infallible>(handle(&app, &streams, &auth, req).await) }
                });
                let io = hyper_util::rt::TokioIo::new(tcp);
                let _ = hyper::server::conn::http1::Builder::new().serve_connection(io, svc).await;
            });
        }
    });
}

fn full(status: StatusCode, content_type: &str, body: impl Into<Bytes>) -> Response<Body> {
    let mut r = Response::new(Full::new(body.into()).boxed());
    *r.status_mut() = status;
    r.headers_mut().insert("content-type", content_type.parse().expect("a content type"));
    r.headers_mut().insert("cache-control", "no-store".parse().expect("a header"));
    r.headers_mut().insert("x-content-type-options", "nosniff".parse().expect("a header"));
    r
}

fn json_reply(status: StatusCode, v: Value) -> Response<Body> {
    full(status, "application/json", v.to_string())
}

fn refused(status: u16, why: &str) -> Response<Body> {
    json_reply(StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST), json!({ "ok": false, "error": why }))
}

async fn read_json(req: Request<Incoming>) -> Option<Value> {
    let limited = http_body_util::Limited::new(req.into_body(), rules::MAX_IPC_BODY);
    let bytes = limited.collect().await.ok()?.to_bytes();
    if bytes.is_empty() {
        return Some(json!({}));
    }
    serde_json::from_slice(&bytes).ok()
}

fn header<'a>(req: &'a Request<Incoming>, name: &str) -> Option<&'a str> {
    req.headers().get(name).and_then(|v| v.to_str().ok())
}

/// One request to a console port: the console gate, then the window's own console proxy
/// (`dbconsole_proxy::forward`: the Rust-side cookie jar, the redirects followed), and its CSP's
/// `frame-ancestors` set to the app's origin alone.
async fn handle_console(app: &AppHandle, auth: &Arc<Auth>, console: crate::core::dbconsole_proxy::Console, req: Request<Incoming>) -> Response<Body> {
    let method = req.method().as_str().to_string();
    let host = header(&req, "host").unwrap_or_default().to_string();
    let session = header(&req, "cookie").and_then(|c| rules::cookie_value(c, rules::SESSION_COOKIE)).map(str::to_string);
    let csrf = session.as_deref().and_then(|id| auth.sessions.lock().ok()?.get(id).cloned());
    let verdict = rules::console_gate(&rules::Ask {
        method: &method,
        host: Some(&host).filter(|h| !h.is_empty()).map(|h| h.as_str()),
        origin: header(&req, "origin"),
        has_token: false,
        session_csrf: csrf.as_deref(),
        csrf_header: None,
    });
    if let rules::Gate::Refuse(status, why) = verdict {
        return full(StatusCode::from_u16(status).unwrap_or(StatusCode::FORBIDDEN), "text/plain; charset=utf-8", why.to_string());
    }
    let path_and_query = req.uri().path_and_query().map(|pq| pq.as_str().to_string()).unwrap_or_else(|| "/".into());
    let content_type = header(&req, "content-type").map(str::to_string);
    let body = match http_body_util::Limited::new(req.into_body(), rules::MAX_CONSOLE_BODY).collect().await {
        Ok(b) => b.to_bytes().to_vec(),
        Err(_) => return full(StatusCode::PAYLOAD_TOO_LARGE, "text/plain; charset=utf-8", "that upload is larger than the web UI takes (256 MB)"),
    };
    // The edge's port as the user set it (`core::edge_ports`); no guess before the state exists.
    let Some(state) = app.try_state::<crate::state::app::AppState>() else {
        return full(StatusCode::SERVICE_UNAVAILABLE, "text/plain; charset=utf-8", "rexenv is still starting — retry in a moment");
    };
    let edge_https_port = crate::commands::services::edge_ports(state.inner()).https;
    match crate::core::dbconsole_proxy::forward(console, edge_https_port, &method, &path_and_query, content_type.as_deref(), body).await {
        Ok(r) => {
            let app_origin = rules::app_origin_for(&host);
            let mut out = Response::new(Full::new(Bytes::from(r.body)).boxed());
            *out.status_mut() = StatusCode::from_u16(r.status).unwrap_or(StatusCode::BAD_GATEWAY);
            let mut had_csp = false;
            for (name, value) in r.headers {
                let value = if name == "content-security-policy" {
                    had_csp = true;
                    rules::console_csp(&value, &app_origin)
                } else {
                    value
                };
                if let (Ok(n), Ok(v)) = (hyper::header::HeaderName::from_bytes(name.as_bytes()), value.parse()) {
                    out.headers_mut().append(n, v);
                }
            }
            if !had_csp {
                if let Ok(v) = rules::console_csp("", &app_origin).parse() {
                    out.headers_mut().insert("content-security-policy", v);
                }
            }
            out
        }
        Err(e) => full(StatusCode::BAD_GATEWAY, "text/plain; charset=utf-8", format!("rexenv db proxy: {e}")),
    }
}

async fn handle(app: &AppHandle, streams: &Arc<Streams>, auth: &Arc<Auth>, req: Request<Incoming>) -> Response<Body> {
    let names = crate::command_names();
    let method = req.method().as_str().to_string();
    let path = req.uri().path().to_string();
    let session = header(&req, "cookie").and_then(|c| rules::cookie_value(c, rules::SESSION_COOKIE)).map(str::to_string);
    let csrf = session.as_deref().and_then(|id| auth.sessions.lock().ok()?.get(id).cloned());
    let token = rules::token_in_query(req.uri().query()).map(str::to_string);
    let verdict = rules::gate(&rules::Ask {
        method: &method,
        host: header(&req, "host"),
        origin: header(&req, "origin"),
        has_token: token.is_some(),
        session_csrf: csrf.as_deref(),
        csrf_header: header(&req, rules::CSRF_HEADER),
    });
    let csrf = match verdict {
        // A page the browser navigated to reads the sentence as text; a script's call gets JSON.
        rules::Gate::Refuse(status, why) if method == "GET" && !path.starts_with("/events") => {
            return full(StatusCode::from_u16(status).unwrap_or(StatusCode::FORBIDDEN), "text/plain; charset=utf-8", why.to_string());
        }
        rules::Gate::Refuse(status, why) => return refused(status, why),
        rules::Gate::Exchange => return exchange(auth, token.as_deref().unwrap_or_default(), &path),
        rules::Gate::Pass => csrf.unwrap_or_default(),
    };
    match rules::route(&method, &path, &names) {
        Route::Ipc(cmd) => {
            let cmd = cmd.to_string();
            let Some(args) = read_json(req).await else {
                return refused(400, "the body is not JSON");
            };
            match dispatch(app, &cmd, args).await {
                Ok(data) => json_reply(StatusCode::OK, json!({ "ok": true, "data": data })),
                Err(error) => json_reply(StatusCode::OK, json!({ "ok": false, "error": error })),
            }
        }
        Route::Events => open_stream(app, streams),
        Route::Listen => {
            let Some(v) = read_json(req).await else { return refused(400, "the body is not JSON") };
            let (Some(stream), Some(event)) = (v["stream"].as_str(), v["event"].as_str()) else {
                return refused(400, "listen takes {stream, event}");
            };
            listen(app, streams, stream, event)
        }
        Route::Unlisten => {
            let Some(v) = read_json(req).await else { return refused(400, "the body is not JSON") };
            let (Some(stream), Some(id)) = (v["stream"].as_str(), v["id"].as_u64()) else {
                return refused(400, "unlisten takes {stream, id}");
            };
            let mine = streams.0.lock().ok().is_some_and(|mut m| {
                m.get_mut(stream).is_some_and(|s| {
                    let before = s.listeners.len();
                    s.listeners.retain(|l| u64::from(*l) != id);
                    before != s.listeners.len()
                })
            });
            if mine {
                app.unlisten(id as tauri::EventId);
            }
            json_reply(StatusCode::OK, json!({ "ok": true }))
        }
        Route::Static(rel) => asset(app, rel, None),
        Route::Index => asset(app, "index.html", Some(&csrf)),
        Route::Refused(status, why) => refused(status, why),
    }
}

/// A sign-in link: a right token starts a session and sends the browser to the same path without
/// the token (so it leaves the address bar); a wrong one is refused.
fn exchange(auth: &Auth, token: &str, path: &str) -> Response<Body> {
    if !rules::same_secret(token, &auth.token) {
        return refused(403, "that sign-in link is not this server's — run `rex web url` again");
    }
    let (id, csrf) = (random_hex(), random_hex());
    if let Ok(mut s) = auth.sessions.lock() {
        s.insert(id.clone(), csrf);
    }
    let mut r = full(StatusCode::SEE_OTHER, "text/plain", "");
    r.headers_mut().insert("location", rules::after_sign_in(path).parse().expect("a plain path"));
    r.headers_mut().insert("set-cookie", rules::session_cookie(&id).parse().expect("a cookie"));
    r.headers_mut().insert("referrer-policy", "no-referrer".parse().expect("a header"));
    r
}

fn asset(app: &AppHandle, rel: &str, index_for: Option<&str>) -> Response<Body> {
    let Some(a) = app.asset_resolver().get(rel.to_string()) else {
        return refused(404, "not part of rexenv's UI");
    };
    let mut r = if let Some(csrf) = index_for {
        let html = String::from_utf8_lossy(&a.bytes);
        match rules::mark_index(&html, csrf) {
            Some(marked) => full(StatusCode::OK, "text/html; charset=utf-8", marked),
            None => return refused(500, "the bundle's index.html has no <head>"),
        }
    } else {
        full(StatusCode::OK, &a.mime_type, a.bytes)
    };
    r.headers_mut().insert("content-security-policy", rules::CSP.parse().expect("a header"));
    r.headers_mut().insert("x-frame-options", "DENY".parse().expect("a header"));
    r.headers_mut().insert("referrer-policy", "no-referrer".parse().expect("a header"));
    r
}

/// Run one command through the main webview's own invoke handler and wait for its answer.
async fn dispatch(app: &AppHandle, cmd: &str, args: Value) -> Result<Value, Value> {
    use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponse, InvokeResponseBody};
    let Some(window) = app.get_webview_window("main") else {
        return Err(json!("rexenv's window is not up yet — retry in a moment"));
    };
    let webview: tauri::Webview = window.as_ref().clone();
    let url = webview.url().map_err(|e| json!(e.to_string()))?;
    let request = tauri::webview::InvokeRequest {
        cmd: cmd.to_string(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url,
        body: InvokeBody::Json(args),
        headers: Default::default(),
        invoke_key: app.invoke_key().to_string(),
    };
    let (tx, rx) = tokio::sync::oneshot::channel::<InvokeResponse>();
    // The main thread, as the window's own calls run: a sync command that touches a window or a
    // menu must not find itself on a tokio worker.
    app.run_on_main_thread(move || {
        webview.on_message(
            request,
            Box::new(move |_webview, _cmd, response, _ok, _err| {
                let _ = tx.send(response);
            }),
        );
    })
    .map_err(|e| json!(e.to_string()))?;
    match rx.await {
        Ok(InvokeResponse::Ok(InvokeResponseBody::Json(s))) => Ok(serde_json::from_str(&s).unwrap_or(Value::Null)),
        Ok(InvokeResponse::Ok(InvokeResponseBody::Raw(bytes))) => Ok(Value::from(bytes)),
        Ok(InvokeResponse::Err(e)) => Err(e.0),
        Err(_) => Err(json!("the command ended without an answer")),
    }
}

fn open_stream(app: &AppHandle, streams: &Arc<Streams>) -> Response<Body> {
    let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(1024);
    let id = uuid::Uuid::new_v4().simple().to_string();
    let hello = rules::sse_frame("hello", &json!({ "stream": id }).to_string());
    let _ = tx.try_send(Bytes::from(hello));
    if let Ok(mut m) = streams.0.lock() {
        m.insert(id.clone(), Stream { tx: tx.clone(), listeners: Vec::new() });
    }
    // A comment every 15 s: keeps proxies from closing an idle stream, and its failed send is
    // how a closed browser tab is noticed — its listeners then come off.
    let (app, streams) = (app.clone(), streams.clone());
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(15)).await;
            if tx.send(Bytes::from_static(b": keepalive\n\n")).await.is_err() {
                streams.drop_stream(&app, &id);
                return;
            }
        }
    });
    let mut r = Response::new(ChannelBody(rx).boxed());
    r.headers_mut().insert("content-type", "text/event-stream".parse().expect("a header"));
    r.headers_mut().insert("cache-control", "no-store".parse().expect("a header"));
    r
}

fn listen(app: &AppHandle, streams: &Arc<Streams>, stream: &str, event: &str) -> Response<Body> {
    if !rules::valid_event_name(event) {
        return refused(400, "not an event name");
    }
    let Some(tx) = streams.0.lock().ok().and_then(|m| m.get(stream).map(|s| s.tx.clone())) else {
        return refused(404, "no such event stream — reconnect");
    };
    let name = event.to_string();
    let slot: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
    let id_for_frame = slot.clone();
    let id = app.listen_any(event.to_string(), move |e| {
        let id = id_for_frame.lock().map(|g| *g).unwrap_or(0);
        if tx.try_send(Bytes::from(rules::sse_event(id, &name, e.payload()))).is_err() {
            log::warn!("web: an event stream is full or gone — dropped one {name} event");
        }
    });
    if let Ok(mut g) = slot.lock() {
        *g = id;
    }
    let kept = streams.0.lock().ok().is_some_and(|mut m| m.get_mut(stream).map(|s| s.listeners.push(id)).is_some());
    if !kept {
        app.unlisten(id);
        return refused(404, "no such event stream — reconnect");
    }
    json_reply(StatusCode::OK, json!({ "ok": true, "id": id }))
}

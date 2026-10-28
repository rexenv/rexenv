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
    tauri::async_runtime::spawn(async move {
        let addr = std::net::SocketAddr::from((rules::WEB_BIND, rules::WEB_PORT));
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                log::error!("web: could not bind {addr}: {e}");
                return;
            }
        };
        log::info!("web: serving the UI on http://{addr} (loopback only)");
        let streams = Arc::new(Streams::default());
        loop {
            let (tcp, _) = match listener.accept().await {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("web: accept failed: {e}");
                    continue;
                }
            };
            let (app, streams) = (app.clone(), streams.clone());
            tauri::async_runtime::spawn(async move {
                let svc = hyper::service::service_fn(move |req| {
                    let (app, streams) = (app.clone(), streams.clone());
                    async move { Ok::<_, Infallible>(handle(&app, &streams, req).await) }
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

async fn handle(app: &AppHandle, streams: &Arc<Streams>, req: Request<Incoming>) -> Response<Body> {
    let names = crate::command_names();
    let method = req.method().as_str().to_string();
    let path = req.uri().path().to_string();
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
        Route::Static(rel) => asset(app, rel, false),
        Route::Index => asset(app, "index.html", true),
        Route::Refused(status, why) => refused(status, why),
    }
}

fn asset(app: &AppHandle, rel: &str, index: bool) -> Response<Body> {
    let Some(a) = app.asset_resolver().get(rel.to_string()) else {
        return refused(404, "not part of rexenv's UI");
    };
    let mut r = if index {
        let html = String::from_utf8_lossy(&a.bytes);
        match rules::mark_index(&html) {
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

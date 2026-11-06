//! The web UI's rules as pure text (docs/PLAN-headless-web.md Phase 1b, T6): what a request may
//! name, which file a path may serve, and how an event reaches the browser. `web_server.rs` runs
//! them; nothing here touches a socket, so every rule is an L0 test on any host.
//!
//! **What the web UI may reach is the webview's surface, exactly.** `/ipc/<name>` is dispatched
//! through the app's OWN invoke handler (`Webview::on_message`, the door the desktop webview's
//! `invoke` takes), and only for a name in [`crate::command_names`] — the list `generate_handler!`
//! is generated from (D2). So the browser cannot reach a command the window cannot, and a plugin
//! command (`plugin:dialog|open`) is refused by name before it is dispatched (ledger #929).

/// `rexenv --web`: serve the UI on [`WEB_PORT`], loopback only (Phase 1b).
pub const WEB_FLAG: &str = "--web";
/// Owner ruling D5 (28 Oct 2026). Port-gated like every service (`core/ports::ensure_free`).
pub const WEB_PORT: u16 = 9443;
/// The ONE bind the web UI takes in Phase 1b. A public bind is Phase 2 and needs TLS and a login.
pub const WEB_BIND: [u8; 4] = [127, 0, 0, 1];

/// The Database Browser's consoles over the web (T8): ONE loopback port each, because each needs
/// an origin of its own — phpMyAdmin's pages use absolute paths (`/index.php`, `/js/…`), so it
/// cannot live under a path of the app's origin, and two consoles must never share a cookie jar
/// (the same reason the window gives each its own `rexdb:` / `rexpma:` scheme, ledger #870).
/// Gated by the app's session — a cookie is per host, not per port, so the one sign-in covers
/// them (ledger #933). Port-gated like 9443; a tunnel forwards all three.
pub const ADMINER_WEB_PORT: u16 = 9444;
pub const PHPMYADMIN_WEB_PORT: u16 = 9445;

/// Each console with its web port.
pub fn console_ports() -> [(crate::core::dbconsole_proxy::Console, u16); 2] {
    use crate::core::dbconsole_proxy::Console;
    [(Console::Adminer, ADMINER_WEB_PORT), (Console::PhpMyAdmin, PHPMYADMIN_WEB_PORT)]
}

/// The IPv6 loopback, held on the same ports as [`WEB_BIND`] (ledger #935): left free, any local
/// user could bind `[::1]:9443` and sit in front of a tunnel whose `localhost` resolves to it.
pub const WEB_BIND_V6: [u16; 8] = [0, 0, 0, 0, 0, 0, 0, 1];

/// Every port the web UI holds, with what it is — all or none of them (ledger #935).
pub fn web_ports() -> [(u16, &'static str); 3] {
    [
        (WEB_PORT, "rexenv web UI"),
        (ADMINER_WEB_PORT, "rexenv web UI's Adminer console"),
        (PHPMYADMIN_WEB_PORT, "rexenv web UI's phpMyAdmin console"),
    ]
}

/// Whether a failed `[::1]` bind stops the web UI. A host with no IPv6 loopback (disabled, or no
/// address) cannot be squatted on it either, so that alone is not a reason to stop; anything else
/// — the port held, a permission — is.
pub fn v6_bind_failure_is_fatal(kind: std::io::ErrorKind) -> bool {
    !matches!(kind, std::io::ErrorKind::AddrNotAvailable | std::io::ErrorKind::Unsupported)
}

/// Open connections served at once, across every web port. The SSE stream holds one per tab.
pub const MAX_CONNECTIONS: usize = 256;
/// How long a connection may take to send its request headers before it is closed.
pub const HEADER_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// The largest console request body read — a phpMyAdmin import upload rides it.
pub const MAX_CONSOLE_BODY: usize = 256 * 1024 * 1024;
/// **How many large console uploads are held in memory at once** (security review of PR #10, item 4,
/// ledger #951). A console body is buffered whole — the proxy replays it on a 307/308, which a stream
/// cannot — so without a cap `MAX_CONNECTIONS` uploads of `MAX_CONSOLE_BODY` each could all be in
/// memory together (64 GB). Two at a time bounds it at 512 MB; a third waits its turn, it is not refused.
pub const MAX_CONCURRENT_CONSOLE_UPLOADS: usize = 2;
/// A body at or under this is a form post, not an upload: it takes no turn.
pub const CONSOLE_UPLOAD_THRESHOLD: u64 = 1024 * 1024;
// The bound the cap promises, checked when it compiles: two turns of the largest body, 512 MB.
const _: () = assert!(MAX_CONCURRENT_CONSOLE_UPLOADS * MAX_CONSOLE_BODY <= 512 * 1024 * 1024);

/// Whether a console request must wait for one of the upload turns: a body-carrying method whose
/// declared length is over the threshold, or not declared (chunked — it could be anything).
pub fn console_upload_takes_a_turn(method: &str, content_length: Option<u64>) -> bool {
    !matches!(method, "GET" | "HEAD" | "OPTIONS") && content_length.map_or(true, |n| n > CONSOLE_UPLOAD_THRESHOLD)
}

/// The largest `/ipc` body read. Commands carry paths and form values, never file contents.
pub const MAX_IPC_BODY: usize = 4 * 1024 * 1024;

/// The marker `index.html` gets when the web server serves it: the frontend's `transport()` reads
/// it and talks `fetch`/SSE instead of Tauri's bridge. A `<meta>` and not a script — the page's
/// CSP allows no inline script, and must not start to.
pub const TRANSPORT_META: &str = r#"<meta name="rexenv-transport" content="web" />"#;

/// The page's policy in a browser. The desktop's (tauri.conf.json) minus the IPC and custom-scheme
/// origins a browser does not have; `frame-ancestors 'none'` because nothing should frame the UI.
pub const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
img-src 'self' data: https://*.w.org; font-src 'self' data:; connect-src 'self'; \
frame-src 'self' http://rexenv.localhost:9444 http://rexenv.localhost:9445 http://localhost:9444 http://127.0.0.1:9444 http://localhost:9445 http://127.0.0.1:9445; \
object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'";

// ── Auth (T7) ────────────────────────────────────────────────────────────────────────────────
//
// Loopback on a server is NOT private: every local user and every process on the box reaches
// 127.0.0.1:9443 — a WordPress plugin's `wp_remote_get` included — and a page in the developer's
// own browser can aim requests at it. So every request passes a gate (`gate`), in this order:
// the Host is ours (DNS rebinding), a token in the URL is traded for a session cookie, every other
// request carries that cookie, and every POST carries our Origin AND the per-session CSRF header.

/// The session cookie: `HttpOnly` (no script reads it), `SameSite=Strict` (no other site's page
/// sends it), `Path=/`. No `Secure` — the tunnel is plain `http://localhost`, a secure context.
pub const SESSION_COOKIE: &str = "rexenv_web";
/// The header every POST carries, holding its session's CSRF value.
pub const CSRF_HEADER: &str = "x-rexenv-csrf";
/// Where the page learns that value: a `<meta>` in the `index.html` a session is served — a
/// cross-site page cannot read our HTML, so it cannot learn it.
pub const CSRF_META: &str = "rexenv-csrf";
/// The URL parameter carrying the sign-in code (`rex web url`).
pub const TOKEN_PARAM: &str = "t";
/// Where a PERMANENT sign-in token lived until 7 Nov 2026 (config dir, owner-only). A link made from it
/// worked forever — in shell history, a browser's history, a pasted chat — so it was replaced by
/// single-use codes; the file is deleted when the web UI starts, so no secret outlives its use.
pub const TOKEN_FILE: &str = "web-token";

/// **How long a sign-in link works, how long a session lives** (security review of PR #10, item 2,
/// 29 Oct 2026: "the token never expires, sessions have no idle/absolute limit or sign-out"). Each
/// `rex web url` mints a NEW code that signs in ONE browser, once, within this window — enough to
/// copy it into a browser, short enough that a leaked link is dead by the time anyone finds it.
pub const SIGN_IN_CODE_TTL: std::time::Duration = std::time::Duration::from_secs(10 * 60);
/// A session unused this long is signed out — a working day, so a lunch break does not cost a sign-in.
pub const SESSION_IDLE: std::time::Duration = std::time::Duration::from_secs(8 * 60 * 60);
/// No session outlives this, used or not: one `rex web url` a day, at most.
pub const SESSION_MAX: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// Whether a sign-in code minted `age` ago may still be traded for a session.
pub fn code_live(age: std::time::Duration) -> bool {
    age < SIGN_IN_CODE_TTL
}

/// Whether a session started `age` ago and last used `idle` ago is still signed in.
pub fn session_live(age: std::time::Duration, idle: std::time::Duration) -> bool {
    age < SESSION_MAX && idle < SESSION_IDLE
}

/// The `Set-Cookie` that ends a session in the browser too (`POST /signout`).
pub fn session_cookie_cleared() -> String {
    format!("{SESSION_COOKIE}=; Max-Age=0; HttpOnly; SameSite=Strict; Path=/")
}

/// What a request without a session is told.
pub const NO_SESSION: &str = "rexenv's web UI needs its sign-in link. On the server, run `rex web url` \
and open the address it prints (through your SSH tunnel).";

/// Whether `host` (the `Host` header) names this machine's loopback: `localhost`, `127.0.0.1` or
/// `[::1]`, any port — an `ssh -L 8000:localhost:9443` tunnel arrives as `localhost:8000`. A DNS
/// rebinding page arrives with ITS name, which is the attack this refuses.
///
/// `rexenv.localhost` is loopback too — the one name a session lives on (`WEB_HOST`).
pub fn host_ok(host: Option<&str>) -> bool {
    let Some(host) = host else { return false };
    let name = match host.strip_prefix('[') {
        Some(rest) => match rest.split_once(']') {
            Some((name, port)) if port.is_empty() || port.strip_prefix(':').is_some_and(all_digits) => return name == "::1",
            _ => return false,
        },
        None => match host.split_once(':') {
            Some((name, port)) if all_digits(port) => name,
            Some(_) => return false,
            None => host,
        },
    };
    name.eq_ignore_ascii_case("localhost") || name == "127.0.0.1" || name.eq_ignore_ascii_case(WEB_HOST)
}

/// **The name the web UI is signed in on** (security review of PR #10, item 1, ledger #952). A cookie is
/// per HOST, never per port: a session cookie set on `localhost` went to every `http://localhost:*` the
/// developer's browser opened — a hostile dev server on the laptop received it and could replay it
/// through the tunnel, `index.html` (CSRF value included) and all. Browsers resolve every `*.localhost`
/// name to loopback themselves (Chromium, WebKit and Firefox; macOS's resolver too — measured 7 Nov
/// 2026), so the sign-in happens on this name and the cookie is host-only to it: no other localhost
/// server sees it. The consoles live on the same name (`rexenv.localhost:9444`), so it reaches them.
pub const WEB_HOST: &str = "rexenv.localhost";

/// Whether `host` (a `Host` header that passed [`host_ok`]) is [`WEB_HOST`], any port.
pub fn is_web_host(host: &str) -> bool {
    let name = host.rsplit_once(':').filter(|(_, p)| all_digits(p)).map_or(host, |(n, _)| n);
    name.eq_ignore_ascii_case(WEB_HOST)
}

/// The same request on [`WEB_HOST`]: a sign-in link opened on `localhost:<port>` is sent there with
/// its code unspent, keeping the tunnel's port.
pub fn on_web_host(host: &str, path_and_query: &str) -> String {
    let port = host.rsplit_once(':').filter(|(_, p)| all_digits(p)).map(|(_, p)| format!(":{p}")).unwrap_or_default();
    format!("http://{WEB_HOST}{port}{path_and_query}")
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.len() <= 5 && s.bytes().all(|b| b.is_ascii_digit())
}

/// Whether `origin` is this page's own: exactly `http://<host>`.
pub fn origin_ok(origin: Option<&str>, host: &str) -> bool {
    origin.is_some_and(|o| o == format!("http://{host}"))
}

/// One cookie's value from a `Cookie` header.
pub fn cookie_value<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    header.split(';').map(str::trim).find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == name).map(|(_, v)| v))
}

/// The value of `t=` in a query string.
pub fn token_in_query(query: Option<&str>) -> Option<&str> {
    query?.split('&').find_map(|kv| kv.strip_prefix(TOKEN_PARAM).and_then(|r| r.strip_prefix('=')))
}

/// Compare two secrets without leaking, through timing, how much of a guess was right.
pub fn same_secret(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The `Set-Cookie` that starts a session.
pub fn session_cookie(id: &str) -> String {
    format!("{SESSION_COOKIE}={id}; HttpOnly; SameSite=Strict; Path=/")
}

/// Where a sign-in link sends the browser: the same app route without the token, or `/`. Never
/// `//host` (another site) and never anything but plain route characters (it becomes a header).
pub fn after_sign_in(path: &str) -> &str {
    let plain = path.starts_with('/')
        && !path.starts_with("//")
        && path.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_'));
    if plain { path } else { "/" }
}

/// The page a spent sign-in link answers with: it moves the browser on to `after_sign_in(path)` itself.
/// A `303` would not do from a link opened on `localhost` (moved to [`WEB_HOST`], #952): the redirect
/// chain then started on another site, and WebKit and Chromium send no `SameSite=Strict` cookie on its
/// last hop — the browser arrived signed out until a reload (measured 7 Nov 2026). A `<meta refresh>` is a
/// navigation THIS page starts, so it is same-site; no script, so the page's CSP is untouched.
pub fn after_sign_in_page(path: &str) -> String {
    let to = after_sign_in(path);
    format!(
        "<!doctype html><meta charset=\"utf-8\"><meta http-equiv=\"refresh\" content=\"0;url={to}\">\
         <title>rexenv</title><p>Signed in — <a href=\"{to}\">open rexenv</a>.</p>"
    )
}

/// What the gate decides for one request.
#[derive(Debug, PartialEq, Eq)]
pub enum Gate {
    /// Through, as this session.
    Pass,
    /// A token in the URL: check it, start a session, send the browser to the same path without it.
    Exchange,
    /// A token in the URL on a host other than [`WEB_HOST`]: send the browser to `WEB_HOST`, code unspent.
    MoveToWebHost,
    /// Refused, with the status and a sentence.
    Refuse(u16, &'static str),
}

/// The facts the gate weighs, gathered by the server from one request.
pub struct Ask<'a> {
    pub method: &'a str,
    pub host: Option<&'a str>,
    pub origin: Option<&'a str>,
    pub has_token: bool,
    /// The session the cookie names, if it names a live one: its CSRF value.
    pub session_csrf: Option<&'a str>,
    pub csrf_header: Option<&'a str>,
}

/// **The one gate every request passes** (ledger #930). Nothing is routed before it.
pub fn gate(a: &Ask) -> Gate {
    if !host_ok(a.host) {
        return Gate::Refuse(403, "rexenv's web UI answers only to localhost");
    }
    if let Some(origin) = a.origin {
        // A browser names the page that sent the request; on a GET too (a cross-site `<img>`,
        // a form): it must be ours wherever it is said.
        if !origin_ok(Some(origin), a.host.unwrap_or_default()) {
            return Gate::Refuse(403, "a request from another site");
        }
    }
    if a.has_token && a.method == "GET" {
        return if is_web_host(a.host.unwrap_or_default()) { Gate::Exchange } else { Gate::MoveToWebHost };
    }
    let Some(csrf) = a.session_csrf else {
        return Gate::Refuse(401, NO_SESSION);
    };
    if a.method != "GET" && a.method != "HEAD" {
        if a.origin.is_none() {
            return Gate::Refuse(403, "a request that does not say where it came from");
        }
        if !a.csrf_header.is_some_and(|h| same_secret(h, csrf)) {
            return Gate::Refuse(403, "a request without this page's CSRF header");
        }
    }
    Gate::Pass
}

/// **The consoles' gate** (ledger #933): the app's gate minus what a console cannot do — its pages
/// are third-party PHP whose forms cannot add our CSRF header, and nobody signs in on a console
/// port. So: a loopback `Host`; an `Origin`, wherever said, is the console's OWN; a live session
/// (the app's cookie, sent because cookies are per host); and a non-GET must say its `Origin` —
/// a form inside the console frame always does, a page of another port or site gets 403.
pub fn console_gate(a: &Ask) -> Gate {
    if !host_ok(a.host) {
        return Gate::Refuse(403, "rexenv's web UI answers only to localhost");
    }
    if a.origin.is_some_and(|o| !origin_ok(Some(o), a.host.unwrap_or_default())) {
        return Gate::Refuse(403, "a request from another site");
    }
    if a.session_csrf.is_none() {
        return Gate::Refuse(401, NO_SESSION);
    }
    if a.method != "GET" && a.method != "HEAD" && a.origin.is_none() {
        return Gate::Refuse(403, "a request that does not say where it came from");
    }
    Gate::Pass
}

/// The app's origin as seen from a console request's `Host`: the same name on [`WEB_PORT`]. The
/// only page allowed to frame a console (`frame-ancestors`), so no other page can.
pub fn app_origin_for(console_host: &str) -> String {
    let name = match console_host.strip_prefix('[') {
        Some(rest) => format!("[{}]", rest.split(']').next().unwrap_or_default()),
        None => console_host.split(':').next().unwrap_or_default().to_string(),
    };
    format!("http://{name}:{WEB_PORT}")
}

/// A console's CSP with `frame-ancestors` set to the app's origin alone (every other directive —
/// the script nonce above all — verbatim; one added when the console sent none).
pub fn console_csp(csp: &str, app_origin: &str) -> String {
    let mut out: Vec<String> = csp
        .split(';')
        .map(str::trim)
        .filter(|d| !d.is_empty() && !d.to_ascii_lowercase().starts_with("frame-ancestors"))
        .map(str::to_string)
        .collect();
    out.push(format!("frame-ancestors {app_origin}"));
    out.join("; ")
}

/// What one request is.
#[derive(Debug, PartialEq, Eq)]
pub enum Route<'a> {
    /// `POST /ipc/<command>` — a registered command, by name.
    Ipc(&'a str),
    /// `GET /events` — the server-sent event stream.
    Events,
    /// `POST /events/listen` / `POST /events/unlisten`.
    Listen,
    Unlisten,
    /// `POST /signout` — end this session (behind the gate like any POST: session, Origin, CSRF).
    SignOut,
    /// `GET` of a built file (`assets/…`, `favicon…`), relative to the bundle root.
    Static(&'a str),
    /// `GET` of anything a client-side route could be: the app's `index.html`.
    Index,
    /// Refused, with the HTTP status and a sentence.
    Refused(u16, &'static str),
}

/// Route one request. `names` is the registered command list (`crate::command_names`).
pub fn route<'a>(method: &str, path: &'a str, names: &[&str]) -> Route<'a> {
    // The path as sent, never decoded: a `%2e%2e` cannot become `..` after the checks below.
    if path.contains('%') || path.contains('\\') || !path.starts_with('/') {
        return Route::Refused(400, "a path rexenv does not serve");
    }
    if let Some(cmd) = path.strip_prefix("/ipc/") {
        if method != "POST" {
            return Route::Refused(405, "commands are POSTed");
        }
        return if names.contains(&cmd) { Route::Ipc(cmd) } else { Route::Refused(404, "no such command") };
    }
    match (method, path) {
        ("GET", "/events") => return Route::Events,
        ("POST", "/events/listen") => return Route::Listen,
        ("POST", "/events/unlisten") => return Route::Unlisten,
        ("POST", "/signout") => return Route::SignOut,
        (_, "/events" | "/events/listen" | "/events/unlisten" | "/signout") => return Route::Refused(405, "wrong method"),
        _ => {}
    }
    if method != "GET" && method != "HEAD" {
        return Route::Refused(405, "wrong method");
    }
    let rel = &path[1..];
    let last = rel.rsplit('/').next().unwrap_or("");
    if rel.is_empty() || !last.contains('.') {
        // `/`, `/sites`, `/sites/abc` — the SPA's own routes.
        return if rel.split('/').any(|s| s == ".." || s == ".") { Route::Refused(400, "a path rexenv does not serve") } else { Route::Index };
    }
    match static_path(rel) {
        Some(p) => Route::Static(p),
        None => Route::Refused(404, "not part of rexenv's UI"),
    }
}

/// A built file's path relative to the bundle, or `None`. Only plain segments — no `..`, no
/// `.`-file, no empty segment — because in a dev build Tauri's resolver reads `dist/` from disk
/// and filters nothing but the root (`AssetResolver::get`, tauri 2.11): a `..` would walk out.
pub fn static_path(rel: &str) -> Option<&str> {
    let ok = !rel.is_empty()
        && rel.split('/').all(|seg| {
            !seg.is_empty()
                && !seg.starts_with('.')
                && seg.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        });
    ok.then_some(rel)
}

/// Where the page frames each console: `<meta name="rexenv-consoles" content="adminer=9444,phpmyadmin=9445">`
/// — the ports written once, here; the frontend builds `http://<its hostname>:<port>` from it.
pub fn consoles_meta() -> String {
    let pairs: Vec<String> = console_ports().iter().map(|(c, p)| format!("{}={p}", c.label())).collect();
    format!(r#"<meta name="rexenv-consoles" content="{}" />"#, pairs.join(","))
}

/// `index.html` with the transport marker and this session's CSRF value, placed first in
/// `<head>` so both exist before any module runs. `None` when the document has no `<head>` (not
/// ours) or the value is not plain hex (it is written into an attribute).
pub fn mark_index(html: &str, csrf: &str) -> Option<String> {
    if csrf.is_empty() || !csrf.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let at = html.find("<head>")? + "<head>".len();
    Some(format!(
        "{}\n    {TRANSPORT_META}\n    <meta name=\"{CSRF_META}\" content=\"{csrf}\" />\n    {}{}",
        &html[..at],
        consoles_meta(),
        &html[at..]
    ))
}

/// A name Tauri's `listen_any` accepts. It PANICS on anything else ("alphanumeric, `-`, `/`, `:`
/// and `_`" — tauri 2.11 `EventName::new(..).unwrap()`), and the name comes from a browser.
pub fn valid_event_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 256 && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '/' | ':' | '_'))
}

/// One server-sent event carrying a Tauri event: `{"id":<listener>,"event":<name>,"payload":<json>}`.
/// `payload` is the JSON Tauri already serialized; a newline in it becomes a second `data:` line,
/// which SSE joins back with `\n` — whitespace to JSON — so no frame can be cut short.
pub fn sse_event(listener: u32, name: &str, payload_json: &str) -> String {
    let body = format!(r#"{{"id":{listener},"event":{},"payload":{payload_json}}}"#, serde_json::Value::from(name));
    sse_frame("message", &body)
}

/// A named SSE frame.
pub fn sse_frame(kind: &str, data: &str) -> String {
    let mut out = format!("event: {kind}\n");
    for line in data.split('\n') {
        out.push_str("data: ");
        out.push_str(line.trim_end_matches('\r'));
        out.push('\n');
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only a real upload takes a turn: a GET, a small form post and an OPTIONS never wait; a large or
    /// undeclared (chunked) body does.
    #[test]
    fn only_large_console_uploads_wait_for_a_turn() {
        assert!(!console_upload_takes_a_turn("GET", None));
        assert!(!console_upload_takes_a_turn("POST", Some(4096)));
        assert!(!console_upload_takes_a_turn("POST", Some(CONSOLE_UPLOAD_THRESHOLD)));
        assert!(console_upload_takes_a_turn("POST", Some(CONSOLE_UPLOAD_THRESHOLD + 1)));
        assert!(console_upload_takes_a_turn("POST", None), "chunked: could be anything");
    }

    /// The turn is taken before the body is read, and the proxy shares one copy across its sends.
    #[test]
    fn a_console_upload_waits_before_it_is_buffered_and_is_never_copied_per_send() {
        let server = crate::core::copy_scan::production_source(
            &std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/web_server.rs")).expect("web_server.rs"),
        );
        let h = &server[server.find("async fn handle_console(").expect("handle_console")..];
        assert!(h.find("UPLOAD_TURNS.acquire()").expect("the turn") < h.find("read_into_one_buffer(").expect("the read"));
        assert!(!h.contains(".collect().await"), "collect() + to_bytes() holds the chunks and their copy together");
        assert!(!h[..h.find("dbconsole_proxy::forward(").expect("forward")].contains(".to_vec()"), "the body copied before the proxy");
        let proxy = crate::core::copy_scan::production_source(
            &std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/core/dbconsole_proxy.rs")).expect("dbconsole_proxy.rs"),
        );
        assert!(proxy.contains("let mut body: bytes::Bytes = body.into();"), "the proxy holds the body as Bytes");
    }

    /// The sign-in answer moves the browser on by itself, to a plain path only (it lands in HTML).
    #[test]
    fn the_sign_in_page_moves_on_to_a_plain_path() {
        let page = after_sign_in_page("/sites");
        assert!(page.contains(r#"http-equiv="refresh" content="0;url=/sites""#), "{page}");
        assert!(!page.contains("<script"), "no script: the CSP forbids inline script");
        let hostile = after_sign_in_page("/\"><script>x</script>");
        assert!(hostile.contains("content=\"0;url=/\"") && !hostile.contains("<script"), "{hostile}");
    }

    /// The page may frame each console on the name it is signed in on: #952 moved the app to
    /// `rexenv.localhost`, the frame followed (`webConsoleOrigins` builds it from the page's hostname),
    /// and the CSP still listed only `localhost` — the site's Database tab showed a refused frame until
    /// the route walk of 7 Nov 2026 found it.
    #[test]
    fn the_csp_frames_the_consoles_on_the_web_host() {
        let frame_src = CSP.split(';').map(str::trim).find(|d| d.starts_with("frame-src")).expect("frame-src");
        for (_, port) in console_ports() {
            let origin = format!("http://{WEB_HOST}:{port}");
            assert!(frame_src.split_whitespace().any(|s| s == origin), "{origin} missing from {frame_src}");
        }
    }

    /// A sign-in link is spent only on `rexenv.localhost` — on `localhost` it moves there, port kept, so
    /// the cookie is never set on the name every local dev server shares.
    #[test]
    fn sign_in_happens_only_on_the_web_host() {
        assert!(host_ok(Some("rexenv.localhost:9443")) && host_ok(Some("REXENV.localhost")));
        assert!(!host_ok(Some("rexenv.localhost.evil.example")) && !host_ok(Some("evil.localhost:9443")));
        assert!(is_web_host("rexenv.localhost:8000") && !is_web_host("localhost:9443") && !is_web_host("127.0.0.1:9443"));
        let ask = |host| Ask { method: "GET", host: Some(host), origin: None, has_token: true, session_csrf: None, csrf_header: None };
        assert_eq!(gate(&ask("rexenv.localhost:9443")), Gate::Exchange);
        assert_eq!(gate(&ask("localhost:9443")), Gate::MoveToWebHost);
        assert_eq!(gate(&ask("[::1]:9443")), Gate::MoveToWebHost);
        assert_eq!(on_web_host("localhost:8000", "/sites?t=ab"), "http://rexenv.localhost:8000/sites?t=ab");
        assert_eq!(on_web_host("[::1]:9443", "/?t=ab"), "http://rexenv.localhost:9443/?t=ab");
        assert_eq!(on_web_host("localhost", "/?t=ab"), "http://rexenv.localhost/?t=ab");
    }

    /// A sign-in code works for its ten minutes and not after; a session ends at 8 h idle or 24 h old,
    /// whichever comes first.
    #[test]
    fn sign_in_codes_and_sessions_expire() {
        use std::time::Duration as D;
        assert!(code_live(D::from_secs(0)) && code_live(D::from_secs(599)));
        assert!(!code_live(D::from_secs(600)) && !code_live(D::from_secs(86_400)));
        assert!(session_live(D::from_secs(60), D::from_secs(0)));
        assert!(session_live(D::from_secs(23 * 3600), D::from_secs(7 * 3600)));
        assert!(!session_live(D::from_secs(9 * 3600), D::from_secs(8 * 3600)), "idle limit");
        assert!(!session_live(D::from_secs(24 * 3600), D::from_secs(0)), "absolute limit, even in use");
    }

    /// Sign-out is a POST behind the gate (a cross-site page cannot sign the developer out), and its
    /// reply clears the cookie in the browser.
    #[test]
    fn sign_out_is_a_gated_post_that_clears_the_cookie() {
        assert_eq!(route("POST", "/signout", &[]), Route::SignOut);
        assert_eq!(route("GET", "/signout", &[]), Route::Refused(405, "wrong method"));
        let no_csrf = Ask { method: "POST", host: Some("localhost:9443"), origin: Some("http://localhost:9443"), has_token: false, session_csrf: Some("abc"), csrf_header: None };
        assert!(matches!(gate(&no_csrf), Gate::Refuse(403, _)));
        let cleared = session_cookie_cleared();
        assert!(cleared.starts_with(&format!("{SESSION_COOKIE}=;")) && cleared.contains("Max-Age=0") && cleared.contains("HttpOnly"), "{cleared}");
    }

    /// The server mints a code per `rex web url` and spends it at the exchange: no permanent token is
    /// read or written any more (TEXT — the server half is not unit-testable without a socket).
    #[test]
    fn the_server_mints_single_use_codes_and_keeps_no_permanent_token() {
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/web_server.rs")).expect("web_server.rs");
        let src = crate::core::copy_scan::production_source(&src);
        assert!(!src.contains("write_private"), "a sign-in secret written to disk again");
        let exchange = &src[src.find("fn exchange(").expect("exchange")..];
        let exchange = &exchange[..exchange.find("\n}\n").expect("its end")];
        assert!(exchange.contains("take_code("), "the exchange must SPEND the code:\n{exchange}");
        assert!(src.contains("rules::code_live(") && src.contains("rules::session_live("), "the limits must be checked");
    }

    const NAMES: &[&str] = &["list_sites", "app_info"];

    #[test]
    fn only_a_registered_command_is_dispatched_and_only_by_post() {
        assert_eq!(route("POST", "/ipc/list_sites", NAMES), Route::Ipc("list_sites"));
        assert_eq!(route("GET", "/ipc/list_sites", NAMES), Route::Refused(405, "commands are POSTed"));
        for other in ["/ipc/plugin:dialog|open", "/ipc/delete_everything", "/ipc/", "/ipc/list_sites/x", "/ipc/List_sites"] {
            assert!(matches!(route("POST", other, NAMES), Route::Refused(..)), "{other}");
        }
    }

    #[test]
    fn the_registered_list_is_the_one_the_desktop_handler_is_built_from() {
        let names = crate::command_names();
        assert!(names.len() > 250, "{}", names.len());
        assert!(names.contains(&"list_sites") && names.contains(&"app_info"));
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "a command registered twice");
        // The handler is built from the same macro, not from a second list.
        let lib = include_str!("lib.rs");
        assert!(lib.contains(".invoke_handler(for_each_command!(app_invoke_handler))"));
        assert_eq!(lib.matches("tauri::generate_handler![").count(), 1, "one generate_handler!, inside the macro");
    }

    /// Every command name a TS wrapper invokes — `invoke("x"` / `invoke<T>("x"` with the generic
    /// spanning lines — read from the ipc module the UI must go through.
    fn frontend_invokes() -> Vec<String> {
        let src = include_str!("../../src/lib/ipc/index.ts");
        let mut out = Vec::new();
        let mut rest = src;
        while let Some(i) = rest.find("invoke") {
            let after = &rest[i + "invoke".len()..];
            rest = after;
            let mut t = after;
            if t.starts_with('<') {
                let mut depth = 0usize;
                let mut end = None;
                for (j, c) in t.char_indices() {
                    match c {
                        '<' => depth += 1,
                        '>' => {
                            depth -= 1;
                            if depth == 0 {
                                end = Some(j + 1);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                let Some(e) = end else { continue };
                t = &t[e..];
            }
            let Some(t) = t.strip_prefix('(') else { continue };
            let t = t.trim_start();
            let Some(t) = t.strip_prefix('"') else { continue };
            let Some(q) = t.find('"') else { continue };
            out.push(t[..q].to_string());
        }
        out
    }

    /// **The web UI and the window reach the same commands** (D2, ledger #929). The handler and
    /// the web's table are one macro list, so they cannot differ; what CAN drift is the frontend
    /// against that list — a wrapper invoking a name nobody registered works in neither, and
    /// silently. So: every name the UI invokes is registered, the web routes by that same list
    /// (no second table in `web_server.rs`), and the scan itself is checked against a count.
    #[test]
    fn every_command_the_ui_invokes_is_on_the_one_list_the_web_routes_by() {
        let ui = frontend_invokes();
        assert!(ui.len() > 250, "only {} invokes parsed — the scan is broken", ui.len());
        let names = crate::command_names();
        let missing: Vec<&String> = ui.iter().filter(|n| !names.contains(&n.as_str())).collect();
        assert!(missing.is_empty(), "the UI invokes commands no list registers: {missing:?}");
        for n in &ui {
            assert_eq!(route("POST", &format!("/ipc/{n}"), &names), Route::Ipc(n), "{n} must route on the web");
        }
        let server = include_str!("web_server.rs");
        assert!(server.contains("let names = crate::command_names();"), "the web must route by the one list");
        assert!(!server.contains("generate_handler") && !server.contains("\"list_sites\""), "no second table");
    }

    #[test]
    fn a_path_can_only_name_a_built_file_or_the_app() {
        assert_eq!(route("GET", "/", NAMES), Route::Index);
        assert_eq!(route("GET", "/sites/abc", NAMES), Route::Index);
        assert_eq!(route("GET", "/assets/index-Tza5dfOO.js", NAMES), Route::Static("assets/index-Tza5dfOO.js"));
        for bad in [
            "/../../etc/passwd",
            "/assets/../../../etc/passwd",
            "/assets/%2e%2e/x.js",
            "/.env",
            "/assets/.hidden.js",
            "/assets//x.js",
            "/assets\\x.js",
            "relative.js",
            "/a/../b",
        ] {
            assert!(matches!(route("GET", bad, NAMES), Route::Refused(..)), "{bad}");
        }
        assert!(matches!(route("POST", "/", NAMES), Route::Refused(405, _)));
    }

    #[test]
    fn the_served_page_says_web_and_keeps_its_policy_strict() {
        let html = "<!doctype html>\n<html><head>\n<title>rexenv</title></head><body></body></html>";
        let marked = mark_index(html, "ab12").unwrap();
        assert!(marked.find(TRANSPORT_META).unwrap() < marked.find("<title>").unwrap());
        assert!(marked.contains(r#"<meta name="rexenv-csrf" content="ab12" />"#));
        assert_eq!(mark_index("<html><body/></html>", "ab12"), None);
        assert_eq!(mark_index(html, "\"><script>"), None, "the value goes into an attribute");
        assert_eq!(mark_index(html, ""), None);
        assert!(!CSP.contains("unsafe-eval") && !CSP.contains("script-src 'self' 'unsafe-inline'"));
        assert!(CSP.contains("frame-ancestors 'none'") && CSP.contains("connect-src 'self';"));
        // The frontend reads the same marker.
        let ts = include_str!("../../src/lib/ipc/transport.ts");
        assert!(ts.contains(r#"meta[name="rexenv-transport"]"#) && TRANSPORT_META.contains(r#"name="rexenv-transport" content="web""#));
    }

    #[test]
    fn an_event_name_tauri_would_panic_on_is_refused_first() {
        for ok in ["service-health", "db-import://state/3f2a", "terminal://output/7", "tray://route"] {
            assert!(valid_event_name(ok), "{ok}");
        }
        for bad in ["", "a b", "x\n", "é", "a;b", &"a".repeat(257)] {
            assert!(!valid_event_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn an_event_frame_cannot_be_cut_short() {
        let f = sse_event(3, "db-import://state/1", "{\"a\":\"x\"}");
        assert_eq!(f, "event: message\ndata: {\"id\":3,\"event\":\"db-import://state/1\",\"payload\":{\"a\":\"x\"}}\n\n");
        let pretty = sse_event(1, "x", "{\n  \"a\": 1\n}");
        assert!(pretty.ends_with("\n\n") && pretty.matches("\n\n").count() == 1, "{pretty:?}");
        assert_eq!(pretty.lines().filter(|l| l.starts_with("data: ")).count(), 3);
    }

    fn ask<'a>(method: &'a str, host: Option<&'a str>, origin: Option<&'a str>) -> Ask<'a> {
        Ask { method, host, origin, has_token: false, session_csrf: Some("c0ffee"), csrf_header: Some("c0ffee") }
    }

    /// Ledger #930 — each check refuses on its own.
    #[test]
    fn the_host_must_be_loopback_whatever_port_the_tunnel_used() {
        for ok in ["localhost:9443", "localhost:8000", "LOCALHOST", "127.0.0.1:9443", "[::1]:9443", "[::1]"] {
            assert!(host_ok(Some(ok)), "{ok}");
        }
        for bad in ["evil.example:9443", "localhost.evil.example", "127.0.0.1.nip.io:9443", "localhost:", "localhost:x", "[::2]:9443", "0.0.0.0:9443", ""] {
            assert!(!host_ok(Some(bad)), "{bad}");
        }
        assert!(!host_ok(None));
        assert_eq!(gate(&ask("GET", Some("rebind.example:9443"), None)), Gate::Refuse(403, "rexenv's web UI answers only to localhost"));
    }

    #[test]
    fn a_request_without_a_session_gets_only_the_sentence() {
        let mut a = ask("GET", Some("localhost:9443"), None);
        a.session_csrf = None;
        assert_eq!(gate(&a), Gate::Refuse(401, NO_SESSION));
        a.method = "POST";
        a.origin = Some("http://localhost:9443");
        assert_eq!(gate(&a), Gate::Refuse(401, NO_SESSION));
        // The token in the URL is the way in, and only by GET.
        a.method = "GET";
        a.has_token = true;
        // On `localhost` it is moved to the web host first (#952); there it is spent.
        assert_eq!(gate(&a), Gate::MoveToWebHost);
        a.host = Some("rexenv.localhost:9443");
        a.origin = None;
        assert_eq!(gate(&a), Gate::Exchange);
        a.method = "POST";
        a.origin = Some("http://rexenv.localhost:9443");
        assert_eq!(gate(&a), Gate::Refuse(401, NO_SESSION));
        assert!(NO_SESSION.contains("rex web url"));
    }

    #[test]
    fn a_post_needs_our_origin_and_the_sessions_csrf_header() {
        assert_eq!(gate(&ask("POST", Some("localhost:9443"), Some("http://localhost:9443"))), Gate::Pass);
        assert!(matches!(gate(&ask("POST", Some("localhost:9443"), Some("http://evil.example"))), Gate::Refuse(403, _)));
        assert!(matches!(gate(&ask("POST", Some("localhost:9443"), Some("http://localhost:8000"))), Gate::Refuse(403, _)));
        assert!(matches!(gate(&ask("POST", Some("localhost:9443"), None)), Gate::Refuse(403, _)));
        let mut a = ask("POST", Some("localhost:9443"), Some("http://localhost:9443"));
        a.csrf_header = None;
        assert!(matches!(gate(&a), Gate::Refuse(403, _)));
        a.csrf_header = Some("c0ffef");
        assert!(matches!(gate(&a), Gate::Refuse(403, _)));
        // A cross-site GET with its Origin said is refused too; a plain navigation (no Origin) passes.
        assert!(matches!(gate(&ask("GET", Some("localhost:9443"), Some("http://evil.example"))), Gate::Refuse(403, _)));
        assert_eq!(gate(&ask("GET", Some("localhost:9443"), None)), Gate::Pass);
    }

    #[test]
    fn the_cookie_the_token_and_the_comparison() {
        assert_eq!(cookie_value("a=1; rexenv_web=abc; b=2", SESSION_COOKIE), Some("abc"));
        assert_eq!(cookie_value("xrexenv_web=abc", SESSION_COOKIE), None);
        assert_eq!(token_in_query(Some("x=1&t=deadbeef")), Some("deadbeef"));
        assert_eq!(token_in_query(Some("tt=1")), None);
        assert!(same_secret("abc", "abc") && !same_secret("abc", "abd") && !same_secret("abc", "ab"));
        assert_eq!(after_sign_in("/sites/abc"), "/sites/abc");
        for odd in ["//evil.example/", "//intranet", "/a\r\nSet-Cookie: x", "/a?b", "", "http://x"] {
            assert_eq!(after_sign_in(odd), "/", "{odd:?}");
        }
        let c = session_cookie("s1");
        assert!(c.contains("HttpOnly") && c.contains("SameSite=Strict") && c.contains("Path=/"));
        // The server runs the gate before it routes anything, and the frontend sends the header.
        let server = include_str!("web_server.rs");
        let handle = &server[server.find("async fn handle(").expect("handle")..];
        assert!(handle.find("rules::gate(&").expect("the gate") < handle.find("rules::route(").expect("the router"), "the gate must come first");
        let ts = include_str!("../../src/lib/ipc/transport.ts");
        assert!(ts.contains(r#"meta[name="rexenv-csrf"]"#) && ts.contains(r#""x-rexenv-csrf""#));
    }

    /// Ledger #933.
    #[test]
    fn a_console_needs_the_session_and_its_own_origin_but_no_csrf_header() {
        let mut a = ask("POST", Some("localhost:9444"), Some("http://localhost:9444"));
        a.csrf_header = None;
        assert_eq!(console_gate(&a), Gate::Pass, "a form in the frame cannot add our header");
        a.origin = Some("http://localhost:9443");
        assert!(matches!(console_gate(&a), Gate::Refuse(403, _)), "the app page is not the console");
        a.origin = Some("http://localhost:9445");
        assert!(matches!(console_gate(&a), Gate::Refuse(403, _)), "one console is not the other");
        a.origin = None;
        assert!(matches!(console_gate(&a), Gate::Refuse(403, _)));
        let mut g = ask("GET", Some("localhost:9444"), None);
        g.session_csrf = None;
        assert_eq!(console_gate(&g), Gate::Refuse(401, NO_SESSION));
        g.has_token = true;
        assert_eq!(console_gate(&g), Gate::Refuse(401, NO_SESSION), "no sign-in on a console port");
        assert!(matches!(console_gate(&ask("GET", Some("evil.example:9444"), None)), Gate::Refuse(403, _)));
    }

    #[test]
    fn only_the_app_page_may_frame_a_console() {
        assert_eq!(app_origin_for("localhost:9444"), "http://localhost:9443");
        assert_eq!(app_origin_for("127.0.0.1:9445"), "http://127.0.0.1:9443");
        assert_eq!(app_origin_for("[::1]:9444"), "http://[::1]:9443");
        let csp = console_csp("default-src 'self'; script-src 'nonce-abc'; frame-ancestors rexdb://localhost", "http://localhost:9443");
        assert_eq!(csp, "default-src 'self'; script-src 'nonce-abc'; frame-ancestors http://localhost:9443");
        assert_eq!(console_csp("", "http://localhost:9443"), "frame-ancestors http://localhost:9443");
        // The page's own policy lets it frame exactly the console ports, and the page learns them.
        for (_, port) in console_ports() {
            for host in ["localhost", "127.0.0.1"] {
                assert!(CSP.contains(&format!("http://{host}:{port}")), "{host}:{port}");
            }
        }
        // Not `[::1]`: Chromium rejects an IPv6 literal as a CSP source ("invalid source", seen in
        // the container run) — over `[::1]` the app works and the consoles do not frame.
        assert!(!CSP.contains("[::1]"));
        {
        }
        assert_eq!(consoles_meta(), r#"<meta name="rexenv-consoles" content="adminer=9444,phpmyadmin=9445" />"#);
        let ts = include_str!("../../src/lib/dbconsole.ts");
        assert!(ts.contains(r#"meta[name="rexenv-consoles"]"#));
        // The server gates console requests with the console gate, before forwarding.
        let server = include_str!("web_server.rs");
        let h = &server[server.find("async fn handle_console(").expect("the console handler")..];
        assert!(h.find("rules::console_gate(&").unwrap() < h.find("dbconsole_proxy::forward(").unwrap());
    }

    #[test]
    fn the_web_ui_binds_loopback_on_d5s_port() {
        assert_eq!(WEB_PORT, 9443);
        assert_eq!(WEB_BIND, [127, 0, 0, 1]);
    }

    /// Ledger #935 — **the web UI holds every port on both loopbacks, or serves nothing.** The
    /// three ports are one list; a `[::1]` bind that fails because the port is HELD stops it, and
    /// only a host with no IPv6 loopback at all is let through on 127.0.0.1 alone.
    #[test]
    fn the_web_ui_holds_all_its_ports_on_both_loopbacks_or_none() {
        let ports: Vec<u16> = web_ports().iter().map(|(p, _)| *p).collect();
        assert_eq!(ports, [WEB_PORT, ADMINER_WEB_PORT, PHPMYADMIN_WEB_PORT]);
        for (_, p) in console_ports() {
            assert!(ports.contains(&p), "console port {p} must be held with the rest");
        }
        assert_eq!(std::net::Ipv6Addr::from(WEB_BIND_V6), std::net::Ipv6Addr::LOCALHOST);
        use std::io::ErrorKind::*;
        for fatal in [AddrInUse, PermissionDenied, Other] {
            assert!(v6_bind_failure_is_fatal(fatal), "{fatal:?} on [::1] must stop the web UI");
        }
        for tolerated in [AddrNotAvailable, Unsupported] {
            assert!(!v6_bind_failure_is_fatal(tolerated), "{tolerated:?}: no IPv6 loopback to squat");
        }
        // The server binds from these lists and stops on the first failure — never `continue`.
        let server = include_str!("web_server.rs");
        let spawn = &server[server.find("pub fn spawn_if_requested").expect("spawn_if_requested")..];
        let spawn = &spawn[..spawn.find("\nfn ").expect("its end")];
        assert!(spawn.contains("rules::web_ports()") && spawn.contains("rules::WEB_BIND_V6"), "binds from the lists");
        let held = spawn.find("could not hold {addr}: {e}\");").expect("the bind failure arm");
        assert!(spawn[held..].trim_start_matches(|c| c != '\n').trim_start().starts_with("return;"), "a port that cannot be held stops the web UI");
        assert!(!spawn.contains("is not served"), "no console is skipped while the rest serve");
        assert!(spawn.contains(".header_read_timeout(rules::HEADER_READ_TIMEOUT)") && spawn.contains(".timer("), "header timeout with a timer");
        assert!(spawn.contains("try_acquire_owned()"), "the connection cap");
    }

    /// The web UI needs no native webview (T5, D8's second step, measured on the Dell 6 Nov 2026: in
    /// Windows session 0, zero `msedgewebview2.exe`, `/ipc/list_sites` answered and an event crossed
    /// `/events`). `dispatch` asks the webview its URL and, when there is no native webview to answer,
    /// uses the app's local origin — `on_message` reads the URL only to class the call as the app's own.
    /// TEXT (the #175 bound): the fallback stays, and it is the platform's origin, not one OS's literal.
    #[test]
    fn the_web_ui_dispatches_without_a_native_webview() {
        let src = include_str!("web_server.rs");
        let body = &src[src.find("async fn dispatch(").expect("dispatch")..];
        let body = &body[..body.find("\n}\n").expect("its end")];
        let ask = body.find("match webview.url() {").expect("the URL is asked first");
        let fallback = body.find("crate::platform::words::current().app_origin").expect("the local-origin fallback");
        assert!(ask < fallback, "{body}");
        assert!(!body.contains("tauri.localhost") && !body.contains("tauri://"), "the origin is the platform's (words.rs), not inline");
        assert!(body.find("webview.on_message(").expect("on_message") > fallback, "{body}");
    }
}

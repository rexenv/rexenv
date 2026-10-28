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

/// The largest `/ipc` body read. Commands carry paths and form values, never file contents.
pub const MAX_IPC_BODY: usize = 4 * 1024 * 1024;

/// The marker `index.html` gets when the web server serves it: the frontend's `transport()` reads
/// it and talks `fetch`/SSE instead of Tauri's bridge. A `<meta>` and not a script — the page's
/// CSP allows no inline script, and must not start to.
pub const TRANSPORT_META: &str = r#"<meta name="rexenv-transport" content="web" />"#;

/// The page's policy in a browser. The desktop's (tauri.conf.json) minus the IPC and custom-scheme
/// origins a browser does not have; `frame-ancestors 'none'` because nothing should frame the UI.
pub const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
img-src 'self' data: https://*.w.org; font-src 'self' data:; connect-src 'self'; frame-src 'self'; \
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
/// The URL parameter carrying the token (`rex web url`).
pub const TOKEN_PARAM: &str = "t";
/// The token's file under the config dir, written owner-only.
pub const TOKEN_FILE: &str = "web-token";

/// What a request without a session is told.
pub const NO_SESSION: &str = "rexenv's web UI needs its sign-in link. On the server, run `rex web url` \
and open the address it prints (through your SSH tunnel).";

/// Whether `host` (the `Host` header) names this machine's loopback: `localhost`, `127.0.0.1` or
/// `[::1]`, any port — an `ssh -L 8000:localhost:9443` tunnel arrives as `localhost:8000`. A DNS
/// rebinding page arrives with ITS name, which is the attack this refuses.
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
    name.eq_ignore_ascii_case("localhost") || name == "127.0.0.1"
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

/// What the gate decides for one request.
#[derive(Debug, PartialEq, Eq)]
pub enum Gate {
    /// Through, as this session.
    Pass,
    /// A token in the URL: check it, start a session, send the browser to the same path without it.
    Exchange,
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
        return Gate::Exchange;
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
        (_, "/events" | "/events/listen" | "/events/unlisten") => return Route::Refused(405, "wrong method"),
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

/// `index.html` with the transport marker and this session's CSRF value, placed first in
/// `<head>` so both exist before any module runs. `None` when the document has no `<head>` (not
/// ours) or the value is not plain hex (it is written into an attribute).
pub fn mark_index(html: &str, csrf: &str) -> Option<String> {
    if csrf.is_empty() || !csrf.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let at = html.find("<head>")? + "<head>".len();
    Some(format!(
        "{}\n    {TRANSPORT_META}\n    <meta name=\"{CSRF_META}\" content=\"{csrf}\" />{}",
        &html[..at],
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
        assert_eq!(gate(&a), Gate::Exchange);
        a.method = "POST";
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

    #[test]
    fn the_web_ui_binds_loopback_on_d5s_port() {
        assert_eq!(WEB_PORT, 9443);
        assert_eq!(WEB_BIND, [127, 0, 0, 1]);
    }
}

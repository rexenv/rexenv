//! core::dbconsole_proxy — the custom-scheme proxy the in-app Database Browser
//! loads a console through, one origin per console.
//!
//! WebKit withholds third-party cookies inside a cross-site `<iframe>` (ITP), so a
//! console framed straight from its https vhost lost its session cookie on the
//! login POST and bounced back to the form, every time. The frame therefore loads
//! `rexdb://localhost/…` (Adminer) or `rexpma://localhost/…` (phpMyAdmin) — on
//! WebView2 `http://rexdb.localhost/`, `http://rexpma.localhost/`, the platform's
//! spelling (`PlatformWords`) — and THIS module forwards each request to the
//! console's vhost over loopback with a RUST-SIDE cookie jar, so browser cookie
//! policy is out of the picture entirely (ledger #38, #703).
//!
//! Two consoles, two origins, two jars. phpMyAdmin's pages use absolute paths
//! (`/index.php`, `/js/…`, `/themes/…`), so it cannot live under a path prefix of
//! Adminer's origin; and two `PHPSESSID`s on two hosts must never share a jar
//! (`docs/archive/PLAN-db-console-open-and-phpmyadmin.md` §3.3, ledger #870).

use crate::core::adminer;
use crate::core::phpmyadmin;
use crate::error::{Error, Result};

/// The consoles the proxy serves. `Copy` so the scheme handler's closure can
/// own one per registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Console {
    Adminer,
    PhpMyAdmin,
}

impl Console {
    /// Every console, for the scheme registrations in `lib.rs`.
    pub const ALL: [Console; 2] = [Console::Adminer, Console::PhpMyAdmin];

    /// The custom scheme the webview loads this console through. On WebView2 the
    /// same name is served at `http://<scheme>.localhost/` (`PlatformWords`).
    pub const fn scheme(self) -> &'static str {
        match self {
            Console::Adminer => "rexdb",
            Console::PhpMyAdmin => "rexpma",
        }
    }

    /// The internal vhost this console answers on, through the edge.
    pub const fn host(self) -> &'static str {
        match self {
            Console::Adminer => adminer::ADMINER_HOST,
            Console::PhpMyAdmin => phpmyadmin::PHPMYADMIN_HOST,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Console::Adminer => "adminer",
            Console::PhpMyAdmin => "phpmyadmin",
        }
    }
}

/// Build the upstream URL from a request's path+query. Guard: it MUST start with
/// `/`. Otherwise a value like `@evil.com/` would parse the host as `evil.com`
/// (with the vhost demoted to userinfo), and because the client pins ONLY the
/// vhost to loopback AND disables cert checks, that would be an SSRF to an
/// arbitrary host. Requests always arrive as `<scheme>://localhost/…`, so a legit
/// path always starts with `/` — a `//path` still resolves to the pinned host
/// (the authority ends at the first `/`). B8, defense-in-depth.
fn proxy_url(console: Console, path_and_query: &str) -> Result<reqwest::Url> {
    if !path_and_query.starts_with('/') {
        return Err(Error::Other(format!(
            "{} proxy: refusing non-absolute request path {path_and_query:?}",
            console.label()
        )));
    }
    reqwest::Url::parse(&format!("https://{}{path_and_query}", console.host()))
        .map_err(|e| Error::Other(format!("{} proxy: bad url: {e}", console.label())))
}

/// A response ready to hand back to the webview's custom-scheme responder.
pub struct ProxiedResponse {
    pub status: u16,
    /// Pass-through headers (hop-by-hop, cookie, framing and length headers already handled).
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

fn build_client(console: Console, edge_https_port: u16) -> reqwest::Client {
    reqwest::Client::builder()
        .cookie_store(true)
        // Loopback-pinned; our local-CA leaf won't chain for reqwest's store
        // (same trade-off as `proxy::edge_answers_as_ours`).
        .danger_accept_invalid_certs(true)
        .resolve(
            console.host(),
            // The edge's port as the user set it (`core::edge_ports`). The URL carries none, so this
            // one is used — and the Host header stays port-less, which keeps `forward`'s Location
            // rewrite exact.
            std::net::SocketAddr::from(([127, 0, 0, 1], edge_https_port)),
        )
        // Redirect policy `none` so [`forward`] resolves POST-redirect-GET itself —
        // WKWebView does NOT follow redirects returned by a custom-scheme handler.
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .expect("db console proxy client")
}

/// One client — and so ONE cookie jar — per console AND edge port, living for the app run so
/// a session persists across iframe remounts. Never shared between the two consoles: a
/// jar is keyed by host, but a bug that let one console's login land in the
/// other's jar would be invisible until it was a security report. Keyed by the port too since the
/// edge's port became a setting (docs/PLAN-headless-web.md T2): a client pinned to the old port
/// would keep dialling it after the change. Each is leaked once to stay `'static`: one per console
/// per port the edge has ever had in this run, so the leak is bounded by how often a user changes it.
fn client(console: Console, edge_https_port: u16) -> &'static reqwest::Client {
    type Clients = std::sync::Mutex<Vec<((Console, u16), &'static reqwest::Client)>>;
    static CLIENTS: std::sync::OnceLock<Clients> = std::sync::OnceLock::new();
    let mut clients = CLIENTS.get_or_init(Default::default).lock().unwrap_or_else(|p| p.into_inner());
    if let Some((_, c)) = clients.iter().find(|(k, _)| *k == (console, edge_https_port)) {
        return c;
    }
    let c: &'static reqwest::Client = Box::leak(Box::new(build_client(console, edge_https_port)));
    clients.push(((console, edge_https_port), c));
    c
}

/// Response headers that must NOT be replayed to the webview: cookies stay in the
/// Rust jar (the whole point); length/hop-by-hop headers are rebuilt by the
/// responder; and `X-Frame-Options` — Adminer's wrapper removes it at the source,
/// phpMyAdmin sends `DENY` and has no wrapper. Dropping it on the replayed copy
/// widens nothing: this copy is served from an origin only rexenv's own scheme
/// handler answers, so no browser can load it, whatever it says about framing
/// (the same argument as `adminer::PROXY_FRAME_ANCESTORS`). The DIRECT https
/// vhost keeps its `DENY` (ledger #865, #869).
const STRIPPED_RESPONSE_HEADERS: &[&str] = &[
    "set-cookie",
    "content-length",
    "transfer-encoding",
    "connection",
    "keep-alive",
    "alt-svc",
    "x-frame-options",
];

/// The headers the webview gets, from the headers the vhost sent — a pure
/// function so the three rewrites are testable without a server: the strip list
/// above, the CSP's `frame-ancestors` swapped for the app's own origins (every
/// other directive — the script nonce above all — verbatim), and an absolute
/// self-`Location` put back onto the proxy origin (relative ones already resolve
/// against `<scheme>://localhost`).
pub(crate) fn replay_headers<'a>(
    console: Console,
    upstream: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Vec<(String, String)> {
    let mut headers = Vec::new();
    for (name, v) in upstream {
        let n = name.to_ascii_lowercase();
        if STRIPPED_RESPONSE_HEADERS.contains(&n.as_str()) {
            continue;
        }
        if n == "content-security-policy" {
            headers.push((n, adminer::rewrite_frame_ancestors(v)));
            continue;
        }
        if n == "location" {
            let v = v.strip_prefix(&format!("https://{}", console.host())).unwrap_or(v);
            headers.push((n, v.to_string()));
            continue;
        }
        headers.push((n, v.to_string()));
    }
    headers
}

/// Forward one Database Browser request (`<scheme>://localhost<path_and_query>`)
/// to the console's vhost through the edge and return the response for the
/// webview. `content_type` is the request's Content-Type (form POSTs).
///
/// Redirects are followed HERE (≤5 hops, same-vhost only): both consoles are
/// POST-redirect-GET, and WKWebView never follows a redirect returned by a
/// custom-scheme handler — replaying the 302 leaves the iframe on a dead blank
/// frame (observed live as "Save does nothing" / blank first open). Per browser
/// form semantics 301/302/303 become a body-less GET; 307/308 keep the method +
/// body.
pub async fn forward(
    console: Console,
    edge_https_port: u16,
    method: &str,
    path_and_query: &str,
    content_type: Option<&str>,
    body: Vec<u8>,
) -> Result<ProxiedResponse> {
    let mut url = proxy_url(console, path_and_query)?;
    let mut method = reqwest::Method::from_bytes(method.as_bytes())
        .map_err(|e| Error::Other(format!("{} proxy: bad method: {e}", console.label())))?;
    let mut content_type = content_type.map(str::to_string);
    let mut body = body;

    let mut hops = 0u8;
    let resp = loop {
        let mut req = client(console, edge_https_port).request(method.clone(), url.clone());
        if let Some(ct) = &content_type {
            req = req.header(reqwest::header::CONTENT_TYPE, ct);
        }
        if !body.is_empty() {
            req = req.body(body.clone());
        }
        let resp = req
            .send()
            .await
            .map_err(|e| Error::Other(format!("{} proxy: {e}", console.label())))?;

        if resp.status().is_redirection() && hops < 5 {
            let target = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|loc| url.join(loc).ok())
                // Never follow OFF the vhost — anything else replays to the
                // webview below (where frame policy applies).
                .filter(|next| next.host_str() == Some(console.host()));
            if let Some(next) = target {
                hops += 1;
                url = next;
                let s = resp.status();
                if s != reqwest::StatusCode::TEMPORARY_REDIRECT
                    && s != reqwest::StatusCode::PERMANENT_REDIRECT
                {
                    method = reqwest::Method::GET;
                    body = Vec::new();
                    content_type = None;
                }
                continue;
            }
        }
        break resp;
    };

    let status = resp.status().as_u16();
    let upstream: Vec<(String, String)> = resp
        .headers()
        .iter()
        .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.as_str().to_string(), v.to_string())))
        .collect();
    let headers = replay_headers(console, upstream.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    let body = resp
        .bytes()
        .await
        .map_err(|e| Error::Other(format!("{} proxy: read body: {e}", console.label())))?
        .to_vec();
    Ok(ProxiedResponse { status, headers, body })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_console_has_its_own_scheme_and_host() {
        assert_eq!(Console::Adminer.scheme(), "rexdb");
        assert_eq!(Console::PhpMyAdmin.scheme(), "rexpma");
        assert_eq!(Console::Adminer.host(), "adminer.rexenv.rex");
        assert_eq!(Console::PhpMyAdmin.host(), "phpmyadmin.rexenv.rex");
        assert_eq!(Console::ALL.len(), 2);
        assert_eq!(adminer::PROXY_SCHEME, Console::Adminer.scheme());
    }

    /// Legit paths always arrive as `<scheme>://localhost/…` → the host stays
    /// pinned for BOTH consoles; `@`, bare hosts and `*` are refused (B8).
    #[test]
    fn proxy_url_requires_an_absolute_path_and_pins_the_host() {
        for c in Console::ALL {
            assert_eq!(proxy_url(c, "/index.php?server=1").unwrap().host_str(), Some(c.host()));
            assert_eq!(proxy_url(c, "/").unwrap().host_str(), Some(c.host()));
            // `//evil.com/x` — the authority ends at the first `/`; still pinned.
            assert_eq!(proxy_url(c, "//evil.com/x").unwrap().host_str(), Some(c.host()));
            assert!(proxy_url(c, "@evil.com/").is_err());
            assert!(proxy_url(c, "evil.com").is_err());
            assert!(proxy_url(c, "*").is_err());
        }
    }

    /// Two consoles, two jars: the clients are distinct objects, each pinned to
    /// its own host (ledger #870). A shared jar would let one console's session
    /// cookie ride along to the other's host.
    #[test]
    fn the_two_consoles_never_share_a_client_or_a_jar() {
        let a = client(Console::Adminer, 443) as *const reqwest::Client;
        let p = client(Console::PhpMyAdmin, 443) as *const reqwest::Client;
        assert_ne!(a, p, "one client — one jar — for both consoles");
        assert_eq!(client(Console::Adminer, 443) as *const _, a, "stable per console and port");
        assert_ne!(client(Console::Adminer, 8443) as *const _, a, "a new edge port is a new client");
    }

    /// The replayed copy: cookies and `X-Frame-Options` gone, the CSP's
    /// `frame-ancestors` swapped for the app's origins with the nonce intact, an
    /// absolute self-Location put back on the proxy origin, everything else as
    /// sent. phpMyAdmin sends `DENY` and a CSP with NO `frame-ancestors`
    /// (measured 14 Oct 2026) — the replay must get one, never be MORE frameable.
    #[test]
    fn replay_strips_cookies_and_xfo_and_rewrites_csp_and_location() {
        let phpmyadmin = [
            ("Set-Cookie", "phpMyAdmin=abc; path=/; HttpOnly; SameSite=Strict"),
            ("X-Frame-Options", "DENY"),
            ("Content-Security-Policy", "default-src 'self' ;script-src 'self' 'unsafe-inline' 'unsafe-eval' ;object-src 'none';"),
            ("X-Content-Type-Options", "nosniff"),
            ("Content-Length", "1234"),
            ("Location", "https://phpmyadmin.rexenv.rex/index.php?route=/"),
        ];
        let out = replay_headers(Console::PhpMyAdmin, phpmyadmin);
        let names: Vec<&str> = out.iter().map(|(n, _)| n.as_str()).collect();
        assert!(!names.contains(&"set-cookie"), "{names:?}");
        assert!(!names.contains(&"x-frame-options"), "{names:?}");
        assert!(!names.contains(&"content-length"), "{names:?}");
        assert!(names.contains(&"x-content-type-options"));
        let csp = &out.iter().find(|(n, _)| n == "content-security-policy").unwrap().1;
        assert!(csp.contains("frame-ancestors tauri://localhost http://tauri.localhost https://tauri.localhost"), "{csp}");
        assert!(csp.contains("script-src 'self' 'unsafe-inline' 'unsafe-eval'"), "{csp}");
        assert_eq!(out.iter().find(|(n, _)| n == "location").unwrap().1, "/index.php?route=/");

        // Adminer's Location rewrite keys on ITS host — the other console's is left alone.
        let out = replay_headers(Console::Adminer, [("Location", "https://adminer.rexenv.rex/?db=x"), ("Content-Security-Policy", "script-src 'nonce-A'; frame-ancestors tauri://localhost")]);
        assert_eq!(out.iter().find(|(n, _)| n == "location").unwrap().1, "/?db=x");
        let csp = &out.iter().find(|(n, _)| n == "content-security-policy").unwrap().1;
        assert!(csp.contains("'nonce-A'") && csp.matches("frame-ancestors").count() == 1, "{csp}");
        let out = replay_headers(Console::Adminer, [("Location", "https://phpmyadmin.rexenv.rex/x")]);
        assert_eq!(out[0].1, "https://phpmyadmin.rexenv.rex/x");
    }
}

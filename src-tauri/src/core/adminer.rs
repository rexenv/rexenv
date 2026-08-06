//! core::adminer — serve the bundled Adminer DB browser as an internal vhost (§5.2).
//!
//! Adminer is a single `adminer.php` (§5.1). We serve it through the shared stack
//! on a fixed internal host (`adminer.rexenv.rex`) rooted at an isolated docroot,
//! behind the edge (TLS, local CA). It is deliberately NOT a `Site` in the DB —
//! so it can never be selected as a public tunnel origin (§9): tunnels are scoped
//! to a single site's Host, and this internal vhost isn't one.

use crate::core::binaries;
use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use std::path::PathBuf;

/// Custom webview scheme the in-app Database Browser loads Adminer through
/// (`rexdb://localhost/…`). The scheme handler proxies to the stack over
/// loopback with a RUST-SIDE cookie jar ([`forward`]) because WebKit withholds
/// third-party cookies inside a cross-site `<iframe>` (ITP): the session cookie
/// never survived the login POST, so every login — auto or manual — bounced
/// straight back to the form. With the jar in Rust, browser cookie policy is
/// out of the picture entirely. (Windows webviews expect `http://rexdb.localhost/`
/// instead — Phase 4.)
pub const PROXY_SCHEME: &str = "rexdb";

/// Internal host Adminer is served on (resolved by the embedded DNS (backbone `.rex` resolver)).
/// NEVER a public tunnel origin (§9) — it isn't a site.
pub const ADMINER_HOST: &str = "adminer.rexenv.rex";

/// Largest SQL dump the Adminer vhost accepts, in bytes — nginx's
/// `client_max_body_size` for the vhost AND the PHP upload/post limits it runs
/// with ([`IMPORT_PHP_VALUE`]), set together so nginx never 413s a body PHP
/// would take. FIXED, deliberately: Adminer does NOT track the default pool's
/// per-version PHP settings the way a site's block does (`php::nginx_body_limits`).
/// Those bound what a *site* accepts off the network; importing a big dump is
/// this vhost's whole job, and it's loopback-only (§5.2). Before this cap the
/// vhost inherited the 128m http-level default and 413'd every larger dump —
/// with no setting anywhere that could raise it.
pub const MAX_IMPORT_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Per-request `PHP_VALUE` ini lines for the Adminer vhost, mirroring
/// [`MAX_IMPORT_BYTES`]. php-fpm applies these over the pool's OVERRIDABLE
/// `php_value[…]` lines, so the shared default pool — and every site on it —
/// keeps the user's own upload limits.
pub const IMPORT_PHP_VALUE: &str = "upload_max_filesize=2048M\npost_max_size=2048M";

/// Query flag a rexenv deep-link sets to request a one-click scoped session (§11.4).
pub const AUTOLOGIN_FLAG: &str = "rexenv_auto";

/// The deep-link wrapper served as `index.php`. It customizes Adminer via the
/// `adminer_object()` hook (defined in the GLOBAL namespace — Adminer checks the
/// global function name) so a rexenv "Open database" link lands the user straight
/// in the site's DB instead of on a manual login screen:
///   - `login()` accepts a passwordless login, but ONLY for the loopback engines
///     rexenv manages (MySQL root / Postgres have no local password) — never a
///     remote host;
///   - `loginForm()` auto-submits Adminer's OWN rendered form (which already
///     carries the CSRF token + CSP nonce) when `?rexenv_auto` is set, guarded by
///     sessionStorage so a failed login can't loop;
///   - `headers()`/`csp()` replace Adminer's blanket `X-Frame-Options: deny` with
///     `frame-ancestors` scoped to the rexenv webview origins, so the in-app
///     Database Browser `<iframe>` renders while any OTHER site framing this
///     passwordless vhost (clickjacking from a page in the user's browser —
///     `adminer.rexenv.rex` resolves locally for them too) stays blocked;
///   - a `header_register_callback` rewrites every `Set-Cookie` to
///     `SameSite=None; Secure; Partitioned`: inside the app the iframe is a
///     cross-site embed (webview origin ≠ `adminer.rexenv.rex`), so Adminer's
///     default `SameSite=lax` session/key cookies are withheld from the login
///     POST and every login bounces back to the form. `Partitioned` (CHIPS)
///     keeps the embedded jar isolated per top-level site, and Adminer's own
///     CSRF token still guards every state-changing request.
///
/// SECURITY: Adminer here is an INTERNAL vhost (`adminer.rexenv.rex` → 127.0.0.1)
/// behind the local edge and is NEVER a public tunnel origin (§9), so passwordless
/// loopback access stays confined to the local machine.
///
/// `pub` so `examples/adminer_login_gate_check` can extract the loopback gate
/// (between the `rexenv-loopback-gate` markers) and prove it through real PHP.
pub const WRAPPER_INDEX_PHP: &str = r#"<?php
// rexenv Adminer deep-link wrapper (§11.4) — generated; do not edit by hand.

// The Database Browser embeds this vhost in a cross-site <iframe> (webview
// origin != adminer.rexenv.rex), where SameSite=lax cookies are withheld and
// the login POST loses its session. Rewrite every cookie to
// "SameSite=None; Secure; Partitioned" at flush time (covers the PHP session
// cookie and Adminer's own key/permanent cookies alike). CSRF stays covered by
// Adminer's per-session token; framing stays limited by frame-ancestors below.
header_register_callback(function () {
    $rewritten = array();
    foreach (headers_list() as $h) {
        if (stripos($h, 'Set-Cookie:') === 0) {
            $v = preg_replace('/;\s*SameSite=\w+/i', '', trim(substr($h, 11)));
            if (stripos($v, 'secure') === false) {
                $v .= '; Secure';
            }
            $rewritten[] = $v . '; SameSite=None; Partitioned';
        }
    }
    if ($rewritten) {
        header_remove('Set-Cookie');
        foreach ($rewritten as $v) {
            header('Set-Cookie: ' . $v, false);
        }
    }
});

// rexenv-loopback-gate:start
// Passwordless login is granted ONLY when the target server's HOST is EXACTLY a
// loopback name. \Adminer\SERVER is request-controlled (auth[server] / ?server= /
// ?pgsql=), so a PREFIX test (a starts-with match on the SERVER string) once
// accepted "127.0.0.1.evil.com" and handed a passwordless session to a REMOTE
// server — a malicious server can then use `LOAD DATA LOCAL INFILE` to read local
// files off this machine. Strip an optional :port (or MySQL :socket, or bracketed
// IPv6) and compare the bare host to the allow-list. See docs/archive/CODEBASE-REVIEW.md B1.
// (examples/adminer_login_gate_check runs this exact function through PHP.)
if (!function_exists('rexenv_is_loopback')) {
    function rexenv_is_loopback($server) {
        $server = (string) $server;
        if ($server === '') {
            return false;
        }
        if ($server[0] === '[') {
            // Bracketed IPv6 literal, optional ":port": [::1] or [::1]:3306.
            $end = strpos($server, ']');
            $host = $end === false ? '' : substr($server, 1, $end - 1);
        } elseif (substr_count($server, ':') === 1) {
            // host:port (or host:/socket for MySQL) — take the host.
            $host = substr($server, 0, strpos($server, ':'));
        } else {
            // Bare host, or a bare IPv6 literal (::1) which has more than one colon.
            $host = $server;
        }
        return in_array(strtolower($host), array('127.0.0.1', '::1', 'localhost'), true);
    }
}
// rexenv-loopback-gate:end

function adminer_object() {
    if (!class_exists('RexenvAdminer')) {
        class RexenvAdminer extends \Adminer\Adminer {
            function login($login, $password) {
                // Passwordless login for rexenv's loopback engines ONLY — the host
                // must match a loopback name EXACTLY (see the rexenv-loopback-gate
                // above). SERVER is request-controlled, so a prefix match would let
                // "127.0.0.1.evil.com" reach a remote server passwordless (B1).
                return rexenv_is_loopback(\Adminer\SERVER);
            }
            function loginForm() {
                parent::loginForm();
                if (isset($_GET['rexenv_auto'])) {
                    // Auto-submit once PER TARGET SERVER. Guards: never on a page
                    // already showing a login error (prevents a submit loop when
                    // the engine is down); sessionStorage is best-effort — it can
                    // throw on the app's custom-scheme origin, and a thrown guard
                    // must not kill the submit. The guard key MUST include the
                    // target server: the embedded webview is ONE permanent tab
                    // (one sessionStorage per origin), so a global one-shot let
                    // the first engine's auto-login permanently suppress every
                    // other engine's (observed live: embedded MariaDB never
                    // auto-logged-in after MySQL had). The param name is part of
                    // the key so MySQL-protocol (server=) and Postgres (pgsql=)
                    // targets can never collide.
                    $target = isset($_GET['pgsql'])
                        ? 'pgsql:' . $_GET['pgsql']
                        : 'server:' . (isset($_GET['server']) ? $_GET['server'] : '');
                    echo "<script" . \Adminer\nonce() . ">"
                       . "(function(){"
                       . "if(document.querySelector('.error'))return;"
                       . "var key=" . json_encode('rexenv_autologin:' . $target) . ";"
                       . "var seen=null;"
                       . "try{seen=sessionStorage.getItem(key);}catch(e){}"
                       . "if(seen)return;"
                       . "try{sessionStorage.setItem(key,'1');}catch(e){}"
                       . "var f=document.querySelector('[name=\"auth[driver]\"]');"
                       . "if(f&&f.form){f.form.submit();}"
                       . "})();"
                       . "</script>";
                }
            }
            function headers() {
                // Adminer hardcodes "X-Frame-Options: deny", which blanks the
                // rexenv Database Browser <iframe>. Drop it; the csp() hook below
                // re-adds frame protection scoped to the app's webview origins.
                header_remove('X-Frame-Options');
            }
            function csp(array $csp) {
                // Embeddable ONLY by the rexenv app webview (prod macOS/Linux +
                // prod Windows origins) — every other ancestor stays blocked. The
                // Vite dev origin is appended by Rust ONLY in debug builds
                // (__REXENV_DEV_ANCESTOR__), so a shipped Adminer isn't frameable
                // by a local process squatting :1420 (B14).
                $csp[0]['frame-ancestors'] =
                    'tauri://localhost https://tauri.localhost__REXENV_DEV_ANCESTOR__';
                return $csp;
            }
        }
    }
    return new \RexenvAdminer();
}
require __DIR__ . '/adminer.php';
"#;

/// The Vite dev server origin, allowed to frame Adminer ONLY in dev builds so
/// the embedded Database Browser works under `npm run tauri dev`. In release
/// builds it's empty — a shipped Adminer must not be frameable by any local
/// process squatting `:1420` (B14). The packaged app frames from
/// `tauri://localhost`, which stays allowed regardless.
#[cfg(debug_assertions)]
const DEV_FRAME_ANCESTOR: &str = " http://localhost:1420";
#[cfg(not(debug_assertions))]
const DEV_FRAME_ANCESTOR: &str = "";

/// The wrapper written to disk, with the dev-only frame-ancestor resolved for
/// this build profile (the `__REXENV_DEV_ANCESTOR__` placeholder in
/// [`WRAPPER_INDEX_PHP`]).
pub fn wrapper_index_php() -> String {
    WRAPPER_INDEX_PHP.replace("__REXENV_DEV_ANCESTOR__", DEV_FRAME_ANCESTOR)
}

/// Web docroot for Adminer, isolated from the binary cache.
pub fn docroot(platform: &dyn Platform) -> Result<PathBuf> {
    Ok(platform.paths().app_data_dir()?.join("adminer"))
}

/// Ensure the Adminer docroot exists: the bundled Adminer as `adminer.php` plus the
/// rexenv deep-link wrapper as `index.php` (the served entrypoint). Downloads +
/// caches `adminer.php` (§5.1) on first use. Idempotent: the Adminer copy is
/// refreshed only when missing / a different size, and the wrapper only when its
/// content differs (so a wrapper update redeploys on next start).
pub async fn ensure(platform: &dyn Platform) -> Result<PathBuf> {
    let dir = docroot(platform)?;
    std::fs::create_dir_all(&dir)?;

    // The real Adminer, copied beside the wrapper (the wrapper `require`s it).
    let adminer_php = dir.join("adminer.php");
    let src = binaries::resolve_file(platform, "adminer", binaries::ADMINER_VERSION).await?;
    let stale = std::fs::metadata(&adminer_php).ok().map(|m| m.len())
        != std::fs::metadata(&src).ok().map(|m| m.len());
    if stale {
        std::fs::copy(&src, &adminer_php)?;
    }

    // The served entrypoint: our deep-link wrapper (rewrite only if changed).
    // Rendered per build profile — the dev frame-ancestor is present only in
    // debug builds (B14).
    let index = dir.join("index.php");
    let rendered = wrapper_index_php();
    if std::fs::read_to_string(&index).ok().as_deref() != Some(rendered.as_str()) {
        std::fs::write(&index, &rendered)?;
    }
    Ok(dir)
}

/// Build the upstream Adminer URL from a request's path+query. Guard: it MUST
/// start with `/`. Otherwise a value like `@evil.com/` would parse the host as
/// `evil.com` (with `ADMINER_HOST` demoted to userinfo), and because the proxy
/// client pins ONLY `ADMINER_HOST` to loopback AND disables cert checks, that
/// would be an SSRF to an arbitrary host. Requests always arrive as
/// `rexdb://localhost/…`, so a legit path always starts with `/` — a `//path`
/// still resolves to the pinned host (the authority ends at the first `/`). B8,
/// defense-in-depth (not reachable today).
fn proxy_url(path_and_query: &str) -> Result<reqwest::Url> {
    if !path_and_query.starts_with('/') {
        return Err(Error::Other(format!(
            "adminer proxy: refusing non-absolute request path {path_and_query:?}"
        )));
    }
    reqwest::Url::parse(&format!("https://{ADMINER_HOST}{path_and_query}"))
        .map_err(|e| Error::Other(format!("adminer proxy: bad url: {e}")))
}

/// A response ready to hand back to the webview's custom-scheme responder.
pub struct ProxiedResponse {
    pub status: u16,
    /// Pass-through headers (hop-by-hop, cookie and length headers already stripped).
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Shared client for the Database Browser proxy: pinned to the loopback edge
/// (no DNS), with a persistent RUST-SIDE cookie jar so Adminer's session
/// survives the login POST no matter what cookie policy the webview applies to
/// cross-site iframes. Redirect policy is `none` so [`forward`] can resolve
/// Adminer's POST-redirect-GET itself — WKWebView does NOT follow redirects
/// returned by a custom-scheme handler (`WKURLSchemeTask` has no redirect
/// mechanism), so a passed-through 302 dead-ends as a blank frame. The jar
/// lives for the app run, so the session persists across iframe remounts.
fn proxy_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .cookie_store(true)
            // Loopback-pinned; our local-CA leaf won't chain for reqwest's
            // store (same trade-off as `proxy::edge_answers_as_ours`).
            .danger_accept_invalid_certs(true)
            .resolve(
                ADMINER_HOST,
                std::net::SocketAddr::from(([127, 0, 0, 1], crate::core::proxy::DEFAULT_HTTPS_PORT)),
            )
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .expect("adminer proxy client")
    })
}

/// Response headers that must NOT be replayed to the webview: cookies stay in
/// the Rust jar (the whole point), and framing/length headers are rebuilt by
/// the responder.
const STRIPPED_RESPONSE_HEADERS: &[&str] = &[
    "set-cookie",
    "content-length",
    "transfer-encoding",
    "connection",
    "keep-alive",
    "alt-svc",
];

/// Forward one Database Browser request (`rexdb://localhost<path_and_query>`)
/// to the Adminer vhost through the edge and return the response for the
/// webview. `content_type` is the request's Content-Type (form POSTs).
///
/// Redirects are followed HERE (≤5 hops, same-vhost only): Adminer is strictly
/// POST-redirect-GET, and WKWebView never follows a redirect returned by a
/// custom-scheme handler — replaying the 302 to the webview leaves the iframe
/// on a dead blank frame (observed live as "Save does nothing" / blank first
/// open). Per browser form semantics 301/302/303 become a body-less GET;
/// 307/308 keep the method + body.
pub async fn forward(
    method: &str,
    path_and_query: &str,
    content_type: Option<&str>,
    body: Vec<u8>,
) -> Result<ProxiedResponse> {
    let mut url = proxy_url(path_and_query)?;
    let mut method = reqwest::Method::from_bytes(method.as_bytes())
        .map_err(|e| Error::Other(format!("adminer proxy: bad method: {e}")))?;
    let mut content_type = content_type.map(str::to_string);
    let mut body = body;

    let mut hops = 0u8;
    let resp = loop {
        let mut req = proxy_client().request(method.clone(), url.clone());
        if let Some(ct) = &content_type {
            req = req.header(reqwest::header::CONTENT_TYPE, ct);
        }
        if !body.is_empty() {
            req = req.body(body.clone());
        }
        let resp = req
            .send()
            .await
            .map_err(|e| Error::Other(format!("adminer proxy: {e}")))?;

        if resp.status().is_redirection() && hops < 5 {
            let target = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|loc| url.join(loc).ok())
                // Never follow OFF the vhost — anything else replays to the
                // webview below (where frame policy applies).
                .filter(|next| next.host_str() == Some(ADMINER_HOST));
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
    let mut headers = Vec::new();
    for (name, value) in resp.headers() {
        let n = name.as_str().to_ascii_lowercase();
        if STRIPPED_RESPONSE_HEADERS.contains(&n.as_str()) {
            continue;
        }
        let Ok(v) = value.to_str() else { continue };
        // Absolute self-redirects must stay on the proxy origin; relative ones
        // (Adminer's norm) already resolve against `rexdb://localhost`.
        if n == "location" {
            let v = v
                .strip_prefix(&format!("https://{ADMINER_HOST}"))
                .unwrap_or(v);
            headers.push((n, v.to_string()));
            continue;
        }
        headers.push((n, v.to_string()));
    }
    let body = resp
        .bytes()
        .await
        .map_err(|e| Error::Other(format!("adminer proxy: read body: {e}")))?
        .to_vec();
    Ok(ProxiedResponse { status, headers, body })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_is_an_internal_subdomain_on_the_backbone_tld() {
        // Lives on the backbone TLD (whose resolver onboarding installs), so
        // Adminer never depends on any other TLD being present.
        assert!(ADMINER_HOST.ends_with(&format!(".rexenv.{}", crate::core::tld::BACKBONE_TLD)));
        // Not a wildcard / not a user site domain.
        assert_eq!(ADMINER_HOST, "adminer.rexenv.rex");
    }

    #[test]
    fn wrapper_defines_the_deeplink_hook_and_loopback_gate() {
        // Global-namespace hook (Adminer checks the global function name).
        assert!(WRAPPER_INDEX_PHP.contains("function adminer_object()"));
        assert!(WRAPPER_INDEX_PHP.contains("extends \\Adminer\\Adminer"));
        // Passwordless login is gated to loopback only (never a remote host).
        assert!(WRAPPER_INDEX_PHP.contains("function login("));
        assert!(WRAPPER_INDEX_PHP.contains("127.0.0.1"));
        // Auto-submit is keyed on the deep-link flag + carries the CSP nonce.
        assert!(WRAPPER_INDEX_PHP.contains(AUTOLOGIN_FLAG));
        assert!(WRAPPER_INDEX_PHP.contains("\\Adminer\\nonce()"));
        // It serves the real Adminer beside it.
        assert!(WRAPPER_INDEX_PHP.contains("require __DIR__ . '/adminer.php'"));
    }

    #[test]
    fn login_gate_matches_loopback_host_exactly_not_by_prefix() {
        // B1 regression: the passwordless gate must compare the target HOST to a
        // loopback allow-list EXACTLY. The OLD prefix test
        // (`strpos(\Adminer\SERVER, '127.0.0.1') === 0`) accepted
        // "127.0.0.1.evil.com" and granted a passwordless session to a REMOTE
        // server (→ LOAD DATA LOCAL INFILE local-file read). The behavioral proof
        // over a real PHP interpreter lives in examples/adminer_login_gate_check.
        assert!(WRAPPER_INDEX_PHP.contains("function rexenv_is_loopback("));
        assert!(WRAPPER_INDEX_PHP.contains("return rexenv_is_loopback(\\Adminer\\SERVER)"));
        assert!(WRAPPER_INDEX_PHP.contains(
            "in_array(strtolower($host), array('127.0.0.1', '::1', 'localhost'), true)"
        ));
        // The vulnerable prefix gate must be GONE — both halves of it.
        assert!(!WRAPPER_INDEX_PHP.contains("strpos(\\Adminer\\SERVER, '127.0.0.1') === 0"));
        assert!(!WRAPPER_INDEX_PHP.contains("strpos(\\Adminer\\SERVER, 'localhost') === 0"));
        // Extraction markers the example relies on stay present.
        assert!(WRAPPER_INDEX_PHP.contains("rexenv-loopback-gate:start"));
        assert!(WRAPPER_INDEX_PHP.contains("rexenv-loopback-gate:end"));
    }

    #[test]
    fn autologin_guard_is_keyed_per_target_server() {
        // The embedded webview is one permanent tab (one sessionStorage per
        // origin) — a global one-shot guard let the first engine's auto-login
        // suppress every other engine's. The key must carry the target server,
        // for BOTH param families we expose (server= for MySQL/MariaDB,
        // pgsql= for Postgres), with the param name in the key so the two
        // families can never collide.
        assert!(WRAPPER_INDEX_PHP.contains("'rexenv_autologin:' . $target"));
        assert!(WRAPPER_INDEX_PHP.contains("'pgsql:' . $_GET['pgsql']"));
        assert!(WRAPPER_INDEX_PHP.contains("'server:' . (isset($_GET['server'])"));
        // Same-server loop protection stays: the key is still set before the
        // submit and checked before firing.
        assert!(WRAPPER_INDEX_PHP.contains("sessionStorage.getItem(key)"));
        assert!(WRAPPER_INDEX_PHP.contains("sessionStorage.setItem(key,'1')"));
        // No global (unkeyed) guard left behind.
        assert!(!WRAPPER_INDEX_PHP.contains("getItem('rexenv_autologin')"));
    }

    #[test]
    fn wrapper_allows_framing_only_from_the_app_webview() {
        // Adminer's blanket deny is dropped (it blanks the in-app iframe)…
        assert!(WRAPPER_INDEX_PHP.contains("header_remove('X-Frame-Options')"));
        // …and replaced by frame-ancestors scoped to rexenv webview origins.
        assert!(WRAPPER_INDEX_PHP.contains("function csp("));
        assert!(WRAPPER_INDEX_PHP.contains("'frame-ancestors'"));
        assert!(WRAPPER_INDEX_PHP.contains("tauri://localhost"));
        // No wildcard — never embeddable by arbitrary origins.
        assert!(!WRAPPER_INDEX_PHP.contains("frame-ancestors *"));

        // The Vite dev origin is present ONLY in debug builds (B14): a shipped
        // Adminer must not be frameable by a local process squatting :1420. The
        // placeholder is always resolved — it must never ship literally.
        let rendered = wrapper_index_php();
        assert!(!rendered.contains("__REXENV_DEV_ANCESTOR__"));
        assert!(WRAPPER_INDEX_PHP.contains("__REXENV_DEV_ANCESTOR__"));
        assert!(rendered.contains("tauri://localhost"));
        #[cfg(debug_assertions)]
        assert!(rendered.contains("http://localhost:1420"));
        #[cfg(not(debug_assertions))]
        assert!(!rendered.contains("http://localhost:1420"));
    }

    #[test]
    fn proxy_url_requires_an_absolute_path_and_pins_the_host() {
        // Legit paths always arrive as rexdb://localhost/… → host stays pinned.
        assert_eq!(
            proxy_url("/index.php?server=127.0.0.1").unwrap().host_str(),
            Some(ADMINER_HOST)
        );
        assert_eq!(proxy_url("/").unwrap().host_str(), Some(ADMINER_HOST));
        // A `//path` is still on the pinned host (authority ends at the first
        // `/`) — a legit-looking shape the guard must NOT reject.
        assert_eq!(proxy_url("//evil.com/x").unwrap().host_str(), Some(ADMINER_HOST));
        // The SSRF shape (`@evil.com/` would smuggle a host via userinfo) and any
        // other non-absolute lead are refused before a request is ever built.
        assert!(proxy_url("@evil.com/").is_err());
        assert!(proxy_url("evil.com").is_err());
        assert!(proxy_url("*").is_err());
    }

    #[test]
    fn relative_locations_resolve_on_the_vhost() {
        // Adminer redirects with bare query-string Locations ("?select=notes");
        // the follow loop must resolve them onto the vhost so they get followed
        // in Rust (WKWebView can't follow custom-scheme redirects itself).
        let url =
            reqwest::Url::parse(&format!("https://{ADMINER_HOST}/?edit=notes&where%5Bid%5D=1"))
                .unwrap();
        let next = url.join("?select=notes").unwrap();
        assert_eq!(next.host_str(), Some(ADMINER_HOST));
        assert_eq!(next.query(), Some("select=notes"));
        // An absolute off-vhost Location must NOT be followed.
        let foreign = url.join("https://example.com/x").unwrap();
        assert_ne!(foreign.host_str(), Some(ADMINER_HOST));
    }

    #[test]
    fn import_php_limits_never_exceed_the_nginx_cap() {
        // The 413 this pair exists to kill comes back the moment PHP is told it
        // may accept MORE than nginx will pass — so pin BOTH keys against the cap
        // (and require both to be present, or a typo'd key reverts to PHP's 2M/8M
        // defaults with nothing failing).
        let mut seen = 0;
        for line in IMPORT_PHP_VALUE.lines() {
            let (key, value) = line.split_once('=').expect("key=value");
            let bytes = crate::core::php::parse_php_size(value).expect("php size");
            assert!(
                bytes <= MAX_IMPORT_BYTES,
                "{key} ({value}) exceeds the nginx cap ({MAX_IMPORT_BYTES} bytes) — nginx 413s first"
            );
            assert!(matches!(key, "upload_max_filesize" | "post_max_size"), "unexpected key {key}");
            seen += 1;
        }
        assert_eq!(seen, 2, "both upload_max_filesize and post_max_size must be set");
    }

    #[test]
    fn wrapper_rewrites_cookies_for_the_cross_site_iframe() {
        // SameSite=lax cookies are withheld from the embedded iframe's login
        // POST; the wrapper rewrites them to None+Secure+Partitioned at flush.
        assert!(WRAPPER_INDEX_PHP.contains("header_register_callback"));
        assert!(WRAPPER_INDEX_PHP.contains("SameSite=None; Partitioned"));
        assert!(WRAPPER_INDEX_PHP.contains("Secure"));
    }
}

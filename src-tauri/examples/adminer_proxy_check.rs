//! QA P0-1 check: the `rexdb://` Database Browser proxy logs into Adminer.
//!
//! WebKit withholds third-party cookies inside the app's cross-site Adminer
//! `<iframe>` (ITP), so the login POST used to lose its session and bounce back
//! to the form. `core::adminer::forward` proxies with a RUST-SIDE cookie jar —
//! this check replays the exact webview flow against the LIVE stack:
//!   1. GET the deep-link → Adminer login form + our auto-submit script;
//!   2. POST the form — WITH the CSRF token carried out of it, which is what the
//!      auto-submit script sends and what a hardcoded body left out;
//!   3. land on an AUTHENTICATED page ("Select database"), proving the jar
//!      carried the session across the redirect `forward` follows internally.
//!
//! Needs the real stack running (edge :443 + MySQL :13306) — it does NOT
//! start/stop anything. Run: `cargo run --example adminer_proxy_check`

use rexenv_lib::core::adminer;

/// Percent-encode the few bytes an Adminer token can contain (`id:hash`).
/// Deliberately tiny — pulling a crate in for one colon would be the larger change.
fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

#[tokio::main]
async fn main() {
    let deep_link = "/?server=127.0.0.1:13306&username=root&rexenv_auto=1";

    // 1. Login form with the auto-submit script (what the iframe first loads).
    let form = adminer::forward("GET", deep_link, None, Vec::new())
        .await
        .expect("GET login form through the proxy (is the stack running?)");
    let html = String::from_utf8_lossy(&form.body).to_string();
    assert!(html.contains("auth[driver]"), "login form rendered");
    assert!(html.contains("rexenv_autologin"), "auto-submit script injected");
    println!("✓ GET login form ({} bytes, status {})", form.body.len(), form.status);

    // 2. The POST the auto-submit script performs — including the CSRF TOKEN
    //    carried out of the form above.
    //
    //    This used to be a hardcoded body with no token, which is a flow the
    //    webview never performs: the auto-submit script submits the RENDERED
    //    form, and that form has always contained
    //    `<input type='hidden' name='token' …>`. Adminer answers a POST without
    //    a valid token by re-rendering the login page with 403, so the example
    //    failed while the product was fine — a check whose own header says it
    //    "replays the exact webview flow" replaying something else.
    //
    //    The token is session-bound (`id:hash`), which is exactly why it has to
    //    come from THIS response: `adminer::forward`'s Rust-side cookie jar is
    //    what makes the GET and the POST the same session, and that jar is the
    //    thing this check exists to prove.
    let token = html
        .split_once("name='token' value='")
        .and_then(|(_, rest)| rest.split_once('\''))
        .map(|(v, _)| v.to_string())
        .expect("the login form must carry a CSRF token — if Adminer stopped emitting one, this check needs rewriting, not relaxing");
    println!("✓ carried the form's CSRF token ({token})");
    let body = format!(
        "auth%5Bdriver%5D=server&auth%5Bserver%5D=127.0.0.1%3A13306\
         &auth%5Busername%5D=root&auth%5Bpassword%5D=&auth%5Bdb%5D=&token={}",
        urlencode(&token)
    )
    .into_bytes();
    let post = adminer::forward(
        "POST",
        deep_link,
        Some("application/x-www-form-urlencoded"),
        body,
    )
    .await
    .expect("POST login through the proxy");
    // `forward` FOLLOWS same-vhost redirects itself (≤5 hops) — deliberately,
    // because WKWebView never follows a redirect returned by a custom-scheme
    // handler, and replaying the 302 leaves the iframe on a dead blank frame.
    // So a successful login arrives here as the FINAL page, never as the 302.
    //
    // This used to assert 302 and then fetch the Location by hand, which is the
    // shape `forward` had before redirect-following was added for that blank-frame
    // bug. It asserted a status the function is now built never to return.
    assert_eq!(post.status, 200, "login POST lands on a page (got {})", post.status);
    assert!(
        !post.headers.iter().any(|(n, _)| n == "location"),
        "a same-vhost redirect must have been followed, not handed back"
    );

    // The landing page must be AUTHENTICATED — the Rust jar, not the webview,
    // carried the session cookie across the redirect. This is the whole claim:
    // the status above only says a page came back.
    let html = String::from_utf8_lossy(&post.body).to_string();
    assert!(
        html.contains("Select database") || html.contains("logout"),
        "post-login page is authenticated (status {}): {}",
        post.status,
        &html[..html.len().min(300)]
    );
    assert!(
        !post.headers.iter().any(|(n, _)| n == "set-cookie"),
        "Set-Cookie never leaks to the webview"
    );
    println!("✓ login followed through to an authenticated page — the jar carried the session");

    // 4. The CSP the WEBVIEW receives names the origin the app is framed from.
    //
    // The wrapper bakes macOS's origins, which is right for the direct https
    // vhost and wrong for this proxied copy: on Windows the app is served from
    // `http://tauri.localhost`, so the engine refused to frame Adminer and the
    // Database Browser rendered EMPTY (ledger #699). Read off a REAL response
    // here — the L0 test only proves the rewriter, not that the proxy calls it.
    let csp = [&form, &post]
        .iter()
        .filter_map(|r| {
            r.headers
                .iter()
                .find(|(n, _)| n == "content-security-policy")
                .map(|(_, v)| v.clone())
        })
        .next()
        .expect("Adminer sent a Content-Security-Policy");
    for origin in ["tauri://localhost", "http://tauri.localhost", "https://tauri.localhost"] {
        assert!(
            csp.contains(origin),
            "the replayed CSP does not let the app frame it ({origin} missing): {csp}"
        );
    }
    assert_eq!(
        csp.matches("frame-ancestors").count(),
        1,
        "exactly one frame-ancestors survives the rewrite: {csp}"
    );
    assert!(
        csp.contains("'strict-dynamic'") && csp.contains("nonce-"),
        "the rewrite mangled the script policy — Adminer's own JS would stop: {csp}"
    );
    println!("✓ the replayed CSP names every origin rexenv's webview is served from");
    println!("✓ adminer_proxy_check passed");
}

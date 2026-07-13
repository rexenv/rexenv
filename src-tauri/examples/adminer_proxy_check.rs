//! QA P0-1 check: the `rexdb://` Database Browser proxy logs into Adminer.
//!
//! WebKit withholds third-party cookies inside the app's cross-site Adminer
//! `<iframe>` (ITP), so the login POST used to lose its session and bounce back
//! to the form. `core::adminer::forward` proxies with a RUST-SIDE cookie jar —
//! this check replays the exact webview flow against the LIVE stack:
//!   1. GET the deep-link → Adminer login form + our auto-submit script;
//!   2. POST the form (empty password, loopback `login()` override) → 302;
//!   3. follow the Location → an AUTHENTICATED page ("Select database"),
//!      proving the jar carried the session across requests.
//!
//! Needs the real stack running (edge :443 + MySQL :13306) — it does NOT
//! start/stop anything. Run: `cargo run --example adminer_proxy_check`

use rexenv_lib::core::adminer;

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

    // 2. The POST the auto-submit script performs.
    let body = "auth%5Bdriver%5D=server&auth%5Bserver%5D=127.0.0.1%3A13306\
                &auth%5Busername%5D=root&auth%5Bpassword%5D=&auth%5Bdb%5D="
        .as_bytes()
        .to_vec();
    let post = adminer::forward(
        "POST",
        deep_link,
        Some("application/x-www-form-urlencoded"),
        body,
    )
    .await
    .expect("POST login through the proxy");
    assert_eq!(post.status, 302, "login POST redirects (got {})", post.status);
    let location = post
        .headers
        .iter()
        .find(|(n, _)| n == "location")
        .map(|(_, v)| v.clone())
        .expect("redirect Location");
    assert!(
        !location.starts_with("http"),
        "Location stays proxy-relative, got {location}"
    );
    println!("✓ POST login → 302 {location}");

    // 3. The redirect target must be an AUTHENTICATED page — the Rust jar, not
    //    the webview, carried the session cookie across.
    let target = if location.starts_with('/') { location } else { format!("/{location}") };
    let after = adminer::forward("GET", &target, None, Vec::new())
        .await
        .expect("GET post-login page");
    let html = String::from_utf8_lossy(&after.body).to_string();
    assert!(
        html.contains("Select database") || html.contains("logout"),
        "post-login page is authenticated (status {}): {}",
        after.status,
        &html[..html.len().min(300)]
    );
    assert!(
        !after.headers.iter().any(|(n, _)| n == "set-cookie"),
        "Set-Cookie never leaks to the webview"
    );
    println!("✓ session survived the redirect — landed on \"Select database\"");
    println!("✓ adminer_proxy_check passed");
}

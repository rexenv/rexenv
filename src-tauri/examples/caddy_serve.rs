//! Manual check for Caddy config + supervision (task 4.2).
//! Resolves Caddy, issues a site cert from the local CA, writes a Caddyfile with
//! explicit `tls`, starts Caddy on high ports (8080/8443 — no root), then serves
//! for ~20s so you can probe it, and stops cleanly.
//!
//! While it runs:
//!   openssl s_client -connect 127.0.0.1:8443 -servername proxytest.test </dev/null \
//!     | openssl x509 -noout -issuer        # => issuer = rexenv Local CA
//!   curl --resolve proxytest.test:8443:127.0.0.1 --cacert <ca> https://proxytest.test:8443/

use rexenv_lib::core::{binaries, proxy, ssl};
use rexenv_lib::platform;
use std::thread;
use std::time::Duration;

mod common;

#[tokio::main]
async fn main() {
    // Refuse beside a live stack: these services would JOIN it, not collide
    // with it, and a connect-based readiness gate is satisfied by the user's
    // server. FIRST statement — after anything is spawned, exiting leaks it.
    common::require_stack_stopped();
    let plat = platform::current();
    let host = "proxytest.test";

    let caddy = binaries::resolve(&*plat, "caddy", binaries::CADDY_VERSION)
        .await
        .expect("resolve caddy");
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).expect("ca");
    let cert = ssl::ensure_site_cert(plat.paths(), plat.permissions(), &ca, host).expect("cert");

    let cfg = proxy::CaddyConfig {
        http_port: 8080,
        https_port: 8443,
        routes: vec![proxy::SiteRoute {
            host: host.into(),
            wildcard: false,
            upstream: "127.0.0.1:9999".into(),
            cert_path: cert.cert_path.clone(),
            key_path: cert.key_path.clone(),
        }],
        admin_socket: Some(proxy::admin_socket_path(&*plat).expect("admin socket path")),
    };
    let caddyfile = proxy::write_caddyfile(&*plat, &cfg).expect("write caddyfile");
    println!("CADDYFILE={}", caddyfile.display());

    // Drop-guarded, and READY only once it is TRUE. `proxy::start` returns at
    // fork, so the line below used to advertise a URL for a human to curl before
    // anything was listening — and the refusal they got read as rexenv's edge
    // being broken. This example asserts nothing, which is exactly why it
    // survived the readiness sweep: there was no failing assertion to notice.
    let mut child = common::OwnedService::new(
        proxy::start(&*plat, &caddy, &caddyfile).expect("start caddy"),
        "caddy",
    );
    println!("CADDY_PID={}", child.id());
    common::await_listening(8443, "the caddy edge", None);
    // ACCEPT is not ANSWER: caddy binds before it has loaded certificates and
    // routes. The upstream here is a deliberately dead :9999, so the answer that
    // ends this wait is a 502 — which still proves the route table is live, and
    // is the honest thing to advertise to someone about to curl it.
    common::await_answering(host, 8443, &ca.cert_path, "the caddy edge answering HTTPS");
    println!("CADDY_READY https=8443 http=8080 host={host}");

    thread::sleep(Duration::from_secs(20));

    child.stop();
    println!("caddy stopped");
}

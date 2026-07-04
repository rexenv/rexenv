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

#[tokio::main]
async fn main() {
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

    let mut child = proxy::start(&*plat, &caddy, &caddyfile).expect("start caddy");
    println!("CADDY_PID={}", child.id());
    println!("CADDY_READY https=8443 http=8080 host={host}");

    thread::sleep(Duration::from_secs(20));

    let _ = proxy::stop(&*plat, child.id());
    let _ = child.wait();
    println!("caddy stopped");
}

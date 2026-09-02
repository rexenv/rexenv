//! Manual check for Caddy on privileged :443 (task 4.2).
//! Starts Caddy as root via ONE admin prompt (`caddy start` backgrounds it),
//! then returns — Caddy keeps serving on :80/:443. Stop later with `caddy stop`.

use rexenv_lib::core::{binaries, proxy, ssl};
use rexenv_lib::platform;

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
        http_port: proxy::DEFAULT_HTTP_PORT,   // 80
        https_port: proxy::DEFAULT_HTTPS_PORT, // 443
        routes: vec![proxy::SiteRoute {
            host: host.into(),
            wildcard: false,
            upstream: "127.0.0.1:9999".into(),
            cert_path: cert.cert_path.clone(),
            key_path: cert.key_path.clone(),
        aliases: Vec::new(),
    }],
        admin_socket: Some(proxy::admin_socket_path(&*plat).expect("admin socket path")),
    };
    let caddyfile = proxy::write_caddyfile(&*plat, &cfg).expect("write caddyfile");
    proxy::start_privileged(&*plat, &caddy, &caddyfile).expect("start caddy on :443");
    println!("CADDY_READY :80/:443 host={host} caddy={}", caddy.display());
}

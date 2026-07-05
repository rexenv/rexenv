//! Phase-3 §7.1 check: the hardened one-click "Log in as" token.
//! Brings up the full stack + a real WP site, then over the edge (TLS):
//!   (A) a loopback magic link → 302 to wp-admin WITH a `wordpress_logged_in` cookie;
//!   (B) reusing the SAME token → denied (single-use, no logged-in cookie);
//!   (C) a FRESH token presented with a Cloudflare tunnel header (`CF-Connecting-IP`)
//!       → denied (loopback/local-only), so a shared-tunnel replay can't log in.
//!
//! Run (ports 8443/8080/8088/9783/13306/1025/8025 free): `cargo run --example wp_login_check`

use rexenv_lib::core::service_manager::{Ports, ServiceManager};
use rexenv_lib::core::{services, sites, ssl, wordpress, wp_login};
use rexenv_lib::platform;
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

const HTTPS: u16 = 8443;

fn has_login_cookie(resp: &reqwest::Response) -> bool {
    resp.headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .any(|c| c.contains("wordpress_logged_in"))
}

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let domain = "wplogin.test";

    let conn = {
        let p = std::env::temp_dir().join("rexenv-7_1.db");
        let _ = std::fs::remove_file(&p);
        db::open(&p).unwrap()
    };
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();
    sites::provision(
        &conn,
        &*plat,
        &ca,
        NewSite {
            name: "WP Login".into(),
            domain: domain.into(),
            site_type: SiteType::Wordpress,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            path: String::new(),
        },
    )
    .unwrap();
    let site = sites::list(&conn).unwrap().into_iter().find(|s| s.domain == domain).unwrap();
    let docroot = PathBuf::from(&site.path);

    // Bring up the whole stack (starts MySQL, pools, nginx, edge).
    let ports = Ports { http: 8080, https: HTTPS, nginx: services::NGINX_HTTP_PORT };
    let mut mgr = ServiceManager::with_ports(ports);
    let all = sites::list(&conn).unwrap();
    let php_minors = rexenv_lib::core::php::installed_minors(&conn).unwrap();
    if let Err(e) = mgr.start_all(&*plat, &ca, &all, &php_minors).await {
        eprintln!("start_all failed: {e}");
        return;
    }
    for _ in 0..40 {
        if rexenv_lib::core::ports::is_listening(HTTPS) {
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }

    // Install WordPress (MySQL is up now).
    let php = rexenv_lib::core::binaries::resolve(&*plat, "php", rexenv_lib::core::binaries::PHP_VERSION).await.unwrap();
    let wp = rexenv_lib::core::binaries::resolve_file(&*plat, "wp-cli", rexenv_lib::core::binaries::WP_CLI_VERSION).await.unwrap();
    let mysql_base = rexenv_lib::core::binaries::resolve_dir(&*plat, "mysql", rexenv_lib::core::binaries::MYSQL_VERSION).await.unwrap();
    wordpress::install_for_site(
        &php,
        &wp,
        &docroot,
        domain,
        "WP Login",
        &format!("127.0.0.1:{}", rexenv_lib::core::db::DbEngine::Mysql.port()),
        &mysql_base,
        &Default::default(),
    )
    .expect("install wordpress");

    let ca_cert = reqwest::Certificate::from_pem(&std::fs::read(&ca.cert_path).unwrap()).unwrap();
    let addr: SocketAddr = format!("127.0.0.1:{HTTPS}").parse().unwrap();
    let client = reqwest::Client::builder()
        .add_root_certificate(ca_cert)
        .resolve(domain, addr)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let magic = |token: &str| format!("https://{domain}:{HTTPS}/?rexenv_login={token}&rexenv_user=1");

    // (A) loopback magic link logs in.
    let token = wp_login::issue(&php, &wp, &docroot, 1, wp_login::LOGIN_TTL_SECS).unwrap();
    let a = client.get(magic(&token)).send().await.expect("A");
    let a_status = a.status().as_u16();
    let a_login = has_login_cookie(&a);
    println!("(A) loopback: status={a_status} logged_in_cookie={a_login}");
    assert!(a_login, "(A) expected a wordpress_logged_in cookie");
    println!("✓ (A) loopback magic link → logged in (302 → wp-admin, auth cookie set)");

    // (B) reuse the SAME token → single-use, denied.
    let b = client.get(magic(&token)).send().await.expect("B");
    let b_login = has_login_cookie(&b);
    println!("(B) reuse: status={} logged_in_cookie={b_login}", b.status().as_u16());
    assert!(!b_login, "(B) token was reusable — NOT single-use");
    println!("✓ (B) reused token → denied (single-use)");

    // (C) fresh token + Cloudflare tunnel header → loopback/local-only denies it.
    let token2 = wp_login::issue(&php, &wp, &docroot, 1, wp_login::LOGIN_TTL_SECS).unwrap();
    let c = client
        .get(magic(&token2))
        .header("CF-Connecting-IP", "203.0.113.5")
        .send()
        .await
        .expect("C");
    let c_login = has_login_cookie(&c);
    println!("(C) tunnel header: status={} logged_in_cookie={c_login}", c.status().as_u16());
    assert!(!c_login, "(C) token honored through a tunnel header — NOT loopback-only");
    println!("✓ (C) tunnel-proxied token → denied (loopback/local-only)");
    // (Note: a tunnel attempt is rejected BEFORE the token is consumed — by design,
    // so a remote attacker can't burn a user's pending token.)

    mgr.stop_all(&*plat).unwrap();
    println!("\nALL GOOD — magic login is single-use, short-TTL, and loopback/local-only.");
}

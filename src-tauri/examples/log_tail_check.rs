//! Phase-3 §3.1 check: live log tailing. Brings up the stack via `ServiceManager`
//! (high ports), provisions a PHP site, tails `nginx-access.log` BEFORE and AFTER
//! hitting the site through the edge — proving new access lines appear in
//! near-real-time — and prints the site's log targets + a php-fpm pool tail.
//!
//! Run (ports 8443/8080/18088/9783/13306/11025/18025 free): `cargo run --example log_tail_check`

use rexenv_lib::core::service_manager::{Ports, ServiceManager};
use rexenv_lib::core::{logs, services, sites, ssl};
use rexenv_lib::platform;
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::net::SocketAddr;
use std::time::Duration;

const HTTPS: u16 = 8443;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let domain = "logtail.test";

    let conn = {
        let p = std::env::temp_dir().join("rexenv-3_1.db");
        let _ = std::fs::remove_file(&p);
        db::open(&p).unwrap()
    };
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();

    sites::provision(
        &conn,
        &*plat,
        &ca,
        NewSite {
            name: "Log Tail".into(),
            domain: domain.into(),
            site_type: SiteType::Php,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            path: String::new(), db_engine: rexenv_lib::state::models::SiteDbEngine::Mysql,
        },
    )
    .unwrap();
    let site = sites::list(&conn).unwrap().into_iter().find(|s| s.domain == domain).unwrap();

    println!("log targets for {domain}:");
    for t in logs::targets_for_site(&site) {
        println!("  {:<28} {}", t.key, t.label);
    }

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

    let before = logs::tail(&*plat, "nginx-access.log", 1000).unwrap().len();
    println!("\nnginx-access.log lines before: {before}");

    // Hit the site a few times through the edge (each is an access-log line).
    let ca_cert = reqwest::Certificate::from_pem(&std::fs::read(&ca.cert_path).unwrap()).unwrap();
    let addr: SocketAddr = format!("127.0.0.1:{HTTPS}").parse().unwrap();
    let client = reqwest::Client::builder()
        .add_root_certificate(ca_cert)
        .resolve(domain, addr)
        .build()
        .unwrap();
    for path in ["/", "/?ping=1", "/index.php"] {
        let _ = client.get(format!("https://{domain}:{HTTPS}{path}")).send().await;
    }
    std::thread::sleep(Duration::from_millis(700));

    let tail = logs::tail(&*plat, "nginx-access.log", 1000).unwrap();
    let after = tail.len();
    println!("nginx-access.log lines after:  {after}");
    assert!(after > before, "no new nginx access lines appeared ({before} → {after})");
    println!("✓ {} new access line(s); last:\n   {}", after - before, tail.last().cloned().unwrap_or_default());

    // A different source tails independently (php-fpm pool log).
    let fpm = logs::tail(&*plat, "php-fpm-8.3.log", 20).unwrap();
    println!("✓ php-fpm-8.3.log readable ({} line(s))", fpm.len());

    // Path-traversal guard.
    assert!(logs::tail(&*plat, "../../etc/passwd", 5).is_err(), "traversal not rejected");
    println!("✓ traversal key rejected");

    mgr.stop_all(&*plat).unwrap();
    println!("\nALL GOOD — log tailing follows nginx access in near-real-time.");
}

//! Phase-3 §5.2 check: Adminer served through the stack + connects to MySQL.
//! Brings up the stack via `ServiceManager` (high ports), then over the edge (TLS,
//! local CA): (1) `adminer.rexenv.rex` returns Adminer with the login form
//! PRE-FILLED from the query (server/username/db); (2) a normal `.test` site still
//! loads (the internal vhost didn't break routing); (3) a server-side probe in the
//! Adminer docroot runs `SELECT 6*7` against the site's MySQL via the same php-fpm
//! pool Adminer uses → proves the connect+SELECT path end-to-end. (Adminer's own
//! UI login uses a JS-injected CSRF token, so the interactive login is a one-click
//! browser step — not scriptable headlessly.)
//!
//! Run (ports 8443/8080/18088/9783/13306/11025/18025 free): `cargo run --example adminer_serve_check`

use rexenv_lib::core::service_manager::{Ports, ServiceManager};
use rexenv_lib::core::{adminer, services, sites, ssl};
use rexenv_lib::platform;
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::net::SocketAddr;
use std::time::Duration;

const HTTPS: u16 = 8443;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let site_domain = "other.test";

    let conn = {
        let p = std::env::temp_dir().join("rexenv-5_2.db");
        let _ = std::fs::remove_file(&p);
        db::open(&p).unwrap()
    };
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();
    sites::provision(
        &conn,
        &*plat,
        &ca,
        NewSite {
            name: "Other".into(),
            domain: site_domain.into(),
            site_type: SiteType::Php,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            path: String::new(),
            db_engine: rexenv_lib::state::models::SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
            git_migrate: true,
            git_build_assets: false,
        },
    )
    .unwrap();

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

    // A server-side probe in the Adminer docroot: same vhost/pool Adminer uses.
    let probe = adminer::docroot(&*plat).unwrap().join("dbprobe.php");
    std::fs::write(
        &probe,
        "<?php $m=@mysqli_connect('127.0.0.1','root','','',13306); \
         echo $m ? 'RESULT='.mysqli_fetch_row(mysqli_query($m,'SELECT 6*7'))[0] : 'ERR='.mysqli_connect_error();",
    )
    .unwrap();

    let ca_cert = reqwest::Certificate::from_pem(&std::fs::read(&ca.cert_path).unwrap()).unwrap();
    let addr: SocketAddr = format!("127.0.0.1:{HTTPS}").parse().unwrap();
    let client = reqwest::Client::builder()
        .add_root_certificate(ca_cert)
        .resolve(adminer::ADMINER_HOST, addr)
        .resolve(site_domain, addr)
        .build()
        .unwrap();

    // (1) Adminer served + login pre-filled from the query.
    let url = format!(
        "https://{}:{HTTPS}/?server=127.0.0.1:13306&username=root&db=mysql",
        adminer::ADMINER_HOST
    );
    let body = client.get(&url).send().await.expect("GET adminer").text().await.unwrap();
    assert!(body.contains("Adminer"), "not Adminer");
    assert!(body.contains("value=\"127.0.0.1:13306\""), "server not pre-filled");
    assert!(body.contains("value=\"root\""), "username not pre-filled");
    assert!(body.contains("name=\"auth[db]\" value=\"mysql\""), "db not pre-filled");
    println!("✓ Adminer served via edge (TLS) with login pre-filled (server/username/db)");

    // (2) Negative routing check: the normal site still loads.
    let site_body = client
        .get(format!("https://{site_domain}:{HTTPS}/"))
        .send()
        .await
        .expect("GET site")
        .text()
        .await
        .unwrap();
    assert!(site_body.contains("phpinfo()") || site_body.contains("PHP Version"), "normal site broke");
    println!("✓ normal site {site_domain} still loads (internal vhost didn't shadow it)");

    // (3) connect + SELECT through the Adminer pool.
    let probe_out = client
        .get(format!("https://{}:{HTTPS}/dbprobe.php", adminer::ADMINER_HOST))
        .send()
        .await
        .expect("GET probe")
        .text()
        .await
        .unwrap();
    println!("probe → {}", probe_out.trim());
    assert!(probe_out.contains("RESULT=42"), "SELECT through the Adminer pool failed: {probe_out}");
    println!("✓ connect + SELECT 6*7 → 42 through the Adminer php-fpm pool → MySQL");

    let _ = std::fs::remove_file(&probe);
    mgr.stop_all(&*plat).unwrap();
    println!("\nALL GOOD — Adminer is served through the stack, pre-filled, and reaches MySQL.");
}

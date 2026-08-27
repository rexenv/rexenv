//! Phase-3 §2.2 check: a site's PHP `mail()` is captured by Mailpit.
//! Drives the REAL integrated path via `ServiceManager` (high ports, no :443
//! prompt): provision a PHP site → `start_all` (Mailpit up + every php-fpm pool's
//! config pins `php_admin_value[sendmail_path]` → Mailpit's sendmail shim) → drop
//! a `mailtest.php` that calls `mail()` into the docroot → request it through the
//! edge (browser → Caddy → Nginx → php-fpm) → assert Mailpit's API count went up
//! and the captured message carries our subject.
//!
//! Run (ports 8443/8080/18088/9783/13306/11025/18025 free): `cargo run --example mail_route_check`

use rexenv_lib::core::service_manager::{Ports, ServiceManager};
use rexenv_lib::core::{binaries, mail, services, sites, ssl};
use rexenv_lib::platform;
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::net::SocketAddr;
use std::time::Duration;

mod common;

const HTTPS: u16 = 8443;
const SUBJECT: &str = "FPM mail test";

#[tokio::main]
async fn main() -> std::process::ExitCode {
    // Refuse beside a live stack: these services would JOIN it, not collide
    // with it, and a connect-based readiness gate is satisfied by the user's
    // server. FIRST statement — after anything is spawned, exiting leaks it.
    common::require_stack_stopped();
    let plat = platform::current();
    let domain = "mailroute.test";

    let conn = {
        let p = std::env::temp_dir().join("rexenv-2_2.db");
        let _ = std::fs::remove_file(&p);
        db::open(&p).unwrap()
    };
    // Fixture-owned docroot. `sites::provision` reads the `sites_dir` SETTING,
    // which falls back to a path derived from $HOME — so without this the site
    // lands in the user's real ~/rexenv/Sites and SURVIVES into the next run.
    // That is not untidy, it is the bug: two checks failed on their own
    // leftovers on 21 Aug 2026 (`wp_tools_check`, `wp_themes_check`).
    let _sites_dir = common::pin_fixture_sites_dir(&conn, "mailroute");
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();

    sites::provision(
        &conn,
        &*plat,
        &ca,
        NewSite {
            name: "Mail Route".into(),
            domain: domain.into(),
            site_type: SiteType::Php,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            path: String::new(),
            db_engine: rexenv_lib::state::models::SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
            git_migrate: true,
            git_build_assets: false,
            starter_db: false,
        },
    )
    .unwrap();
    let site = sites::list(&conn).unwrap().into_iter().find(|s| s.domain == domain).unwrap();

    // A page that sends mail via PHP's mail() — routed by the pool's sendmail_path.
    let php = format!(
        "<?php $ok = mail('catch@rexenv.test', '{SUBJECT}', 'Hello from the fpm pool'); \
         echo $ok ? 'SENT' : 'FAIL';"
    );
    std::fs::write(format!("{}/mailtest.php", site.path), php).unwrap();

    let ports = Ports { http: 8080, https: HTTPS, nginx: services::NGINX_HTTP_PORT };
    let mut mgr = ServiceManager::with_ports(ports);
    let all = sites::list(&conn).unwrap();
    let php_minors = rexenv_lib::core::php::installed_minors(&conn).unwrap();
    println!("starting stack (Caddy :{HTTPS}, Mailpit :{})…", mail::MAILPIT_SMTP_PORT);
    if let Err(e) = mgr.start_all(&*plat, &ca, &all, &php_minors, binaries::ADMINER_VERSION).await {
        eprintln!("start_all failed: {e}");
        // `FAILURE`, never a bare `return` — a bare return from `main` exits 0 and the
        // tier records a run that asserted nothing as green (common/mod.rs, the
        // verdict contract).
        return std::process::ExitCode::FAILURE;
    }

    // Wait for the edge to accept connections (Caddy warms up ~1s).
    let mut edge_up = false;
    for _ in 0..40 {
        if rexenv_lib::core::ports::is_listening(HTTPS) {
            edge_up = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    // The edge is not the whole stack: the request below traverses Caddy → the
    // shared nginx → the 8.3 pool, and `start_all` emits ReadyChecks for the
    // databases, mailpit and the FrankenPHP overrides only. `edge_up` above
    // stays the example's own reported fact; these are the two it never waited
    // for.
    //
    // AFTER the loop, deliberately. These two lines first landed INSIDE it, one
    // statement past the `break` — so on the ordinary path (edge already
    // listening on the first poll) they never ran at all, and on the unlucky
    // path they ran once per 250ms poll and would have failed naming nginx for
    // an edge that had not come up. A gate placed after an early exit is not a
    // gate; it is a comment that compiles.
    common::await_listening(rexenv_lib::core::services::NGINX_HTTP_PORT, "nginx", None);
    common::await_listening(rexenv_lib::core::services::PHP_FPM_PORT, "php-fpm 8.3", None);
    println!("edge listening on :{HTTPS} = {edge_up}");
    for s in mgr.status(&*plat, &[]) {
        println!("  {:<10} running={} port={}", s.name, s.running, s.port);
    }

    // Start clean: clear Mailpit's inbox.
    let plain = reqwest::Client::new();
    let _ = plain
        .delete(format!("{}/api/v1/messages", mail::api_base()))
        .send()
        .await;
    let before = message_total(&plain).await;
    println!("mailpit messages before: {before}");

    // Request the mail-sending page THROUGH the edge (validated against our CA).
    let ca_cert = reqwest::Certificate::from_pem(&std::fs::read(&ca.cert_path).unwrap()).unwrap();
    let addr: SocketAddr = format!("127.0.0.1:{HTTPS}").parse().unwrap();
    let client = reqwest::Client::builder()
        .add_root_certificate(ca_cert)
        .resolve(domain, addr)
        .build()
        .unwrap();
    let url = format!("https://{domain}:{HTTPS}/mailtest.php");
    let resp = client.get(&url).send().await.expect("request mailtest.php");
    let body = resp.text().await.unwrap_or_default();
    println!("GET {url} -> body: {body:?}");
    assert!(body.contains("SENT"), "mail() did not report success: {body:?}");

    // Give Mailpit a beat to ingest, then assert the count rose + subject present.
    std::thread::sleep(Duration::from_millis(600));
    let after = message_total(&plain).await;
    let listing = plain
        .get(format!("{}/api/v1/messages", mail::api_base()))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    println!("mailpit messages after: {after}");
    assert!(after > before, "Mailpit count did not increase ({before} → {after})");
    assert!(listing.contains(SUBJECT), "captured message missing subject {SUBJECT:?}");
    println!("✓ subject {SUBJECT:?} captured by Mailpit");

    mgr.stop_all(&*plat).unwrap();
    println!("\nALL GOOD — site PHP mail() routed through the pool into Mailpit.");
    std::process::ExitCode::SUCCESS
}

/// Mailpit's total message count from `GET /api/v1/messages` (parses the `total`
/// field without pulling in a JSON crate).
async fn message_total(client: &reqwest::Client) -> i64 {
    let body = client
        .get(format!("{}/api/v1/messages", mail::api_base()))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    // …,"total":N,… — find the field and read the integer.
    body.split("\"total\":")
        .nth(1)
        .and_then(|s| s.trim_start().split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(-1)
}

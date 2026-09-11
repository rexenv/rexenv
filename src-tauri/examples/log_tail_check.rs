//! Phase-3 §3.1 check: live log tailing. Brings up the stack via `ServiceManager`
//! (high ports), provisions a PHP site, tails `nginx-access.log` BEFORE and AFTER
//! hitting the site through the edge — proving new access lines appear in
//! near-real-time — and prints the site's log targets + a php-fpm pool tail.
//!
//! Run (ports 8443/8080/18088/9783/13306/11025/18025 free): `cargo run --example log_tail_check`

use rexenv_lib::core::service_manager::{Ports, ServiceManager};
use rexenv_lib::core::{binaries, logs, services, sites, ssl};
use rexenv_lib::platform;
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::net::SocketAddr;
use std::time::Duration;

mod common;

const HTTPS: u16 = 8443;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    // Refuse beside a live stack: these services would JOIN it, not collide
    // with it, and a connect-based readiness gate is satisfied by the user's
    // server. FIRST statement — after anything is spawned, exiting leaks it.
    common::require_stack_stopped();
    let plat = platform::current();
    let domain = "logtail.test";

    let conn = {
        let p = std::env::temp_dir().join("rexenv-3_1.db");
        let _ = std::fs::remove_file(&p);
        db::open(&p).unwrap()
    };
    // Fixture-owned docroot. `sites::provision` reads the `sites_dir` SETTING,
    // which falls back to a path derived from $HOME — so without this the site
    // lands in the user's real ~/rexenv/Sites and SURVIVES into the next run.
    // That is not untidy, it is the bug: two checks failed on their own
    // leftovers on 21 Aug 2026 (`wp_tools_check`, `wp_themes_check`).
    let _sites_dir = common::pin_fixture_sites_dir(&conn, "logtail");
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

    println!("log targets for {domain}:");
    for t in logs::targets_for_site(&site, &plat.paths().log_dir().unwrap(), &sites::other_domains(&conn, &site.id).unwrap()) {
        println!("  {:<28} {}", t.key, t.label);
    }

    let ports = Ports { http: 8080, https: HTTPS, nginx: services::NGINX_HTTP_PORT };
    let mut mgr = ServiceManager::with_ports(ports);
    let all = sites::list(&conn).unwrap();
    let php_minors = rexenv_lib::core::php::installed_minors(&conn).unwrap();
    if let Err(e) = mgr.start_all(&*plat, &ca, &all, &php_minors, binaries::ADMINER_VERSION, true).await {
        eprintln!("start_all failed: {e}");
        // `FAILURE`, never a bare `return` — a bare return from `main` exits 0 and the
        // tier records a run that asserted nothing as green (common/mod.rs, the
        // verdict contract).
        return std::process::ExitCode::FAILURE;
    }
    // The EDGE is not the whole stack. This loop waited only for Caddy, while
    // every request below traverses Caddy → the shared nginx → the 8.3 pool —
    // and `start_all` awaits ReadyChecks for the databases, mailpit and the
    // FrankenPHP overrides ONLY: neither nginx nor the pools emit one, so both
    // are spawned and returned from unwaited. Waiting for the front door and
    // then knocking on the back one is how this reads as a 502 from a working
    // stack.
    common::await_listening(HTTPS, "the caddy edge", None);
    common::await_listening(rexenv_lib::core::services::NGINX_HTTP_PORT, "nginx", None);
    common::await_listening(rexenv_lib::core::services::PHP_FPM_PORT, "php-fpm 8.3", None);

    // Keep the tail ITSELF, not its length. `logs::tail(_, N)` returns at most
    // N lines, so on a file past N the length pins at N and any `after > before`
    // is unreachable — this example read the REAL log dir, where
    // nginx-access.log stood at 31,544 lines on 20 Aug 2026, and had been
    // asserting nothing for a long time while exiting 0.
    let before = logs::tail(&*plat, "nginx-access.log", 1000).unwrap();
    println!("\nnginx-access.log tail before: {} line(s)", before.len());

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
    println!("nginx-access.log tail after:   {after} line(s)");
    // Ask whether the WINDOW MOVED. New lines shift a full tail and lengthen a
    // short one, so this holds at any file size — where a count saturates and a
    // content match cannot work either: nginx's access format here is
    // `domain timestamp bytes`, with no request line, so the `?ping=1` above is
    // never written down. (That was the first repair attempt, and it failed for
    // exactly that reason — the format has to be read, not assumed.)
    assert!(
        tail != before,
        "the tailed requests never reached the access log — the window is unchanged \
         at {after} line(s), which on a capped tail is what NOTHING happening looks \
         like as well as what a full window looks like"
    );
    println!("✓ the access-log window moved; last:\n   {}", tail.last().cloned().unwrap_or_default());

    // A different source tails independently (php-fpm pool log).
    let fpm = logs::tail(&*plat, "php-fpm-8.3.log", 20).unwrap();
    println!("✓ php-fpm-8.3.log readable ({} line(s))", fpm.len());

    // Path-traversal guard.
    assert!(logs::tail(&*plat, "../../etc/passwd", 5).is_err(), "traversal not rejected");
    println!("✓ traversal key rejected");

    mgr.stop_all(&*plat).unwrap();
    println!("\nALL GOOD — log tailing follows nginx access in near-real-time.");
    std::process::ExitCode::SUCCESS
}

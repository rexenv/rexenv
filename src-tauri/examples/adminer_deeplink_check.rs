//! Phase-3 §11.4 check: Adminer per-site deep-link with a scoped session.
//!
//! Brings the stack up + installs a real WordPress DB, then proves the §11.4
//! wrapper turns a rexenv "Open database" link into a one-click scoped session:
//!   - GET the deep-link (`?server=…&username=root&db=…&rexenv_auto=1`) returns
//!     Adminer's login form WITH our auto-submit script injected (CSP nonce);
//!   - replaying what that script does — POST Adminer's own CSRF-tokened form with
//!     an EMPTY password (the `login()` override accepts loopback) — lands an
//!     authenticated session straight in the site's DB (its tables are listed),
//!     instead of the stock "does not support accessing without a password" wall.
//!
//! Run (ports 8443/8080/18088/9783/13306/11025/18025 free): `cargo run --example adminer_deeplink_check`

use rexenv_lib::core::service_manager::{Ports, ServiceManager};
use rexenv_lib::core::{adminer, binaries, services, sites, ssl, wordpress};
use rexenv_lib::platform;
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::Command;

mod common;

const HTTPS: u16 = 8443;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let plat = platform::current();
    let domain = "dbsite.test";
    let db_name = wordpress::db_name_for(SiteType::Wordpress, domain);

    let conn = {
        let p = std::env::temp_dir().join("rexenv-11_4.db");
        let _ = std::fs::remove_file(&p);
        db::open(&p).unwrap()
    };
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();
    sites::provision(
        &conn,
        &*plat,
        &ca,
        NewSite {
            name: "DB Site".into(),
            domain: domain.into(),
            site_type: SiteType::Wordpress,
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
    let site = sites::list(&conn).unwrap().into_iter().find(|s| s.domain == domain).unwrap();
    let docroot = PathBuf::from(&site.path);

    let mut mgr = ServiceManager::with_ports(Ports { http: 8080, https: HTTPS, nginx: services::NGINX_HTTP_PORT });
    let all = sites::list(&conn).unwrap();
    let php_minors = rexenv_lib::core::php::installed_minors(&conn).unwrap();
    if let Err(e) = mgr.start_all(&*plat, &ca, &all, &php_minors, binaries::ADMINER_VERSION).await {
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

    // Real DB with tables to land in.
    let php = binaries::resolve(&*plat, "php", binaries::PHP_VERSION).await.unwrap();
    let wp = binaries::resolve_file(&*plat, "wp-cli", binaries::WP_CLI_VERSION).await.unwrap();
    let mysql_base = binaries::resolve_dir(&*plat, "mysql", binaries::MYSQL_VERSION).await.unwrap();
    let (db_client, _) = rexenv_lib::core::db::DbEngine::Mysql
        .sql_client_bins(&*plat, rexenv_lib::core::binaries::MYSQL_VERSION)
        .await
        .expect("bundled MySQL client");
    {
        let _ = Command::new(mysql_base.join("bin/mysql"))
            .args(["-h127.0.0.1", "-P13306", "-uroot", "-e", &format!("DROP DATABASE IF EXISTS {db_name}")])
            .status();
        let _ = std::fs::remove_dir_all(&docroot);
        std::fs::create_dir_all(&docroot).unwrap();
    }
    wordpress::install_for_site(
        &php, &wp, &docroot, domain, "DB Site",
        &db_name,
        &format!("127.0.0.1:{}", rexenv_lib::core::db::DbEngine::Mysql.port()),
        &db_client,
        &Default::default(),
    )
    .expect("install wordpress");
    println!("✓ stack up + WordPress DB `{db_name}` installed (has wp_* tables)");

    // HTTP client trusting only our CA; manual cookie jar (reqwest built w/o cookies).
    let ca_cert = reqwest::Certificate::from_pem(&std::fs::read(&ca.cert_path).unwrap()).unwrap();
    let addr: SocketAddr = format!("127.0.0.1:{HTTPS}").parse().unwrap();
    let client = reqwest::Client::builder()
        .add_root_certificate(ca_cert)
        .resolve(adminer::ADMINER_HOST, addr)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let base = format!("https://{}:{HTTPS}", adminer::ADMINER_HOST);
    let deeplink = format!("{base}/?server=127.0.0.1:13306&username=root&db={db_name}&rexenv_auto=1");

    let mut jar: BTreeMap<String, String> = BTreeMap::new();

    // 1) GET the deep-link → login form with our auto-submit script injected.
    let resp = client.get(&deeplink).header("Cookie", cookie_header(&jar)).send().await.expect("GET deeplink");
    capture_cookies(&resp, &mut jar);
    let login_html = resp.text().await.unwrap_or_default();
    assert!(login_html.contains("rexenv_autologin"), "auto-submit script not injected by the wrapper");
    println!("✓ deep-link returns Adminer login form WITH the rexenv auto-submit script");

    // 2) Replay what the script does: POST the form with an EMPTY password. The
    //    login form carries no CSRF token (Adminer auto-fills a valid one when none
    //    is posted for the unauthenticated login), so we post auth fields only.
    let form = format!(
        "auth%5Bdriver%5D=server&auth%5Bserver%5D=127.0.0.1%3A13306&auth%5Busername%5D=root&auth%5Bpassword%5D=&auth%5Bdb%5D={db_name}"
    );
    let post = client
        .post(&deeplink)
        .header("Cookie", cookie_header(&jar))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form)
        .send()
        .await
        .expect("POST auth");
    capture_cookies(&post, &mut jar);
    let code = post.status().as_u16();
    let post_body = post.text().await.unwrap_or_default();
    println!("auth POST → {code} (cookies: {})", jar.keys().cloned().collect::<Vec<_>>().join(","));
    // The stock empty-password wall must NOT appear (our login() override removed it).
    assert!(
        !post_body.contains("does not support accessing a database without a password"),
        "empty-password login was rejected — the login() override didn't take effect"
    );

    // 3) Follow into the DB view with the session cookies → its tables are listed.
    let db_view = format!("{base}/?server=127.0.0.1%3A13306&username=root&db={db_name}");
    let view = client.get(&db_view).header("Cookie", cookie_header(&jar)).send().await.expect("GET db view");
    let body = view.text().await.unwrap_or_default();
    let landed = body.contains("wp_options") || body.contains("wp_posts");
    println!("DB view shows wp_* tables: {landed}");
    assert!(landed, "authenticated session did not land in the site's DB (no wp_* tables listed)");
    println!("✓ scoped session lands straight in `{db_name}` (wp_* tables listed) — no manual login");

    mgr.stop_all(&*plat).unwrap();
    println!("\nALL GOOD — the Adminer deep-link injects the auto-login script and a passwordless loopback session lands in the site's DB.");
    std::process::ExitCode::SUCCESS
}

/// Serialize the cookie jar to a `Cookie:` header value.
fn cookie_header(jar: &BTreeMap<String, String>) -> String {
    jar.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; ")
}

/// Capture `Set-Cookie` name=value pairs from a response into the jar.
fn capture_cookies(resp: &reqwest::Response, jar: &mut BTreeMap<String, String>) {
    for hv in resp.headers().get_all("set-cookie") {
        if let Ok(s) = hv.to_str() {
            let pair = s.split(';').next().unwrap_or("");
            if let Some((k, v)) = pair.split_once('=') {
                jar.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }
}


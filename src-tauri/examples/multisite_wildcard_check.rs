//! Phase-3 §10.2 check: wildcard cert + edge route for SUBDOMAIN multisite.
//!
//! Provisions a subdomain-multisite WP site (`mysite.test`) plus a plain
//! non-multisite site (`other.test`), then verifies:
//!   - the regenerated nginx + Caddy configs carry the wildcard host
//!     (`server_name mysite.test *.mysite.test;` / `https://mysite.test,
//!     https://*.mysite.test {`);
//!   - real network sub-sites `a.mysite.test` / `b.mysite.test` are served with a
//!     VALID LOCK — the request goes over a client that trusts ONLY our local CA,
//!     so a successful TLS handshake proves the wildcard leaf (`*.mysite.test`)
//!     chains to the CA;
//!   - `openssl s_client` confirms SAN `DNS:*.mysite.test` + issuer = our CA;
//!   - NEGATIVE (precedence footgun): `other.test` STILL loads after the
//!     `*.mysite.test` route is added — the wildcard never shadows another site.
//!
//! Run (ports 8443/8080/18088/9783/13306/11025/18025 free): `cargo run --example multisite_wildcard_check`

use rexenv_lib::core::service_manager::{self, Ports, ServiceManager};
use rexenv_lib::core::{binaries, services, sites, ssl, wordpress};
use rexenv_lib::platform;

use rexenv_lib::state::models::{MultisiteMode, NewSite, SiteType, WebServer};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::Command;

const HTTPS: u16 = 8443;


mod common;
#[tokio::main]
async fn main() -> std::process::ExitCode {
    let plat = platform::current();
    let domain = "mysite.test";
    let other = "other.test";

    let (conn, _dbf) = common::fixture_db("multisite_wildcard_check");
    // Fixture-owned sites dir, pinned BEFORE the first `sites::provision`.
    // `sites::provision` reads the `sites_dir` SETTING, which falls back to a
    // path computed from the HOME directory — so without this the docroot lands
    // in the user's real ~/rexenv/Sites, and the `remove_dir_all` below deletes
    // it there. See `common::pin_fixture_sites_dir`.
    let _sites_dir = common::pin_fixture_sites_dir(&conn, "mswild");
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();

    // The subdomain-multisite site + a plain non-multisite control site.
    sites::provision(&conn, &*plat, &ca, new_site("Network", domain, SiteType::Wordpress)).unwrap();
    sites::provision(&conn, &*plat, &ca, new_site("Other", other, SiteType::Php)).unwrap();
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

    let php = binaries::resolve(&*plat, "php", binaries::PHP_VERSION).await.unwrap();
    let wp = binaries::resolve_file(&*plat, "wp-cli", binaries::WP_CLI_VERSION).await.unwrap();

    // Clean slate (re-runnable): drop the leftover DB + docroot so we install fresh.
    let mysql_base = binaries::resolve_dir(&*plat, "mysql", binaries::MYSQL_VERSION).await.unwrap();
    let (db_client, _) = rexenv_lib::core::db::DbEngine::Mysql
        .sql_client_bins(&*plat, rexenv_lib::core::binaries::MYSQL_VERSION)
        .await
        .expect("bundled MySQL client");
    {
        let _ = Command::new(mysql_base.join("bin/mysql"))
            .args(["-h127.0.0.1", "-P13306", "-uroot", "-e",
                &format!("DROP DATABASE IF EXISTS {}", wordpress::db_name_for(SiteType::Wordpress, domain))])
            .status();
        let _ = std::fs::remove_dir_all(&docroot);
        std::fs::create_dir_all(&docroot).unwrap();
    }

    common::install_wp(&php, &wp, &docroot, domain, "Network", &db_client);

    // Convert to SUBDOMAIN multisite + persist the mode, then reload the edge so the
    // wildcard nginx server_name + Caddy host route are regenerated.
    let updated = sites::convert_multisite(&conn, &php, &wp, &docroot, &site.id, MultisiteMode::Subdomain)
        .expect("convert").expect("site exists");
    assert!(matches!(updated.multisite, MultisiteMode::Subdomain), "mode not persisted");
    println!("✓ converted to subdomain multisite; persisted mode = {}", updated.multisite.as_db());

    let sites_now = sites::list(&conn).unwrap();
    let checks = mgr.reload(&*plat, &ca, &sites_now, false).await.expect("reload");
    service_manager::await_ready(checks).await.expect("backends ready");

    // Configs carry the wildcard host.
    let cfg_dir = plat.paths().config_dir().unwrap();
    let nginx_conf = std::fs::read_to_string(cfg_dir.join("nginx.conf")).unwrap();
    assert!(nginx_conf.contains("server_name mysite.test *.mysite.test;"),
        "nginx.conf missing wildcard server_name");
    let caddyfile = std::fs::read_to_string(cfg_dir.join("Caddyfile")).unwrap();
    assert!(caddyfile.contains("https://mysite.test, https://*.mysite.test {"),
        "Caddyfile missing wildcard host route");
    // Negative (config level): other.test stays an exact, non-wildcard host.
    assert!(caddyfile.contains("https://other.test {"), "other.test route missing/changed");
    assert!(!caddyfile.contains("*.other.test"), "other.test must not gain a wildcard");
    println!("✓ nginx server_name + Caddy route carry `*.mysite.test`; other.test stays exact-host");

    // Create two real network sub-sites (subdomain install ⇒ a.mysite.test / b.mysite.test).
    for slug in ["a", "b"] {
        let out = wordpress::wp_run(&php, &wp, &docroot,
            &["site", "create", &format!("--slug={slug}"), "--porcelain"]);
        println!("wp site create --slug={slug} → {out:?}");
        assert!(out.is_ok(), "sub-site create failed");
    }
    let site_count = wordpress::wp_run(&php, &wp, &docroot, &["site", "list", "--format=count"]).unwrap();
    println!("wp site list count = {} (apex + a + b)", site_count.trim());
    assert!(site_count.trim().parse::<u64>().unwrap_or(0) >= 3, "network missing sub-sites");

    // Each sub-site is served with a VALID LOCK: this client trusts ONLY our CA, so a
    // successful handshake to `*.mysite.test` proves the wildcard leaf chains to it.
    let ca_cert = reqwest::Certificate::from_pem(&std::fs::read(&ca.cert_path).unwrap()).unwrap();
    let addr: SocketAddr = format!("127.0.0.1:{HTTPS}").parse().unwrap();
    let client = reqwest::Client::builder()
        .add_root_certificate(ca_cert.clone())
        .resolve("a.mysite.test", addr)
        .resolve("b.mysite.test", addr)
        .resolve(other, addr)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    for host in ["a.mysite.test", "b.mysite.test"] {
        let resp = client.get(format!("https://{host}:{HTTPS}/")).send().await
            .unwrap_or_else(|e| panic!("TLS/handshake to {host} failed (lock invalid?): {e}"));
        let code = resp.status().as_u16();
        let loc = resp.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
        println!("GET https://{host}/ → {code} location={loc}");
        // Handshake succeeded (valid lock) + nginx/php/WP answered for the sub-site host.
        assert!(code < 500, "{host} not served (got {code})");
    }
    println!("✓ a.mysite.test + b.mysite.test served with a valid lock (wildcard cert + route)");

    // NEGATIVE (precedence footgun): other.test STILL loads — not shadowed by `*.mysite.test`.
    let resp = client.get(format!("https://{other}:{HTTPS}/")).send().await
        .expect("GET other.test");
    let code = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    println!("GET https://{other}/ → {code} (phpinfo bytes: {})", body.len());
    assert_eq!(code, 200, "other.test stopped loading after wildcard route (SHADOWED!)");
    assert!(body.contains("phpinfo") || body.contains("PHP Version"), "other.test served wrong content");
    println!("✓ other.test still loads (200, phpinfo) — wildcard route does NOT shadow it");

    // openssl: SAN *.mysite.test + issuer = our CA, presented for a sub-site SNI.
    let s = Command::new("openssl")
        .args(["s_client", "-servername", "a.mysite.test", "-connect", &format!("127.0.0.1:{HTTPS}")])
        .arg("-showcerts")
        .stdin(std::process::Stdio::null())
        .output();
    if let Ok(out) = s {
        let txt = String::from_utf8_lossy(&out.stdout);
        // Decode the leaf so SAN/issuer are human-readable.
        let parsed = Command::new("openssl")
            .args(["x509", "-noout", "-issuer", "-ext", "subjectAltName"])
            .stdin(piped(&txt))
            .output();
        if let Ok(p) = parsed {
            let info = String::from_utf8_lossy(&p.stdout);
            println!("--- leaf for a.mysite.test ---\n{}", info.trim());
            assert!(info.contains("*.mysite.test"), "leaf SAN missing *.mysite.test");
            assert!(info.to_lowercase().contains("rexenv local ca"), "issuer is not our CA");
            println!("✓ openssl: SAN DNS:*.mysite.test, issuer = rexenv Local CA");
        }
    } else {
        println!("(openssl not available — skipped SAN/issuer dump; reqwest handshake already proved the lock)");
    }

    mgr.stop_all(&*plat).unwrap();
    println!("\nALL GOOD — subdomain multisite serves *.mysite.test sub-sites over the wildcard cert/route without shadowing other.test.");
    std::process::ExitCode::SUCCESS
}

fn new_site(name: &str, domain: &str, t: SiteType) -> NewSite {
    NewSite {
        name: name.into(),
        domain: domain.into(),
        site_type: t,
        php_version: "8.3".into(),
        web_server: WebServer::Nginx,
        path: String::new(),
            db_engine: rexenv_lib::state::models::SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
            git_migrate: true,
            git_build_assets: false,
            starter_db: false,
    }
}

/// Feed `text` to a child's stdin via a temp file (s_client output → x509 -in).
fn piped(text: &str) -> std::process::Stdio {
    let p = std::env::temp_dir().join("rexenv-10_2-leaf.pem");
    std::fs::write(&p, text).unwrap();
    std::process::Stdio::from(std::fs::File::open(&p).unwrap())
}

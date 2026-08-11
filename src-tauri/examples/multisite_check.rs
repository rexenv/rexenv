//! Phase-3 §10.1 check: convert a WP site to SUBDIRECTORY multisite.
//! Full stack + real WP install, then `convert_multisite(Subdirectory)`:
//!   - WP-CLI writes the network constants (MULTISITE=1, SUBDOMAIN_INSTALL=false);
//!   - the persisted mode maps to RewriteMode::SubdirectoryMultisite, so the
//!     regenerated nginx.conf gains the WP network rewrite rules;
//!   - `/wp-admin/network/` is recognized (redirects to wp-login, not a 404).
//!
//! Run (ports 8443/8080/18088/9783/13306/11025/18025 free): `cargo run --example multisite_check`

use rexenv_lib::core::service_manager::{self, Ports, ServiceManager};
use rexenv_lib::core::{binaries, services, sites, ssl, wordpress};
use rexenv_lib::platform;
use rexenv_lib::state::db;
use rexenv_lib::state::models::{MultisiteMode, NewSite, SiteType, WebServer};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

const HTTPS: u16 = 8443;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let domain = "mysite.test";

    let conn = {
        let p = std::env::temp_dir().join("rexenv-10_1.db");
        let _ = std::fs::remove_file(&p);
        db::open(&p).unwrap()
    };
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();
    sites::provision(
        &conn,
        &*plat,
        &ca,
        NewSite {
            name: "Network".into(),
            domain: domain.into(),
            site_type: SiteType::Wordpress,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            path: String::new(),
            db_engine: rexenv_lib::state::models::SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
        },
    )
    .unwrap();
    let site = sites::list(&conn).unwrap().into_iter().find(|s| s.domain == domain).unwrap();
    let docroot = PathBuf::from(&site.path);

    let mut mgr = ServiceManager::with_ports(Ports { http: 8080, https: HTTPS, nginx: services::NGINX_HTTP_PORT });
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

    let php = binaries::resolve(&*plat, "php", binaries::PHP_VERSION).await.unwrap();
    let wp = binaries::resolve_file(&*plat, "wp-cli", binaries::WP_CLI_VERSION).await.unwrap();

    // Clean slate (re-runnable): drop any leftover DB + docroot from a prior run so
    // we genuinely start single-site before converting.
    let mysql_base = binaries::resolve_dir(&*plat, "mysql", binaries::MYSQL_VERSION).await.unwrap();
    {
        let _ = std::process::Command::new(mysql_base.join("bin/mysql"))
            .args([
                "-h127.0.0.1",
                "-P13306",
                "-uroot",
                "-e",
                &format!("DROP DATABASE IF EXISTS {}", wordpress::db_name_for(domain)),
            ])
            .status();
        let _ = std::fs::remove_dir_all(&docroot);
        std::fs::create_dir_all(&docroot).unwrap();
    }

    wordpress::install_for_site(
        &php,
        &wp,
        &docroot,
        domain,
        "Network",
        &wordpress::db_name_for(domain),
        &format!("127.0.0.1:{}", rexenv_lib::core::db::DbEngine::Mysql.port()),
        &mysql_base,
        &Default::default(),
    )
    .expect("install wordpress");
    assert!(!wordpress::wp_info(&php, &wp, &docroot).unwrap().multisite, "should start single-site");

    // Convert to SUBDIRECTORY multisite + persist the mode.
    let updated = sites::convert_multisite(&conn, &php, &wp, &docroot, &site.id, MultisiteMode::Subdirectory)
        .expect("convert")
        .expect("site exists");
    assert!(matches!(updated.multisite, MultisiteMode::Subdirectory), "mode not persisted");
    println!("✓ converted; persisted mode = {}", updated.multisite.as_db());

    // Constants written.
    let multisite = wordpress::wp_run(&php, &wp, &docroot, &["config", "get", "MULTISITE"]).unwrap();
    let subdomain = wordpress::wp_run(&php, &wp, &docroot, &["config", "get", "SUBDOMAIN_INSTALL"]).unwrap();
    println!("MULTISITE={multisite} SUBDOMAIN_INSTALL={subdomain}");
    assert_eq!(multisite.trim(), "1", "MULTISITE not set");
    assert!(matches!(subdomain.trim(), "" | "false"), "subdirectory install should not be subdomain");
    assert!(wordpress::wp_info(&php, &wp, &docroot).unwrap().multisite, "wp_info should report multisite");
    // Authoritative: the network exists (multisite-only `wp site list` succeeds with ≥1 site).
    let site_count = wordpress::wp_run(&php, &wp, &docroot, &["site", "list", "--format=count"]).unwrap();
    println!("wp site list count = {site_count}");
    assert!(site_count.trim().parse::<u64>().unwrap_or(0) >= 1, "network has no sites");
    println!("✓ network constants written + wp_info.multisite = true + `wp site list` works");

    // Reload the edge from the updated site list → nginx gets the subdirectory template.
    let sites_now = sites::list(&conn).unwrap();
    let checks = mgr.reload(&*plat, &ca, &sites_now, false).await.expect("reload");
    service_manager::await_ready(checks).await.expect("backends ready");
    let nginx_conf = plat.paths().config_dir().unwrap().join("nginx.conf");
    let conf = std::fs::read_to_string(&nginx_conf).unwrap();
    assert!(conf.contains("rewrite /wp-admin$"), "nginx.conf missing the subdirectory-multisite rewrite");
    println!("✓ nginx.conf regenerated with the subdirectory-multisite rewrite template");

    // /wp-admin/network/ is recognized (redirects to wp-login, not 404/500).
    let ca_cert = reqwest::Certificate::from_pem(&std::fs::read(&ca.cert_path).unwrap()).unwrap();
    let addr: SocketAddr = format!("127.0.0.1:{HTTPS}").parse().unwrap();
    let client = reqwest::Client::builder()
        .add_root_certificate(ca_cert)
        .resolve(domain, addr)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let resp = client
        .get(format!("https://{domain}:{HTTPS}/wp-admin/network/"))
        .send()
        .await
        .expect("GET network admin");
    let code = resp.status().as_u16();
    let location = resp.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    println!("GET /wp-admin/network/ → {code} location={location}");
    // Recognized + routed (auth redirect), not a 404/500 — the network admin path
    // is served through the subdirectory rewrite rather than erroring.
    assert!(code == 302 || code == 200, "network admin not recognized (got {code})");
    println!("✓ /wp-admin/network/ recognized (HTTP {code}, routed through the multisite rewrite)");

    mgr.stop_all(&*plat).unwrap();
    println!("\nALL GOOD — subdirectory multisite conversion writes constants, maps RewriteMode, serves network admin.");
}

//! Phase-3 §10.3 check: multisite network management (sub-sites, network-activate,
//! super-admins) against a real SUBDIRECTORY network.
//!
//! Full stack + WP install → convert to subdirectory multisite, then exercise the
//! `core::wordpress` network helpers end to end:
//!   - `network_site_create("team")` + `network_site_list` (apex + team);
//!   - the new sub-site `mysite.test/team/` is served (recognized, < 500);
//!   - `plugin_activate_network("akismet")` → `plugin list` shows `active-network`
//!     (the "Network active" badge);
//!   - `super_admin_add` + `super_admin_list` includes the granted user;
//!   - `network_site_delete` removes the sub-site (list shrinks back).
//!
//! Run (ports 8443/8080/8088/9783/13306/1025/8025 free): `cargo run --example network_check`

use rexenv_lib::core::service_manager::{self, Ports, ServiceManager};
use rexenv_lib::core::{binaries, services, sites, ssl, wordpress};
use rexenv_lib::platform;
use rexenv_lib::state::db;
use rexenv_lib::state::models::{MultisiteMode, NewSite, SiteType, WebServer};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

const HTTPS: u16 = 8443;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let domain = "mysite.test";

    let conn = {
        let p = std::env::temp_dir().join("rexenv-10_3.db");
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

    // Clean slate (re-runnable).
    let mysql_base = binaries::resolve_dir(&*plat, "mysql", binaries::MYSQL_VERSION).await.unwrap();
    {
        let _ = Command::new(mysql_base.join("bin/mysql"))
            .args(["-h127.0.0.1", "-P13306", "-uroot", "-e",
                &format!("DROP DATABASE IF EXISTS {}", wordpress::db_name_for(domain))])
            .status();
        let _ = std::fs::remove_dir_all(&docroot);
        std::fs::create_dir_all(&docroot).unwrap();
    }

    wordpress::install_for_site(
        &php, &wp, &docroot, domain, "Network",
        &format!("127.0.0.1:{}", rexenv_lib::core::db::DbEngine::Mysql.port()),
        &mysql_base,
        &Default::default(),
    )
    .expect("install wordpress");

    // Subdirectory network (sub-sites are paths — no extra DNS), then reload edge.
    sites::convert_multisite(&conn, &php, &wp, &docroot, &site.id, MultisiteMode::Subdirectory)
        .expect("convert").expect("site exists");
    let sites_now = sites::list(&conn).unwrap();
    let checks = mgr.reload(&*plat, &ca, &sites_now).await.expect("reload");
    service_manager::await_ready(checks).await.expect("backends ready");
    println!("✓ subdirectory network ready");

    // 1) Create a sub-site + list.
    let blog_id = wordpress::network_site_create(&php, &wp, &docroot, "team").expect("create sub-site");
    println!("network_site_create(team) → blog_id {}", blog_id.trim());
    let listed = wordpress::network_site_list(&php, &wp, &docroot).expect("site list");
    println!("network_site_list → {} site(s): {:?}", listed.len(),
        listed.iter().map(|s| (s.id.clone(), s.url.clone())).collect::<Vec<_>>());
    assert!(listed.len() >= 2, "sub-site not listed");
    assert!(listed.iter().any(|s| s.url.contains("/team")), "team sub-site missing from list");
    println!("✓ sub-site created + appears in `wp site list`");

    // 2) The sub-site is served (recognized + routed through the subdirectory rewrite).
    let ca_cert = reqwest::Certificate::from_pem(&std::fs::read(&ca.cert_path).unwrap()).unwrap();
    let addr: SocketAddr = format!("127.0.0.1:{HTTPS}").parse().unwrap();
    let client = reqwest::Client::builder()
        .add_root_certificate(ca_cert)
        .resolve(domain, addr)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let resp = client.get(format!("https://{domain}:{HTTPS}/team/")).send().await.expect("GET sub-site");
    let code = resp.status().as_u16();
    println!("GET https://{domain}/team/ → {code}");
    assert!(code < 500, "sub-site not served (got {code})");
    println!("✓ /team/ served (HTTP {code}) through the subdirectory rewrite");

    // 3) Network-activate a bundled plugin → status becomes `active-network` (badge).
    wordpress::plugin_activate_network(&php, &wp, &docroot, &["akismet".into()]).expect("network activate");
    let plugins = wordpress::plugin_list(&php, &wp, &docroot).expect("plugin list");
    let akismet = plugins.iter().find(|p| p.name == "akismet").expect("akismet present");
    println!("akismet status after --network = {}", akismet.status);
    assert_eq!(akismet.status, "active-network", "akismet not network-active");
    println!("✓ plugin network-activated → `active-network` (Network-active badge)");

    // 4) Super-admins: create a user, grant super-admin, list includes it.
    let _ = wordpress::user_create(&php, &wp, &docroot, "boss", "boss@mysite.test", "administrator");
    wordpress::super_admin_add(&php, &wp, &docroot, "boss").expect("super-admin add");
    let supers = wordpress::super_admin_list(&php, &wp, &docroot).expect("super-admin list");
    println!("super_admin_list → {supers:?}");
    assert!(supers.iter().any(|u| u == "boss"), "boss not a super admin");
    println!("✓ super-admin granted + listed");

    // 5) Delete the sub-site → list shrinks back.
    wordpress::network_site_delete(&php, &wp, &docroot, blog_id.trim()).expect("delete sub-site");
    let after = wordpress::network_site_list(&php, &wp, &docroot).expect("site list");
    println!("after delete → {} site(s)", after.len());
    assert!(!after.iter().any(|s| s.url.contains("/team") && !s.deleted),
        "team sub-site still active after delete");
    println!("✓ sub-site deleted (removed from the active network list)");

    mgr.stop_all(&*plat).unwrap();
    println!("\nALL GOOD — network sub-site CRUD, network-activate (active-network badge), and super-admins all work.");
}

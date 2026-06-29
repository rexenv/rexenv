//! Phase-3 §11.3 check: site blueprints (store CRUD + apply-on-create).
//!
//! Verifies:
//!   - migration v4 seeds the example blueprints + store CRUD round-trips;
//!   - applying a blueprint to a freshly-installed WordPress site does the real
//!     "site setup" automation — installs + activates its plugin, turns on
//!     WP_DEBUG, and converts to multisite — the same path `create_site` runs.
//!
//! Run (ports 8443/8080/8088/9783/13306/1025/8025 free): `cargo run --example blueprint_check`

use rexenv_lib::core::service_manager::{Ports, ServiceManager};
use rexenv_lib::core::{binaries, blueprints, services, sites, ssl, wordpress};
use rexenv_lib::platform;
use rexenv_lib::state::models::{
    Blueprint, BlueprintItem, BlueprintSpec, MultisiteMode, NewSite, SiteType, WebServer,
};
use rexenv_lib::state::{db, store};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

const HTTPS: u16 = 8443;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let domain = "bpsite.test";

    let conn = {
        let p = std::env::temp_dir().join("rexenv-11_3.db");
        let _ = std::fs::remove_file(&p);
        db::open(&p).unwrap()
    };

    // 1) Seeds present + store CRUD round-trip.
    let seeded = store::list_blueprints(&conn).unwrap();
    println!("seeded blueprints: {:?}", seeded.iter().map(|b| b.id.clone()).collect::<Vec<_>>());
    assert!(seeded.iter().any(|b| b.id == "seed-woocommerce"), "woocommerce seed missing");
    assert!(seeded.iter().any(|b| b.id == "seed-multisite"), "multisite seed missing");

    let custom = Blueprint {
        id: "bp-test".into(),
        name: "Test blueprint".into(),
        spec: BlueprintSpec {
            site_type: SiteType::Wordpress,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            multisite: MultisiteMode::Subdirectory,
            plugins: vec![BlueprintItem { slug: "hello-dolly".into(), activate: true }],
            themes: vec![],
            wp_debug: true,
            language: String::new(),
        },
    };
    store::upsert_blueprint(&conn, &custom).unwrap();
    let got = store::get_blueprint(&conn, "bp-test").unwrap().expect("saved");
    assert_eq!(got.name, "Test blueprint");
    assert_eq!(got.spec.plugins[0].slug, "hello-dolly");
    assert!(matches!(got.spec.multisite, MultisiteMode::Subdirectory));
    println!("✓ seeds present + blueprint CRUD round-trips (spec JSON parsed back)");

    // 2) Apply the blueprint to a real WordPress site (the create_site path).
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();
    sites::provision(
        &conn,
        &*plat,
        &ca,
        NewSite {
            name: "BP Site".into(),
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
    {
        let mysql_base = binaries::resolve_dir(&*plat, "mysql", binaries::MYSQL_VERSION).await.unwrap();
        let _ = Command::new(mysql_base.join("bin/mysql"))
            .args(["-h127.0.0.1", "-P13306", "-uroot", "-e",
                &format!("DROP DATABASE IF EXISTS {}", wordpress::db_name_for(domain))])
            .status();
        let _ = std::fs::remove_dir_all(&docroot);
        std::fs::create_dir_all(&docroot).unwrap();
    }
    wordpress::install_for_site(
        &php, &wp, &docroot, domain, "BP Site",
        &format!("127.0.0.1:{}", rexenv_lib::core::db::DbEngine::Mysql.port()),
        &Default::default(),
    )
    .expect("install wordpress");

    // Apply the WP parts (plugins/themes/WP_DEBUG), then multisite (create_site order).
    let applied = blueprints::apply_wordpress(&php, &wp, &docroot, &got.spec).expect("apply");
    println!("applied: {applied:?}");
    assert_eq!(applied.plugins_installed, 1);
    assert!(applied.wp_debug_set);
    sites::convert_multisite(&conn, &php, &wp, &docroot, &site.id, got.spec.multisite)
        .expect("convert").expect("site");

    // Assert the outcomes the blueprint promised.
    let plugins = wordpress::plugin_list(&php, &wp, &docroot).expect("plugin list");
    let hello = plugins.iter().find(|p| p.name == "hello-dolly").expect("hello-dolly installed");
    println!("hello-dolly status = {}", hello.status);
    assert!(hello.status.starts_with("active"), "blueprint plugin not active");
    assert!(wordpress::wp_debug_get(&php, &wp, &docroot).unwrap(), "WP_DEBUG not on");
    assert!(wordpress::wp_info(&php, &wp, &docroot).unwrap().multisite, "site not multisite");
    println!("✓ blueprint applied: hello-dolly active + WP_DEBUG on + subdirectory multisite");

    mgr.stop_all(&*plat).unwrap();
    println!("\nALL GOOD — blueprints persist, round-trip, and one-click apply installs plugins, sets WP_DEBUG, and converts to multisite.");
}

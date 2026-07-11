//! Phase-3 §7.2 check: WordPress Tools via WP-CLI.
//! Brings up MySQL, installs a real WP site, then:
//!   - toggles WP_DEBUG and confirms `wp config get WP_DEBUG` reflects it;
//!   - runs search-replace --dry-run (reports N rows, DB UNCHANGED) then for real
//!     (changes them); confirms the option value flips only on the real run;
//!   - flushes permalinks (`wp rewrite flush`).
//!
//! Run (MySQL :13306 free): `cargo run --example wp_tools_check`

use rexenv_lib::core::{binaries, database, sites, ssl, wordpress};
use rexenv_lib::platform;
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::time::Duration;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let domain = "wptools.test";

    let conn = {
        let p = std::env::temp_dir().join("rexenv-7_2.db");
        let _ = std::fs::remove_file(&p);
        db::open(&p).unwrap()
    };
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();
    let php = binaries::resolve(&*plat, "php", binaries::PHP_VERSION).await.unwrap();
    let wp = binaries::resolve_file(&*plat, "wp-cli", binaries::WP_CLI_VERSION).await.unwrap();
    let mysql_base = binaries::resolve_dir(&*plat, "mysql", binaries::MYSQL_VERSION).await.unwrap();

    let datadir = database::data_dir(&*plat).unwrap();
    let socket = database::socket_path(&*plat).unwrap();
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    database::initialize(&*plat, &mysql_base, &datadir).unwrap();
    let mut mysqld =
        database::start(&*plat, &mysql_base, &datadir, database::MYSQL_PORT, &socket).unwrap();
    for _ in 0..30 {
        if database::mysql_running(database::MYSQL_PORT) {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    let site = sites::provision(
        &conn,
        &*plat,
        &ca,
        NewSite {
            name: "WP Tools".into(),
            domain: domain.into(),
            site_type: SiteType::Wordpress,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            path: String::new(),
        },
    )
    .unwrap();
    let docroot = std::path::PathBuf::from(&site.path);
    wordpress::install_for_site(
        &php,
        &wp,
        &docroot,
        domain,
        "WP Tools",
        &wordpress::db_name_for(domain),
        &format!("127.0.0.1:{}", database::MYSQL_PORT),
        &mysql_base,
        &Default::default(),
    )
    .expect("install wordpress");

    // WP_DEBUG toggle.
    let before = wordpress::wp_debug_get(&php, &wp, &docroot).unwrap();
    wordpress::wp_debug_set(&php, &wp, &docroot, true).unwrap();
    assert!(wordpress::wp_debug_get(&php, &wp, &docroot).unwrap(), "WP_DEBUG didn't turn on");
    wordpress::wp_debug_set(&php, &wp, &docroot, false).unwrap();
    assert!(!wordpress::wp_debug_get(&php, &wp, &docroot).unwrap(), "WP_DEBUG didn't turn off");
    println!("✓ WP_DEBUG toggles (was {before}, on→off verified via `wp config get`)");

    // search-replace: dry-run reports rows WITHOUT changing; real run changes them.
    let siteurl = || wordpress::wp_run(&php, &wp, &docroot, &["option", "get", "siteurl"]).unwrap();
    let original = siteurl();
    println!("siteurl before = {original}");
    let dry = wordpress::search_replace(&php, &wp, &docroot, domain, "changed.test", true).unwrap();
    println!("dry-run replacements = {dry}");
    assert!(dry > 0, "dry-run found no rows to change");
    assert_eq!(siteurl(), original, "dry-run MUTATED the DB (it must not)");
    println!("✓ dry-run reports {dry} rows, DB unchanged");

    let real = wordpress::search_replace(&php, &wp, &docroot, domain, "changed.test", false).unwrap();
    println!("real replacements = {real}");
    assert!(real > 0, "real run changed nothing");
    assert!(siteurl().contains("changed.test"), "real run didn't change siteurl");
    println!("✓ real run changed {real} rows (siteurl now {})", siteurl());

    // Permalinks flush.
    wordpress::rewrite_flush(&php, &wp, &docroot).expect("rewrite flush");
    println!("✓ rewrite flush ran");

    let _ = database::stop(&*plat, mysqld.id());
    let _ = mysqld.wait();
    println!("\nALL GOOD — WP_DEBUG toggle, search-replace (dry vs real), and permalink flush work.");
}

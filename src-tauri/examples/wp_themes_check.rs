//! Phase-3 §6.2 check: WordPress theme management via WP-CLI.
//! Brings up MySQL, installs a real WP site, then drives the theme API:
//!   - install `twentytwenty` by slug → it appears in theme_list,
//!   - activate it → its `status` flips to `active` (and the old active flips off),
//!   - delete a non-active theme → it disappears.
//!
//! Run (MySQL :13306 free): `cargo run --example wp_themes_check`

use rexenv_lib::core::{binaries, database, sites, ssl, wordpress};
use rexenv_lib::platform;
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use std::time::Duration;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let domain = "wpthemes.test";

    let conn = {
        let p = std::env::temp_dir().join("rexenv-6_2.db");
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
            name: "WP Themes".into(),
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
        "WP Themes",
        &wordpress::db_name_for(domain),
        &format!("127.0.0.1:{}", database::MYSQL_PORT),
        &mysql_base,
        &Default::default(),
    )
    .expect("install wordpress");

    let status = |list: &[wordpress::WpTheme], name: &str| {
        list.iter().find(|t| t.name == name).map(|t| t.status.clone())
    };
    const NEW: &str = "twentytwenty";

    // Install a theme by slug (not active yet).
    wordpress::theme_install(&php, &wp, &docroot, NEW, false).expect("install theme");
    let list = wordpress::theme_list(&php, &wp, &docroot, false).expect("list");
    println!("after install: {NEW} status = {:?}", status(&list, NEW));
    assert_eq!(status(&list, NEW).as_deref(), Some("inactive"), "theme not installed/inactive");

    // Activate it → flips to active; whatever was active flips off.
    let prev_active = list.iter().find(|t| t.status == "active").map(|t| t.name.clone());
    wordpress::theme_activate(&php, &wp, &docroot, NEW).expect("activate");
    let list = wordpress::theme_list(&php, &wp, &docroot, false).unwrap();
    assert_eq!(status(&list, NEW).as_deref(), Some("active"), "theme not activated");
    if let Some(prev) = &prev_active {
        assert_ne!(status(&list, prev).as_deref(), Some("active"), "old theme still active");
    }
    println!("✓ install by slug + activate → Active (old active {prev_active:?} flipped off)");

    // Delete a non-active theme.
    if let Some(victim) = list.iter().find(|t| t.status != "active").map(|t| t.name.clone()) {
        wordpress::theme_delete(&php, &wp, &docroot, std::slice::from_ref(&victim)).expect("delete");
        let list = wordpress::theme_list(&php, &wp, &docroot, false).unwrap();
        assert!(!list.iter().any(|t| t.name == victim), "{victim} still present");
        println!("✓ delete non-active theme '{victim}' → gone ({} themes remain)", list.len());
    }

    let _ = database::stop(&*plat, mysqld.id());
    let _ = mysqld.wait();
    println!("\nALL GOOD — theme install/activate/delete reflect in wp theme list.");
}

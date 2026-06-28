//! Phase-3 §1.1 check: the WP-CLI JSON bridge + WP detection.
//! Brings up MySQL, installs a real WordPress site, then exercises the bridge:
//!   - `wordpress::wp_info` returns isWordpress/version/multisite,
//!   - `wordpress::wp_run` returns an `wp option get siteurl` value,
//!   - `wordpress::wp_json` (typed JSON runner) parses `wp plugin list`.
//! Finally a Blank-PHP docroot must report `isWordpress: false`.
//!
//! Run (MySQL port :13306 must be free): `cargo run --example wp_info_check`

use rexenv_lib::core::{binaries, database, sites, ssl, wordpress};
use rexenv_lib::platform;
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Deserialize)]
struct PluginRow {
    name: String,
    status: String,
}

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let domain = "wpinfo.test";
    let url = format!("https://{domain}");

    let conn = {
        let p = std::env::temp_dir().join("rexenv-3_1_1.db");
        let _ = std::fs::remove_file(&p);
        db::open(&p).unwrap()
    };
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();

    let php = binaries::resolve(&*plat, "php", binaries::PHP_VERSION).await.unwrap();
    let wp = binaries::resolve_file(&*plat, "wp-cli", binaries::WP_CLI_VERSION).await.unwrap();
    let mysql_base = binaries::resolve_dir(&*plat, "mysql", binaries::MYSQL_VERSION).await.unwrap();

    // MySQL (needed to install WordPress).
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
    println!("mysql running={}", database::mysql_running(database::MYSQL_PORT));

    // Provision + one-click install a real WordPress site.
    let site = sites::provision(
        &conn,
        &*plat,
        &ca,
        NewSite {
            name: "WP Info Check".into(),
            domain: domain.into(),
            site_type: SiteType::Wordpress,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            path: String::new(),
        },
    )
    .unwrap();
    let docroot = std::path::PathBuf::from(&site.path);
    wordpress::install_wordpress(
        &php,
        &wp,
        &wordpress::WpInstall {
            docroot: &docroot,
            db_name: &wordpress::db_name_for(domain),
            db_host: &format!("127.0.0.1:{}", database::MYSQL_PORT),
            url: &url,
            title: "WP Info Check",
            admin_user: "admin",
            admin_password: "rexenv-admin-pw",
            admin_email: "admin@wpinfo.test",
        },
    )
    .expect("install wordpress");

    // 1) wp_info on the real WordPress site.
    let info = wordpress::wp_info(&php, &wp, &docroot).expect("wp_info");
    println!("wp_info(WordPress) = {info:?}");

    // 2) text bridge: an `wp option get` value.
    let siteurl = wordpress::wp_run(&php, &wp, &docroot, &["option", "get", "siteurl"]).unwrap();
    println!("wp option get siteurl = {siteurl}");

    // 3) typed JSON runner: `wp plugin list`.
    let plugins: Vec<PluginRow> =
        wordpress::wp_json(&php, &wp, &docroot, &["plugin", "list"]).expect("wp_json plugin list");
    println!("wp plugin list ({} plugins):", plugins.len());
    for p in &plugins {
        println!("  - {} ({})", p.name, p.status);
    }

    // 4) Blank-PHP docroot must report isWordpress:false.
    let blank = std::env::temp_dir().join("rexenv-blank-docroot");
    std::fs::create_dir_all(&blank).unwrap();
    std::fs::write(blank.join("index.php"), "<?php phpinfo();\n").unwrap();
    let blank_info = wordpress::wp_info(&php, &wp, &blank).expect("wp_info blank");
    println!("wp_info(Blank-PHP) = {blank_info:?}");

    let _ = database::stop(&*plat, mysqld.id());
    let _ = mysqld.wait();

    let ok = info.is_wordpress
        && info.version.is_some()
        && !siteurl.trim().is_empty()
        && !plugins.is_empty()
        && !blank_info.is_wordpress;
    if ok {
        println!("\nOK — JSON bridge returns version + option value + parsed plugin list; Blank-PHP = not WordPress.");
    } else {
        eprintln!("\nFAILED — info={info:?} siteurl={siteurl:?} plugins={} blank={blank_info:?}", plugins.len());
        std::process::exit(1);
    }
}

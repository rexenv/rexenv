//! Phase-3 §1.1 check: the WP-CLI JSON bridge + WP detection.
//! Brings up MySQL, installs a real WordPress site, then exercises the bridge:
//!   - `wordpress::wp_info` returns isWordpress/version/multisite,
//!   - `wordpress::wp_run` returns an `wp option get siteurl` value,
//!   - `wordpress::wp_json` (typed JSON runner) parses `wp plugin list`.
//!
//! Then a Blank-PHP docroot must report `isWordpress: false` — and, with MySQL
//! STOPPED, the real site must still report `isWordpress: true` + a version:
//! `core is-installed` exits 1 for a down database exactly as it does for a
//! non-WordPress path, and reading the exit code alone made every WordPress
//! site not-WordPress while rexenv's stack was stopped (5 Sep 2026).
//!
//! Run (MySQL port :13306 must be free): `cargo run --example wp_info_check`

use rexenv_lib::core::{binaries, database, sites, ssl, wordpress};
use rexenv_lib::platform;
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use serde::Deserialize;
use std::time::Duration;

mod common;

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
    // Fixture-owned docroot. `sites::provision` reads the `sites_dir` SETTING,
    // which falls back to a path derived from $HOME — so without this the site
    // lands in the user's real ~/rexenv/Sites and SURVIVES into the next run.
    // That is not untidy, it is the bug: two checks failed on their own
    // leftovers on 21 Aug 2026 (`wp_tools_check`, `wp_themes_check`).
    let _sites_dir = common::pin_fixture_sites_dir(&conn, "wpinfo");
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();

    let php = binaries::resolve(&*plat, "php", binaries::pins().php).await.unwrap();
    let wp = binaries::resolve_file(&*plat, "wp-cli", binaries::pins().wp_cli).await.unwrap();
    let mysql_base = binaries::resolve_dir(&*plat, "mysql", binaries::pins().mysql).await.unwrap();
    let (db_client, _) = rexenv_lib::core::db::DbEngine::Mysql
        .sql_client_bins(&*plat, rexenv_lib::core::binaries::pins().mysql)
        .await
        .expect("bundled MySQL client");

    // MySQL (needed to install WordPress).
    let datadir = database::data_dir(&*plat).unwrap();
    let socket = database::socket_path(&*plat).unwrap();
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    database::initialize(&*plat, &mysql_base, &datadir).unwrap();
    let mut mysqld = common::OwnedService::new(
        database::start(&*plat, &mysql_base, &datadir, database::MYSQL_PORT, &socket).unwrap(),
        "mysqld",
    );
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
            db_engine: rexenv_lib::state::models::SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
            git_migrate: true,
            git_build_assets: false,
            starter_db: false,
        },
    )
    .unwrap();
    let docroot = std::path::PathBuf::from(&site.path);
    wordpress::install_wordpress(
        &php,
        &wp,
        &wordpress::WpInstall {
            docroot: &docroot,
            db_name: &wordpress::db_name_for(SiteType::Wordpress, domain),
            db_host: &format!("127.0.0.1:{}", database::MYSQL_PORT),
            db_client: &db_client,
            url: &url,
            title: "WP Info Check",
            admin_user: "admin",
            admin_password: "rexenv-admin-pw",
            admin_email: "admin@wpinfo.test",
            locale: "",
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

    // 5) The database server DOWN: the files are still WordPress. This is the
    //    state every site is in while the stack is stopped; the pre-fix answer
    //    here was `is_wordpress: false`.
    mysqld.stop();
    for _ in 0..30 {
        if !database::mysql_running(database::MYSQL_PORT) {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    println!("mysql running={} (stopped on purpose)", database::mysql_running(database::MYSQL_PORT));
    let down_info = wordpress::wp_info(&php, &wp, &docroot).expect("wp_info with the database down");
    println!("wp_info(WordPress, database down) = {down_info:?}");

    let ok = info.is_wordpress
        && info.version.is_some()
        && !siteurl.trim().is_empty()
        && !plugins.is_empty()
        && !blank_info.is_wordpress
        && down_info.is_wordpress
        && down_info.version == info.version;
    if ok {
        println!(
            "\nOK — JSON bridge returns version + option value + parsed plugin list; Blank-PHP = not WordPress; \
             database down = still WordPress."
        );
    } else {
        eprintln!(
            "\nFAILED — info={info:?} siteurl={siteurl:?} plugins={} blank={blank_info:?} down={down_info:?}",
            plugins.len()
        );
        std::process::exit(1);
    }
}

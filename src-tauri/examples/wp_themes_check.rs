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

mod common;

#[tokio::main]
async fn main() {
    // FIRST statement: this example starts a real mysqld on the production
    // port. Without this it silently BORROWS whatever is already there —
    // including a corpse left by another example, which on 14 Aug 2026 gave
    // "ERROR 3680: Failed to create schema directory (errno 2)" and got three
    // innocent examples blamed for it.
    common::require_ports_free(&[(database::MYSQL_PORT, "MySQL")]);

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
    let (db_client, _) = rexenv_lib::core::db::DbEngine::Mysql
        .sql_client_bins(&*plat, rexenv_lib::core::binaries::MYSQL_VERSION)
        .await
        .expect("bundled MySQL client");

    let datadir = database::data_dir(&*plat).unwrap();
    let socket = database::socket_path(&*plat).unwrap();
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    database::initialize(&*plat, &mysql_base, &datadir).unwrap();
    let mysqld =
        database::start(&*plat, &mysql_base, &datadir, database::MYSQL_PORT, &socket).unwrap();
    // Drop-owned, like `wp_plugins_check`: this example had the raw child, so
    // the assertion panic on 19 Aug 2026 left mysqld holding :13306 and the
    // NEXT run refused to start — the corpse-mysqld incident of 14 Aug, again,
    // in the one example that had not been converted.
    let mut mysqld = common::OwnedService::new(mysqld, "mysqld");
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
            db_engine: rexenv_lib::state::models::SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
            git_migrate: true,
            git_build_assets: false,
        },
    )
    .unwrap();
    let docroot = std::path::PathBuf::from(&site.path);
    // The docroot is rebuilt every run; the DATABASE was not, and that is what
    // broke this check on 19 Aug 2026 — five days after the previous run. The
    // example ends with `twentytwenty` ACTIVE, so the surviving schema still
    // said `stylesheet = twentytwenty`, and the freshly installed theme came
    // back `active` where the first assertion demands `inactive`. A fixture is
    // only a fixture if the run owns ALL of it, so this run drops its own
    // database first rather than inheriting the last one's opinions.
    //
    // Guarded rather than trusted: the name is derived, and a future rename of
    // the fixture domain must not turn this into a DROP of something a person
    // owns (the July incident where an example's derived path took out the
    // whole Sites folder).
    let db_name = wordpress::db_name_for(SiteType::Wordpress, domain);
    assert_eq!(db_name, "wp_wpthemes_test", "the fixture database name drifted — refusing to drop");
    database::drop_database(&db_client, database::MYSQL_PORT, &db_name).expect("drop fixture db");

    wordpress::install_for_site(
        &php,
        &wp,
        &docroot,
        domain,
        "WP Themes",
        &wordpress::db_name_for(SiteType::Wordpress, domain),
        &format!("127.0.0.1:{}", database::MYSQL_PORT),
        &db_client,
        &Default::default(),
    )
    .expect("install wordpress");

    let status = |list: &[wordpress::WpTheme], name: &str| {
        list.iter().find(|t| t.name == name).map(|t| t.status.clone())
    };
    const NEW: &str = "twentytwenty";

    // Install a theme by slug (not active yet; fixture seeding — captured
    // wp_cli; the app's install paths are all streamed jobs now).
    let path_arg = format!("--path={}", docroot.display());
    let out = wordpress::wp_cli(&php, &wp, &["theme", "install", NEW, &path_arg], None)
        .expect("install theme");
    assert!(out.status.success(), "install {NEW}: {}", String::from_utf8_lossy(&out.stderr));
    let list = wordpress::theme_list(&php, &wp, &docroot, "wp-content", false).expect("list");
    println!("after install: {NEW} status = {:?}", status(&list, NEW));
    // The card is labelled with the theme's own name, so the field has to
    // arrive from wp-cli — `title` is NOT in its default set for themes, which
    // is the same silent-empty trap `update_version` fell into. A lib test can
    // only prove the captured shape parses; this is the half that proves the
    // ARGV still asks for it.
    let row = list.iter().find(|t| t.name == NEW).expect("the theme we just installed");
    assert!(!row.title.is_empty(), "{NEW} came back with no title — the field list dropped it");
    assert_ne!(
        row.title, row.name,
        "the title equals the slug, so this assertion could not tell a populated field \
         from a fallback"
    );
    println!("✓ title arrives from wp-cli: {:?} (slug {:?})", row.title, row.name);
    assert_eq!(status(&list, NEW).as_deref(), Some("inactive"), "theme not installed/inactive");

    // Activate it → flips to active; whatever was active flips off.
    let prev_active = list.iter().find(|t| t.status == "active").map(|t| t.name.clone());
    wordpress::theme_activate(&php, &wp, &docroot, NEW).expect("activate");
    let list = wordpress::theme_list(&php, &wp, &docroot, "wp-content", false).unwrap();
    assert_eq!(status(&list, NEW).as_deref(), Some("active"), "theme not activated");
    if let Some(prev) = &prev_active {
        assert_ne!(status(&list, prev).as_deref(), Some("active"), "old theme still active");
    }
    println!("✓ install by slug + activate → Active (old active {prev_active:?} flipped off)");

    // Delete a non-active theme.
    if let Some(victim) = list.iter().find(|t| t.status != "active").map(|t| t.name.clone()) {
        wordpress::theme_delete(&php, &wp, &docroot, std::slice::from_ref(&victim)).expect("delete");
        let list = wordpress::theme_list(&php, &wp, &docroot, "wp-content", false).unwrap();
        assert!(!list.iter().any(|t| t.name == victim), "{victim} still present");
        println!("✓ delete non-active theme '{victim}' → gone ({} themes remain)", list.len());
    }

    mysqld.stop();
    println!("\nALL GOOD — theme install/activate/delete reflect in wp theme list.");
}

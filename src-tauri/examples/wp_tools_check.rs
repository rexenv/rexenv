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
    let domain = "wptools.test";

    let conn = {
        let p = std::env::temp_dir().join("rexenv-7_2.db");
        let _ = std::fs::remove_file(&p);
        db::open(&p).unwrap()
    };
    // Fixture-owned docroot. Without the pin `sites::provision` reads the
    // `sites_dir` SETTING, which falls back to the home directory, so every run
    // left `wptools.test` in the user's real ~/rexenv/Sites — and the NEXT run
    // inherited it.
    let _sites_dir = common::pin_fixture_sites_dir(&conn, "wptools");
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
    // Drop-GUARDED: the assertions below panic, and a raw `Child` survives an
    // unwind — which is how a failing run left mysqld on :13306 and took nine
    // later examples down with it (21 Aug 2026).
    let mut mysqld = common::OwnedService::new(
        database::start(&*plat, &mysql_base, &datadir, database::MYSQL_PORT, &socket).unwrap(),
        "mysqld",
    );
    common::await_ready("mysqld (accepting queries)", None, || {
        database::mysql_running(database::MYSQL_PORT)
    });

    // Start from a database this run owns. `search-replace` below REWRITES the
    // site's rows (`wptools.test` → `changed.test`) and the previous run left
    // them rewritten, so run N+1 opened a site whose siteurl was already
    // `https://changed.test`, found nothing to replace, and failed on
    // "dry-run found no rows to change" — after which it could never pass again
    // without someone dropping the database by hand. A check that poisons its
    // own next run is worse than one that fails, because the second failure
    // looks like a different bug.
    //
    // Guarded rather than trusted, exactly as `wp_themes_check` guards its own:
    // the name is DERIVED, and a future rename of the fixture domain must not
    // turn this into a DROP of something a person owns (the July incident where
    // an example's derived path took out the whole Sites folder).
    let db_name = wordpress::db_name_for(SiteType::Wordpress, domain);
    assert_eq!(db_name, "wp_wptools_test", "the fixture database name drifted — refusing to drop");
    database::drop_database(&db_client, database::MYSQL_PORT, &db_name).expect("drop fixture db");

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
    common::install_wp(&php, &wp, &docroot, domain, "WP Tools", &db_client);

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
    let dry =
        wordpress::search_replace(&php, &wp, &docroot, domain, "changed.test", true, false).unwrap();
    println!("dry-run replacements = {dry}");
    assert!(dry > 0, "dry-run found no rows to change");
    assert_eq!(siteurl(), original, "dry-run MUTATED the DB (it must not)");
    println!("✓ dry-run reports {dry} rows, DB unchanged");

    let real =
        wordpress::search_replace(&php, &wp, &docroot, domain, "changed.test", false, false).unwrap();
    println!("real replacements = {real}");
    assert!(real > 0, "real run changed nothing");
    assert!(siteurl().contains("changed.test"), "real run didn't change siteurl");
    println!("✓ real run changed {real} rows (siteurl now {})", siteurl());

    // Permalinks flush.
    wordpress::rewrite_flush(&php, &wp, &docroot).expect("rewrite flush");
    println!("✓ rewrite flush ran");

    mysqld.stop();
    println!("\nALL GOOD — WP_DEBUG toggle, search-replace (dry vs real), and permalink flush work.");
}

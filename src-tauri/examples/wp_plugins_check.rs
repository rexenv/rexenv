//! Phase-3 §6.1 check: WordPress plugin management via WP-CLI.
//! Brings up MySQL, installs a real WP site, then drives the plugin API:
//!   - install `hello-dolly` by slug + activate → it shows `active` in plugin_list,
//!   - bulk-deactivate then delete → it disappears from plugin_list.
//!
//! Run (MySQL :13306 free): `cargo run --example wp_plugins_check`

use rexenv_lib::core::{binaries, database, sites, ssl, wordpress};
use rexenv_lib::platform;

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
    let domain = "wpplugins.test";

    let (conn, _dbf) = common::fixture_db("wp_plugins_check");
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
    // Drop-owned: an assertion panic below must not leave mysqld running.
    // The 14 Aug 2026 corpse-mysqld incident is what a leak here costs — the
    // NEXT example borrows the corpse and fails with an error naming nothing.
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
            name: "WP Plugins".into(),
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
    wordpress::install_for_site(
        &php,
        &wp,
        &docroot,
        domain,
        "WP Plugins",
        &wordpress::db_name_for(SiteType::Wordpress, domain),
        &format!("127.0.0.1:{}", database::MYSQL_PORT),
        &db_client,
        &Default::default(),
    )
    .expect("install wordpress");

    let has = |list: &[wordpress::WpPlugin], name: &str| list.iter().any(|p| p.name == name);
    let status = |list: &[wordpress::WpPlugin], name: &str| {
        list.iter().find(|p| p.name == name).map(|p| p.status.clone()).unwrap_or_default()
    };

    // Install hello-dolly by slug + activate (fixture seeding — captured
    // wp_cli; the app's install paths are all streamed jobs now).
    let path_arg = format!("--path={}", docroot.display());
    let out = wordpress::wp_cli(
        &php,
        &wp,
        &["plugin", "install", "hello-dolly", "--activate", &path_arg],
        None,
    )
    .expect("install hello-dolly");
    assert!(
        out.status.success(),
        "install hello-dolly: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let list = wordpress::plugin_list(&php, &wp, &docroot, false).expect("list");
    println!("after install+activate: hello-dolly status = {}", status(&list, "hello-dolly"));
    assert!(has(&list, "hello-dolly"), "hello-dolly not installed");
    assert_eq!(status(&list, "hello-dolly"), "active", "hello-dolly not active");
    println!("✓ installed by slug + activated → Active");

    // Bulk-deactivate (one call), confirm inactive.
    //
    // This assertion failed ONCE (14 Aug 2026) with deactivate returning Ok and
    // the plugin still `active`, and has not reproduced since — filed in
    // docs/TODO.md as an UNEXPLAINED failure, not a flake (the obvious
    // mechanisms were measured and refuted: wp_run checks exit status, and the
    // instrumented re-run showed deactivate doing what it claims). What that
    // sighting lacked was a capture, so a recurrence answered nothing. On a
    // mismatch this now dumps everything a diagnosis would need BEFORE
    // panicking: deactivate's own stdout, the parsed list, and a raw
    // `wp plugin list` re-read — the raw read is what separates "the parsed
    // list was stale" from "the plugin really is still active".
    let deactivate_out = wordpress::plugin_deactivate(&php, &wp, &docroot, &["hello-dolly".into()])
        .expect("deactivate");
    let list = wordpress::plugin_list(&php, &wp, &docroot, false).unwrap();
    let st = status(&list, "hello-dolly");
    if st != "inactive" {
        eprintln!("\nUNEXPLAINED-FAILURE CAPTURE (docs/TODO.md, wp_plugins_check 14 Aug 2026):");
        eprintln!("  deactivate returned Ok, status is {st:?} not \"inactive\"");
        eprintln!("  deactivate stdout: {deactivate_out:?}");
        eprintln!("  parsed list: {list:?}");
        match wordpress::wp_cli(&php, &wp, &["plugin", "list", "--format=csv", &path_arg], None) {
            Ok(o) => eprintln!(
                "  raw `wp plugin list` (exit {:?}):\n  stdout: {}\n  stderr: {}",
                o.status.code(),
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            ),
            Err(e) => eprintln!("  raw `wp plugin list` itself failed: {e}"),
        }
        panic!("not deactivated — capture above; append it to the TODO item");
    }
    println!("✓ bulk-deactivate → Inactive");

    // Delete, confirm gone.
    wordpress::plugin_delete(&php, &wp, &docroot, &["hello-dolly".into()]).expect("delete");
    let list = wordpress::plugin_list(&php, &wp, &docroot, false).unwrap();
    assert!(!has(&list, "hello-dolly"), "hello-dolly still present after delete");
    println!("✓ delete → gone ({} plugins remain)", list.len());

    mysqld.stop();
    println!("\nALL GOOD — plugin install/activate/deactivate/delete reflect in wp plugin list.");
}

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
        &database::mysql_client_bin(&mysql_base),
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
    wordpress::plugin_deactivate(&php, &wp, &docroot, &["hello-dolly".into()]).expect("deactivate");
    let list = wordpress::plugin_list(&php, &wp, &docroot, false).unwrap();
    assert_eq!(status(&list, "hello-dolly"), "inactive", "not deactivated");
    println!("✓ bulk-deactivate → Inactive");

    // Delete, confirm gone.
    wordpress::plugin_delete(&php, &wp, &docroot, &["hello-dolly".into()]).expect("delete");
    let list = wordpress::plugin_list(&php, &wp, &docroot, false).unwrap();
    assert!(!has(&list, "hello-dolly"), "hello-dolly still present after delete");
    println!("✓ delete → gone ({} plugins remain)", list.len());

    let _ = database::stop(&*plat, mysqld.id());
    let _ = mysqld.wait();
    println!("\nALL GOOD — plugin install/activate/deactivate/delete reflect in wp plugin list.");
}

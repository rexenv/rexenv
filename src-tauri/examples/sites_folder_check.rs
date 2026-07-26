//! Manual check for the configurable sites folder (task 10.2).
//! Sets a custom sites_dir, provisions a site, and confirms its docroot lands
//! under that folder. `cargo run --example sites_folder_check`.

use rexenv_lib::core::{sites, ssl};
use rexenv_lib::platform;
use rexenv_lib::state::models::{NewSite, SiteType, WebServer};
use rexenv_lib::state::{db, store};

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let custom = std::env::temp_dir().join("rexenv-custom-sites");
    let _ = std::fs::remove_dir_all(&custom);

    let db_path = std::env::temp_dir().join("rexenv-10_2.db");
    let _ = std::fs::remove_file(&db_path);
    let conn = db::open(&db_path).unwrap();
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).unwrap();

    println!("default sites_dir = {}", sites::sites_dir(&conn, &*plat).unwrap().display());
    store::set_setting(&conn, sites::SITES_DIR_KEY, custom.to_str().unwrap()).unwrap();
    println!("after set, sites_dir = {}", sites::sites_dir(&conn, &*plat).unwrap().display());

    let site = sites::provision(
        &conn,
        &*plat,
        &ca,
        NewSite {
            name: "Custom".into(),
            domain: "custom.test".into(),
            site_type: SiteType::Php,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            path: String::new(), db_engine: rexenv_lib::state::models::SiteDbEngine::Mysql,
        },
    )
    .unwrap();

    let path = std::path::Path::new(&site.path);
    println!("site docroot = {}", site.path);
    println!("under custom dir = {}", path.starts_with(&custom));
    println!("index.php present = {}", path.join("index.php").exists());

    // Clean up the test site (teardown should remove the docroot under the custom dir).
    let removed = sites::teardown(&conn, &*plat, &site.id).unwrap();
    println!(
        "teardown existed = {}; reported docroot_removed = {}; docroot gone = {}",
        removed.existed,
        removed.docroot_removed,
        !path.exists()
    );
}

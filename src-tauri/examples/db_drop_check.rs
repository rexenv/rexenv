//! Manual check for site-teardown DB cleanup: create a site-style database
//! (`wp_<domain>` via `db_name_for`), verify it exists, `drop_database` it,
//! verify it's gone. Uses (or starts) the shared loopback MySQL.
//! `cargo run --example db_drop_check`.

use rexenv_lib::core::{binaries, database, wordpress};
use rexenv_lib::state::models::SiteType;
use rexenv_lib::platform;
use std::process::Command;
use std::thread;
use std::time::Duration;

fn db_exists(client: &std::path::Path, port: u16, name: &str) -> bool {
    let out = Command::new(client)
        .args([
            "--no-defaults",
            "--protocol=TCP",
            "-h",
            "127.0.0.1",
            "-P",
            &port.to_string(),
            "-u",
            "root",
            "-N",
            "-e",
            &format!("SHOW DATABASES LIKE '{name}'"),
        ])
        .output()
        .expect("run mysql client");
    String::from_utf8_lossy(&out.stdout).contains(name)
}

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let port = database::MYSQL_PORT;

    let basedir = binaries::resolve_dir(&*plat, "mysql", binaries::MYSQL_VERSION)
        .await
        .expect("resolve mysql");
    let (db_client, _) = rexenv_lib::core::db::DbEngine::Mysql
        .sql_client_bins(&*plat, rexenv_lib::core::binaries::MYSQL_VERSION)
        .await
        .expect("bundled MySQL client");

    // Use a running server if there is one; otherwise start our own and stop it after.
    let mut started: Option<std::process::Child> = None;
    if !database::mysql_running(port) {
        let datadir = database::data_dir(&*plat).unwrap();
        let socket = database::socket_path(&*plat).unwrap();
        std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
        database::initialize(&*plat, &basedir, &datadir).expect("initialize");
        started = Some(database::start(&*plat, &basedir, &datadir, port, &socket).expect("start"));
        for _ in 0..30 {
            if database::mysql_running(port) {
                break;
            }
            thread::sleep(Duration::from_millis(500));
        }
    }
    assert!(database::mysql_running(port), "MySQL did not come up on {port}");

    // The exact name a site delete would derive.
    let name = wordpress::db_name_for(SiteType::Wordpress, "dropcheck.test");
    assert_eq!(name, "wp_dropcheck_test");

    database::create_database(&db_client, port, &name).expect("create");
    assert!(db_exists(db_client.path(), port, &name), "database missing after create");
    println!("created {name} ✓");

    database::drop_database(&db_client, port, &name).expect("drop");
    assert!(!db_exists(db_client.path(), port, &name), "database still there after drop");
    println!("dropped {name} ✓");

    // Dropping a nonexistent DB is a clean no-op (IF EXISTS).
    database::drop_database(&db_client, port, &name).expect("re-drop is a no-op");
    println!("re-drop no-op ✓");

    if let Some(child) = &started {
        let _ = database::stop(&*plat, child.id());
    }
    println!("db_drop_check: PASS");
}

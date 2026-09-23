//! Manual check for the MySQL service (task 8.1). Resolves MySQL, initializes a
//! datadir, starts mysqld on a loopback port, connects with the bundled client
//! (`SELECT VERSION()`), then stops. `cargo run --example mysql_serve`.

#[path = "common/mod.rs"]
mod common;

use rexenv_lib::core::{binaries, database};
use rexenv_lib::platform;
use std::process::Command;
use std::thread;
use std::time::Duration;

#[tokio::main]
async fn main() {
    // Refuse beside a live stack: these services would JOIN it, not collide
    // with it, and a connect-based readiness gate is satisfied by the user's
    // server. FIRST statement — after anything is spawned, exiting leaks it.
    common::require_stack_stopped();
    let plat = platform::current();
    let port = database::MYSQL_PORT;

    let basedir = binaries::resolve_dir(&*plat, "mysql", binaries::pins().mysql)
        .await
        .expect("resolve mysql");
    println!("mysql basedir: {}", basedir.display());

    let datadir = database::data_dir(&*plat).unwrap();
    let socket = database::socket_path(&*plat).unwrap();
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();

    print!("initializing datadir… ");
    database::initialize(&*plat, &basedir, &datadir).expect("initialize");
    println!("ok");

    let mut mysqld = common::OwnedService::new(
        database::start(&*plat, &basedir, &datadir, port, &socket).expect("start mysqld"),
        "mysqld",
    );
    println!("mysqld pid={}", mysqld.id());

    // Wait for it to accept connections (startup takes a few seconds).
    let mut up = false;
    for _ in 0..30 {
        if database::mysql_running(port) {
            up = true;
            break;
        }
        thread::sleep(Duration::from_millis(500));
    }
    println!("mysql_running={up}");

    // Connect with the bundled client.
    let out = Command::new(
        rexenv_lib::core::db::DbEngine::Mysql
            .sql_client_bins(&*plat, binaries::pins().mysql)
            .await
            .expect("bundled MySQL client")
            .0
            .path(),
    )
        .args([
            "--no-defaults",
            "--protocol=TCP",
            "-h",
            "127.0.0.1",
            "-P",
            &port.to_string(),
            "-u",
            "root",
            "--ssl-mode=DISABLED",
            "-e",
            "SELECT VERSION() AS version, 1+1 AS two;",
        ])
        .output()
        .expect("run mysql client");
    println!("client stdout:\n{}", String::from_utf8_lossy(&out.stdout));
    if !out.status.success() {
        eprintln!("client stderr:\n{}", String::from_utf8_lossy(&out.stderr));
    }

    mysqld.stop();
    thread::sleep(Duration::from_millis(500));
    println!("stopped; running={}", database::mysql_running(port));
}

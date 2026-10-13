//! L1 proof for ledger #858: the shared MySQL starts with the binary log OFF, and the
//! `binlog.*` files a server that ran WITH the log left behind are removed on start —
//! and nothing else is. Sandbox tier: its own datadir, port 13399, its own socket.
//! `cargo run --example mysql_binlog_check`.
//!
//! Background: MySQL 8.x defaults `log_bin=ON` with a 30-day expiry, and rexenv's
//! `--no-defaults` left no way to turn it off from a my.cnf — a user measured 60 GB of
//! binlog on a Mac (14 Oct 2026). The fix is `--skip-log-bin` in `database::server_args`
//! plus `database::remove_binlogs` right before the spawn. This example proves both
//! against the real bundled mysqld: a datadir seeded with a fake `binlog.index`,
//! `binlog.000001` and a decoy `binlog.000001.bak` starts cleanly, answers
//! `log_bin = OFF`, refuses `SHOW BINARY LOGS` (1381), and only the decoy survives.

#[path = "common/mod.rs"]
mod common;

use rexenv_lib::core::{binaries, database};
use std::process::Command;
use std::thread;
use std::time::Duration;

const PORT: u16 = 13399;

#[tokio::main]
async fn main() {
    let (plat, guard) = common::sandbox("binlog");
    let basedir = binaries::resolve_dir(&*plat, "mysql", binaries::pins().mysql)
        .await
        .expect("resolve mysql");
    println!("mysql basedir: {}", basedir.display());

    let datadir = guard.root().join("mysql").join("data");
    let socket = guard.root().join("run").join("mysql.sock");
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    print!("initializing sandbox datadir… ");
    database::initialize(&*plat, &basedir, &datadir).expect("initialize");
    println!("ok");

    // Seed what a server that ran with log_bin ON leaves behind, plus a decoy
    // that is NOT a binary log and must survive.
    std::fs::write(datadir.join("binlog.index"), "./binlog.000001\n").unwrap();
    std::fs::write(datadir.join("binlog.000001"), vec![0u8; 4096]).unwrap();
    std::fs::write(datadir.join("binlog.000001.bak"), b"decoy").unwrap();

    let mut mysqld = common::OwnedService::new(
        database::start(&*plat, &basedir, &datadir, PORT, &socket).expect("start mysqld"),
        "mysqld",
    );
    println!("mysqld pid={}", mysqld.id());

    let mut up = false;
    for _ in 0..40 {
        if database::mysql_running(PORT) {
            up = true;
            break;
        }
        thread::sleep(Duration::from_millis(500));
    }
    assert!(up, "mysqld did not accept connections on {PORT} within 20 s");

    // The seeded log files went; the decoy stayed.
    assert!(!datadir.join("binlog.index").exists(), "binlog.index survived the start");
    assert!(!datadir.join("binlog.000001").exists(), "binlog.000001 survived the start");
    assert!(datadir.join("binlog.000001.bak").exists(), "the decoy was deleted — the filter is too wide");
    // …and the running server wrote no new ones.
    let new_logs: Vec<String> = std::fs::read_dir(&datadir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| database::is_binlog_file(n))
        .collect();
    assert!(new_logs.is_empty(), "the server created binary logs: {new_logs:?}");

    let client = database::mysql_client_bin(&basedir);
    let q = |sql: &str| {
        Command::new(&client)
            .args(database::client_base_args(PORT))
            .args(["-N", "-e", sql])
            .output()
            .expect("run mysql client")
    };
    let out = q("SHOW VARIABLES LIKE 'log_bin'");
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    println!("log_bin: {text:?}");
    assert_eq!(text, "log_bin\tOFF", "log_bin is not OFF");

    let out = q("SHOW BINARY LOGS");
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    println!("SHOW BINARY LOGS: {err}");
    assert!(!out.status.success() && err.contains("1381"), "expected ERROR 1381 (not using binary logging)");

    // A clean start: the error log carries no [ERROR] line about the flag.
    let log = std::fs::read_to_string(plat.paths().log_dir().unwrap().join("mysql-error.log")).unwrap_or_default();
    let errors: Vec<&str> = log.lines().filter(|l| l.contains("[ERROR]")).collect();
    assert!(errors.is_empty(), "mysql-error.log has errors: {errors:?}");

    mysqld.stop();
    println!("mysql_binlog_check: all pass");
}

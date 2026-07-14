//! Manual check: MariaDB via a bottle bundle (TODO "Deferred services").
//! Run: `cargo run --example mariadb_bundle_check`
//!
//! Proves the mariadb bundle end to end on the REAL binary cache:
//!   1. `resolve_bundle("mariadb")` — three ghcr bottles (mariadb, openssl@3,
//!      pcre2) download + verify, merge, relink to `@loader_path`, re-sign.
//!   2. Every bundled Mach-O carries ONLY system / `@loader_path` load
//!      commands + a valid ad-hoc signature.
//!   3. First `DbEngine::Mariadb.start()` BOOTSTRAPS the datadir (mariadbd
//!      --bootstrap over stdin — no install-db script) then serves on :13307:
//!      `SELECT VERSION()` + a CREATE/DROP DATABASE round-trip through the
//!      bundled `mariadb` client, passwordless root over TCP (the MySQL model).
//! Cleans up its own child only — never touches the running stack.

use rexenv_lib::core::db::DbEngine;
use rexenv_lib::core::{binaries, mariadb, ports};
use rexenv_lib::platform;
use std::process::Command;
use std::thread;
use std::time::Duration;

fn load_commands_clean(path: &std::path::Path) -> bool {
    let out = Command::new("otool").arg("-L").arg(path).output().expect("otool");
    let listing = String::from_utf8_lossy(&out.stdout);
    let mut clean = true;
    for line in listing.lines().skip(1) {
        let Some(dep) = line.split_whitespace().next() else { continue };
        let ok = dep.starts_with("/usr/lib/")
            || dep.starts_with("/System/")
            || dep.starts_with("@loader_path/");
        if !ok {
            eprintln!("  UNRESOLVED load command in {}: {dep}", path.display());
            clean = false;
        }
    }
    clean
}

fn signature_valid(path: &std::path::Path) -> bool {
    Command::new("codesign")
        .args(["--verify", "--strict"])
        .arg(path)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn sql(basedir: &std::path::Path, port: u16, stmt: &str) -> (bool, String) {
    let out = Command::new(mariadb::mariadb_client_bin(basedir))
        .args([
            "--no-defaults",
            "--protocol=TCP",
            "--host=127.0.0.1",
            &format!("--port={port}"),
            "--user=root",
            "-N",
            "-B",
            "-e",
            stmt,
        ])
        .output()
        .expect("run mariadb client");
    (
        out.status.success(),
        if out.status.success() {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        } else {
            String::from_utf8_lossy(&out.stderr).trim().to_string()
        },
    )
}

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let mut ok = true;

    println!("=== resolve_bundle(mariadb {}) ===", binaries::MARIADB_VERSION);
    let basedir = binaries::resolve_bundle(&*plat, "mariadb", binaries::MARIADB_VERSION)
        .await
        .expect("resolve mariadb bundle");
    println!("  published at {}", basedir.display());

    println!("\n=== relink + signature audit ===");
    for rel in [
        "bin/mariadbd",
        "bin/mariadb",
        "bin/mariadb-dump",
        "lib/libssl.3.dylib",
        "lib/libcrypto.3.dylib",
        "lib/libpcre2-8.0.dylib",
    ] {
        let path = basedir.join(rel);
        let exists = path.is_file();
        let clean = exists && load_commands_clean(&path);
        let signed = exists && signature_valid(&path);
        println!("  {rel:<24} exists={exists} loads-clean={clean} signed={signed}");
        ok &= exists && clean && signed;
    }
    // Runtime share data must have made it into the bundle too.
    for rel in ["share/mysql/english/errmsg.sys", "share/mysql/charsets/Index.xml"] {
        let present = basedir.join(rel).is_file();
        println!("  {rel:<40} present={present}");
        ok &= present;
    }

    println!("\n=== DbEngine::Mariadb lifecycle (bootstrap + serve) ===");
    let port = DbEngine::Mariadb.port();
    if let Err(e) = ports::ensure_free(&*plat, port, ports::Proto::Tcp, "MariaDB") {
        eprintln!("  port {port} busy — {e}");
        std::process::exit(1);
    }
    let fresh = !mariadb::is_initialized(&mariadb::data_dir(&*plat).unwrap());
    println!("  datadir fresh (bootstrap will run) = {fresh}");
    let mut child = DbEngine::Mariadb.start(&*plat).await.expect("start mariadb");
    let pid = child.id();
    let mut up = false;
    for _ in 0..60 {
        if DbEngine::Mariadb.running() {
            up = true;
            break;
        }
        thread::sleep(Duration::from_millis(500));
    }
    println!("  pid {pid} · listening on :{port} = {up}");
    ok &= up;

    if up {
        let (vok, ver) = sql(&basedir, port, "SELECT VERSION()");
        println!("  SELECT VERSION() → {ver}");
        ok &= vok && ver.contains("MariaDB");
        let (cok, _) = sql(&basedir, port, "CREATE DATABASE IF NOT EXISTS rexenv_check");
        let (_, dbs) = sql(&basedir, port, "SHOW DATABASES LIKE 'rexenv_check'");
        let (dok, _) = sql(&basedir, port, "DROP DATABASE IF EXISTS rexenv_check");
        println!("  CREATE → {cok} · visible → {} · DROP → {dok}", dbs == "rexenv_check");
        ok &= cok && dbs == "rexenv_check" && dok;
    }

    let _ = DbEngine::Mariadb.stop(&*plat, pid);
    let _ = child.wait();
    println!("  stopped");

    if ok {
        println!("\nOK — mariadb bundle resolved + relinked + signed; bootstrap + serve + SQL round-trip.");
    } else {
        eprintln!("\nFAILED — see above.");
        std::process::exit(1);
    }
}

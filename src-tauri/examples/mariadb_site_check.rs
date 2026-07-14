//! Manual check: a WordPress SITE on the MariaDB engine, end to end (TODO
//! "Deferred services" — site→engine selection).
//! Run: `cargo run --example mariadb_site_check`
//!
//! Exercises the engine-aware site DB path exactly as `create_site` does for a
//! `db_engine: mariadb` site — but against a TEMP docroot and NO app site row,
//! so the user's real sites/configs are untouched:
//!   1. `DbEngine::Mariadb.start()` (bootstrap-on-first-run) on :13307.
//!   2. `install_for_site` with the bundle's `mariadb` client + DB_HOST
//!      `127.0.0.1:13307` — proves create-db + `wp core install` over the
//!      MariaDB wire (WordPress itself connects via php mysqli).
//!   3. `wp option get siteurl` through PHP → MariaDB round-trip.
//!   4. `export_to_downloads` via `mariadb-dump`, then import the dump back.
//!   5. `reset_site` against the MariaDB port (drop + reinstall).
//!   6. Drop the check database, stop the engine child, remove the docroot.

use rexenv_lib::core::db::DbEngine;
use rexenv_lib::core::{binaries, database, ports, wordpress};
use rexenv_lib::platform;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

const DOMAIN: &str = "mdbcheck.rex";
const DB: &str = "wp_mdbcheck_rex";

fn wp(php_bin: &Path, phar: &Path, docroot: &Path, args: &[&str]) -> (bool, String) {
    let mut cmd = Command::new(php_bin);
    cmd.arg("-d").arg("memory_limit=512M").arg(phar);
    cmd.args(args);
    cmd.arg(format!("--path={}", docroot.display()));
    let out = cmd.output().expect("run wp-cli");
    (
        out.status.success(),
        String::from_utf8_lossy(if out.status.success() { &out.stdout } else { &out.stderr })
            .trim()
            .to_string(),
    )
}

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let mut ok = true;

    // Tools: default-pin PHP CLI, WP-CLI, the mariadb bundle.
    let php_bin = binaries::resolve(&*plat, "php", binaries::PHP_VERSION).await.expect("php");
    let phar = binaries::resolve_file(&*plat, "wp-cli", binaries::WP_CLI_VERSION)
        .await
        .expect("wp-cli");
    let engine = DbEngine::Mariadb;
    let (client, dump_bin) = engine.sql_client_bins(&*plat, engine.default_version()).await.expect("mariadb bins");

    println!("=== engine up (bootstrap on first run) ===");
    if let Err(e) = ports::ensure_free(&*plat, engine.port(), ports::Proto::Tcp, "MariaDB") {
        eprintln!("  port {} busy — {e}", engine.port());
        std::process::exit(1);
    }
    let mut child = engine.start(&*plat, engine.default_version()).await.expect("start mariadb");
    let pid = child.id();
    for _ in 0..60 {
        if engine.running() {
            break;
        }
        thread::sleep(Duration::from_millis(500));
    }
    println!("  pid {pid} · listening on :{} = {}", engine.port(), engine.running());
    ok &= engine.running();

    // Temp docroot — never the user's sites dir.
    let docroot = std::env::temp_dir().join("rexenv-mariadb-site-check");
    let _ = std::fs::remove_dir_all(&docroot);
    std::fs::create_dir_all(&docroot).unwrap();

    println!("\n=== install_for_site against 127.0.0.1:{} ===", engine.port());
    let db_host = format!("127.0.0.1:{}", engine.port());
    let install = wordpress::install_for_site(
        &php_bin,
        &phar,
        &docroot,
        DOMAIN,
        "MariaDB Check",
        DB,
        &db_host,
        &client,
        &Default::default(),
    );
    println!("  install → {:?}", install.as_ref().map(|_| "ok").map_err(|e| e.to_string()));
    ok &= install.is_ok();

    if install.is_ok() {
        let (iok, _) = wp(&php_bin, &phar, &docroot, &["core", "is-installed"]);
        let (sok, siteurl) = wp(&php_bin, &phar, &docroot, &["option", "get", "siteurl"]);
        println!("  is-installed={iok} · siteurl → {siteurl}");
        ok &= iok && sok && siteurl == format!("https://{DOMAIN}");

        println!("\n=== export via mariadb-dump + import back ===");
        match database::export_to_downloads(&dump_bin, engine.port(), DOMAIN, DB) {
            Ok(path) => {
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                println!("  exported {} ({size} bytes)", path.display());
                let import = database::import_from_file(&client, engine.port(), DB, &path);
                println!("  re-import → {:?}", import.as_ref().map(|_| "ok").map_err(|e| e.to_string()));
                ok &= size > 10_000 && import.is_ok();
                let _ = std::fs::remove_file(&path);
            }
            Err(e) => {
                eprintln!("  export failed: {e}");
                ok = false;
            }
        }

        println!("\n=== reset_site (drop + reinstall on MariaDB) ===");
        let reset = wordpress::reset_site(
            &php_bin,
            &phar,
            &docroot,
            DOMAIN,
            "MariaDB Check",
            DB,
            &client,
            engine.port(),
        );
        let (rok, _) = wp(&php_bin, &phar, &docroot, &["core", "is-installed"]);
        println!("  reset → {:?} · is-installed after = {rok}", reset.as_ref().map(|_| "ok").map_err(|e| e.to_string()));
        ok &= reset.is_ok() && rok;
    }

    // Cleanup: drop the check DB, stop the engine, remove the docroot.
    let _ = database::drop_database(&client, engine.port(), DB);
    let _ = engine.stop(&*plat, pid);
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&docroot);
    // The engine datadir keeps the bootstrap system tables — that's the real
    // shared datadir (app-data/mariadb), same one the app will use. Leave it.

    if ok {
        println!("\nOK — WordPress installed, queried, dumped, re-imported, and reset on MariaDB.");
    } else {
        eprintln!("\nFAILED — see above.");
        std::process::exit(1);
    }
}

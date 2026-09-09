//! Live check for the Blank-PHP starter (ledger #426–#429).
//! Run: `cargo run --example starter_seed_check`
//!
//! One sandbox mysqld on a fixture port, so nothing here touches the user's
//! engines or their databases.
//!
//! Proves, in order:
//!   1. `starter::seed` creates the table and seeds it — four rows;
//!   2. it is IDEMPOTENT: the same call again leaves four rows, not eight. This
//!      is the leg L0 cannot reach. The guard is a SQL `WHERE NOT EXISTS`, and
//!      whether a server agrees with a piece of SQL is not something a string
//!      comparison in Rust can answer (the library-flags lesson: a guarantee
//!      read off syntax is unproven until a real server answers it);
//!   3. a row the DEVELOPER inserted survives a re-seed — what a Retry does to
//!      work already done, which is the reason the guard exists rather than a
//!      DROP TABLE;
//!   4. the page's own SELECT runs against the seeded table — the same column
//!      list `index.php` asks for, executed by the engine rather than compared
//!      as text;
//!   5. both generated files PARSE under the bundled PHP the site will run on.
//!      A generated page with a syntax error cannot report itself: PHP never
//!      gets far enough to render the error card.

use rexenv_lib::core::db::DbEngine;
use rexenv_lib::core::{binaries, database, starter};
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

mod common;
use common::Reaped;

/// Fixture port (docs/PORTS.md band) — never the production 13306.
const PORT: u16 = 13396;
const DB: &str = "starter_seed_check";

fn exec(client: &Path, sql: &str) -> Result<String, String> {
    let out = std::process::Command::new(client)
        .args(["--no-defaults", "--protocol=TCP", "--host=127.0.0.1"])
        .arg(format!("--port={PORT}"))
        .args(["--user=root", "-N", "-B", "-e", sql])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

fn count(client: &Path) -> u64 {
    exec(client, &format!("SELECT COUNT(*) FROM `{DB}`.`{}`", starter::TABLE))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

#[tokio::main]
async fn main() -> ExitCode {
    let (plat, sandbox) = common::sandbox("starter_seed_check");
    common::require_ports_free(&[(PORT, "sandbox mysqld")]);
    let mut ok = true;

    let basedir = match binaries::resolve_dir(&*plat, "mysql", binaries::MYSQL_VERSION).await {
        Ok(b) => b,
        Err(e) => {
            eprintln!("mysql tree unavailable: {e}");
            return ExitCode::FAILURE;
        }
    };
    let datadir = sandbox.root().join("mysql-data");
    if let Err(e) = database::initialize(&*plat, &basedir, &datadir) {
        eprintln!("init datadir failed: {e}");
        return ExitCode::FAILURE;
    }
    let socket = sandbox.root().join("mysql.sock");
    let child = match database::start(&*plat, &basedir, &datadir, PORT, &socket) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("start mysqld failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let _mysqld = Reaped::new(child, PORT, "mysqld");
    for _ in 0..150 {
        if database::mysql_running(PORT) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    if !database::mysql_running(PORT) {
        eprintln!("sandbox mysqld never came up on :{PORT}");
        return ExitCode::FAILURE;
    }
    let client = match DbEngine::Mysql.sql_client_bins(&*plat, binaries::MYSQL_VERSION).await {
        Ok((c, _)) => c,
        Err(e) => {
            eprintln!("bundled MySQL client unavailable: {e}");
            return ExitCode::FAILURE;
        }
    };

    // ── 1. create + seed ────────────────────────────────────────────────────
    if let Err(e) = DbEngine::Mysql.create_database(&client, PORT, DB) {
        eprintln!("create database failed: {e}");
        return ExitCode::FAILURE;
    }
    if let Err(e) = starter::seed(&client, PORT, DB) {
        eprintln!("FAIL 1: first seed failed: {e}");
        return ExitCode::FAILURE;
    }
    let first = count(client.path());
    println!("1. first seed → {first} rows {}", if first == 4 { "PASS" } else { "FAIL" });
    ok &= first == 4;

    // ── 2. idempotent ───────────────────────────────────────────────────────
    if let Err(e) = starter::seed(&client, PORT, DB) {
        eprintln!("FAIL 2: re-seed failed: {e}");
        return ExitCode::FAILURE;
    }
    let second = count(client.path());
    println!("2. re-seed → {second} rows (still 4) {}", if second == 4 { "PASS" } else { "FAIL" });
    ok &= second == 4;

    // ── 3. the developer's own row survives a Retry ─────────────────────────
    exec(
        client.path(),
        &format!(
            "INSERT INTO `{DB}`.`{}` (title, note) VALUES ('mine', 'written by the developer')",
            starter::TABLE
        ),
    )
    .expect("insert the developer's row");
    if let Err(e) = starter::seed(&client, PORT, DB) {
        eprintln!("FAIL 3: seed over a used table failed: {e}");
        return ExitCode::FAILURE;
    }
    let mine = exec(
        client.path(),
        &format!("SELECT COUNT(*) FROM `{DB}`.`{}` WHERE title = 'mine'", starter::TABLE),
    )
    .unwrap_or_default();
    let total = count(client.path());
    let kept = mine.trim() == "1" && total == 5;
    println!("3. developer row survives a re-seed → {total} rows, mine={} {}",
        mine.trim(), if kept { "PASS" } else { "FAIL" });
    ok &= kept;

    // ── 4. the page's own SELECT ────────────────────────────────────────────
    // The query `index.php` runs, verbatim in shape, executed by the engine.
    // L0 only proves the two files MENTION the same columns.
    let page_query = format!(
        "SELECT `id`, `title`, `note`, `created_at` FROM `{DB}`.`{}` ORDER BY `id`",
        starter::TABLE
    );
    match exec(client.path(), &page_query) {
        Ok(rows) => {
            let n = rows.lines().filter(|l| !l.trim().is_empty()).count();
            println!("4. the page's SELECT runs → {n} rows {}", if n == 5 { "PASS" } else { "FAIL" });
            ok &= n == 5;
        }
        Err(e) => {
            println!("4. the page's SELECT runs → FAIL ({e})");
            ok = false;
        }
    }

    // ── 5. the generated files parse under the bundled PHP ──────────────────
    let docroot = sandbox.root().join("docroot");
    std::fs::create_dir_all(&docroot).expect("fixture docroot");
    let settings = starter::StarterDb::for_engine(DbEngine::Mysql, DB);
    if let Err(e) = starter::write_files(&docroot, Some(&settings)) {
        eprintln!("FAIL 5: writing the starter files failed: {e}");
        return ExitCode::FAILURE;
    }
    match binaries::resolve(&*plat, "php", binaries::PHP_VERSION).await {
        Ok(php) => {
            for file in ["index.php", "db.php"] {
                let out = std::process::Command::new(&php)
                    .args(["-l".as_ref(), docroot.join(file).as_os_str()])
                    .output()
                    .expect("run php -l");
                let good = out.status.success();
                println!(
                    "5. {file} parses under PHP {} → {}",
                    binaries::PHP_VERSION,
                    if good { "PASS" } else { "FAIL" }
                );
                if !good {
                    eprintln!("{}", String::from_utf8_lossy(&out.stdout).trim());
                }
                ok &= good;
            }
        }
        Err(e) => {
            // A missing PHP is a precondition failure, not a passing run: the
            // verdict contract says exit 0 means PROVEN.
            eprintln!("FAIL 5: bundled PHP unavailable: {e}");
            ok = false;
        }
    }

    println!("\n{}", if ok { "ALL PASS" } else { "FAILURES ABOVE" });
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

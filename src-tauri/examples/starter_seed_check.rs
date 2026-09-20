//! Live check for the Blank-PHP starter (ledger #426–#429).
//! Run: `cargo run --example starter_seed_check`
//!
//! Sandbox servers on fixture ports — one mysqld, one PostgreSQL — so nothing
//! here touches the user's engines or their databases. **Both, because the
//! starter's DDL is per DIALECT**: MySQL's `AUTO_INCREMENT`/`ENGINE=`/backticks
//! and PostgreSQL's `IDENTITY`/`VALUES`/double quotes are two scripts, and a
//! script only a Rust test has read is a script no server has agreed to.
//!
//! Proves, for EACH engine, in order:
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
//!
//! …and for PostgreSQL, a sixth that only exists there: **the generated `db.php`
//! CONNECTS**, through the driver that was missing from every bundled PHP until
//! 10 Sep 2026 (ledger #545). `php -l` proves the file parses; it says nothing
//! about whether `pgsql:` reaches a server.

use rexenv_lib::core::db::{DbEngine, SqlClient};
use rexenv_lib::core::{binaries, database, postgres, starter};
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

mod common;
use common::Reaped;

/// Fixture ports (docs/PORTS.md band) — never the production 13306 / 15432.
const MYSQL_PORT: u16 = 13396;
const PG_PORT: u16 = 13395;
const DB: &str = "starter_seed_check";

/// Run one query and get its rows back, through whichever client this engine
/// speaks to. Both are asked for BARE, tab-separated output so the counting
/// below is the same code for both.
fn exec(engine: DbEngine, client: &SqlClient, port: u16, sql: &str) -> Result<String, String> {
    let mut cmd = std::process::Command::new(client.path());
    match engine {
        DbEngine::Postgres => {
            cmd.args(postgres::psql_base_args(port, DB)).args(["-tA", "--command", sql]);
        }
        _ => {
            cmd.args(["--no-defaults", "--protocol=TCP", "--host=127.0.0.1"])
                .arg(format!("--port={port}"))
                .args(["--user=root", "-N", "-B", "-e", sql]);
        }
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

/// `db.table` as this engine spells it. The starter's own SQL never needs this
/// — it runs INSIDE the database — but the check does, because MySQL's client
/// connects to no database at all.
fn qualified(engine: DbEngine) -> String {
    match engine {
        DbEngine::Postgres => format!("\"{}\"", starter::TABLE),
        _ => format!("`{DB}`.`{}`", starter::TABLE),
    }
}

fn count(engine: DbEngine, client: &SqlClient, port: u16) -> u64 {
    exec(engine, client, port, &format!("SELECT COUNT(*) FROM {}", qualified(engine)))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// The five legs, run against whichever engine's server is up. Returns whether
/// they all passed — printing per leg, so a failure names itself rather than
/// being counted at the end.
async fn legs(
    plat: &dyn rexenv_lib::platform::traits::Platform,
    engine: DbEngine,
    client: &SqlClient,
    port: u16,
    docroot: &Path,
) -> bool {
    let mut ok = true;
    let table = qualified(engine);
    println!("\n══ {} on :{port} ══", engine.label());

    // ── 1. create + seed ────────────────────────────────────────────────────
    if let Err(e) = engine.create_database(client, port, DB) {
        eprintln!("FAIL: create database: {e}");
        return false;
    }
    if let Err(e) = starter::seed(engine, client, port, DB) {
        eprintln!("FAIL 1: first seed failed: {e}");
        return false;
    }
    let first = count(engine, client, port);
    println!("1. first seed → {first} rows {}", if first == 4 { "PASS" } else { "FAIL" });
    ok &= first == 4;

    // ── 2. idempotent ───────────────────────────────────────────────────────
    if let Err(e) = starter::seed(engine, client, port, DB) {
        eprintln!("FAIL 2: re-seed failed: {e}");
        return false;
    }
    let second = count(engine, client, port);
    println!("2. re-seed → {second} rows (still 4) {}", if second == 4 { "PASS" } else { "FAIL" });
    ok &= second == 4;

    // ── 3. the developer's own row survives a Retry ─────────────────────────
    exec(
        engine,
        client,
        port,
        &format!("INSERT INTO {table} (title, note) VALUES ('mine', 'written by the developer')"),
    )
    .expect("insert the developer's row");
    if let Err(e) = starter::seed(engine, client, port, DB) {
        eprintln!("FAIL 3: seed over a used table failed: {e}");
        return false;
    }
    let mine = exec(
        engine,
        client,
        port,
        &format!("SELECT COUNT(*) FROM {table} WHERE title = 'mine'"),
    )
    .unwrap_or_default();
    let total = count(engine, client, port);
    let kept = mine.trim() == "1" && total == 5;
    println!(
        "3. developer row survives a re-seed → {total} rows, mine={} {}",
        mine.trim(),
        if kept { "PASS" } else { "FAIL" }
    );
    ok &= kept;

    // ── 4. the page's own SELECT ────────────────────────────────────────────
    // The query `index.php` runs, verbatim in shape — unquoted identifiers, so
    // the same text must parse on both engines. L0 only proves the two files
    // MENTION the same columns.
    let page_query = format!("SELECT id, title, note, created_at FROM {table} ORDER BY id");
    match exec(engine, client, port, &page_query) {
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
    // `for_engine` answers with the PRODUCTION port, which is the right answer
    // and the wrong server for a fixture: this check's engines are on 13395/6
    // precisely so they cannot be the user's. Only the port is substituted —
    // driver, user and DSN shape stay exactly what a real site would get, and
    // they are what leg 6 exercises.
    let mut settings = starter::StarterDb::for_engine(engine, DB);
    assert_eq!(settings.port, engine.port(), "for_engine must answer with the real port");
    settings.port = port;
    if let Err(e) = starter::write_files(docroot, Some(&settings)) {
        eprintln!("FAIL 5: writing the starter files failed: {e}");
        return false;
    }
    let php = match binaries::resolve(plat, "php", binaries::PHP_VERSION).await {
        Ok(p) => p,
        Err(e) => {
            // A missing PHP is a precondition failure, not a passing run: the
            // verdict contract says exit 0 means PROVEN.
            eprintln!("FAIL 5: bundled PHP unavailable: {e}");
            return false;
        }
    };
    for file in ["index.php", "db.php"] {
        let out = std::process::Command::new(&php)
            .args(["-l".as_ref(), docroot.join(file).as_os_str()])
            .output()
            .expect("run php -l");
        let good = out.status.success();
        println!("5. {file} parses under PHP {} → {}", binaries::PHP_VERSION, if good { "PASS" } else { "FAIL" });
        if !good {
            eprintln!("{}", String::from_utf8_lossy(&out.stdout).trim());
        }
        ok &= good;
    }

    // ── 6. …and it CONNECTS. Parsing is not connecting ──────────────────────
    // `php -l` would pass on a db.php naming a driver this PHP does not have —
    // which is exactly what every bundled PHP looked like until 10 Sep 2026
    // (ledger #545): `pgsql:` accepted, then a 60-second stall. So the file is
    // REQUIRED, and asked for a row.
    let probe = docroot.join("connect-probe.php");
    std::fs::write(
        &probe,
        format!(
            "<?php\n$pdo = require __DIR__ . '/db.php';\n\
             echo $pdo->query('SELECT COUNT(*) FROM ' . '{}')->fetchColumn();\n",
            match engine {
                DbEngine::Postgres => format!("\"{}\"", starter::TABLE),
                _ => format!("`{}`", starter::TABLE),
            }
        ),
    )
    .expect("write the connect probe");
    let out = std::process::Command::new(&php).arg(&probe).output().expect("run the probe");
    let said = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let connected = out.status.success() && said == "5";
    println!("6. db.php connects and reads → {said:?} {}", if connected { "PASS" } else { "FAIL" });
    if !connected {
        eprintln!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    ok &= connected;
    let _ = std::fs::remove_file(&probe);

    ok
}

#[tokio::main]
async fn main() -> ExitCode {
    let (plat, sandbox) = common::sandbox("starter_seed_check");
    common::require_ports_free(&[(MYSQL_PORT, "sandbox mysqld"), (PG_PORT, "sandbox postgres")]);
    let mut ok = true;

    // ── MySQL ───────────────────────────────────────────────────────────────
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
    let child = match database::start(&*plat, &basedir, &datadir, MYSQL_PORT, &socket) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("start mysqld failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut mysqld = Reaped::new(child, MYSQL_PORT, "mysqld");
    for _ in 0..150 {
        if database::mysql_running(MYSQL_PORT) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    if !database::mysql_running(MYSQL_PORT) {
        eprintln!("sandbox mysqld never came up on :{MYSQL_PORT}");
        return ExitCode::FAILURE;
    }
    let my_client = match DbEngine::Mysql.sql_client_bins(&*plat, binaries::MYSQL_VERSION).await {
        Ok((c, _)) => c,
        Err(e) => {
            eprintln!("bundled MySQL client unavailable: {e}");
            return ExitCode::FAILURE;
        }
    };
    let my_docroot = sandbox.root().join("docroot-mysql");
    std::fs::create_dir_all(&my_docroot).expect("fixture docroot");
    ok &= legs(&*plat, DbEngine::Mysql, &my_client, MYSQL_PORT, &my_docroot).await;
    // Stopped before PostgreSQL starts: two servers up at once buys nothing and
    // doubles what an interrupted run leaves behind.
    mysqld.reap();

    // ── PostgreSQL ──────────────────────────────────────────────────────────
    let pg_version = DbEngine::Postgres.default_version();
    let pg_base = match binaries::resolve_dir(&*plat, "postgres", pg_version).await {
        Ok(b) => b,
        Err(e) => {
            eprintln!("postgres tree unavailable: {e}");
            return ExitCode::FAILURE;
        }
    };
    let pg_data = sandbox.root().join("pg-data");
    if let Err(e) = postgres::initialize(&*plat, &pg_base, &pg_data) {
        eprintln!("initdb failed: {e}");
        return ExitCode::FAILURE;
    }
    let child = match postgres::start(&*plat, &pg_base, &pg_data, PG_PORT) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("start postgres failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut pg = Reaped::from_proc(child, PG_PORT, "postgres");
    for _ in 0..150 {
        if rexenv_lib::core::ports::is_listening(PG_PORT) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    if !rexenv_lib::core::ports::is_listening(PG_PORT) {
        eprintln!("sandbox postgres never came up on :{PG_PORT}");
        return ExitCode::FAILURE;
    }
    let pg_client = match DbEngine::Postgres.sql_client_bins(&*plat, pg_version).await {
        Ok((c, _)) => c,
        Err(e) => {
            eprintln!("bundled psql unavailable: {e}");
            return ExitCode::FAILURE;
        }
    };
    // A docroot of its own: `write_files` never overwrites, so reusing MySQL's
    // would have "written" a db.php that still names MySQL and proven nothing.
    let pg_docroot = sandbox.root().join("docroot-postgres");
    std::fs::create_dir_all(&pg_docroot).expect("fixture docroot");
    ok &= legs(&*plat, DbEngine::Postgres, &pg_client, PG_PORT, &pg_docroot).await;
    pg.reap();

    println!("\n{}", if ok { "ALL PASS" } else { "FAILURES ABOVE" });
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

//! Manual check: the PostgreSQL site-database path against the REAL `psql` and
//! `pg_dump` (docs/archive/PLAN-postgres-sites.md step a).
//! Run: `cargo run --example postgres_site_db_check`
//!
//! PostgreSQL is the first site engine that is not MySQL-with-another-name, so
//! every op here is new code answering to a server rather than to a test double:
//!   1. `DbEngine::Postgres.start()` (bootstrap on first run) on :15432.
//!   2. `create_database` twice — PG has no `CREATE DATABASE IF NOT EXISTS`, so
//!      idempotence is ours to get right and worth proving against the server.
//!   3. `import_from_file` of a good dump, then of a BROKEN one — the second
//!      MUST fail. `psql` exits 0 on SQL errors without `ON_ERROR_STOP=1`, so
//!      this leg is the whole reason the flag is pinned by a lib test.
//!   4. `db_sizes` lists the database with a real byte count.
//!   5. `pg_dump` export → re-import into a SECOND database → drop both, and
//!      `db_sizes` no longer lists them.
//!   6. The engine-mismatch refusal (a MySQL client handed to a PG op).
//!
//! Fixture-owned throughout: two `rex_pgcheck_*` databases this file creates and
//! drops, a temp dir for the .sql files, and the exported dump is deleted. It
//! never touches a site's database and creates no site row.

use rexenv_lib::core::db::DbEngine;
use rexenv_lib::core::ports;
use rexenv_lib::platform;
use std::thread;
use std::time::Duration;

const DB: &str = "rex_pgcheck_one";
const DB2: &str = "rex_pgcheck_two";
const DOMAIN: &str = "pgcheck.rex";

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let engine = DbEngine::Postgres;
    let mut ok = true;

    let (client, dump_bin) = engine
        .sql_client_bins(&*plat, engine.default_version())
        .await
        .expect("psql + pg_dump from the pinned tree");
    println!("client  {}", client.path().display());
    println!("dump    {}", dump_bin.display());

    println!("\n=== engine up (bootstrap on first run) ===");
    if let Err(e) = ports::ensure_free(&*plat, engine.port(), ports::Proto::Tcp, "PostgreSQL") {
        eprintln!("  port {} busy — {e}", engine.port());
        eprintln!("  stop PostgreSQL on the Databases page first; this check owns the engine.");
        std::process::exit(1);
    }
    let mut child = engine
        .start(&*plat, engine.default_version())
        .await
        .expect("start postgres");
    let pid = child.id();
    for _ in 0..60 {
        if engine.running() {
            break;
        }
        thread::sleep(Duration::from_millis(500));
    }
    println!("  pid {pid} · listening on :{} = {}", engine.port(), engine.running());
    ok &= engine.running();

    let port = engine.port();
    let tmp = std::env::temp_dir().join("rexenv-pg-site-check");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    println!("\n=== create_database, twice (no IF NOT EXISTS in PG) ===");
    let first = engine.create_database(&client, port, DB);
    let again = engine.create_database(&client, port, DB);
    println!("  first  → {:?}", first.as_ref().map(|_| "ok").map_err(|e| e.to_string()));
    println!("  repeat → {:?}", again.as_ref().map(|_| "ok").map_err(|e| e.to_string()));
    ok &= first.is_ok() && again.is_ok();

    println!("\n=== import a good dump, then a broken one ===");
    let good = tmp.join("good.sql");
    std::fs::write(
        &good,
        "CREATE TABLE IF NOT EXISTS notes (id serial PRIMARY KEY, title text NOT NULL);\n\
         INSERT INTO notes (title) VALUES ('hello from the check');\n",
    )
    .unwrap();
    let imported = engine.import_from_file(&client, port, DB, &good);
    println!("  good   → {:?}", imported.as_ref().map(|_| "ok").map_err(|e| e.to_string()));
    ok &= imported.is_ok();

    // The ON_ERROR_STOP leg. Without the flag psql exits 0 here and the import
    // "succeeds" having executed nothing — the one failure in this feature that
    // produces a wrong answer instead of an error.
    let bad = tmp.join("bad.sql");
    std::fs::write(&bad, "SELECT * FROM a_table_that_does_not_exist;\n").unwrap();
    let broken = engine.import_from_file(&client, port, DB, &bad);
    match &broken {
        Err(e) => println!("  broken → refused: {}", e.to_string().lines().next().unwrap_or("")),
        Ok(()) => eprintln!("  broken → REPORTED SUCCESS — ON_ERROR_STOP is not in effect"),
    }
    ok &= broken.is_err();

    // An empty file is refused before the client is even spawned.
    let empty = tmp.join("empty.sql");
    std::fs::write(&empty, "").unwrap();
    let empty_res = engine.import_from_file(&client, port, DB, &empty);
    println!("  empty  → {}", if empty_res.is_err() { "refused" } else { "ACCEPTED (wrong)" });
    ok &= empty_res.is_err();

    println!("\n=== db_sizes ===");
    match engine.db_sizes(&client, port) {
        Ok(sizes) => {
            let mine = sizes.iter().find(|(n, _)| n == DB);
            println!("  {} databases · {DB} → {:?} bytes", sizes.len(), mine.map(|(_, b)| b));
            // Template databases are excluded on purpose: they are not sites.
            ok &= mine.map(|(_, b)| *b > 0).unwrap_or(false)
                && !sizes.iter().any(|(n, _)| n == "template0");
        }
        Err(e) => {
            eprintln!("  db_sizes failed: {e}");
            ok = false;
        }
    }

    println!("\n=== pg_dump export → import into a second database ===");
    match engine.export_to_downloads(&dump_bin, port, DOMAIN, DB) {
        Ok(path) => {
            let body = std::fs::read_to_string(&path).unwrap_or_default();
            println!("  exported {} ({} bytes)", path.display(), body.len());
            ok &= body.contains("CREATE TABLE") && body.contains("notes");
            ok &= engine.create_database(&client, port, DB2).is_ok();
            let re = engine.import_from_file(&client, port, DB2, &path);
            println!("  re-import → {:?}", re.as_ref().map(|_| "ok").map_err(|e| e.to_string()));
            ok &= re.is_ok();
            let _ = std::fs::remove_file(&path);
        }
        Err(e) => {
            eprintln!("  export failed: {e}");
            ok = false;
        }
    }

    println!("\n=== a MySQL client is refused by a PostgreSQL op ===");
    let (mysql_client, _) = DbEngine::Mysql
        .sql_client_bins(&*plat, DbEngine::Mysql.default_version())
        .await
        .expect("mysql client");
    let mismatch = engine.create_database(&mysql_client, port, DB);
    println!("  → {}", mismatch.as_ref().err().map(|e| e.to_string()).unwrap_or_else(|| "ACCEPTED (wrong)".into()));
    ok &= mismatch.is_err();

    println!("\n=== drop both, and they are gone ===");
    let d1 = engine.drop_database(&client, port, DB);
    let d2 = engine.drop_database(&client, port, DB2);
    let redrop = engine.drop_database(&client, port, DB); // IF EXISTS ⇒ a no-op
    let left = engine
        .db_sizes(&client, port)
        .map(|s| s.into_iter().any(|(n, _)| n == DB || n == DB2))
        .unwrap_or(true);
    println!("  drop={:?} drop2={:?} re-drop={:?} · still listed = {left}", d1.is_ok(), d2.is_ok(), redrop.is_ok());
    ok &= d1.is_ok() && d2.is_ok() && redrop.is_ok() && !left;

    let _ = engine.stop(&*plat, pid, None);
    child.wait();
    let _ = std::fs::remove_dir_all(&tmp);
    // The engine datadir stays: it is the shared app-data cluster, the same one
    // the app uses, and this check created nothing in it but the two databases
    // it just dropped.

    println!("\n{}", if ok { "postgres site db: all green" } else { "postgres site db: FAILURES above" });
    if !ok {
        std::process::exit(1);
    }
}

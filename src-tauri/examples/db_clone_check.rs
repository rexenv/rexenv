//! Live check for **copying one site's database into a new one on the same
//! server** (`core::dbclone`, W3 of `docs/PLAN-git-worktrees.md`). Run:
//! `cargo run --example db_clone_check`
//!
//! Sandbox servers on fixture ports — one mysqld, one PostgreSQL — and an
//! in-memory app database, so nothing here touches the user's engines, their
//! databases or their site rows.
//!
//! Proves, for EACH engine:
//!   1. a copy lands every source table with every row, and the child row's
//!      `db_created` is recorded `true` (teardown may drop it);
//!   2. the copy is INDEPENDENT — a write to it leaves the source unchanged;
//!   3. a Retry into OUR half-made target converges (same rows, not doubled);
//!   4. a target that already exists and is NOT ours is refused, and its own
//!      table survives the refusal (ledger #817);
//!   5. a missing source is refused before anything is recorded or created;
//!   6. the private dump file is gone after success AND after failure.
//!
//! What it does NOT prove: the 0600 mode of the dump while it exists (the file
//! is gone by the time anything could look); that is `write_private`'s own
//! proof (#… in the platform section) plus the dump tools keeping the mode on
//! truncate.

use rexenv_lib::core::db::{DbEngine, SqlClient};
use rexenv_lib::core::{binaries, database, dbclone, dbrestore, postgres};
use rexenv_lib::state::db;
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

mod common;
use common::Reaped;

/// Fixture ports (docs/PORTS.md band) — never the production 13306 / 15432.
const MYSQL_PORT: u16 = 13394;
const PG_PORT: u16 = 13393;

fn exec(engine: DbEngine, client: &SqlClient, port: u16, dbname: &str, sql: &str) -> Result<String, String> {
    let mut cmd = std::process::Command::new(client.path());
    match engine {
        DbEngine::Postgres => {
            cmd.args(postgres::psql_base_args(port, dbname)).args(["-tA", "--command", sql]);
        }
        _ => {
            cmd.args(["--no-defaults", "--protocol=TCP", "--host=127.0.0.1"])
                .arg(format!("--port={port}"))
                .args(["--user=root", "-N", "-B", "-D", dbname, "-e", sql]);
        }
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

fn count(engine: DbEngine, client: &SqlClient, port: u16, dbname: &str, table: &str) -> i64 {
    exec(engine, client, port, dbname, &format!("SELECT COUNT(*) FROM {table}"))
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(-1)
}

fn empty(dir: &Path) -> bool {
    std::fs::read_dir(dir).map(|mut d| d.next().is_none()).unwrap_or(true)
}

fn legs(
    plat: &dyn rexenv_lib::platform::traits::Platform,
    engine: DbEngine,
    client: &SqlClient,
    dump: &Path,
    port: u16,
    scratch: &Path,
) -> bool {
    let mut ok = true;
    let mut check = |n: &str, what: &str, good: bool| {
        println!("{n}. {what} → {}", if good { "PASS" } else { "FAIL" });
        ok &= good;
    };
    println!("\n══ {} on :{port} ══", engine.label());

    // The app database: the sandbox's own file, three site rows only.
    let app_db = scratch.parent().expect("sandbox root").join(format!("app-{}.sqlite", engine.key()));
    let _ = std::fs::remove_file(&app_db);
    let conn = db::open(&app_db).expect("app db");
    for (id, domain) in [("parent", "shop.rex"), ("child", "fix.shop.rex"), ("other", "x.shop.rex")] {
        conn.execute(
            "INSERT INTO sites (id, name, domain, type, php_version, path) \
             VALUES (?1, ?1, ?2, 'wordpress', '8.3', '/nowhere')",
            rusqlite::params![id, domain],
        )
        .expect("site row");
    }
    let record = |site: &'static str| {
        let conn = &conn;
        move |exists: bool| dbrestore::record_provenance(conn, site, exists)
    };

    // The source: two tables, rows in both.
    let src = "clone_src";
    let dst = "clone_dst";
    let _ = engine.drop_database(client, port, src);
    let _ = engine.drop_database(client, port, dst);
    engine.create_database(client, port, src).expect("create source");
    for sql in [
        "CREATE TABLE wp_options (option_id INT PRIMARY KEY, option_name VARCHAR(64), option_value TEXT)",
        "INSERT INTO wp_options VALUES (1, 'siteurl', 'https://shop.rex'), (2, 'home', 'https://shop.rex')",
        "CREATE TABLE wp_posts (id INT PRIMARY KEY, title VARCHAR(64))",
        "INSERT INTO wp_posts VALUES (1, 'hello'), (2, 'world'), (3, 'again')",
    ] {
        exec(engine, client, port, src, sql).expect("seed source");
    }

    let spec = |source: &'static str, target: &'static str| dbclone::CloneSpec {
        engine,
        client,
        dump,
        port,
        source,
        target,
        scratch_dir: scratch,
    };

    // 1. the copy
    let rec = record("child");
    match dbclone::clone_database(&spec(src, dst), plat.permissions(), &rec) {
        Ok(r) => check("1a", &format!("copy reports {} tables, {} dump bytes", r.tables, r.dump_bytes), r.tables == 2 && r.dump_bytes > 0),
        Err(e) => check("1a", &format!("copy failed: {e}"), false),
    }
    check("1b", "every row arrived", count(engine, client, port, dst, "wp_options") == 2 && count(engine, client, port, dst, "wp_posts") == 3);
    let created = rexenv_lib::state::store::get_site(&conn, "child").unwrap().unwrap().db_created;
    check("1c", "the child's db_created is recorded true", created == Some(true));

    // 2. independence
    exec(engine, client, port, dst, "INSERT INTO wp_posts VALUES (4, 'only in the copy')").expect("write copy");
    check("2", "a write to the copy leaves the source alone", count(engine, client, port, src, "wp_posts") == 3);

    // 3. retry converges
    let again = dbclone::clone_database(&spec(src, dst), plat.permissions(), &rec);
    check("3", "a Retry into our own target converges (3 posts, not 4 or 6)", again.is_ok() && count(engine, client, port, dst, "wp_posts") == 3);

    // 4. a pre-existing target that is not ours
    let theirs = "clone_theirs";
    let _ = engine.drop_database(client, port, theirs);
    engine.create_database(client, port, theirs).expect("their db");
    exec(engine, client, port, theirs, "CREATE TABLE mine (id INT)").expect("their table");
    exec(engine, client, port, theirs, "INSERT INTO mine VALUES (42)").expect("their row");
    let err = dbclone::clone_database(&spec(src, theirs), plat.permissions(), &record("other"))
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    check("4a", "a target we did not create is refused", err.contains("did not create"));
    let tables = engine.list_tables(client, port, theirs).unwrap_or_default();
    check("4b", "…and it is untouched", tables == ["mine"] && count(engine, client, port, theirs, "mine") == 1);

    // 5. missing source
    let err = dbclone::clone_database(&spec("clone_missing", "clone_never"), plat.permissions(), &record("parent"))
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    let parent_recorded = rexenv_lib::state::store::get_site(&conn, "parent").unwrap().unwrap().db_created;
    check(
        "5",
        "a missing source is refused before anything is recorded or created",
        err.contains("does not exist")
            && parent_recorded.is_none()
            && !engine.database_exists(client, port, "clone_never").unwrap_or(true),
    );

    // 6. no dump left behind
    check("6", "the scratch dir holds no dump", empty(scratch));

    for d in [src, dst, theirs] {
        let _ = engine.drop_database(client, port, d);
    }
    ok
}

#[tokio::main]
async fn main() -> ExitCode {
    let (plat, sandbox) = common::sandbox("db_clone_check");
    common::require_ports_free(&[(MYSQL_PORT, "sandbox mysqld"), (PG_PORT, "sandbox postgres")]);
    let mut ok = true;
    let scratch = sandbox.root().join("clone-scratch");
    std::fs::create_dir_all(&scratch).expect("scratch dir");

    // ── MySQL ───────────────────────────────────────────────────────────────
    let basedir = match binaries::resolve_dir(&*plat, "mysql", binaries::pins().mysql).await {
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
    let (my_client, my_dump) = match DbEngine::Mysql.sql_client_bins(&*plat, binaries::pins().mysql).await {
        Ok(b) => b,
        Err(e) => {
            eprintln!("bundled MySQL client unavailable: {e}");
            return ExitCode::FAILURE;
        }
    };
    ok &= legs(&*plat, DbEngine::Mysql, &my_client, &my_dump, MYSQL_PORT, &scratch);
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
    let (pg_client, pg_dump) = match DbEngine::Postgres.sql_client_bins(&*plat, pg_version).await {
        Ok(b) => b,
        Err(e) => {
            eprintln!("bundled psql unavailable: {e}");
            return ExitCode::FAILURE;
        }
    };
    ok &= legs(&*plat, DbEngine::Postgres, &pg_client, &pg_dump, PG_PORT, &scratch);
    pg.reap();

    if ok {
        println!("\ndb_clone_check: all green");
        ExitCode::SUCCESS
    } else {
        eprintln!("\ndb_clone_check: FAILED");
        ExitCode::FAILURE
    }
}

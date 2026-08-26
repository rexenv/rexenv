//! **The L1 leg every M3 claim was waiting on** (ledger #399/#400/#401/#403).
//!
//! Everything M3 says about what a read-only agent principal can and cannot do
//! is, until this runs, a statement about SQL TEXT. The L0 tests assert that the
//! GRANT says `SELECT`, that the escape is present, that no LOCAL INFILE handler
//! is installed. What none of them can assert is that MySQL AGREES — that the
//! server actually refuses an `INSERT` from this account, actually refuses
//! `INTO OUTFILE`, actually confines the grant to one database rather than the
//! wildcard pattern the escape exists to prevent.
//!
//! So this connects as the real principal, through the real driver, against a
//! real engine, and tries each forbidden thing expecting failure.
//!
//! **A refusal must be the RIGHT refusal.** Every negative leg here asserts on
//! the error, not merely on "it did not succeed": a query that fails because the
//! table does not exist would otherwise read as a privilege being enforced, and
//! the whole check would pass while proving nothing. That is the vacuous-green
//! shape this repo keeps finding, and it is the reason each leg below prints
//! what the server actually said.
//!
//! `cargo run --example agent_db_check` — service tier (needs the DB engine; it
//! starts one if none is running and stops it again).

#[path = "common/mod.rs"]
mod common;

use rexenv_lib::core::agent_db::{self, Principal};
use rexenv_lib::core::{agent_query, binaries, database};
use rexenv_lib::platform;
use std::process::Command;
use std::thread;
use std::time::Duration;

/// The fixture's own databases. Two, because "this grant names ONE database" is
/// only checkable against a second one it must NOT reach — and the names are
/// chosen so the underscore-wildcard bug would make the second reachable from
/// the first's grant (`agent\_probe` vs `agentxprobe`), which is exactly the
/// cross-site over-grant `grant_db_object` escapes.
const DB: &str = "agent_probe";
const SIBLING: &str = "agentxprobe";

fn root_sql(client: &std::path::Path, port: u16, sql: &str) -> String {
    let out = Command::new(client)
        .args(["--no-defaults", "--protocol=TCP", "-h", "127.0.0.1", "-P", &port.to_string()])
        .args(["-u", "root", "-N", "-e", sql])
        .output()
        .expect("run the bundled client as root");
    if !out.status.success() {
        panic!("root SQL failed: {sql}\n{}", String::from_utf8_lossy(&out.stderr));
    }
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// Run one statement as the agent and REQUIRE it to fail, with the reason named.
/// The `expect` fragment is what makes this a proof rather than a shrug.
async fn must_refuse(port: u16, user: &str, db: &str, sql: &str, expect: &str, what: &str) {
    match agent_query::run_query(port, user, db, sql).await {
        Ok(r) => panic!("{what}: the server ALLOWED it (returned {} rows)", r.rows.len()),
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.to_lowercase().contains(&expect.to_lowercase()),
                "{what}: refused, but not for the reason claimed — expected something naming \
                 {expect:?}, got: {msg}\n  A refusal for the wrong reason (a missing table, a \
                 syntax error) reads as a privilege being enforced when it is not."
            );
            println!("  {what} refused ✓ ({})", msg.trim().chars().take(110).collect::<String>());
        }
    }
}

#[tokio::main]
async fn main() {
    // Refuse beside a live stack: these services would JOIN it, not collide
    // with it, and a connect-based readiness gate is satisfied by the user's
    // server. FIRST statement — after anything is spawned, exiting leaks it.
    common::require_stack_stopped();
    let plat = platform::current();
    let port = database::MYSQL_PORT;

    let basedir = binaries::resolve_dir(&*plat, "mysql", binaries::MYSQL_VERSION)
        .await
        .expect("resolve mysql");
    let (client, _) = rexenv_lib::core::db::DbEngine::Mysql
        .sql_client_bins(&*plat, binaries::MYSQL_VERSION)
        .await
        .expect("bundled MySQL client");

    let mut started: Option<common::OwnedService> = None;
    if !database::mysql_running(port) {
        let datadir = database::data_dir(&*plat).unwrap();
        let socket = database::socket_path(&*plat).unwrap();
        std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
        database::initialize(&*plat, &basedir, &datadir).expect("initialize");
        started = Some(common::OwnedService::new(
            database::start(&*plat, &basedir, &datadir, port, &socket).expect("start"),
            "mysqld",
        ));
        for _ in 0..30 {
            if database::mysql_running(port) {
                break;
            }
            thread::sleep(Duration::from_millis(500));
        }
    }
    assert!(database::mysql_running(port), "MySQL did not come up on {port}");

    // ── fixture ──────────────────────────────────────────────────────────────
    // Fixture-owned names, dropped at the end. Nothing here touches a real
    // site's database: both names are this example's own.
    database::create_database(&client, port, DB).expect("create the probe db");
    database::create_database(&client, port, SIBLING).expect("create the sibling db");
    root_sql(client.path(), port, &format!(
        "CREATE TABLE IF NOT EXISTS `{DB}`.t (id INT PRIMARY KEY, v VARCHAR(32)); \
         DELETE FROM `{DB}`.t; INSERT INTO `{DB}`.t VALUES (1,'one'),(2,'two');"
    ));
    root_sql(client.path(), port, &format!(
        "CREATE TABLE IF NOT EXISTS `{SIBLING}`.t (id INT PRIMARY KEY); \
         DELETE FROM `{SIBLING}`.t; INSERT INTO `{SIBLING}`.t VALUES (9);"
    ));

    let user = agent_db::principal_name(Principal::ReadOnly, "agentprobe.rex");
    // Start from clean, so a leftover account from an earlier run cannot be the
    // thing that passes: this must prove what `provision` grants TODAY.
    agent_db::deprovision(&client, port, &user).expect("clean any earlier principal");
    agent_db::provision(&client, port, Principal::ReadOnly, DB, &user).expect("provision");
    println!("provisioned {user} on {DB}");

    // ── the positive leg, first ──────────────────────────────────────────────
    // Without this, every refusal below could be "the account cannot connect at
    // all" and the run would look like a wall of enforced privileges.
    let r = agent_query::run_query(port, &user, DB, "SELECT id, v FROM t ORDER BY id")
        .await
        .expect("the read-only principal must be able to READ");
    assert_eq!(r.rows.len(), 2, "expected the two fixture rows, got {:?}", r.rows);
    assert_eq!(r.columns, vec!["id", "v"]);
    assert_eq!(r.rows[0][1].as_deref(), Some("one"));
    assert!(!r.truncated, "two rows is not a truncated result");
    println!("  SELECT works ✓ ({} rows, columns {:?})", r.rows.len(), r.columns);

    // ── what the GRANT must refuse ───────────────────────────────────────────
    must_refuse(port, &user, DB, "INSERT INTO t VALUES (3,'three')", "denied", "INSERT").await;
    must_refuse(port, &user, DB, "UPDATE t SET v='x' WHERE id=1", "denied", "UPDATE").await;
    must_refuse(port, &user, DB, "DELETE FROM t WHERE id=1", "denied", "DELETE").await;
    must_refuse(port, &user, DB, &format!("DROP DATABASE `{DB}`"), "denied", "DROP DATABASE").await;
    // FILE is a global privilege this account does not hold, so INTO OUTFILE is
    // unreachable — the §3.6 claim, against the server rather than the GRANT text.
    must_refuse(
        port,
        &user,
        DB,
        "SELECT * FROM t INTO OUTFILE '/tmp/rexenv-agent-probe.txt'",
        "denied",
        "SELECT … INTO OUTFILE",
    )
    .await;

    // ── the escape, against the server that reads the wildcard ───────────────
    // THE reason `grant_db_object` exists. Unescaped, `GRANT … ON `agent_probe`.*`
    // also matches `agentxprobe`, and this account would read a database nobody
    // granted it. This is ledger #196 as a live assertion.
    must_refuse(
        port,
        &user,
        DB,
        &format!("SELECT id FROM `{SIBLING}`.t"),
        "denied",
        "reading the wildcard-sibling database",
    )
    .await;

    // ── what the PROTOCOL must refuse, regardless of privileges ──────────────
    // The second statement is where an injection lands. This leg is the reason
    // the query path prepares rather than sending text: the first version
    // believed CLIENT_MULTI_STATEMENTS was off, THIS CHECK FOUND IT ON (the
    // driver sets it unconditionally and offers no way to clear it), and
    // `SELECT 1; SELECT 2` ran. On a scratch principal holding ALL that made
    // `SELECT 1; DROP TABLE x` a working call. COM_STMT_PREPARE accepts exactly
    // one statement, so the refusal is now the server's, before execution.
    //
    // Asserted on the SYNTAX error specifically: a privilege refusal here would
    // mean the bound is the GRANT, which is not a bound at all on scratch.
    must_refuse(
        port,
        &user,
        DB,
        "SELECT 1; SELECT 2",
        "syntax",
        "a second statement after a semicolon",
    )
    .await;

    // LOCAL INFILE. `CLIENT_LOCAL_FILES` is also set by the driver and not
    // clearable, so this was written expecting the guarantee to be the missing
    // HANDLER — and the server gave a different, stronger answer: ERROR 1295,
    // the prepared-statement protocol does not support the command at all. The
    // expectation is the one the server actually enforces, not the one the code
    // was reasoning from. Two independent things now stop it (the protocol, and
    // the absent handler), and only the first is what fires.
    must_refuse(
        port,
        &user,
        DB,
        "LOAD DATA LOCAL INFILE '/etc/hosts' INTO TABLE t",
        "not supported in the prepared statement protocol",
        "LOAD DATA LOCAL INFILE",
    )
    .await;

    // ── the row cap, on real rows ────────────────────────────────────────────
    // The cap and its `truncated` flag are asserted in L0 by reading the source;
    // this is the same rule against an engine actually returning more rows than
    // the cap, which is the only way to see the flag set for real.
    root_sql(client.path(), port, &format!(
        "INSERT INTO `{DB}`.t (id, v) SELECT n, 'x' FROM (WITH RECURSIVE s(n) AS \
         (SELECT 100 UNION ALL SELECT n+1 FROM s WHERE n < {}) SELECT n FROM s) q",
        100 + agent_query::MAX_ROWS as u32 + 10
    ));
    let big = agent_query::run_query(port, &user, DB, "SELECT id FROM t").await.expect("big read");
    assert_eq!(big.rows.len(), agent_query::MAX_ROWS, "the cap did not hold");
    assert!(big.truncated, "more rows existed and the result did not say so");
    println!("  row cap holds ✓ ({} rows, truncated=true)", big.rows.len());

    // ── revocation really closes it ──────────────────────────────────────────
    // The UI's Revoke button drops the account. If DROP USER did not take
    // effect, a revoked grant would be a UI telling the user they are safe while
    // the agent still reads — the failure direction that matters.
    agent_db::deprovision(&client, port, &user).expect("deprovision");
    match agent_query::run_query(port, &user, DB, "SELECT 1").await {
        Ok(_) => panic!("the account still works after being dropped — revoke does not revoke"),
        Err(e) => println!("  revoked account cannot connect ✓ ({})",
            e.to_string().trim().chars().take(90).collect::<String>()),
    }

    // ── teardown: only what this fixture created ─────────────────────────────
    database::drop_database(&client, port, DB).expect("drop the probe db");
    database::drop_database(&client, port, SIBLING).expect("drop the sibling db");
    if let Some(child) = &started {
        let _ = database::stop(&*plat, child.id());
        println!("stopped the engine this check started");
    }
    println!("agent_db_check: PASS");
}

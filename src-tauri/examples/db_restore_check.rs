//! Live check for HALF B — restore, recovery, mirroring (Stage 2 steps 6–7).
//! Run: `cargo run --example db_restore_check`
//!
//! One sandbox mysqld plays source AND target (different database names), so
//! nothing here touches the user's engines, Valet, Herd, or DBngin.
//!
//! Proves, in order:
//!   1. a real dump restores whole — asserted as TABLES WITH ROW COUNTS, not an
//!      exit code;
//!   2. a restore that dies part-way leaves an honest failure: the missing
//!      table is named, and `finish` is unreachable without the Verified proof;
//!   3. RETRY RECOVERS UNAIDED — the same code path re-run drops the wreckage
//!      and ends with every table at its expected row count (the Stage 1
//!      lesson, asserted at the level it broke);
//!   4. a PRE-EXISTING database is never dropped by any path — its extra table
//!      survives both the failed restore and the successful retry;
//!   5. provenance survives the crash-then-retry inversion (the record wins
//!      over re-derivation);
//!   6. credential mirroring: loopback-scoped, idempotent, usable by the site's
//!      own user — and refused for root as an OUTCOME, not an error.

use rexenv_lib::core::dbcompat::{compat, Source, Target, Version};
use rexenv_lib::core::dbdump::{self, DumpOutcome, DumpRequest, LiveCheck, Manifest};
use rexenv_lib::core::dbimport::{ConfigSource, DbConnection, Driver};
use rexenv_lib::core::dbmirror::{self, MirrorOutcome};
use rexenv_lib::core::dbrestore::{self, FeedOutcome};
use rexenv_lib::core::dbsource::Vendor;
use rexenv_lib::core::{binaries, database};
use rexenv_lib::state::db;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

mod common;
use common::Reaped;

const PORT: u16 = 13398;
const SRC_DB: &str = "restorecheck_src";
const TGT_DB: &str = "restorecheck_tgt";
const POSTS: u64 = 500;
const OPTIONS: u64 = 20;

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

fn count(client: &Path, db: &str, table: &str) -> u64 {
    exec(client, &format!("SELECT COUNT(*) FROM `{db}`.`{table}`"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

#[tokio::main]
async fn main() {
    let (plat, sandbox) = common::sandbox("db_restore_check");
    let mut ok = true;

    // ── sandbox mysqld: source db with KNOWN row counts ─────────────────────
    let basedir = binaries::resolve_dir(&*plat, "mysql", binaries::MYSQL_VERSION)
        .await
        .expect("mysql tree (cached)");
    let datadir = sandbox.root().join("mysql-data");
    database::initialize(&*plat, &basedir, &datadir).expect("init datadir");
    let socket = sandbox.root().join("mysql.sock");
    let mut mysqld = Reaped::new(
        database::start(&*plat, &basedir, &datadir, PORT, &socket).expect("start mysqld"),
        PORT,
        "mysqld",
    );
    for _ in 0..150 {
        if database::mysql_running(PORT) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(database::mysql_running(PORT), "sandbox mysqld is up");
    let client = database::mysql_client_bin(&basedir);

    database::create_database(&client, PORT, SRC_DB).unwrap();
    let mut seed = format!(
        "USE {SRC_DB}; CREATE TABLE wp_posts (id INT PRIMARY KEY, title VARCHAR(64)); \
         CREATE TABLE wp_options (id INT PRIMARY KEY, v VARCHAR(64));"
    );
    seed.push_str(&format!(
        " INSERT INTO wp_posts VALUES {};",
        (1..=POSTS).map(|i| format!("({i},'post {i}')")).collect::<Vec<_>>().join(",")
    ));
    seed.push_str(&format!(
        " INSERT INTO wp_options VALUES {};",
        (1..=OPTIONS).map(|i| format!("({i},'opt {i}')")).collect::<Vec<_>>().join(",")
    ));
    exec(&client, &seed).expect("seed source");

    // ── a real artifact + manifest via Half A ───────────────────────────────
    let dest = sandbox.root().join("db-imports");
    let verdict = compat(
        &Source { vendor: Some(Vendor::Mysql), version: Version::parse("8.4.6") },
        &Target { vendor: Vendor::Mysql, version: Version::parse(binaries::MYSQL_VERSION).unwrap() },
    );
    let cleared = dbdump::gate(None, &verdict, false).expect("gate clears");
    let src_conn = DbConnection {
        driver: Driver::MysqlFamily,
        host: "127.0.0.1".into(),
        port: PORT,
        database: SRC_DB.into(),
        user: "root".into(),
        password: String::new(),
        table_prefix: Some("wp_".into()),
        source: ConfigSource::WpConfig { path: "/fixture/wp-config.php".into() },
    };
    let defaults = dbdump::DefaultsFile::create(&*plat, &dest, &src_conn).unwrap();
    let size = match dbdump::preflight_live(&cleared, &client, &defaults, SRC_DB).unwrap() {
        LiveCheck::Ready(s) => s,
        other => panic!("{other:?}"),
    };
    let preflight = dbdump::check_disk(size, &dest).expect("disk ok");
    let dump_tool = basedir.join("bin/mysqldump");
    let req = DumpRequest {
        tool: &dump_tool,
        tool_vendor: Vendor::Mysql,
        db: SRC_DB,
        domain: "restorecheck.test",
        source_host: "127.0.0.1",
        source_port: PORT,
        source_vendor: Vendor::Mysql,
        source_version: "8.4.6",
        target_engine: "mysql",
        target_version: binaries::MYSQL_VERSION,
        dump_tool_label: "mysqldump 8.4.6",
        dest_dir: &dest,
    };
    let DumpOutcome::Done { artifact, manifest } =
        dbdump::dump(&cleared, &*plat, &preflight, &req, &defaults, &AtomicBool::new(false), &mut |_| {})
            .expect("dump")
    else {
        panic!("not cancelled")
    };
    println!("artifact holds tables: {:?}", manifest.tables);
    ok &= manifest.tables == vec!["wp_options", "wp_posts"]
        || manifest.tables == vec!["wp_posts", "wp_options"];

    let conn = db::open(&sandbox.root().join("restore.db")).expect("sqlite");
    conn.execute(
        "INSERT INTO sites (id, name, domain, type, php_version, path, db_name)
         VALUES ('s1','R','restorecheck.test','wordpress','8.3','/x/r','wp_r_test')",
        [],
    )
    .unwrap();

    println!("\n=== 1. restore whole: tables AND row counts ===");
    let exists = dbrestore::database_exists(&client, PORT, TGT_DB).unwrap();
    let rec = dbrestore::record_provenance(&conn, "s1", exists).unwrap();
    println!("  existed before: {exists}; recorded ours: {}", rec.ours());
    ok &= !exists && rec.ours();
    dbrestore::prepare_target(&rec, &client, PORT, TGT_DB).unwrap();
    let fed = dbrestore::feed(
        &rec, &client, PORT, TGT_DB, &artifact, &manifest, &AtomicBool::new(false), &mut |_| {},
    )
    .expect("feed");
    let verified = match fed {
        FeedOutcome::Fed { bytes } => {
            println!("  fed {bytes} bytes");
            dbrestore::verify_complete(&client, PORT, TGT_DB, &manifest).expect("verifies whole")
        }
        FeedOutcome::Cancelled => panic!("not cancelled"),
    };
    dbrestore::finish(&conn, "s1", TGT_DB, &verified).unwrap();
    let (p, o) = (count(&client, TGT_DB, "wp_posts"), count(&client, TGT_DB, "wp_options"));
    println!("  wp_posts={p} (want {POSTS}), wp_options={o} (want {OPTIONS})");
    ok &= p == POSTS && o == OPTIONS;
    let site = rexenv_lib::state::store::get_site(&conn, "s1").unwrap().unwrap();
    println!("  site db_name -> {} (recorded on settle only)", site.db_name);
    ok &= site.db_name == TGT_DB && site.db_created == Some(true);

    println!("\n=== 2. a restore that dies part-way is an honest failure ===");
    // A crafted artifact: wp_posts restores, then a hard syntax error, and
    // wp_options never arrives. Manifest written the way dump() would have.
    let bad_dir = sandbox.root().join("db-imports-bad");
    std::fs::create_dir_all(&bad_dir).unwrap();
    let bad_artifact = dbdump::artifact_path(&bad_dir, "restorecheck.test");
    std::fs::write(
        &bad_artifact,
        "CREATE TABLE `wp_posts` (id INT PRIMARY KEY);\n\
         INSERT INTO wp_posts VALUES (1),(2),(3);\n\
         THIS IS NOT SQL AND THE RESTORE DIES HERE;\n\
         CREATE TABLE `wp_options` (id INT PRIMARY KEY);\n",
    )
    .unwrap();
    let bad_manifest = Manifest {
        artifact_bytes: std::fs::metadata(&bad_artifact).unwrap().len(),
        tables: vec!["wp_posts".into(), "wp_options".into()],
        ..(*manifest).clone()
    };
    plat.permissions()
        .write_private(
            &dbdump::manifest_path(&bad_dir, "restorecheck.test"),
            serde_json::to_string(&bad_manifest).unwrap().as_bytes(),
        )
        .unwrap();
    let (bad_art, bad_man) = dbdump::load_manifest(&bad_dir, "restorecheck.test").unwrap();

    conn.execute(
        "INSERT INTO sites (id, name, domain, type, php_version, path, db_name)
         VALUES ('s2','R2','retry.test','wordpress','8.3','/x/r2','wp_r2_test')",
        [],
    )
    .unwrap();
    let tgt2 = "restorecheck_retry";
    let rec2 = dbrestore::record_provenance(
        &conn, "s2", dbrestore::database_exists(&client, PORT, tgt2).unwrap(),
    )
    .unwrap();
    dbrestore::prepare_target(&rec2, &client, PORT, tgt2).unwrap();
    let feed_err = dbrestore::feed(
        &rec2, &client, PORT, tgt2, &bad_art, &bad_man, &AtomicBool::new(false), &mut |_| {},
    );
    let failed_honestly = feed_err.is_err();
    println!("  feed -> {}", match &feed_err {
        Err(e) => format!("failed as it must: {e}"),
        Ok(_) => "SUCCEEDED (wrong)".into(),
    });
    // The dangerous state: wp_posts EXISTS with rows. It must not verify.
    let partial_rows = count(&client, tgt2, "wp_posts");
    let verify = dbrestore::verify_complete(&client, PORT, tgt2, &bad_man);
    println!("  partial state: wp_posts has {partial_rows} rows, and verify says:");
    match &verify {
        Err(e) => println!("    {e}"),
        Ok(_) => println!("    VERIFIED (wrong)"),
    }
    ok &= failed_honestly && partial_rows == 3 && verify.is_err();
    ok &= verify.as_ref().err().map(|e| e.to_string().contains("missing wp_options")).unwrap_or(false);

    println!("\n=== 3. RETRY RECOVERS UNAIDED — same path, full row counts ===");
    // The cause is gone (a good artifact — in the real job, a fresh re-dump).
    // Retry is literally the same calls again; provenance consults the RECORD,
    // so the half-made database stays OURS despite now existing (the inversion).
    let exists_now = dbrestore::database_exists(&client, PORT, tgt2).unwrap();
    let rec3 = dbrestore::record_provenance(&conn, "s2", exists_now).unwrap();
    println!("  db exists now: {exists_now}; record still says ours: {}", rec3.ours());
    ok &= exists_now && rec3.ours();
    dbrestore::prepare_target(&rec3, &client, PORT, tgt2).unwrap(); // drops the wreckage
    let refed = dbrestore::feed(
        &rec3, &client, PORT, tgt2, &artifact, &manifest, &AtomicBool::new(false), &mut |_| {},
    )
    .expect("retry feed");
    assert!(matches!(refed, FeedOutcome::Fed { .. }));
    let v2 = dbrestore::verify_complete(&client, PORT, tgt2, &manifest).expect("retry verifies");
    dbrestore::finish(&conn, "s2", tgt2, &v2).unwrap();
    let (p2, o2) = (count(&client, tgt2, "wp_posts"), count(&client, tgt2, "wp_options"));
    println!("  after retry: wp_posts={p2} (want {POSTS}), wp_options={o2} (want {OPTIONS})");
    ok &= p2 == POSTS && o2 == OPTIONS;

    println!("\n=== 4. a pre-existing database is never dropped by any path ===");
    let keep = "restorecheck_keep";
    exec(&client, &format!(
        "CREATE DATABASE {keep}; CREATE TABLE {keep}.their_extra (id INT PRIMARY KEY); \
         INSERT INTO {keep}.their_extra VALUES (42);"
    ))
    .unwrap();
    conn.execute(
        "INSERT INTO sites (id, name, domain, type, php_version, path, db_name)
         VALUES ('s3','K','keep.test','wordpress','8.3','/x/k','wp_k_test')",
        [],
    )
    .unwrap();
    let rec_k = dbrestore::record_provenance(
        &conn, "s3", dbrestore::database_exists(&client, PORT, keep).unwrap(),
    )
    .unwrap();
    println!("  recorded ours: {} (want false)", rec_k.ours());
    ok &= !rec_k.ours();
    // cleanup refuses…
    ok &= !dbrestore::cleanup_failed(&rec_k, &client, PORT, keep).unwrap();
    // …prepare doesn't drop…
    dbrestore::prepare_target(&rec_k, &client, PORT, keep).unwrap();
    // …and a full restore into it leaves their extra table intact.
    let fed_k = dbrestore::feed(
        &rec_k, &client, PORT, keep, &artifact, &manifest, &AtomicBool::new(false), &mut |_| {},
    )
    .expect("feed into pre-existing");
    assert!(matches!(fed_k, FeedOutcome::Fed { .. }));
    let v_k = dbrestore::verify_complete(&client, PORT, keep, &manifest)
        .expect("membership verify tolerates their extra table");
    let extra = count(&client, keep, "their_extra");
    println!("  their_extra survives with {extra} row(s); manifest tables verified: {}", v_k.tables);
    ok &= extra == 1;
    // The SQL guard: once pre-existing, nothing can re-claim it.
    ok &= !rexenv_lib::state::store::set_site_db_created(&conn, "s3", true).unwrap();

    println!("\n=== 5. mirroring: loopback-scoped, idempotent, usable — and root refused ===");
    let m1 = dbmirror::mirror(&client, PORT, TGT_DB, "ea_user", r#"p'a\s"s!"#).unwrap();
    let m2 = dbmirror::mirror(&client, PORT, TGT_DB, "ea_user", r#"p'a\s"s!"#).unwrap();
    println!("  first: {m1:?}; rerun: {m2:?} (idempotent)");
    ok &= matches!(m1, MirrorOutcome::Mirrored { .. }) && matches!(m2, MirrorOutcome::Mirrored { .. });
    let hosts = exec(&client, "SELECT host FROM mysql.user WHERE user='ea_user' ORDER BY host").unwrap();
    let hosts: Vec<&str> = hosts.lines().map(|l| l.trim()).collect();
    println!("  hosts: {hosts:?} (must be loopback only)");
    ok &= hosts == ["127.0.0.1", "localhost"];
    // The mirrored user actually works, with their own password.
    let their_conn = DbConnection {
        user: "ea_user".into(),
        password: r#"p'a\s"s!"#.into(),
        database: TGT_DB.into(),
        ..src_conn.clone()
    };
    let their_defaults =
        dbdump::DefaultsFile::create(&*plat, &sandbox.root().join("their-cnf"), &their_conn).unwrap();
    let as_them = std::process::Command::new(&client)
        .arg(format!("--defaults-extra-file={}", their_defaults.path().display()))
        .args(["-N", "-B", "-e", &format!("SELECT COUNT(*) FROM `{TGT_DB}`.wp_posts")])
        .output()
        .unwrap();
    let their_count: u64 =
        String::from_utf8_lossy(&as_them.stdout).trim().parse().unwrap_or(0);
    println!("  connect as ea_user with their password: wp_posts={their_count}");
    ok &= as_them.status.success() && their_count == POSTS;
    let root_refusal = dbmirror::mirror(&client, PORT, TGT_DB, "root", "whatever").unwrap();
    println!("  root -> {root_refusal:?}");
    ok &= matches!(root_refusal, MirrorOutcome::RefusedReserved { .. });
    // And our root is still passwordless — the pinned flag array still works.
    ok &= exec(&client, "SELECT 1").is_ok();

    mysqld.reap();
    drop(defaults);
    drop(their_defaults);

    if ok {
        println!("\nOK — restores verify by membership with real row counts, Retry recovers unaided, pre-existing databases survive everything, and mirroring is loopback-only and idempotent.");
    } else {
        eprintln!("\nFAILED — see above.");
        std::process::exit(1);
    }
}

//! Live check for HALF A's DUMP (Stage 2 step 5). Run:
//! `cargo run --example db_dump_check`
//!
//! Spins its OWN MySQL in the sandbox — a throwaway datadir on a fixture port —
//! as the "source", so nothing here touches the user's engines, Valet, Herd, or
//! DBngin. The shared binary cache is the only real thing used (read-only).
//!
//! Proves, in order:
//!   1. the pre-auth probe identifies the sandbox server without logging in;
//!   2. the gate refuses a self-import BEFORE any connection;
//!   3. the live preflight distinguishes a missing database from a present one;
//!   4. the dump produces a 0600 artifact + a manifest that matches it, and the
//!      progress signal is real byte growth;
//!   5. cancel mid-dump leaves NO artifact, NO manifest, no `.partial`;
//!   6. an interrupted dump (a stray `.partial`, an artifact whose manifest is
//!      missing or stale) can never be loaded for restore;
//!   7. a table the SOURCE cannot read is FOUND by the probe and SKIPPED by the
//!      dump — with a control leg proving that same table still aborts a dump
//!      that doesn't skip it (the bug a user hit on 8 Aug 2026: 12 dead plugin
//!      tables in a 2 GB database ended the whole migration).

use rexenv_lib::core::dbcompat::{compat, Source, Target, Version};
use rexenv_lib::core::dbdump::{self, DumpOutcome, DumpRequest, LiveCheck, OurEngine, SelfImport};
use rexenv_lib::core::dbimport::{ConfigSource, DbConnection, Driver};
use rexenv_lib::core::dbsource::{self, Identity, Vendor};
use rexenv_lib::core::{binaries, database};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

mod common;
use common::Reaped;

const PORT: u16 = 13399;
const DB: &str = "dumpcheck_src";

fn conn_for(port: u16) -> DbConnection {
    DbConnection {
        driver: Driver::MysqlFamily,
        host: "127.0.0.1".into(),
        port,
        database: DB.into(),
        user: "root".into(),
        password: String::new(),
        table_prefix: Some("wp_".into()),
        source: ConfigSource::WpConfig { path: "/fixture/wp-config.php".into() },
    }
}

fn mode_of(p: &Path) -> u32 {
    std::fs::metadata(p).map(|m| m.permissions().mode() & 0o777).unwrap_or(0)
}

#[tokio::main]
async fn main() {
    let (plat, sandbox) = common::sandbox("db_dump_check");
    let mut ok = true;

    // ── a sandbox MySQL as the "source" ─────────────────────────────────────
    let basedir = binaries::resolve_dir(&*plat, "mysql", binaries::MYSQL_VERSION)
        .await
        .expect("mysql tree (cached)");
    let datadir = sandbox.root().join("mysql-src-data");
    database::initialize(&*plat, &basedir, &datadir).expect("init sandbox datadir");
    let socket = sandbox.root().join("mysql-src.sock");
    let mut mysqld = Reaped::new(
        database::start(&*plat, &basedir, &datadir, PORT, &socket).expect("start sandbox mysqld"),
        PORT,
        "mysqld",
    );
    for _ in 0..100 {
        if database::mysql_running(PORT) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(database::mysql_running(PORT), "sandbox mysqld came up");

    let client = database::mysql_client_bin(&basedir);
    database::create_database(&client, PORT, DB).expect("create source db");
    let seed = format!(
        "USE {DB}; CREATE TABLE wp_posts (id INT PRIMARY KEY, title TEXT); \
         INSERT INTO wp_posts VALUES (1,'hello'),(2,'p)ss;w(rd survives'),(3,'third');"
    );
    let out = std::process::Command::new(&client)
        .args(["--no-defaults", "--protocol=TCP", "--host=127.0.0.1"])
        .arg(format!("--port={PORT}"))
        .args(["--user=root", "-e", &seed])
        .output()
        .expect("seed");
    assert!(out.status.success(), "seed failed: {}", String::from_utf8_lossy(&out.stderr));

    println!("=== 1. identify the source without logging in ===");
    let identity = match dbsource::probe("127.0.0.1", PORT) {
        dbsource::Probe::Listening(id) => id,
        other => panic!("sandbox server should answer: {other:?}"),
    };
    let observed = matches!(
        &identity,
        Identity::Handshake { vendor: Vendor::Mysql, version } if version.starts_with("8.4")
    );
    println!("  {identity:?} -> observed-as-MySQL-8.4: {observed}");
    ok &= observed;

    println!("\n=== 2. the gate refuses a self-import before any connection ===");
    // Pretend OUR engine runs on this port: two facts agree -> it's "us".
    let ours = vec![OurEngine { port: PORT, version: binaries::MYSQL_VERSION.into() }];
    let is_ours = dbdump::server_is_ours("127.0.0.1", PORT, &identity, &ours);
    let verdict = compat(
        &Source { vendor: Some(Vendor::Mysql), version: Version::parse("8.4.6") },
        &Target { vendor: Vendor::Mysql, version: Version::parse(binaries::MYSQL_VERSION).unwrap() },
    );
    let refused = dbdump::gate(Some((SelfImport::ThisSite, DB.into())), &verdict, false);
    println!("  server_is_ours={is_ours}; gate -> {}", match &refused {
        Err(r) => r.message(),
        Ok(_) => "CLEARED (wrong)".into(),
    });
    ok &= is_ours && refused.is_err();

    // With an empty ours-snapshot (their server, not ours), the gate clears.
    let cleared = dbdump::gate(None, &verdict, false).expect("their server clears the gate");

    println!("\n=== 3. live preflight: missing vs present database ===");
    let dest = sandbox.root().join("db-imports");
    let defaults =
        dbdump::DefaultsFile::create(&*plat, &dest, &conn_for(PORT)).expect("defaults file");
    println!("  defaults file mode: {:o} (want 600)", mode_of(defaults.path()));
    ok &= mode_of(defaults.path()) == 0o600;

    let mut missing_conn = conn_for(PORT);
    missing_conn.database = "no_such_db".into();
    let missing_defaults =
        dbdump::DefaultsFile::create(&*plat, &sandbox.root().join("df2"), &missing_conn).unwrap();
    match dbdump::preflight_live(&cleared, &client, &missing_defaults, "no_such_db").unwrap() {
        LiveCheck::DatabaseMissing { available } => {
            println!("  no_such_db -> missing; server does have {available:?}");
            ok &= available.contains(&DB.to_string());
        }
        other => {
            println!("  UNEXPECTED: {other:?}");
            ok = false;
        }
    }
    let size = match dbdump::preflight_live(&cleared, &client, &defaults, DB).unwrap() {
        LiveCheck::Ready { size, unreadable } => {
            println!("  {DB} -> ready: {} tables, {} data bytes", size.table_count, size.data_bytes);
            ok &= size.table_count == 1;
            // A healthy database reports NOTHING unreadable. Stated here so the
            // §7 finding below can't be a probe that always cries wolf.
            println!("  unreadable on a healthy database: {unreadable:?} (want [])");
            ok &= unreadable.is_empty();
            size
        }
        other => panic!("expected Ready: {other:?}"),
    };
    let preflight = dbdump::check_disk(size, &dest).expect("disk fits a tiny dump");

    println!("\n=== 4. dump -> 0600 artifact + matching manifest + real progress ===");
    let dump_tool = basedir.join("bin/mysqldump");
    let req = DumpRequest {
        tool: &dump_tool,
        tool_vendor: Vendor::Mysql,
        db: DB,
        domain: "dumpcheck.test",
        source_host: "127.0.0.1",
        source_port: PORT,
        source_vendor: Vendor::Mysql,
        source_version: "8.4.6",
        target_engine: "mysql",
        target_version: binaries::MYSQL_VERSION,
        dump_tool_label: "mysqldump 8.4.6",
        dest_dir: &dest,
        skip_tables: &[],
    };
    let mut progress_points: Vec<u64> = Vec::new();
    let outcome = dbdump::dump(
        &cleared,
        &*plat,
        &preflight,
        &req,
        &defaults,
        &AtomicBool::new(false),
        &mut |b| progress_points.push(b),
    )
    .expect("dump runs");
    let DumpOutcome::Done { artifact, manifest } = outcome else {
        panic!("not cancelled");
    };
    let body = std::fs::read_to_string(&artifact).unwrap();
    let monotonic = progress_points.windows(2).all(|w| w[0] <= w[1]);
    println!("  artifact: {} ({} bytes, mode {:o})", artifact.display(), manifest.artifact_bytes, mode_of(&artifact));
    println!("  progress points: {} (monotonic: {monotonic})", progress_points.len());
    println!("  manifest: {} tables, tool {}", manifest.table_count, manifest.dump_tool);
    ok &= mode_of(&artifact) == 0o600;
    ok &= mode_of(&dbdump::manifest_path(&dest, "dumpcheck.test")) == 0o600;
    ok &= body.contains("CREATE TABLE") && body.contains("p)ss;w(rd survives");
    ok &= monotonic && !progress_points.is_empty();
    ok &= manifest.artifact_bytes == std::fs::metadata(&artifact).unwrap().len();
    // …and load_manifest accepts exactly this artifact.
    ok &= dbdump::load_manifest(&dest, "dumpcheck.test").is_ok();

    println!("\n=== 5. cancel leaves nothing that looks like a backup ===");
    let dest2 = sandbox.root().join("db-imports-cancel");
    let defaults2 = dbdump::DefaultsFile::create(&*plat, &dest2, &conn_for(PORT)).unwrap();
    let req2 = DumpRequest { dest_dir: &dest2, ..req };
    let cancelled = dbdump::dump(
        &cleared,
        &*plat,
        &preflight,
        &req2,
        &defaults2,
        &AtomicBool::new(true), // cancelled before the first poll
        &mut |_| {},
    )
    .expect("cancel is an outcome, not an error");
    let nothing_left = !dbdump::artifact_path(&dest2, "dumpcheck.test").exists()
        && !dbdump::manifest_path(&dest2, "dumpcheck.test").exists()
        && !dest2.join("dumpcheck.test.sql.partial").exists();
    println!("  cancelled -> {}; directory holds no artifact/manifest/partial: {nothing_left}",
        matches!(cancelled, DumpOutcome::Cancelled));
    ok &= matches!(cancelled, DumpOutcome::Cancelled) && nothing_left;

    println!("\n=== 6. an interrupted dump can never be restored ===");
    // A stray .partial is not an artifact (wrong name).
    std::fs::write(dest.join("crashed.test.sql.partial"), "-- half a dump").unwrap();
    let partial_refused = dbdump::load_manifest(&dest, "crashed.test").is_err();
    // An artifact whose size no longer matches its manifest is refused.
    std::fs::OpenOptions::new().append(true).open(&artifact).unwrap();
    std::fs::write(&artifact, format!("{body}-- tampered")).unwrap();
    let stale = dbdump::load_manifest(&dest, "dumpcheck.test");
    println!("  .partial refused: {partial_refused}; size-mismatch refused: {}", stale.is_err());
    ok &= partial_refused && stale.is_err();

    println!("\n=== 7. a table the source can't read is found, and skipped ===");
    // Plant the REAL failure, not a simulation of it: DISCARD TABLESPACE is how
    // an InnoDB table loses its .ibd, and it produces the same error 1812
    // ("Tablespace is missing for table") that ended a user's 2 GB migration.
    // wp_empty is planted alongside it for the opposite reason — an EMPTY table
    // must still prove itself readable, or the probe would skip live tables
    // that simply have no rows yet.
    let plant = format!(
        "USE {DB}; \
         CREATE TABLE wp_empty (id INT PRIMARY KEY); \
         CREATE TABLE wp_broken (id INT PRIMARY KEY, note TEXT) ENGINE=InnoDB; \
         INSERT INTO wp_broken VALUES (1,'doomed'); \
         ALTER TABLE wp_broken DISCARD TABLESPACE;"
    );
    let out = std::process::Command::new(&client)
        .args(["--no-defaults", "--protocol=TCP", "--host=127.0.0.1"])
        .arg(format!("--port={PORT}"))
        .args(["--user=root", "-e", &plant])
        .output()
        .expect("plant");
    assert!(out.status.success(), "plant failed: {}", String::from_utf8_lossy(&out.stderr));
    // The plant must actually be broken, or everything below proves nothing.
    let reads = std::process::Command::new(&client)
        .args(["--no-defaults", "--protocol=TCP", "--host=127.0.0.1"])
        .arg(format!("--port={PORT}"))
        .args(["--user=root", "-e", &format!("SELECT * FROM {DB}.wp_broken")])
        .output()
        .expect("probe plant");
    // Either wording counts, and the difference is the point: a table whose file
    // was DELETED says "Tablespace is missing" (1812, what the user's database
    // said); one DISCARDed says "Tablespace has been discarded" (1814). DISCARD
    // is the only way to plant this deterministically, and the probe must not
    // care which — it never reads the error text.
    let stderr = String::from_utf8_lossy(&reads.stderr).to_string();
    let planted = !reads.status.success()
        && (stderr.contains("Tablespace is missing")
            || stderr.contains("Tablespace has been discarded"));
    println!("  planted wp_broken actually unreadable: {planted}");
    ok &= planted;

    let found = dbdump::unreadable_tables(&client, &defaults, DB).expect("probe runs");
    println!("  probe found: {found:?} (want exactly [wp_broken])");
    ok &= found == vec!["wp_broken".to_string()];
    // The empty table is READABLE — the COUNT(*) shape earning its place.
    ok &= !found.contains(&"wp_empty".to_string());
    ok &= !found.contains(&"wp_posts".to_string());

    // CONTROL: the same database, same tool, skipping NOTHING. This must fail —
    // it is what makes the leg below mean "the skip fixed it" rather than "that
    // table was harmless all along".
    let dest3 = sandbox.root().join("db-imports-broken");
    let defaults3 = dbdump::DefaultsFile::create(&*plat, &dest3, &conn_for(PORT)).unwrap();
    let size3 = match dbdump::preflight_live(&cleared, &client, &defaults3, DB).unwrap() {
        LiveCheck::Ready { size, unreadable } => {
            ok &= unreadable == vec!["wp_broken".to_string()];
            size
        }
        other => panic!("expected Ready: {other:?}"),
    };
    let preflight3 = dbdump::check_disk(size3, &dest3).expect("disk fits");
    let control = dbdump::dump(
        &cleared,
        &*plat,
        &preflight3,
        &DumpRequest { dest_dir: &dest3, skip_tables: &[], ..req },
        &defaults3,
        &AtomicBool::new(false),
        &mut |_| {},
    );
    let control_failed = control.is_err();
    println!("  control (skip nothing) -> {}", match &control {
        Err(e) => format!("failed as it must: {e}"),
        Ok(_) => "SUCCEEDED (wrong — the plant isn't biting)".into(),
    });
    ok &= control_failed;

    // THE FIX: same everything, skipping what the probe found.
    let fixed = dbdump::dump(
        &cleared,
        &*plat,
        &preflight3,
        &DumpRequest { dest_dir: &dest3, skip_tables: &found, ..req },
        &defaults3,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .expect("the dump must succeed once the unreadable table is skipped");
    let DumpOutcome::Done { artifact: a3, manifest: m3 } = fixed else { panic!("not cancelled") };
    let body3 = std::fs::read_to_string(&a3).unwrap();
    let kept_the_data = body3.contains("p)ss;w(rd survives") && body3.contains("wp_empty");
    let left_out = !body3.contains("CREATE TABLE `wp_broken`");
    println!("  dump succeeded: {} tables; healthy data present: {kept_the_data}; wp_broken absent: {left_out}", m3.table_count);
    println!("  manifest records the omission: {:?}", m3.skipped_tables);
    ok &= kept_the_data && left_out;
    // The manifest must SAY what is missing — an artifact that silently lacks a
    // table is the same data loss with none of the warning.
    ok &= m3.skipped_tables == vec!["wp_broken".to_string()];
    ok &= !m3.tables.contains(&"wp_broken".to_string());
    ok &= dbdump::load_manifest(&dest3, "dumpcheck.test").is_ok();

    mysqld.reap();
    drop(defaults);
    drop(defaults2);
    drop(defaults3);
    drop(missing_defaults);

    if ok {
        println!("\nOK — the gate runs before any connection, a dump is 0600 + manifested, and no interrupted copy can ever be mistaken for a whole one.");
    } else {
        eprintln!("\nFAILED — see above.");
        std::process::exit(1);
    }
}

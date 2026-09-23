//! Live check: a datadir written by a LEGACY-tier engine starts on the STANDARD
//! pin of the same series and still holds its rows — the forward move a macOS
//! 13 host makes the day it becomes a 15 host (`docs/PLAN-macos-13-floor.md`
//! §6.4, T6).
//!
//!   cargo run --example legacy_upgrade_check      (network tier)
//!
//! # The claim, and what it rests on
//!
//! The tier is re-derived at every launch and never stored, so an in-place OS
//! upgrade silently moves every engine to a newer pin of the SAME series —
//! MySQL 8.4.3 → 8.4.6, 8.0.40 → 8.0.44, MariaDB 12.0.2 → 12.3.2, 11.4.8 →
//! 11.4.12. That is safe only if (a) both pins resolve the SAME datadir (the
//! per-series rule keys on the series, and the default's series keeps the
//! legacy path on both tiers) and (b) the newer server accepts the older
//! server's files. (a) is asserted here from `DbEngine::data_dir` under each
//! tier; (b) is proven by doing it: init + write on the legacy binary, start the
//! standard binary on the same directory, read the row back. The reverse move
//! never happens (macOS does not downgrade), so it is not tested.
//!
//! # Fixture scope
//!
//! `common::sandbox` for the platform, so every datadir, socket and log lands
//! under a throwaway app-data root; the shared binary cache is the documented
//! exception (the legacy trees land under their own version dirs). Servers
//! listen on FIXTURE ports (23306 / 23307) — never the stack's 13306 / 13307,
//! which `db_version_switch_check` needs free and this check does not — and
//! every spawn is owned by a `Reaped` guard.

use rexenv_lib::core::binaries::{self, BinaryTier, PinSet};
use rexenv_lib::core::db::DbEngine;
use rexenv_lib::core::{database, mariadb, ports};
use rexenv_lib::platform::traits::Platform;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

mod common;

const MYSQL_FIXTURE_PORT: u16 = 23306;
const MARIADB_FIXTURE_PORT: u16 = 23307;
const MARKER: &str = "legacy-wrote-this";

struct Case {
    engine: DbEngine,
    legacy: &'static str,
    standard: &'static str,
    port: u16,
}

/// Run one SQL statement through the bundled client, return stdout (trimmed).
fn sql(client: &Path, port: u16, stmt: &str) -> Result<String, String> {
    let out = Command::new(client)
        .args(database::client_base_args(port))
        .args(["--batch", "--skip-column-names", "-e", stmt])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn wait_listening(port: u16, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(300)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    false
}

fn wait_closed(port: u16, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(300)).is_err() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    false
}

/// Resolve `version` of the engine, start it on `datadir` at the fixture port,
/// wait for it to answer. Returns the server's basedir and the guard.
async fn start(
    plat: &dyn Platform,
    case: &Case,
    version: &str,
    datadir: &Path,
) -> Result<(PathBuf, common::Reaped), String> {
    // The guard wraps the spawn on the SAME line as the spawn: a panic between
    // the two would leak a server onto the fixture port (`no_example_holds_a_spawned_service_as_a_bare_child`).
    let (basedir, guard) = match case.engine {
        DbEngine::Mysql => {
            let base = binaries::resolve_dir(plat, "mysql", version).await.map_err(|e| e.to_string())?;
            let socket = database::socket_path(plat).map_err(|e| e.to_string())?;
            if let Some(p) = socket.parent() {
                std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            database::initialize(plat, &base, datadir).map_err(|e| e.to_string())?;
            let guard = common::Reaped::new(
                database::start(plat, &base, datadir, case.port, &socket).map_err(|e| e.to_string())?,
                case.port,
                "mysqld",
            );
            (base, guard)
        }
        DbEngine::Mariadb => {
            let base = binaries::resolve_bundle(plat, "mariadb", version).await.map_err(|e| e.to_string())?;
            let socket = mariadb::socket_path(plat).map_err(|e| e.to_string())?;
            if let Some(p) = socket.parent() {
                std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            mariadb::initialize(plat, &base, datadir).map_err(|e| e.to_string())?;
            let guard = common::Reaped::new(
                mariadb::start(plat, &base, datadir, case.port, &socket).map_err(|e| e.to_string())?,
                case.port,
                "mariadbd",
            );
            (base, guard)
        }
        _ => unreachable!("only the MySQL-protocol engines carry a datadir across tiers here"),
    };
    if !wait_listening(case.port, Duration::from_secs(90)) {
        return Err(format!("{} {version} did not listen on :{} within 90s", case.engine.label(), case.port));
    }
    Ok((basedir, guard))
}

fn client_of(case: &Case, basedir: &Path) -> PathBuf {
    match case.engine {
        DbEngine::Mysql => basedir.join("bin/mysql"),
        // The bundle's client sits beside the server (`mariadb::mariadb_client_bin` is crate-private).
        _ => basedir.join("bin/mariadb"),
    }
}

async fn run_case(plat: &dyn Platform, case: &Case) -> Result<(), String> {
    let label = format!("{} {} → {}", case.engine.label(), case.legacy, case.standard);
    println!("=== {label} ===");
    ports::ensure_free(plat, case.port, ports::Proto::Tcp, case.engine.label()).map_err(|e| e.to_string())?;

    // (a) Both tiers resolve the SAME datadir for this series.
    binaries::install_tier(BinaryTier::Legacy13);
    let legacy_dir = case.engine.data_dir(plat, case.legacy).map_err(|e| e.to_string())?;
    binaries::install_tier(BinaryTier::Standard);
    let standard_dir = case.engine.data_dir(plat, case.standard).map_err(|e| e.to_string())?;
    if legacy_dir != standard_dir {
        return Err(format!(
            "datadirs differ across tiers — Legacy13 {} vs Standard {}: an OS upgrade would start an EMPTY database",
            legacy_dir.display(),
            standard_dir.display()
        ));
    }
    println!("  datadir (both tiers): {}", legacy_dir.display());

    // (b) Legacy writes…
    let (base, mut guard) = start(plat, case, case.legacy, &legacy_dir).await?;
    let client = client_of(case, &base);
    let v = sql(&client, case.port, "SELECT VERSION()")?;
    println!("  legacy server: {v}");
    if !v.contains(case.legacy) {
        return Err(format!("expected {} to be serving, got {v}", case.legacy));
    }
    sql(&client, case.port, "CREATE DATABASE rexenv_upgrade")?;
    sql(&client, case.port, "CREATE TABLE rexenv_upgrade.t (v VARCHAR(64) NOT NULL)")?;
    sql(&client, case.port, &format!("INSERT INTO rexenv_upgrade.t VALUES ('{MARKER}')"))?;
    println!("  legacy wrote 1 row");
    guard.reap();
    if !wait_closed(case.port, Duration::from_secs(30)) {
        return Err("legacy server did not release its port".into());
    }

    // …the standard pin reads, on the very same directory.
    let (base, mut guard) = start(plat, case, case.standard, &legacy_dir).await?;
    let client = client_of(case, &base);
    let v = sql(&client, case.port, "SELECT VERSION()")?;
    println!("  standard server: {v}");
    if !v.contains(case.standard) {
        return Err(format!("expected {} to be serving, got {v}", case.standard));
    }
    let got = sql(&client, case.port, "SELECT v FROM rexenv_upgrade.t")?;
    println!("  standard read: {got:?}");
    guard.reap();
    let _ = wait_closed(case.port, Duration::from_secs(30));
    if got != MARKER {
        return Err(format!("the row did not survive the upgrade: {got:?}"));
    }
    println!("  ok\n");
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    let (plat, _guard) = common::sandbox("legacy-upgrade");
    let plat: &dyn Platform = &*plat;
    let l13 = PinSet::for_tier(BinaryTier::Legacy13);
    let std = PinSet::for_tier(BinaryTier::Standard);
    // Pairs by what the OS upgrade actually does: a legacy version moves to the
    // standard version of the SAME series where one exists (MySQL 8.4.3 → 8.4.6,
    // 8.0.40 → 8.0.44, MariaDB 11.4.8 → 11.4.12), and the tier's DEFAULT moves
    // to the standard default even across a series (MariaDB 12.0.2 → 12.3.2):
    // both defaults keep the legacy `<engine>/data` path, so that directory is
    // handed from one series to the next. The datadir assertion in `run_case`
    // is what makes each pairing a proof rather than a guess.
    let mut cases: Vec<Case> = Vec::new();
    let pair = |engine: DbEngine, legacy: &'static [&'static str], legacy_default: &'static str, standard: &'static [&'static str], standard_default: &'static str, port: u16, cases: &mut Vec<Case>| {
        for lv in legacy {
            let series = engine.series_of(lv);
            let partner = standard
                .iter()
                .find(|v| engine.series_of(v) == series)
                .copied()
                .or_else(|| (*lv == legacy_default).then_some(standard_default));
            if let Some(sv) = partner {
                cases.push(Case { engine, legacy: lv, standard: sv, port });
            }
        }
    };
    pair(DbEngine::Mysql, l13.mysql_versions, l13.mysql, std.mysql_versions, std.mysql, MYSQL_FIXTURE_PORT, &mut cases);
    pair(DbEngine::Mariadb, l13.mariadb_versions, l13.mariadb, std.mariadb_versions, std.mariadb, MARIADB_FIXTURE_PORT, &mut cases);
    if cases.len() != 4 {
        eprintln!("expected 4 series pairs (MySQL 8.4, 8.0; MariaDB 12, 11.4), found {}", cases.len());
        return ExitCode::FAILURE;
    }
    let mut failed = 0;
    for case in &cases {
        if let Err(e) = run_case(plat, case).await {
            eprintln!("  FAIL {} {} → {}: {e}\n", case.engine.label(), case.legacy, case.standard);
            failed += 1;
        }
    }
    binaries::install_tier(BinaryTier::Standard);
    if failed == 0 {
        println!("legacy_upgrade_check: ALL PASS — every legacy datadir starts and reads on the standard pin of its series");
        ExitCode::SUCCESS
    } else {
        eprintln!("legacy_upgrade_check: FAILED ({failed} of {})", cases.len());
        ExitCode::FAILURE
    }
}

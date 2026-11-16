//! Live check (network tier): a PostgreSQL cluster survives the DEFAULT moving
//! to another major under it — ledger #973.
//!
//!   cargo run --example postgres_tier_move_check
//!
//! # The claim, and what it rests on
//!
//! The default PostgreSQL is a per-host pin: 16 on a macOS 14 host, 18 on 15+.
//! The tier is re-derived at every launch, so an in-place OS upgrade moves the
//! default across a MAJOR while the cluster stays where it is — the unversioned
//! `postgres/data`. A PostgreSQL major refuses another major's files, so the
//! rule "the default's series owns the unversioned path" handed a 16 cluster to
//! an 18 server (which refused it) and sent 16 to an empty `postgres/16/data`.
//!
//! Two answers changed, and both are asked here against real servers:
//!
//!   1. `DbEngine::effective_version` — a host that never chose a version runs
//!      the one its data was written by, not the new default.
//!   2. `DbEngine::data_dir` — the unversioned path belongs to the cluster in it
//!      (its `PG_VERSION`); the new default gets a directory of its own.
//!
//! The legs: PostgreSQL 16 writes a row as a Legacy14 default; the tier becomes
//! Standard; 16 starts on the SAME directory and reads the row; 18 starts on its
//! own directory and does not have the table. CONTROL, without which "it still
//! works" could mean the hazard was never real: the 18 server pointed at the 16
//! cluster — what the old rule did — refuses to start, in PostgreSQL's own words.
//!
//! # Fixture scope
//!
//! `common::sandbox` for the platform: every datadir and log is under a throwaway
//! app-data root. The binaries are resolved under the STANDARD tier, before any
//! tier is installed, so the shared cache receives only what a standard host
//! holds anyway (a legacy resolve would have written rexenv's own build under a
//! name the real app reads — ledger #972). The server listens on a FIXTURE port
//! (25432), never the stack's 15432, and every spawn is owned by a `Reaped` guard.
//! The control runs `postgres` to completion: it exits before it listens.

use rexenv_lib::core::binaries::{self, BinaryTier, PinSet};
use rexenv_lib::core::db::DbEngine;
use rexenv_lib::core::{ports, postgres};
use rexenv_lib::platform::traits::Platform;
use std::net::TcpStream;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

mod common;

const PORT: u16 = 25432;
const MARKER: &str = "written-under-the-old-default";

fn listening() -> bool {
    TcpStream::connect_timeout(&([127, 0, 0, 1], PORT).into(), Duration::from_millis(300)).is_ok()
}

fn wait_for(want: bool, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if listening() == want {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    false
}

/// One statement through the tree's own `psql`, on the production argv.
fn sql(basedir: &Path, stmt: &str) -> Result<String, String> {
    let out = Command::new(postgres::psql_bin(basedir))
        .args(postgres::psql_base_args(PORT, "postgres"))
        .args(["-At", "-c", stmt])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Initialize `datadir` if it is empty and start `basedir`'s server on it.
fn start(plat: &dyn Platform, basedir: &Path, datadir: &Path) -> Result<common::Reaped, String> {
    postgres::initialize(plat, basedir, datadir).map_err(|e| e.to_string())?;
    let guard = common::Reaped::from_proc(
        postgres::start(plat, basedir, datadir, PORT).map_err(|e| e.to_string())?,
        PORT,
        "postgres",
    );
    if !wait_for(true, Duration::from_secs(60)) {
        return Err(format!("no server on :{PORT} within 60s for {}", datadir.display()));
    }
    Ok(guard)
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    common::require_ports_free(&[(PORT, "this example's fixture PostgreSQL")]);
    let (plat, _sandbox) = common::sandbox("pg-tier-move");
    let plat: &dyn Platform = &*plat;
    let mut check = common::Check::new("postgres_tier_move_check");
    let pg = DbEngine::Postgres;

    let old = PinSet::for_tier(BinaryTier::Legacy14).postgres;
    let new = PinSet::for_tier(BinaryTier::Standard).postgres;
    check.is(
        "the fixture is a real move: the two tiers default to different majors",
        pg.series_of(old) != pg.series_of(new),
        &format!("Legacy14 {old}, Standard {new}"),
    );

    // Resolved BEFORE a tier is installed — see "Fixture scope".
    let old_base = binaries::resolve_dir(plat, "postgres", old).await.expect("the old default's tree");
    let new_base = binaries::resolve_dir(plat, "postgres", new).await.expect("the new default's tree");
    if let Err(e) = ports::ensure_free(plat, PORT, ports::Proto::Tcp, "PostgreSQL (fixture)") {
        eprintln!("{e}");
        return std::process::ExitCode::FAILURE;
    }
    let conn = common::sandbox_db(plat);
    let unversioned = postgres::data_dir(plat).expect("sandbox datadir");

    // ── 1. The old host: its default writes a cluster into the unversioned path ──
    binaries::install_tier(BinaryTier::Legacy14);
    let before_version = pg.effective_version(plat, &conn);
    let before_dir = pg.data_dir(plat, &before_version).expect("datadir");
    check.is("old host: the effective version is its default", before_version == old, &before_version);
    check.is(
        "old host: the default's cluster is the unversioned path",
        before_dir == unversioned,
        &before_dir.display().to_string(),
    );
    match start(plat, &old_base, &before_dir) {
        Ok(mut guard) => {
            let wrote = sql(&old_base, "CREATE TABLE tier_move (v text NOT NULL)")
                .and_then(|_| sql(&old_base, &format!("INSERT INTO tier_move VALUES ('{MARKER}')")));
            check.is("old host: the server took a row", wrote.is_ok(), &format!("{wrote:?}"));
            guard.reap();
            check.is("old host: the server stopped", wait_for(false, Duration::from_secs(30)), "still listening");
        }
        Err(e) => check.is("old host: the server took a row", false, &e),
    }

    // ── 2. The same disk under the new default ──────────────────────────────────
    binaries::install_tier(BinaryTier::Standard);
    check.is("new host: the default moved to another major", pg.default_version() == new, pg.default_version());
    let after_version = pg.effective_version(plat, &conn);
    check.is(
        "new host: with no version chosen, the effective version follows the cluster",
        after_version == old,
        &format!("effective {after_version} — the default is {new}, the data is {old}'s"),
    );
    let after_dir = pg.data_dir(plat, &after_version).expect("datadir");
    check.is(
        "new host: that version still resolves the SAME directory",
        after_dir == before_dir,
        &after_dir.display().to_string(),
    );
    let new_dir = pg.data_dir(plat, new).expect("datadir");
    check.is(
        "new host: the new default gets a directory of its own",
        new_dir != unversioned && new_dir.ends_with(Path::new(&pg.series_of(new)).join("data")),
        &new_dir.display().to_string(),
    );

    match start(plat, &old_base, &after_dir) {
        Ok(mut guard) => {
            let got = sql(&old_base, "SELECT v FROM tier_move").unwrap_or_else(|e| e);
            check.is("new host: the row is read back", got == MARKER, &got);
            let v = sql(&old_base, "SHOW server_version").unwrap_or_else(|e| e);
            check.is("…by a server of the cluster's own major", pg.series_of(&v) == pg.series_of(old), &v);
            guard.reap();
            check.is("new host: the server stopped", wait_for(false, Duration::from_secs(30)), "still listening");
        }
        Err(e) => check.is("new host: the row is read back", false, &e),
    }

    // The new default is usable too — on its own, empty cluster.
    if new_dir != unversioned {
        match start(plat, &new_base, &new_dir) {
            Ok(mut guard) => {
                let v = sql(&new_base, "SHOW server_version").unwrap_or_else(|e| e);
                check.is("new default: starts on its own directory", pg.series_of(&v) == pg.series_of(new), &v);
                let table = sql(&new_base, "SELECT to_regclass('tier_move') IS NULL").unwrap_or_else(|e| e);
                check.is("new default: it is a separate cluster (no such table)", table == "t", &table);
                guard.reap();
                check.is("new default: the server stopped", wait_for(false, Duration::from_secs(30)), "still listening");
            }
            Err(e) => check.is("new default: starts on its own directory", false, &e),
        }
    }

    // ── 3. CONTROL: what the old rule did ───────────────────────────────────────
    // The new default's server on the old default's cluster. It reads PG_VERSION
    // and exits before it binds, so this runs to completion with nothing to reap.
    let out = Command::new(postgres::postgres_bin(&new_base))
        .arg("-D")
        .arg(&unversioned)
        .arg("-p")
        .arg(PORT.to_string())
        .output();
    match out {
        Ok(out) => {
            let said = String::from_utf8_lossy(&out.stderr).to_string();
            check.is(
                "control: the new default's server REFUSES the old default's cluster",
                !out.status.success() && said.contains("incompatible"),
                said.lines().find(|l| l.contains("incompatible")).unwrap_or(said.trim()),
            );
        }
        Err(e) => check.is("control: the new default's server REFUSES the old default's cluster", false, &e.to_string()),
    }
    check.is("teardown: the fixture port is free", wait_for(false, Duration::from_secs(10)), "still listening");
    check.verdict()
}

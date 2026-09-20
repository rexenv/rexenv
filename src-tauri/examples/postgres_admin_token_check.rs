//! The PostgreSQL launch path, on a machine whose token the server refuses.
//!
//! `postgres.exe` will not run under an Administrators token — "Execution of
//! PostgreSQL by a user with administrative permissions is not permitted" — and
//! on Windows every process carries one when UAC is off (`EnableLUA=0`). That is
//! how it shipped broken (ledger #698): the engine died on every start, the
//! health watchdog respawned it three times and gave up, and the only thing the
//! user saw was "PostgreSQL did not start within 15s". `initdb` survives the same
//! machine because it re-executes ITSELF under a restricted token; `postgres.exe`
//! has no such code — `pg_ctl` is where PostgreSQL keeps it.
//!
//! So this runs the REAL launch path end to end and asserts the three things the
//! bug broke: the server starts, it answers a query, and stopping it FREES THE
//! PORT (a hard kill of the postmaster on Windows leaves its backend processes
//! holding the datadir and the port, which is why the stop goes through `pg_ctl`
//! too). It is written to be run on a machine with the token — the Windows VM —
//! and it passes on macOS through the direct-spawn path, which is the control.
//!
//! **Touches the REAL cluster**, like `db_engine_serve`: the datadir under
//! app-data and the engine's own port. It refuses to run unless that port is
//! free, starts what was not running, and stops it again. Run:
//! `cargo run --example postgres_admin_token_check`

mod common;

use rexenv_lib::core::db::DbEngine;
use rexenv_lib::core::{postgres, ports};
use rexenv_lib::platform;
use std::process::Command;
use std::thread;
use std::time::Duration;

fn wait_for(port: u16, want_listening: bool) -> bool {
    for _ in 0..60 {
        if ports::is_listening(port) == want_listening {
            return true;
        }
        thread::sleep(Duration::from_millis(500));
    }
    false
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let plat = platform::current();
    let engine = DbEngine::Postgres;
    let port = engine.port();
    let version = engine.default_version().to_string();

    if ports::is_listening(port) {
        eprintln!(
            "REFUSED: something already listens on :{port} — this check starts and stops the \
             real cluster, so it will not touch one it did not start"
        );
        return std::process::ExitCode::FAILURE;
    }

    println!(
        "launch path: {}",
        if plat.supervisor().may_spawn_with_admin_token() {
            "pg_ctl (this OS can hand a spawn an admin token)"
        } else {
            "direct spawn (no admin token on this OS) — the control"
        }
    );

    // The CACHED tree, never a download: this check is about how the server is
    // LAUNCHED, and `resolve_dir` would reach the network (measured: it is what
    // the run hung in on a VM with no route out).
    let basedir = plat
        .paths()
        .bin_dir()
        .map(|d| d.join(format!("postgres-{version}")))
        .ok()
        .filter(|d| d.is_dir())
        .unwrap_or_else(|| {
            eprintln!(
                "REFUSED: PostgreSQL {version} is not in the binary cache — install it from the \
                 app first; this check does not download"
            );
            std::process::exit(1);
        });
    let datadir = postgres::data_dir(&*plat).expect("data dir");
    postgres::initialize(&*plat, &basedir, &datadir).expect("initdb");

    // 1. It starts at all — the leg that failed on every attempt.
    let proc_ = match postgres::start(&*plat, &basedir, &datadir, port) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("✗ PostgreSQL did not start: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let pid = proc_.id();
    let up = wait_for(port, true);
    println!("{} pid {pid} · listening on :{port} = {up}", if up { "✓" } else { "✗" });

    // 2. It is a SERVER, not merely a process: the port could be open while the
    //    cluster is mid-recovery, and "started" has to mean "answers".
    let out = Command::new(postgres::psql_bin(&basedir))
        // `-w`: never prompt for a password. Without it psql BLOCKS on stdin
        // forever when the server asks for one, and the check hangs instead of
        // failing (measured on the VM).
        .args([
            "-w", "-h", "127.0.0.1", "-p", &port.to_string(), "-U", "postgres", "-tAc",
            "SELECT version();",
        ])
        .env("PGCONNECT_TIMEOUT", "10")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("run psql");
    let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let queried = out.status.success() && ver.starts_with("PostgreSQL");
    println!("{} psql SELECT version() → {ver}", if queried { "✓" } else { "✗" });

    // 3. Stopping FREES THE PORT. On Windows the postmaster's backends are
    //    separate processes: terminate the postmaster and they survive, holding
    //    the datadir and the port, and the next start fails on a cluster that
    //    never checkpointed.
    if let Err(e) = engine.stop(&*plat, pid, Some(&version)) {
        eprintln!("✗ stop failed: {e}");
        return std::process::ExitCode::FAILURE;
    }
    let freed = wait_for(port, false);
    println!("{} the port is free again after the stop", if freed { "✓" } else { "✗" });

    if up && queried && freed {
        println!("\npostgres_admin_token_check: PASS");
        std::process::ExitCode::SUCCESS
    } else {
        println!("\npostgres_admin_token_check: FAIL");
        std::process::ExitCode::FAILURE
    }
}

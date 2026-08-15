//! Ledger #104/#191: the php-fpm settings gate runs against a CANDIDATE file
//! the live pool never reads — proven against the real binary, not the path
//! strings. Run: `cargo run --example fpm_candidate_check`   (sandbox tier)
//!
//! The claim ("a bad value can never brick a pool: the `-t` gate validates a
//! `.conf.candidate` the running pool never reads") was ✅ only as SHAPE —
//! two different paths returned by two functions. Whether php-fpm actually
//! honours that separation is the binary's fact: `-y <conf>` could be
//! ignored, a glob could sweep the candidate in, the candidate writer could
//! clobber the real file. So:
//!
//!   1. a real pool runs from the REAL conf on the fixture port;
//!   2. a candidate is written beside it carrying a DIFFERENT port and a
//!      marker — if anything ever reads the candidate, the pool visibly
//!      binds the wrong port; the real conf must stay byte-identical;
//!   3. `-t` passes on the production-written candidate (gate-positive
//!      control) and FAILS once the candidate is corrupted (gate-negative
//!      control — a gate that cannot fail validates nothing), while the
//!      live pool keeps serving through both;
//!   4. a full stop + restart from the real conf binds the ORIGINAL port,
//!      with the poison candidate still on disk — the restart path reads
//!      the real file, never the candidate.

use rexenv_lib::core::{binaries, services};
use std::fs;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

mod common;
use common::Reaped;

const POOL_PORT: u16 = 9796; // fixture — claimed in common/mod.rs
const POISON_PORT: u16 = 9797; // fixture — bound by NOTHING unless the candidate leaks

fn wait_listening(port: u16, up: bool) -> bool {
    for _ in 0..40 {
        if services::fpm_running(port) == up {
            return true;
        }
        thread::sleep(Duration::from_millis(250));
    }
    services::fpm_running(port) == up
}

#[tokio::main]
async fn main() -> ExitCode {
    common::require_ports_free(&[
        (POOL_PORT, "the fixture php-fpm pool"),
        (POISON_PORT, "the candidate's poison port — a listener here fakes a leak"),
    ]);
    let (plat, _sandbox) = common::sandbox("fpm_candidate_check");
    let mut checks = common::Check::new("fpm_candidate_check");

    let fpm_bin =
        binaries::resolve(&*plat, "php-fpm", binaries::PHP_VERSION).await.expect("php-fpm");

    // 1. The real pool, from the real conf.
    let conf = services::write_fpm_config(&*plat, "8.3", POOL_PORT, None, &[]).unwrap();
    let real_before = fs::read(&conf).unwrap();
    let fpm = services::start_fpm(&*plat, &fpm_bin, &conf).expect("start php-fpm");
    let mut fpm = Reaped::new(fpm, POOL_PORT, "php-fpm");
    checks.is("control: the pool serves from the real conf", wait_listening(POOL_PORT, true), "never bound");

    // 2. The candidate, via the PRODUCTION writer — different port + marker.
    let candidate = services::write_fpm_config_candidate(
        &*plat,
        "8.3",
        POISON_PORT,
        &[("memory_limit".into(), "256M".into())],
    )
    .unwrap();
    checks.is(
        "the candidate is its own file, not the real conf",
        candidate != conf,
        &format!("{} == {}", candidate.display(), conf.display()),
    );
    checks.is(
        "writing the candidate left the real conf byte-identical",
        fs::read(&conf).unwrap() == real_before,
        "the candidate writer touched the live file",
    );

    // 3a. Gate-positive: the production candidate passes the real `-t`.
    checks.is(
        "php-fpm -t accepts the production-written candidate",
        services::test_fpm_config(&*plat, &fpm_bin, &candidate).is_ok(),
        "a valid candidate failed the gate",
    );
    // 3b. Gate-negative: a corrupted candidate FAILS — the control that makes
    //     every later 'gate passed' non-vacuous.
    let mut poisoned = fs::read_to_string(&candidate).unwrap();
    poisoned.push_str("\npm = \n"); // a directive php-fpm rejects
    fs::write(&candidate, &poisoned).unwrap();
    checks.is(
        "php-fpm -t REJECTS the corrupted candidate",
        services::test_fpm_config(&*plat, &fpm_bin, &candidate).is_err(),
        "the gate cannot fail — it validates nothing",
    );
    checks.is(
        "the live pool served through both gate runs",
        services::fpm_running(POOL_PORT),
        "-t touched the running pool",
    );
    checks.is(
        "the real conf is still byte-identical after both gate runs",
        fs::read(&conf).unwrap() == real_before,
        "-t (or the corruption) reached the live file",
    );

    // 4. Restart from the real conf with the POISON candidate still on disk:
    //    the pool must come back on ITS port, and the poison port stays empty.
    fpm.reap();
    checks.is("pool stopped for the restart", wait_listening(POOL_PORT, false), "still listening");
    let fpm2 = services::start_fpm(&*plat, &fpm_bin, &conf).expect("restart php-fpm");
    let mut fpm2 = Reaped::new(fpm2, POOL_PORT, "php-fpm");
    checks.is(
        "restarted pool binds the REAL conf's port",
        wait_listening(POOL_PORT, true),
        "never came back",
    );
    checks.is(
        "the poison candidate's port was never bound (the live path never reads it)",
        !services::fpm_running(POISON_PORT),
        "something read the candidate: the pool is on the candidate's port",
    );
    fpm2.reap();
    checks.verdict()
}

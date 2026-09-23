//! The busy-workers note on real pools (ledger #608, plan §3 D1(b)): `core::pool_busy` fed with
//! `ProcessSupervisor::established_on` samples, as `commands::services::enriched_status` feeds it.
//!
//! ```text
//! cargo run --example pool_busy_check                                            # macOS, php-fpm
//! scripts/probes/windows-example.sh dell@<host> pool_busy_check                  # Windows, php-cgi
//! ```
//!
//! What it proves, one pool on a FIXTURE port, sampled every second (the Services screen polls
//! every two): idle, the tracker never turns busy; with workers + 2 requests sleeping 15 s it
//! turns busy — within the sleep, after `SAMPLES_TO_CHANGE` agreeing samples — and turns free
//! again once the requests have completed; the whole sample series is printed. The plan's "Done
//! when" said `sleep(5)`: measured, macOS php-fpm (`pm = dynamic`) takes several seconds to spawn
//! all ten workers (4 at 2 s, 10 at 8 s), so a 5-second burst never holds all ten there — 15 s does.
//!
//! Does NOT prove the Services row or the health-log line — the row is the WebKit check's, the
//! wiring in `enriched_status` is read, not driven.
//!
//! Fixture-owned: macOS `common::sandbox` + a php-fpm pool on port 9795 held by `Reaped`; Windows a
//! sandboxed platform under `%TEMP%\rexenv-pool-busy-check` and a php-cgi group on the same port
//! held by `OwnedService`. The binary cache is the documented exception. `demo` tier.

use rexenv_lib::core::pool_busy::{BusyTracker, Change};
use rexenv_lib::core::{php, services};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::ExitCode;
use std::time::{Duration, Instant};

mod common;

const PORT: u16 = 9795; // fixture — never a pool's production port
const POOL: &str = "PHP-FPM 8.3";

#[tokio::main]
async fn main() -> ExitCode {
    let mut check = common::Check::new("pool_busy_check");
    let root = std::env::temp_dir().join("rexenv-pool-busy-check");
    let _ = std::fs::remove_dir_all(&root);
    let www = root.join("www");
    std::fs::create_dir_all(&www).unwrap();
    let sleep_php = www.join("sleep.php");
    std::fs::write(&sleep_php, "<?php sleep(15); echo 'slept';").unwrap();
    let sleep_script = sleep_php.display().to_string();

    let Some(pool) = start_pool(&mut check).await else {
        let _ = std::fs::remove_dir_all(&root);
        return check.verdict();
    };
    let sup = pool.1.supervisor();
    let workers = php::pool_workers(sup.php_pool_model());
    common::await_listening(PORT, "pool", None);
    let mut tracker = BusyTracker::default();

    let mut idle_changes = Vec::new();
    for _ in 0..3 {
        idle_changes.push(tracker.observe(POOL, sup.established_on(PORT), workers));
        std::thread::sleep(Duration::from_secs(1));
    }
    check.is("idle: never busy", idle_changes.iter().all(|c| *c == Change::None) && !tracker.is_busy(POOL), &format!("{idle_changes:?}"));

    let requests = workers as usize + 2;
    let started = Instant::now();
    let sleepers: Vec<_> = (0..requests)
        .map(|_| {
            let script = sleep_script.clone();
            std::thread::spawn(move || fastcgi_get(PORT, &script, 60))
        })
        .collect();
    let (mut busy_at, mut free_at, mut series) = (None, None, Vec::new());
    while started.elapsed() < Duration::from_secs(40) {
        let held = sup.established_on(PORT);
        let change = tracker.observe(POOL, held, workers);
        series.push(format!("{:.0}s:{}", started.elapsed().as_secs_f64(), held.map_or("?".into(), |n| n.to_string())));
        match change {
            Change::Busy { .. } if busy_at.is_none() => busy_at = Some(started.elapsed()),
            Change::Free if busy_at.is_some() => {
                free_at = Some(started.elapsed());
                break;
            }
            _ => {}
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    println!("  · {requests} requests on {workers} workers — held per second: {}", series.join(" "));
    println!("  · busy at {busy_at:?}, free at {free_at:?}");
    check.is("busy: the note turns on while every worker holds a request", busy_at.is_some_and(|t| t < Duration::from_secs(15)), &format!("{busy_at:?}"));
    let done = sleepers.into_iter().filter_map(|t| t.join().ok()).filter(|b| b.contains("slept")).count();
    check.is("free: the note turns off once the requests have completed", free_at.is_some() && !tracker.is_busy(POOL), &format!("{free_at:?}"));
    check.is("every request completed", done == requests, &format!("{done}/{requests}"));
    check.is("the worker count read is the configured one", workers == 10, &workers.to_string());
    let _ = services::fpm_running(PORT);

    stop_pool(pool);
    let _ = std::fs::remove_dir_all(&root);
    check.verdict()
}

#[cfg(unix)]
type Pool = (common::Reaped, Box<dyn rexenv_lib::platform::traits::Platform>, common::SandboxGuard);

#[cfg(unix)]
async fn start_pool(check: &mut common::Check) -> Option<Pool> {
    use rexenv_lib::core::binaries;
    let (plat, guard) = common::sandbox("poolbusy");
    let fpm = binaries::resolve(&*plat, "php-fpm", binaries::pins().php).await.ok()?;
    let conf = services::write_fpm_config(&*plat, "8.3", PORT, None, &[]).ok()?;
    let child = services::start_fpm(&*plat, &fpm, &conf);
    check.is("php-fpm starts on the fixture port", child.is_ok(), &format!("{:?}", child.as_ref().err()));
    Some((common::Reaped::new(child.ok()?, PORT, "php-fpm"), plat, guard))
}

#[cfg(unix)]
fn stop_pool(mut pool: Pool) {
    pool.0.reap();
}

#[cfg(windows)]
type Pool = (common::OwnedService, Box<dyn rexenv_lib::platform::traits::Platform>);

#[cfg(windows)]
async fn start_pool(check: &mut common::Check) -> Option<Pool> {
    use rexenv_lib::core::{binaries, php_cgi};
    use rexenv_lib::platform::traits::PoolModel;
    common::require_stack_stopped();
    rexenv_lib::core::stack_guard::allow_real_stack_control();
    let plat = common::sandbox_platform_at(std::env::temp_dir().join("rexenv-pool-busy-check").join("app"));
    let PoolModel::CgiGroup(group) = plat.supervisor().php_pool_model() else { return None };
    let dir = binaries::resolve_dir(&*plat, "php", binaries::pins().php).await.ok()?;
    let child = php_cgi::start_group(&*plat, &group, &dir, "8.3", PORT, None, &[]);
    check.is("the php-cgi group starts on the fixture port", child.is_ok(), &format!("{:?}", child.as_ref().err()));
    Some((common::OwnedService::new(child.ok()?, "php-cgi group"), plat))
}

#[cfg(windows)]
fn stop_pool(mut pool: Pool) {
    pool.0.stop();
}

fn fastcgi_get(port: u16, script: &str, timeout_secs: u64) -> String {
    let record = |kind: u8, body: &[u8]| {
        let mut r = vec![1, kind, 0, 1, (body.len() >> 8) as u8, body.len() as u8, 0, 0];
        r.extend_from_slice(body);
        r
    };
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else { return "no connection".into() };
    let _ = s.set_read_timeout(Some(Duration::from_secs(timeout_secs)));
    let mut params = Vec::new();
    for (k, v) in [("SCRIPT_FILENAME", script), ("REQUEST_METHOD", "GET"), ("SCRIPT_NAME", "/sleep.php"),
                   ("QUERY_STRING", ""), ("SERVER_PROTOCOL", "HTTP/1.1"), ("GATEWAY_INTERFACE", "CGI/1.1"), ("REDIRECT_STATUS", "200")] {
        for len in [k.len(), v.len()] {
            if len < 128 { params.push(len as u8) } else { params.extend_from_slice(&((len as u32) | 0x8000_0000).to_be_bytes()) }
        }
        params.extend_from_slice(k.as_bytes());
        params.extend_from_slice(v.as_bytes());
    }
    let mut out = record(1, &[0, 1, 0, 0, 0, 0, 0, 0]);
    out.extend(record(4, &params));
    out.extend(record(4, &[]));
    out.extend(record(5, &[]));
    if s.write_all(&out).is_err() {
        return "write failed".into();
    }
    let mut stdout = Vec::new();
    let mut head = [0u8; 8];
    while s.read_exact(&mut head).is_ok() {
        let len = ((head[4] as usize) << 8) | head[5] as usize;
        let mut body = vec![0u8; len + head[6] as usize];
        if s.read_exact(&mut body).is_err() {
            break;
        }
        match head[1] {
            6 => stdout.extend_from_slice(&body[..len]),
            3 => break,
            _ => {}
        }
    }
    String::from_utf8_lossy(&stdout).into_owned()
}

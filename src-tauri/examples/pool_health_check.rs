//! The pool health gate on real pools (ledger #607, plan §3 D1(a)): `PhpFpmPools::reap_dead` reads
//! a pool as serving when it answers FastCGI `GET_VALUES` OR holds connections on its port.
//!
//! ```text
//! cargo run --example pool_health_check                                          # macOS, php-fpm
//! scripts/probes/windows-example.sh dell@<host> pool_health_check                # Windows, php-cgi
//! ```
//!
//! What it proves, one pool on a FIXTURE port ADOPTED into `PhpFpmPools` (the path a relaunched app
//! takes; an adopted pool is identified by its command line, so this port is allowed):
//!
//! - readiness: `services::pool_answers` turns true once a worker is up;
//! - idle: two `reap_dead` polls, no reap;
//! - every worker busy (12 requests sleeping 15 s, 10 workers): the pool does NOT answer
//!   `GET_VALUES` and holds connections, and three `reap_dead` polls across the sleep reap nothing;
//!   all 12 requests then complete — the restart this gate exists to prevent would have cut them;
//! - macOS only, frozen with nothing held (`SIGSTOP`): no answer, nothing held, the first poll
//!   keeps it (miss 1) and the second reaps it.
//!
//! Fixture-owned: macOS `common::sandbox` + a php-fpm pool on port 9794 held by `Reaped`; Windows
//! a sandboxed platform under `%TEMP%\rexenv-pool-health-check` and a php-cgi group on the same
//! port held by `OwnedService`. The binary cache is the documented exception. `demo` tier.

use rexenv_lib::core::php::PhpFpmPools;
use rexenv_lib::core::services;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::ExitCode;
use std::time::{Duration, Instant};

mod common;

const PORT: u16 = 9794; // fixture — never a pool's production port
const WORKERS: usize = 10;

#[tokio::main]
async fn main() -> ExitCode {
    let mut check = common::Check::new("pool_health_check");
    let root = std::env::temp_dir().join("rexenv-pool-health-check");
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
    let plat = &*pool.1;
    let deadline = Instant::now() + Duration::from_secs(20);
    while !services::pool_answers(PORT, Duration::from_millis(500)) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
    }
    check.is("readiness: the pool answers GET_VALUES once a worker is up", services::pool_answers(PORT, Duration::from_millis(500)), "no answer in 20 s");

    let mut pools = PhpFpmPools::default();
    pools.adopt("8.3", PORT, pool.0.id(), false);
    let idle: Vec<_> = (0..2).flat_map(|_| pools.reap_dead(plat)).collect();
    check.is("idle: two health polls reap nothing", idle.is_empty() && pools.has("8.3", false), &format!("{idle:?}"));

    let started = Instant::now();
    let sleepers: Vec<_> = (0..WORKERS + 2)
        .map(|_| {
            let script = sleep_script.clone();
            std::thread::spawn(move || fastcgi_get(PORT, &script, 60))
        })
        .collect();
    std::thread::sleep(Duration::from_secs(3));
    let held = plat.supervisor().established_on(PORT);
    let answers = services::pool_answers(PORT, Duration::from_secs(1));
    println!("  · busy at {:.0} s: established {held:?}, GET_VALUES answered {answers}", started.elapsed().as_secs_f64());
    check.is("busy: the pool does not answer GET_VALUES and holds connections", !answers && held.is_some_and(|n| n > 0), &format!("answers {answers}, held {held:?}"));
    let mut reaped = Vec::new();
    for _ in 0..3 {
        reaped.extend(pools.reap_dead(plat));
        std::thread::sleep(Duration::from_secs(3));
    }
    check.is("busy: three health polls across the sleep reap nothing", reaped.is_empty() && pools.has("8.3", false), &format!("{reaped:?}"));
    let done = sleepers.into_iter().filter_map(|t| t.join().ok()).filter(|b| b.contains("slept")).count();
    println!("  · {done}/{} sleepers completed after {:.0} s", WORKERS + 2, started.elapsed().as_secs_f64());
    check.is("every busy request completed — nothing cut them", done == WORKERS + 2, &format!("{done}"));

    frozen_phase(&mut check, &pool, &mut pools);

    stop_pool(pool);
    let _ = std::fs::remove_dir_all(&root);
    check.verdict()
}

#[cfg(unix)]
type Pool = (common::Reaped, Box<dyn rexenv_lib::platform::traits::Platform>, common::SandboxGuard);

#[cfg(unix)]
async fn start_pool(check: &mut common::Check) -> Option<Pool> {
    use rexenv_lib::core::binaries;
    let (plat, guard) = common::sandbox("poolhealth");
    let fpm = binaries::resolve(&*plat, "php-fpm", binaries::pins().php).await.ok()?;
    let conf = services::write_fpm_config(&*plat, "8.3", PORT, None, &[]).ok()?;
    let child = services::start_fpm(&*plat, &fpm, &conf);
    check.is("php-fpm starts on the fixture port", child.is_ok(), &format!("{:?}", child.as_ref().err()));
    Some((common::Reaped::new(child.ok()?, PORT, "php-fpm"), plat, guard))
}

#[cfg(unix)]
fn frozen_phase(check: &mut common::Check, pool: &Pool, pools: &mut PhpFpmPools) {
    let master = pool.0.id();
    let kids = std::process::Command::new("pgrep").args(["-P", &master.to_string()]).output();
    let mut pids: Vec<String> = kids
        .map(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().map(str::to_string).collect())
        .unwrap_or_default();
    pids.push(master.to_string());
    let signal = |sig: &str| {
        let _ = std::process::Command::new("kill").arg(sig).args(&pids).status();
    };
    signal("-STOP");
    std::thread::sleep(Duration::from_millis(500));
    let held = pool.1.supervisor().established_on(PORT);
    let answers = services::pool_answers(PORT, Duration::from_secs(1));
    println!("  · frozen: established {held:?}, GET_VALUES answered {answers}");
    let first = pools.reap_dead(&*pool.1);
    check.is("frozen with nothing held: the first poll keeps it (one miss)", first.is_empty() && pools.has("8.3", false), &format!("{first:?}"));
    // The reap stops the pool; a stopped process only acts on its signals once continued.
    let second = pools.reap_dead(&*pool.1);
    signal("-CONT");
    check.is("frozen with nothing held: the second poll reaps it", second == vec![("8.3".to_string(), false)], &format!("{second:?}"));
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
    let plat = common::sandbox_platform_at(std::env::temp_dir().join("rexenv-pool-health-check").join("app"));
    let PoolModel::CgiGroup(group) = plat.supervisor().php_pool_model() else { return None };
    let dir = binaries::resolve_dir(&*plat, "php", binaries::pins().php).await.ok()?;
    let child = php_cgi::start_group(&*plat, &group, &dir, "8.3", PORT, None, &[]);
    check.is("the php-cgi group starts on the fixture port", child.is_ok(), &format!("{:?}", child.as_ref().err()));
    Some((common::OwnedService::new(child.ok()?, "php-cgi group"), plat))
}

#[cfg(windows)]
fn frozen_phase(_check: &mut common::Check, _pool: &Pool, _pools: &mut PhpFpmPools) {
    println!("  · frozen — skipped on Windows (no SIGSTOP; the idle-silent reap is macOS's run and the L0's)");
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

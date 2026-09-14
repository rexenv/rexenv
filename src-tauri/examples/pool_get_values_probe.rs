//! Can a FastCGI `GET_VALUES` round trip be a PHP pool's health probe? Measured before it is
//! built (docs/TODO.md W4, plan §3 D1(a) "a serving check, not a process check").
//!
//! ```text
//! cargo run --example pool_get_values_probe                                      # macOS, php-fpm
//! scripts/probes/windows-example.sh dell@<host> pool_get_values_probe            # Windows, php-cgi
//! ```
//!
//! A PROBE, not a proof: it prints what each state looks like to the two probes. Why it exists:
//! PHP answers `GET_VALUES` inside `fcgi_read_request`, which a WORKER runs after `accept()`
//! (main/fastcgi.c, the same code for php-fpm and php-cgi) — so a pool whose workers are all busy
//! may not answer at all, and a health probe that reads that as death would restart a pool in the
//! middle of real requests. Three states, one pool on a FIXTURE port:
//!
//! 0. idle — `GET_VALUES` latency over several tries, and the TCP connect `services::fpm_running`
//!    uses today;
//! 1. every worker busy — 12 parallel requests to a script that sleeps 12 s (10 workers): the TCP
//!    connect, and whether `GET_VALUES` answers within 3 s; then again once the sleeps end;
//! 2. frozen (macOS only) — master and workers `SIGSTOP`ped: the TCP connect and `GET_VALUES`, then
//!    again after `SIGCONT`. The state a TCP connect cannot see and `GET_VALUES` should.
//!
//! Fixture-owned: macOS `common::sandbox` + a php-fpm pool on port 9793 held by `Reaped`; Windows a
//! sandboxed platform under `%TEMP%\rexenv-get-values-probe` and a php-cgi group on the same port
//! held by `OwnedService`. The binary cache is the documented exception. `demo` tier.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::ExitCode;
use std::time::{Duration, Instant};

mod common;

const PORT: u16 = 9793; // fixture — never a pool's production port
const WORKERS: usize = 10;

#[tokio::main]
async fn main() -> ExitCode {
    let mut check = common::Check::new("pool_get_values_probe");
    let root = std::env::temp_dir().join("rexenv-get-values-probe");
    let _ = std::fs::remove_dir_all(&root);
    let www = root.join("www");
    std::fs::create_dir_all(&www).unwrap();
    let (ok_php, sleep_php) = (www.join("ok.php"), www.join("sleep.php"));
    std::fs::write(&ok_php, "<?php echo 'ok';").unwrap();
    std::fs::write(&sleep_php, "<?php sleep(12); echo 'slept';").unwrap();
    let sleep_script = sleep_php.display().to_string();
    let ok_script = ok_php.display().to_string();

    let pool = match start_pool(&mut check).await {
        Some(p) => p,
        None => {
            let _ = std::fs::remove_dir_all(&root);
            return check.verdict();
        }
    };
    common::await_listening(PORT, "pool", None);
    check.is("the pool answers a request", fastcgi_get(PORT, &ok_script, 10).contains("ok"), "no answer");

    // The busy-workers count the ruling puts in front of GET_VALUES (plan §3 D1(b)).
    let established = |p: &Pool| {
        p.1.supervisor().established_on(PORT).map_or_else(|| "unreadable".to_string(), |n| n.to_string())
    };

    println!("## 0. idle");
    let times: Vec<String> = (0..5).map(|_| describe(get_values(Duration::from_secs(3)))).collect();
    println!("  TCP connect {} · GET_VALUES {} · established {}", tcp_connect(), times.join(", "), established(&pool));

    println!("## 1. every worker busy ({} × sleep(12), {WORKERS} workers)", WORKERS + 2);
    let started = Instant::now();
    let sleepers: Vec<_> = (0..WORKERS + 2)
        .map(|_| {
            let script = sleep_script.clone();
            std::thread::spawn(move || fastcgi_get(PORT, &script, 40))
        })
        .collect();
    for wait in [2, 3] {
        std::thread::sleep(Duration::from_secs(wait));
        // Counted BEFORE the GET_VALUES connection is opened, so it is only the sleepers.
        let count = established(&pool);
        println!(
            "  at {:.0} s: established {count} · TCP connect {} · GET_VALUES {}",
            started.elapsed().as_secs_f64(),
            tcp_connect(),
            describe(get_values(Duration::from_secs(3)))
        );
    }
    let answered = sleepers.into_iter().filter_map(|t| t.join().ok()).filter(|b| b.contains("slept")).count();
    println!("  sleepers answered: {answered}/{} after {:.0} s", WORKERS + 2, started.elapsed().as_secs_f64());
    std::thread::sleep(Duration::from_millis(500));
    println!("  after the sleeps: established {} · GET_VALUES {}", established(&pool), describe(get_values(Duration::from_secs(3))));

    frozen_phase(&pool);

    stop_pool(pool);
    std::thread::sleep(Duration::from_millis(800));
    check.is("the pool port is free after the stop", !tcp_connect(), "still accepting");
    let _ = std::fs::remove_dir_all(&root);
    check.verdict()
}

fn describe(r: Result<Duration, String>) -> String {
    match r {
        Ok(d) => format!("answered in {} ms", d.as_millis()),
        Err(e) => format!("NO ANSWER ({e})"),
    }
}

fn tcp_connect() -> bool {
    TcpStream::connect_timeout(&([127, 0, 0, 1], PORT).into(), Duration::from_millis(300)).is_ok()
}

// ── the pool, per OS ──

#[cfg(unix)]
type Pool = (common::Reaped, Box<dyn rexenv_lib::platform::traits::Platform>, common::SandboxGuard);

#[cfg(unix)]
async fn start_pool(check: &mut common::Check) -> Option<Pool> {
    use rexenv_lib::core::{binaries, services};
    let (plat, guard) = common::sandbox("getvalues");
    let fpm = binaries::resolve(&*plat, "php-fpm", binaries::PHP_VERSION).await.ok()?;
    let conf = services::write_fpm_config(&*plat, "8.3", PORT, None, &[]).ok()?;
    let child = services::start_fpm(&*plat, &fpm, &conf);
    check.is("php-fpm starts on the fixture port", child.is_ok(), &format!("{:?}", child.as_ref().err()));
    Some((common::Reaped::new(child.ok()?, PORT, "php-fpm"), plat, guard))
}

#[cfg(unix)]
fn frozen_phase(pool: &Pool) {
    let master = pool.0.id();
    let kids = std::process::Command::new("pgrep").args(["-P", &master.to_string()]).output();
    let mut pids: Vec<String> = kids
        .map(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().map(str::to_string).collect())
        .unwrap_or_default();
    pids.push(master.to_string());
    println!("## 2. frozen (SIGSTOP on master + {} workers)", pids.len() - 1);
    let signal = |sig: &str| {
        let _ = std::process::Command::new("kill").arg(sig).args(&pids).status();
    };
    signal("-STOP");
    std::thread::sleep(Duration::from_millis(500));
    println!("  frozen: TCP connect {} · GET_VALUES {}", tcp_connect(), describe(get_values(Duration::from_secs(3))));
    signal("-CONT");
    std::thread::sleep(Duration::from_millis(500));
    println!("  resumed: TCP connect {} · GET_VALUES {}", tcp_connect(), describe(get_values(Duration::from_secs(3))));
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
    let plat = common::sandbox_platform_at(std::env::temp_dir().join("rexenv-get-values-probe").join("app"));
    let PoolModel::CgiGroup(group) = plat.supervisor().php_pool_model() else { return None };
    let dir = binaries::resolve_dir(&*plat, "php", binaries::PHP_VERSION).await.ok()?;
    let child = php_cgi::start_group(&*plat, &group, &dir, "8.3", PORT, None, &[]);
    check.is("the php-cgi group starts on the fixture port", child.is_ok(), &format!("{:?}", child.as_ref().err()));
    Some((common::OwnedService::new(child.ok()?, "php-cgi group"), plat))
}

#[cfg(windows)]
fn frozen_phase(_pool: &Pool) {
    println!("## 2. frozen — skipped on Windows (no SIGSTOP; measured on macOS)");
}

#[cfg(windows)]
fn stop_pool(mut pool: Pool) {
    pool.0.stop();
}

// ── FastCGI ──

fn record(kind: u8, request: u16, body: &[u8]) -> Vec<u8> {
    let mut r = vec![1, kind, (request >> 8) as u8, request as u8, (body.len() >> 8) as u8, body.len() as u8, 0, 0];
    r.extend_from_slice(body);
    r
}

fn pair(out: &mut Vec<u8>, k: &str, v: &str) {
    for len in [k.len(), v.len()] {
        if len < 128 {
            out.push(len as u8)
        } else {
            out.extend_from_slice(&((len as u32) | 0x8000_0000).to_be_bytes())
        }
    }
    out.extend_from_slice(k.as_bytes());
    out.extend_from_slice(v.as_bytes());
}

/// One `FCGI_GET_VALUES` (type 9, request id 0) round trip: the time to a `GET_VALUES_RESULT`.
fn get_values(timeout: Duration) -> Result<Duration, String> {
    let started = Instant::now();
    let mut s = TcpStream::connect_timeout(&([127, 0, 0, 1], PORT).into(), timeout).map_err(|e| format!("connect: {e}"))?;
    let _ = s.set_read_timeout(Some(timeout));
    let mut body = Vec::new();
    for name in ["FCGI_MAX_CONNS", "FCGI_MAX_REQS", "FCGI_MPXS_CONNS"] {
        pair(&mut body, name, "");
    }
    s.write_all(&record(9, 0, &body)).map_err(|e| format!("write: {e}"))?;
    let mut head = [0u8; 8];
    s.read_exact(&mut head).map_err(|e| format!("read after {} ms: {e}", started.elapsed().as_millis()))?;
    if head[1] != 10 {
        return Err(format!("record type {}", head[1]));
    }
    Ok(started.elapsed())
}

fn fastcgi_get(port: u16, script: &str, timeout_secs: u64) -> String {
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else { return "no connection".into() };
    let _ = s.set_read_timeout(Some(Duration::from_secs(timeout_secs)));
    let mut params = Vec::new();
    for (k, v) in [("SCRIPT_FILENAME", script), ("REQUEST_METHOD", "GET"), ("SCRIPT_NAME", "/probe.php"),
                   ("QUERY_STRING", ""), ("SERVER_PROTOCOL", "HTTP/1.1"), ("GATEWAY_INTERFACE", "CGI/1.1"), ("REDIRECT_STATUS", "200")] {
        pair(&mut params, k, v);
    }
    let mut out = record(1, 1, &[0, 1, 0, 0, 0, 0, 0, 0]);
    out.extend(record(4, 1, &params));
    out.extend(record(4, 1, &[]));
    out.extend(record(5, 1, &[]));
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

//! W4's churn breaker on a real Windows machine (ledger #605, plan §3 D1(a)).
//!
//! ```text
//! scripts/probes/windows-example.sh dell@<host> windows_cgi_breaker_check
//! ```
//!
//! What it proves there, ticking `PhpFpmPools::trip_spinning` every 10 s as the health
//! watchdog does:
//!
//! - **Never on legitimate load:** a group started by `PhpFpmPools::ensure("8.3")` (the spawned
//!   path, its CPU baseline set at the spawn) under one client's requests as fast as it sends
//!   them — workers recycled every 500 — survives three ticks and still answers.
//! - **Never on a script that kills its own worker:** three more ticks of that, the same.
//! - **A spin is stopped:** a group started from a COPY of the PHP tree and ADOPTED (the path a
//!   relaunched app takes — judged only while its command line names its ini) has its
//!   php-cgi.exe renamed and one worker killed; the next tick stops it, quoting the group's own
//!   `unable to spawn` line, no php-cgi of the group is left, the port is free and the output
//!   log stops growing.
//!
//! Does NOT prove the watchdog's `gave-up` event or that nothing restarts the group — those are
//! `ServiceManager::reconcile_health` wiring, held by an L0 source guard.
//!
//! Fixture-owned: a sandboxed platform under `%TEMP%\rexenv-cgi-breaker-check` (removed at the
//! end; the renamed exe is the fixture's copy), the pool's fixed port on a machine with no rexenv
//! stack (`require_stack_stopped`), the copy's group held by `OwnedService`. The binary cache is
//! the documented exception, resolved and never modified. `demo` tier: Windows-only; on macOS it
//! prints a skip line.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_cgi_breaker_check: skipped — a Windows live check (ledger #605)");
}

#[cfg(target_os = "windows")]
#[tokio::main]
async fn main() -> std::process::ExitCode {
    windows::main().await
}

#[cfg(target_os = "windows")]
mod windows {
    use super::common::{self, Check, OwnedService};
    use rexenv_lib::core::php::{fpm_port, PhpFpmPools};
    use rexenv_lib::core::ports::{self, Proto};
    use rexenv_lib::core::{binaries, php_cgi};
    use rexenv_lib::platform::traits::PoolModel;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::path::Path;
    use std::process::ExitCode;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    const MINOR: &str = "8.3";
    /// The health watchdog's interval (`lib.rs`).
    const TICK: Duration = Duration::from_secs(10);

    pub async fn main() -> ExitCode {
        common::require_stack_stopped();
        rexenv_lib::core::stack_guard::allow_real_stack_control();
        let mut check = Check::new("windows_cgi_breaker_check");
        let root = std::env::temp_dir().join("rexenv-cgi-breaker-check");
        let _ = std::fs::remove_dir_all(&root);
        let plat = common::sandbox_platform_at(root.clone());
        let sup = plat.supervisor();
        let PoolModel::CgiGroup(group) = sup.php_pool_model() else {
            check.is("this platform names the php-cgi group model", false, "Fpm");
            return check.verdict();
        };
        let port = fpm_port(MINOR).expect("pool port");
        let ini = php_cgi::ini_file_name(MINOR);
        let www = root.join("www");
        std::fs::create_dir_all(&www).unwrap();
        let (ok_php, kill_php) = (www.join("ok.php"), www.join("kill.php"));
        std::fs::write(&ok_php, "<?php echo getmypid();").unwrap();
        std::fs::write(&kill_php, "<?php exec('taskkill /F /PID ' . getmypid() . ' 2>NUL');").unwrap();
        let (ok, kill) = (ok_php.display().to_string(), kill_php.display().to_string());
        let log = php_cgi::output_log(&plat.paths().log_dir().unwrap(), MINOR);

        // ── The spawned path: legitimate load, then a worker-killing script. ──
        let mut pools = PhpFpmPools::default();
        let started = pools.ensure(&*plat, MINOR).await;
        check.is("ensure(8.3) starts the group", started.is_ok(), &format!("{started:?}"));
        if started.is_err() {
            let _ = std::fs::remove_dir_all(&root);
            return check.verdict();
        }
        common::await_listening(port, "php-cgi group", Some(&log));

        for (label, script) in [("legitimate load", ok.clone()), ("a script that kills its own worker", kill.clone())] {
            let stop = Arc::new(AtomicBool::new(false));
            let sent = Arc::new(AtomicU32::new(0));
            let load = {
                let (stop, sent, script) = (stop.clone(), sent.clone(), script.clone());
                std::thread::spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        let _ = fastcgi_get(port, &script, 5);
                        sent.fetch_add(1, Ordering::Relaxed);
                    }
                })
            };
            let t = Instant::now();
            let mut tripped = Vec::new();
            for _ in 0..3 {
                std::thread::sleep(TICK);
                tripped.extend(pools.trip_spinning(&*plat));
            }
            stop.store(true, Ordering::Relaxed);
            let _ = load.join();
            let sent = sent.load(Ordering::Relaxed);
            println!("  · {label}: {sent} requests in {:.0} s ({:.0}/s)", t.elapsed().as_secs_f64(), f64::from(sent) / t.elapsed().as_secs_f64());
            check.is(&format!("{label}: three ticks, no trip"), tripped.is_empty() && pools.has(MINOR, false), &format!("{tripped:?}"));
            let answer = fastcgi_get(port, &ok, 10);
            check.is(&format!("{label}: the group still answers"), body_of(&answer).parse::<u32>().is_ok(), &answer);
        }
        pools.stop_all(&*plat);
        check.is("the spawned group stops", ports::wait_free(&*plat, port, Proto::Tcp, 50, Duration::from_millis(100)), "port held");

        // ── The adopted path: a group whose worker cannot be spawned. ──
        let cached = binaries::cached_path(&*plat, "php", binaries::pins().php).expect("php cached by ensure");
        let php_dir = root.join("php");
        let copied = copy_tree(&cached, &php_dir);
        check.is("the PHP tree is copied into the fixture", copied.is_ok(), &format!("{copied:?}"));
        let started = if copied.is_ok() {
            php_cgi::start_group(&*plat, &group, &php_dir, MINOR, port, None, &[]).map_err(|e| e.to_string())
        } else {
            Err("no copy".into())
        };
        check.is("a group starts from the copy", started.is_ok(), &format!("{:?}", started.as_ref().err()));
        let Ok(child) = started else {
            let _ = std::fs::remove_dir_all(&root);
            return check.verdict();
        };
        let mut svc = OwnedService::new(child, "php-cgi group (copy)");
        let parent = svc.id();
        common::await_listening(port, "php-cgi group (copy)", Some(&log));
        let mut adopted = PhpFpmPools::default();
        adopted.adopt(MINOR, port, parent, false);
        // The adopted pool's first read only sets its baseline.
        check.is("the adopted group's first tick sets a baseline, no trip", adopted.trip_spinning(&*plat).is_empty(), "tripped");

        let exe = php_dir.join("php-cgi.exe");
        let moved = php_dir.join("php-cgi.exe.moved");
        let renamed = std::fs::rename(&exe, &moved);
        check.is("the running php-cgi.exe is renamed", renamed.is_ok(), &format!("{renamed:?}"));
        let victim = sup.owned_pids(&ini).into_iter().find(|&p| p != parent);
        println!("  · killed worker {victim:?}: {:?}", victim.map(taskkill));
        let spin_began = Instant::now();
        let mut tripped = Vec::new();
        let mut ticks = 0;
        while tripped.is_empty() && ticks < 3 {
            std::thread::sleep(TICK);
            ticks += 1;
            tripped = adopted.trip_spinning(&*plat);
        }
        println!("  · tripped after {ticks} tick(s), {:.0} s: {tripped:?}", spin_began.elapsed().as_secs_f64());
        check.is("the spin is stopped within two ticks", !tripped.is_empty() && ticks <= 2, &format!("{ticks} ticks"));
        check.is(
            "the reason quotes the group's own unable-to-spawn line",
            tripped.first().is_some_and(|(m, r)| m == MINOR && r.contains("unable to spawn: [0x00000002]")),
            &format!("{tripped:?}"),
        );
        check.is("the tripped pool is no longer managed", !adopted.has(MINOR, false), "still listed");
        std::thread::sleep(Duration::from_millis(1500));
        let left = sup.owned_pids(&ini);
        check.is("no php-cgi of the group is left", left.is_empty(), &format!("{left:?}"));
        check.is("the pool port is free", ports::wait_free(&*plat, port, Proto::Tcp, 30, Duration::from_millis(100)), "held");
        let size = file_len(&log);
        std::thread::sleep(Duration::from_secs(2));
        check.is("the output log stopped growing", file_len(&log) == size, &format!("{size} -> {}", file_len(&log)));

        svc.stop();
        if renamed.is_ok() {
            let _ = std::fs::rename(&moved, &exe);
        }
        let _ = std::fs::remove_dir_all(&root);
        check.verdict()
    }

    fn taskkill(pid: u32) -> String {
        match std::process::Command::new("taskkill").args(["/F", "/PID", &pid.to_string()]).output() {
            Ok(out) => format!("{} {}", out.status, String::from_utf8_lossy(&out.stdout).trim()),
            Err(e) => e.to_string(),
        }
    }

    fn file_len(path: &Path) -> u64 {
        std::fs::metadata(path).map_or(0, |m| m.len())
    }

    fn copy_tree(from: &Path, to: &Path) -> std::io::Result<u64> {
        std::fs::create_dir_all(to)?;
        let mut total = 0;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            let target = to.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                total += copy_tree(&entry.path(), &target)?;
            } else {
                total += std::fs::copy(entry.path(), &target)?;
            }
        }
        Ok(total)
    }

    fn body_of(response: &str) -> String {
        response.split("\r\n\r\n").last().unwrap_or("").trim().to_string()
    }

    fn record(kind: u8, body: &[u8]) -> Vec<u8> {
        let mut r = vec![1, kind, 0, 1, (body.len() >> 8) as u8, body.len() as u8, 0, 0];
        r.extend_from_slice(body);
        r
    }

    /// A minimal FastCGI GET for `script`: BEGIN_REQUEST, PARAMS, empty STDIN, read to END_REQUEST.
    fn fastcgi_get(port: u16, script: &str, timeout_secs: u64) -> String {
        let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else { return "no connection".into() };
        let _ = s.set_read_timeout(Some(Duration::from_secs(timeout_secs)));
        let mut params = Vec::new();
        for (k, v) in [("SCRIPT_FILENAME", script), ("REQUEST_METHOD", "GET"), ("SCRIPT_NAME", "/probe.php"),
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
}

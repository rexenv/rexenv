//! W4's churn breaker, measured before it is built (docs/PLAN-windows-port.md §3 D1(a)): what
//! a php-cgi group's respawn loop looks like from OUTSIDE, on a real Windows machine.
//!
//! ```text
//! scripts/probes/windows-example.sh dell@<host> windows_cgi_churn_probe
//! ```
//!
//! A PROBE, not a check: it prints the numbers the breaker's design needs, and passes when
//! every phase ran. Why it exists: the plan's threshold — "more than 2 × workers replacements
//! in 10 s" — was written against a watchdog that looks once every 10 s, and a look sees only
//! the children alive at that instant. Whether that view can count a churn at all, and what a
//! spin costs, are measurements, so they come first. One group, started through
//! `php_cgi::start_group` from a COPY of the cached PHP tree (phase 3 renames its php-cgi.exe):
//!
//! 0. idle — the parent's CPU, the child count, what one process-table poll costs, and whether
//!    php-cgi answers a FastCGI `GET_VALUES` management record (a serving check that runs no
//!    script);
//! 1. legitimate churn — requests as fast as one client sends them for 15 s against
//!    `PHP_FCGI_MAX_REQUESTS`: children born as a 100 ms poll sees them, and as a single look
//!    10 s in sees them (the watchdog's view), with the parent's CPU;
//! 2. a script that kills its own worker, requested for 11 s — the same numbers, and whether a
//!    healthy script still answers meanwhile;
//! 3. a child that cannot be spawned — php-cgi.exe renamed under the running parent, one child
//!    killed — the parent's CPU, "unable to spawn" lines and log bytes per second, whether the
//!    port still accepts; capped at 5 s, then the group is stopped.
//!
//! Fixture-owned: a sandboxed platform under `%TEMP%\rexenv-cgi-churn-probe` (removed at the
//! end; the renamed exe is the fixture's copy), the pool's fixed port on a machine with no
//! rexenv stack (`require_stack_stopped`), the group held by `OwnedService`. The binary cache
//! is the documented exception, resolved and never modified. `demo` tier: Windows-only; on
//! macOS it prints a skip line.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_cgi_churn_probe: skipped — a Windows probe (plan §3 D1(a))");
}

#[cfg(target_os = "windows")]
#[tokio::main]
async fn main() -> std::process::ExitCode {
    windows::main().await
}

#[cfg(target_os = "windows")]
mod windows {
    use super::common::{self, Check, OwnedService};
    use rexenv_lib::core::php::fpm_port;
    use rexenv_lib::core::ports::{self, Proto};
    use rexenv_lib::core::{binaries, php_cgi};
    use rexenv_lib::platform::traits::PoolModel;
    use std::collections::HashSet;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::path::Path;
    use std::process::ExitCode;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    const MINOR: &str = "8.3";
    /// Every member of the group carries its ini path on the command line.
    const INI: &str = "php-cgi-8.3.ini";

    pub async fn main() -> ExitCode {
        common::require_stack_stopped();
        rexenv_lib::core::stack_guard::allow_real_stack_control();
        let mut check = Check::new("windows_cgi_churn_probe");
        let root = std::env::temp_dir().join("rexenv-cgi-churn-probe");
        let _ = std::fs::remove_dir_all(&root);
        let plat = common::sandbox_platform_at(root.clone());
        let sup = plat.supervisor();
        let PoolModel::CgiGroup(group) = sup.php_pool_model() else {
            check.is("this platform names the php-cgi group model", false, "Fpm");
            return check.verdict();
        };
        let port = fpm_port(MINOR).expect("pool port");

        let cached = match binaries::resolve_dir(&*plat, "php", binaries::PHP_VERSION).await {
            Ok(dir) => dir,
            Err(e) => {
                check.is("PHP resolves", false, &e.to_string());
                return check.verdict();
            }
        };
        let php_dir = root.join("php");
        let copied = copy_tree(&cached, &php_dir);
        check.is("the PHP tree is copied into the fixture", copied.is_ok(), &format!("{copied:?}"));
        if copied.is_err() {
            let _ = std::fs::remove_dir_all(&root);
            return check.verdict();
        }

        let www = root.join("www");
        std::fs::create_dir_all(&www).unwrap();
        let (ok_php, kill_php) = (www.join("ok.php"), www.join("kill.php"));
        std::fs::write(&ok_php, "<?php echo getmypid();").unwrap();
        std::fs::write(&kill_php, "<?php exec('taskkill /F /PID ' . getmypid() . ' 2>NUL');").unwrap();
        let (ok, kill) = (ok_php.display().to_string(), kill_php.display().to_string());
        let log = php_cgi::output_log(&plat.paths().log_dir().unwrap(), MINOR);

        let started = php_cgi::start_group(&*plat, &group, &php_dir, MINOR, port, None, &[]);
        check.is("the group starts from the copied tree", started.is_ok(), &format!("{:?}", started.as_ref().err()));
        let Ok(child) = started else {
            let _ = std::fs::remove_dir_all(&root);
            return check.verdict();
        };
        let mut svc = OwnedService::new(child, "php-cgi group");
        let parent = svc.id();
        common::await_listening(port, "php-cgi group", Some(&log));
        std::thread::sleep(Duration::from_secs(2));

        println!("## 0. idle");
        let t = Instant::now();
        let kids = children(parent);
        println!("  children {} · one process-table poll {} ms", kids.len(), t.elapsed().as_millis());
        println!("  parent CPU over 3 s: {:.1}%", cpu_percent(parent, Duration::from_secs(3)));
        println!("  FCGI_GET_VALUES: {}", get_values(port));
        let answered = fastcgi_get(port, &ok, 20);
        check.is("a child answers ok.php", body_of(&answered).parse::<u32>().is_ok(), &answered);

        println!("## 1. legitimate churn: one client for 15 s (PHP_FCGI_MAX_REQUESTS = {})", php_cgi::MAX_REQUESTS);
        let (sent, seen) = watch(parent, || {
            let end = Instant::now() + Duration::from_secs(15);
            let mut n = 0u32;
            while Instant::now() < end {
                let _ = fastcgi_get(port, &ok, 5);
                n += 1;
            }
            n
        });
        report(sent, &seen);
        check.is("phase 1 sent requests", sent > 0, "none");

        println!("## 2. a script that kills its own worker, for 11 s");
        let first = fastcgi_get(port, &kill, 5);
        println!("  what the client of kill.php reads: {first:?}");
        let ((sent, healthy, healthy_ok), seen) = watch(parent, || {
            let end = Instant::now() + Duration::from_secs(11);
            let (mut n, mut healthy, mut healthy_ok) = (0u32, 0u32, 0u32);
            while Instant::now() < end {
                let _ = fastcgi_get(port, &kill, 5);
                n += 1;
                if n % 5 == 0 {
                    healthy += 1;
                    if body_of(&fastcgi_get(port, &ok, 5)).parse::<u32>().is_ok() {
                        healthy_ok += 1;
                    }
                }
            }
            (n, healthy, healthy_ok)
        });
        report(sent, &seen);
        println!("  ok.php meanwhile: {healthy_ok}/{healthy} answered");
        std::thread::sleep(Duration::from_secs(2));
        println!(
            "  2 s later: children {} · parent CPU over 2 s {:.1}%",
            children(parent).len(),
            cpu_percent(parent, Duration::from_secs(2))
        );
        check.is("phase 2 sent requests", sent > 0, "none");

        println!("## 3. a child that cannot be spawned (php-cgi.exe renamed under the running parent)");
        let exe = php_dir.join("php-cgi.exe");
        let moved = php_dir.join("php-cgi.exe.moved");
        let renamed = std::fs::rename(&exe, &moved);
        check.is("the running php-cgi.exe can be renamed", renamed.is_ok(), &format!("{renamed:?}"));
        if renamed.is_ok() {
            let (lines0, bytes0) = (spawn_lines(&log), file_len(&log));
            let victim = children(parent).into_iter().next();
            println!("  killed child {victim:?}: {:?}", victim.map(taskkill));
            let t = Instant::now();
            let cpu = cpu_percent(parent, Duration::from_secs(5));
            let secs = t.elapsed().as_secs_f64();
            let (lines, bytes) = (spawn_lines(&log).saturating_sub(lines0), file_len(&log).saturating_sub(bytes0));
            println!(
                "  parent CPU {cpu:.1}% · \"unable to spawn\" {lines} lines ({:.0}/s) · log +{bytes} bytes ({:.0}/s)",
                lines as f64 / secs,
                bytes as f64 / secs
            );
            println!(
                "  port accepts {} · children left {} · ok.php answers {:?}",
                ports::is_listening(port),
                children(parent).len(),
                body_of(&fastcgi_get(port, &ok, 3))
            );
            println!("  first line: {:?}", first_spawn_line(&log));
            check.is("phase 3 saw the spin in the log", lines > 0, "no \"unable to spawn\" line");
        }
        svc.stop();
        if renamed.is_ok() {
            let _ = std::fs::rename(&moved, &exe);
        }
        std::thread::sleep(Duration::from_millis(1500));
        let left = rexenv_lib::platform::current().supervisor().owned_pids(INI);
        check.is("stopping the parent leaves no php-cgi of the group", left.is_empty(), &format!("{left:?}"));
        check.is(
            "the pool port is free",
            ports::wait_free(&*plat, port, Proto::Tcp, 30, Duration::from_millis(100)),
            "held",
        );
        let _ = std::fs::remove_dir_all(&root);
        check.verdict()
    }

    /// What one phase showed of the group from outside.
    struct Seen {
        /// Children born during the phase, as a 100 ms poll of the process table sees them.
        fast: usize,
        /// New children alive at ONE look 10 s in — what a 10 s watchdog tick would count.
        at_10s: Option<usize>,
        polls: u32,
        cpu: f64,
        secs: f64,
    }

    fn watch<R>(parent: u32, work: impl FnOnce() -> R) -> (R, Seen) {
        let before: HashSet<u32> = children(parent).into_iter().collect();
        let stop = Arc::new(AtomicBool::new(false));
        let poller = {
            let (stop, before) = (stop.clone(), before.clone());
            std::thread::spawn(move || {
                let start = Instant::now();
                let (mut seen, mut at_10s, mut polls) = (HashSet::new(), None, 0u32);
                while !stop.load(Ordering::Relaxed) {
                    let now: HashSet<u32> = children(parent).into_iter().collect();
                    polls += 1;
                    if at_10s.is_none() && start.elapsed() >= Duration::from_secs(10) {
                        at_10s = Some(now.difference(&before).count());
                    }
                    seen.extend(now.difference(&before).copied());
                    std::thread::sleep(Duration::from_millis(100));
                }
                (seen.len(), at_10s, polls)
            })
        };
        let (cpu0, t) = (cpu_ms(parent), Instant::now());
        let result = work();
        let secs = t.elapsed().as_secs_f64();
        let cpu = cpu_ms(parent).saturating_sub(cpu0) as f64 / (secs * 1000.0) * 100.0;
        stop.store(true, Ordering::Relaxed);
        let (fast, at_10s, polls) = poller.join().unwrap_or((0, None, 0));
        (result, Seen { fast, at_10s, polls, cpu, secs })
    }

    fn report(sent: u32, s: &Seen) {
        println!(
            "  requests {sent} in {:.1} s ({:.0}/s) · children born {} by a 100 ms poll ({} polls) · {} new alive at the 10 s look · parent CPU {:.1}%",
            s.secs,
            f64::from(sent) / s.secs,
            s.fast,
            s.polls,
            s.at_10s.map_or_else(|| "n/a".to_string(), |n| n.to_string()),
            s.cpu
        );
    }

    fn children(parent: u32) -> Vec<u32> {
        rexenv_lib::platform::current()
            .supervisor()
            .owned_pids(INI)
            .into_iter()
            .filter(|&p| p != parent)
            .collect()
    }

    /// The process's user + kernel CPU so far, in ms.
    fn cpu_ms(pid: u32) -> u64 {
        let mut sys = sysinfo::System::new();
        let p = sysinfo::Pid::from_u32(pid);
        sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[p]), true);
        sys.process(p).map_or(0, |proc_| proc_.accumulated_cpu_time())
    }

    fn cpu_percent(pid: u32, over: Duration) -> f64 {
        let (c0, t) = (cpu_ms(pid), Instant::now());
        std::thread::sleep(over);
        cpu_ms(pid).saturating_sub(c0) as f64 / (t.elapsed().as_secs_f64() * 1000.0) * 100.0
    }

    fn taskkill(pid: u32) -> String {
        match std::process::Command::new("taskkill").args(["/F", "/PID", &pid.to_string()]).output() {
            Ok(out) => format!("{} {}", out.status, String::from_utf8_lossy(&out.stdout).trim()),
            Err(e) => e.to_string(),
        }
    }

    fn spawn_lines(log: &Path) -> usize {
        let bytes = std::fs::read(log).unwrap_or_default();
        String::from_utf8_lossy(&bytes).lines().filter(|l| l.contains("unable to spawn")).count()
    }

    fn first_spawn_line(log: &Path) -> Option<String> {
        let bytes = std::fs::read(log).ok()?;
        String::from_utf8_lossy(&bytes).lines().find(|l| l.contains("unable to spawn")).map(str::to_string)
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

    fn record(kind: u8, request: u16, body: &[u8]) -> Vec<u8> {
        let mut r = vec![1, kind, (request >> 8) as u8, request as u8, (body.len() >> 8) as u8, body.len() as u8, 0, 0];
        r.extend_from_slice(body);
        r
    }

    fn pair(params: &mut Vec<u8>, k: &str, v: &str) {
        for len in [k.len(), v.len()] {
            if len < 128 {
                params.push(len as u8)
            } else {
                params.extend_from_slice(&((len as u32) | 0x8000_0000).to_be_bytes())
            }
        }
        params.extend_from_slice(k.as_bytes());
        params.extend_from_slice(v.as_bytes());
    }

    /// FCGI_GET_VALUES (type 9, request id 0): answered by the FastCGI layer itself, if at all.
    fn get_values(port: u16) -> String {
        let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else { return "no connection".into() };
        let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
        let mut body = Vec::new();
        for name in ["FCGI_MAX_CONNS", "FCGI_MAX_REQS", "FCGI_MPXS_CONNS"] {
            pair(&mut body, name, "");
        }
        if s.write_all(&record(9, 0, &body)).is_err() {
            return "write failed".into();
        }
        let mut head = [0u8; 8];
        if let Err(e) = s.read_exact(&mut head) {
            return format!("no answer ({e})");
        }
        let len = ((head[4] as usize) << 8) | head[5] as usize;
        let mut reply = vec![0u8; len + head[6] as usize];
        let _ = s.read_exact(&mut reply);
        format!("record type {} · {:?}", head[1], String::from_utf8_lossy(&reply[..len.min(reply.len())]))
    }

    /// A minimal FastCGI GET for `script`: BEGIN_REQUEST, PARAMS, empty STDIN, read to END_REQUEST.
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
        loop {
            match s.read_exact(&mut head) {
                Ok(()) => {}
                Err(e) if stdout.is_empty() => return format!("read ended: {e}"),
                Err(_) => break,
            }
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

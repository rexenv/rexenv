//! W5, ledger #611: the edge's start, adopt, crash and stop on Windows, through the paths the app
//! takes — `sites::rebuild_configs` writes the Caddyfile, `ServiceManager::prepare_edge` plans the
//! start, `proxy::start` spawns the child, `proxy::admin_alive` dials the admin socket through the
//! platform's `LocalIpc` (AF_UNIX), `adopt_startup` finds a survivor, `stop_all` frees the ports.
//!
//! ```text
//! scripts/probes/windows-example.sh <host> windows_edge_start_check
//! ```
//!
//! What it holds the Windows edge to:
//!
//! 1. The Caddyfile carries `default_bind 127.0.0.1` once (the owner's loopback-only ruling — an
//!    all-interfaces bind raised Windows Defender Firewall's allow prompt on the desktop).
//! 2. `prepare_edge` plans an UNPRIVILEGED start — Windows has no privileged ports, so the macOS
//!    LaunchDaemon path (which Windows does not have) is never taken.
//! 3. Up: `admin_alive` is true over AF_UNIX; `:443`/`:80` listen on 127.0.0.1 only (netstat, and
//!    the machine's LAN address refuses `:443`); TLS answers with the local CA's certificate.
//! 4. Adopt: a fresh manager's `adopt_startup` takes the live edge, and `prepare_edge` reloads it
//!    in place — same pid.
//! 5. Crash: after `taskkill /F`, the socket FILE outlives the process; `admin_alive` is false, and
//!    the connect error is `ConnectionRefused` (a file nobody serves) where it was `NotFound` before
//!    the first start. `prepare_edge` resets the stale handle and a fresh start comes up over the
//!    leftover file.
//! 6. `stop_all` frees `:443` and `:80`, the admin socket goes quiet, no Caddy runs our binary.
//!
//! Fixture-owned: a sandboxed platform under `%TEMP%\rexenv edge start` (a SPACE in it), removed at
//! the end; every Caddy pid this check started is killed at the end if still alive; `:443`/`:80`
//! checked free first. The binary cache is the documented exception. `demo` tier: Windows-only; on
//! macOS it prints a skip line.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_edge_start_check: skipped — a Windows check (ledger #611, W5)");
}

#[cfg(target_os = "windows")]
#[tokio::main]
async fn main() -> std::process::ExitCode {
    windows::main().await
}

#[cfg(target_os = "windows")]
mod windows {
    use super::common::{self, Check};
    use rexenv_lib::core::proxy;
    use rexenv_lib::core::service_manager::ServiceManager;
    use rexenv_lib::core::services::NGINX_HTTP_PORT;
    use rexenv_lib::core::{sites, ssl};
    use rexenv_lib::platform::traits::Platform;
    use rexenv_lib::state::models::{NewSite, SiteDbEngine, SiteType, WebServer};
    use std::io::ErrorKind;
    use std::net::{TcpStream, UdpSocket};
    use std::path::Path;
    use std::process::{Command, ExitCode};
    use std::time::{Duration, Instant};

    const DOMAIN: &str = "edgestart.rex";

    pub async fn main() -> ExitCode {
        common::require_stack_stopped();
        rexenv_lib::core::stack_guard::allow_real_stack_control();
        let mut check = Check::new("windows_edge_start_check");
        for port in [443u16, 80] {
            check.is(&format!(":{port} is free before the check"), !answers("127.0.0.1", port), "something answers there");
        }
        let root = std::env::temp_dir().join("rexenv edge start");
        let _ = std::fs::remove_dir_all(&root);
        let plat = common::sandbox_platform_at(root.clone());
        let mut started = EdgeReaper(Vec::new());
        run(&mut check, &*plat, &root, &mut started.0).await;
        drop(started);
        if let Err(e) = std::fs::remove_dir_all(&root) {
            println!("  · sandbox not fully removed: {e}");
        }
        check.verdict()
    }

    /// Every Caddy pid this check started, killed on drop — the panic path included. The children
    /// themselves belong to the `ServiceManager` under test (its `stop_all` Child branch is a subject),
    /// so no `OwnedService` can hold them; this is the owner of last resort. Filtered by image name,
    /// so a pid Windows has since reused for another program is never killed.
    struct EdgeReaper(Vec<u32>);

    impl Drop for EdgeReaper {
        fn drop(&mut self) {
            for pid in &self.0 {
                let out = Command::new("taskkill")
                    .args(["/F", "/FI", &format!("PID eq {pid}"), "/FI", "IMAGENAME eq caddy.exe"])
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                    .unwrap_or_default();
                if out.contains("SUCCESS") {
                    println!("  · teardown: killed Caddy pid {pid} this check started");
                }
            }
        }
    }

    fn answers(host: &str, port: u16) -> bool {
        let Ok(addr) = format!("{host}:{port}").parse() else { return false };
        TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
    }

    fn wait(mut ok: impl FnMut() -> bool, within: Duration) -> bool {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            if ok() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        ok()
    }

    fn connect_kind(plat: &dyn Platform, sock: &Path) -> Option<ErrorKind> {
        plat.local_ipc().connect(sock, None).err().map(|e| e.kind())
    }

    fn log_tail(plat: &dyn Platform) -> String {
        let log = plat.paths().log_dir().unwrap().join("caddy-stdout.log");
        let text = std::fs::read_to_string(log).unwrap_or_default();
        let tail: Vec<&str> = text.lines().rev().take(8).collect::<Vec<_>>().into_iter().rev().collect();
        tail.iter().map(|l| format!("      {}", &l[..l.len().min(400)])).collect::<Vec<_>>().join("\n")
    }

    /// Plan and start the edge the way `start_services` does for an unprivileged plan; the pid of the
    /// child, once the admin socket and `:443` both answer.
    fn start_edge(check: &mut Check, plat: &dyn Platform, mgr: &mut ServiceManager, label: &str, started: &mut Vec<u32>) -> Option<u32> {
        let caddyfile = plat.paths().config_dir().unwrap().join(proxy::CADDYFILE);
        let plan = match mgr.prepare_edge(plat, caddyfile) {
            Ok(Some(plan)) => plan,
            other => {
                check.is(&format!("prepare_edge returns a start plan ({label})"), false, &format!("{:?}", other.map(|p| p.is_some())));
                return None;
            }
        };
        check.is(&format!("the plan is unprivileged — no LaunchDaemon path on Windows ({label})"), !plan.privileged, "privileged = true");
        if plan.privileged {
            return None;
        }
        let child = match proxy::start(plat, &plan.caddy_bin, &plan.caddyfile) {
            Ok(c) => c,
            Err(e) => {
                check.is(&format!("proxy::start spawns Caddy ({label})"), false, &e.to_string());
                return None;
            }
        };
        let pid = child.id();
        started.push(pid);
        mgr.set_edge_child(child);
        let up = wait(|| proxy::admin_alive(plat) && answers("127.0.0.1", 443), Duration::from_secs(15));
        check.is(&format!("the edge comes up: admin_alive over AF_UNIX and :443 answering ({label})"), up, &format!("Caddy's log:\n{}", log_tail(plat)));
        up.then_some(pid)
    }

    async fn run(check: &mut Check, plat: &dyn Platform, root: &Path, started: &mut Vec<u32>) -> Option<()> {
        let conn = common::sandbox_db(plat);
        let ca = ssl::load_or_create(plat.paths(), plat.permissions());
        check.is("the local CA is created", ca.is_ok(), &format!("{:?}", ca.as_ref().err()));
        let ca = ca.ok()?;
        let site = sites::provision(
            &conn,
            plat,
            &ca,
            NewSite {
                name: "Edge Start Check".into(),
                domain: DOMAIN.into(),
                site_type: SiteType::Php,
                php_version: "8.3".into(),
                web_server: WebServer::Nginx,
                path: String::new(),
                db_engine: SiteDbEngine::Mysql,
                git_url: String::new(),
                git_ref: None,
                git_migrate: true,
                git_build_assets: false,
                starter_db: false,
            },
        );
        check.is("sites::provision creates the site (docroot, certificate, row)", site.is_ok(), &format!("{:?}", site.as_ref().err()));
        let site = site.ok()?;
        check.is("the site's docroot is inside the sandbox", Path::new(&site.path).starts_with(root), &site.path);

        // ── 1. The Caddyfile the app writes. ──
        let cfg = sites::rebuild_configs(&conn, plat, &ca, NGINX_HTTP_PORT, 80, 443);
        check.is("sites::rebuild_configs writes the Caddyfile", cfg.is_ok(), &format!("{:?}", cfg.as_ref().err()));
        let caddyfile = std::fs::read_to_string(cfg.ok()?.caddyfile).unwrap_or_default();
        let global = caddyfile.split("\n}\n").next().unwrap_or("");
        check.is(
            "the Caddyfile's global block binds loopback: `default_bind 127.0.0.1`, once",
            global.lines().any(|l| l == "\tdefault_bind 127.0.0.1") && caddyfile.matches("default_bind").count() == 1,
            &caddyfile,
        );

        let mut mgr = ServiceManager::default();
        if let Err(e) = mgr.ensure_bins(plat).await {
            check.is("the edge's binaries resolve", false, &e.to_string());
            return None;
        }
        let sock = proxy::admin_socket_path(plat).ok()?;
        let before = connect_kind(plat, &sock);
        check.is("before any start: no socket file, the connect says NotFound", before == Some(ErrorKind::NotFound), &format!("{before:?}"));
        check.is("before any start: admin_alive is false", !proxy::admin_alive(plat), "");

        // ── 2 + 3. Start, and where it listens. ──
        let pid = start_edge(check, plat, &mut mgr, "first start", started)?;
        let netstat = Command::new("netstat").args(["-ano", "-p", "TCP"]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
        for port in [443u16, 80] {
            let locals: Vec<String> = netstat
                .lines()
                .filter(|l| l.contains("LISTENING"))
                .filter_map(|l| l.split_whitespace().nth(1).map(str::to_string))
                .filter(|local| local.ends_with(&format!(":{port}")))
                .collect();
            println!("  · :{port} listening on {locals:?}");
            check.is(
                &format!(":{port} listens on 127.0.0.1 only"),
                !locals.is_empty() && locals.iter().all(|l| l == &format!("127.0.0.1:{port}")),
                &format!("{locals:?}"),
            );
        }
        // The address a LAN peer would dial: the interface the default route leaves by (no packet sent).
        let lan = UdpSocket::bind("0.0.0.0:0").and_then(|s| s.connect("8.8.8.8:53").and_then(|_| s.local_addr())).map(|a| a.ip());
        match lan {
            Ok(ip) if !ip.is_loopback() && !ip.is_unspecified() => {
                println!("  · LAN address {ip}");
                check.is(&format!("{ip}:443 refuses — the edge is unreachable off the machine"), !answers(&ip.to_string(), 443), "it answered");
            }
            other => println!("  · no LAN address to test from ({other:?})"),
        }
        let tls = https_status(&ca.cert_path).await;
        check.is("TLS on :443 answers with the local CA's certificate", tls.is_ok(), &format!("{tls:?}"));

        // ── 4. A relaunched app adopts the live edge. ──
        let mut mgr2 = ServiceManager::default();
        let adopted = mgr2.adopt_startup(plat, &[], false);
        check.is("adopt_startup takes the live edge", adopted >= 1 && proxy::admin_alive(plat), &format!("adopted {adopted}"));
        if let Err(e) = mgr2.ensure_bins(plat).await {
            check.is("the edge's binaries resolve (second manager)", false, &e.to_string());
            return None;
        }
        let caddyfile_path = plat.paths().config_dir().unwrap().join(proxy::CADDYFILE);
        let replan = mgr2.prepare_edge(plat, caddyfile_path);
        check.is("prepare_edge reloads the adopted edge in place (no start plan)", matches!(replan, Ok(None)), &format!("{:?}", replan.map(|p| p.is_some())));
        check.is("the adopted edge is the same process", plat.supervisor().pid_alive(pid) && proxy::admin_alive(plat), &format!("pid {pid}"));

        // ── 5. A crash leaves the socket file behind. ──
        let _ = Command::new("taskkill").args(["/F", "/PID", &pid.to_string()]).output();
        let dead = wait(|| !plat.supervisor().pid_alive(pid), Duration::from_secs(10));
        check.is("taskkill /F ends the edge", dead, &format!("pid {pid} still alive"));
        println!("  · socket file after the kill: exists {}", sock.exists());
        let after = connect_kind(plat, &sock);
        let expected = if sock.exists() { ErrorKind::ConnectionRefused } else { ErrorKind::NotFound };
        check.is(&format!("after the kill: the connect says {expected:?}"), after == Some(expected), &format!("{after:?}"));
        check.is("after the kill: admin_alive is false", !proxy::admin_alive(plat), "");
        let restarted = start_edge(check, plat, &mut mgr2, "over the stale handle and socket file", started);
        check.is("a fresh start comes up after the crash", restarted.is_some(), "");

        // ── 6. Stop all. ──
        let stopped = mgr2.stop_all(plat);
        check.is("stop_all returns Ok", stopped.is_ok(), &format!("{stopped:?}"));
        let freed = wait(|| !answers("127.0.0.1", 443) && !answers("127.0.0.1", 80), Duration::from_secs(10));
        check.is("after stop_all, :443 and :80 are released", freed, "still answering");
        check.is("after stop_all, admin_alive is false", !proxy::admin_alive(plat), "");
        let caddy_marker = rexenv_lib::core::binaries::cached_bin(plat, "caddy", rexenv_lib::core::binaries::CADDY_VERSION)
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let survivors: Vec<u32> = plat.supervisor().owned_pids(&caddy_marker).into_iter().filter(|p| started.contains(p)).collect();
        check.is("no Caddy this check started survives stop_all", survivors.is_empty(), &format!("{survivors:?}"));
        drop(mgr);
        Some(())
    }

    async fn https_status(ca_pem: &Path) -> Result<u16, String> {
        let pem = std::fs::read(ca_pem).map_err(|e| e.to_string())?;
        let client = reqwest::Client::builder()
            .add_root_certificate(reqwest::Certificate::from_pem(&pem).map_err(|e| e.to_string())?)
            .resolve(DOMAIN, "127.0.0.1:443".parse().unwrap())
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| e.to_string())?;
        // No nginx behind it: Caddy's own 502 still proves the TLS handshake was ours.
        let resp = client.get(format!("https://{DOMAIN}/")).send().await.map_err(|e| format!("{e:?}"))?;
        Ok(resp.status().as_u16())
    }
}

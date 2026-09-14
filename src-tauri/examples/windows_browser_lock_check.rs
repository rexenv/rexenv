//! W5's done-when, ledger #614: with rexenv's local CA trusted in this user's Root certificate store,
//! Edge, Chrome and Firefox accept a `.rex` site the Windows edge serves — and without it they do not.
//!
//! ```text
//! (desktop session, someone at the machine) scripts/probes/windows-browser-lock.ps1
//! ```
//!
//! **What "accepts" means here, without a screen:** a browser that rejects the site's certificate
//! stops at its error page and never sends the HTTP request; one that accepts it sends it. So the
//! edge's upstream — a responder inside this check — records every request, and a run counts as
//! accepted exactly when its own `/?run=<browser>-<phase>` arrived (and, for the page it served, the
//! `/beacon?run=…` image it names — the page rendered). Every browser runs headless on a FRESH
//! profile (no remembered exceptions), three times:
//!
//! 1. before the CA is trusted — must NOT arrive (the control: the check can see a rejection);
//! 2. after `trust_ca` (Windows asks; the person at the desktop answers Yes) — must arrive;
//! 3. after `untrust_ca` (Windows asks again; Yes) — must NOT arrive.
//!
//! Name resolution is the browser's own, not the machine's (`.rex` DNS is W6): Chromium's
//! `--host-resolver-rules`, Firefox's `network.dns.localDomains`. Firefox's profile gets rexenv's
//! `user.js` through `core::firefox::enable_in_profiles`, as the trust step writes it.
//!
//! **Changes this user's Root certificate store** (the fixture CA is added, then removed) — runs only
//! with `REXENV_CERT_TRUST_WRITE=1`; without it, only phase 1. Fixture-owned: a sandbox platform under
//! `%TEMP%\rexenv browser lock` (a SPACE in it) removed at the end; Caddy held by `OwnedService`;
//! every browser killed with its tree if it outlives its deadline; `:443`/`:80` checked free first.
//! A browser that is not installed is reported and not counted. `demo` tier: Windows-only.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_browser_lock_check: skipped — a Windows check (ledger #614, W5)");
}

#[cfg(target_os = "windows")]
#[tokio::main]
async fn main() -> std::process::ExitCode {
    windows::main().await
}

#[cfg(target_os = "windows")]
mod windows {
    use super::common::{self, Check, OwnedService};
    use rexenv_lib::core::proxy::{self, CaddyConfig, SiteRoute};
    use rexenv_lib::core::{binaries, firefox, ssl};
    use rexenv_lib::platform::traits::Platform;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, ExitCode};
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::{Duration, Instant};

    const HOST: &str = "lockcheck.rex";
    const BROWSER_DEADLINE: Duration = Duration::from_secs(60);
    const PROMPT_DEADLINE: Duration = Duration::from_secs(180);

    #[derive(Clone, Copy)]
    enum Kind {
        Chromium,
        Firefox,
    }

    struct Browser {
        name: &'static str,
        exe: PathBuf,
        kind: Kind,
    }

    /// A browser run, killed with its whole tree on drop.
    struct Run(Child);

    impl Drop for Run {
        fn drop(&mut self) {
            if let Ok(None) = self.0.try_wait() {
                let _ = Command::new("taskkill").args(["/F", "/T", "/PID", &self.0.id().to_string()]).output();
                let _ = self.0.wait();
            }
        }
    }

    fn installed() -> Vec<Browser> {
        let pf = std::env::var("ProgramFiles").unwrap_or_default();
        let pf86 = std::env::var("ProgramFiles(x86)").unwrap_or_default();
        let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
        let find = |rel: &str| [&pf, &pf86, &local].iter().map(|b| Path::new(b.as_str()).join(rel)).find(|p| p.is_file());
        let mut out = Vec::new();
        for (name, rel, kind) in [
            ("Edge", r"Microsoft\Edge\Application\msedge.exe", Kind::Chromium),
            ("Chrome", r"Google\Chrome\Application\chrome.exe", Kind::Chromium),
            ("Firefox", r"Mozilla Firefox\firefox.exe", Kind::Firefox),
        ] {
            match find(rel) {
                Some(exe) => out.push(Browser { name, exe, kind }),
                None => println!("  · {name}: not installed — not measured"),
            }
        }
        out
    }

    pub async fn main() -> ExitCode {
        common::require_stack_stopped();
        rexenv_lib::core::stack_guard::allow_real_stack_control();
        let mut check = Check::new("windows_browser_lock_check");
        for port in [443u16, 80] {
            let busy = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(300)).is_ok();
            check.is(&format!(":{port} is free before the check"), !busy, "something answers there");
        }
        let root = std::env::temp_dir().join("rexenv browser lock");
        let _ = std::fs::remove_dir_all(&root);
        let plat = common::sandbox_platform_at(root.clone());
        run(&mut check, &*plat, &root).await;
        if let Err(e) = std::fs::remove_dir_all(&root) {
            println!("  · sandbox not fully removed: {e}");
        }
        check.verdict()
    }

    async fn run(check: &mut Check, plat: &dyn Platform, root: &Path) -> Option<()> {
        let ca = ssl::load_or_create(plat.paths(), plat.permissions());
        check.is("the local CA is created", ca.is_ok(), &format!("{:?}", ca.as_ref().err()));
        let ca = ca.ok()?;
        let cert = ssl::ensure_site_cert(plat.paths(), plat.permissions(), &ca, HOST);
        check.is("a site certificate is issued", cert.is_ok(), &format!("{:?}", cert.as_ref().err()));
        let cert = cert.ok()?;
        let caddy = binaries::resolve(plat, "caddy", binaries::CADDY_VERSION).await;
        check.is("Caddy resolves", caddy.is_ok(), &format!("{caddy:?}"));
        let caddy = caddy.ok()?;

        // The upstream: records every request line, serves a page naming its beacon.
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let upstream = TcpListener::bind("127.0.0.1:0").ok()?;
        let upstream_port = upstream.local_addr().ok()?.port();
        let record = seen.clone();
        std::thread::spawn(move || {
            for mut conn in upstream.incoming().flatten() {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let _ = conn.set_read_timeout(Some(Duration::from_secs(5)));
                while let Ok(n) = conn.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let head = String::from_utf8_lossy(&buf);
                let path = head.lines().next().and_then(|l| l.split_whitespace().nth(1)).unwrap_or("").to_string();
                record.lock().unwrap().push(path.clone());
                let body = match path.strip_prefix("/?run=") {
                    Some(run) => format!("<!doctype html><title>lockcheck</title><p>lockcheck-page {run}</p><img src=\"/beacon?run={run}\">"),
                    None => String::from("ok"),
                };
                let _ = write!(
                    conn,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });

        let sock = proxy::admin_socket_path(plat).ok()?;
        let cfg = CaddyConfig {
            http_port: 80,
            https_port: 443,
            routes: vec![SiteRoute {
                host: HOST.into(),
                wildcard: false,
                aliases: Vec::new(),
                upstream: format!("127.0.0.1:{upstream_port}"),
                cert_path: cert.cert_path.clone(),
                key_path: cert.key_path.clone(),
                stopped: None,
            }],
            admin_socket: Some(sock.clone()),
            default_bind: plat.edge().default_bind().map(str::to_string),
        };
        let caddyfile = proxy::write_caddyfile(plat, &cfg).ok()?;
        // Caddy as a plain child of this check, on the Caddyfile rexenv writes — NOT `proxy::start`.
        // This check has to run inside a Task Scheduler task (the desktop session, where the trust
        // prompt can be answered), and that task's job forbids breakaway: rexenv's supervisor, which
        // spawns every service out of its own job so services outlive the app, was refused with
        // ACCESS_DENIED there (measured, 14 Sep 2026). The edge's start path itself is #611's, proven
        // over SSH; the subject here is what the browsers do with its certificate.
        let log = std::fs::File::create(plat.paths().log_dir().ok()?.join("caddy-stdout.log")).ok();
        let spawned = Command::new(&caddy)
            .args(["run", "--config"])
            .arg(&caddyfile)
            .args(["--adapter", "caddyfile"])
            .stdout(std::process::Stdio::null())
            .stderr(log.map(std::process::Stdio::from).unwrap_or_else(std::process::Stdio::null))
            .spawn();
        let child = match spawned {
            Ok(c) => c,
            Err(e) => {
                check.is("the edge starts", false, &e.to_string());
                return None;
            }
        };
        let mut edge = OwnedService::new(child, "caddy");
        let up = wait(|| sock.exists() && TcpStream::connect_timeout(&([127, 0, 0, 1], 443).into(), Duration::from_millis(200)).is_ok(), Duration::from_secs(15));
        check.is("the edge serves :443", up, "no :443");
        if !up {
            return None;
        }

        let browsers = installed();
        check.is("at least one browser is installed", !browsers.is_empty(), "");
        let trust = plat.cert_trust();
        check.is("the fixture CA is not trusted before the check", !trust.is_trusted(&ca.cert_path), "it already is");

        // ── 1. Not trusted: every browser must reject. ──
        for b in &browsers {
            let accepted = visit(b, "untrusted", root, &seen);
            check.is(&format!("{}: rejects the site before the CA is trusted (the control)", b.name), !accepted, "it reached the upstream");
        }

        if std::env::var("REXENV_CERT_TRUST_WRITE").as_deref() != Ok("1") {
            println!("  · trust phases skipped (REXENV_CERT_TRUST_WRITE=1 runs them — changes this user's Root store)");
            edge.stop();
            return Some(());
        }

        // ── 2. Trusted: every browser must accept. ──
        println!("  · trust_ca: answer Yes at the desktop");
        let added = prompted(&ca.cert_path, false);
        check.is("trust_ca returned Ok (the prompt answered Yes)", matches!(added, Some(Ok(()))), &format!("{added:?}"));
        if trust.is_trusted(&ca.cert_path) {
            for b in &browsers {
                let accepted = visit(b, "trusted", root, &seen);
                check.is(&format!("{}: accepts the site with the CA trusted", b.name), accepted, "its request never reached the upstream");
            }
            // ── 3. Untrusted again: every browser must reject. ──
            println!("  · untrust_ca: answer Yes at the desktop");
            let removed = prompted(&ca.cert_path, true);
            check.is("untrust_ca returned Ok (the prompt answered Yes)", matches!(removed, Some(Ok(()))), &format!("{removed:?}"));
            if !trust.is_trusted(&ca.cert_path) {
                for b in &browsers {
                    let accepted = visit(b, "untrusted-again", root, &seen);
                    check.is(&format!("{}: rejects the site again once the CA is removed", b.name), !accepted, "it reached the upstream");
                }
            }
        }
        if trust.is_trusted(&ca.cert_path) {
            println!("  · !! the fixture CA is STILL trusted — remove it with: certutil -user -delstore Root \"rexenv Local CA\"");
        }
        edge.stop();
        Some(())
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

    /// `trust_ca` / `untrust_ca` on a thread with a deadline; `None` when it did not return.
    fn prompted(cert: &Path, untrust: bool) -> Option<Result<(), String>> {
        let (tx, rx) = mpsc::channel();
        let path = cert.to_path_buf();
        let t = Instant::now();
        std::thread::spawn(move || {
            let p = rexenv_lib::platform::current();
            let r = if untrust { p.cert_trust().untrust_ca(&path) } else { p.cert_trust().trust_ca(&path) };
            let _ = tx.send(r.map_err(|e| e.to_string()));
        });
        let out = rx.recv_timeout(PROMPT_DEADLINE).ok();
        println!("  · {} returned {out:?} after {:.1} s", if untrust { "untrust_ca" } else { "trust_ca" }, t.elapsed().as_secs_f64());
        out
    }

    /// One headless visit on a fresh profile; whether its request reached the upstream.
    fn visit(b: &Browser, phase: &str, root: &Path, seen: &Arc<Mutex<Vec<String>>>) -> bool {
        let run = format!("{}-{phase}", b.name.to_lowercase());
        let url = format!("https://{HOST}/?run={run}");
        let profile = root.join("profiles").join(&run);
        let _ = std::fs::create_dir_all(&profile);
        let mut cmd = Command::new(&b.exe);
        match b.kind {
            Kind::Chromium => {
                cmd.arg("--headless")
                    .arg("--disable-gpu")
                    .arg("--no-first-run")
                    .arg("--no-default-browser-check")
                    .arg(format!("--user-data-dir={}", profile.display()))
                    .arg(format!("--host-resolver-rules=MAP {HOST} 127.0.0.1"))
                    .arg("--dump-dom")
                    .arg(&url);
            }
            Kind::Firefox => {
                // rexenv's own user.js, through a profiles root shaped like Firefox's.
                let ff_root = root.join("firefox").join(&run);
                let dir = ff_root.join("profile");
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::write(ff_root.join("profiles.ini"), "[Profile0]\r\nName=check\r\nIsRelative=1\r\nPath=profile\r\n");
                let forced = firefox::enable_in_profiles(&ff_root);
                let mut user_js = std::fs::read_to_string(dir.join("user.js")).unwrap_or_default();
                user_js.push_str(&format!("user_pref(\"network.dns.localDomains\", \"{HOST}\");\n"));
                let _ = std::fs::write(dir.join("user.js"), user_js);
                if !matches!(forced, Ok(1)) {
                    println!("  · {run}: rexenv's user.js was not written ({forced:?})");
                }
                cmd.args(["-headless", "-no-remote", "-wait-for-browser", "-profile"])
                    .arg(&dir)
                    .arg("-screenshot")
                    .arg(root.join(format!("{run}.png")))
                    .arg(&url);
            }
        }
        let t = Instant::now();
        let mut child = match cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn() {
            Ok(c) => Run(c),
            Err(e) => {
                println!("  · {run}: did not start: {e}");
                return false;
            }
        };
        let page = format!("/?run={run}");
        let beacon = format!("/beacon?run={run}");
        let arrived = |p: &str| seen.lock().unwrap().iter().any(|s| s == p);
        let deadline = Instant::now() + BROWSER_DEADLINE;
        let mut exited = None;
        while Instant::now() < deadline {
            if let Ok(Some(status)) = child.0.try_wait() {
                exited = Some(status);
                break;
            }
            if arrived(&page) && arrived(&beacon) {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        // A beacon can land just after the page; give it a moment before judging.
        std::thread::sleep(Duration::from_millis(500));
        let (got_page, got_beacon) = (arrived(&page), arrived(&beacon));
        println!(
            "  · {run}: page request {got_page}, beacon {got_beacon}, browser {} after {:.1} s",
            exited.map(|s| format!("exited {s}")).unwrap_or_else(|| "still running (killed)".into()),
            t.elapsed().as_secs_f64()
        );
        drop(child);
        got_page
    }
}

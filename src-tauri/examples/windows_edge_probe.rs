//! W5's first hour (plan §3 D3, §5 W5): does the Caddy edge rexenv writes work on Windows as it
//! stands — its admin API on a UNIX socket (the rule: never TCP `:2019`), `:443`/`:80`, the local
//! CA's certificates — and can rexenv reach that socket?
//!
//! ```text
//! scripts/probes/windows-example.sh dell@<host> windows_edge_probe
//! ```
//!
//! A PROBE, not a proof: it prints what each step did. D3 said to measure before W5 whether Caddy
//! on Windows accepts a unix-socket admin address (Go supports AF_UNIX on Windows 10 1803+; Caddy's
//! admin listener unconfirmed) — and that if it does not, that is a blocker to escalate, not a rule
//! to relax. **It does** (14 Sep 2026): the first run found rexenv's `unix//C:\…` naming the socket
//! `/C:\…` — Caddy splits at the first slash — which Caddy refused and its CLI could not dial; the
//! second run found `unix/C:\…` serving; the third, after `proxy::admin_address` was fixed, passed
//! as written. Steps, against the Caddyfile `proxy::generate_caddyfile` writes (the admin address
//! from `proxy::admin_address` with `|0600`, `https_port 443`, `http_port 80`, one site on the local
//! CA's certificate proxying to an in-process upstream):
//!
//! 1. `proxy::start` — does Caddy come up, and is the socket file created? Caddy's own log tail.
//! 2. The admin API over AF_UNIX from Rust (Winsock; std has no unix sockets on Windows):
//!    `GET /config/`.
//! 3. TLS on `:443` with the local CA (reqwest, the CA added as the only root); `:80` → the 308.
//! 4. The addresses `:443`/`:80` are bound on (a wildcard bind is what raises the firewall prompt
//!    for a desktop user) and the socket file's ACL.
//! 5. `proxy::reload` and `proxy::stop_admin` — Caddy's CLI dialling the socket.
//!
//! If step 1 does not come up, Caddy's own log tail says why — the first run's said "cannot reuse
//! socket /C:\…".
//!
//! Runs with the SSH session's ELEVATED token: binding `:443` as the desktop user's filtered token
//! is a separate run (`windows-limited-token.sh`). Fixture-owned: a sandboxed platform under
//! `%TEMP%\rexenv edge probe` (a SPACE in it), removed at the end; `:443`, `:80` on a machine with
//! nothing on them (checked first); Caddy held by `OwnedService`. The binary cache is the documented
//! exception. `demo` tier: Windows-only; on macOS it prints a skip line.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_edge_probe: skipped — a Windows probe (plan §3 D3, W5)");
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
    use rexenv_lib::core::{binaries, ssl};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::Path;
    use std::process::{Command, ExitCode};
    use std::time::{Duration, Instant};

    const HOST: &str = "probe.rex";

    pub async fn main() -> ExitCode {
        common::require_stack_stopped();
        rexenv_lib::core::stack_guard::allow_real_stack_control();
        let mut check = Check::new("windows_edge_probe");
        for port in [443u16, 80] {
            let holders = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(300)).is_ok();
            check.is(&format!(":{port} is free before the probe"), !holders, "something answers there");
        }
        let root = std::env::temp_dir().join("rexenv edge probe");
        let _ = std::fs::remove_dir_all(&root);
        let plat = common::sandbox_platform_at(root.clone());

        let ca = ssl::load_or_create(plat.paths(), plat.permissions());
        check.is("the local CA is created", ca.is_ok(), &format!("{:?}", ca.as_ref().err()));
        let caddy = binaries::resolve(&*plat, "caddy", binaries::CADDY_VERSION).await;
        check.is("Caddy resolves", caddy.is_ok(), &format!("{caddy:?}"));
        let (Ok(ca), Ok(caddy)) = (ca, caddy) else { return finish(check, &root) };
        let cert = ssl::ensure_site_cert(plat.paths(), plat.permissions(), &ca, HOST);
        check.is("a site certificate is issued", cert.is_ok(), &format!("{:?}", cert.as_ref().err()));
        let Ok(cert) = cert else { return finish(check, &root) };

        // The upstream: a plain HTTP responder the edge proxies to.
        let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
        let upstream_port = upstream.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut conn in upstream.incoming().flatten() {
                let mut buf = [0u8; 4096];
                let _ = conn.read(&mut buf);
                let _ = conn.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nedge-ok");
            }
        });

        let sock = match proxy::admin_socket_path(&*plat) {
            Ok(s) => s,
            Err(e) => {
                check.is("the admin socket path is accepted", false, &e.to_string());
                return finish(check, &root);
            }
        };
        println!("  · admin socket path: {} ({} bytes)", sock.display(), sock.display().to_string().len());
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
            // What production writes on this OS (`EdgeSupervisor::default_bind`, ledger #611).
            default_bind: plat.edge().default_bind().map(str::to_string),
        };
        let caddyfile = proxy::write_caddyfile(&*plat, &cfg).expect("write the Caddyfile");
        let written = std::fs::read_to_string(&caddyfile).unwrap_or_default();
        println!("  · admin line: {}", written.lines().find(|l| l.trim_start().starts_with("admin")).unwrap_or("(none)").trim());
        let log = plat.paths().log_dir().unwrap().join("caddy-stdout.log");

        // ── 1. Start, as written. ──
        // The variants the first two runs tried (without `|0600`, forward slashes, one slash) are gone:
        // the one slash was the answer, and `proxy::admin_address` writes it now.
        let edge = start_and_wait(&mut check, &*plat, &caddy, &caddyfile, &sock, &log, "as written");
        let Some(mut edge) = edge else { return finish(check, &root) };

        // ── 2. The admin API over AF_UNIX. ──
        let config = admin_get(&sock, "/config/");
        println!("  · admin GET /config/: {}", config.as_ref().map(|r| r.lines().next().unwrap_or("").to_string()).unwrap_or_else(|e| e.clone()));
        check.is("rexenv reaches the admin API over the unix socket (AF_UNIX from Rust)", config.as_ref().is_ok_and(|r| r.starts_with("HTTP/1.1 200")), &format!("{config:?}"));

        // ── 3. TLS with the local CA, and the HTTP redirect. ──
        let tls = https_get(&ca.cert_path).await;
        check.is(":443 serves the site over TLS with the local CA's certificate", tls.as_ref().is_ok_and(|(code, body)| *code == 200 && body == "edge-ok"), &format!("{tls:?}"));
        let redirect = plain_get(80);
        check.is(":80 redirects to https", redirect.starts_with("HTTP/1.1 308"), &redirect.lines().next().unwrap_or("").to_string());

        // ── 4. Where it binds, and who may open the socket. ──
        let netstat = Command::new("netstat").args(["-ano", "-p", "TCP"]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
        for port in [":443 ", ":80 "] {
            let rows: Vec<&str> = netstat.lines().filter(|l| l.contains(port) && l.contains("LISTENING")).map(str::trim).collect();
            println!("  · listening on {}: {rows:?}", port.trim());
        }
        let acl = Command::new("icacls").arg(&sock).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_else(|e| e.to_string());
        println!("  · the socket file's ACL:\n{}", acl.lines().map(|l| format!("      {l}")).collect::<Vec<_>>().join("\n"));

        // ── 5. Caddy's CLI through the socket. ──
        let reloaded = proxy::reload(&*plat, &caddy, &caddyfile, false);
        check.is("caddy reload --address <the admin socket> succeeds", reloaded.is_ok(), &format!("{reloaded:?}"));
        let stopped = proxy::stop_admin(&*plat, &caddy);
        check.is("caddy stop --address <the admin socket> succeeds", stopped.is_ok(), &format!("{stopped:?}"));
        let gone = wait(|| TcpStream::connect_timeout(&([127, 0, 0, 1], 443).into(), Duration::from_millis(200)).is_err(), Duration::from_secs(10));
        check.is("after the stop, :443 is released", gone, "still answering");
        edge.stop();
        finish(check, &root)
    }

    fn finish(check: Check, root: &Path) -> ExitCode {
        if let Err(e) = std::fs::remove_dir_all(root) {
            println!("  · fixture not fully removed: {e}");
        }
        check.verdict()
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

    /// Start Caddy on `caddyfile`; `Some` when it stays up and serves `:443` with its socket file
    /// present. A start that fails is stopped and its log tail printed.
    fn start_and_wait(
        check: &mut Check,
        plat: &dyn rexenv_lib::platform::traits::Platform,
        caddy: &Path,
        caddyfile: &Path,
        sock: &Path,
        log: &Path,
        label: &str,
    ) -> Option<OwnedService> {
        let _ = std::fs::remove_file(sock);
        let _ = std::fs::remove_file(log);
        let child = match proxy::start(plat, caddy, caddyfile) {
            Ok(c) => c,
            Err(e) => {
                check.is(&format!("Caddy starts ({label})"), false, &e.to_string());
                return None;
            }
        };
        let mut edge = OwnedService::new(child, "caddy");
        let up = wait(|| sock.exists() && TcpStream::connect_timeout(&([127, 0, 0, 1], 443).into(), Duration::from_millis(200)).is_ok(), Duration::from_secs(15));
        let alive = plat.supervisor().pid_alive(edge.id());
        println!("  · start ({label}): process alive {alive}, socket file {}, :443 answering {}", sock.exists(), up);
        if !up {
            let text = std::fs::read_to_string(log).unwrap_or_default();
            let tail: Vec<&str> = text.lines().rev().take(8).collect::<Vec<_>>().into_iter().rev().collect();
            println!("  · Caddy's log ({label}):\n{}", tail.iter().map(|l| format!("      {}", &l[..l.len().min(400)])).collect::<Vec<_>>().join("\n"));
            check.is(&format!("Caddy comes up with its admin on the unix socket ({label})"), false, "no socket file or no :443");
            edge.stop();
            std::thread::sleep(Duration::from_millis(500));
            return None;
        }
        check.is(&format!("Caddy comes up with its admin on the unix socket ({label})"), true, "");
        Some(edge)
    }

    /// One HTTP request over the AF_UNIX socket at `sock` (Winsock; SO_RCVTIMEO 5 s).
    fn admin_get(sock: &Path, path: &str) -> Result<String, String> {
        use windows_sys::Win32::Networking::WinSock::{
            closesocket, connect, recv, send, setsockopt, socket, WSAGetLastError, WSAStartup, AF_UNIX, INVALID_SOCKET,
            SOCKADDR, SOCKADDR_UN, SOCK_STREAM, SOL_SOCKET, SO_RCVTIMEO, WSADATA,
        };
        let bytes = sock.display().to_string().into_bytes();
        if bytes.len() >= 108 {
            return Err(format!("path is {} bytes; sun_path holds 107", bytes.len()));
        }
        // SAFETY: plain Winsock calls on buffers owned by this frame; the socket is closed on every path.
        unsafe {
            let mut wsa: WSADATA = std::mem::zeroed();
            WSAStartup(0x0202, &mut wsa);
            let s = socket(AF_UNIX as i32, SOCK_STREAM, 0);
            if s == INVALID_SOCKET {
                return Err(format!("socket(AF_UNIX) failed: WSA error {}", WSAGetLastError()));
            }
            let timeout: u32 = 5000;
            setsockopt(s, SOL_SOCKET, SO_RCVTIMEO, (&timeout as *const u32).cast(), 4);
            let mut addr: SOCKADDR_UN = std::mem::zeroed();
            addr.sun_family = AF_UNIX;
            for (i, b) in bytes.iter().enumerate() {
                addr.sun_path[i] = *b as i8;
            }
            if connect(s, (&addr as *const SOCKADDR_UN).cast::<SOCKADDR>(), std::mem::size_of::<SOCKADDR_UN>() as i32) != 0 {
                let e = WSAGetLastError();
                closesocket(s);
                return Err(format!("connect failed: WSA error {e}"));
            }
            let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
            send(s, request.as_ptr(), request.len() as i32, 0);
            let mut out = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = recv(s, buf.as_mut_ptr(), buf.len() as i32, 0);
                if n <= 0 {
                    break;
                }
                out.extend_from_slice(&buf[..n as usize]);
            }
            closesocket(s);
            Ok(String::from_utf8_lossy(&out).into_owned())
        }
    }

    async fn https_get(ca_pem: &Path) -> Result<(u16, String), String> {
        let pem = std::fs::read(ca_pem).map_err(|e| e.to_string())?;
        let client = reqwest::Client::builder()
            .add_root_certificate(reqwest::Certificate::from_pem(&pem).map_err(|e| e.to_string())?)
            .resolve(HOST, "127.0.0.1:443".parse().unwrap())
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| e.to_string())?;
        let resp = client.get(format!("https://{HOST}/")).send().await.map_err(|e| format!("{e:?}"))?;
        let code = resp.status().as_u16();
        Ok((code, resp.text().await.unwrap_or_default()))
    }

    fn plain_get(port: u16) -> String {
        let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else { return "no connection".into() };
        let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
        let _ = write!(s, "GET / HTTP/1.1\r\nHost: {HOST}\r\nConnection: close\r\n\r\n");
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    }
}

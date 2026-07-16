//! core::ports — central port registry + conflict detection (Phase 1 task 10.1).
//!
//! rexenv's services bind known loopback ports (Caddy :80/:443, the shared
//! Nginx, php-fpm, MySQL, the DNS resolver). Before spawning a service the
//! service manager checks its port is free and surfaces a clear error instead
//! of letting the process crash on bind. Detection method depends on the port:
//!  - **privileged TCP (<1024)** can't be bind-tested without root, so we probe
//!    for something already *listening* (a connect attempt);
//!  - **high ports** are bind-tested directly (free iff the bind succeeds).

use crate::core::{db, dns, mail, php, proxy, services};
use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proto {
    Tcp,
    Udp,
}

impl Proto {
    pub fn as_str(&self) -> &'static str {
        match self {
            Proto::Tcp => "tcp",
            Proto::Udp => "udp",
        }
    }
}

/// A port a service needs.
#[derive(Debug, Clone)]
pub struct PortReq {
    pub service: &'static str,
    pub port: u16,
    pub proto: Proto,
}

/// Result of probing one `PortReq`.
#[derive(Debug, Clone)]
pub struct PortStatus {
    pub service: &'static str,
    pub port: u16,
    pub proto: Proto,
    pub free: bool,
}

/// Whether `port` appears usable for `proto` on loopback.
pub fn is_free(port: u16, proto: Proto) -> bool {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    match proto {
        // Privileged TCP: can't bind-test without root — treat "free" as
        // "nothing is currently listening".
        Proto::Tcp if port < 1024 => {
            TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_err()
        }
        Proto::Tcp => TcpListener::bind(addr).is_ok(),
        Proto::Udp => UdpSocket::bind(addr).is_ok(),
    }
}

/// True if something is currently listening on `127.0.0.1:port` (TCP connect
/// probe). Use this for "is the service up?" — unlike a bind probe it's reliable
/// for servers that bind dual-stack / all interfaces (e.g. Caddy).
pub fn is_listening(port: u16) -> bool {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
}

/// Error if `port` is not free — names the service, the process holding the
/// port (when discoverable), and a copy-paste command that frees it, so the
/// user can resolve the conflict without leaving the error message.
///
/// **Format contract with the frontend:** a suggested shell command, when
/// present, is the last line and starts with `"$ "` — the toast layer parses
/// it out to render a copyable command block.
/// Bounded wait for `port` to actually close. A stopped master's workers exit
/// a beat after it; "stopped" must mean the port is FREE, or the next spawn's
/// port gate trips over our own dying tree. True = freed within the budget.
pub fn wait_free(port: u16, proto: Proto, tries: u32, interval: std::time::Duration) -> bool {
    for _ in 0..tries {
        if is_free(port, proto) {
            return true;
        }
        std::thread::sleep(interval);
    }
    is_free(port, proto)
}

pub fn ensure_free(platform: &dyn Platform, port: u16, proto: Proto, service: &str) -> Result<()> {
    if is_free(port, proto) {
        return Ok(());
    }
    let help = platform.supervisor().port_conflict_help(port, matches!(proto, Proto::Udp));
    let by = match &help.holder {
        Some(h) => format!(" by {h}"),
        None => String::new(),
    };
    let mut msg = format!(
        "port {port}/{} (needed by {service}) is already in use{by}.",
        proto.as_str()
    );
    if let Some(cmd) = &help.free_command {
        msg.push_str(&format!(
            " To free it, run this in a terminal, then start services again:\n$ {cmd}"
        ));
    }
    Err(Error::Other(msg))
}

/// The canonical ports rexenv's services use, in startup order. One php-fpm port
/// per pinned PHP version (8.1→9781, 8.2→9782, 8.3→9783).
pub fn default_ports() -> Vec<PortReq> {
    let mut reqs = vec![
        PortReq { service: "DNS resolver", port: dns::DEFAULT_DNS_PORT, proto: Proto::Udp },
        PortReq { service: "Caddy (HTTP)", port: proxy::DEFAULT_HTTP_PORT, proto: Proto::Tcp },
        PortReq { service: "Caddy (HTTPS)", port: proxy::DEFAULT_HTTPS_PORT, proto: Proto::Tcp },
        PortReq { service: "Nginx", port: services::NGINX_HTTP_PORT, proto: Proto::Tcp },
    ];
    for minor in php::all_minors() {
        if let Some(port) = php::fpm_port(&minor) {
            reqs.push(PortReq { service: "PHP-FPM", port, proto: Proto::Tcp });
        }
    }
    for engine in db::DbEngine::ALL {
        reqs.push(PortReq { service: engine.label(), port: engine.port(), proto: Proto::Tcp });
    }
    reqs.push(PortReq { service: "Mailpit (SMTP)", port: mail::MAILPIT_SMTP_PORT, proto: Proto::Tcp });
    reqs.push(PortReq { service: "Mailpit (HTTP)", port: mail::MAILPIT_HTTP_PORT, proto: Proto::Tcp });
    reqs
}

/// Probe every requested port.
pub fn check(reqs: &[PortReq]) -> Vec<PortStatus> {
    reqs.iter()
        .map(|r| PortStatus {
            service: r.service,
            port: r.port,
            proto: r.proto,
            free: is_free(r.port, r.proto),
        })
        .collect()
}

/// Just the conflicts (ports already in use).
pub fn conflicts(reqs: &[PortReq]) -> Vec<PortStatus> {
    check(reqs).into_iter().filter(|s| !s.free).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_tcp_port_bind_probe() {
        // Bind a high TCP port; while held it reads as not-free, then free.
        // Same ephemeral-port-reuse race as the UDP twin below: a concurrent
        // test can re-grab the just-freed port before the second probe, so
        // retry on a fresh port when that happens.
        for _ in 0..10 {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            assert!(!is_free(port, Proto::Tcp));
            // The error must name the port + the service that needs it (§2.1).
            let platform = crate::platform::current();
            let err = ensure_free(&*platform, port, Proto::Tcp, "edge").unwrap_err().to_string();
            assert!(err.contains(&port.to_string()), "msg: {err}");
            assert!(err.contains("edge") && err.contains("in use"), "msg: {err}");
            // macOS discovers the holder (this test process) + suggests a command
            // on a `$ `-prefixed last line (the frontend's parsing contract).
            #[cfg(target_os = "macos")]
            {
                assert!(err.contains(&format!("pid {}", std::process::id())), "msg: {err}");
                assert!(err.lines().last().unwrap().starts_with("$ sudo kill"), "msg: {err}");
            }
            drop(listener);
            if is_free(port, Proto::Tcp) {
                return;
            }
        }
        panic!("freed TCP port never probed free across 10 attempts");
    }

    #[test]
    fn high_udp_port_bind_probe() {
        // A concurrent test binding UDP :0 (e.g. the DNS ones) can re-grab our
        // just-freed ephemeral port before the second probe — macOS hands the
        // last-freed port right back. Retry on a fresh port when that happens.
        for _ in 0..10 {
            let sock = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            let port = sock.local_addr().unwrap().port();
            assert!(!is_free(port, Proto::Udp));
            drop(sock);
            if is_free(port, Proto::Udp) {
                return;
            }
        }
        panic!("freed UDP port never probed free across 10 attempts");
    }

    #[test]
    fn default_ports_cover_all_services() {
        let reqs = default_ports();
        let names: Vec<_> = reqs.iter().map(|r| r.service).collect();
        assert!(names.contains(&"Caddy (HTTPS)"));
        assert!(names.contains(&"MySQL"));
        assert!(names.contains(&"DNS resolver"));
        // HTTPS uses 443.
        assert!(reqs.iter().any(|r| r.port == 443 && r.proto == Proto::Tcp));
        // One php-fpm port per pinned PHP version (e.g. 9783 for 8.3).
        let fpm = reqs.iter().filter(|r| r.service == "PHP-FPM").count();
        assert_eq!(fpm, crate::core::php::all_minors().len());
        assert!(reqs.iter().any(|r| r.port == 9783 && r.proto == Proto::Tcp));
        // One port per DB engine (MySQL 13306 … Redis 16379).
        let dbs = crate::core::db::DbEngine::ALL.len();
        assert!(reqs.iter().any(|r| r.port == 13306 && r.proto == Proto::Tcp));
        assert!(reqs.iter().any(|r| r.port == crate::core::db::REDIS_PORT));
        // Mailpit binds two ports (SMTP + HTTP).
        assert!(names.contains(&"Mailpit (SMTP)"));
        assert!(reqs.iter().any(|r| r.port == 11025 && r.proto == Proto::Tcp));
        assert!(reqs.iter().any(|r| r.port == 18025 && r.proto == Proto::Tcp));
        // DNS + 2 Caddy + Nginx = 4 fixed, plus one php-fpm per version + one per DB
        // engine + Mailpit's 2 ports.
        assert_eq!(reqs.len(), 4 + fpm + dbs + 2);
    }
}

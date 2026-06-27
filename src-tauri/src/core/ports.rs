//! core::ports — central port registry + conflict detection (Phase 1 task 10.1).
//!
//! rexenv's services bind known loopback ports (Caddy :80/:443, the shared
//! Nginx, php-fpm, MySQL, the DNS resolver). Before spawning a service the
//! service manager checks its port is free and surfaces a clear error instead
//! of letting the process crash on bind. Detection method depends on the port:
//!  - **privileged TCP (<1024)** can't be bind-tested without root, so we probe
//!    for something already *listening* (a connect attempt);
//!  - **high ports** are bind-tested directly (free iff the bind succeeds).

use crate::core::{database, dns, proxy, services};
use crate::error::{Error, Result};
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

/// Error if `port` is not free, naming the service so the message is actionable.
pub fn ensure_free(port: u16, proto: Proto, service: &str) -> Result<()> {
    if is_free(port, proto) {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "port {port}/{} (needed by {service}) is already in use",
            proto.as_str()
        )))
    }
}

/// The canonical ports rexenv's services use, in startup order.
pub fn default_ports() -> Vec<PortReq> {
    vec![
        PortReq { service: "DNS resolver", port: dns::DEFAULT_DNS_PORT, proto: Proto::Udp },
        PortReq { service: "Caddy (HTTP)", port: proxy::DEFAULT_HTTP_PORT, proto: Proto::Tcp },
        PortReq { service: "Caddy (HTTPS)", port: proxy::DEFAULT_HTTPS_PORT, proto: Proto::Tcp },
        PortReq { service: "Nginx", port: services::NGINX_HTTP_PORT, proto: Proto::Tcp },
        PortReq { service: "PHP-FPM", port: services::PHP_FPM_PORT, proto: Proto::Tcp },
        PortReq { service: "MySQL", port: database::MYSQL_PORT, proto: Proto::Tcp },
    ]
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
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(!is_free(port, Proto::Tcp));
        assert!(ensure_free(port, Proto::Tcp, "test").is_err());
        drop(listener);
        assert!(is_free(port, Proto::Tcp));
    }

    #[test]
    fn high_udp_port_bind_probe() {
        let sock = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = sock.local_addr().unwrap().port();
        assert!(!is_free(port, Proto::Udp));
        drop(sock);
        assert!(is_free(port, Proto::Udp));
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
        assert_eq!(reqs.len(), 6);
    }
}

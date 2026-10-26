//! core::edge_ports — the edge's HTTP/HTTPS ports and bind address, as the user set them
//! (docs/PLAN-headless-web.md T2, owner ruling D6, 26 Oct 2026).
//!
//! **Why a setting at all.** The edge took `:80`/`:443` unconditionally, so on a server whose own
//! nginx already holds them rexenv could not start (GitHub issue rexenv/rexenv#7). The defaults stay
//! `80`/`443` and the platform's bind, so nothing changes for anyone who never touches these.
//!
//! **Why only while no site exists.** Every site's address carries the port once it is not 443 —
//! WordPress stores `https://x.rex:8443` in `siteurl`/`home` — so a change under existing sites
//! would break every one of them. The first version refuses instead of rewriting them (D6).
//!
//! **What reads these and when.** The ServiceManager takes them at launch, before adoption
//! (`commands::services::load_mirrors`), and at every Start all; the edge picks them up through
//! Caddy's reload, which rebinds. So a change takes effect at the next Start all, and between the
//! write and that start `ServiceManager`'s ports are the ones the running edge was given.

use crate::core::proxy::{DEFAULT_HTTPS_PORT, DEFAULT_HTTP_PORT};
use crate::error::{Error, Result};
use crate::state::store;
use rusqlite::Connection;

pub const HTTP_PORT_KEY: &str = "edge_http_port";
pub const HTTPS_PORT_KEY: &str = "edge_https_port";
pub const BIND_KEY: &str = "edge_bind";

/// The sentence a port change gets while sites exist.
pub const SITES_EXIST: &str = "the edge's ports can change only while there are no sites: every site's \
address carries the port (WordPress stores it), so changing it would break each one. Delete the sites \
first, or keep the current ports.";

/// What the edge listens on, as stored. `bind: None` = the platform's own answer
/// (`EdgeSupervisor::default_bind`: every interface, or `127.0.0.1` on Windows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgePorts {
    pub http: u16,
    pub https: u16,
    pub bind: Option<String>,
}

impl Default for EdgePorts {
    fn default() -> Self {
        Self { http: DEFAULT_HTTP_PORT, https: DEFAULT_HTTPS_PORT, bind: None }
    }
}

/// The stored ports, each falling back to its default when absent or unreadable. A stored value
/// that no longer parses (an edited database) reads as the default rather than failing the launch.
pub fn load(conn: &Connection) -> EdgePorts {
    let port = |key: &str, default: u16| {
        store::get_setting(conn, key).ok().flatten().and_then(|v| v.trim().parse::<u16>().ok()).filter(|p| *p != 0).unwrap_or(default)
    };
    let bind = store::get_setting(conn, BIND_KEY).ok().flatten().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    EdgePorts { http: port(HTTP_PORT_KEY, DEFAULT_HTTP_PORT), https: port(HTTPS_PORT_KEY, DEFAULT_HTTPS_PORT), bind }
}

/// Validating setter for [`HTTP_PORT_KEY`] (registered in `settings_access::GATED_SETTERS`).
pub fn set_http_port(conn: &Connection, value: &str) -> Result<String> {
    set_port(conn, HTTP_PORT_KEY, value)
}

/// Validating setter for [`HTTPS_PORT_KEY`].
pub fn set_https_port(conn: &Connection, value: &str) -> Result<String> {
    set_port(conn, HTTPS_PORT_KEY, value)
}

fn set_port(conn: &Connection, key: &str, value: &str) -> Result<String> {
    let current = load(conn);
    let (mine, other, other_name) = if key == HTTP_PORT_KEY {
        (current.http, current.https, "HTTPS")
    } else {
        (current.https, current.http, "HTTP")
    };
    let port = check_port(value, other, other_name, &taken_by_rexenv())?;
    if port != mine && !store::list_sites(conn)?.is_empty() {
        return Err(Error::Other(SITES_EXIST.into()));
    }
    store::set_setting(conn, key, &port.to_string())?;
    Ok(port.to_string())
}

/// Validating setter for [`BIND_KEY`]: an IP address literal, or empty for the platform's default.
/// Allowed with sites present — it changes who can reach the edge, not any site's address.
pub fn set_bind(conn: &Connection, value: &str) -> Result<String> {
    let bind = check_bind(value)?;
    store::set_setting(conn, BIND_KEY, bind.as_deref().unwrap_or(""))?;
    Ok(bind.unwrap_or_default())
}

/// The ports rexenv's own services already use (`core::ports::default_ports`, minus the edge's
/// two), with the service that uses each.
fn taken_by_rexenv() -> Vec<(u16, &'static str)> {
    crate::core::ports::default_ports()
        .into_iter()
        .filter(|r| !r.service.starts_with("Caddy"))
        .map(|r| (r.port, r.service))
        .collect()
}

/// A port value the edge may take: a whole number 1–65535, not the edge's other port, not one a
/// rexenv service already listens on.
pub(crate) fn check_port(value: &str, other: u16, other_name: &str, taken: &[(u16, &str)]) -> Result<u16> {
    let port: u16 = value
        .trim()
        .parse()
        .ok()
        .filter(|p| *p != 0)
        .ok_or_else(|| Error::Other(format!("`{}` is not a port: use a whole number from 1 to 65535", value.trim())))?;
    if port == other {
        return Err(Error::Other(format!("{port} is already the edge's {other_name} port — the two must differ")));
    }
    if let Some((_, service)) = taken.iter().find(|(p, _)| *p == port) {
        return Err(Error::Other(format!("{port} is rexenv's own {service} port — pick another")));
    }
    Ok(port)
}

/// A bind value: empty (or `default`) clears it; otherwise an IPv4 or IPv6 address literal,
/// stored in its canonical spelling. Anything else is refused: the value is written into the
/// Caddyfile's `default_bind`, and a hostname or a stray token there is config, not an address.
pub(crate) fn check_bind(value: &str) -> Result<Option<String>> {
    let v = value.trim();
    if v.is_empty() || v.eq_ignore_ascii_case("default") {
        return Ok(None);
    }
    v.parse::<std::net::IpAddr>()
        .map(|ip| Some(ip.to_string()))
        .map_err(|_| Error::Other(format!("`{v}` is not an IP address (for example 127.0.0.1, 0.0.0.0 or ::1)")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAKEN: &[(u16, &str)] = &[(18088, "Nginx"), (13306, "MySQL")];

    #[test]
    fn a_port_is_a_whole_number_that_collides_with_nothing() {
        assert_eq!(check_port(" 8443 ", 80, "HTTP", TAKEN).unwrap(), 8443);
        assert_eq!(check_port("1", 80, "HTTP", TAKEN).unwrap(), 1);
        for bad in ["0", "65536", "-1", "443x", "", "8443.0"] {
            assert!(check_port(bad, 80, "HTTP", TAKEN).is_err(), "{bad:?}");
        }
        assert!(check_port("8080", 8080, "HTTPS", TAKEN).unwrap_err().to_string().contains("must differ"));
        assert!(check_port("18088", 80, "HTTP", TAKEN).unwrap_err().to_string().contains("Nginx"));
    }

    #[test]
    fn a_bind_is_an_address_literal_or_the_default() {
        assert_eq!(check_bind("").unwrap(), None);
        assert_eq!(check_bind(" Default ").unwrap(), None);
        assert_eq!(check_bind("127.0.0.1").unwrap().as_deref(), Some("127.0.0.1"));
        assert_eq!(check_bind("0:0:0:0:0:0:0:1").unwrap().as_deref(), Some("::1"), "canonical spelling");
        for bad in ["localhost", "127.0.0.1:80", "127.0.0.1\n\tadmin off", "10.0.0", "*"] {
            assert!(check_bind(bad).is_err(), "{bad:?}");
        }
    }

    /// **No runtime reader names the default ports any more** (ledger #914). Every check of the
    /// edge — the port gate, the wire probe, the conflict help, the database console's client, the
    /// privileged start's sentence — asks the user's ports; the defaults appear only where they ARE
    /// the default. Scans the whole tree, because a file not on a list is exactly where the next
    /// hard-coded 443 would hide (the guard-covers-claimed-surface family). And not only the
    /// constant's NAME: the first version of this guard missed a literal `([127, 0, 0, 1], 443)` in
    /// `confverify.rs`, a `const EDGE_HTTPS_PORT: u16 = 443` in the MCP server and four sentences
    /// saying "port 443" — so loopback-shaped literals and the sentence count too. A public 443
    /// (the tunnel host, `wp_tunnel`'s `SERVER_PORT`) is not the edge and has neither shape.
    #[test]
    fn the_default_ports_are_named_only_where_they_are_the_default() {
        let allowed: &[(&str, &str)] = &[
            ("core/proxy.rs", "the constants, `CaddyConfig::default`, and the legacy `start_privileged` only `caddy_443.rs` calls"),
            ("core/service_manager.rs", "`Ports::default`, replaced by `set_edge_ports` before any start"),
            ("core/edge_ports.rs", "`EdgePorts::default` and `load`'s fallbacks"),
            ("core/ports.rs", "`default_ports`, a thin `ports_for(defaults)`"),
            ("lib.rs", "the database console before the app state exists"),
            ("core/php.rs", "the fixed-port collision table of the pools"),
        ];
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut found = Vec::new();
        fn walk(dir: &std::path::Path, root: &std::path::Path, found: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, root, found);
                } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                    let text = std::fs::read_to_string(&path).unwrap_or_default();
                    // Code only: a comment that recalls `127.0.0.1:443` (Herd's shadow bind) is history.
                    // A whole-line comment, or a trailing ` // …` — never the `//` of `https://`.
                    let body: String = crate::core::copy_scan::production_source(&text)
                        .lines()
                        .map(|l| if l.trim_start().starts_with("//") { "" } else { l.split(" // ").next().unwrap_or("") })
                        .collect::<Vec<_>>()
                        .join("\n");
                    let literal = ["[127, 0, 0, 1], 443", "LOCALHOST, 443", "127.0.0.1:443", "u16 = 443", "port 443", "ports 80 and 443"];
                    if body.contains("DEFAULT_HTTPS_PORT")
                        || body.contains("DEFAULT_HTTP_PORT")
                        || literal.iter().any(|l| body.contains(l))
                    {
                        found.push(path.strip_prefix(root).unwrap().display().to_string().replace('\\', "/"));
                    }
                }
            }
        }
        walk(&root, &root, &mut found);
        for file in &found {
            assert!(
                allowed.iter().any(|(f, _)| f == file),
                "{file} names the edge's default port at runtime — ask the user's \
                 (`commands::services::edge_ports`, `ServiceManager::edge_ports`) instead"
            );
        }
        assert!(found.len() >= 4, "the scan found almost nothing — is it reading the tree? {found:?}");
    }

    #[test]
    fn the_ports_change_only_while_no_site_exists_and_load_falls_back() {
        let conn = crate::state::db::open_in_memory().unwrap();
        assert_eq!(load(&conn), EdgePorts::default());
        assert_eq!(set_https_port(&conn, "8443").unwrap(), "8443");
        assert_eq!(set_http_port(&conn, "8080").unwrap(), "8080");
        assert!(set_http_port(&conn, "8443").is_err(), "the two ports must differ");
        assert_eq!(set_bind(&conn, "0.0.0.0").unwrap(), "0.0.0.0");
        assert_eq!(load(&conn), EdgePorts { http: 8080, https: 8443, bind: Some("0.0.0.0".into()) });
        assert_eq!(set_bind(&conn, "").unwrap(), "");
        assert_eq!(load(&conn).bind, None);

        conn.execute(
            "INSERT INTO sites (id, name, domain, type, php_version, path) VALUES ('s1','A','a.rex','php','8.3','/x/a')",
            [],
        )
        .unwrap();
        assert_eq!(set_https_port(&conn, "9443").unwrap_err().to_string(), SITES_EXIST);
        assert_eq!(set_https_port(&conn, "8443").unwrap(), "8443", "re-setting the same port is no change");
        assert_eq!(set_bind(&conn, "127.0.0.1").unwrap(), "127.0.0.1", "the bind is not part of any address");

        store::set_setting(&conn, HTTPS_PORT_KEY, "not-a-port").unwrap();
        assert_eq!(load(&conn).https, DEFAULT_HTTPS_PORT, "an edited value reads as the default");
    }
}

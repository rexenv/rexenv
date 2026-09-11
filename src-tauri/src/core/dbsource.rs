//! core::dbsource — finding the database servers a Valet/Herd user already runs,
//! and identifying them WITHOUT authenticating (Stage 2 step 3).
//!
//! Two rules shape everything here.
//!
//! **Plists label, listeners decide.** Nothing is ever reported as present
//! because a file said so. DBngin's `DBEngines.plist` on the dev machine says
//! `Status = started` for a MySQL 8.0.27 whose port refuses instantly — verified,
//! not folklore. So config files contribute *labels* ("DBngin calls this MySQL
//! 8.0.27") and candidate *ports*; whether anything is there is decided by a
//! live probe, every time.
//!
//! **Identification never authenticates.** MySQL-protocol servers speak first:
//! the initial handshake carries a protocol byte and a human-readable version
//! before any login. Verified live on 2026-07-26 against both vendors:
//!
//! ```text
//! 127.0.0.1:13306  proto=10  version='8.4.6'
//! 127.0.0.1:13307  proto=10  version='12.3.2-MariaDB'
//! ```
//!
//! That matters beyond tidiness: our bundled MariaDB clients cannot authenticate
//! to MySQL 8 at all (`caching_sha2_password` is excluded from the bottle), so
//! *choosing a client* requires already knowing the vendor. Reading it off the
//! wire breaks that circle. When the wire can't tell us, we say so and let the
//! user declare it — but a declaration can only ever FILL an unknown, never
//! override what the server itself said ([`resolve_vendor`]).

use std::io::Read;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

/// How long a probe may spend connecting, and then waiting for the greeting.
/// A dead loopback port refuses instantly (verified 0.02s); these bounds exist
/// for a filtered or remote host named in someone's config.
const CONNECT_TIMEOUT: Duration = Duration::from_millis(700);
const READ_TIMEOUT: Duration = Duration::from_millis(700);

/// Which MySQL-protocol server we are talking to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Vendor {
    Mysql,
    Mariadb,
}

impl Vendor {
    pub fn label(self) -> &'static str {
        match self {
            Vendor::Mysql => "MySQL",
            Vendor::Mariadb => "MariaDB",
        }
    }
}

/// Proof that a vendor was *declared* rather than observed. Its field is
/// private, so [`Identity::Declared`] cannot be constructed outside this module
/// — the only way to produce one is [`resolve_vendor`], which refuses to
/// contradict a server that identified itself. A caller cannot route around the
/// rule, because the type it would need to build is not available to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct DeclarationGuard(());

/// What we know about a listening server's identity.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Identity {
    /// Read off the wire, pre-authentication. The version is the server's own.
    Handshake { vendor: Vendor, version: String },
    /// The user told us, because the wire couldn't.
    Declared { vendor: Vendor, guard: DeclarationGuard },
    /// Something is listening; it isn't speaking a greeting we understand.
    /// `note` carries the server's own words when it sent an error packet
    /// instead of a greeting (a blocked host, too many connections).
    Unknown { note: Option<String> },
}

impl Identity {
    pub fn vendor(&self) -> Option<Vendor> {
        match self {
            Identity::Handshake { vendor, .. } | Identity::Declared { vendor, .. } => Some(*vendor),
            Identity::Unknown { .. } => None,
        }
    }

    /// True only when the SERVER told us — a declaration is not evidence.
    pub fn is_observed(&self) -> bool {
        matches!(self, Identity::Handshake { .. })
    }
}

/// A user's declaration is refused when it contradicts the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VendorConflict {
    pub observed: Vendor,
    pub observed_version: String,
    pub declared: Vendor,
}

impl VendorConflict {
    pub fn message(&self) -> String {
        format!(
            "This server identified itself as {} {} when it answered, so rexenv won't \
             treat it as {}. Reading the wrong vendor picks client tools that can't even \
             sign in, and a dump taken with them would be wrong in ways that only show up \
             after the restore.",
            self.observed.label(),
            self.observed_version,
            self.declared.label()
        )
    }
}

/// Settle on the vendor to use, given what the server said and what (if
/// anything) the user declared.
///
/// The whole rule lives here, and it is the only way to reach
/// [`Identity::Declared`]:
///
/// - the server identified itself → that wins; a matching declaration is
///   harmless, a contradicting one is REFUSED;
/// - the server didn't → the declaration fills the gap;
/// - neither → we still don't know, and say so.
pub fn resolve_vendor(
    identity: &Identity,
    declared: Option<Vendor>,
) -> Result<Identity, VendorConflict> {
    match (identity, declared) {
        (Identity::Handshake { vendor, version }, Some(d)) if *vendor != d => {
            Err(VendorConflict {
                observed: *vendor,
                observed_version: version.clone(),
                declared: d,
            })
        }
        // Observed wins whether or not it was also declared.
        (obs @ Identity::Handshake { .. }, _) => Ok(obs.clone()),
        (Identity::Declared { vendor, .. }, Some(d)) if *vendor != d => {
            Ok(Identity::Declared { vendor: d, guard: DeclarationGuard(()) })
        }
        (prev @ Identity::Declared { .. }, _) => Ok(prev.clone()),
        (Identity::Unknown { note }, Some(d)) => {
            let _ = note;
            Ok(Identity::Declared { vendor: d, guard: DeclarationGuard(()) })
        }
        (unknown @ Identity::Unknown { .. }, None) => Ok(unknown.clone()),
    }
}

/// Where a candidate port came from. Labels only — never evidence of presence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HintSource {
    /// A site's own config named this host and port. The most reliable pointer
    /// there is: it's what the site actually connects to.
    SiteConfig,
    DBngin,
    Herd,
    /// A default worth trying because it costs one refused connection.
    WellKnown,
}

/// What some config file claims about a port. Never asserted as fact.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hint {
    pub source: HintSource,
    /// e.g. "DBngin MySQL 8.0.27".
    pub label: String,
    pub version: Option<String>,
    /// What the file said about whether it was running. Recorded ONLY so the UI
    /// can contradict it honestly; never used to decide anything.
    pub claimed_status: Option<String>,
}

/// A host/port worth probing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub host: String,
    pub port: u16,
    pub hints: Vec<Hint>,
}

impl Candidate {
    /// The best label we have for the UI, or a plain address.
    pub fn label(&self) -> String {
        self.hints
            .iter()
            .find(|h| !matches!(h.source, HintSource::WellKnown))
            .map(|h| h.label.clone())
            .unwrap_or_else(|| format!("{}:{}", self.host, self.port))
    }
}

/// A server that is actually there, because it answered.
///
/// The `answered` field is private, so a `SourceServer` **cannot be constructed
/// outside this module** — the only thing that mints one is [`discover`], after
/// a live probe returned [`Probe::Listening`]. "Plists label, listeners decide"
/// is therefore enforced by the type rather than by everyone remembering it: no
/// caller can turn a config file's claim into a server, because it cannot build
/// the value that would represent one.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceServer {
    pub host: String,
    pub port: u16,
    pub identity: Identity,
    /// The labels that pointed us here, for display.
    pub hints: Vec<Hint>,
    // Never read, and that is the point: its job is to be UNCONSTRUCTIBLE from
    // outside, so no caller can fabricate a server from a config file's claim.
    #[serde(skip)]
    #[allow(dead_code)]
    answered: Answered,
}

/// Witness that a live probe got an answer. Private field: unforgeable outside
/// this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Answered(());

/// The outcome of one probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    /// Nothing there. Carries why, for the message.
    NotListening(String),
    Listening(Identity),
}

/// Everything discovery found, split by the only distinction that matters.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Discovery {
    /// Verified live. Every entry answered a connection.
    pub servers: Vec<SourceServer>,
    /// Candidates that did NOT answer, with whatever a config file claimed
    /// about them — so the UI can say "DBngin calls this MySQL 8.0.27 and says
    /// it's started; it isn't listening" instead of either believing the file
    /// or hiding it.
    pub silent: Vec<Candidate>,
}

/// Probe one address. Never authenticates, never sends a byte.
pub fn probe(host: &str, port: u16) -> Probe {
    let Ok(mut addrs) = (host, port).to_socket_addrs() else {
        return Probe::NotListening(format!("{host} isn't an address rexenv can resolve"));
    };
    let Some(addr) = addrs.next() else {
        return Probe::NotListening(format!("{host} resolved to nothing"));
    };
    probe_addr(&addr)
}

fn probe_addr(addr: &SocketAddr) -> Probe {
    let mut stream = match TcpStream::connect_timeout(addr, CONNECT_TIMEOUT) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
            return Probe::NotListening("nothing is listening on that port".into())
        }
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
            return Probe::NotListening("the connection timed out".into())
        }
        Err(e) => return Probe::NotListening(e.to_string()),
    };
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    read_greeting(&mut stream)
}

/// Read only the greeting. We never write, so the server sees a client that
/// connected and went away — the cheapest possible visit.
fn read_greeting(stream: &mut impl Read) -> Probe {
    let mut buf = [0u8; 256];
    let n = match stream.read(&mut buf) {
        Ok(n) => n,
        // Listening but silent (PostgreSQL waits for the client to speak first,
        // and so does anything that isn't a MySQL-protocol server).
        Err(_) => return Probe::Listening(Identity::Unknown { note: None }),
    };
    Probe::Listening(identity_from_greeting(&buf[..n]))
}

/// Probe a MySQL-protocol server through its unix SOCKET — same greeting, same
/// parse, never authenticates.
///
/// For a server that turns TCP away before it identifies itself. Local's
/// per-site mysqld runs `skip-name-resolve` with only `root@localhost`, so a TCP
/// connect from 127.0.0.1 gets an ERR packet (1130, "Host '127.0.0.1' is not
/// allowed to connect") IN PLACE of the handshake, while the socket — the route
/// its WordPress uses — greets normally. Measured on Local 10.1.2 / MySQL 8.4.0,
/// 11 Sep 2026: the first Local import on a real site died on exactly this, with
/// the TCP-only probe reporting an unidentifiable server.
pub fn probe_socket(path: &Path) -> Probe {
    let mut stream = match std::os::unix::net::UnixStream::connect(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Probe::NotListening("there's no socket file there — the server isn't running".into())
        }
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
            return Probe::NotListening("the socket file is there, but nothing is listening on it".into())
        }
        Err(e) => return Probe::NotListening(e.to_string()),
    };
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    read_greeting(&mut stream)
}

#[cfg(test)]
mod socket_probe_tests {
    use super::*;

    /// A socket probe reads the greeting a TCP one would — the version off the
    /// wire — and an ERR packet in its place keeps the server's own words.
    #[test]
    fn a_socket_probe_reads_the_handshake_and_keeps_a_refusal_in_the_servers_words() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("rexenv-sockprobe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // A space in the path, as Local's "Application Support" has.
        let path = dir.join("my sql.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let packet = |payload: &[u8]| {
                let mut p = vec![payload.len() as u8, 0, 0, 0];
                p.extend_from_slice(payload);
                p
            };
            let (mut a, _) = listener.accept().unwrap();
            a.write_all(&packet(b"\x0a8.4.0\x00salt")).unwrap();
            let (mut b, _) = listener.accept().unwrap();
            b.write_all(&packet(b"\xff\x6a\x04Host '127.0.0.1' is not allowed to connect to this MySQL server"))
                .unwrap();
        });

        match probe_socket(&path) {
            Probe::Listening(Identity::Handshake { vendor, version }) => {
                assert_eq!((vendor, version.as_str()), (Vendor::Mysql, "8.4.0"))
            }
            p => panic!("the socket greeting was not read: {p:?}"),
        }
        match probe_socket(&path) {
            Probe::Listening(Identity::Unknown { note: Some(n) }) => assert!(n.contains("not allowed"), "{n}"),
            p => panic!("a refusal lost the server's words: {p:?}"),
        }
        server.join().unwrap();
        assert!(matches!(probe_socket(&dir.join("absent.sock")), Probe::NotListening(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Parse a MySQL-protocol initial handshake (or the error packet a server sends
/// instead when it won't talk to us).
///
/// Layout: 3-byte little-endian payload length, 1-byte sequence, then the
/// payload — protocol byte `10`, then a NUL-terminated version string.
pub fn identity_from_greeting(bytes: &[u8]) -> Identity {
    if bytes.len() < 5 {
        return Identity::Unknown { note: None };
    }
    let payload = &bytes[4..];
    // 0xff is an ERR packet: 2-byte code, then (usually) the message. The
    // server's own words beat anything we could invent.
    if payload[0] == 0xff {
        let msg = payload
            .get(3..)
            .map(|m| String::from_utf8_lossy(m).trim_matches(char::from(0)).trim().to_string())
            .filter(|m| !m.is_empty());
        return Identity::Unknown { note: msg };
    }
    if payload[0] != 10 {
        return Identity::Unknown {
            note: Some(format!("it answered with protocol {}, which rexenv doesn't read", payload[0])),
        };
    }
    let Some(end) = payload[1..].iter().position(|b| *b == 0) else {
        return Identity::Unknown { note: None };
    };
    let raw = String::from_utf8_lossy(&payload[1..1 + end]).to_string();
    match vendor_of(&raw) {
        Some((vendor, version)) => Identity::Handshake { vendor, version },
        None => Identity::Unknown {
            note: Some(format!("it called itself {raw:?}, which rexenv doesn't recognise")),
        },
    }
}

/// Vendor + clean version from a handshake version string.
///
/// MariaDB 10.x prefixes `5.5.5-` (the old replication-compatibility hack) —
/// documented, and NOT observed here: our 12.3.2 sends no prefix. It is stripped
/// when present, and a live 10.x sighting should be recorded in the plan.
fn vendor_of(raw: &str) -> Option<(Vendor, String)> {
    let v = raw.strip_prefix("5.5.5-").unwrap_or(raw).trim();
    if v.to_ascii_lowercase().contains("mariadb") {
        return Some((Vendor::Mariadb, v.to_string()));
    }
    // A MySQL-protocol server that doesn't say MariaDB is MySQL or a MySQL
    // derivative (Percona reports `8.0.36-28`), all of which take MySQL tools.
    v.starts_with(|c: char| c.is_ascii_digit()).then(|| (Vendor::Mysql, v.to_string()))
}

// ---------------------------------------------------------------------------
// Candidates
// ---------------------------------------------------------------------------

/// Ports worth probing, from every source we have — deduplicated by address.
///
/// `from_sites` is the union of `host:port` pairs read out of the SITES' own
/// configs (`core::dbimport`), which is the most reliable pointer there is: it
/// is what the site actually connects to. The rest are labels and guesses.
pub fn candidates(home: &Path, from_sites: &[(String, u16)]) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    let mut push = |host: &str, port: u16, hint: Option<Hint>| {
        let host = if host.is_empty() { "127.0.0.1" } else { host };
        // `localhost` in a PHP config means the unix socket, but for a probe it
        // is the same machine — normalise so it doesn't become a second entry.
        let host = if host.eq_ignore_ascii_case("localhost") { "127.0.0.1" } else { host };
        match out.iter_mut().find(|c| c.host == host && c.port == port) {
            Some(existing) => {
                if let Some(h) = hint {
                    if !existing.hints.contains(&h) {
                        existing.hints.push(h);
                    }
                }
            }
            None => out.push(Candidate {
                host: host.to_string(),
                port,
                hints: hint.into_iter().collect(),
            }),
        }
    };

    for (host, port) in from_sites {
        push(
            host,
            *port,
            Some(Hint {
                source: HintSource::SiteConfig,
                label: format!("a site connects to {host}:{port}"),
                version: None,
                claimed_status: None,
            }),
        );
    }
    for h in dbngin_hints(home) {
        push(&h.0, h.1, Some(h.2));
    }
    for h in herd_hints(home) {
        push(&h.0, h.1, Some(h.2));
    }
    for port in [3306u16, 3307, 5432, 8889] {
        push(
            "127.0.0.1",
            port,
            Some(Hint {
                source: HintSource::WellKnown,
                label: format!("the usual port {port}"),
                version: None,
                claimed_status: None,
            }),
        );
    }
    out
}

/// Probe every candidate and split them by what actually answered.
pub fn discover(home: &Path, from_sites: &[(String, u16)]) -> Discovery {
    let mut servers = Vec::new();
    let mut silent = Vec::new();
    for c in candidates(home, from_sites) {
        match probe(&c.host, c.port) {
            Probe::Listening(identity) => servers.push(SourceServer {
                host: c.host,
                port: c.port,
                identity,
                hints: c.hints,
                // The ONLY place this witness is minted, on the only branch
                // where something actually answered.
                answered: Answered(()),
            }),
            // A candidate that didn't answer is NEVER a server, however
            // confidently a plist described it.
            Probe::NotListening(_) => {
                // Only worth showing if something actually claimed it exists.
                if c.hints.iter().any(|h| !matches!(h.source, HintSource::WellKnown)) {
                    silent.push(c);
                }
            }
        }
    }
    Discovery { servers, silent }
}

/// DBngin's engine list: `~/Library/Application Support/com.tinyapp.DBngin/
/// Data/DBEngines.plist`, an XML plist array of flat dicts.
///
/// Read for LABELS ONLY. Its `Status` field is recorded and immediately
/// contradicted where it's wrong — on the dev machine it reads `started` for an
/// engine whose port refuses instantly.
pub fn dbngin_hints(home: &Path) -> Vec<(String, u16, Hint)> {
    let path = home
        .join("Library/Application Support/com.tinyapp.DBngin/Data/DBEngines.plist");
    let Ok(text) = std::fs::read_to_string(&path) else { return Vec::new() };
    // Binary plists exist too; we simply have no hints from one (a hint is never
    // load-bearing, so there is nothing to fall back to).
    if !text.trim_start().starts_with("<?xml") {
        return Vec::new();
    }
    plist_dicts(&text)
        .into_iter()
        .filter_map(|d| {
            let port: u16 = d.get("Port")?.trim().parse().ok()?;
            let kind = d.get("Type").or_else(|| d.get("Name"))?.clone();
            let version = d.get("Version").cloned();
            let label = match &version {
                Some(v) => format!("DBngin {kind} {v}"),
                None => format!("DBngin {kind}"),
            };
            Some((
                "127.0.0.1".to_string(),
                port,
                Hint {
                    source: HintSource::DBngin,
                    label,
                    version,
                    claimed_status: d.get("Status").cloned(),
                },
            ))
        })
        .collect()
}

/// Herd Pro's services, when they exist. The free Herd has no services config
/// at all (verified on the dev machine — `config/` holds only nginx, fpm,
/// dnsmasq, php, valet, certificates), so this degrades to nothing rather than
/// guessing, and the well-known ports still get probed.
///
/// *Unverified against a real Herd Pro install — no such machine here.*
pub fn herd_hints(home: &Path) -> Vec<(String, u16, Hint)> {
    let dir = home.join("Library/Application Support/Herd/config/services");
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        let Some(port) = v.get("port").and_then(|p| p.as_u64()).and_then(|p| u16::try_from(p).ok())
        else {
            continue;
        };
        let kind = v.get("type").and_then(|t| t.as_str()).unwrap_or("database").to_string();
        let version = v.get("version").and_then(|s| s.as_str()).map(|s| s.to_string());
        out.push((
            "127.0.0.1".to_string(),
            port,
            Hint {
                source: HintSource::Herd,
                label: match &version {
                    Some(ver) => format!("Herd {kind} {ver}"),
                    None => format!("Herd {kind}"),
                },
                version,
                claimed_status: v.get("status").and_then(|s| s.as_str()).map(|s| s.to_string()),
            },
        ));
    }
    out
}

/// Flat `<dict>` blocks of an XML plist as key → string maps. Enough for the
/// shapes we read; anything richer is simply not a hint.
fn plist_dicts(xml: &str) -> Vec<std::collections::HashMap<String, String>> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<dict>") {
        let body_start = start + "<dict>".len();
        let Some(end) = rest[body_start..].find("</dict>") else { break };
        let body = &rest[body_start..body_start + end];
        let mut map = std::collections::HashMap::new();
        let mut cursor = body;
        while let Some(ks) = cursor.find("<key>") {
            let after_key = ks + "<key>".len();
            let Some(ke) = cursor[after_key..].find("</key>") else { break };
            let key = cursor[after_key..after_key + ke].to_string();
            let tail = &cursor[after_key + ke + "</key>".len()..];
            // Only string values matter here; a <true/>/<false/> is skipped.
            if let Some(vs) = tail.find("<string>") {
                let before = &tail[..vs];
                if !before.contains("<key>") {
                    let after_val = vs + "<string>".len();
                    if let Some(ve) = tail[after_val..].find("</string>") {
                        map.insert(key, unescape_xml(&tail[after_val..after_val + ve]));
                    }
                }
            }
            cursor = tail;
        }
        if !map.is_empty() {
            out.push(map);
        }
        rest = &rest[body_start + end..];
    }
    out
}

fn unescape_xml(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact bytes both bundled servers sent on 2026-07-26, captured from a
    /// live read. If the parser ever stops reading these, it has regressed
    /// against reality rather than against a guess.
    const MYSQL_GREETING: &[u8] = b"\x49\x00\x00\x00\x0a8.4.6\x00\x01\x00\x00\x00";
    const MARIADB_GREETING: &[u8] = b"\x52\x00\x00\x00\x0a12.3.2-MariaDB\x00\x02\x00\x00\x00";

    #[test]
    fn the_captured_greetings_identify_both_vendors() {
        assert_eq!(
            identity_from_greeting(MYSQL_GREETING),
            Identity::Handshake { vendor: Vendor::Mysql, version: "8.4.6".into() }
        );
        assert_eq!(
            identity_from_greeting(MARIADB_GREETING),
            Identity::Handshake { vendor: Vendor::Mariadb, version: "12.3.2-MariaDB".into() }
        );
    }

    #[test]
    fn mariadb_10_x_five_five_five_prefix_is_stripped() {
        // Documented, not observed here (our 12.3.2 sends no prefix) — the
        // parser handles it so a 10.x source isn't read as MySQL 5.5.
        let g = b"\x40\x00\x00\x00\x0a5.5.5-10.11.2-MariaDB\x00";
        assert_eq!(
            identity_from_greeting(g),
            Identity::Handshake { vendor: Vendor::Mariadb, version: "10.11.2-MariaDB".into() }
        );
    }

    #[test]
    fn a_mysql_derivative_still_takes_mysql_tools() {
        let g = b"\x40\x00\x00\x00\x0a8.0.36-28\x00";
        assert_eq!(
            identity_from_greeting(g),
            Identity::Handshake { vendor: Vendor::Mysql, version: "8.0.36-28".into() }
        );
    }

    #[test]
    fn an_error_packet_carries_the_servers_own_words() {
        // MySQL answers a blocked host with an ERR packet instead of a greeting.
        let mut g = vec![0x30, 0x00, 0x00, 0x00, 0xff, 0x6a, 0x04];
        g.extend_from_slice(b"Host '10.0.0.9' is not allowed to connect");
        match identity_from_greeting(&g) {
            Identity::Unknown { note: Some(n) } => assert!(n.contains("not allowed to connect")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_silent_or_unreadable_server_is_unknown_not_a_guess() {
        assert_eq!(identity_from_greeting(&[]), Identity::Unknown { note: None });
        assert_eq!(identity_from_greeting(b"\x01\x00\x00"), Identity::Unknown { note: None });
        // Postgres never greets; a read that returns junk must not become a vendor.
        assert!(identity_from_greeting(b"\x08\x00\x00\x00\x09nonsense\x00").vendor().is_none());
    }

    #[test]
    fn a_declaration_fills_an_unknown_but_never_overrides_the_wire() {
        let observed =
            Identity::Handshake { vendor: Vendor::Mysql, version: "8.0.27".into() };

        // Fills a gap.
        let filled = resolve_vendor(&Identity::Unknown { note: None }, Some(Vendor::Mariadb))
            .expect("a declaration may fill an unknown");
        assert_eq!(filled.vendor(), Some(Vendor::Mariadb));
        assert!(!filled.is_observed(), "a declaration is not evidence");

        // Agrees harmlessly.
        assert_eq!(
            resolve_vendor(&observed, Some(Vendor::Mysql)).unwrap(),
            observed
        );

        // Contradicts — refused, with the reason.
        let err = resolve_vendor(&observed, Some(Vendor::Mariadb)).unwrap_err();
        assert_eq!(err.observed, Vendor::Mysql);
        assert_eq!(err.declared, Vendor::Mariadb);
        assert!(err.message().contains("identified itself as MySQL 8.0.27"));
        assert!(err.message().contains("can't even sign in"));

        // And with no declaration at all, an observed identity is untouched.
        assert_eq!(resolve_vendor(&observed, None).unwrap(), observed);
    }

    #[test]
    fn the_dbngin_plist_contributes_a_label_and_never_a_claim_of_presence() {
        // The real file from the dev machine, verbatim in shape: it says
        // "started" for an engine whose port refuses instantly.
        let dir = std::env::temp_dir().join("rexenv-dbsource-plist");
        let _ = std::fs::remove_dir_all(&dir);
        let plist_dir = dir.join("Library/Application Support/com.tinyapp.DBngin/Data");
        std::fs::create_dir_all(&plist_dir).unwrap();
        std::fs::write(
            plist_dir.join("DBEngines.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<array>
	<dict>
		<key>AutoStartup</key>
		<false/>
		<key>Name</key>
		<string>MySQL</string>
		<key>Port</key>
		<string>3306</string>
		<key>Status</key>
		<string>started</string>
		<key>Type</key>
		<string>MySQL</string>
		<key>Version</key>
		<string>8.0.27</string>
	</dict>
</array>
</plist>"#,
        )
        .unwrap();

        let hints = dbngin_hints(&dir);
        assert_eq!(hints.len(), 1);
        let (host, port, hint) = &hints[0];
        assert_eq!((host.as_str(), *port), ("127.0.0.1", 3306));
        assert_eq!(hint.label, "DBngin MySQL 8.0.27");
        assert_eq!(hint.version.as_deref(), Some("8.0.27"));
        // We record the claim so we can contradict it — never to act on it.
        assert_eq!(hint.claimed_status.as_deref(), Some("started"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_labelled_candidate_that_does_not_answer_is_reported_as_silent_not_present() {
        // Port 9 is the discard service — reliably nothing on loopback.
        let dir = std::env::temp_dir().join("rexenv-dbsource-silent");
        let _ = std::fs::remove_dir_all(&dir);
        let plist_dir = dir.join("Library/Application Support/com.tinyapp.DBngin/Data");
        std::fs::create_dir_all(&plist_dir).unwrap();
        std::fs::write(
            plist_dir.join("DBEngines.plist"),
            "<?xml version=\"1.0\"?><plist><array><dict>\
             <key>Type</key><string>MySQL</string>\
             <key>Port</key><string>9</string>\
             <key>Status</key><string>started</string>\
             <key>Version</key><string>8.0.27</string>\
             </dict></array></plist>",
        )
        .unwrap();
        let d = discover(&dir, &[]);
        assert!(!d.servers.iter().any(|s| s.port == 9), "nothing answered on 9");
        let silent = d.silent.iter().find(|c| c.port == 9).expect("still shown, as a claim");
        assert_eq!(silent.label(), "DBngin MySQL 8.0.27");
        assert_eq!(silent.hints[0].claimed_status.as_deref(), Some("started"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_or_odd_config_contributes_nothing_and_breaks_nothing() {
        let empty = std::env::temp_dir().join("rexenv-dbsource-absent");
        let _ = std::fs::remove_dir_all(&empty);
        std::fs::create_dir_all(&empty).unwrap();
        assert!(dbngin_hints(&empty).is_empty());
        // Herd Pro's services dir doesn't exist on a free Herd — verified on
        // the dev machine.
        assert!(herd_hints(&empty).is_empty());
        // A binary plist yields no hints rather than garbage.
        let bin_dir = empty.join("Library/Application Support/com.tinyapp.DBngin/Data");
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::write(bin_dir.join("DBEngines.plist"), b"bplist00\xd1\x01\x02").unwrap();
        assert!(dbngin_hints(&empty).is_empty());
        let _ = std::fs::remove_dir_all(&empty);
    }

    #[test]
    fn a_sites_own_config_is_a_candidate_and_localhost_is_not_a_second_one() {
        let dir = std::env::temp_dir().join("rexenv-dbsource-candidates");
        let cands = candidates(
            &dir,
            &[("127.0.0.1".into(), 3306), ("localhost".into(), 3306), ("db.internal".into(), 3399)],
        );
        let loopback_3306: Vec<_> =
            cands.iter().filter(|c| c.host == "127.0.0.1" && c.port == 3306).collect();
        assert_eq!(loopback_3306.len(), 1, "localhost and 127.0.0.1 are one candidate");
        assert!(cands.iter().any(|c| c.host == "db.internal" && c.port == 3399));
        // The well-known ports are always probed — one refused connection each.
        for p in [3306u16, 3307, 5432, 8889] {
            assert!(cands.iter().any(|c| c.port == p), "missing well-known {p}");
        }
        // A site's own pointer outranks the generic label.
        assert_eq!(loopback_3306[0].hints[0].source, HintSource::SiteConfig);
    }

    #[test]
    fn probing_a_dead_port_says_nothing_is_listening() {
        assert_eq!(
            probe("127.0.0.1", 9),
            Probe::NotListening("nothing is listening on that port".into())
        );
    }
}

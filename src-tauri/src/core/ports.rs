//! core::ports — central port registry + conflict detection (Phase 1 task 10.1).
//!
//! rexenv's services bind known loopback ports (Caddy :80/:443, the shared
//! Nginx, php-fpm, MySQL, the DNS resolver). Before spawning a service the
//! service manager checks its port is free and surfaces a clear error instead
//! of letting the process crash on bind. Detection method depends on the port:
//!  - **a platform with socket tables** (Windows) answers first: any holder on ANY
//!    local address makes the port busy, whatever a trial bind says (ledger #599);
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

/// Whether `port` is usable for `proto` on loopback.
///
/// **Never "free" while the platform's socket tables name a holder (ledger #599).**
/// Measured on Windows (plan §6): a trial bind on `127.0.0.1` succeeds beside another
/// process's `0.0.0.0` or `[::]` listener and then takes its localhost traffic, so the
/// bind alone called a developer's own all-interfaces MySQL "free" and stole its
/// clients. The tables see every local address. The bind still runs after them, as a
/// second refusal for anything that refuses a bind without owning a row — WinNAT's
/// run-time excluded ranges are reported to, not measured here; an ADMINISTERED
/// excluded range was measured NOT to (the Dell, 13 Sep 2026: 127.0.0.1, 0.0.0.0 and
/// `[::]` all listened and answered inside 50000–50059) — and on a platform without
/// tables (`port_holders` = `None`) it is the whole answer, exactly as before.
///
/// Takes the platform so no caller can reach the bind-only answer by accident — the
/// old signature had no way to ask the tables, and every caller used it.
pub fn is_free(platform: &dyn Platform, port: u16, proto: Proto) -> bool {
    let udp = matches!(proto, Proto::Udp);
    if let Some(holders) = platform.supervisor().port_holders(port, udp) {
        if !holders.is_empty() {
            return false;
        }
    }
    bind_probe(port, proto)
}

/// The trial bind (or, for privileged TCP, the connect) — half of [`is_free`], never a
/// verdict on its own where the platform can read its tables.
fn bind_probe(port: u16, proto: Proto) -> bool {
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
pub fn wait_free(
    platform: &dyn Platform,
    port: u16,
    proto: Proto,
    tries: u32,
    interval: std::time::Duration,
) -> bool {
    for _ in 0..tries {
        if is_free(platform, port, proto) {
            return true;
        }
        std::thread::sleep(interval);
    }
    is_free(platform, port, proto)
}

pub fn ensure_free(platform: &dyn Platform, port: u16, proto: Proto, service: &str) -> Result<()> {
    if is_free(platform, port, proto) {
        return Ok(());
    }
    // OUR OWN leftover (the app-data marker on the holder's cmdline) is
    // rexenv's problem, never the user's: spawn paths reap it automatically
    // (spawn_override self-heal), so reaching here means that reap failed or
    // an unguarded process hit the gate. Say so honestly — and never suggest
    // `sudo kill` for our own same-user process (the old message did exactly
    // that for an orphaned httpd). TCP only: the listener query is TCP-based.
    if matches!(proto, Proto::Tcp) {
        let ours = platform
            .paths()
            .app_data_dir()
            .ok()
            .map(|d| d.display().to_string())
            .filter(|m| !m.is_empty())
            .and_then(|m| platform.supervisor().owned_master(port, &m));
        if let Some(pid) = ours {
            return Err(Error::Other(format!(
                "port {port}/{} (needed by {service}) is still held by a leftover rexenv \
                 process (pid {pid}). rexenv reclaims these automatically on start — if \
                 this keeps happening, run this in a terminal, then start services again:\n\
                 $ {}",
                proto.as_str(),
                platform.supervisor().stop_pid_command(pid)
            )));
        }
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
pub fn check(platform: &dyn Platform, reqs: &[PortReq]) -> Vec<PortStatus> {
    reqs.iter()
        .map(|r| PortStatus {
            service: r.service,
            port: r.port,
            proto: r.proto,
            free: is_free(platform, r.port, r.proto),
        })
        .collect()
}

/// Just the conflicts (ports already in use).
pub fn conflicts(platform: &dyn Platform, reqs: &[PortReq]) -> Vec<PortStatus> {
    check(platform, reqs).into_iter().filter(|s| !s.free).collect()
}

#[cfg(test)]
mod tests {
    /// Examples that spawn a service and hold it as a bare `Child`, each with
    /// the reason it is safe. Empty is the goal; an entry is an argument.
    const RAW_CHILD_OK: &[(&str, &str)] = &[
        (
            "mcp_mail_check",
            "owns its mailpit through a static + `MailpitGuard` rather than a local, because \
             the child has to be reachable from a signal path — the same deliberate shape \
             `tunnel_exposure_check` uses for its STACK. Wrapping the local as well would \
             give it two owners.",
        ),
        (
            "wp_real443_setup",
            "HOLDS its services alive on purpose. It is part A of a two-step check and a \
             separate foreground step binds Caddy on :443 against them, so reaping on exit \
             would destroy the thing the next step verifies. `let _mysqld` is the correct \
             shape here — the one entry in this list that must NEVER be wrapped.",
        ),
    ];

    /// **No example holds a spawned service as a bare `std::process::Child`.**
    ///
    /// `Child::drop` does NOT kill the process. An example that stops its
    /// service with a line at the end of `main` therefore leaks it on every
    /// other path — and one did: `cli_wp_install_check` panicked mid-run when
    /// `api.wordpress.org` became unresolvable, left mysqld on the **real app
    /// datadir**, and the 20+ examples after it in the network tier all refused
    /// (#413). One example's leak cost the tier.
    ///
    /// `common::OwnedService` reaps in `Drop`, so a panic unwinds and stops the
    /// service. Only `process::exit` skips it, which is the documented limit and
    /// the reason `require_stack_stopped` may exit only BEFORE anything spawns.
    ///
    /// A guard rather than a sweep, for the usual reason: a sweep fixes the
    /// twelve that exist and the thirteenth is written next month. **Two of the
    /// twelve were written the same week, by me, after fixing the first one.**
    #[test]
    fn no_example_holds_a_spawned_service_as_a_bare_child() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
        // The spawn helpers that hand back a `Child`.
        const SPAWNERS: &[&str] = &["database::start(", "mail::start(", "proxy::start("];
        let mut raw = Vec::new();
        let mut scanned = 0usize;
        for entry in std::fs::read_dir(&dir).expect("examples dir") {
            let path = entry.expect("entry").path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let Ok(src) = std::fs::read_to_string(&path) else { continue };
            let name = path.file_stem().unwrap().to_string_lossy().to_string();
            for spawner in SPAWNERS {
                let mut from = 0;
                while let Some(at) = src[from..].find(spawner) {
                    let at = from + at;
                    scanned += 1;
                    // Owned iff an owner appears in the WINDOW around the spawn —
                    // before it (wrapped inline) or just after (spawned into a
                    // local, then wrapped on the next line, which several
                    // examples do for readability).
                    //
                    // **Both owners count.** `Reaped` is the stronger one — it
                    // also sweeps the port for orphaned workers — and an earlier
                    // version of this test looked only for `OwnedService`, so it
                    // named ten examples that were already correct. A guard that
                    // demands a rewrite of working code is worse than no guard;
                    // measuring the hits before trusting them is what caught it.
                    // A LINE window, not a character one: an explanatory
                    // comment between the spawn and its wrapper pushed the
                    // owner past a 300-char window in `wp_plugins_check`, and
                    // the test named a file whose very next statement wraps it.
                    let line_no = src[..at].lines().count();
                    let lines: Vec<&str> = src.lines().collect();
                    let lo = line_no.saturating_sub(4);
                    let hi = (line_no + 8).min(lines.len());
                    let owned = lines[lo..hi]
                        .iter()
                        .any(|l| l.contains("OwnedService::new(") || l.contains("Reaped::new("));
                    if !owned && !RAW_CHILD_OK.iter().any(|(n, _)| *n == name) {
                        raw.push(format!("{name} ({})", spawner.trim_end_matches('(')));
                    }
                    from = at + spawner.len();
                }
            }
        }
        assert!(
            scanned >= 10,
            "only {scanned} service spawns found in examples/ — the scan matched almost \
             nothing and would report a clean tree either way"
        );
        raw.sort();
        raw.dedup();
        assert!(
            raw.is_empty(),
            "these examples hold a spawned service as a bare `Child`, which `Drop` does not \
             kill — a panic or early return leaks it onto a FIXED port and every later \
             example refuses (#413):\n  {}\n  \
             Wrap it: `common::OwnedService::new(database::start(…), \"mysqld\")`.",
            raw.join("\n  ")
        );

        // The exception list may only shrink by being RIGHT. An entry naming an
        // example that no longer spawns anything is a standing excuse nobody is
        // checking — the same hole the `sites_dir` guard closed (#411), and it
        // passed here until it was planted.
        for (name, _) in RAW_CHILD_OK {
            let path = dir.join(format!("{name}.rs"));
            let src = std::fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("RAW_CHILD_OK names {name}, which is gone"));
            assert!(
                SPAWNERS.iter().any(|s| src.contains(s)),
                "RAW_CHILD_OK still excuses {name}, which no longer spawns a service — delete \
                 the entry rather than leaving an exception nobody needs"
            );
        }
    }

    /// **Every service-tier example refuses beside a live stack, and the two
    /// lists of rexenv's ports agree.**
    ///
    /// Two failures this closes, both measured rather than imagined.
    ///
    /// 1. **The per-example guard was never going to be remembered.** After the
    ///    rule was agreed, 20 of 24 service-tier examples still had none —
    ///    including two written the same day, by the person who agreed it. So it
    ///    is enforced here rather than trusted.
    ///
    /// 2. **The runner's port list is shell, the examples' is Rust.** Two
    ///    hand-written copies of the same numbers drift; this asserts they are
    ///    the same set, and `common::rexenv_service_ports` DERIVES its half from
    ///    the constants that decide the ports rather than repeating them.
    ///
    /// What this does NOT check: that the guard is called before anything is
    /// spawned. `process::exit` after an `OwnedService` exists leaks it (a panic
    /// unwinds and reaps; a tidy exit does not), so the call site matters — but
    /// "first statement in main" is a shape a grep cannot judge, and claiming
    /// otherwise would be worse than saying so.
    #[test]
    fn every_service_tier_example_refuses_beside_a_live_stack() {
        let tiers = include_str!("../../../scripts/live-checks.sh");

        // The tier table: `<example> <tier>` lines.
        let service: Vec<&str> = tiers
            .lines()
            .filter_map(|l| {
                let mut it = l.split_whitespace();
                let name = it.next()?;
                let tier = it.next()?;
                (tier == "service" && !name.starts_with('#')).then_some(name)
            })
            .collect();
        assert!(
            service.len() >= 20,
            "only {} service-tier examples found — the tier table's shape changed and this \
             guard is now reading nothing",
            service.len()
        );

        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
        let mut missing = Vec::new();
        for name in &service {
            let path = dir.join(format!("{name}.rs"));
            let Ok(src) = std::fs::read_to_string(&path) else { continue };
            if !src.contains("require_stack_stopped") && !src.contains("require_ports_free") {
                missing.push(*name);
            }
        }
        assert!(
            missing.is_empty(),
            "these service-tier examples do not refuse beside a live stack: {missing:?}\n  \
             Add `common::require_stack_stopped();` as the FIRST statement of main().\n  \
             Beside a running stack these do not collide with it, they JOIN it — a\n  \
             readiness gate that connects is satisfied by the user's server, and the\n  \
             example then reports on services it does not own."
        );

        // The runner's list and the examples' list must be the same set.
        let shell: std::collections::BTreeSet<u16> = tiers
            .lines()
            .find(|l| l.contains("for p in") && l.contains("18088"))
            .expect("the runner's port loop")
            .split_whitespace()
            .filter_map(|w| w.trim_end_matches(';').parse::<u16>().ok())
            .collect();
        let mut derived: std::collections::BTreeSet<u16> = [
            crate::core::services::NGINX_HTTP_PORT,
            crate::core::mail::MAILPIT_HTTP_PORT,
            crate::core::database::MYSQL_PORT,
            crate::core::db::MARIADB_PORT,
        ]
        .into_iter()
        .collect();
        for full in crate::core::binaries::PHP_VERSIONS {
            let minor = full.rsplit_once('.').map(|(m, _)| m).unwrap_or(full);
            if let Some(p) = crate::core::php::fpm_port(minor) {
                derived.insert(p);
            }
        }
        assert_eq!(
            shell, derived,
            "\nscripts/live-checks.sh probes a different set of ports than the code says \
             rexenv uses.\n  shell:   {shell:?}\n  derived: {derived:?}\n  \
             A shipped PHP minor gains a pool port, or a service moves, and one of the two \
             lists is updated — this is the other one."
        );
    }

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
            let platform = crate::platform::current();
            assert!(!is_free(&*platform, port, Proto::Tcp));
            // The error must name the port + the service that needs it (§2.1).
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
            if is_free(&*platform, port, Proto::Tcp) {
                return;
            }
        }
        panic!("freed TCP port never probed free across 10 attempts");
    }

    /// A conflict with OUR OWN leftover (holder cmdline carries the app-data
    /// marker) must say so and give a plain `kill` — never `sudo kill` for a
    /// same-user process rexenv spawned (the original bug told the user to
    /// sudo-kill our own orphaned httpd).
    #[test]
    fn conflict_with_our_own_leftover_says_kill_without_sudo() {
        use crate::platform::traits::*;
        struct OursPaths;
        impl Paths for OursPaths {
            fn app_data_dir(&self) -> Result<std::path::PathBuf> {
                Ok(std::env::temp_dir())
            }
            fn config_dir(&self) -> Result<std::path::PathBuf> {
                unimplemented!()
            }
            fn log_dir(&self) -> Result<std::path::PathBuf> {
                unimplemented!()
            }
            fn bin_dir(&self) -> Result<std::path::PathBuf> {
                unimplemented!()
            }
            fn hosts_file(&self) -> std::path::PathBuf {
                unimplemented!()
            }
        }
        struct OursSup;
        impl ProcessSupervisor for OursSup {
            fn spawn(
                &self,
                _: &std::path::Path,
                _: &[String],
            ) -> Result<std::process::Child> {
                unimplemented!()
            }
            fn spawn_logged(
                &self,
                _: &std::path::Path,
                _: &[String],
                _: &std::path::Path,
            ) -> Result<std::process::Child> {
                unimplemented!()
            }
            fn stop(&self, _: u32) -> Result<()> {
                unimplemented!()
            }
            fn owned_master(&self, _: u16, _: &str) -> Option<u32> {
                Some(4321)
            }
        }
        struct OursPlatform;
        impl Platform for OursPlatform {
            fn paths(&self) -> &dyn Paths {
                &OursPaths
            }
            fn supervisor(&self) -> &dyn ProcessSupervisor {
                &OursSup
            }
            fn dns(&self) -> &dyn DnsManager {
                unimplemented!()
            }
            fn cert_trust(&self) -> &dyn CertTrustManager {
                unimplemented!()
            }
            fn privileges(&self) -> &dyn PrivilegeManager {
                unimplemented!()
            }
            fn autostart(&self) -> &dyn AutostartManager {
                unimplemented!()
            }
            fn permissions(&self) -> &dyn PermissionManager {
                unimplemented!()
            }
            fn shell(&self) -> &dyn ShellRunner {
                unimplemented!()
            }
            fn binaries(&self) -> &dyn BinaryProvider {
                unimplemented!()
            }
            fn edge(&self) -> &dyn EdgeSupervisor {
                unimplemented!()
            }
            fn dns_agent(&self) -> &dyn DnsAgentManager {
                unimplemented!()
            }
        fn app_bundle(&self) -> &dyn AppBundle { unimplemented!() }
        }
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let err = ensure_free(&OursPlatform, port, Proto::Tcp, "Apache")
            .unwrap_err()
            .to_string();
        assert!(err.contains("leftover rexenv process (pid 4321)"), "msg: {err}");
        assert!(err.lines().last().unwrap().starts_with("$ kill 4321"), "msg: {err}");
        assert!(!err.contains("sudo"), "never sudo for our own process: {err}");
        drop(listener);
    }

    /// **A port the tables say is held is busy even when a trial bind on it
    /// succeeds (ledger #599)** — the Windows shape measured on the Dell, where the
    /// bind succeeds beside another process's wildcard listener. And the tables only
    /// ADD refusals: an empty table still runs the bind, which refuses a port a
    /// socket holds — whatever refuses a bind without a row still refuses.
    ///
    /// Plant: dropping the `port_holders` check from `is_free` fails the first two
    /// assertions (the bind on a free port says free); returning early on an empty
    /// table fails the last one.
    #[test]
    fn a_holder_in_the_tables_is_busy_whatever_the_trial_bind_says() {
        use crate::platform::traits::*;
        /// Socket tables that say what they are told to.
        struct TableSup(Option<Vec<u32>>);
        impl ProcessSupervisor for TableSup {
            fn spawn(&self, _: &std::path::Path, _: &[String]) -> Result<std::process::Child> {
                unimplemented!()
            }
            fn spawn_logged(
                &self,
                _: &std::path::Path,
                _: &[String],
                _: &std::path::Path,
            ) -> Result<std::process::Child> {
                unimplemented!()
            }
            fn stop(&self, _: u32) -> Result<()> {
                unimplemented!()
            }
            fn port_holders(&self, _: u16, _: bool) -> Option<Vec<u32>> {
                self.0.clone()
            }
        }
        /// Those tables over the real platform's paths; nothing else is reached.
        struct TablePlatform {
            sup: TableSup,
            real: Box<dyn Platform>,
        }
        impl Platform for TablePlatform {
            fn paths(&self) -> &dyn Paths {
                self.real.paths()
            }
            fn supervisor(&self) -> &dyn ProcessSupervisor {
                &self.sup
            }
            fn dns(&self) -> &dyn DnsManager {
                unimplemented!()
            }
            fn cert_trust(&self) -> &dyn CertTrustManager {
                unimplemented!()
            }
            fn privileges(&self) -> &dyn PrivilegeManager {
                unimplemented!()
            }
            fn autostart(&self) -> &dyn AutostartManager {
                unimplemented!()
            }
            fn permissions(&self) -> &dyn PermissionManager {
                unimplemented!()
            }
            fn shell(&self) -> &dyn ShellRunner {
                unimplemented!()
            }
            fn binaries(&self) -> &dyn BinaryProvider {
                unimplemented!()
            }
            fn edge(&self) -> &dyn EdgeSupervisor {
                unimplemented!()
            }
            fn dns_agent(&self) -> &dyn DnsAgentManager {
                unimplemented!()
            }
            fn app_bundle(&self) -> &dyn AppBundle {
                unimplemented!()
            }
        }
        let held = TablePlatform { sup: TableSup(Some(vec![4242])), real: crate::platform::current() };
        for proto in [Proto::Tcp, Proto::Udp] {
            // A port nothing binds any more: the trial bind alone would call it free.
            let port = match proto {
                Proto::Tcp => TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap().local_addr().unwrap().port(),
                Proto::Udp => UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap().local_addr().unwrap().port(),
            };
            assert!(!is_free(&held, port, proto), "{proto:?}: a table holder must make the port busy");
            let err = ensure_free(&held, port, proto, "MySQL").unwrap_err().to_string();
            assert!(err.contains("in use") && err.contains("MySQL"), "{proto:?}: {err}");
        }
        let empty = TablePlatform { sup: TableSup(Some(Vec::new())), real: crate::platform::current() };
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(!is_free(&empty, port, Proto::Tcp), "an empty table must not skip the bind");
    }

    #[test]
    fn high_udp_port_bind_probe() {
        // A concurrent test binding UDP :0 (e.g. the DNS ones) can re-grab our
        // just-freed ephemeral port before the second probe — macOS hands the
        // last-freed port right back. Retry on a fresh port when that happens.
        let platform = crate::platform::current();
        for _ in 0..10 {
            let sock = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            let port = sock.local_addr().unwrap().port();
            assert!(!is_free(&*platform, port, Proto::Udp));
            drop(sock);
            if is_free(&*platform, port, Proto::Udp) {
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

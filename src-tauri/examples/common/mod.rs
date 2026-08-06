//! Shared helpers for the live-check examples.
//!
//! Not an example itself: Cargo auto-discovers `examples/*.rs` and
//! `examples/*/main.rs`, so a `mod.rs` in a subdirectory is compiled only where
//! it is declared (`mod common;`).
//!
//! # THE INVARIANT — read this before writing an example
//!
//! **Examples run against REAL app data and REAL processes.** They link the app
//! library, resolve the same binary cache, and (historically) wrote into the
//! same `~/Library/Application Support/dev.rexenv.rexenv` tree the user's
//! running stack lives in. So:
//!
//! > Anything an example WRITES, SPAWNS or DELETES must be inside
//! > fixture-owned scope — a sandbox app-data root, a temp directory the
//! > example created, a Drop-owned process guard. Never a derived path, never
//! > the shared prefix, never a path computed from the real `Paths`.
//!
//! **The actual shape of this boundary — where it is structure and where it is
//! discipline.** [`sandbox`] makes the invariant STRUCTURAL, but only for the
//! ~20 of 109 examples that call it; the rest run on the REAL `Paths` and rest
//! on per-example review (a [`Reaped`] guard for spawns, a self-created temp dir
//! for writes), not on a type that redirects the paths. And even under
//! `sandbox` the shared binary cache is a DELIBERATE real, mutable exception:
//! examples add to it, and `download_progress_check` deliberately DELETES one
//! content-addressed entry to test re-download. So the honest boundary is not
//! "an example writes nothing real" — it is "an example writes nothing real
//! EXCEPT the content-addressed binary cache, and only the ~20 sandbox callers
//! have even that guaranteed by structure rather than by review."
//!
//! This has now bitten three times, each fixed per-instance until this note:
//!
//! 1. an example `rm -rf`'d `docroot.parent()` and took the user's whole Sites
//!    folder with it;
//! 2. examples SIGKILLed php-fpm masters and leaked title-rewritten workers
//!    that squatted ports for days (fixed by [`Reaped`], below);
//! 3. examples started nginx with the REAL `cfg.nginx_prefix`, so their nginx
//!    wrote — and on exit cleared — the running stack's `nginx.pid`. The real
//!    master stayed alive but became undiscoverable, and the user's very next
//!    site import failed with `nginx -s reload` → `invalid PID number ""`.
//!
//! [`sandbox`] is the structural answer to (3): it hands back a `Platform`
//! whose paths are all temporary, so an example never HOLDS the real config
//! dir, prefix or pid path and cannot pass one by accident. Use it for anything
//! that generates configs or spawns services. The deliberate exceptions are
//! documented on [`sandbox`] itself.
//!
//! ## Cleanup discipline
//!
//! An example that spawns a service must reap it from a DROP GUARD, never from
//! a statement at the end of `main`: everything in between is an `assert!` /
//! `.expect()` that can unwind straight past that statement.
//!
//! And a FORKING service (php-fpm, httpd, nginx) has to be stopped gracefully.
//! SIGKILL to the master leaves its workers alive, reparented to pid 1, still
//! holding the listen socket — the failure mode `Proc::alive`'s doc comment
//! describes. Production solves this with [`Proc::terminate`] (SIGTERM → poll →
//! SIGKILL as a last resort); examples get the same behavior here rather than a
//! second implementation.
//!
//! One caveat worth stating, because it is what made the leak invisible: php-fpm
//! workers REWRITE their process title to `php-fpm: pool www`, which contains
//! neither the binary path nor the config path. A sweep keyed on the executable
//! would miss precisely the processes that leak, so [`Reaped`] matches on the
//! program NAME (`php-fpm`, `httpd`, `nginx`).

#![allow(dead_code)] // each example uses a subset

// ---------------------------------------------------------------------------
// Fixture ports
// ---------------------------------------------------------------------------
//
// An example must NEVER bind a production port (docs/PORTS.md) — a sandboxed
// example on :18088 still collides with the user's running stack, and the
// runner may execute examples while the stack is up. Claim a port here (one
// line per example, unique, in the 18100+/9790+/13390+ bands) and reference
// the const; a hardcoded port literal in an example is a review smell.
// Already-claimed fixture ports live in their examples today: 18097
// (retry_recovery_check), 18099 (linked_site_check, valet_import_check),
// 13397-13399 (config_rewrite/db_restore/db_dump), 9799 (apache_site_check),
// 9998/19003 (xdebug_pool_check), 18131/9791 (nginx_php_serve), 9792
// (php_fpm_serve), 18132/9793 (dotfile_guard_check).

use rexenv_lib::error::Result as RexResult;
use rexenv_lib::platform::traits::{
    AutostartManager, BinaryProvider, CertTrustManager, DnsAgentManager, DnsManager,
    EdgeSupervisor, Paths, Platform, PermissionManager, PrivilegeManager, ProcessSupervisor,
    ShellRunner,
};
use rexenv_lib::core::proc::Proc;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Sandboxed platform
// ---------------------------------------------------------------------------

/// `Paths` rooted in a throwaway directory.
///
/// Every service path the app computes — the config dir, the log dir, the nginx
/// PREFIX and therefore `nginx.pid`, the `run/` sockets, the SQLite file, certs
/// — is derived from these, so redirecting `Paths` redirects all of them at
/// once. That is what makes the sandbox structural rather than a convention.
struct SandboxPaths {
    root: PathBuf,
    /// The REAL binary cache: the one deliberate exception. It is
    /// content-addressed, checksum-verified and atomically published, and not
    /// sharing it would mean re-downloading ~600 MB of MySQL per example run.
    /// Deliberately MUTABLE, not add-only (that was overstated): examples add to
    /// it, and `download_progress_check` deletes one content-addressed entry to
    /// exercise re-download — safe only because every entry is re-fetchable by
    /// checksum.
    bin: PathBuf,
    hosts: PathBuf,
}

fn ensure(p: PathBuf) -> RexResult<PathBuf> {
    std::fs::create_dir_all(&p)?;
    Ok(p)
}

impl Paths for SandboxPaths {
    fn app_data_dir(&self) -> RexResult<PathBuf> {
        ensure(self.root.clone())
    }
    fn config_dir(&self) -> RexResult<PathBuf> {
        ensure(self.root.join("config"))
    }
    fn log_dir(&self) -> RexResult<PathBuf> {
        ensure(self.root.join("logs"))
    }
    fn bin_dir(&self) -> RexResult<PathBuf> {
        Ok(self.bin.clone())
    }
    fn hosts_file(&self) -> PathBuf {
        self.hosts.clone()
    }
    fn cli_symlink_path(&self) -> RexResult<PathBuf> {
        // An example must never install or remove the user's `rex` symlink.
        Err(rexenv_lib::error::Error::Unsupported("CLI PATH install (sandboxed example)"))
    }
}

/// The real platform with its paths swapped for [`SandboxPaths`]. Everything
/// else (process supervision, privileges, DNS command builders) delegates
/// unchanged, because those are what the example is usually there to exercise.
struct SandboxPlatform {
    inner: Box<dyn Platform>,
    paths: SandboxPaths,
}

impl Platform for SandboxPlatform {
    fn paths(&self) -> &dyn Paths {
        &self.paths
    }
    fn dns(&self) -> &dyn DnsManager {
        self.inner.dns()
    }
    fn cert_trust(&self) -> &dyn CertTrustManager {
        self.inner.cert_trust()
    }
    fn privileges(&self) -> &dyn PrivilegeManager {
        self.inner.privileges()
    }
    fn supervisor(&self) -> &dyn ProcessSupervisor {
        self.inner.supervisor()
    }
    fn autostart(&self) -> &dyn AutostartManager {
        self.inner.autostart()
    }
    fn permissions(&self) -> &dyn PermissionManager {
        self.inner.permissions()
    }
    fn shell(&self) -> &dyn ShellRunner {
        self.inner.shell()
    }
    fn binaries(&self) -> &dyn BinaryProvider {
        self.inner.binaries()
    }
    fn edge(&self) -> &dyn EdgeSupervisor {
        self.inner.edge()
    }
    fn dns_agent(&self) -> &dyn DnsAgentManager {
        self.inner.dns_agent()
    }
}

/// Removes the sandbox tree when the example ends, however it ends.
pub struct SandboxGuard {
    root: PathBuf,
}

impl SandboxGuard {
    /// Where the sandbox lives — for printing, or for planting fixtures.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for SandboxGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A `Platform` whose app data is a throwaway directory.
///
/// Use this in ANY example that generates configs or spawns services. The
/// example then never holds the real config dir, nginx prefix or pid path, so
/// it cannot pass one by accident — the failure mode that broke a user's stack
/// (see the module note). Keep the guard alive for the whole run; dropping it
/// deletes the sandbox.
///
/// ```ignore
/// let (plat, _sandbox) = common::sandbox("my_check");
/// let cfg = sites::rebuild_configs(&conn, &*plat, &ca, PORT, 8081, 8444)?; // writes into the sandbox
/// ```
///
/// Two deliberate exceptions, stated on the methods above: the binary cache is
/// shared (re-downloading 600 MB per run is worse) and MUTABLE — examples add to
/// it and one deletes a content-addressed entry to test re-download (safe:
/// re-fetchable by checksum); the hosts file is the real one (only ever read).
/// Privileged operations
/// and the DNS command builders also delegate to the real platform — an example
/// that calls those is asking for a real system change and should say so.
pub fn sandbox(tag: &str) -> (Box<dyn Platform>, SandboxGuard) {
    let real = rexenv_lib::platform::current();
    let bin = real.paths().bin_dir().expect("real binary cache");
    let hosts = real.paths().hosts_file();
    let root = std::env::temp_dir()
        .join(format!("rexenv-sandbox-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create the sandbox root");
    let platform = SandboxPlatform {
        inner: real,
        paths: SandboxPaths { root: root.clone(), bin, hosts },
    };
    // Belt: prove the sandbox really is somewhere else before handing it out.
    // If a future change ever makes these coincide, an example would start
    // writing the user's stack again — and that failure is silent until their
    // next reload breaks, which is exactly how this got shipped once already.
    let real_data = rexenv_lib::platform::current()
        .paths()
        .app_data_dir()
        .expect("real app data dir");
    let sandboxed = platform.paths().app_data_dir().expect("sandbox app data dir");
    assert!(
        sandboxed != real_data && !sandboxed.starts_with(&real_data),
        "sandbox root {} is inside the REAL app data {} — an example must never write there",
        sandboxed.display(),
        real_data.display()
    );
    (Box::new(platform), SandboxGuard { root })
}

// ---------------------------------------------------------------------------
// Fixture SQLite
// ---------------------------------------------------------------------------

/// Deletes the fixture database file when the example ends, however it ends.
pub struct FixtureDb {
    path: PathBuf,
}

impl FixtureDb {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for FixtureDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// A migrated, throwaway app database in the temp dir — the replacement for
/// the hand-rolled `temp_dir().join("rexenv-<task>.db")` idiom, whose
/// hardcoded names collided across examples (`rexenv-6_1.db` was two different
/// examples' database; concurrent runs corrupted each other). The pid suffix
/// makes concurrent runs safe; the guard cleans up what the old idiom left
/// behind forever. Keep the guard alive for the whole run.
pub fn fixture_db(tag: &str) -> (rusqlite::Connection, FixtureDb) {
    let path = std::env::temp_dir()
        .join(format!("rexenv-fixture-{tag}-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let conn = rexenv_lib::state::db::open(&path).expect("open fixture SQLite");
    (conn, FixtureDb { path })
}

// ---------------------------------------------------------------------------
// Plain HTTP probe
// ---------------------------------------------------------------------------

/// One loopback HTTP/1.1 GET with an explicit Host header (how the shared
/// nginx routes vhosts). Returns the raw response (headers + body), or the
/// error as a string — callers assert on content either way. Deliberately
/// std-only: a probe with its own connection pool would hide first-connection
/// failures.
pub fn http_get(port: u16, host: &str, path: &str) -> String {
    http_get_timeout(port, host, path, Duration::from_secs(5))
}

/// [`http_get`] with an explicit client read timeout — for probes that are
/// deliberately SLOWER than a server-side default under test (nginx's own
/// 60s `fastcgi_read_timeout`, say), where the 5s default would time out the
/// prober instead of the thing being proven.
pub fn http_get_timeout(port: u16, host: &str, path: &str, timeout: Duration) -> String {
    use std::io::{Read, Write};
    let run = || -> std::io::Result<String> {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port))?;
        s.set_read_timeout(Some(timeout))?;
        write!(s, "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")?;
        let mut out = String::new();
        s.read_to_string(&mut out)?;
        Ok(out)
    };
    run().unwrap_or_else(|e| format!("<probe error: {e}>"))
}

// ---------------------------------------------------------------------------
// Uniform verdict reporting
// ---------------------------------------------------------------------------

/// The one pass/fail contract every example should speak: ✓/✗ lines while
/// running, one `<name>: PASS`/`FAIL` line at the end, exit code 0/1 — so a
/// runner (scripts/live-checks.sh) can trust any example's exit status.
///
/// End `main` with `return checks.verdict();` (signature
/// `fn main() -> std::process::ExitCode`) rather than `std::process::exit` —
/// exit skips destructors, and the whole point of [`Reaped`]/[`SandboxGuard`]/
/// [`FixtureDb`] is that they run.
pub struct Check {
    name: &'static str,
    failed: u32,
}

impl Check {
    pub fn new(name: &'static str) -> Self {
        Self { name, failed: 0 }
    }

    /// Record one named assertion. `detail` prints only on failure.
    pub fn is(&mut self, label: &str, pass: bool, detail: &str) {
        if pass {
            println!("  ✓ {label}");
        } else {
            println!("  ✗ {label} — {detail}");
            self.failed += 1;
        }
    }

    pub fn all_passed(&self) -> bool {
        self.failed == 0
    }

    /// Print the one green/red line and return the exit code for `main`.
    pub fn verdict(self) -> std::process::ExitCode {
        if self.failed == 0 {
            println!("{}: PASS", self.name);
            std::process::ExitCode::SUCCESS
        } else {
            println!("{}: FAIL ({} check(s))", self.name, self.failed);
            std::process::ExitCode::FAILURE
        }
    }
}

/// A spawned check process that is reaped when it goes out of scope — including
/// when an assertion panics and unwinds.
///
/// Owns the [`Child`] exclusively: nothing else may `wait`/`kill` it, so the pid
/// can never be signalled after it has been reaped (and possibly reused).
///
/// `std::process::exit` does NOT run destructors, so an example that exits
/// non-zero must call [`Reaped::reap`] explicitly on that path — it is
/// idempotent, and the guard stays as the panic backstop.
pub struct Reaped {
    proc: Option<Proc>,
    pid: u32,
    /// Dedicated fixture port this service listens on. Swept after the master
    /// dies, since a forking service's workers can outlive it.
    port: u16,
    /// Program name as it appears in `ps` output — see the module note on
    /// title-rewritten workers.
    marker: String,
}

impl Reaped {
    /// Take exclusive ownership of `child`, listening on the FIXTURE `port`.
    ///
    /// `marker` must be the program name (`php-fpm`, `httpd`), not a path.
    /// `port` must be a throwaway port the example dedicates to this check —
    /// the sweep is reachable only through this type precisely so it can never
    /// be pointed at a port the running stack owns.
    pub fn new(child: Child, port: u16, marker: impl Into<String>) -> Self {
        let pid = child.id();
        Self {
            proc: Some(Proc::Child(child, Instant::now())),
            pid,
            port,
            marker: marker.into(),
        }
    }

    /// The spawned master's pid — valid for printing after reaping too.
    pub fn id(&self) -> u32 {
        self.pid
    }

    /// Stop the service and free its port. Idempotent.
    pub fn reap(&mut self) {
        if let Some(mut proc) = self.proc.take() {
            proc.terminate();
            sweep_port(self.port, &self.marker);
        }
    }
}

impl Drop for Reaped {
    fn drop(&mut self) {
        self.reap();
    }
}

/// Kill anything still LISTENING on `port` whose command line contains
/// `marker` — the orphaned workers a dead master leaves behind.
///
/// Deliberately private: reachable only via [`Reaped`], so a sweep always
/// belongs to a process this example spawned on a port it chose.
fn sweep_port(port: u16, marker: &str) {
    let ours = |pid: u32| command_of(pid).contains(marker);

    for pid in listeners_on(port).into_iter().filter(|p| ours(*p)) {
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }
    for _ in 0..20 {
        if listeners_on(port).is_empty() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    for pid in listeners_on(port).into_iter().filter(|p| ours(*p)) {
        let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
    }
}

/// Pids listening on `port` (empty when the port is free or `lsof` is absent).
fn listeners_on(port: u16) -> Vec<u32> {
    let out = Command::new("lsof")
        .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-t"])
        .output();
    let Ok(out) = out else { return Vec::new() };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .collect()
}

/// A pid's full command line, or "" if it is gone.
fn command_of(pid: u32) -> String {
    Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

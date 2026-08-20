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
//! ## …and the gap `sandbox` does NOT close: PORTS
//!
//! **`sandbox` covers paths and says nothing about ports**, and every
//! service-or-network-tier example inherits that gap. It bites in the direction
//! that looks like success: with the user's stack up, an example that brings up
//! its own services binds — or worse, TALKS TO — theirs. `mcp_mail_check` found
//! it with Mailpit (its legs would have planted test mail in a real inbox and
//! proven the filter against the user's own mail). `tunnel_exposure_check` found
//! it again with MySQL on the shared port, where `install_for_site` would have
//! created its fixture database inside the user's RUNNING engine — same accident,
//! bigger blast radius, and both would have read as the example working.
//!
//! So the refusal lives HERE, in [`require_ports_free`], rather than in each
//! example: a correct implementation with a warning beside it did not stop the
//! same mistake being made twice in this repo (see `core::copy_scan`), and the
//! next network-tier example should not have to rediscover it.
//!
//! ## …and the SECOND gap: the sites dir
//!
//! `sites::provision` puts a docroot under the `sites_dir` SETTING, which falls
//! back to `~/rexenv/Sites` — a path computed from the home directory, **not
//! from `Paths`**. A sandboxed `Platform` cannot redirect it. So an example that
//! sandboxes its paths and provisions a site writes into the USER'S real Sites
//! folder, and nothing says so.
//!
//! That is not hypothetical either: it happened on 13 Aug 2026, hours after the
//! ports gap above was written down. Documenting a gap did not prevent the next
//! instance of it — so this one is closed by SHAPE, twice over:
//!
//! - [`sandbox_db`] is the one-liner that opens an example's database, and it
//!   pins `sites_dir` into the sandbox as it does. The easy path is the safe
//!   one; reaching the unsafe path now means deliberately calling
//!   `db::open_for_platform` yourself.
//! - [`SandboxGuard`] snapshots the real Sites folder when the sandbox is
//!   created and SHOUTS on drop if anything new appeared there — including on a
//!   panic. Prevention for the ordinary path, a loud backstop for the rest.
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
//!
//! ## The verdict contract: exit 0 means PROVEN
//!
//! `scripts/live-checks.sh` takes an example's verdict from its EXIT STATUS and
//! nothing else, and `docs/TESTING.md` states the rule as "exit 0 = proven,
//! non-zero = not". So a precondition that fails must reach the exit code, and
//! the idiom that quietly broke this in 13 examples is:
//!
//! ```ignore
//! if let Err(e) = mgr.start_all(..).await { eprintln!("start_all failed: {e}"); return; }
//! ```
//!
//! A bare `return` from `async fn main() -> ()` **exits 0**. The tier prints
//! `all green` for a run in which the stack never came up, no assertion
//! executed, and every readiness gate below was jumped over — and the operator
//! sees the `start_all failed:` line only if they read the log of a run that
//! passed. Fixed 21 Aug 2026 by giving those `main`s a return type:
//! `async fn main() -> std::process::ExitCode`, `ExitCode::FAILURE` on the
//! precondition path, `ExitCode::SUCCESS` at the end.
//!
//! **`ExitCode::FAILURE`, not `process::exit(1)`** — the two are not equivalent
//! here. `exit` runs no destructors, so it skips the [`Reaped`] and
//! [`OwnedService`] guards and the `ServiceManager`'s own `Drop`, turning a
//! failed run into a leaked service holding a production port. Returning the
//! code unwinds the stack normally first. (The same reasoning is why
//! [`await_ready`] panics rather than exits.)

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
// (php_fpm_serve), 18132/9793/9794/9795 (dotfile_guard_check), 9796/9797 (fpm_candidate_check), 18134-18136
// (edge_wire_check: a squatting listener, a deliberately EMPTY port, and a
// plaintext responder — the three shapes `proxy::edge_wire` must tell apart).

/// Open an example's database inside the sandbox, with `sites_dir` PINNED.
///
/// Use this instead of `db::open_for_platform` in anything that provisions a
/// site. It exists because the pin is invisible when it is missing: the site is
/// created, the example works, and the folders turn up in the user's own Sites
/// directory. See the module doc for what that cost.
pub fn sandbox_db(platform: &dyn Platform) -> rusqlite::Connection {
    let conn = rexenv_lib::state::db::open_for_platform(platform.paths())
        .expect("open the sandbox database");
    pin_sites_dir(&conn, platform);
    conn
}

/// Pin `sites_dir` into the sandbox on a connection the example opened ITSELF.
///
/// [`sandbox_db`] is the preferred door because it cannot be used without the
/// pin. This exists for the examples that open their database somewhere of their
/// own choosing (a named file under the sandbox root, a fixture path a later
/// assertion refers to) and would otherwise have to give that up to be safe.
///
/// Called by 11 examples found on 14 Aug 2026 by grepping for `sandbox()` callers
/// that provision without pinning. Two had been found before that — one on
/// 13 Aug, one this morning — and BOTH were caught by [`SandboxGuard`]'s alarm
/// after the fact rather than by review. The alarm is the backstop working; the
/// eleven are what it had not happened to catch yet, because catching them
/// required someone to run them.
pub fn pin_sites_dir(conn: &rusqlite::Connection, platform: &dyn Platform) {
    let sites = platform
        .paths()
        .app_data_dir()
        .expect("sandbox app data")
        .join("Sites");
    std::fs::create_dir_all(&sites).expect("sandbox sites dir");
    rexenv_lib::state::store::set_setting(conn, "sites_dir", &sites.to_string_lossy())
        .expect("pin the sandbox sites dir");
}

/// Every entry in the REAL sites folder, for the guard's before/after.
fn real_sites_snapshot() -> Vec<String> {
    let Ok(dir) = rexenv_lib::core::sites::default_sites_dir() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<String> =
        entries.flatten().filter_map(|e| e.file_name().into_string().ok()).collect();
    out.sort();
    out
}

/// Refuse to run while anything is listening on a port this example needs.
///
/// Call it BEFORE creating or starting anything. Each entry is
/// `(port, what it is and what borrowing it would do)` — the second half is the
/// message a reader gets, so say what would happen, not just which port.
///
/// Exits the process rather than returning an error: there is nothing sensible
/// to do with the failure, and a caller that could ignore it is the shape this
/// exists to prevent.
pub fn require_ports_free(ports: &[(u16, &str)]) {
    for (port, what) in ports {
        if rexenv_lib::core::ports::is_listening(*port) {
            eprintln!(
                "\n✗ REFUSING TO RUN — something is already on a port this needs\n  \
                 127.0.0.1:{port} is answering ({what}).\n  \
                 This example brings up its OWN services: `common::sandbox` makes the PATHS \
                 temporary and does nothing about PORTS, so running beside a live stack means \
                 using the user's.\n  Stop the stack (in rexenv, or `rex stop`), then run this \
                 again.\n"
            );
            std::process::exit(1);
        }
    }
}

/// Block until `port` is accepting connections, or die naming the service.
///
/// # The defect this exists to end
///
/// Three examples spawned a service and then USED it with nothing in between:
/// `apache_site_check` (no wait at all), `dotfile_guard_check` (a flat
/// `sleep(800ms)`, which is a timing assumption wearing a wait's clothes), and
/// the shape recurs wherever a pool is spawned before a server that fronts it.
/// The server binds in milliseconds and satisfies its own readiness loop at
/// once; a cold php-fpm under CPU contention has not bound yet, so the first
/// request reaches the front-end with nothing behind it.
///
/// What made it expensive was not the flake — it was the MISDIAGNOSIS. The
/// failure surfaced downstream as `php-via-fpm=false` (Apache), `502` (nginx),
/// `503` (httpd): all of which read as "the web server cannot execute PHP" and
/// send the reader at the wrong subject. `apache_site_check`'s instance sat in
/// `docs/TODO.md` as an unexplained transient from 3 Aug 2026 until the second
/// capture, on 20 Aug, showed the two PHP legs failing while the two static
/// legs passed — which is the pool's fingerprint, not the server's.
///
/// So: wait HERE, and when the wait fails, fail HERE — with the port and the
/// service named, before any downstream check can offer a plausible wrong
/// answer. Exits the process for [`require_ports_free`]'s reason: there is
/// nothing sensible for a caller to do with it, and a caller that could ignore
/// it is the shape this exists to prevent.
///
/// `log` is an optional file to spill on failure (php-fpm's error_log is the
/// one that says WHY, and every one of these examples was discarding it).
pub fn await_listening(port: u16, what: &str, log: Option<&Path>) {
    await_ready(&format!("{what} (127.0.0.1:{port})"), log, || {
        rexenv_lib::core::ports::is_listening(port)
    });
}

/// [`await_listening`] for readiness that is not a TCP port.
///
/// The edge's admin surface is a UNIX SOCKET by design — a TCP admin on a root
/// Caddy is arbitrary file r/w as root (docs/ARCHITECTURE.md) — so the examples
/// that wait for it cannot poll a port at all, and a helper that only knew
/// about ports would have sent them back to flat sleeps. Same contract: poll,
/// then fail HERE with the subject named and its log spilled, so a missing
/// precondition never masquerades as a broken subject downstream.
pub fn await_ready(what: &str, log: Option<&Path>, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if ready() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let said = log
        .map(|path| match std::fs::read_to_string(path) {
            Ok(text) if !text.trim().is_empty() => {
                format!("\n  its own log ({}):\n{text}", path.display())
            }
            Ok(_) => format!("\n  its own log ({}) is empty — it never got far enough to write", path.display()),
            Err(e) => format!("\n  its own log ({}) is unreadable: {e}", path.display()),
        })
        .unwrap_or_default();
    // PANIC, never `process::exit`. This is the difference between reporting a
    // leak and CAUSING one: `exit` skips every Drop, so the `Reaped` and
    // `OwnedService` guards that own the just-spawned children never run and
    // those children keep their ports. Measured on 20 Aug 2026, the first day
    // this helper existed — one `exit` here left a php-fpm on :9783 and took
    // out EIGHT later examples in the same tier run, every one of them
    // reporting "already in use by rexenv (php-fpm, pid 11694)". A readiness
    // guard that poisons the rest of the suite is worse than the flake it
    // replaced.
    //
    // `require_ports_free` may still `exit`, and the distinction is the point:
    // it runs BEFORE anything is spawned, so it has nothing to leak. This runs
    // after.
    panic!(
        "{what} was not ready within 20s.\n  \
         Every check below that needs it would fail as though the SERVICE were \
         broken — this says otherwise, here, before they run.{said}"
    );
}

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
    /// The real Sites folder as it was when the sandbox opened. Compared on
    /// drop — see the module doc's second gap.
    real_sites_before: Vec<String>,
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
        // The backstop for the sites-dir gap. Runs on a panic too, which is
        // exactly when it is needed: on 13 Aug 2026 an example provisioned two
        // sites into the user's REAL Sites folder, then died, and nothing said
        // so — the folders were found by hand afterwards. This does NOT delete
        // anything: a cleanup that guessed at what was a fixture is a worse
        // failure than the litter, and one this project has already had.
        let after = real_sites_snapshot();
        let new: Vec<&String> =
            after.iter().filter(|d| !self.real_sites_before.contains(d)).collect();
        if !new.is_empty() {
            let dir = rexenv_lib::core::sites::default_sites_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            eprintln!(
                "\n!! THIS EXAMPLE WROTE INTO THE USER'S REAL SITES FOLDER !!\n   \
                 {dir}\n   new: {new:?}\n   \
                 `common::sandbox` sandboxes PATHS; the docroot comes from the `sites_dir` \
                 SETTING, which falls back to the home directory. Open the example's database \
                 with `common::sandbox_db`, which pins it.\n   \
                 Nothing has been deleted — check these are yours before removing them.\n"
            );
        }
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
    // NOT `std::env::temp_dir()`, and the difference is 37 bytes that decide
    // whether the edge can start at all. macOS's per-user TMPDIR is 49
    // characters (`/var/folders/51/kd63p2nj6sz67msbn4pvzc3h0000gq/T/`), which
    // put every sandboxed `<root>/config/caddy-admin.sock` at 110-115 bytes
    // against a 103-byte ceiling — so caddy died on `bind: invalid argument`
    // before it ever listened, in EVERY sandboxed example that enables the
    // admin socket. The note below blames the TAG, and that reading is what hid
    // this: shortening `create_site_serve` to `createsrv` (8 characters off)
    // still failed, because the base was always the larger half. `/private/tmp`
    // is 12 characters and puts the same paths at 75-79.
    //
    // The tradeoff, stated: `/private/tmp` is world-writable where TMPDIR is
    // per-user 0700. The root is still pid-scoped and still removed by
    // `SandboxGuard`, and this is fixture scaffolding on a developer's machine
    // — but it IS a weaker directory, and that is the price of an edge that
    // starts.
    let root = std::path::PathBuf::from("/private/tmp")
        .join(format!("rexenv-sandbox-{tag}-{}", std::process::id()));

    // A sandbox root that is too LONG breaks the edge, and the failure names
    // nothing. Caddy's admin unix socket lives at `<root>/config/caddy-admin.sock`
    // and macOS binds at most 103 bytes of socket path, so whether a sandboxed
    // example can start the edge depends on how it was NAMED: `wp_create_serve`
    // came to 108 bytes and caddy said only `bind: invalid argument` (14 Aug
    // 2026). Refuse here, where the length is chosen, with the arithmetic — an
    // example that fails on its own tag should say so in one line.
    //
    // This is the FIXTURE half. The same ceiling exists in production
    // (`core::proxy::check_unix_socket_len`), where the driver is the user's home
    // directory rather than a tag.
    let sock_len = root.join("config").join("caddy-admin.sock").as_os_str().len();
    if sock_len > rexenv_lib::core::proxy::MAX_UNIX_SOCKET_PATH {
        let over = sock_len - rexenv_lib::core::proxy::MAX_UNIX_SOCKET_PATH;
        // A WARNING, not a refusal — and the first version got this wrong.
        // Whether the length matters depends on whether the example's edge asks
        // for an ADMIN SOCKET, and `sandbox` cannot know that: the sandboxed edge
        // in `tunnel_exposure_check` runs with admin off, so the path is never
        // bound and the example is fine. Refusing here blocked a working check —
        // found by migrating that example onto the shared guard in the same
        // session, which is the argument for migrating in the same session.
        // The hard failure belongs at the point of USE, where the answer is
        // known: `core::proxy::admin_socket_path` refuses there with the same
        // arithmetic. This line exists so a future example that DOES ask for the
        // socket learns why before it reads "bind: invalid argument".
        eprintln!(
            "\n⚠ sandbox tag {tag:?} produces a caddy admin socket path of {sock_len} bytes; \
             macOS binds at most {} (over by {over}).\n  \
             Harmless unless this example's edge enables the ADMIN socket — if it does, it \
             will fail with \"bind: invalid argument\". Shorten the tag by {over}.\n",
            rexenv_lib::core::proxy::MAX_UNIX_SOCKET_PATH
        );
    }

    let _ = std::fs::remove_dir_all(&root);
    // Sweep this tag's LEFTOVERS from earlier runs. `SandboxGuard`'s Drop
    // removes the root, but an example that ends with `process::exit` — which
    // every `fail()` in this tree does — runs no destructors, so a failing
    // run leaves its root behind. Two failed `tunnel_exposure_check` runs left
    // 280 MB each (13 Aug 2026). Self-healing beats remembering: the next run
    // of the SAME example clears them, and nothing outside this tag is touched.
    if let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) {
        let prefix = format!("rexenv-sandbox-{tag}-");
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name.starts_with(&prefix) && entry.path() != root {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
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
    (Box::new(platform), SandboxGuard { root, real_sites_before: real_sites_snapshot() })
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

/// Own a service that listens on a SHARED (production) port and stop it on drop
/// — including when the example panics.
///
/// **Why not [`Reaped`].** That type also sweeps its port for title-rewritten
/// workers, and the sweep decides ownership by PROGRAM NAME. On a fixture port
/// that is unambiguous. On 9783 or 18088 the user's own php-fpm and nginx match
/// the same names, so pointing the sweep there could kill the running stack —
/// which is exactly what `Reaped`'s contract forbids. Ownership here is the
/// `Child` handle: a process THIS example spawned, never ambiguous.
///
/// **What it does not cover, stated because the gap is the interesting part.**
/// [`Proc::terminate`] sends SIGTERM and only escalates to SIGKILL after a
/// grace period, and a master that shuts down on SIGTERM takes its workers with
/// it. A master that ignores SIGTERM gets killed and CAN leave workers holding
/// the port — the case `Reaped`'s sweep exists for. Pair this with
/// [`require_ports_free`] so that residue surfaces in the NEXT run as a refusal
/// naming the port, rather than as a service quietly borrowed from a corpse.
///
/// Written 14 Aug 2026 after `wp_create_serve` panicked before its teardown
/// lines and left mysqld, nginx and php-fpm running — while its sandbox datadir
/// was removed on drop, leaving a MySQL answering on a directory that no longer
/// existed. Three later examples connected to it and failed with
/// `ERROR 3680: Failed to create schema directory (errno 2)`, which names
/// nothing, and the wrong three examples got blamed.
pub struct OwnedService {
    proc: Option<Proc>,
    pid: u32,
    what: &'static str,
}

impl OwnedService {
    /// Take exclusive ownership of a service `child`. `what` is for messages.
    pub fn new(child: Child, what: &'static str) -> Self {
        let pid = child.id();
        Self { proc: Some(Proc::Child(child, Instant::now())), pid, what }
    }

    /// The spawned pid — valid for printing after stopping too.
    pub fn id(&self) -> u32 {
        self.pid
    }

    /// Stop the service. Idempotent; runs from `Drop` on the panic path too.
    pub fn stop(&mut self) {
        if let Some(mut proc) = self.proc.take() {
            proc.terminate();
        }
    }

    /// Name, for a teardown line that says what it stopped.
    pub fn what(&self) -> &'static str {
        self.what
    }
}

impl Drop for OwnedService {
    fn drop(&mut self) {
        self.stop();
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

// ---------------------------------------------------------------------------
// Public tunnels
// ---------------------------------------------------------------------------

/// The live tunnel's pid, reachable from every teardown path (a static, because
/// `fail()` and a panic hook cannot be handed a value).
static TUNNEL_PID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// Its public URL, printed if the reap cannot prove the process died.
static TUNNEL_URL: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
static HOOK_INSTALLED: std::sync::Once = std::sync::Once::new();

/// Guard for a tunnel that is PUBLIC while it runs.
///
/// A leaked cloudflared is the one outcome an example must not produce, and the
/// three paths that end a run today all skip something:
///
/// - normal scope exit → `Drop` runs;
/// - `fail()` → `process::exit` runs NO destructors (this has bitten twice —
///   a Mailpit guard, then this file's own tunnel), so `fail` must reap first;
/// - a panic → `Drop` runs while unwinding, but only if the guard is still in
///   scope, so a hook is installed as well.
///
/// [`tunnel_guard_check`](../tunnel_guard_check.rs) proves all three by running
/// this example's own binary in each mode and asserting the child is dead
/// afterwards — arranged, not assumed.
///
/// # The guard's own failure mode
///
/// `tunnels::stop` used to be called as `let _ = …`: best-effort, silent. If the
/// tunnel did not die, nothing said so and a public URL stayed up. So this reaps
/// in three escalating steps and PROVES the outcome — stop, then SIGTERM, then
/// SIGKILL, polling for the process to actually go. If it is still alive after
/// all three, the example SHOUTS the pid and the URL, because at that point the
/// only remaining remedy is a human with a terminal.
pub struct PublicTunnel(());

/// Register a live tunnel and install the panic hook. Hold the returned guard
/// for the rest of the run.
pub fn adopt_public_tunnel(pid: u32, public_url: &str) -> PublicTunnel {
    TUNNEL_PID.store(pid, std::sync::atomic::Ordering::SeqCst);
    if let Ok(mut u) = TUNNEL_URL.lock() {
        *u = public_url.to_string();
    }
    HOOK_INSTALLED.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            reap_public_tunnel();
            prev(info);
        }));
    });
    eprintln!("public tunnel adopted: pid {pid} at {public_url}");
    PublicTunnel(())
}

/// Record the public URL once it is known. A quick tunnel's URL does not exist
/// until cloudflared prints it, so the guard is adopted BEFORE the URL — the pid
/// is the thing that must be reachable from the teardown paths, and a tunnel
/// with an unknown URL still has to die.
pub fn note_public_tunnel_url(url: &str) {
    if let Ok(mut u) = TUNNEL_URL.lock() {
        *u = url.to_string();
    }
}

/// Alive means RUNNING — a zombie counts as dead, exactly as production's
/// `process_running` counts it. This cannot be `kill -0`: that succeeds on a
/// zombie, and the tunnel pid here is this example's OWN child, which nothing
/// ever `wait()`s — so from the moment cloudflared exits until the example
/// does, `kill -0` answers "alive" about a corpse. Found 15 Aug 2026 when the
/// guard's evidence file recorded `tunnel_guard_check`'s sleep stand-in as
/// surviving SIGKILL (it was a zombie throughout); the 14 Aug "still alive 3s
/// after tunnels::stop" sighting has the same measurement under it.
fn pid_alive(pid: u32) -> bool {
    match std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "state="])
        .output()
    {
        Ok(o) => {
            let state = String::from_utf8_lossy(&o.stdout);
            let state = state.trim();
            !state.is_empty() && !state.starts_with('Z')
        }
        Err(_) => false,
    }
}

fn gone_within(pid: u32, ms: u64) -> bool {
    wait_dead(pid, ms).is_some()
}

/// Poll until `pid` is gone; `Some(elapsed ms)` if it died within `ms`.
fn wait_dead(pid: u32, ms: u64) -> Option<u64> {
    let start = Instant::now();
    loop {
        if !pid_alive(pid) {
            return Some(start.elapsed().as_millis() as u64);
        }
        if start.elapsed() >= Duration::from_millis(ms) {
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Append one reap observation to the persistent evidence file for the
/// "does `tunnels::stop` routinely need SIGTERM?" question (docs/TODO.md,
/// filed 14 Aug 2026 on ONE sighting).
///
/// This file exists because the original plan — "watch the guard's output over
/// the next few tunnel runs" — could not work: a successful `live-checks.sh`
/// run DELETES its log directory, so the only sightings that persist are ones
/// a human happened to be watching. Every reap now leaves a line here, fast
/// path included — the base rate ("N runs, all dead under a second") is itself
/// evidence, and it is exactly what a one-sighting question needs.
///
/// Lives under `target/` (compile-time manifest dir): machine-local,
/// gitignored, survives across runs, and an example writing there is not
/// touching anything real.
fn record_tunnel_stop_evidence(pid: u32, outcome: &str, elapsed_ms: u64) {
    use std::io::Write;
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/target/tunnel-stop-evidence.log");
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let example = std::env::args()
        .next()
        .map(|a| {
            Path::new(&a).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or(a)
        })
        .unwrap_or_default();
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "epoch={epoch} example={example} pid={pid} outcome={outcome} elapsed_ms={elapsed_ms}");
    }
}

/// Stop the registered tunnel and PROVE it stopped. Idempotent; safe from
/// `Drop`, from `fail()`, and from the panic hook.
pub fn reap_public_tunnel() {
    let pid = TUNNEL_PID.swap(0, std::sync::atomic::Ordering::SeqCst);
    if pid == 0 {
        return;
    }
    let url = TUNNEL_URL.lock().map(|u| u.clone()).unwrap_or_default();
    let plat = rexenv_lib::platform::current();

    let start = Instant::now();
    let _ = rexenv_lib::core::tunnels::stop(&*plat, pid);
    if let Some(ms) = wait_dead(pid, 3000) {
        record_tunnel_stop_evidence(pid, "stop", ms);
        eprintln!("public tunnel reaped: pid {pid} stopped in {ms}ms (verified dead)");
        return;
    }
    // The production stop path did not take within 3s. Observed once on the
    // first real run of this guard (14 Aug 2026), and whether that was a
    // defect in `tunnels::stop` or a graceful shutdown slower than the window
    // could not be told apart — signalling immediately DESTROYS the evidence,
    // because a death right after SIGTERM is indistinguishable from the
    // earlier stop still finishing. So the anomalous path now watches a
    // further 7s before escalating: a death in the 3–10s window with no signal
    // sent is "graceful but slow", recorded as such; survival past 10s is the
    // stop path genuinely not taking. The cost is bounded (≤7s more of a
    // teardown that is already anomalous) and only ever paid on the case the
    // TODO question is about. The previous `let _ = tunnels::stop(..)` would
    // have returned quietly with the process still up and the URL still public.
    eprintln!(
        "public tunnel pid {pid} still alive 3s after tunnels::stop — \
         watching to 10s before escalating (slow-graceful vs stop-defect)"
    );
    if wait_dead(pid, 7000).is_some() {
        let total = start.elapsed().as_millis() as u64;
        record_tunnel_stop_evidence(pid, "stop_slow_no_signal", total);
        eprintln!(
            "public tunnel reaped: pid {pid} stopped in {total}ms with NO signal sent — \
             tunnels::stop worked, just slower than 3s (recorded)"
        );
        return;
    }
    record_tunnel_stop_evidence(pid, "stop_no_effect_10s", start.elapsed().as_millis() as u64);
    eprintln!("public tunnel pid {pid} still alive 10s after tunnels::stop — escalating to SIGTERM");
    let _ = std::process::Command::new("kill").arg(pid.to_string()).status();
    if gone_within(pid, 3000) {
        record_tunnel_stop_evidence(pid, "sigterm", start.elapsed().as_millis() as u64);
        eprintln!("public tunnel reaped: pid {pid} stopped after SIGTERM (verified dead)");
        return;
    }
    let _ = std::process::Command::new("kill").args(["-9", &pid.to_string()]).status();
    if gone_within(pid, 3000) {
        record_tunnel_stop_evidence(pid, "sigkill", start.elapsed().as_millis() as u64);
        eprintln!("public tunnel reaped: pid {pid} stopped after SIGKILL (verified dead)");
        return;
    }
    record_tunnel_stop_evidence(pid, "survived_all", start.elapsed().as_millis() as u64);
    eprintln!(
        "\n!! A PUBLIC TUNNEL IS STILL RUNNING AND THIS EXAMPLE COULD NOT STOP IT !!\n  \
         pid {pid} survived tunnels::stop, SIGTERM and SIGKILL.\n  \
         public URL: {url}\n  \
         It is reachable from the internet until someone kills it by hand:\n    \
         kill -9 {pid}\n"
    );
}

impl Drop for PublicTunnel {
    fn drop(&mut self) {
        reap_public_tunnel();
    }
}

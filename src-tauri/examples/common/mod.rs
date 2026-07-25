//! Shared helpers for the live-check examples.
//!
//! Not an example itself: Cargo auto-discovers `examples/*.rs` and
//! `examples/*/main.rs`, so a `mod.rs` in a subdirectory is compiled only where
//! it is declared (`mod common;`).
//!
//! ## Cleanup discipline (why this module exists)
//!
//! An example that spawns a service must reap it from a DROP GUARD, never from
//! a statement at the end of `main`: everything in between is an `assert!` /
//! `.expect()` that can unwind straight past that statement. Two examples got
//! this wrong and left php-fpm workers holding :9998 and :9799 for eleven days.
//!
//! And a FORKING service (php-fpm, httpd) has to be stopped gracefully. SIGKILL
//! to the master leaves its workers alive, reparented to pid 1, still holding
//! the listen socket — the failure mode `Proc::alive`'s doc comment describes
//! ("php-fpm workers outlive a SIGKILLed master and keep accepting"). Production
//! solves this with [`Proc::terminate`] (SIGTERM → poll → SIGKILL as a last
//! resort); examples get the same behavior here rather than a second
//! implementation.
//!
//! One caveat worth stating, because it is what made the leak invisible: php-fpm
//! workers REWRITE their process title to `php-fpm: pool www`, which contains
//! neither the binary path nor the config path. A sweep keyed on the executable
//! would miss precisely the processes that leak, so [`Reaped`] matches on the
//! program NAME (`php-fpm`, `httpd`).

#![allow(dead_code)] // each example uses a subset

use rexenv_lib::core::proc::Proc;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

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

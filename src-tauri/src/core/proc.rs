//! core::proc — a handle to a supervised long-running process.
//!
//! Services outlive the app by design: quitting rexenv leaves the stack
//! serving, and the next launch ADOPTS the survivors instead of restarting
//! them (`ServiceManager::adopt_startup`). A `Proc` is either a child this
//! session spawned (we own the OS handle) or a prior session's process we
//! adopted by pid.

use std::process::Child;
use std::time::{Duration, Instant};

pub enum Proc {
    /// Spawned this session — we own the OS child handle (can `wait`/`kill`).
    /// The `Instant` is the spawn time, backing the health watchdog's start
    /// grace ([`Proc::starting`]).
    Child(Child, Instant),
    /// Adopted from a prior app session: pid only. Stopped via
    /// `ProcessSupervisor::stop` like any other service; never killed
    /// implicitly (it isn't our child and outliving the app is intended).
    Adopted(u32),
    /// Started by THIS session through a launcher that detaches the server —
    /// PostgreSQL through `pg_ctl` on Windows, which is the only way the server
    /// runs there (#698). A pid like [`Proc::Adopted`], because there is no
    /// child handle to hold, but with the spawn instant, because it is OURS and
    /// the watchdog's start grace must apply.
    ///
    /// Adopting it instead cost a shipped bug: `pg_ctl -w` takes seconds to
    /// return, an adopted handle is never "starting", so the watchdog fired
    /// mid-start, tried to respawn, and refused its OWN server's port —
    /// "[restart-failed] … already in use by postgres.exe" with the cluster
    /// serving perfectly (measured on the VM, 20 Sep 2026).
    Detached(u32, Instant),
}

impl Proc {
    /// How long after a spawn the health watchdog trusts a closed port to mean
    /// "still starting" rather than "dead". Spawns happen under the services
    /// lock but readiness is awaited AFTER it drops (the locking rule), so a
    /// watchdog tick can land in the gap and probe a healthy child that hasn't
    /// bound its port yet — without this grace it would kill + respawn it
    /// mid-start (and could respawn-loop a slow starter into `gave-up`).
    /// 2× the longest readiness budget (30 tries × 500ms); a genuinely wedged
    /// start is still caught on the first tick after the window.
    pub const START_GRACE: Duration = Duration::from_secs(30);

    pub fn id(&self) -> u32 {
        match self {
            Proc::Child(c, _) => c.id(),
            Proc::Adopted(pid) | Proc::Detached(pid, _) => *pid,
        }
    }

    /// Whether this handle was adopted from a prior session rather than
    /// spawned by this process — the `stack_guard` provenance check: a non-app
    /// process may stop only what it spawned.
    pub fn is_adopted(&self) -> bool {
        // `Detached` is NOT adopted: this session started it, so the provenance
        // check ("may stop only what it spawned") answers yes for it.
        matches!(self, Proc::Adopted(_))
    }

    /// Whether this process was spawned within `window`. Adopted processes are
    /// never "starting" — they were already serving when we picked them up.
    pub fn within_grace(&self, window: Duration) -> bool {
        match self {
            Proc::Child(_, spawned) | Proc::Detached(_, spawned) => spawned.elapsed() < window,
            Proc::Adopted(_) => false,
        }
    }

    /// [`Self::within_grace`] with the standard [`Self::START_GRACE`] window —
    /// the health watchdog's "leave it alone, it's still starting" check.
    /// Only ever shields a child whose MASTER is alive; a dead master is
    /// reaped regardless (a crash during start must still restart).
    pub fn starting(&self) -> bool {
        self.within_grace(Self::START_GRACE)
    }

    /// Reap a spawned child after an external stop (no-op for adopted — the OS
    /// re-parents it, there is no zombie for us to collect).
    pub fn wait(&mut self) {
        if let Proc::Child(c, _) = self {
            let _ = c.wait();
        }
    }

    /// Best-effort SIGKILL for a spawned child (drop-path safety net). Adopted
    /// processes are deliberately left alone.
    pub fn kill(&mut self) {
        if let Proc::Child(c, _) = self {
            let _ = c.kill();
        }
    }

    /// Graceful-then-forceful stop for Drop paths. SIGTERM first — a php-fpm /
    /// nginx MASTER takes its workers down with it, where a bare SIGKILL
    /// orphans them still holding the listen socket (port probes then read
    /// "running" against a frozen, masterless pool) — then SIGKILL after a
    /// short grace. Adopted processes are deliberately left alone.
    ///
    /// The signalling is the platform's (`ProcessSupervisor::terminate_child`).
    /// This runs from `Drop`, which has no `Platform` in hand, so it asks for the
    /// build OS's — as `wordpress.rs` already does.
    pub fn terminate(&mut self) {
        if let Proc::Child(c, _) = self {
            crate::platform::current().supervisor().terminate_child(c);
        }
    }

    /// Whether the process itself is still alive. Port probes can't tell a
    /// healthy service from its orphaned children still squatting on the
    /// listen socket (php-fpm workers outlive a SIGKILLed master and keep
    /// accepting) — this checks the PROCESS we manage. Child: `try_wait`
    /// (also reaps a zombie); adopted: the platform's `pid_alive` (signal 0 on macOS).
    pub fn alive(&mut self) -> bool {
        match self {
            Proc::Child(c, _) => matches!(c.try_wait(), Ok(None)),
            Proc::Adopted(pid) | Proc::Detached(pid, _) => {
                crate::platform::current().supervisor().pid_alive(*pid)
            }
        }
    }
}

impl From<Child> for Proc {
    fn from(c: Child) -> Self {
        Proc::Child(c, Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A detached process is OURS: it gets the start grace, and the
    /// provenance check says we may stop it.**
    ///
    /// `Adopted` was the first shape for a `pg_ctl`-started PostgreSQL and it
    /// cost a shipped bug: an adopted handle is never "starting", `pg_ctl -w`
    /// takes seconds, so the watchdog fired mid-start, respawned, and refused
    /// its OWN server's port — the app reported PostgreSQL dead while the
    /// cluster was serving (VM, 20 Sep 2026).
    #[test]
    fn a_detached_process_is_ours_and_starting() {
        let now = Proc::Detached(4242, Instant::now());
        assert_eq!(now.id(), 4242);
        assert!(!now.is_adopted(), "we started it — provenance must say so");
        assert!(now.starting(), "the watchdog must leave a detached start alone");
        let old = Proc::Detached(4242, Instant::now() - Proc::START_GRACE - Duration::from_secs(1));
        assert!(!old.starting(), "the grace is a window, not a permanent shield");
        // Adopted keeps its meaning: a prior session's process is never starting.
        assert!(!Proc::Adopted(4242).starting());
    }

    #[test]
    fn adopted_reports_pid_and_never_waits_or_kills() {
        let mut p = Proc::Adopted(4242);
        assert_eq!(p.id(), 4242);
        p.wait(); // must not panic / touch the (foreign) pid
        p.kill();
        assert_eq!(p.id(), 4242);
    }

    #[test]
    fn child_reports_pid_and_reaps() {
        let child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        let mut p: Proc = child.into();
        assert_eq!(p.id(), pid);
        p.wait();
    }

    #[test]
    fn start_grace_covers_fresh_children_and_never_adopted() {
        let child = std::process::Command::new("true").spawn().unwrap();
        let mut p: Proc = child.into();
        // Freshly spawned: inside the standard grace, outside a zero window.
        assert!(p.starting());
        assert!(p.within_grace(Proc::START_GRACE));
        assert!(!p.within_grace(Duration::ZERO));
        p.wait();
        // Adopted survivors were already serving — no grace, ever.
        let a = Proc::Adopted(4242);
        assert!(!a.starting());
        assert!(!a.within_grace(Duration::from_secs(3600)));
    }
}

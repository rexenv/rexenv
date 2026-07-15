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
            Proc::Adopted(pid) => *pid,
        }
    }

    /// Whether this process was spawned within `window`. Adopted processes are
    /// never "starting" — they were already serving when we picked them up.
    pub fn within_grace(&self, window: Duration) -> bool {
        match self {
            Proc::Child(_, spawned) => spawned.elapsed() < window,
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
    pub fn terminate(&mut self) {
        if let Proc::Child(c, _) = self {
            let _ = std::process::Command::new("kill").arg(c.id().to_string()).status();
            for _ in 0..20 {
                if matches!(c.try_wait(), Ok(Some(_))) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// Whether the process itself is still alive. Port probes can't tell a
    /// healthy service from its orphaned children still squatting on the
    /// listen socket (php-fpm workers outlive a SIGKILLed master and keep
    /// accepting) — this checks the PROCESS we manage. Child: `try_wait`
    /// (also reaps a zombie); adopted: signal 0.
    pub fn alive(&mut self) -> bool {
        match self {
            Proc::Child(c, _) => matches!(c.try_wait(), Ok(None)),
            Proc::Adopted(pid) => std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .status()
                .map(|s| s.success())
                .unwrap_or(false),
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

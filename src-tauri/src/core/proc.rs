//! core::proc — a handle to a supervised long-running process.
//!
//! Services outlive the app by design: quitting rexenv leaves the stack
//! serving, and the next launch ADOPTS the survivors instead of restarting
//! them (`ServiceManager::adopt_startup`). A `Proc` is either a child this
//! session spawned (we own the OS handle) or a prior session's process we
//! adopted by pid.

use std::process::Child;

pub enum Proc {
    /// Spawned this session — we own the OS child handle (can `wait`/`kill`).
    Child(Child),
    /// Adopted from a prior app session: pid only. Stopped via
    /// `ProcessSupervisor::stop` like any other service; never killed
    /// implicitly (it isn't our child and outliving the app is intended).
    Adopted(u32),
}

impl Proc {
    pub fn id(&self) -> u32 {
        match self {
            Proc::Child(c) => c.id(),
            Proc::Adopted(pid) => *pid,
        }
    }

    /// Reap a spawned child after an external stop (no-op for adopted — the OS
    /// re-parents it, there is no zombie for us to collect).
    pub fn wait(&mut self) {
        if let Proc::Child(c) = self {
            let _ = c.wait();
        }
    }

    /// Best-effort SIGKILL for a spawned child (drop-path safety net). Adopted
    /// processes are deliberately left alone.
    pub fn kill(&mut self) {
        if let Proc::Child(c) = self {
            let _ = c.kill();
        }
    }

    /// Graceful-then-forceful stop for Drop paths. SIGTERM first — a php-fpm /
    /// nginx MASTER takes its workers down with it, where a bare SIGKILL
    /// orphans them still holding the listen socket (port probes then read
    /// "running" against a frozen, masterless pool) — then SIGKILL after a
    /// short grace. Adopted processes are deliberately left alone.
    pub fn terminate(&mut self) {
        if let Proc::Child(c) = self {
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
            Proc::Child(c) => matches!(c.try_wait(), Ok(None)),
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
        Proc::Child(c)
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
}

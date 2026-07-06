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

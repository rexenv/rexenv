//! core::monitor — real resource metrics via `sysinfo` (Phase 1 task 7.4).
//!
//! Backs the per-service rows AND the sidebar footer app-total (which sums the
//! same rows — one source of truth). Metrics are per process TREE: a service's
//! master plus every descendant (php-fpm/nginx workers hold most of the real
//! memory — a masters-only read undercounted by ~100MB on a small stack). A
//! `Monitor` is held in app state and refreshed per poll, so CPU% reflects
//! usage over the polling interval (a single fresh `System` would always read
//! 0% — CPU usage is a delta between refreshes).
//!
//! CPU scale: per-core percent, Activity-Monitor style — one saturated core =
//! 100, so sums across processes/cores can exceed 100. Consumers normalize by
//! [`Monitor::cpu_cores`] when they need a 0-100 machine share.

use sysinfo::{Pid, ProcessesToUpdate, System};

const MB: u64 = 1024 * 1024;

/// Per-process(-tree) metrics (one supervised service).
#[derive(Debug, Clone, Copy)]
pub struct ProcessMetrics {
    /// Per-core percent (Activity-Monitor style; can exceed 100 for a tree).
    pub cpu_percent: f32,
    pub ram_mb: u64,
}

pub struct Monitor {
    sys: System,
}

impl Monitor {
    pub fn new() -> Self {
        Self {
            sys: System::new(),
        }
    }

    /// The machine's total RAM in MB (denominator for the footer meter).
    pub fn machine_ram_total_mb(&mut self) -> u64 {
        self.sys.refresh_memory();
        self.sys.total_memory() / MB
    }

    /// Logical CPU cores — the denominator for turning a per-core CPU sum into
    /// a 0-100 machine share.
    pub fn cpu_cores(&self) -> u32 {
        std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1)
    }

    /// Refresh the process table once (a single system sweep). Call ONCE per poll,
    /// before reading per-pid metrics with [`Monitor::tree`]. Previously `process`
    /// re-scanned every system process on each call, so an N-service poll did N full
    /// sweeps (M6). CPU% is a delta between refreshes, so a single per-poll refresh
    /// also makes it span the whole interval instead of the ~0 gap between the old
    /// back-to-back sweeps.
    pub fn refresh_processes(&mut self) {
        self.sys.refresh_processes(ProcessesToUpdate::All, true);
    }

    /// PIDs of `root` plus every live descendant (breadth-first parent-pid walk
    /// over the refreshed process table).
    pub fn tree_pids(&self, root: u32) -> Vec<u32> {
        let mut out = vec![root];
        let mut frontier = vec![Pid::from_u32(root)];
        while let Some(parent) = frontier.pop() {
            for (pid, p) in self.sys.processes() {
                if p.parent() == Some(parent) {
                    out.push(pid.as_u32());
                    frontier.push(*pid);
                }
            }
        }
        out
    }

    /// Metrics for a whole process TREE — `root` (a service's master) plus all
    /// its workers/children (`None` if `root` isn't alive). Read-only: call
    /// [`Monitor::refresh_processes`] once per poll first.
    pub fn tree(&self, root: u32) -> Option<ProcessMetrics> {
        self.sys.process(Pid::from_u32(root))?;
        let mut cpu = 0.0f32;
        let mut ram = 0u64;
        for pid in self.tree_pids(root) {
            if let Some(p) = self.sys.process(Pid::from_u32(pid)) {
                cpu += p.cpu_usage();
                ram += p.memory() / MB;
            }
        }
        Some(ProcessMetrics { cpu_percent: cpu, ram_mb: ram })
    }
}

impl Default for Monitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_totals_are_plausible() {
        let mut m = Monitor::new();
        assert!(m.machine_ram_total_mb() > 0);
        assert!(m.cpu_cores() >= 1);
    }

    #[test]
    fn tree_metrics_for_self() {
        let mut m = Monitor::new();
        let pid = std::process::id();
        m.refresh_processes(); // populate the table once, then read
        let p = m.tree(pid).expect("our own process is alive");
        assert!(p.ram_mb > 0);
        assert!(m.tree(u32::MAX - 7).is_none(), "dead root reads as None");
    }

    #[test]
    fn tree_includes_spawned_children() {
        // Spawn two children; the parent-pid walk from OUR pid must find both
        // (that's what folds php-fpm/nginx workers into their service row).
        let mut kids: Vec<std::process::Child> = (0..2)
            .map(|_| std::process::Command::new("sleep").arg("10").spawn().unwrap())
            .collect();
        let mut m = Monitor::new();
        m.refresh_processes();
        let pids = m.tree_pids(std::process::id());
        for k in &kids {
            assert!(pids.contains(&k.id()), "child {} missing from tree", k.id());
        }
        for k in &mut kids {
            let _ = k.kill();
            let _ = k.wait();
        }
    }
}

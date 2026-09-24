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

use std::collections::HashSet;
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
        walk_tree(root, |parent| {
            let parent = Pid::from_u32(parent);
            self.sys
                .processes()
                .iter()
                .filter(|(_, p)| p.parent() == Some(parent))
                .map(|(pid, _)| pid.as_u32())
                .collect()
        })
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
                // A THREAD is not a process: on Linux sysinfo lists every task of a
                // process as its own entry, parented to the thread-group leader, and each
                // reports the whole process's RSS — so a tree walk summed mysqld's ~30
                // threads at ~490 MB each and the sidebar read "14.5 GB" for a stack using
                // 600 MB (Ubuntu 22.04 VM, 24 Sep 2026). macOS and Windows list no
                // threads, so the filter changes nothing there.
                if p.thread_kind().is_some() {
                    continue;
                }
                cpu += p.cpu_usage();
                ram += p.memory() / MB;
            }
        }
        Some(ProcessMetrics { cpu_percent: cpu, ram_mb: ram })
    }
}

#[cfg(test)]
mod thread_tests {
    use super::*;

    /// A process's own threads must not multiply its memory: `tree(self)` while this test
    /// holds a dozen parked threads equals the process's memory once, not thirteen times.
    /// On Linux every thread is a sysinfo entry (the "14.5 GB" sidebar); elsewhere the
    /// table lists no threads and both sides are the single reading.
    #[test]
    fn a_processs_own_threads_do_not_multiply_its_memory() {
        let parked: Vec<_> = (0..12)
            .map(|_| {
                let (ptx, prx) = std::sync::mpsc::channel::<()>();
                let h = std::thread::spawn(move || {
                    let _ = prx.recv();
                });
                (h, ptx)
            })
            .collect();
        let mut m = Monitor::new();
        m.refresh_processes();
        let me = std::process::id();
        let tree = m.tree(me).expect("this process is alive");
        let own = m.sys.process(Pid::from_u32(me)).map(|p| p.memory() / MB).unwrap_or(0);
        // A thread-summed tree would read ≥ 13× the process (twelve parked threads plus the
        // main one); the tree may legitimately exceed the process alone by the CHILDREN other
        // tests in this binary are running at the same moment (a few MB of `sh`), so the bound
        // is a multiple, not an equality — measured 24 Sep 2026: 55 MB vs 53 MB on the Mac.
        assert!(tree.ram_mb < own * 4 + 8, "tree {} MB vs the process's own {} MB — threads were summed", tree.ram_mb, own);
        for (h, ptx) in parked {
            let _ = ptx.send(());
            let _ = h.join();
        }
    }
}

/// Breadth-first tree walk from `root`, collecting `root` plus every descendant.
/// `children_of` yields a pid's direct children. A `visited` set bounds each pid
/// to one visit, so a parent-pid **cycle** (possible under PID recycling, where a
/// "child" is reported as an ancestor's parent) terminates instead of looping
/// forever and can't double-count (B27). On a real acyclic tree every process has
/// exactly one parent, so the guard never rejects anything — output is unchanged.
fn walk_tree(root: u32, mut children_of: impl FnMut(u32) -> Vec<u32>) -> Vec<u32> {
    let mut visited = HashSet::from([root]);
    let mut out = vec![root];
    let mut frontier = vec![root];
    while let Some(parent) = frontier.pop() {
        for child in children_of(parent) {
            if visited.insert(child) {
                out.push(child);
                frontier.push(child);
            }
        }
    }
    out
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
            .map(|_| crate::test_support::live_child())
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

    #[test]
    fn walk_tree_bounds_a_parent_pid_cycle() {
        // A parent-pid cycle (1→2, 2→1 under PID recycling) must TERMINATE and
        // visit each pid once — without the visited-set this loops forever.
        let children = |p: u32| match p {
            1 => vec![2],
            2 => vec![1, 3], // 2 points back at the root (the cycle) + a leaf
            _ => vec![],
        };
        let mut got = walk_tree(1, children);
        got.sort();
        assert_eq!(got, vec![1, 2, 3]);
    }

    #[test]
    fn walk_tree_covers_an_acyclic_tree_once() {
        // Normal tree: 1 → {2,3}, 2 → {4}. Every pid exactly once, no change.
        let children = |p: u32| match p {
            1 => vec![2, 3],
            2 => vec![4],
            _ => vec![],
        };
        let mut got = walk_tree(1, children);
        got.sort();
        assert_eq!(got, vec![1, 2, 3, 4]);
    }
}

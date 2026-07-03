//! core::monitor — real resource metrics via `sysinfo` (Phase 1 task 7.4).
//!
//! Backs the sidebar status footer (system CPU/RAM totals) and per-service rows
//! (RAM/CPU by supervised PID). A `Monitor` is held in app state and refreshed
//! per poll, so CPU% reflects usage over the polling interval (a single fresh
//! `System` would always read 0% — CPU usage is a delta between refreshes).

use sysinfo::{Pid, ProcessesToUpdate, System};

const MB: u64 = 1024 * 1024;

/// System-wide totals.
#[derive(Debug, Clone, Copy)]
pub struct SystemMetrics {
    pub cpu_percent: f32,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
}

/// Per-process metrics (one supervised service).
#[derive(Debug, Clone, Copy)]
pub struct ProcessMetrics {
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

    /// Sample system-wide CPU% (since the previous sample) and memory.
    pub fn sample(&mut self) -> SystemMetrics {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        SystemMetrics {
            cpu_percent: self.sys.global_cpu_usage(),
            ram_used_mb: self.sys.used_memory() / MB,
            ram_total_mb: self.sys.total_memory() / MB,
        }
    }

    /// Refresh the process table once (a single system sweep). Call ONCE per poll,
    /// before reading per-pid metrics with [`Monitor::process`]. Previously `process`
    /// re-scanned every system process on each call, so an N-service poll did N full
    /// sweeps (M6). CPU% is a delta between refreshes, so a single per-poll refresh
    /// also makes it span the whole interval instead of the ~0 gap between the old
    /// back-to-back sweeps.
    pub fn refresh_processes(&mut self) {
        self.sys.refresh_processes(ProcessesToUpdate::All, true);
    }

    /// Metrics for a single supervised process by PID (`None` if it's not alive).
    /// Read-only: call [`Monitor::refresh_processes`] once per poll first.
    pub fn process(&self, pid: u32) -> Option<ProcessMetrics> {
        self.sys.process(Pid::from_u32(pid)).map(|p| ProcessMetrics {
            cpu_percent: p.cpu_usage(),
            ram_mb: p.memory() / MB,
        })
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
    fn sample_returns_plausible_totals() {
        let mut m = Monitor::new();
        let s = m.sample();
        // Some RAM is always used; total > used.
        assert!(s.ram_total_mb > 0);
        assert!(s.ram_used_mb > 0);
        assert!(s.ram_total_mb >= s.ram_used_mb);
        assert!(s.cpu_percent >= 0.0);
    }

    #[test]
    fn process_metrics_for_self() {
        let mut m = Monitor::new();
        let pid = std::process::id();
        m.refresh_processes(); // populate the table once, then read
        let p = m.process(pid).expect("our own process is alive");
        assert!(p.ram_mb > 0);
    }
}

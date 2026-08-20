//! Verify the reworked resource monitoring against the LIVE stack (read-only —
//! needs `Start all` done in the app). For every rexenv-owned process found by
//! the app-data cmdline marker:
//!   - group into trees (masters + workers) exactly like the Services rows do,
//!   - read each tree via Monitor::tree (+ the ps fallback for the ROOT edge),
//!   - cross-check the summed RAM against raw `ps` RSS ground truth (±15%).
//!
//! Prints a per-tree table you can eyeball against Activity Monitor.

use rexenv_lib::core::monitor::Monitor;
use rexenv_lib::platform;

/// All rexenv-owned (marker in cmdline) pids + the title-rewritten workers
/// (php-fpm/nginx) whose PARENT is one of them.
fn ps_rows() -> Vec<(u32, u32, u64, String)> {
    // pid, ppid, rss_kb, command
    let out = std::process::Command::new("ps")
        .args(["-axo", "pid=,ppid=,rss=,command="])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let rest = l.split_whitespace().collect::<Vec<_>>();
            let pid = rest.first()?.parse().ok()?;
            let ppid = rest.get(1)?.parse().ok()?;
            let rss = rest.get(2)?.parse().ok()?;
            let cmd = rest.get(3..)?.join(" ");
            Some((pid, ppid, rss, cmd))
        })
        .collect()
}

fn main() {
    let plat = platform::current();
    let marker = plat.paths().app_data_dir().unwrap().display().to_string();
    let rows = ps_rows();
    // Roots: marker in cmdline, parent NOT marker'd (masters/single processes).
    let marked: Vec<&(u32, u32, u64, String)> =
        rows.iter().filter(|(_, _, _, c)| c.contains(&marker)).collect();
    assert!(!marked.is_empty(), "no rexenv processes found — Start all first");
    let is_marked = |pid: u32| marked.iter().any(|(p, ..)| *p == pid);
    let roots: Vec<&&(u32, u32, u64, String)> =
        marked.iter().filter(|(_, ppid, _, _)| !is_marked(*ppid)).collect();

    // Ground truth per root: RSS of the root + every descendant (ps ppid walk).
    let ps_tree_rss_mb = |root: u32| -> u64 {
        let mut total = 0u64;
        let mut frontier = vec![root];
        while let Some(parent) = frontier.pop() {
            for (pid, ppid, rss, _) in &rows {
                if *ppid == parent {
                    frontier.push(*pid);
                }
                if *pid == parent {
                    total += rss;
                }
            }
        }
        total / 1024
    };

    let mut mon = Monitor::new();
    mon.refresh_processes();
    std::thread::sleep(std::time::Duration::from_millis(600));
    mon.refresh_processes();

    println!("{:<8} {:<28} {:>10} {:>10} {:>8}", "PID", "SERVICE", "tree MB", "ps MB", "src");
    let mut app_total_ours = 0u64;
    let mut app_total_ps = 0u64;
    for (pid, _, _, cmd) in roots.iter().map(|r| **r) {
        let name = cmd.split('/').next_back().unwrap_or("?").split_whitespace().next().unwrap_or("?");
        let truth = ps_tree_rss_mb(*pid);
        let (ours, src) = match mon.tree(*pid) {
            Some(t) if t.ram_mb > 0 => (t.ram_mb, "sysinfo"),
            _ => (
                plat.supervisor().resource_usage(*pid).map(|(_, r)| r).unwrap_or(0),
                "ps-fb",
            ),
        };
        println!("{:<8} {:<28} {:>10} {:>10} {:>8}", pid, name, ours, truth, src);
        app_total_ours += ours;
        app_total_ps += truth;
        // Per-tree tolerance (RSS vs sysinfo memory differ slightly; the root
        // edge fallback measures only the single caddy process — no children).
        let hi = truth.max(1) as f64;
        assert!(
            (ours as f64) > hi * 0.5,
            "{name} (pid {pid}): ours={ours}MB vs ps={truth}MB — undercounted"
        );
    }
    println!("\napp-total: ours={app_total_ours} MB vs ps ground truth={app_total_ps} MB");
    let ratio = app_total_ours as f64 / app_total_ps.max(1) as f64;
    assert!((0.85..=1.15).contains(&ratio), "app-total off by {:.0}%", (ratio - 1.0) * 100.0);
    println!("TOTALS MATCH (±15%) — workers counted, root edge measured");
}

//! Manual check for core::monitor (task 7.4): machine totals + a process-TREE
//! read of our own pid (the same API the Services rows + footer app-total use).

use rexenv_lib::core::monitor::Monitor;
use std::time::Duration;

fn main() {
    let mut m = Monitor::new();
    println!(
        "machine: ram_total={} MB cores={}",
        m.machine_ram_total_mb(),
        m.cpu_cores()
    );
    m.refresh_processes(); // prime CPU (first read has no delta)
    std::thread::sleep(Duration::from_millis(500));
    m.refresh_processes();
    match m.tree(std::process::id()) {
        Some(p) => println!("self tree: cpu={:.1}% ram={} MB", p.cpu_percent, p.ram_mb),
        None => println!("self tree: not visible?!"),
    }
}

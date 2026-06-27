//! Manual check for the resource monitor (task 7.4). Prints real system CPU/RAM
//! and this process's metrics. `cargo run --example metrics_check`.

use rexenv_lib::core::monitor::Monitor;
use std::time::Duration;

fn main() {
    let mut m = Monitor::new();
    let _ = m.sample(); // prime CPU (first read has no delta)
    std::thread::sleep(Duration::from_millis(500));
    let s = m.sample();
    println!(
        "system: CPU {:.1}%  RAM {} / {} MB",
        s.cpu_percent, s.ram_used_mb, s.ram_total_mb
    );
    if let Some(p) = m.process(std::process::id()) {
        println!("self:   CPU {:.1}%  RAM {} MB", p.cpu_percent, p.ram_mb);
    }
}

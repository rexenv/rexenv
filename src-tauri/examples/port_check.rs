//! Manual check for port-conflict detection (task 10.1).
//! `cargo run --example port_check` — probes rexenv's required ports and reports
//! which are free vs already in use (e.g. :443 held by another local stack).

use rexenv_lib::core::ports;

fn main() {
    let platform = rexenv_lib::platform::current();
    println!("{:<16} {:<8} status", "service", "port");
    for s in ports::check(&*platform, &ports::default_ports()) {
        println!(
            "{:<16} {:<8} {}",
            s.service,
            format!("{}/{}", s.port, s.proto.as_str()),
            if s.free { "free" } else { "IN USE" }
        );
    }
    let c = ports::conflicts(&*platform, &ports::default_ports());
    println!("\nconflicts: {}", c.len());
}

//! Live check for the WordPress debug-log Logs-tab feature (no app needed):
//! `cargo run --example wp_debug_log_check -- <docroot>`
//! Prints the resolved status, then tails the log.

use rexenv_lib::core::logs;
use std::path::PathBuf;

fn main() {
    let docroot = PathBuf::from(
        std::env::args().nth(1).expect("usage: wp_debug_log_check <site docroot>"),
    );
    let status = logs::wp_debug_log_status(&docroot);
    println!("status: {status:#?}");
    let lines = logs::wp_debug_log_tail(&docroot, 10).expect("tail failed");
    println!("--- last {} lines ---", lines.len());
    for l in lines {
        println!("{l}");
    }
}

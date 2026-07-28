//! Live end-to-end check for the WordPress → Tools Debugging toggle:
//! enable full debugging, trigger a PHP warning, confirm it lands in
//! `wp-content/debug.log` and in the Logs-tab tail, then disable cleanly.
//!
//! `cargo run --example wp_debug_toggle_check -- <docroot> <php> <wp-cli.phar>`

use rexenv_lib::core::{logs, wordpress};
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let docroot = PathBuf::from(args.next().expect("docroot"));
    let php = PathBuf::from(args.next().expect("php binary"));
    let wp = PathBuf::from(args.next().expect("wp-cli.phar"));

    println!("== before: {:?}", logs::wp_debug_log_status(&docroot, "wp-content"));

    println!("== enable debugging");
    wordpress::wp_debug_set(&php, &wp, &docroot, true).expect("enable failed");
    let s = logs::wp_debug_log_status(&docroot, "wp-content");
    println!("   status: {s:?}");
    assert!(s.debug && s.log_enabled, "toggle on must enable WP_DEBUG + WP_DEBUG_LOG");

    println!("== trigger a PHP warning through WordPress");
    wordpress::wp_run(
        &php,
        &wp,
        &docroot,
        &["eval", "trigger_error('rexenv debug-log verify', E_USER_WARNING);"],
    )
    .expect("wp eval failed");

    let s = logs::wp_debug_log_status(&docroot, "wp-content");
    println!("   status after warning: {s:?}");
    assert!(s.exists && s.size_bytes > 0, "debug.log must be auto-created on first entry");
    let tail = logs::wp_debug_log_tail(&docroot, "wp-content", 5).expect("tail failed");
    println!("   tail: {tail:#?}");
    assert!(
        tail.iter().any(|l| l.contains("rexenv debug-log verify")),
        "the warning must appear in the Logs-tab tail"
    );

    println!("== disable debugging");
    wordpress::wp_debug_set(&php, &wp, &docroot, false).expect("disable failed");
    let s = logs::wp_debug_log_status(&docroot, "wp-content");
    println!("   status: {s:?}");
    assert!(!s.debug && !s.log_enabled, "toggle off must disable both constants");

    println!("OK — full enable → log → view → disable cycle verified");
}

//! Reverse the batched system setup (task 3.4): remove the `.test` resolver file
//! and untrust the local CA. Run: `cargo run --example system_teardown`.

use rexenv_lib::core::setup;
use rexenv_lib::platform;

fn main() {
    let plat = platform::current();
    match setup::run_system_teardown(&*plat) {
        Ok(()) => println!("system teardown OK (resolver removed, CA untrusted)"),
        Err(e) => eprintln!("system teardown failed: {e}"),
    }
}

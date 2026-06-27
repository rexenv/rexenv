//! System setup (task 3.4) — installs the `.test` resolver file (admin prompt)
//! and trusts the local CA in the login keychain (native trust dialog).
//! Run: `cargo run --example system_setup`.
//! Reverse with: `cargo run --example system_teardown`.

use rexenv_lib::core::setup;
use rexenv_lib::platform;

fn main() {
    let plat = platform::current();
    match setup::run_system_setup(&*plat) {
        Ok(ca) => {
            println!("system setup OK");
            println!("CA: {}", ca.cert_path.display());
        }
        Err(e) => eprintln!("system setup failed: {e}"),
    }
}

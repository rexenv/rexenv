//! Manual live check for the macOS PrivilegeManager (task 2.3).
//! Run: `cargo run --example priv_check` — this pops ONE macOS auth dialog,
//! runs a no-op privileged command as root, and prints the created file's owner
//! (expected: `root`). Cannot be automated (the prompt needs a human password).

use rexenv_lib::platform;

fn main() {
    let plat = platform::current();
    let marker = "/tmp/rexenv_priv_check";
    // Run as root: create a file (owned by root) and report its owner.
    let script = format!("rm -f {marker}; touch {marker}; stat -f '%Su' {marker}");

    match plat.privileges().run_privileged(&script) {
        Ok(owner) => {
            println!("privileged op succeeded; owner of {marker} = {owner}");
            if owner.trim() == "root" {
                println!("OK: command ran with administrator privileges.");
            } else {
                println!("WARN: expected owner 'root', got '{}'.", owner.trim());
            }
        }
        Err(e) => eprintln!("privileged op failed: {e}"),
    }
}

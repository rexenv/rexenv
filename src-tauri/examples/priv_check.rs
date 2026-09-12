//! Manual live check for the macOS PrivilegeManager (task 2.3; the branded
//! dialog since 12 Sep 2026).
//! Run: `scripts/live-checks.sh system priv_check` — this pops ONE macOS auth
//! dialog, runs a no-op privileged command as root, and prints the created file's
//! owner (expected: `root`). Cannot be automated (the prompt needs a human password).
//!
//! **Look at the dialog before answering it.** It must read **rexenv** in bold with
//! rexenv's logo on the lock — never "osascript wants to make changes." over a plain
//! lock. That name is the one thing no lib test can see: SecurityAgent draws it from
//! the asking bundle (`platform/macos/prompt_applet.rs`), and "osascript" means the
//! applet could not be built or launched and the unbranded fallback ran (the app log
//! says why: "branded password prompt unavailable").
//!
//! Cancel once too: the error must read as a cancelled permission, not a raw `-128`.

use rexenv_lib::platform;

fn main() {
    let plat = platform::current();
    let marker = "/tmp/rexenv_priv_check";
    // Run as root: create a file (owned by root) and report its owner.
    let script = format!("rm -f {marker}; touch {marker}; stat -f '%Su' {marker}");

    println!("A password dialog is opening — check it reads \"rexenv\" with rexenv's logo.");
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

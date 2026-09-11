//! Release §3.2 check: run the REAL system teardown and verify the machine is clean.
//!
//! Invokes the exact app code path (`core::setup::run_system_teardown`) → removes
//! `/etc/resolver/test` (osascript admin prompt — enter your macOS password when it
//! pops up), flushes the DNS cache, and untrusts the local CA. Then asserts:
//!   - the resolver file is gone;
//!   - `.test` no longer resolves (`dscacheutil -q host foo.test` returns no IP);
//!   - the rexenv CA is no longer in the login keychain.
//!
//! This MUTATES your machine (reversible: re-run first-run setup to reinstall).
//! Run: `cargo run --example teardown_check`

use rexenv_lib::core::setup;
use rexenv_lib::platform;
use rexenv_lib::state::db;
use std::process::Command;

fn resolver_present() -> bool {
    std::path::Path::new("/etc/resolver/test").exists()
}

fn test_domain_resolves() -> bool {
    // dscacheutil prints an `ip_address:` line only when the name resolves.
    let out = Command::new("dscacheutil")
        .args(["-q", "host", "-a", "name", "foo.test"])
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).contains("ip_address:"),
        Err(_) => false,
    }
}

fn ca_trusted() -> bool {
    // TRUST (not mere keychain presence): `remove-trusted-cert` drops the trust
    // settings but can leave the cert in the keychain, so we probe the user's
    // trust-settings domain — the CA appears there only while it's trusted.
    Command::new("security")
        .arg("dump-trust-settings")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase().contains("rexenv"))
        .unwrap_or(false)
}

fn main() {
    let plat = platform::current();

    println!("── BEFORE ──");
    let before_resolver = resolver_present();
    let before_resolves = test_domain_resolves();
    let before_ca = ca_trusted();
    println!("/etc/resolver/test present : {before_resolver}");
    println!("foo.test resolves          : {before_resolves}");
    println!("rexenv CA trusted (login)  : {before_ca}");

    // Re-runnable: only run the (privileged) teardown if there's something to undo.
    if !before_resolver && !before_resolves && !before_ca {
        println!("\n(Already torn down — verifying the clean state is stable.)");
    } else {
        println!("\n── Running run_system_teardown (enter your macOS password if prompted) ──");
        let conn = db::open_for_platform(plat.paths()).expect("app database");
        match setup::run_system_teardown(&std::sync::Mutex::new(conn), &*plat) {
            Ok(r) => println!(
                "teardown returned Ok — removed {:?}, restored {:?}, left alone {:?}",
                r.removed, r.restored, r.left_alone
            ),
            Err(e) => {
                eprintln!("teardown failed: {e}");
                std::process::exit(1);
            }
        }
    }

    // Give mDNSResponder a moment after the flush.
    std::thread::sleep(std::time::Duration::from_millis(800));

    println!("\n── AFTER ──");
    let after_resolver = resolver_present();
    let after_resolves = test_domain_resolves();
    let after_ca = ca_trusted();
    println!("/etc/resolver/test present : {after_resolver}");
    println!("foo.test resolves          : {after_resolves}");
    println!("rexenv CA trusted (login)  : {after_ca}");

    assert!(!after_resolver, "resolver file still present after teardown");
    assert!(!after_resolves, "foo.test still resolves after teardown");
    assert!(!after_ca, "rexenv CA still trusted after teardown");

    println!("\nALL GOOD — teardown removed the .test resolver, .test no longer resolves, and the local CA is untrusted.");
    println!("(To resume local dev later, run first-run setup again to reinstall the resolver + re-trust the CA.)");
}

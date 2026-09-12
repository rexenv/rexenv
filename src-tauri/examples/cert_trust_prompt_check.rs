//! Live check for the macOS CA-trust dialog (`platform/macos/keychain_trust.rs`).
//! Run: `scripts/live-checks.sh system cert_trust_prompt_check`.
//!
//! **System tier.** It adds a THROWAWAY CA — made by the production generator, so
//! its name reads "rexenv Local CA" exactly like the real one — to the real login
//! keychain, and raises three dialogs. Answer them in this order:
//!
//! 1. Trust → **Cancel**. Must read as a cancelled permission, never a status code.
//! 2. Trust → **approve** (your login password). The CA must then be trusted.
//! 3. Untrust → **approve**. The CA must then be untrusted.
//!
//! Every call runs on a spawned thread, as in the app (`core::prompt::while_prompting`
//! hands the wait off the runtime worker): the measurement behind the module
//! called the API from a main thread only.
//!
//! The dialog's title here is this example's executable name over a generic icon —
//! it is not a bundle. "rexenv" with the logo comes from `rexenv.app`, which is the
//! packaged app's half (SMOKE-TEST).
//!
//! Cleanup is fixture-owned: the throwaway certificate leaves the login keychain by
//! ITS OWN SHA-1, never by name — the user's real CA has the same name. If the CA is
//! still trusted at the end (step 3 cancelled or never reached), nothing is guessed:
//! the exact commands to remove it are printed and its file is kept for them.

use rexenv_lib::platform;
use std::path::PathBuf;
use std::process::Command;

fn login_keychain() -> String {
    format!("{}/Library/Keychains/login.keychain-db", std::env::var("HOME").expect("HOME"))
}

struct Fixture {
    dir: PathBuf,
    pem: PathBuf,
    sha1: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if platform::current().cert_trust().is_trusted(&self.pem) {
            eprintln!(
                "LEFT BEHIND: the throwaway CA is still trusted. Remove it with:\n  \
                 security remove-trusted-cert '{}'\n  \
                 security delete-certificate -Z {} '{}'\n  rm -r '{}'",
                self.pem.display(),
                self.sha1,
                login_keychain(),
                self.dir.display()
            );
            return;
        }
        let deleted = Command::new("security")
            .args(["delete-certificate", "-Z", &self.sha1, &login_keychain()])
            .output()
            .is_ok_and(|o| o.status.success());
        println!(
            "cleanup: throwaway CA {} {}",
            &self.sha1[..8],
            if deleted { "removed from the login keychain" } else { "was not in the login keychain" }
        );
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn on_a_thread<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::spawn(f).join().expect("the trust thread panicked")
}

fn main() {
    let (cert_pem, _key) = rexenv_lib::core::ssl::generate_ca().expect("generate a throwaway CA");
    let dir = std::env::temp_dir().join(format!("rexenv-cert-trust-check-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("fixture dir");
    let pem = dir.join("throwaway-ca.pem");
    std::fs::write(&pem, &cert_pem).expect("write the throwaway CA");
    let fingerprint = Command::new("openssl")
        .args(["x509", "-noout", "-fingerprint", "-sha1", "-in"])
        .arg(&pem)
        .output()
        .expect("openssl");
    let sha1: String = String::from_utf8_lossy(&fingerprint.stdout)
        .split('=')
        .nth(1)
        .expect("a SHA-1 fingerprint")
        .chars()
        .filter(char::is_ascii_hexdigit)
        .collect::<String>()
        .to_ascii_uppercase();
    assert_eq!(sha1.len(), 40, "SHA-1 fingerprint of the throwaway CA");
    let fx = Fixture { dir, pem: pem.clone(), sha1 };
    let mut failures: Vec<String> = Vec::new();

    println!("1/3 A keychain trust dialog is opening — press CANCEL.");
    let p = pem.clone();
    match on_a_thread(move || platform::current().cert_trust().trust_ca(&p)) {
        Err(e) if e.to_string().contains("cancelled") && !e.to_string().contains("60006") => {
            println!("  OK: {e}")
        }
        Err(e) => failures.push(format!("step 1: a cancel must read as cancelled, got: {e}")),
        Ok(()) => failures.push("step 1: the trust succeeded — the dialog was approved, not cancelled".into()),
    }

    println!("2/3 The trust dialog again — APPROVE it.");
    let p = pem.clone();
    let trusted = match on_a_thread(move || platform::current().cert_trust().trust_ca(&p)) {
        Ok(()) if platform::current().cert_trust().is_trusted(&pem) => {
            println!("  OK: trusted");
            true
        }
        Ok(()) => {
            failures.push("step 2: trust returned Ok but `security verify-cert` does not trust the CA".into());
            false
        }
        Err(e) => {
            failures.push(format!("step 2: trust failed: {e}"));
            false
        }
    };

    if trusted {
        println!("3/3 An untrust dialog — APPROVE it.");
        let p = pem.clone();
        match on_a_thread(move || platform::current().cert_trust().untrust_ca(&p)) {
            Ok(()) if !platform::current().cert_trust().is_trusted(&pem) => println!("  OK: untrusted"),
            Ok(()) => failures.push("step 3: untrust returned Ok but the CA is still trusted".into()),
            Err(e) => failures.push(format!("step 3: untrust failed: {e}")),
        }
    } else {
        println!("3/3 skipped — step 2 did not leave the CA trusted.");
    }

    drop(fx);
    if failures.is_empty() {
        println!("cert_trust_prompt_check: PASS");
    } else {
        for f in &failures {
            eprintln!("FAIL {f}");
        }
        std::process::exit(1);
    }
}

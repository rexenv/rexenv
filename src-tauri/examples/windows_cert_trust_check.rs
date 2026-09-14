//! W5, ledger #613: rexenv's local CA in this user's Root certificate store on Windows —
//! `CertTrustManager::{is_trusted, trust_ca, untrust_ca}`.
//!
//! ```text
//! scripts/probes/windows-example.sh <host> windows_cert_trust_check                       # read only
//! REXENV_CERT_TRUST_WRITE=1 scripts/probes/windows-example.sh <host> windows_cert_trust_check
//! ```
//!
//! **Read only by default:** a fixture CA (created under `%TEMP%\rexenv cert trust check`, a SPACE in
//! it, removed at the end) is NOT trusted — `is_trusted` answers false without a prompt; the fixture's
//! KEY file is refused by `trust_ca` before any store call.
//!
//! **With `REXENV_CERT_TRUST_WRITE=1` it changes this user's Root store** — run only with the
//! machine owner's go: `trust_ca` (Windows asks "install this certificate?"; someone must answer at
//! the desktop), then `is_trusted`, then `untrust_ca` (Windows asks again), then `is_trusted` false.
//! Each call runs on its own thread with a deadline and its elapsed time and outcome are printed, so
//! a prompt nobody can see (an SSH session) is measured rather than hung on. If the CA is still
//! trusted at the end, the thumbprint and the command to remove it are printed.
//!
//! `demo` tier: Windows-only; on macOS it prints a skip line.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_cert_trust_check: skipped — a Windows check (ledger #613, W5)");
}

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    windows::main()
}

#[cfg(target_os = "windows")]
mod windows {
    use super::common::Check;
    use rexenv_lib::core::ssl;
    use rexenv_lib::platform::traits::Platform;
    use std::path::{Path, PathBuf};
    use std::process::ExitCode;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    const PROMPT_DEADLINE: Duration = Duration::from_secs(180);

    pub fn main() -> ExitCode {
        let mut check = Check::new("windows_cert_trust_check");
        let fixture = std::env::temp_dir().join("rexenv cert trust check");
        let _ = std::fs::remove_dir_all(&fixture);
        std::fs::create_dir_all(&fixture).expect("fixture dir");
        let plat = rexenv_lib::platform::current();
        let ca = ssl::load_or_create_at(&fixture.join("ca.pem"), &fixture.join("ca.key"), None);
        check.is("a fixture CA is created", ca.is_ok(), &format!("{:?}", ca.as_ref().err()));
        if let Ok(ca) = ca {
            run(&mut check, &*plat, &ca.cert_path, &fixture.join("ca.key"));
        }
        if let Err(e) = std::fs::remove_dir_all(&fixture) {
            println!("  · fixture not fully removed: {e}");
        }
        check.verdict()
    }

    /// Run a store call that may wait on a prompt, with a deadline. `None` when it did not return.
    fn timed<T: Send + 'static>(label: &str, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
        let (tx, rx) = mpsc::channel();
        let t = Instant::now();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        let out = rx.recv_timeout(PROMPT_DEADLINE).ok();
        println!(
            "  · {label}: {} after {:.1} s",
            if out.is_some() { "returned" } else { "DID NOT RETURN" },
            t.elapsed().as_secs_f64()
        );
        out
    }

    fn run(check: &mut Check, plat: &dyn Platform, cert: &Path, key: &Path) {
        // ── Read only. ──
        let trust = plat.cert_trust();
        check.is("a fresh fixture CA is not trusted (no prompt)", !trust.is_trusted(cert), "is_trusted answered true");
        let refused = trust.trust_ca(key);
        check.is(
            "trust_ca refuses a key file before any store call",
            refused.as_ref().is_err_and(|e| e.to_string().contains("not a certificate")),
            &format!("{refused:?}"),
        );
        check.is("untrusting a CA the store does not hold is Ok with no prompt", trust.untrust_ca(cert).is_ok(), "");

        if std::env::var("REXENV_CERT_TRUST_WRITE").as_deref() != Ok("1") {
            println!("  · write phase skipped (REXENV_CERT_TRUST_WRITE=1 runs it — changes this user's Root store)");
            return;
        }

        // ── Write: trust, look, untrust, look. ──
        let path: PathBuf = cert.to_path_buf();
        let added = timed("trust_ca", move || rexenv_lib::platform::current().cert_trust().trust_ca(&path).map_err(|e| e.to_string()));
        println!("  · trust_ca outcome: {added:?}");
        let trusted = trust.is_trusted(cert);
        println!("  · is_trusted after trust_ca: {trusted}");
        check.is("trust_ca returned Ok and the CA is then trusted", matches!(added, Some(Ok(()))) && trusted, &format!("{added:?}, trusted {trusted}"));
        if trusted {
            let path: PathBuf = cert.to_path_buf();
            let again = timed("trust_ca again", move || rexenv_lib::platform::current().cert_trust().trust_ca(&path).map_err(|e| e.to_string()));
            check.is("trusting an already-trusted CA is Ok (no prompt expected)", matches!(again, Some(Ok(()))), &format!("{again:?}"));
            let path: PathBuf = cert.to_path_buf();
            let removed = timed("untrust_ca", move || rexenv_lib::platform::current().cert_trust().untrust_ca(&path).map_err(|e| e.to_string()));
            println!("  · untrust_ca outcome: {removed:?}");
            let still = trust.is_trusted(cert);
            check.is("untrust_ca returned Ok and the CA is no longer trusted", matches!(removed, Some(Ok(()))) && !still, &format!("{removed:?}, trusted {still}"));
        }
        if trust.is_trusted(cert) {
            let serial = std::fs::read(cert)
                .ok()
                .and_then(|pem| x509_parser::pem::parse_x509_pem(&pem).ok().map(|(_, p)| p.contents))
                .and_then(|der| x509_parser::parse_x509_certificate(&der).ok().map(|(_, c)| c.raw_serial_as_string().replace(':', "")));
            println!("  · !! the fixture CA is STILL in this user's Root store — remove it: certutil -user -delstore Root {}", serial.unwrap_or_default());
        }
    }
}

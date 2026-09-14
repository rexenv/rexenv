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
//! **With `REXENV_CERT_TRUST_ANSWERS=no-yes` as well**, the person at the desktop answers four prompts
//! in a fixed order — install No, install Yes, delete No, delete Yes — and the check holds each answer
//! to its outcome: a No on either reads as the cancel and leaves the store as it was.
//!
//! While any store call waits, a watcher thread prints the title and text of every visible window
//! this process owns — Windows' prompt, read without touching it.
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

    /// Print the title and child-control text of every visible top-level window this process owns, once
    /// each, until `stop` — the certificate prompt, recorded without clicking anything.
    fn watch_own_windows(stop: std::sync::Arc<std::sync::atomic::AtomicBool>) -> std::thread::JoinHandle<()> {
        use windows_sys::Win32::Foundation::{HWND, LPARAM};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            EnumChildWindows, EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
        };
        unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> windows_sys::core::BOOL {
            // SAFETY: `lparam` is the `Vec<isize>` passed by the enumerating call below, alive for it.
            unsafe { (*(lparam as *mut Vec<isize>)).push(hwnd as isize) };
            1
        }
        fn text(hwnd: HWND) -> String {
            let mut buf = [0u16; 512];
            // SAFETY: the buffer's length is passed; a stale handle yields 0.
            let n = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
            String::from_utf16_lossy(&buf[..n.max(0) as usize])
        }
        std::thread::spawn(move || {
            let me = std::process::id();
            let mut seen = std::collections::HashSet::new();
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let mut tops: Vec<isize> = Vec::new();
                // SAFETY: `collect` only pushes into `tops`, which outlives the call.
                unsafe { EnumWindows(Some(collect), &mut tops as *mut Vec<isize> as LPARAM) };
                for raw in tops {
                    let hwnd = raw as HWND;
                    let mut pid = 0u32;
                    // SAFETY: plain queries on a window handle; a stale one answers 0.
                    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
                    if pid != me || unsafe { IsWindowVisible(hwnd) } == 0 || !seen.insert(raw) {
                        continue;
                    }
                    let mut kids: Vec<isize> = Vec::new();
                    // SAFETY: as above.
                    unsafe { EnumChildWindows(hwnd, Some(collect), &mut kids as *mut Vec<isize> as LPARAM) };
                    let texts: Vec<String> = kids.into_iter().map(|k| text(k as HWND)).filter(|s| !s.trim().is_empty()).collect();
                    println!("  · window on screen: title {:?}, text {:?}", text(hwnd), texts);
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        })
    }

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
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let watcher = watch_own_windows(stop.clone());
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        let out = rx.recv_timeout(PROMPT_DEADLINE).ok();
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = watcher.join();
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

        if std::env::var("REXENV_CERT_TRUST_ANSWERS").as_deref() == Ok("no-yes") {
            answered(check, plat, cert);
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
        leftover(plat, cert);
    }

    fn call(label: &str, cert: &Path, untrust: bool) -> Option<Result<(), String>> {
        let path: PathBuf = cert.to_path_buf();
        println!("  · {label}: waiting for the answer at the desktop");
        timed(label, move || {
            let p = rexenv_lib::platform::current();
            if untrust { p.cert_trust().untrust_ca(&path) } else { p.cert_trust().trust_ca(&path) }.map_err(|e| e.to_string())
        })
    }

    /// Four prompts in a fixed order: install No, install Yes, delete No, delete Yes.
    fn answered(check: &mut Check, plat: &dyn Platform, cert: &Path) {
        let trust = plat.cert_trust();
        let cancelled = |r: &Option<Result<(), String>>| matches!(r, Some(Err(m)) if m.contains("cancelled"));
        let no_add = call("trust_ca — answer No", cert, false);
        println!("  · outcome: {no_add:?}");
        check.is("install answered No: trust_ca reads as the cancel and the CA is not trusted", cancelled(&no_add) && !trust.is_trusted(cert), &format!("{no_add:?}"));
        let yes_add = call("trust_ca — answer Yes", cert, false);
        println!("  · outcome: {yes_add:?}");
        check.is("install answered Yes: trust_ca Ok and the CA is trusted", matches!(yes_add, Some(Ok(()))) && trust.is_trusted(cert), &format!("{yes_add:?}"));
        if trust.is_trusted(cert) {
            let no_del = call("untrust_ca — answer No", cert, true);
            println!("  · outcome: {no_del:?}");
            check.is("delete answered No: untrust_ca reads as the cancel and the CA is still trusted", cancelled(&no_del) && trust.is_trusted(cert), &format!("{no_del:?}"));
            let yes_del = call("untrust_ca — answer Yes", cert, true);
            println!("  · outcome: {yes_del:?}");
            check.is("delete answered Yes: untrust_ca Ok and the CA is no longer trusted", matches!(yes_del, Some(Ok(()))) && !trust.is_trusted(cert), &format!("{yes_del:?}"));
        }
        leftover(plat, cert);
    }

    fn leftover(plat: &dyn Platform, cert: &Path) {
        let trust = plat.cert_trust();
        if trust.is_trusted(cert) {
            let serial = std::fs::read(cert)
                .ok()
                .and_then(|pem| x509_parser::pem::parse_x509_pem(&pem).ok().map(|(_, p)| p.contents))
                .and_then(|der| x509_parser::parse_x509_certificate(&der).ok().map(|(_, c)| c.raw_serial_as_string().replace(':', "")));
            println!("  · !! the fixture CA is STILL in this user's Root store — remove it: certutil -user -delstore Root {}", serial.unwrap_or_default());
        }
    }
}

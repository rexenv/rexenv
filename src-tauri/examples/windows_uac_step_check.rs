//! W6 S3, ledger #619: Windows' `PrivilegeManager::run_privileged` end to end — rexenv's own dialog
//! naming the reason, UAC elevating `rexenv.exe --elevated-step`, the step running rexenv's ops only — with
//! a person at the desktop answering, in this order:
//!
//! 1. install  — rexenv's dialog: **OK**, then UAC: **Yes**
//! 2. remove   — rexenv's dialog: **OK**, then UAC: **Yes**
//! 3. install  — rexenv's dialog: **Cancel** (no UAC should follow)
//! 4. install  — rexenv's dialog: **OK**, then UAC: **No**
//!
//! ```text
//! (desktop session, someone at the machine) scripts/probes/windows-uac-step.ps1 through windows-limited-token.sh
//! ```
//!
//! Before any dialog: a script that is not a rexenv op is refused with NO dialog shown. Throughout: a
//! watcher prints every visible window this process owns — rexenv's dialog and its words (UAC runs on the
//! secure desktop, which no process can read). After each answer the NRPT route of the test TLD
//! `.rexuaccheck` is read to hold the outcome to it, and no `rexenv-elevated-*` result file may be left in
//! the temp directory.
//!
//! This example is its own elevated step: started by UAC with `--elevated-step`, it does what `main.rs`
//! does (`platform::run_elevated_step`). The desktop token cannot remove an NRPT rule by itself, so if a
//! run leaves one the command to remove it is printed. `demo` tier: Windows-only.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_uac_step_check: skipped — a Windows check (ledger #619, W6)");
}

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    if let Some(code) = rexenv_lib::platform::run_elevated_step(&argv) {
        std::process::exit(code);
    }
    windows::main()
}

#[cfg(target_os = "windows")]
mod windows {
    use super::common::Check;
    use rexenv_lib::core::dns::ResolverOwner;
    use rexenv_lib::platform::traits::{Platform, PromptReason};
    use std::process::ExitCode;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    const TLD: &str = "rexuaccheck";

    /// Print the title and text of every visible window this process owns, once each, until `stop`.
    fn watch(stop: Arc<AtomicBool>) -> std::thread::JoinHandle<Vec<String>> {
        use windows_sys::Win32::Foundation::{HWND, LPARAM};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            EnumChildWindows, EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
        };
        unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> windows_sys::core::BOOL {
            // SAFETY: `lparam` is the Vec passed by the enumerating call below, alive for it.
            unsafe { (*(lparam as *mut Vec<isize>)).push(hwnd as isize) };
            1
        }
        fn text(hwnd: HWND) -> String {
            let mut buf = [0u16; 1024];
            // SAFETY: the buffer's length is passed.
            let n = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
            String::from_utf16_lossy(&buf[..n.max(0) as usize])
        }
        std::thread::spawn(move || {
            let me = std::process::id();
            let mut seen = std::collections::HashSet::new();
            let mut lines = Vec::new();
            while !stop.load(Ordering::Relaxed) {
                let mut tops: Vec<isize> = Vec::new();
                // SAFETY: `collect` only pushes into `tops`.
                unsafe { EnumWindows(Some(collect), &mut tops as *mut Vec<isize> as LPARAM) };
                for raw in tops {
                    let hwnd = raw as HWND;
                    let mut pid = 0u32;
                    // SAFETY: plain queries on a window handle.
                    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
                    if pid != me || unsafe { IsWindowVisible(hwnd) } == 0 || !seen.insert(raw) {
                        continue;
                    }
                    let mut kids: Vec<isize> = Vec::new();
                    // SAFETY: as above.
                    unsafe { EnumChildWindows(hwnd, Some(collect), &mut kids as *mut Vec<isize> as LPARAM) };
                    let texts: Vec<String> = kids.into_iter().map(|k| text(k as HWND)).filter(|s| !s.trim().is_empty()).collect();
                    let line = format!("title {:?}, text {:?}", text(hwnd), texts);
                    println!("  · window on screen: {line}");
                    lines.push(line);
                }
                std::thread::sleep(Duration::from_millis(150));
            }
            lines
        })
    }

    /// One `run_privileged`, with the watcher running; the outcome, the windows seen, and how long it took.
    fn run(plat: &dyn Platform, label: &str, ops: &str, why: &str) -> (Result<String, String>, Vec<String>, Duration) {
        println!("  · {label}: answer at the desktop now");
        let stop = Arc::new(AtomicBool::new(false));
        let watcher = watch(stop.clone());
        let t = Instant::now();
        let reason = PromptReason::new(why);
        let outcome = plat.privileges().run_privileged(ops, &reason).map_err(|e| e.to_string());
        let took = t.elapsed();
        stop.store(true, Ordering::Relaxed);
        let windows = watcher.join().unwrap_or_default();
        println!("  · {label}: {outcome:?} after {:.1} s", took.as_secs_f64());
        (outcome, windows, took)
    }

    fn leftover_results() -> Vec<String> {
        std::fs::read_dir(std::env::temp_dir())
            .map(|d| {
                d.flatten()
                    .filter_map(|e| e.file_name().into_string().ok())
                    .filter(|n| n.starts_with("rexenv-elevated-"))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn main() -> ExitCode {
        let mut check = Check::new("windows_uac_step_check");
        let plat = rexenv_lib::platform::current();
        let dns = plat.dns();
        let clean = dns.route_owner(TLD, 53) == ResolverOwner::Absent;
        check.is("no rule routes the test TLD before the check", clean, "one already does");
        if !clean {
            return check.verdict();
        }

        // ── Refused before any dialog. ──
        // Each step's dialog names its own change: a first run gave the removal the install's words, and the
        // answer to it could not be told from the words on screen.
        let add = format!("add a DNS rule so .{TLD} sites open on this computer");
        let remove = format!("remove the DNS rule for .{TLD}");
        let (refused, windows, _) = run(&*plat, "a script that is not a rexenv op", "Remove-Item -Recurse C:\\rexenv-nothing", &add);
        check.is("a non-op script is refused", refused.as_ref().is_err_and(|e| e.contains("not a privileged step")), &format!("{refused:?}"));
        check.is("…and no dialog was shown for it", windows.is_empty(), &format!("{windows:?}"));

        // ── 1. OK, Yes → the rule is ours. ──
        let (installed, windows, _) = run(&*plat, "1. install — OK, then Yes", &dns.install_command(TLD, 53), &add);
        check.is("rexenv's dialog appeared, titled rexenv, naming the change and that Windows asks next",
            windows.iter().any(|w| w.contains("title \"rexenv\"") && w.contains(&format!(".{TLD} sites open on this computer")) && w.contains("Windows will ask for your permission next")),
            &format!("{windows:?}"));
        check.is("OK + Yes: run_privileged is Ok and the route is ours", installed.is_ok() && dns.route_owner(TLD, 53) == ResolverOwner::Ours,
            &format!("{installed:?}, {:?}", dns.route_owner(TLD, 53)));

        // ── 2. OK, Yes → the rule is gone. ──
        let (removed, windows, _) = run(&*plat, "2. remove — OK, then Yes", &dns.uninstall_command(&[TLD.to_string()]), &remove);
        check.is("the removal's dialog names the removal", windows.iter().any(|w| w.contains(&remove)), &format!("{windows:?}"));
        check.is("OK + Yes on the removal: Ok and the route is absent", removed.is_ok() && dns.route_owner(TLD, 53) == ResolverOwner::Absent,
            &format!("{removed:?}, {:?}", dns.route_owner(TLD, 53)));

        // ── 3. Cancel in rexenv's dialog → cancelled, nothing changed. ──
        let (cancelled, _, _) = run(&*plat, "3. install — Cancel", &dns.install_command(TLD, 53), &add);
        check.is("Cancel: the cancel wording, and the route is still absent",
            cancelled.as_ref().is_err_and(|e| e.contains("cancelled")) && dns.route_owner(TLD, 53) == ResolverOwner::Absent,
            &format!("{cancelled:?}, {:?}", dns.route_owner(TLD, 53)));

        // ── 4. OK, then No to UAC → cancelled, nothing changed. ──
        let (declined, _, _) = run(&*plat, "4. install — OK, then No", &dns.install_command(TLD, 53), &add);
        check.is("No to UAC: the cancel wording, and the route is still absent",
            declined.as_ref().is_err_and(|e| e.contains("cancelled")) && dns.route_owner(TLD, 53) == ResolverOwner::Absent,
            &format!("{declined:?}, {:?}", dns.route_owner(TLD, 53)));

        let left = leftover_results();
        check.is("no rexenv-elevated result file is left in the temp directory", left.is_empty(), &format!("{left:?}"));
        if dns.route_owner(TLD, 53) != ResolverOwner::Absent {
            println!("  · !! a rule for .{TLD} is left — remove it elevated: Get-DnsClientNrptRule | Where-Object {{ $_.Namespace -contains '.{TLD}' }} | Remove-DnsClientNrptRule -Force");
        }
        check.verdict()
    }
}

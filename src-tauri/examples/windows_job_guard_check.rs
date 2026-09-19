//! Live check: the job-limit fact the Windows start-up hop is decided on is read
//! correctly from INSIDE real jobs, and Explorer does start a copy outside ours.
//! Run: `cargo run --example windows_job_guard_check`  (tier: sandbox, Windows only)
//!
//! # What this proves that L0 cannot
//!
//! `job_guard_rules` is pure and proven on both hosts; what it decides on is a
//! kernel answer — `QueryInformationJobObject(NULL, …)` for the CALLING process —
//! and the measurement behind the rule (19 Sep 2026: the installer's Finish page
//! put rexenv in a job whose limits were `0x0`, and every service start failed
//! with access denied) is worth nothing if that query reads the wrong job or the
//! wrong flags. So this builds two real jobs, one confining and one not, starts a
//! fixture inside each and has the fixture report what `own_job_limits()` says
//! from in there. Then the hop itself: `explorer.exe "<fixture>"` must produce a
//! NEW process — Explorer's, not ours — which is the only thing that makes the
//! hop an escape rather than a restart in the same job.
//!
//! # Fixture-owned
//!
//! The fixture is a COPY OF THIS EXAMPLE under the sandbox root, named
//! `rexenv-jobfixture.exe` so no process table ever confuses it with rexenv; run
//! with `REXENV_JOB_MARKER` it writes its own job limits (or `none`) to that file
//! and exits. The jobs are this process's own handles and die with it. The
//! Explorer leg opens the FIXTURE, never the app.

#![cfg_attr(not(target_os = "windows"), allow(dead_code, unused_imports))]

use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};

mod common;

const MARKER_ENV: &str = "REXENV_JOB_MARKER";
/// The fixture's file name — its ROLE: a copy of this example under that name is the fixture.
const FIXTURE_NAME: &str = "rexenv-jobfixture.exe";

#[cfg(target_os = "windows")]
mod win {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    };
    use windows_sys::Win32::System::Threading::{OpenProcess, ResumeThread, CREATE_SUSPENDED, PROCESS_ALL_ACCESS};

    pub struct Job(pub HANDLE);
    impl Drop for Job {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    /// A job with exactly `flags` as its limits.
    pub fn job(flags: u32) -> Option<Job> {
        let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if raw.is_null() {
            return None;
        }
        let job = Job(raw);
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = flags;
        let ok = unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        (ok != 0).then_some(job)
    }

    /// Start `exe` suspended, put it in `job`, resume it, wait for it.
    pub fn run_inside(job: &Job, exe: &std::path::Path, marker: &std::path::Path) -> Result<(), String> {
        let mut child = std::process::Command::new(exe)
            .env(super::MARKER_ENV, marker)
            .creation_flags(CREATE_SUSPENDED)
            .spawn()
            .map_err(|e| format!("spawn: {e}"))?;
        let h = unsafe { OpenProcess(PROCESS_ALL_ACCESS, 0, child.id()) };
        if h.is_null() {
            let _ = child.kill();
            return Err("OpenProcess failed".into());
        }
        let assigned = unsafe { AssignProcessToJobObject(job.0, h) } != 0;
        unsafe { CloseHandle(h) };
        if !assigned {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("AssignProcessToJobObject failed: {}", std::io::Error::last_os_error()));
        }
        // The one thread of a suspended process: resume it through ToolHelp's list.
        if !resume(child.id()) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("could not resume the fixture".into());
        }
        child.wait().map_err(|e| format!("wait: {e}"))?;
        Ok(())
    }

    fn resume(pid: u32) -> bool {
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
        };
        use windows_sys::Win32::System::Threading::{OpenThread, THREAD_SUSPEND_RESUME};
        let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut resumed = false;
        let mut ok = unsafe { Thread32First(snap, &mut entry) };
        while ok != 0 {
            if entry.th32OwnerProcessID == pid {
                let t = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
                if !t.is_null() {
                    resumed |= unsafe { ResumeThread(t) } != u32::MAX;
                    unsafe { CloseHandle(t) };
                }
            }
            ok = unsafe { Thread32Next(snap, &mut entry) };
        }
        unsafe { CloseHandle(snap) };
        resumed
    }
}

fn read_marker(marker: &Path) -> Option<String> {
    std::fs::read_to_string(marker).ok().map(|s| s.trim().to_string())
}

fn wait_marker(marker: &Path, limit: Duration) -> Option<String> {
    let start = Instant::now();
    while start.elapsed() < limit {
        if let Some(s) = read_marker(marker) {
            if !s.is_empty() {
                return Some(s);
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

#[cfg(target_os = "windows")]
fn main() -> ExitCode {
    // ── The fixture half: a copy of this file NAMED as the fixture reports the job
    // it is in and leaves — to the marker its parent named, or, when Explorer
    // started it (Explorer's environment, not ours), to `hop.txt` beside itself.
    let me = std::env::current_exe().expect("current exe");
    if me.file_name().is_some_and(|n| n.eq_ignore_ascii_case(FIXTURE_NAME)) {
        let line = match rexenv_lib::platform::own_job_limits() {
            Some(flags) => format!("{flags:#x}"),
            None => "none".into(),
        };
        let marker = std::env::var_os(MARKER_ENV)
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| me.with_file_name("hop.txt"));
        let _ = std::fs::write(&marker, line);
        return ExitCode::SUCCESS;
    }

    let (plat, _sandbox) = common::sandbox("winjobguard");
    let mut checks = common::Check::new("windows_job_guard_check");
    let root = plat.paths().app_data_dir().expect("sandbox app data");
    std::fs::create_dir_all(&root).expect("sandbox root");
    let fixture = root.join(FIXTURE_NAME);
    std::fs::copy(&me, &fixture).expect("copy the fixture exe");

    // ── 1. A confining job is read as confining from inside it ───────────────
    let marker = root.join("confined.txt");
    match win::job(0).ok_or("CreateJobObject".to_string()).and_then(|j| win::run_inside(&j, &fixture, &marker)) {
        Ok(()) => {
            let got = read_marker(&marker).unwrap_or_default();
            checks.is(
                "a job with limits 0x0 reads as limits 0x0 from inside (the installer's shape)",
                got == "0x0",
                &got,
            );
        }
        Err(e) => checks.is("a confining job could be built and entered", false, &e),
    }

    // ── 2. A job that permits breakaway is read as permitting it ─────────────
    let marker = root.join("free.txt");
    let flags = 0x2000 | 0x800; // KILL_ON_JOB_CLOSE | BREAKAWAY_OK — the SSH shell's shape
    match win::job(flags).ok_or("CreateJobObject".to_string()).and_then(|j| win::run_inside(&j, &fixture, &marker)) {
        Ok(()) => {
            let got = read_marker(&marker).unwrap_or_default();
            checks.is(
                "a job with BREAKAWAY_OK reads its flags back exactly (0x2800)",
                got == "0x2800",
                &got,
            );
        }
        Err(e) => checks.is("a permissive job could be built and entered", false, &e),
    }

    // ── 3. The hop: Explorer starts the fixture in a job that is NOT confining ─
    // The fixture Explorer starts inherits Explorer's environment, so it reports
    // to `hop.txt` beside itself. What the shell's child is in differs by OS —
    // no job at all on the Windows 11 VM, a job with BREAKAWAY_OK (`0x800`) on the
    // Windows 10 Dell (measured 19 Sep 2026) — and the hop needs only that it is
    // not confining, which is exactly what `confining()` decides.
    let hop = fixture.with_file_name("hop.txt");
    let _ = std::fs::remove_file(&hop);
    let shell = std::path::PathBuf::from(std::env::var_os("SystemRoot").expect("SystemRoot")).join("explorer.exe");
    match std::process::Command::new(&shell).arg(&fixture).spawn() {
        Ok(mut child) => {
            let got = wait_marker(&hop, Duration::from_secs(10));
            let _ = child.wait();
            let limits = got.as_deref().and_then(|g| {
                if g == "none" {
                    Some(None)
                } else {
                    u32::from_str_radix(g.trim_start_matches("0x"), 16).ok().map(Some)
                }
            });
            let escaped = limits.is_some_and(|l| !l.is_some_and(|f| f & (0x800 | 0x1000) == 0));
            checks.is(
                "explorer.exe <path> starts the fixture, and where it lands is NOT confining (the hop is an escape)",
                escaped,
                &format!("{got:?} at {} (None = Explorer never started it within 10 s)", fixture.display()),
            );
        }
        Err(e) => checks.is("explorer.exe could be started", false, &e.to_string()),
    }

    println!(
        "\nNOT covered here: the REAL hop — the installed app started by the installer's Finish\n\
         page, hopping and then starting its services. That is SMOKE-TEST's Windows section on\n\
         an installed copy (measured 19 Sep 2026 before the guard existed: Start all failed)."
    );
    checks.verdict()
}

#[cfg(not(target_os = "windows"))]
fn main() -> ExitCode {
    eprintln!("windows_job_guard_check: skipped — Windows-only (job objects)");
    ExitCode::SUCCESS
}

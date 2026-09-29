//! Windows: start outside a job that forbids its services to outlive it.
//!
//! The rule and the measurement behind it are in `job_guard_rules.rs`; this file
//! is the two facts (this process's job limits, its parent's image name) and the
//! hop. Called from `main.rs` BEFORE Tauri boots — before the app pipe is claimed
//! and before any window — so the copy that hops holds nothing the copy Explorer
//! starts will need.
//!
//! Fail-open, deliberately: if Explorer does not start a new rexenv within a few
//! seconds the process carries on in place, where every service start still
//! fails with the access-denied text that names the cause. A hop that could leave
//! the user with NO rexenv would be worse than the bug it fixes.

use super::job_guard_rules as rules;
use super::process;
use std::time::{Duration, Instant};
use windows_sys::Win32::System::JobObjects::{
    IsProcessInJob, JobObjectExtendedLimitInformation, QueryInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

/// The limit flags of the job THIS process is in; `None` when it is in none (or
/// the question cannot be answered, which is read as "no job" so a query failure
/// never hops).
pub(crate) fn own_job_limits() -> Option<u32> {
    let mut in_job = 0i32;
    // SAFETY: the pseudo-handle needs no closing; a null job asks "any job".
    if unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut in_job) } == 0 || in_job == 0 {
        return None;
    }
    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    let mut written = 0u32;
    // SAFETY: a null job handle queries the calling process's own job (documented);
    // the pointer and size describe `info`, the struct this class fills.
    let ok = unsafe {
        QueryInformationJobObject(
            std::ptr::null_mut(),
            JobObjectExtendedLimitInformation,
            (&mut info as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            &mut written,
        )
    };
    (ok != 0).then_some(info.BasicLimitInformation.LimitFlags)
}

/// The image file name of this process's parent (`msedge.exe`), as ToolHelp
/// records it; `None` when the parent is gone.
pub(crate) fn parent_exe() -> Option<String> {
    let me = std::process::id();
    let table = process::processes();
    let parent = table.iter().find(|e| e.pid == me)?.parent;
    table.into_iter().find(|e| e.pid == parent).map(|e| e.exe)
}

/// If this process is confined, ask Explorer to start rexenv again and return the
/// exit code this copy should leave with; `None` means "run here".
pub(crate) fn relaunch_outside_confining_job() -> Option<i32> {
    let limits = own_job_limits();
    let parent = parent_exe();
    if !rules::should_hop(rules::confining(limits), parent.as_deref()) {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let shell = std::path::PathBuf::from(std::env::var_os("SystemRoot")?).join(rules::SHELL_EXE);
    // Explorer passes no arguments: a `--hidden` (login) launch leaves the flag in a marker the
    // new copy reads once (`hopped_hidden`), or it would arrive as a launch the user made.
    let marker = hop_marker_path().filter(|_| std::env::args().any(|a| a == crate::HIDDEN_LAUNCH_FLAG));
    if let Some(m) = &marker {
        let _ = std::fs::write(m, rules::hop_marker_contents(now_secs()));
    }
    let before: Vec<u32> = rexenv_pids(&exe);
    // Explorer opens the path in ITS process and hands us nothing back — its own
    // exit code is meaningless (1 on success is normal), so the proof that the hop
    // took is a new rexenv process, watched for below.
    let mut child = std::process::Command::new(shell).arg(&exe).spawn().ok()?;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        if rexenv_pids(&exe).iter().any(|p| !before.contains(p)) {
            let _ = child.wait();
            eprintln!(
                "rexenv: started inside a job that forbids its services to outlive it (limits {:#x}, launcher {}) — reopened through Explorer",
                limits.unwrap_or(0),
                parent.as_deref().unwrap_or("gone")
            );
            return Some(0);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.wait();
    if let Some(m) = &marker {
        let _ = std::fs::remove_file(m);
    }
    None
}

/// Whether the copy that hopped here was a `--hidden` launch: reads the marker once, deletes it,
/// and trusts it only while fresh (`job_guard_rules::hop_marker_fresh`).
pub(crate) fn hopped_hidden() -> bool {
    let Some(m) = hop_marker_path() else { return false };
    let Ok(contents) = std::fs::read_to_string(&m) else { return false };
    let _ = std::fs::remove_file(&m);
    rules::hop_marker_fresh(&contents, now_secs())
}

fn hop_marker_path() -> Option<std::path::PathBuf> {
    crate::platform::current().paths().config_dir().ok().map(|d| d.join(rules::HOP_HIDDEN_MARKER))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Every process running from `exe`'s file name, this one included.
fn rexenv_pids(exe: &std::path::Path) -> Vec<u32> {
    let name = exe.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    process::processes()
        .into_iter()
        .filter(|e| e.exe.eq_ignore_ascii_case(&name))
        .map(|e| e.pid)
        .collect()
}

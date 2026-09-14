//! The DNS agent's output on Windows (W6 S2, ledger #616).
//!
//! launchd writes a LaunchAgent's stdout and stderr to the files its plist names; Task Scheduler
//! gives a task's action neither, so everything `run_agent` prints — the bind it got, the bind it
//! could not get and why — went nowhere, and a wedged resolver left no evidence. The task's definition
//! passes the log path instead (`--dns-agent --log <path>`), and this points both standard handles at
//! that file before the agent starts. Rust's standard streams look the handle up on every write, so
//! `eprintln!` lands in the file from here on.

use std::os::windows::io::IntoRawHandle;
use std::path::Path;
use windows_sys::Win32::System::Console::{SetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE};

/// Append this process's stdout and stderr to `log`. Best-effort: if the file cannot be opened, the
/// agent still runs, as it does with no log on any OS.
pub(crate) fn send_output_to(log: &Path) {
    if let Some(parent) = log.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(log) else {
        return;
    };
    // The handle is deliberately leaked: it must stay open for the life of the process.
    let handle = file.into_raw_handle();
    // SAFETY: `handle` is a valid, open file handle this process owns and never closes.
    unsafe {
        SetStdHandle(STD_ERROR_HANDLE, handle.cast());
        SetStdHandle(STD_OUTPUT_HANDLE, handle.cast());
    }
}

//! The panic hook — a panic says so where a person can read it.
//!
//! A release build has no console (`windows_subsystem = "windows"` on Windows, and
//! an app opened from Finder or a login item has no terminal either), and until
//! 13 Sep 2026 rexenv installed no hook. A panic's message went to a stderr nobody
//! had: the window never appeared, or — from an IPC command — the frontend waited
//! forever (`docs/PLAN-windows-port.md` §3a Q1, W3 step 0).
//!
//! So every panic appends a report to `crash.log`: version, OS, thread, message,
//! location and a backtrace. It goes in the log directory when `Paths` answers,
//! and in the temp directory when it does not — a half-ported platform may have no
//! log directory yet, and the hook must not need one. The first panic of a process
//! also calls `platform::fatal_notice`, which on a Windows release build raises a
//! native message box naming the file. The previous hook still runs afterwards, so
//! a debug build keeps its stderr line (ledger #596).
//!
//! A panic that something later catches is recorded too: a hook runs before
//! unwinding and cannot know. Production code catches none today (the two
//! `catch_unwind`s in `commands/php.rs` are in its tests).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// The file every panic report is appended to.
pub const CRASH_FILE: &str = "crash.log";
/// The previous file, once `crash.log` passes [`MAX_BYTES`].
pub const CRASH_FILE_OLD: &str = "crash.log.old";
/// Past this size the file is rotated before the next report, so a panic loop
/// cannot fill a disk.
const MAX_BYTES: u64 = 1024 * 1024;

/// Whether this process has already told a person. A background task that
/// panics repeatedly must not raise a dialog per panic.
static NOTIFIED: AtomicBool = AtomicBool::new(false);

/// Install the hook. Call first thing in `main`, before any mode and before Tauri.
pub fn install() {
    install_at(None);
}

/// [`install`], writing to `dir` instead of the resolved crash directory — the
/// seam the test drives from a child process.
pub fn install_at(dir: Option<PathBuf>) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // The pieces, not the info type: `PanicHookInfo` is stable only since
        // 1.81 and this crate's MSRV is 1.77.2 (clippy `incompatible_msrv`),
        // while `payload()`, `location()` and `Location` are far older.
        let message = message(info.payload());
        let location = location(info.location());
        let dir = dir.clone().unwrap_or_else(crash_dir);
        let written = append(&dir, &report(&message, &location)).ok();
        if !NOTIFIED.swap(true, Ordering::SeqCst) {
            crate::platform::fatal_notice(&format!("{message} (at {location})"), written.as_deref());
        }
        previous(info);
    }));
}

/// The log directory when the platform has one, else the temp directory.
fn crash_dir() -> PathBuf {
    crate::platform::log_dir_or_temp()
}

fn message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(a panic whose payload is not text)".to_string())
}

fn location(location: Option<&std::panic::Location<'_>>) -> String {
    location
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
        .unwrap_or_else(|| "(unknown)".to_string())
}

/// The full report appended to the file.
fn report(message: &str, location: &str) -> String {
    let when = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "(time unavailable)".to_string());
    let thread = std::thread::current();
    format!(
        "=== rexenv {} panicked at {when}\nos: {} {}\nthread: {}\nmessage: {}\nlocation: {}\nbacktrace:\n{}\n\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        thread.name().unwrap_or("(unnamed)"),
        message,
        location,
        std::backtrace::Backtrace::force_capture(),
    )
}

/// Append `report` to `dir/crash.log`, rotating a file past [`MAX_BYTES`] first.
fn append(dir: &Path, report: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(CRASH_FILE);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(&path, dir.join(CRASH_FILE_OLD));
    }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
    file.write_all(report.as_bytes())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    /// Set only in the child process the hook test spawns.
    const CHILD_DIR_ENV: &str = "REXENV_CRASH_HOOK_CHILD_DIR";

    /// The child half of [`a_panic_under_the_hook_is_written_to_crash_log`]. In
    /// an ordinary test run the variable is unset and this returns at once; in
    /// the child it installs the hook and panics, so the hook runs in a process
    /// of its own and never touches the parent's global hook.
    #[test]
    fn child_panics_under_the_hook() {
        let Ok(dir) = std::env::var(CHILD_DIR_ENV) else { return };
        super::install_at(Some(dir.into()));
        panic!("planted panic for ledger 596");
    }

    #[test]
    fn a_panic_under_the_hook_is_written_to_crash_log() {
        let dir = std::env::temp_dir().join(format!("rexenv-crash-hook-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let out = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args(["--exact", "crash::tests::child_panics_under_the_hook", "--nocapture", "--test-threads=1"])
            .env(CHILD_DIR_ENV, &dir)
            .output()
            .expect("run the child");
        assert!(!out.status.success(), "the child did not panic: {}", String::from_utf8_lossy(&out.stdout));
        let log = std::fs::read_to_string(dir.join(super::CRASH_FILE))
            .expect("the hook did not write crash.log");
        for want in [
            "planted panic for ledger 596",
            // Not "src/crash.rs": `Location::file()` uses the host's separator, so
            // Windows writes `src\crash.rs` — correctly, since that is what the rest of
            // the report and the backtrace say too. The file name is the claim.
            "crash.rs",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            "backtrace:",
        ] {
            assert!(log.contains(want), "crash.log lacks {want:?}:\n{log}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_crash_log_past_its_cap_rotates_before_the_next_report() {
        let dir = std::env::temp_dir().join(format!("rexenv-crash-rotate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(super::CRASH_FILE), vec![b'x'; super::MAX_BYTES as usize + 1]).unwrap();
        let path = super::append(&dir, "=== the next report\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "=== the next report\n");
        assert_eq!(
            std::fs::metadata(dir.join(super::CRASH_FILE_OLD)).unwrap().len(),
            super::MAX_BYTES + 1,
            "the oversized file must be kept as crash.log.old, not deleted"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

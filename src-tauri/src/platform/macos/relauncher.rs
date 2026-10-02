//! The detached relauncher: reopen rexenv once the process that swapped the
//! bundle is actually gone.
//!
//! # Why this exists rather than `AppHandle::restart`
//!
//! Tauri's restart spawns the child and THEN exits, which breaks two things this
//! app relies on:
//!
//! - **The single-instance lock.** rexenv's lock is a unix socket claimed before
//!   Tauri boots, and liveness is one `connect()` with no retry (ledger #441). A
//!   child that reaches the claim while the dying parent's listener is still open
//!   hands its launch to a process on its way out — and then BOTH exit, leaving
//!   no instance at all. Waiting for the parent to be gone removes the
//!   precondition instead of racing it.
//! - **The ONE quit gate.** `AppHandle::restart` on the main thread skips
//!   `RunEvent::ExitRequested` and `RunEvent::Exit` entirely — the live-share
//!   confirm, the tunnel kill and the repo-job cancel all belong to those events
//!   (#436). Off the main thread it can be cancelled by `prevent_exit` and leave
//!   a thread sleeping forever with `restart_on_exit` latched, so the NEXT quit
//!   silently relaunches instead.
//!
//! It also inherits the dying process's stdio, which upstream tauri#15742
//! records as an EPIPE abort before the webview loads. `open` hands the launch
//! to LaunchServices: clean stdio, launchd as the parent, no inherited argv.
//!
//! # The registration gap, closed the way the tunnel guard closes it
//!
//! Between rexenv reading its own pid and this process registering for its
//! death, that pid could be recycled. So the parent's START TOKEN rides argv and
//! is checked before the wait: a mismatch means the parent we were told about is
//! already gone, which is the answer, not an error.

use crate::core::app_update::{ParentWait, RelaunchArgs, RelaunchLog};
use std::time::Duration;

/// How long to wait before giving up.
///
/// The bound exists because the wait is not guaranteed to end: a user who
/// answers "Keep sharing" to the quit confirm leaves the app running with the
/// new bundle already in place, and this process must not sit on that pid for
/// the rest of the login session. Giving up is safe — the update is installed,
/// and the next ordinary launch runs it.
const CAP: Duration = Duration::from_secs(120);

pub fn run(args: RelaunchArgs) -> i32 {
    let RelaunchArgs { parent, parent_start, bundle, log_dir } = args;
    // The record (ledger #768): a reopen that does not happen used to leave no trace —
    // stderr is `/dev/null` by design and `open`'s exit code died with this process (1 of 2
    // on the 15.8 VM, 30 Sep 2026). `relaunch.log` carries the wait and the open result.
    let log = RelaunchLog::in_dir(&log_dir.unwrap_or_else(crate::platform::log_dir_or_temp));
    log.note(&format!(
        "started: waiting for pid {parent} (token {parent_start}) to exit, then opening {}",
        bundle.display()
    ));
    let began = std::time::Instant::now();
    let waited = wait_for_parent(parent, &parent_start);
    log.note(&format!("parent {parent} {} after {:.1}s", waited.describe(), began.elapsed().as_secs_f64()));
    // `open` the PATH, never `open -b dev.rexenv.rexenv`: at this moment the
    // PREVIOUS bundle is still on disk in the staging directory, and a bundle-id
    // launch is free to pick it — which would silently start the version the
    // user just replaced.
    match std::process::Command::new("/usr/bin/open").arg(&bundle).status() {
        Ok(s) if s.success() => {
            log.note(&format!("opened {} (open exit 0)", bundle.display()));
            0
        }
        Ok(s) => {
            log.note(&format!("open exited {:?} for {}", s.code(), bundle.display()));
            eprintln!("relauncher: open exited {:?}", s.code());
            1
        }
        Err(e) => {
            log.note(&format!("could not run /usr/bin/open for {}: {e}", bundle.display()));
            eprintln!("relauncher: could not open {}: {e}", bundle.display());
            1
        }
    }
}

/// Block until `parent` exits, it turns out to be somebody else, or [`CAP`] — and say which.
fn wait_for_parent(parent: u32, parent_start: &str) -> ParentWait {
    if !super::process_start_token(parent).is_some_and(|t| t == parent_start) {
        // Either it already exited, or that pid belongs to a different process
        // now. Both mean the same thing here: stop waiting.
        return ParentWait::AlreadyGone;
    }
    // SAFETY: kqueue/kevent with a locally-owned fd and stack-allocated event
    // structs; every raw pointer below points at a live local.
    unsafe {
        let kq = libc::kqueue();
        if kq < 0 {
            return ParentWait::Unwatchable;
        }
        let mut ev: libc::kevent = std::mem::zeroed();
        ev.ident = parent as usize;
        ev.filter = libc::EVFILT_PROC;
        ev.flags = libc::EV_ADD | libc::EV_ONESHOT;
        ev.fflags = libc::NOTE_EXIT;
        if libc::kevent(kq, &ev, 1, std::ptr::null_mut(), 0, std::ptr::null()) < 0 {
            // ESRCH: the pid is gone. That is the answer; anything else is the kernel
            // refusing to watch it.
            let errno = std::io::Error::last_os_error().raw_os_error();
            libc::close(kq);
            return if errno == Some(libc::ESRCH) { ParentWait::AlreadyGone } else { ParentWait::Unwatchable };
        }
        // Re-check AFTER registering: a pid that died and was replaced between
        // the check above and this registration would have registered a
        // stranger, and we would wait on their lifetime instead.
        if !super::process_start_token(parent).is_some_and(|t| t == parent_start) {
            libc::close(kq);
            return ParentWait::AlreadyGone;
        }
        let timeout = libc::timespec {
            tv_sec: CAP.as_secs() as libc::time_t,
            tv_nsec: 0,
        };
        let mut out: libc::kevent = std::mem::zeroed();
        let n = libc::kevent(kq, std::ptr::null(), 0, &mut out, 1, &timeout);
        libc::close(kq);
        match n {
            0 => ParentWait::TimedOut,
            n if n > 0 => ParentWait::Exited,
            _ => ParentWait::Unwatchable,
        }
    }
}

//! The Linux PARENT-DEATH guard: a public share dies with the app even when the app is
//! KILLED — the macOS guard (`macos/parent_death_guard.rs`) with the kernel's Linux event.
//!
//! **Why a separate process on Linux too, when `PR_SET_PDEATHSIG` exists.** The death
//! signal is tied to the THREAD that spawned the child, not the process: a service spawned
//! from a tokio blocking-pool thread would be SIGTERMed the moment that idle thread was
//! retired, with the app alive and the user watching their share vanish. So the shape stays
//! the macOS one — our own binary re-executed with `GUARD_FLAG`, the DNS agent's self-exec.
//!
//! **Why pidfd and not a poll.** `pidfd_open(2)` (Linux 5.3; Ubuntu 22.04 ships 5.15) gives a
//! file descriptor that becomes readable when the process exits — the kernel telling us, as
//! kqueue's `NOTE_EXIT` does on macOS; a timer poll leaves a window sized by its interval.
//! One pidfd for the parent, one for the child, `poll(2)` on both: the ordinary case (the
//! user stops sharing) reaps the guard at once.
//!
//! **The registration gap, closed the same way.** The parent's START TIME rides argv
//! (`GuardArgs::parent_start`, here `/proc/<pid>/stat`'s `starttime` in clock ticks since
//! boot — recycled pids wear a different one) and is checked before AND after opening the
//! pidfd. **Never kill on a bare pid**: the child is re-identified by argv (`is_our_tunnel`)
//! before any signal, as every sweep does.

use crate::core::tunnels::{is_our_tunnel, GuardArgs};
use crate::platform::linux::{LinuxPaths, LinuxSupervisor};
use crate::platform::traits::{Paths, ProcessSupervisor};
use std::time::Duration;

/// Grace between SIGTERM and SIGKILL — the app's own escalation, bounded so a guard can never
/// outlive the thing it guards.
const TERM_GRACE: Duration = Duration::from_millis(1500);

/// Run the guard. Blocks until the parent OR the child exits, then stops the child if (and
/// only if) it is still provably ours. Returns the process exit code — the caller is `main`,
/// before Tauri boots.
pub fn run(args: GuardArgs) -> i32 {
    let GuardArgs { parent, child, domain, parent_start } = args;
    match wait_for_exit(parent, child, &parent_start) {
        Exit::Child => 0,
        Exit::Parent | Exit::Immediate => {
            if !still_ours(child, &domain) {
                return 0;
            }
            stop_child(child);
            0
        }
        Exit::Unwatchable => 1,
    }
}

enum Exit {
    Parent,
    Child,
    Immediate,
    /// `pidfd_open` refused (a kernel older than 5.3, or a seccomp policy). Loud, no guessing.
    Unwatchable,
}

fn parent_is_ours(pid: u32, start: &str) -> bool {
    super::process_start_token(pid).as_deref() == Some(start)
}

/// A pidfd for `pid`, or `None` when the process is already gone (`ESRCH`) — `Err` for
/// anything else, which is "cannot watch", not "gone".
fn pidfd(pid: u32) -> Result<Option<libc::c_int>, ()> {
    // SAFETY: a raw syscall with two integer arguments and no pointers.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0 as libc::c_uint) };
    if fd >= 0 {
        return Ok(Some(fd as libc::c_int));
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::ESRCH) => Ok(None),
        _ => Err(()),
    }
}

/// Block until one of the two pidfds is readable and report which.
fn wait_for_exit(parent: u32, child: u32, parent_start: &str) -> Exit {
    if !parent_is_ours(parent, parent_start) {
        return Exit::Immediate;
    }
    let pfd = match pidfd(parent) {
        Ok(Some(fd)) => fd,
        Ok(None) => return Exit::Immediate,
        Err(()) => return Exit::Unwatchable,
    };
    let cfd = match pidfd(child) {
        Ok(Some(fd)) => fd,
        Ok(None) => {
            // SAFETY: closing a descriptor this function opened.
            unsafe { libc::close(pfd) };
            return Exit::Child;
        }
        Err(()) => {
            // SAFETY: as above.
            unsafe { libc::close(pfd) };
            return Exit::Unwatchable;
        }
    };
    // Re-check AFTER opening: a parent that died and was replaced in between handed us a
    // stranger's pidfd.
    if !parent_is_ours(parent, parent_start) {
        // SAFETY: closing descriptors this function opened.
        unsafe {
            libc::close(pfd);
            libc::close(cfd);
        }
        return Exit::Immediate;
    }
    let mut fds = [
        libc::pollfd { fd: pfd, events: libc::POLLIN, revents: 0 },
        libc::pollfd { fd: cfd, events: libc::POLLIN, revents: 0 },
    ];
    let outcome = loop {
        // SAFETY: `fds` is a live local array of the length passed; -1 = no timeout.
        let rc = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) };
        if rc < 0 {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            break Exit::Unwatchable;
        }
        if fds[0].revents != 0 {
            break Exit::Parent;
        }
        if fds[1].revents != 0 {
            break Exit::Child;
        }
    };
    // SAFETY: closing descriptors this function opened.
    unsafe {
        libc::close(pfd);
        libc::close(cfd);
    }
    outcome
}

/// Is `pid` still the cloudflared serving `domain` for THIS install?
fn still_ours(pid: u32, domain: &str) -> bool {
    let marker = LinuxPaths.app_data_dir().map(|p| p.display().to_string()).unwrap_or_default();
    if marker.is_empty() {
        return false;
    }
    LinuxSupervisor.pid_command(pid).map(|cmd| is_our_tunnel(&cmd, &marker, domain)).unwrap_or(false)
}

/// SIGTERM, brief grace, then SIGKILL — `libc::kill`, never a `kill` found on PATH: the app is
/// gone when this runs and the share is public.
fn stop_child(pid: u32) {
    // SAFETY: kill(2) on a pid `still_ours` just identified.
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
    std::thread::sleep(TERM_GRACE);
    // SAFETY: signal 0 only probes.
    let alive = unsafe { libc::kill(pid as libc::pid_t, 0) == 0 };
    if alive {
        // SAFETY: as above.
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGKILL);
        }
    }
}

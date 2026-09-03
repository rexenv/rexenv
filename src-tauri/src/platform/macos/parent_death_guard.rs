//! The PARENT-DEATH guard: a public share dies with the app even when the app is
//! KILLED.
//!
//! Not to be confused with `examples/tunnel_guard_check.rs`, which pins the test
//! harness's OWN reaper (`common::Reaped`). This one is production code watching
//! a real rexenv process; its live check is
//! `examples/tunnel_parent_death_check.rs`.
//!
//! **The gap this closes.** "Tunnels die with the app" (ruled 28 Jul 2026) had two
//! implementations and a hole between them: `RunEvent::Exit` kills them on a clean
//! quit, and the launch sweep kills the survivors of a crash — *at the next
//! launch*. Between a SIGKILL and the next time the user opens rexenv, a site is
//! served to the internet with nothing supervising it, and nothing on the machine
//! can see that but `ps`. That window is not bounded by anything: it is however
//! long the user takes to come back. Measured on this machine once already — the
//! rowless sweep's own row records shares that had been public for WEEKS.
//!
//! **Why a separate process.** macOS has no `PR_SET_PDEATHSIG`; a dead parent runs
//! no code, so the watcher cannot live inside rexenv. One tiny guard per share
//! watches the parent instead — our own binary, re-executed with
//! [`crate::core::tunnels::GUARD_FLAG`], the same self-exec shape the DNS agent
//! uses.
//!
//! **The registration gap, closed.** The parent pid arrives on argv; between
//! rexenv reading its own pid and this guard's `kevent`, rexenv could die and
//! the kernel recycle the number. Registration would then succeed against a
//! STRANGER, and the guard sleep until that process exits — the share public for
//! however long that is. So the parent's START TIME rides argv too
//! (`GuardArgs::parent_start`), and the pid is checked against it twice: before
//! registering, and again AFTER — a pid that died and came back between the
//! two wears a different start time, and either mismatch is read as "the
//! parent is gone" (the share dies). `still_ours` keeps the kill itself safe.
//!
//! **Why kqueue and not a poll.** `EVFILT_PROC`/`NOTE_EXIT` is the kernel telling
//! us the process is gone; a poll is a timer that pretends to be an event and
//! leaves a window sized by its own interval. The guard also watches the CHILD, so
//! the ordinary case — the user stops sharing — reaps the guard immediately
//! instead of leaving a process per share sitting on a dead pid.
//!
//! **The rule it must not break: never kill on a bare pid.** Between the parent's
//! death and this waking up, the child's pid may have been recycled. The guard
//! re-reads the pid's argv and kills only on the same positive identification the
//! sweeps use (`is_our_tunnel`: our app-data marker AND this domain's
//! `--http-host-header`). No identity, no signal — the launch sweep will settle it.

use crate::core::tunnels::{is_our_tunnel, GuardArgs};
use crate::platform::macos::{MacosPaths, MacosSupervisor};
use crate::platform::traits::{Paths, ProcessSupervisor};
use std::time::Duration;

/// Grace between SIGTERM and SIGKILL. cloudflared exits promptly on TERM; the
/// escalation exists for a wedged one, and is bounded so a guard can never sit
/// around after the thing it guards.
const TERM_GRACE: Duration = Duration::from_millis(1500);

/// Run the guard. Blocks until the parent OR the child exits, then stops the
/// child if (and only if) it is still provably ours. Returns the process exit
/// code — the caller is `main`, before Tauri boots.
pub fn run(args: GuardArgs) -> i32 {
    let GuardArgs { parent, child, domain, parent_start } = args;
    match wait_for_exit(parent, child, &parent_start) {
        Exit::Child => 0, // the share ended on its own terms; nothing to do
        Exit::Parent | Exit::Immediate => {
            if !still_ours(child, &domain) {
                // Either it is already gone, or that pid is somebody else's
                // now. Both mean: do nothing. The launch sweep is the backstop
                // for the case where it IS ours and we somehow misread it.
                return 0;
            }
            stop_child(child);
            0
        }
        Exit::Unwatchable => 1,
    }
}

enum Exit {
    /// The parent died — the case the guard exists for.
    Parent,
    /// The child died first (a normal stop, or cloudflared crashing).
    Child,
    /// The parent was already gone before we could watch it: same action as
    /// `Parent`, and reached whenever the app dies inside the spawn window.
    Immediate,
    /// kqueue itself is unavailable. Loud in the log, no guessing.
    Unwatchable,
}

/// Is `pid` the process whose start time we were handed? `false` for a gone
/// pid and for a recycled one alike — both mean the parent we were told about
/// is dead.
fn parent_is_ours(pid: u32, start: &str) -> bool {
    super::process_start_token(pid).as_deref() == Some(start)
}

/// Block on `NOTE_EXIT` for both pids and report which fired first.
fn wait_for_exit(parent: u32, child: u32, parent_start: &str) -> Exit {
    if !parent_is_ours(parent, parent_start) {
        return Exit::Immediate;
    }
    // SAFETY: kqueue/kevent with a locally-owned fd and stack-allocated event
    // structs; every raw pointer below points at a live local.
    unsafe {
        let kq = libc::kqueue();
        if kq < 0 {
            return Exit::Unwatchable;
        }
        let mut changes = [proc_event(parent), proc_event(child)];
        // Registering an exited pid fails with ESRCH, which is the answer, not
        // an error: a parent that died between spawn and here must still end
        // the share. Registered one at a time so we can tell WHICH is gone.
        for (i, ev) in changes.iter_mut().enumerate() {
            let rc = libc::kevent(kq, ev, 1, std::ptr::null_mut(), 0, std::ptr::null());
            if rc < 0 {
                libc::close(kq);
                return if i == 0 { Exit::Immediate } else { Exit::Child };
            }
        }
        // Re-check AFTER registering: a parent that died and was replaced
        // between the check above and the `kevent` registered a stranger.
        if !parent_is_ours(parent, parent_start) {
            libc::close(kq);
            return Exit::Immediate;
        }
        let mut out: libc::kevent = std::mem::zeroed();
        let rc = libc::kevent(kq, std::ptr::null(), 0, &mut out, 1, std::ptr::null());
        libc::close(kq);
        if rc <= 0 {
            return Exit::Unwatchable;
        }
        if out.ident as u32 == parent {
            Exit::Parent
        } else {
            Exit::Child
        }
    }
}

/// A one-shot `EVFILT_PROC` registration for `pid`, firing on exit.
unsafe fn proc_event(pid: u32) -> libc::kevent {
    let mut ev: libc::kevent = std::mem::zeroed();
    ev.ident = pid as usize;
    ev.filter = libc::EVFILT_PROC;
    ev.flags = libc::EV_ADD | libc::EV_ENABLE | libc::EV_ONESHOT;
    ev.fflags = libc::NOTE_EXIT;
    ev
}

/// Is `pid` still the cloudflared serving `domain` for THIS install? The whole
/// safety of the guard is here: a pid the kernel recycled must never be signalled.
fn still_ours(pid: u32, domain: &str) -> bool {
    let marker = MacosPaths
        .app_data_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    if marker.is_empty() {
        return false; // cannot identify ⇒ cannot kill
    }
    MacosSupervisor
        .pid_command(pid)
        .map(|cmd| is_our_tunnel(&cmd, &marker, domain))
        .unwrap_or(false)
}

/// SIGTERM, brief grace, then SIGKILL — the same escalation the app uses, kept
/// here because the app is by definition gone when this runs.
fn stop_child(pid: u32) {
    // `libc::kill`, not `/usr/bin/kill` by PATH lookup: this runs when the
    // app is by definition gone and the share is public — a PATH that does
    // not resolve `kill` would have left it public, silently.
    // SAFETY: kill(2) on a pid `still_ours` just identified; SIGTERM then
    // SIGKILL are the same escalation the app uses.
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
    std::thread::sleep(TERM_GRACE);
    // SAFETY: kill(2) with signal 0 only probes for existence.
    let alive = unsafe { libc::kill(pid as libc::pid_t, 0) == 0 };
    if alive {
        // SAFETY: as above.
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGKILL);
        }
    }
}

//! How `WindowsSupervisor::stop` ends a process — the pure sequencing, with no Win32 in
//! it, so it is also compiled into the macOS test build (`platform/mod.rs`) and its
//! order is tested on every host (ledger #600).
//!
//! # The rule (owner ruling 13 Sep 2026)
//!
//! Windows has no SIGTERM. A process that publishes its OWN clean-shutdown channel is
//! asked through it and given a bounded grace; anything else — or anything still alive
//! when the grace runs out — is ended with `TerminateProcess`. Today the one channel is
//! MySQL's named event. Go servers (Mailpit, later Caddy and cloudflared) turn
//! CTRL_BREAK into SIGINT, but delivering it needs a shared console and is unmeasured,
//! so they are terminated until a service needs more (Mailpit: exited in 110 ms, port
//! released — the Dell, 13 Sep 2026).

use std::time::Duration;

/// `mysqld`'s clean-shutdown event when it runs without its restart monitor.
///
/// Measured on the Dell (MySQL 8.4.6, 13 Sep 2026): started with `--no-monitor`, the one
/// process published `MYSQLShutdown<pid>`, and `SetEvent` on it gave "Normal shutdown …
/// Shutdown complete" in 906 ms. With the monitor the event is named after the MONITOR
/// (`mysqld<monitor pid>_shutdown`) and lives in its child — which is why rexenv runs
/// `mysqld` with [`MYSQLD_ARGS`].
pub(crate) fn mysqld_shutdown_event(pid: u32) -> String {
    format!("MYSQLShutdown{pid}")
}

/// nginx's graceful-quit event for its MASTER `pid` — what `nginx -s quit` sets.
///
/// Measured on the Dell (nginx 1.30.4, 14 Sep 2026): the master publishes `ngx_quit_<pid>`,
/// `ngx_stop_<pid>`, `ngx_reload_<pid>` and `ngx_reopen_<pid>`. `TerminateProcess` on the
/// master left its worker alive, LISTENING and serving — so nginx must be asked to quit,
/// never only terminated; `-s quit` ended master and worker in 331 ms.
pub(crate) fn nginx_quit_event(pid: u32) -> String {
    format!("ngx_quit_{pid}")
}

/// nginx's reload event for its master `pid` — what `nginx -s reload` sets.
pub(crate) fn nginx_reload_event(pid: u32) -> String {
    format!("ngx_reload_{pid}")
}

/// Every clean-shutdown event a process might publish for `pid`, tried in order. A
/// name belongs to exactly one program, so the one that opens is that program's own.
pub(crate) fn clean_exit_events(pid: u32) -> [String; 2] {
    [mysqld_shutdown_event(pid), nginx_quit_event(pid)]
}

/// The arguments rexenv adds to `mysqld` on Windows (`ProcessSupervisor::mysqld_supervision_args`).
///
/// `--no-monitor` because MySQL 8.4 on Windows otherwise runs as TWO processes: a restart
/// monitor that `CreateProcess`es the real server with no job object. rexenv spawns and
/// holds the monitor; the listener is the child. Measured: `TerminateProcess` on the
/// monitor left the child alive and LISTENING — an orphan holding the port, the macOS
/// orphan-worker class. The monitor exists only to serve the SQL `RESTART` statement;
/// rexenv supervises the server itself (owner ruling 13 Sep 2026).
pub(crate) const MYSQLD_ARGS: &[&str] = &["--no-monitor"];

/// How long a process that accepted a clean-shutdown request gets before it is
/// terminated. MySQL idle took 0.6–0.9 s on the Dell; the rest is room for flushing under
/// load, bounded so a Stop never hangs.
pub(crate) const CLEAN_EXIT_GRACE: Duration = Duration::from_secs(10);

/// How long to wait for the process object to signal after `TerminateProcess`.
pub(crate) const TERMINATE_WAIT: Duration = Duration::from_secs(3);

/// The process being stopped, as the policy sees it.
pub(crate) trait Target {
    /// Still running.
    fn alive(&mut self) -> bool;
    /// Ask through the process's own shutdown channel; `true` = the request was delivered.
    fn request_clean_exit(&mut self) -> bool;
    /// Wait up to `budget` for it to exit; `true` = it did.
    fn wait_exit(&mut self, budget: Duration) -> bool;
    /// `TerminateProcess`; `true` = the call succeeded.
    fn terminate(&mut self) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Not running when the stop began.
    AlreadyGone,
    /// Exited through its own shutdown channel within the grace.
    CleanExit,
    /// Ended by `TerminateProcess`.
    Terminated,
    /// Still running after `TerminateProcess` and its wait.
    Survived,
}

/// Stop `target`: clean request and grace when it has a channel, `TerminateProcess` when
/// it has none or ignores it. Termination is never skipped for a process still alive, and
/// never reached for one that exited cleanly.
pub(crate) fn stop(target: &mut impl Target, grace: Duration, terminate_wait: Duration) -> Outcome {
    if !target.alive() {
        return Outcome::AlreadyGone;
    }
    if target.request_clean_exit() && target.wait_exit(grace) {
        return Outcome::CleanExit;
    }
    // A failed TerminateProcess is not the verdict: the process may have exited on its
    // own in between, and the wait below is what says whether it is gone.
    let _ = target.terminate();
    if target.wait_exit(terminate_wait) {
        Outcome::Terminated
    } else {
        Outcome::Survived
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A process that records what the policy did to it.
    struct Fake {
        running: bool,
        has_channel: bool,
        honours_channel: bool,
        dies_on_terminate: bool,
        calls: Vec<String>,
    }

    impl Fake {
        fn new(running: bool, has_channel: bool, honours_channel: bool, dies_on_terminate: bool) -> Self {
            Fake { running, has_channel, honours_channel, dies_on_terminate, calls: Vec::new() }
        }
    }

    impl Target for Fake {
        fn alive(&mut self) -> bool {
            self.calls.push("alive".into());
            self.running
        }
        fn request_clean_exit(&mut self) -> bool {
            self.calls.push("request".into());
            if self.has_channel && self.honours_channel {
                self.running = false;
            }
            self.has_channel
        }
        fn wait_exit(&mut self, budget: Duration) -> bool {
            self.calls.push(format!("wait {}ms", budget.as_millis()));
            !self.running
        }
        fn terminate(&mut self) -> bool {
            self.calls.push("terminate".into());
            if self.dies_on_terminate {
                self.running = false;
            }
            true
        }
    }

    const GRACE: Duration = Duration::from_millis(10_000);
    const TERM: Duration = Duration::from_millis(3_000);

    #[test]
    fn a_process_with_a_channel_that_honours_it_is_never_terminated() {
        let mut p = Fake::new(true, true, true, true);
        assert_eq!(stop(&mut p, GRACE, TERM), Outcome::CleanExit);
        assert_eq!(p.calls, ["alive", "request", "wait 10000ms"]);
    }

    #[test]
    fn a_channel_ignored_past_the_grace_ends_in_terminate() {
        let mut p = Fake::new(true, true, false, true);
        assert_eq!(stop(&mut p, GRACE, TERM), Outcome::Terminated);
        assert_eq!(p.calls, ["alive", "request", "wait 10000ms", "terminate", "wait 3000ms"]);
    }

    /// No channel (Mailpit): terminate at once — no grace spent waiting for a request
    /// that was never delivered.
    #[test]
    fn no_channel_means_terminate_without_waiting_the_grace() {
        let mut p = Fake::new(true, false, false, true);
        assert_eq!(stop(&mut p, GRACE, TERM), Outcome::Terminated);
        assert_eq!(p.calls, ["alive", "request", "terminate", "wait 3000ms"]);
    }

    #[test]
    fn a_gone_process_is_left_alone_and_a_survivor_is_reported() {
        let mut gone = Fake::new(false, true, true, true);
        assert_eq!(stop(&mut gone, GRACE, TERM), Outcome::AlreadyGone);
        assert_eq!(gone.calls, ["alive"]);
        let mut stubborn = Fake::new(true, false, false, false);
        assert_eq!(stop(&mut stubborn, GRACE, TERM), Outcome::Survived);
    }

    #[test]
    fn nginx_is_asked_to_quit_and_reload_by_the_measured_event_names() {
        assert_eq!(nginx_quit_event(10208), "ngx_quit_10208");
        assert_eq!(nginx_reload_event(10208), "ngx_reload_10208");
        assert_eq!(clean_exit_events(7), ["MYSQLShutdown7".to_string(), "ngx_quit_7".to_string()]);
    }

    #[test]
    fn mysqld_runs_without_its_monitor_and_its_event_is_the_measured_name() {
        assert!(MYSQLD_ARGS.contains(&"--no-monitor"));
        assert_eq!(mysqld_shutdown_event(12056), "MYSQLShutdown12056");
    }
}

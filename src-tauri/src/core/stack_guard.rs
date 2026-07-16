//! core::stack_guard — protects the user's RUNNING stack from non-app processes.
//!
//! Live-check examples link this lib and deliberately run against the REAL
//! app-data dir (the shared binary cache and admin socket are the point of a
//! live check) — which historically let an example's `stop_all` /
//! `recover_stale_edge` tear down the user's serving stack over the shared
//! admin socket (docs/TODO.md "Isolate live-check examples").
//!
//! Rule: a process that is not the rexenv app may stop only what it SPAWNED
//! (`Proc::Child`). Stopping ADOPTED survivors, the admin-socket edge stop
//! (`proxy::stop_edge` / `recover_stale_edge`), and the marker-gated orphan
//! sweep (`stop_stale_owned`) are refused unless this process is the app
//! ([`mark_app_process`]), opted in explicitly ([`allow_real_stack_control`]),
//! or [`ALLOW_ENV`] is set. Drop paths were already safe (`Proc::terminate` /
//! `kill` are no-ops for `Proc::Adopted`).

use std::sync::atomic::{AtomicBool, Ordering};

static APP_PROCESS: AtomicBool = AtomicBool::new(false);
static EXPLICIT_ALLOW: AtomicBool = AtomicBool::new(false);

/// Env override for ad-hoc runs of a guarded example against the real stack:
/// `REXENV_CONTROL_REAL_STACK=1 cargo run --example …`.
pub const ALLOW_ENV: &str = "REXENV_CONTROL_REAL_STACK";

/// Called once from the app's entry point (`lib::run`). The interactive app is
/// the ONE process whose Stop-all must reach adopted survivors — services
/// outlive the app by design, so a relaunch adopts them and the user still
/// expects Stop-all to stop them.
pub fn mark_app_process() {
    APP_PROCESS.store(true, Ordering::SeqCst);
}

/// Opt-in for utilities whose PURPOSE is controlling the real stack (e.g.
/// `examples/stack_stop`, the edge recovery/manager demos). Call at the top of
/// `main` so the intent is visible in the example's source.
pub fn allow_real_stack_control() {
    EXPLICIT_ALLOW.store(true, Ordering::SeqCst);
}

/// Whether this process may stop stack pieces it didn't spawn. Always true
/// under `cfg(test)`: unit tests exercise stop paths against the mock
/// platform and must not trip the guard.
pub fn may_control_real_stack() -> bool {
    cfg!(test) || explicitly_allowed()
}

fn explicitly_allowed() -> bool {
    APP_PROCESS.load(Ordering::SeqCst)
        || EXPLICIT_ALLOW.load(Ordering::SeqCst)
        || std::env::var(ALLOW_ENV).map(|v| v == "1").unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    // One test for the whole flag sequence: the statics are process-wide, so
    // the default-deny assertion must run before anything flips them.
    #[test]
    fn guard_denies_by_default_and_opens_on_explicit_allow() {
        assert!(
            !explicitly_allowed(),
            "a fresh non-app process must not control the real stack"
        );
        // Unit tests always pass the public check regardless of the flags.
        assert!(may_control_real_stack());
        allow_real_stack_control();
        assert!(explicitly_allowed());
    }
}

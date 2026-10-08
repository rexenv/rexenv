//! core::stack_guard — protects the user's RUNNING stack from non-app processes.
//!
//! Live-check examples link this lib and deliberately run against the REAL
//! app-data dir (the shared binary cache and admin socket are the point of a
//! live check) — which historically let an example's `stop_all` /
//! `recover_stale_edge` tear down the user's serving stack over the shared
//! admin socket (docs/archive/SHIPPED-2026-07.md, "Isolate live-check examples").
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

/// Why Stop all (the footer, the tray, `rex stop`, and so `rex restart`) must not run now: a site is
/// still being provisioned, and its install or migration is talking to the database this would stop.
/// A user's `rex restart` mid-provision killed MySQL under `artisan migrate` — `1317 Query execution
/// was interrupted`, then `1050 Table … already exists` on every Retry (8 Oct 2026). `None` = go.
pub fn stop_refusal(provisioning: &[String]) -> Option<String> {
    let names = match provisioning {
        [] => return None,
        [one] => one.clone(),
        many => many.join(", "),
    };
    let is = if provisioning.len() == 1 { "is" } else { "are" };
    Some(format!(
        "Nothing was stopped: {names} {is} still being set up, and stopping the stack now would cut \
         off its install or migrations mid-way — a half-applied migration does not undo itself. \
         Wait for it to finish, or Cancel it on its card, then Stop all."
    ))
}

#[cfg(test)]
mod tests {
    /// Ledger #795: nothing running → go; a site being provisioned → refuse, naming it and the way out.
    #[test]
    fn stop_is_refused_while_a_site_is_being_provisioned() {
        assert_eq!(super::stop_refusal(&[]), None);
        let one = super::stop_refusal(&["shop.rex".into()]).expect("refused");
        assert!(one.starts_with("Nothing was stopped: shop.rex is still being set up"), "{one}");
        assert!(one.contains("Cancel it on its card"), "{one}");
        let two = super::stop_refusal(&["a.rex".into(), "b.rex".into()]).expect("refused");
        assert!(two.contains("a.rex, b.rex are still being set up"), "{two}");
    }

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

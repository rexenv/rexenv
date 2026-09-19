//! Windows: the PURE half of starting outside a confining job — the decision over
//! two facts, with no Windows API in it, so the macOS test host runs these tests
//! too (`platform/mod.rs` includes this file under `cfg(test)` there, like
//! `app_bundle_rules.rs`).
//!
//! # The fact this exists for
//!
//! Every service rexenv starts is spawned with `CREATE_BREAKAWAY_FROM_JOB`, so it
//! outlives the app (ledger #600). A job whose limits carry neither
//! `JOB_OBJECT_LIMIT_BREAKAWAY_OK` nor `JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK`
//! refuses that flag, and `CreateProcess` answers ERROR_ACCESS_DENIED — naming
//! nothing. **Measured 19 Sep 2026 on the first installed copy** (clean Windows 11
//! 24H2 VM): the NSIS installer's Finish page ran rexenv (`RunMainBinary` →
//! `nsis_tauri_utils::RunAsUser`) from the Edge → `setup.exe` chain, and inside
//! that job "Start all" failed on the very first service — `Windows refused to
//! start …\mysql-8.4.6\bin\mysqld (access denied)`. The same copy started from
//! Explorer spawned all five. Measured limits of the launch contexts on that
//! machine (`QueryInformationJobObject(NULL, …)`): an SSH shell `0x2800`
//! (KILL_ON_JOB_CLOSE | BREAKAWAY_OK — services can leave), a scheduled task
//! `0x0` (confining), an Explorer launch — no job at all there; on the Windows 10
//! Dell the shell's child sits in a job with `0x800` (BREAKAWAY_OK). Either way it
//! is not confining, which is all the hop needs.
//!
//! So a confined app hops ONCE: it asks the running Explorer to open its own
//! executable (`explorer.exe "<exe>"` — the shell starts it, not us, so the new
//! process is outside our job), and exits. `should_hop` is the whole rule.

/// `JOB_OBJECT_LIMIT_BREAKAWAY_OK`, as `windows_sys` names it.
pub(crate) const BREAKAWAY_OK: u32 = 0x800;
/// `JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK`.
pub(crate) const SILENT_BREAKAWAY_OK: u32 = 0x1000;

/// The image name of the process that hands a hop its new life.
pub(crate) const SHELL_EXE: &str = "explorer.exe";

/// Whether limits of the job this process is in (`None` = no job) forbid a child
/// from breaking away.
pub(crate) fn confining(limits: Option<u32>) -> bool {
    limits.is_some_and(|flags| flags & (BREAKAWAY_OK | SILENT_BREAKAWAY_OK) == 0)
}

/// Hop exactly once. A process Explorer started that is STILL confined has nowhere
/// better to go — hopping again would loop — so it runs in place and lets the
/// service spawn name the problem (`WindowsSupervisor::spawn`'s access-denied text).
pub(crate) fn should_hop(confined: bool, parent_exe: Option<&str>) -> bool {
    confined && !parent_exe.is_some_and(|p| p.eq_ignore_ascii_case(SHELL_EXE))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three measured contexts, and the two the measurement implies.
    #[test]
    fn the_measured_launch_contexts_decide_as_measured() {
        assert!(!confining(None), "an Explorer launch is in no job — nothing to leave");
        assert!(!confining(Some(0x2800)), "the SSH shell's job lets a child break away");
        assert!(confining(Some(0x0)), "a scheduled task's job forbids it — and so did the installer's");
        assert!(!confining(Some(SILENT_BREAKAWAY_OK)), "silent breakaway is a permission too");
        assert!(confining(Some(0x2000)), "kill-on-close alone still refuses the flag");
    }

    #[test]
    fn a_confined_process_hops_once_and_only_from_a_launcher_that_is_not_the_shell() {
        assert!(should_hop(true, Some("rexenv_0.7.0_x64-setup.exe")), "the installer's Finish page");
        assert!(should_hop(true, Some("msedge.exe")));
        assert!(should_hop(true, None), "a parent that is already gone is still not the shell");
        assert!(!should_hop(true, Some("explorer.exe")), "Explorer started it and it is STILL confined: run in place, no loop");
        assert!(!should_hop(true, Some("Explorer.EXE")), "the shell's own casing");
        assert!(!should_hop(false, Some("msedge.exe")), "not confined: nothing to do, whoever started it");
    }
}

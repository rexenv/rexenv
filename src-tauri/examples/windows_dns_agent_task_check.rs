//! W6 S2, ledger #616: the DNS agent as a logon Scheduled Task on Windows —
//! `DnsAgentManager::{install, is_installed, kickstart, uninstall}` — registered, run and kept alive by
//! Task Scheduler with no elevation, logging to the file its definition names.
//!
//! ```text
//! (desktop session) scripts/probes/windows-dns-agent-task.ps1 through windows-limited-token.sh
//! ```
//!
//! This example is its own agent: re-run as `--dns-agent --log <path>` it does what `main.rs` does
//! (`platform::send_output_to`, then `core::dns::run_agent`), so the task it installs points at itself.
//!
//! 1. Nothing of the real agent exists first — no `\rexenv\dns-agent` task, no definition file, no
//!    rexenv resolver answering :53 — or the check refuses to run: it registers the REAL task name and
//!    writes the REAL definition path, so it may only run where neither is in use.
//! 2. `install` → the task exists; the agent answers on 127.0.0.1:53, names this build, and its log file
//!    carries its bind line (the output a task gives no stream for).
//! 3. `install` again with the same definition → Ok, and the same agent process still runs (no churn on
//!    every app launch).
//! 4. `kickstart` → a NEW agent process answers.
//! 5. The agent killed → the per-minute trigger (the owner's keep-alive ruling) brings one back; the time
//!    it took is printed.
//! 6. `uninstall` → no task, no agent, no definition file, nothing answering :53.
//!
//! Fixture-owned: the log lives under `%TEMP%\rexenv dns agent task` (a SPACE in it), removed at the
//! end; a guard ends and deletes the task and kills any agent process this example started if a step
//! panics. `demo` tier: Windows-only.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_dns_agent_task_check: skipped — a Windows check (ledger #616, W6)");
}

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    if argv.iter().any(|a| a == "--dns-agent") {
        if let Some(log) = argv.iter().position(|a| a == "--log").and_then(|i| argv.get(i + 1)) {
            rexenv_lib::platform::send_output_to(std::path::Path::new(log));
        }
        std::process::exit(rexenv_lib::core::dns::run_agent());
    }
    windows::main()
}

#[cfg(target_os = "windows")]
mod windows {
    use super::common::Check;
    use rexenv_lib::core::dns;
    use rexenv_lib::platform::traits::Platform;
    use std::path::{Path, PathBuf};
    use std::process::{Command, ExitCode};
    use std::time::{Duration, Instant};

    const TASK: &str = r"\rexenv\dns-agent";

    fn task_exists() -> bool {
        Command::new("schtasks").args(["/Query", "/TN", TASK]).output().is_ok_and(|o| o.status.success())
    }

    /// Agent processes: this example's image with `--dns-agent` on its command line.
    fn agent_pids(plat: &dyn Platform, exe: &Path) -> Vec<u32> {
        let me = std::process::id();
        plat.supervisor()
            .owned_pids(&exe.display().to_string())
            .into_iter()
            .filter(|&pid| pid != me && plat.supervisor().pid_command(pid).is_some_and(|c| c.contains("--dns-agent")))
            .collect()
    }

    fn wait(mut ok: impl FnMut() -> bool, within: Duration) -> Option<Duration> {
        let t = Instant::now();
        while t.elapsed() < within {
            if ok() {
                return Some(t.elapsed());
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        ok().then(|| t.elapsed())
    }

    /// Ends and deletes the task and kills this example's agents if the run did not get to `uninstall`.
    struct Guard {
        exe: PathBuf,
        definition: Option<PathBuf>,
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            if task_exists() {
                println!("  · teardown: the task is still registered — ending and deleting it");
                let _ = Command::new("schtasks").args(["/End", "/TN", TASK]).output();
                let _ = Command::new("schtasks").args(["/Delete", "/TN", TASK, "/F"]).output();
            }
            let plat = rexenv_lib::platform::current();
            for pid in agent_pids(&*plat, &self.exe) {
                println!("  · teardown: killing agent pid {pid}");
                let _ = Command::new("taskkill").args(["/F", "/PID", &pid.to_string()]).output();
            }
            if let Some(d) = &self.definition {
                let _ = std::fs::remove_file(d);
            }
        }
    }

    pub fn main() -> ExitCode {
        let mut check = Check::new("windows_dns_agent_task_check");
        let plat = rexenv_lib::platform::current();
        let agent = plat.dns_agent();
        let exe = std::env::current_exe().expect("exe");
        let fixture = std::env::temp_dir().join("rexenv dns agent task");
        let _ = std::fs::remove_dir_all(&fixture);
        let log = fixture.join("dns-agent.log");
        let definition = agent.definition_path().ok();

        // ── 1. Nothing of the real agent exists. ──
        let clean = !task_exists() && definition.as_ref().is_some_and(|d| !d.exists()) && !dns::answers_as_ours(53);
        check.is(
            "no \\rexenv\\dns-agent task, no definition file, no rexenv resolver on :53 before the check",
            clean,
            &format!("task {}, definition {:?}, answering {}", task_exists(), definition, dns::answers_as_ours(53)),
        );
        if !clean {
            return check.verdict();
        }
        let guard = Guard { exe: exe.clone(), definition: definition.clone() };

        // ── 2. Install. ──
        let installed = agent.install(&exe, &log);
        check.is("install returns Ok (no elevation)", installed.is_ok(), &format!("{installed:?}"));
        check.is("the task is registered (is_installed)", agent.is_installed(), "");
        let up = wait(|| dns::answers_as_ours(53), Duration::from_secs(45));
        println!("  · agent answering after {up:?}");
        check.is("the task's agent answers on 127.0.0.1:53", up.is_some(), "no answer within 45 s");
        let identity = dns::agent_build_identity(53);
        check.is("the agent names this build", identity.as_deref() == Some(dns::build_identity().as_str()), &format!("{identity:?}"));
        std::thread::sleep(Duration::from_secs(1));
        let logged = std::fs::read_to_string(&log).unwrap_or_default();
        println!("  · log: {}", logged.lines().collect::<Vec<_>>().join(" | "));
        check.is("the agent's log file carries its bind line (--log)", logged.contains("listening on 127.0.0.1:53"), &logged);
        let first = agent_pids(&*plat, &exe);
        println!("  · agent pids: {first:?}");
        check.is("exactly one agent runs", first.len() == 1, &format!("{first:?}"));

        // ── 3. Install again, unchanged. ──
        let again = agent.install(&exe, &log);
        std::thread::sleep(Duration::from_secs(2));
        let after_again = agent_pids(&*plat, &exe);
        check.is("a second install with the same definition is Ok and leaves the same agent running", again.is_ok() && after_again == first, &format!("{again:?}, pids {after_again:?}"));

        // ── 4. Kickstart. ──
        let kicked = agent.kickstart();
        check.is("kickstart returns Ok", kicked.is_ok(), &format!("{kicked:?}"));
        let restarted = wait(|| dns::answers_as_ours(53) && agent_pids(&*plat, &exe).iter().any(|p| !first.contains(p)), Duration::from_secs(45));
        let second = agent_pids(&*plat, &exe);
        println!("  · after kickstart ({restarted:?}): pids {second:?}");
        check.is("after kickstart a new agent process answers", restarted.is_some() && second.len() == 1, &format!("{second:?}"));

        // ── 5. Killed: the per-minute trigger brings it back. ──
        for pid in &second {
            let _ = Command::new("taskkill").args(["/F", "/PID", &pid.to_string()]).output();
        }
        let down = wait(|| !dns::answers_as_ours(53), Duration::from_secs(10));
        check.is("the killed agent stops answering", down.is_some(), "still answering");
        let back = wait(|| dns::answers_as_ours(53), Duration::from_secs(100));
        println!("  · back after the kill: {back:?}");
        check.is("the per-minute trigger brings a killed agent back (within 100 s)", back.is_some(), "not back within 100 s");

        // ── 6. Uninstall. ──
        let removed = agent.uninstall();
        check.is("uninstall returns Ok", removed.is_ok(), &format!("{removed:?}"));
        check.is("the task is gone", !agent.is_installed() && !task_exists(), "still registered");
        let quiet = wait(|| !dns::answers_as_ours(53), Duration::from_secs(10));
        check.is("nothing answers on :53 after uninstall", quiet.is_some(), "still answering");
        check.is("no agent process is left", agent_pids(&*plat, &exe).is_empty(), "");
        check.is("the definition file is removed", definition.as_ref().is_some_and(|d| !d.exists()), &format!("{definition:?}"));

        drop(guard);
        if let Err(e) = std::fs::remove_dir_all(&fixture) {
            println!("  · fixture not fully removed: {e}");
        }
        check.verdict()
    }
}

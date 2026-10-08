//! A program Windows' loader cannot start must END with a code rexenv can name — never sit behind
//! a dialog nobody sees while rexenv waits on it (user report, 8 Oct 2026: no Visual C++ runtime,
//! `mysqld --initialize-insecure` stopped on "The code execution cannot proceed because
//! VCRUNTIME140.dll was not found" and provisioning hung).
//!
//! ```text
//! (in the signed-in desktop session — a schtasks /IT task; over plain SSH no dialog can appear)
//! .\windows_loader_dialog_check.exe
//! ```
//!
//! The fixture is a real binary with a real missing DLL: `php-cgi.exe` copied ALONE into an empty
//! folder cannot find its `php8.dll`, which fails in the loader exactly as a missing
//! `VCRUNTIME140.dll` does (`0xC0000135`, STATUS_DLL_NOT_FOUND). What it proves there:
//! 1. the CONTROL — spawned with Windows' default error mode, the copy is still alive after 5 s
//!    (the dialog holds it). This is what makes the next step mean anything: in a session where no
//!    dialog can appear, both would exit at once and the check says so instead of passing;
//! 2. after `platform::quiet_loader_dialogs_before_boot()` — the call `main.rs` makes — the same
//!    copy spawned through the platform's service spawn exits within 10 s with `-1073741515`;
//! 3. `core::proc::exit_text` names that code with the Visual C++ runtime and Microsoft's link.
//!
//! Fixture-owned: `%TEMP%\rexenv-loader-check` (removed at the end; the control is killed), the
//! real binary cache read-only. `demo` tier: Windows, and the desktop session.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_loader_dialog_check: skipped — a Windows live check");
}

#[cfg(target_os = "windows")]
#[tokio::main]
async fn main() -> std::process::ExitCode {
    windows::main().await
}

#[cfg(target_os = "windows")]
mod windows {
    use super::common::{self, Check};
    use rexenv_lib::core::{binaries, proc};
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command, ExitCode};
    use std::time::{Duration, Instant};

    /// The child's exit code if it ended within `limit`, else `None` (still running).
    fn ended_within(child: &mut Child, limit: Duration) -> Option<Option<i32>> {
        let start = Instant::now();
        while start.elapsed() < limit {
            if let Ok(Some(status)) = child.try_wait() {
                return Some(status.code());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        None
    }

    pub async fn main() -> ExitCode {
        let mut check = Check::new("windows_loader_dialog_check");
        let root = std::env::temp_dir().join("rexenv-loader-check");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let plat = common::sandbox_platform_at(root.join("app"));

        let tree = binaries::resolve_dir(&*plat, "php", binaries::pins().php).await.expect("php tree");
        let lone = root.join("php-cgi.exe");
        std::fs::copy(tree.join("php-cgi.exe"), &lone).expect("copy php-cgi.exe");

        // 1. The control: Windows' default error mode, as before the fix.
        const CREATE_DEFAULT_ERROR_MODE: u32 = 0x0400_0000;
        let mut control = Command::new(&lone).arg("-v").creation_flags(CREATE_DEFAULT_ERROR_MODE).spawn().expect("spawn control");
        let held = ended_within(&mut control, Duration::from_secs(5));
        let _ = control.kill();
        let _ = control.wait();
        check.is(
            "control: with the default error mode the loader's dialog holds the child (desktop session)",
            held.is_none(),
            &format!("it ended on its own with {held:?} — no dialog can appear in this session, so step 2 proves nothing here; run it from a schtasks /IT task"),
        );

        // 2. The fix: the call main.rs makes, then the platform's own service spawn.
        rexenv_lib::platform::quiet_loader_dialogs_before_boot();
        let mut child = plat.supervisor().spawn(&lone, &["-v".to_string()]).expect("spawn");
        let ended = ended_within(&mut child, Duration::from_secs(10));
        if ended.is_none() {
            plat.supervisor().terminate_child(&mut child);
        }
        check.is(
            "with rexenv's error mode the child exits at once with 0xC0000135",
            ended == Some(Some(-1073741515)),
            &format!("{ended:?}"),
        );

        // 3. What the user reads.
        let said = proc::exit_text(Some(-1073741515));
        check.is(
            "the exit is named: the Visual C++ runtime and Microsoft's link",
            said.contains("Visual C++ Redistributable") && said.contains("https://aka.ms/vs/17/release/vc_redist.x64.exe"),
            &said,
        );

        let _ = std::fs::remove_dir_all(&root);
        check.verdict()
    }
}

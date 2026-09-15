//! W7 S4, ledger #623: Windows' `AutostartManager` through the real platform, against the REAL per-user Run
//! key — read back with `reg.exe`, not with the code under test.
//!
//! ```text
//! scripts/probes/windows-example.sh <host> windows_autostart_check      # SSH, the desktop user's account
//! ```
//!
//! HKCU needs no elevation and the SSH session loads the same user's hive, so no desktop session is needed.
//! In order: nothing enabled first; `enable` writes `"<this exe>" --hidden`; a Task Manager "Disabled"
//! (`StartupApproved\Run`, first byte 03 — written here as Task Manager writes it) makes `is_enabled` false;
//! `refresh` touches neither the unchanged Run value nor that disable; a second `enable` clears the disable;
//! a Run value naming another program that exists is rewritten by `refresh`; `disable` removes both values.
//!
//! The value name is the product's, so the check REFUSES to start when either value already exists — it will
//! not overwrite a real login item — and a guard removes both values on every path. The sign-in itself (the
//! app started by Explorer from this value, hidden) is the real app's run with a person signing out and in.
//! `demo` tier: Windows-only.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_autostart_check: skipped — a Windows check (ledger #623, W7)");
}

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    windows::main()
}

#[cfg(target_os = "windows")]
mod windows {
    use super::common::Check;
    use std::process::{Command, ExitCode};

    const RUN: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const APPROVED: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

    /// The data of value `rexenv` under `key` as `reg query` prints it, or `None` when absent.
    fn reg_value(key: &str) -> Option<String> {
        let out = Command::new("reg").args(["query", key, "/v", "rexenv"]).output().ok()?;
        if !out.status.success() {
            return None;
        }
        String::from_utf8_lossy(&out.stdout).lines().find_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("rexenv")?.trim_start();
            let (_, data) = rest.split_once("REG_")?;
            Some(data.split_once(char::is_whitespace).map(|(_, d)| d.trim().to_string()).unwrap_or_default())
        })
    }

    fn reg(args: &[&str]) -> bool {
        Command::new("reg").args(args).output().is_ok_and(|o| o.status.success())
    }

    /// Removes both values on every path out of the check.
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = reg(&["delete", RUN, "/v", "rexenv", "/f"]);
            let _ = reg(&["delete", APPROVED, "/v", "rexenv", "/f"]);
            println!(
                "  · guard: Run value left: {}, StartupApproved value left: {}",
                reg_value(RUN).is_some(),
                reg_value(APPROVED).is_some()
            );
        }
    }

    pub fn main() -> ExitCode {
        let mut check = Check::new("windows_autostart_check");
        let clean = reg_value(RUN).is_none() && reg_value(APPROVED).is_none();
        check.is("no rexenv login item exists before the check (a real one is never overwritten)", clean,
            &format!("Run: {:?}, StartupApproved: {:?}", reg_value(RUN), reg_value(APPROVED)));
        if !clean {
            return check.verdict();
        }
        let _guard = Guard;
        let plat = rexenv_lib::platform::current();
        let autostart = plat.autostart();
        let exe = std::env::current_exe().expect("own path");
        let want = format!("\"{}\" --hidden", exe.display());

        check.is("is_enabled is false with no Run value", autostart.is_enabled().ok() == Some(false), &format!("{:?}", autostart.is_enabled()));

        let enabled = autostart.enable();
        check.is("enable writes \"<exe>\" --hidden to the per-user Run key", enabled.is_ok() && reg_value(RUN).as_deref() == Some(want.as_str()),
            &format!("{enabled:?}, {:?}", reg_value(RUN)));
        check.is("…and is_enabled is true", autostart.is_enabled().ok() == Some(true), &format!("{:?}", autostart.is_enabled()));

        // Task Manager's Startup tab: disabled, the Run value untouched.
        let wrote = reg(&["add", APPROVED, "/v", "rexenv", "/t", "REG_BINARY", "/d", "030000005A1B6C7D8E9FA0B1", "/f"]);
        check.is("with Task Manager's Disabled (03…), is_enabled is false", wrote && autostart.is_enabled().ok() == Some(false),
            &format!("wrote {wrote}, {:?}", autostart.is_enabled()));

        let refreshed = autostart.refresh();
        check.is("refresh leaves an unchanged Run value and Task Manager's Disabled alone",
            refreshed.is_ok() && reg_value(RUN).as_deref() == Some(want.as_str()) && reg_value(APPROVED).is_some_and(|v| v.starts_with("03")),
            &format!("{refreshed:?}, Run {:?}, StartupApproved {:?}", reg_value(RUN), reg_value(APPROVED)));

        let again = autostart.enable();
        check.is("enable again clears the Disabled and is_enabled is true",
            again.is_ok() && reg_value(APPROVED).is_none() && autostart.is_enabled().ok() == Some(true),
            &format!("{again:?}, StartupApproved {:?}, {:?}", reg_value(APPROVED), autostart.is_enabled()));

        // A value naming another program that exists (an older install): an installed build rewrites it.
        let other = r#""C:\Windows\System32\notepad.exe" --hidden"#;
        let moved = reg(&["add", RUN, "/v", "rexenv", "/t", "REG_SZ", "/d", other, "/f"]);
        let refreshed = autostart.refresh();
        check.is("refresh rewrites a value naming another program to this build",
            moved && refreshed.is_ok() && reg_value(RUN).as_deref() == Some(want.as_str()),
            &format!("wrote {moved}, {refreshed:?}, {:?}", reg_value(RUN)));

        let disabled = autostart.disable();
        check.is("disable removes the Run value, and is_enabled is false",
            disabled.is_ok() && reg_value(RUN).is_none() && autostart.is_enabled().ok() == Some(false),
            &format!("{disabled:?}, {:?}", reg_value(RUN)));
        check.verdict()
    }
}

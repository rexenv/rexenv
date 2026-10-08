//! The Visual C++ runtime verdict (`BinaryProvider::runtime_problem`, #798) agrees with what the
//! machine actually does: a PC that runs the pinned PHP is never told it cannot, and a PC where PHP
//! dies `0xC0000135` gets the BLOCKING verdict.
//!
//! ```text
//! ssh dell@<host> .\windows_runtime_check.exe
//! ```
//!
//! What it proves there: the registry/System32 reader's answer, printed, against a real `php.exe -v`
//! of the pinned tree. Run it on a PC with the redistributable (the consistent-positive case) AND on
//! one without (SMOKE §Windows "No Visual C++ runtime") — the same check judges both.
//!
//! Fixture-owned: a sandboxed platform under `%TEMP%\rexenv-runtime-check` (removed), the real
//! binary cache read-only. `demo` tier: Windows.

#[cfg(target_os = "windows")]
mod common;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_runtime_check: skipped — a Windows live check");
}

#[cfg(target_os = "windows")]
#[tokio::main]
async fn main() -> std::process::ExitCode {
    windows::main().await
}

#[cfg(target_os = "windows")]
mod windows {
    use super::common::{self, Check};
    use rexenv_lib::core::binaries;
    use std::process::{Command, ExitCode};

    pub async fn main() -> ExitCode {
        let mut check = Check::new("windows_runtime_check");
        let root = std::env::temp_dir().join("rexenv-runtime-check");
        let _ = std::fs::remove_dir_all(&root);
        let plat = common::sandbox_platform_at(root.clone());
        rexenv_lib::platform::quiet_loader_dialogs_before_boot();

        let verdict = plat.binaries().runtime_problem();
        println!("  · verdict: {verdict:?}");
        let php = binaries::resolve_program(&*plat, "php", binaries::pins().php).await.expect("php.exe");
        let out = Command::new(&php).arg("-v").output().expect("run php -v");
        let code = out.status.code();
        println!("  · php -v exit {code:?}");
        if out.status.success() {
            check.is(
                "PHP runs here, and the verdict does not block",
                !verdict.as_ref().is_some_and(|p| p.blocking),
                &format!("{verdict:?}"),
            );
        } else if code == Some(-1073741515) {
            check.is(
                "PHP cannot load its runtime here, and the verdict BLOCKS with Microsoft's link",
                verdict.as_ref().is_some_and(|p| p.blocking && p.url.starts_with("https://aka.ms/")),
                &format!("{verdict:?}"),
            );
        } else {
            check.is("php -v either ran or failed in the loader", false, &format!("exit {code:?}: {}", String::from_utf8_lossy(&out.stderr)));
        }
        let _ = std::fs::remove_dir_all(&root);
        check.verdict()
    }
}

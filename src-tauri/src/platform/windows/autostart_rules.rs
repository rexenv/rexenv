//! The pure half of Windows' "Open rexenv at login" (W7 S4, plan §5 W7, ledger #623): the Run value, what
//! Task Manager's "Disabled" looks like, what a dev build is, and what a launch-time refresh does. No Win32
//! here — `WindowsAutostart` in `mod.rs` reads and writes the registry — so this is compiled into the macOS
//! test build.
//!
//! The login item is the per-user Run key (plan §3 D1: rexenv must not be LAUNCHED by a scheduled task,
//! whose job refuses the breakaway its services need — measured). The rules are macOS's (`MacosAutostart`):
//! `--hidden` is always on the command line (the flag is a request; `lib.rs` still shows the window when
//! setup is incomplete); `enable` is the user's explicit choice and points at THIS build; a launch-time
//! `refresh` rewrites only a changed value and never re-points the item at a dev build while the recorded
//! program still exists.
//!
//! Task Manager's Startup tab disables an item without touching the Run key: it writes a binary value of the
//! same name under `Explorer\StartupApproved\Run` whose first byte has its low bit set. So "enabled" is the
//! Run value present AND not disabled there; `enable` clears that value (the user turned it on in rexenv,
//! which overrules an older "Disabled"); `refresh` never touches it.

use std::path::{Path, PathBuf};

/// Under `HKEY_CURRENT_USER`.
pub(crate) const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Under `HKEY_CURRENT_USER`: Task Manager's enabled/disabled state for the Run key's items.
pub(crate) const APPROVED_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
/// The value name in both keys.
pub(crate) const VALUE_NAME: &str = "rexenv";

/// The Run value for `exe`: the quoted program, then the hidden-launch flag.
pub(crate) fn run_value(exe: &Path) -> String {
    format!("\"{}\" {}", exe.display(), crate::HIDDEN_LAUNCH_FLAG)
}

/// The program a Run value starts: its quoted first token, or its first token when unquoted. `None` when
/// there is none.
pub(crate) fn program_in(value: &str) -> Option<PathBuf> {
    let v = value.trim();
    let program = match v.strip_prefix('"') {
        Some(rest) => rest.split('"').next()?,
        None => v.split_whitespace().next()?,
    };
    (!program.is_empty()).then(|| PathBuf::from(program))
}

/// Whether Task Manager's Startup tab has disabled the item: a `StartupApproved\Run` value whose first byte
/// has its low bit set (02 enabled, 03 disabled). No value, or an empty one, is not disabled.
pub(crate) fn disabled_by_user(approved: Option<&[u8]>) -> bool {
    approved.and_then(|b| b.first()).is_some_and(|first| first & 1 == 1)
}

/// Whether `exe` is a build out of a cargo `target` folder — the binary the next `cargo clean` deletes.
pub(crate) fn is_dev_build(exe: &Path) -> bool {
    let p = exe.to_string_lossy().replace('/', "\\").to_ascii_lowercase();
    ["\\target\\debug\\", "\\target\\release\\", "\\target\\xwin\\", "\\target\\x86_64-pc-windows-msvc\\", "\\target\\aarch64-pc-windows-msvc\\"]
        .iter()
        .any(|dir| p.contains(dir))
}

/// What a launch-time refresh does.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Refresh {
    /// The value already says exactly this.
    Unchanged,
    /// This launch is a dev build and the recorded program still exists: leave the item on it.
    KeepRecorded,
    /// Write this build's value.
    Write,
}

/// The refresh decision, given the Run value as it is (`have`) and this build's executable.
pub(crate) fn refresh(have: Option<&str>, exe: &Path, exists: &dyn Fn(&Path) -> bool) -> Refresh {
    if have == Some(run_value(exe).as_str()) {
        return Refresh::Unchanged;
    }
    if is_dev_build(exe) && have.and_then(program_in).is_some_and(|recorded| exists(&recorded)) {
        return Refresh::KeepRecorded;
    }
    Refresh::Write
}

#[cfg(test)]
mod tests {
    use super::*;

    const INSTALLED: &str = r"C:\Users\A B\AppData\Local\Programs\rexenv\rexenv.exe";
    const DEV: &str = r"C:\Users\A B\code\rexenv\src-tauri\target\debug\rexenv.exe";

    #[test]
    fn the_run_value_quotes_the_program_and_asks_for_a_hidden_launch() {
        let value = run_value(Path::new(INSTALLED));
        assert_eq!(value, format!("\"{INSTALLED}\" --hidden"));
        assert_eq!(program_in(&value).as_deref(), Some(Path::new(INSTALLED)));
        assert_eq!(program_in(r"C:\x\old.exe --hidden").as_deref(), Some(Path::new(r"C:\x\old.exe")));
        assert_eq!(program_in("  "), None);
    }

    /// Ledger #623 — Task Manager's "Disabled" is read, not overlooked.
    #[test]
    fn task_managers_disabled_is_the_low_bit_of_the_first_byte() {
        assert!(!disabled_by_user(None));
        assert!(!disabled_by_user(Some(&[])));
        assert!(!disabled_by_user(Some(&[0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])));
        assert!(disabled_by_user(Some(&[0x03, 0, 0, 0, 0x5a, 0x1b, 0, 0, 0, 0, 0, 0])));
        assert!(disabled_by_user(Some(&[0x07])));
    }

    /// Ledger #623 — a refresh rewrites only a changed value, and a dev build never takes the item from a
    /// program that still exists.
    #[test]
    fn a_refresh_never_moves_the_item_to_a_dev_build_while_the_recorded_program_exists() {
        let installed = run_value(Path::new(INSTALLED));
        let everything = |_: &Path| true;
        let nothing = |_: &Path| false;
        assert_eq!(refresh(Some(&installed), Path::new(INSTALLED), &everything), Refresh::Unchanged);
        assert_eq!(refresh(Some(&installed), Path::new(DEV), &everything), Refresh::KeepRecorded);
        // The recorded program is gone (moved, deleted): the dev build takes it rather than a dead item.
        assert_eq!(refresh(Some(&installed), Path::new(DEV), &nothing), Refresh::Write);
        // An older value without --hidden, or another install location: an installed build rewrites it.
        assert_eq!(refresh(Some(&format!("\"{INSTALLED}\"")), Path::new(INSTALLED), &everything), Refresh::Write);
        assert_eq!(refresh(Some(r#""D:\old\rexenv.exe" --hidden"#), Path::new(INSTALLED), &everything), Refresh::Write);
        assert!(is_dev_build(Path::new("C:/code/rexenv/src-tauri/target/xwin/x86_64-pc-windows-msvc/debug/rexenv.exe")));
        assert!(!is_dev_build(Path::new(r"C:\Users\DELL\rexenv-s5\rexenv.exe")));
    }
}

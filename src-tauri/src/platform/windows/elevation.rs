//! Windows' privileged step (W6 S3, ledger #619) — the Win32 half of `elevation_rules.rs`.
//!
//! The app side (`run_privileged`): check the ops, show rexenv's own dialog with the reason, start
//! `rexenv.exe --elevated-step <ops> <result>` through `ShellExecuteExW` with the `runas` verb (UAC names
//! the program it elevates — rexenv), wait for it, read its result file. The elevated side (`run_step`):
//! check the result path and the ops again, build the PowerShell from rexenv's own builders, run it, write
//! the result.

use super::{elevation_rules as rules, nrpt_rules};
use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// rexenv's own dialog before UAC: the reason and OK/Cancel. `true` for OK.
fn ask_first(text: &str) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDOK, MB_ICONINFORMATION, MB_OKCANCEL, MB_SETFOREGROUND, MB_TOPMOST,
    };
    let (body, caption) = (wide(text), wide("rexenv"));
    // SAFETY: both pointers are NUL-terminated UTF-16 buffers that outlive the call; no owner window.
    let answer = unsafe {
        MessageBoxW(std::ptr::null_mut(), body.as_ptr(), caption.as_ptr(), MB_OKCANCEL | MB_ICONINFORMATION | MB_SETFOREGROUND | MB_TOPMOST)
    };
    answer == IDOK
}

/// Where rexenv keeps its resolver backups — the ONLY place a restore reads from, computed here rather
/// than taken from the caller.
fn backup_dir() -> Result<PathBuf> {
    use crate::platform::traits::Paths;
    Ok(super::WindowsPaths.app_data_dir()?.join("resolver-backups"))
}

/// The app side: dialog, UAC, wait, result.
pub(crate) fn run_privileged(ops: &str, sentence: &str) -> Result<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SEE_MASK_NO_CONSOLE, SHELLEXECUTEINFOW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    // Refuse what the step would refuse, before any dialog is shown.
    nrpt_rules::parse_ops(ops).map_err(Error::Other)?;
    if !ask_first(&rules::before_uac_text(sentence)) {
        return Err(rules::cancelled());
    }
    let exe = std::env::current_exe()?;
    let result = std::env::temp_dir().join(format!(
        "{}{}-{}.txt",
        rules::RESULT_PREFIX,
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
    ));
    let _ = std::fs::remove_file(&result);
    let (verb, file, params) = (wide("runas"), wide(&exe.display().to_string()), wide(&rules::step_parameters(ops, &result)));
    // SAFETY: a zeroed SHELLEXECUTEINFOW is valid; every pointer set below outlives the call.
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NO_CONSOLE;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = params.as_ptr();
    info.nShow = SW_HIDE;
    // SAFETY: `info` is fully initialised for the call.
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        // SAFETY: a plain call, straight after the failure.
        let code = unsafe { GetLastError() };
        if code == rules::ERROR_CANCELLED {
            return Err(rules::cancelled());
        }
        return Err(Error::Other(format!("Windows could not start rexenv's administrator step (error {code})")));
    }
    let process = info.hProcess;
    if process.is_null() {
        return Err(Error::Other("rexenv's administrator step started without a process to wait for".into()));
    }
    // SAFETY: `process` is the handle ShellExecuteExW returned; closed once below.
    let waited = unsafe { WaitForSingleObject(process, INFINITE) };
    let mut exit = 0u32;
    // SAFETY: as above.
    unsafe {
        GetExitCodeProcess(process, &mut exit);
        CloseHandle(process);
    }
    let text = std::fs::read_to_string(&result).unwrap_or_default();
    let _ = std::fs::remove_file(&result);
    if waited != WAIT_OBJECT_0 {
        return Err(Error::Other("rexenv's administrator step could not be waited for".into()));
    }
    match rules::parse_result(&text) {
        Some((0, output)) => Ok(output),
        Some((code, output)) => Err(rules::failed(code, &output)),
        None => Err(rules::failed(exit as i32, "the step left no result")),
    }
}

/// Run `ops` in THIS process — the elevated step's body, and what an already-elevated check runs directly.
/// Returns `(exit code, output)`.
pub(crate) fn run_ops_here(ops: &str) -> (i32, String) {
    use std::os::windows::process::CommandExt;
    let parsed = match nrpt_rules::parse_ops(ops) {
        Ok(p) => p,
        Err(e) => return (2, e),
    };
    let dir = match backup_dir() {
        Ok(d) => d,
        Err(e) => return (2, e.to_string()),
    };
    let script = format!("$ProgressPreference = 'SilentlyContinue'\n{}", nrpt_rules::script_for(&parsed, &dir));
    let encoded = {
        use base64::Engine;
        let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
        base64::engine::general_purpose::STANDARD.encode(utf16)
    };
    match std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &encoded])
        .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
        .output()
    {
        Ok(o) => (
            o.status.code().unwrap_or(1),
            format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)).trim().to_string(),
        ),
        Err(e) => (1, e.to_string()),
    }
}

/// The elevated side, when `argv` names the step: `Some(exit code)`; `None` when it does not.
pub(crate) fn run_step(argv: &[String]) -> Option<i32> {
    let (ops, result) = rules::parse_step_args(argv)?;
    if !rules::result_path_allowed(&result, &std::env::temp_dir()) {
        return Some(3);
    }
    let (exit, output) = run_ops_here(&ops);
    let _ = std::fs::write(Path::new(&result), rules::format_result(exit, &output));
    Some(exit)
}

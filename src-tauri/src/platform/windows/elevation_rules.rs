//! The pure half of Windows' privileged step (W6 S3, ledger #619): how the unelevated app hands ops to
//! `rexenv.exe --elevated-step`, where the elevated process may write its result, how the result reads
//! back, and the words the user sees. No Win32 here — `elevation.rs` makes the calls — so this is compiled
//! into the macOS test build.
//!
//! The flow (owner's rulings R2 and 15 Sep 2026): rexenv's own dialog says what the change is for, then
//! UAC elevates `rexenv.exe` itself — the prompt names rexenv — and the elevated process runs ONLY
//! rexenv's ops (`nrpt_rules::parse_ops`), never a script it was handed.

use crate::error::Error;
use std::path::{Path, PathBuf};

/// The flag that makes `rexenv.exe` the elevated step instead of the app.
pub(crate) const STEP_FLAG: &str = "--elevated-step";

/// The prefix of every result file; the elevated process writes nowhere else.
pub(crate) const RESULT_PREFIX: &str = "rexenv-elevated-";

/// `ERROR_CANCELLED`: the user answered No to UAC (or cancelled rexenv's own dialog, reported the same).
pub(crate) const ERROR_CANCELLED: u32 = 1223;

/// The parameters for `ShellExecuteExW`: the flag, the ops as base64 (one token, nothing to quote), and the
/// result path in quotes (a user's temp directory may hold a space).
pub(crate) fn step_parameters(ops: &str, result: &Path) -> String {
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(ops.as_bytes());
    format!("{STEP_FLAG} {encoded} \"{}\"", result.display())
}

/// The ops and result path from the elevated process's arguments, when it was started as the step.
pub(crate) fn parse_step_args(argv: &[String]) -> Option<(String, PathBuf)> {
    use base64::Engine;
    let at = argv.iter().position(|a| a == STEP_FLAG)?;
    let ops = base64::engine::general_purpose::STANDARD.decode(argv.get(at + 1)?).ok()?;
    let result = PathBuf::from(argv.get(at + 2)?);
    Some((String::from_utf8(ops).ok()?, result))
}

/// Whether the elevated process may write its result to `result`: a `rexenv-elevated-*.txt` file directly
/// in this user's temp directory. Anything else — another directory, another name — is refused, so the
/// step cannot be pointed at a file an administrator's write would damage.
pub(crate) fn result_path_allowed(result: &Path, temp_dir: &Path) -> bool {
    // Split on the last separator of EITHER kind, not `Path::parent`: a Windows path means the same thing
    // whichever host reads the rule, and the rule's tests run on macOS too.
    let full = result.to_string_lossy();
    let Some(cut) = full.rfind(['\\', '/']) else { return false };
    let (dir, name) = (&full[..cut], &full[cut + 1..]);
    let named = name.starts_with(RESULT_PREFIX) && name.ends_with(".txt");
    let temp = temp_dir.to_string_lossy();
    named && dir.trim_end_matches(['\\', '/']).eq_ignore_ascii_case(temp.trim_end_matches(['\\', '/']))
}

/// The result file's text.
pub(crate) fn format_result(exit: i32, output: &str) -> String {
    format!("exit {exit}\n{output}")
}

/// `(exit code, output)` from a result file; `None` for anything else.
pub(crate) fn parse_result(text: &str) -> Option<(i32, String)> {
    let (head, output) = text.split_once('\n').unwrap_or((text, ""));
    let exit = head.strip_prefix("exit ")?.trim().parse().ok()?;
    Some((exit, output.to_string()))
}

/// What rexenv's own dialog says before Windows asks: the reason, and that a Windows prompt follows.
pub(crate) fn before_uac_text(sentence: &str) -> String {
    format!("{sentence}\n\nWindows will ask for your permission next.")
}

/// A cancelled dialog or a No to UAC.
pub(crate) fn cancelled() -> Error {
    Error::Other("The permission prompt was cancelled — nothing was changed. Try again and choose Yes.".into())
}

/// The step ran and failed: its own output says why.
pub(crate) fn failed(exit: i32, output: &str) -> Error {
    let said = output.split_whitespace().collect::<Vec<_>>().join(" ");
    Error::Other(format!("rexenv's administrator step failed (exit {exit}): {said}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_step_arguments_round_trip_and_a_space_in_the_result_path_survives() {
        let ops = "nrpt-install test ; nrpt-remove rex";
        let result = PathBuf::from(r"C:\Users\A B\AppData\Local\Temp\rexenv-elevated-42.txt");
        let params = step_parameters(ops, &result);
        assert!(params.starts_with("--elevated-step "), "{params}");
        // As Windows splits a command line: the flag, the base64 token, the quoted path.
        let argv: Vec<String> = vec!["rexenv.exe".into(), "--elevated-step".into(), params.split(' ').nth(1).unwrap().into(), result.display().to_string()];
        assert_eq!(parse_step_args(&argv), Some((ops.to_string(), result)));
        assert_eq!(parse_step_args(&["rexenv.exe".into()]), None);
        assert_eq!(parse_step_args(&["x".into(), "--elevated-step".into(), "!!not base64!!".into(), "p".into()]), None);
    }

    /// Ledger #619 — the elevated process writes only a `rexenv-elevated-*.txt` directly in the temp dir.
    #[test]
    fn the_result_goes_only_to_a_rexenv_file_in_the_temp_directory() {
        let temp = Path::new(r"C:\Users\A B\AppData\Local\Temp");
        assert!(result_path_allowed(&temp.join("rexenv-elevated-42.txt"), temp));
        assert!(result_path_allowed(Path::new(r"c:\users\a b\appdata\local\temp\rexenv-elevated-1.txt"), Path::new(r"C:\Users\A B\AppData\Local\Temp\")));
        for bad in [
            r"C:\Windows\System32\drivers\etc\hosts",
            r"C:\Users\A B\AppData\Local\Temp\hosts",
            r"C:\Users\A B\AppData\Local\Temp\sub\rexenv-elevated-1.txt",
            r"C:\Users\A B\AppData\Local\Temp\rexenv-elevated-1.exe",
            r"C:\Windows\Temp\rexenv-elevated-1.txt",
        ] {
            assert!(!result_path_allowed(Path::new(bad), temp), "{bad}");
        }
    }

    #[test]
    fn the_result_reads_back_and_the_words_say_what_happened() {
        assert_eq!(parse_result(&format_result(0, "done\nline two")), Some((0, "done\nline two".into())));
        assert_eq!(parse_result(&format_result(1, "")), Some((1, String::new())));
        assert_eq!(parse_result("exit x\n"), None);
        assert_eq!(parse_result("done"), None);
        let before = before_uac_text("rexenv wants to add a DNS rule so .test sites open on this computer.");
        assert!(before.starts_with("rexenv wants to add a DNS rule") && before.ends_with("Windows will ask for your permission next."));
        assert!(cancelled().to_string().contains("cancelled") && cancelled().to_string().contains("nothing was changed"));
        let f = failed(1, "Add-DnsClientNrptRule :\n  Access is denied.");
        assert!(f.to_string().contains("exit 1") && f.to_string().contains("Access is denied."), "{f}");
    }
}

//! The pure half of Windows' single-instance lock (W7 S1, plan §5 W7 ruling Q1, ledger #620): the pipe's
//! name, what a first-instance create's answer means, and which requests the pipe takes before W8. No
//! Win32 here — `app_pipe.rs` makes the calls — so this is compiled into the macOS test build.
//!
//! The lock is a named pipe created with `FILE_FLAG_FIRST_PIPE_INSTANCE`, as on macOS it is the listening
//! unix socket (ledger #441): a pipe dies with its holder's last handle, so it cannot outlive a crash the
//! way a pid file does. Measured on the Dell, 15 Sep 2026 (`windows_desktop_probe`): a second process's
//! first-instance create of a held name is refused with error 5, and its client connect still reaches
//! the holder.

use sha2::{Digest, Sha256};

/// `ERROR_ACCESS_DENIED` — what a first-instance create answers while another process holds the name.
pub(crate) const ERROR_ACCESS_DENIED: u32 = 5;

/// The one request the pipe serves until the `rex` CLI reaches Windows (W8).
pub(crate) const APP_OPEN: &str = "app.open";

/// The pipe for the app-data directory whose config folder is `config_dir`: one rexenv per app-data
/// directory, as on macOS. A digest, not the path: a pipe name is limited to 256 characters and may not
/// hold a second backslash-separated part in every API. Lower-cased first, because Windows paths are
/// case-insensitive — two launches that spell the same folder differently must meet at the same lock.
pub(crate) fn pipe_name(config_dir: &str) -> String {
    let folded = config_dir.trim_end_matches(['\\', '/']).to_lowercase();
    let digest = Sha256::digest(folded.as_bytes());
    let hex: String = digest.iter().take(10).map(|b| format!("{b:02x}")).collect();
    format!(r"\\.\pipe\rexenv-app-{hex}")
}

/// What a first-instance create of the lock found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Claim {
    /// The create succeeded: this process holds the lock.
    Ours,
    /// Refused with access denied: another rexenv holds this app-data directory.
    AnotherInstance,
    /// Anything else. The app starts without the lock — refusing to launch over an error nobody can
    /// interpret is the worse failure of the two (the macOS rule, ledger #441).
    Unclear(u32),
}

/// `create` is the first-instance create's result: `Ok` or the Win32 error code.
pub(crate) fn claim_from(create: Result<(), u32>) -> Claim {
    match create {
        Ok(()) => Claim::Ours,
        Err(ERROR_ACCESS_DENIED) => Claim::AnotherInstance,
        Err(code) => Claim::Unclear(code),
    }
}

/// Whether the pipe serves `cmd` before W8 (ruling Q1: it knows only `app.open`).
pub(crate) fn serves(cmd: &str) -> bool {
    cmd == APP_OPEN
}

/// The reply to any other request.
pub(crate) fn not_yet(cmd: &str) -> String {
    serde_json::json!({
        "ok": false,
        "error": format!("`{cmd}` does not reach rexenv on Windows yet — the rex CLI comes to Windows in a later build"),
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ledger #620 — one lock per app-data directory, whatever case the path is spelled in.
    #[test]
    fn one_pipe_per_app_data_directory_whatever_its_case() {
        let a = pipe_name(r"C:\Users\A B\AppData\Local\rexenv\rexenv\data\config");
        assert_eq!(a, pipe_name(r"c:\users\a b\appdata\local\rexenv\rexenv\data\config\"));
        assert_ne!(a, pipe_name(r"C:\Users\Other\AppData\Local\rexenv\rexenv\data\config"));
        assert!(a.starts_with(r"\\.\pipe\rexenv-app-"), "{a}");
        let tail = a.trim_start_matches(r"\\.\pipe\");
        assert!(!tail.contains('\\') && tail.len() < 64, "{tail}");
        // `rex` computes this name itself (plan §5 W8 ruling Q1) and asserts the same literal
        // (`cli/src/main.rs`, ledger #630), so the two cannot drift apart unseen.
        assert_eq!(a, r"\\.\pipe\rexenv-app-c472155a9cab9003d05c");
    }

    /// Ledger #620 — only access denied means another instance; anything unclear starts the app.
    #[test]
    fn only_access_denied_is_another_instance() {
        assert_eq!(claim_from(Ok(())), Claim::Ours);
        assert_eq!(claim_from(Err(5)), Claim::AnotherInstance);
        assert_eq!(claim_from(Err(231)), Claim::Unclear(231));
        assert_eq!(claim_from(Err(2)), Claim::Unclear(2));
    }

    #[test]
    fn the_pipe_serves_only_app_open_before_w8() {
        assert!(serves("app.open"));
        for cmd in ["status", "doctor", "site.create", "App.Open", ""] {
            assert!(!serves(cmd), "{cmd}");
        }
        let reply: serde_json::Value = serde_json::from_str(&not_yet("status")).unwrap();
        assert_eq!(reply["ok"], false);
        assert!(reply["error"].as_str().unwrap().contains("`status`"));
    }
}

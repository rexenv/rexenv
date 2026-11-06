//! The pure half of headless mode on Windows (docs/PLAN-headless-web.md T5 W1): the words, and the
//! TEXT tests that keep a headless launch from ever reaching a dialog. No Win32 here, so this is
//! compiled into the macOS test build.
//!
//! **Why a headless launch needs nothing prepared, and why it must never show a dialog.** Measured on
//! the Dell, 6 Nov 2026 (plan §2b): `rexenv.exe` in session 0 — an SSH session, or an S4U scheduled task
//! that runs whether or not anyone is logged on — runs the whole backend with no code change. Session 0
//! has no interactive desktop, though: a `MessageBoxW` there has nobody to answer it and blocks its
//! thread for good, and UAC cannot prompt at all. So every door that would open one — rexenv's own
//! dialog before UAC (`elevation.rs`), and the panic notice — answers in words instead.

/// What a privileged change answers on a headless launch: there is no desktop for rexenv's dialog or
/// for UAC, so the change is refused and the sentence says where it can be made.
pub(crate) const NEEDS_SETUP: &str = "rexenv is running headless, with no desktop session, so there is \
nobody to answer a Windows administrator (UAC) prompt and this change was not made. Make it once from \
the rexenv app on this PC's desktop, or from an administrator console.";

/// `--headless-setup` before it exists on Windows (T5 W2): `--headless` already runs, so the sentence says
/// what to do instead of claiming there is no headless mode.
pub(crate) const SETUP_NOT_YET: &str = "rexenv.exe --headless-setup is not available on Windows yet. rexenv.exe --headless runs (from an SSH session or a scheduled task that runs whether or not you are logged on), but the .rex resolver and the trusted certificate it relies on come from the rexenv app's own setup: finish that once on this PC's desktop first.";

#[cfg(test)]
mod tests {
    /// `windows/mod.rs` as text: the Win32 halves are not compiled on the macOS test host, so the
    /// order of their first lines is what can be checked here (the #175 bound: TEXT, not behaviour).
    fn mod_rs() -> String {
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/platform/windows/mod.rs"))
            .expect("windows/mod.rs")
    }

    /// From `signature` to the first line that closes at `indent` — the function's own closing brace.
    fn body_of(src: &str, signature: &str, indent: &str) -> String {
        let start = src.find(signature).unwrap_or_else(|| panic!("{signature} is gone"));
        let rest = &src[start..];
        let end = rest.find(&format!("\n{indent}}}\n")).unwrap_or_else(|| panic!("{signature} has no end"));
        rest[..end].to_string()
    }

    /// A headless launch refuses BEFORE `elevation::run_privileged`, which opens rexenv's dialog and then
    /// UAC — both would wait forever in session 0.
    #[test]
    fn a_headless_launch_refuses_a_privileged_change_before_any_dialog() {
        let body = body_of(&mod_rs(), "fn run_privileged(&self, script: &str, reason:", "    ");
        let headless = body
            .find("if crate::is_headless_launch() {\n            return Err(Error::Other(headless_rules::NEEDS_SETUP.into()));")
            .expect("the headless refusal");
        let elevate = body.find("elevation::run_privileged(").expect("the elevation call");
        assert!(headless < elevate, "the refusal must come first:\n{body}");
    }

    /// The panic notice is a `MessageBoxW`: on a headless launch it would block the panicking thread on a
    /// box nobody can see. The crash log is still written (`crash.rs`); only the box is skipped.
    #[test]
    fn a_headless_launch_shows_no_panic_box() {
        let src = mod_rs();
        let body = body_of(&src, "pub fn fatal_notice(summary: &str, crash_log: Option<&Path>) {", "");
        let headless = body.find("if crate::is_headless_launch() {\n        return;\n    }").expect("the headless return");
        let boxed = body.find("MessageBoxW(").expect("the message box");
        assert!(headless < boxed, "the return must come first:\n{body}");
    }

    #[test]
    fn the_refusal_says_what_happened_and_where_to_go() {
        let s = super::NEEDS_SETUP;
        assert!(s.contains("headless") && s.contains("UAC") && s.contains("was not made"), "{s}");
        assert!(s.contains("administrator console"), "{s}");
    }
}

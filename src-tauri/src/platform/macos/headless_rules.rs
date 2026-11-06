//! Headless mode on macOS (docs/PLAN-headless-web.md T5 M1): when it may run, and the words.
//!
//! **Why a logged-in session is required.** Measured on the macOS 15.8 VM, 6 Nov 2026 (plan §2b): with
//! this account logged in, rexenv launched from SSH or by a system LaunchDaemon ran the whole app, web UI
//! included; with NOBODY logged in, the same launch never reached Tauri's `setup` — the main thread sat in
//! `-[NSApplication run]` waiting for a finish-launching event that only a window-server session sends,
//! with no tokio thread and `rex` hanging. A supervisor would see a live process doing nothing. So a
//! headless launch checks first, and refuses in words instead of hanging.
//!
//! **How "logged in" is read.** `/dev/console` belongs to the user at the console, and to root at the
//! login window — the long-standing way to ask without linking a framework. A launch whose uid owns it
//! has a window-server session; a launch whose uid does not (nobody logged in, or another account in
//! front after fast user switching) does not have one it can use.

/// Whether a headless launch by `uid` can start: the console belongs to it. `None` = the console could
/// not be read, which is refused (a hang is worse than a sentence).
pub(crate) fn session_is_ours(console_uid: Option<u32>, uid: u32) -> bool {
    console_uid == Some(uid)
}

/// The refusal when this account is not the one logged in.
pub(crate) const NEEDS_SESSION: &str = "rexenv's headless mode on macOS needs this account logged in: with \
nobody logged in, macOS never finishes launching an app, so rexenv would start and do nothing. Turn on \
automatic login for this account (System Settings → Users & Groups → Automatically log in as), or log in \
once — the screen can stay locked.";

/// What a privileged change answers on a headless launch: rexenv's administrator prompt would open on a
/// screen nobody is watching and wait there.
pub(crate) const NEEDS_PROMPT: &str = "rexenv is running headless, so nobody is at the screen to answer \
macOS's administrator prompt and this change was not made. Make it once from the rexenv app on this Mac.";

/// `--headless-setup` before it exists on macOS (T5 M3): `--headless` runs, so the sentence says what to do.
pub(crate) const SETUP_NOT_YET: &str = "rexenv --headless-setup is not available on macOS yet. rexenv \
--headless runs while this account is logged in (turn on automatic login for it), but the .rex resolver, \
the trusted certificate and the HTTPS server it relies on come from the rexenv app's own setup: finish \
that once in the app first.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_account_at_the_console_starts_headless() {
        assert!(session_is_ours(Some(501), 501));
        assert!(!session_is_ours(Some(0), 501), "the login window: nobody logged in");
        assert!(!session_is_ours(Some(502), 501), "another account in front");
        assert!(!session_is_ours(None, 501), "an unreadable console is refused, not guessed");
    }

    fn mod_rs() -> &'static str {
        include_str!("mod.rs")
    }

    /// TEXT (the #175 bound): the headless refusal comes before the branded prompt is built — the
    /// applet's dialog would wait on a screen nobody watches.
    #[test]
    fn a_headless_launch_refuses_a_privileged_change_before_any_prompt() {
        let src = mod_rs();
        let body = &src[src.find("    fn run_privileged(&self, script: &str, reason: &PromptReason) -> Result<String> {").expect("run_privileged")..];
        let body = &body[..body.find("\n    }\n").expect("its end")];
        let refuse = body
            .find("if crate::is_headless_launch() {\n            return Err(Error::Other(headless_rules::NEEDS_PROMPT.into()));")
            .expect("the headless refusal");
        let prompt = body.find("prompt_applet::build(").expect("the prompt");
        assert!(refuse < prompt, "{body}");
    }

    /// The launch check reads the console's owner and refuses unless it is this uid.
    #[test]
    fn prepare_refuses_without_the_session() {
        let src = mod_rs();
        let body = &src[src.find("pub(crate) fn prepare_headless() -> Result<()> {").expect("prepare_headless")..];
        let body = &body[..body.find("\n}\n").expect("its end")];
        assert!(body.contains("std::fs::metadata(\"/dev/console\")"), "{body}");
        assert!(body.contains("headless_rules::session_is_ours(") && body.contains("headless_rules::NEEDS_SESSION"), "{body}");
    }
}

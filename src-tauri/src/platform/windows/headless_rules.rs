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

/// `rexenv.exe --headless-setup` (T5 W2), and `--enable-web`.
pub(crate) const SETUP_FLAG: &str = "--headless-setup";
pub(crate) const ENABLE_WEB_FLAG: &str = "--enable-web";

/// What the setup was asked. `consent` empty = ask at the console, or leave both as they are with no
/// console to ask at.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct SetupArgs {
    pub consent: crate::core::headless_consent::Consent,
    /// `--enable-web`: the boot task runs `rexenv.exe --headless --web`. The task is rewritten on every
    /// run, so a run without it turns the web UI off — said in the setup's output. Session 0 starts no
    /// WebView2, and the web UI answers anyway: its dispatch falls back to the app's local origin
    /// (`web_server::dispatch`, measured on the Dell 6 Nov 2026).
    pub web: bool,
}

/// Every argument the setup takes; anything else is refused BEFORE any change (a mistyped
/// `--enable-mpc` must not read as "no consent given"). No `--user`: on Windows the setup runs AS the
/// account rexenv runs as — an administrator console that account opened — because the tasks it writes
/// run as the token that registers them and the consent goes into that account's own database.
pub(crate) fn parse_setup_args(argv: &[String]) -> std::result::Result<SetupArgs, String> {
    use crate::core::headless_consent::{self, AGENT_ACCESS_FLAG, ENABLE_MCP_FLAG};
    const USAGE: &str = "the setup takes --enable-mcp, --agent-access <read|changes|full> and --enable-web";
    let mut out = SetupArgs::default();
    let mut it = argv.iter().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            SETUP_FLAG => {}
            ENABLE_WEB_FLAG => {
                if std::mem::replace(&mut out.web, true) {
                    return Err(format!("{ENABLE_WEB_FLAG} given twice"));
                }
            }
            ENABLE_MCP_FLAG => {
                if out.consent.mcp.replace(true).is_some() {
                    return Err(format!("{ENABLE_MCP_FLAG} given twice"));
                }
            }
            AGENT_ACCESS_FLAG => {
                let v = it.next().ok_or_else(|| headless_consent::bad_level(""))?;
                let level = crate::core::agent_access::AccessLevel::parse(v).ok_or_else(|| headless_consent::bad_level(v))?;
                if out.consent.access.replace(level).is_some() {
                    return Err(format!("{AGENT_ACCESS_FLAG} given twice"));
                }
            }
            other => return Err(format!("unknown argument {other:?} — {USAGE}")),
        }
    }
    Ok(out)
}

/// The setup run without an elevated token.
pub(crate) const NOT_ELEVATED: &str = "run it from an administrator console opened by the account rexenv \
runs as: right-click PowerShell, choose \"Run as administrator\", then run rexenv.exe --headless-setup there.";

/// The setup run from session 0 (an SSH session) while rexenv's certificate is not trusted yet: trusting
/// it into the user's store shows a Windows confirmation, and session 0 has no desktop to show it on —
/// the setup would wait forever. Refused before any change.
pub(crate) const CA_NEEDS_DESKTOP: &str = "rexenv's local certificate is not trusted on this account yet, and \
trusting it shows a Windows confirmation that needs this PC's desktop — this console has none (an SSH \
session runs without one). Run the setup once from an administrator console on the desktop; after that it \
can be re-run from anywhere.";

/// A build folder's `rexenv.exe` (cargo's `target\…`): the boot tasks would run whatever the next build
/// leaves there, and nothing once it is cleaned. The installed copy is the one to register.
pub(crate) fn is_build_output(exe: &std::path::Path) -> bool {
    // Split by hand on both separators: the rule is tested on the macOS host too, where `components()`
    // would not split a Windows path.
    exe.to_string_lossy().split(['\\', '/']).any(|c| c.eq_ignore_ascii_case("target"))
}

pub(crate) const BUILD_OUTPUT: &str = "this rexenv.exe is in a build folder (target\\…); the boot tasks would \
point at whatever the next build leaves there. Run the setup with the installed rexenv.exe \
(%LOCALAPPDATA%\\rexenv\\rexenv.exe).";

/// The consents, when none was typed and there is no console to ask at.
pub(crate) const CONSENT_UNCHANGED: &str = "MCP endpoint and Agent access: left as they are (off and read \
unless set before). Re-run with --enable-mcp and --agent-access <level> to change them.";

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

    fn args(v: &[&str]) -> Vec<String> {
        std::iter::once("rexenv.exe").chain(v.iter().copied()).map(String::from).collect()
    }

    #[test]
    fn the_setup_takes_its_three_arguments_and_refuses_anything_else() {
        use crate::core::agent_access::AccessLevel;
        let a = super::parse_setup_args(&args(&["--headless-setup", "--enable-web", "--enable-mcp", "--agent-access", "changes"])).unwrap();
        assert!(a.web);
        assert_eq!(a.consent.mcp, Some(true));
        assert_eq!(a.consent.access, Some(AccessLevel::Changes));
        let none = super::parse_setup_args(&args(&["--headless-setup"])).unwrap();
        assert!(!none.web && none.consent.is_empty());
        for bad in [&["--headless-setup", "--enable-mpc"][..], &["--headless-setup", "--user", "dell"], &["--headless-setup", "--agent-access"], &["--headless-setup", "--agent-access", "root"], &["--headless-setup", "--enable-web", "--enable-web"]] {
            assert!(super::parse_setup_args(&args(bad)).is_err(), "{bad:?} must be refused before any change");
        }
    }

    #[test]
    fn a_build_folder_exe_is_not_registered() {
        use std::path::Path;
        assert!(super::is_build_output(Path::new(r"C:\src\rexenv\src-tauri\target\debug\rexenv.exe")));
        assert!(super::is_build_output(Path::new(r"C:\src\rexenv\src-tauri\target\x86_64-pc-windows-msvc\release\rexenv.exe")));
        assert!(!super::is_build_output(Path::new(r"C:\Users\DELL\AppData\Local\rexenv\rexenv.exe")));
        assert!(!super::is_build_output(Path::new(r"C:\Users\dell\t5\rexenv.exe")));
    }

    #[test]
    fn the_refusal_says_what_happened_and_where_to_go() {
        let s = super::NEEDS_SETUP;
        assert!(s.contains("headless") && s.contains("UAC") && s.contains("was not made"), "{s}");
        assert!(s.contains("administrator console"), "{s}");
    }
}

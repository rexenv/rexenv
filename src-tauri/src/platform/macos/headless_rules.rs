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

/// Whether the launchd session this process was started in can read the user's keychain and trust
/// settings — `launchctl managername`'s answer. Measured on the 15.8 VM, 6 Nov 2026 (T5 M2): `Aqua` (the
/// GUI login, a LaunchAgent) and `Background` (an SSH login) both see the login keychain and the CA as
/// trusted; `System` — a LaunchDaemon, even one with `UserName` set to this account — sees only
/// `System.keychain`, no user trust settings, and `security verify-cert` fails the trusted CA with
/// `CSSMERR_TP_NOT_TRUSTED`. rexenv started there believed its own setup incomplete. Anything else
/// (`LoginWindow`, an unreadable answer) is refused too.
pub(crate) fn session_context_ok(managername: Option<&str>) -> bool {
    matches!(managername.map(str::trim), Some("Aqua") | Some("Background"))
}

/// The refusal for a launch from the system domain.
pub(crate) const NEEDS_USER_DOMAIN: &str = "rexenv --headless was started by a system LaunchDaemon, where \
macOS keeps this account's keychain out of reach — rexenv would see its own certificate as untrusted. Start \
it in this account's session instead: a LaunchAgent in ~/Library/LaunchAgents, or from an SSH login.";

/// The refusal when this account is not the one logged in.
pub(crate) const NEEDS_SESSION: &str = "rexenv's headless mode on macOS needs this account logged in: with \
nobody logged in, macOS never finishes launching an app, so rexenv would start and do nothing. Turn on \
automatic login for this account (System Settings → Users & Groups → Automatically log in as), or log in \
once — the screen can stay locked.";

/// What a privileged change answers on a headless launch: rexenv's administrator prompt would open on a
/// screen nobody is watching and wait there.
pub(crate) const NEEDS_PROMPT: &str = "rexenv is running headless, so nobody is at the screen to answer \
macOS's administrator prompt and this change was not made. Make it once from the rexenv app on this Mac.";

/// `sudo rexenv --headless-setup` (T5 M3), `--user`, `--enable-web`.
pub(crate) const SETUP_FLAG: &str = "--headless-setup";
pub(crate) const ENABLE_WEB_FLAG: &str = "--enable-web";

/// What the setup was asked. `consent` empty = ask in the terminal, or leave both with no terminal.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct SetupArgs {
    pub user: Option<String>,
    pub consent: crate::core::headless_consent::Consent,
    /// `--enable-web`: the LaunchAgent runs `rexenv --headless --web`. Rewritten on every run, so a run
    /// without it turns the web UI off — said in the output.
    pub web: bool,
}

/// Every argument the setup takes; anything else is refused BEFORE any change (a mistyped
/// `--enable-mpc` must not read as "no consent given").
pub(crate) fn parse_setup_args(argv: &[String]) -> std::result::Result<SetupArgs, String> {
    use crate::core::headless_consent::{self, AGENT_ACCESS_FLAG, ENABLE_MCP_FLAG};
    const USAGE: &str = "the setup takes --user <name>, --enable-mcp, --agent-access <read|changes|full> and --enable-web";
    let mut out = SetupArgs::default();
    let mut it = argv.iter().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            SETUP_FLAG => {}
            "--user" => {
                let v = it.next().ok_or_else(|| format!("--user needs a name — {USAGE}"))?;
                if out.user.replace(v.clone()).is_some() {
                    return Err("--user given twice".into());
                }
            }
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

/// The LaunchAgent the setup writes (M2: a LaunchAgent, never a system LaunchDaemon — the system domain
/// cannot read the user's keychain).
pub(crate) const AGENT_LABEL: &str = "dev.rexenv.rexenv.headless";

/// The agent's plist: `exe --headless [--web]` at this account's GUI login (`LimitLoadToSessionType`
/// `Aqua` — with automatic login, at boot), restarted when it CRASHES (`KeepAlive` `SuccessfulExit`
/// false) but not when it exits cleanly — a launch that found the app already running hands off and
/// exits 0, and a plain `KeepAlive` would relaunch it every ten seconds for as long as the app ran.
pub(crate) fn agent_plist(exe: &std::path::Path, web: bool, log: &std::path::Path) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let web_arg = if web { "\n\t\t<string>--web</string>" } else { "" };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n\
         \t<key>Label</key>\n\t<string>{AGENT_LABEL}</string>\n\
         \t<key>ProgramArguments</key>\n\t<array>\n\t\t<string>{exe}</string>\n\t\t<string>--headless</string>{web_arg}\n\t</array>\n\
         \t<key>RunAtLoad</key>\n\t<true/>\n\
         \t<key>KeepAlive</key>\n\t<dict>\n\t\t<key>SuccessfulExit</key>\n\t\t<false/>\n\t</dict>\n\
         \t<key>LimitLoadToSessionType</key>\n\t<string>Aqua</string>\n\
         \t<key>ProcessType</key>\n\t<string>Interactive</string>\n\
         \t<key>AbandonProcessGroup</key>\n\t<true/>\n\
         \t<key>AssociatedBundleIdentifiers</key>\n\t<array>\n\t\t<string>dev.rexenv.rexenv</string>\n\t</array>\n\
         \t<key>StandardOutPath</key>\n\t<string>{log}</string>\n\
         \t<key>StandardErrorPath</key>\n\t<string>{log}</string>\n\
         </dict>\n</plist>\n",
        exe = esc(&exe.display().to_string()),
        log = esc(&log.display().to_string()),
    )
}

/// The installed app's binary, not a build folder's: the agent would run whatever the next build left.
pub(crate) fn is_app_bundle_binary(exe: &std::path::Path) -> bool {
    exe.to_string_lossy().contains(".app/Contents/MacOS/")
}

pub(crate) const NEEDS_ROOT: &str = "run it as an administrator, from the account rexenv runs as:\n$ sudo \
/Applications/rexenv.app/Contents/MacOS/rexenv --headless-setup";

pub(crate) const NEEDS_APP_BINARY: &str = "run the setup with the installed app's binary \
(/Applications/rexenv.app/Contents/MacOS/rexenv): the LaunchAgent runs the file it was given, and a build \
folder's copy changes with every build.";

/// What a headless Mac still needs from the app's own setup. Unlike Linux's setup, which makes these
/// itself, macOS asks the user to confirm trusting the certificate on the screen — a step nobody can
/// answer from a terminal — so the setup checks instead of doing.
pub(crate) fn needs_app_setup(missing: &[&str]) -> String {
    format!(
        "this Mac has not finished rexenv's own setup ({}). Open the rexenv app once at the Mac's screen \
         and finish it — trusting the certificate needs your confirmation there — then run this again.",
        missing.join(", ")
    )
}

pub(crate) const CONSENT_UNCHANGED: &str = "MCP endpoint and Agent access: left as they are (off and read \
unless set before). Re-run with --enable-mcp and --agent-access <level> to change them.";

/// The last line: what makes it run with nobody at the screen.
pub(crate) const AUTOLOGIN: &str = "For it to start at boot with nobody at the screen, turn on automatic \
login for this account: System Settings → Users & Groups → Automatically log in as (FileVault must be off \
for that option to appear). The screen can stay locked.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_session_that_can_read_the_users_keychain_starts_headless() {
        assert!(session_context_ok(Some("Aqua")), "a LaunchAgent in the GUI login");
        assert!(session_context_ok(Some("Background\n")), "an SSH login (launchctl ends its answer with a newline)");
        assert!(!session_context_ok(Some("System")), "a LaunchDaemon, UserName or not: no login keychain");
        assert!(!session_context_ok(Some("LoginWindow")));
        assert!(!session_context_ok(None), "unreadable is refused");
    }

    #[test]
    fn only_the_account_at_the_console_starts_headless() {
        assert!(session_is_ours(Some(501), 501));
        assert!(!session_is_ours(Some(0), 501), "the login window: nobody logged in");
        assert!(!session_is_ours(Some(502), 501), "another account in front");
        assert!(!session_is_ours(None, 501), "an unreadable console is refused, not guessed");
    }

    fn args(v: &[&str]) -> Vec<String> {
        std::iter::once("rexenv").chain(v.iter().copied()).map(String::from).collect()
    }

    #[test]
    fn the_setup_takes_its_arguments_and_refuses_anything_else() {
        let a = parse_setup_args(&args(&["--headless-setup", "--user", "linkon", "--enable-web", "--enable-mcp", "--agent-access", "read"])).unwrap();
        assert_eq!(a.user.as_deref(), Some("linkon"));
        assert!(a.web && a.consent.mcp == Some(true) && a.consent.access.is_some());
        assert!(parse_setup_args(&args(&["--headless-setup"])).unwrap().consent.is_empty());
        for bad in [&["--headless-setup", "--enable-mpc"][..], &["--headless-setup", "--user"], &["--headless-setup", "--agent-access", "root"], &["--headless-setup", "--user", "a", "--user", "b"]] {
            assert!(parse_setup_args(&args(bad)).is_err(), "{bad:?}");
        }
    }

    /// The LaunchAgent's shape (M2, M3): the user's GUI session only, restarted on a crash but NOT on a
    /// clean exit (the hand-off to a running app exits 0), the flags as asked.
    #[test]
    fn the_agent_runs_headless_in_the_users_session_and_does_not_loop_on_a_hand_off() {
        use std::path::Path;
        let exe = Path::new("/Applications/rexenv.app/Contents/MacOS/rexenv");
        let p = agent_plist(exe, false, Path::new("/Users/a b/Library/Logs/headless.log"));
        assert!(p.contains(&format!("<string>{AGENT_LABEL}</string>")));
        assert!(p.contains("<string>/Applications/rexenv.app/Contents/MacOS/rexenv</string>\n\t\t<string>--headless</string>\n\t</array>"), "{p}");
        assert!(p.contains("<key>KeepAlive</key>\n\t<dict>\n\t\t<key>SuccessfulExit</key>\n\t\t<false/>"), "{p}");
        assert!(p.contains("<key>LimitLoadToSessionType</key>\n\t<string>Aqua</string>"), "{p}");
        assert!(!p.contains("UserName"), "a LaunchAgent, never a daemon's UserName");
        let w = agent_plist(exe, true, Path::new("/l"));
        assert!(w.contains("<string>--headless</string>\n\t\t<string>--web</string>"), "{w}");
        assert!(agent_plist(Path::new("/A&B/rexenv.app/Contents/MacOS/rexenv"), false, Path::new("/l")).contains("/A&amp;B/"));
    }

    #[test]
    fn only_the_installed_binary_is_registered() {
        use std::path::Path;
        assert!(is_app_bundle_binary(Path::new("/Applications/rexenv.app/Contents/MacOS/rexenv")));
        assert!(!is_app_bundle_binary(Path::new("/Users/me/rexenv/src-tauri/target/debug/rexenv")));
    }

    /// A headless launch takes itself out of the reopen-at-login list (M3, measured: the list's copy won
    /// the race against the LaunchAgent's). TEXT: the call sits in the headless branch of `setup`.
    #[test]
    fn a_headless_launch_is_not_reopened_by_the_login_list() {
        let lib = include_str!("../../lib.rs");
        let branch = lib.find("log::info!(\"headless: no tray icon").expect("the headless branch of setup");
        let call = lib[branch..].find("platform::stay_out_of_login_relaunch();").expect("the call");
        assert!(call < 400, "the call belongs in that branch");
        let act = include_str!("activation.rs");
        assert!(act.contains(".disableRelaunchOnLogin();") && !act.contains("enableRelaunchOnLogin"), "disable once, never paired with enable");
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
        let console = body.find("headless_rules::session_is_ours(").unwrap();
        let context = body.find("headless_rules::session_context_ok(").expect("the session-context check");
        assert!(body.contains("\"managername\"") && body.contains("headless_rules::NEEDS_USER_DOMAIN"), "{body}");
        assert!(console < context, "nobody logged in is the first thing to say: {body}");
    }
}

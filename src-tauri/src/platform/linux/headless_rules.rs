//! Headless mode's rules on Linux, as pure text (docs/PLAN-headless-web.md T1): the Xvfb display
//! rexenv draws its window on, and the CLOSED set of root commands a headless instance may run
//! without a password.
//!
//! **Why a closed set, and why so small.** A headless box has no polkit agent, so `pkexec` can
//! never ask anyone (T0, 26 Oct 2026: `rex stop` refused). The tempting fix — a polkit rule that
//! lets the service user run `rexenv --privileged-step` with no password — would be passwordless
//! ROOT for every process of that user: the step runs ANY shell script it is handed
//! (`linux::run_step`), and the user's processes include every site's PHP-FPM and every plugin
//! under test. So the passwordless half is three fixed argv lists for rexenv's own edge unit,
//! written into `/etc/sudoers.d/` by an administrator (`sudo rexenv --headless-setup`) from the
//! SAME list the matcher reads, and nothing else. A script that is not exactly one of the edge's
//! start/stop scripts is refused with a sentence naming the setup command — never run, never
//! widened. The edge's INSTALL stays out on purpose: it copies a binary from the user-writable
//! download cache into the root tree.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use super::units::{edge_start_command, edge_stop_command, EDGE_UNIT};
use crate::core::agent_access::AccessLevel;
use crate::core::headless_consent::{self, Consent, AGENT_ACCESS_FLAG, ENABLE_MCP_FLAG};
use std::path::{Path, PathBuf};

/// The flag that runs rexenv with no display (`main.rs` → `platform::prepare_headless`).
pub(crate) const HEADLESS_FLAG: &str = "--headless";
/// The administrator's one-time step (`sudo rexenv --headless-setup`).
pub(crate) const SETUP_FLAG: &str = "--headless-setup";
/// Where the setup writes the passwordless rule. No `.` in the name: sudo skips such files in
/// `sudoers.d`, which is also why the staging copy beside it starts with one.
pub(crate) const SUDOERS_PATH: &str = "/etc/sudoers.d/rexenv-headless";
pub(crate) const SUDOERS_STAGING: &str = "/etc/sudoers.d/.rexenv-headless.new";
pub(crate) const SUDO: &str = "/usr/bin/sudo";
pub(crate) const SYSTEMCTL: &str = "/usr/bin/systemctl";
pub(crate) const VISUDO: &str = "/usr/sbin/visudo";
pub(crate) const SYNC: &str = "/bin/sync";
pub(crate) const XVFB: &str = "/usr/bin/Xvfb";
pub(crate) const XAUTH: &str = "/usr/bin/xauth";

/// The system unit that runs rexenv headless at boot (T4), written by the setup.
pub(crate) const SERVICE_UNIT: &str = "rexenv-headless.service";
pub(crate) const SERVICE_PATH: &str = "/etc/systemd/system/rexenv-headless.service";
/// Staged beside it and renamed into place, so a crash never leaves half a unit systemd would load.
/// A dot-file: systemd loads only names ending in a unit suffix.
pub(crate) const SERVICE_STAGING: &str = "/etc/systemd/system/.rexenv-headless.service.new";
/// The deb's binary — the one path the unit can name. An AppImage moves with its file, and a unit
/// pointing at a Downloads folder would break on the next update, so the setup asks for the deb.
pub(crate) const REXENV_BIN: &str = "/usr/bin/rexenv";
pub(crate) const LOGINCTL: &str = "/usr/bin/loginctl";
pub(crate) const NEEDS_DEB: &str = "the headless service runs the installed package's /usr/bin/rexenv, which \
is not here. Install the .deb (the apt repository in docs/INSTALL.md), then re-run:\n$ sudo rexenv --headless-setup";

/// The virtual screen's size. The window is never shown; the size only has to fit its minimum.
pub(crate) const SCREEN: &str = "1280x800x24";
/// The display numbers tried, first free one wins — 99 and up, where `xvfb-run` starts too.
pub(crate) const DISPLAYS: std::ops::RangeInclusive<u32> = 99..=199;

/// The sentence for anything privileged a headless instance cannot do by itself.
pub(crate) const NEEDS_SETUP: &str = "rexenv is running headless, so there is no one to ask for an \
administrator's password. Run this once on the server as an administrator, then retry:\n$ sudo rexenv --headless-setup";
/// Said when neither consent was typed and there is no terminal to ask in (a scripted install).
pub(crate) const CONSENT_UNCHANGED: &str = "MCP endpoint and Agent access: left as they are (off and read \
unless set before) — there was no terminal to ask in. To set them, re-run with --enable-mcp and \
--agent-access <read|changes|full>.";
pub(crate) const XVFB_MISSING: &str = "rexenv's headless mode draws its window off-screen with Xvfb, \
which is not installed. Install it, then retry:\n$ sudo apt install xvfb xauth";

/// Every root command a headless instance may run without a password: the edge unit's enable,
/// restart and stop. The ONE list — `steps_for` maps a script onto it and `sudoers_contents`
/// grants exactly it, so the rule and the matcher cannot disagree.
pub(crate) fn passwordless_commands() -> [[&'static str; 4]; 3] {
    [
        [SYSTEMCTL, "enable", EDGE_UNIT, ""],
        [SYSTEMCTL, "restart", EDGE_UNIT, ""],
        [SYSTEMCTL, "disable", "--now", EDGE_UNIT],
    ]
}

/// One passwordless command, and whether its failure fails the step (the scripts it stands in
/// for ignore a failed `enable`/`disable` and so does this).
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Step {
    pub argv: Vec<&'static str>,
    pub must_succeed: bool,
}

fn argv(i: usize) -> Vec<&'static str> {
    passwordless_commands()[i].iter().copied().filter(|a| !a.is_empty()).collect()
}

/// The passwordless steps that do exactly what `script` does, when `script` is one of the
/// edge's own start/stop scripts byte for byte (`units::edge_start_command` /
/// `edge_stop_command`). `None` for anything else — which a headless instance refuses.
pub(crate) fn steps_for(script: &str) -> Option<Vec<Step>> {
    if script == edge_start_command() {
        Some(vec![Step { argv: argv(0), must_succeed: false }, Step { argv: argv(1), must_succeed: true }])
    } else if script == edge_stop_command() {
        Some(vec![Step { argv: argv(2), must_succeed: false }])
    } else {
        None
    }
}

/// A login name sudoers can hold as written: what `useradd` accepts by default
/// (`[a-z_][a-z0-9_-]*`, at most 32). Anything else could carry sudoers syntax into the rule.
pub(crate) fn valid_user(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c == '_')
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
        && name.len() <= 32
}

/// The `sudoers.d` file granting `user` the passwordless commands and nothing else.
/// `None` for a user name `valid_user` refuses.
pub(crate) fn sudoers_contents(user: &str) -> Option<String> {
    if !valid_user(user) {
        return None;
    }
    let cmds: Vec<String> = (0..passwordless_commands().len()).map(|i| argv(i).join(" ")).collect();
    Some(format!(
        "# Written by `sudo rexenv --headless-setup`: lets rexenv, running headless as {user}, start\n\
         # and stop its own edge (Caddy on :80/:443). Exactly these commands; nothing else.\n\
         {user} ALL=(root) NOPASSWD: {}\n",
        cmds.join(", ")
    ))
}

/// Whether `sudo` itself refused to run the command (no rule grants it), as opposed to the
/// command running and failing. A refusal fails EVERY step, even one whose own failure is
/// ignored: the stop's `disable` ignores a failed command, and before this told the two apart a
/// refused stop read as done — "stopped but its admin socket is still answering" (the VM, 26 Oct
/// 2026, with no rule installed).
pub(crate) fn sudo_refused(stderr: &str) -> bool {
    stderr.contains("password is required") || stderr.contains("not allowed") || stderr.contains("may not run")
}

/// What a failed `sudo -n` step means, as a sentence.
pub(crate) fn sudo_refusal(stderr: &str) -> String {
    if sudo_refused(stderr) {
        NEEDS_SETUP.to_string()
    } else {
        format!("the edge's root command failed: {}", stderr.trim())
    }
}

/// `rexenv-headless.service` for `user` (uid `uid`): rexenv at boot, as that user, with no one
/// logged in. `None` for a user name `valid_user` refuses — the name is written into the unit.
///
/// - `KillMode=process`: a stop or a crash-restart ends the APP only. With systemd's default
///   (`control-group`) every service rexenv started — MySQL, PHP-FPM, Nginx, all in this unit's
///   cgroup — would die with it, and services outliving the app is a rule of this project; the
///   next start adopts them (`adopt_startup`). Xvfb still dies with the app (`PR_SET_PDEATHSIG`).
/// - `XDG_RUNTIME_DIR` + `Wants=`/`After=user@<uid>.service`: rexenv installs and restarts its DNS
///   agent as a USER unit (`systemctl --user`), which reaches the user's manager through
///   `/run/user/<uid>`. A system service gets no such variable, and the manager runs at boot only
///   with lingering on (`linger_argv`, written by the same setup).
/// - `Restart=on-failure`: a clean exit — a second launch handing off to one already running —
///   is not restarted into a loop.
pub(crate) fn service_unit(user: &str, uid: u32) -> Option<String> {
    if !valid_user(user) {
        return None;
    }
    Some(format!(
        "# Written by `sudo rexenv --headless-setup`: rexenv on this server, headless, as {user}.\n\
         [Unit]\n\
         Description=rexenv (headless, driven by rex and rex mcp)\n\
         Wants=network-online.target user@{uid}.service\n\
         After=network-online.target user@{uid}.service\n\
         \n\
         [Service]\n\
         Type=simple\n\
         User={user}\n\
         WorkingDirectory=~\n\
         Environment=XDG_RUNTIME_DIR=/run/user/{uid}\n\
         ExecStart={REXENV_BIN} {HEADLESS_FLAG}\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         KillMode=process\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n"
    ))
}

/// The root commands the setup runs after writing the unit, in order: load it, enable it at boot,
/// and keep `user`'s own systemd running with no login (the DNS agent's user unit lives there).
pub(crate) fn install_commands(user: &str) -> [Vec<String>; 3] {
    [
        vec![SYSTEMCTL.into(), "daemon-reload".into()],
        vec![SYSTEMCTL.into(), "enable".into(), SERVICE_UNIT.into()],
        vec![LOGINCTL.into(), "enable-linger".into(), user.into()],
    ]
}

/// What `sudo rexenv --headless-setup …` was asked to do: the account, and what the administrator
/// typed as consent (`--enable-mcp`, `--agent-access <level>`, D7). Empty consent = ask in the
/// terminal, or leave both as they are when there is no terminal.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct SetupArgs {
    pub user: Option<String>,
    pub consent: Consent,
}

/// Every argument the setup takes. Anything else is refused BEFORE any change — a mistyped
/// `--enable-mpc` must not read as "no consent given" and quietly leave the endpoint off, nor a
/// misplaced level as some other flag's value.
pub(crate) fn parse_setup_args(argv: &[String]) -> std::result::Result<SetupArgs, String> {
    const USAGE: &str = "the setup takes --user <name>, --enable-mcp and --agent-access <read|changes|full>";
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
            ENABLE_MCP_FLAG => {
                if out.consent.mcp.replace(true).is_some() {
                    return Err(format!("{ENABLE_MCP_FLAG} given twice"));
                }
            }
            AGENT_ACCESS_FLAG => {
                let v = it.next().ok_or_else(|| headless_consent::bad_level(""))?;
                let level = AccessLevel::parse(v).ok_or_else(|| headless_consent::bad_level(v))?;
                if out.consent.access.replace(level).is_some() {
                    return Err(format!("{AGENT_ACCESS_FLAG} given twice"));
                }
            }
            other => return Err(format!("unknown argument {other:?} — {USAGE}")),
        }
    }
    Ok(out)
}

/// The socket and lock an X server on display `n` leaves.
pub(crate) fn display_paths(n: u32) -> [PathBuf; 2] {
    [PathBuf::from(format!("/tmp/.X11-unix/X{n}")), PathBuf::from(format!("/tmp/.X{n}-lock"))]
}

/// The first display number with neither a socket nor a lock, or `None` when all are taken.
pub(crate) fn free_display(taken: impl Fn(&Path) -> bool) -> Option<u32> {
    DISPLAYS.clone().find(|&n| display_paths(n).iter().all(|p| !taken(p)))
}

/// Xvfb's arguments: our display, our cookie file, no TCP listener.
pub(crate) fn xvfb_args(display: u32, auth: &Path) -> Vec<String> {
    vec![
        format!(":{display}"),
        "-auth".into(),
        auth.display().to_string(),
        "-nolisten".into(),
        "tcp".into(),
        "-screen".into(),
        "0".into(),
        SCREEN.into(),
        "-noreset".into(),
    ]
}

/// `xauth`'s arguments to write the cookie for `display` into `auth`.
pub(crate) fn xauth_args(display: u32, auth: &Path, cookie_hex: &str) -> Vec<String> {
    vec!["-q".into(), "-f".into(), auth.display().to_string(), "add".into(), format!(":{display}"), ".".into(), cookie_hex.into()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_edges_own_scripts_map_to_passwordless_steps() {
        let start = steps_for(&edge_start_command()).expect("the start script");
        assert_eq!(start[0], Step { argv: vec![SYSTEMCTL, "enable", EDGE_UNIT], must_succeed: false });
        assert_eq!(start[1], Step { argv: vec![SYSTEMCTL, "restart", EDGE_UNIT], must_succeed: true });
        let stop = steps_for(&edge_stop_command()).expect("the stop script");
        assert_eq!(stop, vec![Step { argv: vec![SYSTEMCTL, "disable", "--now", EDGE_UNIT], must_succeed: false }]);
        // Anything else — the install, a DNS route, a CA, a near-copy of a known script — is refused.
        for other in [
            "/usr/bin/systemctl restart rexenv-edge.service",
            &format!("{} ; /bin/sh", edge_stop_command()),
            &format!(" {}", edge_start_command()),
            "/bin/cp /home/u/caddy /usr/local/lib/rexenv/bin/caddy",
            "",
        ] {
            assert_eq!(steps_for(other), None, "{other:?} must not run without a password");
        }
    }

    #[test]
    fn the_sudoers_rule_grants_exactly_the_steps_and_nothing_else() {
        let rule = sudoers_contents("rexenv").expect("a valid user");
        let grant = rule.lines().find(|l| !l.starts_with('#')).expect("the grant line");
        let (who, cmds) = grant.split_once(" NOPASSWD: ").expect("the NOPASSWD form");
        assert_eq!(who, "rexenv ALL=(root)");
        let granted: Vec<&str> = cmds.split(", ").collect();
        let mut stepped: Vec<String> = [edge_start_command(), edge_stop_command()]
            .iter()
            .flat_map(|s| steps_for(s).unwrap())
            .map(|s| s.argv.join(" "))
            .collect();
        stepped.sort();
        let mut sorted = granted.clone();
        sorted.sort();
        assert_eq!(sorted, stepped, "the rule and the matcher must name the same commands");
        // Exact argv: no wildcard, no shell, no rexenv binary.
        assert!(!cmds.contains('*') && !cmds.contains("sh") && !cmds.contains("rexenv "), "{cmds}");
    }

    #[test]
    fn a_headless_instance_never_reaches_pkexec() {
        let src = include_str!("mod.rs");
        let body = &src[src.find("    fn run_privileged(&self, script: &str, _reason: &PromptReason)").expect("the trait impl")..];
        let headless = body.find("if crate::is_headless_launch() {\n            return headless::run_privileged(script);").expect("the headless branch");
        let pkexec = body.find("Self::pkexec(").expect("the pkexec call");
        assert!(headless < pkexec, "the headless branch must come before any pkexec");
        // And the headless door runs nothing `steps_for` did not hand it.
        let door = include_str!("headless.rs");
        let run = &door[door.find("pub(crate) fn run_privileged(").expect("the door")..];
        let run = &run[..run.find("\n}\n").expect("its end")];
        assert!(run.contains("let Some(steps) = rules::steps_for(script) else {"), "{run}");
        assert_eq!(run.matches("crate::platform::command(").count(), 2, "sudo for the steps, then sync");
        assert!(run.contains("crate::platform::command(rules::SUDO).arg(\"-n\").args(&step.argv)"));
        assert!(run.contains("crate::platform::command(rules::SYNC).status()"), "the boot links reach the disk (#740)");
    }

    /// The deb's post-install is a NOTE and nothing else (ledger #922): a package must not change
    /// a server on its own — the setup is the administrator's deliberate `sudo rexenv
    /// --headless-setup`. Every command line in it is the shell's own control flow, `cat` of a
    /// here-doc, or `exit 0`; and the deb really ships it.
    #[test]
    fn the_debs_post_install_only_prints_a_note() {
        let script = include_str!("../../../linux/postinst");
        assert!(script.starts_with("#!/bin/sh\n"), "a POSIX sh script");
        assert!(script.contains("sudo rexenv --headless-setup") && script.contains("xvfb xauth"), "the note names the two steps");
        let mut in_note = false;
        for line in script.lines().map(str::trim) {
            if in_note {
                in_note = line != "NOTE";
                continue;
            }
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let allowed = line == "set -e"
                || line == "fi"
                || line == "exit 0"
                || line == "cat <<'NOTE'"
                || (line.starts_with("if [ \"$1\" = \"configure\" ]") && line.ends_with("then"));
            assert!(allowed, "the post-install runs `{line}` — it may only print its note");
            in_note = line == "cat <<'NOTE'";
        }
        assert!(script.trim_end().ends_with("exit 0"), "never fails an install");
        let conf = include_str!("../../../tauri.conf.json");
        assert!(conf.contains("\"postInstallScript\": \"linux/postinst\""), "the deb does not ship the note");
    }

    #[test]
    fn a_user_name_that_could_carry_sudoers_syntax_is_refused() {
        for ok in ["rexenv", "_ci", "dev-1", "a"] {
            assert!(valid_user(ok), "{ok}");
        }
        for bad in ["", "root ALL=(ALL) NOPASSWD: ALL", "a,b", "Dev", "1dev", "dev\n", "%sudo", &"a".repeat(33)] {
            assert!(!valid_user(bad), "{bad:?}");
            assert_eq!(sudoers_contents(bad), None);
        }
    }

    #[test]
    fn a_refused_sudo_names_the_setup_command() {
        assert_eq!(sudo_refusal("sudo: a password is required\n"), NEEDS_SETUP);
        assert_eq!(sudo_refusal("Sorry, user ci is not allowed to execute '/usr/bin/systemctl …' as root"), NEEDS_SETUP);
        assert!(sudo_refusal("Failed to restart rexenv-edge.service: Unit not found.").contains("Unit not found"));
        assert!(sudo_refused("sudo: a password is required\n"));
        assert!(!sudo_refused("Failed to disable unit: Unit file rexenv-edge.service does not exist."));
        // The door fails a refused step whatever its `must_succeed`.
        let door = include_str!("headless.rs");
        assert!(door.contains("if !out.status.success() && (step.must_succeed || rules::sudo_refused(&stderr)) {"), "a refused ignorable step must still fail");
        assert!(NEEDS_SETUP.contains("sudo rexenv --headless-setup"));
    }

    #[test]
    fn the_setup_takes_typed_consent_and_refuses_anything_it_does_not_know() {
        let a = |v: &[&str]| parse_setup_args(&std::iter::once("rexenv").chain(v.iter().copied()).map(String::from).collect::<Vec<_>>());
        assert_eq!(a(&[SETUP_FLAG]).unwrap(), SetupArgs::default());
        let full = a(&[SETUP_FLAG, "--user", "ci", ENABLE_MCP_FLAG, AGENT_ACCESS_FLAG, "full"]).unwrap();
        assert_eq!(full.user.as_deref(), Some("ci"));
        assert_eq!(full.consent, Consent { mcp: Some(true), access: Some(AccessLevel::Full) });
        // A level alone is a consent to the dial only; the endpoint is left as it is.
        assert_eq!(a(&[SETUP_FLAG, AGENT_ACCESS_FLAG, "read"]).unwrap().consent, Consent { mcp: None, access: Some(AccessLevel::Read) });
        for bad in [
            &[SETUP_FLAG, "--enable-mpc"][..],
            &[SETUP_FLAG, AGENT_ACCESS_FLAG][..],
            &[SETUP_FLAG, AGENT_ACCESS_FLAG, "admin"][..],
            &[SETUP_FLAG, AGENT_ACCESS_FLAG, ""][..],
            &[SETUP_FLAG, AGENT_ACCESS_FLAG, "read", AGENT_ACCESS_FLAG, "full"][..],
            &[SETUP_FLAG, ENABLE_MCP_FLAG, ENABLE_MCP_FLAG][..],
            &[SETUP_FLAG, "--user"][..],
            &[SETUP_FLAG, "--yes"][..],
        ] {
            assert!(a(bad).is_err(), "{bad:?} must be refused before any change");
        }
    }

    #[test]
    fn the_service_runs_rexenv_headless_as_the_user_and_never_takes_the_stack_down() {
        let unit = service_unit("ci", 1001).expect("a valid user");
        let line = |k: &str| unit.lines().find(|l| l.starts_with(k)).unwrap_or_else(|| panic!("no {k} in\n{unit}"));
        assert_eq!(line("User="), "User=ci");
        assert_eq!(line("ExecStart="), format!("ExecStart=/usr/bin/rexenv {HEADLESS_FLAG}"));
        assert_eq!(HEADLESS_FLAG, crate::HEADLESS_FLAG, "the unit must start the flag the app reads");
        assert_eq!(line("Restart="), "Restart=on-failure");
        // The stack outlives the app: only the main process is killed on stop or restart.
        assert_eq!(line("KillMode="), "KillMode=process");
        // The DNS agent's user manager: reachable, and up before the app.
        assert_eq!(line("Environment=XDG_RUNTIME_DIR="), "Environment=XDG_RUNTIME_DIR=/run/user/1001");
        assert!(line("After=").contains("user@1001.service") && line("Wants=").contains("user@1001.service"));
        assert_eq!(line("WantedBy="), "WantedBy=multi-user.target");
        assert!(!unit.contains("User=root"));
        for bad in ["root\nExecStartPre=/bin/sh", "a b", "", "Dev"] {
            assert_eq!(service_unit(bad, 1001), None, "{bad:?}");
        }
    }

    #[test]
    fn the_setup_enables_the_unit_and_lingers_with_absolute_paths() {
        let [reload, enable, linger] = install_commands("ci");
        assert_eq!(reload, [SYSTEMCTL, "daemon-reload"]);
        assert_eq!(enable, [SYSTEMCTL, "enable", SERVICE_UNIT]);
        assert_eq!(linger, [LOGINCTL, "enable-linger", "ci"]);
        assert!(SERVICE_PATH.ends_with(SERVICE_UNIT) && SERVICE_STAGING.starts_with("/etc/systemd/system/."));
        // The setup writes the unit through the staging file and runs exactly these commands.
        let door = include_str!("headless.rs");
        let setup = &door[door.find("fn setup(argv: &[String])").expect("the setup")..];
        assert!(setup.contains("rules::service_unit(&user, account.uid)"));
        assert!(setup.contains("std::fs::rename(rules::SERVICE_STAGING, rules::SERVICE_PATH)?;"));
        assert!(setup.contains("for argv in rules::install_commands(&user) {"));
    }

    #[test]
    fn the_first_display_with_no_socket_and_no_lock_wins() {
        assert_eq!(free_display(|_| false), Some(99));
        let taken = |p: &Path| p.ends_with("X99") || p.ends_with(".X100-lock");
        assert_eq!(free_display(taken), Some(101));
        assert_eq!(free_display(|_| true), None);
    }

    #[test]
    fn xvfb_listens_on_no_tcp_and_needs_our_cookie() {
        let args = xvfb_args(99, Path::new("/home/u/.config/rexenv/headless/Xauthority"));
        assert_eq!(args[0], ":99");
        assert!(args.windows(2).any(|w| w == ["-nolisten", "tcp"]));
        assert!(args.windows(2).any(|w| w[0] == "-auth" && w[1].ends_with("Xauthority")));
        assert_eq!(xauth_args(99, Path::new("/a"), "ff")[4..], [":99".to_string(), ".".into(), "ff".into()]);
    }
}

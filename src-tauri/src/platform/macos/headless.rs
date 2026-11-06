//! `sudo rexenv --headless-setup` on macOS (docs/PLAN-headless-web.md T5 M3): the administrator's
//! one-time step for a Mac that runs rexenv headless. The rules and words are `headless_rules.rs`.
//!
//! **Why root, when nothing it writes is root's.** The setup writes D7's two consents (the MCP endpoint and
//! the Agent access dial), which only an administrator's setup may write outside the GUI (ledger #920):
//! run as the user, it would be a door any of the user's own processes — an agent included — could open
//! to widen its own access. `sudo` is the administrator's password. Everything it writes is then written
//! AS the account, from a forked child that dropped root, so no root-owned file lands in the user's home
//! or database.
//!
//! **Why it checks the app's setup instead of doing it** (Linux's makes the route, the CA trust and the
//! edge itself, #923): on macOS trusting the certificate asks the user to confirm on the screen, which no
//! terminal can answer. So a Mac that has not finished the app's own setup is told to, and nothing changes.
//!
//! **Why a LaunchAgent** (M2, measured): a system LaunchDaemon, even with `UserName`, cannot read the
//! user's keychain, and rexenv started there read its own certificate as untrusted.

use super::headless_rules as rules;
use crate::core::headless_consent::Consent;
use crate::error::{Error, Result};
use std::path::PathBuf;

/// The account rexenv runs as, from the user database.
struct Account {
    name: String,
    uid: libc::uid_t,
    gid: libc::gid_t,
    home: PathBuf,
}

/// `None` when argv is not the setup; otherwise the exit code, after printing what was done.
pub(crate) fn run_setup(argv: &[String]) -> Option<i32> {
    argv.iter().position(|a| a == rules::SETUP_FLAG)?;
    Some(match setup(argv) {
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
            0
        }
        Err(e) => {
            eprintln!("rexenv --headless-setup: {e}");
            1
        }
    })
}

fn setup(argv: &[String]) -> Result<Vec<String>> {
    // SAFETY: a plain syscall with no arguments.
    if unsafe { libc::geteuid() } != 0 {
        return Err(Error::Other(rules::NEEDS_ROOT.into()));
    }
    let args = rules::parse_setup_args(argv).map_err(Error::Other)?;
    let user = args.user.clone().or_else(|| std::env::var("SUDO_USER").ok()).filter(|u| u != "root").ok_or_else(|| {
        Error::Other("say which account rexenv runs as: run it with sudo from that account, or pass --user <name>".into())
    })?;
    let account = account(&user)?;
    let exe = std::env::current_exe()?;
    if !rules::is_app_bundle_binary(&exe) || super::app_bundle::is_translocated(&exe) {
        return Err(Error::Other(rules::NEEDS_APP_BINARY.into()));
    }
    // Typed consent wins; with none, ask — and with no terminal to ask in, change neither. Asked before the
    // first change, so an interrupted setup has changed nothing.
    let consent = if !args.consent.is_empty() {
        args.consent
    } else if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        ask()?
    } else {
        Consent::default()
    };
    let web = args.web;
    let mut said = vec![format!("rexenv --headless-setup for {} ({}).", account.name, exe.display())];
    said.extend(as_user(&account, || prepare_as_user(&exe, web, &consent))?);
    // Load it now when the account is the one at the console; otherwise it loads at its next login.
    let plist = account.home.join("Library/LaunchAgents").join(format!("{}.plist", rules::AGENT_LABEL));
    use std::os::unix::fs::MetadataExt;
    let console = std::fs::metadata("/dev/console").ok().map(|m| m.uid());
    if console == Some(account.uid) {
        let domain = format!("gui/{}", account.uid);
        // A re-run replaces the loaded copy: out first (an absent one is fine), then in.
        let _ = std::process::Command::new("/bin/launchctl").args(["bootout", &format!("{domain}/{}", rules::AGENT_LABEL)]).output();
        let out = std::process::Command::new("/bin/launchctl").args(["bootstrap", &domain]).arg(&plist).output()?;
        if out.status.success() {
            said.push(format!(
                "✓ started now in {}'s session. If the rexenv app was open, this copy handed off to it and exited — quit \
                 the app, then: launchctl kickstart gui/{}/{}",
                account.name,
                account.uid,
                rules::AGENT_LABEL
            ));
        } else {
            said.push(format!(
                "could not start it now ({}); it starts at {}'s next login.",
                String::from_utf8_lossy(&out.stderr).trim(),
                account.name
            ));
        }
    } else {
        said.push(format!("{} is not logged in at the screen now; it starts at the next login.", account.name));
    }
    said.push(rules::AUTOLOGIN.into());
    Ok(said)
}

/// What the child does as the account: check the app's setup, write the LaunchAgent, retire the app's own
/// login item (two launchers at one login would race for the single instance), write the consents.
fn prepare_as_user(exe: &std::path::Path, web: bool, consent: &Consent) -> Result<Vec<String>> {
    use crate::core::{dns, ssl, tld};
    let platform = crate::platform::current();
    let mut missing = Vec::new();
    if platform.dns().route_owner(tld::BACKBONE_TLD, dns::DEFAULT_DNS_PORT) == dns::ResolverOwner::Absent {
        missing.push("the .rex resolver");
    }
    let trusted = ssl::ca_dir(platform.paths())
        .map(|dir| dir.join(ssl::CA_CERT_FILE))
        .is_ok_and(|cert| cert.exists() && platform.cert_trust().is_trusted(&cert));
    if !trusted {
        missing.push("the trusted certificate");
    }
    if !platform.edge().is_installed() {
        missing.push("the HTTPS server");
    }
    if !missing.is_empty() {
        return Err(Error::Other(rules::needs_app_setup(&missing)));
    }
    let mut said = vec![format!("✓ .{} resolves, the certificate is trusted and the HTTPS server is installed.", tld::BACKBONE_TLD)];
    let home = PathBuf::from(std::env::var_os("HOME").ok_or_else(|| Error::Other("HOME is not set".into()))?);
    let agents = home.join("Library/LaunchAgents");
    std::fs::create_dir_all(&agents)?;
    let log_dir = platform.paths().log_dir()?;
    std::fs::create_dir_all(&log_dir)?;
    let plist = agents.join(format!("{}.plist", rules::AGENT_LABEL));
    crate::platform::durable::write_durable(&plist, rules::agent_plist(exe, web, &log_dir.join("headless.log")).as_bytes())?;
    said.push(format!("✓ {} runs rexenv headless at this account's login.", plist.display()));
    said.push(if web {
        "✓ web UI: on — 127.0.0.1:9443 only; `rex web url` prints its sign-in link.".to_string()
    } else {
        format!("web UI: off (re-run with {} to serve it on 127.0.0.1:9443).", rules::ENABLE_WEB_FLAG)
    });
    let autostart = platform.autostart();
    if autostart.is_enabled().unwrap_or(false) {
        autostart.disable()?;
        said.push("✓ the app's own Start at login is off — the headless agent replaces it.".into());
    }
    if consent.is_empty() {
        said.push(rules::CONSENT_UNCHANGED.into());
    } else {
        let conn = crate::state::db::open_for_platform(platform.paths())?;
        said.extend(crate::core::headless_consent::apply(&conn, consent)?.into_iter().map(|l| format!("✓ {l}")));
    }
    Ok(said)
}

/// Run `f` in a forked child that became `account`, and return what it said. Called from `main.rs` before
/// any other thread exists (the setup is checked before Tauri boots), which is what makes the fork sound.
fn as_user(account: &Account, f: impl FnOnce() -> Result<Vec<String>>) -> Result<Vec<String>> {
    use std::io::{Read, Write};
    use std::os::fd::FromRawFd;
    let name = std::ffi::CString::new(account.name.as_str()).map_err(|_| Error::Other("a user name with a NUL".into()))?;
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: `pipe` fills the two-element array it is handed.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: one thread (see above); the child drops privileges, does `f`, writes and `_exit`s.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    if pid == 0 {
        // SAFETY: the read end is the parent's; the write end is ours from here.
        let mut out = unsafe {
            libc::close(fds[0]);
            std::fs::File::from_raw_fd(fds[1])
        };
        let result = if become_user(account, &name) {
            f()
        } else {
            Err(Error::Other(format!("could not become {} to write its files", account.name)))
        };
        let code = match result {
            Ok(lines) => {
                let _ = out.write_all(lines.join("\n").as_bytes());
                0
            }
            Err(e) => {
                let _ = out.write_all(e.to_string().as_bytes());
                1
            }
        };
        drop(out);
        // SAFETY: the child's end; no destructors of the parent's state run.
        unsafe { libc::_exit(code) };
    }
    // SAFETY: the write end is the child's.
    let mut input = unsafe {
        libc::close(fds[1]);
        std::fs::File::from_raw_fd(fds[0])
    };
    let mut text = String::new();
    input.read_to_string(&mut text)?;
    let mut status = 0;
    // SAFETY: waiting for the child forked above.
    unsafe { libc::waitpid(pid, &mut status, 0) };
    if libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0 {
        Ok(text.lines().map(str::to_string).collect())
    } else if text.is_empty() {
        Err(Error::Other("the setup's step as the account did not finish".into()))
    } else {
        Err(Error::Other(text))
    }
}

/// Drop root for good: groups, gid, uid — and prove root cannot be taken back.
fn become_user(account: &Account, name: &std::ffi::CString) -> bool {
    // SAFETY: plain syscalls on values owned here. `initgroups` takes the gid as `c_int` on macOS.
    let dropped = unsafe {
        libc::initgroups(name.as_ptr(), account.gid as libc::c_int) == 0
            && libc::setgid(account.gid) == 0
            && libc::setuid(account.uid) == 0
            && libc::getuid() == account.uid
            && libc::geteuid() == account.uid
            && libc::setuid(0) != 0
    };
    if dropped {
        std::env::set_var("HOME", &account.home);
        std::env::set_var("USER", &account.name);
        std::env::set_var("LOGNAME", &account.name);
        let _ = std::env::set_current_dir(&account.home);
    }
    dropped
}

fn account(name: &str) -> Result<Account> {
    use std::ffi::{CStr, CString};
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(name).map_err(|_| Error::Other(format!("{name:?} is not a user name")))?;
    // SAFETY: `getpwnam_r` writes only into `pw` and `buf`, both owned here and sized for it.
    let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
    let mut buf = vec![0 as libc::c_char; 16 * 1024];
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    let rc = unsafe { libc::getpwnam_r(c.as_ptr(), &mut pw, buf.as_mut_ptr(), buf.len(), &mut found) };
    if rc != 0 || found.is_null() {
        return Err(Error::Other(format!("there is no account named {name:?} on this Mac")));
    }
    if pw.pw_uid == 0 {
        return Err(Error::Other("rexenv does not run as root — name the account it runs as with --user <name>".into()));
    }
    // SAFETY: `pw_dir` points into `buf`, alive until the end of this function.
    let home = std::ffi::OsStr::from_bytes(unsafe { CStr::from_ptr(pw.pw_dir) }.to_bytes());
    Ok(Account { name: name.to_string(), uid: pw.pw_uid, gid: pw.pw_gid, home: home.into() })
}

/// Ask both consents in the terminal, with the Settings card's own sentences (D7).
fn ask() -> Result<Consent> {
    use crate::core::headless_consent::{access_question, mcp_question, parse_level, parse_yes};
    fn prompt<T>(question: &str, parse: impl Fn(&str) -> Option<T>) -> Result<T> {
        use std::io::Write;
        loop {
            print!("{question}");
            std::io::stdout().flush()?;
            let mut line = String::new();
            if std::io::stdin().read_line(&mut line)? == 0 {
                return Err(Error::Other("no answer (end of input) — nothing was changed".into()));
            }
            if let Some(v) = parse(&line) {
                return Ok(v);
            }
        }
    }
    let mcp = prompt(&mcp_question(), parse_yes)?;
    let access = if mcp { Some(prompt(&format!("\n{}", access_question()), parse_level)?) } else { None };
    Ok(Consent { mcp: Some(mcp), access })
}

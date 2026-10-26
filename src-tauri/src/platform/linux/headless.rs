//! Headless mode on Linux (docs/PLAN-headless-web.md T1, owner ruling D1 = Xvfb): the whole app on
//! a server with no display, driven by `rex` and `rex mcp`.
//!
//! **Why Xvfb and not "no window".** tao builds its event loop with `gtk::init()`, before Tauri's
//! `setup` and before any window exists, and with no display that panics (`Failed to initialize
//! gtk backend!`, exit 101 — T0 on the Ubuntu VM). So rexenv starts its OWN Xvfb first, points
//! GTK at it, and never shows a window there. The rules (display choice, argv, the passwordless
//! set) are `headless_rules.rs`; this file runs them.
//!
//! **The display is rexenv's alone.** Xvfb gets `-nolisten tcp` and a random cookie in an
//! owner-only file: without it any local user could connect and drive the hidden window. It dies
//! with rexenv — `PR_SET_PDEATHSIG` from the MAIN thread, which lives as long as the process
//! (the thread-tied death signal is why the tunnel guard is a separate process; here the spawner
//! is the one thread that never retires).

use super::headless_rules as rules;
use crate::core::headless_consent::Consent;
use crate::error::{Error, Result};
use std::path::Path;
use std::time::{Duration, Instant};

/// How long one Xvfb gets to create its socket before the next display number is tried.
const XVFB_READY: Duration = Duration::from_secs(5);
/// Display numbers tried before giving up — a race with another X server costs one each.
const XVFB_TRIES: u32 = 5;

/// Start rexenv's own Xvfb and point this process (and so GTK) at it. Called by `main.rs` on the
/// main thread, before Tauri boots and before any other thread exists.
pub(crate) fn prepare() -> Result<()> {
    if !Path::new(rules::XVFB).is_file() || !Path::new(rules::XAUTH).is_file() {
        return Err(Error::Other(rules::XVFB_MISSING.into()));
    }
    let paths = super::LinuxPaths;
    let dir = crate::platform::traits::Paths::config_dir(&paths)?.join("headless");
    std::fs::create_dir_all(&dir)?;
    let auth = dir.join("Xauthority");
    // Created here: on a fresh account nothing has made the log folder yet, and opening the log
    // inside a missing one failed the whole launch with a bare ENOENT (a new user on the VM, T2 S4).
    let log_dir = crate::platform::traits::Paths::log_dir(&paths)?;
    std::fs::create_dir_all(&log_dir)?;
    let log = log_dir.join("xvfb.log");
    let mut last = String::new();
    for _ in 0..XVFB_TRIES {
        let Some(display) = rules::free_display(|p| p.exists()) else {
            return Err(Error::Other("no free X display number between :99 and :199 for rexenv's Xvfb".into()));
        };
        write_cookie(display, &auth)?;
        match spawn_xvfb(display, &auth, &log) {
            Ok(()) => {
                // One thread, before boot: nothing else reads the environment yet.
                std::env::set_var("DISPLAY", format!(":{display}"));
                std::env::set_var("XAUTHORITY", &auth);
                std::env::set_var("GDK_BACKEND", "x11");
                std::env::remove_var("WAYLAND_DISPLAY");
                // No session bus for GTK. Measured on the VM (T1, 26 Oct 2026): with the user's bus
                // reachable, GTK's startup waited on xdg-desktop-portal, which waits ~25 s for its GNOME
                // backend — a backend that never comes with no desktop — and the window-less app took
                // 31 s to reach `setup` (`rex` timed out meanwhile); with no bus, 1 s. Nothing headless
                // needs that bus (the tray and "reveal in Files" are desktop features), and a system
                // service on a real server has none anyway. The path is ours and never exists.
                std::env::set_var("DBUS_SESSION_BUS_ADDRESS", format!("unix:path={}", dir.join("no-session-bus").display()));
                std::env::set_var("NO_AT_BRIDGE", "1");
                return Ok(());
            }
            Err(e) => last = e.to_string(),
        }
    }
    Err(Error::Other(format!("rexenv's Xvfb did not start ({last}) — see {}", log.display())))
}

/// A fresh random cookie for `display`, in an owner-only file Xvfb and GTK both read.
fn write_cookie(display: u32, auth: &Path) -> Result<()> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let _ = std::fs::remove_file(auth);
    std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(auth)?;
    let out = crate::platform::command(rules::XAUTH).args(rules::xauth_args(display, auth, &hex)).output()?;
    if !out.status.success() {
        return Err(Error::Other(format!("xauth could not write the cookie: {}", String::from_utf8_lossy(&out.stderr).trim())));
    }
    Ok(())
}

fn spawn_xvfb(display: u32, auth: &Path, log: &Path) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let out = std::fs::OpenOptions::new().create(true).append(true).open(log)?;
    let mut cmd = crate::platform::command(rules::XVFB);
    cmd.args(rules::xvfb_args(display, auth)).stdin(std::process::Stdio::null()).stdout(out.try_clone()?).stderr(out);
    // SAFETY: `prctl` is async-signal-safe and touches only the child's own state.
    unsafe {
        cmd.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = cmd.spawn()?;
    let [socket, _] = rules::display_paths(display);
    let started = Instant::now();
    while started.elapsed() < XVFB_READY {
        if let Some(status) = child.try_wait()? {
            return Err(Error::Other(format!("Xvfb :{display} exited ({status})")));
        }
        if socket.exists() {
            // The child is deliberately not waited on: it lives until this process dies.
            std::mem::forget(child);
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    Err(Error::Other(format!("Xvfb :{display} made no socket in {}s", XVFB_READY.as_secs())))
}

/// Run a script a headless instance was asked to run as root: only the edge's own start/stop,
/// through `sudo -n` and the rule the setup wrote; anything else is refused with the sentence.
pub(crate) fn run_privileged(script: &str) -> Result<String> {
    let Some(steps) = rules::steps_for(script) else {
        return Err(Error::Other(rules::NEEDS_SETUP.into()));
    };
    let mut said = String::new();
    for step in steps {
        let out = crate::platform::command(rules::SUDO).arg("-n").args(&step.argv).output()?;
        let stderr = String::from_utf8_lossy(&out.stderr);
        if !out.status.success() && (step.must_succeed || rules::sudo_refused(&stderr)) {
            return Err(Error::Other(rules::sudo_refusal(&stderr)));
        }
        said.push_str(String::from_utf8_lossy(&out.stdout).trim_end());
    }
    // `enable`/`disable` write systemd's boot links: on disk before this returns, as every
    // privileged step's are (`durable::flushed_script`, ledger #740). `sync(2)` needs no root.
    let _ = crate::platform::command(rules::SYNC).status();
    Ok(said)
}

/// `sudo rexenv --headless-setup [--user <name>] [--enable-mcp] [--agent-access <level>]`: the
/// administrator's one-time step. Writes the passwordless rule for the account rexenv runs as
/// (checked by `visudo` before it takes effect), and the two consents D7 moved here from the
/// Settings screen — typed as flags, or asked in the terminal with the card's own sentences
/// (`core::headless_consent`). `None` when argv is not the setup.
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

/// The account rexenv runs as, read from the passwd database.
struct Account {
    name: String,
    uid: libc::uid_t,
    gid: libc::gid_t,
    home: std::path::PathBuf,
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
        return Err(Error::Other(format!("there is no account named {name:?} on this machine")));
    }
    if pw.pw_uid == 0 {
        return Err(Error::Other("rexenv does not run as root — name the account it runs as with --user <name>".into()));
    }
    // SAFETY: `pw_dir` points into `buf`, alive until the end of this function.
    let home = std::ffi::OsStr::from_bytes(unsafe { CStr::from_ptr(pw.pw_dir) }.to_bytes());
    Ok(Account { name: name.to_string(), uid: pw.pw_uid, gid: pw.pw_gid, home: home.into() })
}

/// Ask both consents in the terminal, before anything is changed — so an interrupted setup has
/// changed nothing. The endpoint first; the dial only when the endpoint is on (it governs nothing
/// otherwise, and the card shows it under the toggle for the same reason).
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

fn setup(argv: &[String]) -> Result<Vec<String>> {
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: a plain syscall with no arguments.
    if unsafe { libc::geteuid() } != 0 {
        return Err(Error::Other("run it as an administrator:\n$ sudo rexenv --headless-setup".into()));
    }
    let args = rules::parse_setup_args(argv).map_err(Error::Other)?;
    let user = args.user.or_else(|| std::env::var("SUDO_USER").ok()).filter(|u| u != "root").ok_or_else(|| {
        Error::Other("say which account rexenv runs as: run it with sudo from that account, or pass --user <name>".into())
    })?;
    let contents = rules::sudoers_contents(&user)
        .ok_or_else(|| Error::Other(format!("{user:?} is not a user name rexenv will write into sudoers")))?;
    let account = account(&user)?;
    // Everything that can refuse, refuses before the first change.
    if !Path::new(rules::REXENV_BIN).is_file() {
        return Err(Error::Other(rules::NEEDS_DEB.into()));
    }
    let unit = rules::service_unit(&user, account.uid)
        .ok_or_else(|| Error::Other(format!("{user:?} is not a user name rexenv will write into a unit")))?;
    // Typed consent wins; with none, ask — and with no terminal to ask in, change neither.
    // SAFETY: a plain syscall on fd 0.
    let consent = if !args.consent.is_empty() {
        args.consent
    } else if unsafe { libc::isatty(0) } == 1 {
        ask()?
    } else {
        Consent::default()
    };
    std::fs::write(rules::SUDOERS_STAGING, contents)?;
    std::fs::set_permissions(rules::SUDOERS_STAGING, std::fs::Permissions::from_mode(0o440))?;
    let check = crate::platform::command(rules::VISUDO).args(["-cf", rules::SUDOERS_STAGING]).output()?;
    if !check.status.success() {
        let _ = std::fs::remove_file(rules::SUDOERS_STAGING);
        return Err(Error::Other(format!(
            "visudo refused the rule, so nothing was changed: {}",
            String::from_utf8_lossy(&check.stderr).trim()
        )));
    }
    std::fs::rename(rules::SUDOERS_STAGING, rules::SUDOERS_PATH)?;
    let mut said = vec![format!(
        "✓ {user} may now start and stop rexenv's edge without a password ({}). Nothing else was granted.",
        rules::SUDOERS_PATH
    )];
    // The boot service (T4): staged, renamed into place, loaded, enabled — and the user's own
    // systemd kept running with no login, for the DNS agent's user unit.
    std::fs::write(rules::SERVICE_STAGING, unit)?;
    std::fs::set_permissions(rules::SERVICE_STAGING, std::fs::Permissions::from_mode(0o644))?;
    std::fs::rename(rules::SERVICE_STAGING, rules::SERVICE_PATH)?;
    for argv in rules::install_commands(&user) {
        run_root(&argv)?;
    }
    said.push(format!(
        "✓ {} starts rexenv headless as {user} at every boot, with no one logged in (lingering on for {user}).",
        rules::SERVICE_PATH
    ));
    if consent.is_empty() {
        said.push(rules::CONSENT_UNCHANGED.into());
    } else {
        said.extend(write_as_user(&account, &consent)?.into_iter().map(|l| format!("✓ {l}")));
    }
    // A running service reads the endpoint's setting at launch: restart it so the consent applies.
    // `KillMode=process` — only the app restarts; the stack it started keeps running and is adopted.
    let active = crate::platform::command(rules::SYSTEMCTL).args(["is-active", "--quiet", rules::SERVICE_UNIT]).status()?.success();
    if active && !consent.is_empty() {
        run_root(&[rules::SYSTEMCTL.into(), "restart".into(), rules::SERVICE_UNIT.into()])?;
        said.push("✓ restarted rexenv-headless.service so the settings apply.".into());
    } else if !active {
        said.push(format!(
            "Start it now (stop any rexenv you started by hand first — a second one hands off and exits):\n$ sudo systemctl start {}",
            rules::SERVICE_UNIT
        ));
    }
    Ok(said)
}

/// One of the setup's own root commands (absolute argv from `headless_rules`), failing the setup
/// with its output when it fails.
fn run_root(argv: &[String]) -> Result<()> {
    let out = crate::platform::command(&argv[0]).args(&argv[1..]).output()?;
    if !out.status.success() {
        return Err(Error::Other(format!("`{}` failed: {}", argv.join(" "), String::from_utf8_lossy(&out.stderr).trim())));
    }
    Ok(())
}

/// Write the consent into `account`'s own database, as that account: a forked child drops to its
/// groups, gid and uid (and checks it cannot climb back) before it opens anything, so the files it
/// creates are the user's and no link the user planted in their data folder is followed as root.
/// A fork and not a re-exec on purpose: an argv mode that writes these keys would be one the
/// user's own processes — and so an agent — could run (ledger #920). `main.rs` reaches the setup
/// on its first thread, before anything else starts one, which is what makes the fork sound.
fn write_as_user(account: &Account, consent: &Consent) -> Result<Vec<String>> {
    use std::ffi::CString;
    use std::io::{Read, Write};
    use std::os::fd::FromRawFd;
    let name = CString::new(account.name.as_str()).map_err(|_| Error::Other("a user name with a NUL".into()))?;
    // The fork's soundness, checked rather than assumed: a second thread could hold a lock the
    // child would then wait on forever.
    if std::fs::read_dir("/proc/self/task").map(|d| d.count()).unwrap_or(0) != 1 {
        return Err(Error::Other("the setup is not running on one thread, so it will not fork to write the settings".into()));
    }
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: `pipe` fills the two-element array it is handed.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: one thread (see above); the child only drops privileges, writes and `_exit`s.
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
        // SAFETY: plain syscalls on values owned here. Groups first, then the gid, then the uid —
        // the only order that works (each later one gives up the right to the earlier) — and then
        // proof it took: our uid is theirs, and root cannot be taken back.
        let dropped = unsafe {
            libc::initgroups(name.as_ptr(), account.gid) == 0
                && libc::setgid(account.gid) == 0
                && libc::setuid(account.uid) == 0
                && libc::getuid() == account.uid
                && libc::geteuid() == account.uid
                && libc::setuid(0) != 0
        };
        let result = if !dropped {
            Err(Error::Other(format!("could not become {} to write the settings", account.name)))
        } else {
            // The user's own paths: `directories` reads HOME, and root's XDG overrides must not leak.
            std::env::set_var("HOME", &account.home);
            std::env::set_var("USER", &account.name);
            std::env::set_var("LOGNAME", &account.name);
            for v in ["XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME"] {
                std::env::remove_var(v);
            }
            let _ = std::env::set_current_dir(&account.home);
            crate::state::db::open_for_platform(&super::LinuxPaths)
                .and_then(|conn| crate::core::headless_consent::apply(&conn, consent))
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
        // SAFETY: leave without running the parent's exit handlers twice.
        unsafe { libc::_exit(code) }
    }
    // SAFETY: the write end is the child's; the read end is ours from here.
    let mut from_child = unsafe {
        libc::close(fds[1]);
        std::fs::File::from_raw_fd(fds[0])
    };
    let mut said = String::new();
    let _ = from_child.read_to_string(&mut said);
    let mut status = 0;
    // SAFETY: waits for the child forked above.
    if unsafe { libc::waitpid(pid, &mut status, 0) } != pid || !libc::WIFEXITED(status) || libc::WEXITSTATUS(status) != 0 {
        return Err(Error::Other(format!(
            "the passwordless rule is in place, but the MCP / Agent access settings were NOT written: {}",
            if said.is_empty() { "the writer did not finish" } else { said.as_str() }
        )));
    }
    Ok(said.lines().map(String::from).collect())
}

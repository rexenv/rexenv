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
    let log = crate::platform::traits::Paths::log_dir(&paths)?.join("xvfb.log");
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

/// `sudo rexenv --headless-setup [--user <name>]`: write the passwordless rule for the account
/// rexenv runs as, checked by `visudo` before it takes effect. `None` when argv is not the setup.
pub(crate) fn run_setup(argv: &[String]) -> Option<i32> {
    argv.iter().position(|a| a == rules::SETUP_FLAG)?;
    Some(match setup(argv) {
        Ok(user) => {
            println!(
                "✓ {user} may now start and stop rexenv's edge without a password ({}). Nothing else was granted.",
                rules::SUDOERS_PATH
            );
            0
        }
        Err(e) => {
            eprintln!("rexenv --headless-setup: {e}");
            1
        }
    })
}

fn setup(argv: &[String]) -> Result<String> {
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: a plain syscall with no arguments.
    if unsafe { libc::geteuid() } != 0 {
        return Err(Error::Other("run it as an administrator:\n$ sudo rexenv --headless-setup".into()));
    }
    let named = argv.iter().position(|a| a == "--user").and_then(|i| argv.get(i + 1)).cloned();
    let user = named.or_else(|| std::env::var("SUDO_USER").ok()).filter(|u| u != "root").ok_or_else(|| {
        Error::Other("say which account rexenv runs as: run it with sudo from that account, or pass --user <name>".into())
    })?;
    let contents = rules::sudoers_contents(&user)
        .ok_or_else(|| Error::Other(format!("{user:?} is not a user name rexenv will write into sudoers")))?;
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
    Ok(user)
}

//! macOS implementations of the platform traits (Phase 1 target OS).
//!
//! Trivial, dependency-free pieces (paths, permissions, process spawn, shell)
//! are implemented now. Pieces that need later Phase 1 tasks (DNS resolver,
//! CA trust, privileged port binding, autostart, binary download) are wired up
//! as `todo!()` so the architecture is complete and `cargo check` passes.

pub mod activation;
pub mod app_bundle;
mod keychain_trust;
pub mod relauncher;
pub mod parent_death_guard;
mod prompt_applet;
pub mod webview_dialogs;

use crate::error::{Error, Result};
use crate::platform::traits::*;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::Duration;

// The ONE canonical reverse-DNS identity (release task 1.1). It must match the
// `identifier` in tauri.conf.json (the bundle id / signing id), the app-data
// namespace below, and the launchd autostart label. The drift-guard tests at the
// bottom of this file fail the build if any of these diverge.
pub const APP_IDENTIFIER: &str = "dev.rexenv.rexenv";

// app-data namespace parts: `directories::ProjectDirs::from(qualifier, org, name)`
// composes these to `<qualifier>.<org>.<name>` = APP_IDENTIFIER on macOS. Defined
// in platform/mod.rs, shared with Windows — one namespace for every OS.
use crate::platform::{APP_NAME, APP_ORG, APP_QUALIFIER};

pub struct MacosPaths;
impl Paths for MacosPaths {
    fn app_data_dir(&self) -> Result<PathBuf> {
        let dirs = directories::ProjectDirs::from(APP_QUALIFIER, APP_ORG, APP_NAME)
            .ok_or(Error::Other("cannot resolve home directory".into()))?;
        Ok(dirs.data_dir().to_path_buf())
    }
    fn config_dir(&self) -> Result<PathBuf> {
        Ok(self.app_data_dir()?.join("config"))
    }
    fn log_dir(&self) -> Result<PathBuf> {
        Ok(self.app_data_dir()?.join("logs"))
    }
    fn bin_dir(&self) -> Result<PathBuf> {
        Ok(self.app_data_dir()?.join("bin"))
    }
    fn hosts_file(&self) -> PathBuf {
        PathBuf::from("/etc/hosts")
    }
    fn cli_symlink_path(&self) -> Result<PathBuf> {
        Ok(PathBuf::from("/usr/local/bin/rex"))
    }
}

pub struct MacosDns;

/// The directory macOS reads resolver files from.
const RESOLVER_DIR: &str = "/etc/resolver";

impl MacosDns {
    /// macOS reads /etc/resolver/<domain> — one file per development TLD.
    pub(crate) fn resolver_path(&self, tld: &str) -> PathBuf {
        PathBuf::from(RESOLVER_DIR).join(tld)
    }

    /// macOS resolver(5): send the TLD to our loopback resolver on `port`. Identical for every TLD —
    /// this content is the ownership signature rexenv enumerates its files by.
    pub(crate) fn resolver_contents(&self, port: u16) -> String {
        format!("nameserver 127.0.0.1\nport {port}\n")
    }
}

impl DnsManager for MacosDns {
    fn route_label(&self, tld: &str) -> String {
        self.resolver_path(tld).display().to_string()
    }

    fn route_contents(&self, port: u16) -> String {
        self.resolver_contents(port)
    }

    fn route_owner(&self, tld: &str, port: u16) -> ResolverOwner {
        crate::platform::resolver_files::owner_of(&self.resolver_path(tld), &self.resolver_contents(port))
    }

    fn our_route_tlds(&self, port: u16) -> Vec<String> {
        crate::platform::resolver_files::tlds_matching_signature(Path::new(RESOLVER_DIR), &self.resolver_contents(port))
    }

    fn foreign_route_tlds(&self, port: u16) -> Vec<String> {
        crate::platform::resolver_files::tlds_not_matching_signature(
            Path::new(RESOLVER_DIR),
            &self.resolver_contents(port),
        )
    }

    fn install_command(&self, tld: &str, port: u16) -> String {
        let path = self.resolver_path(tld);
        let dir = path
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "/etc/resolver".into());
        // Use printf so the content survives the AppleScript→sh escaping chain
        // (literal newlines in the file are written as \n for printf). Then flush
        // the DNS cache so macOS picks up the new resolver file immediately.
        let printf_arg = self.resolver_contents(port).replace('\n', "\\n");
        format!(
            "mkdir -p {dir} && printf '{printf_arg}' > {} \
             && dscacheutil -flushcache && killall -HUP mDNSResponder",
            path.display()
        )
    }

    fn uninstall_command(&self, tlds: &[String]) -> String {
        // Remove every listed resolver file, then ONE DNS-cache flush, so the
        // TLDs stop resolving immediately (mirror of install_command's flush).
        // Every `tld` here is an [a-z]{1,63} label: install-time values pass
        // `tld::ensure_allowed`, and the teardown sweep filters scanned filenames
        // through `tld::is_valid_label` (platform::resolver_files::tlds_matching_signature), so
        // no shell-metachar name can reach this root `rm` (B10).
        let files = tlds
            .iter()
            .map(|t| self.resolver_path(t).display().to_string())
            .collect::<Vec<_>>()
            .join(" ");
        if files.is_empty() {
            // Nothing to remove — `rm -f` with zero operands would error.
            return "dscacheutil -flushcache && killall -HUP mDNSResponder".into();
        }
        format!("rm -f {files} && dscacheutil -flushcache && killall -HUP mDNSResponder")
    }

    fn restore_command(&self, restores: &[(String, PathBuf)]) -> String {
        // `cp` the backup back into place — their file's bytes never enter this
        // root shell string (see the trait doc). `chmod 644` because the copy
        // inherits our 0600 backup's mode, and a resolver file the system can't
        // read is worse than useless. One flush for the batch, like its siblings.
        let cmds = restores
            .iter()
            .map(|(tld, backup)| {
                let dest = self.resolver_path(tld);
                format!(
                    "cp {} {} && chmod 644 {}",
                    sh_quote(backup),
                    dest.display(),
                    dest.display()
                )
            })
            .collect::<Vec<_>>()
            .join(" && ");
        if cmds.is_empty() {
            return "dscacheutil -flushcache && killall -HUP mDNSResponder".into();
        }
        format!("{cmds} && dscacheutil -flushcache && killall -HUP mDNSResponder")
    }
}

pub struct MacosCertTrust;

impl MacosCertTrust {
    /// Path to the user's login keychain. The CA is added HERE explicitly, never
    /// to "the default keychain" (which a user can change) and never to the
    /// System keychain (root, and trust is per-user by design).
    fn login_keychain() -> String {
        directories::BaseDirs::new()
            .map(|b| {
                b.home_dir()
                    .join("Library/Keychains/login.keychain-db")
                    .display()
                    .to_string()
            })
            .unwrap_or_else(|| "login.keychain-db".into())
    }


}

impl CertTrustManager for MacosCertTrust {
    // In-process, not `security add-trusted-cert`: the keychain dialog is named
    // after the process that calls the trust API (`keychain_trust`, measured).
    fn trust_ca(&self, ca_cert_path: &Path) -> Result<()> {
        keychain_trust::trust(ca_cert_path, &Self::login_keychain())
    }
    fn untrust_ca(&self, ca_cert_path: &Path) -> Result<()> {
        keychain_trust::untrust(ca_cert_path)
    }
    fn is_trusted(&self, ca_cert_path: &Path) -> bool {
        // `security verify-cert` exits 0 iff the cert chains to a trust anchor
        // for THIS user (login-keychain trust settings included); an untrusted
        // local CA fails with CSSMERR_TP_NOT_TRUSTED. Verified empirically.
        std::process::Command::new("security")
            .args(["verify-cert", "-c"])
            .arg(ca_cert_path)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    fn firefox_profiles_root(&self) -> Option<std::path::PathBuf> {
        // Present only if Firefox has ever run for this user (the dir + a
        // profiles.ini are created on first launch).
        let root = directories::BaseDirs::new()?
            .home_dir()
            .join("Library/Application Support/Firefox");
        root.join("profiles.ini").is_file().then_some(root)
    }
}

pub struct MacosPrivileges;

impl MacosPrivileges {
    /// Build the `osascript -e` program that runs `script` as admin. Factored
    /// out so escaping can be unit-tested without triggering the auth prompt.
    fn osascript_program(script: &str, reason: &PromptReason) -> String {
        // Escape for an AppleScript double-quoted string literal.
        let lit = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        format!(
            "do shell script \"{}\" with prompt \"{}\" with administrator privileges",
            lit(script),
            lit(&reason.sentence())
        )
    }

    /// Turn osascript's stderr into a clear message. A user who dismisses the auth
    /// dialog gets AppleScript error `-128` ("User canceled.") — surface that as a
    /// friendly, actionable line instead of a raw error code, so a cancelled prompt
    /// reads as recoverable rather than a failure.
    fn privileged_error_message(stderr: &str) -> String {
        if stderr.contains("-128") || stderr.to_lowercase().contains("user canceled") {
            "Administrator permission was cancelled — this step needs it. Try again and approve the prompt."
                .to_string()
        } else {
            format!("privileged operation failed: {stderr}")
        }
    }
}

impl PrivilegeManager for MacosPrivileges {
    fn run_privileged(&self, script: &str, reason: &PromptReason) -> Result<String> {
        // One dialog at a time — and one build of the per-process work dir.
        static ONE_PROMPT: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _one = ONE_PROMPT.lock().unwrap_or_else(|p| p.into_inner());

        // The dialog names rexenv and carries our icon (`prompt_applet`). If the
        // applet cannot be built or launched, no dialog was shown, so osascript's
        // unbranded one is still the only prompt the user sees.
        let dir = prompt_applet::work_dir();
        let icon = prompt_applet::icon_source();
        let ran = match prompt_applet::build(&dir, script, &reason.sentence(), icon.as_deref()) {
            Ok(()) => prompt_applet::run(&dir),
            Err(e) => Err(prompt_applet::RunError::NoDialog(e.to_string())),
        };
        let _ = prompt_applet::remove(&dir);
        match Self::settle(ran) {
            Settled::Done(answer) => answer,
            Settled::AskThroughOsascript(why) => {
                log::warn!("branded password prompt unavailable ({why}); asking through osascript");
                Self::run_osascript(script, reason)
            }
        }
    }
}

/// What `run_privileged` does with one run of the branded prompt.
#[derive(Debug)]
enum Settled {
    /// The answer, a refusal included. No other dialog follows it.
    Done(Result<String>),
    /// No dialog was shown, so osascript may ask instead. Carries why, for the log.
    AskThroughOsascript(String),
}

impl MacosPrivileges {
    /// The one decision about a second dialog: only a run that showed none may be
    /// asked again. An applet that ran and reported nothing may have had its
    /// password accepted already, so that is an error, never an osascript retry.
    fn settle(
        ran: std::result::Result<prompt_applet::Outcome, prompt_applet::RunError>,
    ) -> Settled {
        match ran {
            Ok(prompt_applet::Outcome::Ran(stdout)) => Settled::Done(Ok(stdout)),
            Ok(prompt_applet::Outcome::Failed { number, message }) => Settled::Done(Err(
                Error::Other(Self::privileged_error_message(&format!("{message} ({number})"))),
            )),
            Err(prompt_applet::RunError::NoResult(why)) => Settled::Done(Err(Error::Other(format!(
                "the rexenv password prompt closed without reporting what happened ({why})"
            )))),
            Err(prompt_applet::RunError::NoDialog(why)) => Settled::AskThroughOsascript(why),
        }
    }


    /// The unbranded fallback: the dialog names `osascript`, but still says why.
    fn run_osascript(script: &str, reason: &PromptReason) -> Result<String> {
        // `do shell script … with administrator privileges` shows one macOS
        // auth dialog and runs the script as root via /bin/sh.
        let program = Self::osascript_program(script, reason);
        let out = std::process::Command::new("osascript")
            .arg("-e")
            .arg(&program)
            .output()?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
        } else {
            Err(Error::Other(Self::privileged_error_message(
                String::from_utf8_lossy(&out.stderr).trim(),
            )))
        }
    }
}

/// Grace window for a SIGTERM'd process to exit before we escalate to SIGKILL (L3):
/// `STOP_GRACE_TRIES` × `STOP_POLL_INTERVAL` ≈ 3s. Bounded so a signal-ignoring
/// process can't block a caller's `wait()` forever; a healthy service exits well
/// within it and `stop` returns as soon as it does (it doesn't wait out the window).
const STOP_GRACE_TRIES: u32 = 30;
const STOP_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Whether `pid` is a live, *non-zombie* process. A zombie (`ps` state `Z`) has
/// already exited and is only awaiting its parent's reap, so it counts as not
/// running — otherwise a clean SIGTERM exit (our services are our own children, so
/// they zombie until the caller's `wait()`) would look alive for the whole grace
/// window. An empty/failed probe also counts as not running (never loop forever).
fn process_running(pid: u32) -> bool {
    match std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "state="])
        .output()
    {
        Ok(o) => {
            let state = String::from_utf8_lossy(&o.stdout);
            let state = state.trim();
            !state.is_empty() && !state.starts_with('Z')
        }
        Err(_) => false,
    }
}

/// SIGTERM `pid`, wait up to `grace_tries` × `interval` for it to exit, then SIGKILL
/// if it's still running (L3). Returns as soon as the process is gone. All our
/// services are our own children, so `kill` never fails for lack of permission; the
/// SIGTERM status is ignored (the process may already be gone) and liveness is
/// confirmed via [`process_running`].
fn stop_pid(pid: u32, grace_tries: u32, interval: Duration) -> Result<()> {
    let _ = std::process::Command::new("kill").arg(pid.to_string()).status(); // SIGTERM
    for _ in 0..grace_tries {
        if !process_running(pid) {
            return Ok(());
        }
        std::thread::sleep(interval);
    }
    // Ignored SIGTERM → force it. SIGKILL can't be caught, so `wait()` is guaranteed
    // to return afterward.
    let _ = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status();
    for _ in 0..10 {
        if !process_running(pid) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if process_running(pid) {
        Err(Error::Other(format!("pid {pid} survived SIGKILL")))
    } else {
        Ok(())
    }
}

pub struct MacosSupervisor;
impl ProcessSupervisor for MacosSupervisor {
    fn terminate_child(&self, child: &mut Child) {
        // SIGTERM first — a php-fpm / nginx MASTER takes its workers down with it,
        // where a bare SIGKILL orphans them holding the listen socket — then
        // SIGKILL after a 2s grace. `try_wait` both polls and reaps.
        let _ = std::process::Command::new("kill").arg(child.id().to_string()).status();
        for _ in 0..20 {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = child.kill();
        let _ = child.wait();
    }
    fn signal_reload(&self, pid: u32) -> bool {
        matches!(
            std::process::Command::new("kill").args(["-HUP", &pid.to_string()]).status(),
            Ok(s) if s.success()
        )
    }
    fn pid_alive(&self, pid: u32) -> bool {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    fn spawn(&self, program: &Path, args: &[String]) -> Result<Child> {
        Ok(std::process::Command::new(program).args(args).spawn()?)
    }
    fn guard_child_against_our_death(&self, child: u32, domain: &str) -> Result<()> {
        // Our own binary, re-executed in guard mode — the same self-exec shape
        // the DNS agent uses, and for the same reason: no second artifact to
        // ship, sign and keep in step with the app.
        let exe = std::env::current_exe()?;
        let me = std::process::id();
        let start = process_start_token(me).ok_or_else(|| {
            Error::Other(format!("could not read this process's start time (pid {me})"))
        })?;
        let args = crate::core::tunnels::guard_argv(me, child, domain, &start);
        // Detached and silent: it must outlive us (that is its whole job), and
        // it has nothing to say — it either signals a still-identified child or
        // exits. Anything worth reading is already in the child's own log.
        let mut guard = std::process::Command::new(exe)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        // REAPED. `Child::drop` neither kills nor waits, and nothing installs
        // a SIGCHLD reaper, so a dropped handle left one zombie per share for
        // the life of the app. The guard exits when the share ends (or when we
        // die, at which point launchd reaps it) — a thread parked on `wait`
        // costs nothing and clears the corpse the moment it appears.
        std::thread::Builder::new()
            .name(format!("tunnel-guard-reaper-{child}"))
            .spawn(move || {
                let _ = guard.wait();
            })?;
        Ok(())
    }

    fn spawn_logged(&self, program: &Path, args: &[String], log_path: &Path) -> Result<Child> {
        self.spawn_logged_env(program, args, log_path, &[])
    }
    fn spawn_logged_env(
        &self,
        program: &Path,
        args: &[String],
        log_path: &Path,
        env: &[(String, String)],
    ) -> Result<Child> {
        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let out = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)?;
        let err = out.try_clone()?;
        Ok(std::process::Command::new(program)
            .args(args)
            .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdout(out)
            .stderr(err)
            .spawn()?)
    }
    fn stop(&self, pid: u32) -> Result<()> {
        stop_pid(pid, STOP_GRACE_TRIES, STOP_POLL_INTERVAL)
    }

    fn pids_named(&self, name: &str) -> Vec<u32> {
        // `pgrep -x` = exact process-name match; identification happens at the
        // caller via `pid_command` — this is only the candidate list.
        std::process::Command::new("pgrep")
            .args(["-x", name])
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .filter_map(|l| l.trim().parse().ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn pid_command(&self, pid: u32) -> Option<String> {
        // Same world-readable `ps` source `owned_listeners` trusts for its
        // marker check; empty output = no such process.
        let out = std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "command="])
            .output()
            .ok()?;
        let cmd = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!cmd.is_empty()).then_some(cmd)
    }

    fn spawn_streamed(
        &self,
        program: &Path,
        args: &[String],
        cwd: &Path,
        env: &[(String, String)],
    ) -> Result<Child> {
        use std::os::unix::process::CommandExt;
        use std::process::Stdio;
        let mut cmd = std::process::Command::new(program);
        cmd.args(args)
            .current_dir(cwd)
            // FULL env replacement: the child sees the user's shell env (nvm
            // PATH, SSH_AUTH_SOCK), never our bare launchd inheritance.
            .env_clear()
            .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Own group (pgid = child pid) so stop_group signals the TREE.
            .process_group(0);
        Ok(cmd.spawn()?)
    }

    fn stop_group(&self, pgid: u32) -> Result<()> {
        // pkill -g matches by process group — no negative-pid argv parsing
        // pitfalls. Liveness = "any process left in the group" (pgrep -g),
        // NOT the leader's pid: a dead leader can leave TERM-ignoring
        // children holding the group.
        fn group_signal(sig: &str, pgid: u32) {
            let _ = std::process::Command::new("pkill")
                .args([&format!("-{sig}"), "-g", &pgid.to_string()])
                .status();
        }
        fn group_alive(pgid: u32) -> bool {
            std::process::Command::new("pgrep")
                .args(["-g", &pgid.to_string()])
                .output()
                .map(|o| o.status.success() && !o.stdout.is_empty())
                .unwrap_or(false)
        }
        group_signal("TERM", pgid);
        for _ in 0..STOP_GRACE_TRIES {
            if !group_alive(pgid) {
                return Ok(());
            }
            std::thread::sleep(STOP_POLL_INTERVAL);
        }
        group_signal("KILL", pgid);
        for _ in 0..STOP_GRACE_TRIES {
            if !group_alive(pgid) {
                return Ok(());
            }
            std::thread::sleep(STOP_POLL_INTERVAL);
        }
        Err(Error::Other(format!("process group {pgid} survived SIGKILL")))
    }

    /// `lsof`'s `txt` (text/executable) descriptor, which survives a process
    /// title rewrite where `ps -o comm=` does not — `-Fn` gives one
    /// `n<path>` line per record, machine-readable and space-safe (our app-data
    /// paths contain spaces). Same tool `owned_listeners` already depends on,
    /// with the same "missing lsof → unknown" fallback.
    fn pid_exe(&self, pid: u32) -> Option<PathBuf> {
        let out = std::process::Command::new("lsof")
            .args(["-p", &pid.to_string(), "-a", "-d", "txt", "-Fn"])
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .find_map(|l| l.strip_prefix('n'))
            .map(PathBuf::from)
    }

    fn owned_listeners(&self, port: u16, owner_marker: &str) -> Vec<u32> {
        // `lsof -t` → pids with a LISTEN socket on this TCP port.
        let out = match std::process::Command::new("lsof")
            .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-t"])
            .output()
        {
            Ok(o) => o,
            Err(_) => return Vec::new(), // lsof missing → fall back to handles
        };
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.trim().parse::<u32>().ok())
            .filter(|&pid| {
                // Only OUR services: the process command must reference our app-data
                // dir (every rexenv service's binary/config/data path lives under it),
                // so we never terminate an unrelated process squatting on the port.
                std::process::Command::new("ps")
                    .args(["-p", &pid.to_string(), "-o", "command="])
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).contains(owner_marker))
                    .unwrap_or(false)
            })
            .collect()
    }

    fn owned_master(&self, port: u16, owner_marker: &str) -> Option<u32> {
        let pids = self.owned_listeners(port, owner_marker);
        if pids.is_empty() {
            return None;
        }
        // One ps call for the whole set: pid+ppid pairs feed the pure
        // parent-based selection (see `traits::select_master` — the lowest-pid
        // fallback broke on Apache's churned/recycled worker pids).
        let list = pids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
        let pairs: Vec<(u32, u32)> = std::process::Command::new("ps")
            .args(["-o", "pid=,ppid=", "-p", &list])
            .output()
            .ok()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .filter_map(|l| {
                        let mut it = l.split_whitespace();
                        Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
                    })
                    .collect()
            })
            .unwrap_or_default();
        super::traits::select_master(&pairs).or_else(|| pids.into_iter().min())
    }

    fn resource_usage(&self, pid: u32) -> Option<(f32, u64)> {
        // `ps` reads world-readable kinfo_proc, so this works for the ROOT edge
        // Caddy where sysinfo's proc_pidinfo (same-user only) returns nothing.
        let out = std::process::Command::new("ps")
            .args(["-o", "%cpu=,rss=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let mut it = text.split_whitespace();
        let cpu: f32 = it.next()?.parse().ok()?;
        let rss_kb: u64 = it.next()?.parse().ok()?;
        Some((cpu, rss_kb / 1024))
    }

    fn owned_pids(&self, marker: &str) -> Vec<u32> {
        // Substring match against every process's full command (robust vs. a regex
        // over paths with spaces/dots). `marker` is an owned app-data path, so only
        // our processes match.
        let out = match std::process::Command::new("ps")
            .args(["-axo", "pid=,command="])
            .output()
        {
            Ok(o) => o,
            Err(_) => return Vec::new(),
        };
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|line| {
                let line = line.trim_start();
                let (pid_str, cmd) = line.split_once(char::is_whitespace)?;
                let pid = pid_str.parse::<u32>().ok()?;
                cmd.contains(marker).then_some(pid)
            })
            .collect()
    }

    /// `lsof`'s field output: TCP listeners and UDP endpoints whose LOCAL address is on
    /// `port` (ledger #599). Measured 13 Sep 2026 on macOS 26.6.2: a `127.0.0.1` bind made
    /// the way Rust's `TcpListener::bind` makes it (`SO_REUSEADDR`) SUCCEEDS beside another
    /// process's `0.0.0.0` or `[::]` TCP listener and then receives the `127.0.0.1` traffic —
    /// the trial bind alone called that port free. `None` only when lsof cannot run; then
    /// the gate falls back to its bind, as before.
    ///
    /// Unprivileged lsof lists this user's processes only, so a ROOT wildcard holder is not
    /// in this answer; the trial bind is what stands for it (not measured — no root holder
    /// was set up to see whether BSD refuses a cross-user shadow bind).
    fn port_holders(&self, port: u16, udp: bool) -> Option<Vec<u32>> {
        let selector = if udp { format!("-iUDP:{port}") } else { format!("-iTCP:{port}") };
        let mut cmd = std::process::Command::new("lsof");
        cmd.args(["-nP", &selector]);
        if !udp {
            cmd.arg("-sTCP:LISTEN");
        }
        // Exit 1 with no output is lsof's "nothing matched" — an answer, not a failure.
        let out = cmd.arg("-Fpn").output().ok()?;
        Some(lsof_local_port_holders(&String::from_utf8_lossy(&out.stdout), port))
    }

    /// `lsof` lists sockets by the process holding them, so a connection still queued for a
    /// worker (not yet `accept()`ed) belongs to no process and is not counted: on macOS this is
    /// the connections workers hold. Unprivileged `lsof` sees this user's processes only — the
    /// pools are.
    fn established_on(&self, port: u16) -> Option<usize> {
        let out = std::process::Command::new("lsof")
            .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:ESTABLISHED", "-Fpn"])
            .output()
            .ok()?;
        Some(lsof_established_local(&String::from_utf8_lossy(&out.stdout), port))
    }

    fn port_conflict_help(&self, port: u16, udp: bool) -> PortConflictHelp {
        // `-i` selector for the port; UDP has no LISTEN state to filter on.
        let (sel, state): (String, &[&str]) = if udp {
            (format!("-iUDP:{port}"), &[])
        } else {
            (format!("-iTCP:{port}"), &["-sTCP:LISTEN"])
        };
        // Unprivileged lsof only sees this user's processes — a root-owned
        // listener yields no holder, but the suggested command (run with sudo
        // by the user) still finds and stops it. lsof lists EVERY process
        // sharing the listen socket (an nginx master + all its workers); take
        // the LOWEST pid — masters fork first — so we attribute the master,
        // not a meaningless "nginx: worker process".
        let master_pid = std::process::Command::new("lsof")
            .args(["-nP", &sel])
            .args(state)
            .arg("-t")
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .filter_map(|l| l.trim().parse::<u32>().ok())
                    .min()
            });
        let (holder, app, free_command) = match master_pid {
            Some(pid) => {
                let exe = executable_path(pid);
                let (h, a) = if holder_is_locals_router(&exe, MacosSupervisor.pid_command(pid).as_deref()) {
                    locals_router_holder(pid)
                } else {
                    attribute_holder(&exe, pid, holder_is_valets_nginx(&exe, pid))
                };
                let cmd = free_port_command(a.as_deref(), &exe, pid);
                (Some(h), a, Some(cmd))
            }
            // Holder invisible to unprivileged lsof (a root listener): the
            // generic sudo one-liner still finds and stops it when the USER
            // runs it — sudo's lsof sees everything.
            None => (
                None,
                None,
                Some(format!(
                    "sudo kill $(sudo lsof -t {sel}{})",
                    if udp { "" } else { " -sTCP:LISTEN" }
                )),
            ),
        };
        PortConflictHelp { holder, app, free_command }
    }
}

/// The copy-paste command that ACTUALLY frees the holder's port — matched to how
/// the holder is MANAGED, because "kill the pid" is often wrong advice:
///
/// 1. An app-supervised process (Herd, OrbStack, Docker Desktop, …): killing its
///    nginx just gets respawned by the app — the right action is to quit the APP.
///    (`osascript -e 'quit app "Herd"'` live-verified: Herd quits, 127.0.0.1:443
///    freed, rexenv's edge answers again.) Our own processes are exempt — never
///    tell the user to quit rexenv.
/// 2. A Homebrew-installed binary: it's usually a `brew services` daemon that
///    would ALSO be respawned by a bare kill — stop the service. Tried without
///    sudo first (user service), then with (root service; e.g. Valet's nginx is
///    exactly a brew formula under the hood).
/// 3. Anything else (incl. our own orphans): direct `sudo kill <master-pid>` —
///    the last resort, aimed at the master we resolved, never a worker.
fn free_port_command(app: Option<&str>, exe_path: &str, pid: u32) -> String {
    // Valet is not an .app to quit and not a formula to stop: `brew services
    // stop nginx` frees the port but leaves the user's Valet half-running, and
    // the next `valet` command puts it back. Their own CLI is the action that
    // matches how the thing is managed — the same rule as the Herd tier.
    if app == Some("Valet") {
        return "valet stop".into();
    }
    match app {
        // Only build the "quit app" osascript when the name is a safe literal.
        // The name is derived from the holder's OWN path segments (a same-user
        // squatter can craft them) and this text is copy-pasted by the user, so
        // an unsafe name must not reach the shell — it falls through to the
        // brew/pid tiers, ending at `sudo kill <pid>` (pid is a u32, always safe).
        Some(app) if app != "rexenv" && safe_cmd_token(app, true) => {
            format!("osascript -e 'quit app \"{app}\"'")
        }
        _ => match brew_formula(exe_path).filter(|f| safe_cmd_token(f, false)) {
            Some(f) => format!("brew services stop {f} || sudo brew services stop {f}"),
            None => format!("sudo kill {pid}"),
        },
    }
}

/// True if `s` is safe to embed literally in a suggested copy-paste command — an
/// allowlist, because the name comes from the port holder's path segments
/// (attacker-influenceable for a same-user squatter) and lands in text the user
/// is told to run (one tier includes `sudo`). `allow_space` is set for the
/// AppleScript app name — it's inside `"…"` so spaces are legal ("Docker
/// Desktop") — and clear for a brew formula, which is a bare shell token a space
/// would split. Excludes the break-out chars (`"` `'` `` ` `` `$` `;` `\` …).
fn safe_cmd_token(s: &str, allow_space: bool) -> bool {
    !s.is_empty()
        && s.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '.' | '_' | '+' | '@' | '-')
                || (allow_space && c == ' ')
        })
}

/// Homebrew formula name from an executable path, if it lives in a brew prefix:
/// `…/Cellar/<formula>/<ver>/…` or `/opt/homebrew/opt/<formula>/…` (also
/// `/usr/local/opt/<formula>/…` on Intel).
fn brew_formula(exe_path: &str) -> Option<String> {
    let segs: Vec<&str> = exe_path.split('/').collect();
    if let Some(i) = segs.iter().position(|s| *s == "Cellar") {
        return segs.get(i + 1).map(|s| s.to_string());
    }
    segs.windows(2)
        .position(|w| w[0] == "opt" && w[1] != "homebrew")
        .filter(|_| exe_path.starts_with("/opt/homebrew/") || exe_path.starts_with("/usr/local/"))
        .and_then(|i| segs.get(i + 1).map(|s| s.to_string()))
}

/// Pids from `lsof -Fpn` output with a socket whose LOCAL end is on `port`.
///
/// `-i<proto>:<port>` matches EITHER end of a socket: measured 13 Sep 2026, a UDP client
/// talking TO the port shows as `n127.0.0.1:64140->127.0.0.1:64885`. Counting that as a
/// holder would refuse the DNS agent's port every time something queries it. So only the
/// part before `->` is compared, and it must end in `:<port>` exactly.
/// How many connections in `lsof -Fpn` field output have their LOCAL end on `port` — the
/// `n` lines of the form `local->remote` whose part before `->` ends in `:port`. A client
/// connecting to the port (nginx) has it after `->` and is not counted; a listener has no
/// `->` and is not a connection.
fn lsof_established_local(fields: &str, port: u16) -> usize {
    let suffix = format!(":{port}");
    fields
        .lines()
        .filter_map(|line| line.strip_prefix('n'))
        .filter_map(|name| name.split_once("->"))
        .filter(|(local, _)| local.ends_with(&suffix))
        .count()
}

fn lsof_local_port_holders(fields: &str, port: u16) -> Vec<u32> {
    let suffix = format!(":{port}");
    let mut pid = None;
    let mut pids = Vec::new();
    for line in fields.lines() {
        if let Some(p) = line.strip_prefix('p') {
            pid = p.trim().parse::<u32>().ok();
        } else if let Some(name) = line.strip_prefix('n') {
            let local = name.split("->").next().unwrap_or(name);
            if let Some(p) = pid.filter(|_| local.ends_with(&suffix)) {
                if !pids.contains(&p) {
                    pids.push(p);
                }
            }
        }
    }
    pids
}

/// The REAL executable path of `pid`. `ps -o comm=` is a trap for daemons that
/// rewrite their process title — an nginx worker reports the meaningless
/// "nginx: worker process" — so read the first `txt` file descriptor (the
/// binary) via lsof instead, and fall back to `ps` only when that fails.
fn executable_path(pid: u32) -> String {
    let via_txt = std::process::Command::new("lsof")
        .args(["-nP", "-p", &pid.to_string(), "-a", "-d", "txt", "-Fn"])
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .find(|l| l.starts_with("n/"))
                .map(|l| l[1..].to_string())
        });
    via_txt.unwrap_or_else(|| {
        std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "comm="])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    })
}

/// Is this port holder Valet's nginx? Valet ships no server of its own — it
/// drives the Homebrew nginx — so without this the message says
/// `nginx (pid 1234, /opt/homebrew/…)` and suggests `brew services stop nginx`,
/// which is true, unhelpful, and (for a Valet user) the wrong tool: their own
/// `valet stop` is the command they know and the one that also stops the php-fpm
/// and dnsmasq that came with it.
///
/// POSITIVE identification only: the config that binary loads must carry Valet's
/// own include. "Valet is installed on this machine" would attribute a
/// developer's own nginx to Valet, which is a confident sentence about the wrong
/// program — worse than the generic answer it replaced.
fn holder_is_valets_nginx(exe_path: &str, pid: u32) -> bool {
    // The config THIS process loads — `-c` from its argv when it has one —
    // and only when the executable is nginx at all.
    let cmdline = MacosSupervisor.pid_command(pid);
    crate::core::valet::nginx_conf_for(exe_path, cmdline.as_deref())
        .and_then(|conf| std::fs::read_to_string(conf).ok())
        .is_some_and(|conf| crate::core::valet::conf_is_valets(&conf))
}

/// Is this port holder Local's ROUTER — the nginx Local runs on :80/:443 in its
/// "Site Domains" router mode? By path alone it already reads "Local (nginx,
/// pid …)" and the offered fix is quitting Local, which also stops the per-site
/// database a Local import reads from — the owner met exactly this mid-import
/// (12 Sep 2026: every rexenv site dark, the MCP verdict unable to say by whom).
/// Positive identification only: Local's bundled nginx AND a config under
/// Local's `run/router/`; Local's per-site nginx (`run/<id>/`) is not the router.
/// Pure — the command line is passed in, so the test is the same on every machine.
fn holder_is_locals_router(exe_path: &str, cmdline: Option<&str>) -> bool {
    exe_path.contains("/Application Support/Local/lightning-services/nginx")
        && cmdline.is_some_and(|c| c.contains("/Application Support/Local/run/router/"))
}

/// Local's router, named with the way out that keeps Local (and the databases
/// it runs) up: its router mode is Local's own setting. The app stays "Local",
/// so messages that say "quit {app}" and the osascript quit still read true.
fn locals_router_holder(pid: u32) -> (String, Option<String>) {
    (
        format!(
            "Local's router (nginx, pid {pid}; or set Local → Preferences → Advanced → Router \
             Mode to localhost to free the port without quitting Local)"
        ),
        Some("Local".into()),
    )
}

/// Attribute a port holder to its OWNING APPLICATION — the actionable name.
/// "quit nginx: worker process" tells a user nothing; "quit Herd" is the whole
/// point of the message (Herd users ARE the target audience). Returns the
/// display string plus the bare app name (for "quit {app}" phrasing).
///
/// Attribution, in order:
/// 1. a `Foo.app` bundle segment (`/Applications/Herd.app/…/nginx` → Herd);
/// 2. the directory right after `Application Support` — app-managed helper
///    binaries live there (`…/Application Support/Herd/bin/nginx-arm` → Herd);
///    our own reverse-DNS dir maps back to "rexenv";
/// 3. no app identified: degrade HONESTLY to what we do know — process name,
///    pid, and the full executable path so an unknown holder stays actionable.
///
/// **`valet` is passed IN rather than probed here**, and that is not a style
/// preference: the probe reads a file on the developer's own machine, so a
/// self-probing `attribute_holder` gives a different answer depending on
/// whether the person running the tests has Valet installed. That is exactly
/// what happened — the existing brew-nginx case started returning "Valet" on
/// this machine the moment the probe went inside.
fn attribute_holder(exe_path: &str, pid: u32, valet: bool) -> (String, Option<String>) {
    if valet {
        let exe = exe_path.rsplit('/').next().unwrap_or(exe_path).trim();
        return (format!("Valet ({exe}, pid {pid})"), Some("Valet".into()));
    }
    let exe = exe_path.rsplit('/').next().unwrap_or(exe_path).trim();
    let segs: Vec<&str> = exe_path.split('/').collect();
    let app = segs
        .iter()
        .find(|seg| seg.ends_with(".app"))
        .map(|seg| seg.trim_end_matches(".app").to_string())
        .or_else(|| {
            segs.windows(2)
                .find(|w| w[0] == "Application Support")
                .map(|w| w[1].to_string())
        })
        .map(|app| if app == APP_IDENTIFIER { "rexenv".to_string() } else { app });
    match app {
        Some(app) if app != exe => (format!("{app} ({exe}, pid {pid})"), Some(app)),
        // The app name IS the process (plain .app binary) — one name suffices.
        Some(app) => (format!("{app} (pid {pid})"), Some(app)),
        // Unknown ownership: name + pid + path beats a bare title.
        None if exe_path.starts_with('/') => {
            (format!("{exe} (pid {pid}, {exe_path})"), None)
        }
        None => (format!("{exe} (pid {pid})"), None),
    }
}

/// launchd label for the per-user LaunchAgent — the one canonical app identity.
const AUTOSTART_LABEL: &str = APP_IDENTIFIER;

pub struct MacosAutostart;
impl MacosAutostart {
    /// `~/Library/LaunchAgents/dev.rexenv.rexenv.plist` — the per-user LaunchAgent
    /// whose presence is the source of truth for "start on login".
    fn plist_path() -> Result<PathBuf> {
        let home = std::env::var_os("HOME")
            .ok_or_else(|| Error::Other("HOME is not set".into()))?;
        Ok(PathBuf::from(home)
            .join("Library/LaunchAgents")
            .join(format!("{AUTOSTART_LABEL}.plist")))
    }

    /// The on-login plist that launches the rexenv app at GUI login. Quotes the
    /// program path (app paths contain spaces).
    ///
    /// **`--hidden` is the whole point of an on-login launch.** rexenv starts
    /// its services and lives in the menu bar; a window thrown at the user
    /// every time they log in is an app that has to be closed before work can
    /// start. The flag is a REQUEST, not a command — `lib.rs` still shows the
    /// window when first-run setup is incomplete, because a silent tray on a
    /// machine that cannot resolve `.rex` is an app that looks broken and hides
    /// the fix.
    fn plist_contents(program: &Path) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n\
             <dict>\n\
             \t<key>Label</key>\n\
             \t<string>{AUTOSTART_LABEL}</string>\n\
             \t<key>ProgramArguments</key>\n\
             \t<array>\n\
             \t\t<string>{program}</string>\n\
             \t\t<string>{HIDDEN_LAUNCH_FLAG}</string>\n\
             \t</array>\n\
             \t<key>RunAtLoad</key>\n\
             \t<true/>\n\
             \t<key>ProcessType</key>\n\
             \t<string>Interactive</string>\n\
             </dict>\n\
             </plist>\n",
            program = program.display(),
            HIDDEN_LAUNCH_FLAG = crate::HIDDEN_LAUNCH_FLAG,
        )
    }
}
impl AutostartManager for MacosAutostart {
    fn enable(&self) -> Result<()> {
        let plist = Self::plist_path()?;
        if let Some(parent) = plist.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let program = std::env::current_exe()?;
        std::fs::write(&plist, Self::plist_contents(&program))?;
        // Register with launchd so it takes effect this session too (best-effort:
        // the plist on disk is the authoritative state, surviving a failed load).
        let _ = std::process::Command::new("launchctl")
            .args(["load", "-w"])
            .arg(&plist)
            .status();
        Ok(())
    }
    fn disable(&self) -> Result<()> {
        let plist = Self::plist_path()?;
        if plist.exists() {
            let _ = std::process::Command::new("launchctl")
                .args(["unload", "-w"])
                .arg(&plist)
                .status();
            std::fs::remove_file(&plist)?;
        }
        Ok(())
    }
    fn is_enabled(&self) -> Result<bool> {
        Ok(Self::plist_path()?.exists())
    }
    fn refresh(&self) -> Result<()> {
        let plist = Self::plist_path()?;
        let program = std::env::current_exe()?;
        let want = Self::plist_contents(&program);
        let have = std::fs::read_to_string(&plist).unwrap_or_default();
        // Byte-identical: nothing to do, and no `launchctl load -w` churn —
        // `-w` also clears a `Disabled` the user set by hand.
        if have == want {
            return Ok(());
        }
        let in_bundle = program.to_string_lossy().contains(".app/Contents/MacOS/");
        if !in_bundle {
            if let Some(recorded) = login_item_program(&have).filter(|p| p.exists()) {
                log::info!(
                    "autostart: this launch is {} (not an .app bundle) — keeping the login item \
                     on {}",
                    program.display(),
                    recorded.display()
                );
                return Ok(());
            }
        }
        // An installed build, or a recorded binary that no longer exists (the
        // app moved, an old build was deleted): re-point, and reload.
        self.enable()
    }
}

/// A process's start time as `ps` prints it (`lstart`, second resolution) —
/// the identity the tunnel guard checks beyond the pid, because a pid is
/// recycled and a start time is not. `None` when the pid is gone.
pub fn process_start_token(pid: u32) -> Option<String> {
    let out = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !s.is_empty()).then_some(s)
}

/// The program a login-item plist names: the first `<string>` after
/// `ProgramArguments`. `None` for anything that is not our plist.
fn login_item_program(plist: &str) -> Option<PathBuf> {
    let after = plist.split("<key>ProgramArguments</key>").nth(1)?;
    let start = after.find("<string>")? + "<string>".len();
    let end = after[start..].find("</string>")? + start;
    Some(PathBuf::from(after[start..end].trim()))
}

#[cfg(test)]
mod login_item_tests {
    use super::*;

    /// The refresh reads the program back out of the plist it wrote — and
    /// `Label` comes first in that plist, so a reader that took the FIRST
    /// `<string>` in the file would return the label, and every "does the
    /// recorded binary still exist" check would say no.
    #[test]
    fn the_recorded_program_is_read_back_from_our_own_plist() {
        let plist = MacosAutostart::plist_contents(Path::new("/Applications/rexenv.app/Contents/MacOS/rexenv"));
        assert_eq!(
            login_item_program(&plist),
            Some(PathBuf::from("/Applications/rexenv.app/Contents/MacOS/rexenv"))
        );
        assert_eq!(login_item_program("<plist></plist>"), None);
    }
}

/// Per-user LaunchAgent that keeps the loopback DNS resolver alive across app
/// quits (and from login after a reboot): runs `<app binary> --dns-agent` with
/// `KeepAlive` + `RunAtLoad`. Everything is unprivileged — the resolver binds a
/// high loopback UDP port and the plist lives in `~/Library/LaunchAgents` — so
/// install/kickstart/uninstall are direct ops with NO auth prompt (`launchctl
/// load/unload -w`, same calls [`MacosAutostart`] already uses).
pub struct MacosDnsAgent;

/// launchd label for the DNS LaunchAgent — a sub-label of the canonical app
/// identity, distinct from the app-autostart agent and the root edge daemon.
const DNS_AGENT_LABEL: &str = "dev.rexenv.rexenv.dns";

impl MacosDnsAgent {
    fn launchctl(args: &[&str], plist: &Path) -> Result<()> {
        let st = std::process::Command::new("launchctl").args(args).arg(plist).status()?;
        if st.success() {
            Ok(())
        } else {
            Err(Error::Other(format!("launchctl {} failed (exit {:?})", args.join(" "), st.code())))
        }
    }
}

impl DnsAgentManager for MacosDnsAgent {
    fn is_installed(&self) -> bool {
        self.definition_path().map(|p| p.exists()).unwrap_or(false)
    }

    fn definition_path(&self) -> Result<PathBuf> {
        let home = std::env::var_os("HOME")
            .ok_or_else(|| Error::Other("HOME is not set".into()))?;
        Ok(PathBuf::from(home)
            .join("Library/LaunchAgents")
            .join(format!("{DNS_AGENT_LABEL}.plist")))
    }

    /// `KeepAlive` + `RunAtLoad`: the resolver is up from login and relaunched on
    /// any death. `Background` (a helper, not an interactive app); agent output
    /// goes to the shared log dir so a wedged resolver leaves evidence.
    fn definition_contents(&self, exe: &Path, log: &Path) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n\
             <dict>\n\
             \t<key>Label</key>\n\
             \t<string>{DNS_AGENT_LABEL}</string>\n\
             \t<key>ProgramArguments</key>\n\
             \t<array>\n\
             \t\t<string>{exe}</string>\n\
             \t\t<string>--dns-agent</string>\n\
             \t</array>\n\
             \t<key>KeepAlive</key>\n\
             \t<true/>\n\
             \t<key>RunAtLoad</key>\n\
             \t<true/>\n\
             \t<key>ProcessType</key>\n\
             \t<string>Background</string>\n\
             \t<key>StandardOutPath</key>\n\
             \t<string>{log}</string>\n\
             \t<key>StandardErrorPath</key>\n\
             \t<string>{log}</string>\n\
             </dict>\n\
             </plist>\n",
            exe = exe.display(),
            log = log.display(),
        )
    }

    fn install(&self, exe: &Path, log: &Path) -> Result<()> {
        let plist = self.definition_path()?;
        if let Some(parent) = plist.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let contents = self.definition_contents(exe, log);
        // Skip the unload/load churn when nothing changed (every app launch calls
        // this): a byte-identical plist with a live agent is already correct.
        let unchanged = std::fs::read_to_string(&plist).map(|c| c == contents).unwrap_or(false);
        std::fs::write(&plist, &contents)?;
        if unchanged {
            return Ok(());
        }
        // Exe changed (dev <-> installed build) or first install: reload so launchd
        // runs the CURRENT binary. Unload is best-effort (nothing loaded on a fresh
        // install); load must succeed.
        let _ = Self::launchctl(&["unload", "-w"], &plist);
        Self::launchctl(&["load", "-w"], &plist)
    }

    fn kickstart(&self) -> Result<()> {
        // Restart the job IN PLACE (`launchctl kickstart -k`), never unload/load:
        // an unload+load pair RE-REGISTERS the login item with Background Task
        // Management, and macOS posts an "App Background Activity" notification
        // for every re-registration — the old pair here fired that nag on every
        // watchdog kick (31 in one health log). Kickstart restarts the process
        // under the EXISTING registration, so launchd relaunches the resolver
        // and the user hears nothing.
        let uid = String::from_utf8(std::process::Command::new("id").arg("-u").output()?.stdout)
            .map_err(|e| Error::Other(format!("id -u produced non-UTF-8 output: {e}")))?;
        let target = format!("gui/{}/{DNS_AGENT_LABEL}", uid.trim());
        let st = std::process::Command::new("launchctl").args(["kickstart", "-k", &target]).status()?;
        if st.success() {
            return Ok(());
        }
        // Job unknown to launchd (e.g. someone ran `launchctl unload` by hand):
        // fall back to a plain load — a REGISTRATION, not a re-registration, so
        // the one notification it may show is honest (the item really was gone).
        Self::launchctl(&["load", "-w"], &self.definition_path()?)
    }

    fn uninstall(&self) -> Result<()> {
        let plist = self.definition_path()?;
        if plist.exists() {
            let _ = Self::launchctl(&["unload", "-w"], &plist);
            std::fs::remove_file(&plist)?;
        }
        Ok(())
    }
}

/// Root LaunchDaemon that keeps the Caddy edge alive across ANY death — SIGTERM,
/// crash, sleep/wake, logout, reboot — with `KeepAlive=true` + `RunAtLoad=true`.
/// launchd (as root) owns the edge, so a stray external SIGTERM (the incident this
/// replaced) is relaunched in seconds instead of leaving every site unreachable
/// until a manual Start-all.
///
/// Security: a root daemon must NEVER execute a **user-writable** binary — that is a
/// standing local privilege escalation (any user process overwrites it → runs as
/// root). So install copies our caddy into a **root-owned** tree
/// (`/Library/Application Support/dev.rexenv.rexenv/bin/caddy`, `root:wheel 0755`)
/// and the plist points there, never at the user-writable `bin/` cache. The plist
/// itself is `root:wheel 0644` (launchd refuses a group/other-writable daemon plist).
///
/// Like [`MacosDns`] these are pure BUILDERS (no privilege, no side effects) so the
/// install/stop/uninstall shell is unit-testable without an auth prompt and batched
/// into ONE `PrivilegeManager` elevation. The plist + wrapper file CONTENTS are
/// written unprivileged to a staging dir (plain `fs::write` — no shell-escaping of
/// multi-line files); the privileged command only `cp`s them into the root tree.
/// Single-quote a path for a `/bin/sh` command (app-data + `/Library` paths contain
/// spaces). Mirrors `core::proxy::sh_quote`; kept local so the platform layer has no
/// core dependency. Paths here are our own fixed locations, never user input.
fn sh_quote(path: &Path) -> String {
    // POSIX-escape any embedded `'` as `'\''` so a path can't break out of the
    // single quotes into the ROOT command context. Identity for `'`-free paths
    // (every rexenv path is), so generated root commands are byte-identical
    // (B12). Mirrors core::proxy::sh_quote + the cli.rs quote discipline.
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

pub struct MacosEdgeDaemon;

/// launchd label for the root edge daemon — distinct from [`AUTOSTART_LABEL`] (the
/// per-user LaunchAgent that starts the APP). The drift-guard test keeps it a
/// sub-label of the one canonical identity.
const EDGE_DAEMON_LABEL: &str = "dev.rexenv.rexenv.edge";
/// Root-owned support tree for the daemon (parallel to the user app-data dir, but
/// writable only by root so neither the binary nor the wrapper is a root-exec of a
/// user-writable file).
const EDGE_ROOT_DIR: &str = "/Library/Application Support/dev.rexenv.rexenv";

impl EdgeSupervisor for MacosEdgeDaemon {
    /// The root LaunchDaemon plist (system domain → lives under `/Library`).
    fn plist_path(&self) -> PathBuf {
        PathBuf::from("/Library/LaunchDaemons").join(format!("{EDGE_DAEMON_LABEL}.plist"))
    }

    /// Root-owned copy of the caddy binary the daemon executes (NOT the user cache).
    fn daemon_binary_path(&self) -> PathBuf {
        PathBuf::from(EDGE_ROOT_DIR).join("bin/caddy")
    }

    /// Root-owned launcher the plist runs: it hands the admin socket back to the
    /// invoking user, then `exec`s caddy (so launchd tracks caddy directly by PID).
    fn wrapper_path(&self) -> PathBuf {
        PathBuf::from(EDGE_ROOT_DIR).join("edge-launch.sh")
    }

    /// Whether the daemon is installed (its plist is on disk). Source of truth for
    /// "is the edge under launchd supervision".
    fn is_installed(&self) -> bool {
        self.plist_path().exists()
    }

    /// Whether launchd will run the label. `launchctl print-disabled system` is
    /// readable WITHOUT privilege; after an explicit Stop-all the label shows
    /// `"…edge" => disabled` (we `disable` before `bootout` so the edge stays down
    /// across reboots). Unknown/failed reads count as enabled — a wrong "disabled"
    /// diagnosis would mislead more than a generic one.
    fn is_enabled(&self) -> bool {
        match std::process::Command::new("launchctl").args(["print-disabled", "system"]).output()
        {
            Ok(o) if o.status.success() => {
                let out = String::from_utf8_lossy(&o.stdout);
                !out.lines().any(|l| {
                    l.contains(&format!("\"{EDGE_DAEMON_LABEL}\"")) && l.contains("=> disabled")
                })
            }
            _ => true,
        }
    }

    /// The plist. `KeepAlive`+`RunAtLoad` = up now and after every death/boot;
    /// `ProcessType Background` (a daemon, not the app's `Interactive`). Runs the
    /// wrapper via `/bin/sh`; the wrapper `exec`s caddy so this label tracks the
    /// real edge PID. Start diagnostics go to the same log the osascript path used.
    fn plist_contents(&self, wrapper: &Path, start_log: &Path) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n\
             <dict>\n\
             \t<key>Label</key>\n\
             \t<string>{EDGE_DAEMON_LABEL}</string>\n\
             \t<key>ProgramArguments</key>\n\
             \t<array>\n\
             \t\t<string>/bin/sh</string>\n\
             \t\t<string>{wrapper}</string>\n\
             \t</array>\n\
             \t<key>KeepAlive</key>\n\
             \t<true/>\n\
             \t<key>RunAtLoad</key>\n\
             \t<true/>\n\
             \t<key>ProcessType</key>\n\
             \t<string>Background</string>\n\
             \t<key>StandardOutPath</key>\n\
             \t<string>{log}</string>\n\
             \t<key>StandardErrorPath</key>\n\
             \t<string>{log}</string>\n\
             </dict>\n\
             </plist>\n",
            wrapper = wrapper.display(),
            log = start_log.display(),
        )
    }

    /// The launcher script. caddy runs as root and drives its 0600 admin socket;
    /// rexenv (the user) must own that socket to reload/stop it promptlessly. Because
    /// caddy **recreates the socket as root on every config reload**, a one-shot chown
    /// would be lost after the first reload — locking rexenv out (the port-443 wedge).
    /// So a backgrounded loop keeps the socket owned by the invoking user for caddy's
    /// whole life, re-chowning within ~1s whenever ownership drifts back to root. Then
    /// `exec` replaces the shell with caddy so launchd tracks the real edge PID (the
    /// loop is a separate child; launchd tears it down with the job). Paths quoted
    /// (spaces); the owner uid is read live from the user app-data dir.
    ///
    /// The chown is guarded by a LINK-COUNT check (`stat -f %l = 1`): a real unix
    /// socket always has one link, so this is transparent in every legitimate case
    /// (the loop chowns exactly as before). It refuses to chown a HARDLINKED inode
    /// — a same-user attacker who replaced the socket path with a hardlink to
    /// another root-owned socket could otherwise have this ROOT loop chown that
    /// foreign inode to them (a privilege-escalation vector). macOS's own hardlink
    /// restrictions already largely block it; this is cost-free defense-in-depth on
    /// a root path (B16). Uses base-system `stat -f` (already used above for %u),
    /// resolved before any PATH setup.
    fn wrapper_contents(
        &self,
        caddy_bin: &Path,
        caddyfile: &Path,
        admin_sock: &Path,
        appdata: &Path,
    ) -> String {
        format!(
            "#!/bin/sh\n\
             # Managed by rexenv — root Caddy edge under launchd KeepAlive. Do not edit.\n\
             SOCK={sock}\n\
             OWNER=$(stat -f %u {appdata})\n\
             ( while :; do \
             [ -S \"$SOCK\" ] && [ \"$(stat -f %u \"$SOCK\" 2>/dev/null)\" != \"$OWNER\" ] \
             && [ \"$(stat -f %l \"$SOCK\" 2>/dev/null)\" = 1 ] \
             && chown -h \"$OWNER\" \"$SOCK\" 2>/dev/null; sleep 1; done ) &\n\
             exec {caddy} run --config {cfg} --adapter caddyfile\n",
            sock = sh_quote(admin_sock),
            appdata = sh_quote(appdata),
            caddy = sh_quote(caddy_bin),
            cfg = sh_quote(caddyfile),
        )
    }

    /// ONE privileged shell (run via `PrivilegeManager`, one prompt) that installs
    /// the daemon: copy our caddy into the root tree, drop in the staged wrapper +
    /// plist with `root:wheel` ownership and safe perms, then (re)bootstrap it into
    /// the system domain. `staged_*` are the unprivileged files rexenv already wrote.
    /// `&&`-chained so a failed copy aborts before bootstrap; the pre-bootout is
    /// best-effort (`;`) so a first install (nothing to bout) still proceeds.
    ///
    /// `enable` MUST precede `bootstrap`: an explicit Stop-all `disable`s the label
    /// (so it stays down across reboots), and `bootstrap` will NOT run a service that
    /// launchd has on its disabled list — so a Start-all after a Stop-all would
    /// reinstall but never actually launch the edge without this re-enable.
    fn install_command(
        &self,
        src_caddy: &Path,
        staged_wrapper: &Path,
        staged_plist: &Path,
    ) -> String {
        let bin = self.daemon_binary_path();
        let wrapper = self.wrapper_path();
        let plist = self.plist_path();
        format!(
            "mkdir -p {bindir} && \
             cp {src} {bin} && chown root:wheel {bin} && chmod 755 {bin} && \
             cp {sw} {wrapper} && chown root:wheel {wrapper} && chmod 755 {wrapper} && \
             cp {sp} {plist} && chown root:wheel {plist} && chmod 644 {plist} && \
             {{ launchctl bootout system/{label} 2>/dev/null ; \
             launchctl enable system/{label} 2>/dev/null ; \
             launchctl bootstrap system {plist} ; }}",
            bindir = sh_quote(&PathBuf::from(EDGE_ROOT_DIR).join("bin")),
            src = sh_quote(src_caddy),
            bin = sh_quote(&bin),
            sw = sh_quote(staged_wrapper),
            wrapper = sh_quote(&wrapper),
            sp = sh_quote(staged_plist),
            plist = sh_quote(&plist),
            label = EDGE_DAEMON_LABEL,
        )
    }

    /// Privileged shell to EXPLICITLY stop the edge (Stop-all). With `KeepAlive=true`
    /// a graceful `caddy stop` is instantly relaunched, so a real stop must bout the
    /// daemon out of the system domain. `disable` keeps it down across reboots until
    /// the next Start-all bootstraps it again.
    fn stop_command(&self) -> String {
        format!(
            "launchctl disable system/{label} 2>/dev/null ; \
             launchctl bootout system/{label} 2>/dev/null ; :",
            label = EDGE_DAEMON_LABEL,
        )
    }

    /// Privileged shell to (re)start the edge after an explicit stop: re-enable then
    /// bootstrap; if it is somehow already loaded, force a fresh start via kickstart.
    fn start_command(&self) -> String {
        let plist = self.plist_path();
        format!(
            "launchctl enable system/{label} 2>/dev/null ; \
             launchctl bootstrap system {plist} 2>/dev/null ; \
             launchctl kickstart -k system/{label}",
            label = EDGE_DAEMON_LABEL,
            plist = sh_quote(&plist),
        )
    }

    /// Privileged shell to fully remove the daemon (uninstall / reset): bout it out
    /// and delete the plist, wrapper, and root-owned binary.
    fn uninstall_command(&self) -> String {
        format!(
            "launchctl bootout system/{label} 2>/dev/null ; \
             rm -f {plist} {wrapper} {bin}",
            label = EDGE_DAEMON_LABEL,
            plist = sh_quote(&self.plist_path()),
            wrapper = sh_quote(&self.wrapper_path()),
            bin = sh_quote(&self.daemon_binary_path()),
        )
    }
}

pub struct MacosPermissions;
impl PermissionManager for MacosPermissions {
    fn set_executable(&self, path: &Path) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms)?;
        Ok(())
    }
    fn set_private(&self, path: &Path) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(path, perms)?;
        Ok(())
    }

    fn write_private(&self, path: &Path, contents: &[u8]) -> Result<()> {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        // mode() applies ONLY at create — a brand-new key file is born 0600,
        // with no world-readable window (B6).
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        // Overwriting an EXISTING file keeps its old mode — re-harden before
        // writing the new secret into it, so a pre-existing 0644 file can't
        // expose the fresh contents.
        let mut perms = f.metadata()?.permissions();
        if perms.mode() & 0o777 != 0o600 {
            perms.set_mode(0o600);
            f.set_permissions(perms)?;
        }
        f.write_all(contents)?;
        Ok(())
    }
}

/// How a terminal app is told which directory to open in
/// (`MacosShell::TERMINALS`).
#[derive(Debug, Clone, Copy)]
enum TermLaunch {
    /// The app takes the directory as a document: `open -a <app> <dir>`.
    Folder,
    /// One `--flag=<dir>` token: `open -na <app> --args --flag=<dir>`.
    FlagEq(&'static str),
    /// Fixed argv, then the directory as its own token:
    /// `open -na <app> --args <args…> <dir>`.
    Args(&'static [&'static str]),
}

pub struct MacosShell;
impl ShellRunner for MacosShell {
    fn run(&self, command: &str, args: &[String]) -> Result<String> {
        let out = std::process::Command::new(command).args(args).output()?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            // Don't swallow a failure as an empty-stdout success: surface the exit
            // status + stderr so a caller can see what went wrong (L4).
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stderr = stderr.trim();
            Err(Error::Other(format!(
                "`{command}` failed ({}): {}",
                out.status,
                if stderr.is_empty() { "(no stderr)" } else { stderr }
            )))
        }
    }

    fn open(&self, target: &str) -> Result<()> {
        let status = std::process::Command::new("open").arg(target).status()?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::Other(format!("`open {target}` failed: {status}")))
        }
    }

    fn reveal(&self, path: &str) -> Result<()> {
        let status = std::process::Command::new("open").args(["-R", path]).status()?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::Other(format!("`open -R {path}` failed: {status}")))
        }
    }

    fn detect_editors(&self) -> Vec<crate::platform::traits::EditorApp> {
        MacosShell::EDITORS
            .iter()
            .filter(|(_, _, app)| Self::app_installed(app))
            .map(|(id, name, app)| crate::platform::traits::EditorApp {
                id: (*id).to_string(),
                name: (*name).to_string(),
                icon: Self::app_icon_data_uri(app),
            })
            .collect()
    }

    fn open_in_editor(&self, editor_id: &str, path: &str) -> Result<()> {
        let (_, name, app) = MacosShell::EDITORS
            .iter()
            .find(|(id, _, _)| *id == editor_id)
            .ok_or_else(|| Error::Other(format!("unknown editor: {editor_id}")))?;
        if !Self::app_installed(app) {
            return Err(Error::Other(format!("{name} is not installed anymore")));
        }
        // `open -a <App> <folder>` opens the folder as a project/workspace in
        // every editor on the list (VS Code window, PhpStorm project, …).
        let status = std::process::Command::new("open").args(["-a", app, path]).status()?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::Other(format!("`open -a {app}` failed: {status}")))
        }
    }

    fn detect_browsers(&self) -> Vec<crate::platform::traits::BrowserApp> {
        let default_bundle = Self::default_browser_bundle_id();
        MacosShell::BROWSERS
            .iter()
            .filter(|(_, _, app, _, _)| Self::app_installed(app))
            .map(|(id, name, app, bundle, private)| crate::platform::traits::BrowserApp {
                id: (*id).to_string(),
                name: (*name).to_string(),
                icon: Self::app_icon_data_uri(app),
                system_default: default_bundle
                    .as_deref()
                    .is_some_and(|d| d.eq_ignore_ascii_case(bundle)),
                supports_private: private.is_some(),
            })
            .collect()
    }

    fn open_in_browser(&self, browser_id: &str, url: &str, private: bool) -> Result<()> {
        // URLs only — `open -a <browser> <path>` would hand a LOCAL FILE to the
        // browser, and every caller here is a link affordance. Checked before we
        // look the browser up, so the refusal never depends on what's installed.
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(Error::Other(format!(
                "refusing to open {url} in a browser — only http:// and https:// URLs go to a \
                 chosen browser (paths go to the system handler)"
            )));
        }
        let (_, name, app, _, private_flag) = MacosShell::BROWSERS
            .iter()
            .find(|(id, _, _, _, _)| *id == browser_id)
            .ok_or_else(|| Error::Other(format!("unknown browser: {browser_id}")))?;
        if !Self::app_installed(app) {
            return Err(Error::Other(format!("{name} is not installed anymore")));
        }
        let status = if private {
            // Refuse rather than fall back: opening a normal window here would
            // record the visit in the user's history under a control that said
            // "private". The UI hides the affordance for these browsers, so
            // reaching this is a bug, not a user mistake.
            let flag = private_flag.ok_or_else(|| {
                Error::Other(format!(
                    "{name} has no private-window command line — rexenv only offers private mode \
                     for browsers it can actually open one in"
                ))
            })?;
            // `-n` (new instance) is REQUIRED. `open -a <app> --args …` DROPS the
            // arguments entirely when the app is already running, so the url
            // would land in an ordinary tab — silently the opposite of what was
            // asked. With `-n` the second instance hands its command line to the
            // running one, which opens exactly one private window.
            std::process::Command::new("open").args(["-na", app, "--args", flag, url]).status()?
        } else {
            std::process::Command::new("open").args(["-a", app, url]).status()?
        };
        if status.success() {
            Ok(())
        } else {
            Err(Error::Other(format!("`open -a {app}` failed: {status}")))
        }
    }

    fn detect_terminals(&self) -> Vec<crate::platform::traits::TerminalApp> {
        MacosShell::TERMINALS
            .iter()
            .filter(|(_, _, app, _)| Self::app_installed(app))
            .map(|(id, name, app, _)| crate::platform::traits::TerminalApp {
                id: (*id).to_string(),
                name: (*name).to_string(),
                icon: Self::app_icon_data_uri(app),
            })
            .collect()
    }

    fn open_in_terminal(&self, terminal_id: &str, path: &Path) -> Result<()> {
        // Directories only. `open -a Terminal <file>` RUNS the file as a script
        // — the one mistake this control must not make. Checked before the
        // lookup so the refusal never depends on what is installed.
        if !path.is_dir() {
            return Err(Error::Other(format!(
                "refusing to open a terminal at {} — only an existing directory is a working \
                 directory (a terminal handed a file would run it)",
                path.display()
            )));
        }
        let dir = path
            .to_str()
            .ok_or_else(|| Error::Other(format!("path is not valid UTF-8: {}", path.display())))?;
        let (_, name, app, launch) = MacosShell::TERMINALS
            .iter()
            .find(|(id, _, _, _)| *id == terminal_id)
            .ok_or_else(|| Error::Other(format!("unknown terminal: {terminal_id}")))?;
        if !Self::app_installed(app) {
            return Err(Error::Other(format!("{name} is not installed anymore")));
        }
        let mut cmd = std::process::Command::new("open");
        match launch {
            TermLaunch::Folder => {
                cmd.args(["-a", app, dir]);
            }
            // `-n` is load-bearing: for an ALREADY-RUNNING app macOS drops
            // `--args` entirely, and the new window would open in $HOME.
            TermLaunch::FlagEq(flag) => {
                cmd.args(["-na", app, "--args", &format!("{flag}={dir}")]);
            }
            TermLaunch::Args(args) => {
                cmd.args(["-na", app, "--args"]);
                cmd.args(*args);
                cmd.arg(dir);
            }
        }
        let status = cmd.status()?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::Other(format!("`open -a {app}` failed: {status}")))
        }
    }

    fn login_shell_env(&self) -> Result<Vec<(String, String)>> {
        use std::io::Read;
        use std::process::Stdio;
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        // -l loads .zprofile (Homebrew shellenv), -i loads .zshrc (nvm/fnm/
        // asdf init) — a terminal gives tools both, so resolution does too.
        // rc files print freely; the NUL marker + `env -0` make the parse
        // immune to that noise (core::devtools::parse_shell_env_output).
        // stdin is /dev/null so an rc-file `read` can't hang us.
        let cmd = format!(
            "printf '\\0{}\\0'; command env -0",
            crate::platform::traits::ENV_MARKER
        );
        let mut child = std::process::Command::new(&shell)
            .args(["-ilc", &cmd])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let mut stdout = child.stdout.take().expect("stdout piped above");
        // Reader thread + timeout: reading in-line could block forever on a
        // pathological rc file; killing on a timer without draining the pipe
        // could deadlock a noisy one. The thread drains to EOF (also after a
        // kill), recv_timeout caps the wait.
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stdout.read_to_end(&mut buf);
            let _ = tx.send(buf);
        });
        let raw = match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(buf) => buf,
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Other(format!(
                    "your shell ({shell}) took more than 10s to start — a slow \
                     startup file? Fix the shell startup, then hit Re-detect."
                )));
            }
        };
        let _ = child.wait(); // reap — no zombie
        let env = crate::platform::traits::parse_shell_env_output(&raw);
        if env.iter().any(|(k, _)| k == "PATH") {
            Ok(env)
        } else {
            Err(Error::Other(format!(
                "couldn't read your shell environment ({shell} printed no \
                 PATH). Check the shell's startup files, then hit Re-detect."
            )))
        }
    }

    fn symlink_dir(&self, target: &Path, link: &Path) -> Result<()> {
        std::os::unix::fs::symlink(target, link)?;
        Ok(())
    }

    fn symlink_file(&self, target: &Path, link: &Path) -> Result<()> {
        // A unix symlink is untyped — the same call as `symlink_dir`. EEXIST
        // surfaces as `AlreadyExists` for the caller to decide on.
        std::os::unix::fs::symlink(target, link)?;
        Ok(())
    }

    fn remove_symlink(&self, link: &Path) -> Result<()> {
        // A dir symlink is a FILE entry on unix — remove_file drops the link
        // itself and can never recurse into the target.
        let meta = std::fs::symlink_metadata(link)?;
        if !meta.file_type().is_symlink() {
            return Err(Error::Other(format!(
                "{} is not a symlink — refusing to remove it here",
                link.display()
            )));
        }
        std::fs::remove_file(link)?;
        Ok(())
    }

    fn git_preflight(&self) -> Result<()> {
        // `/usr/bin/git` is an Xcode CLT shim: executing it WITHOUT the tools
        // installed pops a GUI install dialog — never acceptable from a
        // background task. `xcode-select -p` answers quietly: exit 0 = a
        // developer directory exists, git is real.
        let ok = std::process::Command::new("/usr/bin/xcode-select")
            .arg("-p")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            Ok(())
        } else {
            Err(Error::Other(
                "git needs the Xcode Command Line Tools (not installed). \
                 Install them, then hit Re-detect:\n\
                 $ xcode-select --install"
                    .into(),
            ))
        }
    }
}

impl MacosShell {
    /// Editors we can detect: (stable id, display name, .app bundle name).
    /// Ordered by rough popularity — the first detected one is the default.
    const EDITORS: &'static [(&'static str, &'static str, &'static str)] = &[
        ("vscode", "VS Code", "Visual Studio Code"),
        ("cursor", "Cursor", "Cursor"),
        ("phpstorm", "PhpStorm", "PhpStorm"),
        ("windsurf", "Windsurf", "Windsurf"),
        ("zed", "Zed", "Zed"),
        ("sublime", "Sublime Text", "Sublime Text"),
        ("webstorm", "WebStorm", "WebStorm"),
        ("vscodium", "VSCodium", "VSCodium"),
        ("nova", "Nova", "Nova"),
        ("textmate", "TextMate", "TextMate"),
    ];

    /// Browsers we can detect: (stable id, display name, .app bundle name,
    /// bundle identifier, private-window flag). The bundle id is what
    /// LaunchServices names as the `https` handler, so it is what marks the
    /// system default. Ordered by rough popularity — the first detected one is
    /// the fallback default.
    ///
    /// The last field is the command-line flag that opens the url straight in a
    /// private/incognito window, or `None` when this browser has no such flag
    /// **that we have actually seen work**. `None` is the safe default and the
    /// UI simply draws no private affordance on that row:
    /// - Safari has no private-window command line at all (only a ⇧⌘N keystroke
    ///   through the Accessibility API, a TCC grant rexenv does not ask for).
    /// - Arc and ChatGPT Atlas are Chromium forks, but a fork is free to swallow
    ///   the window flags — and a flag that is *ignored* opens a NORMAL window
    ///   under a control labelled private, which is worse than no control.
    ///   Filling one in is a one-line change once it is tested on a real install.
    /// - Orion is WebKit with no documented flag; every Tor Browser window is
    ///   already private, so the affordance would say nothing.
    const BROWSERS: &'static [(
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        Option<&'static str>,
    )] = &[
        ("safari", "Safari", "Safari", "com.apple.safari", None),
        ("chrome", "Google Chrome", "Google Chrome", "com.google.chrome", Some("--incognito")),
        ("firefox", "Firefox", "Firefox", "org.mozilla.firefox", Some("-private-window")),
        ("brave", "Brave", "Brave Browser", "com.brave.browser", Some("--incognito")),
        ("edge", "Microsoft Edge", "Microsoft Edge", "com.microsoft.edgemac", Some("--inprivate")),
        ("arc", "Arc", "Arc", "company.thebrowser.browser", None),
        ("opera", "Opera", "Opera", "com.operasoftware.opera", Some("--private")),
        ("vivaldi", "Vivaldi", "Vivaldi", "com.vivaldi.vivaldi", Some("--incognito")),
        ("chromium", "Chromium", "Chromium", "org.chromium.chromium", Some("--incognito")),
        ("atlas", "ChatGPT Atlas", "ChatGPT Atlas", "com.openai.atlas", None),
        ("zen", "Zen Browser", "Zen Browser", "app.zen-browser.zen", Some("-private-window")),
        ("orion", "Orion", "Orion", "com.kagi.kagimacos", None),
        (
            "librewolf",
            "LibreWolf",
            "LibreWolf",
            "io.gitlab.librewolf-community",
            Some("-private-window"),
        ),
        (
            "chrome-canary",
            "Chrome Canary",
            "Google Chrome Canary",
            "com.google.chrome.canary",
            Some("--incognito"),
        ),
        (
            "firefox-dev",
            "Firefox Developer Edition",
            "Firefox Developer Edition",
            "org.mozilla.firefoxdeveloperedition",
            Some("-private-window"),
        ),
        ("tor", "Tor Browser", "Tor Browser", "org.torproject.torbrowser", None),
    ];

    /// Terminal emulators we can detect: (stable id, display name, .app bundle
    /// name, how it is told which directory to start in). Ordered by rough
    /// popularity; Terminal.app is first because every Mac has it.
    ///
    /// The last field is the honest part. `Folder` means the app takes a
    /// directory as a plain document argument (`open -a <app> <dir>`), which is
    /// how Terminal.app and iTerm open a window already `cd`-ed there — both
    /// verified on a real install. The flag variants come from each app's
    /// documented command line and are used with `-n`, because `open -a <app>
    /// --args …` DROPS the arguments when the app is already running (the same
    /// trap the private-window browser flags hit) — a dropped `--working-
    /// directory` would open a window in the user's HOME under a control that
    /// said "this plugin's folder". An app whose cwd handling we do not know is
    /// simply not on this list: a missing row costs a menu entry, a wrong row
    /// costs the user a command typed in the wrong directory.
    const TERMINALS: &'static [(&'static str, &'static str, &'static str, TermLaunch)] = &[
        ("terminal", "Terminal", "Terminal", TermLaunch::Folder),
        ("iterm", "iTerm", "iTerm", TermLaunch::Folder),
        ("warp", "Warp", "Warp", TermLaunch::Folder),
        ("ghostty", "Ghostty", "Ghostty", TermLaunch::FlagEq("--working-directory")),
        ("wezterm", "WezTerm", "WezTerm", TermLaunch::Args(&["start", "--cwd"])),
        ("kitty", "kitty", "kitty", TermLaunch::Args(&["--directory"])),
        ("alacritty", "Alacritty", "Alacritty", TermLaunch::Args(&["--working-directory"])),
    ];

    /// The `.app` bundle directory for a bundle NAME, searched in the places
    /// [`Self::app_installed`] accepts.
    ///
    /// `/System/Applications/Utilities` is on the list for ONE app that every
    /// Mac has and no Mac can move: Terminal.app. Since macOS 11 the stock apps
    /// live on the sealed system volume, so a search of `/Applications` +
    /// `~/Applications` alone reports "no terminal installed" on a machine whose
    /// terminal is the one Apple shipped.
    fn app_bundle_path(app: &str) -> Option<PathBuf> {
        let bundle = format!("{app}.app");
        for dir in ["/Applications", "/System/Applications/Utilities", "/System/Applications"] {
            let candidate = Path::new(dir).join(&bundle);
            if candidate.exists() {
                return Some(candidate);
            }
        }
        directories::BaseDirs::new()
            .map(|b| b.home_dir().join("Applications").join(&bundle))
            .filter(|p| p.exists())
    }

    /// An app bundle exists in one of the searched application folders.
    fn app_installed(app: &str) -> bool {
        Self::app_bundle_path(app).is_some()
    }

    /// The app's OWN icon as a `data:image/png;base64,…` URI (64px), read off
    /// the installed bundle: `Info.plist` → `CFBundleIconFile` → `sips` to PNG.
    ///
    /// Real icons, not a hand-drawn brand table: a table of brand SVGs would
    /// hardcode vendor hex (against DESIGN.md) and go stale on every rebrand.
    /// `None` when the bundle keeps its icon only in a compiled asset catalog —
    /// the UI then draws its own monochrome glyph, which is honest, rather than
    /// a wrong mark. Measured ~24ms/app, so the result is cached for the life of
    /// the process (an app's icon doesn't change under a running rexenv).
    fn app_icon_data_uri(app: &str) -> Option<String> {
        use std::collections::HashMap;
        use std::sync::{Mutex, OnceLock};
        static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
        let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        if let Ok(c) = cache.lock() {
            if let Some(hit) = c.get(app) {
                return hit.clone();
            }
        }
        let icon = Self::read_app_icon(app);
        if let Ok(mut c) = cache.lock() {
            c.insert(app.to_string(), icon.clone());
        }
        icon
    }

    /// The uncached half of [`Self::app_icon_data_uri`].
    fn read_app_icon(app: &str) -> Option<String> {
        use base64::Engine;
        let bundle = Self::app_bundle_path(app)?;
        let resources = bundle.join("Contents/Resources");

        // `CFBundleIconFile` may omit the extension (Safari stores "AppIcon"),
        // and some bundles don't declare it at all — hence the two conventional
        // fallbacks. Every candidate is a path we build, never user input.
        let declared = std::process::Command::new("defaults")
            .arg("read")
            .arg(bundle.join("Contents/Info"))
            .arg("CFBundleIconFile")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
            .map(|s| if s.ends_with(".icns") { s } else { format!("{s}.icns") });

        let icns = declared
            .into_iter()
            .chain(["AppIcon.icns".to_string(), "app.icns".to_string()])
            .map(|name| resources.join(name))
            .find(|p| p.is_file())?;

        // sips writes to a file, so round-trip through a uniquely named temp
        // (pid + bundle name) and delete it — a shared fixed name would race
        // between two rexenv processes.
        let tmp = std::env::temp_dir().join(format!(
            "rexenv-appicon-{}-{}.png",
            std::process::id(),
            app.replace(' ', "-")
        ));
        let ok = std::process::Command::new("sips")
            .args(["-s", "format", "png", "-Z", "64"])
            .arg(&icns)
            .arg("--out")
            .arg(&tmp)
            .output()
            .is_ok_and(|o| o.status.success());
        let png = ok.then(|| std::fs::read(&tmp).ok()).flatten();
        let _ = std::fs::remove_file(&tmp);
        let png = png?;
        Some(format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png)
        ))
    }

    /// Bundle id of the OS's current default `https` handler, or `None` when
    /// LaunchServices has no entry for the scheme (a machine that never changed
    /// its browser) — the caller then treats macOS's factory default, Safari,
    /// as the default when it's installed. Display only: `open` asks
    /// LaunchServices itself, so a wrong answer here can only mis-draw an icon.
    fn default_browser_bundle_id() -> Option<String> {
        let out = std::process::Command::new("defaults")
            .args([
                "read",
                "com.apple.LaunchServices/com.apple.launchservices.secure",
                "LSHandlers",
            ])
            .output()
            .ok()?;
        let parsed = crate::platform::traits::parse_default_browser_bundle_id(
            &String::from_utf8_lossy(&out.stdout),
        );
        parsed.or_else(|| Self::app_installed("Safari").then(|| "com.apple.safari".to_string()))
    }
}

pub struct MacosBinaryProvider;

impl MacosBinaryProvider {
    /// Map a non-system dylib dependency (e.g. a Homebrew path) to the macOS
    /// system equivalent under `/usr/lib`, if known. macOS system dylibs live in
    /// the dyld shared cache (not real files), so we can't stat them — instead we
    /// match a known list by library-name prefix. Unknown deps return `None` so
    /// relinking fails loudly rather than shipping a binary that won't load.
    fn system_lib_for(dep: &str) -> Option<String> {
        let base = Path::new(dep).file_name()?.to_str()?;
        const KNOWN: &[(&str, &str)] = &[
            ("libpcre2-8", "/usr/lib/libpcre2-8.dylib"),
            ("libpcre.", "/usr/lib/libpcre.dylib"),
            ("libz.", "/usr/lib/libz.dylib"),
            ("libiconv", "/usr/lib/libiconv.dylib"),
        ];
        KNOWN
            .iter()
            .find(|(prefix, _)| base.starts_with(prefix))
            .map(|(_, target)| target.to_string())
    }

    /// Ad-hoc code-sign a Mach-O in place. Must run LAST in any prepare step —
    /// `install_name_tool` invalidates signatures and Apple Silicon SIGKILLs
    /// unsigned binaries.
    fn ad_hoc_sign(path: &Path) -> Result<()> {
        let out = std::process::Command::new("codesign")
            .arg("--force")
            .arg("--sign")
            .arg("-")
            .arg(path)
            .output()?;
        if out.status.success() {
            Ok(())
        } else {
            Err(Error::Other(format!(
                "codesign {} failed: {}",
                path.display(),
                String::from_utf8_lossy(&out.stderr).trim()
            )))
        }
    }

    /// Rewrite any non-system (e.g. Homebrew) dylib dependencies to macOS system
    /// libs so the binary runs without Homebrew. Errors if a dep has no system
    /// equivalent (so we never ship a binary that will fail to load).
    ///
    /// The dep list is READ FROM THE FILE (`core::macho::linked_dylibs`), never
    /// asked of `otool`: `/usr/bin/otool` is an Xcode Command Line Tools shim
    /// that pops an install dialog and fails on every Mac without the tools —
    /// which is every clean Mac, and was every first cold run of 0.7.0–0.7.2
    /// (18 Sep 2026, clean-VM smoke test: all six components
    /// `otool -L failed: xcode-select: error: …`). No pinned single binary has
    /// a foreign dep today, so on the common path this touches no tool at all;
    /// `install_name_tool` is reached only when a rewrite is actually needed,
    /// and then says plainly that it needs the tools (`clt_preflight`).
    fn relink_to_system_libs(path: &Path) -> Result<()> {
        for dep in Self::load_command_deps(path)? {
            if dep.starts_with("/usr/lib/") || dep.starts_with("/System/") {
                continue; // already a system lib
            }
            let target = Self::system_lib_for(&dep).ok_or_else(|| {
                Error::Other(format!("no macOS system lib for dependency {dep}"))
            })?;
            Self::install_name_tool(&["-change", &dep, &target], path)?;
        }
        Ok(())
    }

    /// Lexically resolve a `@loader_path`-relative load command against the
    /// Mach-O's own directory. **Lexical on purpose** — `canonicalize` would fail
    /// on a dep not staged yet and would resolve symlinks the bundle keeps
    /// deliberately; what matters here is where the load command POINTS, which is
    /// a string question, not a filesystem one.
    fn resolve_loader_relative(macho_dir: &Path, rest: &str) -> PathBuf {
        use std::path::Component;
        let mut out = macho_dir.to_path_buf();
        for comp in Path::new(rest).components() {
            match comp {
                Component::ParentDir => {
                    out.pop();
                }
                Component::CurDir => {}
                c => out.push(c.as_os_str()),
            }
        }
        out
    }

    /// Whether a load-command path must be rewritten to point inside the bundle
    /// tree. System libs are fine; everything else (Homebrew
    /// `@@HOMEBREW_PREFIX@@`/`@@HOMEBREW_CELLAR@@` placeholders, real
    /// `/opt/homebrew`//`/usr/local` paths, `@rpath` we don't manage) is not.
    ///
    /// **A `@loader_path/` prefix is not by itself proof of being in-tree, and
    /// treating it as such published bundles dyld refuses to load.** This
    /// returned `false` for anything starting `@loader_path/`, so the
    /// shivammathur `php@7.4` bottle — whose every non-system dep reads
    /// `@loader_path/../../../../opt/<formula>/lib/…` — sailed through BOTH the
    /// rewrite loop and the post-relink verify loop, `prepare_binary_tree`
    /// reported success over 49 Mach-Os, and the published binary then died:
    ///
    /// ```text
    /// dyld: Library not loaded: @loader_path/../../../../opt/tidy-html5/lib/libtidy.58.dylib
    /// ```
    ///
    /// Reproduced end to end 14 Aug 2026 (`docs/archive/PLAN-php-74-support.md` §5.2), and
    /// the failure is unrecoverable in the field: `resolve_bundle`'s early return
    /// only checks that the `member` file EXISTS, so the dead tree is cached and
    /// every later resolve short-circuits to it. The guard's claimed surface is
    /// "nothing unresolvable remains" and it was checking one syntactic prefix
    /// inside that surface — the ledger's `guard covers claimed surface` family.
    ///
    /// So the prefix is now RESOLVED and required to land under `root`. Escaping
    /// entries are relinked like any other foreign dep, which means a dep we did
    /// not bundle is a loud error instead of a silent publish.
    ///
    /// `@executable_path/` is deliberately NOT given the same treatment and always
    /// needs relinking: the executable that will load a given dylib is chosen by
    /// whoever spawns it, so it cannot be resolved from the tree at all. Rewriting
    /// it to `@loader_path` makes it well-defined. No bundle rexenv ships uses it
    /// (checked across the cached redis/mariadb/httpd/xdebug trees).
    fn needs_tree_relink(root: &Path, macho: &Path, dep: &str) -> bool {
        if dep.starts_with("/usr/lib/") || dep.starts_with("/System/") {
            return false;
        }
        let Some(rest) = dep.strip_prefix("@loader_path/") else {
            return true;
        };
        match macho.parent() {
            Some(dir) => !Self::resolve_loader_relative(dir, rest).starts_with(root),
            None => true,
        }
    }

    /// The `@loader_path`-relative path from `macho` to the bundle's
    /// `lib/<base>`: one `../` per directory level below `root`. Pure.
    fn loader_path_dep(root: &Path, macho: &Path, base: &str) -> Result<String> {
        let parent = macho
            .parent()
            .ok_or_else(|| Error::Other(format!("{} has no parent dir", macho.display())))?;
        let rel = parent.strip_prefix(root).map_err(|_| {
            Error::Other(format!(
                "{} is not under bundle root {}",
                macho.display(),
                root.display()
            ))
        })?;
        let ups = "../".repeat(rel.components().count());
        Ok(format!("@loader_path/{ups}lib/{base}"))
    }

    /// First four bytes match a Mach-O (thin or fat, either endianness).
    fn is_mach_o(path: &Path) -> bool {
        use std::io::Read;
        let Ok(mut f) = std::fs::File::open(path) else {
            return false;
        };
        let mut magic = [0u8; 4];
        if f.read_exact(&mut magic).is_err() {
            return false;
        }
        matches!(
            magic,
            [0xfe, 0xed, 0xfa, 0xce]
                | [0xfe, 0xed, 0xfa, 0xcf]
                | [0xce, 0xfa, 0xed, 0xfe]
                | [0xcf, 0xfa, 0xed, 0xfe]
                | [0xca, 0xfe, 0xba, 0xbe]
                | [0xca, 0xfe, 0xba, 0xbf]
        )
    }

    /// All Mach-O regular files under `root` (recursive; symlinks skipped —
    /// their targets are visited as real files).
    fn mach_o_files(root: &Path) -> Result<Vec<PathBuf>> {
        let mut found = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir)? {
                let entry = entry?;
                let path = entry.path();
                let meta = std::fs::symlink_metadata(&path)?;
                if meta.file_type().is_symlink() {
                    continue;
                }
                if meta.is_dir() {
                    stack.push(path);
                } else if meta.is_file() && Self::is_mach_o(&path) {
                    found.push(path);
                }
            }
        }
        Ok(found)
    }

    /// The dependency paths as `otool -L` would list them (header line
    /// dropped): for a dylib this INCLUDES its own install name (ID) as the
    /// first entry. Read from the load commands — no toolchain involved.
    fn load_command_deps(path: &Path) -> Result<Vec<String>> {
        let linked = Self::linked_dylibs(path)?;
        let mut out = linked.id.into_iter().collect::<Vec<_>>();
        out.extend(linked.deps);
        Ok(out)
    }

    /// A dylib's install name (`LC_ID_DYLIB`), or `None` for executables.
    fn dylib_id(path: &Path) -> Result<Option<String>> {
        Ok(Self::linked_dylibs(path)?.id)
    }

    fn linked_dylibs(path: &Path) -> Result<crate::core::macho::LinkedDylibs> {
        crate::core::macho::linked_dylibs(path).ok_or_else(|| {
            Error::Other(format!(
                "{} is not a 64-bit Mach-O this build can read — refusing to publish \
                 a binary whose load commands cannot be checked",
                path.display()
            ))
        })
    }

    /// `install_name_tool` IS the Xcode Command Line Tools, and its
    /// `/usr/bin` shim pops a GUI install dialog when they are absent (same
    /// shape as `git_preflight`). Probe quietly first so a missing toolchain is
    /// a plain error naming the fix, never a surprise dialog from a download.
    fn clt_preflight(what: &str) -> Result<()> {
        let ok = std::process::Command::new("/usr/bin/xcode-select")
            .arg("-p")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            Ok(())
        } else {
            Err(Error::Other(format!(
                "{what} needs the Xcode Command Line Tools (not installed). \
                 Install them, then Retry:\n\
                 $ xcode-select --install"
            )))
        }
    }

    fn install_name_tool(args: &[&str], path: &Path) -> Result<()> {
        Self::clt_preflight(&format!("relinking {}", path.display()))?;
        let st = std::process::Command::new("install_name_tool")
            .args(args)
            .arg(path)
            .output()?;
        if st.status.success() {
            Ok(())
        } else {
            Err(Error::Other(format!(
                "install_name_tool {} {} failed: {}",
                args.join(" "),
                path.display(),
                String::from_utf8_lossy(&st.stderr).trim()
            )))
        }
    }

    /// Rewrite one Mach-O so every non-system load command points inside the
    /// bundle tree via `@loader_path`, then VERIFY nothing unresolvable remains.
    /// A dep whose dylib is not bundled under `<root>/lib/` is a loud error —
    /// never publish a tree that can't load.
    fn relink_into_tree(root: &Path, macho: &Path) -> Result<()> {
        let id = Self::dylib_id(macho)?;
        if let Some(old) = id
            .as_deref()
            .filter(|d| Self::needs_tree_relink(root, macho, d))
        {
            let base = Path::new(old)
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(|| Error::Other(format!("unparseable install name {old}")))?;
            Self::install_name_tool(&["-id", &Self::loader_path_dep(root, macho, base)?], macho)?;
        }
        for dep in Self::load_command_deps(macho)? {
            // A dylib's own ID shows up in -L output — handled above, skip here.
            if id.as_deref() == Some(dep.as_str()) || !Self::needs_tree_relink(root, macho, &dep) {
                continue;
            }
            let base = Path::new(&dep)
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(|| Error::Other(format!("unparseable dylib dep {dep}")))?;
            if !root.join("lib").join(base).exists() {
                return Err(Error::Other(format!(
                    "{} depends on {dep}, but lib/{base} is not in the bundle",
                    macho.display()
                )));
            }
            let target = Self::loader_path_dep(root, macho, base)?;
            Self::install_name_tool(&["-change", &dep, &target], macho)?;
        }
        // Verify: every load command must now be system or in-tree relative —
        // and "in-tree" means RESOLVED under `root`, not merely spelled with a
        // `@loader_path/` prefix. See `needs_tree_relink`.
        for dep in Self::load_command_deps(macho)? {
            if Self::needs_tree_relink(root, macho, &dep) {
                return Err(Error::Other(format!(
                    "{} still references {dep} after relinking",
                    macho.display()
                )));
            }
        }
        Ok(())
    }
}

impl BinaryProvider for MacosBinaryProvider {
    fn arch(&self) -> Arch {
        if cfg!(target_arch = "aarch64") {
            Arch::Arm64
        } else {
            Arch::X86_64
        }
    }
    fn prepare_binary(&self, path: &Path) -> Result<()> {
        // De-quarantine (no-op/ignored if the attribute isn't present).
        let _ = std::process::Command::new("xattr")
            .arg("-d")
            .arg("com.apple.quarantine")
            .arg(path)
            .output();
        // Make self-contained: rewrite Homebrew dylib deps to system libs.
        Self::relink_to_system_libs(path)?;
        // Ad-hoc code-sign LAST — install_name_tool invalidates any signature,
        // and Apple Silicon needs a valid signature to exec the binary.
        Self::ad_hoc_sign(path)
    }
    fn prepare_binary_tree(&self, root: &Path) -> Result<()> {
        let machos = Self::mach_o_files(root)?;
        if machos.is_empty() {
            return Err(Error::Other(format!(
                "no Mach-O files under {} — not a binary bundle",
                root.display()
            )));
        }
        // Relink every Mach-O first, then sign — each file is rewritten at most
        // once, and signing LAST means no signature is ever invalidated after
        // it's laid down (same order rule as prepare_binary).
        for m in &machos {
            Self::relink_into_tree(root, m)?;
        }
        for m in &machos {
            Self::ad_hoc_sign(m)?;
        }
        Ok(())
    }
}

/// Aggregate macOS platform, handed to `core/` as `&dyn Platform`.
/// Local IPC on macOS: a unix-domain socket. The stream is returned with the
/// requested read timeout already applied, and a connect error keeps the kind the
/// OS reported (`NotFound` = no socket file, `ConnectionRefused` = a file nobody
/// listens on), which `dbsource::probe_socket` turns into two different messages.
pub struct MacosLocalIpc;
impl LocalIpc for MacosLocalIpc {
    fn connect(
        &self,
        path: &Path,
        read_timeout: Option<Duration>,
    ) -> std::io::Result<Box<dyn std::io::Read + Send>> {
        let stream = std::os::unix::net::UnixStream::connect(path)?;
        // Best-effort, as the probe it replaces was: a stream whose timeout could
        // not be set still answers the one read a probe makes.
        let _ = stream.set_read_timeout(read_timeout);
        Ok(Box::new(stream))
    }
}

pub struct MacosPlatform {
    paths: MacosPaths,
    dns: MacosDns,
    cert_trust: MacosCertTrust,
    privileges: MacosPrivileges,
    supervisor: MacosSupervisor,
    autostart: MacosAutostart,
    permissions: MacosPermissions,
    shell: MacosShell,
    binaries: MacosBinaryProvider,
    edge: MacosEdgeDaemon,
    dns_agent: MacosDnsAgent,
    app_bundle: app_bundle::MacosAppBundle,
}

impl MacosPlatform {
    pub fn new() -> Self {
        Self {
            paths: MacosPaths,
            dns: MacosDns,
            cert_trust: MacosCertTrust,
            privileges: MacosPrivileges,
            supervisor: MacosSupervisor,
            autostart: MacosAutostart,
            permissions: MacosPermissions,
            shell: MacosShell,
            binaries: MacosBinaryProvider,
            edge: MacosEdgeDaemon,
            dns_agent: MacosDnsAgent,
            app_bundle: app_bundle::MacosAppBundle,
        }
    }
}

impl Default for MacosPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl Platform for MacosPlatform {
    fn paths(&self) -> &dyn Paths {
        &self.paths
    }
    fn dns(&self) -> &dyn DnsManager {
        &self.dns
    }
    fn cert_trust(&self) -> &dyn CertTrustManager {
        &self.cert_trust
    }
    fn privileges(&self) -> &dyn PrivilegeManager {
        &self.privileges
    }
    fn supervisor(&self) -> &dyn ProcessSupervisor {
        &self.supervisor
    }
    fn autostart(&self) -> &dyn AutostartManager {
        &self.autostart
    }
    fn permissions(&self) -> &dyn PermissionManager {
        &self.permissions
    }
    fn shell(&self) -> &dyn ShellRunner {
        &self.shell
    }
    fn binaries(&self) -> &dyn BinaryProvider {
        &self.binaries
    }
    fn edge(&self) -> &dyn EdgeSupervisor {
        &self.edge
    }
    fn dns_agent(&self) -> &dyn DnsAgentManager {
        &self.dns_agent
    }
    fn app_bundle(&self) -> &dyn AppBundle {
        &self.app_bundle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only a socket whose LOCAL end is on the port holds it — the shapes are lsof's own
    /// output, measured 13 Sep 2026 (ledger #599). Plant: comparing the whole name instead
    /// of the part before `->` counts the UDP client and the accepted TCP connection.
    #[test]
    fn lsof_holders_are_local_ends_on_the_port_and_nothing_else() {
        let out = "p98098\nf3\nn*:49634\nf4\nn127.0.0.1:49635->127.0.0.1:49634\n\
                   p77\nf6\nn127.0.0.1:64140->127.0.0.1:49634\n\
                   p88\nf1\nn[::1]:49634\n\
                   p99\nf1\nn127.0.0.1:149634\nn127.0.0.1:9634\n";
        assert_eq!(lsof_local_port_holders(out, 49634), vec![98098, 88]);
        assert_eq!(lsof_local_port_holders(out, 9634), vec![99]);
        assert!(lsof_local_port_holders("", 49634).is_empty());
    }

    /// The busy-workers count (plan §3 D1(b)): connections whose LOCAL end is the pool's port.
    /// nginx's client end has the port after `->`; the listener has no `->`. Plant: splitting on
    /// nothing counts nginx's ends and doubles every request.
    #[test]
    fn lsof_established_counts_the_pools_ends_of_connections_only() {
        let out = "p501\nf8\nn127.0.0.1:9083->127.0.0.1:53211\nf9\nn127.0.0.1:9083->127.0.0.1:53212\n\
                   p400\nf12\nn127.0.0.1:53211->127.0.0.1:9083\nf13\nn127.0.0.1:53212->127.0.0.1:9083\n\
                   p500\nf6\nn127.0.0.1:9083\n\
                   p502\nf8\nn127.0.0.1:19083->127.0.0.1:53999\n";
        assert_eq!(lsof_established_local(out, 9083), 2);
        assert_eq!(lsof_established_local(out, 19083), 1);
        assert_eq!(lsof_established_local("", 9083), 0);
    }

    /// A terminal is handed a DIRECTORY or nothing. `open -a Terminal <file>`
    /// RUNS the file — the one mistake this control must not make — so the
    /// refusal is checked before the app lookup, and therefore holds for a
    /// terminal that is not installed too.
    #[test]
    fn open_in_terminal_refuses_anything_that_is_not_a_directory() {
        let shell = MacosShell;
        let file = std::env::temp_dir().join(format!("rexenv-term-guard-{}.sh", std::process::id()));
        std::fs::write(&file, b"#!/bin/sh\necho nope\n").expect("write temp file");

        // A real, installed terminal (Terminal.app is on every Mac) — so this
        // proves the GUARD, not a missing app.
        let err = shell.open_in_terminal("terminal", &file).unwrap_err().to_string();
        assert!(err.contains("only an existing directory"), "{err}");

        // A path that does not exist at all is the same refusal, not a spawn.
        let ghost = file.with_extension("missing");
        assert!(shell.open_in_terminal("terminal", &ghost).is_err());

        // The directory check runs BEFORE the id lookup: an unknown terminal
        // with a file path must not be able to reach the spawn either.
        let err = shell.open_in_terminal("no-such-terminal", &file).unwrap_err().to_string();
        assert!(err.contains("only an existing directory"), "{err}");

        // A directory gets past the guard and fails only on the unknown id —
        // the guard is not passing everything.
        let err = shell
            .open_in_terminal("no-such-terminal", &std::env::temp_dir())
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown terminal"), "{err}");

        std::fs::remove_file(&file).ok();
    }

    /// Terminal.app lives on the sealed system volume since macOS 11. The
    /// two-folder search this file used for editors and browsers reported "no
    /// terminal installed" on a stock Mac; every Mac must detect at least it.
    #[test]
    fn detect_terminals_finds_the_stock_terminal_app() {
        let found = MacosShell.detect_terminals();
        assert!(
            found.iter().any(|t| t.id == "terminal"),
            "Terminal.app not detected: {:?}",
            found.iter().map(|t| &t.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn attribute_holder_names_the_owning_app() {
        // Herd's bundled nginx must be attributed to HERD — "quit nginx: worker
        // process" tells a user nothing; a real user burned a session on this.
        assert_eq!(
            attribute_holder("/Applications/Herd.app/Contents/Resources/nginx", 1234, false),
            ("Herd (nginx, pid 1234)".into(), Some("Herd".into()))
        );
        // App-managed helper binaries under Application Support (Herd's actual
        // layout: no .app segment in the exe path) attribute via rule 2.
        assert_eq!(
            attribute_holder(
                "/Users/x/Library/Application Support/Herd/bin/nginx-arm",
                52766
            , false),
            ("Herd (nginx-arm, pid 52766)".into(), Some("Herd".into()))
        );
        // Our own reverse-DNS app-data dir reads as rexenv, not dev.rexenv.rexenv.
        assert_eq!(
            attribute_holder(
                "/Library/Application Support/dev.rexenv.rexenv/bin/caddy",
                5
            , false),
            ("rexenv (caddy, pid 5)".into(), Some("rexenv".into()))
        );
        // An app whose process IS the bundle name doesn't repeat itself.
        assert_eq!(
            attribute_holder("/Applications/OrbStack.app/Contents/MacOS/OrbStack", 9, false),
            ("OrbStack (pid 9)".into(), Some("OrbStack".into()))
        );
        // Unknown ownership degrades HONESTLY: name + pid + full path (still
        // actionable), never a bare rewritten process title.
        assert_eq!(
            attribute_holder("/opt/homebrew/bin/nginx", 7, false),
            ("nginx (pid 7, /opt/homebrew/bin/nginx)".into(), None)
        );
        // No path at all (ps fallback returned a title): keep what we have.
        assert_eq!(
            attribute_holder("nginx: worker process", 8, false),
            ("nginx: worker process (pid 8)".into(), None)
        );
    }

    /// The Valet ATTRIBUTION itself, with the flag passed in — because the
    /// probe behind it reads a real file and would make this test say different
    /// things on different machines. (It already did: with the probe inside
    /// `attribute_holder`, the plain-brew-nginx case below started answering
    /// "Valet" on the developer's machine, which has Valet's include in its
    /// Homebrew nginx.conf. A test whose result depends on who runs it is not a
    /// test.)
    #[test]
    fn a_valet_held_port_names_valet() {
        assert_eq!(
            attribute_holder("/opt/homebrew/opt/nginx/bin/nginx", 4242, true),
            ("Valet (nginx, pid 4242)".into(), Some("Valet".into()))
        );
        // The same binary, not identified as Valet's: the generic answer, with
        // the path — never a guess dressed as a name.
        assert_eq!(
            attribute_holder("/opt/homebrew/opt/nginx/bin/nginx", 4242, false),
            ("nginx (pid 4242, /opt/homebrew/opt/nginx/bin/nginx)".into(), None)
        );
    }

    /// Local's router is named as the ROUTER, with the setting that frees the
    /// port while Local keeps running; Local's per-site nginx and anyone else's
    /// nginx are not claimed.
    #[test]
    fn locals_router_is_named_with_the_way_out_that_keeps_local_running() {
        let exe = "/Users/x/Library/Application Support/Local/lightning-services/nginx-1.26.1+3/bin/darwin-arm64/sbin/nginx";
        // The shape measured on the owner's Mac (ps -o command), user renamed.
        let router = format!(
            "nginx: master process {exe} -c /Users/x/Library/Application Support/Local/run/router/nginx/conf/nginx.conf \
             -p /Users/x/Library/Application Support/Local/run/router/nginx"
        );
        assert!(holder_is_locals_router(exe, Some(&router)));
        let per_site = format!("nginx: master process {exe} -c /Users/x/Library/Application Support/Local/run/5V6xCZo2_/conf/nginx/nginx.conf");
        assert!(!holder_is_locals_router(exe, Some(&per_site)), "a Local SITE's nginx is not the router");
        assert!(!holder_is_locals_router(exe, None), "no command line, no claim");
        assert!(!holder_is_locals_router("/Applications/Herd.app/Contents/Resources/nginx", Some(&router)));

        let (holder, app) = locals_router_holder(26114);
        assert_eq!(app.as_deref(), Some("Local"), "\"quit Local\" and the osascript quit stay true");
        assert!(
            holder.starts_with("Local's router (nginx, pid 26114") && holder.contains("Router Mode to localhost"),
            "{holder}"
        );
        // What path-only attribution said before: true, and no way out that keeps Local up.
        assert_eq!(attribute_holder(exe, 26114, false).0, "Local (nginx, pid 26114)");
    }

    /// Valet's tier of the free-the-port advice: their own CLI, not brew.
    ///
    /// `brew services stop nginx` DOES free the port and is what this used to
    /// suggest, via the generic Homebrew tier. It is still the wrong answer for
    /// a Valet user: it leaves their Valet half-stopped, the next `valet`
    /// command puts nginx back, and it is not the command they know. Same rule
    /// as the Herd tier — match the advice to how the thing is MANAGED.
    #[test]
    fn valets_nginx_is_freed_by_valets_own_command() {
        assert_eq!(
            free_port_command(Some("Valet"), "/opt/homebrew/opt/nginx/bin/nginx", 4242),
            "valet stop"
        );
        // A Homebrew nginx that is NOT Valet's keeps the brew tier — the
        // attribution decides this, and when it says nothing the advice must
        // not start guessing.
        assert_eq!(
            free_port_command(None, "/opt/homebrew/opt/nginx/bin/nginx", 4242),
            "brew services stop nginx || sudo brew services stop nginx"
        );
    }

    #[test]
    fn free_port_command_matches_how_the_holder_is_managed() {
        // App-supervised (Herd): kill a worker and the app respawns it — the only
        // command that actually frees the port is quitting the APP. Live-verified:
        // this exact command quit Herd and freed 127.0.0.1:443.
        assert_eq!(
            free_port_command(Some("Herd"), "/Applications/Herd.app/x/nginx", 52766),
            "osascript -e 'quit app \"Herd\"'"
        );
        // Homebrew binary: usually a brew service — a bare kill gets respawned
        // too. Try the user domain first, then root (e.g. a root brew nginx).
        assert_eq!(
            free_port_command(None, "/opt/homebrew/Cellar/nginx/1.27.0/bin/nginx", 7),
            "brew services stop nginx || sudo brew services stop nginx"
        );
        assert_eq!(
            free_port_command(None, "/usr/local/opt/nginx/sbin/nginx", 7),
            "brew services stop nginx || sudo brew services stop nginx"
        );
        // Our own orphan: never "quit rexenv" — a direct kill of the master.
        assert_eq!(
            free_port_command(
                Some("rexenv"),
                "/Library/Application Support/dev.rexenv.rexenv/bin/caddy",
                5
            ),
            "sudo kill 5"
        );
        // Unknown standalone binary: last resort, aimed at the MASTER pid.
        assert_eq!(free_port_command(None, "/usr/sbin/httpd", 99), "sudo kill 99");
        // A non-brew /opt path must not be mistaken for a formula.
        assert_eq!(free_port_command(None, "/opt/custom/opt/thing/bin/x", 3), "sudo kill 3");
        // A legit spaced app name is still quoted safely in the AppleScript.
        assert_eq!(
            free_port_command(Some("Docker Desktop"), "/Applications/Docker.app/x", 8),
            "osascript -e 'quit app \"Docker Desktop\"'"
        );
        // Injection guard: a crafted app name (quote/semicolon) must NOT reach the
        // shell — it degrades to the safe `sudo kill <pid>` tier.
        assert_eq!(
            free_port_command(Some("evil\";reboot #"), "/Applications/Evil.app/x", 42),
            "sudo kill 42"
        );
        // Same for a crafted brew-formula path segment.
        assert_eq!(
            free_port_command(None, "/opt/homebrew/Cellar/ev;il/1.0/bin/x", 43),
            "sudo kill 43"
        );
    }

    #[test]
    fn dns_agent_plist_keeps_resolver_alive_from_login() {
        let agent = MacosDnsAgent;
        let plist = agent.definition_contents(
            Path::new("/Applications/rexenv.app/Contents/MacOS/rexenv"),
            Path::new("/l/dns-agent.log"),
        );
        // KeepAlive + RunAtLoad: resolver up from login, relaunched on any death —
        // this is what makes sites resolve with the app closed and after reboot.
        assert!(plist.contains("<key>KeepAlive</key>\n\t<true/>"), "plist:\n{plist}");
        assert!(plist.contains("<key>RunAtLoad</key>\n\t<true/>"));
        // Runs the app binary in headless resolver mode.
        assert!(plist.contains("<string>/Applications/rexenv.app/Contents/MacOS/rexenv</string>"));
        assert!(plist.contains("<string>--dns-agent</string>"));
        // Own label, distinct from the app-autostart agent and the edge daemon.
        assert!(plist.contains(&format!("<string>{DNS_AGENT_LABEL}</string>")));
        assert_ne!(DNS_AGENT_LABEL, AUTOSTART_LABEL);
        assert_ne!(DNS_AGENT_LABEL, EDGE_DAEMON_LABEL);
        // Unprivileged: a per-user LaunchAgent, never a system daemon.
        let path = agent.definition_path().unwrap();
        assert!(path.display().to_string().contains("Library/LaunchAgents"));
    }

    #[test]
    fn edge_daemon_keeps_alive_and_uses_root_owned_binary() {
        // KeepAlive + RunAtLoad = the whole point: relaunch on ANY death and after boot.
        let ed = MacosEdgeDaemon;
        let wrapper = ed.wrapper_path();
        let plist = ed.plist_contents(&wrapper, Path::new("/l/edge.log"));
        assert!(plist.contains("<key>KeepAlive</key>\n\t<true/>"), "plist:\n{plist}");
        assert!(plist.contains("<key>RunAtLoad</key>\n\t<true/>"));
        // A daemon, not the app's Interactive LaunchAgent.
        assert!(plist.contains("<string>Background</string>"));
        assert!(!plist.contains("Interactive"));
        // Distinct label from the app's autostart LaunchAgent — never the same job.
        assert!(plist.contains(&format!("<string>{EDGE_DAEMON_LABEL}</string>")));
        assert_ne!(EDGE_DAEMON_LABEL, AUTOSTART_LABEL);
        // The daemon runs the wrapper, which lives in the root tree (not user-writable).
        assert!(plist.contains(wrapper.to_str().unwrap()));
        assert!(ed.daemon_binary_path().starts_with(EDGE_ROOT_DIR));
        assert!(ed.plist_path().starts_with("/Library/LaunchDaemons"));
    }

    #[test]
    fn edge_daemon_install_hardens_ownership_and_targets_root_binary() {
        // The user cache binary is the SOURCE; the daemon must execute the ROOT copy.
        let ed = MacosEdgeDaemon;
        let src = Path::new("/Users/me/Library/Application Support/dev.rexenv.rexenv/bin/caddy");
        let cmd = ed.install_command(src, &ed.wrapper_path(), Path::new("/tmp/staged.plist"));
        let root_bin = ed.daemon_binary_path();
        // Copies the user binary INTO the root tree, then locks it root:wheel 0755 —
        // the LPE guard: launchd never re-execs the user-writable cache binary.
        assert!(cmd.contains(&format!("cp {}", super::sh_quote(src))));
        assert!(cmd.contains(&format!("chown root:wheel {}", super::sh_quote(&root_bin))));
        assert!(cmd.contains(&format!("chmod 755 {}", super::sh_quote(&root_bin))));
        // The daemon plist must be root:wheel 0644 — launchd rejects a
        // group/other-writable daemon plist, and a writable one is an LPE.
        let plist = ed.plist_path();
        assert!(cmd.contains(&format!("chmod 644 {}", super::sh_quote(&plist))));
        // Bootstraps into the SYSTEM (root) domain, replacing any prior job.
        assert!(cmd.contains(&format!("launchctl bootstrap system {}", super::sh_quote(&plist))));
        assert!(cmd.contains(&format!("launchctl bootout system/{EDGE_DAEMON_LABEL}")));
        // Re-enables BEFORE bootstrap — an explicit Stop-all disables the label, and
        // bootstrap won't run a disabled service, so a Start-all after Stop-all needs
        // this or the edge never launches.
        assert!(cmd.contains(&format!("launchctl enable system/{EDGE_DAEMON_LABEL}")));
        assert!(
            cmd.find("enable").unwrap() < cmd.find("bootstrap").unwrap(),
            "enable must precede bootstrap: {cmd}"
        );
    }

    #[test]
    fn edge_daemon_stop_boots_out_so_keepalive_cannot_relaunch() {
        // Explicit stop must remove the job from launchd — otherwise KeepAlive fights
        // `caddy stop`. disable BEFORE bootout so it stays down across reboots.
        let cmd = MacosEdgeDaemon.stop_command();
        assert!(cmd.contains(&format!("launchctl disable system/{EDGE_DAEMON_LABEL}")));
        assert!(cmd.contains(&format!("launchctl bootout system/{EDGE_DAEMON_LABEL}")));
        assert!(
            cmd.find("disable").unwrap() < cmd.find("bootout").unwrap(),
            "disable must precede bootout: {cmd}"
        );
    }

    #[test]
    fn edge_daemon_uninstall_boots_out_and_removes_plist_wrapper_and_binary() {
        // "Remove system changes" must FULLY remove the daemon — bootout AND rm the
        // plist, wrapper, and root-owned caddy copy — or KeepAlive keeps a root
        // Caddy serving :443 after uninstall (B2). (stop_command only disables +
        // boots out; it deliberately leaves the files for a temporary Stop-all.)
        let d = MacosEdgeDaemon;
        let cmd = d.uninstall_command();
        assert!(
            cmd.contains(&format!("launchctl bootout system/{EDGE_DAEMON_LABEL}")),
            "{cmd}"
        );
        assert!(cmd.contains("rm -f"), "must remove files, not just bootout: {cmd}");
        for path in [d.plist_path(), d.wrapper_path(), d.daemon_binary_path()] {
            assert!(
                cmd.contains(&path.display().to_string()),
                "must remove {}: {cmd}",
                path.display()
            );
        }
    }

    #[test]
    fn edge_daemon_wrapper_chowns_socket_then_execs_caddy() {
        let w = MacosEdgeDaemon.wrapper_contents(
            Path::new("/root/bin/caddy"),
            Path::new("/u/Caddyfile"),
            Path::new("/u/config/caddy-admin.sock"),
            Path::new("/u/appdata"),
        );
        // Reads the invoking user's uid from the app-data dir owner...
        assert!(w.contains("OWNER=$(stat -f %u '/u/appdata')"));
        // ...and PERSISTENTLY re-chowns the socket to that user (caddy recreates it as
        // root on every reload, so a one-shot chown would be lost — the :443 wedge).
        // `-h`: never follow a symlink — a root loop chowning a user-controlled path
        // must not be redirectable onto another daemon's socket (LPE).
        assert!(w.contains("chown -h \"$OWNER\" \"$SOCK\""));
        assert!(w.contains("while :; do"), "must loop, not run once: {w}");
        // Link-count guard BEFORE the chown (B16): a real socket has one link, so
        // this is transparent; it refuses to chown a HARDLINKED inode (a same-user
        // hardlink to another root socket would otherwise get chowned by this root
        // loop — LPE). Must sit in the `&&` chain ahead of the chown.
        assert!(
            w.contains(
                "&& [ \"$(stat -f %l \"$SOCK\" 2>/dev/null)\" = 1 ] && chown -h \"$OWNER\" \"$SOCK\""
            ),
            "the link-count guard must precede the chown: {w}"
        );
        // exec (not fork) so launchd's tracked PID is caddy itself.
        assert!(w.contains("exec '/root/bin/caddy' run --config '/u/Caddyfile'"));
    }

    #[test]
    fn osascript_program_wraps_and_escapes() {
        let reason = PromptReason::new(r#"test "quoted" \ things"#);
        let program = MacosPrivileges::osascript_program(r#"echo "hi" \ there"#, &reason);
        assert!(program.starts_with("do shell script \""));
        assert!(program.ends_with("\" with administrator privileges"));
        // Quotes and backslashes are escaped for the AppleScript string literal.
        assert!(program.contains(r#"echo \"hi\" \\ there"#));
        // The fallback dialog still says what it is for.
        assert!(program.contains(r#" with prompt "rexenv wants to test \"quoted\" \\ things." "#), "{program}");
    }

    #[test]
    fn privileged_cancel_is_a_friendly_recoverable_message() {
        // A dismissed auth dialog (AppleScript -128) reads as recoverable, not raw.
        let cancel = MacosPrivileges::privileged_error_message("User canceled. (-128)");
        assert!(cancel.to_lowercase().contains("cancelled"));
        assert!(cancel.to_lowercase().contains("try again"));
        assert!(!cancel.contains("-128"));
        // Other failures keep their detail.
        let other = MacosPrivileges::privileged_error_message("rm: permission denied");
        assert!(other.contains("permission denied"));
    }

    /// #577's fallback split. osascript may ask only when the branded prompt
    /// showed no dialog; a cancel is an answer, and an applet that ran but wrote
    /// nothing is an error — asking again there would be a second password dialog
    /// for a step the user may already have approved.
    #[test]
    fn only_a_run_that_showed_no_dialog_is_asked_again_through_osascript() {
        use prompt_applet::{Outcome, RunError};
        assert!(matches!(
            MacosPrivileges::settle(Ok(Outcome::Ran("root".into()))),
            Settled::Done(Ok(s)) if s == "root"
        ));
        match MacosPrivileges::settle(Ok(Outcome::Failed { number: -128, message: "User canceled.".into() })) {
            Settled::Done(Err(e)) => assert!(e.to_string().contains("cancelled"), "{e}"),
            other => panic!("a cancel is an answer, not a reason to ask again: {other:?}"),
        }
        match MacosPrivileges::settle(Err(RunError::NoResult("gone".into()))) {
            Settled::Done(Err(e)) => assert!(e.to_string().contains("closed without reporting"), "{e}"),
            other => panic!("an applet that ran must never be asked again: {other:?}"),
        }
        assert!(matches!(
            MacosPrivileges::settle(Err(RunError::NoDialog("osacompile failed".into()))),
            Settled::AskThroughOsascript(w) if w == "osacompile failed"
        ));
    }

    #[test]
    fn osascript_program_handles_multi_command_batch() {
        // Batching: multiple privileged commands joined into one script => one prompt.
        let script = "mkdir -p /etc/resolver\ncp /tmp/test /etc/resolver/test";
        let program = MacosPrivileges::osascript_program(script, &PromptReason::new("test"));
        assert!(program.contains("mkdir -p /etc/resolver"));
        assert!(program.contains("cp /tmp/test /etc/resolver/test"));
    }

    #[test]
    fn dns_resolver_path_and_contents() {
        let dns = MacosDns;
        // One file per TLD; the content is TLD-independent (the ownership
        // signature core::dns matches when enumerating our files).
        assert_eq!(dns.resolver_path("test"), PathBuf::from("/etc/resolver/test"));
        assert_eq!(dns.resolver_path("rex"), PathBuf::from("/etc/resolver/rex"));
        assert_eq!(
            dns.resolver_contents(15353),
            "nameserver 127.0.0.1\nport 15353\n"
        );
    }

    /// Ledger #617 — the route trait on macOS says what the file-shaped one said: the label a message
    /// shows is the resolver file's path, and the content a takeover preview shows is the file's exact
    /// bytes (the ownership signature). `route_owner` and the two scans read `/etc/resolver` itself, so
    /// they are not driven here — this machine's own resolver files would decide the answer.
    #[test]
    fn dns_route_label_and_contents_are_the_resolver_file_and_its_bytes() {
        let dns = MacosDns;
        assert_eq!(dns.route_label("test"), "/etc/resolver/test");
        assert_eq!(dns.route_label("rex"), dns.resolver_path("rex").display().to_string());
        assert_eq!(dns.route_contents(15353), "nameserver 127.0.0.1\nport 15353\n");
        assert_eq!(dns.route_contents(15353), dns.resolver_contents(15353));
    }

    #[test]
    fn dns_install_command_creates_dir_and_writes_file() {
        let dns = MacosDns;
        let cmd = dns.install_command("test", 15353);
        assert!(cmd.contains("mkdir -p /etc/resolver"));
        // printf carries the file content with escaped newlines for sh.
        assert!(cmd.contains(r"printf 'nameserver 127.0.0.1\nport 15353\n'"));
        assert!(cmd.contains("> /etc/resolver/test"));
        // flushes the DNS cache so the new resolver file takes effect at once.
        assert!(cmd.contains("dscacheutil -flushcache"));
        assert!(cmd.contains("killall -HUP mDNSResponder"));
        // A non-default TLD writes its own file.
        assert!(dns.install_command("rex", 15353).contains("> /etc/resolver/rex"));
    }

    #[test]
    fn dns_uninstall_command_removes_all_files_and_flushes_cache_once() {
        let cmd = MacosDns.uninstall_command(&["test".into(), "rex".into()]);
        assert!(cmd.contains("rm -f /etc/resolver/test /etc/resolver/rex"));
        // ONE flush so the TLDs stop resolving immediately after teardown (§3.2).
        assert_eq!(cmd.matches("dscacheutil -flushcache").count(), 1);
        assert!(cmd.contains("killall -HUP mDNSResponder"));
        // Zero TLDs: no `rm -f` with no operands (that would error) — flush only.
        let empty = MacosDns.uninstall_command(&[]);
        assert!(!empty.contains("rm"), "no rm without operands: {empty}");
        assert!(empty.contains("dscacheutil -flushcache"));
    }

    #[test]
    fn autostart_plist_is_a_login_launchagent_for_the_program() {
        let plist = MacosAutostart::plist_contents(Path::new("/Applications/rexenv.app"));
        assert!(plist.contains("<string>dev.rexenv.rexenv</string>"));
        assert!(plist.contains("<string>/Applications/rexenv.app</string>"));
        // Runs at GUI login. One contiguous match — split asserts would pass
        // a plist that sets RunAtLoad to <false/> yet has a <true/> elsewhere.
        assert!(plist.contains("<key>RunAtLoad</key>\n\t<true/>"));
        // Lives in the per-user LaunchAgents dir (no root).
        assert!(MacosAutostart::plist_path()
            .unwrap()
            .ends_with("Library/LaunchAgents/dev.rexenv.rexenv.plist"));
    }

    // ── Identity drift guards (release task 1.1) ──────────────────────────────
    // One reverse-DNS id across tauri.conf.json, app-data path, and launchd label.

    #[test]
    fn app_data_namespace_matches_the_identifier() {
        // ProjectDirs composes qualifier.org.name; it must equal APP_IDENTIFIER so
        // the app-data dir lives under `~/Library/Application Support/<identifier>/`.
        assert_eq!(
            format!("{APP_QUALIFIER}.{APP_ORG}.{APP_NAME}"),
            APP_IDENTIFIER
        );
        let dir = MacosPaths.app_data_dir().unwrap();
        assert!(
            dir.ends_with(APP_IDENTIFIER),
            "app_data_dir {dir:?} must end with {APP_IDENTIFIER}"
        );
    }

    #[test]
    fn autostart_label_matches_the_identifier() {
        assert_eq!(AUTOSTART_LABEL, APP_IDENTIFIER);
    }

    #[test]
    fn tauri_conf_identifier_matches_the_identifier() {
        // Read the bundle id straight from tauri.conf.json so a stray edit there
        // (signing/bundle id) can't silently diverge from the runtime identity.
        let conf = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tauri.conf.json"));
        let needle = format!("\"identifier\": \"{APP_IDENTIFIER}\"");
        assert!(
            conf.contains(&needle),
            "tauri.conf.json identifier must be {APP_IDENTIFIER} (found mismatch)"
        );
    }

    #[test]
    fn the_ca_goes_to_this_users_login_keychain_never_the_system_one() {
        let kc = MacosCertTrust::login_keychain();
        assert!(kc.ends_with("/Library/Keychains/login.keychain-db"), "{kc}");
        assert!(!kc.contains("System.keychain"), "{kc}");
    }

    #[test]
    fn system_lib_for_maps_homebrew_pcre2_to_usr_lib() {
        // macOS ships /usr/lib/libpcre2-8.dylib; Homebrew's is libpcre2-8.0.dylib.
        let mapped =
            MacosBinaryProvider::system_lib_for("/opt/homebrew/opt/pcre2/lib/libpcre2-8.0.dylib");
        assert_eq!(mapped.as_deref(), Some("/usr/lib/libpcre2-8.dylib"));
        // Nonexistent lib → no mapping.
        assert!(MacosBinaryProvider::system_lib_for("/opt/homebrew/lib/libnope-9.dylib").is_none());
    }

    #[test]
    fn loader_path_dep_climbs_to_the_bundle_lib() {
        let root = Path::new("/cache/redis-8.8.0");
        // Executable in bin/ → one level up, into lib/.
        let bin = MacosBinaryProvider::loader_path_dep(
            root,
            Path::new("/cache/redis-8.8.0/bin/redis-server"),
            "libssl.3.dylib",
        )
        .unwrap();
        assert_eq!(bin, "@loader_path/../lib/libssl.3.dylib");
        // Dylib in lib/ → also one level up (lib/../lib resolves to lib).
        let lib = MacosBinaryProvider::loader_path_dep(
            root,
            Path::new("/cache/redis-8.8.0/lib/libssl.3.dylib"),
            "libcrypto.3.dylib",
        )
        .unwrap();
        assert_eq!(lib, "@loader_path/../lib/libcrypto.3.dylib");
        // Nested (e.g. lib/ossl-modules/) → two levels up.
        let nested = MacosBinaryProvider::loader_path_dep(
            root,
            Path::new("/cache/redis-8.8.0/lib/ossl-modules/legacy.dylib"),
            "libcrypto.3.dylib",
        )
        .unwrap();
        assert_eq!(nested, "@loader_path/../../lib/libcrypto.3.dylib");
        // A Mach-O outside the root is a hard error, never a bogus path.
        assert!(MacosBinaryProvider::loader_path_dep(
            root,
            Path::new("/elsewhere/bin/redis-server"),
            "libssl.3.dylib"
        )
        .is_err());
    }

    #[test]
    fn needs_tree_relink_flags_only_unresolvable_deps() {
        let root = Path::new("/cache/redis-8.8.0");
        let bin = Path::new("/cache/redis-8.8.0/bin/redis-server");
        let flag = |dep: &str| MacosBinaryProvider::needs_tree_relink(root, bin, dep);

        // Bottle placeholders and real Homebrew prefixes must be rewritten.
        assert!(flag("@@HOMEBREW_PREFIX@@/opt/openssl@3/lib/libssl.3.dylib"));
        assert!(flag("@@HOMEBREW_CELLAR@@/openssl@3/3.6.3/lib/libcrypto.3.dylib"));
        assert!(flag("/opt/homebrew/opt/openssl@3/lib/libssl.3.dylib"));
        // @rpath is unmanaged in bundles — rewrite it into the tree too.
        assert!(flag("@rpath/libfoo.dylib"));
        // System entries stay untouched.
        assert!(!flag("/usr/lib/libSystem.B.dylib"));
        assert!(!flag(
            "/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation"
        ));
        // A @loader_path that lands INSIDE the tree is genuinely in-tree.
        assert!(!flag("@loader_path/../lib/libssl.3.dylib"));
        // …and one from a nested Mach-O, which is what our own rewrite emits.
        assert!(!MacosBinaryProvider::needs_tree_relink(
            root,
            Path::new("/cache/redis-8.8.0/lib/ossl-modules/legacy.dylib"),
            "@loader_path/../../lib/libcrypto.3.dylib"
        ));
    }

    /// The regression that motivated resolving the prefix instead of trusting it.
    /// Every non-system dep of the shivammathur `php@7.4` bottle is spelled
    /// `@loader_path/../../../../opt/<formula>/lib/…` — which ESCAPES the bundle.
    /// The old predicate returned false for the whole `@loader_path/` family, so
    /// both the rewrite loop and the post-relink verify loop skipped them,
    /// `prepare_binary_tree` reported success, and dyld then refused the binary
    /// with `Library not loaded: @loader_path/../../../../opt/tidy-html5/…`.
    #[test]
    fn an_escaping_loader_path_is_not_mistaken_for_in_tree() {
        let root = Path::new("/cache/php-7.4.33");
        let bin = Path::new("/cache/php-7.4.33/bin/php");
        // Real strings, read off the bottle (docs/archive/PLAN-php-74-support.md §5.2).
        for dep in [
            "@loader_path/../../../../opt/tidy-html5/lib/libtidy.58.dylib",
            "@loader_path/../../../../opt/openssl@3/lib/libssl.3.dylib",
            "@loader_path/../../../../opt/icu4c@74/lib/libicuuc.74.dylib",
        ] {
            assert!(
                MacosBinaryProvider::needs_tree_relink(root, bin, dep),
                "escapes the bundle but was treated as in-tree: {dep}"
            );
        }
        // Climbing past the filesystem root and back down does not sneak in.
        assert!(MacosBinaryProvider::needs_tree_relink(
            root,
            bin,
            "@loader_path/../../../../../../../../../../cache/php-7.4.33-evil/lib/libx.dylib"
        ));
        // `@executable_path` is unresolvable from the tree — the loading
        // executable is whoever spawns — so it always gets rewritten.
        assert!(MacosBinaryProvider::needs_tree_relink(
            root,
            bin,
            "@executable_path/../lib/libssl.3.dylib"
        ));
    }

    #[test]
    fn is_mach_o_detects_magic_not_extension() {
        let dir = std::env::temp_dir().join("rexenv-macho-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // 64-bit little-endian Mach-O magic as written on disk.
        std::fs::write(dir.join("real"), [0xcf, 0xfa, 0xed, 0xfe, 0, 0]).unwrap();
        assert!(MacosBinaryProvider::is_mach_o(&dir.join("real")));
        std::fs::write(dir.join("fake.dylib"), b"#!/bin/sh\n").unwrap();
        assert!(!MacosBinaryProvider::is_mach_o(&dir.join("fake.dylib")));
        std::fs::write(dir.join("tiny"), [0xcf]).unwrap();
        assert!(!MacosBinaryProvider::is_mach_o(&dir.join("tiny")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn spawn_logged_captures_output() {
        let path = std::env::temp_dir().join("rexenv-spawnlog-test.log");
        let _ = std::fs::remove_file(&path);
        let mut child = MacosSupervisor
            .spawn_logged(
                Path::new("/bin/echo"),
                &["rexenv-log-ok".to_string()],
                &path,
            )
            .unwrap();
        let _ = child.wait();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("rexenv-log-ok"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn set_private_sets_owner_only_mode() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join("rexenv-perm-test");
        std::fs::write(&path, b"secret").unwrap();
        MacosPermissions.set_private(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn shell_run_returns_stdout_on_success() {
        let out = MacosShell.run("echo", &["hello".to_string()]).unwrap();
        assert_eq!(out.trim(), "hello");
    }

    #[test]
    fn shell_run_errors_with_status_and_stderr_on_failure() {
        // A non-zero exit surfaces as an error carrying the exit status + stderr —
        // not a silent Ok("") (L4).
        let err = MacosShell
            .run("sh", &["-c".to_string(), "echo boom >&2; exit 3".to_string()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("boom"), "error carries stderr: {err}");
        assert!(err.contains('3'), "error carries the exit status: {err}");
    }

    #[test]
    fn process_running_distinguishes_live_from_gone() {
        let mut child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id();
        assert!(process_running(pid), "a live sleep should read as running");
        let _ = std::process::Command::new("kill").args(["-9", &pid.to_string()]).status();
        child.wait().unwrap(); // reap the zombie so the pid is fully gone
        assert!(!process_running(pid), "a reaped process should read as not running");
    }

    #[test]
    fn stop_pid_terminates_a_normal_process_via_sigterm() {
        // `sleep` takes the default SIGTERM action, so stop returns quickly (well
        // inside the grace window) without ever needing SIGKILL.
        let mut child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id();
        stop_pid(pid, STOP_GRACE_TRIES, STOP_POLL_INTERVAL).unwrap();
        child.wait().unwrap();
        assert!(!process_running(pid));
    }

    #[test]
    fn stop_pid_sigkills_a_sigterm_ignoring_process() {
        // A process that traps (ignores) SIGTERM must still be stopped by the SIGKILL
        // fallback, so a caller's wait() can't block forever (L3). Small grace params
        // keep the test fast (~150ms).
        let mut child = std::process::Command::new("sh")
            .args(["-c", r#"trap "" TERM; sleep 5"#])
            .spawn()
            .unwrap();
        let pid = child.id();
        stop_pid(pid, 3, std::time::Duration::from_millis(50)).unwrap();
        child.wait().unwrap();
        assert!(!process_running(pid), "SIGKILL fallback should have stopped it");
    }
}

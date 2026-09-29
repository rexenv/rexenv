//! Linux implementations — the port in progress (`docs/PLAN-linux-port.md`), Ubuntu first.
//!
//! **No `todo!()` here, by rule (ledger #595, widened to Linux).** A stub that can return an
//! error returns `Error::Unported`, so a half-ported build fails that ONE feature as an
//! ordinary error the UI shows; one whose trait method cannot return an error panics through
//! `unported!`. `core/` does not change.
//!
//! What Linux has that the other two do not: systemd (a supervisor with `Restart=always`, per
//! user and system-wide), polkit's `pkexec` (the desktop's own privilege prompt), `/proc`
//! (process identity without `lsof`), and php-fpm itself — so the pool model is macOS's.
//! The pure halves (`proc_table`, `resolved`, `units`, `desktop`, `trust`) are text, tested on
//! the macOS host too (`platform/mod.rs` includes them under `cfg(test)`).
#![allow(dead_code)]

use crate::error::{Error, Result};
use crate::platform::traits::*;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::Duration;

pub mod app_bundle;
mod app_bundle_rules;
mod desktop;
mod dnsroute;
mod libcompat;
pub mod parent_death_guard;
mod proc_table;
mod trust;
mod units;

use crate::platform::{APP_NAME, APP_ORG, APP_QUALIFIER};

/// The file the user installed and runs — `$APPIMAGE` when this is an AppImage (the process's
/// own exe is then a file inside a FUSE mount that is gone at the next launch), the exe itself
/// otherwise (`/usr/bin/rexenv` from the deb, a `target/` binary in development). Every unit
/// and autostart entry records THIS path.
fn installed_exe() -> Result<PathBuf> {
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        let p = PathBuf::from(appimage);
        if p.is_file() {
            return Ok(p);
        }
    }
    Ok(std::env::current_exe()?)
}

/// `LD_LIBRARY_PATH` for a service spawn when this distribution renamed a soname a bundled
/// binary asks for (`libcompat.rs`: `libaio.so.1` → `libaio.so.1t64` on Ubuntu 24.04+). The
/// symlinks live in `<app data>/lib-compat/`, made on first use; `None` on a system that has the
/// real names, so nothing changes where nothing is missing. Read once per process: the library
/// directory does not change under a running app, and a spawn is on the start-all path.
fn lib_compat_env() -> Option<(&'static str, String)> {
    use std::sync::OnceLock;
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    let dir = DIR.get_or_init(|| {
        let lib_dir = libcompat::multiarch_lib_dir(std::env::consts::ARCH);
        let present: Vec<String> = std::fs::read_dir(&lib_dir)
            .ok()?
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        let shims = libcompat::shims_for(&lib_dir, &present);
        if shims.is_empty() {
            return None;
        }
        let compat = LinuxPaths.app_data_dir().ok()?.join("lib-compat");
        std::fs::create_dir_all(&compat).ok()?;
        for shim in shims {
            let link = compat.join(&shim.wanted);
            if std::fs::read_link(&link).ok().as_deref() != Some(shim.target.as_path()) {
                let _ = std::fs::remove_file(&link);
                if let Err(e) = std::os::unix::fs::symlink(&shim.target, &link) {
                    log::warn!("lib-compat: could not link {} -> {}: {e}", link.display(), shim.target.display());
                    return None;
                }
            }
            log::info!("lib-compat: {} -> {}", link.display(), shim.target.display());
        }
        Some(compat)
    })
    .as_ref()?;
    let existing = std::env::var("LD_LIBRARY_PATH").ok();
    Some(("LD_LIBRARY_PATH", libcompat::library_path(dir, existing.as_deref())))
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| Error::Other("HOME is not set".into()))
}

fn config_home() -> Result<PathBuf> {
    if let Some(x) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(x));
    }
    Ok(home_dir()?.join(".config"))
}

fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(bin)).find(|p| p.is_file())
}

/// Run a command and turn a non-zero exit into an error carrying stderr.
fn run_ok(program: &str, args: &[String]) -> Result<String> {
    let out = crate::platform::command(program).args(args).output()?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stderr = stderr.trim();
        Err(Error::Other(format!(
            "`{program} {}` failed ({}): {}",
            args.join(" "),
            out.status,
            if stderr.is_empty() { "(no stderr)" } else { stderr }
        )))
    }
}

/// Start a desktop program detached: no inherited stdio, and a reaper thread so the child
/// never sits as a zombie for the app's life (`Child::drop` neither kills nor waits).
fn spawn_detached(program: &Path, args: &[String]) -> Result<()> {
    use std::process::Stdio;
    let mut child = crate::platform::command(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    std::thread::Builder::new()
        .name("detached-reaper".into())
        .spawn(move || {
            let _ = child.wait();
        })?;
    Ok(())
}

// ── Paths ───────────────────────────────────────────────────────────────────

pub struct LinuxPaths;
impl Paths for LinuxPaths {
    /// `~/.local/share/rexenv` — `directories` drops the qualifier and org on Linux, the XDG
    /// convention; ONE namespace fact shared with the other OSes (`platform/mod.rs`).
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
    /// The same place as macOS: on every distro `/usr/local/bin` is on the default `PATH`
    /// (`~/.local/bin` is only on GNOME's, and only once it exists at login).
    fn cli_symlink_path(&self) -> Result<PathBuf> {
        Ok(PathBuf::from("/usr/local/bin/rex"))
    }
}

// ── DNS route (D-L2) ────────────────────────────────────────────────────────

/// A marker per TLD under `/etc/rexenv/dns.d`, applied to the dummy link `rexenv0` by a root
/// unit (`dnsroute.rs` — why not a resolved drop-in is measured there).
pub struct LinuxDns;
impl DnsManager for LinuxDns {
    fn route_label(&self, tld: &str) -> String {
        dnsroute::marker_path(tld).display().to_string()
    }
    fn route_contents(&self, port: u16) -> String {
        dnsroute::signature(port)
    }
    /// Ours only while the link is LIVE too (`dnsroute::live_given`, ledger #741): a route whose
    /// marker is in place but that resolved no longer routes reads as not installed, so the app
    /// offers its setup step and the unit re-applies. macOS and Windows need no such check — the
    /// resolver file and the NRPT rule ARE the live state; nothing else can quietly lose them.
    fn route_owner(&self, tld: &str, port: u16) -> ResolverOwner {
        let marker = dnsroute::owner_of(tld, port);
        if marker != ResolverOwner::Ours {
            return marker;
        }
        let status = which("resolvectl").map(|r| {
            crate::platform::command(r)
                .args(dnsroute::LINK_STATUS_ARGS)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                .ok_or(())
        });
        dnsroute::live_given(marker, status.as_ref().map(|r| r.as_deref().map_err(|_| ())), tld, port)
    }
    fn our_route_tlds(&self, port: u16) -> Vec<String> {
        dnsroute::our_tlds(port)
    }
    fn foreign_route_tlds(&self, port: u16) -> Vec<String> {
        dnsroute::foreign_tlds(port)
    }
    fn install_command(&self, tld: &str, port: u16) -> String {
        dnsroute::install_command(tld, port)
    }
    fn uninstall_command(&self, tlds: &[String]) -> String {
        dnsroute::uninstall_command(tlds)
    }
    fn restore_command(&self, restores: &[(String, PathBuf)]) -> String {
        dnsroute::restore_command(restores)
    }
}

// ── CA trust (D-L3) ─────────────────────────────────────────────────────────

pub struct LinuxCertTrust;

impl LinuxCertTrust {
    fn certutil() -> Result<PathBuf> {
        which("certutil").ok_or_else(|| Error::Other(trust::CERTUTIL_MISSING.into()))
    }
    /// The PEM the NSS database at `db` holds under our nickname, if any.
    fn nss_current_in(db: &Path) -> Option<String> {
        let certutil = which("certutil")?;
        let out = crate::platform::command(certutil).args(trust::nss_show_args_for(db)).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }
    /// The PEM the user's primary database (`~/.pki/nssdb`) holds under our nickname.
    fn nss_current(home: &Path) -> Option<String> {
        Self::nss_current_in(&trust::nss_db_dir(home))
    }
}

impl CertTrustManager for LinuxCertTrust {
    /// Both stores, the browsers' first (no prompt — every NSS database a browser here reads,
    /// the snap Chromium's included), then the system's (one `pkexec`).
    fn trust_ca(&self, ca_cert_path: &Path) -> Result<()> {
        let home = home_dir()?;
        let certutil = Self::certutil()?;
        for db in trust::nss_db_dirs(&home) {
            if !db.join("cert9.db").exists() {
                std::fs::create_dir_all(&db)?;
                run_ok(&certutil.display().to_string(), &trust::nss_create_args_for(&db))?;
            }
            // A stale entry under the nickname would make `-A` a no-op that keeps the OLD root.
            if Self::nss_current_in(&db).is_some() {
                let _ = run_ok(&certutil.display().to_string(), &trust::nss_delete_args_for(&db));
            }
            run_ok(&certutil.display().to_string(), &trust::nss_add_args_for(&db, ca_cert_path))?;
        }
        // The root leg only when the store lacks this PEM — setup may already have run it in
        // its one batched step, and a re-setup must not ask for a store that is already right.
        match self.root_trust_command(ca_cert_path) {
            Some(cmd) => {
                LinuxPrivileges.run_privileged(
                    &cmd,
                    &PromptReason::new("add its local certificate authority to this computer's trust store, so curl and PHP accept https://*.rex"),
                )?;
            }
            None => log::info!("trust: the system store already holds this CA — no root step"),
        }
        Ok(())
    }
    fn root_trust_command(&self, ca_cert_path: &Path) -> Option<String> {
        let ours = std::fs::read_to_string(ca_cert_path).ok()?;
        let system = std::fs::read_to_string(Path::new(trust::SYSTEM_CERT_DIR).join(trust::SYSTEM_CERT_NAME)).ok();
        trust::system_store_needs(system.as_deref(), &ours).then(|| trust::system_trust_command(ca_cert_path))
    }
    fn untrust_ca(&self, _ca_cert_path: &Path) -> Result<()> {
        let home = home_dir()?;
        if let Ok(certutil) = Self::certutil() {
            for db in trust::nss_db_dirs(&home) {
                if Self::nss_current_in(&db).is_some() {
                    run_ok(&certutil.display().to_string(), &trust::nss_delete_args_for(&db))?;
                }
            }
        }
        // Only when teardown did not already take it in its batched step.
        if let Some(cmd) = self.root_untrust_command() {
            LinuxPrivileges.run_privileged(
                &cmd,
                &PromptReason::new("remove its local certificate authority from this computer's trust store"),
            )?;
        }
        Ok(())
    }
    fn root_untrust_command(&self) -> Option<String> {
        Path::new(trust::SYSTEM_CERT_DIR).join(trust::SYSTEM_CERT_NAME).exists().then(trust::system_untrust_command)
    }
    /// One nickname holds one certificate per database: a different one there is a stale root.
    fn untrust_stale(&self, current_ca: &Path) -> Result<usize> {
        let home = home_dir()?;
        let ours = std::fs::read_to_string(current_ca)?;
        let mut removed = 0;
        for db in trust::nss_db_dirs(&home) {
            let Some(held) = Self::nss_current_in(&db) else { continue };
            if trust::same_pem(&held, &ours) {
                continue;
            }
            run_ok(&Self::certutil()?.display().to_string(), &trust::nss_delete_args_for(&db))?;
            removed += 1;
        }
        Ok(removed)
    }
    /// BOTH stores hold the CURRENT CA — the browsers' NSS database AND the system store's copy
    /// (`/usr/local/share/ca-certificates/rexenv-local-ca.crt`, what `curl`, PHP and WP-CLI
    /// read). This asked NSS alone until 27 Sep 2026, when the Dell's WSL Ubuntu carried a
    /// stale CA in the system store beside the right one in NSS: `rex status` said
    /// "CA trusted" while every `curl https://acme.rex` died with `certificate signature
    /// failure` — the guard-covers-claimed-surface shape (a two-store claim, one store
    /// checked). A missing or different system copy is "not trusted", which re-offers the
    /// one-prompt step that rewrites both.
    fn is_trusted(&self, ca_cert_path: &Path) -> bool {
        let Ok(home) = home_dir() else { return false };
        let Some(held) = Self::nss_current(&home) else { return false };
        let Ok(ours) = std::fs::read_to_string(ca_cert_path) else { return false };
        if !trust::same_pem(&held, &ours) {
            return false;
        }
        let system = Path::new(trust::SYSTEM_CERT_DIR).join(trust::SYSTEM_CERT_NAME);
        std::fs::read_to_string(system).map(|sys| trust::same_pem(&sys, &ours)).unwrap_or(false)
    }
    fn firefox_profiles_root(&self) -> Option<PathBuf> {
        let home = home_dir().ok()?;
        trust::firefox_roots(&home).into_iter().find(|r| r.join("profiles.ini").is_file())
    }
}

// ── Privileges: pkexec ──────────────────────────────────────────────────────

pub struct LinuxPrivileges;

impl LinuxPrivileges {
    /// `pkexec` exit codes: 126 = the user dismissed the dialog or failed to authenticate,
    /// 127 = polkit refused (no agent, not an admin). Both read as recoverable sentences.
    fn error_message(code: Option<i32>, stderr: &str) -> String {
        match code {
            Some(126) => "Administrator permission was cancelled — this step needs it. Try again and approve the prompt.".into(),
            Some(127) => format!(
                "This account is not allowed to administer the computer (polkit refused), or no authentication agent is running. {}",
                stderr.trim()
            ),
            _ => format!("privileged operation failed: {}", stderr.trim()),
        }
    }
}

impl PrivilegeManager for LinuxPrivileges {
    fn run_privileged(&self, script: &str, _reason: &PromptReason) -> Result<String> {
        static ONE_PROMPT: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _one = ONE_PROMPT.lock().unwrap_or_else(|p| p.into_inner());
        let pkexec = which("pkexec").ok_or_else(|| {
            Error::Other("pkexec (polkit) is not installed — rexenv needs it to ask for administrator permission.".into())
        })?;
        // The reason is on screen already (the consent card). The dialog's own words come from
        // the polkit action the deb installs, which names `/usr/bin/rexenv`: when both exist the
        // step runs through rexenv itself (`--privileged-step`, `run_step` below) and the dialog
        // says what rexenv is doing; otherwise (an AppImage, a dev build) it is `/bin/sh` and
        // polkit's generic sentence.
        // Whatever the step wrote reaches the disk before the prompt returns (`durable`).
        let script = &crate::platform::durable::flushed_script(script);
        let mut cmd = crate::platform::command(pkexec);
        match privileged_step_program() {
            Some(exe) => cmd.arg(exe).arg(PRIVILEGED_STEP_FLAG).arg(script),
            None => cmd.args(["/bin/sh", "-c", script]),
        };
        let out = cmd.output()?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
        } else {
            Err(Error::Other(Self::error_message(out.status.code(), &String::from_utf8_lossy(&out.stderr))))
        }
    }
}

/// The flag the privileged step is dispatched on (`main.rs`, before anything else).
pub(crate) const PRIVILEGED_STEP_FLAG: &str = "--privileged-step";
/// Where the deb installs the polkit action, and the program path that action annotates.
const POLKIT_ACTION_PATH: &str = "/usr/share/polkit-1/actions/dev.rexenv.rexenv.policy";
const POLKIT_ANNOTATED_EXE: &str = "/usr/bin/rexenv";

/// `/usr/bin/rexenv` when THIS process is that file and the action file is installed — the
/// only pair polkit would show rexenv's own sentence for. Anything else falls back to `/bin/sh`.
fn privileged_step_program() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    (exe == Path::new(POLKIT_ANNOTATED_EXE) && Path::new(POLKIT_ACTION_PATH).is_file()).then_some(exe)
}

/// The privileged step itself, run as root by pkexec: `/bin/sh -c <script>` with stdio
/// inherited, so the caller reads exactly what the shell form would have produced. Only rexenv's
/// own scripts reach here (through `run_privileged`), and polkit asked for an administrator's
/// password first. `None` when argv is not a step.
pub(crate) fn run_step(argv: &[String]) -> Option<i32> {
    let i = argv.iter().position(|a| a == PRIVILEGED_STEP_FLAG)?;
    let script = argv.get(i + 1)?;
    let status = std::process::Command::new("/bin/sh").arg("-c").arg(script).status();
    Some(match status {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("rexenv privileged step: cannot run /bin/sh: {e}");
            127
        }
    })
}

// ── Process supervision ─────────────────────────────────────────────────────

const STOP_GRACE_TRIES: u32 = 30;
const STOP_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// A process's start time in clock ticks since boot (`/proc/<pid>/stat` field 22) — the
/// identity the tunnel guard checks beyond the pid: a recycled pid wears a different one.
/// `None` when the pid is gone.
pub fn process_start_token(pid: u32) -> Option<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    proc_table::start_time_of(&stat)
}

fn proc_stat(pid: u32) -> Option<proc_table::StatFields> {
    proc_table::parse_stat(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

/// Live and not a zombie (a zombie has exited and only awaits its parent's reap).
fn process_running(pid: u32) -> bool {
    matches!(proc_stat(pid), Some(s) if s.state != 'Z' && s.state != 'X')
}

fn stop_pid(pid: u32, grace_tries: u32, interval: Duration) -> Result<()> {
    let _ = crate::platform::command("kill").arg(pid.to_string()).status();
    for _ in 0..grace_tries {
        if !process_running(pid) {
            return Ok(());
        }
        std::thread::sleep(interval);
    }
    let _ = crate::platform::command("kill").args(["-9", &pid.to_string()]).status();
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

/// Every numeric entry of `/proc`.
fn all_pids() -> Vec<u32> {
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse().ok())
        .collect()
}

fn ss_rows(args: &[&str]) -> Option<Vec<proc_table::SocketRow>> {
    let out = crate::platform::command("ss").args(args).output().ok()?;
    Some(proc_table::parse_ss(&String::from_utf8_lossy(&out.stdout)))
}

pub struct LinuxSupervisor;
impl ProcessSupervisor for LinuxSupervisor {
    fn terminate_child(&self, child: &mut Child) {
        let _ = crate::platform::command("kill").arg(child.id().to_string()).status();
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
        matches!(crate::platform::command("kill").args(["-HUP", &pid.to_string()]).status(), Ok(s) if s.success())
    }
    fn pid_alive(&self, pid: u32) -> bool {
        process_running(pid)
    }
    fn spawn(&self, program: &Path, args: &[String]) -> Result<Child> {
        let mut cmd = crate::platform::command(program);
        cmd.args(args);
        if let Some((k, v)) = lib_compat_env() {
            cmd.env(k, v);
        }
        Ok(cmd.spawn()?)
    }
    /// Our own binary re-executed in guard mode (`parent_death_guard`), the macOS shape: a
    /// pidfd watcher, because `PR_SET_PDEATHSIG` binds to the spawning THREAD and a retired
    /// pool thread would end the share with the app alive.
    fn guard_child_against_our_death(&self, child: u32, domain: &str) -> Result<()> {
        let exe = std::env::current_exe()?;
        let me = std::process::id();
        let start = process_start_token(me)
            .ok_or_else(|| Error::Other(format!("could not read this process's start time (pid {me})")))?;
        let args = crate::core::tunnels::guard_argv(me, child, domain, &start);
        let mut guard = crate::platform::command(exe)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        // Reaped by a parked thread, as on macOS: a dropped handle would leave a zombie per share.
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
    fn spawn_logged_env(&self, program: &Path, args: &[String], log_path: &Path, env: &[(String, String)]) -> Result<Child> {
        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let out = std::fs::OpenOptions::new().create(true).append(true).open(log_path)?;
        let err = out.try_clone()?;
        let mut cmd = crate::platform::command(program);
        cmd.args(args).envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str()))).stdout(out).stderr(err);
        if let Some((k, v)) = lib_compat_env() {
            cmd.env(k, v);
        }
        Ok(cmd.spawn()?)
    }
    fn stop(&self, pid: u32) -> Result<()> {
        stop_pid(pid, STOP_GRACE_TRIES, STOP_POLL_INTERVAL)
    }
    /// `/proc/<pid>/comm` (15 bytes, what `pgrep -x` compares) or the exe's file name — a
    /// title-rewriting master (`php-fpm: master process`) keeps `comm` = `php-fpm`.
    fn pids_named(&self, name: &str) -> Vec<u32> {
        all_pids()
            .into_iter()
            .filter(|&pid| {
                let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
                comm.trim() == name
                    || self.pid_exe(pid).and_then(|p| p.file_name().map(|f| f == name)).unwrap_or(false)
            })
            .collect()
    }
    fn pid_command(&self, pid: u32) -> Option<String> {
        proc_table::cmdline_to_string(&std::fs::read(format!("/proc/{pid}/cmdline")).ok()?)
    }
    /// `readlink /proc/<pid>/exe` — the kernel's answer, immune to a title rewrite. `EACCES`
    /// for another user's process (the root edge) is `None`: cannot tell, never "not ours".
    fn pid_exe(&self, pid: u32) -> Option<PathBuf> {
        let p = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
        let s = p.to_string_lossy();
        Some(PathBuf::from(s.strip_suffix(" (deleted)").unwrap_or(&s)))
    }
    fn spawn_streamed(&self, program: &Path, args: &[String], cwd: &Path, env: &[(String, String)]) -> Result<Child> {
        use std::os::unix::process::CommandExt;
        use std::process::Stdio;
        let mut cmd = crate::platform::command(program);
        cmd.args(args)
            .current_dir(cwd)
            .env_clear()
            .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        Ok(cmd.spawn()?)
    }
    fn stop_group(&self, pgid: u32) -> Result<()> {
        // `pgrep`/`pkill -g 0` means "MY OWN process group" — the app's — so a caller
        // that reaches here with 0 (or 1, init) is refused before any signal is sent.
        // Found the day procps's `kill -KILL -<pid>` killed the whole `cargo test`
        // group in the release pipeline (27 Sep 2026, `dist_archive`'s test helper);
        // this is the same family a step away, and a guard is cheaper than a repeat.
        if pgid <= 1 {
            return Err(Error::Other(format!("refusing to signal process group {pgid} — that would be rexenv's own")));
        }
        fn group_signal(sig: &str, pgid: u32) {
            let _ = crate::platform::command("pkill").args([&format!("-{sig}"), "-g", &pgid.to_string()]).status();
        }
        // `pgrep -g` counts ZOMBIES: a leader that died on the TERM but whose parent has not
        // reaped it yet (the caller still holds the `Child`, reading its pipe) kept the group
        // "alive" for the whole grace loop, so a cancel that should return at once took the
        // full 3 s + 3 s before `Err` — measured in the Ubuntu 22.04 image, 27 Sep 2026
        // (`captured_cap_kills_the_whole_group…` at 6.8 s, the Mac at under 1 s). /proc is
        // asked directly: a member of the group that is not Z/X is what "alive" means.
        fn group_alive(pgid: u32) -> bool {
            all_pids().into_iter().any(|pid| matches!(proc_stat(pid), Some(s) if s.pgrp == pgid && s.state != 'Z' && s.state != 'X'))
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
    fn owned_listeners(&self, port: u16, owner_marker: &str) -> Vec<u32> {
        let Some(rows) = ss_rows(&["-Hltnp"]) else { return Vec::new() };
        let mut pids: Vec<u32> = rows
            .into_iter()
            .filter(|r| r.local_port == port)
            .flat_map(|r| r.users.into_iter().map(|(_, pid)| pid))
            .filter(|&pid| self.pid_command(pid).is_some_and(|c| c.contains(owner_marker)))
            .collect();
        pids.sort();
        pids.dedup();
        pids
    }
    fn owned_master(&self, port: u16, owner_marker: &str) -> Option<u32> {
        let pids = self.owned_listeners(port, owner_marker);
        if pids.is_empty() {
            return None;
        }
        let pairs: Vec<(u32, u32)> = pids.iter().filter_map(|&pid| Some((pid, proc_stat(pid)?.ppid))).collect();
        super::traits::select_master(&pairs).or_else(|| pids.into_iter().min())
    }
    fn resource_usage(&self, pid: u32) -> Option<(f32, u64)> {
        let out = crate::platform::command("ps").args(["-o", "%cpu=,rss=", "-p", &pid.to_string()]).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let mut it = text.split_whitespace();
        let cpu: f32 = it.next()?.parse().ok()?;
        let rss_kb: u64 = it.next()?.parse().ok()?;
        Some((cpu, rss_kb / 1024))
    }
    fn owned_pids(&self, marker: &str) -> Vec<u32> {
        all_pids().into_iter().filter(|&pid| self.pid_command(pid).is_some_and(|c| c.contains(marker))).collect()
    }
    /// `ss` names the pid only for this user's sockets; a root holder is a row with no pid and
    /// is left to the trial bind, as on macOS.
    fn port_holders(&self, port: u16, udp: bool) -> Option<Vec<u32>> {
        let rows = ss_rows(&[if udp { "-Hlunp" } else { "-Hltnp" }])?;
        let mut pids: Vec<u32> =
            rows.into_iter().filter(|r| r.local_port == port).flat_map(|r| r.users.into_iter().map(|(_, p)| p)).collect();
        pids.sort();
        pids.dedup();
        Some(pids)
    }
    fn established_on(&self, port: u16) -> Option<usize> {
        let rows = ss_rows(&["-Htn", "state", "established"])?;
        Some(rows.iter().filter(|r| r.local_port == port).count())
    }
    fn port_conflict_help(&self, port: u16, udp: bool) -> PortConflictHelp {
        let rows = ss_rows(&[if udp { "-Hlunp" } else { "-Hltnp" }]).unwrap_or_default();
        let row = rows.into_iter().find(|r| r.local_port == port);
        let holder = row.and_then(|r| {
            // Masters fork first: the lowest pid of the set.
            let (name, pid) = r.users.iter().min_by_key(|(_, pid)| *pid)?.clone();
            let exe = self.pid_exe(pid).map(|p| p.display().to_string()).unwrap_or_default();
            Some(if exe.is_empty() { format!("{name} (pid {pid})") } else { format!("{name} (pid {pid}, {exe})") })
        });
        PortConflictHelp { holder, app: None, free_command: Some(proc_table::free_port_command(port, udp)) }
    }
}

// ── Autostart: an XDG autostart entry ───────────────────────────────────────

pub struct LinuxAutostart;
impl LinuxAutostart {
    fn entry_path() -> Result<PathBuf> {
        Ok(config_home()?.join("autostart").join("rexenv.desktop"))
    }
}
impl AutostartManager for LinuxAutostart {
    fn enable(&self) -> Result<()> {
        let entry = Self::entry_path()?;
        if let Some(parent) = entry.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::platform::durable::write_durable(
            &entry,
            desktop::autostart_contents(&installed_exe()?, crate::HIDDEN_LAUNCH_FLAG).as_bytes(),
        )?;
        Ok(())
    }
    fn disable(&self) -> Result<()> {
        let entry = Self::entry_path()?;
        if entry.exists() {
            std::fs::remove_file(&entry)?;
        }
        Ok(())
    }
    fn is_enabled(&self) -> Result<bool> {
        Ok(Self::entry_path()?.exists())
    }
    /// Re-point an installed launch; keep a development launch's entry on the recorded
    /// binary while that binary exists (the macOS "not an .app bundle" rule).
    fn refresh(&self) -> Result<()> {
        let entry = Self::entry_path()?;
        let program = installed_exe()?;
        let want = desktop::autostart_contents(&program, crate::HIDDEN_LAUNCH_FLAG);
        let have = std::fs::read_to_string(&entry).unwrap_or_default();
        if have == want {
            return Ok(());
        }
        let installed = program.starts_with("/usr") || program.starts_with("/opt") || std::env::var_os("APPIMAGE").is_some();
        if !installed {
            if let Some(recorded) = desktop::autostart_program(&have).filter(|p| p.exists()) {
                log::info!("autostart: this launch is {} (a development build) — keeping the entry on {}", program.display(), recorded.display());
                return Ok(());
            }
        }
        self.enable()
    }
}

// ── The DNS agent: a systemd USER unit ──────────────────────────────────────

pub struct LinuxDnsAgent;
impl LinuxDnsAgent {
    fn systemctl_user(args: &[&str]) -> Result<()> {
        let st = crate::platform::command("systemctl").arg("--user").args(args).status()?;
        if st.success() {
            Ok(())
        } else {
            Err(Error::Other(format!("systemctl --user {} failed (exit {:?})", args.join(" "), st.code())))
        }
    }
}
impl DnsAgentManager for LinuxDnsAgent {
    fn is_installed(&self) -> bool {
        self.definition_path().map(|p| p.exists()).unwrap_or(false)
    }
    fn definition_path(&self) -> Result<PathBuf> {
        Ok(config_home()?.join("systemd/user").join(units::DNS_UNIT))
    }
    fn definition_contents(&self, exe: &Path, log: &Path) -> String {
        units::dns_unit_contents(exe, log)
    }
    fn install(&self, exe: &Path, log: &Path) -> Result<()> {
        let unit = self.definition_path()?;
        if let Some(parent) = unit.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // The exe the app was started from, unless that is an AppImage's mount.
        let exe = if std::env::var_os("APPIMAGE").is_some() { installed_exe()? } else { exe.to_path_buf() };
        let contents = self.definition_contents(&exe, log);
        let unchanged = std::fs::read_to_string(&unit).map(|c| c == contents).unwrap_or(false);
        if !unchanged {
            crate::platform::durable::write_durable(&unit, contents.as_bytes())?;
        }
        Self::systemctl_user(&["daemon-reload"])?;
        if unchanged {
            // Already defined; make sure it is up (a fresh login, a stopped unit).
            return Self::systemctl_user(&["enable", "--now", units::DNS_UNIT]);
        }
        Self::systemctl_user(&["enable", units::DNS_UNIT])?;
        Self::systemctl_user(&["restart", units::DNS_UNIT])
    }
    fn kickstart(&self) -> Result<()> {
        Self::systemctl_user(&["restart", units::DNS_UNIT])
            .or_else(|_| Self::systemctl_user(&["enable", "--now", units::DNS_UNIT]))
    }
    fn uninstall(&self) -> Result<()> {
        let unit = self.definition_path()?;
        if unit.exists() {
            let _ = Self::systemctl_user(&["disable", "--now", units::DNS_UNIT]);
            std::fs::remove_file(&unit)?;
            let _ = Self::systemctl_user(&["daemon-reload"]);
        }
        Ok(())
    }
}

// ── The edge: a systemd SYSTEM unit ─────────────────────────────────────────

pub struct LinuxEdge;
impl EdgeSupervisor for LinuxEdge {
    fn plist_path(&self) -> PathBuf {
        units::edge_unit_path()
    }
    fn wrapper_path(&self) -> PathBuf {
        units::edge_wrapper_path()
    }
    fn daemon_binary_path(&self) -> PathBuf {
        units::edge_binary_path()
    }
    fn is_installed(&self) -> bool {
        self.plist_path().exists()
    }
    /// `systemctl is-enabled` is readable without privilege; anything but a plain `disabled`
    /// counts as enabled (a wrong "disabled" diagnosis would mislead more than a generic one).
    fn is_enabled(&self) -> bool {
        match crate::platform::command("systemctl").args(["is-enabled", units::EDGE_UNIT]).output() {
            Ok(o) => String::from_utf8_lossy(&o.stdout).trim() != "disabled",
            Err(_) => true,
        }
    }
    fn plist_contents(&self, wrapper: &Path, start_log: &Path) -> String {
        units::edge_unit_contents(wrapper, start_log)
    }
    fn wrapper_contents(&self, caddy_bin: &Path, caddyfile: &Path, admin_sock: &Path, appdata: &Path) -> String {
        units::edge_wrapper_contents(caddy_bin, caddyfile, admin_sock, appdata)
    }
    fn install_command(&self, src_caddy: &Path, staged_wrapper: &Path, staged_plist: &Path) -> String {
        units::edge_install_command(src_caddy, staged_wrapper, staged_plist)
    }
    fn start_command(&self) -> String {
        units::edge_start_command()
    }
    fn stop_command(&self) -> String {
        units::edge_stop_command()
    }
    fn uninstall_command(&self) -> String {
        units::edge_uninstall_command()
    }
}

// ── Permissions ─────────────────────────────────────────────────────────────

pub struct LinuxPermissions;
impl PermissionManager for LinuxPermissions {
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
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
        let mut perms = f.metadata()?.permissions();
        if perms.mode() & 0o777 != 0o600 {
            perms.set_mode(0o600);
            f.set_permissions(perms)?;
        }
        f.write_all(contents)?;
        Ok(())
    }
}

// ── Shell and the desktop's apps ────────────────────────────────────────────

pub struct LinuxShell;

impl LinuxShell {
    fn installed(app: &desktop::DesktopApp) -> Option<PathBuf> {
        app.bins.iter().find_map(|b| which(b))
    }
    fn default_browser_id() -> Option<&'static str> {
        let out = crate::platform::command("xdg-settings").args(["get", "default-web-browser"]).output().ok()?;
        desktop::browser_for_desktop_id(&String::from_utf8_lossy(&out.stdout)).map(|b| b.id)
    }
}

impl ShellRunner for LinuxShell {
    fn interactive_shell(&self) -> String {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
    }
    fn run(&self, command: &str, args: &[String]) -> Result<String> {
        run_ok(command, args)
    }
    fn open(&self, target: &str) -> Result<()> {
        let xdg = which("xdg-open").ok_or_else(|| Error::Other("xdg-open is not installed (xdg-utils)".into()))?;
        spawn_detached(&xdg, &[target.to_string()])
    }
    /// The file manager's own D-Bus interface selects the item; a desktop without it gets the
    /// containing folder opened, which is honest about what happened (nothing is selected).
    fn reveal(&self, path: &str) -> Result<()> {
        if let Some(dbus) = which("dbus-send") {
            let uri = format!("file://{path}");
            let ok = crate::platform::command(dbus)
                .args([
                    "--session",
                    "--print-reply",
                    "--dest=org.freedesktop.FileManager1",
                    "/org/freedesktop/FileManager1",
                    "org.freedesktop.FileManager1.ShowItems",
                    &format!("array:string:{uri}"),
                    "string:",
                ])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if ok {
                return Ok(());
            }
        }
        let parent = Path::new(path).parent().map(|p| p.display().to_string()).unwrap_or_else(|| path.to_string());
        self.open(&parent)
    }
    fn detect_editors(&self) -> Vec<EditorApp> {
        desktop::EDITORS
            .iter()
            .filter(|a| Self::installed(a).is_some())
            .map(|a| EditorApp { id: a.id.into(), name: a.name.into(), icon: None })
            .collect()
    }
    fn open_in_editor(&self, editor_id: &str, path: &str) -> Result<()> {
        let app = desktop::EDITORS.iter().find(|a| a.id == editor_id).ok_or_else(|| Error::Other(format!("unknown editor: {editor_id}")))?;
        let bin = Self::installed(app).ok_or_else(|| Error::Other(format!("{} is not installed anymore", app.name)))?;
        spawn_detached(&bin, &[path.to_string()])
    }
    fn detect_browsers(&self) -> Vec<BrowserApp> {
        let default = Self::default_browser_id();
        desktop::BROWSERS
            .iter()
            .filter(|a| Self::installed(a).is_some())
            .map(|a| BrowserApp {
                id: a.id.into(),
                name: a.name.into(),
                icon: None,
                system_default: default == Some(a.id),
                supports_private: a.private_flag.is_some(),
            })
            .collect()
    }
    fn open_in_browser(&self, browser_id: &str, url: &str, private: bool) -> Result<()> {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(Error::Other(format!(
                "refusing to open {url} in a browser — only http:// and https:// URLs go to a chosen browser (paths go to the system handler)"
            )));
        }
        let app = desktop::BROWSERS.iter().find(|a| a.id == browser_id).ok_or_else(|| Error::Other(format!("unknown browser: {browser_id}")))?;
        let bin = Self::installed(app).ok_or_else(|| Error::Other(format!("{} is not installed anymore", app.name)))?;
        let mut args = Vec::new();
        if private {
            let flag = app.private_flag.ok_or_else(|| {
                Error::Other(format!(
                    "{} has no private-window command line — rexenv only offers private mode for browsers it can actually open one in",
                    app.name
                ))
            })?;
            args.push(flag.to_string());
        }
        args.push(url.to_string());
        spawn_detached(&bin, &args)
    }
    fn detect_terminals(&self) -> Vec<TerminalApp> {
        desktop::TERMINALS
            .iter()
            .filter(|t| t.bins.iter().any(|b| which(b).is_some()))
            .map(|t| TerminalApp { id: t.id.into(), name: t.name.into(), icon: None })
            .collect()
    }
    fn open_in_terminal(&self, terminal_id: &str, path: &Path) -> Result<()> {
        if !path.is_dir() {
            return Err(Error::Other(format!(
                "refusing to open a terminal at {} — only an existing directory is a working directory (a terminal handed a file would run it)",
                path.display()
            )));
        }
        let dir = path.to_str().ok_or_else(|| Error::Other(format!("path is not valid UTF-8: {}", path.display())))?;
        let term = desktop::TERMINALS.iter().find(|t| t.id == terminal_id).ok_or_else(|| Error::Other(format!("unknown terminal: {terminal_id}")))?;
        let bin = term.bins.iter().find_map(|b| which(b)).ok_or_else(|| Error::Other(format!("{} is not installed anymore", term.name)))?;
        spawn_detached(&bin, &desktop::terminal_args(term.cwd, dir))
    }
    fn login_shell_env(&self) -> Result<Vec<(String, String)>> {
        use std::io::Read;
        use std::process::Stdio;
        let shell = self.interactive_shell();
        let cmd = format!("printf '\\0{}\\0'; command env -0", ENV_MARKER);
        let mut child = crate::platform::command(&shell)
            .args(["-ilc", &cmd])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let mut stdout = child.stdout.take().expect("stdout piped above");
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
                    "your shell ({shell}) took more than 10s to start — a slow startup file? Fix the shell startup, then hit Re-detect."
                )));
            }
        };
        let _ = child.wait();
        let env = parse_shell_env_output(&raw);
        if env.iter().any(|(k, _)| k == "PATH") {
            Ok(env)
        } else {
            Err(Error::Other(format!(
                "couldn't read your shell environment ({shell} printed no PATH). Check the shell's startup files, then hit Re-detect."
            )))
        }
    }
    fn symlink_dir(&self, target: &Path, link: &Path) -> Result<()> {
        std::os::unix::fs::symlink(target, link)?;
        Ok(())
    }
    fn symlink_file(&self, target: &Path, link: &Path) -> Result<()> {
        std::os::unix::fs::symlink(target, link)?;
        Ok(())
    }
    fn remove_symlink(&self, link: &Path) -> Result<()> {
        let meta = std::fs::symlink_metadata(link)?;
        if !meta.file_type().is_symlink() {
            return Err(Error::Other(format!("{} is not a symlink — refusing to remove it here", link.display())));
        }
        std::fs::remove_file(link)?;
        Ok(())
    }
}

// ── Binaries ────────────────────────────────────────────────────────────────

pub struct LinuxBinaryProvider;
impl BinaryProvider for LinuxBinaryProvider {
    fn arch(&self) -> Arch {
        if cfg!(target_arch = "aarch64") {
            Arch::Arm64
        } else {
            Arch::X86_64
        }
    }
    /// No quarantine, no signature: an executable bit is the whole preparation. Every single
    /// binary rexenv pins for Linux is static (static-php, Caddy, Mailpit, cloudflared, the
    /// jirutka nginx) or carries its own `RUNPATH` (theseus PostgreSQL).
    fn prepare_binary(&self, path: &Path) -> Result<()> {
        LinuxPermissions.set_executable(path)
    }
    /// Homebrew-bottle trees (Redis, MariaDB, httpd, Xdebug) are not in Linux v1 (D-L8):
    /// `ships_on` refuses them before a download, so this is reached by no path today.
    fn prepare_binary_tree(&self, _root: &Path) -> Result<()> {
        Err(Error::Unported("linux bundle trees (patchelf relink)"))
    }
}

// ── The app bundle: `app_bundle.rs` (a `.deb` through dpkg, an AppImage by exchange — L7) ──
pub use app_bundle::LinuxAppBundle;

/// Local IPC on Linux: a unix-domain socket, exactly as macOS.
pub struct LinuxLocalIpc;
impl LocalIpc for LinuxLocalIpc {
    fn connect(&self, path: &Path, read_timeout: Option<Duration>) -> std::io::Result<Box<dyn std::io::Read + Send>> {
        let stream = std::os::unix::net::UnixStream::connect(path)?;
        let _ = stream.set_read_timeout(read_timeout);
        Ok(Box::new(stream))
    }
}

pub struct LinuxPlatform {
    paths: LinuxPaths,
    dns: LinuxDns,
    cert_trust: LinuxCertTrust,
    privileges: LinuxPrivileges,
    supervisor: LinuxSupervisor,
    autostart: LinuxAutostart,
    permissions: LinuxPermissions,
    shell: LinuxShell,
    binaries: LinuxBinaryProvider,
    edge: LinuxEdge,
    dns_agent: LinuxDnsAgent,
    app_bundle: LinuxAppBundle,
}

impl LinuxPlatform {
    pub fn new() -> Self {
        Self {
            paths: LinuxPaths,
            dns: LinuxDns,
            cert_trust: LinuxCertTrust,
            privileges: LinuxPrivileges,
            supervisor: LinuxSupervisor,
            autostart: LinuxAutostart,
            permissions: LinuxPermissions,
            shell: LinuxShell,
            binaries: LinuxBinaryProvider,
            edge: LinuxEdge,
            dns_agent: LinuxDnsAgent,
            app_bundle: LinuxAppBundle,
        }
    }
}

impl Default for LinuxPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl Platform for LinuxPlatform {
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

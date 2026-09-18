//! Windows implementations — the port in progress (`docs/PLAN-windows-port.md`).
//!
//! **No `todo!()` here, by rule (ledger #595).** A stub that can return an error
//! returns `Error::Unported`, so a half-ported build fails that ONE feature as an
//! ordinary error the UI shows while the rest of the app runs. A stub whose trait
//! method cannot return an error panics through `unported!`, with the same
//! wording, for the panic hook to record. `todo!()` did neither: a release build
//! has no console, so its message reached nobody, and a panic in a command left
//! the frontend waiting forever (plan §3a Q1). Filling these in is the port;
//! `core/` does not change.
#![allow(dead_code)]

use crate::error::{Error, Result};
use crate::platform::traits::*;
use std::path::{Path, PathBuf};
use std::process::Child;

mod acl;
mod af_unix;
mod agent_output;
mod app_catalog;
mod app_pipe;
mod app_pipe_rules;
mod app_registry;
mod autostart;
mod autostart_rules;
mod cert_rules;
mod cert_store;
mod elevation;
mod elevation_rules;
mod firefox_root;
mod handles;
mod ipc_rules;
mod junction;
mod junction_rules;
mod login_env;
mod logon_task;
mod nrpt_rules;
mod owner_only;
mod pe;
mod port_table;
mod process;
mod resolver_socket;
mod shell_rules;
mod stop_policy;
mod user_path;
mod user_path_rules;

pub(crate) use agent_output::send_output_to;
pub(crate) use app_pipe::{
    claim as claim_app_pipe, create_mcp as create_mcp_pipe, hand_off as hand_off_to_app_pipe, next_instance as next_app_pipe_instance, AppPipeClaim,
    HeldAppPipe,
};
pub(crate) use elevation::{run_ops_here, run_step};
pub(crate) use resolver_socket::bind_resolver_udp;

pub struct WindowsPaths;
impl Paths for WindowsPaths {
    /// `%LOCALAPPDATA%\rexenv\rexenv\data` — `directories` resolves the Local AppData
    /// known folder (`SHGetKnownFolderPath`, not an environment variable) and drops the
    /// qualifier on Windows. LOCAL, not roaming: this folder holds gigabytes of
    /// binaries and database datadirs, which must never sync to a roaming profile.
    fn app_data_dir(&self) -> Result<PathBuf> {
        let dirs = directories::ProjectDirs::from(
            crate::platform::APP_QUALIFIER,
            crate::platform::APP_ORG,
            crate::platform::APP_NAME,
        )
        .ok_or(Error::Other("cannot resolve the Local AppData folder".into()))?;
        Ok(dirs.data_local_dir().to_path_buf())
    }
    // Same layout under app data as macOS: one tree, one set of relative names.
    fn config_dir(&self) -> Result<PathBuf> {
        Ok(self.app_data_dir()?.join("config"))
    }
    fn log_dir(&self) -> Result<PathBuf> {
        Ok(self.app_data_dir()?.join("logs"))
    }
    fn bin_dir(&self) -> Result<PathBuf> {
        Ok(self.app_data_dir()?.join("bin"))
    }
    /// `rex` goes on the user's `Path` as a copy in rexenv's own folder (plan §5 W8 rulings Q3, Q4, ledger #634):
    /// `%LOCALAPPDATA%\rexenv\bin`, beside the app-data tree rather than inside it, so the folder on `Path`
    /// holds `rex.exe` and nothing else — never the downloaded binaries in `bin_dir`.
    fn cli_install(&self) -> Result<crate::platform::traits::CliInstall> {
        let base = directories::BaseDirs::new().ok_or(Error::Other("cannot resolve the Local AppData folder".into()))?;
        Ok(crate::platform::traits::CliInstall::CopyOnUserPath(
            base.data_local_dir().join(crate::platform::APP_ORG).join("bin"),
        ))
    }
    fn hosts_file(&self) -> PathBuf {
        PathBuf::from(r"C:\Windows\System32\drivers\etc\hosts")
    }
}

pub struct WindowsDns;
impl DnsManager for WindowsDns {
    /// An NRPT rule, not a file (plan §3 D2, ledger #618).
    fn route_label(&self, tld: &str) -> String {
        format!("the NRPT rule for .{tld}")
    }
    fn route_contents(&self, _port: u16) -> String {
        format!(
            "an NRPT rule sending the TLD to {} (rexenv's resolver), comment \"{}\"",
            nrpt_rules::OUR_SERVER,
            nrpt_rules::OUR_COMMENT
        )
    }
    /// From the rules in the registry, read without elevation. NRPT has no port: the resolver answers
    /// on `RESOLVER_PORT`, 53, so `port` does not enter the signature.
    fn route_owner(&self, tld: &str, _port: u16) -> ResolverOwner {
        nrpt_rules::owner(&process::read_nrpt_rules(), tld)
    }
    fn our_route_tlds(&self, _port: u16) -> Vec<String> {
        nrpt_rules::our_tlds(&process::read_nrpt_rules())
    }
    fn foreign_route_tlds(&self, _port: u16) -> Vec<String> {
        nrpt_rules::foreign_tlds(&process::read_nrpt_rules())
    }
    /// rexenv OPS, not PowerShell (ledger #619): the elevated step turns them into the script itself.
    fn install_command(&self, tld: &str, _port: u16) -> String {
        nrpt_rules::install_op(tld)
    }
    fn uninstall_command(&self, tlds: &[String]) -> String {
        nrpt_rules::remove_op(tlds)
    }
    /// The backups are read from rexenv's own backup directory by the elevated step, which does not take a
    /// path from the caller; the TLDs are what cross.
    fn restore_command(&self, restores: &[(String, PathBuf)]) -> String {
        nrpt_rules::restore_op(&restores.iter().map(|(t, _)| t.clone()).collect::<Vec<_>>())
    }
}

pub struct WindowsCertTrust;
impl CertTrustManager for WindowsCertTrust {
    /// This user's Root certificate store (`cert_store.rs`, ledger #613) — Windows asks the user to
    /// confirm the add, and the call waits on the answer.
    fn trust_ca(&self, ca_cert_path: &Path) -> Result<()> {
        cert_store::trust(ca_cert_path)
    }
    /// Removed from the same store; Windows asks again.
    fn untrust_ca(&self, ca_cert_path: &Path) -> Result<()> {
        cert_store::untrust(ca_cert_path)
    }
    /// The stale-CA sweep (#678), which was the `Ok(0)` default here until 18 Sep 2026: a
    /// hand-wiped app-data folder left its old `rexenv Local CA` a trusted root on Windows too.
    fn untrust_stale(&self, current_ca: &Path) -> Result<usize> {
        cert_store::untrust_stale(current_ca)
    }
    fn is_trusted(&self, ca_cert_path: &Path) -> bool {
        cert_store::is_trusted(ca_cert_path)
    }
    /// `%APPDATA%\Mozilla\Firefox` (`firefox_root.rs`, ledger #612): `directories`' config dir is the
    /// Roaming AppData known folder on Windows.
    fn firefox_profiles_root(&self) -> Option<PathBuf> {
        firefox_root::profiles_root_in(directories::BaseDirs::new()?.config_dir())
    }
}

pub struct WindowsPrivileges;
impl PrivilegeManager for WindowsPrivileges {
    /// rexenv's own dialog with the reason, then UAC elevating `rexenv.exe --elevated-step`, which runs only
    /// rexenv's ops (`elevation.rs`, ledger #619). `script` is ops text — `WindowsDns`'s commands — never
    /// PowerShell; anything else is refused before a dialog is shown.
    fn run_privileged(&self, script: &str, reason: &crate::platform::traits::PromptReason) -> Result<String> {
        elevation::run_privileged(script, &reason.sentence())
    }
    /// No privileged ports on Windows: the desktop user's filtered token bound `:443` and `:80`
    /// (measured on the Dell, 14 Sep 2026, ledger #611).
    fn port_needs_privilege(&self, _port: u16) -> bool {
        false
    }
}

/// Identity and the port gate (`process.rs`, `port_table.rs`, ledger #599); spawning
/// services that outlive the app and stopping them (ledger #600).
pub struct WindowsSupervisor;

/// Flags for a SERVICE — a process that must outlive rexenv (ledger #600):
/// - `CREATE_BREAKAWAY_FROM_JOB`: measured on the Dell 13 Sep 2026, a process spawned from
///   inside a kill-on-close job (an SSH session is one; a terminal or IDE can be) died with
///   the job without it and survived with it;
/// - `CREATE_NO_WINDOW`: no console window flashing up for a console program;
/// - `CREATE_NEW_PROCESS_GROUP`: a Ctrl+C in a console rexenv was started from is not
///   delivered to its services.
const SERVICE_FLAGS: u32 = windows_sys::Win32::System::Threading::CREATE_BREAKAWAY_FROM_JOB
    | windows_sys::Win32::System::Threading::CREATE_NO_WINDOW
    | windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP;

impl ProcessSupervisor for WindowsSupervisor {
    /// A short-lived helper (`mysqld --initialize-insecure`, a config test) the caller
    /// waits for: no console window, stdio inherited, and NOT broken away — a helper
    /// that dies with the app is correct, and a launcher that forbids breakaway must not
    /// fail it.
    fn spawn(&self, program: &Path, args: &[String]) -> Result<Child> {
        use std::os::windows::process::CommandExt;
        Ok(std::process::Command::new(program)
            .args(args)
            .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
            .spawn()?)
    }
    fn spawn_logged(&self, program: &Path, args: &[String], log_path: &Path) -> Result<Child> {
        self.spawn_logged_env(program, args, log_path, &[])
    }
    /// A streamed step — Composer, git, npm (`core::repo::run_step_streamed`, ledger #609): the given
    /// environment ONLY (the registry-fresh one `login_shell_env` builds), stdout and stderr piped, no
    /// console window, and started SUSPENDED so it is inside its kill-on-close job before it can start
    /// a grandchild. A job that cannot be made, or joined, refuses the step: a step rexenv could not
    /// stop is not started.
    fn spawn_streamed(&self, program: &Path, args: &[String], cwd: &Path, env: &[(String, String)]) -> Result<Child> {
        use std::os::windows::io::AsRawHandle;
        use std::os::windows::process::CommandExt;
        use std::process::Stdio;
        use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, CREATE_SUSPENDED};
        process::keep_inheritable_handles_out_of_children();
        let job = process::StepJob::new()
            .ok_or_else(|| Error::Other(format!("could not create a job object to run {}", program.display())))?;
        let mut child = std::process::Command::new(program)
            .args(args)
            .current_dir(cwd)
            .env_clear()
            .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | CREATE_SUSPENDED)
            .spawn()?;
        let refuse = |child: &mut Child, why: &str| {
            let _ = child.kill();
            let _ = child.wait();
            Error::Other(format!("{} was not started: {why}", program.display()))
        };
        if !job.assign(child.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE) {
            return Err(refuse(&mut child, "it could not be put in its job object"));
        }
        if !process::resume_suspended(child.id()) {
            return Err(refuse(&mut child, "its suspended thread could not be resumed"));
        }
        let mut jobs = step_jobs().lock().map_err(|_| Error::Other("the step job registry is poisoned".into()))?;
        // Finished steps' jobs are empty; closing them now kills nothing. A job whose count cannot
        // be read is kept — closing it would kill whatever it holds.
        jobs.retain(|_, j| j.active_processes().map_or(true, |n| n > 0));
        jobs.insert(child.id(), job);
        Ok(child)
    }
    /// End a streamed step: terminate its job — the step and every process it started — and wait
    /// until the job is empty. A pid with no job registered has nothing of rexenv's left to stop.
    fn stop_group(&self, pgid: u32) -> Result<()> {
        let job = step_jobs()
            .lock()
            .map_err(|_| Error::Other("the step job registry is poisoned".into()))?
            .remove(&pgid);
        let Some(job) = job else {
            return Ok(());
        };
        job.terminate();
        for _ in 0..50 {
            if job.active_processes() == Some(0) {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        Err(Error::Other(format!("step {pgid}'s processes were still running 5 s after its job was terminated")))
    }
    /// A service: stdout and stderr appended to its log, stdin closed, [`SERVICE_FLAGS`].
    fn spawn_logged_env(
        &self,
        program: &Path,
        args: &[String],
        log_path: &Path,
        env: &[(String, String)],
    ) -> Result<Child> {
        use std::os::windows::process::CommandExt;
        // Before the spawn: a service must not inherit — and pin open for its whole life —
        // the handles rexenv itself was started with (`handles.rs`, measured).
        process::keep_inheritable_handles_out_of_children();
        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let out = std::fs::OpenOptions::new().create(true).append(true).open(log_path)?;
        let err = out.try_clone()?;
        std::process::Command::new(program)
            .args(args)
            .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdin(std::process::Stdio::null())
            .stdout(out)
            .stderr(err)
            .creation_flags(SERVICE_FLAGS)
            .spawn()
            .map_err(|e| {
                if e.raw_os_error() == Some(windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED as i32) {
                    // CreateProcess answers access denied both for a blocked file and for
                    // a breakaway the launcher's job forbids — name both, since only the
                    // second is fixed by how rexenv is started.
                    Error::Other(format!(
                        "Windows refused to start {} (access denied): either the file is blocked, \
                         or rexenv was started inside a job that forbids its services to outlive \
                         it — start rexenv from the Start menu or Explorer rather than from a tool \
                         that confines it",
                        program.display()
                    ))
                } else {
                    e.into()
                }
            })
    }
    /// The process's own clean-shutdown channel with a grace, else `TerminateProcess`
    /// (`stop_policy.rs`, owner ruling 13 Sep 2026, ledger #600).
    fn stop(&self, pid: u32) -> Result<()> {
        let mut target = process::Stoppable::open(pid)?;
        match stop_policy::stop(&mut target, stop_policy::CLEAN_EXIT_GRACE, stop_policy::TERMINATE_WAIT) {
            stop_policy::Outcome::Survived => {
                Err(Error::Other(format!("pid {pid} was still running after TerminateProcess")))
            }
            _ => Ok(()),
        }
    }
    /// Our own child: the same stop, then reap. The `Child` handle keeps the process
    /// object — and so its pid — alive until the wait.
    fn terminate_child(&self, child: &mut Child) {
        let _ = self.stop(child.id());
        let _ = child.wait();
    }
    fn mysqld_supervision_args(&self) -> Vec<String> {
        stop_policy::MYSQLD_ARGS.iter().map(|a| a.to_string()).collect()
    }
    /// No php-fpm exists for Windows: a php-cgi group (plan D1). The extensions are the 26
    /// the official NTS x64 zip ships that the macOS build also carries (owner ruling 14 Sep
    /// 2026); pcntl/posix/sysvmsg/sysvsem do not exist here, and the PECL ones (apcu,
    /// imagick, redis, event, swoole, protobuf, opentelemetry) are not in v1.
    fn php_pool_model(&self) -> PoolModel {
        PoolModel::CgiGroup(CgiGroup {
            extensions: &[
                "bz2", "curl", "dba", "exif", "fileinfo", "ftp", "gd", "gmp", "imap", "intl",
                "mbstring", "mysqli", "openssl", "pdo_mysql", "pdo_pgsql", "pdo_sqlite", "pgsql",
                "shmop", "soap", "sockets", "sodium", "sqlite3", "sysvshm", "xsl", "zip",
            ],
            zend_extensions: &["opcache"],
        })
    }
    fn pid_alive(&self, pid: u32) -> bool {
        process::alive(pid)
    }
    /// ToolHelp reports image file names with their extension, and callers pass the
    /// bare name the macOS `pgrep -x` takes — both spellings match, ignoring case.
    fn pids_named(&self, name: &str) -> Vec<u32> {
        let with_exe = format!("{name}.exe");
        process::processes()
            .into_iter()
            .filter(|e| e.exe.eq_ignore_ascii_case(name) || e.exe.eq_ignore_ascii_case(&with_exe))
            .map(|e| e.pid)
            .collect()
    }
    fn pid_exe(&self, pid: u32) -> Option<PathBuf> {
        process::image_path(pid)
    }
    fn pid_command(&self, pid: u32) -> Option<String> {
        process::command_line(pid)
    }
    fn port_holders(&self, port: u16, udp: bool) -> Option<Vec<u32>> {
        process::port_holders(port, udp)
    }
    fn established_on(&self, port: u16) -> Option<usize> {
        process::established_on(port)
    }
    fn stop_pid_command(&self, pid: u32) -> String {
        format!("Stop-Process -Id {pid}")
    }
    /// TCP listeners on `port` whose command line carries the marker, case-insensitively
    /// (`port_table::command_carries_marker`).
    fn owned_listeners(&self, port: u16, owner_marker: &str) -> Vec<u32> {
        process::port_holders(port, false)
            .unwrap_or_default()
            .into_iter()
            .filter(|&pid| {
                process::command_line(pid)
                    .is_some_and(|cmd| port_table::command_carries_marker(&cmd, owner_marker))
            })
            .collect()
    }
    /// Climbed from the listeners to the group's topmost marked process: nginx's listener
    /// is its worker, not its master (measured — `process::climb_to_group_root`).
    fn owned_master(&self, port: u16, owner_marker: &str) -> Option<u32> {
        process::climb_to_group_root(&self.owned_listeners(port, owner_marker), owner_marker)
    }
    /// nginx listens for a reload EVENT on Windows, not a signal (`stop_policy.rs`).
    fn signal_reload(&self, pid: u32) -> bool {
        process::set_named_event(&stop_policy::nginx_reload_event(pid))
    }
    fn owned_pids(&self, marker: &str) -> Vec<u32> {
        process::processes()
            .into_iter()
            .filter(|e| {
                process::command_line(e.pid)
                    .is_some_and(|cmd| port_table::command_carries_marker(&cmd, marker))
            })
            .map(|e| e.pid)
            .collect()
    }
    /// The root holder by image, hosted services or "cannot inspect"; with no holder at
    /// all, the excluded range the port sits in — the one refusal no socket table shows.
    fn port_conflict_help(&self, port: u16, udp: bool) -> PortConflictHelp {
        self.port_conflict_help_on(port, udp, false)
    }
    /// With `loopback_only`, only a holder on `127.0.0.1`/`::1` is named, and never an excluded
    /// range: a range refuses every address alike, which is not what address-in-use on loopback
    /// means (ledger #615).
    fn port_conflict_help_on(&self, port: u16, udp: bool, loopback_only: bool) -> PortConflictHelp {
        let holders = process::port_holders_where(port, udp, loopback_only).unwrap_or_default();
        let named = match process::root_of(&holders) {
            Some(pid) => {
                let image = process::image_path(pid).map(|p| p.display().to_string());
                Some(port_table::describe_holder(pid, image.as_deref(), &process::services_hosted_by(pid)))
            }
            None if loopback_only => None,
            None => process::excluded_ranges(udp)
                .and_then(|out| port_table::excluded_range_containing(&out, port))
                .map(|range| port_table::describe_excluded(range, udp)),
        };
        named
            .map(|h| PortConflictHelp { holder: Some(h.holder), app: h.app, free_command: h.free_command })
            .unwrap_or_default()
    }
}

/// The per-user Run value `"<exe>" --hidden` (`autostart.rs` + `autostart_rules.rs`, W7 S4, ledger #623) —
/// no elevation, and never a scheduled task: a task's job refuses the breakaway rexenv's services need (plan
/// §3 D1, measured).
pub struct WindowsAutostart;
impl AutostartManager for WindowsAutostart {
    fn enable(&self) -> Result<()> {
        autostart::enable()
    }
    fn disable(&self) -> Result<()> {
        autostart::disable()
    }
    fn is_enabled(&self) -> Result<bool> {
        autostart::is_enabled()
    }
    fn refresh(&self) -> Result<()> {
        autostart::refresh()
    }
}

pub struct WindowsPermissions;
impl PermissionManager for WindowsPermissions {
    /// Windows runs a file by its extension; there is no execute bit to set. The
    /// metadata read keeps the macOS contract that a missing file is an error rather
    /// than a silent success.
    fn set_executable(&self, path: &Path) -> Result<()> {
        std::fs::metadata(path)?;
        Ok(())
    }
    /// A protected DACL with one entry, the current user (`acl.rs`, ledger #597).
    fn set_private(&self, path: &Path) -> Result<()> {
        acl::set_private(path)
    }
    /// Born owner-only through `CreateFileW`'s security attributes; an existing file
    /// is re-hardened before it is truncated and written (`acl.rs`, ledger #597).
    fn write_private(&self, path: &Path, contents: &[u8]) -> Result<()> {
        acl::write_private(path, contents)
    }
}

/// Streamed steps' kill-on-close jobs, by the step's pid — the `pgid` `stop_group` is handed
/// (ledger #609).
fn step_jobs() -> &'static std::sync::Mutex<std::collections::HashMap<u32, process::StepJob>> {
    static JOBS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<u32, process::StepJob>>> =
        std::sync::OnceLock::new();
    JOBS.get_or_init(Default::default)
}

/// `ShellExecuteW("open")` on a target `WindowsShell::open` already allowed. A return of 32 or less is the
/// shell's error code (measured on the Dell: a folder and a URL both returned 42).
fn shell_open(target: &str) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide = |s: &str| -> Vec<u16> { std::ffi::OsStr::new(s).encode_wide().chain(Some(0)).collect() };
    let (verb, file) = (wide("open"), wide(target));
    // SAFETY: nul-terminated strings that outlive the call; no parent window, no parameters or directory.
    let code = unsafe {
        ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL)
    } as isize;
    if code > 32 {
        Ok(())
    } else {
        Err(Error::Other(format!("Windows could not open `{target}` (shell error {code})")))
    }
}

pub struct WindowsShell;
impl ShellRunner for WindowsShell {
    fn run(&self, _command: &str, _args: &[String]) -> Result<String> {
        Err(Error::Unported("windows shell runner"))
    }
    /// `http(s)` URLs, existing folders and existing reading files go to `ShellExecuteW("open")`; anything the
    /// shell could RUN is refused by name (`shell_rules.rs`, owner's ruling 15 Sep 2026, ledger #621).
    fn open(&self, target: &str) -> Result<()> {
        use shell_rules::{classify_open, OnDisk, OpenTarget};
        let on_disk = match std::fs::metadata(target) {
            Ok(m) if m.is_dir() => OnDisk::Folder,
            Ok(_) => OnDisk::File,
            Err(_) => OnDisk::Missing,
        };
        if let OpenTarget::Refused(why) = classify_open(target, on_disk) {
            return Err(Error::Other(why));
        }
        shell_open(target)
    }
    /// Explorer with the item selected. The path must exist: explorer's own exit code carries no meaning (it
    /// was 1 on success, measured), so a missing path is caught here or not at all.
    fn reveal(&self, path: &str) -> Result<()> {
        use std::os::windows::process::CommandExt;
        if std::fs::symlink_metadata(path).is_err() {
            return Err(Error::Other(format!("`{path}` does not exist")));
        }
        std::process::Command::new("explorer.exe").raw_arg(shell_rules::reveal_argument(path)).spawn()?;
        Ok(())
    }
    /// The user environment a fresh logon would build, read from the registry on every call, so a
    /// tool installed while rexenv runs is on its Path (owner ruling 14 Sep 2026, ledger #609,
    /// `login_env.rs`). Variables the registry does not hold — `SystemRoot`, `USERPROFILE` — come
    /// from this process.
    fn login_shell_env(&self) -> Result<Vec<(String, String)>> {
        let own: Vec<(String, String)> = std::env::vars_os()
            .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
            .collect();
        let (system, user) = (process::registry_env(true), process::registry_env(false));
        Ok(login_env::merge_login_env(&own, system.as_deref(), user.as_deref()))
    }

    /// The user's own `Path` in `HKEY_CURRENT_USER\Environment` (`user_path.rs`, ledger #634).
    fn user_path_has(&self, dir: &Path) -> Result<bool> {
        user_path::has(dir)
    }

    fn add_to_user_path(&self, dir: &Path) -> Result<()> {
        user_path::add(dir)
    }

    fn remove_from_user_path(&self, dir: &Path) -> Result<()> {
        user_path::remove(dir)
    }

    /// The editors `app_catalog.rs` finds from what installers registered, each executable existing on
    /// disk (ledger #622). Read fresh on every call — an app can be uninstalled any day. Icons are `None`.
    fn detect_editors(&self) -> Vec<EditorApp> {
        app_catalog::editors(&app_registry::installed(), &|p| p.is_file())
            .into_iter()
            .map(|f| EditorApp { id: f.id.into(), name: f.name.into(), icon: None })
            .collect()
    }

    fn open_in_editor(&self, editor_id: &str, path: &str) -> Result<()> {
        if !Path::new(path).exists() {
            return Err(Error::Other(format!("`{path}` does not exist")));
        }
        let found = app_catalog::editors(&app_registry::installed(), &|p| p.is_file())
            .into_iter()
            .find(|f| f.id == editor_id)
            .ok_or_else(|| Error::Other(format!("the editor `{editor_id}` is not installed")))?;
        start(app_catalog::editor_launch(&found, Path::new(path)))
    }

    fn detect_browsers(&self) -> Vec<BrowserApp> {
        app_catalog::browsers(&app_registry::installed(), &|p| p.is_file())
            .into_iter()
            .map(|(f, system_default, supports_private)| BrowserApp {
                id: f.id.into(),
                name: f.name.into(),
                icon: None,
                system_default,
                supports_private,
            })
            .collect()
    }

    /// `http(s)` only, refused before the lookup (`app_catalog::web_url_only`, the one check); a private
    /// window errors for a browser with no flag rather than opening a recorded one.
    fn open_in_browser(&self, browser_id: &str, url: &str, private: bool) -> Result<()> {
        app_catalog::web_url_only(url).map_err(Error::Other)?;
        let (found, _, _) = app_catalog::browsers(&app_registry::installed(), &|p| p.is_file())
            .into_iter()
            .find(|(f, _, _)| f.id == browser_id)
            .ok_or_else(|| Error::Other(format!("the browser `{browser_id}` is not installed")))?;
        start(app_catalog::browser_launch(&found, url, private).map_err(Error::Other)?)
    }

    fn detect_terminals(&self) -> Vec<TerminalApp> {
        app_catalog::terminals(&app_registry::installed(), &|p| p.is_file())
            .into_iter()
            .map(|f| TerminalApp { id: f.id.into(), name: f.name.into(), icon: None })
            .collect()
    }

    /// An existing DIRECTORY only, refused before the lookup: a terminal handed a file would run it.
    fn open_in_terminal(&self, terminal_id: &str, path: &Path) -> Result<()> {
        if !path.is_dir() {
            return Err(Error::Other(format!(
                "refusing to open a terminal at {} — only an existing directory is a working directory (a \
                 terminal handed a file would run it)",
                path.display()
            )));
        }
        let found = app_catalog::terminals(&app_registry::installed(), &|p| p.is_file())
            .into_iter()
            .find(|f| f.id == terminal_id)
            .ok_or_else(|| Error::Other(format!("the terminal `{terminal_id}` is not installed")))?;
        start(app_catalog::terminal_launch(&found, path))
    }

    /// A directory JUNCTION, not a symbolic link: any user may make one, while a directory symlink needs
    /// Developer Mode or a privilege (refused with 1314 on the Dell — measured) (`junction.rs`, ledger #625).
    fn symlink_dir(&self, target: &Path, link: &Path) -> Result<()> {
        junction::create(target, link)
    }

    /// Only a link — a junction or a symbolic link — and only the link itself (`junction.rs`, ledger #625).
    fn remove_symlink(&self, link: &Path) -> Result<()> {
        junction::remove(link)
    }
}

/// Start one editor, browser or terminal: the executable and its arguments, no shell in between. Not
/// waited on — the app outlives this call. Its standard handles are NUL, never this process's: a browser or
/// an editor started with them keeps writing its logs into them for as long as it runs (measured on the Dell,
/// 15 Sep 2026: a Chrome the check started held the check's output pipe open with GCM errors, and the run
/// never ended; VS Code's Electron warnings landed there too). A GUI app needs none of them.
/// NUL alone was not enough: `std` starts every child inheriting ALL of this process's inheritable handles, so
/// the same Chrome still held the pipe open, silently, and the run still never ended (measured the same day).
/// So every handle's inherit flag is cleared first, as before a service spawn (ledger #600): an editor or a
/// browser can never pin a pipe, a log or a socket of rexenv's for as long as it stays open.
fn start(launch: app_catalog::Launch) -> Result<()> {
    if launch.new_console {
        return start_in_new_console(&launch);
    }
    let mut cmd = std::process::Command::new(&launch.exe);
    cmd.args(&launch.args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(dir) = &launch.current_dir {
        cmd.current_dir(dir);
    }
    process::keep_inheritable_handles_out_of_children();
    cmd.spawn().map_err(|e| Error::Other(format!("could not start {}: {e}", launch.exe.display())))?;
    Ok(())
}

/// A console program in a console of its OWN. Not `std::process::Command`: it always hands the child this
/// process's standard handles (`STARTF_USESTDHANDLES`), so a terminal started by a process whose output is
/// redirected — a check run, `tauri dev` in a terminal — reads and writes THOSE instead of its window
/// (measured on the Dell, 15 Sep 2026: the new PowerShell's prompt landed in the check's log). Here no handle
/// is passed or inherited, so the new console gives the child its own. Its arguments are fixed flags only
/// (`app_catalog::console_command_line` refuses anything else), so the command line needs no escaping.
fn start_in_new_console(launch: &app_catalog::Launch) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, CREATE_NEW_CONSOLE, PROCESS_INFORMATION, STARTUPINFOW,
    };
    let line = app_catalog::console_command_line(launch).ok_or_else(|| {
        Error::Other(format!("refusing to start {} in a console with anything but fixed flags", launch.exe.display()))
    })?;
    let wide = |s: &std::ffi::OsStr| -> Vec<u16> { s.encode_wide().chain(Some(0)).collect() };
    let exe = wide(launch.exe.as_os_str());
    let mut command_line: Vec<u16> = line.encode_utf16().chain(Some(0)).collect();
    let dir = launch.current_dir.as_ref().map(|d| wide(d.as_os_str()));
    // SAFETY: plain-old-data structs, zero-initialised as CreateProcessW expects before `cb` is set.
    let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
    startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    // SAFETY: as above.
    let mut process: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: nul-terminated strings that outlive the call, a writable command-line buffer (CreateProcessW may
    // modify it), no inherited handles, and valid in/out structs.
    let ok = unsafe {
        CreateProcessW(
            exe.as_ptr(),
            command_line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_NEW_CONSOLE,
            std::ptr::null(),
            dir.as_ref().map_or(std::ptr::null(), |d| d.as_ptr()),
            &startup,
            &mut process,
        )
    };
    if ok == 0 {
        let e = std::io::Error::last_os_error();
        return Err(Error::Other(format!("could not start {}: {e}", launch.exe.display())));
    }
    // SAFETY: the two handles CreateProcessW returned, closed once; the process runs on without them.
    unsafe {
        CloseHandle(process.hProcess);
        CloseHandle(process.hThread);
    }
    Ok(())
}

/// Windows needs no relinking and no signature to run a downloaded executable, so
/// "prepare" is two checks rather than two rewrites (ledger #598):
///
/// - **The Mark of the Web is removed** — the `Zone.Identifier` stream SmartScreen
///   consults before running a file. Files rexenv fetches itself (reqwest) and
///   extracts itself (the `zip` crate) are believed to carry none: the stream is
///   written by browsers and `IAttachmentExecute` callers, per Microsoft's docs — not
///   yet measured on Windows. So this is defence, cheap and idempotent, for a file
///   that arrived some other way.
/// - **Every image must be one x64 Windows can run** — x64, or x86 under WOW64 — or
///   it is refused before it is published: a pin naming the wrong archive member, an
///   arm64 build, or bytes that are no executable at all. The Windows form of macOS's
///   "never publish a binary that cannot load". x86 is accepted because the official
///   artifacts carry it: nginx.org's `nginx.exe`, PHP 7.4.33's ICU data DLL, MySQL
///   8.4.6's configurator (measured 13 Sep 2026, `pe.rs`).
///
/// `core` calls `prepare_binary` for single executables (Caddy, Mailpit,
/// cloudflared) and `prepare_binary_dir` for directory distributions — PHP, nginx,
/// MySQL, PostgreSQL (owner ruling 13 Sep 2026). `prepare_binary_tree` runs only for
/// `Shape::Bundle`, which D4 refuses on Windows, so no path reaches it today.
pub struct WindowsBinaryProvider;
impl BinaryProvider for WindowsBinaryProvider {
    fn arch(&self) -> Arch {
        Arch::X86_64
    }
    fn prepare_binary(&self, path: &Path) -> Result<()> {
        strip_mark_of_the_web(path)?;
        require_runnable_image(path)
    }
    fn prepare_binary_tree(&self, root: &Path) -> Result<()> {
        check_tree(root)
    }
    fn prepare_binary_dir(&self, root: &Path) -> Result<()> {
        check_tree(root)
    }
}

/// Every file loses its Mark of the Web; every `.exe`/`.dll` must be runnable; a tree
/// with none is not a binary tree. One function for bundles and directory
/// distributions, so the two can never apply different rules.
fn check_tree(root: &Path) -> Result<()> {
    let mut files = Vec::new();
    collect_files(root, &mut files)?;
    let mut images = 0usize;
    for file in &files {
        strip_mark_of_the_web(file)?;
        let is_image = file
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(pe::is_image_name);
        if is_image {
            require_runnable_image(file)?;
            images += 1;
        }
    }
    if images == 0 {
        return Err(Error::Other(format!(
            "no .exe or .dll under {} — not a Windows binary tree",
            root.display()
        )));
    }
    Ok(())
}

/// Remove `path`'s `Zone.Identifier` alternate data stream, if it has one.
fn strip_mark_of_the_web(path: &Path) -> Result<()> {
    let mut stream = path.as_os_str().to_os_string();
    stream.push(":Zone.Identifier");
    match std::fs::remove_file(&stream) {
        Ok(()) => Ok(()),
        // No stream — the common case — or a volume that cannot hold one: FAT32 and
        // exFAT answer ERROR_INVALID_NAME for a `:stream` path.
        Err(e)
            if e.kind() == std::io::ErrorKind::NotFound
                || e.raw_os_error() == Some(windows_sys::Win32::Foundation::ERROR_INVALID_NAME as i32) =>
        {
            Ok(())
        }
        Err(e) => Err(Error::Other(format!(
            "could not remove the Mark of the Web from {}: {e}",
            path.display()
        ))),
    }
}

/// Refuse `path` unless its PE header names a machine x64 Windows runs.
fn require_runnable_image(path: &Path) -> Result<()> {
    use std::io::Read;
    let mut head = Vec::with_capacity(pe::HEAD_BYTES);
    std::fs::File::open(path)?
        .take(pe::HEAD_BYTES as u64)
        .read_to_end(&mut head)?;
    match pe::classify(&head) {
        kind if pe::runs_on_x64_windows(kind) => Ok(()),
        pe::PeKind::OtherMachine(machine) => Err(Error::Other(format!(
            "{} is a Windows executable for machine 0x{machine:04x}, which x64 Windows cannot \
             run — this pin names the wrong build",
            path.display()
        ))),
        pe::PeKind::NotPe => Err(Error::Other(format!(
            "{} is not a Windows executable — the pin names the wrong file or archive member",
            path.display()
        ))),
        // X64 and X86 are matched by the guard above.
        pe::PeKind::X64 | pe::PeKind::X86 => Ok(()),
    }
}

/// Every regular file under `dir`, recursively. Symlinks are not followed: the zip
/// extractor already refuses them (ledger #585), and a tree check must never walk
/// out of the tree it was given.
fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            collect_files(&entry.path(), out)?;
        } else if kind.is_file() {
            out.push(entry.path());
        }
    }
    Ok(())
}

pub struct WindowsEdge;
impl EdgeSupervisor for WindowsEdge {
    /// Windows runs the edge as an ordinary service process — there is no OS supervisor to install
    /// (ledger #611) — so there is never a daemon to find, boot out or uninstall.
    fn is_installed(&self) -> bool {
        false
    }
    fn is_enabled(&self) -> bool {
        true
    }
    /// Loopback only: an all-interfaces bind raised Windows Defender Firewall's allow prompt on the
    /// desktop (measured on the Dell); owner ruling 14 Sep 2026.
    fn default_bind(&self) -> Option<&'static str> {
        Some("127.0.0.1")
    }
    fn plist_path(&self) -> PathBuf {
        unported!("windows edge supervisor")
    }
    fn wrapper_path(&self) -> PathBuf {
        unported!("windows edge supervisor")
    }
    fn daemon_binary_path(&self) -> PathBuf {
        unported!("windows edge supervisor")
    }
    fn plist_contents(&self, _wrapper: &Path, _start_log: &Path) -> String {
        unported!("windows edge supervisor")
    }
    fn wrapper_contents(
        &self,
        _caddy_bin: &Path,
        _caddyfile: &Path,
        _admin_sock: &Path,
        _appdata: &Path,
    ) -> String {
        unported!("windows edge supervisor")
    }
    fn install_command(
        &self,
        _src_caddy: &Path,
        _staged_wrapper: &Path,
        _staged_plist: &Path,
    ) -> String {
        unported!("windows edge supervisor")
    }
    fn start_command(&self) -> String {
        unported!("windows edge supervisor")
    }
    fn stop_command(&self) -> String {
        unported!("windows edge supervisor")
    }
    fn uninstall_command(&self) -> String {
        unported!("windows edge supervisor")
    }
}

pub struct WindowsDnsAgent;

impl WindowsDnsAgent {
    /// `schtasks` with no console window (the app is a GUI process; a console program it starts would
    /// flash one).
    fn schtasks(args: &[&str]) -> Result<std::process::Output> {
        use std::os::windows::process::CommandExt;
        Ok(std::process::Command::new("schtasks")
            .args(args)
            .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
            .output()?)
    }

    /// Run `schtasks`, and on a non-zero exit say which step failed with what schtasks printed.
    fn schtasks_ok(step: &str, args: &[&str]) -> Result<()> {
        let out = Self::schtasks(args)?;
        if out.status.success() {
            return Ok(());
        }
        let said = format!("{} {}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        Err(Error::Other(format!(
            "could not {step} rexenv's DNS agent task ({}): {}",
            logon_task::DNS_AGENT_TASK,
            said.split_whitespace().collect::<Vec<_>>().join(" ")
        )))
    }
}

/// The agent as a logon Scheduled Task for this user (`logon_task.rs`, ledger #616): registered without
/// elevation, run at logon and — as the keep-alive — every minute, a no-op while it already runs.
impl DnsAgentManager for WindowsDnsAgent {
    fn is_installed(&self) -> bool {
        Self::schtasks(&["/Query", "/TN", logon_task::DNS_AGENT_TASK]).is_ok_and(|o| o.status.success())
    }
    /// rexenv's copy of the definition it registered, under app data — what `install` compares with.
    fn definition_path(&self) -> Result<PathBuf> {
        Ok(WindowsPaths.config_dir()?.join("dns-agent-task.xml"))
    }
    fn definition_contents(&self, exe: &Path, log: &Path) -> String {
        // An unreadable SID yields a definition schtasks refuses; `install` reads it first and fails
        // with the real reason instead.
        logon_task::dns_agent_task_xml(&acl::current_user_sid().unwrap_or_default(), exe, log)
    }
    /// Register (or re-register) the task and start it now. An unchanged definition with the task
    /// present is left alone — the app calls this on every launch.
    fn install(&self, exe: &Path, log: &Path) -> Result<()> {
        let sid = acl::current_user_sid()?;
        let bytes = logon_task::utf16_file_bytes(&logon_task::dns_agent_task_xml(&sid, exe, log));
        let path = self.definition_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let unchanged = std::fs::read(&path).is_ok_and(|b| b == bytes);
        if unchanged && self.is_installed() {
            return Ok(());
        }
        std::fs::write(&path, &bytes)?;
        let file = path.display().to_string();
        Self::schtasks_ok("register", &["/Create", "/TN", logon_task::DNS_AGENT_TASK, "/XML", &file, "/F"])?;
        // A changed definition (another build's path): end the old instance so the new one runs now.
        let _ = Self::schtasks(&["/End", "/TN", logon_task::DNS_AGENT_TASK]);
        Self::schtasks_ok("start", &["/Run", "/TN", logon_task::DNS_AGENT_TASK])
    }
    /// End the running agent and start it again, under the same registration.
    fn kickstart(&self) -> Result<()> {
        let _ = Self::schtasks(&["/End", "/TN", logon_task::DNS_AGENT_TASK]);
        Self::schtasks_ok("restart", &["/Run", "/TN", logon_task::DNS_AGENT_TASK])
    }
    /// End it, delete the task (Task Scheduler drops the emptied `\rexenv\` folder) and rexenv's copy.
    fn uninstall(&self) -> Result<()> {
        if self.is_installed() {
            let _ = Self::schtasks(&["/End", "/TN", logon_task::DNS_AGENT_TASK]);
            Self::schtasks_ok("delete", &["/Delete", "/TN", logon_task::DNS_AGENT_TASK, "/F"])?;
        }
        let path = self.definition_path()?;
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }
}
pub struct WindowsAppBundle;
impl AppBundle for WindowsAppBundle {
    fn facts(&self, _exe: &Path) -> Result<BundleFacts> {
        Err(Error::Unported("windows app bundle facts (self-update, plan D5/W11)"))
    }
    fn stage(
        &self,
        _facts: &BundleFacts,
        _archive: &Path,
        _expect: &StagedExpect,
    ) -> Result<StagedBundle> {
        Err(Error::Unported("windows stage a replacement bundle"))
    }
    fn swap(
        &self,
        _installed: &Path,
        _staged: &StagedBundle,
    ) -> std::result::Result<SwapReceipt, SwapFailure> {
        Err(SwapFailure::Other(Error::Unported("windows swap the bundle").to_string()))
    }
    fn spawn_relauncher(&self, _bundle: &Path) -> Result<()> {
        Err(Error::Unported("windows relauncher"))
    }
    fn sweep_leftovers(
        &self,
        _parent: &Path,
        _my_version: &str,
        _delete_previous: bool,
    ) -> Result<Vec<Leftover>> {
        Err(Error::Unported("windows sweep update leftovers"))
    }
}
pub struct WindowsLocalIpc;
impl LocalIpc for WindowsLocalIpc {
    /// An AF_UNIX stream (`af_unix.rs`, ledger #611): what this trait dials — Caddy's admin socket
    /// and a database's socket — are unix sockets on every OS, and Windows serves them. The CLI
    /// and MCP endpoints rexenv itself LISTENS on are D3's named pipes, a separate half (W8).
    fn connect(
        &self,
        path: &Path,
        read_timeout: Option<std::time::Duration>,
    ) -> std::io::Result<Box<dyn std::io::Read + Send>> {
        Ok(Box::new(af_unix::connect_path(path, read_timeout)?))
    }
}
/// The panic hook's notice (`platform::fatal_notice`): a native message box,
/// because a `windows_subsystem = "windows"` process has no console to print to.
/// Compiled in every Windows build so `windows-check` type-checks it; called only
/// when debug assertions are off. The app may survive a panic on a background
/// thread, so the text does not claim it stopped.
pub fn fatal_notice(summary: &str, crash_log: Option<&Path>) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONERROR, MB_OK, MB_SETFOREGROUND,
    };
    let details = match crash_log {
        Some(path) => format!("Details were written to:\n{}", path.display()),
        None => "Details could not be written to a file.".to_string(),
    };
    let body = format!("rexenv hit an internal error and may have stopped.\n\n{summary}\n\n{details}");
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (text, caption) = (wide(&body), wide("rexenv"));
    // SAFETY: both pointers are NUL-terminated UTF-16 buffers that outlive the
    // call, and a null owner window is allowed.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND,
        );
    }
}

pub struct WindowsPlatform {
    paths: WindowsPaths,
    dns: WindowsDns,
    cert_trust: WindowsCertTrust,
    privileges: WindowsPrivileges,
    supervisor: WindowsSupervisor,
    autostart: WindowsAutostart,
    permissions: WindowsPermissions,
    shell: WindowsShell,
    binaries: WindowsBinaryProvider,
    edge: WindowsEdge,
    dns_agent: WindowsDnsAgent,
    app_bundle: WindowsAppBundle,
}

impl WindowsPlatform {
    pub fn new() -> Self {
        Self {
            paths: WindowsPaths,
            dns: WindowsDns,
            cert_trust: WindowsCertTrust,
            privileges: WindowsPrivileges,
            supervisor: WindowsSupervisor,
            autostart: WindowsAutostart,
            permissions: WindowsPermissions,
            shell: WindowsShell,
            binaries: WindowsBinaryProvider,
            edge: WindowsEdge,
            dns_agent: WindowsDnsAgent,
            app_bundle: WindowsAppBundle,
        }
    }
}

impl Default for WindowsPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl Platform for WindowsPlatform {
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

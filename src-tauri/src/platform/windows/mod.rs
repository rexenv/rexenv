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
mod handles;
mod owner_only;
mod pe;
mod port_table;
mod process;
mod stop_policy;

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
    fn hosts_file(&self) -> PathBuf {
        PathBuf::from(r"C:\Windows\System32\drivers\etc\hosts")
    }
}

pub struct WindowsDns;
impl DnsManager for WindowsDns {
    fn resolver_path(&self, _tld: &str) -> PathBuf {
        unported!("windows DNS — no /etc/resolver equivalent (NRPT, plan D2)")
    }
    fn resolver_contents(&self, _port: u16) -> String {
        unported!("windows DNS")
    }
    fn install_command(&self, _tld: &str, _port: u16) -> String {
        unported!("windows DNS")
    }
    fn uninstall_command(&self, _tlds: &[String]) -> String {
        unported!("windows DNS")
    }
    fn restore_command(&self, _restores: &[(String, PathBuf)]) -> String {
        unported!("windows DNS")
    }
}

pub struct WindowsCertTrust;
impl CertTrustManager for WindowsCertTrust {
    fn trust_ca(&self, _ca_cert_path: &Path) -> Result<()> {
        Err(Error::Unported("windows CA trust (CurrentUser Root store)"))
    }
    fn untrust_ca(&self, _ca_cert_path: &Path) -> Result<()> {
        Err(Error::Unported("windows CA untrust (CurrentUser Root store)"))
    }
}

pub struct WindowsPrivileges;
impl PrivilegeManager for WindowsPrivileges {
    fn run_privileged(
        &self,
        _script: &str,
        _reason: &crate::platform::traits::PromptReason,
    ) -> Result<String> {
        Err(Error::Unported("windows UAC elevation"))
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
    fn owned_master(&self, port: u16, owner_marker: &str) -> Option<u32> {
        process::root_of(&self.owned_listeners(port, owner_marker))
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
        let holders = process::port_holders(port, udp).unwrap_or_default();
        let named = match process::root_of(&holders) {
            Some(pid) => {
                let image = process::image_path(pid).map(|p| p.display().to_string());
                Some(port_table::describe_holder(pid, image.as_deref(), &process::services_hosted_by(pid)))
            }
            None => process::excluded_ranges(udp)
                .and_then(|out| port_table::excluded_range_containing(&out, port))
                .map(|range| port_table::describe_excluded(range, udp)),
        };
        named
            .map(|h| PortConflictHelp { holder: Some(h.holder), app: h.app, free_command: h.free_command })
            .unwrap_or_default()
    }
}

pub struct WindowsAutostart;
impl AutostartManager for WindowsAutostart {
    fn enable(&self) -> Result<()> {
        Err(Error::Unported("windows autostart (HKCU Run key)"))
    }
    fn disable(&self) -> Result<()> {
        Err(Error::Unported("windows autostart (HKCU Run key)"))
    }
    fn is_enabled(&self) -> Result<bool> {
        Err(Error::Unported("windows autostart (HKCU Run key)"))
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

pub struct WindowsShell;
impl ShellRunner for WindowsShell {
    fn run(&self, _command: &str, _args: &[String]) -> Result<String> {
        Err(Error::Unported("windows shell runner"))
    }
    fn open(&self, _target: &str) -> Result<()> {
        Err(Error::Unported("windows shell open"))
    }
    fn reveal(&self, _path: &str) -> Result<()> {
        Err(Error::Unported("windows shell reveal"))
    }
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
    fn is_installed(&self) -> bool {
        unported!("windows edge supervisor")
    }
    fn is_enabled(&self) -> bool {
        unported!("windows edge supervisor")
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
impl DnsAgentManager for WindowsDnsAgent {
    fn is_installed(&self) -> bool {
        unported!("windows dns agent (logon task, plan W6)")
    }
    fn plist_path(&self) -> Result<PathBuf> {
        Err(Error::Unported("windows dns agent"))
    }
    fn plist_contents(&self, _exe: &Path, _log: &Path) -> String {
        unported!("windows dns agent")
    }
    fn install(&self, _exe: &Path, _log: &Path) -> Result<()> {
        Err(Error::Unported("windows dns agent"))
    }
    fn kickstart(&self) -> Result<()> {
        Err(Error::Unported("windows dns agent"))
    }
    fn uninstall(&self) -> Result<()> {
        Err(Error::Unported("windows dns agent"))
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
    fn connect(
        &self,
        _path: &Path,
        _read_timeout: Option<std::time::Duration>,
    ) -> std::io::Result<Box<dyn std::io::Read + Send>> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            Error::Unported("windows local IPC — a named pipe with a current-user ACL (D3, W8)").to_string(),
        ))
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

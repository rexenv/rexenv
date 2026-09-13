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
mod owner_only;
mod pe;

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

pub struct WindowsSupervisor;
impl ProcessSupervisor for WindowsSupervisor {
    fn spawn(&self, _program: &Path, _args: &[String]) -> Result<Child> {
        Err(Error::Unported("windows spawn"))
    }
    fn spawn_logged(&self, _program: &Path, _args: &[String], _log_path: &Path) -> Result<Child> {
        Err(Error::Unported("windows spawn_logged"))
    }
    fn stop(&self, _pid: u32) -> Result<()> {
        Err(Error::Unported("windows stop"))
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
/// - **The file must be an x64 PE image**, or it is refused before it is published:
///   a pin naming the wrong archive member, an x86 or arm64 build (every Windows pin
///   is x64, plan D6), or bytes that are no executable at all. The Windows form of
///   macOS's "never publish a binary that cannot load".
///
/// `core` calls `prepare_binary` for single executables (Caddy, Mailpit,
/// cloudflared). `prepare_binary_tree` runs only for `Shape::Bundle`, and every
/// bundle is refused on Windows (D4), so no path reaches it today — and
/// `resolve_dir` (PHP, nginx, MySQL, PostgreSQL trees) calls no prepare at all, on
/// either OS.
pub struct WindowsBinaryProvider;
impl BinaryProvider for WindowsBinaryProvider {
    fn arch(&self) -> Arch {
        Arch::X86_64
    }
    fn prepare_binary(&self, path: &Path) -> Result<()> {
        strip_mark_of_the_web(path)?;
        require_x64_image(path)
    }
    fn prepare_binary_tree(&self, root: &Path) -> Result<()> {
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
                require_x64_image(file)?;
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

/// Refuse `path` unless its PE header says x64.
fn require_x64_image(path: &Path) -> Result<()> {
    use std::io::Read;
    let mut head = Vec::with_capacity(pe::HEAD_BYTES);
    std::fs::File::open(path)?
        .take(pe::HEAD_BYTES as u64)
        .read_to_end(&mut head)?;
    match pe::classify(&head) {
        pe::PeKind::X64 => Ok(()),
        pe::PeKind::OtherMachine(machine) => Err(Error::Other(format!(
            "{} is a Windows executable for machine 0x{machine:04x}, not x64 — every Windows \
             artifact rexenv pins is x64, so this pin names the wrong build",
            path.display()
        ))),
        pe::PeKind::NotPe => Err(Error::Other(format!(
            "{} is not a Windows executable — the pin names the wrong file or archive member",
            path.display()
        ))),
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

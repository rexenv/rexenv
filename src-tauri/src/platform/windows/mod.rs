//! Windows implementations — Phase 4. Stubs only: every method is `todo!()`.
//! Filling these in is the entire Windows port; `core/` does not change.
#![allow(dead_code)]

use crate::error::Result;
use crate::platform::traits::*;
use std::path::{Path, PathBuf};
use std::process::Child;

pub struct WindowsPaths;
impl Paths for WindowsPaths {
    fn app_data_dir(&self) -> Result<PathBuf> {
        todo!("windows app_data_dir")
    }
    fn config_dir(&self) -> Result<PathBuf> {
        todo!("windows config_dir")
    }
    fn log_dir(&self) -> Result<PathBuf> {
        todo!("windows log_dir")
    }
    fn bin_dir(&self) -> Result<PathBuf> {
        todo!("windows bin_dir")
    }
    fn hosts_file(&self) -> PathBuf {
        PathBuf::from(r"C:\Windows\System32\drivers\etc\hosts")
    }
}

pub struct WindowsDns;
impl DnsManager for WindowsDns {
    fn resolver_path(&self, _tld: &str) -> PathBuf {
        todo!("windows DNS — no /etc/resolver equivalent")
    }
    fn resolver_contents(&self, _port: u16) -> String {
        todo!("windows DNS")
    }
    fn install_command(&self, _tld: &str, _port: u16) -> String {
        todo!("windows DNS")
    }
    fn uninstall_command(&self, _tlds: &[String]) -> String {
        todo!("windows DNS")
    }
}

pub struct WindowsCertTrust;
impl CertTrustManager for WindowsCertTrust {
    fn trust_ca(&self, _ca_cert_path: &Path) -> Result<()> {
        todo!("windows certutil -addstore Root")
    }
    fn untrust_ca(&self, _ca_cert_path: &Path) -> Result<()> {
        todo!("windows certutil -delstore Root")
    }
}

pub struct WindowsPrivileges;
impl PrivilegeManager for WindowsPrivileges {
    fn run_privileged(&self, _script: &str) -> Result<String> {
        todo!("windows UAC elevation (runas / ShellExecute 'runas')")
    }
}

pub struct WindowsSupervisor;
impl ProcessSupervisor for WindowsSupervisor {
    fn spawn(&self, _program: &Path, _args: &[String]) -> Result<Child> {
        todo!("windows spawn")
    }
    fn spawn_logged(&self, _program: &Path, _args: &[String], _log_path: &Path) -> Result<Child> {
        todo!("windows spawn_logged")
    }
    fn stop(&self, _pid: u32) -> Result<()> {
        todo!("windows stop")
    }
}

pub struct WindowsAutostart;
impl AutostartManager for WindowsAutostart {
    fn enable(&self) -> Result<()> {
        todo!("windows service / task scheduler")
    }
    fn disable(&self) -> Result<()> {
        todo!("windows service / task scheduler")
    }
    fn is_enabled(&self) -> Result<bool> {
        todo!("windows service / task scheduler")
    }
}

pub struct WindowsPermissions;
impl PermissionManager for WindowsPermissions {
    fn set_executable(&self, _path: &Path) -> Result<()> {
        // No-op on Windows (executability is by extension), but kept for parity.
        todo!("windows ACLs")
    }
    fn set_private(&self, _path: &Path) -> Result<()> {
        todo!("windows ACLs (owner-only)")
    }
}

pub struct WindowsShell;
impl ShellRunner for WindowsShell {
    fn run(&self, _command: &str, _args: &[String]) -> Result<String> {
        todo!("windows PowerShell runner")
    }
    fn open(&self, _target: &str) -> Result<()> {
        todo!("windows shell open")
    }
    fn reveal(&self, _path: &str) -> Result<()> {
        todo!("windows shell reveal")
    }
}

pub struct WindowsBinaryProvider;
impl BinaryProvider for WindowsBinaryProvider {
    fn arch(&self) -> Arch {
        Arch::X86_64
    }
    fn prepare_binary(&self, _path: &Path) -> Result<()> {
        todo!("windows binary prepare (no codesign needed)")
    }
}

pub struct WindowsEdge;
impl EdgeSupervisor for WindowsEdge {
    fn is_installed(&self) -> bool {
        todo!("windows edge supervisor (Windows Service KeepAlive)")
    }
    fn is_enabled(&self) -> bool {
        todo!("windows edge supervisor")
    }
    fn plist_path(&self) -> PathBuf {
        todo!("windows edge supervisor")
    }
    fn wrapper_path(&self) -> PathBuf {
        todo!("windows edge supervisor")
    }
    fn daemon_binary_path(&self) -> PathBuf {
        todo!("windows edge supervisor")
    }
    fn plist_contents(&self, _wrapper: &Path, _start_log: &Path) -> String {
        todo!("windows edge supervisor")
    }
    fn wrapper_contents(
        &self,
        _caddy_bin: &Path,
        _caddyfile: &Path,
        _admin_sock: &Path,
        _appdata: &Path,
    ) -> String {
        todo!("windows edge supervisor")
    }
    fn install_command(
        &self,
        _src_caddy: &Path,
        _staged_wrapper: &Path,
        _staged_plist: &Path,
    ) -> String {
        todo!("windows edge supervisor")
    }
    fn start_command(&self) -> String {
        todo!("windows edge supervisor")
    }
    fn stop_command(&self) -> String {
        todo!("windows edge supervisor")
    }
    fn uninstall_command(&self) -> String {
        todo!("windows edge supervisor")
    }
}

pub struct WindowsDnsAgent;
impl DnsAgentManager for WindowsDnsAgent {
    fn is_installed(&self) -> bool {
        todo!("windows dns agent")
    }
    fn plist_path(&self) -> Result<PathBuf> {
        todo!("windows dns agent")
    }
    fn plist_contents(&self, _exe: &Path, _log: &Path) -> String {
        todo!("windows dns agent")
    }
    fn install(&self, _exe: &Path, _log: &Path) -> Result<()> {
        todo!("windows dns agent")
    }
    fn kickstart(&self) -> Result<()> {
        todo!("windows dns agent")
    }
    fn uninstall(&self) -> Result<()> {
        todo!("windows dns agent")
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
}

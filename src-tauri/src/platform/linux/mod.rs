//! Linux implementations — Phase 4 (same era as Windows; this said "Phase 5"
//! until 21 Aug 2026, which is two numbers for one era). Stubs only: every method is `todo!()`.
//! Filling these in is the entire Linux port; `core/` does not change.
#![allow(dead_code)]

use crate::error::Result;
use crate::platform::traits::*;
use std::path::{Path, PathBuf};
use std::process::Child;

pub struct LinuxPaths;
impl Paths for LinuxPaths {
    fn app_data_dir(&self) -> Result<PathBuf> {
        todo!("linux app_data_dir")
    }
    fn config_dir(&self) -> Result<PathBuf> {
        todo!("linux config_dir")
    }
    fn log_dir(&self) -> Result<PathBuf> {
        todo!("linux log_dir")
    }
    fn bin_dir(&self) -> Result<PathBuf> {
        todo!("linux bin_dir")
    }
    fn hosts_file(&self) -> PathBuf {
        PathBuf::from("/etc/hosts")
    }
}

pub struct LinuxDns;
impl DnsManager for LinuxDns {
    fn route_label(&self, _tld: &str) -> String {
        todo!("linux systemd-resolved / dnsmasq")
    }
    fn route_contents(&self, _port: u16) -> String {
        todo!("linux DNS")
    }
    fn route_owner(&self, _tld: &str, _port: u16) -> ResolverOwner {
        todo!("linux DNS")
    }
    fn our_route_tlds(&self, _port: u16) -> Vec<String> {
        todo!("linux DNS")
    }
    fn foreign_route_tlds(&self, _port: u16) -> Vec<String> {
        todo!("linux DNS")
    }
    fn install_command(&self, _tld: &str, _port: u16) -> String {
        todo!("linux DNS")
    }
    fn uninstall_command(&self, _tlds: &[String]) -> String {
        todo!("linux DNS")
    }
    fn restore_command(&self, _restores: &[(String, PathBuf)]) -> String {
        todo!("linux DNS")
    }
}

pub struct LinuxCertTrust;
impl CertTrustManager for LinuxCertTrust {
    fn trust_ca(&self, _ca_cert_path: &Path) -> Result<()> {
        todo!("linux update-ca-certificates (+ NSS for Firefox)")
    }
    fn untrust_ca(&self, _ca_cert_path: &Path) -> Result<()> {
        todo!("linux untrust")
    }
}

pub struct LinuxPrivileges;
impl PrivilegeManager for LinuxPrivileges {
    fn run_privileged(
        &self,
        _script: &str,
        _reason: &crate::platform::traits::PromptReason,
    ) -> Result<String> {
        todo!("linux pkexec / sudo elevation")
    }
}

pub struct LinuxSupervisor;
impl ProcessSupervisor for LinuxSupervisor {
    fn spawn(&self, _program: &Path, _args: &[String]) -> Result<Child> {
        todo!("linux spawn")
    }
    fn spawn_logged(&self, _program: &Path, _args: &[String], _log_path: &Path) -> Result<Child> {
        todo!("linux spawn_logged")
    }
    fn stop(&self, _pid: u32) -> Result<()> {
        todo!("linux stop")
    }
}

pub struct LinuxAutostart;
impl AutostartManager for LinuxAutostart {
    fn enable(&self) -> Result<()> {
        todo!("linux systemd user unit")
    }
    fn disable(&self) -> Result<()> {
        todo!("linux systemd user unit")
    }
    fn is_enabled(&self) -> Result<bool> {
        todo!("linux systemd user unit")
    }
}

pub struct LinuxPermissions;
impl PermissionManager for LinuxPermissions {
    fn set_executable(&self, _path: &Path) -> Result<()> {
        todo!("linux chmod")
    }
    fn set_private(&self, _path: &Path) -> Result<()> {
        todo!("linux chmod 0600")
    }
    fn write_private(&self, _path: &Path, _contents: &[u8]) -> Result<()> {
        todo!("linux owner-only create (OpenOptions mode 0600)")
    }
}

pub struct LinuxShell;
impl ShellRunner for LinuxShell {
    fn run(&self, _command: &str, _args: &[String]) -> Result<String> {
        todo!("linux shell runner")
    }
    fn open(&self, _target: &str) -> Result<()> {
        todo!("linux shell open")
    }
    fn reveal(&self, _path: &str) -> Result<()> {
        todo!("linux shell reveal")
    }
}

pub struct LinuxBinaryProvider;
impl BinaryProvider for LinuxBinaryProvider {
    fn arch(&self) -> Arch {
        if cfg!(target_arch = "aarch64") {
            Arch::Arm64
        } else {
            Arch::X86_64
        }
    }
    fn prepare_binary(&self, _path: &Path) -> Result<()> {
        todo!("linux binary prepare (chmod handled via PermissionManager)")
    }
    fn prepare_binary_tree(&self, _root: &Path) -> Result<()> {
        todo!("linux bundle-tree prepare (patchelf RUNPATH $ORIGIN relink)")
    }
}

pub struct LinuxEdge;
impl EdgeSupervisor for LinuxEdge {
    fn is_installed(&self) -> bool {
        todo!("linux edge supervisor (systemd unit Restart=always)")
    }
    fn is_enabled(&self) -> bool {
        todo!("linux edge supervisor")
    }
    fn plist_path(&self) -> PathBuf {
        todo!("linux edge supervisor")
    }
    fn wrapper_path(&self) -> PathBuf {
        todo!("linux edge supervisor")
    }
    fn daemon_binary_path(&self) -> PathBuf {
        todo!("linux edge supervisor")
    }
    fn plist_contents(&self, _wrapper: &Path, _start_log: &Path) -> String {
        todo!("linux edge supervisor")
    }
    fn wrapper_contents(
        &self,
        _caddy_bin: &Path,
        _caddyfile: &Path,
        _admin_sock: &Path,
        _appdata: &Path,
    ) -> String {
        todo!("linux edge supervisor")
    }
    fn install_command(
        &self,
        _src_caddy: &Path,
        _staged_wrapper: &Path,
        _staged_plist: &Path,
    ) -> String {
        todo!("linux edge supervisor")
    }
    fn start_command(&self) -> String {
        todo!("linux edge supervisor")
    }
    fn stop_command(&self) -> String {
        todo!("linux edge supervisor")
    }
    fn uninstall_command(&self) -> String {
        todo!("linux edge supervisor")
    }
}

pub struct LinuxDnsAgent;
impl DnsAgentManager for LinuxDnsAgent {
    fn is_installed(&self) -> bool {
        todo!("linux dns agent")
    }
    fn definition_path(&self) -> Result<PathBuf> {
        todo!("linux dns agent")
    }
    fn definition_contents(&self, _exe: &Path, _log: &Path) -> String {
        todo!("linux dns agent")
    }
    fn install(&self, _exe: &Path, _log: &Path) -> Result<()> {
        todo!("linux dns agent")
    }
    fn kickstart(&self) -> Result<()> {
        todo!("linux dns agent")
    }
    fn uninstall(&self) -> Result<()> {
        todo!("linux dns agent")
    }
}
pub struct LinuxAppBundle;
impl AppBundle for LinuxAppBundle {
    fn facts(&self, _exe: &Path) -> Result<BundleFacts> {
        todo!("linux app bundle facts — self-update is macOS-only today")
    }
    fn stage(
        &self,
        _facts: &BundleFacts,
        _archive: &Path,
        _expect: &StagedExpect,
    ) -> Result<StagedBundle> {
        todo!("linux stage a replacement bundle")
    }
    fn swap(
        &self,
        _installed: &Path,
        _staged: &StagedBundle,
    ) -> std::result::Result<SwapReceipt, SwapFailure> {
        todo!("linux swap the bundle")
    }
    fn spawn_relauncher(&self, _bundle: &Path) -> Result<()> {
        todo!("linux relauncher")
    }
    fn sweep_leftovers(
        &self,
        _parent: &Path,
        _my_version: &str,
        _delete_previous: bool,
    ) -> Result<Vec<Leftover>> {
        todo!("linux sweep update leftovers")
    }
}
pub struct LinuxLocalIpc;
impl LocalIpc for LinuxLocalIpc {
    fn connect(
        &self,
        _path: &Path,
        _read_timeout: Option<std::time::Duration>,
    ) -> std::io::Result<Box<dyn std::io::Read + Send>> {
        todo!("linux local IPC — a unix-domain socket, as on macOS")
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

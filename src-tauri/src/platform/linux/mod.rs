//! Linux implementations — Phase 5. Stubs only: every method is `todo!()`.
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
    fn configure_resolver(&self, _port: u16) -> Result<()> {
        todo!("linux systemd-resolved / dnsmasq")
    }
    fn teardown_resolver(&self) -> Result<()> {
        todo!("linux DNS teardown")
    }
}

pub struct LinuxCertTrust;
impl CertTrustManager for LinuxCertTrust {
    fn trust_ca(&self, _ca_cert_path: &Path) -> Result<()> {
        todo!("linux update-ca-certificates + NSS")
    }
    fn untrust_ca(&self, _ca_cert_path: &Path) -> Result<()> {
        todo!("linux untrust")
    }
}

pub struct LinuxPrivileges;
impl PrivilegeManager for LinuxPrivileges {
    fn ensure_port_privileges(&self) -> Result<()> {
        todo!("linux setcap / sudo")
    }
}

pub struct LinuxSupervisor;
impl ProcessSupervisor for LinuxSupervisor {
    fn spawn(&self, _program: &Path, _args: &[String]) -> Result<Child> {
        todo!("linux spawn")
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
}

pub struct LinuxPermissions;
impl PermissionManager for LinuxPermissions {
    fn set_executable(&self, _path: &Path) -> Result<()> {
        todo!("linux chmod")
    }
}

pub struct LinuxShell;
impl ShellRunner for LinuxShell {
    fn run(&self, _command: &str, _args: &[String]) -> Result<String> {
        todo!("linux shell runner")
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
    fn resolve(&self, _name: &str, _version: &str) -> Result<PathBuf> {
        todo!("linux binary provider")
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
}

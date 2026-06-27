//! macOS implementations of the platform traits (Phase 1 target OS).
//!
//! Trivial, dependency-free pieces (paths, permissions, process spawn, shell)
//! are implemented now. Pieces that need later Phase 1 tasks (DNS resolver,
//! CA trust, privileged port binding, autostart, binary download) are wired up
//! as `todo!()` so the architecture is complete and `cargo check` passes.

use crate::error::{Error, Result};
use crate::platform::traits::*;
use std::path::{Path, PathBuf};
use std::process::Child;

const APP_QUALIFIER: &str = "dev";
const APP_ORG: &str = "rexenv";
const APP_NAME: &str = "rexenv";

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
}

pub struct MacosDns;
impl DnsManager for MacosDns {
    fn configure_resolver(&self, _port: u16) -> Result<()> {
        // Task 2.2: write /etc/resolver/test pointing at the embedded resolver.
        todo!("macOS /etc/resolver/test setup")
    }
    fn teardown_resolver(&self) -> Result<()> {
        todo!("macOS /etc/resolver/test teardown")
    }
}

pub struct MacosCertTrust;
impl CertTrustManager for MacosCertTrust {
    fn trust_ca(&self, _ca_cert_path: &Path) -> Result<()> {
        // Task 3.3: `security add-trusted-cert` into the login keychain.
        todo!("macOS keychain trust via `security`")
    }
    fn untrust_ca(&self, _ca_cert_path: &Path) -> Result<()> {
        todo!("macOS keychain untrust")
    }
}

pub struct MacosPrivileges;

impl MacosPrivileges {
    /// Build the `osascript -e` program that runs `script` as admin. Factored
    /// out so escaping can be unit-tested without triggering the auth prompt.
    fn osascript_program(script: &str) -> String {
        // Escape for an AppleScript double-quoted string literal.
        let escaped = script.replace('\\', "\\\\").replace('"', "\\\"");
        format!("do shell script \"{escaped}\" with administrator privileges")
    }
}

impl PrivilegeManager for MacosPrivileges {
    fn run_privileged(&self, script: &str) -> Result<String> {
        // `do shell script … with administrator privileges` shows one macOS
        // auth dialog and runs the script as root via /bin/sh.
        let program = Self::osascript_program(script);
        let out = std::process::Command::new("osascript")
            .arg("-e")
            .arg(&program)
            .output()?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
        } else {
            Err(Error::Other(format!(
                "privileged operation failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )))
        }
    }
}

pub struct MacosSupervisor;
impl ProcessSupervisor for MacosSupervisor {
    fn spawn(&self, program: &Path, args: &[String]) -> Result<Child> {
        Ok(std::process::Command::new(program).args(args).spawn()?)
    }
    fn stop(&self, pid: u32) -> Result<()> {
        // SIGTERM via kill(2). Refined with a proper supervisor in §4–6.
        let status = std::process::Command::new("kill")
            .arg(pid.to_string())
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::Other(format!("failed to stop pid {pid}")))
        }
    }
}

pub struct MacosAutostart;
impl AutostartManager for MacosAutostart {
    fn enable(&self) -> Result<()> {
        // launchd .plist (LaunchAgents).
        todo!("macOS launchd autostart")
    }
    fn disable(&self) -> Result<()> {
        todo!("macOS launchd autostart disable")
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
}

pub struct MacosShell;
impl ShellRunner for MacosShell {
    fn run(&self, command: &str, args: &[String]) -> Result<String> {
        let out = std::process::Command::new(command).args(args).output()?;
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

pub struct MacosBinaryProvider;
impl BinaryProvider for MacosBinaryProvider {
    fn arch(&self) -> Arch {
        if cfg!(target_arch = "aarch64") {
            Arch::Arm64
        } else {
            Arch::X86_64
        }
    }
    fn resolve(&self, _name: &str, _version: &str) -> Result<PathBuf> {
        // Task 4.1+: download from the binary manifest, verify checksum, extract.
        todo!("macOS binary download/extract via manifest")
    }
}

/// Aggregate macOS platform, handed to `core/` as `&dyn Platform`.
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osascript_program_wraps_and_escapes() {
        let program = MacosPrivileges::osascript_program(r#"echo "hi" \ there"#);
        assert!(program.starts_with("do shell script \""));
        assert!(program.ends_with("\" with administrator privileges"));
        // Quotes and backslashes are escaped for the AppleScript string literal.
        assert!(program.contains(r#"echo \"hi\" \\ there"#));
    }

    #[test]
    fn osascript_program_handles_multi_command_batch() {
        // Batching: multiple privileged commands joined into one script => one prompt.
        let script = "mkdir -p /etc/resolver\ncp /tmp/test /etc/resolver/test";
        let program = MacosPrivileges::osascript_program(script);
        assert!(program.contains("mkdir -p /etc/resolver"));
        assert!(program.contains("cp /tmp/test /etc/resolver/test"));
    }
}

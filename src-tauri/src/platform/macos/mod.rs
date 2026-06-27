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
    fn resolver_path(&self) -> PathBuf {
        // macOS reads /etc/resolver/<domain>; our local TLD is `test`.
        PathBuf::from("/etc/resolver/test")
    }

    fn resolver_contents(&self, port: u16) -> String {
        // macOS resolver(5): send `.test` to our loopback resolver on `port`.
        format!("nameserver 127.0.0.1\nport {port}\n")
    }

    fn install_command(&self, port: u16) -> String {
        let path = self.resolver_path();
        let dir = path
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "/etc/resolver".into());
        // Use printf so the content survives the AppleScript→sh escaping chain
        // (literal newlines in the file are written as \n for printf).
        let printf_arg = self.resolver_contents(port).replace('\n', "\\n");
        format!(
            "mkdir -p {dir} && printf '{printf_arg}' > {}",
            path.display()
        )
    }

    fn uninstall_command(&self) -> String {
        format!("rm -f {}", self.resolver_path().display())
    }
}

pub struct MacosCertTrust;

/// Single-quote a path for safe use in the /bin/sh command (app-data paths
/// contain spaces). Paths won't contain single quotes in practice.
fn sh_quote(path: &Path) -> String {
    format!("'{}'", path.display())
}

impl CertTrustManager for MacosCertTrust {
    fn trust_command(&self, ca_cert_path: &Path) -> String {
        // Add as a trusted root in the System keychain (system-wide trust for
        // Safari/Chrome). Requires root → run via PrivilegeManager.
        format!(
            "security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain {}",
            sh_quote(ca_cert_path)
        )
    }
    fn untrust_command(&self, ca_cert_path: &Path) -> String {
        format!("security remove-trusted-cert -d {}", sh_quote(ca_cert_path))
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
    fn set_private(&self, path: &Path) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
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

    #[test]
    fn dns_resolver_path_and_contents() {
        let dns = MacosDns;
        assert_eq!(dns.resolver_path(), PathBuf::from("/etc/resolver/test"));
        assert_eq!(
            dns.resolver_contents(15353),
            "nameserver 127.0.0.1\nport 15353\n"
        );
    }

    #[test]
    fn dns_install_command_creates_dir_and_writes_file() {
        let dns = MacosDns;
        let cmd = dns.install_command(15353);
        assert!(cmd.contains("mkdir -p /etc/resolver"));
        // printf carries the file content with escaped newlines for sh.
        assert!(cmd.contains(r"printf 'nameserver 127.0.0.1\nport 15353\n'"));
        assert!(cmd.contains("> /etc/resolver/test"));
    }

    #[test]
    fn dns_uninstall_command_removes_file() {
        assert_eq!(MacosDns.uninstall_command(), "rm -f /etc/resolver/test");
    }

    #[test]
    fn cert_trust_command_targets_system_keychain_as_root() {
        let cmd = MacosCertTrust.trust_command(Path::new("/tmp/My CA/rexenv-ca.pem"));
        assert!(cmd.starts_with("security add-trusted-cert -d -r trustRoot -k "));
        assert!(cmd.contains("/Library/Keychains/System.keychain"));
        // Path is single-quoted (handles the space in app-data paths).
        assert!(cmd.ends_with("'/tmp/My CA/rexenv-ca.pem'"));
    }

    #[test]
    fn cert_untrust_command_removes_trust() {
        let cmd = MacosCertTrust.untrust_command(Path::new("/tmp/ca.pem"));
        assert_eq!(cmd, "security remove-trusted-cert -d '/tmp/ca.pem'");
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
}

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
use std::time::Duration;

// The ONE canonical reverse-DNS identity (release task 1.1). It must match the
// `identifier` in tauri.conf.json (the bundle id / signing id), the app-data
// namespace below, and the launchd autostart label. The drift-guard tests at the
// bottom of this file fail the build if any of these diverge.
pub const APP_IDENTIFIER: &str = "dev.rexenv.rexenv";

// app-data namespace parts: `directories::ProjectDirs::from(qualifier, org, name)`
// composes these to `<qualifier>.<org>.<name>` = APP_IDENTIFIER on macOS.
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
        // (literal newlines in the file are written as \n for printf). Then flush
        // the DNS cache so macOS picks up the new resolver file immediately.
        let printf_arg = self.resolver_contents(port).replace('\n', "\\n");
        format!(
            "mkdir -p {dir} && printf '{printf_arg}' > {} \
             && dscacheutil -flushcache && killall -HUP mDNSResponder",
            path.display()
        )
    }

    fn uninstall_command(&self) -> String {
        // Remove the resolver file AND flush the DNS cache, so `.test` stops
        // resolving immediately (mirror of install_command's flush — §3.2).
        format!(
            "rm -f {} && dscacheutil -flushcache && killall -HUP mDNSResponder",
            self.resolver_path().display()
        )
    }
}

pub struct MacosCertTrust;

impl MacosCertTrust {
    /// Path to the user's login keychain. Targeting it explicitly is required:
    /// without `-k`, `security add-trusted-cert` references whichever keychain
    /// already holds the cert, which is non-deterministic.
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

    /// `security` args to trust the CA as a root in the user login keychain.
    /// No `-d`/System keychain ⇒ no root; `security` shows its own native auth
    /// dialog. Factored out so the arg construction is unit-testable.
    fn trust_args(ca_cert_path: &Path) -> Vec<String> {
        vec![
            "add-trusted-cert".into(),
            "-r".into(),
            "trustRoot".into(),
            "-k".into(),
            Self::login_keychain(),
            ca_cert_path.display().to_string(),
        ]
    }
    fn untrust_args(ca_cert_path: &Path) -> Vec<String> {
        vec![
            "remove-trusted-cert".into(),
            ca_cert_path.display().to_string(),
        ]
    }

    fn run_security(args: &[String]) -> Result<()> {
        let out = std::process::Command::new("security").args(args).output()?;
        if out.status.success() {
            Ok(())
        } else {
            Err(Error::Other(format!(
                "security {} failed: {}",
                args.first().map(String::as_str).unwrap_or(""),
                String::from_utf8_lossy(&out.stderr).trim()
            )))
        }
    }
}

impl CertTrustManager for MacosCertTrust {
    fn trust_ca(&self, ca_cert_path: &Path) -> Result<()> {
        Self::run_security(&Self::trust_args(ca_cert_path))
    }
    fn untrust_ca(&self, ca_cert_path: &Path) -> Result<()> {
        Self::run_security(&Self::untrust_args(ca_cert_path))
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
    fn spawn(&self, program: &Path, args: &[String]) -> Result<Child> {
        Ok(std::process::Command::new(program).args(args).spawn()?)
    }
    fn spawn_logged(&self, program: &Path, args: &[String], log_path: &Path) -> Result<Child> {
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
            .stdout(out)
            .stderr(err)
            .spawn()?)
    }
    fn stop(&self, pid: u32) -> Result<()> {
        stop_pid(pid, STOP_GRACE_TRIES, STOP_POLL_INTERVAL)
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
             \t</array>\n\
             \t<key>RunAtLoad</key>\n\
             \t<true/>\n\
             \t<key>ProcessType</key>\n\
             \t<string>Interactive</string>\n\
             </dict>\n\
             </plist>\n",
            program = program.display()
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

    /// Rewrite any non-system (e.g. Homebrew) dylib dependencies to macOS system
    /// libs so the binary runs without Homebrew. Errors if a dep has no system
    /// equivalent (so we never ship a binary that will fail to load).
    fn relink_to_system_libs(path: &Path) -> Result<()> {
        let out = std::process::Command::new("otool")
            .arg("-L")
            .arg(path)
            .output()?;
        if !out.status.success() {
            return Err(Error::Other(format!(
                "otool -L failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        let listing = String::from_utf8_lossy(&out.stdout);
        for line in listing.lines().skip(1) {
            let dep = match line.trim().split_whitespace().next() {
                Some(d) => d,
                None => continue,
            };
            if dep.starts_with("/usr/lib/") || dep.starts_with("/System/") {
                continue; // already a system lib
            }
            let target = Self::system_lib_for(dep).ok_or_else(|| {
                Error::Other(format!("no macOS system lib for dependency {dep}"))
            })?;
            let st = std::process::Command::new("install_name_tool")
                .arg("-change")
                .arg(dep)
                .arg(&target)
                .arg(path)
                .output()?;
            if !st.status.success() {
                return Err(Error::Other(format!(
                    "install_name_tool -change {dep} {target} failed: {}",
                    String::from_utf8_lossy(&st.stderr).trim()
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
                "codesign failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )))
        }
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
        // flushes the DNS cache so the new resolver file takes effect at once.
        assert!(cmd.contains("dscacheutil -flushcache"));
        assert!(cmd.contains("killall -HUP mDNSResponder"));
    }

    #[test]
    fn dns_uninstall_command_removes_file_and_flushes_cache() {
        let cmd = MacosDns.uninstall_command();
        assert!(cmd.contains("rm -f /etc/resolver/test"));
        // Flush so `.test` stops resolving immediately after teardown (§3.2).
        assert!(cmd.contains("dscacheutil -flushcache"));
        assert!(cmd.contains("killall -HUP mDNSResponder"));
    }

    #[test]
    fn autostart_plist_is_a_login_launchagent_for_the_program() {
        let plist = MacosAutostart::plist_contents(Path::new("/Applications/rexenv.app"));
        assert!(plist.contains("<string>dev.rexenv.rexenv</string>"));
        assert!(plist.contains("<string>/Applications/rexenv.app</string>"));
        // Runs at GUI login.
        assert!(plist.contains("<key>RunAtLoad</key>"));
        assert!(plist.contains("<true/>"));
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
    fn cert_trust_args_use_login_keychain_no_root() {
        let args = MacosCertTrust::trust_args(Path::new("/tmp/My CA/rexenv-ca.pem"));
        assert_eq!(&args[0..4], &["add-trusted-cert", "-r", "trustRoot", "-k"]);
        // Targets the login keychain explicitly; never the System keychain / -d.
        assert!(args[4].ends_with("login.keychain-db"));
        assert_eq!(args[5], "/tmp/My CA/rexenv-ca.pem");
        assert!(!args.iter().any(|a| a == "-d"));
        assert!(!args.iter().any(|a| a.contains("System.keychain")));
    }

    #[test]
    fn cert_untrust_args_remove_trust() {
        let args = MacosCertTrust::untrust_args(Path::new("/tmp/ca.pem"));
        assert_eq!(args, vec!["remove-trusted-cert", "/tmp/ca.pem"]);
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

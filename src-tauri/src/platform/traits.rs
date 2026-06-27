//! Platform abstraction traits — the whole cross-platform strategy.
//!
//! Every OS difference is expressed here as a trait. `core/` depends ONLY on
//! these traits, never on OS-specific code. Per-OS implementations live in
//! `macos/`, `windows/`, `linux/` and are selected in `mod.rs` via
//! `#[cfg(target_os = "...")]`.
//!
//! Phase 1 builds the macOS implementations; Windows and Linux start as
//! `todo!()` stubs. Adding those platforms later means filling stubs — NOT
//! restructuring `core/`.

use crate::error::Result;
use std::path::PathBuf;
use std::process::Child;

/// OS file-system locations (app data, config, logs, downloaded binaries).
/// macOS / Windows / Linux each use different conventions.
pub trait Paths: Send + Sync {
    /// Root directory for all rexenv app state (SQLite db, certs, configs).
    fn app_data_dir(&self) -> Result<PathBuf>;
    /// Directory for generated service configs (nginx/caddy/php-fpm).
    fn config_dir(&self) -> Result<PathBuf>;
    /// Directory for service log files.
    fn log_dir(&self) -> Result<PathBuf>;
    /// Directory where downloaded native binaries are cached.
    fn bin_dir(&self) -> Result<PathBuf>;
    /// The system hosts file (`/etc/hosts` vs Windows path).
    fn hosts_file(&self) -> PathBuf;
}

/// Points the OS resolver at our embedded `hickory-dns` server so `*.test`
/// resolves to `127.0.0.1`. The embedded server is platform-agnostic; only the
/// "point the OS at it" step differs per OS.
///
/// These are pure command BUILDERS (no privilege, no side effects) so the
/// install/remove can be unit-tested without sudo AND batched with other
/// privileged ops into a single `PrivilegeManager` elevation (see core::dns +
/// the batched system-setup step). The actual run goes through `PrivilegeManager`.
pub trait DnsManager: Send + Sync {
    /// Path of the OS resolver file (e.g. `/etc/resolver/test`).
    fn resolver_path(&self) -> PathBuf;
    /// Contents of the resolver file pointing `.test` at our resolver on `port`.
    fn resolver_contents(&self, port: u16) -> String;
    /// Shell command(s) that install the resolver file — run via `PrivilegeManager`.
    fn install_command(&self, port: u16) -> String;
    /// Shell command(s) that remove the resolver file — run via `PrivilegeManager`.
    fn uninstall_command(&self) -> String;
}

/// Installs / removes the local CA in the system (and browser) trust store.
pub trait CertTrustManager: Send + Sync {
    /// Add the CA at `ca_cert_path` to the system trust store as trusted.
    fn trust_ca(&self, ca_cert_path: &std::path::Path) -> Result<()>;
    /// Remove the CA (matched by `ca_cert_path` or its fingerprint).
    fn untrust_ca(&self, ca_cert_path: &std::path::Path) -> Result<()>;
}

/// Runs privileged shell operations behind a single OS authentication prompt.
/// Consumers (`DnsManager`, `CertTrustManager`) build their privileged command
/// strings; the system-setup step batches them into ONE call so the user
/// authenticates only once (e.g. writing `/etc/resolver/test` + trusting the CA
/// together). Also covers privilege needs like binding :80/:443.
pub trait PrivilegeManager: Send + Sync {
    /// Run `script` (a `/bin/sh` script) with administrator privileges, showing
    /// the OS auth prompt once. Returns captured stdout on success.
    fn run_privileged(&self, script: &str) -> Result<String>;
}

/// Spawns and supervises long-running child processes (web servers, php-fpm,
/// databases, the edge router). Spawning is similar across OSes; this wraps it
/// behind a trait so supervision policy stays in one place.
pub trait ProcessSupervisor: Send + Sync {
    /// Spawn `program` with `args`; returns the child handle.
    fn spawn(&self, program: &std::path::Path, args: &[String]) -> Result<Child>;
    /// Stop a previously spawned process by pid.
    fn stop(&self, pid: u32) -> Result<()>;
}

/// Registers / unregisters rexenv (or its services) to start on login/boot.
/// launchd (macOS) vs systemd user unit (Linux) vs Service/Task Scheduler (Win).
pub trait AutostartManager: Send + Sync {
    fn enable(&self) -> Result<()>;
    fn disable(&self) -> Result<()>;
}

/// File permissions. POSIX chmod/chown on Unix; ACLs on Windows.
pub trait PermissionManager: Send + Sync {
    /// Make `path` executable (e.g. a downloaded binary).
    fn set_executable(&self, path: &std::path::Path) -> Result<()>;
}

/// Runs shell commands and backs the built-in terminal. bash/zsh vs PowerShell.
pub trait ShellRunner: Send + Sync {
    /// Run `command` and capture stdout as a string.
    fn run(&self, command: &str, args: &[String]) -> Result<String>;
}

/// Target architecture, used by `BinaryProvider` to pick the right artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    Arm64,
    X86_64,
}

/// Resolves the correct native binary for this OS+arch, downloading and caching
/// it on demand. Source URL, archive format, and extraction differ per OS.
pub trait BinaryProvider: Send + Sync {
    /// The detected CPU architecture for the running machine.
    fn arch(&self) -> Arch;
    /// Path to a ready-to-run binary for `name` at `version`, downloading and
    /// extracting it into `bin_dir` if not already cached.
    fn resolve(&self, name: &str, version: &str) -> Result<PathBuf>;
}

/// Aggregate of every platform capability. `core/` is handed one of these and
/// never names a concrete OS type.
pub trait Platform: Send + Sync {
    fn paths(&self) -> &dyn Paths;
    fn dns(&self) -> &dyn DnsManager;
    fn cert_trust(&self) -> &dyn CertTrustManager;
    fn privileges(&self) -> &dyn PrivilegeManager;
    fn supervisor(&self) -> &dyn ProcessSupervisor;
    fn autostart(&self) -> &dyn AutostartManager;
    fn permissions(&self) -> &dyn PermissionManager;
    fn shell(&self) -> &dyn ShellRunner;
    fn binaries(&self) -> &dyn BinaryProvider;
}

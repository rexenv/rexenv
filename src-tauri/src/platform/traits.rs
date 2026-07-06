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

/// Installs / removes the local CA in the trust store.
///
/// On macOS the trust setting is written to the USER login keychain (no root):
/// `security add-trusted-cert` presents its own native authorization dialog, so
/// this is NOT a `PrivilegeManager` op. (System-keychain trust can't be set from
/// a detached-root osascript session — see the system-setup notes.) These
/// methods execute the trust change and manage that dialog themselves.
pub trait CertTrustManager: Send + Sync {
    /// Trust the CA at `ca_cert_path` as a root (shows a native auth dialog).
    fn trust_ca(&self, ca_cert_path: &std::path::Path) -> Result<()>;
    /// Remove the CA's trust setting.
    fn untrust_ca(&self, ca_cert_path: &std::path::Path) -> Result<()>;
    /// Whether the CA is currently trusted for THIS OS user. Trust is per-user
    /// (macOS: login keychain), so a fresh account needs its own [`trust_ca`]
    /// even when system-wide setup (e.g. the resolver file) already happened —
    /// first-run detection must check this, not just system-wide artifacts.
    /// Default: `false` (conservative — re-offers setup).
    fn is_trusted(&self, _ca_cert_path: &std::path::Path) -> bool {
        false
    }
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
    /// Spawn `program` with `args`; returns the child handle (stdio inherited).
    fn spawn(&self, program: &std::path::Path, args: &[String]) -> Result<Child>;
    /// Spawn `program` with its stdout+stderr redirected (appended) to `log_path`
    /// — used for long-running services so their output is captured to a per-
    /// service log file from the moment they start (Phase 3 log viewer).
    fn spawn_logged(
        &self,
        program: &std::path::Path,
        args: &[String],
        log_path: &std::path::Path,
    ) -> Result<Child>;
    /// Stop a previously spawned process by pid.
    fn stop(&self, pid: u32) -> Result<()>;

    /// PIDs currently listening on TCP `port` whose process command line contains
    /// `owner_marker` — so callers only ever touch their OWN services. Lets the
    /// service manager stop orphaned processes still holding our known ports that
    /// we no longer have a handle for (survivors of an app restart or crash).
    /// Default: none — a platform without this capability relies on tracked handles.
    fn owned_listeners(&self, _port: u16, _owner_marker: &str) -> Vec<u32> {
        Vec::new()
    }

    /// PIDs whose full command line contains `marker` (a substring, typically an
    /// app-data path we own). Unlike `owned_listeners` this doesn't require the
    /// process to hold a port — used to reap a wedged, listener-less service (e.g.
    /// a Caddy edge that lost its sockets) the port/admin paths can't reach.
    /// Default: none.
    fn owned_pids(&self, _marker: &str) -> Vec<u32> {
        Vec::new()
    }

    /// Help for a port-conflict error: who is holding `port` (any process, not
    /// just ours — this is diagnostic, never used to kill anything ourselves)
    /// and a copy-paste shell command the USER can run to terminate the holder
    /// (macOS/Linux: `lsof`+`kill`; Windows: `Get-NetTCPConnection`+`Stop-Process`).
    /// Default: no help — the plain "port in use" error stands on its own.
    fn port_conflict_help(&self, _port: u16, _udp: bool) -> PortConflictHelp {
        PortConflictHelp { holder: None, free_command: None }
    }
}

/// What [`ProcessSupervisor::port_conflict_help`] discovered about a busy port.
#[derive(Debug, Clone, Default)]
pub struct PortConflictHelp {
    /// The listener, if discoverable without privileges — e.g. `nginx (pid 554)`.
    pub holder: Option<String>,
    /// A shell one-liner that stops whatever holds the port (run by the user).
    pub free_command: Option<String>,
}

/// Registers / unregisters rexenv (or its services) to start on login/boot.
/// launchd (macOS) vs systemd user unit (Linux) vs Service/Task Scheduler (Win).
pub trait AutostartManager: Send + Sync {
    fn enable(&self) -> Result<()>;
    fn disable(&self) -> Result<()>;
    /// Whether rexenv is currently registered to start on login.
    fn is_enabled(&self) -> Result<bool>;
}

/// File permissions. POSIX chmod/chown on Unix; ACLs on Windows.
pub trait PermissionManager: Send + Sync {
    /// Make `path` executable (e.g. a downloaded binary).
    fn set_executable(&self, path: &std::path::Path) -> Result<()>;
    /// Restrict `path` to owner-only access (0600 on Unix; ACL on Windows).
    /// Used for private keys (e.g. the local CA key).
    fn set_private(&self, path: &std::path::Path) -> Result<()>;
}

/// Runs shell commands and backs the built-in terminal. bash/zsh vs PowerShell.
pub trait ShellRunner: Send + Sync {
    /// Run `command` and capture stdout as a string. A non-zero exit is an error
    /// carrying the exit status + stderr — never a silent success with empty output.
    fn run(&self, command: &str, args: &[String]) -> Result<String>;

    /// Open a path or URL in the OS default handler (Finder for a folder, the
    /// default browser for an `http(s)` URL). macOS: `open <target>`.
    fn open(&self, target: &str) -> Result<()>;
}

/// Target architecture, used by `BinaryProvider` to pick the right artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    Arm64,
    X86_64,
}

/// OS-specific primitives for provisioning native binaries. The manifest +
/// download + extract + checksum orchestration is platform-agnostic and lives in
/// `core::binaries`, which calls these primitives.
pub trait BinaryProvider: Send + Sync {
    /// The detected CPU architecture for the running machine.
    fn arch(&self) -> Arch;
    /// Prepare a freshly downloaded binary for execution. macOS: ad-hoc
    /// code-sign (`codesign --force --sign -`) + de-quarantine
    /// (`xattr -d com.apple.quarantine`), else Apple Silicon kills it.
    fn prepare_binary(&self, path: &std::path::Path) -> Result<()>;
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

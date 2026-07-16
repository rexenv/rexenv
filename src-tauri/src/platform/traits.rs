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
use std::path::{Path, PathBuf};
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
    /// Where the `rex` CLI symlink is installed on PATH (macOS:
    /// `/usr/local/bin/rex` — the Docker/Herd convention). Default:
    /// unsupported, so platforms without a CLI story fail honestly.
    fn cli_symlink_path(&self) -> Result<PathBuf> {
        Err(crate::error::Error::Unsupported("CLI PATH install"))
    }
}

/// Points the OS resolver at our embedded `hickory-dns` server so `*.<tld>`
/// resolves to `127.0.0.1`. The embedded server is platform-agnostic and
/// answers ANY name; WHICH TLDs reach it is scoped per-OS by one resolver
/// file per TLD (macOS: `/etc/resolver/<tld>`) — so adding a TLD never
/// restarts the DNS server, and multiple TLDs coexist.
///
/// These are pure command BUILDERS (no privilege, no side effects) so the
/// install/remove can be unit-tested without sudo AND batched with other
/// privileged ops into a single `PrivilegeManager` elevation (see core::dns +
/// the batched system-setup step). The actual run goes through `PrivilegeManager`.
pub trait DnsManager: Send + Sync {
    /// Path of the OS resolver file for `tld` (e.g. `/etc/resolver/test`).
    /// `tld` is a bare label already validated by `core::tld` — callers never
    /// pass user input here directly.
    fn resolver_path(&self, tld: &str) -> PathBuf;
    /// Contents of a resolver file pointing a TLD at our resolver on `port`.
    /// TLD-independent (the TLD lives in the file NAME) — this exact content
    /// is also the ownership signature used to enumerate OUR resolver files.
    fn resolver_contents(&self, port: u16) -> String;
    /// Shell command(s) that install the resolver file for `tld` — run via
    /// `PrivilegeManager`.
    fn install_command(&self, tld: &str, port: u16) -> String;
    /// Shell command(s) that remove the resolver files for `tlds` (one batch,
    /// one cache flush) — run via `PrivilegeManager`.
    fn uninstall_command(&self, tlds: &[String]) -> String;
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
    /// This user's Firefox profiles root (the dir holding `profiles.ini`), or
    /// `None` when Firefox never ran / isn't supported on this OS yet. Firefox
    /// keeps its OWN trust store (NSS) — `core::firefox` uses this to force the
    /// pref that imports OS-trust-store roots (our CA) per profile. Only the
    /// LOCATION is per-OS; the profile work is platform-agnostic.
    fn firefox_profiles_root(&self) -> Option<std::path::PathBuf> {
        None
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
    /// Like [`Self::spawn_logged`] but with extra process ENVIRONMENT variables
    /// (inherited env + `env`). Used for per-site FrankenPHP backends (§1.6):
    /// one process per site, so process env IS per-site env — it's what makes
    /// `getenv()`/`$_ENV` work there (its SAPI doesn't consult request params
    /// for `getenv()`). Default: delegates when `env` is empty, ERRORS otherwise
    /// — a platform without an override must not silently drop env vars.
    fn spawn_logged_env(
        &self,
        program: &std::path::Path,
        args: &[String],
        log_path: &std::path::Path,
        env: &[(String, String)],
    ) -> Result<Child> {
        if env.is_empty() {
            return self.spawn_logged(program, args, log_path);
        }
        Err(crate::error::Error::Other(
            "per-process env vars are not supported on this platform yet".into(),
        ))
    }
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

    /// The MASTER among [`Self::owned_listeners`] on `port` — the pid to ADOPT
    /// (and later signal). Workers share the master's listen socket (httpd,
    /// nginx, php-fpm, mysqld all show the whole tree in a listener query), so
    /// "who listens" is a SET and adoption must pick its root. The old
    /// lowest-pid heuristic ("masters fork first") is FALSE under worker churn
    /// plus pid recycling — observed live with Apache: recycled worker 71063
    /// sat below master 95274, so the worker got adopted, Stop-all killed the
    /// worker, and the surviving master blocked the next start's port gate.
    /// Default: platforms without parent info degrade to lowest-pid.
    fn owned_master(&self, port: u16, owner_marker: &str) -> Option<u32> {
        self.owned_listeners(port, owner_marker).into_iter().min()
    }

    /// PIDs whose full command line contains `marker` (a substring, typically an
    /// app-data path we own). Unlike `owned_listeners` this doesn't require the
    /// process to hold a port — used to reap a wedged, listener-less service (e.g.
    /// a Caddy edge that lost its sockets) the port/admin paths can't reach.
    /// Default: none.
    fn owned_pids(&self, _marker: &str) -> Vec<u32> {
        Vec::new()
    }

    /// CPU% (per-core, Activity-Monitor style) + RAM MB for `pid`, readable
    /// ACROSS users — the root-owned Caddy edge is invisible to sysinfo's
    /// task-level read (`proc_pidinfo` is same-user only on macOS) but its
    /// `kinfo_proc` accounting is world-readable via `ps`. Coarser than the
    /// sysinfo path (ps CPU% is a decaying average, not an interval delta) —
    /// used only as the fallback when the primary read yields nothing.
    /// Default: unavailable.
    fn resource_usage(&self, _pid: u32) -> Option<(f32, u64)> {
        None
    }

    /// Help for a port-conflict error: who is holding `port` (any process, not
    /// just ours — this is diagnostic, never used to kill anything ourselves)
    /// and a copy-paste shell command the USER can run to terminate the holder
    /// (macOS/Linux: `lsof`+`kill`; Windows: `Get-NetTCPConnection`+`Stop-Process`).
    /// Default: no help — the plain "port in use" error stands on its own.
    fn port_conflict_help(&self, _port: u16, _udp: bool) -> PortConflictHelp {
        PortConflictHelp::default()
    }
}

/// What [`ProcessSupervisor::port_conflict_help`] discovered about a busy port.
#[derive(Debug, Clone, Default)]
pub struct PortConflictHelp {
    /// The listener, human-attributed, if discoverable without privileges —
    /// e.g. `Herd (nginx, pid 554)` or `nginx (pid 554, /opt/homebrew/bin/nginx)`.
    pub holder: Option<String>,
    /// The OWNING APPLICATION when one is identifiable (e.g. "Herd") — lets
    /// messages say "quit Herd" instead of a generic "quit that app". `None`
    /// for bare binaries (Homebrew nginx) even when `holder` is set.
    pub app: Option<String>,
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

    /// Reveal a file in the OS file manager with the file selected (not opened).
    /// macOS: `open -R <path>`.
    fn reveal(&self, path: &str) -> Result<()>;

    /// Code editors installed on this machine ("Open in editor", §1.3).
    /// Default: none — Windows/Linux fill this in Phase 4.
    fn detect_editors(&self) -> Vec<EditorApp> {
        Vec::new()
    }

    /// Open `path` as a PROJECT in the editor with [`EditorApp::id`] (macOS:
    /// `open -a <app> <path>` — every mainstream editor treats a folder argument
    /// as a project/workspace). Errors if the editor is not installed.
    fn open_in_editor(&self, _editor_id: &str, _path: &str) -> Result<()> {
        Err(crate::error::Error::Unsupported("open_in_editor"))
    }
}

/// A detected code editor (`ShellRunner::detect_editors`).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorApp {
    /// Stable key stored as the `preferred_editor` setting (e.g. "vscode").
    pub id: String,
    /// Display name (e.g. "Visual Studio Code").
    pub name: String,
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
    /// Prepare a freshly extracted multi-dylib TREE (a Homebrew-bottle bundle,
    /// `core::binaries::resolve_bundle`) for execution. macOS: relink every
    /// Mach-O's non-system dylib deps (`@@HOMEBREW_*@@` placeholders) to
    /// `@loader_path`-relative paths into the tree's `lib/`, then ad-hoc re-sign
    /// each Mach-O LAST (relinking invalidates signatures). Must error loudly if
    /// a dependency is NOT bundled — never publish a tree that can't load.
    fn prepare_binary_tree(&self, root: &std::path::Path) -> Result<()>;
}

/// Keeps the privileged Caddy edge (`:80`/`:443`, root) alive across ANY death —
/// external SIGTERM, crash, sleep/wake, logout, reboot — via an OS supervisor
/// (macOS: a root LaunchDaemon with `KeepAlive`). This is what makes the edge
/// "run continuously unless explicitly stopped": the health watchdog can't restart
/// it (a privileged start needs an auth prompt), so the OS owns the restart instead.
///
/// Like [`DnsManager`] these are pure BUILDERS (no privilege, no side effects) so the
/// install/stop/start shell is unit-testable and batched into ONE `PrivilegeManager`
/// elevation. `core::proxy` writes the plist + wrapper CONTENTS to a staging dir
/// unprivileged (plain file writes — no shell-escaping of multi-line files), then
/// runs [`install_command`] once to `cp` them into the root-owned tree and bootstrap.
///
/// [`install_command`]: EdgeSupervisor::install_command
pub trait EdgeSupervisor: Send + Sync {
    /// Whether the edge daemon is installed (its plist is on disk) — the source of
    /// truth for "is the edge under OS supervision" (vs the legacy osascript spawn).
    fn is_installed(&self) -> bool;
    /// Whether the supervisor will actually run the daemon (macOS: the label is not
    /// on launchd's system disabled list — an explicit Stop-all `disable`s it).
    /// Diagnosis input for the watchdog when a supervised edge stays down; readable
    /// without privilege. Return `true` when the state can't be determined (avoid a
    /// false "disabled" diagnosis).
    fn is_enabled(&self) -> bool;
    /// Path of the OS supervisor definition (macOS: the root LaunchDaemon plist).
    fn plist_path(&self) -> PathBuf;
    /// Path of the root-owned launcher the supervisor runs.
    fn wrapper_path(&self) -> PathBuf;
    /// Path of the root-owned caddy binary the supervisor executes — NEVER the
    /// user-writable download cache (re-execing a user-writable file as root is an
    /// LPE). Install copies our caddy here and locks it `root:wheel`.
    fn daemon_binary_path(&self) -> PathBuf;
    /// Contents of the supervisor definition (macOS plist): keep-alive + start-at-boot,
    /// running `wrapper`, with start diagnostics to `start_log`.
    fn plist_contents(&self, wrapper: &Path, start_log: &Path) -> String;
    /// Contents of the launcher: hand the admin socket to the invoking user, then
    /// `exec` caddy (so the supervisor tracks caddy's own PID).
    fn wrapper_contents(
        &self,
        caddy_bin: &Path,
        caddyfile: &Path,
        admin_sock: &Path,
        appdata: &Path,
    ) -> String;
    /// ONE privileged shell that installs the daemon: copy `src_caddy` into the root
    /// tree (`root:wheel 0755`), drop in the staged wrapper + plist with safe perms,
    /// and (re)bootstrap the supervisor. Run via `PrivilegeManager` (one prompt).
    fn install_command(&self, src_caddy: &Path, staged_wrapper: &Path, staged_plist: &Path)
        -> String;
    /// Privileged shell to (re)start after an explicit stop.
    fn start_command(&self) -> String;
    /// Privileged shell to EXPLICITLY stop — must remove the job so keep-alive can't
    /// relaunch it (macOS: `disable` then `bootout`).
    fn stop_command(&self) -> String;
    /// Privileged shell to fully remove the daemon (uninstall / reset).
    fn uninstall_command(&self) -> String;
}

/// Keeps the loopback DNS resolver (`*.<tld> → 127.0.0.1`, UDP 15353) alive
/// INDEPENDENTLY of the app — a user-level supervisor (macOS: a per-user
/// LaunchAgent with `KeepAlive` running `<app binary> --dns-agent`). This is the
/// missing half of "services outlive the app": the data plane (nginx/fpm/DB/edge)
/// already survives a quit, but an in-process resolver dies with the app and takes
/// name resolution — and therefore every site — down with it after client caches
/// expire (observed live: sites survived a quit for ~1h40m on connection reuse,
/// then died; reopening the app fixed them in seconds).
///
/// Everything here is UNPRIVILEGED (the resolver binds a high loopback port; the
/// plist lives in `~/Library/LaunchAgents`), so unlike [`EdgeSupervisor`] these are
/// direct side-effecting ops (same style as [`AutostartManager`]) — no
/// `PrivilegeManager`, no prompt. The in-process resolver remains the automatic
/// fallback when the agent can't come up, so DNS never regresses below the old
/// behavior.
pub trait DnsAgentManager: Send + Sync {
    /// Whether the agent is installed (its plist is on disk).
    fn is_installed(&self) -> bool;
    /// Path of the agent's plist (`~/Library/LaunchAgents/<label>.dns.plist`).
    fn plist_path(&self) -> Result<PathBuf>;
    /// Plist contents: run `exe --dns-agent` at login, keep it alive, log to `log`.
    /// Pure builder (unit-testable); [`DnsAgentManager::install`] writes it.
    fn plist_contents(&self, exe: &Path, log: &Path) -> String;
    /// Write/refresh the plist for `exe` and (re)load the agent with launchd.
    /// Idempotent; called on every app launch so the plist always tracks the
    /// last-launched build (dev ↔ installed hand off automatically).
    fn install(&self, exe: &Path, log: &Path) -> Result<()>;
    /// Restart the agent (watchdog recovery for a wedged/dead resolver).
    fn kickstart(&self) -> Result<()>;
    /// Unload the agent and remove its plist (app reset / uninstall).
    fn uninstall(&self) -> Result<()>;
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
    /// OS supervisor that keeps the root edge alive (macOS LaunchDaemon KeepAlive).
    fn edge(&self) -> &dyn EdgeSupervisor;
    /// User-level supervisor that keeps the DNS resolver alive across app quits
    /// (macOS LaunchAgent KeepAlive).
    fn dns_agent(&self) -> &dyn DnsAgentManager;
}

/// Pick the MASTER from `(pid, ppid)` pairs of processes sharing one listen
/// socket: the process whose parent is NOT itself in the set — workers are
/// children of the master, while a master orphaned by an app quit is
/// reparented to launchd (ppid 1) and a supervised one to its (non-listening)
/// launcher. Multiple roots (shouldn't happen) resolve to the lowest pid for
/// determinism; a set with no root (can't happen — a cycle) degrades the same
/// way. Pure so the wraparound case is unit-testable without an OS.
pub(crate) fn select_master(procs: &[(u32, u32)]) -> Option<u32> {
    let pids: std::collections::HashSet<u32> = procs.iter().map(|&(pid, _)| pid).collect();
    procs
        .iter()
        .filter(|(_, ppid)| !pids.contains(ppid))
        .map(|&(pid, _)| pid)
        .min()
        .or_else(|| pids.into_iter().min())
}

#[cfg(test)]
mod tests {
    use super::select_master;

    #[test]
    fn master_forked_first_still_wins() {
        // The common shape the old lowest-pid heuristic happened to get right.
        assert_eq!(select_master(&[(100, 1), (101, 100), (102, 100)]), Some(100));
    }

    #[test]
    fn pid_wraparound_worker_below_master() {
        // The live Apache bug: churned workers get recycled LOW pids, so
        // lowest-pid picks a WORKER. Parent-based selection must not.
        let procs = [(71063, 95274), (95274, 1), (95279, 95274), (95280, 95274)];
        assert_eq!(select_master(&procs), Some(95274));
    }

    #[test]
    fn single_process_is_its_own_master() {
        assert_eq!(select_master(&[(500, 321)]), Some(500));
    }

    #[test]
    fn supervised_master_with_a_live_foreign_parent() {
        // Master's parent alive but not a listener (launcher/supervisor).
        assert_eq!(select_master(&[(60, 42), (61, 60)]), Some(60));
    }

    #[test]
    fn empty_and_multi_root_degrade_deterministically() {
        assert_eq!(select_master(&[]), None);
        assert_eq!(select_master(&[(20, 1), (10, 1)]), Some(10));
    }
}

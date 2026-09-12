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
    /// Shell command(s) that put BORROWED resolver files back — one batch, one
    /// cache flush. Each entry is `(tld, our 0600 backup of their file)`.
    ///
    /// Copies the backup rather than writing its contents inline: the bytes are
    /// the USER'S file, arbitrary, and must never be interpolated into a root
    /// shell string. The backup path lives under app-data (spaces) so it needs
    /// quoting; the TLD is an `[a-z]{1,63}` label by construction.
    fn restore_command(&self, restores: &[(String, PathBuf)]) -> String;
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
    /// the OS auth prompt once, worded by `reason`. Returns captured stdout on
    /// success.
    fn run_privileged(&self, script: &str, reason: &PromptReason) -> Result<String>;
}

/// What a privileged prompt is for. It completes the sentence the dialog shows —
/// "rexenv wants to <reason>." — so it is a lower-case verb phrase naming the
/// change in the user's terms, without the trailing period.
///
/// Why it exists: every admin dialog read the OS default, "… wants to make
/// changes.", so a user asked for their password could not tell a DNS resolver
/// from the HTTPS server from uninstall (owner, 12 Sep 2026). A TYPE, not a
/// `&str`, because it sits beside the script: swapped strings would compile, show
/// the script to the user and run the sentence as root. REQUIRED, not a default,
/// so a new caller cannot fall back to the wordless dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptReason(String);

impl PromptReason {
    pub fn new(what: impl Into<String>) -> Self {
        Self(what.into().trim().trim_end_matches('.').to_string())
    }

    /// The whole sentence the dialog shows.
    pub fn sentence(&self) -> String {
        format!("rexenv wants to {}.", self.0)
    }
}

#[cfg(test)]
mod prompt_reason_tests {
    use super::PromptReason;

    #[test]
    fn a_reason_completes_one_sentence_whatever_punctuation_the_caller_brought() {
        let want = "rexenv wants to add a DNS resolver so .rex sites open on this Mac.";
        for given in [
            "add a DNS resolver so .rex sites open on this Mac",
            "add a DNS resolver so .rex sites open on this Mac.",
            "  add a DNS resolver so .rex sites open on this Mac. ",
        ] {
            assert_eq!(PromptReason::new(given).sentence(), want, "{given:?}");
        }
    }
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

    /// PIDs of every process whose NAME is exactly `name` (candidate
    /// enumeration only — callers must positively identify each pid via
    /// [`Self::pid_command`] before touching it; the name alone proves
    /// nothing). Default: none — a platform without this relies on records.
    fn pids_named(&self, _name: &str) -> Vec<u32> {
        Vec::new()
    }

    /// Full command line of `pid` as the OS reports it (argv, space-joined),
    /// or `None` when the process is gone or the platform can't read it.
    /// Callers use this to POSITIVELY identify a recorded pid before sending
    /// any signal — a recycled pid must fail identification and never be
    /// killed on the bare number (§5 ownership doctrine, ported off ports:
    /// cloudflared listens on nothing, so its identity lives in its argv).
    /// Default: unknown — callers must treat that as "not identified".
    /// The EXECUTABLE path of `pid` — what the kernel is running, not what the
    /// process calls itself.
    ///
    /// Distinct from [`Self::pid_command`] and not derivable from it: php-fpm
    /// rewrites its process title, so on macOS `ps -o comm=` returns
    /// `php-fpm: master process (…/config/php-fpm-8.3.conf)` — the CONF path,
    /// which names the minor and never the patch. Measured on a live 8.3 master
    /// 16 Aug 2026. The 7.4 and 8.0 masters happen to still show their real
    /// path, which is exactly the trap: an argv-based implementation passes a
    /// hand check and is silently wrong for every other minor.
    ///
    /// Default: unknown — a platform that cannot answer must not guess, and
    /// callers must treat `None` as "cannot tell", never as "not ours".
    fn pid_exe(&self, _pid: u32) -> Option<PathBuf> {
        None
    }

    fn pid_command(&self, _pid: u32) -> Option<String> {
        None
    }

    /// Spawn `program` in its OWN process group with stdout+stderr piped, a
    /// working dir, and a caller-supplied FULL environment (the login-shell
    /// snapshot — not our launchd env). The repo-job primitive
    /// (`core::repo::run_step_streamed`). Own group because npm/git spawn
    /// worker trees — a positive-pid kill would orphan them (§5
    /// orphan-workers lesson, preempted).
    fn spawn_streamed(
        &self,
        _program: &std::path::Path,
        _args: &[String],
        _cwd: &std::path::Path,
        _env: &[(String, String)],
    ) -> Result<Child> {
        Err(crate::error::Error::Unsupported("spawn_streamed"))
    }

    /// SIGTERM the whole process GROUP `pgid`, wait a short grace, then
    /// SIGKILL the group; returns when the group is empty. Group liveness is
    /// checked (not the leader's — a leader can die while a TERM-ignoring
    /// child lives on).
    fn stop_group(&self, _pgid: u32) -> Result<()> {
        Err(crate::error::Error::Unsupported("stop_group"))
    }

    /// Spawn a detached watcher that stops `child` as soon as THIS process
    /// dies — including a SIGKILL, which runs none of our own shutdown code.
    ///
    /// The third leg of "tunnels die with the app": `RunEvent::Exit` covers a
    /// clean quit and the launch sweep covers a crash *at the next launch*,
    /// which leaves a site public for however long the user takes to come
    /// back. macOS has no `PR_SET_PDEATHSIG`, so the watcher cannot live
    /// inside a dead process — hence a separate one, per share.
    ///
    /// The implementation must kill on positive argv IDENTITY (`domain`), never
    /// on the bare pid: the number may belong to somebody else by the time the
    /// watcher wakes. Default: unsupported — a platform without it still has
    /// both other legs.
    fn guard_child_against_our_death(&self, _child: u32, _domain: &str) -> Result<()> {
        Err(crate::error::Error::Unsupported("guard_child_against_our_death"))
    }

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
    /// Bring an ENABLED login item up to date at launch — the binary it names
    /// and the flags it passes — without doing what `enable` does on a launch
    /// the user did not mean as "make THIS the login item". `enable` is the
    /// user's explicit choice and always points at the current binary; a
    /// refresh from a dev build outside an `.app` bundle must not re-point the
    /// login item at a `target/debug` path the next `cargo clean` deletes.
    /// Default: the same as `enable`.
    fn refresh(&self) -> Result<()> {
        self.enable()
    }
}

/// File permissions. POSIX chmod/chown on Unix; ACLs on Windows.
pub trait PermissionManager: Send + Sync {
    /// Make `path` executable (e.g. a downloaded binary).
    fn set_executable(&self, path: &std::path::Path) -> Result<()>;
    /// Restrict `path` to owner-only access (0600 on Unix; ACL on Windows).
    /// Used for private keys (e.g. the local CA key).
    fn set_private(&self, path: &std::path::Path) -> Result<()>;
    /// Write `contents` to `path` with the file BORN owner-only (0600 on Unix:
    /// `OpenOptions.mode(0o600)` at create) — no world-readable window between
    /// write and a later chmod, unlike write-then-`set_private` (B6). Used for
    /// private keys; also re-hardens `path` if it already exists.
    fn write_private(&self, path: &std::path::Path, contents: &[u8]) -> Result<()>;
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

    /// Web browsers installed on this machine ("Open in browser", §1.3), in the
    /// same detection-ordered shape as [`Self::detect_editors`]. Exactly one
    /// entry may carry [`BrowserApp::system_default`].
    /// Default: none — Windows/Linux fill this in Phase 4.
    fn detect_browsers(&self) -> Vec<BrowserApp> {
        Vec::new()
    }

    /// Open `url` in the browser with [`BrowserApp::id`] (macOS:
    /// `open -a <app> <url>`). Errors if the browser is not installed.
    ///
    /// URLS ONLY: implementations MUST reject anything that is not `http://` or
    /// `https://` — `open -a <browser> <path>` will happily hand a local FILE to
    /// a browser, and every caller of this (the chevron menu, the preferred-
    /// browser route in `open_external`) is a link affordance. Paths keep going
    /// through [`Self::open`], which asks the OS handler. The scheme check must
    /// cover BOTH modes from one place — a private-window path with its own
    /// copy of the guard is a second surface to forget.
    ///
    /// `private` asks for a private/incognito window. Implementations MUST error
    /// rather than fall back to a normal window when the browser has no such
    /// command line ([`BrowserApp::supports_private`] is `false`): a "private"
    /// action that silently opens a recorded window is the one failure this
    /// feature cannot have.
    fn open_in_browser(&self, _browser_id: &str, _url: &str, _private: bool) -> Result<()> {
        Err(crate::error::Error::Unsupported("open_in_browser"))
    }

    /// Terminal emulators installed on this machine ("Open in terminal", the
    /// chevron beside the built-in Terminal), detection-ordered like
    /// [`Self::detect_editors`]. Default: none — Windows/Linux fill this in
    /// Phase 4.
    fn detect_terminals(&self) -> Vec<TerminalApp> {
        Vec::new()
    }

    /// Open a NEW window of the terminal with [`TerminalApp::id`], with `path`
    /// as its working directory (macOS: `open -a <app> <dir>` for apps that take
    /// a folder as a document, `open -na <app> --args <cwd flag> <dir>` for the
    /// rest). Errors if the terminal is not installed.
    ///
    /// `path` is a DIRECTORY resolved on the backend side (the site docroot or a
    /// plugin/theme folder) — the frontend passes ids, never a path, so this can
    /// never be pointed at an arbitrary place on disk. Implementations MUST
    /// reject a `path` that is not an existing directory: `open -a <term> <file>`
    /// would hand the file to the terminal as a *script to run*.
    fn open_in_terminal(&self, _terminal_id: &str, _path: &std::path::Path) -> Result<()> {
        Err(crate::error::Error::Unsupported("open_in_terminal"))
    }

    /// The user's REAL shell environment (PATH, SSH_AUTH_SOCK, …), resolved by
    /// running their login shell the way a terminal would. A Finder-launched
    /// app inherits the bare launchd environment — Homebrew's shellenv lives in
    /// `.zprofile` and nvm/fnm/asdf init in `.zshrc` — so SYSTEM dev-tool
    /// discovery (git/node/npm/composer, `core::devtools`) must go through
    /// this, never through our own inherited PATH. Expensive (spawns a shell,
    /// runs the user's rc files) — callers cache the snapshot.
    fn login_shell_env(&self) -> Result<Vec<(String, String)>> {
        Err(crate::error::Error::Unsupported("login_shell_env"))
    }

    /// Create a directory symlink `link` → `target` (Link-folder assets:
    /// wp-content/<kind>s/<name> → the user's own checkout elsewhere).
    /// OS-specific: unix symlink vs Windows directory junction/symlink.
    fn symlink_dir(&self, _target: &std::path::Path, _link: &std::path::Path) -> Result<()> {
        Err(crate::error::Error::Unsupported("symlink_dir"))
    }

    /// Remove a symlink WITHOUT touching its target. The unlink-only delete
    /// path for linked assets — the caller has already verified `link` IS a
    /// symlink (fs truth, not metadata).
    fn remove_symlink(&self, _link: &std::path::Path) -> Result<()> {
        Err(crate::error::Error::Unsupported("remove_symlink"))
    }

    /// Preflight before executing SYSTEM `git`: on macOS `/usr/bin/git` is an
    /// Xcode CLT shim that pops a GUI install dialog when the tools are
    /// missing — probe quietly (`xcode-select -p`) instead of letting a
    /// background `git --version` throw a dialog at the user. `Ok(())` means
    /// git is safe to execute. Default: Ok (no shim on other platforms).
    fn git_preflight(&self) -> Result<()> {
        Ok(())
    }
}

/// Marker a [`ShellRunner::login_shell_env`] impl has the shell print before
/// `env -0`, so rc-file noise (echoes, motd) can never corrupt the parse —
/// everything before the LAST marker is noise. Lives here, next to the trait,
/// so platform impls share the protocol without importing `core`.
pub const ENV_MARKER: &str = "REXENV-ENV";

/// Parse the raw stdout of `printf '\0<MARKER>\0'; command env -0` into env
/// pairs. NUL separation means values may contain newlines; entries that
/// aren't `KEY=value` with an identifier key (rc noise, partial writes) are
/// dropped. Without the marker (defensive) the whole buffer is parsed.
pub fn parse_shell_env_output(raw: &[u8]) -> Vec<(String, String)> {
    let marker: Vec<u8> = format!("\0{ENV_MARKER}\0").into_bytes();
    let start = raw
        .windows(marker.len())
        .rposition(|w| w == marker.as_slice())
        .map(|p| p + marker.len())
        .unwrap_or(0);
    raw[start..]
        .split(|b| *b == 0)
        .filter_map(|entry| {
            let s = std::str::from_utf8(entry).ok()?;
            let (k, v) = s.split_once('=')?;
            let key_ok = !k.is_empty()
                && k.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            key_ok.then(|| (k.to_string(), v.to_string()))
        })
        .collect()
}

/// A detected code editor (`ShellRunner::detect_editors`).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorApp {
    /// Stable key stored as the `preferred_editor` setting (e.g. "vscode").
    pub id: String,
    /// Display name (e.g. "Visual Studio Code").
    pub name: String,
    /// The app's OWN icon as a `data:image/png;base64,…` URI, or `None` when it
    /// couldn't be read (see [`BrowserApp::icon`] — same rule, same fallback).
    pub icon: Option<String>,
}

/// A detected terminal emulator (`ShellRunner::detect_terminals`).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalApp {
    /// Stable key passed back to `ShellRunner::open_in_terminal` (e.g. "iterm").
    pub id: String,
    /// Display name (e.g. "iTerm").
    pub name: String,
    /// The app's OWN icon as a `data:image/png;base64,…` URI, or `None` when it
    /// couldn't be read (see [`BrowserApp::icon`] — same rule, same fallback).
    pub icon: Option<String>,
}

/// A detected web browser (`ShellRunner::detect_browsers`).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserApp {
    /// Stable key stored as the `preferred_browser` setting (e.g. "chrome").
    pub id: String,
    /// Display name (e.g. "Google Chrome").
    pub name: String,
    /// The app's OWN icon as a `data:image/png;base64,…` URI, or `None` when it
    /// couldn't be read (an app that ships its icon only in a compiled asset
    /// catalog). `None` is honest — the UI draws its monochrome glyph instead
    /// of a wrong or invented brand mark, which is why this is an Option and
    /// not a hand-drawn SVG table.
    pub icon: Option<String>,
    /// This is the OS's current default handler for `https`. Display only: it
    /// picks which icon the button wears when the user has chosen nothing. The
    /// actual open still goes through the OS handler.
    pub system_default: bool,
    /// This browser can be opened straight into a private/incognito window
    /// (`ShellRunner::open_in_browser` with `private`). `false` is common and
    /// honest — Safari has no such command line at all — and the UI then draws
    /// NO private affordance on that row. Never guess `true`: an ignored flag
    /// opens a normal, recorded window under a control that promised privacy.
    pub supports_private: bool,
}

/// Pull the `https` handler's bundle id out of raw
/// `defaults read com.apple.LaunchServices/com.apple.launchservices.secure LSHandlers`
/// output. Pure so it is testable without a machine that has a given browser.
///
/// The old-style plist text is a list of `{ … }` dicts; the one we want holds
/// `LSHandlerURLScheme = https;` and answers with `LSHandlerRoleAll = "<id>";`.
/// Other dicts pair the same key with a content type (`public.html`) or a
/// different scheme, so the block — not the file — is the unit of the search.
/// Lives here (not in the macOS impl) only because it is pure text and the
/// tests must run on every platform.
pub fn parse_default_browser_bundle_id(raw: &str) -> Option<String> {
    // A key can share a line with the opening brace, so strip that too — real
    // `defaults` output puts each key on its own line, but nothing guarantees it.
    fn key(l: &str) -> &str {
        l.trim().trim_start_matches('{').trim()
    }
    raw.split('}')
        .find(|block| {
            block
                .lines()
                .any(|l| key(l).starts_with("LSHandlerURLScheme") && l.contains("https"))
        })?
        .lines()
        // RoleAll is what LaunchServices writes for a URL scheme; RoleViewer is
        // accepted as a second chance rather than reporting "no default" on a
        // machine that spells it the other way.
        .find(|l| {
            let l = key(l);
            l.starts_with("LSHandlerRoleAll") || l.starts_with("LSHandlerRoleViewer")
        })
        .and_then(|l| l.split_once('='))
        .map(|(_, v)| v.trim().trim_end_matches(';').trim().trim_matches('"').to_string())
        .filter(|id| !id.is_empty() && id != "-")
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

/// Where the running app is installed, as far as REPLACING it is concerned.
///
/// A location, not a verdict: `core::app_update::preflight` decides what each
/// one means, so the decision is a pure function a test can drive over fixture
/// facts rather than something only a real Mac can answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallKind {
    /// The ordinary case: `/Applications/<name>.app`.
    Applications,
    /// `~/Applications/<name>.app` — replaceable, and nobody else's business.
    UserApplications,
    /// Not inside a `.app` at all: `cargo run`, or a bare binary. There is no
    /// bundle to replace.
    DevBuild,
    /// Running from a mounted image (`/Volumes/…`) — read-only, and the answer
    /// is to drag the app to Applications rather than to work around it.
    DiskImage,
    /// macOS is running a read-only copy from `…/AppTranslocation/…` because the
    /// bundle still carries a quarantine attribute and was never Finder-moved.
    /// There is no supported way to find the original, so the answer is the same
    /// sentence as `DiskImage`.
    Translocated,
    /// A `.app` somewhere else — Downloads, a project folder, a second copy.
    Elsewhere,
}

/// What the platform can SEE about the installed bundle. Facts only: no
/// decision, no message, nothing that needs a policy to state.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleFacts {
    /// `…/rexenv.app`, derived from the running executable.
    pub bundle: PathBuf,
    /// The directory the bundle sits in — and therefore where a replacement is
    /// staged, which is what makes a cross-device rename impossible by
    /// construction rather than by a check.
    pub parent: PathBuf,
    pub kind: InstallKind,
    /// A Homebrew cask manages this install. Not a refusal — the bundle is the
    /// user's either way — but the card says `brew upgrade --cask rexenv` also
    /// works, and the tap's README says what `--greedy` still does.
    pub homebrew: bool,
    /// The parent directory is writable by THIS user (`access(W_OK)`), so the
    /// staging directory and the rename can happen with no privilege at all.
    pub parent_writable: bool,
    /// The bundle belongs to this uid. A bundle another login installed can be
    /// renamed by an admin but its leftovers could never be cleaned up.
    pub owned_by_me: bool,
    /// The parent's filesystem is mounted read-only.
    pub read_only: bool,
    /// `canonicalize(exe) == exe`: no symlink in the path. A symlinked launch
    /// means the running exe and the bundle being replaced can disagree.
    pub canonical: bool,
    /// Free bytes on the parent's volume.
    pub free_parent_bytes: u64,
}

/// What a staged bundle must turn out to BE before anything is swapped.
///
/// Passed in rather than read from a constant so the checks can be driven over a
/// fixture bundle in a test — a verifier that only ever runs against the real
/// app is a verifier nobody has watched fail.
#[derive(Debug, Clone)]
pub struct StagedExpect {
    /// `CFBundleShortVersionString` must equal this — the version the signed
    /// descriptor named, so a stale archive is caught before it is installed.
    pub version: String,
    /// `CFBundleIdentifier` must equal this.
    pub identifier: String,
    /// `CFBundleExecutable` must equal this, and the file must exist: the
    /// relaunch resolves the binary through this key.
    pub executable: String,
    /// Every Mach-O named here must contain exactly these architectures.
    /// Parameterised because a fixture bundle is single-arch while the shipped
    /// one is universal.
    pub archs: Vec<Arch>,
    /// Files under `Contents/MacOS/` that must be present — the `rex` sidecar,
    /// whose absence would silently break every terminal after an update.
    pub required_binaries: Vec<String>,
    /// Run `codesign --verify --deep --strict`. Off for fixtures that were never
    /// signed; ON for anything a user would launch.
    pub codesign: bool,
}

/// A verified bundle sitting beside the installed one, ready to swap in.
#[derive(Debug, Clone)]
pub struct StagedBundle {
    /// `<stage_dir>/<name>.app`.
    pub path: PathBuf,
    /// The staging directory itself — dot-prefixed, so Finder and Spotlight skip
    /// it, and removed by the sweep once the new app is healthy.
    pub stage_dir: PathBuf,
}

/// How the swap happened, and where the previous bundle went.
#[derive(Debug, Clone)]
pub struct SwapReceipt {
    pub installed: PathBuf,
    /// The PREVIOUS bundle, still on disk. Deleted only once the new app has
    /// launched and confirmed its own version — never by the process that
    /// swapped it, which is gone by then and could not honestly watch.
    pub previous: PathBuf,
    pub method: SwapMethod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapMethod {
    /// `renamex_np(RENAME_SWAP)` — one syscall, so there is no moment when the
    /// install path holds no bundle.
    AtomicSwap,
    /// rename-aside then rename-in, with a restore if the second fails. The
    /// fallback for a filesystem without `RENAME_SWAP`.
    RenamePair,
}

/// Why a swap could not happen. The EPERM→`PolicyBlocked` classification is a
/// macOS fact and lives here; what to SAY about it is `core`'s decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwapFailure {
    /// `EACCES` — an ordinary permission problem.
    NotWritable,
    /// `EPERM` — the kernel refused the operation itself. On macOS this is what
    /// App Management looks like, and root does not bypass it, so it is never
    /// retried with privileges.
    PolicyBlocked,
    /// `EXDEV`. Impossible with sibling staging, and kept so that a future
    /// caller staging somewhere else fails loudly rather than silently.
    CrossDevice,
    /// `EROFS`.
    ReadOnly,
    /// `ENOTSUP`/`EINVAL` — the filesystem has no atomic swap.
    Unsupported,
    Other(String),
}

impl std::fmt::Display for SwapFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotWritable => write!(f, "the folder is not writable"),
            Self::PolicyBlocked => write!(f, "macOS refused the operation (operation not permitted)"),
            Self::CrossDevice => write!(f, "the two paths are on different volumes"),
            Self::ReadOnly => write!(f, "the volume is read-only"),
            Self::Unsupported => write!(f, "this filesystem has no atomic swap"),
            Self::Other(e) => write!(f, "{e}"),
        }
    }
}

/// A bundle left in a staging directory by some earlier update.
#[derive(Debug, Clone)]
pub struct Leftover {
    pub path: PathBuf,
    /// The version inside it, when its `Info.plist` could be read. The sweep
    /// classifies by THIS rather than by a marker file: a crash between the swap
    /// and any write cannot make a version lie about itself.
    pub version: Option<String>,
    pub deleted: bool,
}

/// Replacing the running application bundle with a verified copy of a newer one.
///
/// Every method here is a syscall or an OS tool. The DECISIONS — may this be
/// replaced, what does a refusal say, which leftover is the previous bundle —
/// live in `core::app_update`, over [`BundleFacts`], so they are provable
/// without a Mac and without an installed app.
///
/// macOS is real; Windows and Linux are `todo!()` until someone ports them,
/// which is the standing rule for a new capability.
pub trait AppBundle: Send + Sync {
    /// What can be seen about the bundle the running executable belongs to.
    fn facts(&self, exe: &Path) -> Result<BundleFacts>;
    /// Extract `archive` into a dot-prefixed staging directory beside the
    /// installed bundle and verify it against `expect`. On ANY failure the
    /// staging directory is removed and the installed bundle is untouched.
    fn stage(
        &self,
        facts: &BundleFacts,
        archive: &Path,
        expect: &StagedExpect,
    ) -> Result<StagedBundle>;
    /// Put the staged bundle at the install path and the installed one in
    /// staging. Atomic where the filesystem allows it; restored on failure where
    /// it does not. Nothing is deleted on any path.
    fn swap(
        &self,
        installed: &Path,
        staged: &StagedBundle,
    ) -> std::result::Result<SwapReceipt, SwapFailure>;
    /// Classify and optionally remove staging leftovers beside the bundle.
    /// `delete_previous` is false until the new app is healthy, which is what
    /// keeps a rollback possible.
    fn sweep_leftovers(
        &self,
        parent: &Path,
        my_version: &str,
        delete_previous: bool,
    ) -> Result<Vec<Leftover>>;
    /// Spawn the detached helper that reopens `bundle` once THIS process is
    /// gone. Called from the exit hook, after the quit gate has already let the
    /// quit through — so a cancelled quit leaves no helper waiting on a pid that
    /// is not going to die.
    fn spawn_relauncher(&self, bundle: &Path) -> Result<()>;
}

/// Local (non-TCP) inter-process transport — how rexenv reaches a server on THIS
/// machine: a unix-domain socket on macOS/Linux, a named pipe with a current-user
/// ACL on Windows (owner ruling D3, 12 Sep 2026, `docs/PLAN-windows-port.md`).
/// Today it only DIALS — the edge admin's liveness (`proxy::admin_alive`) and a
/// MySQL server's greeting (`dbsource::probe_socket`); the `rex` CLI and MCP
/// listeners move behind it in the port's W8.
pub trait LocalIpc: Send + Sync {
    /// Connect to the endpoint at `path`. `read_timeout` bounds each read on the
    /// returned stream (`None` = the OS default). A connect error keeps the kind
    /// the OS reported, so callers can tell "no endpoint there" (`NotFound`) from
    /// "an endpoint nobody listens on" (`ConnectionRefused`).
    fn connect(
        &self,
        path: &Path,
        read_timeout: Option<std::time::Duration>,
    ) -> std::io::Result<Box<dyn std::io::Read + Send>>;
}

/// Aggregate of every platform capability. `core/` is handed one of these and
/// never names a concrete OS type.
pub trait Platform: Send + Sync {
    /// Local IPC. The one accessor with a DEFAULT: the build OS's implementation
    /// (`platform::host_local_ipc`). There are a dozen stub `Platform`s in tests and
    /// examples, and before this trait existed `core/` dialled the socket itself —
    /// so the default keeps every one of them dialling a real socket exactly as it
    /// did, instead of each growing a field it would only forward.
    fn local_ipc(&self) -> &dyn LocalIpc {
        crate::platform::host_local_ipc()
    }
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
    /// Replacing the app's own bundle (self-update).
    fn app_bundle(&self) -> &dyn AppBundle;
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

#[cfg(test)]
mod default_browser_tests {
    use super::parse_default_browser_bundle_id;

    /// Verbatim shape of `defaults read … LSHandlers` on a real Mac: entries are
    /// `{ … }` dicts, `LSHandlerPreferredVersions` is a NESTED dict (its `}`
    /// used to be the whole reason a naive whole-file scan picked up the wrong
    /// `LSHandlerRoleAll`), and content-type entries surround the scheme one.
    const REAL: &str = r#"(
        {
        LSHandlerContentType = "public.html";
        LSHandlerPreferredVersions =         {
            LSHandlerRoleAll = "-";
        };
        LSHandlerRoleAll = "com.apple.safari";
    },
        {
        LSHandlerPreferredVersions =         {
            LSHandlerRoleAll = "-";
        };
        LSHandlerRoleAll = "com.google.chrome";
        LSHandlerURLScheme = https;
    },
        {
        LSHandlerRoleAll = "com.microsoft.vscode";
        LSHandlerURLScheme = "vscode";
    }
)"#;

    #[test]
    fn picks_the_https_entrys_handler_not_a_neighbours() {
        assert_eq!(parse_default_browser_bundle_id(REAL).as_deref(), Some("com.google.chrome"));
    }

    /// One entry, in the real one-key-per-line layout — a fixture that crams
    /// the first key onto the `{` line passes for the WRONG reason (the key is
    /// simply never found), which is how a "no https entry" assertion can go
    /// green against a parser that reads nothing at all.
    fn entry(body: &[&str]) -> String {
        format!("(\n    {{\n        {}\n    }}\n)", body.join("\n        "))
    }

    #[test]
    fn no_https_entry_is_none_not_a_guess() {
        // A machine that never changed its default has no https row at all —
        // reporting the `public.html` handler here would be an invention.
        let only_html = entry(&[
            r#"LSHandlerContentType = "public.html";"#,
            r#"LSHandlerRoleAll = "com.brave.browser";"#,
        ]);
        assert_eq!(parse_default_browser_bundle_id(&only_html), None);
    }

    #[test]
    fn placeholder_and_empty_handlers_are_none() {
        let dash = entry(&[r#"LSHandlerRoleAll = "-";"#, "LSHandlerURLScheme = https;"]);
        assert_eq!(parse_default_browser_bundle_id(&dash), None);
        assert_eq!(parse_default_browser_bundle_id(""), None);
        assert_eq!(parse_default_browser_bundle_id("nonsense without braces"), None);
    }

    #[test]
    fn role_viewer_is_accepted_when_role_all_is_absent() {
        let viewer =
            entry(&[r#"LSHandlerRoleViewer = "org.mozilla.firefox";"#, "LSHandlerURLScheme = https;"]);
        assert_eq!(
            parse_default_browser_bundle_id(&viewer).as_deref(),
            Some("org.mozilla.firefox")
        );
    }

    #[test]
    fn an_http_only_entry_does_not_answer_for_https() {
        // `http` contains no "https" substring, so the block must not match —
        // guards the naive `contains("http")` version of this parser.
        let http =
            entry(&[r#"LSHandlerRoleAll = "com.operasoftware.opera";"#, "LSHandlerURLScheme = http;"]);
        assert_eq!(parse_default_browser_bundle_id(&http), None);
    }

    #[test]
    fn a_key_sharing_the_brace_line_is_still_read() {
        // Defensive: not the layout `defaults` emits, but a parser that only
        // works on one whitespace convention is a fixture-shaped parser.
        let inline = r#"{ LSHandlerRoleAll = "com.apple.safari";
        LSHandlerURLScheme = https; }"#;
        assert_eq!(parse_default_browser_bundle_id(inline).as_deref(), Some("com.apple.safari"));
    }
}

#[cfg(test)]
mod import_graph {
    /// Ledger #163 — the module doc's own rule, as a scan instead of a review
    /// habit: `core/` PRODUCTION code never names an OS implementation module
    /// and never carries an OS conditional. The day either appears, "adding an
    /// OS = filling stubs, NOT restructuring core" stops being true — the
    /// non-negotiable in CLAUDE.md this file anchors. Tests are exempt
    /// (asserting macOS-specific MESSAGES from a stub platform is legitimate
    /// and common), which is why the scan reads production lines only.
    #[test]
    fn core_production_code_never_names_an_os_or_carries_an_os_cfg() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let needles =
            ["platform::macos", "platform::windows", "platform::linux", "#[cfg(target_os"];

        fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                    if let Ok(text) = std::fs::read_to_string(&path) {
                        // Production lines only, comments stripped — a guard
                        // that reads prose reads its own explanation (#235).
                        let prod = crate::core::copy_scan::production_source(&text);
                        let stripped: String = prod
                            .lines()
                            .map(|l| {
                                let cut = l
                                    .match_indices("//")
                                    .find(|(i, _)| *i == 0 || !l[..*i].ends_with(':'))
                                    .map(|(i, _)| i);
                                cut.map_or(l, |i| &l[..i])
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        out.push((path.display().to_string(), stripped));
                    }
                }
            }
        }

        // Canary: the same needles over platform/ must hit — mod.rs selects
        // impls with #[cfg(target_os)] by design. A matcher that finds
        // nothing there would make core's zero vacuous.
        let mut platform_files = Vec::new();
        walk(&root.join("src/platform"), &mut platform_files);
        let canary: usize = platform_files
            .iter()
            .map(|(_, t)| needles.iter().map(|n| t.matches(n).count()).sum::<usize>())
            .sum();
        assert!(canary >= 2, "only {canary} needle hits in platform/ — the matcher is broken");

        let mut core_files = Vec::new();
        walk(&root.join("src/core"), &mut core_files);
        assert!(core_files.len() > 30, "core walk found {} files — it stopped working", core_files.len());

        let violations: Vec<String> = core_files
            .iter()
            .flat_map(|(p, t)| {
                needles.iter().filter(|n| t.contains(*n)).map(move |n| format!("{p}: {n}"))
            })
            .collect();
        assert!(
            violations.is_empty(),
            "core/ production code reaches OS-specific ground:\n  {}\n\
             ALL OS-specific code lives behind the traits in this file — express the \
             difference as a trait method and implement it per platform (#163).",
            violations.join("\n  ")
        );
    }
}

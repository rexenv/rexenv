//! Platform selection. `core/` calls `platform::current()` to get a
//! `&dyn Platform` and never names a concrete OS type. The right implementation
//! is chosen at compile time via `#[cfg(target_os = "...")]`.

pub mod traits;

// Resolver files: the macOS route, and the fixture classification the core's test platforms share (#617).
pub(crate) mod resolver_files;

// The app-data namespace, ONE fact for every OS: `directories::ProjectDirs::from`
// composes it. macOS → `~/Library/Application Support/dev.rexenv.rexenv` (its drift
// guard checks that equals the bundle identifier); Windows → `%LOCALAPPDATA%\rexenv\
// rexenv\data` (the crate drops the qualifier there). Moved here from macos/ for the
// Windows port, so the two platforms cannot name the folder differently.
pub(crate) const APP_QUALIFIER: &str = "dev";
pub(crate) const APP_ORG: &str = "rexenv";
pub(crate) const APP_NAME: &str = "rexenv";

/// A platform method with no implementation yet, in a trait method that cannot
/// return an error (a `PathBuf`, a `bool`, a command string). Panics with the
/// same wording as `Error::Unported`, which the panic hook records — so an
/// unported path fails loudly and names itself, where `todo!()` said only
/// "not yet implemented" to a console a release build does not have.
/// Defined BEFORE the OS modules: `macro_rules!` is textually scoped.
#[allow(unused_macros)]
macro_rules! unported {
    ($what:expr) => {
        panic!("rexenv: {} is not ported to this OS yet (docs/PLAN-windows-port.md, docs/PLAN-linux-port.md)", $what)
    };
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;
// The single-instance pipe (W7 S1, ledger #620), for `cli_server`'s Windows half, and the MCP pipe (#632).
#[cfg(target_os = "windows")]
pub(crate) use windows::{
    claim_app_pipe, create_mcp_pipe, hand_off_to_app_pipe, next_app_pipe_instance, AppPipeClaim, HeldAppPipe,
};
#[cfg(target_os = "linux")]
mod linux;

// The Linux port's pure halves — `ss`/`/proc` parsing, the DNS route (dummy link), the two
// systemd units, the autostart entry and app catalog, the NSS/system trust argv — are text,
// so the macOS host proves them in `verify.sh` too (docs/PLAN-linux-port.md L1). The
// `desktop`/`dnsroute` modules are siblings `units`/`trust` reach through `super::`, so all
// five are mounted under ONE test-only parent that mirrors `linux/`.
#[cfg(all(test, not(target_os = "linux")))]
#[allow(dead_code)]
#[path = "linux"]
mod linux_pure {
    #[path = "app_bundle_rules.rs"]
    pub(crate) mod app_bundle_rules;
    #[path = "desktop.rs"]
    pub(crate) mod desktop;
    #[path = "proc_table.rs"]
    pub(crate) mod proc_table;
    #[path = "dnsroute.rs"]
    pub(crate) mod dnsroute;
    #[path = "libcompat.rs"]
    pub(crate) mod libcompat;
    #[path = "trust.rs"]
    pub(crate) mod trust;
    #[path = "units.rs"]
    pub(crate) mod units;
}

// The Windows owner-only SDDL builder is pure text: include it on the macOS/Linux
// test host so its tests run in `verify.sh` too, not only on a Windows machine.
#[cfg(all(test, not(target_os = "windows")))]
#[path = "windows/owner_only.rs"]
mod windows_owner_only;
// The self-update's path and version rules (install kind, stage-dir names, the
// VERSIONINFO words → `a.b.c`): pure, so the macOS host proves them too.
#[cfg(all(test, not(target_os = "windows")))]
#[allow(dead_code)]
#[path = "windows/app_bundle_rules.rs"]
mod windows_app_bundle_rules;
// And the one-hop rule for a launcher whose job forbids breakaway (ledger #692).
#[cfg(all(test, not(target_os = "windows")))]
#[allow(dead_code)]
#[path = "windows/job_guard_rules.rs"]
mod windows_job_guard_rules;
// Same for the PE header check `WindowsBinaryProvider` refuses non-x64 artifacts with.
// `dead_code` allowed: only its tests use it here; the Windows build uses the rest.
#[cfg(all(test, not(target_os = "windows")))]
#[allow(dead_code)]
#[path = "windows/pe.rs"]
mod windows_pe;
// And the port gate's pure half: socket-table rows, netsh's excluded ranges, the
// holder wording (ledger #599).
#[cfg(all(test, not(target_os = "windows")))]
#[allow(dead_code)]
#[path = "windows/port_table.rs"]
mod windows_port_table;
// And the stop sequencing (ledger #600).
#[cfg(all(test, not(target_os = "windows")))]
#[allow(dead_code)]
#[path = "windows/stop_policy.rs"]
mod windows_stop_policy;
// And the handle-snapshot parse a service spawn clears inheritance from (ledger #600).
#[cfg(all(test, not(target_os = "windows")))]
#[allow(dead_code)]
#[path = "windows/handles.rs"]
mod windows_handles;
// And the registry environment merge streamed steps get (ledger #609).
#[cfg(all(test, not(target_os = "windows")))]
#[allow(dead_code)]
#[path = "windows/login_env.rs"]
mod windows_login_env;
// And what a failed AF_UNIX connect means (ledger #611).
#[cfg(all(test, not(target_os = "windows")))]
#[allow(dead_code)]
#[path = "windows/ipc_rules.rs"]
mod windows_ipc_rules;
// And where Firefox's profiles live (ledger #612).
#[cfg(all(test, not(target_os = "windows")))]
#[allow(dead_code)]
#[path = "windows/firefox_root.rs"]
mod windows_firefox_root;
// And the CA trust's PEM read and cancel wording (ledger #613).
#[cfg(all(test, not(target_os = "windows")))]
#[allow(dead_code)]
#[path = "windows/cert_rules.rs"]
mod windows_cert_rules;
// And the DNS agent's logon task definition (ledger #616).
#[cfg(all(test, not(target_os = "windows")))]
#[allow(dead_code)]
#[path = "windows/logon_task.rs"]
mod windows_logon_task;
// And the elevated step's arguments, result and words (ledger #619).
#[cfg(all(test, not(target_os = "windows")))]
#[allow(dead_code)]
#[path = "windows/elevation_rules.rs"]
mod windows_elevation_rules;
// And the single-instance pipe's name and claim (ledger #620).
#[cfg(all(test, not(target_os = "windows")))]
#[allow(dead_code)]
#[path = "windows/app_pipe_rules.rs"]
mod windows_app_pipe_rules;
// And what the shell may be handed to open, and explorer's select argument (ledger #621).
#[cfg(all(test, not(target_os = "windows")))]
#[allow(dead_code)]
#[path = "windows/shell_rules.rs"]
mod windows_shell_rules;
// And the editors, browsers and terminals rexenv knows, how each is found and started (ledger #622).
#[cfg(all(test, not(target_os = "windows")))]
#[allow(dead_code)]
#[path = "windows/app_catalog.rs"]
mod windows_app_catalog;
// And the login item's Run value, Task Manager's disable, and the refresh decision (ledger #623).
#[cfg(all(test, not(target_os = "windows")))]
#[allow(dead_code)]
#[path = "windows/autostart_rules.rs"]
mod windows_autostart_rules;
// And what a linked folder's junction may point at, and its reparse data (ledger #625).
#[cfg(all(test, not(target_os = "windows")))]
#[allow(dead_code)]
#[path = "windows/junction_rules.rs"]
mod windows_junction_rules;
// And which user-`Path` entry is rexenv's CLI folder, adding it and removing it (ledger #634).
#[cfg(all(test, not(target_os = "windows")))]
#[allow(dead_code)]
#[path = "windows/user_path_rules.rs"]
mod windows_user_path_rules;
// And NRPT ownership and its PowerShell (ledger #618).
#[cfg(all(test, not(target_os = "windows")))]
#[allow(dead_code)]
#[path = "windows/nrpt_rules.rs"]
mod windows_nrpt_rules;

use traits::Platform;

/// Native JS dialog panels (alert/confirm/prompt) for the app webview — a
/// container gap, not a service concern, so it lives beside the traits rather
/// than behind one. macOS only: wry implements no WKUIDelegate JS-dialog
/// methods there, so `confirm()` silently returns `false`. Windows (WebView2)
/// and Linux (webkitgtk) webviews render their own JS dialogs, and tauri's
/// `PlatformWebview::inner` is macOS/iOS-only — so no cross-platform shim.
#[cfg(target_os = "macos")]
pub use macos::webview_dialogs::install_js_dialog_panels;

/// Run the tunnel guard (`main.rs`, before Tauri boots): a detached watcher that
/// ends ONE public share when the app that started it dies, SIGKILL included.
/// Re-exported by NAME rather than making the OS module public — same shape as
/// the dialog shim above, and the reason is the architecture rule: nothing
/// outside `platform/` may name a concrete OS type.
#[cfg(target_os = "macos")]
pub use macos::parent_death_guard::run as run_tunnel_guard;
/// Linux: the same guard over pidfds (`linux/parent_death_guard.rs`).
#[cfg(target_os = "linux")]
pub use linux::parent_death_guard::run as run_tunnel_guard;
#[cfg(target_os = "macos")]
pub use macos::relauncher::run as run_relauncher;
#[cfg(target_os = "windows")]
pub use windows::app_bundle::run_relauncher;
#[cfg(target_os = "linux")]
pub use linux::app_bundle::run_relauncher;

/// A process's start-time token — the guard's second identity for its parent.
/// Re-exported for the live check that spawns guards by hand.
#[cfg(target_os = "macos")]
pub use macos::process_start_token;
#[cfg(target_os = "windows")]
pub use windows::app_bundle::process_start_token;
#[cfg(target_os = "linux")]
pub use linux::process_start_token;

/// Bring rexenv to the front. Needed because it is an ACCESSORY app (menu bar,
/// no dock tile): nothing activates it on the user's behalf, so a window it
/// shows — or a modal it opens — can come up behind whatever the developer was
/// reading. Same re-export shape as the two above, same reason.
#[cfg(target_os = "macos")]
pub use macos::activation::activate_app;

/// The loopback port the embedded resolver serves on (`core::dns::DEFAULT_DNS_PORT`).
///
/// Per OS because the OS's routing decides it, not rexenv: a macOS resolver file names a port, so
/// the agent stays off the privileged range on 15353; Windows' NRPT rule names only a server, so the
/// agent must answer on 53 (plan §3 D2, ledger #615).
#[cfg(target_os = "windows")]
pub const RESOLVER_PORT: u16 = 53;
#[cfg(not(target_os = "windows"))]
pub const RESOLVER_PORT: u16 = 15353;

/// The words for the OS's own things — its file manager, its login items, how a tool gets installed (W7 S7,
/// ledger #626). Data for every OS on every host; `words::current()` is this build's.
pub mod words;

/// How this OS finds a command on `PATH` — the separator, the variable's case, `git` being `git.exe` (ledger
/// #628). Data for every OS on every host; `path_lookup::current()` is this build's.
pub mod path_lookup;

/// The embedded resolver's UDP socket, bound on `127.0.0.1:port` — loopback by construction (the
/// resolver answers every name, which is safe only there; ledger #44).
///
/// Windows binds it with `SO_EXCLUSIVEADDRUSE` and UDP connection-reset reports off
/// (`windows/resolver_socket.rs`, ledger #615); every other OS binds plainly, as before.
pub fn bind_resolver_udp(port: u16) -> std::io::Result<std::net::UdpSocket> {
    #[cfg(target_os = "windows")]
    {
        windows::bind_resolver_udp(port)
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, port))
    }
}

/// Send this process's stdout and stderr to `log`, appending — the DNS agent's output when nothing
/// else captures it. launchd writes the agent's output to the file its plist names, so this does
/// nothing on macOS; a Windows logon task has no such field, and its definition passes `--log` (W6 S2,
/// ledger #616).
pub fn send_output_to(log: &std::path::Path) {
    #[cfg(target_os = "windows")]
    windows::send_output_to(log);
    #[cfg(not(target_os = "windows"))]
    let _ = log;
}

/// When this process was started as Windows' elevated step (`rexenv.exe --elevated-step`, W6 S3, ledger
/// #619), run it and return its exit code; `None` otherwise — and always `None` off Windows. Checked by
/// `main.rs` before anything else starts: the step opens no window and touches no app state.
pub fn run_elevated_step(argv: &[String]) -> Option<i32> {
    #[cfg(target_os = "windows")]
    {
        windows::run_step(argv)
    }
    // Linux: the polkit step (`rexenv --privileged-step <script>`, run as root by pkexec so the
    // dialog carries rexenv's own action text — `linux/dev.rexenv.rexenv.policy`).
    #[cfg(target_os = "linux")]
    {
        linux::run_step(argv)
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = argv;
        None
    }
}

/// Clear the inherit flag on every handle this process was born holding, so no child — a
/// service, a step, an editor — inherits and pins them (ledger #600: services inherited 20–25 of
/// sshd's handles and held the launcher's SSH session open for as long as they ran). Called by
/// `main.rs` FIRST, while the process has one thread: a sweep beside each spawn, which is where
/// it lived until 27 Sep 2026, cleared the pipe ends of whatever OTHER spawn was in flight on
/// another thread, and that child started with no stdio (measured: 119 of 400 spawns). A no-op
/// off Windows, where `std` spawns with `CLOEXEC` and nothing is inherited.
pub fn sweep_inheritable_handles_before_boot() {
    #[cfg(target_os = "windows")]
    {
        windows::sweep_inheritable_handles();
    }
}

/// When this process was started inside a job that forbids its services to break away — the
/// installer's Finish page did that on the first installed copy, 19 Sep 2026 (ledger #692) — ask
/// Explorer to start rexenv again and return the code this copy exits with; `None` means run here.
/// Always `None` off Windows. Checked by `main.rs` before Tauri boots, so the hop holds no pipe
/// and no window the new copy will need.
pub fn relaunch_outside_confining_job() -> Option<i32> {
    #[cfg(target_os = "windows")]
    {
        windows::relaunch_outside_confining_job()
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

/// The limit flags of the job THIS process is in (`None` = no job) — the fact the hop above is
/// decided on, re-exported for the live check that spawns fixtures inside jobs it builds. Windows-only.
#[cfg(target_os = "windows")]
pub fn own_job_limits() -> Option<u32> {
    windows::own_job_limits()
}

/// Run Windows privileged ops in THIS process, with no dialog and no UAC — the elevated step's body, for a
/// check that already holds an elevated token (the Dell's SSH session). `(exit code, output)`. Windows-only.
#[cfg(target_os = "windows")]
pub fn run_elevated_ops_in_this_process(ops: &str) -> (i32, String) {
    windows::run_ops_here(ops)
}

/// The build OS's `LocalIpc` — what `Platform::local_ipc` returns unless a
/// platform overrides it. Selected here, beside `current()`, so `traits.rs`
/// never names an OS type.
pub fn host_local_ipc() -> &'static dyn traits::LocalIpc {
    #[cfg(target_os = "macos")]
    {
        static IPC: macos::MacosLocalIpc = macos::MacosLocalIpc;
        &IPC
    }
    #[cfg(target_os = "windows")]
    {
        static IPC: windows::WindowsLocalIpc = windows::WindowsLocalIpc;
        &IPC
    }
    #[cfg(target_os = "linux")]
    {
        static IPC: linux::LinuxLocalIpc = linux::LinuxLocalIpc;
        &IPC
    }
}

/// A `Command` for a helper rexenv runs itself — a database client or dump, WP-CLI,
/// `nginx -t`, `pg_ctl` — and **the ONE way `core/` builds one** (a source scan in
/// this module's tests refuses a bare `Command::new` there).
///
/// On Windows it carries `CREATE_NO_WINDOW`. A release build has no console
/// (`windows_subsystem = "windows"`), so every console program it starts without
/// that flag gets a console of its OWN — a visible window, and on Windows 11 a
/// Windows Terminal tab. Measured 21 Sep 2026 on the VM: Start all left an empty
/// terminal titled `…\pg_ctl.exe` on the desktop for as long as PostgreSQL ran
/// (the server inherits that console, so closing the "empty" window would have
/// killed the database), and opening Tunnels left one titled `…\php.exe`. Every
/// spawn in `core/` had been written on macOS, where there is no such thing.
/// Elsewhere this is `Command::new`.
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new(program);
    #[cfg(target_os = "windows")]
    windows::hide_console(&mut cmd);
    cmd
}

/// Tell a person the app panicked, where nothing else would — called by the
/// panic hook (`crash::install`), for the first panic of a process only.
///
/// On a Windows release build, which has no console by `windows_subsystem`, a
/// native message box naming `crash.log`. Everywhere else a no-op: a debug build
/// prints to its console, and on macOS the report file is the record (an alert
/// raised from a panicking thread would need AppKit's main thread). Ledger #596.
pub fn fatal_notice(summary: &str, crash_log: Option<&std::path::Path>) {
    #[cfg(all(target_os = "windows", not(debug_assertions)))]
    windows::fatal_notice(summary, crash_log);
    #[cfg(not(all(target_os = "windows", not(debug_assertions))))]
    let _ = (summary, crash_log);
}

/// An xz decoder over `reader` — MySQL's generic Linux tarballs are `.tar.xz` and nothing
/// else rexenv fetches is (docs/PLAN-linux-port.md L2). Only the Linux build links liblzma
/// (`xz2`, a Linux-only dependency); every other OS answers `Unsupported` rather than
/// carrying a C library for an archive it never downloads, and `core` keeps no `cfg`.
pub fn xz_decoder(reader: Box<dyn std::io::Read + Send>) -> crate::error::Result<Box<dyn std::io::Read + Send>> {
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(xz2::read::XzDecoder::new(reader)))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = reader;
        Err(crate::error::Error::Unsupported("xz archives (Linux only)"))
    }
}

/// Construct the platform implementation for the current OS.
pub fn current() -> Box<dyn Platform> {
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacosPlatform::new())
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsPlatform::new())
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxPlatform::new())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        compile_error!("unsupported target OS")
    }
}

#[cfg(test)]
mod stub_guard {
    /// Production lines of `text` (tests and comments stripped) that contain any
    /// needle — a guard that reads comments would read its own explanation (#235).
    fn hits(text: &str, needles: &[&str]) -> Vec<String> {
        crate::core::copy_scan::production_source(text)
            .lines()
            .map(|l| l.find("//").map_or(l, |i| &l[..i]))
            .filter(|l| needles.iter().any(|n| l.contains(n)))
            .map(|l| l.trim().to_string())
            .collect()
    }

    /// Ledger #595 — W3 step 0 (`docs/PLAN-windows-port.md` §3a Q1): the Windows
    /// stubs never `todo!()`. A release build has no console, so `todo!()`'s
    /// message reached nobody, and one reached from an IPC command left the
    /// frontend waiting forever. Stubs return `Error::Unported` or panic through
    /// `unported!` instead. Read as TEXT, so it runs on the macOS test host where
    /// the Windows module does not compile.
    #[test]
    fn windows_stubs_fail_as_unported_never_todo() {
        stubs_fail_as_unported_never_todo("windows", "impl Paths for WindowsPaths");
    }

    /// The same rule for Linux (docs/PLAN-linux-port.md L1): every method of `linux/mod.rs`
    /// was `todo!()` until 24 Sep 2026, and a `todo!()` reached from an IPC command is a
    /// frontend waiting forever on a release build with no console.
    #[test]
    fn linux_stubs_fail_as_unported_never_todo() {
        stubs_fail_as_unported_never_todo("linux", "impl Paths for LinuxPaths");
    }

    fn stubs_fail_as_unported_never_todo(os: &str, anchor: &str) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("src/platform/{os}/mod.rs"));
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read platform/{os}/mod.rs: {e}"));
        assert!(text.contains(anchor), "not the {os} platform module — a moved file would make this scan vacuous");
        let forbidden = ["todo!(", "unimplemented!("];
        // Canary: the matcher must see a planted one, or its zero proves nothing.
        assert_eq!(
            hits("fn f() -> u8 {\n    todo!(\"x\")\n}\n", &forbidden).len(),
            1,
            "the matcher cannot see a todo! at all"
        );
        let found = hits(&text, &forbidden);
        assert!(
            found.is_empty(),
            "platform/{os} has {} todo!/unimplemented! — return Error::Unported, or unported! where the trait cannot return an error: {found:?}",
            found.len()
        );
    }

    /// Ledger #705 — every helper the app starts comes from `platform::command`,
    /// never a bare `Command::new`: on Windows the bare one gives a console program
    /// its OWN console, a visible window (a pg_ctl terminal that hosted PostgreSQL,
    /// measured on the VM 21 Sep 2026). Scans all of `src/` but `platform/` — the
    /// one place that picks per-OS flags itself — and `test_support.rs`, which never
    /// ships. Text, so it holds on the macOS test host too.
    #[test]
    fn helpers_are_built_by_platform_command_never_a_bare_command_new() {
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(dir).expect("read src dir").flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
                    out.push(p);
                }
            }
        }
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        walk(&src, &mut files);
        let forbidden = ["Command::new("];
        assert_eq!(
            hits("fn f() {\n    let c = std::process::Command::new(\"x\");\n}\n", &forbidden).len(),
            1,
            "the matcher cannot see a bare Command::new at all"
        );
        let mut found = Vec::new();
        let mut scanned_core = 0;
        for f in &files {
            let rel = f.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/");
            if rel.starts_with("platform/") || rel == "test_support.rs" {
                continue;
            }
            if rel.starts_with("core/") {
                scanned_core += 1;
            }
            let text = std::fs::read_to_string(f).expect("read source");
            found.extend(hits(&text, &forbidden).into_iter().map(|l| format!("{rel}: {l}")));
        }
        assert!(scanned_core > 20, "the walk missed core/ — a zero from an empty scan proves nothing");
        assert!(
            found.is_empty(),
            "{} bare Command::new outside platform/ — use crate::platform::command, or a Windows \
             user gets a console window per call: {found:#?}",
            found.len()
        );
    }
}

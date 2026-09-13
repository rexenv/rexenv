//! Platform selection. `core/` calls `platform::current()` to get a
//! `&dyn Platform` and never names a concrete OS type. The right implementation
//! is chosen at compile time via `#[cfg(target_os = "...")]`.

pub mod traits;

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
        panic!("rexenv: {} is not ported to this OS yet (docs/PLAN-windows-port.md)", $what)
    };
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "linux")]
mod linux;

// The Windows owner-only SDDL builder is pure text: include it on the macOS/Linux
// test host so its tests run in `verify.sh` too, not only on a Windows machine.
#[cfg(all(test, not(target_os = "windows")))]
#[path = "windows/owner_only.rs"]
mod windows_owner_only;
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
#[cfg(target_os = "macos")]
pub use macos::relauncher::run as run_relauncher;

/// A process's start-time token — the guard's second identity for its parent.
/// Re-exported for the live check that spawns guards by hand.
#[cfg(target_os = "macos")]
pub use macos::process_start_token;

/// Bring rexenv to the front. Needed because it is an ACCESSORY app (menu bar,
/// no dock tile): nothing activates it on the user's behalf, so a window it
/// shows — or a modal it opens — can come up behind whatever the developer was
/// reading. Same re-export shape as the two above, same reason.
#[cfg(target_os = "macos")]
pub use macos::activation::activate_app;

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
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/platform/windows/mod.rs");
        let text = std::fs::read_to_string(&path).expect("read platform/windows/mod.rs");
        assert!(
            text.contains("impl Paths for WindowsPaths"),
            "not the Windows platform module — a moved file would make this scan vacuous"
        );
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
            "platform/windows has {} todo!/unimplemented! — return Error::Unported, or unported! where the trait cannot return an error: {found:?}",
            found.len()
        );
    }
}

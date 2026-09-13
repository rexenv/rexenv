// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // FIRST, before any mode: a panic anywhere below — the DNS agent, the tunnel
    // guard, the relauncher or the app — is written to crash.log, and on a
    // Windows release build shown, instead of vanishing into a console the
    // process does not have (docs/PLAN-windows-port.md §3a Q1, ledger #596).
    rexenv_lib::crash::install();

    // Headless resolver mode, run by the per-user LaunchAgent so local-TLD DNS
    // survives app quits (and is up from login). Checked BEFORE Tauri boots:
    // the agent must never open a window, touch SQLite, or start services.
    if std::env::args().any(|a| a == "--dns-agent") {
        std::process::exit(rexenv_lib::core::dns::run_agent());
    }
    // Tunnel guard: a detached watcher that ends ONE public share when the app
    // that started it dies — including a SIGKILL, which runs none of our
    // shutdown code. Checked here, before Tauri boots, for the same reason the
    // DNS agent is: it must never open a window or touch app state.
    #[cfg(target_os = "macos")]
    {
        let argv: Vec<String> = std::env::args().collect();
        if let Some(args) = rexenv_lib::core::tunnels::parse_guard_args(&argv) {
            std::process::exit(rexenv_lib::platform::run_tunnel_guard(args));
        }
        // The relauncher: waits for the process that swapped the bundle to be
        // gone, then reopens rexenv. Spawned by the OLD build, so this flag is a
        // cross-version contract — and checked here, before Tauri, because it
        // must open no window and touch no app state.
        if let Some(args) = rexenv_lib::core::app_update::parse_relaunch_args(&argv) {
            std::process::exit(rexenv_lib::platform::run_relauncher(args));
        }
    }
    rexenv_lib::run();
}

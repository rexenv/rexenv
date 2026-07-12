// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Headless resolver mode, run by the per-user LaunchAgent so local-TLD DNS
    // survives app quits (and is up from login). Checked BEFORE Tauri boots:
    // the agent must never open a window, touch SQLite, or start services.
    if std::env::args().any(|a| a == "--dns-agent") {
        std::process::exit(rexenv_lib::core::dns::run_agent());
    }
    rexenv_lib::run();
}

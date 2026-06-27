//! rexenv backend entry. Registers all Tauri commands and wires the module tree.
//!
//! Architecture: `commands/` (thin IPC) → `core/` (platform-agnostic) →
//! `platform/` (OS traits, selected via cfg). See CLAUDE.md.

pub mod commands;
pub mod core;
pub mod error;
pub mod platform;
pub mod state;
pub mod utils;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            // Start the embedded DNS resolver as a managed background task on a
            // fixed loopback port. Held in app state so it lives for the app's
            // lifetime and is aborted cleanly on exit (DnsService::drop). A bind
            // failure is logged, not fatal — the app still runs.
            match tauri::async_runtime::block_on(core::dns::DnsService::start_default()) {
                Ok(dns) => {
                    app.manage(dns);
                }
                Err(e) => {
                    log::error!("dns: failed to start embedded resolver: {e}");
                }
            }

            // Open the app SQLite database (creating it + running migrations) and
            // hold it in app state for the IPC commands.
            let platform = platform::current();
            match (
                state::db::open_for_platform(platform.paths()),
                core::ssl::load_or_create(platform.paths(), platform.permissions()),
            ) {
                (Ok(conn), Ok(ca)) => {
                    app.manage(state::app::AppState::new(conn, platform, ca));
                }
                (Err(e), _) => log::error!("db: failed to open app database: {e}"),
                (_, Err(e)) => log::error!("ssl: failed to load/create CA: {e}"),
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::system::app_info,
            commands::system::global_status,
            commands::system::port_status,
            commands::sites::list_sites,
            commands::sites::start_site,
            commands::sites::stop_site,
            commands::sites::delete_site,
            commands::services::start_services,
            commands::services::stop_services,
            commands::services::services_status,
            commands::settings::get_setting,
            commands::settings::set_setting,
            commands::settings::sites_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

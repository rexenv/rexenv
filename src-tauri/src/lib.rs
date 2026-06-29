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

            // Registry of live PTY terminal sessions (§4.1).
            app.manage(commands::terminal::Terminals::default());
            // Registry of live per-site public tunnels (§9.1).
            app.manage(commands::tunnels::Tunnels::default());

            // Open the app SQLite database (creating it + running migrations) and
            // hold it in app state for the IPC commands.
            let platform = platform::current();
            match (
                state::db::open_for_platform(platform.paths()),
                core::ssl::load_or_create(platform.paths(), platform.permissions()),
            ) {
                (Ok(conn), Ok(ca)) => {
                    // Seed/refresh the PHP version registry (Phase 2 §1.2);
                    // preserves the user's installed choices on re-run.
                    if let Err(e) = core::php::seed_registry(&conn) {
                        log::error!("php: failed to seed version registry: {e}");
                    }
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
            commands::system::open_external,
            commands::system::dns_status,
            commands::system::trust_local_ca,
            commands::system::regenerate_certs,
            commands::system::autostart_status,
            commands::system::set_autostart,
            commands::blueprints::list_blueprints,
            commands::blueprints::save_blueprint,
            commands::blueprints::delete_blueprint,
            commands::sites::list_sites,
            commands::sites::create_site,
            commands::sites::start_site,
            commands::sites::stop_site,
            commands::sites::set_site_php_version,
            commands::sites::set_site_web_server,
            commands::sites::delete_site,
            commands::php::list_php_versions,
            commands::php::set_php_version_installed,
            commands::wordpress::wp_info,
            commands::wordpress::wp_plugins,
            commands::wordpress::wp_plugin_install,
            commands::wordpress::wp_plugin_activate,
            commands::wordpress::wp_plugin_deactivate,
            commands::wordpress::wp_plugin_update,
            commands::wordpress::wp_plugin_delete,
            commands::wordpress::wp_themes,
            commands::wordpress::wp_theme_install,
            commands::wordpress::wp_theme_activate,
            commands::wordpress::wp_theme_update,
            commands::wordpress::wp_theme_delete,
            commands::wordpress::wp_users,
            commands::wordpress::wp_user_create,
            commands::wordpress::wp_user_login_url,
            commands::wordpress::wp_debug_get,
            commands::wordpress::wp_debug_set,
            commands::wordpress::wp_search_replace,
            commands::wordpress::wp_rewrite_flush,
            commands::wordpress::wp_core_update,
            commands::wordpress::wp_core_reinstall,
            commands::wordpress::wp_multisite_convert,
            commands::wordpress::wp_network_sites,
            commands::wordpress::wp_network_site_create,
            commands::wordpress::wp_network_site_delete,
            commands::wordpress::wp_plugin_activate_network,
            commands::wordpress::wp_plugin_deactivate_network,
            commands::wordpress::wp_theme_enable_network,
            commands::wordpress::wp_theme_disable_network,
            commands::wordpress::wp_super_admins,
            commands::wordpress::wp_super_admin_add,
            commands::services::start_services,
            commands::services::stop_services,
            commands::services::services_status,
            commands::logs::log_targets,
            commands::logs::tail_log,
            commands::terminal::terminal_open,
            commands::terminal::terminal_write,
            commands::terminal::terminal_resize,
            commands::terminal::terminal_close,
            commands::mail::mailpit_status,
            commands::mail::mailpit_messages,
            commands::mail::mailpit_message,
            commands::mail::mailpit_message_raw,
            commands::mail::mailpit_clear,
            commands::database::databases_status,
            commands::database::start_database,
            commands::database::stop_database,
            commands::settings::get_setting,
            commands::settings::set_setting,
            commands::settings::sites_folder,
            commands::tunnels::start_tunnel,
            commands::tunnels::stop_tunnel,
            commands::tunnels::tunnels_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

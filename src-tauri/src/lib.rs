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

            let platform = platform::current();

            // Start the embedded DNS resolver as a managed background task on a
            // fixed loopback port, gated on ports::ensure_free (a conflict names
            // the holder + a free-it command). Held in app state so it lives for
            // the app's lifetime and is aborted cleanly on exit
            // (DnsService::drop). A failure is logged, not fatal — the app
            // still runs.
            let dns = match tauri::async_runtime::block_on(core::dns::DnsService::start_default(
                platform.as_ref(),
            )) {
                Ok(dns) => Some(dns),
                Err(e) => {
                    log::error!("dns: failed to start embedded resolver: {e}");
                    None
                }
            };
            // ALWAYS managed (even as None) so status + the health watchdog can
            // read/restart it without a missing-state panic.
            app.manage(state::app::DnsState(std::sync::Mutex::new(dns)));

            // Registry of live PTY terminal sessions (§4.1).
            app.manage(commands::terminal::Terminals::default());
            // Registry of live per-site public tunnels (§9.1).
            app.manage(commands::tunnels::Tunnels::default());

            // Open the app SQLite database (creating it + running migrations) and
            // hold it in app state for the IPC commands.
            // Fatal init: open the DB + load/create the CA. On failure `AppState` can't
            // be built — so we do NOT leave it unmanaged (every AppState command would
            // then panic with a cryptic "state not managed"). Instead we record a
            // human-readable reason in the ALWAYS-managed `InitError`; the frontend
            // reads it first and shows an error screen rather than driving the app
            // (task 1.2 / H3).
            let init_error: Option<String> = match (
                state::db::open_for_platform(platform.paths()),
                core::ssl::load_or_create(platform.paths(), platform.permissions()),
            ) {
                (Ok(conn), Ok(ca)) => {
                    // Seed/refresh the PHP version registry (Phase 2 §1.2);
                    // preserves the user's installed choices on re-run.
                    if let Err(e) = core::php::seed_registry(&conn) {
                        log::error!("php: failed to seed version registry: {e}");
                    }
                    // Services OUTLIVE the app: closing rexenv doesn't stop the
                    // stack, so adopt any rexenv-owned survivors into this session's
                    // manager — status shows them running, Stop all works, Start all
                    // skips them. (Replaces the old stop-orphans-at-boot behavior.)
                    let sites = core::sites::list(&conn).unwrap_or_default();
                    let state = state::app::AppState::new(conn, platform, ca);
                    {
                        let mut mgr = tauri::async_runtime::block_on(state.services.lock());
                        let adopted = mgr.adopt_startup(state.platform.as_ref(), &sites);
                        if adopted > 0 {
                            log::info!("adopted {adopted} running service(s) from a prior session");
                        }
                    }
                    app.manage(state);
                    None
                }
                (Err(e), _) => {
                    log::error!("db: failed to open app database: {e}");
                    Some(format!(
                        "rexenv couldn't open its local database.\n\n{e}\n\nThis usually means \
                         its data folder isn't writable or the disk is full. Fix that, then reopen rexenv."
                    ))
                }
                (_, Err(e)) => {
                    log::error!("ssl: failed to load/create CA: {e}");
                    Some(format!(
                        "rexenv couldn't set up its local certificate authority.\n\n{e}\n\nThis \
                         usually means its data folder isn't writable. Fix that, then reopen rexenv."
                    ))
                }
            };
            app.manage(commands::system::InitError(init_error));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::system::app_info,
            commands::system::init_error,
            commands::system::global_status,
            commands::system::open_external,
            commands::system::dns_status,
            commands::system::system_setup,
            commands::system::trust_local_ca,
            commands::system::regenerate_certs,
            commands::system::autostart_status,
            commands::system::set_autostart,
            commands::system::uninstall_system,
            commands::blueprints::list_blueprints,
            commands::blueprints::save_blueprint,
            commands::blueprints::delete_blueprint,
            commands::sites::list_sites,
            commands::sites::sites_serving,
            commands::sites::create_site,
            commands::sites::rename_site,
            commands::sites::set_site_php_version,
            commands::sites::set_site_web_server,
            commands::sites::delete_site,
            commands::php::list_php_versions,
            commands::php::set_php_version_installed,
            commands::php::set_default_php_version,
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

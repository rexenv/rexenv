//! Tauri IPC handlers — THIN. Each command translates an IPC call into a
//! `core/` call and returns serializable data. No business logic here.

pub mod blueprints;
pub mod database;
pub mod db_import;
pub mod rewrite;
pub mod downloads;
pub mod logs;
pub mod mail;
/// Unix-only: IPC for the opt-in MCP endpoint (mirrors the `mcp_server` module).
#[cfg(unix)]
pub mod mcp;
pub mod php;
pub mod repo;
pub mod services;
pub mod settings;
pub mod site_provision;
pub mod sites;
pub mod system;
pub mod terminal;
pub mod tunnels;
pub mod valet_import;
pub mod wordpress;
pub mod wp_install;

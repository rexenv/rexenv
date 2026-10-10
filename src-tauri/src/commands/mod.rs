//! Tauri IPC handlers — THIN. Each command translates an IPC call into a
//! `core/` call and returns serializable data. No business logic here.

pub mod app_update;
pub mod blueprints;
pub mod database;
pub mod db_import;
pub mod rewrite;
pub mod downloads;
pub mod logs;
pub mod mail;
/// IPC for the opt-in MCP endpoint and the agent-activity feed. Compiled on every
/// OS: only the endpoint's unix-socket transport is `cfg(unix)` (inside
/// `mcp_server`), and on a target without it `mcp_set_enabled` refuses rather than
/// reading on (docs/PLAN-windows-port.md W1).
pub mod mcp;
pub mod php;
pub mod repo;
pub mod services;
pub mod settings;
pub mod site_provision;
pub mod scratch;
pub mod sites;
pub mod system;
pub mod terminal;
pub mod tunnels;
pub mod valet_import;
pub mod wordpress;
pub mod live_sync;
pub mod worktree;
pub mod wp_install;

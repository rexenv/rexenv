//! Tauri IPC handlers — THIN. Each command translates an IPC call into a
//! `core/` call and returns serializable data. No business logic here.

pub mod database;
pub mod logs;
pub mod mail;
pub mod php;
pub mod services;
pub mod settings;
pub mod sites;
pub mod system;
pub mod terminal;
pub mod tunnels;
pub mod wordpress;

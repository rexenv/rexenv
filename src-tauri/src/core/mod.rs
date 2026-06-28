//! Domain logic — PLATFORM-AGNOSTIC ("the what"). Modules here orchestrate
//! sites, services, DNS, SSL, the edge router, etc. by calling `platform/`
//! traits. They must NEVER import OS-specific code directly.
//!
//! Submodules start as files and grow into folders as each Phase 1 task lands.

pub mod adminer;
pub mod binaries;
pub mod database;
pub mod db;
pub mod dns;
pub mod frankenphp;
pub mod logs;
pub mod mail;
pub mod monitor;
pub mod php;
pub mod ports;
pub mod postgres;
pub mod proxy;
pub mod service_manager;
pub mod services;
pub mod setup;
pub mod sites;
pub mod ssl;
pub mod terminal;
pub mod tunnels;
pub mod wp_login;
pub mod wordpress;

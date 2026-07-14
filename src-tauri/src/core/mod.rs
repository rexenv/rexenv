//! Domain logic — PLATFORM-AGNOSTIC ("the what"). Modules here orchestrate
//! sites, services, DNS, SSL, the edge router, etc. by calling `platform/`
//! traits. They must NEVER import OS-specific code directly.
//!
//! Submodules start as files and grow into folders as each Phase 1 task lands.

pub mod adminer;
pub mod binaries;
pub mod blueprints;
pub mod database;
pub mod db;
pub mod dns;
pub mod downloads;
pub mod firefox;
pub mod frankenphp;
pub mod logs;
pub mod mail;
pub mod mariadb;
pub mod monitor;
pub mod php;
pub mod ports;
pub mod postgres;
pub mod proc;
pub mod proxy;
pub mod redis;
pub mod service_manager;
pub mod services;
pub mod setup;
pub mod site_env;
pub mod site_metrics;
pub mod sites;
pub mod ssl;
pub mod terminal;
pub mod tld;
pub mod tunnels;
pub mod wp_login;
pub mod wporg;
pub mod wp_tunnel;
pub mod wordpress;

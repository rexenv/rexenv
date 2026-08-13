//! Domain logic — PLATFORM-AGNOSTIC ("the what"). Modules here orchestrate
//! sites, services, DNS, SSL, the edge router, etc. by calling `platform/`
//! traits. They must NEVER import OS-specific code directly.
//!
//! Submodules start as files and grow into folders as each Phase 1 task lands.

pub mod adminer;
pub mod apache;
pub mod binaries;
pub mod blueprints;
pub mod cli;
pub mod confedit;
/// Test-only: the must-say copy guards' shared scanner (see its module doc —
/// two guards independently shipped the same defect before it was extracted).
#[cfg(test)]
pub(crate) mod copy_scan;
pub mod confrewrite;
pub mod confverify;
pub mod database;
pub mod db;
pub mod dbcompat;
pub mod dbdump;
pub mod dbimport;
pub mod dbmirror;
pub mod dbrestore;
pub mod dbsource;
pub mod devtools;
pub mod dist_archive;
pub mod dns;
pub mod dotenv;
pub mod downloads;
pub mod firefox;
pub mod frankenphp;
pub mod laravel;
pub mod logs;
pub mod mail;
pub mod mariadb;
pub mod monitor;
pub mod php;
pub mod phpconf;
pub mod ports;
pub mod postgres;
pub mod proc;
pub mod proxy;
pub mod redis;
pub mod repo;
pub mod scratch;
pub mod service_manager;
pub mod services;
pub mod setup;
pub mod site_env;
pub mod site_metrics;
pub mod sites;
pub mod ssl;
pub mod stack_guard;
pub mod terminal;
pub mod tld;
pub mod valet;
pub mod tunnels;
pub mod wp_dns;
pub mod wp_login;
pub mod wporg;
pub mod wp_mailtag;
pub mod wp_packages;
pub mod wp_tunnel;
pub mod wordpress;

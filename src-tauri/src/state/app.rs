//! App-wide state held by Tauri (`app.manage`) and accessed from commands.

use crate::core::monitor::Monitor;
use crate::core::service_manager::{DbInfo, ServiceInfo, ServiceManager};
use crate::core::ssl::LocalCa;
use crate::platform::traits::Platform;
use rusqlite::Connection;
use std::sync::Mutex;

/// Shared application state. The SQLite connection is behind a `Mutex` (rusqlite
/// `Connection` is `Send` but not `Sync`); commands lock it briefly. The platform
/// impl backs filesystem ops; the `Monitor` is kept across polls; the `LocalCa`
/// is loaded once; the `ServiceManager` owns the shared-service lifecycle and is
/// behind an async `Mutex` (its `start_all` is async — downloads binaries).
pub struct AppState {
    pub db: Mutex<Connection>,
    pub platform: Box<dyn Platform>,
    pub monitor: Mutex<Monitor>,
    pub ca: LocalCa,
    pub services: tauri::async_runtime::Mutex<ServiceManager>,
    /// Last successful status snapshots. Served when `services` is locked by a
    /// long start/stop so status polls never block the UI (try_lock fallback).
    pub service_status_cache: Mutex<Vec<ServiceInfo>>,
    pub db_status_cache: Mutex<Vec<DbInfo>>,
}

impl AppState {
    pub fn new(conn: Connection, platform: Box<dyn Platform>, ca: LocalCa) -> Self {
        Self {
            db: Mutex::new(conn),
            platform,
            monitor: Mutex::new(Monitor::new()),
            ca,
            services: tauri::async_runtime::Mutex::new(ServiceManager::default()),
            service_status_cache: Mutex::new(Vec::new()),
            db_status_cache: Mutex::new(Vec::new()),
        }
    }
}

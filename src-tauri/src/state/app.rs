//! App-wide state held by Tauri (`app.manage`) and accessed from commands.

use crate::core::monitor::Monitor;
use crate::core::service_manager::{DbInfo, ServiceInfo, ServiceManager};
use crate::core::ssl::LocalCa;
use crate::platform::traits::Platform;
use rusqlite::Connection;
use std::sync::Mutex;

/// Who serves local-TLD DNS right now. `Agent` is the goal state: resolution
/// survives app quits (the observed sites-die-after-quit failure was exactly the
/// old always-in-process resolver dying with the app). `InProcess` is the
/// automatic fallback when the agent can't come up, so DNS never regresses below
/// the pre-agent behavior — but it dies with the app, and Settings says so.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DnsMode {
    /// Served by the per-user LaunchAgent (`rexenv --dns-agent`) — survives quits.
    Agent,
    /// Served by the legacy in-process task — dies with the app.
    InProcess,
    /// Nothing serving (agent install AND in-process bind both failed).
    Down,
}

/// DNS resolution state, managed as its OWN Tauri state (separate from
/// `AppState` — it starts before, and must survive failure of, the DB/CA init).
/// ALWAYS managed: `service` is `Some` only in `InProcess` mode; in `Agent` mode
/// the resolver lives in the LaunchAgent's process and liveness is probed over
/// the wire (`dns::answers_as_ours`).
pub struct DnsState {
    pub service: Mutex<Option<crate::core::dns::DnsService>>,
    pub mode: Mutex<DnsMode>,
}

impl DnsState {
    pub fn new(service: Option<crate::core::dns::DnsService>, mode: DnsMode) -> Self {
        Self { service: Mutex::new(service), mode: Mutex::new(mode) }
    }

    /// Whether the IN-PROCESS resolver task is alive (authoritative for
    /// `InProcess` mode only — a panicked/returned task reads false). Agent-mode
    /// liveness is `dns::answers_as_ours` instead.
    pub fn running(&self) -> bool {
        self.service
            .lock()
            .map(|g| g.as_ref().is_some_and(|d| d.is_running()))
            .unwrap_or(false)
    }

    pub fn mode(&self) -> DnsMode {
        self.mode.lock().map(|g| *g).unwrap_or(DnsMode::Down)
    }

    pub fn set(&self, service: Option<crate::core::dns::DnsService>, mode: DnsMode) {
        if let Ok(mut g) = self.service.lock() {
            *g = service;
        }
        if let Ok(mut g) = self.mode.lock() {
            *g = mode;
        }
    }
}

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
    /// Domain of the database import currently running, if any (one at a time).
    /// Lives HERE rather than in the job registry so `site_provision::start`
    /// can refuse a provision/retry for a site whose database is mid-import —
    /// the two jobs would otherwise race on the same site's database and edge.
    pub db_import_active: Mutex<Option<String>>,
    /// Domain of the connection rewrite currently applying/reverting, if any
    /// (one at a time) — the same cross-guard shape as `db_import_active`:
    /// provision and db-import refuse while a rewrite holds the site's config
    /// file, and the rewrite refuses while they run.
    pub rewrite_active: Mutex<Option<String>>,
}

impl AppState {
    /// Live per-service snapshot — the SINGLE source of truth for "what is running".
    /// Both `services_status` (Services tab) and `global_status` (sidebar footer)
    /// read through here, so the two views can never disagree about whether the
    /// stack is up. Non-blocking: if a long start/stop holds `services`, return the
    /// last cached snapshot so status polls never freeze the UI.
    pub fn service_infos(&self) -> Vec<ServiceInfo> {
        // Installed PHP minors from the registry, so every installed pool is listed
        // (idle) even before Start all. Brief DB lock, released before the manager
        // try_lock — never held across it.
        let installed_php = self
            .db
            .lock()
            .ok()
            .and_then(|conn| crate::core::php::installed_minors(&conn).ok())
            .unwrap_or_default();
        match self.services.try_lock() {
            Ok(mgr) => {
                let infos = mgr.status(self.platform.as_ref(), &installed_php);
                if let Ok(mut cache) = self.service_status_cache.lock() {
                    *cache = infos.clone();
                }
                infos
            }
            Err(_) => self
                .service_status_cache
                .lock()
                .map(|c| c.clone())
                .unwrap_or_default(),
        }
    }

    pub fn new(conn: Connection, platform: Box<dyn Platform>, ca: LocalCa) -> Self {
        Self {
            db: Mutex::new(conn),
            platform,
            monitor: Mutex::new(Monitor::new()),
            ca,
            services: tauri::async_runtime::Mutex::new(ServiceManager::default()),
            service_status_cache: Mutex::new(Vec::new()),
            db_status_cache: Mutex::new(Vec::new()),
            db_import_active: Mutex::new(None),
            rewrite_active: Mutex::new(None),
        }
    }
}

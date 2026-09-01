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

    /// Take the in-process resolver OUT, leaving the state saying it is not
    /// running — because it is not. Used by the agent handoff, which must
    /// release the port before the agent can bind it; dropping the returned
    /// service is what closes the socket.
    ///
    /// Deliberately not `stop()`-in-place: a stopped-but-present service reads
    /// as `running() == false` with a `Some` in the slot, which is a state
    /// nothing else in the app knows how to interpret.
    pub fn take_service(&self) -> Option<crate::core::dns::DnsService> {
        self.service.lock().ok().and_then(|mut g| g.take())
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
    /// Live control of the opt-in MCP endpoint (Settings → AI agents). Holds the
    /// running server's shutdown handle so the toggle can bind/unbind the socket
    /// at runtime; `None` shutdown = not serving. Unix-only, mirroring the
    /// unix-socket `mcp_server` module.
    #[cfg(unix)]
    pub mcp: Mutex<crate::mcp_server::McpControl>,
    /// Agents' outstanding asks to read a real site's database (MCP M3).
    /// In memory and session-scoped by design — see `core::agent_db::GrantRequests`:
    /// a consent prompt whose context is gone is not consent.
    pub agent_db_requests: Mutex<crate::core::agent_db::GrantRequests>,
    /// Auto-allow for database consent (session-scoped — see
    /// `core::agent_db::AutoAllow`). Deliberately NOT a settings row: it must
    /// not survive a restart.
    pub agent_db_auto_allow: Mutex<crate::core::agent_db::AutoAllow>,
}

impl AppState {
    /// Live per-service snapshot — the SINGLE source of truth for "what is running".
    /// Both `services_status` (Services tab) and `global_status` (sidebar footer)
    /// read through here, so the two views can never disagree about whether the
    /// stack is up. Non-blocking: if a long start/stop holds `services`, return the
    /// last cached snapshot so status polls never freeze the UI.
    pub fn service_infos(&self) -> Vec<ServiceInfo> {
        self.service_infos_fresh().0
    }

    /// The same snapshot, plus whether it is FRESH. The bool is what the tray
    /// menu needs and the UI does not: the Services screen re-polls a second
    /// later and corrects itself, while a menu built from a cached snapshot may
    /// sit on screen unchanged for as long as the user holds it open. `false`
    /// means the services lock was busy and these rows are the previous
    /// snapshot — the caller must label them rather than present them as now.
    pub fn service_infos_fresh(&self) -> (Vec<ServiceInfo>, bool) {
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
                (infos, true)
            }
            Err(_) => (
                self.service_status_cache.lock().map(|c| c.clone()).unwrap_or_default(),
                false,
            ),
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
            #[cfg(unix)]
            mcp: Mutex::new(crate::mcp_server::McpControl::default()),
            agent_db_requests: Mutex::new(Default::default()),
            agent_db_auto_allow: Mutex::new(Default::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The handoff's first move is to RELEASE the port, and the state has to
    /// tell the truth about that for the seconds it is in flight: no service,
    /// and `running()` false. A `stop()`-in-place would leave a `Some` that
    /// reads as not-running — a state the status command, the watchdog and the
    /// tray would each have to learn to interpret, which is three chances to
    /// interpret it differently.
    #[tokio::test]
    async fn taking_the_resolver_out_leaves_the_state_saying_nothing_is_running() {
        // Ephemeral port: this test binds a real socket, and must never reach
        // for the fixed one the developer's own resolver is on.
        let svc = crate::core::dns::DnsService::start(0).await.expect("bind an ephemeral port");
        let state = DnsState::new(Some(svc), DnsMode::InProcess);
        assert!(state.running(), "fixture must start with a live resolver");

        let taken = state.take_service();
        assert!(taken.is_some(), "the caller gets the service, and dropping it frees the port");
        assert!(!state.running(), "with the service taken, nothing is running and it says so");
        // A second take is empty rather than a panic: the handoff can run twice.
        assert!(state.take_service().is_none());
    }
}

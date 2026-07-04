//! commands::system — app-level IPC. App info + the global resource/status block.

use crate::core;
use crate::error::{Error, Result};
use crate::state::app::AppState;
use serde::Serialize;
use tauri::State;

/// Mirrors the frontend `AppInfo` type in `src/types/index.ts`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub tauri_version: String,
    /// Human-readable OS + CPU, e.g. `macOS · Apple silicon` — derived from the
    /// build target, not hardcoded, so it stays correct on Windows/Linux/Intel.
    pub platform: String,
}

/// A friendly "OS · CPU" label from the compile-time target (`std::env::consts`).
fn platform_label() -> String {
    let os = match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    };
    let arch = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "Apple silicon",
        ("macos", "x86_64") => "Intel",
        (_, "aarch64") => "ARM64",
        (_, "x86_64") => "x64",
        (_, other) => other,
    };
    format!("{os} · {arch}")
}

/// Round-trip smoke test for the typed IPC bridge (Phase 1 task 0.5).
#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        name: "rexenv".into(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        tauri_version: tauri::VERSION.to_string(),
        platform: platform_label(),
    }
}

/// Startup init outcome, ALWAYS managed — unlike `AppState`, which is absent when
/// init fails. `Some(msg)` = a fatal DB/CA failure the frontend should surface instead
/// of driving the app (task 1.2 / H3). Reading it can never panic.
pub struct InitError(pub Option<String>);

/// The fatal startup error, if any. The frontend calls this FIRST and, when it's
/// `Some`, shows an error screen without touching AppState-backed commands (which
/// would panic with "state not managed" while `AppState` is absent).
#[tauri::command]
pub fn init_error(state: State<'_, InitError>) -> Option<String> {
    state.0.clone()
}

/// The sidebar status footer's global block. Mirrors the frontend `GlobalStatus`
/// type. CPU/RAM are real system totals (`sysinfo`); running/total count LIVE
/// services (the single source of truth shared with `services_status`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalStatus {
    /// "all" | "partial" | "stopped".
    pub summary: &'static str,
    pub running: u32,
    pub total: u32,
    pub cpu_percent: f32,
    pub ram_mb: u64,
    pub ram_total_mb: u64,
}

/// Live global status for the sidebar footer (task 7.4): real system CPU/RAM via
/// `sysinfo` + a running/total derived from LIVE service status (`ServiceManager`),
/// the single source of truth shared with `services_status` so the footer and the
/// Services tab can never disagree about whether the stack is up.
#[tauri::command]
pub fn global_status(state: State<'_, AppState>) -> Result<GlobalStatus> {
    let metrics = {
        let mut monitor = state
            .monitor
            .lock()
            .map_err(|_| Error::Other("monitor lock poisoned".into()))?;
        monitor.sample()
    };

    // Running/total/summary derive from live service status (NOT the sites table),
    // via the same `AppState::service_infos` the Services tab reads.
    let (running, total, summary) = summarize(&state.service_infos());

    Ok(GlobalStatus {
        summary,
        running,
        total,
        cpu_percent: metrics.cpu_percent,
        ram_mb: metrics.ram_used_mb,
        ram_total_mb: metrics.ram_total_mb,
    })
}

/// Reduce a live per-service snapshot to the footer's running/total/summary.
/// Pure (no locks/DB) so it is unit-testable and pins the invariant: "running"
/// counts RUNNING SERVICES, never site DB rows.
fn summarize(infos: &[crate::core::service_manager::ServiceInfo]) -> (u32, u32, &'static str) {
    let total = infos.len() as u32;
    let running = infos.iter().filter(|i| i.running).count() as u32;
    let summary = if running == 0 {
        "stopped"
    } else if running == total {
        "all"
    } else {
        "partial"
    };
    (running, total, summary)
}

/// Open a path or URL in the OS default handler — Finder for a docroot, the
/// default browser for an `http(s)` link (Phase 3 §1.3 Overview quick links).
#[tauri::command]
pub fn open_external(state: State<'_, AppState>, target: String) -> Result<()> {
    state.platform.shell().open(&target)
}

// ── DNS & SSL + autostart (Settings, §11.1) ──────────────────────────────────

/// DNS resolver health for the Settings indicator (mirrors the frontend `DnsStatus`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsStatus {
    /// The embedded resolver is bound on its loopback UDP port.
    pub running: bool,
    pub port: u16,
    /// The OS resolver file (`/etc/resolver/test`) is installed.
    pub resolver_installed: bool,
    pub resolver_path: String,
}

/// Embedded-DNS + OS-resolver status. `running` probes the loopback UDP port (the
/// resolver binds UDP, so the TCP `is_listening` check doesn't apply); a failed
/// bind means something — our in-process resolver — already holds it.
#[tauri::command]
pub fn dns_status(state: State<'_, AppState>) -> DnsStatus {
    let port = core::dns::DEFAULT_DNS_PORT;
    let running = core::dns::port_bound(port);
    let path = state.platform.dns().resolver_path();
    DnsStatus {
        running,
        port,
        resolver_installed: path.exists(),
        resolver_path: path.display().to_string(),
    }
}

/// Run first-run system setup (§3.4): install the `.test` OS resolver (one admin
/// prompt) + trust the local CA (native keychain dialog). Idempotent — safe to
/// re-run. Backs the Onboarding "Set up domains & SSL" step. `async` so the blocking
/// privileged prompts run off the UI thread (same handling as `start_services`).
#[tauri::command]
pub async fn system_setup(state: State<'_, AppState>) -> Result<()> {
    core::setup::run_system_setup(state.platform.as_ref())?;
    Ok(())
}

/// Re-trust the local CA in the user trust store (macOS login keychain — shows the
/// native auth dialog, no root). Idempotent: re-adding an already-trusted cert is fine.
#[tauri::command]
pub fn trust_local_ca(state: State<'_, AppState>) -> Result<()> {
    core::ssl::trust_ca(state.platform.as_ref(), &state.ca)
}

/// Regenerate every site's TLS cert (delete + re-issue from the local CA), plus the
/// internal Adminer vhost cert, then reload the edge if the stack is running. Returns
/// how many certs were re-issued. Use after re-trusting the CA or if a cert is stale.
#[tauri::command]
pub async fn regenerate_certs(state: State<'_, AppState>) -> Result<u32> {
    let sites = {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        core::sites::list(&conn)?
    };
    let paths = state.platform.paths();
    let perms = state.platform.permissions();
    let mut count = 0u32;
    let reissue = |domain: &str| -> Result<()> {
        // Delete first so the idempotent issuer actually regenerates the material.
        let dir = core::ssl::site_cert_dir(paths, domain)?;
        let _ = std::fs::remove_dir_all(&dir);
        core::ssl::ensure_site_cert(paths, perms, &state.ca, domain)?;
        Ok(())
    };
    for s in &sites {
        reissue(&s.domain)?;
        count += 1;
    }
    reissue(core::adminer::ADMINER_HOST)?;

    // Reload the edge so Caddy serves the fresh certs (only if it's up).
    let mut mgr = state.services.lock().await;
    if mgr.is_running() {
        mgr.reload(state.platform.as_ref(), &state.ca, &sites).await?;
    }
    Ok(count)
}

/// Whether rexenv is set to start on login.
#[tauri::command]
pub fn autostart_status(state: State<'_, AppState>) -> Result<bool> {
    state.platform.autostart().is_enabled()
}

/// Enable/disable "Start rexenv on login" via the platform `AutostartManager`
/// (macOS: a per-user launchd LaunchAgent).
#[tauri::command]
pub fn set_autostart(state: State<'_, AppState>, enabled: bool) -> Result<()> {
    if enabled {
        state.platform.autostart().enable()
    } else {
        state.platform.autostart().disable()
    }
}

/// Reverse rexenv's system-level changes (Settings → "Remove system changes", §3.1):
/// stop ALL services (so the edge releases :80/:443), then remove the `.test` DNS
/// resolver (`/etc/resolver/test`, admin prompt) and untrust the local CA — leaving
/// the machine as if rexenv's system setup never ran. Site files + databases under
/// app-data are NOT touched (the user can still delete the app + its support dir).
#[tauri::command]
pub async fn uninstall_system(state: State<'_, AppState>) -> Result<()> {
    {
        let mut mgr = state.services.lock().await;
        if mgr.is_running() {
            mgr.stop_all(state.platform.as_ref())?;
        }
    }
    core::setup::run_system_teardown(state.platform.as_ref())
}

#[cfg(test)]
mod tests {
    use super::summarize;
    use crate::core::service_manager::ServiceInfo;

    fn svc(name: &str, running: bool) -> ServiceInfo {
        ServiceInfo { name: name.into(), running, pid: None, port: 0 }
    }

    /// Pins the fix: `global_status` running/total/summary come from RUNNING
    /// SERVICES, never site DB rows — so the footer (global_status) and the
    /// Services tab (services_status) can't drift apart.
    #[test]
    fn summarize_reflects_running_services_not_sites() {
        // Nothing listed → stopped.
        assert_eq!(summarize(&[]), (0, 0, "stopped"));
        // Services present but none running → stopped. (Even if a site row were
        // marked Running elsewhere, summarize never sees sites — that's the point.)
        assert_eq!(
            summarize(&[svc("Nginx", false), svc("MySQL", false)]),
            (0, 2, "stopped"),
        );
        // Some running → partial — the exact case the footer used to get wrong
        // (stack up, zero sites DB-marked Running ⇒ must still be "running").
        assert_eq!(
            summarize(&[svc("Nginx", true), svc("MySQL", false)]),
            (1, 2, "partial"),
        );
        // All running → all.
        assert_eq!(
            summarize(&[svc("Nginx", true), svc("MySQL", true)]),
            (2, 2, "all"),
        );
    }
}

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
/// type. CPU/RAM are REXENV'S OWN totals — the sum of every supervised process
/// tree (masters + workers, incl. the root edge) from the same enriched rows the
/// Services tab shows — NOT the whole machine's usage. `cpu_percent` is a sum of
/// per-core percents (Activity-Monitor style, can exceed 100); divide by
/// `cpu_cores` for a 0-100 machine share. `ram_total_mb` is the machine's RAM,
/// kept as the meter denominator ("rexenv uses X of Y GB").
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalStatus {
    /// "all" | "partial" | "stopped".
    pub summary: &'static str,
    pub running: u32,
    pub total: u32,
    pub cpu_percent: f32,
    pub cpu_cores: u32,
    pub ram_mb: u64,
    pub ram_total_mb: u64,
}

/// Live global status for the sidebar footer (task 7.4): running/total AND the
/// app-total CPU/RAM both derive from `enriched_status` — the single monitor
/// source of truth shared with `services_status`, so the footer and the
/// Services tab can never disagree (about liveness OR resources).
#[tauri::command]
pub fn global_status(state: State<'_, AppState>) -> Result<GlobalStatus> {
    let rows = crate::commands::services::enriched_status(&state)?;
    let (running, total, summary) =
        summarize(&rows.iter().map(|r| (r.running, r.optional)).collect::<Vec<_>>());
    let cpu_percent = rows.iter().map(|r| r.cpu_percent).sum();
    let ram_mb = rows.iter().map(|r| r.ram_mb).sum();
    let (cpu_cores, ram_total_mb) = {
        let mut monitor = state
            .monitor
            .lock()
            .map_err(|_| Error::Other("monitor lock poisoned".into()))?;
        (monitor.cpu_cores(), monitor.machine_ram_total_mb())
    };
    Ok(GlobalStatus { summary, running, total, cpu_percent, cpu_cores, ram_mb, ram_total_mb })
}

/// Reduce live per-service `(running, optional)` flags to the footer's
/// running/total/summary. Pure (no locks/DB) so it is unit-testable. Pins two
/// invariants: "running" counts RUNNING SERVICES, never site DB rows; and an
/// OPTIONAL service (a user-toggled engine like Postgres, which Start-all
/// never starts) counts only WHILE RUNNING — otherwise a never-used engine
/// pins the footer at "Partial" forever.
fn summarize(flags: &[(bool, bool)]) -> (u32, u32, &'static str) {
    let counted: Vec<bool> = flags
        .iter()
        .filter(|(running, optional)| *running || !*optional)
        .map(|(running, _)| *running)
        .collect();
    let total = counted.len() as u32;
    let running = counted.iter().filter(|r| **r).count() as u32;
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

/// Reveal a file in the OS file manager with the file selected — e.g. the
/// "Show in Finder" action on the database-export success toast.
#[tauri::command]
pub fn reveal_path(state: State<'_, AppState>, path: String) -> Result<()> {
    state.platform.shell().reveal(&path)
}

/// Code editors installed on this machine, detection-ordered (first = default
/// when no `preferred_editor` setting is stored). Empty = none found.
#[tauri::command]
pub fn list_editors(state: State<'_, AppState>) -> Vec<crate::platform::traits::EditorApp> {
    state.platform.shell().detect_editors()
}

/// Open a site's folder as a PROJECT in the given detected editor ("Open in
/// editor", Sites row menu). The honest no-editor fallback lives in the UI —
/// this command errors rather than silently opening Finder.
#[tauri::command]
pub fn open_in_editor(state: State<'_, AppState>, editor_id: String, path: String) -> Result<()> {
    state.platform.shell().open_in_editor(&editor_id, &path)
}

// ── DNS & SSL + autostart (Settings, §11.1) ──────────────────────────────────

/// DNS resolver health for the Settings indicator (mirrors the frontend `DnsStatus`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsStatus {
    /// A resolver with OUR semantics answers on the loopback UDP port.
    pub running: bool,
    /// Who serves DNS: "agent" (LaunchAgent — survives app quits), "in-process"
    /// (legacy fallback — dies with the app), or "down".
    pub mode: &'static str,
    pub port: u16,
    /// The backbone OS resolver file (`/etc/resolver/rex`) is installed.
    pub resolver_installed: bool,
    pub resolver_path: String,
    /// The local CA is trusted for THIS OS user (macOS: login keychain). Per-user,
    /// unlike the resolver file — a fresh account needs its own trust step even
    /// when the resolver already exists, so first-run routing checks BOTH.
    pub ca_trusted: bool,
}

/// DNS + OS-resolver + CA-trust status. `running` is a REAL wire probe
/// (`answers_as_ours`: an A query must come back `127.0.0.1`) — true for both
/// the agent and the in-process fallback, false for a foreign port-holder; the
/// in-process task handle is the cheap fast path.
#[tauri::command]
pub fn dns_status(
    state: State<'_, AppState>,
    dns: State<'_, crate::state::app::DnsState>,
) -> DnsStatus {
    let port = core::dns::DEFAULT_DNS_PORT;
    let running = dns.running() || core::dns::answers_as_ours(port);
    let mode = match dns.mode() {
        crate::state::app::DnsMode::Agent => "agent",
        crate::state::app::DnsMode::InProcess => "in-process",
        crate::state::app::DnsMode::Down => "down",
    };
    // The Settings indicator reports the BACKBONE (.rex) resolver file — the
    // one system setup installs and that always stays active.
    let path = state.platform.dns().resolver_path(core::tld::BACKBONE_TLD);
    DnsStatus {
        running,
        mode,
        port,
        resolver_installed: path.exists(),
        resolver_path: path.display().to_string(),
        ca_trusted: state.platform.cert_trust().is_trusted(&state.ca.cert_path),
    }
}

/// `rex` CLI install status for the Settings card.
#[tauri::command]
pub fn cli_status(state: State<'_, AppState>) -> Result<core::cli::CliStatus> {
    core::cli::status(state.platform.as_ref())
}

/// Install/refresh the `rex` PATH symlink. May show ONE admin prompt
/// (`/usr/local/bin` is root-owned on most machines) — `async` so the
/// blocking prompt runs off the UI thread, same handling as `system_setup`.
/// Returns the refreshed status so the card updates in one round-trip.
#[tauri::command]
pub async fn cli_install(state: State<'_, AppState>) -> Result<core::cli::CliStatus> {
    core::cli::install(state.platform.as_ref())?;
    core::cli::status(state.platform.as_ref())
}

/// Run first-run system setup (§3.4): install the `.rex` backbone OS resolver (one admin
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

/// Firefox trust state for the Settings SSL card, plus the CA file path for the
/// manual-import fallback (Firefox keeps its OWN trust store — see `core::firefox`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirefoxTrustStatus {
    #[serde(flatten)]
    pub trust: core::firefox::FirefoxTrust,
    /// The CA certificate to import manually (Authorities → Import).
    pub ca_path: String,
}

fn firefox_status_for(state: &State<'_, AppState>) -> FirefoxTrustStatus {
    let root = state.platform.cert_trust().firefox_profiles_root();
    FirefoxTrustStatus {
        trust: core::firefox::status(root.as_deref()),
        ca_path: state.ca.cert_path.display().to_string(),
    }
}

/// Firefox detection + per-profile pref state (Settings SSL card).
#[tauri::command]
pub fn firefox_trust_status(state: State<'_, AppState>) -> FirefoxTrustStatus {
    firefox_status_for(&state)
}

/// Force `security.enterprise_roots.enabled` (import OS trust-store roots — our
/// CA) in every Firefox profile via `user.js`. Plain file writes, no prompt;
/// takes effect when Firefox restarts. Returns the refreshed status.
#[tauri::command]
pub fn trust_ca_in_firefox(state: State<'_, AppState>) -> Result<FirefoxTrustStatus> {
    let Some(root) = state.platform.cert_trust().firefox_profiles_root() else {
        return Err(crate::error::Error::Other(
            "Firefox was not found for this user (no profiles.ini).".into(),
        ));
    };
    let written = core::firefox::enable_in_profiles(&root)?;
    log::info!("firefox: forced OS-root import in {written} profile(s)");
    Ok(firefox_status_for(&state))
}

/// FORCED edge reload after a cert re-issue, with an honest failure: cert paths
/// are stable, so re-issuing leaves the Caddyfile byte-identical and a plain
/// reload is skipped by Caddy (the OLD leaf stays in its in-memory cache until
/// an edge restart) — `force: true` is what makes the new cert actually served.
/// On failure the previous (still valid) cert keeps being served — the new pair
/// is on disk and picked up at the next successful reload/start — so the error
/// says exactly that. No-op when the stack isn't running: the cert loads at the
/// next start. Awaits backend readiness with the services lock released (M4).
pub(crate) async fn reload_edge_for_new_certs(
    state: &State<'_, AppState>,
    sites: &[crate::state::models::Site],
) -> Result<()> {
    let run = async {
        let checks = {
            let mut mgr = state.services.lock().await;
            if mgr.is_running() {
                mgr.reload(state.platform.as_ref(), &state.ca, sites, true).await?
            } else {
                Vec::new()
            }
        };
        core::service_manager::await_ready(checks).await
    };
    run.await.map_err(|e| {
        Error::Other(format!(
            "Certificate re-issued, but the edge reload failed — the previous \
             certificate is still being served. Retry, or restart services. ({e})"
        ))
    })
}

/// Regenerate every site's TLS cert (re-issue from the local CA — atomic, the old
/// pair survives a failed issuance), plus the internal Adminer vhost cert, then
/// FORCE-reload the edge if the stack is running so Caddy actually serves the new
/// leaves. Returns how many certs were re-issued. Use after re-trusting the CA or
/// if a cert is stale.
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
    for s in &sites {
        core::ssl::reissue_site_cert(paths, perms, &state.ca, &s.domain)?;
        count += 1;
    }
    core::ssl::reissue_site_cert(paths, perms, &state.ca, core::adminer::ADMINER_HOST)?;

    reload_edge_for_new_certs(&state, &sites).await?;
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
/// stop ALL services (so the edge releases :80/:443), then remove EVERY rexenv
/// DNS resolver file (`/etc/resolver/<tld>` matching our signature — `.rex` plus
/// any TLDs added on demand; admin prompt) and untrust the local CA — leaving
/// the machine as if rexenv's system setup never ran. Site files + databases under
/// app-data are NOT touched (the user can still delete the app + its support dir).
#[tauri::command]
pub async fn uninstall_system(
    state: State<'_, AppState>,
) -> Result<core::setup::TeardownReport> {
    {
        let mut mgr = state.services.lock().await;
        if mgr.is_running() {
            mgr.stop_all(state.platform.as_ref())?;
        }
    }
    let conn = state
        .db
        .lock()
        .map_err(|_| Error::Other("database lock poisoned".into()))?;
    core::setup::run_system_teardown(&conn, state.platform.as_ref())
}

#[cfg(test)]
mod tests {
    use super::summarize;

    /// Pins the fix: `global_status` running/total/summary come from RUNNING
    /// SERVICES, never site DB rows — so the footer (global_status) and the
    /// Services tab (services_status) can't drift apart.
    #[test]
    fn summarize_reflects_running_services_not_sites() {
        let req = |r: bool| (r, false); // required service
        // Nothing listed → stopped.
        assert_eq!(summarize(&[]), (0, 0, "stopped"));
        // Services present but none running → stopped. (Even if a site row were
        // marked Running elsewhere, summarize never sees sites — that's the point.)
        assert_eq!(summarize(&[req(false), req(false)]), (0, 2, "stopped"));
        // Some running → partial — the exact case the footer used to get wrong
        // (stack up, zero sites DB-marked Running ⇒ must still be "running").
        assert_eq!(summarize(&[req(true), req(false)]), (1, 2, "partial"));
        // All running → all.
        assert_eq!(summarize(&[req(true), req(true)]), (2, 2, "all"));
    }

    /// Pins the P1-4 fix: an optional engine (Postgres — Start-all never starts
    /// it) counts only while running, so it can't hold the footer at "Partial".
    #[test]
    fn summarize_counts_optional_services_only_while_running() {
        let opt = |r: bool| (r, true);
        let req = |r: bool| (r, false);
        // Full stack up, Postgres never started → ALL, not partial.
        assert_eq!(summarize(&[req(true), req(true), opt(false)]), (2, 2, "all"));
        // User started Postgres → it joins both counts.
        assert_eq!(summarize(&[req(true), req(true), opt(true)]), (3, 3, "all"));
        // Postgres running but a required service died → partial, as before.
        assert_eq!(summarize(&[req(false), req(true), opt(true)]), (2, 3, "partial"));
        // Only an idle optional engine listed → stopped (not divide-by-zero "all").
        assert_eq!(summarize(&[opt(false)]), (0, 0, "stopped"));
    }
}

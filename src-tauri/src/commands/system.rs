//! commands::system — app-level IPC. App info + the global resource/status block.

use crate::core;
use crate::error::{Error, Result};
use crate::state::app::AppState;
use serde::Serialize;
use tauri::State;

pub use crate::core::app_info::AppInfo;

/// Round-trip smoke test for the typed IPC bridge (Phase 1 task 0.5).
/// A foreign proxy already answering `:443`, for the ONBOARDING warning — or
/// `None`, which is the answer for almost everyone.
///
/// # Why this is not `edge_answers_as_ours`
///
/// Onboarding runs BEFORE any service starts, so "is OURS what answers :443"
/// is false for every user on a clean first run. Wiring the boolean in here —
/// which is what the open item asked for, and what looked like a fifth caller —
/// would have shown a foreign-proxy warning to everybody.
///
/// **`NoAnswer` is silence.** Nothing listening at onboarding is the ORDINARY
/// case: the stack is not running yet, and reporting it would be inventing a
/// problem out of the normal state — the same fault the import path shipped,
/// in a new place. Only `Foreign` is worth a word.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupEdgeConflict {
    /// The listener, attributed where possible — never a guess.
    pub holder: Option<String>,
    /// The app to quit, when identifiable: "quit Herd" beats "quit that app".
    pub app: Option<String>,
    /// A copy-paste line that frees the port.
    pub fix: Option<String>,
}

#[tauri::command]
pub async fn setup_edge_conflict(
    state: State<'_, AppState>,
) -> Result<Option<SetupEdgeConflict>> {
    let wire = core::proxy::edge_wire(
        core::adminer::ADMINER_HOST,
        core::proxy::DEFAULT_HTTPS_PORT,
    )
    .await;
    if wire != core::proxy::EdgeWire::Foreign {
        return Ok(None);
    }
    let help = state
        .platform
        .supervisor()
        .port_conflict_help(core::proxy::DEFAULT_HTTPS_PORT, false);
    Ok(Some(SetupEdgeConflict { holder: help.holder, app: help.app, fix: help.free_command }))
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    core::app_info::current()
}

/// The words the UI uses for this OS's own things — "Show in Finder" / "Show in Explorer", the login item's
/// description (W7 S7, ruling Q3, ledger #626). Static per build, so the frontend reads it once.
#[tauri::command]
pub fn platform_words() -> crate::platform::words::PlatformWords {
    crate::platform::words::current().clone()
}

/// The one sentence a macOS 13 / 14 host sees at onboarding — what this OS version gets and
/// what it does not (`docs/PLAN-macos-13-floor.md` §6.3). `None` on every standard host, which
/// renders nothing. Built in core from the tier's own refusals, so it cannot disagree with them.
#[tauri::command]
pub fn legacy_notice() -> Option<String> {
    core::binaries::legacy_notice()
}

/// TLDs a site ANSWERS on that this machine cannot resolve, each flagged with
/// WHY: `foreign` = another tool owns the resolver file, otherwise it is simply
/// missing.
///
/// The Settings screen's counterpart to the `rex doctor` line (#457). The DNS
/// card shows the DEFAULT TLD's health, which is the common case and says
/// nothing about a site — or an extra domain — on a second TLD whose resolver
/// went away. Empty is the ordinary answer and renders nothing.
#[tauri::command]
pub fn unresolvable_tlds(state: State<'_, AppState>) -> Result<Vec<UnresolvableTld>> {
    let conn = state
        .db
        .lock()
        .map_err(|_| Error::Other("database lock poisoned".into()))?;
    Ok(
        core::dns::unresolvable_tlds_in_use(&conn, state.platform.as_ref(), core::dns::DEFAULT_DNS_PORT)
            .into_iter()
            .map(|(tld, foreign)| UnresolvableTld { tld, foreign })
            .collect(),
    )
}

/// One TLD the machine cannot resolve, and why — the two causes need different
/// sentences and different fixes.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnresolvableTld {
    pub tld: String,
    /// Another tool owns its resolver file (take it back), rather than the file
    /// being absent (install it).
    pub foreign: bool,
}

/// Re-install the OS resolver file for a TLD one of this machine's sites
/// actually answers on — the fix `rex doctor` names when it finds one missing.
///
/// **Scoped to TLDs IN USE, and that is the security half.** `ensure_resolver`
/// writes a root-owned file under `/etc/resolver` behind a privileged prompt; a
/// command that installed one for any string a caller passed would be a way to
/// point arbitrary TLDs at this machine's loopback resolver — a bigger door
/// than "repair what my own sites need", and one an agent or a stray `invoke`
/// could walk through.
///
/// Lives in `commands/` rather than in the CLI's dispatch because every arm
/// must run the same code the UI would (#57): the Settings screen is the
/// obvious second caller, and a repair button there must not be a second
/// implementation of this rule.
#[tauri::command]
pub async fn repair_resolver(state: State<'_, AppState>, tld: String) -> Result<String> {
    let tld = tld.trim().trim_start_matches('.').to_ascii_lowercase();
    let in_use = {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        core::dns::tlds_in_use(&conn)
    };
    if !in_use.contains(&tld) {
        return Err(Error::Other(format!(
            "no site answers on .{tld} — rexenv only installs resolvers for TLDs its own sites \
             use. In use here: {}",
            in_use.iter().map(|t| format!(".{t}")).collect::<Vec<_>>().join(", ")
        )));
    }
    // `async` + `while_prompting`: as a sync command this ran on the MAIN thread,
    // and the whole window froze for as long as the admin prompt stayed open.
    core::prompt::while_prompting(|| {
        core::dns::ensure_resolver(
            state.platform.as_ref(),
            &tld,
            core::dns::DEFAULT_DNS_PORT,
            // The user asked for exactly this; prompting is the point.
            core::dns::ResolverPrompt::Allow,
        )
    })?;
    Ok(tld)
}

/// Remove the OS resolver file for a TLD NO site answers on any more — the
/// counterpart of `repair_resolver`, and just as scoped: a root-owned file under
/// `/etc/resolver` is removed only when it is ours (never Valet's or Herd's),
/// only when nothing here uses the TLD, and never for the backbone `.rex`.
/// Nothing removes one automatically — deleting a site or dropping an extra
/// domain must not raise a password prompt for housekeeping — so this is the
/// explicit verb (`rex tld --remove <tld>`).
#[tauri::command]
pub async fn remove_resolver(state: State<'_, AppState>, tld: String) -> Result<bool> {
    let tld = tld.trim().trim_start_matches('.').to_ascii_lowercase();
    if tld == core::tld::BACKBONE_TLD {
        return Err(Error::Other(format!(
            ".{tld} is rexenv's backbone TLD — its resolver is always installed"
        )));
    }
    let in_use = {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        core::dns::tlds_in_use(&conn)
    };
    if in_use.contains(&tld) {
        return Err(Error::Other(format!(
            "a site still answers on .{tld} — remove or rename that site's names first \
             (`rex site domains <site>` lists them)"
        )));
    }
    core::prompt::while_prompting(|| {
        core::dns::remove_resolver(state.platform.as_ref(), &tld, core::dns::DEFAULT_DNS_PORT)
    })
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

/// Notices raised during startup, held until a webview exists to show them.
///
/// **Why a queue and not an event** (30 Aug 2026): the launch sweeps run in
/// `setup()`, BEFORE the window mounts, so anything emitted there is emitted to
/// nobody — which is why "rexenv stopped a public share you didn't know about"
/// had never reached the user's screen, only `rexenv.log`. A share the app kills
/// on the user's behalf is exactly the fact they must not have to go looking
/// for: the link they gave someone stopped working, and nothing said so.
///
/// ALWAYS managed, like [`InitError`], so reading it can never panic.
#[derive(Default)]
pub struct StartupNotices(pub std::sync::Mutex<Vec<StartupNotice>>);

/// One queued startup notice. `level` maps to the toast kind the frontend uses.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupNotice {
    /// "info" | "warn" — warn is reserved for something the app DID on the
    /// user's behalf, never for something it merely noticed.
    pub level: &'static str,
    pub message: String,
}

impl StartupNotices {
    /// Queue a notice. Best-effort: a poisoned lock loses the notice rather
    /// than taking the app down over a toast, and the same fact is already in
    /// `rexenv.log` — the log is the record, this is the courtesy.
    pub fn push(&self, level: &'static str, message: String) {
        if let Ok(mut q) = self.0.lock() {
            q.push(StartupNotice { level, message });
        }
    }

    /// Take everything queued, leaving the queue empty. Separate from the
    /// command so the once-only rule is testable without a Tauri app.
    pub fn drain(&self) -> Vec<StartupNotice> {
        self.0.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default()
    }
}

/// Drain the startup notices. DRAINING is the point: the frontend calls this
/// once on mount, and a second caller (a reload, a second window) must not
/// re-toast what the user has already been told.
#[tauri::command]
pub fn startup_notices(state: State<'_, StartupNotices>) -> Vec<StartupNotice> {
    state.drain()
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
/// Also the TRAY's status line (`core::tray`), which is why this is
/// `pub(crate)` rather than private: two summaries of the same services, a
/// centimetre apart in the same app, would be free to disagree.
pub(crate) fn summarize(flags: &[(bool, bool)]) -> (u32, u32, &'static str) {
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

/// The setting holding the browser every `http(s)` open goes to. Empty/absent
/// = the OS default handler.
pub const PREFERRED_BROWSER_KEY: &str = "preferred_browser";

/// Open a path or URL — Finder for a docroot, the user's chosen browser for an
/// `http(s)` link (Phase 3 §1.3 Overview quick links).
///
/// The preference is applied HERE, not in the UI, and that is deliberate:
/// rexenv opens links from a dozen call sites (site header, quick tile, Sites
/// row, Tunnels, Mail, Adminer, magic login, WordPress plugin/theme rows…). If
/// each one had to remember to route through the preference, the next call site
/// someone adds would silently open in the system default — the same
/// "whole-surface claim that only checks one place" failure the ledger already
/// records twice. One choke point instead.
///
/// Installed-ness is re-checked on EVERY open (inside `open_in_browser`), never
/// once at save time: the user can drag a browser to the Trash any day. A
/// preference that no longer resolves logs and falls back to the OS handler —
/// the link still opens, and Settings shows the picker back on "System default".
#[tauri::command]
pub fn open_external(state: State<'_, AppState>, target: String) -> Result<()> {
    if target.starts_with("http://") || target.starts_with("https://") {
        if let Some(browser) = preferred_browser(&state) {
            match state.platform.shell().open_in_browser(&browser, &target, false) {
                Ok(()) => return Ok(()),
                Err(e) => log::warn!(
                    "preferred browser {browser:?} could not open {target}: {e} — \
                     falling back to the system default handler"
                ),
            }
        }
    }
    state.platform.shell().open(&target)
}

/// The stored `preferred_browser`, or `None` for "OS default". Never propagates
/// a DB error: a settings read that fails must not stop a link from opening.
/// The guard is dropped before the caller opens anything — no lock is held
/// across a process spawn.
fn preferred_browser(state: &State<'_, AppState>) -> Option<String> {
    let conn = state.db.lock().ok()?;
    let value = crate::state::store::get_setting(&conn, PREFERRED_BROWSER_KEY).ok()??;
    drop(conn);
    let value = value.trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// Web browsers installed on this machine, detection-ordered, one of them
/// flagged as the OS default for `https`. Empty = none found.
#[tauri::command]
pub fn list_browsers(state: State<'_, AppState>) -> Vec<crate::platform::traits::BrowserApp> {
    state.platform.shell().detect_browsers()
}

/// Open ONE url in a specific browser without touching the preference — the
/// chevron menu next to "Open in browser". Changing the default is Settings'
/// job; a menu that silently rewrote it would leave the user wondering why
/// everything opens somewhere new.
///
/// `private` opens a private/incognito window (the second target on each row of
/// that menu). It errors — never quietly opens a normal window — for a browser
/// whose `supports_private` is false; that is the same one-time detour, just in
/// a window the browser won't record.
#[tauri::command]
pub fn open_in_browser(
    state: State<'_, AppState>,
    browser_id: String,
    url: String,
    private: bool,
) -> Result<()> {
    state.platform.shell().open_in_browser(&browser_id, &url, private)
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
        crate::state::app::DnsMode::Removed => "removed",
    };
    // The Settings indicator reports the BACKBONE (.rex) resolver file — the
    // one system setup installs and that always stays active.
    let route = state.platform.dns();
    DnsStatus {
        running,
        mode,
        port,
        resolver_installed: route.route_owner(core::tld::BACKBONE_TLD, port) != core::dns::ResolverOwner::Absent,
        resolver_path: route.route_label(core::tld::BACKBONE_TLD),
        ca_trusted: state.platform.cert_trust().is_trusted(&state.ca.cert_path),
    }
}

/// `rex` CLI install status for the Settings card.
#[tauri::command]
pub fn cli_status(state: State<'_, AppState>) -> Result<core::cli::CliStatus> {
    core::cli::status(state.platform.as_ref())
}

/// Install/refresh `rex` on PATH: on macOS the symlink, which may show ONE admin
/// prompt (`/usr/local/bin` is root-owned on most machines); on Windows a copy on
/// the user's own `Path`, no prompt (#634) — `async` so a blocking prompt runs off
/// the UI thread, same handling as `system_setup`.
/// Returns the refreshed status so the card updates in one round-trip.
#[tauri::command]
pub async fn cli_install(state: State<'_, AppState>) -> Result<core::cli::CliStatus> {
    core::prompt::while_prompting(|| core::cli::install(state.platform.as_ref()))?;
    core::cli::status(state.platform.as_ref())
}

/// Run first-run system setup (§3.4): install the `.rex` backbone OS resolver (one admin
/// prompt) + trust the local CA (native keychain dialog). Idempotent — safe to
/// re-run. Backs the Onboarding "Set up domains & SSL" step.
///
/// The prompts wait for the user, and `run_system_setup` waits with them on a
/// plain thread. `async` alone kept that wait off the UI thread but ON a tokio
/// worker, for as long as the dialogs stayed open — and the first-run downloads
/// the Install step started run on those workers, so they could sit still until
/// the user answered (found 11 Sep 2026, diagnosing #566). Off the runtime now,
/// like `wp_blocking`.
#[tauri::command]
pub async fn system_setup(app: tauri::AppHandle) -> Result<()> {
    tauri::async_runtime::spawn_blocking(move || {
        // `try_state`, not `state`: AppState is absent after a failed init (#178),
        // and a panic on this thread would surface as a bare JoinError.
        let state = tauri::Manager::try_state::<AppState>(&app)
            .ok_or_else(|| Error::Other("rexenv did not finish starting — restart the app".into()))?;
        core::setup::run_system_setup(state.platform.as_ref()).map(drop)
    })
    .await
    .map_err(|e| Error::Other(format!("system setup task failed: {e}")))?
}

/// Re-trust the local CA in the user trust store (macOS login keychain — shows the
/// native auth dialog, no root). Idempotent: re-adding an already-trusted cert is fine.
/// `async` so the dialog's wait is not on the main thread (a sync command froze the
/// whole window until it was answered).
#[tauri::command]
pub async fn trust_local_ca(state: State<'_, AppState>) -> Result<()> {
    core::prompt::while_prompting(|| core::ssl::trust_ca(state.platform.as_ref(), &state.ca))
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
    /// The TLDs rexenv answers on — what a typed `name.<tld>` should open.
    pub tlds: Vec<String>,
    /// Profiles that open a typed `name.<tld>` for EVERY one of `tlds`
    /// instead of searching it (`core::firefox::allow_tlds_in_profiles`).
    pub typing: usize,
}

fn firefox_status_for(state: &State<'_, AppState>) -> FirefoxTrustStatus {
    let root = state.platform.cert_trust().firefox_profiles_root();
    let tlds = core::dns::answered_tlds(state.platform.as_ref());
    FirefoxTrustStatus {
        trust: core::firefox::status(root.as_deref()),
        ca_path: state.ca.cert_path.display().to_string(),
        typing: core::firefox::tlds_allowed_count(root.as_deref(), &tlds),
        tlds,
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

/// Make a typed `name.<tld>` open the site in Firefox instead of searching it,
/// for every TLD rexenv answers on (`browser.fixup.domainsuffixwhitelist.<tld>`
/// in each profile's `user.js`). Plain file writes, no prompt; takes effect when
/// Firefox restarts. Returns the refreshed status.
#[tauri::command]
pub fn allow_tlds_in_firefox(state: State<'_, AppState>) -> Result<FirefoxTrustStatus> {
    let Some(root) = state.platform.cert_trust().firefox_profiles_root() else {
        return Err(crate::error::Error::Other(
            "Firefox was not found for this user (no profiles.ini).".into(),
        ));
    };
    let tlds = core::dns::answered_tlds(state.platform.as_ref());
    let written = core::firefox::allow_tlds_in_profiles(&root, &tlds)?;
    log::info!("firefox: typed-address pref added in {written} profile(s)");
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
    let (sites, aliases) = {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        (core::sites::list(&conn)?, crate::state::store::all_site_aliases(&conn)?)
    };
    let paths = state.platform.paths();
    let perms = state.platform.permissions();
    let mut count = 0u32;
    for s in &sites {
        // Each site's cert is reissued for EVERY name it answers on — the
        // sidecar beside it is rewritten from the same list, so the next
        // rebuild cannot mistake a primary-only cert for a covering one.
        let extra = aliases.get(&s.id).cloned().unwrap_or_default();
        core::ssl::reissue_site_cert(paths, perms, &state.ca, &s.domain, &extra)?;
        count += 1;
    }
    core::ssl::reissue_site_cert(paths, perms, &state.ca, core::adminer::ADMINER_HOST, &[])?;

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
    dns: State<'_, crate::state::app::DnsState>,
) -> Result<core::setup::TeardownReport> {
    {
        let mut mgr = state.services.lock().await;
        if mgr.is_running() {
            mgr.stop_all(state.platform.as_ref())?;
        }
    }
    // The database, not a guard on it: teardown locks it per step and never across
    // its prompts (#569).
    let report =
        core::prompt::while_prompting(|| core::setup::run_system_teardown(&state.db, state.platform.as_ref()))?;
    // The teardown uninstalled the agent; tell the watchdog it was asked to,
    // or it reads the silence as a crash and brings DNS back (#715). Any
    // in-process resolver goes with it — dropping the service closes the socket.
    dns.set(None, crate::state::app::DnsMode::Removed);
    Ok(report)
}

#[cfg(test)]
mod tests {

    /// #567 — **first-run system setup waits on its prompts OFF the async
    /// runtime.** The admin prompt and the keychain dialog stay open as long as
    /// the user takes; called straight from the `async fn`, that wait held a tokio
    /// worker the Install step's downloads share. No test can answer a real
    /// prompt, so the shape is asserted: the call sits inside `spawn_blocking`.
    #[test]
    fn system_setup_waits_on_its_prompts_off_the_async_runtime() {
        let src = crate::core::copy_scan::production_source(include_str!("system.rs"));
        let body = src
            .split("pub async fn system_setup(")
            .nth(1)
            .and_then(|b| b.split("\n#[tauri::command]").next())
            .expect("system_setup");
        let blocking = body.find("spawn_blocking(").expect(
            "system_setup no longer uses spawn_blocking — its prompts would hold a runtime worker \
             (and the first-run downloads on it) until the user answers",
        );
        let call = body.find("run_system_setup(").expect("system_setup no longer calls run_system_setup");
        assert!(
            call > blocking,
            "run_system_setup is called before/outside spawn_blocking, i.e. on the runtime worker"
        );
    }

    /// #178 — **the state a failed startup still has to answer from is ALWAYS
    /// managed, so reading it can never panic.**
    ///
    /// Tauri's `State<'_, T>` is a runtime lookup: a command taking one whose
    /// type was never `manage`d aborts the process with "state not managed".
    /// `AppState` is deliberately absent when init fails — that is the whole
    /// design of the error screen — so the two things the frontend calls on that
    /// path (`init_error`, `startup_notices`) must be managed on EVERY path,
    /// including the one where the database or the CA could not be opened. If
    /// either slipped inside the success branch, the error screen would kill the
    /// app while trying to explain why the app cannot start: the worst possible
    /// place for this bug, and invisible until a real init failure.
    ///
    /// Derived rather than spot-checked: every `State<'_, T>` type the command
    /// layer takes must be managed somewhere, and the two always-managed ones
    /// must be managed UNCONDITIONALLY — asserted as "at the shallowest
    /// indentation any `app.manage(` sits at", which is setup's own statement
    /// level, so nesting one inside an `if` or a `match` arm fails here.
    #[test]
    fn the_state_a_failed_startup_answers_from_is_always_managed() {
        let lib = include_str!("../lib.rs");
        let manages: Vec<(usize, &str)> = lib
            .lines()
            // `starts_with` on the TRIMMED line, not `contains`: a comment
            // mentioning `app.manage(notices)` is not a manage call, and a real
            // one written `app.manage::<T>(…)` is — matching on the paren form
            // alone silently missed the only turbofish call in the file.
            .filter(|l| l.trim_start().starts_with("app.manage"))
            .map(|l| (l.len() - l.trim_start().len(), l.trim()))
            .collect();
        assert!(
            manages.len() > 5,
            "only {} `app.manage(` lines found in lib.rs — the scan is broken, and a guard \
             that reads nothing passes for the wrong reason",
            manages.len()
        );
        let top = manages.iter().map(|(indent, _)| *indent).min().expect("a manage call");

        // Every state type a command takes must be managed at all — a missing
        // one is an abort the first time that command is invoked.
        let mut types: Vec<String> = Vec::new();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/commands");
        for entry in std::fs::read_dir(&dir).expect("commands/").flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let Ok(raw) = std::fs::read_to_string(&path) else { continue };
            // PRODUCTION lines only: this guard's own source says `State<'_, `
            // inside a string literal, and scanning itself made it report a
            // fragment of its own message as a missing state type.
            let text = crate::core::copy_scan::production_source(&raw);
            for (_, after) in text.match_indices("State<'_, ").map(|(i, m)| (i, &text[i + m.len()..]))
            {
                let Some(ty) = after.split('>').next() else { continue };
                let short = ty.rsplit("::").next().unwrap_or(ty).trim().to_string();
                if short.is_empty() || short.starts_with('…') || types.contains(&short) {
                    continue;
                }
                types.push(short);
            }
        }
        assert!(types.len() > 5, "only {types:?} parsed — the state scan is broken");
        for ty in &types {
            if ty == "AppState" {
                continue; // deliberately absent when init fails — the error screen's premise
            }
            assert!(
                manages.iter().any(|(_, line)| line.contains(ty.as_str())),
                "`{ty}` is taken as `State` by a command but nothing in lib.rs manages it — the \
                 first invocation aborts the process with \"state not managed\""
            );
        }

        // …and the two the ERROR SCREEN itself calls must be unconditional.
        for always in ["InitError", "StartupNotices"] {
            let line = manages
                .iter()
                .find(|(_, l)| l.contains(always))
                .unwrap_or_else(|| panic!("`{always}` is not managed in lib.rs at all"));
            assert_eq!(
                line.0, top,
                "`{always}` is managed at indentation {} while setup's unconditional statements \
                 sit at {top} — it has moved inside a branch. When that branch is the \
                 init-SUCCESS one, the error screen aborts the app while trying to explain why \
                 the app could not start",
                line.0
            );
        }
    }
    use super::{summarize, StartupNotices};

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
    /// A share rexenv stopped on the user's behalf must be TOLD ONCE — not
    /// zero times (an event emitted before any window exists) and not on every
    /// reload (a toast that keeps reappearing reads as a fault that keeps
    /// happening).
    #[test]
    fn startup_notices_survive_until_read_and_are_read_once() {
        let q = StartupNotices::default();
        q.push("warn", "Stopped 1 public share(s) left running by a crashed session".into());
        q.push("info", "swept something dull".into());

        let first = q.drain();
        assert_eq!(first.len(), 2, "a queued notice was lost before anything could show it");
        assert_eq!(first[0].level, "warn");
        assert!(
            first[0].message.contains("public share"),
            "the notice no longer says WHAT was stopped: {}",
            first[0].message
        );
        assert!(
            q.drain().is_empty(),
            "a second read re-delivered notices the user has already been shown — a reload \
             would re-toast a share that was stopped once"
        );
    }
}

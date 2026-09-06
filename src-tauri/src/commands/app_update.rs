//! commands::app_update — IPC for the app's own update check. Thin: every rule
//! lives in `core::app_update`, and this file only translates calls.
//!
//! `app_update_apply` is GUI-only, and stays that way: self-update replaces the
//! process that enforces the agent dial and `settings_access`, and the relaunch
//! kills the caller's socket mid-call, so an agent could never observe the result
//! of the thing it asked for. A source scan holds that (ledger #530).

use crate::core;
use crate::error::{Error, Result};
use crate::state::app::AppState;
use serde::Serialize;
use tauri::State;

fn lock<'a>(
    state: &'a State<'a, AppState>,
) -> Result<std::sync::MutexGuard<'a, rusqlite::Connection>> {
    state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))
}

/// What the About card renders. Pure reads — no network, so opening Settings
/// never waits on GitHub.
#[tauri::command]
pub fn app_update_state(state: State<'_, AppState>) -> Result<core::app_update::AppUpdateState> {
    let conn = lock(&state)?;
    Ok(core::app_update::state(&conn))
}

/// Check now: fetch the signed descriptor, accept it under one brief lock, and
/// answer with the state that follows.
///
/// **Fetch UNLOCKED, persist under the lock.** `core::app_update::fetch` takes no
/// `Connection` precisely so this cannot be written the other way round — the
/// house rule about never holding the database lock across a wait, made
/// structural.
///
/// The check timestamp is written ONLY on the success path, so a failed check
/// can never age into "checked just now" over yesterday's answer.
#[tauri::command]
pub async fn app_update_check(
    state: State<'_, AppState>,
) -> Result<core::app_update::AppUpdateState> {
    let (doc, sig) = core::app_update::fetch().await?;
    let conn = lock(&state)?;
    // A serial we already have is not an error — it is the ordinary answer on
    // every check after the first, and `accept` says so by writing nothing.
    core::app_update::accept(&conn, &doc, &sig)?;
    let st = core::app_update::state(&conn);
    let check = core::app_update::store_check(&conn, st.offered.clone())?;
    Ok(core::app_update::AppUpdateState { checked_at: Some(check.checked_at), ..st })
}

/// What an apply actually did — measured, never assumed.
///
/// `swapped` is the fact that matters: the bundle on disk is the new one. The
/// relaunch is a separate claim because it happens after this returns, through
/// the quit gate, and the gate is allowed to say no.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyOutcome {
    pub swapped: bool,
    pub version: String,
}

/// Install the offered release and quit, so the relauncher can reopen the new
/// build.
///
/// # The order, and what each step costs if it fails
///
/// 1. **Claim.** One apply at a time, on the same keyed set the PHP and Adminer
///    applies use.
/// 2. **Pre-flight** (inside `install_downloaded`, and again here through the
///    offer's size) — every refusal costs zero bytes.
/// 3. **Download**, verified against the digest the SIGNED descriptor names, into
///    app data. Reported into the ONE download hub, so the footer and the panel
///    show it.
/// 4. **Stage, verify, swap.** Atomic; every failure before it leaves the
///    installed bundle untouched.
/// 5. **Record** what happened, for the next process to report.
/// 6. **Quit** — `app.exit(0)`, which raises `ExitRequested` and passes through
///    the ONE quit gate exactly like every other quit. `AppHandle::restart` is
///    never called: it would skip that gate entirely (ledger #529).
///
/// # Why this does not refuse a busy app
///
/// The plan first said an apply should refuse while jobs are running. It does
/// not, and the reason is that **an update IS a quit**: the app quits and
/// reopens, and this project already decided what a quit is allowed to
/// interrupt. Refusing here would be stricter than Cmd+Q for the identical
/// consequence, which is a rule a user cannot predict. Live public shares still
/// stop it — through the same confirm the quit gate raises, not through a second
/// check here — and the consent sentence says plainly that terminals and running
/// jobs close with it.
#[tauri::command]
pub async fn app_update_apply(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<ApplyOutcome> {
    let _claim = crate::commands::php::InFlight::claim("app")?;
    let platform = crate::platform::current();

    let offer = {
        let conn = lock(&state)?;
        core::app_update::state(&conn).offered.ok_or_else(|| {
            Error::Other(
                "there is nothing to install — check for updates first, or the release \
                 that was offered no longer applies to this build"
                    .into(),
            )
        })?
    };

    // Download UNLOCKED. `download_artifact` takes no `Connection`, so this
    // cannot be written the other way round.
    let archive = core::app_update::download_artifact(&*platform, &offer).await?;
    let receipt = core::app_update::install_downloaded(&*platform, &offer, &archive)?;
    log::info!(
        "app update: swapped to {} ({:?}); previous bundle kept at {}",
        offer.version,
        receipt.method,
        receipt.previous.display()
    );

    {
        let conn = lock(&state)?;
        if let Err(e) = core::app_update::store_notice(&conn, &offer.version) {
            // Not fatal: the swap already happened, and the next launch reads
            // its own version anyway. It only costs the "updated to X" notice.
            log::warn!("app update: could not record the update: {e}");
        }
    }

    // Tell the exit hook there is a relaunch owed. Set BEFORE the exit and read
    // in `RunEvent::Exit`, which is the point after the quit gate has already
    // agreed — a cancelled quit therefore leaves no helper waiting.
    crate::relaunch_after_exit(receipt.installed.clone());
    let version = offer.version.clone();
    app.exit(0);
    Ok(ApplyOutcome { swapped: true, version })
}

/// The sentence shown above the Update button, and the facts the card needs to
/// decide whether the button may appear at all.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyReadiness {
    /// `None` when the app may install the offer.
    pub refusal: Option<String>,
    pub consent: String,
    pub homebrew: bool,
}

/// Can this installation take the offered update, and what does pressing the
/// button do?
///
/// Both answers come from Rust so the card renders ONE source: a consent
/// sentence copied into the TSX is a copy that drifts from the rule it describes.
#[tauri::command]
pub fn app_update_readiness(state: State<'_, AppState>) -> Result<Option<ApplyReadiness>> {
    let offer = {
        let conn = lock(&state)?;
        core::app_update::state(&conn).offered
    };
    let Some(offer) = offer else { return Ok(None) };
    let platform = crate::platform::current();
    let exe = std::env::current_exe()?;
    let facts = platform.app_bundle().facts(&exe)?;
    Ok(Some(ApplyReadiness {
        refusal: core::app_update::preflight(&facts, offer.size_bytes)
            .err()
            .map(|r| r.message()),
        consent: core::app_update::consent_sentence(&offer, facts.homebrew),
        homebrew: facts.homebrew,
    }))
}

/// Skip exactly this version, or clear the skip.
///
/// The skip is stored as a VERSION and compared live against whatever is
/// offered, so skipping 0.6.0 cannot hide 0.6.1 — a boolean would answer one
/// question forever, which is the shape `core::app_update` refuses.
#[tauri::command]
pub fn app_update_skip(
    state: State<'_, AppState>,
    version: Option<String>,
) -> Result<core::app_update::AppUpdateState> {
    let conn = lock(&state)?;
    core::app_update::set_skipped(&conn, version.as_deref())?;
    Ok(core::app_update::state(&conn))
}

/// Turn automatic checking on or off, and answer with the state that follows.
///
/// The setting is honoured in `lib.rs` BEFORE any network call — both at launch
/// and in the poller — so turning this off stops the requests themselves, not
/// merely the card. Checking by hand keeps working, because a person asking is
/// not the thing this switch is about.
#[tauri::command]
pub fn app_update_set_auto_check(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<core::app_update::AppUpdateState> {
    let conn = lock(&state)?;
    core::app_update::set_auto_check(&conn, enabled)?;
    Ok(core::app_update::state(&conn))
}

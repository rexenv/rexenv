//! commands::update_hub — IPC for the ONE update inventory (`core::update_hub`,
//! `docs/PLAN-unified-updates.md` T1). Thin: this file gathers the facts the four
//! families already compute for their own screens and hands them to the hub; the
//! judging, the order and every sentence live in `core`.
//!
//! Nothing here installs. Each row still installs through its family's own command.

use crate::core;
use crate::error::{Error, Result};
use crate::state::app::AppState;
use tauri::State;

fn lock<'a>(
    state: &'a State<'a, AppState>,
) -> Result<std::sync::MutexGuard<'a, rusqlite::Connection>> {
    state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))
}

fn document() -> String {
    core::app_update::this_document(crate::platform::current().app_bundle().descriptor_variant().as_deref())
}

/// Everything newer than what runs here, in one list (the app last).
#[tauri::command]
pub fn updates_inventory(state: State<'_, AppState>) -> Result<core::update_hub::UpdateInventory> {
    // The running pools are the services lock's to say; read them BEFORE the db lock, the
    // way `list_php_versions` does, and never block on either.
    let running = state
        .services
        .try_lock()
        .ok()
        .and_then(|mgr| mgr.running_php_patches(state.platform.as_ref()))
        .unwrap_or_default();
    let platform = state.platform.as_ref();
    // The Homebrew fact decides the app's consent sentence; a bundle whose facts cannot be
    // read is treated as a plain install, which is the sentence that asks for nothing extra.
    let homebrew = std::env::current_exe()
        .ok()
        .and_then(|exe| platform.app_bundle().facts(&exe).ok())
        .map(|f| f.homebrew)
        .unwrap_or(false);
    let conn = lock(&state)?;
    let inv = core::update_hub::gather(&conn, platform, &running, homebrew, &document())?;
    // The tray reads a snapshot, never the database: publish what was just computed.
    core::update_hub::publish(&inv);
    Ok(inv)
}

/// "Check now": BOTH signed documents on the interactive deadline, then the inventory.
///
/// Fetched UNLOCKED, verified + persisted under one brief lock (the rule `php_update_check`
/// and `app_update_check` each follow). A document that cannot be fetched or is refused does
/// not fail the call — the inventory answers with `check_problem` saying which, because
/// "couldn't reach" and "refused what it published" are different facts and the old About
/// card folded them into one line for a week (ledger, `check_refusal`).
#[tauri::command]
pub async fn updates_check_now(state: State<'_, AppState>) -> Result<core::update_hub::UpdateInventory> {
    let variant = crate::platform::current().app_bundle().descriptor_variant();
    let mut problems: Vec<String> = Vec::new();
    // 1. The PHP / Adminer / phpMyAdmin manifest.
    match core::updates::fetch_interactive().await {
        Ok((doc, sig)) => {
            let conn = lock(&state)?;
            match core::updates::accept(&conn, &doc, sig.trim()) {
                Ok(cat) => {
                    core::binaries::install_catalog(cat);
                    core::update_hub::store_checked(&conn)?;
                }
                Err(e) => problems.push(core::update_hub::refusal_sentence("runtime manifest", &e.to_string())),
            }
        }
        Err(e) => problems.push(core::update_hub::unreachable_sentence(&e.to_string())),
    }
    // 2. The app's own descriptor — unless this build reads none (no key; a dev build).
    if let Some(why) = core::app_update::no_descriptor_reason(variant.as_deref()) {
        problems.push(why);
    } else {
        match core::app_update::fetch(variant.as_deref(), core::updates::INTERACTIVE_DEADLINE).await {
            Ok((doc, sig)) => {
                let conn = lock(&state)?;
                let document = document();
                match core::app_update::accept(&conn, &document, &doc, &sig) {
                    Ok(_) => {
                        let st = core::app_update::state(&conn, &document);
                        core::app_update::store_check(&conn, st.offered)?;
                        core::update_hub::store_checked(&conn)?;
                    }
                    Err(e) => problems.push(core::app_update::refusal_sentence(&e.to_string())),
                }
            }
            Err(e) => {
                // One unreachable sentence is enough when neither answered.
                let s = core::update_hub::unreachable_sentence(&e.to_string());
                if !problems.iter().any(|p| p.starts_with("Couldn't reach")) {
                    problems.push(s);
                }
            }
        }
    }
    let mut inv = updates_inventory(state)?;
    inv.check_problem = (!problems.is_empty()).then(|| problems.join(" "));
    Ok(inv)
}

/// Set one kind's offered version aside (`version: Some`) or take the skip back (`None`),
/// and answer with the inventory that follows.
#[tauri::command]
pub fn updates_skip(
    state: State<'_, AppState>,
    kind: core::update_hub::UpdateKind,
    version: Option<String>,
) -> Result<core::update_hub::UpdateInventory> {
    {
        let conn = lock(&state)?;
        core::update_hub::set_skipped(&conn, &kind, version.as_deref())?;
    }
    updates_inventory(state)
}

// ── Update all (T5) ───────────────────────────────────────────────────────────────────

/// The one Update-all run (at most one at a time), kept for the page to poll.
#[derive(Default)]
pub struct UpdateAllJobs(std::sync::Mutex<Option<std::sync::Arc<std::sync::Mutex<core::update_hub::RunState>>>>);

impl UpdateAllJobs {
    fn current(&self) -> Option<std::sync::Arc<std::sync::Mutex<core::update_hub::RunState>>> {
        self.0.lock().ok().and_then(|g| g.clone())
    }
}

/// Start "Update all": every pending offer, in inventory order (the app LAST), one after
/// another, through each family's own apply command; the first failure stops the chain.
/// Returns the run's first snapshot; `updates_apply_all_state` is polled for the rest.
#[tauri::command]
pub fn updates_apply_all(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    jobs: State<'_, UpdateAllJobs>,
) -> Result<core::update_hub::RunState> {
    if let Some(cur) = jobs.current() {
        if cur.lock().map(|r| r.status == "running").unwrap_or(false) {
            return Err(Error::Other("an Update all is already running — wait for it to settle".into()));
        }
    }
    let inv = updates_inventory(state)?;
    let plan = core::update_hub::plan_all(&inv);
    if plan.is_empty() {
        return Err(Error::Other("nothing is pending — every offer is installed or set aside".into()));
    }
    let run = std::sync::Arc::new(std::sync::Mutex::new(core::update_hub::RunState::new(
        uuid::Uuid::new_v4().to_string(),
        &plan,
    )));
    let snapshot = run.lock().map(|r| r.clone()).map_err(|_| Error::Other("update-all lock poisoned".into()))?;
    if let Ok(mut g) = jobs.0.lock() {
        *g = Some(run.clone());
    }
    tauri::async_runtime::spawn(run_all(app, plan, run));
    Ok(snapshot)
}

/// The current (or last) Update-all run, if any.
#[tauri::command]
pub fn updates_apply_all_state(jobs: State<'_, UpdateAllJobs>) -> Option<core::update_hub::RunState> {
    jobs.current().and_then(|r| r.lock().ok().map(|r| r.clone()))
}

async fn run_all(
    app: tauri::AppHandle,
    plan: Vec<core::update_hub::UpdateOffer>,
    run: std::sync::Arc<std::sync::Mutex<core::update_hub::RunState>>,
) {
    use tauri::{Emitter, Manager};
    loop {
        let next = run.lock().ok().and_then(|mut r| r.start_next());
        let Some(i) = next else { break };
        let outcome = apply_one(&app, &plan[i]).await;
        let Ok(mut r) = run.lock() else { break };
        match outcome {
            Ok((effect, swapped)) => {
                if swapped.is_some() {
                    r.app_swapped = swapped;
                }
                r.settle(i, Ok(effect));
            }
            Err(e) => r.settle(i, Err(e.to_string())),
        }
    }
    // The facts moved: republish the tray's snapshot and tell every surface.
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(conn) = state.db.lock() {
            if let Ok(inv) = core::update_hub::gather(&conn, state.platform.as_ref(), &[], false, &document()) {
                core::update_hub::publish(&inv);
            }
        }
    }
    if let Err(e) = app.emit(crate::UPDATES_EVENT, ()) {
        log::warn!("update all: could not tell the window: {e}");
    }
    crate::refresh_tray(&app);
}

/// One row, through its family's own command — the same code the row's own Install runs —
/// answering the effect the family MEASURED, plus (the app only) the swap the page must
/// acknowledge before anything restarts. The run never restarts anything itself.
async fn apply_one(
    app: &tauri::AppHandle,
    o: &core::update_hub::UpdateOffer,
) -> Result<(String, Option<(String, String)>)> {
    use tauri::Manager;
    let state = app.state::<AppState>();
    match &o.kind {
        core::update_hub::UpdateKind::Php { minor } => {
            let out = crate::commands::php::php_update_apply(state, minor.clone(), o.offered.clone()).await?;
            Ok((
                if out.restarted {
                    format!("PHP {minor} is now on {}", out.patch)
                } else {
                    format!("PHP {minor} will use {} — nothing was running to restart", out.patch)
                },
                None,
            ))
        }
        core::update_hub::UpdateKind::Adminer => {
            let st = crate::commands::database::adminer_update_apply(state, o.offered.clone()).await?;
            Ok((format!("Adminer is now on {}", st.staged.unwrap_or(st.effective)), None))
        }
        core::update_hub::UpdateKind::PhpMyAdmin => {
            let st = crate::commands::database::phpmyadmin_update_apply(state, o.offered.clone()).await?;
            Ok((
                format!("phpMyAdmin is now on {}", st.phpmyadmin_staged.unwrap_or(st.phpmyadmin_effective)),
                None,
            ))
        }
        core::update_hub::UpdateKind::App => {
            let out = crate::commands::app_update::app_update_apply(state).await?;
            Ok((
                format!("rexenv {} is installed — it takes effect when rexenv next opens", out.version),
                Some((out.version, out.notice)),
            ))
        }
    }
}

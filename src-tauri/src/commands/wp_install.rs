//! Streamed wp.org plugin/theme installs — the "show what's happening" card.
//! Reuses the repo streaming PRIMITIVES (`run_step_streamed`, `CancelToken`,
//! the append-log+emit sink pattern, the cached login-shell env) behind a
//! thin registry of its own — the repo `JobEntry`/events are git-shaped.
//!
//! Honesty contract (B25: wp-cli is opaque mid-download — no byte signal):
//! - the phase label is the last output line VERBATIM (WP-core-owned wording
//!   is never shown paraphrased); cursor + summary come from wp-cli phar
//!   LITERALS (per-item header, Success:/Error: terminal), and the bar's
//!   `pct` is PHASE-based observed progress ([`wordpress::InstallProgress`])
//!   — discrete ticks from lines wp-cli actually printed, never a byte
//!   estimate; loose milestone matching only ever costs granularity
//!   (forward implication), it cannot lie forward,
//! - NO idle watchdog — a 300s idle guard would tie-race wp-cli's own 300s
//!   `download_url` bound. The B25 outer wall-clock (120s + 900s·N) survives
//!   as a timer that records "timed_out" then cancels; Cancel is the escape,
//! - exit 0 can still mean installed-but-NOT-activated (chained `--activate`
//!   failures don't touch the exit code) — status never claims activation;
//!   the plugin/theme list refresh is the source of active-state truth,
//! - per-JOB log files (`wp-install-<domain>-<id8>.log`, last 5 kept per
//!   domain) — a failed install's log survives the next attempt.

use crate::commands::repo::{shell_env, RepoJobs};
use crate::commands::wordpress::wp_tools;
use crate::core::{repo, sites, wordpress};
use crate::error::{Error, Result};
use crate::state::app::AppState;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

pub fn state_event(id: &str) -> String {
    format!("wp-install://state/{id}")
}
pub fn output_event(id: &str) -> String {
    format!("wp-install://output/{id}")
}

#[derive(Default)]
pub struct WpInstallJobs {
    jobs: Mutex<HashMap<String, Arc<InstallEntry>>>,
    /// Creation order — a HashMap has none, and "the site's most recent
    /// install" must mean MOST RECENT (the RepoJobs next_seq pattern).
    next_seq: std::sync::atomic::AtomicU64,
}

struct InstallEntry {
    id: String,
    seq: u64,
    site_id: String,
    log_path: PathBuf,
    cancel: repo::CancelToken,
    running: AtomicBool,
    /// Set by the outer-cap timer BEFORE it cancels — the token can't tell
    /// user-cancel from timeout apart, so the reason is pre-recorded here.
    timed_out: AtomicBool,
    state: Mutex<WpInstallState>,
}

/// The card's whole truth. `item_cursor` is an ATTEMPT cursor ("installing
/// item k of N"), never a completion count; `summary` is the verbatim
/// terminal Success:/Error: line.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpInstallState {
    pub id: String,
    pub site_id: String,
    /// "plugin" | "theme".
    pub kind: String,
    pub slugs: Vec<String>,
    pub items_total: usize,
    pub item_cursor: usize,
    /// Phase-based determinate progress (0–100) — OBSERVED discrete progress
    /// (every tick = a line wp-cli actually printed), NOT the byte-estimate
    /// the B25 rule bans. Monotonic; 99-capped until the terminal summary;
    /// frozen in place on failure/cancel/timeout. See
    /// [`wordpress::InstallProgress`] for the full contract.
    pub pct: u8,
    /// "running" | "ok" | "partial" | "failed" | "cancelled" | "timed_out".
    pub status: String,
    pub summary: Option<String>,
    pub error: Option<String>,
    pub log_key: String,
}

fn snapshot(entry: &InstallEntry) -> WpInstallState {
    entry.state.lock().expect("install state lock").clone()
}

fn emit_state<R: tauri::Runtime>(app: &AppHandle<R>, entry: &InstallEntry) {
    let _ = app.emit(&state_event(&entry.id), snapshot(entry));
}

/// Keep only the newest `keep` install logs for a domain (bounded growth;
/// per-job files so a failed log survives later attempts).
fn prune_install_logs(log_dir: &std::path::Path, domain: &str, keep: usize) {
    let prefix = format!("wp-install-{domain}-");
    let Ok(entries) = std::fs::read_dir(log_dir) else { return };
    let mut logs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".log"))
        })
        .collect();
    logs.sort(); // id suffix is time-ordered enough only by mtime — sort by mtime
    logs.sort_by_key(|p| p.metadata().and_then(|m| m.modified()).ok());
    while logs.len() > keep {
        let _ = std::fs::remove_file(logs.remove(0));
    }
}

/// Start a streamed plugin/theme install job. Returns the initial snapshot;
/// progress arrives via `wp-install://state|output/<id>` events. One install
/// job per site at a time (refused, not queued).
#[tauri::command]
pub async fn wp_install_job<R: tauri::Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    repo_jobs: State<'_, RepoJobs>,
    jobs: State<'_, WpInstallJobs>,
    site_id: String,
    kind: String,
    slugs: Vec<String>,
    activate: bool,
) -> Result<WpInstallState> {
    if !matches!(kind.as_str(), "plugin" | "theme") {
        return Err(Error::Other(format!("unknown install kind \"{kind}\"")));
    }
    if slugs.is_empty() {
        return Err(Error::Other("nothing to install".into()));
    }
    wordpress::ensure_slugs(&kind, &slugs)?;
    let site = {
        let conn = state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))?;
        sites::get(&conn, &site_id)?.ok_or_else(|| Error::Other(format!("no site {site_id}")))?
    };
    let docroot = PathBuf::from(&site.path);
    let (php_bin, wp_phar) = wp_tools(&state, &site.php_version).await?;
    let _ = &repo_jobs; // env resolved in the blocking worker (shell spawn)

    let id = uuid::Uuid::new_v4().to_string();
    let log_dir = state.platform.paths().log_dir()?;
    prune_install_logs(&log_dir, &site.domain, 4);
    let log_key = format!("wp-install-{}-{}.log", site.domain, &id[..8]);
    let log_path = log_dir.join(&log_key);

    let entry = Arc::new(InstallEntry {
        id: id.clone(),
        seq: jobs.next_seq.fetch_add(1, Ordering::SeqCst),
        site_id: site_id.clone(),
        log_path,
        cancel: repo::CancelToken::new(),
        running: AtomicBool::new(true),
        timed_out: AtomicBool::new(false),
        state: Mutex::new(WpInstallState {
            id: id.clone(),
            site_id,
            kind: kind.clone(),
            slugs: slugs.clone(),
            items_total: slugs.len(),
            item_cursor: 0,
            pct: 0,
            status: "running".into(),
            summary: None,
            error: None,
            log_key,
        }),
    });
    {
        let mut map = jobs.jobs.lock().expect("install jobs lock");
        let busy = map
            .values()
            .any(|e| e.site_id == entry.site_id && e.running.load(Ordering::SeqCst));
        if busy {
            return Err(Error::Other(
                "an install is already running for this site — wait for it (or cancel it) first."
                    .into(),
            ));
        }
        map.insert(id.clone(), entry.clone());
    }
    let _ = std::fs::write(&entry.log_path, "");

    // The B25 outer wall-clock, preserved across the captured→streamed
    // switch: record the reason FIRST, then cancel (kills the pgid). wp-cli's
    // inner 300s/download bound still fires first on a dead link.
    let timer_entry = entry.clone();
    let timer_app = app.clone();
    let n = slugs.len();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(wordpress::download_timeout(n)).await;
        if timer_entry.running.load(Ordering::SeqCst) {
            timer_entry.timed_out.store(true, Ordering::SeqCst);
            let state = timer_app.state::<AppState>();
            timer_entry.cancel.cancel(state.platform.supervisor());
        }
    });

    let worker = entry.clone();
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        run_install_job(&worker_app, &worker, &php_bin, &wp_phar, &docroot, &kind, activate);
        worker.running.store(false, Ordering::SeqCst);
    });
    Ok(snapshot(&entry))
}

#[allow(clippy::too_many_arguments)] // flat mirror of the job's inputs
fn run_install_job<R: tauri::Runtime>(
    app: &AppHandle<R>,
    entry: &Arc<InstallEntry>,
    php_bin: &std::path::Path,
    wp_phar: &std::path::Path,
    docroot: &std::path::Path,
    kind: &str,
    activate: bool,
) {
    let state = app.state::<AppState>();
    let sup = state.platform.supervisor();
    // Login-shell env snapshot (blocking on first call — we're in a blocking
    // worker) shared with the repo jobs' cache: streamed spawn env_clears, so
    // the child needs the full snapshot (HOME included → wp-cli cache works).
    let env = match shell_env(&state, &app.state::<RepoJobs>(), false) {
        Ok(e) => e,
        Err(e) => {
            {
                let mut st = entry.state.lock().expect("install state lock");
                st.status = "failed".into();
                st.error = Some(e.to_string());
            }
            emit_state(app, entry);
            return;
        }
    };

    let mut args: Vec<String> =
        vec!["-d".into(), "memory_limit=512M".into(), wp_phar.display().to_string()];
    args.push(kind.to_string());
    args.push("install".into());
    args.extend(snapshot(entry).slugs.iter().cloned());
    if activate {
        args.push("--activate".into());
    }
    // Single-token form REQUIRED: a bare `--path <dir>` parses as a boolean
    // flag + a positional "slug" named after the docroot.
    args.push(format!("--path={}", docroot.display()));

    let mut file =
        std::fs::OpenOptions::new().create(true).append(true).open(&entry.log_path).ok();
    let sink_app = app.clone();
    let sink_entry = entry.clone();
    // Phase-based bar: cursor and pct read the SAME lines (headers), so the
    // "installing item k of N" text and the bar always tell one story.
    let mut progress = wordpress::InstallProgress::new(snapshot(entry).items_total, activate);
    let mut on_line = move |line: &str| {
        if let Some(f) = file.as_mut() {
            let _ = writeln!(f, "{line}");
        }
        let _ = sink_app.emit(&output_event(&sink_entry.id), line.to_string());
        let header = wordpress::is_install_item_header(line);
        let pct = progress.observe(line);
        let changed = {
            let mut st = sink_entry.state.lock().expect("install state lock");
            if header {
                st.item_cursor += 1;
            }
            let moved = pct != st.pct;
            st.pct = pct;
            header || moved
        };
        if changed {
            emit_state(&sink_app, &sink_entry);
        }
    };
    let res = repo::run_step_streamed(
        sup,
        php_bin,
        &args,
        docroot,
        &env,
        &entry.cancel,
        &mut on_line,
        None, // idle-exempt by design — see module doc (B25 tie-race)
    );

    {
        let mut st = entry.state.lock().expect("install state lock");
        match res {
            Err(e) => {
                st.status = "failed".into();
                st.error = Some(e.to_string());
            }
            Ok(sr) => {
                // Worker result is ground truth: exit 0 wins even if a
                // cancel/timeout landed in the ms exit window.
                if sr.ok {
                    st.status = "ok".into();
                    st.pct = 100; // exit-0 belt for a missed summary literal
                    st.summary = wordpress::install_summary_line(&sr.tail);
                } else if sr.cancelled {
                    st.status = if entry.timed_out.load(Ordering::SeqCst) {
                        "timed_out".into()
                    } else {
                        "cancelled".into()
                    };
                } else {
                    st.status = wordpress::classify_install_exit(false, &sr.tail).into();
                    st.summary = wordpress::install_summary_line(&sr.tail);
                    // Diagnostics = the verbatim Warning:/Error: lines (the
                    // "why" — e.g. "Plugin not found."); full log has the rest.
                    let diag: Vec<String> = sr
                        .tail
                        .iter()
                        .filter(|l| l.starts_with("Warning:") || l.starts_with("Error:"))
                        .cloned()
                        .collect();
                    st.error = if diag.is_empty() {
                        sr.tail.last().cloned()
                    } else {
                        Some(diag.join("\n"))
                    };
                }
            }
        }
    }
    emit_state(app, entry);
}

/// Cancel a running install — kills the wp-cli process group. Honest residuals
/// (the UI copy states them): the current item may remain installed-but-
/// inactive, and temp files may sit under wp-content/upgrade; a partial
/// plugin dir needs the cross-device fallback rexenv doesn't hit (fresh
/// installs place via atomic rename).
#[tauri::command]
pub async fn wp_install_cancel(
    state: State<'_, AppState>,
    jobs: State<'_, WpInstallJobs>,
    id: String,
) -> Result<()> {
    let entry = jobs
        .jobs
        .lock()
        .expect("install jobs lock")
        .get(&id)
        .cloned()
        .ok_or_else(|| Error::Other(format!("no install job {id}")))?;
    entry.cancel.cancel(state.platform.supervisor());
    Ok(())
}

/// One job's current snapshot by id (the CLI's settle-poll).
pub fn state_of(jobs: &WpInstallJobs, id: &str) -> Result<WpInstallState> {
    jobs.jobs
        .lock()
        .expect("install jobs lock")
        .get(id)
        .map(|e| snapshot(e))
        .ok_or_else(|| Error::Other(format!("no install job {id}")))
}

/// The site's most recent install job (running or settled) — lets the panel
/// re-adopt the card after a tab switch/remount.
#[tauri::command]
pub async fn wp_install_active(
    jobs: State<'_, WpInstallJobs>,
    site_id: String,
    kind: String,
) -> Result<Option<WpInstallState>> {
    let map = jobs.jobs.lock().expect("install jobs lock");
    Ok(map
        .values()
        .filter(|e| {
            let st = e.state.lock().expect("install state lock");
            st.site_id == site_id && st.kind == kind
        })
        .max_by_key(|e| e.seq)
        .map(|e| snapshot(e)))
}

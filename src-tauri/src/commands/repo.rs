//! commands::repo — Tauri IPC for "add plugin/theme from Git". Thin: parse +
//! resolve via `core::{repo, devtools, binaries}`, orchestrate a per-target
//! job whose steps stream to the UI, persist provenance on clone success.
//!
//! Events (frontend listens per job id):
//!   `repo-job://state/<id>`  — full [`RepoJobState`] snapshot per transition
//!   `repo-job://output/<id>` — one log line per event (terminal precedent)
//! Every line also lands in `logs/repo-<domain>-<dir>.log` (flat name — the
//! existing log tail IPC can read it). Repo scripts NEVER run implicitly:
//! clone+detect is one job; composer/install/build each require their own
//! explicit `repo_run_step` click from the UI.
//!
//! Blocking work runs in `spawn_blocking` with a cloned `AppHandle`; managed
//! state (`AppState`, `RepoJobs`) is re-fetched inside via `app.state()` —
//! no long-lived borrows across threads.

use crate::core::{binaries, devtools, php, repo, sites};
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::store;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

/// One resolved login-shell env snapshot, shared across jobs.
type EnvSnapshot = Arc<Vec<(String, String)>>;

/// Tauri-managed registry: this session's jobs + the cached login-shell env
/// snapshot (expensive to resolve — a shell spawn; see `login_shell_env`).
#[derive(Default)]
pub struct RepoJobs {
    jobs: Mutex<HashMap<String, Arc<JobEntry>>>,
    env: Mutex<Option<EnvSnapshot>>,
    /// Creation order for [`repo_site_jobs`] — a HashMap has none.
    next_seq: std::sync::atomic::AtomicU64,
}

struct JobEntry {
    id: String,
    seq: u64,
    site_id: String,
    kind: String,
    php_minor: String,
    dir_name: String,
    url: String,
    git_ref: Option<String>,
    dest: PathBuf,
    log_path: PathBuf,
    /// One-shot script jobs (op == "script"): (package manager, script).
    script: Option<(String, String)>,
    cancel: repo::CancelToken,
    /// One step at a time per job — a second `repo_run_step` while one runs
    /// is refused, not queued.
    step_running: AtomicBool,
    state: Mutex<RepoJobState>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoJobState {
    pub id: String,
    pub site_id: String,
    pub kind: String,
    pub dir_name: String,
    pub url: String,
    pub git_ref: Option<String>,
    /// "add" (clone+detect flow) or a git op: "fetch" | "pull" |
    /// "checkout" | "push". The add panel adopts only "add" jobs; the
    /// RepoPanel owns op jobs.
    pub op: String,
    /// Flat log-file key under log_dir (`repo-<domain>-<dir>.log`) — the UI
    /// seeds its log pane from `tail_log` when it reconnects to a live job.
    pub log_key: String,
    pub steps: Vec<RepoStepState>,
    pub inspection: Option<repo::RepoInspection>,
    pub node_warning: Option<String>,
    /// Every listed step has succeeded → the UI offers Activate.
    pub finished_ok: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoStepState {
    /// "clone" | "detect" | "composer" | "install" | "build".
    pub key: String,
    pub label: String,
    /// "pending" | "running" | "ok" | "failed" | "cancelled" | "skipped"
    /// (skipped = never ran because an earlier step in a Run-all failed or
    /// the run was cancelled; still individually re-runnable).
    pub status: String,
    pub error: Option<String>,
}

fn step(key: &str, label: &str) -> RepoStepState {
    RepoStepState { key: key.into(), label: label.into(), status: "pending".into(), error: None }
}

pub fn state_event(id: &str) -> String {
    format!("repo-job://state/{id}")
}
pub fn output_event(id: &str) -> String {
    format!("repo-job://output/{id}")
}

/// Resolve (and cache) the user's login-shell env. Blocking — call off the
/// async runtime. `refresh` drops the cache (the UI's Re-detect).
fn shell_env(state: &AppState, jobs: &RepoJobs, refresh: bool) -> Result<EnvSnapshot> {
    if !refresh {
        if let Some(env) = jobs.env.lock().expect("env lock").clone() {
            return Ok(env);
        }
    }
    let env = Arc::new(state.platform.shell().login_shell_env()?);
    *jobs.env.lock().expect("env lock") = Some(env.clone());
    Ok(env)
}

fn entry_of(jobs: &RepoJobs, job_id: &str) -> Result<Arc<JobEntry>> {
    jobs.jobs
        .lock()
        .expect("jobs lock")
        .get(job_id)
        .cloned()
        .ok_or_else(|| Error::Other(format!("no repo job {job_id}")))
}

fn snapshot(entry: &JobEntry) -> RepoJobState {
    entry.state.lock().expect("job state lock").clone()
}

fn emit_state<R: tauri::Runtime>(app: &AppHandle<R>, entry: &JobEntry) {
    let _ = app.emit(&state_event(&entry.id), snapshot(entry));
}

/// Mutate one step's status (+ optional error), recompute `finished_ok`, emit.
fn set_step<R: tauri::Runtime>(app: &AppHandle<R>, entry: &JobEntry, key: &str, status: &str, error: Option<String>) {
    {
        let mut st = entry.state.lock().expect("job state lock");
        if let Some(s) = st.steps.iter_mut().find(|s| s.key == key) {
            s.status = status.into();
            s.error = error;
        }
        st.finished_ok = st.steps.len() >= 2 && st.steps.iter().all(|s| s.status == "ok");
    }
    emit_state(app, entry);
}

/// A log sink: append to the flat per-target log file + forward each line as
/// an event. The file was truncated when the add-job started (one file = the
/// last job's log; the existing log-tail IPC can read it).
fn make_sink<R: tauri::Runtime>(app: AppHandle<R>, entry: Arc<JobEntry>) -> impl FnMut(&str) {
    let mut file =
        std::fs::OpenOptions::new().create(true).append(true).open(&entry.log_path).ok();
    move |line: &str| {
        if let Some(f) = file.as_mut() {
            let _ = writeln!(f, "{line}");
        }
        let _ = app.emit(&output_event(&entry.id), line.to_string());
    }
}

/// Load a site (brief DB lock, never held across an await).
fn site_of(state: &AppState, site_id: &str) -> Result<crate::state::models::Site> {
    let conn = state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))?;
    sites::get(&conn, site_id)?.ok_or_else(|| Error::Other(format!("no site {site_id}")))
}

// ---------------------------------------------------------------------------
// Probe
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoProbeResult {
    pub url: String,
    pub host: String,
    pub dir_name: String,
    pub ref_candidate: Option<String>,
    pub default_branch: Option<String>,
    pub branches: Vec<String>,
    pub tags: Vec<String>,
}

/// Parse the pasted text + `git ls-remote` it: validates URL AND auth before
/// any clone, and feeds the branch/tag picker.
#[tauri::command]
pub async fn repo_probe<R: tauri::Runtime>(app: AppHandle<R>, url: String) -> Result<RepoProbeResult> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let jobs = app.state::<RepoJobs>();
        let src = repo::parse_source(&url)?;
        let env = shell_env(&state, &jobs, false)?;
        let git = devtools::resolve_git(state.platform.as_ref(), &env)?;
        let refs = repo::probe_remote(state.platform.supervisor(), &git.path, &env, &src.url)?;
        Ok(RepoProbeResult {
            url: src.url,
            host: src.host,
            dir_name: src.dir_name,
            ref_candidate: src.ref_candidate,
            default_branch: refs.default_branch,
            branches: refs.branches,
            tags: refs.tags,
        })
    })
    .await
    .map_err(|e| Error::Other(format!("probe task failed: {e}")))?
}

// ---------------------------------------------------------------------------
// Add (clone + detect job)
// ---------------------------------------------------------------------------

/// Start the clone→detect job for one repo into one site's wp-content.
/// Returns the initial snapshot immediately; progress streams via events.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // flat mirror of the IPC surface
pub async fn repo_add<R: tauri::Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    jobs: State<'_, RepoJobs>,
    site_id: String,
    kind: String,
    url: String,
    git_ref: Option<String>,
    dir_name: Option<String>,
) -> Result<RepoJobState> {
    // Re-parse the RAW url server-side — the UI's normalized copy is display
    // state, not a trust boundary.
    let src = repo::parse_source(&url)?;
    // Validate the ref here too (the checkout path already does), so everything
    // reaching git argv is gated at parse time — matches the module invariant.
    // validate_ref returns the ref unchanged on success, so valid refs are
    // untouched; malformed ones are refused before the clone.
    let git_ref = git_ref.map(|r| repo::validate_ref(&r)).transpose()?;
    let site = site_of(&state, &site_id)?;
    let dir_name = match dir_name {
        Some(n) => repo::validate_dir_name(&n)?,
        None => src.dir_name.clone(),
    };
    let dest = repo::asset_dest(std::path::Path::new(&site.path), &kind, &dir_name)?;
    if dest.exists() {
        return Err(Error::Other(format!(
            "wp-content/{kind}s/{dir_name} already exists in this site — pick \
             another folder name, or remove the existing one first."
        )));
    }
    let log_key = format!("repo-{}-{}.log", site.domain, dir_name);
    let log_path = state.platform.paths().log_dir()?.join(&log_key);

    let id = uuid::Uuid::new_v4().to_string();
    let seq = jobs.next_seq.fetch_add(1, Ordering::SeqCst);
    let entry = Arc::new(JobEntry {
        id: id.clone(),
        seq,
        site_id: site_id.clone(),
        kind: kind.clone(),
        php_minor: php::minor_of(&site.php_version).to_string(),
        dir_name: dir_name.clone(),
        url: src.url.clone(),
        git_ref: git_ref.clone(),
        dest: dest.clone(),
        log_path,
        script: None,
        cancel: repo::CancelToken::new(),
        step_running: AtomicBool::new(true), // the clone worker below
        state: Mutex::new(RepoJobState {
            id: id.clone(),
            site_id,
            kind,
            dir_name,
            url: src.url,
            git_ref,
            op: "add".into(),
            log_key,
            steps: vec![step("clone", "Clone repository"), step("detect", "Detect dependencies")],
            inspection: None,
            node_warning: None,
            finished_ok: false,
        }),
    });
    {
        // Registry check + insert under ONE lock: a job for this target with a
        // step still running means a second Add is a double-clone/install —
        // refused, not queued (the UI reconnects to the live job instead).
        let mut map = jobs.jobs.lock().expect("jobs lock");
        let busy = map
            .values()
            .any(|e| e.dest == dest && e.step_running.load(Ordering::SeqCst));
        if busy {
            return Err(Error::Other(format!(
                "a job for {} is already running — reconnect to it in the \
                 From Git panel instead of starting another.",
                entry.state.lock().expect("job state lock").dir_name
            )));
        }
        map.insert(id.clone(), entry.clone());
    }
    // Fresh log AFTER the busy-refusal — never truncate a running job's log.
    let _ = std::fs::write(&entry.log_path, "");

    let worker = entry.clone();
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        run_clone_and_detect(&worker_app, &worker);
        worker.step_running.store(false, Ordering::SeqCst);
    });
    Ok(snapshot(&entry))
}

fn run_clone_and_detect<R: tauri::Runtime>(app: &AppHandle<R>, entry: &Arc<JobEntry>) {
    let state = app.state::<AppState>();
    let jobs = app.state::<RepoJobs>();
    set_step(app, entry, "clone", "running", None);
    let mut sink = make_sink(app.clone(), entry.clone());
    let outcome = (|| -> Result<()> {
        let env = shell_env(&state, &jobs, false)?;
        let git = devtools::resolve_git(state.platform.as_ref(), &env)?;
        repo::clone_repo(
            state.platform.supervisor(),
            &git.path,
            &env,
            &entry.url,
            entry.git_ref.as_deref(),
            &entry.dest,
            &entry.cancel,
            &mut sink,
        )
    })();
    match outcome {
        Err(e) if entry.cancel.is_cancelled() => {
            sink(&format!("✕ {e}"));
            set_step(app, entry, "clone", "cancelled", None);
            return;
        }
        Err(e) => {
            sink(&format!("✕ {e}"));
            set_step(app, entry, "clone", "failed", Some(e.to_string()));
            return;
        }
        Ok(()) => set_step(app, entry, "clone", "ok", None),
    }

    // Detection: read-only fs — runs no repo code.
    set_step(app, entry, "detect", "running", None);
    let inspection = repo::inspect_repo(&entry.dest);
    let node_warning = inspection.node_want.as_deref().and_then(|want| {
        let env = shell_env(&state, &jobs, false).ok()?;
        let node = devtools::resolve_node(&env).ok()?;
        repo::node_version_warning(want, node.version.as_deref().unwrap_or(""))
    });
    if inspection.wp.kind == "none" {
        sink(
            "! no plugin/theme header found at the repo root — monorepo? \
             WordPress won't list it until a header exists.",
        );
    } else {
        sink(&format!(
            "✓ {} header: {}",
            inspection.wp.kind,
            inspection.wp.name.as_deref().unwrap_or("?")
        ));
    }
    {
        let mut st = entry.state.lock().expect("job state lock");
        if inspection.composer {
            st.steps.push(step("composer", "composer install"));
        }
        if let Some(node) = &inspection.node {
            st.steps.push(step("install", &format!("{} install", node.manager)));
            if node.has_build {
                st.steps.push(step("build", &format!("{} run build", node.manager)));
            }
        }
        st.inspection = Some(inspection);
        st.node_warning = node_warning;
    }
    set_step(app, entry, "detect", "ok", None);

    // Clone landed → record provenance (badge + future update/watch). Named
    // binding (not an if-let temporary) so the guard drops before `state`.
    let conn = state.db.lock().ok();
    if let Some(conn) = conn.as_deref() {
        let _ = store::upsert_git_asset(
            conn,
            &entry.site_id,
            &entry.kind,
            &entry.dir_name,
            &entry.url,
            entry.git_ref.as_deref(),
            "cloned",
        );
    }
}

// ---------------------------------------------------------------------------
// Explicit steps (composer / install / build) — one click each, by design
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn repo_run_step<R: tauri::Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    jobs: State<'_, RepoJobs>,
    job_id: String,
    step_key: String,
) -> Result<()> {
    let entry = entry_of(&jobs, &job_id)?;
    let offered = snapshot(&entry).steps.iter().any(|s| s.key == step_key);
    // Allow-list: ONLY the dependency steps are re-runnable through here —
    // op/clone/script steps re-run by creating a fresh job instead.
    if !offered || !matches!(step_key.as_str(), "composer" | "install" | "build") {
        return Err(Error::Other(format!("step \"{step_key}\" is not runnable for this job")));
    }
    if entry.step_running.swap(true, Ordering::SeqCst) {
        return Err(Error::Other("another step is already running for this job".into()));
    }
    // Anything async (binaries hub downloads) resolves HERE; on any error the
    // running flag is released before returning.
    let resolve = async {
        match step_key.as_str() {
            "composer" => {
                let patch = php::patch_for_minor(&entry.php_minor).ok_or_else(|| {
                    Error::Other(format!("no pinned PHP build for {}", entry.php_minor))
                })?;
                let php_bin = binaries::resolve(state.platform.as_ref(), "php", patch).await?;
                let phar = binaries::resolve_file(
                    state.platform.as_ref(),
                    "composer",
                    binaries::COMPOSER_VERSION,
                )
                .await?;
                Ok::<_, Error>(Some((php_bin, phar)))
            }
            _ => Ok(None),
        }
    };
    let composer_tools = match resolve.await {
        Ok(t) => t,
        Err(e) => {
            entry.step_running.store(false, Ordering::SeqCst);
            return Err(e);
        }
    };

    let worker = entry.clone();
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        run_one_step(&worker_app, &worker, &step_key, composer_tools);
        worker.step_running.store(false, Ordering::SeqCst);
    });
    Ok(())
}

fn run_one_step<R: tauri::Runtime>(
    app: &AppHandle<R>,
    entry: &Arc<JobEntry>,
    step_key: &str,
    composer_tools: Option<(PathBuf, PathBuf)>,
) {
    let state = app.state::<AppState>();
    let jobs = app.state::<RepoJobs>();
    set_step(app, entry, step_key, "running", None);
    let mut sink = make_sink(app.clone(), entry.clone());
    let sup = state.platform.supervisor();
    let outcome = (|| -> Result<()> {
        let env = shell_env(&state, &jobs, false)?;
        match step_key {
            "composer" => {
                let (php_bin, phar) = composer_tools
                    .as_ref()
                    .ok_or_else(|| Error::Other("composer tools missing".into()))?;
                repo::composer_install(
                    sup,
                    php_bin,
                    phar,
                    &entry.dest,
                    &env,
                    &entry.cancel,
                    &mut sink,
                )
            }
            "install" | "build" => {
                let manager = snapshot(entry)
                    .inspection
                    .and_then(|i| i.node)
                    .map(|n| n.manager)
                    .ok_or_else(|| Error::Other("no package.json detected".into()))?;
                let pm = devtools::resolve_package_manager(&env, &manager)?;
                if step_key == "install" {
                    repo::node_install(sup, &pm.path, &entry.dest, &env, &entry.cancel, &mut sink)
                } else {
                    repo::node_build(sup, &pm.path, &entry.dest, &env, &entry.cancel, &mut sink)
                }
            }
            other => Err(Error::Other(format!("unknown step {other}"))),
        }
    })();
    match outcome {
        Err(e) if entry.cancel.is_cancelled() => {
            sink(&format!("✕ {e}"));
            set_step(app, entry, step_key, "cancelled", None);
        }
        Err(e) => {
            sink(&format!("✕ {e}"));
            set_step(app, entry, step_key, "failed", Some(e.to_string()));
        }
        Ok(()) => {
            set_step(app, entry, step_key, "ok", None);
            // Successful install: record the family's input fingerprint so
            // "Check deps" can later prove up-to-date vs stale. Best-effort —
            // no provenance row (or a poisoned lock) just leaves the marker
            // NULL, which reads as "unverified", never as a false verdict.
            if matches!(step_key, "composer" | "install") {
                let (family, fp) = if step_key == "composer" {
                    ("composer", repo::composer_fingerprint(&entry.dest))
                } else {
                    ("node", repo::node_fingerprint(&entry.dest))
                };
                if let Some(conn) = state.db.lock().ok().as_deref() {
                    let _ = store::set_git_asset_fp(
                        conn, &entry.site_id, &entry.kind, &entry.dir_name, family, &fp,
                    );
                }
            }
        }
    }
}

/// The job's offered dependency steps still pending, in offer order
/// (composer → install → build). Pure — unit-tested.
fn offered_pending(steps: &[RepoStepState]) -> Vec<String> {
    steps
        .iter()
        .filter(|s| matches!(s.key.as_str(), "composer" | "install" | "build"))
        .filter(|s| s.status == "pending")
        .map(|s| s.key.clone())
        .collect()
}

/// Mark every still-pending offered step "skipped" — they never ran and,
/// this run, won't (an earlier step failed or the run was cancelled).
/// Distinct from "pending" (might still run) and "cancelled" (was killed
/// mid-run). Pure — unit-tested; steps stay individually re-runnable.
fn skip_pending_offered(steps: &mut [RepoStepState]) {
    for s in steps.iter_mut() {
        if matches!(s.key.as_str(), "composer" | "install" | "build") && s.status == "pending" {
            s.status = "skipped".into();
        }
    }
}

fn mark_remaining_skipped<R: tauri::Runtime>(app: &AppHandle<R>, entry: &Arc<JobEntry>) {
    {
        let mut st = entry.state.lock().expect("job state lock");
        skip_pending_offered(&mut st.steps);
    }
    emit_state(app, entry);
}

/// Run ALL of a job's offered dependency steps sequentially, STOPPING at the
/// first failure ("Run all" in the panel; `--install` on the CLI — one
/// implementation, promoted from cli_server's per-step polling loop).
///
/// Concurrency: acquires the job's `step_running` flag once and HOLDS it for
/// the whole sequence — the old loop released it between steps, leaving
/// ≥300ms windows where the one-job-per-dest scans admitted a concurrent git
/// op on the same dir. A small window remains at sequence START (between the
/// previous worker's release and this acquire poll); closing it would need
/// job-worker chaining — accepted and stated, not claimed airtight.
///
/// Honesty: the failing step keeps its mapped error; steps that never ran
/// are "skipped" (never "failed", never left "pending"). Cancel kills the
/// current step (→ "cancelled" via run_one_step) and the rest are skipped —
/// the cancelled flag persists, so nothing else can start.
pub async fn run_offered_steps<R: tauri::Runtime>(
    app: AppHandle<R>,
    job_id: String,
) -> Result<RepoJobState> {
    let entry = {
        let jobs = app.state::<RepoJobs>();
        entry_of(&jobs, &job_id)?
    };
    // Acquire: wait for the job's own worker (clone/op/check) to settle, then
    // take the flag in the same swap that observes it free.
    loop {
        if !entry.step_running.swap(true, Ordering::SeqCst) {
            break;
        }
        if entry.cancel.is_cancelled() {
            return Ok(snapshot(&entry));
        }
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }
    // The flag is ours from here — release on every exit path.
    let keys = offered_pending(&snapshot(&entry).steps);
    if keys.is_empty() {
        entry.step_running.store(false, Ordering::SeqCst);
        return Ok(snapshot(&entry));
    }
    let composer_tools = if keys.iter().any(|k| k == "composer") {
        let resolve = async {
            let state = app.state::<AppState>();
            let patch = php::patch_for_minor(&entry.php_minor).ok_or_else(|| {
                Error::Other(format!("no pinned PHP build for {}", entry.php_minor))
            })?;
            let php_bin = binaries::resolve(state.platform.as_ref(), "php", patch).await?;
            let phar = binaries::resolve_file(
                state.platform.as_ref(),
                "composer",
                binaries::COMPOSER_VERSION,
            )
            .await?;
            Ok::<_, Error>(Some((php_bin, phar)))
        };
        match resolve.await {
            Ok(t) => t,
            Err(e) => {
                entry.step_running.store(false, Ordering::SeqCst);
                return Err(e);
            }
        }
    } else {
        None
    };
    let worker = entry.clone();
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        for key in &keys {
            if worker.cancel.is_cancelled() {
                mark_remaining_skipped(&worker_app, &worker);
                break;
            }
            let tools = (key == "composer").then(|| composer_tools.clone()).flatten();
            run_one_step(&worker_app, &worker, key, tools);
            let ok = snapshot(&worker).steps.iter().any(|s| &s.key == key && s.status == "ok");
            if !ok {
                mark_remaining_skipped(&worker_app, &worker);
                break;
            }
        }
        worker.step_running.store(false, Ordering::SeqCst);
    })
    .await
    .map_err(|e| Error::Other(format!("run-all task failed: {e}")))?;
    Ok(snapshot(&entry))
}

/// "Run all" for the panel — fire-and-forget wrapper over
/// [`run_offered_steps`]: the UI is event-driven, so this returns
/// immediately; the CLI awaits the promoted fn directly instead.
#[tauri::command]
pub async fn repo_run_offered_steps<R: tauri::Runtime>(
    app: AppHandle<R>,
    jobs: State<'_, RepoJobs>,
    job_id: String,
) -> Result<RepoJobState> {
    let entry = entry_of(&jobs, &job_id)?;
    let snap = snapshot(&entry);
    if offered_pending(&snap.steps).is_empty() {
        return Err(Error::Other("no pending dependency steps to run".into()));
    }
    let run_app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = run_offered_steps(run_app, job_id).await;
    });
    Ok(snap)
}

/// Zero-exec dependency check — pure fs reads + stored-fingerprint compares;
/// NEVER runs composer/npm/repo code. Shaped as a job (op="check", step key
/// "check" — the key MUST equal the op so the CLI settle detector sees it)
/// so the report + offered install steps ride the existing card, events,
/// busy guard, and consent flow. Two deliberate differences from git ops:
/// - AWAITED: the worker is instant, and the UI subscribes to job events only
///   after this command returns — fire-and-forget would emit the final state
///   before any listener exists. Returning the SETTLED snapshot closes that.
/// - OWN LOG SLOT (`repo-<domain>-<dir>-check.log`, the watch-log precedent):
///   Check is meant to be pressed casually — it must not truncate the
///   previous op/build log ("why did my build fail?" must survive a Check).
#[tauri::command]
pub async fn repo_check<R: tauri::Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    jobs: State<'_, RepoJobs>,
    site_id: String,
    kind: String,
    dir_name: String,
) -> Result<RepoJobState> {
    let site = site_of(&state, &site_id)?;
    let dir_name = repo::validate_dir_name(&dir_name)?;
    let dest = repo::asset_dest(std::path::Path::new(&site.path), &kind, &dir_name)?;
    if !dest.join(".git").exists() {
        return Err(Error::Other(format!(
            "wp-content/{kind}s/{dir_name} is not a git checkout (no .git)."
        )));
    }
    let log_key = format!("repo-{}-{}-check.log", site.domain, dir_name);
    let log_path = state.platform.paths().log_dir()?.join(&log_key);

    let id = uuid::Uuid::new_v4().to_string();
    let seq = jobs.next_seq.fetch_add(1, Ordering::SeqCst);
    let entry = Arc::new(JobEntry {
        id: id.clone(),
        seq,
        site_id: site_id.clone(),
        kind: kind.clone(),
        php_minor: php::minor_of(&site.php_version).to_string(),
        dir_name: dir_name.clone(),
        url: String::new(),
        git_ref: None,
        dest: dest.clone(),
        log_path,
        script: None,
        cancel: repo::CancelToken::new(),
        step_running: AtomicBool::new(true),
        state: Mutex::new(RepoJobState {
            id: id.clone(),
            site_id,
            kind,
            dir_name,
            url: String::new(),
            git_ref: None,
            op: "check".into(),
            log_key,
            steps: vec![step("check", "Check dependencies")],
            inspection: None,
            node_warning: None,
            finished_ok: false,
        }),
    });
    {
        let mut map = jobs.jobs.lock().expect("jobs lock");
        let busy = map
            .values()
            .any(|e| e.dest == dest && e.step_running.load(Ordering::SeqCst));
        if busy {
            return Err(Error::Other(format!(
                "a job for {} is already running — wait for it (or cancel it) first.",
                entry.state.lock().expect("job state lock").dir_name
            )));
        }
        map.insert(id.clone(), entry.clone());
    }
    let _ = std::fs::write(&entry.log_path, "");

    let worker = entry.clone();
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        run_check_job(&worker_app, &worker);
        worker.step_running.store(false, Ordering::SeqCst);
    })
    .await
    .map_err(|e| Error::Other(format!("check task failed: {e}")))?;
    Ok(snapshot(&entry))
}

/// One human line per family. Only Missing/Stale earn a button — the rest is
/// report, not alarm.
fn dep_verdict_line(label: &str, v: repo::DepVerdict, installed_dir: &str) -> String {
    match v {
        repo::DepVerdict::NotApplicable => format!("{label}: not used (no manifest)"),
        repo::DepVerdict::Missing => {
            format!("{label}: {installed_dir}/ missing — install needed")
        }
        repo::DepVerdict::Stale => {
            format!("{label}: lockfile changed since last install — install recommended")
        }
        repo::DepVerdict::UpToDate => format!("{label}: up to date (matches last install)"),
        repo::DepVerdict::Unverified => format!(
            "{label}: {installed_dir}/ present — installed outside rexenv, can't verify \
             against the lockfile"
        ),
    }
}

fn run_check_job<R: tauri::Runtime>(app: &AppHandle<R>, entry: &Arc<JobEntry>) {
    let state = app.state::<AppState>();
    set_step(app, entry, "check", "running", None);
    let mut sink = make_sink(app.clone(), entry.clone());

    let inspection = repo::inspect_repo(&entry.dest);
    let (c_fp, n_fp) = state
        .db
        .lock()
        .ok()
        .as_deref()
        .and_then(|c| store::get_git_asset_fps(c, &entry.site_id, &entry.kind, &entry.dir_name).ok())
        .unwrap_or((None, None));
    let report = repo::check_deps(&entry.dest, &inspection, c_fp.as_deref(), n_fp.as_deref());

    sink(&dep_verdict_line("composer", report.composer, &report.vendor_dir));
    let node_label = inspection
        .node
        .as_ref()
        .map(|n| format!("node ({})", n.manager))
        .unwrap_or_else(|| "node".into());
    sink(&dep_verdict_line(&node_label, report.node, "node_modules"));

    let offer_composer = repo::needs_install(report.composer);
    let offer_node = repo::needs_install(report.node);
    if offer_composer || offer_node {
        sink("! install steps offered below — nothing runs without a click.");
    } else {
        sink("✓ nothing to install.");
    }
    {
        // Offered steps mirror the pull/checkout offer block; inspection MUST
        // be stored — run_one_step reads it to resolve the node manager.
        let mut st = entry.state.lock().expect("job state lock");
        if offer_composer {
            st.steps.push(step("composer", "composer install"));
        }
        if let (true, Some(node)) = (offer_node, &inspection.node) {
            st.steps.push(step("install", &format!("{} install", node.manager)));
            if node.has_build {
                st.steps.push(step("build", &format!("{} run build", node.manager)));
            }
        }
        st.inspection = Some(inspection);
    }
    set_step(app, entry, "check", "ok", None);
}

// ---------------------------------------------------------------------------
// Cancel / state / assets / tools
// ---------------------------------------------------------------------------

/// Cancel the job's RUNNING step (kills its whole process group). A cancelled
/// clone removes its partial checkout (`core::repo` guarantees that).
#[tauri::command]
pub async fn repo_cancel(
    state: State<'_, AppState>,
    jobs: State<'_, RepoJobs>,
    job_id: String,
) -> Result<()> {
    let entry = entry_of(&jobs, &job_id)?;
    entry.cancel.cancel(state.platform.supervisor());
    Ok(())
}

/// This session's jobs for one site+kind, creation-ordered — the panel
/// RECONNECTS to a live/unfinished job after a tab-switch remount (the
/// backend job survives the UI; a blank panel invited a dangerous second
/// run — same bug class as the mount-frozen DNS mode tile).
#[tauri::command]
pub async fn repo_site_jobs(
    jobs: State<'_, RepoJobs>,
    site_id: String,
    kind: String,
) -> Result<Vec<RepoJobState>> {
    let mut entries: Vec<Arc<JobEntry>> = jobs
        .jobs
        .lock()
        .expect("jobs lock")
        .values()
        .filter(|e| e.site_id == site_id && e.kind == kind)
        .cloned()
        .collect();
    entries.sort_by_key(|e| e.seq);
    Ok(entries.iter().map(|e| snapshot(e)).collect())
}

/// Poll/refresh a job's snapshot (the UI re-syncs after a remount).
#[tauri::command]
pub async fn repo_job_state(jobs: State<'_, RepoJobs>, job_id: String) -> Result<RepoJobState> {
    let entry = entry_of(&jobs, &job_id)?;
    Ok(snapshot(&entry))
}

/// A site's git-sourced dirs (list badges).
#[tauri::command]
pub async fn repo_assets(
    state: State<'_, AppState>,
    site_id: String,
) -> Result<Vec<crate::state::models::GitAsset>> {
    let conn = state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))?;
    store::get_git_assets(&conn, &site_id)
}

// ---------------------------------------------------------------------------
// Git ops (phase B) — fetch / pull --ff-only / checkout / push as jobs
// ---------------------------------------------------------------------------

/// Start one git op as a streamed job on a managed asset. Same registry,
/// events, cancel, and one-job-per-dest rule as the add flow. After a
/// successful pull/checkout whose lockfiles changed, install/build steps are
/// OFFERED on the job (explicit clicks — never auto-run).
#[tauri::command]
#[allow(clippy::too_many_arguments)] // flat mirror of the IPC surface
pub async fn repo_git_op<R: tauri::Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    jobs: State<'_, RepoJobs>,
    site_id: String,
    kind: String,
    dir_name: String,
    op: String,
    target_ref: Option<String>,
) -> Result<RepoJobState> {
    if !matches!(op.as_str(), "fetch" | "pull" | "checkout" | "push") {
        return Err(Error::Other(format!("unknown git op \"{op}\"")));
    }
    let target_ref = match (op.as_str(), target_ref) {
        ("checkout", Some(r)) => Some(repo::validate_ref(&r)?),
        ("checkout", None) => {
            return Err(Error::Other("checkout needs a branch or tag".into()));
        }
        (_, r) => r,
    };
    let site = site_of(&state, &site_id)?;
    let dir_name = repo::validate_dir_name(&dir_name)?;
    let dest = repo::asset_dest(std::path::Path::new(&site.path), &kind, &dir_name)?;
    if !dest.join(".git").exists() {
        return Err(Error::Other(format!(
            "wp-content/{kind}s/{dir_name} is not a git checkout (no .git)."
        )));
    }
    let log_key = format!("repo-{}-{}.log", site.domain, dir_name);
    let log_path = state.platform.paths().log_dir()?.join(&log_key);

    let label = match op.as_str() {
        "fetch" => "git fetch".to_string(),
        "pull" => "git pull --ff-only".to_string(),
        "checkout" => format!("git checkout {}", target_ref.as_deref().unwrap_or("?")),
        _ => "git push".to_string(),
    };
    let id = uuid::Uuid::new_v4().to_string();
    let seq = jobs.next_seq.fetch_add(1, Ordering::SeqCst);
    let entry = Arc::new(JobEntry {
        id: id.clone(),
        seq,
        site_id: site_id.clone(),
        kind: kind.clone(),
        php_minor: php::minor_of(&site.php_version).to_string(),
        dir_name: dir_name.clone(),
        url: String::new(),
        git_ref: target_ref.clone(),
        dest: dest.clone(),
        log_path,
        script: None,
        cancel: repo::CancelToken::new(),
        step_running: AtomicBool::new(true), // the worker below
        state: Mutex::new(RepoJobState {
            id: id.clone(),
            site_id,
            kind,
            dir_name,
            url: String::new(),
            git_ref: target_ref.clone(),
            op: op.clone(),
            log_key,
            steps: vec![step(&op, &label)],
            inspection: None,
            node_warning: None,
            finished_ok: false,
        }),
    });
    {
        let mut map = jobs.jobs.lock().expect("jobs lock");
        let busy = map
            .values()
            .any(|e| e.dest == dest && e.step_running.load(Ordering::SeqCst));
        if busy {
            return Err(Error::Other(format!(
                "a job for {} is already running — wait for it (or cancel it) first.",
                entry.state.lock().expect("job state lock").dir_name
            )));
        }
        map.insert(id.clone(), entry.clone());
    }
    let _ = std::fs::write(&entry.log_path, ""); // fresh log per job

    let worker = entry.clone();
    let worker_app = app.clone();
    let op_key = op.clone();
    tauri::async_runtime::spawn_blocking(move || {
        run_git_op_job(&worker_app, &worker, &op_key);
        worker.step_running.store(false, Ordering::SeqCst);
    });
    Ok(snapshot(&entry))
}

fn run_git_op_job<R: tauri::Runtime>(app: &AppHandle<R>, entry: &Arc<JobEntry>, op: &str) {
    let state = app.state::<AppState>();
    let jobs = app.state::<RepoJobs>();
    set_step(app, entry, op, "running", None);
    let mut sink = make_sink(app.clone(), entry.clone());
    let before = repo::lockfile_fingerprint(&entry.dest);
    let outcome = (|| -> Result<()> {
        let env = shell_env(&state, &jobs, false)?;
        let git = devtools::resolve_git(state.platform.as_ref(), &env)?;
        let sup = state.platform.supervisor();
        match op {
            "fetch" => repo::git_fetch(sup, &git.path, &env, &entry.dest, &entry.cancel, &mut sink),
            "pull" => repo::git_pull_ff(sup, &git.path, &env, &entry.dest, &entry.cancel, &mut sink),
            // PR/MR refs take the fetch-then-detach flow; everything else is
            // a plain checkout (branch DWIM / tag detach).
            "checkout" => {
                let target = entry.git_ref.as_deref().unwrap_or_default();
                if repo::is_pull_ref(target) {
                    repo::git_checkout_pull_ref(
                        sup, &git.path, &env, &entry.dest, target, &entry.cancel, &mut sink,
                    )
                } else {
                    repo::git_checkout(
                        sup, &git.path, &env, &entry.dest, target, &entry.cancel, &mut sink,
                    )
                }
            }
            _ => repo::git_push(sup, &git.path, &env, &entry.dest, &entry.cancel, &mut sink),
        }
    })();
    match outcome {
        Err(e) if entry.cancel.is_cancelled() => {
            sink(&format!("✕ {e}"));
            set_step(app, entry, op, "cancelled", None);
            return;
        }
        Err(e) => {
            sink(&format!("✕ {e}"));
            set_step(app, entry, op, "failed", Some(e.to_string()));
            return;
        }
        Ok(()) => {}
    }

    // Checkout: the provenance row keeps saying the truth.
    if op == "checkout" {
        if let Some(r) = entry.git_ref.as_deref() {
            let conn = state.db.lock().ok();
            if let Some(conn) = conn.as_deref() {
                let _ = store::set_git_asset_ref(conn, &entry.site_id, &entry.kind, &entry.dir_name, r);
            }
        }
    }

    // Dependencies changed under a pull/checkout? OFFER install/build steps
    // on this job (explicit clicks — repo_run_step handles them as usual).
    if matches!(op, "pull" | "checkout") && repo::lockfile_fingerprint(&entry.dest) != before {
        let inspection = repo::inspect_repo(&entry.dest);
        sink("! dependencies changed (lockfile) — run install below.");
        let mut st = entry.state.lock().expect("job state lock");
        if inspection.composer {
            st.steps.push(step("composer", "composer install"));
        }
        if let Some(node) = &inspection.node {
            st.steps.push(step("install", &format!("{} install", node.manager)));
            if node.has_build {
                st.steps.push(step("build", &format!("{} run build", node.manager)));
            }
        }
        st.inspection = Some(inspection);
    }
    set_step(app, entry, op, "ok", None);
}

/// Local + remote-tracking branch names + local tags for the checkout picker.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoBranches {
    pub current: Option<String>,
    pub local: Vec<String>,
    pub remote: Vec<String>,
    /// Local tags, newest first (creatordate covers lightweight tags too).
    pub tags: Vec<String>,
}

#[tauri::command]
pub async fn repo_branches<R: tauri::Runtime>(
    app: AppHandle<R>,
    site_id: String,
    kind: String,
    dir_name: String,
) -> Result<RepoBranches> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let jobs = app.state::<RepoJobs>();
        let site = site_of(&state, &site_id)?;
        let dir = repo::asset_dest(std::path::Path::new(&site.path), &kind, &dir_name)?;
        let env = shell_env(&state, &jobs, false)?;
        let git = devtools::resolve_git(state.platform.as_ref(), &env)?;
        let status = repo::read_git_status(state.platform.supervisor(), &git.path, &env, &dir)?;
        let list = |args: &[&str]| -> Vec<String> {
            repo::run_git_lines(state.platform.supervisor(), &git.path, &env, &dir, args).unwrap_or_default()
        };
        let local = list(&["branch", "--format=%(refname:short)"]);
        let remote = list(&["branch", "-r", "--format=%(refname:short)"])
            .into_iter()
            .filter(|b| !b.ends_with("/HEAD") && !b.contains(" -> "))
            .collect();
        let tags =
            list(&["for-each-ref", "refs/tags", "--format=%(refname:short)", "--sort=-creatordate"]);
        Ok(RepoBranches { current: status.branch, local, remote, tags })
    })
    .await
    .map_err(|e| Error::Other(format!("branches task failed: {e}")))?
}

/// PR/MR head refs advertised by `origin` — the picker's Pull Requests group.
/// Network (one ls-remote round trip); the UI calls it lazily on picker open.
#[tauri::command]
pub async fn repo_pull_refs<R: tauri::Runtime>(
    app: AppHandle<R>,
    site_id: String,
    kind: String,
    dir_name: String,
) -> Result<Vec<repo::PullRef>> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let jobs = app.state::<RepoJobs>();
        let site = site_of(&state, &site_id)?;
        let dir = repo::asset_dest(std::path::Path::new(&site.path), &kind, &dir_name)?;
        let env = shell_env(&state, &jobs, false)?;
        let git = devtools::resolve_git(state.platform.as_ref(), &env)?;
        repo::list_pull_refs(state.platform.supervisor(), &git.path, &env, &dir)
    })
    .await
    .map_err(|e| Error::Other(format!("pull-refs task failed: {e}")))?
}

// ---------------------------------------------------------------------------
// Scripts + watch registry (phase C)
// ---------------------------------------------------------------------------

/// package.json scripts for one asset + the manager that would run them.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoScriptsInfo {
    pub manager: Option<String>,
    pub scripts: Vec<repo::RepoScript>,
}

#[tauri::command]
pub async fn repo_scripts(
    state: State<'_, AppState>,
    site_id: String,
    kind: String,
    dir_name: String,
) -> Result<RepoScriptsInfo> {
    let site = site_of(&state, &site_id)?;
    let dir = repo::asset_dest(std::path::Path::new(&site.path), &kind, &dir_name)?;
    let inspection = repo::inspect_repo(&dir);
    Ok(RepoScriptsInfo {
        manager: inspection.node.map(|n| n.manager),
        scripts: repo::list_scripts(&dir),
    })
}

/// Run one script ONCE as a streamed job (op == "script"). Watchy or not —
/// this is the explicit-click "Run"; watching goes through repo_watch_start.
#[tauri::command]
pub async fn repo_script_job<R: tauri::Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    jobs: State<'_, RepoJobs>,
    site_id: String,
    kind: String,
    dir_name: String,
    script: String,
) -> Result<RepoJobState> {
    let site = site_of(&state, &site_id)?;
    let dir_name = repo::validate_dir_name(&dir_name)?;
    let dest = repo::asset_dest(std::path::Path::new(&site.path), &kind, &dir_name)?;
    let inspection = repo::inspect_repo(&dest);
    let manager = inspection
        .node
        .map(|n| n.manager)
        .ok_or_else(|| Error::Other("this folder has no package.json".into()))?;
    let known = repo::list_scripts(&dest);
    if !known.iter().any(|sc| sc.name == script) {
        return Err(Error::Other(format!("no script \"{script}\" in package.json")));
    }
    let log_key = format!("repo-{}-{}.log", site.domain, dir_name);
    let log_path = state.platform.paths().log_dir()?.join(&log_key);
    let id = uuid::Uuid::new_v4().to_string();
    let seq = jobs.next_seq.fetch_add(1, Ordering::SeqCst);
    let entry = Arc::new(JobEntry {
        id: id.clone(),
        seq,
        site_id: site_id.clone(),
        kind: kind.clone(),
        php_minor: php::minor_of(&site.php_version).to_string(),
        dir_name: dir_name.clone(),
        url: String::new(),
        git_ref: None,
        dest: dest.clone(),
        log_path,
        script: Some((manager.clone(), script.clone())),
        cancel: repo::CancelToken::new(),
        step_running: AtomicBool::new(true),
        state: Mutex::new(RepoJobState {
            id: id.clone(),
            site_id,
            kind,
            dir_name,
            url: String::new(),
            git_ref: None,
            op: "script".into(),
            log_key,
            steps: vec![step("script", &format!("{manager} run {script}"))],
            inspection: None,
            node_warning: None,
            finished_ok: false,
        }),
    });
    {
        let mut map = jobs.jobs.lock().expect("jobs lock");
        let busy = map
            .values()
            .any(|e| e.dest == dest && e.step_running.load(Ordering::SeqCst));
        if busy {
            return Err(Error::Other(format!(
                "a job for {} is already running — wait for it (or cancel it) first.",
                entry.state.lock().expect("job state lock").dir_name
            )));
        }
        map.insert(id.clone(), entry.clone());
    }
    let _ = std::fs::write(&entry.log_path, "");
    let worker = entry.clone();
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        run_script_job(&worker_app, &worker);
        worker.step_running.store(false, Ordering::SeqCst);
    });
    Ok(snapshot(&entry))
}

fn run_script_job<R: tauri::Runtime>(app: &AppHandle<R>, entry: &Arc<JobEntry>) {
    let state = app.state::<AppState>();
    let jobs = app.state::<RepoJobs>();
    set_step(app, entry, "script", "running", None);
    let mut sink = make_sink(app.clone(), entry.clone());
    let outcome = (|| -> Result<()> {
        let (manager, script) = entry
            .script
            .clone()
            .ok_or_else(|| Error::Other("script job without a script".into()))?;
        let env = shell_env(&state, &jobs, false)?;
        let pm = devtools::resolve_package_manager(&env, &manager)?;
        let result = repo::node_run_script(
            state.platform.supervisor(),
            &pm.path,
            &entry.dest,
            &script,
            &env,
            &entry.cancel,
            &mut sink,
        )?;
        if result.ok {
            Ok(())
        } else if result.cancelled {
            Err(Error::Other("script cancelled".into()))
        } else {
            Err(repo::map_node_error(&result.tail))
        }
    })();
    match outcome {
        Err(e) if entry.cancel.is_cancelled() => {
            sink(&format!("✕ {e}"));
            set_step(app, entry, "script", "cancelled", None);
        }
        Err(e) => {
            sink(&format!("✕ {e}"));
            set_step(app, entry, "script", "failed", Some(e.to_string()));
        }
        Ok(()) => set_step(app, entry, "script", "ok", None),
    }
}

/// Live watch processes (npm run dev/watch/…). NOT jobs and NOT ServiceManager
/// services: a watcher belongs to an editing session — it dies WITH the app
/// (exit hook), never auto-restarts (a crashed watcher shows its exit code +
/// a Restart button; auto-restarting arbitrary user scripts is a surprise
/// generator). Max ONE per asset dir.
#[derive(Default)]
pub struct RepoWatches {
    watches: Mutex<HashMap<String, Arc<WatchEntry>>>,
}

struct WatchEntry {
    id: String,
    /// One-per-asset key: "<site_id>/<kind>/<dir>".
    asset_key: String,
    cancel: repo::CancelToken,
    state: Mutex<WatchState>,
    ring: Mutex<std::collections::VecDeque<String>>,
}

const WATCH_RING_CAP: usize = 400;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchState {
    pub id: String,
    pub site_id: String,
    pub kind: String,
    pub dir_name: String,
    pub script: String,
    /// "running" | "exited".
    pub status: String,
    pub exit: Option<i32>,
}

fn watch_asset_key(site_id: &str, kind: &str, dir_name: &str) -> String {
    format!("{site_id}/{kind}/{dir_name}")
}

pub fn watch_state_event(id: &str) -> String {
    format!("repo-watch://state/{id}")
}
pub fn watch_output_event(id: &str) -> String {
    format!("repo-watch://output/{id}")
}
/// Global event: full watch list on every change (footer chip).
pub const WATCH_GLOBAL_EVENT: &str = "repo-watch-global";

fn emit_watch_global<R: tauri::Runtime>(app: &AppHandle<R>) {
    let Some(watches) = app.try_state::<RepoWatches>() else {
        return;
    };
    let all: Vec<WatchState> = watches
        .watches
        .lock()
        .expect("watches lock")
        .values()
        .map(|w| w.state.lock().expect("watch state").clone())
        .collect();
    let _ = app.emit(WATCH_GLOBAL_EVENT, all);
}

/// Start watching: `<manager> run <script>` in the asset dir, streamed to the
/// ring + events + `repo-<domain>-<dir>-watch.log` (the Logs tab picks the
/// flat name up automatically).
#[tauri::command]
pub async fn repo_watch_start<R: tauri::Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    watches: State<'_, RepoWatches>,
    site_id: String,
    kind: String,
    dir_name: String,
    script: String,
) -> Result<WatchState> {
    let site = site_of(&state, &site_id)?;
    let dir_name = repo::validate_dir_name(&dir_name)?;
    let dest = repo::asset_dest(std::path::Path::new(&site.path), &kind, &dir_name)?;
    let inspection = repo::inspect_repo(&dest);
    let manager = inspection
        .node
        .map(|n| n.manager)
        .ok_or_else(|| Error::Other("this folder has no package.json".into()))?;
    if !repo::list_scripts(&dest).iter().any(|sc| sc.name == script) {
        return Err(Error::Other(format!("no script \"{script}\" in package.json")));
    }
    let asset_key = watch_asset_key(&site_id, &kind, &dir_name);
    let id = uuid::Uuid::new_v4().to_string();
    let entry = Arc::new(WatchEntry {
        id: id.clone(),
        asset_key: asset_key.clone(),
        cancel: repo::CancelToken::new(),
        state: Mutex::new(WatchState {
            id: id.clone(),
            site_id,
            kind,
            dir_name: dir_name.clone(),
            script: script.clone(),
            status: "running".into(),
            exit: None,
        }),
        ring: Mutex::new(std::collections::VecDeque::with_capacity(WATCH_RING_CAP)),
    });
    {
        // One watcher per asset, checked+inserted under one lock. A dead
        // (exited) entry for the same asset is replaced.
        let mut map = watches.watches.lock().expect("watches lock");
        let running = map.values().any(|w| {
            w.asset_key == asset_key
                && w.state.lock().expect("watch state").status == "running"
        });
        if running {
            return Err(Error::Other(format!(
                "already watching {dir_name} — stop that watcher first."
            )));
        }
        map.retain(|_, w| w.asset_key != asset_key);
        map.insert(id.clone(), entry.clone());
    }
    let log_path = state
        .platform
        .paths()
        .log_dir()?
        .join(format!("repo-{}-{}-watch.log", site.domain, dir_name));
    let _ = std::fs::write(&log_path, "");
    emit_watch_global(&app);

    let worker = entry.clone();
    let worker_app = app.clone();
    let pm_name = manager;
    tauri::async_runtime::spawn_blocking(move || {
        run_watch(&worker_app, &worker, &pm_name, &log_path);
    });
    Ok(snapshot_watch(&entry))
}

fn snapshot_watch(entry: &WatchEntry) -> WatchState {
    entry.state.lock().expect("watch state").clone()
}

fn run_watch<R: tauri::Runtime>(app: &AppHandle<R>, entry: &Arc<WatchEntry>, manager: &str, log_path: &std::path::Path) {
    let state = app.state::<AppState>();
    let jobs = app.state::<RepoJobs>();
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(log_path).ok();
    let app2 = app.clone();
    let entry2 = entry.clone();
    let mut sink = move |line: &str| {
        use std::io::Write;
        if let Some(f) = file.as_mut() {
            let _ = writeln!(f, "{line}");
        }
        {
            let mut ring = entry2.ring.lock().expect("watch ring");
            if ring.len() == WATCH_RING_CAP {
                ring.pop_front();
            }
            ring.push_back(line.to_string());
        }
        let _ = app2.emit(&watch_output_event(&entry2.id), line.to_string());
    };
    let script = snapshot_watch(entry).script;
    let outcome = (|| -> Result<repo::StepResult> {
        let env = shell_env(&state, &jobs, false)?;
        let pm = devtools::resolve_package_manager(&env, manager)?;
        let dest = {
            let st = snapshot_watch(entry);
            let site = site_of(&state, &st.site_id)?;
            repo::asset_dest(std::path::Path::new(&site.path), &st.kind, &st.dir_name)?
        };
        repo::node_run_script(
            state.platform.supervisor(),
            &pm.path,
            &dest,
            &script,
            &env,
            &entry.cancel,
            &mut sink,
        )
    })();
    {
        let mut st = entry.state.lock().expect("watch state");
        st.status = "exited".into();
        st.exit = match &outcome {
            Ok(r) => r.exit,
            Err(_) => None,
        };
    }
    if let Err(e) = outcome {
        sink(&format!("✕ {e}"));
    } else if entry.cancel.is_cancelled() {
        sink("watcher stopped");
    } else {
        sink("watcher exited");
    }
    let _ = app.emit(&watch_state_event(&entry.id), snapshot_watch(entry));
    emit_watch_global(app);
}

/// Stop a watcher (kills its whole process group) and drop it from the list.
#[tauri::command]
pub async fn repo_watch_stop<R: tauri::Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    watches: State<'_, RepoWatches>,
    id: String,
) -> Result<()> {
    let entry = watches
        .watches
        .lock()
        .expect("watches lock")
        .get(&id)
        .cloned()
        .ok_or_else(|| Error::Other(format!("no watcher {id}")))?;
    entry.cancel.cancel(state.platform.supervisor());
    watches.watches.lock().expect("watches lock").remove(&id);
    emit_watch_global(&app);
    Ok(())
}

/// Watch list — all, or one site+kind's (panel + footer chip seed).
#[tauri::command]
pub async fn repo_watches(
    watches: State<'_, RepoWatches>,
    site_id: Option<String>,
    kind: Option<String>,
) -> Result<Vec<WatchState>> {
    let mut out: Vec<WatchState> = watches
        .watches
        .lock()
        .expect("watches lock")
        .values()
        .map(|w| snapshot_watch(w))
        .filter(|w| site_id.as_deref().map_or(true, |s| w.site_id == s))
        .filter(|w| kind.as_deref().map_or(true, |k| w.kind == k))
        .collect();
    out.sort_by(|a, b| a.dir_name.cmp(&b.dir_name));
    Ok(out)
}

/// A watcher's ring-buffer backlog (seeds the pane on remount — in-memory,
/// no file read).
#[tauri::command]
pub async fn repo_watch_log(watches: State<'_, RepoWatches>, id: String) -> Result<Vec<String>> {
    let entry = watches
        .watches
        .lock()
        .expect("watches lock")
        .get(&id)
        .cloned()
        .ok_or_else(|| Error::Other(format!("no watcher {id}")))?;
    let ring = entry.ring.lock().expect("watch ring");
    Ok(ring.iter().cloned().collect())
}

/// A checkout's live state for the RepoPanel + the delete-safety confirm.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetStatusResult {
    #[serde(flatten)]
    pub status: repo::GitStatus,
    /// Where a detached HEAD sits (exact tag name, else short commit id).
    /// None unless detached.
    pub detached_at: Option<String>,
    /// `origin` remote URL, when set.
    pub remote: Option<String>,
    /// What deleting this checkout destroys — the confirm shows it verbatim.
    /// None = clean and provably pushed.
    pub loss_warning: Option<String>,
    /// Log key of the last add-job for this dir, when the file exists.
    pub log_key: Option<String>,
    /// For symlinked dirs: where the link points (the user's real checkout).
    pub link_target: Option<String>,
}

/// Live git status for one managed (or about-to-be-adopted) asset dir.
/// Local + fast, runs no repo code.
#[tauri::command]
pub async fn repo_asset_status<R: tauri::Runtime>(
    app: AppHandle<R>,
    site_id: String,
    kind: String,
    dir_name: String,
) -> Result<AssetStatusResult> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let jobs = app.state::<RepoJobs>();
        let site = site_of(&state, &site_id)?;
        let dir = repo::asset_dest(std::path::Path::new(&site.path), &kind, &dir_name)?;
        if !dir.join(".git").exists() {
            return Err(Error::Other(format!(
                "wp-content/{kind}s/{dir_name} is not a git checkout (no .git)."
            )));
        }
        let env = shell_env(&state, &jobs, false)?;
        let git = devtools::resolve_git(state.platform.as_ref(), &env)?;
        let status = repo::read_git_status(state.platform.supervisor(), &git.path, &env, &dir)?;
        let detached_at = status
            .detached
            .then(|| repo::read_detached_at(state.platform.supervisor(), &git.path, &env, &dir))
            .flatten();
        let remote = repo::read_remote_url(state.platform.supervisor(), &git.path, &env, &dir);
        let loss_warning = repo::loss_warning(&status);
        let log_key = format!("repo-{}-{}.log", site.domain, dir_name);
        let log_key = state
            .platform
            .paths()
            .log_dir()
            .ok()
            .filter(|d| d.join(&log_key).is_file())
            .map(|_| log_key);
        let link_target = std::fs::read_link(&dir).ok().map(|t| t.display().to_string());
        Ok(AssetStatusResult { status, detached_at, remote, loss_warning, log_key, link_target })
    })
    .await
    .map_err(|e| Error::Other(format!("status task failed: {e}")))?
}

/// wp-content dirs that look like git checkouts but have no provenance row —
/// the quiet "git?" adopt chips.
#[tauri::command]
pub async fn repo_unmanaged(
    state: State<'_, AppState>,
    site_id: String,
    kind: String,
) -> Result<Vec<repo::UnmanagedRepo>> {
    let site = site_of(&state, &site_id)?;
    let content = repo::asset_dest(std::path::Path::new(&site.path), &kind, "probe")?
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| Error::Other("no content dir".into()))?;
    let known: Vec<String> = {
        let conn = state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))?;
        store::get_git_assets(&conn, &site_id)?
            .into_iter()
            .filter(|a| a.kind == kind)
            .map(|a| a.dir_name)
            .collect()
    };
    Ok(repo::scan_unmanaged(&content, &known))
}

/// Adopt a manually-cloned (or manually-linked) checkout: record provenance
/// (origin remote + current branch) — metadata only, nothing on disk changes.
#[tauri::command]
pub async fn repo_adopt<R: tauri::Runtime>(
    app: AppHandle<R>,
    site_id: String,
    kind: String,
    dir_name: String,
) -> Result<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let jobs = app.state::<RepoJobs>();
        let site = site_of(&state, &site_id)?;
        let dir_name = repo::validate_dir_name(&dir_name)?;
        let dir = repo::asset_dest(std::path::Path::new(&site.path), &kind, &dir_name)?;
        if !dir.join(".git").exists() {
            return Err(Error::Other(format!(
                "wp-content/{kind}s/{dir_name} is not a git checkout (no .git)."
            )));
        }
        let linked = std::fs::symlink_metadata(&dir)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false);
        let env = shell_env(&state, &jobs, false)?;
        let git = devtools::resolve_git(state.platform.as_ref(), &env)?;
        let remote = repo::read_remote_url(state.platform.supervisor(), &git.path, &env, &dir).unwrap_or_default();
        let branch = repo::read_git_status(state.platform.supervisor(), &git.path, &env, &dir).ok().and_then(|s| s.branch);
        let conn = state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))?;
        store::upsert_git_asset(
            &conn,
            &site_id,
            &kind,
            &dir_name,
            &remote,
            branch.as_deref(),
            if linked { "linked" } else { "adopted" },
        )
    })
    .await
    .map_err(|e| Error::Other(format!("adopt task failed: {e}")))?
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub name: String,
    pub ok: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub error: Option<String>,
}

/// Link-folder result: what landed + what detection saw (the UI surfaces a
/// mismatch/no-header warning).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoLinkResult {
    pub dir_name: String,
    pub is_git: bool,
    pub wp: repo::WpHeader,
}

/// Symlink an EXISTING local folder into wp-content (source == "linked").
/// The folder stays where it is; deleting the asset later removes ONLY the
/// link (the wp_*_delete interception guarantees that on fs truth).
#[tauri::command]
pub async fn repo_link<R: tauri::Runtime>(
    app: AppHandle<R>,
    site_id: String,
    kind: String,
    dir_name: Option<String>,
    target: String,
) -> Result<RepoLinkResult> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let jobs = app.state::<RepoJobs>();
        let site = site_of(&state, &site_id)?;
        let docroot = std::path::PathBuf::from(&site.path);
        let fallback = std::path::Path::new(&target)
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        let name = repo::validate_dir_name(&dir_name.unwrap_or(fallback))?;
        let dest = repo::asset_dest(&docroot, &kind, &name)?;
        let canonical = repo::validate_link_target(&docroot, &dest, std::path::Path::new(&target))?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        state.platform.shell().symlink_dir(&canonical, &dest)?;
        let inspection = repo::inspect_repo(&dest);
        let is_git = dest.join(".git").exists();
        if is_git {
            // Best-effort git metadata — a missing git tool must not fail the link.
            let (remote, branch) = match shell_env(&state, &jobs, false) {
                Ok(env) => match devtools::resolve_git(state.platform.as_ref(), &env) {
                    Ok(git) => (
                        repo::read_remote_url(state.platform.supervisor(), &git.path, &env, &dest).unwrap_or_default(),
                        repo::read_git_status(state.platform.supervisor(), &git.path, &env, &dest).ok().and_then(|s| s.branch),
                    ),
                    Err(_) => (String::new(), None),
                },
                Err(_) => (String::new(), None),
            };
            let conn = state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))?;
            store::upsert_git_asset(&conn, &site_id, &kind, &name, &remote, branch.as_deref(), "linked")?;
        }
        Ok(RepoLinkResult { dir_name: name, is_git, wp: inspection.wp })
    })
    .await
    .map_err(|e| Error::Other(format!("link task failed: {e}")))?
}

/// git + node availability for the Git add panel (composer is always the
/// bundled phar — not listed). `refresh` re-resolves the shell env snapshot.
#[tauri::command]
pub async fn repo_tools<R: tauri::Runtime>(app: AppHandle<R>, refresh: bool) -> Result<Vec<ToolStatus>> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let jobs = app.state::<RepoJobs>();
        let env = shell_env(&state, &jobs, refresh)?;
        let git = devtools::resolve_git(state.platform.as_ref(), &env);
        let node = devtools::resolve_node(&env);
        Ok(vec![tool_status("git", git), tool_status("node", node)])
    })
    .await
    .map_err(|e| Error::Other(format!("tools task failed: {e}")))?
}

fn tool_status(name: &str, resolved: Result<devtools::ToolInfo>) -> ToolStatus {
    match resolved {
        Ok(t) => ToolStatus {
            name: name.into(),
            ok: true,
            version: t.version,
            path: Some(t.path.display().to_string()),
            error: None,
        },
        Err(e) => ToolStatus {
            name: name.into(),
            ok: false,
            version: None,
            path: None,
            error: Some(e.to_string()),
        },
    }
}

/// App-exit hook: kill every live job's process group. Deliberately the
/// OPPOSITE of services-outlive-the-app — an install/build is an interactive
/// action, not infrastructure; a half-done install heals by re-running.
pub fn cancel_all_on_exit<R: tauri::Runtime>(app: &AppHandle<R>) {
    let (Some(jobs), Some(state)) = (app.try_state::<RepoJobs>(), app.try_state::<AppState>())
    else {
        return;
    };
    for entry in jobs.jobs.lock().expect("jobs lock").values() {
        entry.cancel.cancel(state.platform.supervisor());
    }
    if let Some(watches) = app.try_state::<RepoWatches>() {
        for w in watches.watches.lock().expect("watches lock").values() {
            w.cancel.cancel(state.platform.supervisor());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(key: &str, status: &str) -> RepoStepState {
        RepoStepState { key: key.into(), label: key.into(), status: status.into(), error: None }
    }

    #[test]
    fn run_all_collects_only_pending_dep_steps_in_offer_order() {
        let steps = vec![
            st("check", "ok"),
            st("composer", "pending"),
            st("install", "ok"),      // already ran individually — not re-run
            st("build", "pending"),
            st("pull", "ok"),         // op steps never collected
        ];
        assert_eq!(offered_pending(&steps), vec!["composer", "build"]);
        assert!(offered_pending(&[st("check", "ok")]).is_empty());
    }

    #[test]
    fn skip_marks_only_pending_dep_steps_never_op_or_terminal_ones() {
        let mut steps = vec![
            st("checkout", "ok"),
            st("composer", "ok"),      // completed before the failure — stays ok
            st("install", "failed"),   // the failure itself — stays failed
            st("build", "pending"),    // never ran → skipped
            st("check", "pending"),    // op-ish step key — untouched
        ];
        skip_pending_offered(&mut steps);
        let by_key = |k: &str| steps.iter().find(|s| s.key == k).unwrap().status.clone();
        assert_eq!(by_key("composer"), "ok");
        assert_eq!(by_key("install"), "failed");
        assert_eq!(by_key("build"), "skipped");
        assert_eq!(by_key("check"), "pending");
        assert_eq!(by_key("checkout"), "ok");
    }
}

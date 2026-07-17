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
    /// "pending" | "running" | "ok" | "failed" | "cancelled".
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

fn emit_state(app: &AppHandle, entry: &JobEntry) {
    let _ = app.emit(&state_event(&entry.id), snapshot(entry));
}

/// Mutate one step's status (+ optional error), recompute `finished_ok`, emit.
fn set_step(app: &AppHandle, entry: &JobEntry, key: &str, status: &str, error: Option<String>) {
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
fn make_sink(app: AppHandle, entry: Arc<JobEntry>) -> impl FnMut(&str) {
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
pub async fn repo_probe(app: AppHandle, url: String) -> Result<RepoProbeResult> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let jobs = app.state::<RepoJobs>();
        let src = repo::parse_source(&url)?;
        let env = shell_env(&state, &jobs, false)?;
        let git = devtools::resolve_git(state.platform.as_ref(), &env)?;
        let refs = repo::probe_remote(&git.path, &env, &src.url)?;
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
pub async fn repo_add(
    app: AppHandle,
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
        cancel: repo::CancelToken::new(),
        step_running: AtomicBool::new(true), // the clone worker below
        state: Mutex::new(RepoJobState {
            id: id.clone(),
            site_id,
            kind,
            dir_name,
            url: src.url,
            git_ref,
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

fn run_clone_and_detect(app: &AppHandle, entry: &Arc<JobEntry>) {
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
pub async fn repo_run_step(
    app: AppHandle,
    state: State<'_, AppState>,
    jobs: State<'_, RepoJobs>,
    job_id: String,
    step_key: String,
) -> Result<()> {
    let entry = entry_of(&jobs, &job_id)?;
    let offered = snapshot(&entry).steps.iter().any(|s| s.key == step_key);
    if !offered || matches!(step_key.as_str(), "clone" | "detect") {
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

fn run_one_step(
    app: &AppHandle,
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
        Ok(()) => set_step(app, entry, step_key, "ok", None),
    }
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

/// A checkout's live state for the RepoPanel + the delete-safety confirm.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetStatusResult {
    #[serde(flatten)]
    pub status: repo::GitStatus,
    /// `origin` remote URL, when set.
    pub remote: Option<String>,
    /// What deleting this checkout destroys — the confirm shows it verbatim.
    /// None = clean and provably pushed.
    pub loss_warning: Option<String>,
    /// Log key of the last add-job for this dir, when the file exists.
    pub log_key: Option<String>,
}

/// Live git status for one managed (or about-to-be-adopted) asset dir.
/// Local + fast, runs no repo code.
#[tauri::command]
pub async fn repo_asset_status(
    app: AppHandle,
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
        let status = repo::read_git_status(&git.path, &env, &dir)?;
        let remote = repo::read_remote_url(&git.path, &env, &dir);
        let loss_warning = repo::loss_warning(&status);
        let log_key = format!("repo-{}-{}.log", site.domain, dir_name);
        let log_key = state
            .platform
            .paths()
            .log_dir()
            .ok()
            .filter(|d| d.join(&log_key).is_file())
            .map(|_| log_key);
        Ok(AssetStatusResult { status, remote, loss_warning, log_key })
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
pub async fn repo_adopt(
    app: AppHandle,
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
        let remote = repo::read_remote_url(&git.path, &env, &dir).unwrap_or_default();
        let branch = repo::read_git_status(&git.path, &env, &dir).ok().and_then(|s| s.branch);
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

/// git + node availability for the Git add panel (composer is always the
/// bundled phar — not listed). `refresh` re-resolves the shell env snapshot.
#[tauri::command]
pub async fn repo_tools(app: AppHandle, refresh: bool) -> Result<Vec<ToolStatus>> {
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
pub fn cancel_all_on_exit(app: &AppHandle) {
    let (Some(jobs), Some(state)) = (app.try_state::<RepoJobs>(), app.try_state::<AppState>())
    else {
        return;
    };
    for entry in jobs.jobs.lock().expect("jobs lock").values() {
        entry.cancel.cancel(state.platform.supervisor());
    }
}

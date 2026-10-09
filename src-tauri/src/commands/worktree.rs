//! Worktree child sites — the IPC edge of `docs/PLAN-git-worktrees.md`.
//!
//! Thin, like every command module: validate the request against the parent
//! row, derive the domain (`core::worktree::derive_domain`), and hand a
//! `NewSite` to the ONE provisioning path (`site_provision::start_with`), with
//! the worktree relation recorded under the same lock as the row. The phases
//! that copy the parent and add the worktree live in `site_provision` beside
//! every other phase.

use crate::commands::site_provision::{self, ProvisionJobs, SiteProvisionState};
use crate::core::{self, worktree};
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::models::{MultisiteMode, NewSite, Site, SiteType, WorktreeShape};
use std::path::PathBuf;
use crate::state::store;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

/// What the New-worktree dialog (and `rex worktree add`, and the MCP tool)
/// asks for. With `asset_kind` + `asset_dir`: Shape A — a plugin or theme
/// checkout inside a WordPress site (owner, 9 Oct 2026: that shape first).
/// Without them: Shape B — the site's OWN checkout (W10).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeRequest {
    pub parent_id: String,
    #[serde(default)]
    pub asset_kind: Option<worktree::AssetKind>,
    /// The plugin/theme folder name under the parent's content dir.
    #[serde(default)]
    pub asset_dir: Option<String>,
    /// The branch to check out — existing, or the NEW one when `base` is set.
    pub branch: String,
    /// Make `branch` new, from this ref.
    #[serde(default)]
    pub base: Option<String>,
    /// Override the derived domain.
    #[serde(default)]
    pub domain: Option<String>,
    /// Shape A: leave `uploads/` out of the copy (media then 404s on the child).
    #[serde(default)]
    pub skip_uploads: bool,
}

impl WorktreeRequest {
    /// `Some` for Shape A; `None` for Shape B. Half of the pair is refused.
    fn asset(&self) -> Result<Option<(worktree::AssetKind, &str)>> {
        match (self.asset_kind, self.asset_dir.as_deref()) {
            (Some(k), Some(d)) => Ok(Some((k, d))),
            (None, None) => Ok(None),
            _ => Err(Error::Other("a plugin/theme worktree needs both its kind and its folder".into())),
        }
    }
}

/// Validate `req` against the database and derive the child's `NewSite`.
/// Pure of side effects; every refusal happens here, before anything exists.
/// A Shape B child's `path` is left empty — `start` fills it with the folder
/// it makes the worktree in.
pub fn plan_child(
    conn: &rusqlite::Connection,
    req: &WorktreeRequest,
) -> Result<(NewSite, core::worktree::ChildDomain, crate::state::models::Site)> {
    let parent = store::get_site(conn, &req.parent_id)?
        .ok_or_else(|| Error::Other(format!("no site {}", req.parent_id)))?;
    let asset = req.asset()?;
    if !parent.provisioned {
        return Err(Error::Other(format!(
            "{} has not finished setting up — Retry it first.",
            parent.domain
        )));
    }
    if store::get_site_worktree(conn, &parent.id)?.is_some() {
        return Err(Error::Other(format!(
            "{} is itself a worktree — make the new one from its parent instead.",
            parent.domain
        )));
    }
    if parent.multisite == MultisiteMode::Subdomain {
        return Err(Error::Other(format!(
            "{} is a subdomain multisite network — worktrees of a subdomain network are not \
             supported yet (each sub-site needs a name under the copy's own domain).",
            parent.domain
        )));
    }
    match asset {
        Some((kind, dir)) => {
            if parent.site_type != SiteType::Wordpress {
                return Err(Error::Other(format!(
                    "{} is not a WordPress site — a plugin or theme worktree needs one.",
                    parent.domain
                )));
            }
            if parent.content_dir_rel() != "wp-content" {
                return Err(Error::Other(format!(
                    "{} keeps its content in `{}` (a Composer-managed WordPress such as Bedrock) — \
                     plugin worktrees of that layout are not supported yet; a worktree of the \
                     whole repository is.",
                    parent.domain,
                    parent.content_dir_rel()
                )));
            }
            let rel = worktree::asset_rel(parent.content_dir_rel(), kind, dir)?;
            let path = parent.served_root().join(&rel);
            if !path.join(".git").exists() {
                return Err(Error::Other(format!(
                    "{} is not a git checkout — a worktree needs the repository the {} lives in.",
                    path.display(),
                    kind.as_db()
                )));
            }
        }
        None => {
            if !std::path::Path::new(&parent.path).join(".git").exists() {
                return Err(Error::Other(format!(
                    "{}'s folder is not a git checkout — a site worktree needs the site's own \
                     repository (for a plugin or theme in git, make a plugin worktree).",
                    parent.domain
                )));
            }
        }
    }
    core::repo::validate_ref(&req.branch)?;
    if let Some(b) = &req.base {
        core::repo::validate_ref(b)?;
    }
    let child = match &req.domain {
        Some(d) => {
            let d = d.trim().to_ascii_lowercase();
            core::sites::validate_domain(&d)?;
            if worktree::domain_taken(conn, &d)? {
                return Err(Error::Other(format!("{d} is already taken by another site.")));
            }
            worktree::ChildDomain { domain: d, fallback: None }
        }
        None => worktree::derive_domain(
            &parent.domain,
            parent.multisite == MultisiteMode::Subdomain,
            &req.branch,
            &|d| worktree::domain_taken(conn, d).unwrap_or(true),
        )?,
    };
    let new = NewSite {
        name: format!("{} · {}", parent.name, req.branch),
        domain: child.domain.clone(),
        site_type: parent.site_type,
        php_version: parent.php_version.clone(),
        web_server: parent.web_server,
        path: String::new(),
        db_engine: parent.db_engine,
        git_url: String::new(),
        git_ref: None,
        git_migrate: false,
        git_build_assets: false,
        starter_db: false,
    };
    Ok((new, child, parent))
}

fn spec_of(req: &WorktreeRequest) -> worktree::AddSpec {
    match &req.base {
        Some(base) => worktree::AddSpec::New { branch: req.branch.clone(), base: base.clone() },
        None => worktree::AddSpec::Existing(req.branch.clone()),
    }
}

/// Start a worktree child's provisioning job.
///
/// Shape B adds the worktree HERE, before the row: the child is a LINKED site
/// (its folder is git's, never rexenv's to `rm` — ledger #814), and a linked
/// row needs its folder to exist. If the row then cannot be made, the brand-new
/// worktree is removed again (`--force` is safe there: nothing but git's own
/// checkout is in it yet).
pub fn start<R: tauri::Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    jobs: &ProvisionJobs,
    req: WorktreeRequest,
) -> Result<SiteProvisionState> {
    let (mut new, _child, parent) = {
        let conn = state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))?;
        plan_child(&conn, &req)?
    };
    let content_rel = parent.content_dir_rel().to_string();
    let asset = req.asset()?.map(|(k, d)| (k, d.to_string()));
    let rel = match &asset {
        Some((kind, dir)) => Some(worktree::asset_rel(&content_rel, *kind, dir)?),
        None => None,
    };
    // Shape B: the worktree first.
    let mut made: Option<(PathBuf, PathBuf)> = None; // (repo dir, worktree)
    if asset.is_none() {
        let folder = {
            let conn = state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))?;
            worktree::site_worktrees_dir(&core::sites::sites_dir(&conn, state.platform.as_ref())?)?
                .join(&new.domain)
        };
        if let Some(p) = folder.parent() {
            std::fs::create_dir_all(p)?;
        }
        let repo_dir = PathBuf::from(&parent.path);
        let env = state.platform.shell().login_shell_env()?;
        let git = core::devtools::resolve_git(state.platform.as_ref(), &env)?;
        worktree::require_worktree_git(git.version.as_deref())?;
        worktree::add(
            state.platform.supervisor(),
            &git.path,
            &env,
            &repo_dir,
            &folder,
            &spec_of(&req),
            &core::repo::CancelToken::new(),
            &mut |l| log::info!("worktree: {l}"),
        )?;
        new.path = folder.to_string_lossy().into_owned();
        made = Some((repo_dir, folder));
    }
    let worktree_path = |site: &crate::state::models::Site| match &rel {
        Some(r) => site.served_root().join(r).to_string_lossy().into_owned(),
        None => site.path.clone(),
    };
    let record = |conn: &rusqlite::Connection, site: &crate::state::models::Site| -> Result<()> {
        // The child serves the parent's layout: same rewrite mode, same content dir.
        store::set_site_multisite(conn, &site.id, parent.multisite.as_db())?;
        store::set_site_content_dir(conn, &site.id, &content_rel)?;
        store::insert_site_worktree(
            conn,
            &store::NewWorktree {
                site_id: &site.id,
                parent_id: &parent.id,
                shape: if asset.is_some() { WorktreeShape::Asset } else { WorktreeShape::Site },
                worktree_path: &worktree_path(site),
                adopted: false,
                asset_kind: asset.as_ref().map(|(k, _)| k.as_db()),
                asset_dir: asset.as_ref().map(|(_, d)| d.as_str()),
                branch: &req.branch,
                base: req.base.as_deref(),
                skip_uploads: req.skip_uploads,
            },
        )
    };
    let started = site_provision::start_with(
        app,
        state,
        jobs,
        new,
        None,
        None,
        core::sites::Ownership::User,
        Some(&record),
    );
    if started.is_err() {
        if let Some((repo_dir, folder)) = &made {
            if let Ok(env) = state.platform.shell().login_shell_env() {
                if let Ok(git) = core::devtools::resolve_git(state.platform.as_ref(), &env) {
                    let _ = worktree::remove(
                        state.platform.supervisor(),
                        &git.path,
                        &env,
                        repo_dir,
                        folder,
                        true,
                        &core::repo::CancelToken::new(),
                        &mut |_| {},
                    );
                }
            }
        }
    }
    started
}

/// `worktree_create` — the IPC command.
#[tauri::command]
pub async fn worktree_create<R: tauri::Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    jobs: State<'_, ProvisionJobs>,
    request: WorktreeRequest,
) -> Result<SiteProvisionState> {
    off_the_runtime(|| start(&app, &state, &jobs, request))
}

/// Run `f` — which runs git and may take a while — without holding an async
/// worker: on the multi-threaded runtime the app runs, `block_in_place` hands the
/// worker's other tasks elsewhere for the duration (review, 10 Oct 2026: a big
/// checkout or removal held a tokio worker for its whole length). On a
/// current-thread runtime (`#[tokio::test]`), where `block_in_place` would panic,
/// `f` just runs.
pub(crate) fn off_the_runtime<T>(f: impl FnOnce() -> T) -> T {
    match tokio::runtime::Handle::try_current().map(|h| h.runtime_flavor()) {
        Ok(tokio::runtime::RuntimeFlavor::MultiThread) => tokio::task::block_in_place(f),
        _ => f(),
    }
}

/// Release a worktree child's checkout through git, before its site is
/// deleted — the ONE path that deletes a worktree (ledger #814: no
/// `remove_dir_all` may). Called by `commands::sites::delete_site_owned`
/// before anything destructive; a refusal (uncommitted work) therefore leaves
/// the site, its database and its folder exactly as they were.
///
/// No-op for a site that is not a worktree child, for an ADOPTED one (§2.7:
/// the tool that made it owns its lifecycle — rexenv only stops serving it),
/// and for one whose worktree was never added or is already gone (a job that
/// failed before the `worktree` phase). `force` is the user's "remove anyway".
/// The BRANCH is never deleted.
pub(crate) fn release_for_delete(state: &AppState, site: &Site, force: bool) -> Result<()> {
    let (wt, parent) = {
        let conn = state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))?;
        let Some(wt) = store::get_site_worktree(&conn, &site.id)? else { return Ok(()) };
        let parent = store::get_site(&conn, &wt.parent_id)?;
        (wt, parent)
    };
    if wt.adopted {
        return Ok(());
    }
    let path = PathBuf::from(&wt.worktree_path);
    if !path.join(".git").is_file() {
        return Ok(());
    }
    let parent = parent.ok_or_else(|| {
        Error::Other(format!("{}'s parent site is gone — remove {} with git by hand", site.domain, path.display()))
    })?;
    let repo_dir = match (wt.shape, wt.asset_kind.as_deref().and_then(worktree::AssetKind::parse_db), wt.asset_dir.as_deref()) {
        (Some(WorktreeShape::Asset), Some(kind), Some(dir)) => {
            parent.served_root().join(worktree::asset_rel(parent.content_dir_rel(), kind, dir)?)
        }
        (Some(WorktreeShape::Site), _, _) => PathBuf::from(&parent.path),
        _ => return Err(Error::Other(format!("{}'s worktree record is incomplete", site.domain))),
    };
    let env = state.platform.shell().login_shell_env()?;
    let git = core::devtools::resolve_git(state.platform.as_ref(), &env)?;
    worktree::remove(
        state.platform.supervisor(),
        &git.path,
        &env,
        &repo_dir,
        &path,
        force,
        &core::repo::CancelToken::new(),
        &mut |l| log::info!("worktree: {l}"),
    )
}

/// `worktree_remove` — delete a worktree child: its checkout through git
/// (refused while it holds uncommitted work, unless `force`), then the site
/// exactly as Delete does.
#[tauri::command]
pub async fn worktree_remove(
    state: State<'_, AppState>,
    tunnels: State<'_, crate::commands::tunnels::Tunnels>,
    id: String,
    force: bool,
) -> Result<bool> {
    let site = {
        let conn = state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))?;
        store::get_site(&conn, &id)?
    };
    let Some(site) = site else { return Ok(false) };
    off_the_runtime(|| release_for_delete(&state, &site, force))?;
    crate::commands::sites::delete_site_owned(&state, &tunnels, id).await
}

/// The New-worktree dialog's live preview: the domain this request would get,
/// and — when it is the fallback shape — the one sentence saying why.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreePreview {
    pub domain: String,
    pub fallback: Option<String>,
}

/// `worktree_preview` — every refusal `worktree_create` would give, without
/// creating anything.
#[tauri::command]
pub async fn worktree_preview(state: State<'_, AppState>, request: WorktreeRequest) -> Result<WorktreePreview> {
    let conn = state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))?;
    let (_, child, _) = plan_child(&conn, &request)?;
    Ok(WorktreePreview { domain: child.domain, fallback: child.fallback.map(|f| f.sentence().to_string()) })
}

/// One worktree child as the UI shows it. `branch` and `uncommitted` are
/// git's answers, read NOW — never the recorded request (`asked_branch`),
/// which is what the user asked for when it was made.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeView {
    pub site_id: String,
    pub domain: String,
    pub parent_id: String,
    pub parent_domain: String,
    pub asset_kind: Option<String>,
    pub asset_dir: Option<String>,
    pub asked_branch: String,
    /// `None` = detached, or the worktree is not there (yet, or any more).
    pub branch: Option<String>,
    /// Changed + untracked files; `None` when git could not be asked.
    pub uncommitted: Option<u32>,
    pub present: bool,
    pub provisioned: bool,
    pub adopted: bool,
}

fn view_of<R: tauri::Runtime>(app: &AppHandle<R>, w: &crate::state::models::SiteWorktree) -> Result<Option<WorktreeView>> {
    let state = app.state::<AppState>();
    let (site, parent) = {
        let conn = state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))?;
        (store::get_site(&conn, &w.site_id)?, store::get_site(&conn, &w.parent_id)?)
    };
    let (Some(site), Some(parent)) = (site, parent) else { return Ok(None) };
    let path = PathBuf::from(&w.worktree_path);
    let present = path.join(".git").is_file();
    let status = if present {
        crate::commands::repo::shell_env(&state, &app.state::<crate::commands::repo::RepoJobs>(), false)
            .and_then(|env| {
                let git = core::devtools::resolve_git(state.platform.as_ref(), &env)?;
                core::repo::read_git_status(state.platform.supervisor(), &git.path, &env, &path)
            })
            .ok()
    } else {
        None
    };
    Ok(Some(WorktreeView {
        site_id: site.id,
        domain: site.domain,
        parent_id: parent.id,
        parent_domain: parent.domain,
        asset_kind: w.asset_kind.clone(),
        asset_dir: w.asset_dir.clone(),
        asked_branch: w.branch.clone(),
        branch: status.as_ref().and_then(|s| s.branch.clone()),
        uncommitted: status.as_ref().map(|s| s.changed + s.untracked),
        present,
        provisioned: site.provisioned,
        adopted: w.adopted,
    }))
}

/// `worktree_children` — every worktree child of a site, with git's live answers.
#[tauri::command]
pub async fn worktree_children<R: tauri::Runtime>(app: AppHandle<R>, parent_id: String) -> Result<Vec<WorktreeView>> {
    tauri::async_runtime::spawn_blocking(move || {
        let rows = {
            let state = app.state::<AppState>();
            let conn = state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))?;
            store::worktree_children(&conn, &parent_id)?
        };
        let mut out = Vec::new();
        for w in &rows {
            if let Some(v) = view_of(&app, w)? {
                out.push(v);
            }
        }
        Ok(out)
    })
    .await
    .map_err(|e| Error::Other(format!("worktree worker died: {e}")))?
}

/// `worktree_of` — the banner on a child's page: which site, which branch.
#[tauri::command]
pub async fn worktree_of<R: tauri::Runtime>(app: AppHandle<R>, site_id: String) -> Result<Option<WorktreeView>> {
    tauri::async_runtime::spawn_blocking(move || {
        let row = {
            let state = app.state::<AppState>();
            let conn = state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))?;
            store::get_site_worktree(&conn, &site_id)?
        };
        match row {
            Some(w) => view_of(&app, &w),
            None => Ok(None),
        }
    })
    .await
    .map_err(|e| Error::Other(format!("worktree worker died: {e}")))?
}

/// `worktree_relations` — child id → (parent id, asked branch) for every child,
/// for the Sites list's grouping. Rows only: no git, so a list render never
/// waits on N git processes.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeRelation {
    pub site_id: String,
    pub parent_id: String,
    pub asked_branch: String,
}

#[tauri::command]
pub async fn worktree_relations(state: State<'_, AppState>) -> Result<Vec<WorktreeRelation>> {
    let conn = state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))?;
    Ok(store::all_site_worktrees(&conn)?
        .into_iter()
        .map(|w| WorktreeRelation { site_id: w.site_id, parent_id: w.parent_id, asked_branch: w.branch })
        .collect())
}

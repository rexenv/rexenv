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
use crate::state::models::{MultisiteMode, NewSite, SiteType, WorktreeShape};
use crate::state::store;
use serde::Deserialize;
use tauri::{AppHandle, State};

/// What the New-worktree dialog (and `rex worktree add`, and the MCP action)
/// asks for. Shape A only for now — a plugin or theme checkout inside a
/// WordPress site (owner, 9 Oct 2026: that shape first).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeRequest {
    pub parent_id: String,
    pub asset_kind: worktree::AssetKind,
    /// The plugin/theme folder name under the parent's content dir.
    pub asset_dir: String,
    /// The branch to check out — existing, or the NEW one when `base` is set.
    pub branch: String,
    /// Make `branch` new, from this ref.
    #[serde(default)]
    pub base: Option<String>,
    /// Override the derived domain.
    #[serde(default)]
    pub domain: Option<String>,
    /// Leave `uploads/` out of the copy (media then 404s on the child).
    #[serde(default)]
    pub skip_uploads: bool,
}

/// Validate `req` against the database and derive the child's `NewSite`.
/// Pure of side effects; every refusal happens here, before anything exists.
pub fn plan_child(
    conn: &rusqlite::Connection,
    req: &WorktreeRequest,
) -> Result<(NewSite, core::worktree::ChildDomain, crate::state::models::Site)> {
    let parent = store::get_site(conn, &req.parent_id)?
        .ok_or_else(|| Error::Other(format!("no site {}", req.parent_id)))?;
    if parent.site_type != SiteType::Wordpress {
        return Err(Error::Other(format!(
            "{} is not a WordPress site — a plugin or theme worktree needs one.",
            parent.domain
        )));
    }
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
    if parent.content_dir_rel() != "wp-content" {
        return Err(Error::Other(format!(
            "{} keeps its content in `{}` (a Composer-managed WordPress such as Bedrock) — \
             worktrees of that layout are not supported yet.",
            parent.domain,
            parent.content_dir_rel()
        )));
    }
    let rel = worktree::asset_rel(parent.content_dir_rel(), req.asset_kind, &req.asset_dir)?;
    let asset = parent.served_root().join(&rel);
    if !asset.join(".git").exists() {
        return Err(Error::Other(format!(
            "{} is not a git checkout — a worktree needs the repository the {} lives in.",
            asset.display(),
            req.asset_kind.as_db()
        )));
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
        site_type: SiteType::Wordpress,
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

/// Start a worktree child's provisioning job.
pub fn start<R: tauri::Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    jobs: &ProvisionJobs,
    req: WorktreeRequest,
) -> Result<SiteProvisionState> {
    let (new, _child, parent) = {
        let conn = state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))?;
        plan_child(&conn, &req)?
    };
    let content_rel = parent.content_dir_rel().to_string();
    let rel = worktree::asset_rel(&content_rel, req.asset_kind, &req.asset_dir)?;
    let record = move |conn: &rusqlite::Connection, site: &crate::state::models::Site| -> Result<()> {
        // The child serves the parent's layout: same rewrite mode, same content dir.
        store::set_site_multisite(conn, &site.id, parent.multisite.as_db())?;
        store::set_site_content_dir(conn, &site.id, &content_rel)?;
        store::insert_site_worktree(
            conn,
            &store::NewWorktree {
                site_id: &site.id,
                parent_id: &parent.id,
                shape: WorktreeShape::Asset,
                worktree_path: &site.served_root().join(&rel).to_string_lossy(),
                adopted: false,
                asset_kind: Some(req.asset_kind.as_db()),
                asset_dir: Some(&req.asset_dir),
                branch: &req.branch,
                base: req.base.as_deref(),
                skip_uploads: req.skip_uploads,
            },
        )
    };
    site_provision::start_with(
        app,
        state,
        jobs,
        new,
        None,
        None,
        core::sites::Ownership::User,
        Some(&record),
    )
}

/// `worktree_create` — the IPC command.
#[tauri::command]
pub async fn worktree_create<R: tauri::Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    jobs: State<'_, ProvisionJobs>,
    request: WorktreeRequest,
) -> Result<SiteProvisionState> {
    start(&app, &state, &jobs, request)
}

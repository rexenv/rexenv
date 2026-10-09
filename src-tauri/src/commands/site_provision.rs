//! Streamed site provisioning — the "New Site" card's job backend. Mirrors
//! `commands::wp_install` (registry + `site-provision://state|output/<id>`
//! events + per-job log + CancelToken), driving the REAL create sequence with
//! phase-weighted honest progress (`core::sites::ProvisionProgress`).
//!
//! Honesty contract:
//! - phases are OUR step boundaries (deterministic Rust code) — zero
//!   subprocess-output parsing; wp-cli's verbatim lines stream to the log and
//!   the card's sub-detail only,
//! - the only intra-phase progress is the download Hub's REAL byte fraction
//!   during `fetch` (genuine bytes, never an estimate),
//! - pct is monotonic, ≤99 until the job settles ok, frozen on
//!   failure/cancel/timeout,
//! - CANCEL never touches anything shared: the binary prefetch runs in a
//!   DETACHED task (a Hub download another consumer may be waiting on is
//!   never aborted — cancelling a create just stops WAITING; the fetch
//!   completes into the cache), the pgid kill only ever hits wp-cli children
//!   this job spawned, and the DB-spawn/pool/reload phases (shared services)
//!   are cancel-checked at boundaries only,
//! - a job that dies mid-provision leaves the site row with `provisioned=0`
//!   (the honest "setup incomplete" badge); `site_provision_retry` re-enters
//!   the idempotent steps and re-ENSURES prepare's artifacts (docroot, cert)
//!   rather than assuming they exist.

use crate::commands::repo::{shell_env, EnvSnapshot, RepoJobs};
use crate::commands::wordpress::{composer_tools, wp_tools};
use crate::core::db::DbEngine;
use crate::core::{self, blueprints, downloads, repo, service_manager, sites, wordpress};
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::models::{Blueprint, MultisiteMode, NewSite, Site, SiteType, WebServer};
use serde::Serialize;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

pub fn state_event(id: &str) -> String {
    format!("site-provision://state/{id}")
}
pub fn output_event(id: &str) -> String {
    format!("site-provision://output/{id}")
}

#[derive(Default)]
pub struct ProvisionJobs {
    jobs: Mutex<HashMap<String, Arc<ProvisionEntry>>>,
    /// Creation order — "the most recent job" must mean MOST RECENT.
    next_seq: AtomicU64,
}

impl ProvisionJobs {
    /// Is a provision job currently RUNNING for `domain`? The database import
    /// consults this before starting: the two jobs would otherwise race on the
    /// same site's database and edge reload.
    pub(crate) fn busy_for(&self, domain: &str) -> bool {
        self.jobs
            .lock()
            .expect("provision jobs lock")
            .values()
            .any(|e| e.domain == domain && e.running.load(Ordering::SeqCst))
    }

    /// Every domain a provision job is RUNNING for, sorted — what Stop all must not cut off
    /// (`core::stack_guard::stop_refusal`).
    pub(crate) fn running_domains(&self) -> Vec<String> {
        let mut domains: Vec<String> = self
            .jobs
            .lock()
            .expect("provision jobs lock")
            .values()
            .filter(|e| e.running.load(Ordering::SeqCst))
            .map(|e| e.domain.clone())
            .collect();
        domains.sort();
        domains.dedup();
        domains
    }
}

struct ProvisionEntry {
    id: String,
    seq: u64,
    domain: String,
    /// Install options are held HERE, never in the serialized state — they
    /// carry the admin password.
    wp_opts: wordpress::InstallOptions,
    blueprint: Option<Blueprint>,
    cancel: repo::CancelToken,
    running: AtomicBool,
    /// Set by a step's outer-cap timer BEFORE it cancels (the token can't
    /// tell user-cancel from timeout apart — the reason is pre-recorded).
    timed_out: AtomicBool,
    log_path: PathBuf,
    state: Mutex<SiteProvisionState>,
}

/// One phase of the job as the card sees it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseState {
    pub key: String,
    pub label: String,
    /// "pending" | "running" | "ok" | "skipped" | "failed" | "cancelled".
    pub status: String,
}

/// The card's whole truth. `pct` follows the `ProvisionProgress` contract
/// (monotonic, ≤99 until settle-ok, frozen on any early end).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteProvisionState {
    pub id: String,
    pub domain: String,
    pub site_id: Option<String>,
    pub phases: Vec<PhaseState>,
    pub phase_cursor: usize,
    pub pct: u8,
    /// "running" | "ok" | "failed" | "cancelled" | "timed_out".
    pub status: String,
    pub summary: Option<String>,
    pub error: Option<String>,
    pub log_key: String,
    /// Uncached Hub item ids this job's fetch phase waits on — the card
    /// filters the app-wide `download-progress` snapshot to these.
    pub download_ids: Vec<String>,
    /// The site was created, but something else is answering :443 in front of
    /// our edge (a running Herd shadow-binds 127.0.0.1:443), so it will not
    /// actually load yet.
    ///
    /// A FIELD rather than a `status` value on purpose: `status` is a closed
    /// set the card reads as ok-or-failure, so a new variant would render the
    /// failure glyph and freeze the bar on a job that genuinely succeeded.
    #[serde(default)]
    pub serving_blocked: bool,
    /// WHO is answering :443, when it could be attributed. The card used to
    /// guess ("most likely Herd") while every other :443 message in the app
    /// named the real holder — one product, one voice, and a guess is not a
    /// name.
    #[serde(default)]
    pub serving_holder: Option<String>,
    /// The app to quit, when identifiable — "quit Herd" beats "quit it".
    #[serde(default)]
    pub serving_app: Option<String>,
    /// The front-end asset build did not complete, and why — in the developer's
    /// own words where the tool gave any (`node not found`, a failing build
    /// script). `None` = it was not asked for, or it worked.
    ///
    /// A FIELD beside a job that still settles `ok`, exactly like
    /// `serving_blocked`, and for the sharper version of the same reason: the
    /// build runs the DEVELOPER'S toolchain (their nvm node, the repo's own
    /// scripts), so its failure is not evidence that provisioning failed. The
    /// site is created, wired and serving; it is the unbuilt assets that need
    /// saying. Failing the whole job would park a working site behind a "setup
    /// incomplete" badge whose Retry re-runs everything to reach the one step
    /// that was never rexenv's to guarantee.
    #[serde(default)]
    pub assets_warning: Option<String>,
}

fn snapshot(entry: &ProvisionEntry) -> SiteProvisionState {
    entry.state.lock().expect("provision state lock").clone()
}

fn emit_state<R: tauri::Runtime>(app: &AppHandle<R>, entry: &ProvisionEntry) {
    let _ = app.emit(&state_event(&entry.id), snapshot(entry));
}

/// Append one line to the job log + stream it to the card.
fn append_line<R: tauri::Runtime>(app: &AppHandle<R>, entry: &ProvisionEntry, line: &str) {
    if let Ok(mut f) =
        std::fs::OpenOptions::new().create(true).append(true).open(&entry.log_path)
    {
        let _ = writeln!(f, "{line}");
    }
    let _ = app.emit(&output_event(&entry.id), line.to_string());
}


/// May a blueprint be applied to the site about to be created?
///
/// A blueprint is a WORDPRESS preset — plugins, themes, multisite mode,
/// WP_DEBUG, language — and [`phase_defs`] gives the blueprint phase only to a
/// MANAGED WordPress site. Every other combination used to be ACCEPTED and then
/// silently do nothing, which is the dishonest half: the caller believed the
/// preset applied. The dialog no longer offers the field outside that case, and
/// this is the same rule for callers that never see a dialog (CLI, MCP).
///
/// `path` is the linked-folder path from `NewSite`: non-empty means "serve this
/// folder in place", which rexenv adopts as-is and installs nothing into.
fn ensure_blueprint_applies(
    site_type: SiteType,
    path: &str,
    git_url: &str,
    blueprint_id: Option<&str>,
) -> Result<()> {
    if !blueprint_id.is_some_and(|id| !id.is_empty()) {
        return Ok(());
    }
    if !matches!(site_type, SiteType::Wordpress) {
        return Err(Error::Other(format!(
            "blueprints apply to WordPress sites only — this is a {} site",
            site_type.as_db()
        )));
    }
    if !path.trim().is_empty() {
        return Err(Error::Other(
            "this site serves an existing folder, which rexenv adopts as-is — a blueprint would \
             install into someone else's project"
                .into(),
        ));
    }
    // Same rule, other source: a cloned docroot is somebody's repository. A
    // blueprint's plugins and themes would be uncommitted files appearing in
    // their working tree, which is worse than useless — it is a diff they did
    // not write.
    if !git_url.trim().is_empty() {
        return Err(Error::Other(
            "this site is cloned from a repository, which rexenv fills from the remote — a \
             blueprint would add files nobody committed"
                .into(),
        ));
    }
    Ok(())
}

/// The job's applicable phases, in execution order. Weights renormalize over
/// exactly this set (`ProvisionProgress`) — no reserved slice can fail to fill.
///
/// A LINKED site gets prepare/fetch/serve whatever its type: we adopt the
/// folder exactly as it is and install nothing into it. That is not merely
/// polite — `configure` is only half idempotent (it skips `wp config create`
/// when wp-config.php exists, but creates the database unconditionally), so
/// running the WordPress phases over someone's existing install would leave a
/// stray empty database beside their real one.
/// Everything the phase list depends on, read off the row in ONE place.
///
/// A struct rather than a fifth positional `bool`: the list already turned on
/// type + blueprint + linked + from-git, and a call like
/// `phase_defs(ty, true, false, true, false)` is a bug waiting for a reader who
/// miscounts. Every field is named at both the call site and the test.
#[derive(Debug, Clone, Copy)]
struct PhasePlan {
    site_type: SiteType,
    has_blueprint: bool,
    /// The docroot is the user's own folder — adopted, never installed into.
    linked: bool,
    /// The docroot is filled from a repository.
    from_git: bool,
    /// `artisan migrate` was asked for (`Site::runs_migrations`).
    migrate: bool,
    /// The repo's front-end assets were asked for (`Site::builds_assets`).
    build_assets: bool,
    /// A Blank-PHP site asked for a starter database (`Site::has_starter_db`).
    starter_db: bool,
}

impl PhasePlan {
    fn of(site: &Site, has_blueprint: bool) -> Self {
        Self {
            site_type: site.site_type,
            has_blueprint,
            linked: site.docroot_managed == Some(false),
            from_git: site.git_url.is_some(),
            migrate: site.runs_migrations(),
            build_assets: site.builds_assets(),
            starter_db: site.has_starter_db(),
        }
    }
}

fn phase_defs(plan: PhasePlan) -> Vec<(&'static str, &'static str)> {
    let PhasePlan { site_type, has_blueprint, linked, from_git, migrate, build_assets, starter_db } =
        plan;
    let mut v = vec![("prepare", "preparing site (domain, certificate)"), ("fetch", "downloading binaries")];
    // The code arrives before anything can be done to it. A cloned site's
    // remaining phases are the SAME ones a created one runs — a repo is a third
    // source for the docroot, not a different kind of site.
    if from_git {
        v.push(("clone", "cloning the repository"));
    }
    if matches!(site_type, SiteType::Wordpress) && !linked {
        v.push(("db", "starting database"));
        // A CLONED WordPress site runs the same four phases as a created one,
        // and each of them is already skip-aware — `core_download` when core is
        // present, `configure` when `wp-config.php` is, `core_install` when
        // WordPress is. That is why this needed a dependency step and almost
        // nothing else: Bedrock's core arrives from Composer, so `deps` must
        // come FIRST and the skips downstream then say the truth.
        //
        // What the user gets is their CODE from the repository and a fresh,
        // empty database — the dialog says so, because "cloned my site" and
        // "cloned my site's code" are different promises.
        if from_git {
            v.push(("deps", "installing dependencies"));
        }
        v.push(("core_download", "downloading WordPress core"));
        v.push(("configure", "writing wp-config + creating database"));
        v.push(("core_install", "installing WordPress"));
        // Never for a CLONE: a blueprint's plugins and themes would land in
        // somebody's working tree as files nobody committed. The guard
        // (`ensure_blueprint_applies`) refuses that combination outright, and
        // this keeps the two from being able to disagree.
        if has_blueprint && !from_git {
            v.push(("blueprint", "applying blueprint"));
        }
    }
    // Laravel: the same shape one type over. The app comes from Composer rather
    // than a core zip, and `configure` both creates the database and points the
    // skeleton's `.env` at it — a fresh Laravel defaults to SQLite, so without
    // that step the site would run on a file the Databases screen never shows.
    if matches!(site_type, SiteType::Laravel) && !linked {
        v.push(("db", "starting database"));
        // Labels stay in the same size class as the WordPress ones: they render
        // on ONE row beside the domain and the buttons, so a long one is a
        // layout problem, not just wordy. The full commands stream to the log.
        if from_git {
            // `.env` is written BEFORE the dependencies, not after: composer's
            // `post-autoload-dump` runs `artisan package:discover`, which BOOTS
            // the app. Installing first would boot it against Laravel's own
            // defaults — including the SQLite one — every time.
            v.push(("configure", "creating database + .env"));
            v.push(("deps", "installing dependencies"));
            // The label names what this phase will ACTUALLY do. With migrations
            // turned off it still generates the app key — announcing migrations
            // that were declined is the small lie that makes a user distrust
            // the rest of the card.
            v.push(if migrate {
                ("finalize", "app key + migrations")
            } else {
                ("finalize", "generating app key")
            });
        } else {
            v.push(("app_install", "installing Laravel"));
            v.push(("configure", "creating database + .env"));
            // Migrations were a silent tail of `configure` until the cloned
            // path needed them AFTER its own dependency step. One phase, two
            // labels, one implementation — the alternative was the same
            // fifteen lines (and the reasoning comment that makes them
            // readable) copied into both branches.
            v.push(("finalize", "running migrations"));
        }
    }
    // A Blank-PHP site the user asked a database for: the same two phases
    // WordPress and Laravel run, doing less. `starter_db` is recorded NULL for a
    // linked or cloned docroot (`create_recording_ownership`), so those cases
    // cannot reach here — the row states where the question applied, and this
    // reads the row rather than re-deriving the rule a second time.
    if matches!(site_type, SiteType::Php) && starter_db {
        v.push(("db", "starting database"));
        v.push(("configure", "creating database + sample data"));
    }
    // A cloned Blank-PHP site is ANY repository — Symfony, Craft, Statamic,
    // Magento, or plain PHP. `vendor/` is gitignored in every one of them, so
    // the checkout on its own is a 500 rather than a site. Present whenever a
    // Php site was cloned and SKIPPED with a note when the repo turns out to
    // have no composer.json, because the list is fixed before the clone and
    // cannot know yet — the same shape `assets` uses.
    if matches!(site_type, SiteType::Php) && from_git {
        v.push(("deps", "installing dependencies"));
    }
    // Any CLONED site can have a package.json, Laravel or not — so this sits
    // outside the per-type blocks, after everything that could change the code
    // an asset build reads. Present only when asked for; when the repo turns
    // out to have no package.json the phase reports SKIPPED, because the list
    // is fixed before the clone and can't know yet.
    if from_git && build_assets {
        v.push(("assets", "building front-end assets"));
    }
    v.push(("serve", "starting to serve"));
    v
}

/// Build the create plan exactly the way `create_site` always has (the split
/// mirrors `commands/sites.rs` — pool vs override server, engine + wp tooling
/// only for WordPress).
fn build_plan(
    patches: &downloads::PatchMap,
    state: &AppState,
    site: &Site,
    minor: &str,
    engine: DbEngine,
    engine_version: &str,
) -> Vec<downloads::PlannedBinary> {
    let mut plan = if matches!(site.web_server, WebServer::Frankenphp) {
        downloads::plan_for_override(state.platform.as_ref(), site.web_server)
    } else {
        downloads::plan_for_pool_with(state.platform.as_ref(), minor, patches)
    };
    if matches!(site.web_server, WebServer::Apache | WebServer::Openlitespeed) {
        plan.extend(downloads::plan_for_override(state.platform.as_ref(), site.web_server));
    }
    // A LINKED WordPress site is adopted, never installed into, so `phase_defs`
    // omits its db/configure/core_install phases entirely — fetching the engine
    // and wp-cli for them would download ~600 MB of MySQL that this job will
    // never touch. On a dozen imported Valet sites that is the difference
    // between a fast import and a long first-run download.
    let linked = site.docroot_managed == Some(false);
    if matches!(site.site_type, SiteType::Wordpress) && !linked {
        plan.extend(downloads::plan_for_engine(state.platform.as_ref(), engine, engine_version));
        plan.extend(downloads::plan_for_wp_tooling_with(state.platform.as_ref(), minor, patches));
        // A cloned one may be Bedrock, whose CORE comes from Composer — and we
        // cannot know which before the clone, so the phar rides along. It is a
        // couple of megabytes beside wp-cli and the database engine.
        if site.git_url.is_some() {
            plan.extend(downloads::plan_for_composer_tooling_with(state.platform.as_ref(), minor, patches));
        }
    }
    // Laravel needs the same database engine, and Composer + the PHP CLI to run
    // it — the phar is executed by the SITE's PHP so `create-project`'s platform
    // checks are made against the PHP the app will actually run on.
    if matches!(site.site_type, SiteType::Laravel) && !linked {
        plan.extend(downloads::plan_for_engine(state.platform.as_ref(), engine, engine_version));
        plan.extend(downloads::plan_for_composer_tooling_with(state.platform.as_ref(), minor, patches));
    }
    // A Blank-PHP site that asked for a starter database needs the engine, and
    // ONLY then — this is why the field is a choice in the dialog rather than
    // always-on: a scratch PHP file must not cost a ~600 MB MySQL download.
    if matches!(site.site_type, SiteType::Php) && site.has_starter_db() && !linked {
        plan.extend(downloads::plan_for_engine(state.platform.as_ref(), engine, engine_version));
    }
    // A cloned Blank-PHP site needs Composer and the PHP CLI to run it — but
    // NOT a database engine unless it asked for one (above): fetching ~600 MB
    // of MySQL for a phase that will never run is the exact waste the
    // linked-site carve-out above exists to avoid.
    if matches!(site.site_type, SiteType::Php) && site.git_url.is_some() && !linked {
        plan.extend(downloads::plan_for_composer_tooling_with(state.platform.as_ref(), minor, patches));
    }
    plan
}

fn lock_db(state: &AppState) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>> {
    state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()))
}

/// Keep only the newest `keep` logs with `prefix` (per-job files so a failed
/// provision's log survives later attempts).
fn prune_logs(log_dir: &Path, prefix: &str, keep: usize) {
    let Ok(entries) = std::fs::read_dir(log_dir) else { return };
    let mut logs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(prefix) && n.ends_with(".log"))
        })
        .collect();
    logs.sort_by_key(|p| p.metadata().and_then(|m| m.modified()).ok());
    while logs.len() > keep {
        let _ = std::fs::remove_file(logs.remove(0));
    }
}

/// Shared start: register the entry + spawn the worker. `site` must already
/// exist (row inserted by `start_job`, or the retried half-site).
#[allow(clippy::too_many_arguments)]
fn spawn_job<R: tauri::Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    jobs: &ProvisionJobs,
    site: Site,
    wp_opts: wordpress::InstallOptions,
    blueprint: Option<Blueprint>,
    plan: Vec<downloads::PlannedBinary>,
) -> Result<SiteProvisionState> {
    let id = uuid::Uuid::new_v4().to_string();
    let log_dir = state.platform.paths().log_dir()?;
    let log_prefix = format!("site-provision-{}-", site.domain);
    prune_logs(&log_dir, &log_prefix, 4);
    let log_key = format!("site-provision-{}-{}.log", site.domain, &id[..8]);
    let log_path = log_dir.join(&log_key);

    let defs = phase_defs(PhasePlan::of(&site, blueprint.is_some()));
    let mut phases: Vec<PhaseState> = defs
        .iter()
        .map(|(k, l)| PhaseState { key: (*k).into(), label: (*l).into(), status: "pending".into() })
        .collect();
    // Prepare ran inline (row/docroot/cert exist before any job is visible).
    phases[0].status = "ok".into();
    let download_ids: Vec<String> = plan
        .iter()
        .filter(|p| !p.cached)
        .map(|p| downloads::item_id(&p.name, &p.version))
        .collect();

    let entry = Arc::new(ProvisionEntry {
        id: id.clone(),
        seq: jobs.next_seq.fetch_add(1, Ordering::SeqCst),
        domain: site.domain.clone(),
        wp_opts,
        blueprint,
        cancel: repo::CancelToken::new(),
        running: AtomicBool::new(true),
        timed_out: AtomicBool::new(false),
        log_path,
        state: Mutex::new(SiteProvisionState {
            id: id.clone(),
            domain: site.domain.clone(),
            site_id: Some(site.id.clone()),
            phases,
            phase_cursor: 1, // prepare done, fetch is next
            pct: 0,
            status: "running".into(),
            summary: None,
            error: None,
            log_key,
            download_ids,
            serving_blocked: false,
            serving_holder: None,
            serving_app: None,
            assets_warning: None,
        }),
    });
    {
        let mut map = jobs.jobs.lock().expect("provision jobs lock");
        let busy = map
            .values()
            .any(|e| e.domain == entry.domain && e.running.load(Ordering::SeqCst));
        if busy {
            return Err(Error::Other(format!(
                "a provision job is already running for {} — wait for it (or cancel it) first.",
                entry.domain
            )));
        }
        map.insert(id, entry.clone());
    }
    let _ = std::fs::write(&entry.log_path, "");
    append_line(app, &entry, &format!("── preparing site ({}) — done", entry.domain));

    let worker_app = app.clone();
    let worker_entry = entry.clone();
    tauri::async_runtime::spawn(async move {
        run_provision_job(worker_app, worker_entry, site, plan).await;
    });
    Ok(snapshot(&entry))
}

/// Shared start (the command + the `create_site` compat wrapper): runs the
/// PREPARE phase INLINE — resolver, docroot, cert, row insert — so validation
/// errors (bad domain, duplicate, declined privileged prompt) surface as
/// command errors with nothing half created, exactly like the old
/// `create_site` (the resolver runs BEFORE provisioning for the same reason).
pub(crate) fn start<R: tauri::Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    jobs: &ProvisionJobs,
    site: NewSite,
    wp: Option<wordpress::InstallOptions>,
    blueprint_id: Option<String>,
    ownership: core::sites::Ownership,
) -> Result<SiteProvisionState> {
    // The mirror of ProvisionJobs::busy_for: a database import mid-run for this
    // domain owns the site's database (it may be mid-DROP on a retry) — a
    // provision alongside it would race that.
    if let Ok(active) = state.db_import_active.lock() {
        if active.as_deref() == Some(site.domain.as_str()) {
            return Err(Error::Other(format!(
                "a database import is running for {} — wait for it (or cancel it) first.",
                site.domain
            )));
        }
    }
    // Same shape for a connection rewrite mid-apply/revert: it holds the
    // site's config file and the imported-state row.
    if let Ok(active) = state.rewrite_active.lock() {
        if active.as_deref() == Some(site.domain.as_str()) {
            return Err(Error::Other(format!(
                "a connection rewrite is running for {} — wait for it to finish first.",
                site.domain
            )));
        }
    }
    // Step 7: a tunnel EXPOSES rather than mutates — provisioning under a
    // live link publishes a half-built site to whoever holds it. RETRY is the
    // live case (the site exists and can be shared); on a fresh create this
    // is a belt (no site row yet ⇒ no tunnel).
    if let Some(tunnels) = app.try_state::<crate::commands::tunnels::Tunnels>() {
        crate::commands::tunnels::refuse_if_shared(
            &tunnels,
            state,
            &site.domain,
            "provisioning would rebuild it under the live link, publishing a half-built site",
        )?;
    }
    let site_tld = sites::domain_tld(&site.domain)?;
    // The prompt policy is DERIVED from the same ownership value that gets
    // recorded on the row — never passed separately. An agent-created site
    // therefore cannot raise a macOS authorization dialog, and that stays true
    // for the WHOLE operation rather than at an entry check: this is the only
    // prompting call the create path makes, `ensure_resolver`'s policy argument
    // is REQUIRED so a future one cannot be silent about it, and the job phases
    // downstream reach no privileged op (proven by
    // `an_agent_create_never_reaches_a_privileged_prompt`).
    core::prompt::while_prompting(|| {
        core::dns::ensure_resolver(
            state.platform.as_ref(),
            &site_tld,
            core::dns::DEFAULT_DNS_PORT,
            ownership.resolver_prompt(),
        )
    })?;

    // Refused HERE, before the row exists, so the caller gets a prepare-phase
    // error with nothing created rather than a half-site to clean up.
    ensure_blueprint_applies(site.site_type, &site.path, &site.git_url, blueprint_id.as_deref())?;

    let (created, blueprint) = {
        let conn = lock_db(state)?;
        let created =
            sites::provision_with(&conn, state.platform.as_ref(), &state.ca, site, ownership)?;
        // The job owns this row's lifecycle from here: 0 until settle-ok. A
        // crash between insert and this write leaves 1 — yesterday's
        // semantics, never a false alarm.
        crate::state::store::set_site_provisioned(&conn, &created.id, false)?;
        let blueprint = match &blueprint_id {
            Some(id) if !id.is_empty() => crate::state::store::get_blueprint(&conn, id)?,
            _ => None,
        };
        (created, blueprint)
    };
    let minor = core::php::minor_of(&created.php_version);
    let engine = DbEngine::from_site(created.db_engine);
    let engine_version = super::database::effective_db_version(state, engine)?;
    let patches = {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        core::php::effective_patches(&conn)?
    };
    let plan = build_plan(&patches, state, &created, &minor, engine, &engine_version);
    spawn_job(app, state, jobs, created, wp.unwrap_or_default(), blueprint, plan)
}

/// Start a streamed create job. Returns the initial snapshot; progress
/// arrives via `site-provision://state|output/<id>` events.
#[tauri::command]
pub async fn site_provision_job<R: tauri::Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    jobs: State<'_, ProvisionJobs>,
    site: NewSite,
    wp: Option<wordpress::InstallOptions>,
    blueprint_id: Option<String>,
) -> Result<SiteProvisionState> {
    // The IPC create command IS the user's own action.
    start(&app, &state, &jobs, site, wp, blueprint_id, core::sites::Ownership::User)
}

/// Re-enter provisioning for a `provisioned = 0` half-site (the "setup
/// incomplete" badge's Retry). Prepare's artifacts are re-ENSURED, not
/// assumed: docroot `create_dir_all`, the Blank-PHP `index.php` only if
/// missing (never clobbers user files), `ensure_site_cert` (early-returns
/// when both files exist) — a half-site missing any of them (failure landed
/// mid-prepare, user deleted files) gets them back before the idempotent
/// install steps run again.
#[tauri::command]
pub async fn site_provision_retry<R: tauri::Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    jobs: State<'_, ProvisionJobs>,
    site_id: String,
) -> Result<SiteProvisionState> {
    let site = {
        let conn = lock_db(&state)?;
        sites::get(&conn, &site_id)?.ok_or_else(|| Error::Other(format!("no site {site_id}")))?
    };
    if site.provisioned {
        return Err(Error::Other(format!(
            "{} is already fully provisioned — nothing to retry.",
            site.domain
        )));
    }
    // The mirror of ProvisionJobs::busy_for: a database import mid-run for this
    // domain owns the site's database (it may be mid-DROP on a retry) — a
    // provision alongside it would race that.
    if let Ok(active) = state.db_import_active.lock() {
        if active.as_deref() == Some(site.domain.as_str()) {
            return Err(Error::Other(format!(
                "a database import is running for {} — wait for it (or cancel it) first.",
                site.domain
            )));
        }
    }
    // Same shape for a connection rewrite mid-apply/revert: it holds the
    // site's config file and the imported-state row.
    if let Ok(active) = state.rewrite_active.lock() {
        if active.as_deref() == Some(site.domain.as_str()) {
            return Err(Error::Other(format!(
                "a connection rewrite is running for {} — wait for it to finish first.",
                site.domain
            )));
        }
    }
    // Step 7: a tunnel EXPOSES rather than mutates — provisioning under a
    // live link publishes a half-built site to whoever holds it. RETRY is the
    // live case (the site exists and can be shared); on a fresh create this
    // is a belt (no site row yet ⇒ no tunnel).
    if let Some(tunnels) = app.try_state::<crate::commands::tunnels::Tunnels>() {
        crate::commands::tunnels::refuse_if_shared(
            &tunnels,
            &state,
            &site.domain,
            "provisioning would rebuild it under the live link, publishing a half-built site",
        )?;
    }
    let site_tld = sites::domain_tld(&site.domain)?;
    // Retry is a USER action — the Retry button in the app, or the CLI. (No
    // agent tool retries: a scratch create that failed leaves a site the user
    // can retry or delete, and the agent is told exactly that.)
    core::prompt::while_prompting(|| {
        core::dns::ensure_resolver(
            state.platform.as_ref(),
            &site_tld,
            core::dns::DEFAULT_DNS_PORT,
            core::dns::ResolverPrompt::Allow,
        )
    })?;
    let docroot = PathBuf::from(&site.path);
    // Re-ensure prepare's artifacts — but ONLY for a docroot we own. A linked
    // site's folder is the user's: creating it, or dropping our starter page
    // into an empty one, would write into their project on a retry. The cert
    // below is ours either way.
    if site.docroot_managed != Some(false) {
        std::fs::create_dir_all(&docroot)?;
        // Same reason as at create: a clone needs the docroot EMPTY, so the
        // starter page must not be the thing that blocks the retried clone.
        // `write_files` skips a file that already exists, so a retry after the
        // user has edited the page leaves their edit alone.
        let cloning = site.git_url.is_some();
        if matches!(site.site_type, SiteType::Php) && !cloning {
            core::starter::write_files(&docroot, None)?;
        }
    } else if !docroot.is_dir() {
        return Err(Error::Other(format!(
            "{} is linked to {}, which no longer exists — rexenv won't recreate a folder it \
             doesn't own. Restore it (or delete the site) and try again.",
            site.domain,
            docroot.display()
        )));
    }
    core::ssl::ensure_site_cert(
        state.platform.paths(),
        state.platform.permissions(),
        &state.ca,
        &site.domain,
    )?;

    let minor = core::php::minor_of(&site.php_version);
    let engine = DbEngine::from_site(site.db_engine);
    let engine_version = super::database::effective_db_version(&state, engine)?;
    let patches = {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        core::php::effective_patches(&conn)?
    };
    let plan = build_plan(&patches, &state, &site, &minor, engine, &engine_version);
    // NOTE: the original blueprint id isn't persisted on the site — a retry
    // re-runs the core install path; blueprint items that already installed
    // persist (real installs), missing ones need a manual pass.
    spawn_job(&app, &state, &jobs, site, wordpress::InstallOptions::default(), None, plan)
}

/// Cancel a running provision. Only ever kills wp-cli children THIS job
/// spawned (registered pgid); between phases it just stops the job at the
/// next boundary. The detached binary prefetch is untouched — see module doc.
#[tauri::command]
pub async fn site_provision_cancel(
    state: State<'_, AppState>,
    jobs: State<'_, ProvisionJobs>,
    id: String,
) -> Result<()> {
    let entry = jobs
        .jobs
        .lock()
        .expect("provision jobs lock")
        .get(&id)
        .cloned()
        .ok_or_else(|| Error::Other(format!("no provision job {id}")))?;
    entry.cancel.cancel(state.platform.supervisor());
    Ok(())
}

/// One job's current snapshot by id (the CLI's / wrapper's settle-poll).
pub fn state_of(jobs: &ProvisionJobs, id: &str) -> Result<SiteProvisionState> {
    jobs.jobs
        .lock()
        .expect("provision jobs lock")
        .get(id)
        .map(|e| snapshot(e))
        .ok_or_else(|| Error::Other(format!("no provision job {id}")))
}

/// Wait for a provision job to leave `running` and return its SETTLED state,
/// reporting each change of phase or percentage to `on_progress` on the way.
///
/// The one wait for every caller that answers with an outcome. It was a loop
/// inside `commands::sites::create_site_owned_with` only, so the MCP `site_retry`
/// tool — whose description promised "blocks until it settles" — returned the
/// job's FIRST snapshot instead: on 11 Sep 2026 a real retry replied
/// `status: "running"` with every phase pending, and the agent had to poll
/// `site_info` to learn whether its retry had worked.
pub(crate) async fn settle(
    jobs: &ProvisionJobs,
    id: &str,
    on_progress: Option<&(dyn Fn(&SiteProvisionState) + Sync)>,
) -> Result<SiteProvisionState> {
    let mut last = (usize::MAX, u8::MAX);
    loop {
        let st = state_of(jobs, id)?;
        if let Some(report) = on_progress {
            if (st.phase_cursor, st.pct) != last {
                last = (st.phase_cursor, st.pct);
                report(&st);
            }
        }
        if st.status != "running" {
            return Ok(st);
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
}

/// The most recent provision job (optionally for one domain), running or
/// settled — lets the Sites route / reopened dialog re-adopt the card.
#[tauri::command]
pub async fn site_provision_active(
    jobs: State<'_, ProvisionJobs>,
    domain: Option<String>,
) -> Result<Option<SiteProvisionState>> {
    let map = jobs.jobs.lock().expect("provision jobs lock");
    Ok(map
        .values()
        .filter(|e| domain.as_deref().map_or(true, |d| e.domain == d))
        .max_by_key(|e| e.seq)
        .map(|e| snapshot(e)))
}

// ---------------------------------------------------------------------------
// Worker
// ---------------------------------------------------------------------------

/// How the drive ended. Cancelled/timed-out attribution comes from the
/// pre-recorded flag, not the token.
enum JobEnd {
    Ok(String),
    Failed(String),
    Cancelled,
}

async fn run_provision_job<R: tauri::Runtime>(
    app: AppHandle<R>,
    entry: Arc<ProvisionEntry>,
    site: Site,
    plan: Vec<downloads::PlannedBinary>,
) {
    let keys: Vec<String> =
        snapshot(&entry).phases.iter().map(|p| p.key.clone()).collect();
    let key_refs: Vec<&str> = keys.iter().map(String::as_str).collect();
    let mut progress = sites::ProvisionProgress::new(&key_refs);
    progress.advance(1, 0.0); // prepare done

    let end = drive(&app, &entry, &site, plan, &mut progress).await;
    {
        let mut st = entry.state.lock().expect("provision state lock");
        match end {
            JobEnd::Ok(summary) => {
                st.status = "ok".into();
                st.pct = progress.finish();
                st.summary = Some(summary);
            }
            JobEnd::Cancelled => {
                st.status = if entry.timed_out.load(Ordering::SeqCst) {
                    "timed_out".into()
                } else {
                    "cancelled".into()
                };
                let cur = st.phase_cursor;
                if let Some(p) = st.phases.get_mut(cur) {
                    if p.status == "running" {
                        p.status = "cancelled".into();
                    }
                }
            }
            JobEnd::Failed(err) => {
                st.status = "failed".into();
                let cur = st.phase_cursor;
                if let Some(p) = st.phases.get_mut(cur) {
                    if p.status == "running" {
                        p.status = "failed".into();
                    }
                }
                st.error = Some(err);
            }
        }
    }
    // Settle-ok is the ONLY provisioned=1 writer — the flag means "the job
    // finished everything it could", independent of whether the stack was
    // running to serve it (that truth lives in the summary).
    if snapshot(&entry).status == "ok" {
        let state = app.state::<AppState>();
        if let Ok(conn) = lock_db(&state) {
            let _ = crate::state::store::set_site_provisioned(&conn, &site.id, true);
            // A WordPress site cannot reach ITSELF over its .rex hostname
            // without this (bundled PHP resolves via c-ares, blind to
            // /etc/resolver) — WP-Cron would be dead from the first minute,
            // silently. Installed here, at the ONE path every create takes;
            // the startup pass in lib.rs is the backstop, not the mechanism.
            if let Ok(Some(fresh)) = crate::core::sites::get(&conn, &site.id) {
                crate::core::wp_dns::ensure_for_site(&conn, &fresh);
                crate::core::wp_mail_catch::apply_for_site(&conn, &fresh);
            }
        };
    }
    emit_state(&app, &entry);
    entry.running.store(false, Ordering::SeqCst);
}

/// Enter phase `idx`: mark running + log the boundary marker.
fn enter_phase<R: tauri::Runtime>(app: &AppHandle<R>, entry: &ProvisionEntry, idx: usize) {
    let label = {
        let mut st = entry.state.lock().expect("provision state lock");
        st.phase_cursor = idx;
        st.phases[idx].status = "running".into();
        st.phases[idx].label.clone()
    };
    append_line(app, entry, &format!("── {label}"));
    emit_state(app, entry);
}

/// Finish phase `idx` (ok/skipped) and fold the weight into the bar.
fn finish_phase<R: tauri::Runtime>(
    app: &AppHandle<R>,
    entry: &ProvisionEntry,
    progress: &mut sites::ProvisionProgress,
    idx: usize,
    status: &str,
    note: Option<&str>,
) {
    let pct = progress.advance(idx + 1, 0.0);
    {
        let mut st = entry.state.lock().expect("provision state lock");
        st.phases[idx].status = status.into();
        st.pct = pct;
    }
    if let Some(n) = note {
        append_line(app, entry, n);
    }
    emit_state(app, entry);
}

fn phase_index(entry: &ProvisionEntry, key: &str) -> usize {
    snapshot(entry).phases.iter().position(|p| p.key == key).unwrap_or(0)
}

/// The login-shell env a Laravel provisioning step runs with, plus the mail
/// catch-all (`core::laravel::mail_env`).
///
/// Composer sees these too and does not care. Artisan does: `migrate` on a
/// cloned app can fire model events or seeders that send mail, and a
/// provisioning run must not be the one path that reaches a real inbox because
/// the repo shipped a `.env` naming the customer's SMTP provider.
///
/// Appended AFTER the shell's own vars so it wins — the same precedence the
/// pool config relies on.
fn with_mail_catch(state: &AppState, env: crate::commands::repo::EnvSnapshot) -> crate::commands::repo::EnvSnapshot {
    let extra = match state.db.lock() {
        Ok(conn) => core::laravel::mail_env(&conn),
        // A poisoned lock is not a reason to mail the real world: fall back to
        // catching, which is this subsystem's safe direction everywhere else.
        Err(_) => core::mail::laravel_env()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    };
    if extra.is_empty() {
        return env;
    }
    let mut merged = (*env).clone();
    merged.extend(extra);
    std::sync::Arc::new(merged)
}

async fn drive<R: tauri::Runtime>(
    app: &AppHandle<R>,
    entry: &Arc<ProvisionEntry>,
    site: &Site,
    plan: Vec<downloads::PlannedBinary>,
    progress: &mut sites::ProvisionProgress,
) -> JobEnd {
    let cancelled = || entry.cancel.is_cancelled();
    macro_rules! bail_if_cancelled {
        () => {
            if cancelled() {
                return JobEnd::Cancelled;
            }
        };
    }

    // ── fetch ────────────────────────────────────────────────────────────
    let fx = phase_index(entry, "fetch");
    enter_phase(app, entry, fx);
    let download_ids = snapshot(entry).download_ids.clone();
    if download_ids.is_empty() {
        finish_phase(app, entry, progress, fx, "skipped", Some("binaries already cached"));
    } else {
        // DETACHED on purpose: cancel stops WAITING, never the download —
        // a Hub item is shared state other consumers (another create, a PHP
        // switch, Start all) may be waiting on, and an aborted stream would
        // hand the next caller a truncated-restart. A cancelled create still
        // warms the cache for its own retry.
        let plan2 = plan.clone();
        let mut fetch = tauri::async_runtime::spawn(async move {
            let plat = crate::platform::current();
            downloads::prefetch(plat.as_ref(), "Create site", &plan2).await
        });
        let fetched = loop {
            tokio::select! {
                r = &mut fetch => break Some(r),
                _ = tokio::time::sleep(Duration::from_millis(500)) => {
                    if cancelled() {
                        break None; // detach — the task keeps downloading
                    }
                    // Fold REAL bytes into the fetch slice. totalBytes can be
                    // None (no Content-Length) → count that item done-by-phase
                    // instead of by bytes.
                    let snap = downloads::hub().snapshot();
                    let mine: Vec<_> = snap
                        .items
                        .iter()
                        .filter(|i| download_ids.contains(&i.id))
                        .collect();
                    let frac = if mine.iter().all(|i| i.total_bytes.is_some()) && !mine.is_empty() {
                        let got: u64 = mine.iter().map(|i| i.downloaded_bytes).sum();
                        let total: u64 = mine.iter().filter_map(|i| i.total_bytes).sum();
                        if total > 0 { got as f64 / total as f64 } else { 0.0 }
                    } else {
                        let done = mine
                            .iter()
                            .filter(|i| matches!(i.phase, downloads::Phase::Done | downloads::Phase::Cached))
                            .count();
                        done as f64 / download_ids.len() as f64
                    };
                    let pct = progress.advance(fx, frac);
                    let changed = {
                        let mut st = entry.state.lock().expect("provision state lock");
                        let moved = st.pct != pct;
                        st.pct = pct;
                        moved
                    };
                    if changed {
                        emit_state(app, entry);
                    }
                }
            }
        };
        match fetched {
            None => return JobEnd::Cancelled,
            Some(Err(join)) => return JobEnd::Failed(format!("binary fetch worker died: {join}")),
            Some(Ok(Err(e))) => return JobEnd::Failed(format!("binary download failed: {e}")),
            Some(Ok(Ok(()))) => finish_phase(app, entry, progress, fx, "ok", None),
        }
    }
    bail_if_cancelled!();

    let state = app.state::<AppState>();
    // A linked site is ADOPTED, never installed into — see `phase_defs`. The
    // phase list already omits the WordPress phases; this keeps the driver in
    // step so it can't run a step that has no phase to report into.
    let linked = site.docroot_managed == Some(false);
    let is_wp = matches!(site.site_type, SiteType::Wordpress) && !linked;
    let is_laravel = matches!(site.site_type, SiteType::Laravel) && !linked;
    let minor = core::php::minor_of(&site.php_version);
    // Read through the validator, not straight off the row — see
    // `sites::git_source_of`.
    let git = match sites::git_source_of(site) {
        Ok(g) => g,
        Err(e) => return JobEnd::Failed(e.to_string()),
    };
    let from_git = git.is_some();

    // ── clone ────────────────────────────────────────────────────────────
    // Before every type-specific phase: nothing can be installed into, wired
    // to a database, or served until the code is actually on disk.
    if let Some(src) = git {
        let ix = phase_index(entry, "clone");
        enter_phase(app, entry, ix);
        let project = PathBuf::from(&site.path);
        if project.join(".git").exists() {
            // A Retry after a LATER phase failed. Re-cloning would be both a
            // needless download and impossible — the docroot is no longer
            // empty, which `clone_into_docroot` refuses by design.
            finish_phase(app, entry, progress, ix, "skipped", Some("already cloned here"));
        } else {
            let (a2, e2) = (app.clone(), entry.clone());
            let (p2, ty) = (project.clone(), site.site_type);
            let cloned = tauri::async_runtime::spawn_blocking(move || -> Result<_> {
                let st = a2.state::<AppState>();
                let env = shell_env(&st, &a2.state::<RepoJobs>(), false)?;
                // The developer's own git and ssh-agent (the deliberate
                // bundled-client-rule departure, ARCHITECTURE §9): a private
                // repo clones with the keys they already use.
                let git = core::devtools::resolve_git(st.platform.as_ref(), &env)?;
                let mut on_line = |line: &str| append_line(&a2, &e2, line);
                core::sites::clone_into_docroot(
                    st.platform.supervisor(),
                    &git.path,
                    &env,
                    &src,
                    &p2,
                    ty,
                    &e2.cancel,
                    &mut on_line,
                )
            })
            .await;
            let detected = match cloned {
                Ok(Ok(d)) => d,
                Ok(Err(_)) if entry.cancel.is_cancelled() => return JobEnd::Cancelled,
                Ok(Err(e)) => return JobEnd::Failed(e.to_string()),
                Err(e) => return JobEnd::Failed(format!("clone worker died: {e}")),
            };
            // What the checkout actually serves, read from the checkout rather
            // than assumed from the type. Creation could only guess from the
            // site type, since the docroot was empty when the row was written.
            {
                let (a2, sid, rel) =
                    (app.clone(), site.id.clone(), detected.docroot_rel.clone());
                // A WordPress checkout's CONTENT dir has the same problem one
                // level down: creation recorded `wp-content` from an EMPTY
                // folder, and Bedrock's is `app`. Recorded here, from the
                // served root the line above just settled, so the mu-plugin
                // writers never land in a directory the site does not load.
                let content = (detected.site_type == SiteType::Wordpress).then(|| {
                    let served = if rel.is_empty() {
                        PathBuf::from(&site.path)
                    } else {
                        PathBuf::from(&site.path).join(&rel)
                    };
                    sites::detect_content_dir_rel(&served).to_string()
                });
                let recorded = tauri::async_runtime::spawn_blocking(move || {
                    let st = a2.state::<AppState>();
                    let conn = lock_db(&st)?;
                    crate::state::store::set_site_docroot_subdir(&conn, &sid, &rel)?;
                    if let Some(c) = content {
                        crate::state::store::set_site_content_dir(&conn, &sid, &c)?;
                    }
                    Ok::<(), crate::error::Error>(())
                })
                .await;
                match recorded {
                    Ok(Ok(_)) => {}
                    Ok(Err(e)) => {
                        return JobEnd::Failed(format!("recording the document root failed: {e}"))
                    }
                    Err(e) => return JobEnd::Failed(format!("docroot worker died: {e}")),
                }
            }
            append_line(
                app,
                entry,
                &format!(
                    "✓ {} — serving {}",
                    detected.label,
                    if detected.docroot_rel.is_empty() {
                        "the project root".to_string()
                    } else {
                        format!("{}/", detected.docroot_rel)
                    }
                ),
            );
            finish_phase(app, entry, progress, ix, "ok", None);
        }
        // The repository's `require.php`, read the moment the code is on disk — fresh clone or a
        // Retry's skip — and BEFORE the database starts and `.env` is written. It used to wait for
        // `deps`, two phases on: a user's `^8.4.1` Laravel repo on 8.3 started MySQL and wired a
        // database before being told to switch PHP (8 Oct 2026). `deps` still checks: the minor
        // can change between this phase and that one only by a Retry, which comes back here.
        if let Some((requirement, why)) = php_requirement_refused(&project, &minor) {
            append_line(app, entry, &format!("composer.json requires PHP {requirement}; this site runs PHP {minor}"));
            finish_phase(app, entry, progress, ix, "failed", Some(&format!("needs PHP {requirement}")));
            return JobEnd::Failed(why);
        }
        // And the extensions it requires (`ext-*`, the root's and every locked package's), asked
        // of the site's REAL PHP — only when there are any, so a repo that needs none never
        // waits on a PHP download here. A Windows 8.4 site needed `ext-imap` and was told by
        // `composer install`, after the database (8 Oct 2026, #802).
        if !core::repo::project_ext_requirements(&project).is_empty() {
            if let Ok((php_bin, _)) = composer_tools(&state, &minor).await {
                let (p2, m2) = (project.clone(), minor.clone());
                let refused = tauri::async_runtime::spawn_blocking(move || {
                    core::repo::ext_requirement_refused_in(&p2, &m2, &php_bin)
                })
                .await
                .ok()
                .flatten();
                if let Some(why) = refused {
                    append_line(app, entry, &why);
                    finish_phase(app, entry, progress, ix, "failed", Some("needs a PHP extension"));
                    return JobEnd::Failed(why);
                }
            }
        }
        bail_if_cancelled!();
    }

    // What the checkout turned out to be, and therefore what the web server
    // will root at. RECOMPUTED here rather than carried out of the clone phase,
    // because on a Retry that phase SKIPS and the answer is still needed — and
    // because the `site` row in hand still carries the `docroot_subdir` that
    // creation could only guess from the site TYPE, before the folder had any
    // contents. Pure filesystem, no execution (`detect_project`).
    let detected = from_git.then(|| sites::detect_project(Path::new(&site.path)));
    let served = match detected.as_ref().map(|d| d.docroot_rel.as_str()) {
        Some(rel) if !rel.is_empty() => PathBuf::from(&site.path).join(rel),
        _ => site.served_root(),
    };
    // Bedrock/Radicle: Composer owns WordPress core and `.env` owns the
    // configuration, so two of the four WordPress phases must stand down.
    let composer_core = detected.as_ref().is_some_and(sites::wordpress_core_from_composer);

    if is_wp {
        // ── db ───────────────────────────────────────────────────────────
        let ix = phase_index(entry, "db");
        enter_phase(app, entry, ix);
        let engine = DbEngine::from_site(site.db_engine);
        let check = {
            let mut mgr = state.services.lock().await;
            match mgr.spawn_db(state.platform.as_ref(), engine).await {
                Ok(c) => c,
                Err(e) => return JobEnd::Failed(format!("database start failed: {e}")),
            }
        };
        if let Err(e) = service_manager::await_ready(check.into_iter().collect()).await {
            return JobEnd::Failed(format!("database not ready: {e}"));
        }
        finish_phase(app, entry, progress, ix, "ok", None);
        bail_if_cancelled!();

        // Tools + env for the streamed wp-cli steps.
        let (php_bin, wp_phar) = match wp_tools(&state, &minor).await {
            Ok(t) => t,
            Err(e) => return JobEnd::Failed(e.to_string()),
        };
        let env = {
            let a = app.clone();
            match tauri::async_runtime::spawn_blocking(move || {
                let st = a.state::<AppState>();
                shell_env(&st, &a.state::<RepoJobs>(), false)
            })
            .await
            {
                Ok(Ok(env)) => env,
                Ok(Err(e)) => return JobEnd::Failed(e.to_string()),
                Err(e) => return JobEnd::Failed(format!("env worker died: {e}")),
            }
        };
        // The SERVED root, not the project root. They are the same folder for
        // a stock WordPress site and differ for Bedrock (`web/`), where
        // wp-cli's config, core and install all live one level in.
        let docroot = served.clone();
        let engine_version = match super::database::effective_db_version(&state, engine) {
            Ok(v) => v,
            Err(e) => return JobEnd::Failed(e.to_string()),
        };
        let (db_client, _) =
            match engine.sql_client_bins(state.platform.as_ref(), &engine_version).await {
                Ok(c) => c,
                Err(e) => return JobEnd::Failed(e.to_string()),
            };
        let db_host = format!("127.0.0.1:{}", engine.port());
        let resolved =
            wordpress::resolve_install_options(&site.domain, &site.name, &entry.wp_opts);

        // ── deps (a CLONED WordPress site) ───────────────────────────────
        // Bedrock's WordPress CORE arrives here, which is why this runs before
        // `core_download` rather than after: the skip below then reads a tree
        // Composer has already filled.
        if from_git {
            let (composer_php, composer_phar) = match composer_tools(&state, &minor).await {
                Ok(t) => t,
                Err(e) => return JobEnd::Failed(e.to_string()),
            };
            // The PROJECT root: `composer.json` sits beside `web/`, not in it.
            let project = PathBuf::from(&site.path);
            if let Some(end) =
                deps_phase(app, entry, progress, &project, &minor, &composer_php, &composer_phar, &env)
                    .await
            {
                return end;
            }
            bail_if_cancelled!();
        }

        // ── core_download ────────────────────────────────────────────────
        let ix = phase_index(entry, "core_download");
        enter_phase(app, entry, ix);
        if composer_core {
            // Downloading core into `web/` would put a SECOND WordPress beside
            // the one Composer installed at `web/wp` — and the site would keep
            // working, from the wrong copy.
            finish_phase(
                app,
                entry,
                progress,
                ix,
                "skipped",
                Some("this layout installs WordPress core through Composer"),
            );
        } else if docroot.join("wp-load.php").exists() {
            finish_phase(app, entry, progress, ix, "skipped", Some("WordPress core already present"));
        } else {
            // Always the ZIP build (`wordpress::core_zip_url`, ledger #604): the tarball
            // default reaches PharData, which cut 40 member names at 100 characters and
            // left sites missing core files.
            let args = match wordpress::core_zip_url(&resolved.locale, None, false) {
                Ok(url) => wordpress::core_download_args(&url),
                Err(e) => return JobEnd::Failed(format!("wp core download failed: {e}")),
            };
            match streamed_step(app, entry, &env, &php_bin, &wp_phar, &docroot, args).await {
                StepEnd::Ok => finish_phase(app, entry, progress, ix, "ok", None),
                StepEnd::Cancelled => return JobEnd::Cancelled,
                StepEnd::Failed(e) => return JobEnd::Failed(format!("wp core download failed: {e}")),
            }
        }
        bail_if_cancelled!();

        // ── configure ────────────────────────────────────────────────────
        let ix = phase_index(entry, "configure");
        enter_phase(app, entry, ix);
        if composer_core {
            // Bedrock/Radicle keep the WHOLE configuration in `.env`; the
            // `wp-config.php` in `web/` is the repository's own stub and just
            // requires `config/application.php`. `wp config create` here would
            // overwrite that stub with a stock one and the site would stop
            // reading its own config.
            let project = PathBuf::from(&site.path);
            match core::dotenv::ensure_file(&project, core::wordpress::BEDROCK_ENV_SEED) {
                Ok(core::dotenv::EnvOrigin::Example) => {
                    append_line(app, entry, ".env created from the repository's .env.example")
                }
                Ok(core::dotenv::EnvOrigin::Repo) => append_line(
                    app,
                    entry,
                    "kept the .env this repository ships — only the database, URLs and any                      unset salts are written",
                ),
                Ok(core::dotenv::EnvOrigin::Seeded) => append_line(
                    app,
                    entry,
                    "! this repository ships no .env.example — wrote a minimal one",
                ),
                Err(e) => return JobEnd::Failed(e.to_string()),
            }
            let env_file = project.join(".env");
            let original = match std::fs::read_to_string(&env_file) {
                Ok(t) => t,
                Err(e) => {
                    return JobEnd::Failed(format!(
                        "this project has no .env to wire ({}): {e}",
                        env_file.display()
                    ))
                }
            };
            let wired = core::wordpress::wire_bedrock_env(
                &original,
                &format!("https://{}", site.domain),
                &core::wordpress::BedrockDb {
                    name: site.db_name.clone(),
                    user: "root".into(),
                    password: String::new(),
                    host: db_host.clone(),
                },
            );
            if let Err(e) = std::fs::write(&env_file, wired) {
                return JobEnd::Failed(format!("writing {} failed: {e}", env_file.display()));
            }
            append_line(app, entry, ".env wired to this site's database and URL");
        } else if !docroot.join("wp-config.php").exists() {
            let args: Vec<String> = vec![
                "config".into(),
                "create".into(),
                format!("--dbname={}", site.db_name),
                "--dbuser=root".into(),
                "--dbpass=".into(),
                format!("--dbhost={db_host}"),
                "--skip-check".into(),
            ];
            match streamed_step(app, entry, &env, &php_bin, &wp_phar, &docroot, args).await {
                StepEnd::Ok => {}
                StepEnd::Cancelled => return JobEnd::Cancelled,
                StepEnd::Failed(e) => return JobEnd::Failed(format!("wp config create failed: {e}")),
            }
        }
        // Bundled client, never PATH (`wp db create` shells out to a PATH
        // mysql that a Finder-launched app doesn't have).
        let port = engine.port();
        let (dbc, dbn) = (db_client.clone(), site.db_name.clone());
        match tauri::async_runtime::spawn_blocking(move || engine.create_database(&dbc, port, &dbn))
            .await
        {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return JobEnd::Failed(format!("database create failed: {e}")),
            Err(e) => return JobEnd::Failed(format!("db worker died: {e}")),
        }
        finish_phase(app, entry, progress, ix, "ok", None);
        bail_if_cancelled!();

        // ── core_install ─────────────────────────────────────────────────
        let ix = phase_index(entry, "core_install");
        enter_phase(app, entry, ix);
        let (p2, w2, d2) = (php_bin.clone(), wp_phar.clone(), docroot.clone());
        let installed = tauri::async_runtime::spawn_blocking(move || {
            wordpress::core_is_installed(&p2, &w2, &d2)
        })
        .await
        .unwrap_or(false);
        if installed {
            finish_phase(app, entry, progress, ix, "skipped", Some("WordPress already installed"));
        } else {
            let args: Vec<String> = vec![
                "core".into(),
                "install".into(),
                format!("--url={}", resolved.url),
                format!("--title={}", resolved.title),
                format!("--admin_user={}", resolved.admin_user),
                // Throwaway on argv; the real one is set over stdin after the
                // install (`wordpress::install_wordpress` has the reasoning).
                format!("--admin_password={}", wordpress::throwaway_password()),
                format!("--admin_email={}", resolved.admin_email),
            ];
            match streamed_step(app, entry, &env, &php_bin, &wp_phar, &docroot, args).await {
                StepEnd::Ok => {
                    let (p2, w2, d2) = (php_bin.clone(), wp_phar.clone(), docroot.clone());
                    let script = wordpress::set_password_script(1, &resolved.admin_password);
                    let set = tauri::async_runtime::spawn_blocking(move || {
                        wordpress::wp_run_script(&p2, &w2, &d2, &script)
                    })
                    .await;
                    match set {
                        Ok(Ok(_)) => finish_phase(app, entry, progress, ix, "ok", None),
                        Ok(Err(e)) => {
                            return JobEnd::Failed(format!("wp core install: setting the admin password failed: {e}"))
                        }
                        Err(e) => return JobEnd::Failed(format!("password worker died: {e}")),
                    }
                }
                StepEnd::Cancelled => return JobEnd::Cancelled,
                StepEnd::Failed(e) => return JobEnd::Failed(format!("wp core install failed: {e}")),
            }
        }
        bail_if_cancelled!();

        // ── blueprint ────────────────────────────────────────────────────
        if let Some(bp) = entry.blueprint.clone() {
            let ix = phase_index(entry, "blueprint");
            enter_phase(app, entry, ix);
            let (a2, e2) = (app.clone(), entry.clone());
            let (p2, w2, d2, env2) = (php_bin.clone(), wp_phar.clone(), docroot.clone(), env.clone());
            let spec = bp.spec.clone();
            let applied = tauri::async_runtime::spawn_blocking(move || {
                let st = a2.state::<AppState>();
                let stream = wordpress::WpStream {
                    sup: st.platform.supervisor(),
                    env: &env2,
                    cancel: &e2.cancel,
                };
                let mut on_line = |line: &str| append_line(&a2, &e2, line);
                blueprints::apply_wordpress(&p2, &w2, &d2, &spec, &stream, &mut on_line)
            })
            .await;
            match applied {
                Ok(Ok(blueprints::ApplyOutcome::Done(_))) => {}
                Ok(Ok(blueprints::ApplyOutcome::Cancelled(_))) => return JobEnd::Cancelled,
                Ok(Err(e)) => return JobEnd::Failed(e.to_string()),
                Err(e) => return JobEnd::Failed(format!("blueprint worker died: {e}")),
            }
            if !matches!(bp.spec.multisite, MultisiteMode::None) {
                let (a2, sid) = (app.clone(), site.id.clone());
                let (p2, w2, d2, mode) =
                    (php_bin.clone(), wp_phar.clone(), docroot.clone(), bp.spec.multisite);
                let converted = tauri::async_runtime::spawn_blocking(move || {
                    let st = a2.state::<AppState>();
                    let conn = lock_db(&st)?;
                    sites::convert_multisite(&conn, &p2, &w2, &d2, &sid, mode)
                })
                .await;
                match converted {
                    Ok(Ok(_)) => {}
                    Ok(Err(e)) => return JobEnd::Failed(format!("multisite convert failed: {e}")),
                    Err(e) => return JobEnd::Failed(format!("multisite worker died: {e}")),
                }
            }
            finish_phase(app, entry, progress, ix, "ok", None);
            bail_if_cancelled!();
        }
    }

    if is_laravel {
        // ── db ───────────────────────────────────────────────────────────
        let ix = phase_index(entry, "db");
        enter_phase(app, entry, ix);
        let engine = DbEngine::from_site(site.db_engine);
        let check = {
            let mut mgr = state.services.lock().await;
            match mgr.spawn_db(state.platform.as_ref(), engine).await {
                Ok(c) => c,
                Err(e) => return JobEnd::Failed(format!("database start failed: {e}")),
            }
        };
        if let Err(e) = service_manager::await_ready(check.into_iter().collect()).await {
            return JobEnd::Failed(format!("database not ready: {e}"));
        }
        finish_phase(app, entry, progress, ix, "ok", None);
        bail_if_cancelled!();

        let (php_bin, composer_phar) = match composer_tools(&state, &minor).await {
            Ok(t) => t,
            Err(e) => return JobEnd::Failed(e.to_string()),
        };
        let env = {
            let a = app.clone();
            match tauri::async_runtime::spawn_blocking(move || {
                let st = a.state::<AppState>();
                shell_env(&st, &a.state::<RepoJobs>(), false)
            })
            .await
            {
                Ok(Ok(env)) => with_mail_catch(&state, env),
                Ok(Err(e)) => return JobEnd::Failed(e.to_string()),
                Err(e) => return JobEnd::Failed(format!("env worker died: {e}")),
            }
        };
        // The PROJECT root — `site.path`, not `served_root()`. Composer, the
        // `.env` and artisan all live one level above what nginx serves, which
        // is the entire point of `docroot_subdir` (v32).
        let project = PathBuf::from(&site.path);
        let engine_version = match super::database::effective_db_version(&state, engine) {
            Ok(v) => v,
            Err(e) => return JobEnd::Failed(e.to_string()),
        };
        let (db_client, _) =
            match engine.sql_client_bins(state.platform.as_ref(), &engine_version).await {
                Ok(c) => c,
                Err(e) => return JobEnd::Failed(e.to_string()),
            };

        // ── app_install (a NEW app only) ─────────────────────────────────
        // A cloned site's code is already on disk — the `clone` phase put it
        // there — and `composer create-project` would refuse the non-empty
        // directory anyway. Its dependencies come from `deps` below instead.
        if !from_git {
            let ix = phase_index(entry, "app_install");
            enter_phase(app, entry, ix);
            if core::laravel::is_installed(&project) {
                // A retry after a later phase failed: the app is already there and
                // `create-project` would refuse the non-empty directory anyway.
                finish_phase(app, entry, progress, ix, "skipped", Some("Laravel app already present"));
            } else {
                // A retry after `create-project` died past extraction (an
                // advisory-blocked framework, a dropped download): the skeleton
                // is on disk with no `vendor/`, Composer refuses a non-empty
                // target, and the skeleton was resolved for the PHP the site had
                // THEN. Cleared only when rexenv made the folder and it is
                // exactly that shape — see `laravel::is_failed_skeleton`.
                if site.docroot_managed == Some(true) && core::laravel::is_failed_skeleton(&project) {
                    append_line(app, entry, "── clearing the unfinished Laravel install a failed attempt left");
                    if let Err(e) = core::laravel::clear_failed_skeleton(&project) {
                        return JobEnd::Failed(format!("could not clear the unfinished Laravel install: {e}"));
                    }
                }
                let (a2, e2, env2) = (app.clone(), entry.clone(), env.clone());
                let (p2, c2, d2) = (php_bin.clone(), composer_phar.clone(), project.clone());
                let created = tauri::async_runtime::spawn_blocking(move || {
                    let st = a2.state::<AppState>();
                    let mut on_line = |line: &str| append_line(&a2, &e2, line);
                    core::laravel::create_project(
                        st.platform.supervisor(),
                        &p2,
                        &c2,
                        &d2,
                        &env2,
                        &e2.cancel,
                        &mut on_line,
                    )
                })
                .await;
                match created {
                    Ok(Ok(())) => finish_phase(app, entry, progress, ix, "ok", None),
                    Ok(Err(e)) if e.to_string().contains("cancelled") => return JobEnd::Cancelled,
                    Ok(Err(e)) => return JobEnd::Failed(format!("composer create-project failed: {e}")),
                    Err(e) => return JobEnd::Failed(format!("composer worker died: {e}")),
                }
            }
            bail_if_cancelled!();
        }

        // ── configure ────────────────────────────────────────────────────
        let ix = phase_index(entry, "configure");
        enter_phase(app, entry, ix);
        let port = engine.port();
        let (dbc, dbn) = (db_client.clone(), site.db_name.clone());
        match tauri::async_runtime::spawn_blocking(move || engine.create_database(&dbc, port, &dbn))
            .await
        {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return JobEnd::Failed(format!("database create failed: {e}")),
            Err(e) => return JobEnd::Failed(format!("db worker died: {e}")),
        }
        append_line(app, entry, &format!("created database `{}`", site.db_name));
        // Record the PROVENANCE, not just the fact: `should_drop_database` only
        // drops a non-WordPress site's database on `db_created == Some(true)`,
        // because for every other type NULL used to mean "provisioning never
        // made one". Laravel provisioning now does, so without this line
        // deleting the site would leave its database behind forever.
        {
            let (a2, sid) = (app.clone(), site.id.clone());
            let recorded = tauri::async_runtime::spawn_blocking(move || {
                let st = a2.state::<AppState>();
                let conn = lock_db(&st)?;
                crate::state::store::set_site_db_created(&conn, &sid, true)
            })
            .await;
            match recorded {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => return JobEnd::Failed(format!("recording the database failed: {e}")),
                Err(e) => return JobEnd::Failed(format!("db-record worker died: {e}")),
            }
        }

        // A cloned project has no `.env`: it is gitignored in every real
        // Laravel repo, and the copy from `.env.example` is
        // `create-project`'s post-root-package-install script, which a plain
        // `composer install` never fires. Written HERE, before `deps`, so the
        // `artisan package:discover` that composer runs at the end of the
        // install boots against this site's real configuration.
        if from_git {
            match core::laravel::ensure_env_file(&project) {
                Ok(core::dotenv::EnvOrigin::Example) => {
                    append_line(app, entry, ".env created from the repository's .env.example")
                }
                Ok(core::dotenv::EnvOrigin::Repo) => append_line(
                    app,
                    entry,
                    "kept the .env this repository ships — only APP_URL and DB_* are rewritten",
                ),
                Ok(core::dotenv::EnvOrigin::Seeded) => append_line(
                    app,
                    entry,
                    "! this repository ships no .env.example — wrote a minimal local .env",
                ),
                Err(e) => return JobEnd::Failed(e.to_string()),
            }
        }
        let env_file = core::laravel::env_path(&project);
        let db_settings = core::laravel::DbSettings::for_engine(engine, site.db_name.clone());
        let app_url = format!("https://{}", site.domain);
        let catch_mail = match state.db.lock() {
            Ok(conn) => core::mail::catch_all_enabled(&conn),
            Err(_) => true,
        };
        match std::fs::read_to_string(&env_file) {
            Ok(original) => {
                // A cloned repo can ship a committed `.env` holding real
                // credentials — a database password, an SMTP provider's key —
                // and this rewrite replaces them. Keep the original beside it
                // before writing: the values are not recoverable from anywhere
                // else, and "rexenv overwrote my .env" is only a footnote if the
                // file it overwrote is still sitting there.
                //
                // Written ONCE. A retry that overwrote the backup with the
                // already-wired file would destroy the very thing it exists to
                // preserve — the same set-once rule `mu_dir_created` follows.
                let backup = project.join(".env.rexenv-backup");
                if !backup.exists() {
                    if let Err(e) = std::fs::write(&backup, &original) {
                        log::warn!("laravel: could not back up {} : {e}", env_file.display());
                    } else {
                        append_line(app, entry, "kept the original .env as .env.rexenv-backup");
                    }
                }
                let wired = core::laravel::wire_env(&original, &app_url, &db_settings, catch_mail);
                if let Err(e) = std::fs::write(&env_file, wired) {
                    return JobEnd::Failed(format!("writing {} failed: {e}", env_file.display()));
                }
                append_line(
                    app,
                    entry,
                    if catch_mail {
                        ".env wired to this site's database, URL and the Mailpit catch-all"
                    } else {
                        ".env wired to this site's database and URL"
                    },
                );
            }
            // Both paths guarantee a `.env` by now — Composer's post-create
            // script on a new app, `ensure_env_file` on a clone — so a missing
            // one is a real anomaly. The site would run on Laravel's SQLite
            // default and nothing would say so, which is exactly the failure
            // worth being loud about.
            Err(e) => {
                return JobEnd::Failed(format!(
                    "the Laravel project has no .env to wire ({}): {e}",
                    env_file.display()
                ))
            }
        }
        finish_phase(app, entry, progress, ix, "ok", None);
        bail_if_cancelled!();

        // ── deps (a CLONED project only) ─────────────────────────────────
        if from_git {
            match deps_phase(app, entry, progress, &project, &minor, &php_bin, &composer_phar, &env).await {
                None => {}
                Some(end) => return end,
            }
            bail_if_cancelled!();
        }

        // ── finalize ─────────────────────────────────────────────────────
        let ix = phase_index(entry, "finalize");
        enter_phase(app, entry, ix);
        // A cloned repo has no APP_KEY (it lives in the gitignored `.env`), and
        // Laravel throws on the first request that touches the encrypter —
        // sessions, on page one. `--force` because the site is brand new: the
        // interactive confirmation exists to protect data encrypted with the
        // old key, and there is none.
        if from_git {
            let (a2, e2, env2) = (app.clone(), entry.clone(), env.clone());
            let (p2, d2) = (php_bin.clone(), project.clone());
            let keyed = tauri::async_runtime::spawn_blocking(move || {
                let st = a2.state::<AppState>();
                let mut on_line = |line: &str| append_line(&a2, &e2, line);
                core::laravel::artisan(
                    st.platform.supervisor(),
                    &p2,
                    &d2,
                    &["key:generate", "--force"],
                    &env2,
                    &e2.cancel,
                    &mut on_line,
                )
            })
            .await;
            match keyed {
                Ok(Ok(())) => append_line(app, entry, "application key generated"),
                Ok(Err(_)) if entry.cancel.is_cancelled() => return JobEnd::Cancelled,
                Ok(Err(e)) => return JobEnd::Failed(format!("php artisan key:generate failed: {e}")),
                Err(e) => return JobEnd::Failed(format!("artisan worker died: {e}")),
            }
            bail_if_cancelled!();
        }

        // Migrations against the database `.env` now names — unless the user
        // declined them for this cloned site (`Site::runs_migrations`, v34).
        // Read from the ROW, not from the job, so a Retry honours the same
        // answer the create did; the log says so, because a step that silently
        // does not happen is indistinguishable from one that failed quietly.
        //
        // Not belt-and-braces on either path. For a NEW app, `composer
        // create-project` already ran `artisan migrate --graceful` in its
        // post-create script — and at that moment the skeleton's `.env` still
        // said SQLite, so the users/cache/jobs tables went into
        // `database/database.sqlite` and the MySQL database this site
        // advertises is EMPTY. For a CLONE, nothing has migrated at all. Either
        // way, skipping this ships a site whose Databases screen shows a
        // database the app never filled. The stray SQLite file is left alone —
        // it is the project's own file, and deleting what Composer wrote would
        // be a surprise; it is simply no longer the connection `.env` names.
        if site.runs_migrations() {
            let (a2, e2, env2) = (app.clone(), entry.clone(), env.clone());
            let (p2, d2) = (php_bin.clone(), project.clone());
            let migrated = tauri::async_runtime::spawn_blocking(move || {
                let st = a2.state::<AppState>();
                let mut on_line = |line: &str| append_line(&a2, &e2, line);
                core::laravel::artisan(
                    st.platform.supervisor(),
                    &p2,
                    &d2,
                    &["migrate", "--force"],
                    &env2,
                    &e2.cancel,
                    &mut on_line,
                )
            })
            .await;
            match migrated {
                Ok(Ok(())) => append_line(app, entry, "migrations applied to the site's database"),
                Ok(Err(e)) if e.to_string().contains("cancelled") => return JobEnd::Cancelled,
                Ok(Err(e)) => {
                    let e = e.to_string();
                    let note = core::laravel::half_applied_migration_note(&e).map(|n| format!("\n{n}")).unwrap_or_default();
                    return JobEnd::Failed(format!("php artisan migrate failed: {e}{note}"));
                }
                Err(e) => return JobEnd::Failed(format!("artisan worker died: {e}")),
            }
        } else {
            append_line(
                app,
                entry,
                "migrations skipped — the database is empty until you run `php artisan migrate`",
            );
        }
        finish_phase(app, entry, progress, ix, "ok", None);
        bail_if_cancelled!();
    }

    // ── db + configure (a Blank-PHP site with a starter database) ────────
    //
    // The smallest version of what WordPress and Laravel do above: start the
    // engine, create the database, seed one table, and write the `db.php` the
    // generated page reads. The page itself was written at `prepare`; the
    // connection file waits until HERE, because until `CREATE DATABASE` returns
    // it would name a database that does not exist.
    if matches!(site.site_type, SiteType::Php) && site.has_starter_db() && !linked {
        let ix = phase_index(entry, "db");
        enter_phase(app, entry, ix);
        let engine = DbEngine::from_site(site.db_engine);
        let check = {
            let mut mgr = state.services.lock().await;
            match mgr.spawn_db(state.platform.as_ref(), engine).await {
                Ok(c) => c,
                Err(e) => return JobEnd::Failed(format!("database start failed: {e}")),
            }
        };
        if let Err(e) = service_manager::await_ready(check.into_iter().collect()).await {
            return JobEnd::Failed(format!("database not ready: {e}"));
        }
        finish_phase(app, entry, progress, ix, "ok", None);
        bail_if_cancelled!();

        let ix = phase_index(entry, "configure");
        enter_phase(app, entry, ix);
        let engine_version = match super::database::effective_db_version(&state, engine) {
            Ok(v) => v,
            Err(e) => return JobEnd::Failed(e.to_string()),
        };
        let (db_client, _) =
            match engine.sql_client_bins(state.platform.as_ref(), &engine_version).await {
                Ok(c) => c,
                Err(e) => return JobEnd::Failed(e.to_string()),
            };
        let port = engine.port();
        let (dbc, dbn) = (db_client.clone(), site.db_name.clone());
        // Create then seed in ONE blocking hop: both are short client
        // invocations, and splitting them would only add a window in which the
        // database exists with no table for the page to read.
        match tauri::async_runtime::spawn_blocking(move || {
            engine.create_database(&dbc, port, &dbn)?;
            core::starter::seed(engine, &dbc, port, &dbn)
        })
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return JobEnd::Failed(format!("database create failed: {e}")),
            Err(e) => return JobEnd::Failed(format!("db worker died: {e}")),
        }
        append_line(
            app,
            entry,
            &format!(
                "created database `{}` and seeded `{}`",
                site.db_name,
                core::starter::TABLE
            ),
        );
        // The PROVENANCE, for the same reason Laravel records it: without this
        // line `should_drop_database` reads NULL as "provisioning never made
        // one" for a non-WordPress site, and deleting the site would leave its
        // database behind forever.
        {
            let (a2, sid) = (app.clone(), site.id.clone());
            let recorded = tauri::async_runtime::spawn_blocking(move || {
                let st = a2.state::<AppState>();
                let conn = lock_db(&st)?;
                crate::state::store::set_site_db_created(&conn, &sid, true)
            })
            .await;
            match recorded {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => return JobEnd::Failed(format!("recording the database failed: {e}")),
                Err(e) => return JobEnd::Failed(format!("db-record worker died: {e}")),
            }
        }
        // Written LAST, and skipped if it already exists: on a Retry the
        // developer may have edited it, and this file is theirs from the moment
        // it lands.
        let db_settings = core::starter::StarterDb::for_engine(engine, &site.db_name);
        if let Err(e) = core::starter::write_files(&site.served_root(), Some(&db_settings)) {
            return JobEnd::Failed(format!("writing the starter connection failed: {e}"));
        }
        finish_phase(app, entry, progress, ix, "ok", None);
        bail_if_cancelled!();
    }

    // ── deps (a CLONED Blank-PHP site) ───────────────────────────────────
    //
    // The half of "any PHP repository" that the clone alone does not give you.
    // A Symfony, Craft, Statamic or Magento checkout is `vendor/`-less by
    // design; `detect_project` already found its front controller and recorded
    // it as the document root, and this is the step that makes what it points
    // at actually run.
    if from_git && matches!(site.site_type, SiteType::Php) && !linked {
        let (php_bin, composer_phar) = match composer_tools(&state, &minor).await {
            Ok(t) => t,
            Err(e) => return JobEnd::Failed(e.to_string()),
        };
        let env = {
            let a = app.clone();
            match tauri::async_runtime::spawn_blocking(move || {
                let st = a.state::<AppState>();
                shell_env(&st, &a.state::<RepoJobs>(), false)
            })
            .await
            {
                Ok(Ok(env)) => env,
                Ok(Err(e)) => return JobEnd::Failed(e.to_string()),
                Err(e) => return JobEnd::Failed(format!("env worker died: {e}")),
            }
        };
        // The PROJECT root, not `served_root()`: `composer.json` sits beside
        // the front controller's parent, which is exactly the distinction
        // `docroot_subdir` records.
        let project = PathBuf::from(&site.path);
        if let Some(end) =
            deps_phase(app, entry, progress, &project, &minor, &php_bin, &composer_phar, &env).await
        {
            return end;
        }
        bail_if_cancelled!();
    }

    // ── assets (a CLONED site that asked for them) ───────────────────────
    //
    // NON-FATAL by design, and this is the one phase where that is right. It
    // runs the DEVELOPER'S toolchain — their nvm node, the repo's own build
    // script — against code rexenv did not write. A missing node is not a
    // broken site: everything else is done, the site serves, and the honest
    // outcome is a warning that names the problem, not a "setup incomplete"
    // badge over a working install whose Retry re-runs the clone, the database
    // and Composer to reach the one step that was never ours to guarantee.
    if from_git && site.builds_assets() {
        let ix = phase_index(entry, "assets");
        enter_phase(app, entry, ix);
        let project = PathBuf::from(&site.path);
        // Read-only inspection, the same one the Git add panel uses: the
        // package manager comes from the repo's own `packageManager` field or
        // its lockfile, never from a rexenv preference.
        match core::repo::inspect_repo(&project).node {
            None => finish_phase(
                app,
                entry,
                progress,
                ix,
                "skipped",
                Some("no package.json in this repository — nothing to build"),
            ),
            Some(node) => {
                let (a2, e2) = (app.clone(), entry.clone());
                let (p2, manager, has_build) =
                    (project.clone(), node.manager.clone(), node.has_build);
                let built = tauri::async_runtime::spawn_blocking(move || -> Result<()> {
                    let st = a2.state::<AppState>();
                    let env = shell_env(&st, &a2.state::<RepoJobs>(), false)?;
                    let pm = core::devtools::resolve_package_manager(&env, &manager)?;
                    let sup = st.platform.supervisor();
                    let mut on_line = |line: &str| append_line(&a2, &e2, line);
                    core::repo::node_install(sup, &pm.path, &p2, &env, &e2.cancel, &mut on_line)?;
                    if has_build {
                        core::repo::node_build(sup, &pm.path, &p2, &env, &e2.cancel, &mut on_line)?;
                    } else {
                        on_line("! this repository has no \"build\" script — dependencies installed only");
                    }
                    Ok(())
                })
                .await;
                match built {
                    Ok(Ok(())) => finish_phase(app, entry, progress, ix, "ok", None),
                    // A cancel IS the user's decision to stop the whole job —
                    // only a genuine failure is the non-fatal case.
                    Ok(Err(_)) if entry.cancel.is_cancelled() => return JobEnd::Cancelled,
                    Ok(Err(e)) => {
                        let msg = e.to_string();
                        append_line(app, entry, &format!("✕ {msg}"));
                        entry.state.lock().expect("provision state lock").assets_warning =
                            Some(msg);
                        finish_phase(app, entry, progress, ix, "failed", None);
                    }
                    Err(e) => {
                        let msg = format!("the asset build worker died: {e}");
                        append_line(app, entry, &format!("✕ {msg}"));
                        entry.state.lock().expect("provision state lock").assets_warning =
                            Some(msg);
                        finish_phase(app, entry, progress, ix, "failed", None);
                    }
                }
            }
        }
        bail_if_cancelled!();
    }

    // ── serve ────────────────────────────────────────────────────────────
    let ix = phase_index(entry, "serve");
    enter_phase(app, entry, ix);
    let fresh_sites = {
        match lock_db(&state) {
            Ok(conn) => match sites::list(&conn) {
                Ok(s) => s,
                Err(e) => return JobEnd::Failed(e.to_string()),
            },
            Err(e) => return JobEnd::Failed(e.to_string()),
        }
    };
    // Read the edge's blocked state while we already hold the lock: the
    // watchdog maintains it, so this costs nothing, where probing :443 per site
    // would add seconds to every import.
    let edge_blocked;
    let checks = {
        let mut mgr = state.services.lock().await;
        edge_blocked = mgr.edge_blocked();
        if mgr.is_running() {
            if !matches!(site.web_server, WebServer::Frankenphp) {
                if let Err(e) = mgr.ensure_php_pool(state.platform.as_ref(), &minor).await {
                    return JobEnd::Failed(format!("php pool failed: {e}"));
                }
            }
            match mgr.reload(state.platform.as_ref(), &state.ca, &fresh_sites, false).await {
                Ok(c) => Some(c),
                // The site EXISTS — row, folder, certificate — it just isn't
                // being served yet. Say that, say what to do, and carry the
                // underlying reason verbatim rather than an exit code. The row
                // stays `provisioned = 0`, so the list shows "setup incomplete"
                // with Retry, which re-runs exactly these steps.
                Err(e) => {
                    return JobEnd::Failed(format!(
                        "created, but not being served yet — the web server wouldn't reload. \
                         Fix the cause below, then Retry.\n{e}"
                    ))
                }
            }
        } else {
            None
        }
    };
    // The stack was stopped, so nothing could be reloaded — START it, with the
    // new site already in the list `start_core` reads. Until 18 Sep 2026 this
    // settled ok as "serves on next stack start", which made the first site
    // anyone creates the one site rexenv did NOT serve instantly (the clean-VM
    // smoke test: WordPress installed, card green, Safari "can't connect",
    // and a Restart button to find). The start is the Start-all sequence
    // itself (`start_stack`): binaries prefetched, backends awaited, the
    // privileged edge prompt off the runtime, the wire verified — so a foreign
    // :443 or a port conflict fails THIS job with the same words Start all
    // would use, and Retry re-runs exactly this step.
    // …but only the APP starts the real stack. A live-check example drives this same job on a
    // fixture manager that adopts the database tier and nothing else, so `is_running()` is false and
    // this branch was reached on every run: since the 18 Sep change it called `start_stack` with the
    // REAL binaries, ports and privileged edge — on the dev Mac it collided with the user's own PHP
    // pools ("port 9783 … held by a leftover rexenv process"), and on a machine with nothing running
    // it would have started the whole stack, edge prompt included, from an example (found 9 Oct 2026,
    // ledger #805). Such a process settles as it did before 18 Sep: created, served on the next Start
    // all. `stack_guard::may_control_real_stack` is the one answer to "is this the app".
    if checks.is_none() && !crate::core::stack_guard::may_control_real_stack() {
        append_line(
            app,
            entry,
            "the stack is stopped, and this process is not the rexenv app — leaving it as it is \
             (a live check never starts the real stack)",
        );
        finish_phase(app, entry, progress, ix, "skipped", Some("not started outside the app"));
        return JobEnd::Ok(format!("created — {} serves on the next Start all", site.domain));
    }
    let started = if checks.is_none() {
        append_line(
            app,
            entry,
            "stack is stopped — starting it (the HTTPS edge may ask for your password)",
        );
        match crate::commands::services::start_stack(&state).await {
            Ok(()) => true,
            Err(e) => {
                return JobEnd::Failed(format!(
                    "created, but the stack wouldn't start — {} isn't being served yet. \
                     Fix the cause below, then Retry.\n{e}",
                    site.domain
                ))
            }
        }
    } else {
        false
    };
    match checks {
        None if started => {
            // `start_stack` already awaited every backend and verified OUR edge
            // answers :443; the new site was in the config it wrote.
            finish_phase(app, entry, progress, ix, "ok", None);
            JobEnd::Ok(format!("created — serving at https://{}", site.domain))
        }
        Some(checks) => {
            if let Err(e) = service_manager::await_ready(checks).await {
                return JobEnd::Failed(format!("site not answering after reload: {e}"));
            }
            finish_phase(app, entry, progress, ix, "ok", None);
            if edge_blocked {
                // The site really was created and the stack really did reload —
                // but something else owns :443, so claiming it is serving would
                // be a lie the user discovers by clicking the link.
                let help = state
                    .platform
                    .supervisor()
                    .port_conflict_help(core::proxy::DEFAULT_HTTPS_PORT, false);
                let quit = help.app.clone().unwrap_or_else(|| "it".into());
                {
                    let mut st = entry.state.lock().expect("provision state lock");
                    st.serving_blocked = true;
                    st.serving_holder = help.holder.clone();
                    st.serving_app = help.app.clone();
                }
                return JobEnd::Ok(format!(
                    "created — but {} is answering port 443, so {} won't load until you quit {quit}",
                    help.holder.as_deref().unwrap_or("another app"),
                    site.domain
                ));
            }
            JobEnd::Ok(format!("created — serving at https://{}", site.domain))
        }
        None => unreachable!("a stopped stack was started above, or the job already ended"),
    }
}

/// `(requirement, the refusal)` when `project`'s composer.json requires a PHP that `minor` is
/// not; `None` when it fits, has no composer.json, or states nothing this reads.
fn php_requirement_refused(project: &Path, minor: &str) -> Option<(String, String)> {
    let json = std::fs::read_to_string(project.join("composer.json")).ok()?;
    let requirement = core::repo::composer_php_requirement(&json)?;
    let why = core::repo::php_requirement_refusal(&requirement, minor, &core::php::all_minors())?;
    Some((requirement, why))
}

/// The `deps` phase: `composer install` in a cloned project.
///
/// `vendor/` is gitignored in every PHP project worth cloning, so this is what
/// stands between a checkout and a site — which is why it has no opt-out:
/// offering to skip it would be offering to create something that 500s. The
/// pinned phar on the SITE's PHP (never a system `composer`, which can be a
/// wrapper rather than a phar — Herd ships one), so the project's `php` and
/// `ext-*` platform checks are made against the interpreter the site will
/// actually run on.
///
/// Returns `None` when the phase settled (ok or skipped) and `Some(end)` when
/// the JOB must end. Shared by the Laravel and the Blank-PHP paths: a cloned
/// Symfony or Craft site needs the identical step, and a second copy of it
/// would be a second place to fix the day Composer's invocation changes.
///
/// A repo with no `composer.json` is SKIPPED rather than failed. The phase list
/// is fixed before the clone, so it cannot know — and "this repository has no
/// Composer dependencies" is an answer, not a fault.
#[allow(clippy::too_many_arguments)]
async fn deps_phase<R: tauri::Runtime>(
    app: &AppHandle<R>,
    entry: &Arc<ProvisionEntry>,
    progress: &mut sites::ProvisionProgress,
    project: &Path,
    minor: &str,
    php_bin: &Path,
    composer_phar: &Path,
    env: &EnvSnapshot,
) -> Option<JobEnd> {
    let ix = phase_index(entry, "deps");
    enter_phase(app, entry, ix);
    let manifest = project.join("composer.json");
    if !manifest.is_file() {
        finish_phase(
            app,
            entry,
            progress,
            ix,
            "skipped",
            Some("no composer.json in this repository — nothing to install"),
        );
        return None;
    }
    // The site's PHP runs composer, so a `require.php` it cannot satisfy fails HERE, in one
    // sentence on the card naming the minor to switch to — not two screens away in composer's
    // own refusal after the clone (`symfony/demo` on 8.3, 28 Sep 2026; ledger #751). The clone
    // phase asks first (8 Oct 2026); this is the same question at the step that needs the answer.
    if let Some((requirement, why)) = php_requirement_refused(project, minor) {
        append_line(app, entry, &format!("composer.json requires PHP {requirement}; this site runs PHP {minor}"));
        return Some(JobEnd::Failed(why));
    }
    // The extensions too, against the PHP that will run composer (#802) — the same question the
    // clone phase asked, at the step that needs the answer.
    let (p2, m2, b2) = (project.to_path_buf(), minor.to_string(), php_bin.to_path_buf());
    let refused = tauri::async_runtime::spawn_blocking(move || core::repo::ext_requirement_refused_in(&p2, &m2, &b2))
        .await
        .ok()
        .flatten();
    if let Some(why) = refused {
        append_line(app, entry, &why);
        return Some(JobEnd::Failed(why));
    }
    let (a2, e2, env2) = (app.clone(), entry.clone(), env.clone());
    let (p2, c2, d2) =
        (php_bin.to_path_buf(), composer_phar.to_path_buf(), project.to_path_buf());
    let installed = tauri::async_runtime::spawn_blocking(move || {
        let st = a2.state::<AppState>();
        let mut on_line = |line: &str| append_line(&a2, &e2, line);
        core::repo::composer_install(
            st.platform.supervisor(),
            &p2,
            &c2,
            &d2,
            &env2,
            &e2.cancel,
            &mut on_line,
        )
    })
    .await;
    match installed {
        Ok(Ok(())) => {
            finish_phase(app, entry, progress, ix, "ok", None);
            None
        }
        Ok(Err(_)) if entry.cancel.is_cancelled() => Some(JobEnd::Cancelled),
        // `map_composer_error` already says what failed — "composer install failed: …", or the
        // repository script that did; a second prefix here read "composer install failed:
        // composer install failed:" and blamed Composer for a failed migration (8 Oct 2026).
        Ok(Err(e)) => Some(JobEnd::Failed(e.to_string())),
        Err(e) => Some(JobEnd::Failed(format!("composer worker died: {e}"))),
    }
}

/// One streamed wp-cli step under the job's token + a B25 outer wall-clock
/// cap (`download_timeout(1)` — wp-cli's own 300s/download bound still fires
/// first on a dead link; the timer records the reason THEN cancels).
enum StepEnd {
    Ok,
    Cancelled,
    Failed(String),
}

async fn streamed_step<R: tauri::Runtime>(
    app: &AppHandle<R>,
    entry: &Arc<ProvisionEntry>,
    env: &EnvSnapshot,
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    args: Vec<String>,
) -> StepEnd {
    let done = Arc::new(AtomicBool::new(false));
    let (t_done, t_entry, t_app) = (done.clone(), entry.clone(), app.clone());
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(wordpress::download_timeout(1)).await;
        if !t_done.load(Ordering::SeqCst) && t_entry.running.load(Ordering::SeqCst) {
            t_entry.timed_out.store(true, Ordering::SeqCst);
            let st = t_app.state::<AppState>();
            t_entry.cancel.cancel(st.platform.supervisor());
        }
    });

    let (b_app, b_entry, b_env) = (app.clone(), entry.clone(), env.clone());
    let (php, phar, dr) = (php_bin.to_path_buf(), wp_phar.to_path_buf(), docroot.to_path_buf());
    let res = tauri::async_runtime::spawn_blocking(move || {
        let st = b_app.state::<AppState>();
        let stream = wordpress::WpStream {
            sup: st.platform.supervisor(),
            env: &b_env,
            cancel: &b_entry.cancel,
        };
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let mut on_line = |line: &str| append_line(&b_app, &b_entry, line);
        wordpress::wp_step_streamed(&stream, &php, &phar, &dr, &arg_refs, &mut on_line)
    })
    .await;
    done.store(true, Ordering::SeqCst);
    match res {
        Ok(Ok(sr)) if sr.ok => StepEnd::Ok,
        Ok(Ok(sr)) if sr.cancelled => StepEnd::Cancelled,
        Ok(Ok(sr)) => StepEnd::Failed(
            sr.tail.last().cloned().unwrap_or_else(|| format!("exit {}", crate::core::proc::exit_text(sr.exit))),
        ),
        Ok(Err(e)) => StepEnd::Failed(e.to_string()),
        Err(e) => StepEnd::Failed(format!("step worker died: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::sites::Ownership;

    /// A cloned repo's `require.php` is answered from the checkout, with the minor to switch to.
    #[test]
    fn a_repository_that_needs_another_php_is_refused_from_its_checkout() {
        let dir = std::env::temp_dir().join(format!("rexenv-require-php-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(php_requirement_refused(&dir, "8.3").is_none(), "no composer.json, nothing to refuse");
        std::fs::write(dir.join("composer.json"), r#"{"require":{"php":"^8.4.1"}}"#).unwrap();
        let (req, why) = php_requirement_refused(&dir, "8.3").expect("8.3 cannot satisfy ^8.4.1");
        assert_eq!(req, "^8.4.1");
        assert!(why.contains("8.4") && why.contains("Retry"), "{why}");
        assert!(php_requirement_refused(&dir, "8.4").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #805 — TEXT: the serve phase asks `stack_guard::may_control_real_stack` BEFORE its one
    /// `start_stack`, so a live-check example (not the app) never starts the real stack. Behaviour
    /// cannot be shown here: under `cfg(test)` the guard always answers "the app".
    #[test]
    fn only_the_app_starts_the_real_stack_from_a_provision() {
        let src = crate::core::copy_scan::production_source(include_str!("site_provision.rs"));
        assert_eq!(src.matches("services::start_stack(").count(), 1, "one stack start in the job");
        let start = src.find("services::start_stack(").unwrap();
        let guard = src.find("stack_guard::may_control_real_stack()").expect("the guard");
        assert!(guard < start, "the stack start moved ahead of the not-the-app guard");
        let between = &src[guard..start];
        assert!(between.contains("return JobEnd::Ok("), "outside the app the job must settle without starting");
    }

    /// The refusal comes BEFORE any database phase (user report, 8 Oct 2026: a `^8.4.1` repo on 8.3
    /// started MySQL and wrote `.env` first). The phases run in source order inside one function,
    /// so the order IS the text: the clone-end check precedes every `"db"` and `"configure"` phase.
    #[test]
    fn the_php_requirement_is_checked_before_the_database_phase() {
        let src = include_str!("site_provision.rs");
        let check = src.find("if let Some((requirement, why)) = php_requirement_refused(&project, &minor)").expect("the clone-end check");
        let first_db = ["phase_index(entry, \"db\")", "phase_index(entry, \"configure\")"]
            .iter()
            .map(|p| src.find(p).expect(p))
            .min()
            .unwrap();
        assert!(check < first_db, "the require.php check moved after a database phase");
        // #802: and the extension check, in the same place.
        let ext = src.find("core::repo::ext_requirement_refused_in(&p2, &m2, &php_bin)").expect("the clone-end ext check");
        assert!(ext < first_db, "the ext-* check moved after a database phase");
    }

    /// The blueprint rule, stated once. Accepting a blueprint for a site that
    /// has no blueprint phase is the dishonest failure — it succeeds and does
    /// nothing — so every non-WordPress combination must be an ERROR whose
    /// message says why, not a quiet no-op.
    #[test]
    fn a_blueprint_is_refused_wherever_no_blueprint_phase_would_run() {
        // The one case that works: a WordPress site whose folder we create.
        assert!(ensure_blueprint_applies(SiteType::Wordpress, "", "", Some("bp-1")).is_ok());

        for ty in [SiteType::Laravel, SiteType::Php] {
            let err = ensure_blueprint_applies(ty, "", "", Some("bp-1")).unwrap_err().to_string();
            assert!(err.contains("WordPress sites only"), "{ty:?}: {err}");
            assert!(err.contains(ty.as_db()), "the message must name the type: {err}");
        }

        // A LINKED WordPress folder is adopted as-is — `phase_defs` gives it no
        // blueprint phase either, and installing into someone's own project is
        // the worse half of the bug.
        let err =
            ensure_blueprint_applies(SiteType::Wordpress, "/Users/x/Sites/theirs", "", Some("bp-1"))
                .unwrap_err()
                .to_string();
        assert!(err.contains("adopts as-is"), "{err}");

        // A CLONED WordPress docroot is somebody's working tree: blueprint
        // plugins would land there as files nobody committed.
        let err = ensure_blueprint_applies(
            SiteType::Wordpress,
            "",
            "https://github.com/acme/site.git",
            Some("bp-1"),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("nobody committed"), "{err}");

        // No blueprint asked for: every type passes, including the ones above.
        for ty in [SiteType::Wordpress, SiteType::Laravel, SiteType::Php] {
            assert!(ensure_blueprint_applies(ty, "", "", None).is_ok());
            assert!(ensure_blueprint_applies(ty, "", "", Some("")).is_ok(), "empty id means none");
            assert!(ensure_blueprint_applies(ty, "/Users/x/theirs", "", None).is_ok());
            assert!(ensure_blueprint_applies(ty, "", "acme/site", None).is_ok());
        }
    }

    /// The guard and the phase list must agree: exactly the shape that ACCEPTS
    /// a blueprint is the shape that gets a blueprint phase to run it in.
    #[test]
    fn the_guard_admits_exactly_the_shapes_phase_defs_gives_a_blueprint_phase() {
        for ty in [SiteType::Wordpress, SiteType::Laravel, SiteType::Php] {
            for linked in [false, true] {
                for from_git in [false, true] {
                    // The three docroot sources are mutually exclusive by
                    // construction; only the two reachable combinations are
                    // enumerated here.
                    if linked && from_git {
                        continue;
                    }
                    let path = if linked { "/Users/x/Sites/theirs" } else { "" };
                    let url = if from_git { "acme/site" } else { "" };
                    let admitted = ensure_blueprint_applies(ty, path, url, Some("bp-1")).is_ok();
                    let has_phase = phase_defs(PhasePlan {
                        site_type: ty,
                        has_blueprint: true,
                        linked,
                        from_git,
                        migrate: true,
                        build_assets: false,
                        starter_db: false,
                    })
                    .iter()
                    .any(|(k, _)| *k == "blueprint");
                    assert_eq!(admitted, has_phase, "{ty:?} linked={linked} git={from_git}");
                }
            }
        }
    }

    /// A managed Laravel site, from a repo or not, with or without migrations.
    fn laravel_plan(from_git: bool, migrate: bool) -> PhasePlan {
        PhasePlan {
            site_type: SiteType::Laravel,
            has_blueprint: false,
            linked: false,
            from_git,
            migrate,
            build_assets: false,
            starter_db: false,
        }
    }

    /// The asset phase belongs to the CLONE, not to Laravel: any repository can
    /// carry a `package.json`, and its build reads whatever the earlier phases
    /// wrote — so it sits last, after everything that could change the code.
    #[test]
    fn front_end_assets_are_a_cloned_site_phase_and_the_last_one_before_serving() {
        let with_assets = PhasePlan { build_assets: true, ..laravel_plan(true, true) };
        let keys: Vec<&str> = phase_defs(with_assets).iter().map(|(k, _)| *k).collect();
        assert_eq!(
            keys,
            vec![
                "prepare", "fetch", "clone", "db", "configure", "deps", "finalize", "assets",
                "serve"
            ]
        );

        // Not asked for → not a phase. A phase that exists only to report
        // "skipped" every time is noise the card has to explain.
        assert!(!phase_defs(laravel_plan(true, true)).iter().any(|(k, _)| *k == "assets"));
        // Not cloned → nothing to build from, whatever the flag says.
        let flagged_but_not_cloned = PhasePlan { build_assets: true, ..laravel_plan(false, true) };
        assert!(!phase_defs(flagged_but_not_cloned).iter().any(|(k, _)| *k == "assets"));

        // A Blank-PHP repo gets the phase too — the build is the repository's,
        // not Laravel's — and its own `deps`, because a Symfony or Craft
        // checkout is `vendor/`-less by design.
        let php = PhasePlan {
            site_type: SiteType::Php,
            build_assets: true,
            ..laravel_plan(true, true)
        };
        let keys: Vec<&str> = phase_defs(php).iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, vec!["prepare", "fetch", "clone", "deps", "assets", "serve"]);
        // No database phase: a Php site has none, so nothing downloads MySQL
        // for a step that will never run.
        assert!(!keys.contains(&"db"));
        // And a Blank-PHP site rexenv merely CREATES has no deps phase — there
        // is no repository to have dependencies.
        let blank = PhasePlan { site_type: SiteType::Php, ..laravel_plan(false, true) };
        assert_eq!(
            phase_defs(blank).iter().map(|(k, _)| *k).collect::<Vec<_>>(),
            vec!["prepare", "fetch", "serve"]
        );
        // The same site with a starter database gains exactly two phases, in
        // the order the job runs them: the engine must answer before anything
        // can be created in it.
        let seeded = PhasePlan { starter_db: true, ..blank };
        assert_eq!(
            phase_defs(seeded).iter().map(|(k, _)| *k).collect::<Vec<_>>(),
            vec!["prepare", "fetch", "db", "configure", "serve"]
        );

        assert!(
            sites::PROVISION_PHASE_WEIGHTS.iter().any(|(k, _)| *k == "assets"),
            "assets has no weight — the bar would guess at a long network phase"
        );
    }

    #[test]
    fn the_two_recorded_choices_default_the_way_older_rows_actually_behaved() {
        let mut site = crate::state::models::test_site(
            "7f3a1c02-9d51-4d2e-8b77-2c9a4e6f1b30",
            "shop.rex",
            crate::state::models::SiteOrigin::User,
        );
        site.site_type = SiteType::Laravel;
        site.git_url = Some("https://github.com/acme/shop.git".into());

        // Both NULL — a row from before either column. Migrations DID run then;
        // a package manager never did. Opposite defaults, both facts.
        assert!(site.runs_migrations());
        assert!(!site.builds_assets());
        let plan = PhasePlan::of(&site, false);
        assert!(plan.migrate && !plan.build_assets);

        site.git_build_assets = Some(true);
        assert!(PhasePlan::of(&site, false).build_assets, "a recorded yes survives a retry");
    }

    /// The cloned Laravel path's phase list, in order — the card renders these
    /// verbatim, and the ORDER is the design: `.env` before dependencies
    /// (composer's post-autoload-dump boots the app), dependencies before
    /// `key:generate`/`migrate` (artisan needs `vendor/`).
    #[test]
    fn a_cloned_laravel_site_configures_before_it_installs_and_installs_before_it_boots() {
        let keys: Vec<&str> =
            phase_defs(laravel_plan(true, true)).iter().map(|(k, _)| *k).collect();
        assert_eq!(
            keys,
            vec!["prepare", "fetch", "clone", "db", "configure", "deps", "finalize", "serve"]
        );
        // A cloned site never runs `composer create-project`: its code arrived
        // from the remote, and create-project refuses a non-empty directory.
        assert!(!keys.contains(&"app_install"));

        // The NEW-app path keeps its shape, one phase longer: migrations were a
        // silent tail of `configure` and are now the same `finalize` phase the
        // cloned path uses.
        let keys: Vec<&str> =
            phase_defs(laravel_plan(false, true)).iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, vec!["prepare", "fetch", "db", "app_install", "configure", "finalize", "serve"]);

        // A LINKED site is adopted whatever else was asked: no clone phase can
        // appear beside a folder rexenv promised not to write into.
        let keys: Vec<&str> =
            phase_defs(PhasePlan {
                site_type: SiteType::Laravel,
                has_blueprint: false,
                linked: true,
                from_git: false,
                migrate: true,
                build_assets: false,
                starter_db: false,
            })
            .iter()
            .map(|(k, _)| *k)
            .collect();
        assert_eq!(keys, vec!["prepare", "fetch", "serve"]);

        // Every phase the cloned path emits has a real weight — an unweighted
        // key silently falls back to 5 and the bar's budget stops matching
        // where the wall-clock actually goes.
        for key in ["clone", "deps", "finalize"] {
            assert!(
                sites::PROVISION_PHASE_WEIGHTS.iter().any(|(k, _)| *k == key),
                "{key} has no weight — the bar would guess"
            );
        }
    }

    /// Declining migrations must change the LABEL, not just the behaviour: a
    /// phase that announces "app key + migrations" and then only generates a
    /// key is the small lie that makes the rest of the card unreadable.
    #[test]
    fn a_phase_never_announces_a_step_the_user_declined() {
        let label = |plan| {
            phase_defs(plan)
                .into_iter()
                .find(|(k, _)| *k == "finalize")
                .map(|(_, l)| l)
                .expect("every Laravel path finalizes")
        };
        assert_eq!(label(laravel_plan(true, true)), "app key + migrations");
        assert_eq!(label(laravel_plan(true, false)), "generating app key");
        // The phase itself stays either way — the app key is not optional.
        assert!(phase_defs(laravel_plan(true, false)).iter().any(|(k, _)| *k == "finalize"));
        // The NEW-app path never offers the choice (nothing asks), so it keeps
        // the unconditional label its NULL column means.
        assert_eq!(label(laravel_plan(false, true)), "running migrations");
    }

    /// The plan is read off the ROW, so a Retry — which rebuilds it from
    /// scratch, possibly in a later process — cannot disagree with the create.
    #[test]
    fn the_phase_plan_comes_from_the_row_so_a_retry_reads_the_same_answer() {
        let mut site = crate::state::models::test_site(
            "7f3a1c02-9d51-4d2e-8b77-2c9a4e6f1b30",
            "shop.rex",
            crate::state::models::SiteOrigin::User,
        );
        site.site_type = SiteType::Laravel;
        site.git_url = Some("https://github.com/acme/shop.git".into());
        site.git_migrate = Some(false);

        let plan = PhasePlan::of(&site, false);
        assert!(plan.from_git);
        assert!(!plan.migrate, "the declined answer survives into a fresh job");
        assert!(phase_defs(plan).iter().any(|(_, l)| *l == "generating app key"));

        // NULL is the unconditional yes every pre-v34 row provably had.
        site.git_migrate = None;
        assert!(PhasePlan::of(&site, false).migrate);
    }

    use crate::platform::traits::{DnsManager, Paths, PrivilegeManager};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    /// Paths rooted in a throwaway temp dir — nothing here touches real app data.
    struct TmpPaths(PathBuf);
    impl Paths for TmpPaths {
        fn app_data_dir(&self) -> crate::error::Result<PathBuf> {
            Ok(self.0.clone())
        }
        fn config_dir(&self) -> crate::error::Result<PathBuf> {
            Ok(self.0.join("config"))
        }
        fn log_dir(&self) -> crate::error::Result<PathBuf> {
            Ok(self.0.join("logs"))
        }
        fn bin_dir(&self) -> crate::error::Result<PathBuf> {
            Ok(self.0.join("bin"))
        }
        fn hosts_file(&self) -> PathBuf {
            self.0.join("hosts")
        }
    }

    /// A resolver file that does NOT exist — `resolver_owner` reads that as
    /// `Absent`, the state that decides whether a prompt happens. This is the
    /// half-onboarded machine the never-prompt rule exists for.
    struct AbsentResolver(PathBuf);
    impl DnsManager for AbsentResolver {
        fn route_label(&self, tld: &str) -> String {
            self.0.join(format!("resolver-{tld}")).display().to_string()
        }
        fn route_contents(&self, port: u16) -> String {
            format!("nameserver 127.0.0.1\nport {port}\n")
        }
        fn route_owner(&self, tld: &str, port: u16) -> crate::platform::traits::ResolverOwner {
            crate::platform::resolver_files::owner_of(&self.0.join(format!("resolver-{tld}")), &self.route_contents(port))
        }
        fn our_route_tlds(&self, port: u16) -> Vec<String> {
            crate::platform::resolver_files::tlds_matching_signature(&self.0, &self.route_contents(port))
        }
        fn foreign_route_tlds(&self, port: u16) -> Vec<String> {
            crate::platform::resolver_files::tlds_not_matching_signature(&self.0, &self.route_contents(port))
        }
        fn install_command(&self, _tld: &str, _port: u16) -> String {
            "true".into()
        }
        fn uninstall_command(&self, _tlds: &[String]) -> String {
            "true".into()
        }
        fn restore_command(&self, _restores: &[(String, PathBuf)]) -> String {
            "true".into()
        }
    }

    /// Records every privileged escalation instead of performing one. An EMPTY
    /// log is the assertion: "nothing an agent can call ever asks macOS for an
    /// administrator password" is the guarantee paragraph's most brittle
    /// sentence (PLAN §4.2/§6.0), and this recorder is what catches it going
    /// false. Returning Ok keeps the path RUNNING past the escalation, so the
    /// test measures whether one was attempted rather than whether it worked.
    #[derive(Default)]
    struct RecordingPrivileges {
        calls: Mutex<Vec<String>>,
    }
    impl PrivilegeManager for RecordingPrivileges {
        fn run_privileged(
            &self,
            script: &str,
            _reason: &crate::platform::traits::PromptReason,
        ) -> crate::error::Result<String> {
            self.calls.lock().unwrap().push(script.to_string());
            Ok(String::new())
        }
    }

    /// Real file ops in the temp dir — `configure_resolver` writes its backup
    /// through this on the USER path, and stubbing it would stop the control
    /// test before it reaches the escalation it exists to observe.
    struct TmpPermissions;
    impl crate::platform::traits::PermissionManager for TmpPermissions {
        fn set_executable(&self, _path: &std::path::Path) -> crate::error::Result<()> {
            Ok(())
        }
        fn set_private(&self, _path: &std::path::Path) -> crate::error::Result<()> {
            Ok(())
        }
        fn write_private(
            &self,
            path: &std::path::Path,
            contents: &[u8],
        ) -> crate::error::Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(path, contents)?;
            Ok(())
        }
    }

    struct ProvisionTestPlatform {
        paths: TmpPaths,
        dns: AbsentResolver,
        privileges: Arc<RecordingPrivileges>,
    }

    impl crate::platform::traits::Platform for ProvisionTestPlatform {
        fn paths(&self) -> &dyn Paths {
            &self.paths
        }
        fn dns(&self) -> &dyn DnsManager {
            &self.dns
        }
        fn privileges(&self) -> &dyn PrivilegeManager {
            self.privileges.as_ref()
        }
        fn edge(&self) -> &dyn crate::platform::traits::EdgeSupervisor {
            unimplemented!("the create path must not reach the edge")
        }
        fn cert_trust(&self) -> &dyn crate::platform::traits::CertTrustManager {
            // Installing a resolver asks where Firefox keeps its profiles (the typed-address
            // pref, ledger #706); this one has none, so that step is a no-op.
            struct NoFirefox;
            impl crate::platform::traits::CertTrustManager for NoFirefox {
                fn trust_ca(&self, _: &std::path::Path) -> crate::error::Result<()> {
                    unimplemented!()
                }
                fn untrust_ca(&self, _: &std::path::Path) -> crate::error::Result<()> {
                    unimplemented!()
                }
            }
            &NoFirefox
        }
        fn supervisor(&self) -> &dyn crate::platform::traits::ProcessSupervisor {
            unimplemented!()
        }
        fn autostart(&self) -> &dyn crate::platform::traits::AutostartManager {
            unimplemented!()
        }
        fn permissions(&self) -> &dyn crate::platform::traits::PermissionManager {
            &TmpPermissions
        }
        fn shell(&self) -> &dyn crate::platform::traits::ShellRunner {
            unimplemented!()
        }
        fn binaries(&self) -> &dyn crate::platform::traits::BinaryProvider {
            unimplemented!()
        }
        fn dns_agent(&self) -> &dyn crate::platform::traits::DnsAgentManager {
            unimplemented!()
        }
        fn app_bundle(&self) -> &dyn crate::platform::traits::AppBundle { unimplemented!() }
    }

    /// Drive the REAL `start()` — the whole prepare phase, not a precheck —
    /// against a machine whose resolver file is missing. Returns the outcome and
    /// every privileged escalation the path attempted.
    fn run_start(ownership: Ownership) -> (crate::error::Result<SiteProvisionState>, Vec<String>) {
        let dir = std::env::temp_dir()
            .join(format!("rexenv-provision-prompt-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let recorder = Arc::new(RecordingPrivileges::default());
        let platform = ProvisionTestPlatform {
            paths: TmpPaths(dir.clone()),
            dns: AbsentResolver(dir.clone()),
            privileges: Arc::clone(&recorder),
        };
        let conn = crate::state::db::open_in_memory().unwrap();
        let ca = crate::core::ssl::LocalCa {
            cert_pem: String::new(),
            key_pem: String::new(),
            cert_path: dir.join("ca.pem"),
            key_path: dir.join("ca.key"),
        };
        let app = tauri::test::mock_app();
        let state = AppState::new(conn, Box::new(platform), ca);
        let jobs = ProvisionJobs::default();
        let new = NewSite {
            name: "probe".into(),
            domain: "probe.scratch.rex".into(),
            site_type: SiteType::Wordpress,
            php_version: "8.3".into(),
            web_server: crate::state::models::WebServer::Nginx,
            path: String::new(),
            db_engine: crate::state::models::SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
            git_migrate: true,
            git_build_assets: false,
            starter_db: false,
        };
        let out = start(&app.handle().clone(), &state, &jobs, new, None, None, ownership);
        let calls = recorder.calls.lock().unwrap().clone();
        let _ = std::fs::remove_dir_all(&dir);
        (out, calls)
    }

    #[test]
    fn an_agent_create_never_reaches_a_privileged_prompt() {
        // THE guarantee paragraph's most brittle sentence, driven end to end
        // through the real provision path rather than asserted at an entry
        // check: resolver absent, agent ownership, and the recorder must stay
        // empty. If the policy stopped being derived from ownership — or a
        // future call site defaulted to prompting — `calls` fills and this
        // fails. (Proven load-bearing: with `Ownership::User` the same path
        // DOES escalate, see the sibling test.)
        let (out, calls) = run_start(Ownership::Agent { client: "Claude Code".into(), ttl_hours: 24 });
        assert!(calls.is_empty(), "an agent create attempted a privileged escalation: {calls:?}");
        let err = out.expect_err("a missing resolver must FAIL the agent path").to_string();
        assert!(err.contains("administrator password"), "says why it stopped: {err}");
        assert!(err.contains("finish its setup"), "names the USER's action: {err}");
        assert!(!err.contains("try a different"), "never suggests routing around it: {err}");
    }

    #[test]
    fn the_same_path_does_prompt_for_a_user_create_so_the_flag_is_what_stops_it() {
        // The control. Without it, `an_agent_create_never_reaches_a_privileged_prompt`
        // could pass because this fixture cannot escalate at all — which would
        // make it decoration rather than a guard. Same platform, same missing
        // resolver, ownership the only difference.
        let (_out, calls) = run_start(Ownership::User);
        assert!(
            !calls.is_empty(),
            "the user path must still install the resolver — otherwise the agent test proves nothing"
        );
        assert!(calls[0].contains("resolver") || !calls[0].is_empty(), "escalated to install it");
    }

    /// **`settle` answers with the job's SETTLED state — not the snapshot it
    /// found — and reports every phase/percentage change on the way.** The
    /// shape of the defect it fixes: the MCP retry returned `running` with every
    /// phase pending, because only the create path had a wait.
    #[tokio::test]
    async fn settle_waits_for_the_job_to_leave_running_and_reports_each_change() {
        let jobs = ProvisionJobs::default();
        let entry = Arc::new(ProvisionEntry {
            id: "job-settle".into(),
            seq: 0,
            domain: "settle.rex".into(),
            wp_opts: Default::default(),
            blueprint: None,
            cancel: repo::CancelToken::new(),
            running: AtomicBool::new(true),
            timed_out: AtomicBool::new(false),
            log_path: std::env::temp_dir().join("rexenv-settle-probe.log"),
            state: Mutex::new(SiteProvisionState {
                id: "job-settle".into(),
                domain: "settle.rex".into(),
                site_id: Some("site-settle".into()),
                phases: Vec::new(),
                phase_cursor: 1,
                pct: 10,
                status: "running".into(),
                summary: None,
                error: None,
                log_key: "settle".into(),
                download_ids: Vec::new(),
                serving_blocked: false,
                serving_holder: None,
                serving_app: None,
                assets_warning: None,
            }),
        });
        jobs.jobs.lock().unwrap().insert("job-settle".into(), entry.clone());

        let worker = entry.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            {
                let mut s = worker.state.lock().unwrap();
                s.phase_cursor = 2;
                s.pct = 60;
            }
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            let mut s = worker.state.lock().unwrap();
            s.pct = 100;
            s.status = "ok".into();
        });

        let seen = std::sync::Mutex::new(Vec::new());
        let settled = {
            let report = |st: &SiteProvisionState| seen.lock().unwrap().push((st.phase_cursor, st.pct));
            settle(&jobs, "job-settle", Some(&report)).await.unwrap()
        };
        assert_eq!(settled.status, "ok", "the settled state, not the first snapshot");
        assert_eq!(settled.pct, 100);
        let seen = seen.into_inner().unwrap();
        assert_eq!(seen.first(), Some(&(1, 10)), "the starting state is reported");
        assert!(seen.contains(&(2, 60)), "the phase change is reported: {seen:?}");
        assert_eq!(seen.last(), Some(&(2, 100)), "and the end: {seen:?}");
        assert!(settle(&jobs, "no-such-job", None).await.is_err(), "an unknown job is an error, not a wait");
    }
}

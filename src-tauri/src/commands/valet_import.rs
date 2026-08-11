//! commands::valet_import — thin IPC over `core::valet` + the resolver
//! takeover. Call `core/` only.
//!
//! The scan itself is pure filesystem (`core::valet`); this layer adds the
//! judgements that need rexenv's own state — is the domain already ours, does
//! the folder overlap an existing site, do we actually ship that PHP version —
//! and answers the resolver question per TLD so the screen can ask for consent
//! before anything is created.

use crate::core;
use crate::core::valet::{DiscoveredSite, SiteStatus, Source, SourceKind};
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::models::SiteType;
use tauri::State;

fn lock<'a>(
    state: &'a State<'a, AppState>,
) -> Result<std::sync::MutexGuard<'a, rusqlite::Connection>> {
    state
        .db
        .lock()
        .map_err(|_| Error::Other("database lock poisoned".into()))
}

/// One reviewable row: what we found, plus what importing it would actually do.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportCandidate {
    pub source: SourceKind,
    pub name: String,
    pub domain: String,
    /// Their project folder.
    pub path: Option<String>,
    /// The folder we would SERVE — often a subfolder of the above, since a
    /// framework's docroot is rarely its project root.
    pub serve_path: Option<String>,
    pub docroot_rel: Option<String>,
    pub site_type: Option<SiteType>,
    /// Framework name for display ("WordPress", "Laravel", …).
    pub label: Option<String>,
    /// The PHP minor they pinned, exactly as their config says.
    pub php_minor: Option<String>,
    /// The minor we'd use. `None` when theirs isn't one we ship and the user
    /// must choose — we never substitute silently.
    pub php_target: Option<String>,
    pub secured: bool,
    pub proxy_to: Option<String>,
    pub also_in: Option<SourceKind>,
    pub has_custom_valet_driver: bool,
    pub status: SiteStatus,
}

/// Whether a TLD's OS resolver file is ours, theirs, or missing.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolverTldStatus {
    pub tld: String,
    /// `absent` · `ours` · `borrowed` (ours, taken from them) · `foreign`
    /// (theirs) · `drifted` (we borrowed it, they took it back).
    pub owner: String,
    pub path: String,
    /// Their file, verbatim, so the consent panel can show it beside ours.
    pub their_content: Option<String>,
    pub our_content: String,
    /// rexenv sites already on this TLD — the hand-back warning needs the count.
    pub rexenv_sites: usize,
}

/// Everything the migration screen needs in one read-only call.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportScan {
    pub sources: Vec<Source>,
    pub candidates: Vec<ImportCandidate>,
    pub tlds: Vec<ResolverTldStatus>,
    /// PHP minors we ship, for the "not available — pick one" control.
    pub available_php: Vec<String>,
}

/// Scan for Valet/Herd sites. Read-only: nothing of theirs is written, started
/// or stopped, and no file inside a project is opened.
#[tauri::command]
pub fn scan_valet_import(state: State<'_, AppState>) -> Result<ImportScan> {
    let conn = lock(&state)?;
    let platform = state.platform.as_ref();
    let home = directories::BaseDirs::new()
        .ok_or_else(|| Error::Other("could not resolve the home directory".into()))?
        .home_dir()
        .to_path_buf();

    let found = core::valet::discover(&home);
    let available: Vec<String> = core::php::all_minors();
    let existing = core::sites::list(&conn)?;

    let candidates = found
        .sites
        .into_iter()
        .map(|s| enrich(&conn, platform, &existing, &available, s))
        .collect::<Vec<_>>();

    // Every TLD any importable row is served on — including one that appears
    // only in a stray conf, which the config never mentions.
    let mut tld_names: Vec<String> = candidates
        .iter()
        .filter(|c| !matches!(c.status, SiteStatus::Unsupported(_)))
        .filter_map(|c| c.domain.rsplit_once('.').map(|(_, t)| t.to_string()))
        .collect();
    tld_names.sort();
    tld_names.dedup();

    let tlds = tld_names
        .into_iter()
        .map(|tld| resolver_status_for(&conn, platform, &existing, &tld))
        .collect();

    Ok(ImportScan { sources: found.sources, candidates, tlds, available_php: available })
}

/// Add the judgements that need our own state to one scanned row.
fn enrich(
    conn: &rusqlite::Connection,
    platform: &dyn crate::platform::traits::Platform,
    existing: &[crate::state::models::Site],
    available: &[String],
    s: DiscoveredSite,
) -> ImportCandidate {
    let mut c = ImportCandidate {
        source: s.source,
        name: s.name,
        domain: s.domain,
        path: s.path.clone(),
        serve_path: None,
        docroot_rel: None,
        site_type: None,
        label: None,
        php_minor: s.php_minor.clone(),
        php_target: None,
        secured: s.secured,
        proxy_to: s.proxy_to,
        also_in: s.also_in,
        has_custom_valet_driver: false,
        status: s.status,
    };

    // A filesystem-level refusal (missing folder, proxy) already decided this.
    if matches!(c.status, SiteStatus::Unsupported(_)) {
        return c;
    }
    if existing.iter().any(|e| e.domain.eq_ignore_ascii_case(&c.domain)) {
        c.status = SiteStatus::AlreadyImported;
        return c;
    }

    let Some(path) = c.path.clone() else {
        c.status = SiteStatus::Unsupported("its folder is missing".into());
        return c;
    };
    let root = std::path::PathBuf::from(&path);
    let detected = core::sites::detect_project(&root);
    let serve = if detected.docroot_rel.is_empty() {
        root.clone()
    } else {
        root.join(&detected.docroot_rel)
    };
    c.docroot_rel = Some(detected.docroot_rel.clone());
    c.site_type = Some(detected.site_type);
    c.label = Some(detected.label.to_string());
    c.has_custom_valet_driver = core::sites::has_custom_valet_driver(&root);

    // The served folder is what gets stored, so it must pass the link
    // preflight — overlap with an existing site, blast radius, our app data.
    match core::sites::validate_linked_docroot(conn, platform, &serve.display().to_string()) {
        Ok(canon) => c.serve_path = Some(canon.display().to_string()),
        Err(e) => {
            c.status = SiteStatus::NeedsAttention(e.to_string());
            return c;
        }
    }

    // PHP: map their pin onto a version we ship, or ask. Never substitute
    // silently — and never write an unpinned version, which would leave a
    // half-provisioned site the user has to clean up.
    match &c.php_minor {
        Some(m) if available.iter().any(|a| a == m) => c.php_target = Some(m.clone()),
        Some(m) => {
            c.status = SiteStatus::NeedsAttention(format!(
                "PHP {m} isn't one rexenv ships — choose a version to import it on"
            ));
            return c;
        }
        // No pin means they used their global PHP, so there is nothing to
        // honour — our default is as good a choice as any, and the row stays
        // importable rather than nagging for a decision that doesn't exist.
        None => {
            c.php_target = Some(core::php::minor_of(core::binaries::PHP_VERSION));
        }
    }

    if c.has_custom_valet_driver {
        c.status = SiteStatus::NeedsAttention(
            "this project has a LocalValetDriver.php, which picks its document root by \
             running PHP — check the folder rexenv detected is the one Valet served"
                .into(),
        );
    }
    c
}

fn resolver_status_for(
    conn: &rusqlite::Connection,
    platform: &dyn crate::platform::traits::Platform,
    existing: &[crate::state::models::Site],
    tld: &str,
) -> ResolverTldStatus {
    let port = core::dns::DEFAULT_DNS_PORT;
    let borrowed = crate::state::store::get_resolver_takeover(conn, tld).ok().flatten();
    let (owner, their) = match core::dns::resolver_owner(platform, tld, port) {
        core::dns::ResolverOwner::Absent => ("absent", None),
        core::dns::ResolverOwner::Ours if borrowed.is_some() => ("borrowed", None),
        core::dns::ResolverOwner::Ours => ("ours", None),
        core::dns::ResolverOwner::Foreign { content } if borrowed.is_some() => {
            ("drifted", content)
        }
        core::dns::ResolverOwner::Foreign { content } => ("foreign", content),
    };
    ResolverTldStatus {
        tld: tld.to_string(),
        owner: owner.to_string(),
        path: platform.dns().resolver_path(tld).display().to_string(),
        their_content: their.or_else(|| borrowed.map(|b| b.original)),
        our_content: platform.dns().resolver_contents(port),
        rexenv_sites: existing.iter().filter(|s| s.domain.ends_with(&format!(".{tld}"))).count(),
    }
}

/// Take a TLD's resolver file over from Valet/Herd, backing theirs up first.
/// Consent lives in the UI; this is the operation it authorises.
#[tauri::command]
pub async fn resolver_take_over(state: State<'_, AppState>, tld: String) -> Result<()> {
    let conn = lock(&state)?;
    core::dns::take_over_resolver(
        &conn,
        state.platform.as_ref(),
        &tld,
        core::dns::DEFAULT_DNS_PORT,
    )
}

/// Give a borrowed resolver file back.
#[tauri::command]
pub async fn resolver_hand_back(
    state: State<'_, AppState>,
    tld: String,
) -> Result<core::dns::ResolverPlan> {
    let conn = lock(&state)?;
    core::dns::hand_back_resolver(
        &conn,
        state.platform.as_ref(),
        &tld,
        core::dns::DEFAULT_DNS_PORT,
    )
}

/// TLDs we borrowed whose file another tool has since reclaimed — the silent
/// failure where our sites stop resolving while every health check stays green.
#[tauri::command]
pub fn resolver_drift(state: State<'_, AppState>) -> Result<Vec<String>> {
    let conn = lock(&state)?;
    Ok(core::dns::drifted_takeovers(
        &conn,
        state.platform.as_ref(),
        core::dns::DEFAULT_DNS_PORT,
    ))
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

/// Cancel flag for a running import. One import at a time; the flag is checked
/// BETWEEN sites, never mid-site.
#[derive(Default)]
pub struct ImportJobs {
    running: std::sync::atomic::AtomicBool,
    cancel: std::sync::atomic::AtomicBool,
}

/// What the user asked us to import.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRequest {
    /// Domains to import, in the order the screen listed them.
    pub domains: Vec<String>,
    /// Per-domain PHP minor, for rows where the user had to choose because we
    /// don't ship the version they pinned.
    #[serde(default)]
    pub php: std::collections::HashMap<String, String>,
    /// After each site imports, run the SAME per-site database import job for
    /// it. The SCREEN ticks this by default (rexenv is the whole stack, so a
    /// migration is a database migration too); the wire default stays off so a
    /// caller that never mentions databases never copies one. Either way the
    /// copy is a READ of theirs — the old database is never written or moved.
    #[serde(default)]
    pub import_databases: bool,
}

/// What happened to one row. Terminal — every requested domain gets exactly one.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutcome {
    pub domain: String,
    /// `imported` · `failed` · `skipped` (not importable, or cancelled before
    /// we reached it).
    pub status: String,
    pub reason: Option<String>,
    pub site_id: Option<String>,
    /// The job log, so a failure is diagnosable rather than just red.
    pub log_key: Option<String>,
    /// Database outcome when `import_databases` was on: `imported` · `failed` ·
    /// `skipped` (site import failed, or the site has no database to read).
    /// Carries the honest reason after a colon.
    pub db: Option<String>,
}

/// The end-of-run summary.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub outcomes: Vec<ImportOutcome>,
    pub imported: usize,
    pub failed: usize,
    pub skipped: usize,
    /// Databases that came over / didn't, when `import_databases` was on.
    pub db_imported: usize,
    pub db_failed: usize,
    /// Checked ONCE at the end: something else answers :443, so nothing
    /// imported will load until it lets go.
    pub serving_blocked: bool,
}

fn import_event() -> &'static str {
    "valet-import://row"
}

fn progress_event() -> &'static str {
    "valet-import://progress"
}

/// Where the batch is RIGHT NOW — the screen's only in-flight signal.
///
/// Honesty contract (the provision-card family):
/// - every field comes from work that actually happened: `done` counts terminal
///   rows, `site_pct` is the running job's OWN backend-computed pct, `detail` is
///   that job's own step label verbatim — nothing here is a time estimate,
/// - `pct` is monotonic and capped at 99 until the batch settles,
/// - a failure freezes the bar where the work stopped: the failed row still
///   counts as done, so the bar advances by real completions only.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    /// Rows the user asked for — the denominator, fixed for the whole run.
    pub total: usize,
    /// Rows with a terminal outcome (imported, failed or skipped).
    pub done: usize,
    /// 1-based position of the site being worked on; 0 during the shared
    /// preparation steps that belong to no single site.
    pub index: usize,
    pub domain: Option<String>,
    /// `scanning` · `resolvers` · `php` · `site` · `database` · `checking` ·
    /// `done`.
    pub stage: String,
    /// The running job's own step label, verbatim.
    pub detail: Option<String>,
    /// The current site's own fraction, 0..100 (its provision job, plus its
    /// database job when databases were requested).
    pub site_pct: u8,
    /// The whole batch, 0..100.
    pub pct: u8,
}

/// The batch bar: settled rows plus the in-flight site's own fraction, each
/// row weighted equally. Capped at 99 — only the settle emits 100, so the bar
/// can never claim a finished batch while a site is still running.
fn batch_pct(done: usize, site_pct: u8, total: usize) -> u8 {
    if total == 0 {
        return 0;
    }
    let units = done as u32 * 100 + site_pct as u32;
    (units / total as u32).min(99) as u8
}

/// Emits batch progress. Owns the two facts a single tick can't know on its
/// own: how many rows have settled, and how far the bar has already come.
struct Batch<'a, R: tauri::Runtime> {
    app: &'a tauri::AppHandle<R>,
    total: usize,
    done: usize,
    /// Databases were requested, so each site's bar is shared between its
    /// provision job and its database job.
    with_db: bool,
    /// Highest pct emitted — the bar never rolls back.
    last: std::sync::atomic::AtomicU8,
}

/// The share of one site's bar that its provision job owns when a database
/// import follows it. The rest is the database's.
const SITE_SHARE_WITH_DB: u16 = 60;

impl<R: tauri::Runtime> Batch<'_, R> {
    fn tick(
        &self,
        stage: &str,
        index: usize,
        domain: Option<&str>,
        detail: Option<String>,
        site_pct: u8,
    ) {
        use tauri::Emitter;
        let pct = batch_pct(self.done, site_pct, self.total);
        let pct = if stage == "done" {
            100
        } else {
            pct.max(self.last.load(std::sync::atomic::Ordering::SeqCst))
        };
        self.last.store(pct, std::sync::atomic::Ordering::SeqCst);
        let _ = self.app.emit(
            progress_event(),
            ImportProgress {
                total: self.total,
                done: self.done,
                index,
                domain: domain.map(str::to_string),
                stage: stage.to_string(),
                detail,
                site_pct,
                pct,
            },
        );
    }

    fn provision_share(&self, pct: u8) -> u8 {
        provision_share(pct, self.with_db)
    }

    fn db_share(&self, pct: u8) -> u8 {
        db_share(pct)
    }
}

/// The provision job's pct scaled into the site's share of the bar. With a
/// database to follow it, a finished provision is 60 of the site's 100 — "site
/// created" is not "site finished".
fn provision_share(pct: u8, with_db: bool) -> u8 {
    if with_db {
        (pct as u16 * SITE_SHARE_WITH_DB / 100) as u8
    } else {
        pct
    }
}

/// The database job's pct, scaled into the rest of the site's share.
fn db_share(pct: u8) -> u8 {
    (SITE_SHARE_WITH_DB + pct as u16 * (100 - SITE_SHARE_WITH_DB) / 100) as u8
}

/// Import the selected Valet/Herd sites, one at a time.
///
/// Sequential by design: the download hub has a single batch slot, each serve
/// phase takes the services lock for a full edge reload, and job adoption only
/// ever tracks the newest job — running these in parallel fights all three.
///
/// **Continue on failure.** Each site is independent, unlike a build that
/// follows an install, so a failure on site 3 must not cost sites 4 through 20.
/// Every requested domain gets exactly one terminal outcome, and the summary
/// names which succeeded and which didn't.
///
/// **Cancel stops AFTER the current site**, never mid-site: abandoning a site
/// halfway is what leaves the `provisioned=0` half-state users then clean up by
/// hand. Remaining rows come back as `skipped`.
#[tauri::command]
pub async fn valet_import_run<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    jobs: State<'_, ImportJobs>,
    provision: State<'_, crate::commands::site_provision::ProvisionJobs>,
    db_jobs: State<'_, crate::commands::db_import::DbImportJobs>,
    request: ImportRequest,
) -> Result<ImportResult> {
    use std::sync::atomic::Ordering;
    use tauri::Emitter;

    if jobs.running.swap(true, Ordering::SeqCst) {
        return Err(Error::Other("an import is already running".into()));
    }
    jobs.cancel.store(false, Ordering::SeqCst);
    let done = scopeguard(|| jobs.running.store(false, Ordering::SeqCst));

    let mut batch = Batch {
        app: &app,
        total: request.domains.len(),
        done: 0,
        with_db: request.import_databases,
        last: std::sync::atomic::AtomicU8::new(0),
    };
    batch.tick("scanning", 0, None, Some("re-reading your Valet and Herd setup".into()), 0);

    // Re-scan rather than trusting the list we were handed: the screen's rows
    // are a suggestion, and the folders may have changed since it rendered.
    let scan = scan_valet_import(state.clone())?;
    let mut outcomes: Vec<ImportOutcome> = Vec::new();
    let mut queue: Vec<(ImportCandidate, String)> = Vec::new();

    for domain in &request.domains {
        let Some(c) = scan.candidates.iter().find(|c| &c.domain == domain).cloned() else {
            outcomes.push(skipped(domain, "it's no longer in Valet/Herd"));
            continue;
        };
        match &c.status {
            SiteStatus::Unsupported(r) | SiteStatus::NeedsAttention(r)
                if c.serve_path.is_none() =>
            {
                outcomes.push(skipped(domain, r));
                continue;
            }
            SiteStatus::AlreadyImported => {
                outcomes.push(skipped(domain, "rexenv already serves it"));
                continue;
            }
            _ => {}
        }
        let Some(serve) = c.serve_path.clone() else {
            outcomes.push(skipped(domain, "its folder is missing"));
            continue;
        };
        // The user's explicit choice wins for a version we don't ship.
        let php = request
            .php
            .get(domain)
            .cloned()
            .or_else(|| c.php_target.clone())
            .unwrap_or_else(|| core::php::minor_of(core::binaries::PHP_VERSION));
        if !scan.available_php.contains(&php) {
            outcomes.push(skipped(domain, &format!("PHP {php} isn't one rexenv ships")));
            continue;
        }
        let _ = serve;
        queue.push((c, php));
    }

    // Resolver files first, so any password prompt happens at ONE predictable
    // moment instead of surprising the user midway through the batch.
    let mut tlds: Vec<String> = queue
        .iter()
        .filter_map(|(c, _)| c.domain.rsplit_once('.').map(|(_, t)| t.to_string()))
        .collect();
    tlds.sort();
    tlds.dedup();
    // Rows the queue already rejected are terminal — the bar starts where the
    // real work does, not at zero.
    batch.done = outcomes.len();
    for tld in &tlds {
        batch.tick("resolvers", 0, None, Some(format!("making .{tld} resolve to rexenv")), 0);
        match core::dns::resolver_owner(
            state.platform.as_ref(),
            tld,
            core::dns::DEFAULT_DNS_PORT,
        ) {
            core::dns::ResolverOwner::Ours => {}
            core::dns::ResolverOwner::Absent => {
                core::dns::configure_resolver(
                    state.platform.as_ref(),
                    tld,
                    core::dns::DEFAULT_DNS_PORT,
                )?;
            }
            // The screen asks for consent before getting here; refusing beats
            // quietly taking a file we were never given permission to take.
            core::dns::ResolverOwner::Foreign { .. } => {
                return Err(Error::Other(format!(
                    ".{tld} is still managed by Valet or Herd. Hand that TLD to rexenv \
                     (their file is backed up and can be handed back) or import these \
                     sites on .rex instead."
                )))
            }
        }
    }

    // PHP registry BEFORE any create: a site on a minor that isn't marked
    // installed serves once and then dies at the next Start-all — and can't be
    // cleaned up afterwards, because removal refuses a minor a site is using.
    let mut minors: Vec<String> = queue.iter().map(|(_, p)| p.clone()).collect();
    minors.sort();
    minors.dedup();
    {
        let conn = lock(&state)?;
        for m in &minors {
            core::php::set_installed(&conn, m, true)?;
        }
    }
    // Unlocked, before anything takes the services lock (the prefetch-before-lock
    // invariant), and once per minor rather than per site.
    for m in &minors {
        batch.tick("php", 0, None, Some(format!("getting PHP {m} ready")), 0);
        let plan = core::downloads::plan_for_php(state.platform.as_ref(), m);
        core::downloads::prefetch(state.platform.as_ref(), &format!("Import (PHP {m})"), &plan)
            .await?;
    }

    for (i, (c, php)) in queue.into_iter().enumerate() {
        let index = i + 1;
        if jobs.cancel.load(Ordering::SeqCst) {
            let row = skipped(&c.domain, "cancelled before this site was started");
            let _ = app.emit(import_event(), row.clone());
            outcomes.push(row);
            batch.done += 1;
            continue;
        }
        batch.tick("site", index, Some(&c.domain), Some("starting".into()), 0);
        let mut row = import_one(&app, &state, &provision, &c, &php, &batch, index).await;
        // Opt-in database import, per site, CONTINUE ON FAILURE exactly like
        // the sites themselves: a database that won't come over must not cost
        // the rest of the batch, and every row states what happened to its
        // database by name.
        if request.import_databases {
            row.db = Some(match (&row.status[..], &row.site_id) {
                ("imported", Some(site_id)) => {
                    import_db_for(
                        &app, &state, &db_jobs, &provision, site_id, &batch, index,
                        &row.domain,
                    )
                    .await
                }
                _ => "skipped: the site itself didn't import".to_string(),
            });
        }
        let _ = app.emit(import_event(), row.clone());
        outcomes.push(row);
        batch.done += 1;
    }

    batch.tick("checking", 0, None, Some("checking your sites will load".into()), 0);
    // ONE probe for the whole batch: per-site would add seconds each.
    let serving_blocked = !core::proxy::edge_answers_as_ours(
        core::adminer::ADMINER_HOST,
        core::proxy::DEFAULT_HTTPS_PORT,
    )
    .await;

    batch.tick("done", 0, None, None, 0);
    drop(done);
    let imported = outcomes.iter().filter(|o| o.status == "imported").count();
    let failed = outcomes.iter().filter(|o| o.status == "failed").count();
    let skipped_n = outcomes.iter().filter(|o| o.status == "skipped").count();
    let db_imported = outcomes
        .iter()
        .filter(|o| o.db.as_deref().is_some_and(|d| d == "imported"))
        .count();
    let db_failed = outcomes
        .iter()
        .filter(|o| o.db.as_deref().is_some_and(|d| d.starts_with("failed")))
        .count();
    Ok(ImportResult {
        outcomes,
        imported,
        failed,
        skipped: skipped_n,
        db_imported,
        db_failed,
        serving_blocked: serving_blocked && imported > 0,
    })
}

/// Run the ONE database-import job for a freshly imported site and wait for it
/// to settle. The same job the SiteDetail button starts — no parallel
/// implementation to drift (the import_one rule, applied again).
#[allow(clippy::too_many_arguments)]
async fn import_db_for<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &State<'_, AppState>,
    db_jobs: &State<'_, crate::commands::db_import::DbImportJobs>,
    provision: &State<'_, crate::commands::site_provision::ProvisionJobs>,
    site_id: &str,
    batch: &Batch<'_, R>,
    index: usize,
    domain: &str,
) -> String {
    // A site with no database config is the common non-WP case — an honest
    // skip, not a failure.
    let site = {
        let Ok(conn) = state.db.lock() else { return "failed: database lock".into() };
        match crate::core::sites::get(&conn, site_id) {
            Ok(Some(s)) => s,
            _ => return "failed: site not found".into(),
        }
    };
    if let Err((reason, _)) =
        crate::core::dbimport::read_connection(std::path::Path::new(&site.path))
    {
        return format!("skipped: {}", crate::core::dbimport::DbSiteStatus::NeedsAttention {
            reason,
            source: None,
        }
        .message());
    }
    use tauri::Manager as _;
    let Some(tunnels) = app.try_state::<crate::commands::tunnels::Tunnels>() else {
        return "failed: tunnel registry not ready".into();
    };
    let start = crate::commands::db_import::db_import_start(
        app.clone(),
        state.clone(),
        db_jobs.clone(),
        provision.clone(),
        tunnels,
        site_id.to_string(),
        None,
    )
    .await;
    let snap = match start {
        Ok(s) => s,
        Err(e) => return format!("failed: {e}"),
    };
    batch.tick(
        "database",
        index,
        Some(domain),
        Some("copying the database".into()),
        batch.db_share(0),
    );
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        match crate::commands::db_import::db_import_state(db_jobs.clone(), site_id.to_string()) {
            Ok(Some(st)) if st.id == snap.id && st.status == "running" => {
                let label = st.phases.get(st.phase_cursor).map(|p| p.label.clone());
                batch.tick("database", index, Some(domain), label, batch.db_share(st.pct));
                continue;
            }
            Ok(Some(st)) if st.id == snap.id && st.status != "running" => {
                return match st.status.as_str() {
                    "ok" => "imported".into(),
                    "cancelled" => "failed: cancelled".into(),
                    _ => format!(
                        "failed: {}",
                        st.error.unwrap_or_else(|| "see the job log".into())
                    ),
                };
            }
            Ok(_) => continue,
            Err(e) => return format!("failed: {e}"),
        }
    }
}

/// Stop after the site currently being imported.
#[tauri::command]
pub fn valet_import_cancel(jobs: State<'_, ImportJobs>) {
    jobs.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
}

fn skipped(domain: &str, reason: &str) -> ImportOutcome {
    ImportOutcome {
        domain: domain.to_string(),
        status: "skipped".into(),
        reason: Some(reason.to_string()),
        site_id: None,
        log_key: None,
        db: None,
    }
}

/// Import ONE site through the ordinary create path — same job, same phases,
/// same log. There is no parallel import implementation to drift.
async fn import_one<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &State<'_, AppState>,
    provision: &State<'_, crate::commands::site_provision::ProvisionJobs>,
    c: &ImportCandidate,
    php: &str,
    batch: &Batch<'_, R>,
    index: usize,
) -> ImportOutcome {
    let site = crate::state::models::NewSite {
        name: c.name.clone(),
        domain: c.domain.clone(),
        site_type: c.site_type.unwrap_or(SiteType::Php),
        php_version: php.to_string(),
        web_server: crate::state::models::WebServer::Nginx,
        // Non-empty = LINK: served where it already lives, never written into,
        // never deleted with the site.
        path: c.serve_path.clone().unwrap_or_default(),
        db_engine: crate::state::models::SiteDbEngine::Mysql,
        // An import adopts what is already on disk — it never fetches code.
        git_url: String::new(),
        git_ref: None,
        git_migrate: true,
        git_build_assets: false,
    };
    let snap = match crate::commands::site_provision::start(
        app,
        state,
        provision,
        site,
        None,
        None,
        // An import is the USER adopting their own Valet/Herd sites.
        crate::core::sites::Ownership::User,
    )
    {
        Ok(s) => s,
        Err(e) => {
            return ImportOutcome {
                domain: c.domain.clone(),
                status: "failed".into(),
                reason: Some(e.to_string()),
                site_id: None,
                log_key: None,
                db: None,
            }
        }
    };
    let settled = loop {
        match crate::commands::site_provision::state_of(provision, &snap.id) {
            Ok(st) if st.status != "running" => break st,
            Ok(st) => {
                let label = st.phases.get(st.phase_cursor).map(|p| p.label.clone());
                batch.tick(
                    "site",
                    index,
                    Some(&c.domain),
                    label,
                    batch.provision_share(st.pct),
                );
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            }
            Err(e) => {
                return ImportOutcome {
                    domain: c.domain.clone(),
                    status: "failed".into(),
                    reason: Some(e.to_string()),
                    site_id: None,
                    log_key: Some(snap.log_key.clone()),
                    db: None,
                }
            }
        }
    };
    let ok = settled.status == "ok";
    ImportOutcome {
        domain: c.domain.clone(),
        status: if ok { "imported".into() } else { "failed".into() },
        reason: if ok {
            None
        } else {
            Some(settled.error.clone().unwrap_or_else(|| {
                let phase = settled
                    .phases
                    .get(settled.phase_cursor.min(settled.phases.len().saturating_sub(1)))
                    .map(|p| p.label.clone())
                    .unwrap_or_default();
                format!("{} at: {phase}", settled.status)
            }))
        },
        site_id: settled.site_id.clone(),
        log_key: Some(settled.log_key.clone()),
        db: None,
    }
}

/// Minimal RAII so the running flag clears on every exit path.
fn scopeguard<F: FnOnce()>(f: F) -> impl Drop {
    struct G<F: FnOnce()>(Option<F>);
    impl<F: FnOnce()> Drop for G<F> {
        fn drop(&mut self) {
            if let Some(f) = self.0.take() {
                f();
            }
        }
    }
    G(Some(f))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bar advances only on real completions, and one running site can
    /// never fill it — a 3-site batch with site 1 mid-flight stays inside its
    /// own third.
    #[test]
    fn the_batch_bar_only_ever_shows_settled_rows_plus_the_running_one() {
        assert_eq!(batch_pct(0, 0, 3), 0);
        assert_eq!(batch_pct(0, 99, 3), 33);
        assert_eq!(batch_pct(1, 0, 3), 33);
        assert_eq!(batch_pct(2, 50, 3), 83);
        // Every row settled still can't print 100 — only the settle tick does.
        assert_eq!(batch_pct(3, 0, 3), 99);
        assert_eq!(batch_pct(0, 0, 0), 0);
    }

    /// With databases requested, a site's provision job owns 60 of its 100 and
    /// the database owns the rest — so "site done" is never "site finished".
    #[test]
    fn a_requested_database_keeps_the_last_40_of_its_sites_share() {
        assert_eq!(provision_share(100, true), 60);
        assert_eq!(db_share(0), 60);
        assert_eq!(db_share(100), 100);
        assert_eq!(provision_share(100, false), 100);
    }
}

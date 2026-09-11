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
    /// The row needs attention ONLY because its pinned PHP isn't one rexenv
    /// ships — the one decision the screen can take on the row itself (a
    /// version picker). The run refuses such a row without an explicit choice.
    pub php_choice: bool,
    pub secured: bool,
    pub proxy_to: Option<String>,
    pub also_in: Option<SourceKind>,
    /// The name the source served it under, when rexenv can't use that one
    /// (`ea.local` → `ea.rex`). Local rows only (ledger #571).
    pub renamed_from: Option<String>,
    pub has_custom_valet_driver: bool,
    /// Other names Valet/Herd serves this SAME folder under, folded into this
    /// row (v42 extra domains).
    ///
    /// A link farm registers one project under several names, and importing
    /// each as its own site is not possible (the second is refused for
    /// overlapping the first's docroot) and would be wrong if it were: one
    /// project, one database, one set of files, several hostnames. They arrive
    /// as extra domains on the site this row creates.
    #[serde(default)]
    pub extra_domains: Vec<String>,
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

    let mut found = core::valet::discover(&home);
    let available: Vec<String> = core::php::all_minors();
    let existing = core::sites::list(&conn)?;
    // Local's rows arrive already re-homed onto the default TLD (`.local` is
    // refused by policy), so the scan needs the setting — read here, where the
    // connection is, keeping `core::localwp` pure.
    let default_tld = core::sites::default_tld(&conn)?;
    let local_rows = match core::localwp::discover(&home, &default_tld) {
        Some((source, rows)) => {
            found.sources.push(source);
            rows
        }
        None => Vec::new(),
    };

    let candidates = found
        .sites
        .into_iter()
        .map(|s| enrich(&conn, platform, &existing, &available, s))
        .collect::<Vec<_>>();
    // Folding is a Valet link-farm concept (several names, one folder); Local
    // registers one name per site, so its rows join after the fold.
    let (mut candidates, folded) = fold_same_folder(candidates);
    for s in local_rows {
        let c = enrich(&conn, platform, &existing, &available, s);
        let c = refuse_a_taken_name(&candidates, c);
        candidates.push(c);
    }
    candidates.sort_by(|a, b| a.domain.cmp(&b.domain));

    // Every TLD any importable row is served on — including one that appears
    // only in a stray conf, which the config never mentions, and one that only
    // an EXTRA domain uses: the fold above runs first, so a Herd `.localhost`
    // row folded under its Valet `.test` twin would otherwise vanish from the
    // consent screen, get no resolver file, and never resolve.
    let mut tld_names: Vec<String> = candidates
        .iter()
        .filter(|c| !matches!(c.status, SiteStatus::Unsupported(_)))
        .flat_map(|c| std::iter::once(&c.domain).chain(c.extra_domains.iter()))
        .filter_map(|d| d.rsplit_once('.').map(|(_, t)| t.to_string()))
        .collect();
    // …plus every TLD ANOTHER tool has a resolver file for, and every TLD rexenv
    // has borrowed — whether or not a Valet/Herd site still uses it. Until
    // 5 Sep 2026 this list came from their SITES alone, so a leftover
    // `/etc/resolver/test` from an uninstalled Valet appeared on no page:
    // nothing to import, nothing to consent to, and the first `.test` domain
    // typed anywhere met the refusal. A borrowed TLD is kept for the mirror
    // reason — its hand-back row must not vanish the day their last site does.
    let port = core::dns::DEFAULT_DNS_PORT;
    tld_names.extend(core::dns::foreign_tlds(platform, port));
    tld_names.extend(
        crate::state::store::list_resolver_takeovers(&conn)
            .unwrap_or_default()
            .into_iter()
            .map(|t| t.tld),
    );
    tld_names.sort();
    tld_names.dedup();

    let tlds = tld_names
        .into_iter()
        .map(|tld| resolver_status_for(&conn, platform, &existing, &tld))
        .collect();

    let mut sources = found.sources;
    if let Some(first) = sources.first_mut() {
        first.notes.extend(folded);
    }
    Ok(ImportScan { sources, candidates, tlds, available_php: available })
}

/// Fold rows that serve the SAME folder into one, carrying the others as extra
/// domains. Returns the folded list plus a note per fold, for the scan's own
/// report.
///
/// **Why folding rather than importing each.** A Valet link farm registers one
/// project under several names. Importing them as separate sites is impossible
/// today (the second is refused for overlapping the first's docroot) and would
/// be wrong if it were possible: it is ONE project — one folder, one database,
/// one set of files — that answers to several hostnames, which is exactly what
/// v42's extra domains are for.
///
/// **Nothing disappears silently.** The extra names are on the row that
/// survives (`extra_domains`) and every fold is announced in the scan's notes,
/// because a row vanishing between one scan and the next is how a user
/// concludes the tool lost their site.
///
/// The PRIMARY is the shortest domain, ties broken alphabetically — so
/// `acme.test` wins over `www.acme.test`, and the choice is stable across runs
/// rather than dependent on directory order, which is what a user re-running a
/// scan compares against.
fn fold_same_folder(candidates: Vec<ImportCandidate>) -> (Vec<ImportCandidate>, Vec<String>) {
    use std::collections::HashMap;
    let mut primary_of: HashMap<String, usize> = HashMap::new();
    let mut notes: Vec<String> = Vec::new();
    let mut folded_away: Vec<usize> = Vec::new();
    let mut candidates = candidates;

    // Stable pick, independent of scan order.
    let better = |a: &ImportCandidate, b: &ImportCandidate| -> bool {
        (a.domain.len(), a.domain.as_str()) < (b.domain.len(), b.domain.as_str())
    };
    for i in 0..candidates.len() {
        // Only rows that would actually be created can carry aliases; a row
        // that needs attention keeps its own reason and its own line.
        if !matches!(candidates[i].status, SiteStatus::Importable) {
            continue;
        }
        let Some(folder) = candidates[i].serve_path.clone() else { continue };
        match primary_of.get(&folder).copied() {
            None => {
                primary_of.insert(folder, i);
            }
            Some(p) => {
                let (keep, drop) = if better(&candidates[i], &candidates[p]) {
                    primary_of.insert(folder, i);
                    (i, p)
                } else {
                    (p, i)
                };
                let extra = candidates[drop].domain.clone();
                let primary = candidates[keep].domain.clone();
                // Move the loser's already-collected extras too, or a
                // three-name farm would lose one of them.
                let inherited = std::mem::take(&mut candidates[drop].extra_domains);
                let target = &mut candidates[keep];
                target.extra_domains.push(extra.clone());
                target.extra_domains.extend(inherited);
                target.extra_domains.sort();
                target.extra_domains.dedup();
                folded_away.push(drop);
                notes.push(format!(
                    "{extra} serves the same folder as {primary} — importing it as an extra \
                     domain of that one site rather than a second site (same files, same \
                     database)"
                ));
            }
        }
    }
    folded_away.sort_unstable();
    let mut out = Vec::with_capacity(candidates.len() - folded_away.len());
    for (i, c) in candidates.into_iter().enumerate() {
        if folded_away.binary_search(&i).is_err() {
            out.push(c);
        }
    }
    (out, notes)
}

/// A hostname reaches exactly one site. A row whose name an EARLIER row already
/// claims is refused with the reason rather than listed twice: two rows with
/// one name would import the first and fail the second at the end of the batch,
/// and the screen keys its rows by domain.
fn refuse_a_taken_name(earlier: &[ImportCandidate], mut c: ImportCandidate) -> ImportCandidate {
    if let Some(other) = earlier.iter().find(|o| o.domain == c.domain) {
        c.status = SiteStatus::Unsupported(format!(
            "{} also has a site called {} in another folder — rexenv can give a name to only \
             one site, so import that one, or rename this site in {} first",
            other.source.label(),
            c.domain,
            c.source.label()
        ));
        c.serve_path = None;
    }
    c
}

/// Whether rexenv already has this row — by FOLDER first, then by name.
///
/// Folder first because a re-homed Local row can't be matched by name (rexenv
/// serves it under a name the source never used), and a Valet site whose domain
/// was later changed in rexenv is still that folder: by name alone both read as
/// "overlaps an existing site", which sends the user hunting for a conflict that
/// is really their own earlier import.
///
/// A NAME match on a re-homed row is not "already imported" — rexenv picked that
/// name, some other rexenv site has it, and that is a collision to resolve.
fn already_here(
    existing: &[crate::state::models::Site],
    serve: &std::path::Path,
    domain: &str,
    renamed_from: Option<&str>,
) -> Option<SiteStatus> {
    let canon = |p: &std::path::Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let want = canon(serve);
    if existing
        .iter()
        .any(|e| canon(std::path::Path::new(&e.path)) == want || canon(&e.served_root()) == want)
    {
        return Some(SiteStatus::AlreadyImported);
    }
    let e = existing.iter().find(|e| e.domain.eq_ignore_ascii_case(domain))?;
    Some(match renamed_from {
        Some(from) => SiteStatus::NeedsAttention(format!(
            "rexenv would serve {from} as {domain}, but another rexenv site already has that \
             name ({}) — rename one of them first",
            e.path
        )),
        None => SiteStatus::AlreadyImported,
    })
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
        php_choice: false,
        secured: s.secured,
        proxy_to: s.proxy_to,
        also_in: s.also_in,
        renamed_from: s.renamed_from,
        has_custom_valet_driver: false,
        status: s.status,
        extra_domains: Vec::new(),
    };

    // A filesystem-level refusal (missing folder, proxy) already decided this.
    if matches!(c.status, SiteStatus::Unsupported(_)) {
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

    if let Some(status) = already_here(existing, &serve, &c.domain, c.renamed_from.as_deref()) {
        c.status = status;
        return c;
    }

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
            c.php_choice = true;
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

/// Who owns ONE TLD's resolver file — the same row the import scan builds for
/// the TLDs it finds in Valet's config, reachable for a TLD the user typed.
///
/// Until 5 Sep 2026 the takeover consent card existed only on the Import page,
/// and only for TLDs the scan derived from Valet/Herd's own SITES. A user who
/// had left Valet (or never had a `.test` site there) typed `shop.test` into
/// Change domain, met "rexenv can take that TLD over", and had no button
/// anywhere that did — the scan listed nothing, Settings' Repair refused the
/// foreign file by design. This is the read the dialogs ask before they let
/// the write happen; the write is still `resolver_take_over`.
///
/// Refuses a TLD the policy would refuse: `resolver_path` joins the string
/// onto `/etc/resolver`, and a read whose content is shown to the user must
/// not be pointable at an arbitrary file.
#[tauri::command]
pub fn resolver_tld_status(state: State<'_, AppState>, tld: String) -> Result<ResolverTldStatus> {
    let tld = tld.trim().trim_start_matches('.').to_ascii_lowercase();
    let policy = core::tld::classify(&tld);
    if !policy.allowed {
        return Err(Error::Other(policy.reason));
    }
    let conn = lock(&state)?;
    let existing = core::sites::list(&conn)?;
    Ok(resolver_status_for(&conn, state.platform.as_ref(), &existing, &tld))
}

/// Take a TLD's resolver file over from Valet/Herd, backing theirs up first.
/// Consent lives in the UI; this is the operation it authorises.
#[tauri::command]
pub async fn resolver_take_over(state: State<'_, AppState>, tld: String) -> Result<()> {
    // `&state.db`, not a guard: the lock is taken per step, never across the prompt (#569).
    core::prompt::while_prompting(|| {
        core::dns::take_over_resolver(&state.db, state.platform.as_ref(), &tld, core::dns::DEFAULT_DNS_PORT)
    })
}

/// Give a borrowed resolver file back.
#[tauri::command]
pub async fn resolver_hand_back(
    state: State<'_, AppState>,
    tld: String,
) -> Result<core::dns::ResolverPlan> {
    core::prompt::while_prompting(|| {
        core::dns::hand_back_resolver(&state.db, state.platform.as_ref(), &tld, core::dns::DEFAULT_DNS_PORT)
    })
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
    /// Checked ONCE at the end: why the imported sites will not load yet, or
    /// `None` when they will. See [`ServingCaveat`].
    pub serving: Option<ServingCaveat>,
}

/// Why freshly imported sites are not being served, and what the user can do
/// about it — TWO different situations that used to be one boolean.
///
/// The boolean said "another app is answering port 443 — quit it" for BOTH, and
/// this path never checked whether rexenv's own stack was running. Importing
/// before Start-all is an ordinary order, so people were routinely told to quit
/// a program that did not exist. Advice about a nonexistent holder is worse than
/// no message: it sends someone hunting through Activity Monitor for nothing.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServingCaveat {
    /// `"foreign"` — something else holds :443. `"stopped"` — nothing is
    /// listening, which HERE means rexenv's own stack is not running, because
    /// the import itself never starts it.
    pub kind: &'static str,
    /// The process holding the port, when it could be named (`foreign` only).
    pub holder: Option<String>,
    /// The app to quit, when identifiable — "quit Herd" beats "quit that app".
    pub app: Option<String>,
    /// A copy-paste command that frees the port (`foreign` only).
    pub fix: Option<String>,
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
    batch.tick("scanning", 0, None, Some("re-reading your Valet, Herd and Local setup".into()), 0);

    // Re-scan rather than trusting the list we were handed: the screen's rows
    // are a suggestion, and the folders may have changed since it rendered.
    let scan = scan_valet_import(state.clone())?;
    let mut outcomes: Vec<ImportOutcome> = Vec::new();
    let mut queue: Vec<(ImportCandidate, String)> = Vec::new();

    for domain in &request.domains {
        let Some(c) = scan.candidates.iter().find(|c| &c.domain == domain).cloned() else {
            outcomes.push(skipped(domain, "it's no longer in Valet, Herd or Local"));
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
        let php = match choose_php(&c, request.php.get(domain), &scan.available_php) {
            Ok(php) => php,
            Err(why) => {
                outcomes.push(skipped(domain, &why));
                continue;
            }
        };
        let _ = serve;
        queue.push((c, php));
    }

    // Resolver files first, so any password prompt happens at ONE predictable
    // moment instead of surprising the user midway through the batch.
    let mut tlds: Vec<String> = queue
        .iter()
        .flat_map(|(c, _)| std::iter::once(&c.domain).chain(c.extra_domains.iter()))
        .filter_map(|d| d.rsplit_once('.').map(|(_, t)| t.to_string()))
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
                core::prompt::while_prompting(|| {
                    core::dns::configure_resolver(state.platform.as_ref(), tld, core::dns::DEFAULT_DNS_PORT)
                })?;
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
    let php_patches = {
        let conn = lock(&state)?;
        core::php::effective_patches(&conn).unwrap_or_default()
    };
    for m in &minors {
        batch.tick("php", 0, None, Some(format!("getting PHP {m} ready")), 0);
        let plan = core::downloads::plan_for_php_with(state.platform.as_ref(), m, &php_patches);
        core::downloads::prefetch(state.platform.as_ref(), &format!("Import (PHP {m})"), &plan)
            .await?;
    }

    let queue_had_aliases = queue.iter().any(|(c, _)| !c.extra_domains.is_empty());
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

    // The extra domains were RECORDED after each site's provision settled —
    // after the reload that provision did — and the web tier regenerates from
    // the manager's alias mirror, not the table. Without this, a link farm
    // imported as one site listed its other names everywhere and served none
    // of them (nginx's default vhost answered), and the stale mirror poisoned
    // every later reload until Stop all → Start all. ONE refresh + reload for
    // the batch, not one per site. A failure here is a caveat on the result,
    // not a failed import: the sites exist and their primaries serve.
    let alias_reload = if outcomes.iter().any(|o| o.status == "imported")
        && queue_had_aliases
    {
        batch.tick("serving", 0, None, Some("serving the extra domains".into()), 0);
        let read = state.db.lock().map_err(|_| Error::Other("database lock poisoned".into()));
        match read.and_then(|conn| {
            Ok((core::sites::list(&conn)?, crate::state::store::all_site_aliases(&conn)?))
        }) {
            Ok((sites, aliases)) => {
                super::sites::reload_for_domains(&state, sites, aliases).await.err()
            }
            Err(e) => Some(e),
        }
    } else {
        None
    };
    if let Some(e) = alias_reload {
        log::warn!("import: extra domains recorded but the reload failed: {e}");
    }

    batch.tick("checking", 0, None, Some("checking your sites will load".into()), 0);
    // ONE probe for the whole batch: per-site would add seconds each.
    //
    // The TRI-STATE, not the boolean, because this path is the reason the
    // tri-state exists: it does not start the stack and does not check whether
    // the stack is running, so "not ours" here is USUALLY "rexenv is not
    // running yet" rather than "something else took the port".
    let wire = core::proxy::edge_wire(
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
        // Only a caveat when something actually came over — "your sites won't
        // load" about zero sites is noise.
        serving: (imported > 0).then(|| serving_caveat(&state, wire)).flatten(),
    })
}

/// Turn the wire state into the caveat the UI renders, or `None` when the sites
/// really will load. Looks up who holds the port only when something does.
fn serving_caveat(state: &State<'_, AppState>, wire: core::proxy::EdgeWire) -> Option<ServingCaveat> {
    let help = matches!(wire, core::proxy::EdgeWire::Foreign).then(|| {
        state
            .platform
            .supervisor()
            .port_conflict_help(core::proxy::DEFAULT_HTTPS_PORT, false)
    });
    caveat_for(wire, help)
}

/// The mapping itself — pure, so the three states are testable without a
/// `State` or a socket. This is where the old boolean's fault lived: it had one
/// answer for two situations.
fn caveat_for(
    wire: core::proxy::EdgeWire,
    help: Option<crate::platform::traits::PortConflictHelp>,
) -> Option<ServingCaveat> {
    match wire {
        // Our edge answers: the sites really will load.
        core::proxy::EdgeWire::Ours => None,
        // Nothing is listening. On THIS path that means rexenv's own stack is
        // not running — importing never starts it — so there is nobody to quit
        // and nothing to name.
        core::proxy::EdgeWire::NoAnswer => {
            Some(ServingCaveat { kind: "stopped", holder: None, app: None, fix: None })
        }
        core::proxy::EdgeWire::Foreign => {
            let help = help.unwrap_or(crate::platform::traits::PortConflictHelp {
                holder: None,
                app: None,
                free_command: None,
            });
            Some(ServingCaveat {
                kind: "foreign",
                holder: help.holder,
                app: help.app,
                fix: help.free_command,
            })
        }
    }
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

/// The PHP minor one requested row imports on, or why it can't be imported.
///
/// The user's explicit choice wins. A row pinned to a version rexenv doesn't
/// ship and given NO choice is refused: until 11 Sep 2026 the run fell through
/// to the default PHP here, so any caller that named such a row without a
/// `php` entry (the MCP tool, a script) got exactly the silent substitution
/// the scan refuses to make. A row with no pin at all still gets the default —
/// they used their global PHP, so there is nothing to honour.
fn choose_php(
    c: &ImportCandidate,
    chosen: Option<&String>,
    available: &[String],
) -> std::result::Result<String, String> {
    let php = match (chosen, &c.php_target) {
        (Some(p), _) => p.clone(),
        (None, Some(t)) => t.clone(),
        (None, None) if c.php_choice => {
            return Err(format!(
                "PHP {} isn't one rexenv ships, and no version was chosen for it",
                c.php_minor.as_deref().unwrap_or("(unknown)")
            ))
        }
        (None, None) => core::php::minor_of(core::binaries::PHP_VERSION),
    };
    if !available.contains(&php) {
        return Err(format!("PHP {php} isn't one rexenv ships"));
    }
    Ok(php)
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
        // An import LINKS the developer's own folder; writing a starter page
        // and a database into it is the one thing import must never do.
        starter_db: false,
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
    // The other names this folder was served under (v42). AFTER provisioning,
    // and only on success: an alias on a site that failed to come up would be a
    // hostname reserved by a site nobody can use, and the site row may not even
    // exist. Best-effort per name — one refusal (a hostname some other site
    // already answers on) must not fail an import that otherwise worked, and it
    // is reported rather than swallowed.
    let mut alias_failures: Vec<String> = Vec::new();
    if ok {
        if let Some(site_id) = settled.site_id.as_deref() {
            for extra in &c.extra_domains {
                let added = {
                    match state.db.lock() {
                        Ok(conn) => crate::core::sites::add_alias(&conn, site_id, extra),
                        Err(_) => Err(Error::Other("database lock poisoned".into())),
                    }
                };
                if let Err(e) = added {
                    alias_failures.push(format!("{extra} ({e})"));
                }
            }
        }
    }
    ImportOutcome {
        domain: c.domain.clone(),
        status: if ok { "imported".into() } else { "failed".into() },
        reason: if ok {
            // An import that worked still says what it could NOT do — an extra
            // domain silently missing is a name the user will try and find dead.
            (!alias_failures.is_empty()).then(|| {
                format!("imported, but these extra domains were not added: {}", alias_failures.join("; "))
            })
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

    /// The fault this replaced: ONE boolean answered two situations, and the
    /// answer it gave was the wrong one on the ordinary path. Importing before
    /// Start-all leaves nothing on :443, and the user was told another app held
    /// it and to go quit that app.
    #[test]
    fn a_stopped_stack_is_not_reported_as_another_app_holding_the_port() {
        use crate::core::proxy::EdgeWire;
        use crate::platform::traits::PortConflictHelp;

        // Our edge answers: no caveat at all.
        assert!(caveat_for(EdgeWire::Ours, None).is_none());

        // Nothing listening — the stack is not running. NOTHING may be named:
        // a holder, an app to quit or a command here would all be inventions.
        let stopped = caveat_for(EdgeWire::NoAnswer, None).expect("a caveat");
        assert_eq!(stopped.kind, "stopped");
        assert!(
            stopped.holder.is_none() && stopped.app.is_none() && stopped.fix.is_none(),
            "a stopped stack has nobody to quit — naming one is the old bug"
        );

        // Something else holds it: everything we know gets carried, so the UI
        // can say "quit Herd" and offer the command rather than "quit it".
        let foreign = caveat_for(
            EdgeWire::Foreign,
            Some(PortConflictHelp {
                holder: Some("Herd (nginx, pid 554)".into()),
                app: Some("Herd".into()),
                free_command: Some("osascript -e 'quit app \"Herd\"'".into()),
            }),
        )
        .expect("a caveat");
        assert_eq!(foreign.kind, "foreign");
        assert_eq!(foreign.app.as_deref(), Some("Herd"));
        assert!(foreign.fix.is_some());

        // A foreign holder we could not attribute still reports foreign — the
        // port IS taken, and the UI falls back to "another app".
        let anonymous = caveat_for(EdgeWire::Foreign, None).expect("a caveat");
        assert_eq!(anonymous.kind, "foreign");
        assert!(anonymous.holder.is_none());
    }

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

#[cfg(test)]
mod folding_a_link_farm {
    use super::*;

    fn candidate(domain: &str, folder: &str, status: SiteStatus) -> ImportCandidate {
        ImportCandidate {
            source: SourceKind::Valet,
            name: domain.split('.').next().unwrap_or(domain).into(),
            domain: domain.into(),
            path: Some(folder.into()),
            serve_path: Some(folder.into()),
            docroot_rel: None,
            site_type: None,
            label: None,
            php_minor: None,
            php_target: None,
            php_choice: false,
            secured: false,
            proxy_to: None,
            also_in: None,
            renamed_from: None,
            has_custom_valet_driver: false,
            extra_domains: Vec::new(),
            status,
        }
    }

    /// Several Valet names for ONE folder become one site with extra domains —
    /// and the fold is announced.
    ///
    /// Importing them as separate sites is impossible today (the second is
    /// refused for overlapping the first's docroot) and would be wrong if it
    /// were: one project, one database, one set of files, several hostnames.
    /// What must NOT happen is a row quietly vanishing between one scan and the
    /// next, which is how a user concludes the tool lost their site — so every
    /// fold produces a note and the names live on the surviving row.
    #[test]
    fn same_folder_rows_become_one_row_with_extra_domains() {
        let (out, notes) = fold_same_folder(vec![
            candidate("www.acme.test", "/p/acme", SiteStatus::Importable),
            candidate("acme.test", "/p/acme", SiteStatus::Importable),
            candidate("acme-staging.test", "/p/acme", SiteStatus::Importable),
            candidate("other.test", "/p/other", SiteStatus::Importable),
        ]);
        assert_eq!(out.len(), 2, "one row per FOLDER: {out:?}");
        let acme = out.iter().find(|c| c.domain == "acme.test").expect(
            "the shortest domain must be the primary — a stable pick, not directory order",
        );
        assert_eq!(acme.extra_domains, vec!["acme-staging.test", "www.acme.test"]);
        assert_eq!(notes.len(), 2, "every fold is announced: {notes:?}");
        assert!(notes.iter().all(|n| n.contains("acme.test")));
        // The unrelated project is untouched and carries no extras.
        let other = out.iter().find(|c| c.domain == "other.test").expect("other.test");
        assert!(other.extra_domains.is_empty());
    }

    /// Already-imported is decided by FOLDER first, so a re-homed Local site
    /// (imported as `ea.rex`, scanned again as `ea.local` → `ea.rex`) and a Valet
    /// site whose domain was since changed in rexenv both read "already here";
    /// and a name rexenv PICKED that another site holds is a collision, never
    /// "already imported".
    #[test]
    fn already_here_matches_the_folder_before_the_name() {
        use crate::state::models::{test_site, SiteOrigin};
        let dir = std::env::temp_dir().join(format!("rexenv-already-here-{}", std::process::id()));
        let (ea, other) = (dir.join("ea/app/public"), dir.join("other"));
        std::fs::create_dir_all(&ea).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let mut imported = test_site("s1", "renamed-in-rexenv.rex", SiteOrigin::User);
        imported.path = ea.display().to_string();
        let mut taken = test_site("s2", "shop.rex", SiteOrigin::User);
        taken.path = other.display().to_string();
        let existing = vec![imported, taken];

        // Same folder, any name: already here.
        assert_eq!(already_here(&existing, &ea, "ea.rex", Some("ea.local")), Some(SiteStatus::AlreadyImported));
        // A symlinked route to the same folder is the same folder.
        let link = dir.join("link");
        let _ = std::os::unix::fs::symlink(&ea, &link);
        assert_eq!(already_here(&existing, &link, "x.test", None), Some(SiteStatus::AlreadyImported));
        // A different folder whose re-homed name another site holds: a collision.
        let fresh = dir.join("fresh");
        std::fs::create_dir_all(&fresh).unwrap();
        match already_here(&existing, &fresh, "shop.rex", Some("shop.local")) {
            Some(SiteStatus::NeedsAttention(r)) => assert!(r.contains("shop.local") && r.contains("rename"), "{r}"),
            s => panic!("a picked name that is taken must need attention, got {s:?}"),
        }
        // …while a name the SOURCE chose keeps today's reading.
        assert_eq!(already_here(&existing, &fresh, "shop.rex", None), Some(SiteStatus::AlreadyImported));
        assert_eq!(already_here(&existing, &fresh, "new.rex", None), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A pin rexenv doesn't ship is imported ONLY on an explicit choice — the
    /// run never quietly falls back to the default for it.
    #[test]
    fn an_unshipped_pin_needs_an_explicit_choice_and_nothing_else_does() {
        let available = vec!["7.4".to_string(), "8.3".to_string()];
        let mut pinned73 = candidate("ea.rex", "/p/ea", SiteStatus::NeedsAttention("PHP 7.3".into()));
        pinned73.php_minor = Some("7.3".into());
        pinned73.php_choice = true;

        let err = choose_php(&pinned73, None, &available).unwrap_err();
        assert!(err.contains("7.3") && err.contains("no version was chosen"), "{err}");
        assert_eq!(choose_php(&pinned73, Some(&"7.4".to_string()), &available).unwrap(), "7.4");
        assert!(choose_php(&pinned73, Some(&"7.2".to_string()), &available).is_err(), "a choice must be shipped too");

        let mut shipped = candidate("a.test", "/p/a", SiteStatus::Importable);
        shipped.php_target = Some("8.3".into());
        assert_eq!(choose_php(&shipped, None, &available).unwrap(), "8.3");

        // No pin at all: their global PHP — the default is an honest pick, but
        // only when the default is itself something this build ships.
        let unpinned = candidate("b.test", "/p/b", SiteStatus::Importable);
        let default = crate::core::php::minor_of(crate::core::binaries::PHP_VERSION);
        assert_eq!(choose_php(&unpinned, None, std::slice::from_ref(&default)).unwrap(), default);
    }

    /// A Local row whose name an earlier row claims is refused, named, and made
    /// unrunnable (no served folder) — never listed twice.
    #[test]
    fn a_name_an_earlier_row_claims_is_refused_not_duplicated() {
        let valet = candidate("shop.test", "/p/valet-shop", SiteStatus::Importable);
        let mut local = candidate("shop.test", "/p/local-shop", SiteStatus::Importable);
        local.source = SourceKind::Local;
        let out = refuse_a_taken_name(std::slice::from_ref(&valet), local);
        match &out.status {
            SiteStatus::Unsupported(r) => assert!(r.contains("Valet") && r.contains("Local"), "{r}"),
            s => panic!("{s:?}"),
        }
        assert!(out.serve_path.is_none(), "a refused row must not be runnable");
        let fine = refuse_a_taken_name(&[valet], candidate("other.test", "/p/o", SiteStatus::Importable));
        assert_eq!(fine.status, SiteStatus::Importable);
    }

    /// A row that needs attention keeps its own line and its own reason — it is
    /// not folded into somebody else's site.
    ///
    /// The reasons are per-NAME (an unavailable PHP pin, a custom driver, a
    /// docroot that failed the link preflight) and they are what the user has
    /// to act on; folding one away would delete the explanation with it.
    #[test]
    fn a_row_that_needs_attention_is_never_folded_away() {
        let (out, notes) = fold_same_folder(vec![
            candidate("acme.test", "/p/acme", SiteStatus::Importable),
            candidate(
                "old.test",
                "/p/acme",
                SiteStatus::NeedsAttention("PHP 7.2 isn't one rexenv ships".into()),
            ),
        ]);
        assert_eq!(out.len(), 2, "the needs-attention row must survive: {out:?}");
        assert!(notes.is_empty(), "nothing was folded, so nothing is announced");
        assert!(out.iter().all(|c| c.extra_domains.is_empty()));
    }
}


/// #186 — where a cancel is allowed to take effect.
#[cfg(test)]
mod cancel_lands_between_sites {
    /// **A cancel stops the batch BETWEEN sites, never inside one.**
    ///
    /// Importing one site is a provision: a folder is linked, a database is
    /// created, WordPress is configured. Abandoning that half-way is how a user
    /// ends up with a site that exists, does not work, and was not asked for —
    /// and the batch would have handed them no row explaining it. So the flag is
    /// read at the TOP of the per-site loop and nowhere inside `import_one`, and
    /// a site skipped this way still gets a row saying why.
    ///
    /// A source guard, because the alternative is a timing test: you would have
    /// to cancel during a real provision and inspect the wreckage, which is L3
    /// and destroys a site to prove a sentence.
    #[test]
    fn the_cancel_flag_is_read_between_sites_and_never_inside_one() {
        let src = crate::core::copy_scan::production_source(include_str!("valet_import.rs"));
        let run = src
            .split("pub async fn valet_import_run")
            .nth(1)
            .and_then(|b| b.split("\nfn ").next())
            .expect("valet_import_run");

        // The read exists, at the top of the loop, before anything is started.
        let loop_at = run.find("for (i, (c, php)) in queue").expect("the per-site loop");
        let read_at = run[loop_at..]
            .find("jobs.cancel.load(")
            .map(|i| loop_at + i)
            .expect("the loop no longer checks the cancel flag — Cancel would then run the \
                     whole batch and only stop when it ran out of sites");
        let start_at = run[loop_at..]
            .find("import_one(")
            .map(|i| loop_at + i)
            .expect("the loop no longer starts sites — if the shape changed, re-read this");
        assert!(
            read_at < start_at,
            "the cancel flag is read AFTER the site is started, so a cancel lands mid-provision \
             — a linked folder, a created database and a half-configured WordPress the user \
             never asked for"
        );

        // …and a cancelled site still gets a ROW. Silence would leave the user
        // counting the list to work out which ones never ran.
        let between = &run[read_at..start_at];
        assert!(
            between.contains("skipped(") && between.contains("outcomes.push("),
            "a site skipped by cancel produces no outcome row — the report then just has fewer \
             lines than the list, and nobody can tell which sites were skipped or why"
        );

        // Nothing inside the single-site import may consult the flag: that is
        // exactly the mid-site abandonment this rule forbids.
        let import_one = src
            .split("async fn import_one")
            .nth(1)
            .and_then(|b| b.split("\nfn ").next())
            .expect("import_one");
        assert!(
            !import_one.contains("cancel"),
            "`import_one` reads the cancel flag — a cancel inside one site abandons a provision \
             half-way, which is the state this boundary exists to prevent"
        );
    }
}

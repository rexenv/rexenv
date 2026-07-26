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

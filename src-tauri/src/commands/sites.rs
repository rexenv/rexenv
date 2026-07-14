//! commands::sites — thin Tauri IPC handlers for sites. Call `core/` only.

use crate::core;
use crate::core::db::DbEngine;
use crate::core::{binaries, php};
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::models::{NewSite, Site, SiteServing, SiteType, WebServer};
use std::path::Path;
use tauri::State;

fn lock<'a>(
    state: &'a State<'a, AppState>,
) -> Result<std::sync::MutexGuard<'a, rusqlite::Connection>> {
    state
        .db
        .lock()
        .map_err(|_| Error::Other("database lock poisoned".into()))
}

/// List all sites (newest first).
#[tauri::command]
pub fn list_sites(state: State<'_, AppState>) -> Result<Vec<Site>> {
    let conn = lock(&state)?;
    core::sites::list(&conn)
}

/// Live per-site serving status (H1 follow-up): whether each site is actually
/// reachable (edge up AND its own upstream up), not just whether the stack is up.
/// Derived from the non-blocking `service_infos()` snapshot, so it never blocks the
/// UI on a long start/stop. The frontend overlays it on the site rows by domain.
#[tauri::command]
pub fn sites_serving(state: State<'_, AppState>) -> Result<Vec<SiteServing>> {
    let sites = {
        let conn = lock(&state)?;
        core::sites::list(&conn)?
    };
    Ok(core::service_manager::site_serving(&sites, &state.service_infos()))
}

/// Honest per-site resource attribution (Sites page). A site is NOT a process
/// here — default sites share nginx + a per-version php-fpm pool — so the shape
/// is explicit about what each number IS:
/// - FrankenPHP-override sites: REAL CPU/RAM from their own backend's process
///   tree (same `Monitor::tree` source as the Services rows / footer).
/// - Every WP/Laravel site: REAL MySQL database disk size.
/// - Shared sites: ACTIVITY (requests + bytes over the last 60s window, from
///   the shared nginx access log) — never a fabricated per-site CPU/RAM.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteResources {
    pub id: String,
    pub domain: String,
    /// True for a FrankenPHP-override site (own process → real CPU/RAM);
    /// false ⇒ shared nginx + pool (activity metrics only).
    pub dedicated: bool,
    pub cpu_percent: Option<f32>,
    pub ram_mb: Option<u64>,
    /// Requests / sent bytes in the last 60s (shared nginx sites only —
    /// override sites bypass nginx).
    pub requests_per_min: Option<u64>,
    pub bytes_per_min: Option<u64>,
    /// MySQL database size in bytes (`None`: no DB / MySQL not running).
    pub db_size_bytes: Option<u64>,
}

/// Per-site resources for the Sites page — one monitor source of truth
/// (`Monitor::tree`) for the dedicated numbers, the shared nginx access log
/// for activity, one `information_schema` query for DB sizes.
#[tauri::command]
pub async fn sites_resources(state: State<'_, AppState>) -> Result<Vec<SiteResources>> {
    let sites = {
        let conn = lock(&state)?;
        core::sites::list(&conn)?
    };

    // FrankenPHP override backends (domain → pid). try_lock: during a long
    // start/stop just omit the dedicated numbers for one poll.
    let override_pids: std::collections::HashMap<String, u32> = state
        .services
        .try_lock()
        .map(|mgr| mgr.override_pids().into_iter().collect())
        .unwrap_or_default();

    // Activity per host from the shared nginx access log (last 60s).
    let access_log = state.platform.paths().log_dir()?.join("nginx-access.log");
    let activity =
        core::site_metrics::activity_by_host(&access_log, time::OffsetDateTime::now_utc());

    // DB sizes: one query per RUNNING site engine — resolved strictly from the
    // already-published cache (never a download from a status poll). Kept as
    // one map per engine: the same db name could exist in both engines, and a
    // site must read its own engine's number.
    let db_sizes: std::collections::HashMap<&'static str, std::collections::HashMap<String, u64>> =
        [DbEngine::Mysql, DbEngine::Mariadb]
            .into_iter()
            .filter(|e| e.running())
            .filter_map(|e| {
                let version = super::database::effective_db_version(&state, e).ok()?;
                let client = e.cached_sql_client(state.platform.as_ref(), &version)?;
                let sizes = core::database::db_sizes(&client, e.port()).ok()?;
                Some((e.key(), sizes.into_iter().collect()))
            })
            .collect();

    let mut monitor = state
        .monitor
        .lock()
        .map_err(|_| Error::Other("monitor lock poisoned".into()))?;
    monitor.refresh_processes();

    Ok(sites
        .into_iter()
        .map(|s| {
            let tree = override_pids.get(&s.domain).and_then(|pid| monitor.tree(*pid));
            let act = activity.get(&s.domain);
            SiteResources {
                dedicated: matches!(s.web_server, WebServer::Frankenphp | WebServer::Apache),
                cpu_percent: tree.map(|t| t.cpu_percent),
                ram_mb: tree.map(|t| t.ram_mb),
                requests_per_min: act.map(|a| a.requests),
                bytes_per_min: act.map(|a| a.bytes),
                db_size_bytes: db_sizes
                    .get(DbEngine::from_site(s.db_engine).key())
                    .and_then(|m| m.get(&s.db_name))
                    .copied(),
                id: s.id,
                domain: s.domain,
            }
        })
        .collect())
}

/// Rename a site's display name (domain/docroot/DB/certs unchanged); returns the
/// updated site.
#[tauri::command]
pub fn rename_site(state: State<'_, AppState>, id: String, name: String) -> Result<Option<Site>> {
    let conn = lock(&state)?;
    core::sites::rename(&conn, &id, &name)
}

/// Read-only info about a site's HTTPS leaf cert (Settings tab): validity dates,
/// days left, SANs, cert folder. `None` when no cert file exists yet.
#[tauri::command]
pub fn site_cert_info(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<core::ssl::SiteCertInfo>> {
    let site = {
        let conn = lock(&state)?;
        core::sites::get(&conn, &id)?
    }
    .ok_or_else(|| Error::Other(format!("site not found: {id}")))?;
    core::ssl::site_cert_info(state.platform.paths(), &site.domain)
}

/// Re-issue one site's HTTPS leaf cert (same local CA, SANs `domain` + `*.domain`)
/// and FORCE-reload the edge so Caddy serves it immediately. The re-issue is
/// atomic (temp-write + rename — a failure leaves the old cert intact and served);
/// a failed reload keeps the old, still-valid cert served and says so. Backs the
/// Settings-tab "Regenerate certificate" action; also the recovery path for a
/// deleted/corrupted cert or one nearing the 398-day Safari cap.
#[tauri::command]
pub async fn regenerate_site_cert(state: State<'_, AppState>, id: String) -> Result<()> {
    let (site, sites) = {
        let conn = lock(&state)?;
        let site = core::sites::get(&conn, &id)?
            .ok_or_else(|| Error::Other(format!("site not found: {id}")))?;
        (site, core::sites::list(&conn)?)
    };
    core::ssl::reissue_site_cert(
        state.platform.paths(),
        state.platform.permissions(),
        &state.ca,
        &site.domain,
    )?;
    super::system::reload_edge_for_new_certs(&state, &sites).await
}

/// Create a site (Phase 2 §1.6 + Phase 3 §1.2): provision it (docroot + cert + DB
/// row); for a **WordPress** site bring MySQL up and run the one-click installer
/// (`wp`) so the site is browsable; then — if the stack is running — ensure its
/// PHP version's pool is up and reload the edge so it serves immediately. `wp`
/// carries the dialog's WordPress fields (admin account, title, language) and is
/// ignored for non-WordPress sites. Returns the new site.
#[tauri::command]
pub async fn create_site(
    state: State<'_, AppState>,
    site: NewSite,
    wp: Option<core::wordpress::InstallOptions>,
    blueprint_id: Option<String>,
) -> Result<Site> {
    // The domain's TLD must resolve locally: install its OS resolver file on
    // first use (one privileged prompt; no-op when already installed — `.test`
    // lands during system setup). BEFORE provisioning, so a declined prompt or
    // an invalid/blocked domain creates nothing. `domain_tld` fully validates
    // the domain, so only a vetted label ever reaches the resolver command.
    let site_tld = core::sites::domain_tld(&site.domain)?;
    core::dns::ensure_resolver(state.platform.as_ref(), &site_tld, core::dns::DEFAULT_DNS_PORT)?;

    let (created, mut sites, blueprint) = {
        let conn = lock(&state)?;
        let created = core::sites::provision(&conn, state.platform.as_ref(), &state.ca, site)?;
        // Resolve the blueprint up front (so we don't hold the lock across awaits).
        let blueprint = match &blueprint_id {
            Some(id) if !id.is_empty() => crate::state::store::get_blueprint(&conn, id)?,
            _ => None,
        };
        (created, core::sites::list(&conn)?, blueprint)
    };
    let minor = php::minor_of(&created.php_version);

    // Prefetch everything this create could need BEFORE any services-lock scope
    // below — `spawn_db` / `ensure_php_pool` / the reload's override reconcile
    // all run under the lock and must hit cache, or a cold cache would stream
    // downloads while holding it. No-op (no batch) when everything's cached.
    // FrankenPHP embeds its PHP; nginx/Apache sites ride a shared pool — and
    // Apache additionally needs its own httpd bundle.
    let mut plan = if matches!(created.web_server, WebServer::Frankenphp) {
        core::downloads::plan_for_override(state.platform.as_ref(), created.web_server)
    } else {
        core::downloads::plan_for_pool(state.platform.as_ref(), &minor)
    };
    if matches!(created.web_server, WebServer::Apache) {
        plan.extend(core::downloads::plan_for_override(state.platform.as_ref(), created.web_server));
    }
    let engine = DbEngine::from_site(created.db_engine);
    let engine_version = super::database::effective_db_version(&state, engine)?;
    if matches!(created.site_type, SiteType::Wordpress) {
        plan.extend(core::downloads::plan_for_engine(
            state.platform.as_ref(),
            engine,
            &engine_version,
        ));
        plan.extend(core::downloads::plan_for_wp_tooling(state.platform.as_ref(), &minor));
    }
    core::downloads::prefetch(state.platform.as_ref(), "Create site", &plan).await?;

    // WordPress needs a database + a one-click install before it's browsable.
    // The services lock is held only to SPAWN the site's engine; the readiness
    // wait and the (long) installer run with it released (M4), so other
    // commands and status stay responsive during a site create.
    if matches!(created.site_type, SiteType::Wordpress) {
        let check = {
            let mut mgr = state.services.lock().await;
            mgr.spawn_db(state.platform.as_ref(), engine).await?
        };
        core::service_manager::await_ready(check.into_iter().collect()).await?;
        let patch = php::patch_for_minor(&minor)
            .ok_or_else(|| Error::Other(format!("no pinned PHP build for {minor}")))?;
        let php_bin = binaries::resolve(state.platform.as_ref(), "php", patch).await?;
        let wp_phar =
            binaries::resolve_file(state.platform.as_ref(), "wp-cli", binaries::WP_CLI_VERSION)
                .await?;
        let db_host = format!("127.0.0.1:{}", engine.port());
        let (db_client, _) =
            engine.sql_client_bins(state.platform.as_ref(), &engine_version).await?;
        let docroot = Path::new(&created.path);
        core::wordpress::install_for_site(
            &php_bin,
            &wp_phar,
            docroot,
            &created.domain,
            &created.name,
            &created.db_name,
            &db_host,
            &db_client,
            &wp.unwrap_or_default(),
        )?;

        // Apply a blueprint (§11.3): install/activate its plugins + themes, set
        // WP_DEBUG, and convert to multisite — the reusable "site setup" automation.
        if let Some(bp) = &blueprint {
            core::blueprints::apply_wordpress(&php_bin, &wp_phar, docroot, &bp.spec)?;
            if !matches!(bp.spec.multisite, crate::state::models::MultisiteMode::None) {
                let conn = lock(&state)?;
                core::sites::convert_multisite(
                    &conn, &php_bin, &wp_phar, docroot, &created.id, bp.spec.multisite,
                )?;
                // Refresh so the reload below serves the new multisite rewrite.
                sites = core::sites::list(&conn)?;
            }
        }
    }

    let checks = {
        let mut mgr = state.services.lock().await;
        if mgr.is_running() {
            if !matches!(created.web_server, WebServer::Frankenphp) {
                mgr.ensure_php_pool(state.platform.as_ref(), &minor).await?;
            }
            mgr.reload(state.platform.as_ref(), &state.ca, &sites, false).await?
        } else {
            Vec::new()
        }
    };
    core::service_manager::await_ready(checks).await?;
    Ok(created)
}

/// Switch a site's web server (§4.1): update the DB row, then — if the stack is
/// running — bring the new backend up (and the old one down if unused), reload
/// the edge. No docroot/cert/DB rebuild. Returns the updated site.
#[tauri::command]
pub async fn set_site_web_server(
    state: State<'_, AppState>,
    id: String,
    server: WebServer,
) -> Result<Option<Site>> {
    let (site, sites) = {
        let conn = lock(&state)?;
        let updated = core::sites::set_web_server(&conn, &id, server)?;
        (updated, core::sites::list(&conn)?)
    };
    if let Some(ref s) = site {
        // The new backend's binary must be cached BEFORE the locked scope below
        // (pool ensure / override reconcile download otherwise). No-op when warm.
        let mut plan = if matches!(s.web_server, WebServer::Frankenphp) {
            core::downloads::plan_for_override(state.platform.as_ref(), s.web_server)
        } else {
            core::downloads::plan_for_pool(
                state.platform.as_ref(),
                &core::php::minor_of(&s.php_version),
            )
        };
        if matches!(s.web_server, WebServer::Apache) {
            plan.extend(core::downloads::plan_for_override(state.platform.as_ref(), s.web_server));
        }
        core::downloads::prefetch(state.platform.as_ref(), "Switch web server", &plan).await?;
        // Readiness of a newly spawned FrankenPHP backend is awaited with the
        // services lock released (M4).
        let checks = {
            let mut mgr = state.services.lock().await;
            if mgr.is_running() {
                // Switching to an nginx-served site needs its PHP version's pool up;
                // FrankenPHP uses its embedded PHP, so no pool is needed.
                if !matches!(s.web_server, WebServer::Frankenphp) {
                    let minor = core::php::minor_of(&s.php_version);
                    mgr.ensure_php_pool(state.platform.as_ref(), &minor).await?;
                }
                mgr.reload(state.platform.as_ref(), &state.ca, &sites, false).await?
            } else {
                Vec::new()
            }
        };
        core::service_manager::await_ready(checks).await?;
    }
    Ok(site)
}

/// Switch a site's PHP version (§1.4): update the DB row, then — if the stack is
/// running — ensure that version's php-fpm pool is up and reload nginx so the
/// switch takes effect. No docroot/cert/DB rebuild. Returns the updated site.
#[tauri::command]
pub async fn set_site_php_version(
    state: State<'_, AppState>,
    id: String,
    version: String,
) -> Result<Option<Site>> {
    let (site, sites) = {
        let conn = lock(&state)?;
        let updated = core::sites::set_php_version(&conn, &id, &version)?;
        (updated, core::sites::list(&conn)?)
    };
    if let Some(ref s) = site {
        let minor = core::php::minor_of(&s.php_version);
        // Pool binary cached before the locked ensure below. No-op when warm.
        let plan = core::downloads::plan_for_pool(state.platform.as_ref(), &minor);
        core::downloads::prefetch(state.platform.as_ref(), "Switch PHP version", &plan).await?;
        let checks = {
            let mut mgr = state.services.lock().await;
            if mgr.is_running() {
                mgr.ensure_php_pool(state.platform.as_ref(), &minor).await?;
                mgr.reload(state.platform.as_ref(), &state.ca, &sites, false).await?
            } else {
                Vec::new()
            }
        };
        core::service_manager::await_ready(checks).await?;
    }
    Ok(site)
}

/// Move a site's docroot into a user-picked PARENT directory (the folder keeps
/// its name). Ordered so the row NEVER points at a path that doesn't exist:
///   preflight (all rejections, no file touched) → files to the new location
///   (same-volume rename, else copy + verify with partial-copy cleanup) →
///   sites.path update → config regen + reload (nginx root; a FrankenPHP
///   override backend is restarted by the reconcile when its config changed;
///   Caddy holds no docroot) → delete the old tree LAST (copy case only,
///   best-effort). Not destructive: data is verified at the destination before
///   anything old is removed. Works with the stack down (configs load at the
///   next start). Returns the updated site.
#[tauri::command]
pub async fn move_site_docroot(
    state: State<'_, AppState>,
    id: String,
    dest_parent: String,
) -> Result<Site> {
    let dest = std::path::PathBuf::from(&dest_parent);
    let (site, target) = {
        let conn = lock(&state)?;
        let site = core::sites::get(&conn, &id)?
            .ok_or_else(|| Error::Other(format!("site not found: {id}")))?;
        let target = core::sites::check_docroot_move(&site, &dest)?;
        (site, target)
    };
    let src = std::path::PathBuf::from(&site.path);

    // File work off the async runtime AND outside the DB lock — a cross-volume
    // copy of a big docroot must not stall status polls or the UI.
    let copied = {
        let (s, t) = (src.clone(), target.clone());
        super::wordpress::wp_blocking(move || core::sites::move_dir(&s, &t)).await?
    };

    // Files verifiably at the new location — only now flip the row.
    let (updated, sites) = {
        let conn = lock(&state)?;
        let updated = core::sites::set_path(&conn, &id, &target)?
            .ok_or_else(|| Error::Other(format!("site not found: {id}")))?;
        (updated, core::sites::list(&conn)?)
    };

    // Regenerate + reload so nginx's root (and any FrankenPHP override) points
    // at the new path. Retriable: the row is already correct, so a later
    // reload/start also serves from the new location.
    let reload = async {
        let checks = {
            let mut mgr = state.services.lock().await;
            if mgr.is_running() {
                mgr.reload(state.platform.as_ref(), &state.ca, &sites, false).await?
            } else {
                Vec::new()
            }
        };
        core::service_manager::await_ready(checks).await
    };
    reload.await.map_err(|e| {
        Error::Other(format!(
            "Files moved to {}, but the config reload failed — the site may not serve \
             until services reload. Retry from Services → Restart. ({e})",
            target.display()
        ))
    })?;

    // Cross-volume copy: the old tree still exists — delete it LAST, after the
    // reload proved the new location serves. Best-effort: a failure leaves
    // duplicate files, never a broken site.
    if copied {
        let old = src.clone();
        let _ = super::wordpress::wp_blocking(move || {
            std::fs::remove_dir_all(&old).map_err(crate::error::Error::from)
        })
        .await;
    }

    Ok(updated)
}

/// One env-var row as the UI sends it (name/value strings).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct EnvVarInput {
    pub name: String,
    pub value: String,
}

/// A site's per-request env vars, name-sorted (Phase 3 §1.6).
#[tauri::command]
pub fn list_site_env(state: State<'_, AppState>, id: String) -> Result<Vec<EnvVarInput>> {
    let conn = lock(&state)?;
    Ok(crate::state::store::get_site_env(&conn, &id)?
        .into_iter()
        .map(|(name, value)| EnvVarInput { name, value })
        .collect())
}

/// Replace a site's env vars (replace-all, like the PHP settings editor).
/// BACKEND validation is the enforcement (`core::site_env::validate_all` —
/// name shape, reserved names, unescapable characters); the UI mirror is only
/// for instant feedback. Then persist + swap the manager's map + regen/reload:
/// nginx picks up the new `fastcgi_param` lines; a FrankenPHP override whose
/// config changed is restarted by the reconcile diff. Non-destructive and
/// reversible (config text only); no-op beyond persisting when stopped.
#[tauri::command]
pub async fn set_site_env(
    state: State<'_, AppState>,
    id: String,
    vars: Vec<EnvVarInput>,
) -> Result<()> {
    let pairs: Vec<(String, String)> =
        vars.into_iter().map(|v| (v.name.trim().to_string(), v.value)).collect();
    core::site_env::validate_all(&pairs)?;

    // Persist + snapshot under ONE brief DB lock, dropped before any await
    // (same shape as apply_php_settings).
    let (site_exists, sites, all) = {
        let conn = lock(&state)?;
        let exists = core::sites::get(&conn, &id)?.is_some();
        if exists {
            crate::state::store::replace_site_env(&conn, &id, &pairs)?;
        }
        (exists, core::sites::list(&conn)?, crate::state::store::all_site_env(&conn)?)
    };
    if !site_exists {
        return Err(Error::Other(format!("site not found: {id}")));
    }

    let checks = {
        let mut mgr = state.services.lock().await;
        mgr.apply_site_env(state.platform.as_ref(), &state.ca, &sites, all).await?
    };
    core::service_manager::await_ready(checks).await
}

/// Result of a domain change: the updated site, where the pre-change database
/// backup landed (WordPress sites only), and how many search-replace
/// substitutions ran (both passes; 0 for non-WordPress sites).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainChange {
    pub site: Site,
    pub backup_path: Option<String>,
    pub replacements: u64,
}

/// Change a site's domain (e.g. `myapp.test → myshop.test`). DESTRUCTIVE for
/// WordPress sites — the URL rewrite touches serialized data one-way, so the
/// database is ALWAYS exported to Downloads first and the whole flow aborts if
/// that export fails. Refused on multisite (see `core::sites::check_domain_change`).
///
/// Ordered so the one non-reversible step (search-replace) runs while everything
/// around it is still intact or trivially retriable:
///   preflight → backup → new cert (additive) → search-replace dry-run gate →
///   real search-replace (`https://old→https://new`, then bare `old→new`,
///   `--all-tables`) → SQLite domain flip (path + db_name untouched) →
///   config regen + forced edge reload → old-artifact cleanup (best-effort).
/// A failure before the SQLite flip leaves the site fully working on the old
/// domain (the DB restorable from the fresh backup); after the flip the only
/// remaining step that can fail is the reload, which is retriable.
///
/// DNS: the embedded resolver answers any name, so the only DNS step is
/// ensuring the new domain's TLD has its OS resolver file (first use of a TLD
/// = one privileged prompt; `.test` and previously used TLDs are no-ops). The
/// docroot folder and database name stay keyed to the old domain on purpose
/// (cosmetic; renaming either is risk for zero value). Bare-domain pass also
/// rewrites `…@old.test` email addresses — acceptable for local dev, stated in
/// the UI.
#[tauri::command]
pub async fn change_site_domain(
    state: State<'_, AppState>,
    tunnels: State<'_, crate::commands::tunnels::Tunnels>,
    id: String,
    domain: String,
) -> Result<DomainChange> {
    let domain = domain.trim().to_string();
    let site = {
        let conn = lock(&state)?;
        let site = core::sites::get(&conn, &id)?
            .ok_or_else(|| Error::Other(format!("site not found: {id}")))?;
        // Preflight BEFORE any destructive step; re-checked inside set_domain.
        core::sites::check_domain_change(&conn, &site, &domain)?;
        site
    };
    let old_domain = site.domain.clone();
    let is_wp = matches!(site.site_type, SiteType::Wordpress);

    // New TLD → its OS resolver file must exist, or the renamed site wouldn't
    // resolve. First use of a TLD = one privileged prompt; no-op otherwise.
    // BEFORE the backup/search-replace, so declining the prompt changes nothing.
    let new_tld = core::sites::domain_tld(&domain)?;
    core::dns::ensure_resolver(state.platform.as_ref(), &new_tld, core::dns::DEFAULT_DNS_PORT)?;

    let mut backup_path = None;
    let mut replacements = 0u64;
    if is_wp {
        // Fail fast with an actionable message (same guard as export/reset).
        let engine = DbEngine::from_site(site.db_engine);
        if !engine.running() {
            return Err(Error::Other(format!(
                "{} isn't running — start it (Services → Start all, or the Databases page), then change the domain again.",
                engine.label()
            )));
        }
        let engine_version = super::database::effective_db_version(&state, engine)?;
        let (_, dump_bin) =
            engine.sql_client_bins(state.platform.as_ref(), &engine_version).await?;
        let (php, wp) = super::wordpress::wp_tools(&state, &site.php_version).await?;
        let docroot = std::path::PathBuf::from(&site.path);

        // 1) MANDATORY backup — the search-replace below isn't reversible.
        let dump = {
            let (old, db) = (old_domain.clone(), site.db_name.clone());
            super::wordpress::wp_blocking(move || {
                core::database::export_to_downloads(&dump_bin, engine.port(), &old, &db)
            })
            .await
            .map_err(|e| Error::Other(format!("domain unchanged — the safety backup failed: {e}")))?
        };
        backup_path = Some(dump.to_string_lossy().into_owned());

        // 2) Cert for the new domain — purely additive; the old cert keeps
        //    being served until the reload below.
        core::ssl::ensure_site_cert(
            state.platform.paths(),
            state.platform.permissions(),
            &state.ca,
            &domain,
        )?;

        // 3) URL migration. Dry-run first as an environment gate (wp-cli boots,
        //    DB reachable) — if it fails, nothing has been mutated. Then two
        //    real passes: full URL, then bare domain (srcset, protocol-relative
        //    and hardcoded refs). wp-cli handles serialized data; --all-tables
        //    covers non-prefix tables (each site owns its database).
        let (from_url, to_url) = (format!("https://{old_domain}"), format!("https://{domain}"));
        replacements = {
            let (old, new) = (old_domain.clone(), domain.clone());
            super::wordpress::wp_blocking(move || {
                core::wordpress::search_replace(&php, &wp, &docroot, &from_url, &to_url, true, true)?;
                let mut n =
                    core::wordpress::search_replace(&php, &wp, &docroot, &from_url, &to_url, false, true)?;
                n += core::wordpress::search_replace(&php, &wp, &docroot, &old, &new, false, true)?;
                Ok(n)
            })
            .await?
        };
    } else {
        // Non-WordPress sites store no URL — cert + configs are the whole change.
        core::ssl::ensure_site_cert(
            state.platform.paths(),
            state.platform.permissions(),
            &state.ca,
            &domain,
        )?;
    }

    // 4) Flip the row (domain only) — from here the configs regenerate to the
    //    new domain.
    let (updated, sites) = {
        let conn = lock(&state)?;
        let updated = core::sites::set_domain(&conn, &id, &domain)?
            .ok_or_else(|| Error::Other(format!("site not found: {id}")))?;
        (updated, core::sites::list(&conn)?)
    };

    // 5) Regenerate nginx/Caddy from the rows + FORCE-reload the edge (forced:
    //    if only cert bytes changed Caddy would skip a normal reload). No-op
    //    when the stack is down — the new configs load at the next start.
    //    Retriable: the row is already flipped, so a later reload also serves
    //    the new domain.
    let reload = async {
        let checks = {
            let mut mgr = state.services.lock().await;
            if mgr.is_running() {
                mgr.reload(state.platform.as_ref(), &state.ca, &sites, true).await?
            } else {
                Vec::new()
            }
        };
        core::service_manager::await_ready(checks).await
    };
    reload.await.map_err(|e| {
        Error::Other(format!(
            "Domain changed to {domain}, but the edge reload failed — the site may not \
             be reachable until services reload. Retry from Services → Restart. ({e})"
        ))
    })?;

    // 6) Old-domain artifacts (best-effort, non-fatal): a live tunnel proxies a
    //    vhost that no longer exists; cert dir / FrankenPHP config / logs are
    //    keyed by the old domain (mirrors teardown).
    tunnels.stop_for_domain(state.platform.as_ref(), &old_domain);
    if let Ok(dir) = core::ssl::site_cert_dir(state.platform.paths(), &old_domain) {
        let _ = std::fs::remove_dir_all(dir);
    }
    if let Ok(conf) = core::frankenphp::config_path(state.platform.as_ref(), &old_domain) {
        let _ = std::fs::remove_file(conf);
    }
    if let Ok(log) = core::frankenphp::log_path(state.platform.as_ref(), &old_domain) {
        let _ = std::fs::remove_file(log);
    }
    if let Ok(log) = core::tunnels::log_path(state.platform.as_ref(), &old_domain) {
        let _ = std::fs::remove_file(log);
    }

    Ok(DomainChange { site: updated, backup_path, replacements })
}

/// Delete a site — complete cleanup: stop its public tunnel, drop its MySQL
/// database, then tear down the DB row + cert + per-site configs/logs + docroot,
/// and reload the running stack so it stops being served. Returns whether it
/// existed. If the database drop fails the site is left intact (retryable) —
/// never a silently orphaned database.
#[tauri::command]
pub async fn delete_site(
    state: State<'_, AppState>,
    tunnels: State<'_, crate::commands::tunnels::Tunnels>,
    id: String,
) -> Result<bool> {
    let site = {
        let conn = lock(&state)?;
        core::sites::get(&conn, &id)?
    };
    let Some(site) = site else { return Ok(false) };

    // 1) A deleted site must not stay publicly shared: kill its live tunnel
    //    (registry keyed by domain; the mu-plugin goes away with the docroot).
    tunnels.stop_for_domain(state.platform.as_ref(), &site.domain);

    // 2) Drop the site's database. Only WordPress sites get one (the stored
    //    `Site::db_name`, derived from the validated domain at creation —
    //    `drop_database` re-validates the name, so nothing else can be named).
    //    No per-site DB user exists to
    //    remove (local-dev connects as passwordless root). Skipped entirely when
    //    the MySQL datadir was never initialized (then no database can exist);
    //    otherwise MySQL is brought up first, exactly like site creation does.
    let engine = DbEngine::from_site(site.db_engine);
    let engine_version = super::database::effective_db_version(&state, engine)?;
    if matches!(site.site_type, SiteType::Wordpress)
        && engine.datadir_initialized(state.platform.as_ref(), &engine_version)
    {
        // Engine binaries cached before the locked spawn below (an initialized
        // datadir with an evicted binary cache would otherwise download under
        // the lock).
        let plan =
            core::downloads::plan_for_engine(state.platform.as_ref(), engine, &engine_version);
        core::downloads::prefetch(state.platform.as_ref(), "Delete site", &plan).await?;
        let check = {
            let mut mgr = state.services.lock().await;
            mgr.set_db_version(engine, &engine_version);
            mgr.spawn_db(state.platform.as_ref(), engine).await?
        };
        core::service_manager::await_ready(check.into_iter().collect()).await?;
        let (db_client, _) =
            engine.sql_client_bins(state.platform.as_ref(), &engine_version).await?;
        core::database::drop_database(&db_client, engine.port(), &site.db_name)?;
    }

    // 3) Row + cert + per-site configs/logs + docroot.
    let (removed, sites) = {
        let conn = lock(&state)?;
        let removed = core::sites::teardown(&conn, state.platform.as_ref(), &id)?;
        (removed, core::sites::list(&conn)?)
    };
    // Best-effort reload (no-op if services aren't running). A delete only
    // REMOVES backends, so there are no readiness probes to await.
    let mut mgr = state.services.lock().await;
    let _ = mgr.reload(state.platform.as_ref(), &state.ca, &sites, false).await;
    Ok(removed)
}

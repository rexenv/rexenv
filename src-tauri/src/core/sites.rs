//! core::sites — site lifecycle domain logic (PLATFORM-AGNOSTIC).
//!
//! Phase 1 task 1.2: create / list / get / delete persisted via `state::store`.
//! Later tasks extend `create` to also generate the docroot, cert, vhost, and
//! edge-router route (§7); this module stays the single entry point for site
//! operations so commands/ remain thin.

use crate::core::{adminer, frankenphp, php, proxy, services, ssl, tunnels};
use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use crate::state::models::{MultisiteMode, NewSite, ServiceStatus, Site, SiteType, WebServer};
use crate::state::store;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Strict validation for a site domain (defense-in-depth — M7). The domain becomes a
/// filesystem path (docroot), nginx/Caddy config tokens (`server_name`, host), a cert
/// SAN, and a database name — so reject anything that could traverse a path, inject a
/// config directive, or isn't a plain lowercase `.test` hostname. The UI already slugs
/// input to this shape; this is the backstop for every other caller.
fn validate_domain(domain: &str) -> Result<()> {
    let reject = |why: &str| Error::Other(format!("invalid domain '{domain}': {why}"));
    // DNS caps a name at 253 chars; stay well under any fs/DB-identifier limit too.
    if domain.is_empty() || domain.len() > 253 {
        return Err(reject("must be 1–253 characters"));
    }
    let labels: Vec<&str> = domain.split('.').collect();
    // Development domains only, and at least one label before the TLD.
    if labels.last() != Some(&"test") {
        return Err(reject("must end in .test"));
    }
    if labels.len() < 2 {
        return Err(reject("must have a label before .test"));
    }
    for label in &labels {
        if label.is_empty() {
            return Err(reject("has an empty label"));
        }
        if label.len() > 63 {
            return Err(reject("has a label over 63 characters"));
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Err(reject("has a label starting or ending with '-'"));
        }
        // a–z, 0–9, hyphen only: blocks path separators, spaces, wildcards, quotes,
        // ';'/'{'/newlines (config injection), and uppercase (case-dup + fs/DB drift).
        if !label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(reject("labels may contain only a–z, 0–9, and '-'"));
        }
    }
    Ok(())
}

/// Create a site: assign an id, default to stopped + SSL on, persist, return it.
/// Fails if the domain is invalid or already in use.
pub fn create(conn: &Connection, new: NewSite) -> Result<Site> {
    validate_domain(&new.domain)?;
    if store::domain_exists(conn, &new.domain)? {
        return Err(Error::Other(format!(
            "domain already in use: {}",
            new.domain
        )));
    }
    let site = Site {
        id: Uuid::new_v4().to_string(),
        name: new.name,
        domain: new.domain,
        site_type: new.site_type,
        status: ServiceStatus::Stopped,
        php_version: new.php_version,
        web_server: new.web_server,
        ssl: true,
        path: new.path,
        created_at: store::db_now(conn)?,
        multisite: MultisiteMode::None,
    };
    store::insert_site(conn, &site)?;
    Ok(site)
}

/// All sites, newest first.
pub fn list(conn: &Connection) -> Result<Vec<Site>> {
    store::list_sites(conn)
}

/// One site by id, or `None`.
pub fn get(conn: &Connection, id: &str) -> Result<Option<Site>> {
    store::get_site(conn, id)
}

/// Delete a site by id; returns whether it existed.
pub fn delete(conn: &Connection, id: &str) -> Result<bool> {
    store::delete_site(conn, id)
}

/// Set a site's status, returning the updated site (or `None` if it doesn't exist).
pub fn set_status(conn: &Connection, id: &str, status: ServiceStatus) -> Result<Option<Site>> {
    store::set_site_status(conn, id, status)?;
    get(conn, id)
}

/// Rename a site's DISPLAY name only (the domain, docroot, DB and certs are keyed
/// off the domain, so they're untouched). Returns the updated site, or `None` if
/// it doesn't exist. Errors on a blank name.
pub fn rename(conn: &Connection, id: &str, name: &str) -> Result<Option<Site>> {
    let name = name.trim();
    if name.is_empty() {
        return Err(crate::error::Error::Other("site name cannot be empty".into()));
    }
    store::set_site_name(conn, id, name)?;
    get(conn, id)
}

/// Switch a site's web server (Phase 2 §4.1): update ONLY the `web_server` column
/// — no docroot/cert/DB rebuild — and return the updated site. Only Nginx and
/// FrankenPHP have backends in Phase 2 (Apache/OLS are deferred). The caller
/// brings the new backend up / old down and reloads the edge.
pub fn set_web_server(conn: &Connection, id: &str, server: WebServer) -> Result<Option<Site>> {
    if !matches!(server, WebServer::Nginx | WebServer::Frankenphp) {
        return Err(Error::Other(format!(
            "web server {} is not available on this platform yet",
            server.as_db()
        )));
    }
    if !store::set_site_web_server(conn, id, server.as_db())? {
        return Ok(None);
    }
    get(conn, id)
}

/// Convert a WordPress site to a multisite network (§10.1): run `wp core
/// multisite-convert` (subdomain or subdirectory) to write the network constants,
/// then persist the mode so `rebuild_configs` regenerates nginx with the matching
/// rewrite template. The caller reloads the edge afterward (no docroot/cert/DB
/// rebuild). Returns the updated site, or `None` if the id doesn't exist.
pub fn convert_multisite(
    conn: &Connection,
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    id: &str,
    mode: MultisiteMode,
) -> Result<Option<Site>> {
    if matches!(mode, MultisiteMode::None) {
        return Err(Error::Other(
            "multisite mode must be 'subdomain' or 'subdirectory'".into(),
        ));
    }
    crate::core::wordpress::multisite_convert(
        php_bin,
        wp_phar,
        docroot,
        matches!(mode, MultisiteMode::Subdomain),
    )?;
    if !store::set_site_multisite(conn, id, mode.as_db())? {
        return Ok(None);
    }
    get(conn, id)
}

/// Flip a site back to single-site after a reset — the fresh database has no
/// network, and the reset cleared the multisite constants from wp-config.
/// Returns the updated site (`None` if it doesn't exist).
pub fn clear_multisite(conn: &Connection, id: &str) -> Result<Option<Site>> {
    if !store::set_site_multisite(conn, id, MultisiteMode::None.as_db())? {
        return Ok(None);
    }
    get(conn, id)
}

/// Switch a site's PHP version (Phase 2 §1.4): update ONLY the `php_version`
/// column — no docroot, cert, or DB rebuild — and return the updated site (or
/// `None` if it doesn't exist). The version must have a pinned build and is
/// normalized to its minor series (`8.3.31` → `8.3`). The caller ensures the
/// target pool is running and reloads nginx so the change takes effect.
pub fn set_php_version(conn: &Connection, id: &str, version: &str) -> Result<Option<Site>> {
    let minor = php::minor_of(version);
    if php::patch_for_minor(&minor).is_none() {
        return Err(Error::Other(format!("unsupported PHP version: {version}")));
    }
    if !store::set_site_php_version(conn, id, &minor)? {
        return Ok(None);
    }
    get(conn, id)
}

/// Full teardown of a site: remove its DB row, cert material, per-site
/// config/log artifacts, and docroot. Returns `false` if the site didn't exist.
/// Does NOT rewrite the shared configs — call [`rebuild_configs`] + reload after
/// so the site stops being served — and does NOT drop the site's MySQL database
/// (that needs a running server + resolved binaries; `commands::delete_site`
/// does it before calling here). (The docroot is only removed if it lives under
/// our sites dir — a safety guard against deleting an arbitrary path.)
pub fn teardown(conn: &Connection, platform: &dyn Platform, id: &str) -> Result<bool> {
    let site = match get(conn, id)? {
        Some(s) => s,
        None => return Ok(false),
    };

    store::delete_site(conn, id)?;

    // Remove the per-site cert dir (best-effort).
    let cert_dir = ssl::site_cert_dir(platform.paths(), &site.domain)?;
    let _ = std::fs::remove_dir_all(&cert_dir);

    // Remove per-site config/log artifacts (best-effort): the FrankenPHP
    // override config + log (if the site ever ran the override server) and the
    // tunnel log (if it was ever shared). Paths come from the owning modules so
    // the names can't drift.
    if let Ok(conf) = frankenphp::config_path(platform, &site.domain) {
        let _ = std::fs::remove_file(conf);
    }
    if let Ok(log) = frankenphp::log_path(platform, &site.domain) {
        let _ = std::fs::remove_file(log);
    }
    if let Ok(log) = tunnels::log_path(platform, &site.domain) {
        let _ = std::fs::remove_file(log);
    }

    // Remove the docroot, but only if it's under a managed sites dir — the
    // configured one, the current default, OR the legacy app-data default (so
    // neither changing the setting nor the default-change to ~/rexenv/Sites
    // strands teardown of pre-existing sites).
    let configured = sites_dir(conn, platform)?;
    let default_dir = default_sites_dir()?;
    let legacy_dir = legacy_sites_dir(platform)?;
    let path = Path::new(&site.path);
    if !site.path.is_empty()
        && (path.starts_with(&configured)
            || path.starts_with(&default_dir)
            || path.starts_with(&legacy_dir))
    {
        let _ = std::fs::remove_dir_all(path);
    }

    Ok(true)
}

/// Whether a site type needs a database provisioned (the pluggable DB stage —
/// Blank PHP: none; WordPress/Laravel: MySQL, done in §8/§9).
pub fn needs_database(site_type: SiteType) -> bool {
    matches!(site_type, SiteType::Wordpress | SiteType::Laravel)
}

/// Settings key for the configurable sites root.
pub const SITES_DIR_KEY: &str = "sites_dir";

/// Default site docroot root: `~/rexenv/Sites` — user-visible and Finder
/// friendly (`directories` resolves the home dir correctly per OS). Only a
/// DEFAULT: an explicitly configured `sites_dir` setting always wins, and
/// existing site rows hold absolute paths, so changing the default never
/// orphans previously created sites.
fn default_sites_dir() -> Result<PathBuf> {
    let base = directories::BaseDirs::new()
        .ok_or_else(|| Error::Other("could not resolve the home directory".into()))?;
    Ok(base.home_dir().join("rexenv").join("Sites"))
}

/// The pre-`~/rexenv/Sites` default under app-data. Kept ONLY so teardown can
/// still delete the docroots of sites created under the old default — without
/// it, deleting such a site would silently strand its files on disk.
fn legacy_sites_dir(platform: &dyn Platform) -> Result<PathBuf> {
    Ok(platform.paths().app_data_dir()?.join("sites"))
}

/// Root directory for site docroots: the `sites_dir` setting if set, else the
/// `~/rexenv/Sites` default.
pub fn sites_dir(conn: &Connection, _platform: &dyn Platform) -> Result<PathBuf> {
    match store::get_setting(conn, SITES_DIR_KEY)? {
        Some(p) if !p.trim().is_empty() => Ok(PathBuf::from(p)),
        _ => default_sites_dir(),
    }
}

/// Ensure the resolved sites root exists — first run creates `~/rexenv/Sites`
/// (`create_dir_all`: fine when `~/rexenv` already exists without `Sites`,
/// idempotent when both do). Also recreates a user-configured folder that
/// vanished. The caller logs a failure (unwritable home, permissions) without
/// aborting startup — provision surfaces its own clear error later.
pub fn ensure_sites_dir(conn: &Connection, platform: &dyn Platform) -> Result<PathBuf> {
    let dir = sites_dir(conn, platform)?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Provision a new site end-to-end (filesystem + cert + DB row). The docroot is
/// `<sites_dir>/<domain>`; for Blank PHP a `phpinfo()` `index.php` is dropped in.
/// The DB-provisioning step branches on [`needs_database`] (a hook for 9.2).
/// Does NOT (re)write the shared server configs — call [`rebuild_configs`] +
/// reload after, so one apply covers any number of changes.
pub fn provision(
    conn: &Connection,
    platform: &dyn Platform,
    ca: &ssl::LocalCa,
    mut new: NewSite,
) -> Result<Site> {
    validate_domain(&new.domain)?; // before any docroot/cert/DB use of the domain
    if store::domain_exists(conn, &new.domain)? {
        return Err(Error::Other(format!(
            "domain already in use: {}",
            new.domain
        )));
    }

    let docroot = sites_dir(conn, platform)?.join(&new.domain);
    std::fs::create_dir_all(&docroot)?;
    if matches!(new.site_type, SiteType::Php) {
        std::fs::write(docroot.join("index.php"), "<?php phpinfo();\n")?;
    }

    // Issue the per-site cert (wildcard SAN) signed by our CA.
    ssl::ensure_site_cert(platform.paths(), platform.permissions(), ca, &new.domain)?;

    // Database branch (pluggable): Blank PHP needs none. For WordPress the DB is
    // created downstream by `wordpress::install_wordpress` (bundled-mysql
    // `create_database` before `wp core install`), not here.
    if needs_database(new.site_type) {
        // Intentionally empty — marks where a site type needing a DB at
        // provision time (rather than at install time) would plug in.
    }

    new.path = docroot.display().to_string();
    create(conn, new)
}

/// Whether a site is served by the shared nginx. Override servers (FrankenPHP —
/// §2; Apache — §3) run their own backend process and are excluded.
fn is_nginx_served(s: &Site) -> bool {
    !matches!(s.web_server, WebServer::Frankenphp)
}

/// The edge (Caddy) upstream for a site: a FrankenPHP-override site points at its
/// own backend port; every other site goes to the shared nginx port.
fn site_upstream(s: &Site, nginx_http_port: u16) -> String {
    match s.web_server {
        WebServer::Frankenphp => format!("127.0.0.1:{}", frankenphp::site_port(&s.domain)),
        _ => format!("127.0.0.1:{nginx_http_port}"),
    }
}

/// Map a site to its shared-nginx server block, routing `.php` to the FastCGI
/// pool of the site's PHP version (Phase 2 §1.3). An unrecognized version falls
/// back to the default pool so a site is never left pointing at a dead port.
/// `body_limits` (minor → bytes, from `php::nginx_body_limits`) sets the block's
/// `client_max_body_size` so nginx accepts what the version's PHP settings allow.
fn nginx_site_for(
    s: &Site,
    body_limits: &std::collections::HashMap<String, u64>,
) -> services::NginxSite {
    services::NginxSite {
        domain: s.domain.clone(),
        docroot: PathBuf::from(&s.path),
        php_fpm_port: pool_port_for(&s.php_version),
        rewrite: rewrite_mode_for(s.multisite),
        body_limit: body_limits.get(&php::minor_of(&s.php_version)).copied(),
    }
}

/// The php-fpm pool port a site's PHP `version` routes to. Only versions with a
/// pinned build run a pool; an unrecognized version falls back to the default pool
/// rather than pointing at a dead port. Shared by the nginx config builder and the
/// per-site serving check (`service_manager::site_serving`), so both agree on which
/// upstream a site actually uses.
pub(crate) fn pool_port_for(version: &str) -> u16 {
    let minor = php::minor_of(version);
    match php::patch_for_minor(&minor) {
        Some(_) => php::fpm_port(&minor).unwrap_or(services::PHP_FPM_PORT),
        None => services::PHP_FPM_PORT,
    }
}

/// Map a site's multisite mode to its rewrite template (Phase 1 §6.2). Shared by
/// the nginx config builder and the FrankenPHP override backend (`service_manager`).
pub fn rewrite_mode_for(mode: MultisiteMode) -> services::RewriteMode {
    match mode {
        MultisiteMode::None => services::RewriteMode::Single,
        MultisiteMode::Subdomain => services::RewriteMode::SubdomainMultisite,
        MultisiteMode::Subdirectory => services::RewriteMode::SubdirectoryMultisite,
    }
}

/// Paths of the regenerated shared configs (nginx + Caddy).
#[derive(Debug, Clone)]
pub struct RebuiltConfigs {
    pub nginx_conf: PathBuf,
    pub nginx_prefix: PathBuf,
    pub caddyfile: PathBuf,
}

/// Regenerate the shared nginx + Caddy configs from ALL sites in the DB. The
/// configs are derived state: one shared nginx (server block per site, by
/// `server_name`, FastCGI → php-fpm) and Caddy routes (`*.test` host → nginx,
/// TLS with each site's cert). Caller reloads the services afterwards.
pub fn rebuild_configs(
    conn: &Connection,
    platform: &dyn Platform,
    ca: &ssl::LocalCa,
    nginx_http_port: u16,
    caddy_http_port: u16,
    caddy_https_port: u16,
) -> Result<RebuiltConfigs> {
    let sites = list(conn)?;
    let body_limits = php::nginx_body_limits(&store::all_php_settings(conn)?);
    rebuild_configs_for(
        &sites,
        platform,
        ca,
        nginx_http_port,
        caddy_http_port,
        caddy_https_port,
        &body_limits,
    )
}

/// Like [`rebuild_configs`] but from an explicit site list + precomputed nginx
/// body limits (so callers holding an async lock don't keep the DB connection
/// borrowed across `.await`). `body_limits` comes from `php::nginx_body_limits`.
pub fn rebuild_configs_for(
    sites: &[Site],
    platform: &dyn Platform,
    ca: &ssl::LocalCa,
    nginx_http_port: u16,
    caddy_http_port: u16,
    caddy_https_port: u16,
    body_limits: &std::collections::HashMap<String, u64>,
) -> Result<RebuiltConfigs> {
    // Only nginx-served sites get a server block; override servers (§2/§3) have
    // their own backend process.
    let mut nginx_sites: Vec<services::NginxSite> = sites
        .iter()
        .filter(|s| is_nginx_served(s))
        .map(|s| nginx_site_for(s, body_limits))
        .collect();
    // Internal Adminer vhost (§5.2): served by the default php-fpm pool, rooted at
    // its isolated docroot. Not a Site → never a tunnel origin (§9).
    nginx_sites.push(services::NginxSite {
        domain: adminer::ADMINER_HOST.to_string(),
        docroot: adminer::docroot(platform)?,
        php_fpm_port: services::PHP_FPM_PORT,
        rewrite: services::RewriteMode::Single,
        body_limit: None,
    });
    let (nginx_conf, nginx_prefix) =
        services::write_nginx_config(platform, nginx_http_port, nginx_sites)?;

    // Every site (nginx- or override-served) gets a Caddy edge route: TLS with the
    // local CA, reverse-proxy to that site's upstream (shared nginx, or its own
    // override backend port).
    let mut routes = Vec::with_capacity(sites.len());
    for s in sites {
        let cert = ssl::ensure_site_cert(platform.paths(), platform.permissions(), ca, &s.domain)?;
        routes.push(proxy::SiteRoute {
            host: s.domain.clone(),
            // Subdomain multisite serves every `*.mysite.test` sub-site from the
            // one wildcard cert + backend (§10.2); other modes are single-host.
            wildcard: matches!(s.multisite, MultisiteMode::Subdomain),
            upstream: site_upstream(s, nginx_http_port),
            cert_path: cert.cert_path,
            key_path: cert.key_path,
        });
    }
    // Edge route for the internal Adminer vhost (TLS via local CA → shared nginx).
    let adminer_cert =
        ssl::ensure_site_cert(platform.paths(), platform.permissions(), ca, adminer::ADMINER_HOST)?;
    routes.push(proxy::SiteRoute {
        host: adminer::ADMINER_HOST.to_string(),
        wildcard: false,
        upstream: format!("127.0.0.1:{nginx_http_port}"),
        cert_path: adminer_cert.cert_path,
        key_path: adminer_cert.key_path,
    });
    let caddyfile = proxy::write_caddyfile(
        platform,
        &proxy::CaddyConfig {
            http_port: caddy_http_port,
            https_port: caddy_https_port,
            routes,
            // Bind Caddy's admin API to our unix socket (not TCP :2019) so the root
            // edge exposes no unauthenticated local control surface (task 2.3 / H5).
            admin_socket: Some(proxy::admin_socket_path(platform)?),
        },
    )?;

    Ok(RebuiltConfigs {
        nginx_conf,
        nginx_prefix,
        caddyfile,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::db;
    use crate::state::models::{SiteType, WebServer};

    fn sample(name: &str, domain: &str) -> NewSite {
        NewSite {
            name: name.into(),
            domain: domain.into(),
            site_type: SiteType::Wordpress,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            path: format!("~/Sites/{name}"),
        }
    }

    #[test]
    fn validate_domain_accepts_test_hostnames() {
        for d in ["acme.test", "my-site.test", "a.test", "sub.mysite.test", "wp123.test"] {
            assert!(validate_domain(d).is_ok(), "should accept {d}");
        }
    }

    #[test]
    fn validate_domain_rejects_unsafe_or_non_test() {
        for d in [
            "",              // empty
            "acme.com",      // wrong TLD
            "acme",          // no TLD
            ".test",         // no label before .test
            "../etc.test",   // path traversal
            "a/b.test",      // path separator
            "a b.test",      // space
            "a;b.test",      // config-injection char
            "a{b.test",      // config-injection char
            "Acme.test",     // uppercase
            "*.mysite.test", // wildcard
            "-bad.test",     // leading hyphen
            "bad-.test",     // trailing hyphen
            "a..test",       // empty inner label
            "foo.test.evil", // .test not last
        ] {
            assert!(validate_domain(d).is_err(), "should reject {d:?}");
        }
    }

    #[test]
    fn create_rejects_an_invalid_domain() {
        let conn = db::open_in_memory().unwrap();
        assert!(create(&conn, sample("Bad", "../evil.test")).is_err());
        // A rejected domain persists nothing.
        assert!(list(&conn).unwrap().is_empty());
    }

    #[test]
    fn create_then_list_round_trips() {
        let conn = db::open_in_memory().unwrap();
        let created = create(&conn, sample("Acme", "acme.test")).unwrap();

        assert!(!created.id.is_empty());
        assert!(matches!(created.status, ServiceStatus::Stopped));
        assert!(created.ssl);
        assert!(!created.created_at.is_empty());

        let all = list(&conn).unwrap();
        assert_eq!(all.len(), 1);
        let s = &all[0];
        assert_eq!(s.id, created.id);
        assert_eq!(s.name, "Acme");
        assert_eq!(s.domain, "acme.test");
        assert!(matches!(s.site_type, SiteType::Wordpress));
        assert!(matches!(s.web_server, WebServer::Nginx));
        assert_eq!(s.php_version, "8.3");
    }

    #[test]
    fn duplicate_domain_is_rejected() {
        let conn = db::open_in_memory().unwrap();
        create(&conn, sample("One", "dup.test")).unwrap();
        let err = create(&conn, sample("Two", "dup.test")).unwrap_err();
        assert!(err.to_string().contains("domain already in use"));
        assert_eq!(list(&conn).unwrap().len(), 1);
    }

    #[test]
    fn set_status_updates_and_returns_site() {
        let conn = db::open_in_memory().unwrap();
        let site = create(&conn, sample("Acme", "acme.test")).unwrap();
        assert!(matches!(site.status, ServiceStatus::Stopped));

        let updated = set_status(&conn, &site.id, ServiceStatus::Running)
            .unwrap()
            .expect("exists");
        assert!(matches!(updated.status, ServiceStatus::Running));
        // persisted
        assert!(matches!(
            get(&conn, &site.id).unwrap().unwrap().status,
            ServiceStatus::Running
        ));
        // unknown id → None
        assert!(set_status(&conn, "nope", ServiceStatus::Running)
            .unwrap()
            .is_none());
    }

    #[test]
    fn get_and_delete() {
        let conn = db::open_in_memory().unwrap();
        let site = create(&conn, sample("Blog", "blog.test")).unwrap();

        let fetched = get(&conn, &site.id).unwrap().expect("site exists");
        assert_eq!(fetched.domain, "blog.test");

        assert!(delete(&conn, &site.id).unwrap());
        assert!(get(&conn, &site.id).unwrap().is_none());
        assert!(!delete(&conn, &site.id).unwrap(), "second delete is a no-op");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn teardown_removes_row_and_per_site_artifacts() {
        let conn = db::open_in_memory().unwrap();
        let platform = crate::platform::current();
        // sample() path is "~/Sites/<name>" (not under the sites dir), so the
        // docroot-removal guard skips it — the test won't touch real dirs.
        let site = create(&conn, sample("Teardown", "teardown.test")).unwrap();

        // Plant the per-site config/log artifacts a site can accrue (FrankenPHP
        // override config + log, tunnel log) and assert teardown sweeps them.
        let artifacts = [
            frankenphp::config_path(&*platform, &site.domain).unwrap(),
            frankenphp::log_path(&*platform, &site.domain).unwrap(),
            tunnels::log_path(&*platform, &site.domain).unwrap(),
        ];
        for p in &artifacts {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "x").unwrap();
        }

        assert!(teardown(&conn, &*platform, &site.id).unwrap());
        assert!(get(&conn, &site.id).unwrap().is_none());
        for p in &artifacts {
            assert!(!p.exists(), "orphaned artifact left behind: {}", p.display());
        }
        // Deleting again is a no-op.
        assert!(!teardown(&conn, &*platform, &site.id).unwrap());
    }

    #[test]
    fn setting_round_trips_and_upserts() {
        let conn = db::open_in_memory().unwrap();
        assert!(store::get_setting(&conn, "k").unwrap().is_none());
        store::set_setting(&conn, "k", "v").unwrap();
        assert_eq!(store::get_setting(&conn, "k").unwrap().as_deref(), Some("v"));
        store::set_setting(&conn, "k", "v2").unwrap();
        assert_eq!(store::get_setting(&conn, "k").unwrap().as_deref(), Some("v2"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn sites_dir_uses_setting_or_default() {
        let conn = db::open_in_memory().unwrap();
        let platform = crate::platform::current();
        // Default is ~/rexenv/Sites (exact casing).
        assert!(sites_dir(&conn, &*platform).unwrap().ends_with("rexenv/Sites"));
        // A configured value overrides it.
        store::set_setting(&conn, SITES_DIR_KEY, "/tmp/custom-sites").unwrap();
        assert_eq!(
            sites_dir(&conn, &*platform).unwrap(),
            PathBuf::from("/tmp/custom-sites")
        );
    }

    #[test]
    fn override_sites_route_to_backend_and_skip_nginx() {
        let conn = db::open_in_memory().unwrap();
        let mut ng = sample("NG", "ng.test");
        ng.site_type = SiteType::Php;
        ng.web_server = WebServer::Nginx;
        let mut fp = sample("FP", "fp.test");
        fp.site_type = SiteType::Php;
        fp.web_server = WebServer::Frankenphp;
        let ng = create(&conn, ng).unwrap();
        let fp = create(&conn, fp).unwrap();

        // Nginx serves only the nginx site; the FrankenPHP site is excluded.
        assert!(is_nginx_served(&ng));
        assert!(!is_nginx_served(&fp));

        // Edge upstream: nginx site → shared nginx port; FrankenPHP site → its backend.
        assert_eq!(site_upstream(&ng, 18088), "127.0.0.1:18088");
        assert_eq!(
            site_upstream(&fp, 18088),
            format!("127.0.0.1:{}", frankenphp::site_port("fp.test"))
        );
        // The FrankenPHP backend port is in the override range, not the nginx port.
        assert!(frankenphp::site_port("fp.test") >= frankenphp::FRANKENPHP_BASE_PORT);
        assert_ne!(frankenphp::site_port("fp.test"), 18088);
    }

    #[test]
    fn set_php_version_updates_only_the_column() {
        let conn = db::open_in_memory().unwrap();
        let mut new = sample("Sw", "sw.test");
        new.php_version = "8.1".into();
        let site = create(&conn, new).unwrap();

        // Switch 8.1 → 8.3: only php_version changes; path/domain/type untouched.
        let updated = set_php_version(&conn, &site.id, "8.3").unwrap().expect("exists");
        assert_eq!(updated.php_version, "8.3");
        assert_eq!(updated.path, site.path);
        assert_eq!(updated.domain, site.domain);
        assert_eq!(updated.id, site.id);

        // A patch-form value normalizes to its minor.
        let u2 = set_php_version(&conn, &site.id, "8.2.31").unwrap().unwrap();
        assert_eq!(u2.php_version, "8.2");

        // Unsupported version is rejected; unknown id is a no-op (None).
        assert!(set_php_version(&conn, &site.id, "7.4").is_err());
        assert!(set_php_version(&conn, "nope", "8.3").unwrap().is_none());
    }

    #[test]
    fn set_web_server_updates_only_the_column() {
        let conn = db::open_in_memory().unwrap();
        let mut new = sample("SW", "sws.test");
        new.web_server = WebServer::Nginx;
        let site = create(&conn, new).unwrap();

        // Nginx → FrankenPHP → Nginx: only web_server changes; path/domain untouched.
        let fp = set_web_server(&conn, &site.id, WebServer::Frankenphp).unwrap().expect("exists");
        assert!(matches!(fp.web_server, WebServer::Frankenphp));
        assert_eq!(fp.path, site.path);
        let back = set_web_server(&conn, &site.id, WebServer::Nginx).unwrap().unwrap();
        assert!(matches!(back.web_server, WebServer::Nginx));

        // Deferred servers are rejected; unknown id is a no-op (None).
        assert!(set_web_server(&conn, &site.id, WebServer::Apache).is_err());
        assert!(set_web_server(&conn, "nope", WebServer::Nginx).unwrap().is_none());
    }

    #[test]
    fn nginx_site_maps_php_version_to_its_pool_port() {
        let conn = db::open_in_memory().unwrap();
        let mut a = sample("A", "a.test");
        a.site_type = SiteType::Php;
        a.php_version = "8.1".into();
        let mut b = sample("B", "b.test");
        b.site_type = SiteType::Php;
        b.php_version = "8.3".into();
        let a = create(&conn, a).unwrap();
        let b = create(&conn, b).unwrap();

        let no_limits = std::collections::HashMap::new();
        let na = nginx_site_for(&a, &no_limits);
        let nb = nginx_site_for(&b, &no_limits);
        // Each site routes to its own version's pool port — not a hardcoded one.
        assert_eq!(na.php_fpm_port, php::fpm_port("8.1").unwrap()); // 9781
        assert_eq!(nb.php_fpm_port, php::fpm_port("8.3").unwrap()); // 9783
        assert_ne!(na.php_fpm_port, nb.php_fpm_port);

        // A patch-form version still resolves to its minor's pool.
        let mut c = sample("C", "c.test");
        c.php_version = "8.2.31".into();
        let c = create(&conn, c).unwrap();
        assert_eq!(nginx_site_for(&c, &no_limits).php_fpm_port, php::fpm_port("8.2").unwrap());

        // An unknown version falls back to the default pool.
        let mut d = sample("D", "d.test");
        d.php_version = "7.4".into();
        let d = create(&conn, d).unwrap();
        assert_eq!(nginx_site_for(&d, &no_limits).php_fpm_port, services::PHP_FPM_PORT);
    }

    #[test]
    fn nginx_site_body_limit_follows_its_versions_settings() {
        let conn = db::open_in_memory().unwrap();
        let mut a = sample("A", "a.test");
        a.php_version = "8.3".into();
        let a = create(&conn, a).unwrap();
        let limits: std::collections::HashMap<String, u64> =
            [("8.3".to_string(), 64u64 << 20)].into();
        // The site's minor has a limit → per-server client_max_body_size in bytes.
        assert_eq!(nginx_site_for(&a, &limits).body_limit, Some(64 << 20));
        // A patch-form version maps through its minor; an uncovered minor gets none.
        let mut b = sample("B", "b.test");
        b.php_version = "8.3.31".into();
        let b = create(&conn, b).unwrap();
        assert_eq!(nginx_site_for(&b, &limits).body_limit, Some(64 << 20));
        let mut c = sample("C", "c.test");
        c.php_version = "8.1".into();
        let c = create(&conn, c).unwrap();
        assert_eq!(nginx_site_for(&c, &limits).body_limit, None);
    }

    #[test]
    fn needs_database_by_type() {
        assert!(!needs_database(SiteType::Php));
        assert!(needs_database(SiteType::Wordpress));
        assert!(needs_database(SiteType::Laravel));
    }

    #[test]
    fn enum_values_persist_as_canonical_strings() {
        let conn = db::open_in_memory().unwrap();
        let mut new = sample("LO", "lo.test");
        new.site_type = SiteType::Php;
        new.web_server = WebServer::Openlitespeed;
        create(&conn, new).unwrap();

        // Read the raw TEXT to confirm DB storage matches the wire format.
        let (t, ws): (String, String) = conn
            .query_row(
                "SELECT type, web_server FROM sites WHERE domain='lo.test'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(t, "php");
        assert_eq!(ws, "openlitespeed");
    }
}
